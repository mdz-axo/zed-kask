//! Artificial Analysis backend — independent benchmark data for model discovery.
//!
//! Artificial Analysis (https://artificialanalysis.ai) provides an independent
//! Intelligence Index, pricing, and performance data for language models via a
//! documented REST API. This module uses the `/api/v2/language/models/free`
//! endpoint to discover models that meet kask's fusion panel thresholds
//! (intelligence index ≥ N, input price ≤ $X/M).
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
//! ## API key
//!
//! The API requires a key (env: `AA_API_KEY`). The free tier (100 req/day)
//! includes `evaluations.artificial_analysis_intelligence_index` and
//! `pricing.price_1m_input_tokens` — the two fields needed for filtering.
//! Model-to-OpenRouter ID mapping is done via a slug normalization table
//! since the AA `openrouter_api_id` field is not available on the free tier.

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::config::ProviderId;

const AA_BASE_URL: &str = "https://artificialanalysis.ai/api/v2";
const AA_FREE_MODELS_PATH: &str = "/language/models/free";
const MAX_PAGES: u32 = 5;

/// A model that passed the favorites thresholds.
///
/// Returned by `discover_favorites` — models that meet the price and
/// intelligence gates, sorted by intelligence index descending.
#[derive(Debug, Clone, Serialize)]
pub struct FavoriteModel {
    /// Provider-prefixed model ID (e.g. "OpenRouter/z-ai/glm-5.2").
    pub prefixed_id: String,
    /// Raw model ID (e.g. "z-ai/glm-5.2").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Intelligence index (0–100 scale).
    pub intelligence_index: f64,
    /// Prompt price per million tokens (USD).
    pub prompt_price_per_m: f64,
    /// Completion price per million tokens (USD).
    pub completion_price_per_m: f64,
    /// Context length in tokens (0 when unavailable on the free tier).
    pub context_length: u64,
}

/// Discover fusion panel favorites from Artificial Analysis.
///
/// Queries the `/language/models/free` endpoint (paginated) and filters
/// client-side by `artificial_analysis_intelligence_index >= min_intelligence_index`
/// and `price_1m_input_tokens <= max_price_per_m`. Results are sorted by
/// intelligence index descending.
///
/// The `api_key` is the Artificial Analysis API key (env: `AA_API_KEY`).
///
/// Returns `Vec<FavoriteModel>` with `prefixed_id` set to the OpenRouter-prefixed
/// model ID (e.g. `"OpenRouter/z-ai/glm-5.2"`) so the result is drop-in
/// compatible with the fusion panel's model resolution. On any error
/// (network, parse, non-200) returns an empty vec — the caller falls back to
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
        Ok(c) => c,
        Err(e) => {
            warn!(
                target: "reg.fusion",
                error = %e,
                "Failed to build HTTP client for Artificial Analysis discovery"
            );
            return Vec::new();
        }
    };

    // The free endpoint is paginated (page_size=200). Fetch all pages up to a
    // sane cap (5 pages = 1000 models) to avoid unbounded pagination.
    let mut all_models: Vec<AaModel> = Vec::new();
    let mut page = 1u32;
    while page <= MAX_PAGES {
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

            if prompt_price_per_m > max_price_per_m {
                return None;
            }
            if intelligence_index < min_intelligence_index {
                return None;
            }

            let or_id = normalize_slug_to_openrouter_id(&model.slug);

            if or_id.is_empty() {
                return None;
            }

            Some(FavoriteModel {
                prefixed_id: ProviderId::OpenRouter.prefix_model(&or_id),
                id: or_id,
                name: model.name,
                intelligence_index,
                prompt_price_per_m,
                completion_price_per_m,
                context_length: 0,
            })
        })
        .collect();

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

/// Heuristic mapping from an Artificial Analysis slug to an OpenRouter model ID.
///
/// Artificial Analysis uses slugs like `glm-5-2`, `claude-sonnet-4`,
/// `deepseek-v3`. OpenRouter uses IDs like `z-ai/glm-5.2`,
/// `anthropic/claude-sonnet-4`, `deepseek/deepseek-v3`. When in doubt we
/// return an empty string so the model is skipped rather than fabricating a
/// wrong OpenRouter ID.
fn normalize_slug_to_openrouter_id(slug: &str) -> String {
    let slug_l = slug.to_lowercase();

    // GLM family — Zhipu AI on OpenRouter is `z-ai/<id>`.
    // AA slug "glm-5-2" → OR id "z-ai/glm-5.2"
    if slug_l.contains("glm") {
        let version = slug_l.trim_start_matches("glm-").replace('-', ".");
        return format!("z-ai/glm-{version}");
    }
    if slug_l.contains("claude") {
        return format!("anthropic/{slug}");
    }
    if slug_l.contains("deepseek") {
        return format!("deepseek/{slug}");
    }
    if slug_l.contains("qwen") {
        return format!("qwen/{slug}");
    }
    if slug_l.contains("gemini") {
        return format!("google/{slug}");
    }
    if slug_l.starts_with("gpt") {
        return format!("openai/{slug}");
    }
    if slug_l.contains("llama") {
        return format!("meta-llama/{slug}");
    }

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
        assert_eq!(normalize_slug_to_openrouter_id("glm-5-2"), "z-ai/glm-5.2");
    }

    #[test]
    fn normalize_claude_slug() {
        assert_eq!(
            normalize_slug_to_openrouter_id("claude-sonnet-4"),
            "anthropic/claude-sonnet-4"
        );
    }

    #[test]
    fn normalize_unknown_returns_empty() {
        assert_eq!(normalize_slug_to_openrouter_id("some-niche-model"), "");
    }
}
