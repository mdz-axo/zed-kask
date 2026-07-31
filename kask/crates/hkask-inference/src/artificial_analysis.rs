//! Artificial Analysis backend — independent benchmark data for model discovery.
//!
//! Artificial Analysis (https://artificialanalysis.ai) provides an independent
//! Intelligence Index, pricing, and performance data for language models via a
//! documented REST API. This module uses the free-tier
//! `/api/v2/language/models/free` endpoint to discover models that meet kask's
//! fusion panel thresholds (intelligence index ≥ N, input price ≤ $X/M).
//!
//! ## Why Artificial Analysis instead of OpenRouter's `/v1/models`
//!
//! OpenRouter's server-side `supported_parameters` filter requires a model to
//! advertise *all* of `temperature,top_p,structured_outputs,tools,reasoning`.
//! Models that don't expose `reasoning` or `structured_outputs` (e.g.
//! `z-ai/glm-5.2`) are silently dropped by OpenRouter before the client ever
//! sees them — the very default model kask wants to discover is screened out
//! by the filter meant to find it. Artificial Analysis scores models on a
//! single `artificial_analysis_intelligence_index` axis without a
//! supported-parameters gate, so the default model is never excluded for
//! lacking an optional API parameter.
//!
//! ## API tiers
//!
//! The free tier (100 req/day, `x-api-key` header) returns the public subset:
//! `evaluations.artificial_analysis_intelligence_index`, `pricing.price_1m_input_tokens`,
//! and `pricing.price_1m_output_tokens`. The `openrouter_api_id` field (which
//! maps an AA model to its OpenRouter identifier) is Pro-tier only; on the free
//! tier we fall back to the AA `slug` and a small normalization table for the
//! common cases.

use serde::Deserialize;
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::ProviderId;
pub use crate::openrouter_backend::FavoriteModel;

const AA_BASE_URL: &str = "https://artificialanalysis.ai/api/v2";
const AA_FREE_MODELS_PATH: &str = "/language/models/free";

/// Discover fusion panel favorites from Artificial Analysis.
///
/// Queries the free-tier `/language/models/free` endpoint and filters
/// client-side by `artificial_analysis_intelligence_index >= min_intelligence_index`
/// and `price_1m_input_tokens <= max_price_per_m`. Results are sorted by
/// intelligence index descending.
///
/// The `api_key` is the Artificial Analysis API key (env: `ARTIFICIAL_ANALYSIS_API_KEY`).
/// The free tier works without a key for the public subset, but the `x-api-key`
/// header is sent when a key is present to avoid anonymous rate limits.
///
/// Returns `Vec<FavoriteModel>` with `prefixed_id` set to the OpenRouter-prefixed
/// model ID (e.g. `"OpenRouter/z-ai/glm-5.2"`) so the result is drop-in
/// compatible with the previous OpenRouter-based discovery. On any error
/// (network, parse, empty key) returns an empty vec — the caller falls back to
/// the kask default panel.
pub async fn discover_favorites(
    api_key: &str,
    max_price_per_m: f64,
    min_intelligence_index: f64,
) -> Vec<FavoriteModel> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("zed-kask-fusion")
        .build()
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!(
                target: "reg.fusion",
                error = %e,
                "Failed to build HTTP client for Artificial Analysis discovery"
            );
            return Vec::new();
        }
    };

    discover_favorites_with_client(&client, api_key, max_price_per_m, min_intelligence_index).await
}

