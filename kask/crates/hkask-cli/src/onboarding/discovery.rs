//! Dynamic model discovery pipeline for onboarding.
//!
//! OpenRouter-first pipeline (spec):
//!   1. OpenRouter API: list models via /v1/models
//!   2. Filter: max prompt price, min intelligence index, supported parameters
//!   3. Sort: intelligence index (desc), take top 12
//!   4. Display: UI presentation grouped by model family alphabetically
//!
//! Fallbacks (only if OpenRouter is unavailable):
//!   - provider API list (best thinking + best instruct per family)
//!   - static curated list per provider

use hkask_inference::{InferenceConfig, InferenceRouter, ProviderId, RouterModelEntry};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::error::CliError;

// ── OpenRouter API types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
struct OpenRouterModelList {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct OpenRouterModel {
    id: String,
    name: Option<String>,
    created: Option<u64>,
    pricing: Option<OpenRouterPricing>,
    architecture: Option<OpenRouterArchitecture>,
    benchmarks: Option<Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct OpenRouterArchitecture {
    #[serde(default)]
    output_modalities: Vec<String>,
    #[serde(default)]
    tokenizer: Option<String>,
}

// ── Internal types ─────────────────────────────────────────────────────────

/// Classification of a model's reasoning approach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelKind {
    /// Reasoning/thinking models (R1, thinking variants, deep reasoning)
    Thinking,
    /// Standard instruct-tuned / flash models
    Instruct,
}

/// A model entry for the onboarding UI.
#[derive(Debug, Clone)]
pub(crate) struct OnboardingModel {
    pub label: String,
    pub full_id: String,
    pub description: String,
    #[allow(dead_code)]
    pub score: u32,
    pub source: ModelSource,
    pub kind: ModelKind,
    pub family: String,
}

/// Whether a model entry came from dynamic discovery or the static fallback.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum ModelSource {
    Dynamic,
    Fallback,
}

// ── OpenRouter filter spec ─────────────────────────────────────────────────

const OR_SUPPORTED_PARAMETERS: &str = "temperature,top_p,structured_outputs,tools,reasoning";
const OR_TOP_N: usize = 12;

// ── Model classification ───────────────────────────────────────────────────

/// Keywords that signal a model is a thinking/reasoning variant.
const THINKING_KEYWORDS: &[&str] = &[
    "thinking",
    "reasoning",
    "r1-",
    "-r1",
    "deep-think",
    "deepseek-r1",
    "qwen3-max-thinking",
    "kimi-thinking",
    "o1-",
    "o3-",
    "o4-",
    "reasoner",
    "deep-thought",
];

fn classify_kind(model_id: &str, name: Option<&str>) -> ModelKind {
    let mut haystack = model_id.to_lowercase();
    if let Some(n) = name {
        haystack.push(' ');
        haystack.push_str(&n.to_lowercase());
    }
    if THINKING_KEYWORDS.iter().any(|kw| haystack.contains(kw)) {
        ModelKind::Thinking
    } else {
        ModelKind::Instruct
    }
}

// ── Main pipeline ──────────────────────────────────────────────────────────

/// Run the full discovery pipeline.
///
/// Returns `(models, source_label)` where models is the list
/// and source_label describes where they came from.
///
/// The OpenRouter `/v1/models` endpoint is public (no API key required),
/// so we always run the OpenRouter pipeline first to get a filtered,
/// ranked list. Only if OpenRouter is unreachable do we fall back to
/// the provider API list (with heuristic filters applied).
pub(crate) async fn discover_models(config: &InferenceConfig) -> (Vec<OnboardingModel>, String) {
    // ── OpenRouter pipeline (spec) ────────────────────────────────────
    // The /v1/models endpoint is public — no key needed for discovery.
    eprintln!("  Discovering models via OpenRouter...");
    let or_results: Vec<OnboardingModel> =
        run_openrouter_pipeline(config).await.unwrap_or_default();

    if !or_results.is_empty() {
        return (
            or_results,
            format!(
                "OpenRouter (top {}, max ${}/M in, IA ≥ {}, params {})",
                OR_TOP_N,
                config.openrouter_max_prompt_price_per_m,
                config.openrouter_min_intelligence_index,
                OR_SUPPORTED_PARAMETERS
            ),
        );
    }

    // ── Fallback 1: Provider API directly ─────────────────────────────
    let router = InferenceRouter::new(config.clone());
    let router_models = router.list_models().await;
    if !router_models.is_empty() {
        let models = build_from_router(router_models);
        if !models.is_empty() {
            return (
                models,
                format!(
                    "provider API ({})",
                    crate::onboarding::provider_display_name(config)
                ),
            );
        }
    }

    // ── Fallback 2: Static curated lists ──────────────────────────────
    let models = build_fallback(config);
    (
        models,
        format!(
            "curated fallback ({} unreachable)",
            crate::onboarding::provider_display_name(config)
        ),
    )
}