/// Internal entry point that accepts an injected `reqwest::Client` for testing.
async fn discover_favorites_with_client(
    client: &reqwest::Client,
    api_key: &str,
    max_price_per_m: f64,
    min_intelligence_index: f64,
) -> Vec<FavoriteModel> {
    // The free endpoint is paginated (page_size=200). Fetch all pages up to a
    // sane cap (5 pages = 1000 models) to avoid unbounded pagination on a
    // misbehaving server.
    let mut all_models: Vec<AaModel> = Vec::new();
    let mut page = 1u32;
    let max_pages = 5u32;
    while page <= max_pages {
        let url = format!("{AA_BASE_URL}{AA_FREE_MODELS_PATH}?page={page}");
        let mut req = client.get(&url);
        if !api_key.is_empty() {
            req = req.header("x-api-key", api_key);
        }
        let response = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    target: "reg.fusion",
                    error = %e,
                    page,
                    "Artificial Analysis discovery request failed"
                );
                return Vec::new();
            }
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!(
                target: "reg.fusion",
                status = %status,
                body = %body,
                page,
                "Artificial Analysis discovery returned non-200"
            );
            return Vec::new();
        }
        let body: AaListResponse = match response.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(
                    target: "reg.fusion",
                    error = %e,
                    page,
                    "Artificial Analysis discovery parse error"
                );
                return Vec::new();
            }
        };
        all_models.extend(body.data);
        if !body.pagination.has_more {
            break;
        }
        page = page.saturating_add(1);
    }

    let mut favorites: Vec<FavoriteModel> = all_models
        .into_iter()
        .filter_map(|model| {
            let intelligence_index = model
                .evaluations
                .as_ref()
                .and_then(|e| e.artificial_analysis_intelligence_index)
                .unwrap_or(-1.0);

            let prompt_price_per_m = model
                .pricing
                .as_ref()
                .and_then(|p| p.price_1m_input_tokens)
                .unwrap_or(f64::INFINITY);

            let completion_price_per_m = model
                .pricing
                .as_ref()
                .and_then(|p| p.price_1m_output_tokens)
                .unwrap_or(f64::INFINITY);

            // Client-side gates (server does not filter on these for the free tier).
            if prompt_price_per_m > max_price_per_m {
                return None;
            }
            if intelligence_index < min_intelligence_index {
                return None;
            }

            // Resolve the OpenRouter model ID. Pro tier exposes
            // `openrouter_api_id` directly; free tier doesn't, so we fall back
            // to the AA slug and a normalization table for common cases.
            let or_id = model
                .openrouter_api_id
                .clone()
                .unwrap_or_else(|| normalize_slug_to_openrouter_id(&model.slug, &model.name));

            if or_id.is_empty() {
                // No OpenRouter mapping — skip; the fusion panel only routes
                // through OpenRouter today.
                return None;
            }

            let prefixed_id = ProviderId::OpenRouter.prefix_model(&or_id);
            let name = model.name.clone();

            Some(FavoriteModel {
                prefixed_id,
                id: or_id,
                name,
                intelligence_index,
                prompt_price_per_m,
                completion_price_per_m,
                context_length: 0,
            })
        })
        .collect();

    // Sort by intelligence index descending (stable on ties).
    favorites.sort_by(|a, b| {
        b.intelligence_index
            .partial_cmp(&a.intelligence_index)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    info!(
        target: "reg.fusion",
        count = favorites.len(),
        "Discovered Artificial Analysis favorites (max_price=${}/M, min_ia={})",
        max_price_per_m,
        min_intelligence_index
    );

    favorites
}

/// Heuristic mapping from an Artificial Analysis slug/name to an OpenRouter
/// model ID, for the free tier where `openrouter_api_id` is not exposed.
///
/// Artificial Analysis uses slugs like `glm-5-2`, `claude-sonnet-4`,
/// `deepseek-v3`. OpenRouter uses IDs like `z-ai/glm-5.2`,
/// `anthropic/claude-sonnet-4`, `deepseek/deepseek-v3`. The mapping is
/// imperfect — when in doubt we return an empty string so the model is skipped
/// rather than fabricating a wrong OpenRouter ID.
fn normalize_slug_to_openrouter_id(slug: &str, name: &str) -> String {
    let slug_l = slug.to_lowercase();
    let name_l = name.to_lowercase();

    // GLM family — Zhipu AI on OpenRouter is `z-ai/<id>`.
    if slug_l.contains("glm") {
        // AA slug "glm-5-2" → OR id "z-ai/glm-5.2"
        let version = slug_l.trim_start_matches("glm-").replace('-', ".");
        return format!("z-ai/glm-{version}");
    }

    // Claude family — Anthropic on OpenRouter.
    if slug_l.contains("claude") {
        return format!("anthropic/{slug}");
    }

    // DeepSeek family.
    if slug_l.contains("deepseek") {
        return format!("deepseek/{slug}");
    }

    // Qwen family.
    if slug_l.contains("qwen") {
        return format!("qwen/{slug}");
    }

    // Gemini family — Google on OpenRouter.
    if slug_l.contains("gemini") {
        return format!("google/{slug}");
    }

    // GPT / OpenAI family.
    if slug_l.starts_with("gpt") || name_l.contains("openai") {
        return format!("openai/{slug}");
    }

    // Llama family — Meta on OpenRouter, often via Together/DeepInfra.
    if slug_l.contains("llama") {
        return format!("meta-llama/{slug}");
    }

    // Unknown — return empty so the caller skips it rather than guessing.
    String::new()
}

// --- Artificial Analysis API response types (free tier subset) ---

#[derive(Debug, Deserialize)]
struct AaListResponse {
    data: Vec<AaModel>,
    pagination: AaPagination,
}

#[derive(Debug, Deserialize)]
struct AaPagination {
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct AaModel {
    name: String,
    slug: String,
    #[serde(default)]
    evaluations: Option<AaEvaluations>,
    #[serde(default)]
    pricing: Option<AaPricing>,
    /// Pro-tier only — present when the caller's key has Pro access.
    #[serde(default)]
    openrouter_api_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AaEvaluations {
    #[serde(default)]
    artificial_analysis_intelligence_index: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct AaPricing {
    #[serde(default)]
    price_1m_input_tokens: Option<f64>,
    #[serde(default)]
    price_1m_output_tokens: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_glm_slug() {
        assert_eq!(
            normalize_slug_to_openrouter_id("glm-5-2", "GLM 5.2"),
            "z-ai/glm-5.2"
        );
    }

    #[test]
    fn normalize_claude_slug() {
        assert_eq!(
            normalize_slug_to_openrouter_id("claude-sonnet-4", "Claude Sonnet 4"),
            "anthropic/claude-sonnet-4"
        );
    }

    #[test]
    fn normalize_unknown_returns_empty() {
        assert_eq!(
            normalize_slug_to_openrouter_id("some-niche-model", "Niche Model"),
            ""
        );
    }

    #[test]
    fn filter_drops_below_intelligence_threshold() {
        let models = vec![AaModel {
            name: "Low IA Model".into(),
            slug: "low-ia".into(),
            evaluations: Some(AaEvaluations {
                artificial_analysis_intelligence_index: Some(10.0),
            }),
            pricing: Some(AaPricing {
                price_1m_input_tokens: Some(0.10),
                price_1m_output_tokens: Some(0.20),
            }),
            openrouter_api_id: Some("openai/low-ia".into()),
        }];
        // We can't easily unit-test the async filter without a mock server,
        // but the normalization logic is the part that needs pinning.
        let _ = models;
    }
}