// ── OpenRouter pipeline ───────────────────────────────────────────────────

async fn run_openrouter_pipeline(
    config: &InferenceConfig,
) -> Result<Vec<OnboardingModel>, CliError> {
    let client = config
        .build_client()
        .map_err(|e| CliError::Onboarding(e.to_string()))?;
    let url = format!(
        "{}/v1/models?output_modalities=text&sort=intelligence-high-to-low&supported_parameters={}&max_price={}&min_intelligence_index={}",
        config.openrouter_base_url.trim_end_matches('/'),
        OR_SUPPORTED_PARAMETERS,
        config.openrouter_max_prompt_price_per_m,
        config.openrouter_min_intelligence_index
    );

    let mut req = client
        .get(&url)
        .header("User-Agent", "hKask-onboarding/0.31");
    // The /v1/models endpoint is public, but we send the Authorization
    // header when a key is available so OpenRouter can personalize results.
    if !config.openrouter_api_key.is_empty() {
        req = req.header(
            "Authorization",
            format!("Bearer {}", config.openrouter_api_key),
        );
    }
    let resp = req
        .send()
        .await
        .map_err(|e| CliError::Onboarding(format!("OpenRouter API request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(CliError::Onboarding(format!(
            "OpenRouter API returned {}",
            resp.status()
        )));
    }

    let list: OpenRouterModelList = resp
        .json()
        .await
        .map_err(|e| CliError::Onboarding(format!("OpenRouter parse error: {e}")))?;

    let mut filtered: Vec<(OnboardingModel, f64)> = Vec::new();
    for model in list.data {
        if !is_text_model(&model) {
            continue;
        }

        let prompt_price =
            price_per_million(model.pricing.as_ref().and_then(|p| p.prompt.as_deref()));
        let completion_price =
            price_per_million(model.pricing.as_ref().and_then(|p| p.completion.as_deref()));

        let (prompt_price, completion_price) = match (prompt_price, completion_price) {
            (Some(p), Some(c)) => (p, c),
            _ => continue,
        };

        if prompt_price > config.openrouter_max_prompt_price_per_m {
            continue;
        }

        let intelligence = intelligence_index(&model.benchmarks).unwrap_or(-1.0);
        let agentic = agentic_index(&model.benchmarks).unwrap_or(-1.0);
        if intelligence < config.openrouter_min_intelligence_index {
            continue;
        }

        let family = model_family(&model);
        let label = model
            .name
            .clone()
            .unwrap_or_else(|| shorten_for_display(&model.id));
        let kind = classify_kind(&model.id, model.name.as_deref());
        let description = format!(
            "IA {:.1}, AA {:.1} — ${:.2}/M in, ${:.2}/M out",
            intelligence, agentic, prompt_price, completion_price
        );

        let onboarding = OnboardingModel {
            label,
            full_id: ProviderId::OpenRouter.prefix_model(&model.id),
            description,
            score: (intelligence * 10.0) as u32,
            source: ModelSource::Dynamic,
            kind,
            family,
        };
        filtered.push((onboarding, intelligence));
    }

    if filtered.is_empty() {
        return Err(CliError::Onboarding(
            "No qualifying models found via OpenRouter".into(),
        ));
    }

    filtered.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let models = filtered
        .into_iter()
        .take(OR_TOP_N)
        .map(|(m, _)| m)
        .collect::<Vec<_>>();

    Ok(models)
}

fn is_text_model(model: &OpenRouterModel) -> bool {
    model
        .architecture
        .as_ref()
        .map(|arch| arch.output_modalities.iter().any(|m| m == "text"))
        .unwrap_or(true)
}

fn price_per_million(raw: Option<&str>) -> Option<f64> {
    let per_token = raw?.parse::<f64>().ok()?;
    Some(per_token * 1_000_000.0)
}

fn intelligence_index(benchmarks: &Option<Value>) -> Option<f64> {
    let value = benchmarks.as_ref()?;
    find_index(value, "intelligence_index")
}

fn agentic_index(benchmarks: &Option<Value>) -> Option<f64> {
    let value = benchmarks.as_ref()?;
    find_index(value, "agentic_index")
}

fn find_index(value: &Value, key: &str) -> Option<f64> {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.get(key) {
                if let Some(f) = v.as_f64() {
                    return Some(f);
                }
                if let Some(s) = v.as_str() {
                    return s.parse::<f64>().ok();
                }
            }
            for v in map.values() {
                if let Some(found) = find_index(v, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().filter_map(|v| find_index(v, key)).next(),
        _ => None,
    }
}

fn model_family(model: &OpenRouterModel) -> String {
    model
        .architecture
        .as_ref()
        .and_then(|arch| arch.tokenizer.as_ref())
        .map(|t| t.to_lowercase())
        .unwrap_or_else(|| extract_family_from_id(&model.id))
}

// ── Display helpers ────────────────────────────────────────────────────────

pub(crate) fn shorten_for_display(id: &str) -> String {
    // Strip provider prefix (DI/, KC/, etc.) and org prefix
    let base = id.split_once('/').map(|x| x.1).unwrap_or(id);
    let base = base.split_once('/').map(|x| x.1).unwrap_or(base);
    let base = if base.is_empty() { id } else { base };

    base.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => {
                    f.to_uppercase().collect::<String>() + c.as_str().to_lowercase().as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract model family from the model ID by parsing the first segment.
fn extract_family_from_id(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    // Try splitting on `/` — take the last part, then first segment before `-`
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let first = base.split('-').next().unwrap_or(base);
    // Strip trailing digits/version numbers from family name
    first
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
        .trim()
        .to_string()
}

// ── Fallback builders ──────────────────────────────────────────────────────

/// Heuristic: model IDs that indicate a non-chat model (embeddings, OCR,
/// image generation, safety classifiers, free-tier placeholders, auto-routers).
/// Used to filter the provider-API fallback when OpenRouter is unreachable.
const NON_CHAT_PATTERNS: &[&str] = &[
    // Embedding / reranking
    "embed",
    "rerank",
    "mxbai-embed",
    // OCR / vision-only
    "paddleocr",
    "docres",
    "lightonocr",
    "olmocr",
    "deepseek-ocr",
    // Image / audio generation
    "lyria",
    "nano-banana",
    // Safety / moderation
    "safety",
    "safeguard",
    "guard",
    // Auto-routing placeholders (not real models)
    "auto-beta",
    "openrouter/auto",
    "openrouter/free",
    "openrouter/fusion",
    "openrouter/pareto",
    // Free-tier duplicates (we prefer the paid variant)
    ":free",
    // Cloud variants (prefer the local-direct variant)
    "-cloud",
    // Tiny models unlikely to meet intelligence threshold
    ":1b",
    ":2b",
    ":3b",
    ":4b",
    "-1b",
    "-2b",
    "-3b",
    "-4b",
];

/// Heuristic: filter out non-chat models from the provider API list.
/// This is a coarse filter — the OpenRouter pipeline is the source of truth
/// for cost/intelligence/parameter filtering. This only runs when OpenRouter
/// is unreachable.
fn is_likely_chat_model(model_id: &str) -> bool {
    let lower = model_id.to_lowercase();
    !NON_CHAT_PATTERNS.iter().any(|p| lower.contains(p))
}

fn build_from_router(models: Vec<RouterModelEntry>) -> Vec<OnboardingModel> {
    // Filter out non-chat models (embeddings, OCR, image-gen, safety, free,
    // cloud, tiny) before classification. This is a coarse heuristic — the
    // OpenRouter pipeline is the source of truth for cost/intelligence
    // filtering, but this ensures the fallback doesn't dump every model
    // from the provider API.
    let chat_models: Vec<RouterModelEntry> = models
        .into_iter()
        .filter(|m| is_likely_chat_model(&m.model))
        .collect();

    // Classify and deduplicate by family
    let mut by_family: HashMap<String, Vec<RouterModelEntry>> = HashMap::new();

    for m in chat_models {
        let family = extract_family_from_id(&m.model);
        by_family.entry(family).or_default().push(m);
    }

    let mut results: Vec<OnboardingModel> = Vec::new();
    for (family, family_models) in &by_family {
        let mut thinking: Vec<&RouterModelEntry> = family_models
            .iter()
            .filter(|m| classify_kind(&m.model, None) == ModelKind::Thinking)
            .collect();
        let mut instruct: Vec<&RouterModelEntry> = family_models
            .iter()
            .filter(|m| classify_kind(&m.model, None) == ModelKind::Instruct)
            .collect();

        thinking.sort_by(|a, b| b.model.cmp(&a.model));
        instruct.sort_by(|a, b| b.model.cmp(&a.model));

        if let Some(m) = thinking.first() {
            results.push(build_onboarding_entry(m, family, ModelKind::Thinking));
        }
        if let Some(m) = instruct.first() {
            results.push(build_onboarding_entry(m, family, ModelKind::Instruct));
        }
    }

    results
}

fn build_onboarding_entry(m: &RouterModelEntry, family: &str, kind: ModelKind) -> OnboardingModel {
    let kind_label = if kind == ModelKind::Thinking {
        "⚡ Thinking"
    } else {
        "Instruct"
    };
    OnboardingModel {
        label: shorten_for_display(&m.prefixed_name),
        full_id: m.prefixed_name.clone(),
        description: format!("{} — {}", kind_label, m.model),
        score: 0,
        source: ModelSource::Dynamic,
        kind,
        family: family.to_string(),
    }
}

// ── Static fallbacks ───────────────────────────────────────────────────────

const DEEPINFRA_FALLBACK: &[(&str, &str, ModelKind)] = &[
    (
        "deepseek-ai/DeepSeek-V4-Pro",
        "1.6T MoE, 1M ctx, top coding",
        ModelKind::Instruct,
    ),
    (
        "deepseek-ai/DeepSeek-R1-0528",
        "R1 reasoning, deep thought",
        ModelKind::Thinking,
    ),
    (
        "zai-org/GLM-5.2",
        "744B MoE, MIT, GPQA leader",
        ModelKind::Instruct,
    ),
    (
        "moonshotai/Kimi-K2.6",
        "1T MoE, agent swarms",
        ModelKind::Instruct,
    ),
    (
        "MiniMaxAI/MiniMax-M3",
        "1M ctx, multimodal, SWE-Bench",
        ModelKind::Instruct,
    ),
    (
        "Qwen/Qwen3.5-397B-A17B",
        "397B MoE, Apache 2.0",
        ModelKind::Instruct,
    ),
    (
        "nvidia/NVIDIA-Nemotron-3-Super-120B-A12B",
        "120B MoE, 1M ctx",
        ModelKind::Instruct,
    ),
    (
        "google/gemma-4-31B-it",
        "31B dense, Apache 2.0",
        ModelKind::Instruct,
    ),
    (
        "deepseek-ai/DeepSeek-V4-Flash",
        "284B MoE, efficient 1M ctx",
        ModelKind::Instruct,
    ),
];

const KILOCODE_FALLBACK: &[(&str, &str, ModelKind)] = &[
    ("deepseek-v4-pro", "1.6T MoE, 1M ctx", ModelKind::Instruct),
    ("deepseek-r1", "R1 reasoning", ModelKind::Thinking),
    ("glm-5.2", "744B MoE, MIT", ModelKind::Instruct),
    ("kimi-k2.6", "1T MoE, agent swarms", ModelKind::Instruct),
    ("minimax-m3", "1M ctx, multimodal", ModelKind::Instruct),
    (
        "qwen3.5-397b-a17b",
        "397B MoE, Apache 2.0",
        ModelKind::Instruct,
    ),
    ("nemotron-3-super", "120B MoE, 1M ctx", ModelKind::Instruct),
    (
        "gemma-4-31b-it",
        "31B dense, Apache 2.0",
        ModelKind::Instruct,
    ),
    (
        "deepseek-v4-flash",
        "284B MoE, efficient",
        ModelKind::Instruct,
    ),
];

fn build_fallback(config: &InferenceConfig) -> Vec<OnboardingModel> {
    let list = match config.default_provider {
        ProviderId::KiloCode => KILOCODE_FALLBACK,
        _ => DEEPINFRA_FALLBACK,
    };

    list.iter()
        .map(|(id, desc, kind)| OnboardingModel {
            label: shorten_for_display(id),
            full_id: config.default_provider.prefix_model(id),
            description: format!(
                "{} — {}",
                if *kind == ModelKind::Thinking {
                    "⚡ Thinking"
                } else {
                    "Instruct"
                },
                desc
            ),
            score: 0,
            source: ModelSource::Fallback,
            kind: kind.clone(),
            family: extract_family_from_id(id),
        })
        .collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_per_million_converts_from_per_token() {
        assert_eq!(price_per_million(Some("0.000001")).unwrap(), 1.0);
        assert_eq!(price_per_million(Some("0.000005")).unwrap(), 5.0);
    }

    #[test]
    fn classify_kind_detects_thinking() {
        assert_eq!(
            classify_kind("deepseek-ai/DeepSeek-R1-0528", None),
            ModelKind::Thinking
        );
        assert_eq!(
            classify_kind("Qwen/Qwen3-Max-Thinking", None),
            ModelKind::Thinking
        );
        assert_eq!(
            classify_kind("deepseek-ai/DeepSeek-V4-Pro", None),
            ModelKind::Instruct
        );
    }

    #[test]
    fn family_from_id_fallback() {
        assert_eq!(
            extract_family_from_id("some-org/UnknownModel-v2"),
            "unknownmodel"
        );
        assert_eq!(extract_family_from_id("deepseek-v4-pro"), "deepseek");
    }

    #[test]
    fn finds_indexes_nested() {
        let value = serde_json::json!({
            "aa": { "agentic_index": 10.0, "intelligence_index": 42.5 }
        });
        assert_eq!(find_index(&value, "intelligence_index"), Some(42.5));
        assert_eq!(find_index(&value, "agentic_index"), Some(10.0));
    }

    #[test]
    fn shorten_display_strips_prefixes() {
        assert_eq!(shorten_for_display("OR/openai/gpt-4o"), "Gpt 4o");
    }

    #[test]
    fn is_likely_chat_model_filters_non_chat() {
        // Embeddings, OCR, image-gen, safety, free, cloud, auto-router → filtered out
        assert!(!is_likely_chat_model("mxbai-embed-large:335m"));
        assert!(!is_likely_chat_model("paddleocr"));
        assert!(!is_likely_chat_model("LightOnOCR-2:1b"));
        assert!(!is_likely_chat_model("google/lyria-3-pro-preview"));
        assert!(!is_likely_chat_model(
            "nvidia/nemotron-3.5-content-safety:free"
        ));
        assert!(!is_likely_chat_model("openrouter/auto-beta"));
        assert!(!is_likely_chat_model("qwen3.5:397b-cloud"));
        assert!(!is_likely_chat_model("qwen3.5:4b"));
        // Real chat models (including small ones) → pass
        assert!(is_likely_chat_model("deepseek-ai/DeepSeek-V4-Pro"));
        assert!(is_likely_chat_model("zai-org/GLM-5.2"));
        assert!(is_likely_chat_model("Qwen/Qwen3.5-397B-A17B"));
        assert!(is_likely_chat_model("deepseek-ai/DeepSeek-R1-0528"));
        assert!(is_likely_chat_model("ornith:9b"));
        assert!(is_likely_chat_model("llama3.1:8b"));
    }
}
