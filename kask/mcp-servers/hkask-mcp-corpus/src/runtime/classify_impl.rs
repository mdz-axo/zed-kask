//! Section type classifier — config-driven, multi-provider.
//!
//! Classifier configs live in registry/classify/{name}.yaml.
//! corpus.yaml references which one to use via the `classifier` field.
//!
//! Routes through zed's `LanguageModelRegistry` via `InferencePort::generate_with_model`,
//! which forwards the provider-prefixed model string to the IPC bridge.
//! Graceful degradation: no model resolved → all passages default to fallback category.

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;
use hkask_types::json_extract as llm_json;
use hkask_types::template::LLMParameters;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use hkask_guard::{ContentGuard, GuardConfig};

/// Mandatory content safety guard — always active, not configurable off.
/// P3.1 Social Generativity: core controls cannot be disabled.
static GUARD: LazyLock<ContentGuard> =
    LazyLock::new(|| ContentGuard::mandatory(&GuardConfig::from_env()));

/// Classification result for a single passage.
#[derive(Debug, Clone)]
pub struct ClassifyResult {
    /// The classified section type.
    pub category: String,
    /// Number of prompt (input) tokens consumed.
    pub prompt_tokens: u64,
    /// Number of completion (output) tokens consumed.
    pub completion_tokens: u64,
    /// Number of cached prompt tokens (billed at discounted rate).
    pub cached_tokens: u64,
    /// Actual API cost in micro-rJoules (µrJ).
    pub cost_urj: u64,
    /// True if the API call failed but token/cost data was recovered.
    pub failed: bool,
    /// Provider that served this classification (e.g., "deepinfra").
    pub provider: String,
}

/// Semantic h_mem extraction result for a single passage.
/// Produced by the h_mem-extractor classifier.
/// Model configured via `HKASK_CLASSIFIER_MODEL` → `registry/classify/hmem-extractor.yaml`.
#[derive(Debug, Clone, Default)]
pub struct TripleExtraction {
    /// One-sentence summary of what the passage is about.
    pub topic: String,
    /// Key concepts mentioned in the passage.
    pub concepts: Vec<String>,
    /// Named entities, tools, frameworks, or services mentioned.
    pub entities: Vec<String>,
    /// Relationships between concepts or entities.
    pub relationships: Vec<String>,
    /// Which Gentle Lovelace dimension the passage primarily exemplifies.
    pub primary_dimension: String,
    /// Quality assessment flags for the passage.
    pub quality_flags: Vec<String>,
    /// Extra fields from classifier output that don't map to the standard fields.
    /// Each key-value pair is stored as a h_mem: entity_ref → key → value.
    /// Literary classifiers use this for themes, characters, setting, tone, imagery, etc.
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

/// Classifier configuration loaded from registry/classify/{name}.yaml.
#[derive(Debug, Deserialize)]
pub struct ClassifierYaml {
    pub classifier: ClassifierDef,
}

#[derive(Debug, Deserialize)]
pub struct ClassifierDef {
    pub name: String,
    /// Provider-prefixed model id (e.g. `DeepInfra/Qwen/Qwen3-235B-A22B-Instruct-2507`)
    /// passed as `model_override` to `InferencePort::generate_with_model`, which
    /// forwards it to zed's `LanguageModelRegistry` via the IPC bridge. When empty,
    /// `ClassifierConfig::from_def` resolves the canonical classifier model via
    /// `HKASK_CLASSIFIER_MODEL` → `DEFAULT_CLASSIFIER_MODEL`. Leave empty in
    /// `registry/classify/*.yaml` to defer to the single canonical path.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    pub concurrency: usize,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    pub system_prompt: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default)]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_fallback")]
    pub fallback_category: String,
    /// API input token cost in nano-rJ per token (e.g., 30,000 for DeepInfra $0.03/M).
    /// 1 µrJ = 1,000 nJ. Zero means cost tracking disabled.
    #[serde(default)]
    pub cost_input_nj_per_token: u64,
    /// API output token cost in nano-rJ per token (e.g., 60,000 for DeepInfra $0.06/M).
    #[serde(default)]
    pub cost_output_nj_per_token: u64,
    /// API cached input token read cost in nano-rJ per token.
    /// Only charged if the provider supports prompt caching.
    #[serde(default)]
    pub cost_cache_read_nj_per_token: u64,
}

impl Default for ClassifierDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            model: String::new(),
            provider: String::new(),
            concurrency: 1,
            timeout_secs: 30,
            system_prompt: String::new(),
            base_url: String::new(),
            api_key_env: String::new(),
            temperature: 0.0,
            max_tokens: 15,
            fallback_category: "Statement".to_string(),
            cost_input_nj_per_token: 0,
            cost_output_nj_per_token: 0,
            cost_cache_read_nj_per_token: 0,
        }
    }
}

fn default_timeout() -> u64 {
    30
}
fn default_max_tokens() -> u32 {
    15
}
fn default_fallback() -> String {
    "Statement".to_string()
}

/// Load a classifier config from registry/classify/{name}.yaml.
#[must_use = "result must be used"]
pub fn load_classifier_config(
    name: &str,
    registry_dir: &Path,
) -> Result<ClassifierDef, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.classify", operation = "load_config", classifier = %name, "REG");

    let config_path = registry_dir.join("classify").join(format!("{name}.yaml"));
    let yaml_str = std::fs::read_to_string(&config_path).map_err(|e| {
        let msg = format!(
            "Failed to read classifier config {}: {e}",
            config_path.display()
        );
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: msg,
        }
    })?;
    let config: ClassifierYaml = serde_yaml_neo::from_str(&yaml_str).map_err(|e| {
        let msg = format!(
            "Failed to parse classifier config {}: {e}",
            config_path.display()
        );
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: msg,
        }
    })?;
    Ok(config.classifier)
}

/// Runtime classifier configuration (derived from YAML + env).
#[derive(Clone)]
pub struct ClassifierConfig {
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub system_prompt: String,
    pub concurrency: usize,
    pub timeout: Duration,
    pub temperature: f64,
    pub max_tokens: u32,
    pub fallback_category: String,
    pub cost_input_nj_per_token: u64,
    pub cost_output_nj_per_token: u64,
    pub cost_cache_read_nj_per_token: u64,
}

impl ClassifierConfig {
    /// Build from a ClassifierDef, resolving API key from environment.
    /// Auto-derives token costs from provider name when not specified in YAML.
    pub fn from_def(def: &ClassifierDef) -> Self {
        let api_key = if def.api_key_env.is_empty() {
            String::new()
        } else {
            std::env::var(&def.api_key_env).unwrap_or_default()
        };
        // Auto-derive pricing from provider if not explicitly configured
        let (input_nj, output_nj) =
            if def.cost_input_nj_per_token == 0 && def.cost_output_nj_per_token == 0 {
                provider_pricing(&def.provider)
            } else {
                (def.cost_input_nj_per_token, def.cost_output_nj_per_token)
            };
        // Canonical model resolution: an empty `model:` in the YAML defers to
        // `HKASK_CLASSIFIER_MODEL` → `DEFAULT_CLASSIFIER_MODEL`. The full
        // provider-prefixed model string is passed as `model_override` to
        // `InferencePort::generate_with_model`, which forwards it to zed's
        // `LanguageModelRegistry` via the IPC bridge — the registry resolves
        // the provider from the prefix, so we no longer strip it here.
        let model = if def.model.is_empty() {
            hkask_inference::model_constants::classifier_model()
        } else {
            def.model.clone()
        };
        Self {
            model,
            api_key,
            base_url: if def.base_url.is_empty() {
                "https://api.deepinfra.com/v1/openai/chat/completions".to_string()
            } else {
                def.base_url.clone()
            },
            system_prompt: def.system_prompt.clone(),
            concurrency: def.concurrency,
            timeout: Duration::from_secs(def.timeout_secs),
            temperature: def.temperature,
            max_tokens: def.max_tokens,
            fallback_category: def.fallback_category.clone(),
            cost_input_nj_per_token: input_nj,
            cost_output_nj_per_token: output_nj,
            cost_cache_read_nj_per_token: 0,
        }
    }
}

/// Provider pricing lookup table — nano-rJ per token (30 nJ = $0.03/M input).
/// Returns (input_nj_per_token, output_nj_per_token).
fn provider_pricing(provider: &str) -> (u64, u64) {
    match provider.to_lowercase().as_str() {
        "deepinfra" => (30, 60),  // $0.03/M in, $0.06/M out
        "together" => (20, 20),   // $0.02/M in, $0.02/M out (approximate)
        "openrouter" => (50, 50), // varies by model, conservative estimate
        "fal" => (40, 40),        // approximate
        _ => {
            tracing::warn!(
                target: "hkask.classify",
                provider = %provider,
                "Unknown provider — cost tracking disabled. Add cost_input_nj_per_token / cost_output_nj_per_token to classifier config."
            );
            (0, 0)
        }
    }
}

/// Classify a single passage.
async fn classify_one(
    inference_port: &dyn InferencePort,
    config: &ClassifierConfig,
    text: &str,
) -> Result<ClassifyResult, ServiceError> {
    // P3.1: mandatory input guard — scan before sending to any model
    let input_scan = GUARD.scan_input(text);
    if !input_scan.passed {
        return Ok(ClassifyResult {
            category: config.fallback_category.clone(),
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            cost_urj: 0,
            failed: true,
            provider: config.model.clone(),
        });
    }

    let parameters = LLMParameters {
        temperature: config.temperature as f32,
        max_tokens: config.max_tokens,
        top_p: 1.0,
        top_k: 0,
        system_prompt: Some(config.system_prompt.clone()),
        ..Default::default()
    };

    let result = match inference_port
        .generate_with_model(text, &parameters, Some(&config.model), None)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Classifier inference error — returning fallback"
            );
            return Ok(ClassifyResult {
                category: config.fallback_category.clone(),
                prompt_tokens: 0,
                completion_tokens: 0,
                cached_tokens: 0,
                cost_urj: 0,
                failed: true,
                provider: config.model.clone(),
            });
        }
    };

    let content = result.text.as_str();

    // P3.1: mandatory output guard — scan model output before processing
    let output_scan = GUARD.scan_output(content);
    let content = output_scan.output.content(content);

    // Parse the JSON category from the response
    let category = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
        parsed["category"]
            .as_str()
            .unwrap_or(&config.fallback_category)
            .to_string()
    } else {
        // Fallback: try to extract from raw text
        if content.contains("Evidence") {
            "Evidence".to_string()
        } else if content.contains("Diagram") {
            "Diagram".to_string()
        } else if content.contains("Implications") {
            "Implications".to_string()
        } else {
            config.fallback_category.clone()
        }
    };

    let prompt_tokens = u64::from(result.usage.prompt_tokens);
    let completion_tokens = u64::from(result.usage.completion_tokens);
    // The IPC bridge does not surface cached-token breakdown; cost is computed
    // from the totals at the configured input rate.
    let input_cost = (prompt_tokens * config.cost_input_nj_per_token) / 1_000_000;
    let output_cost = (completion_tokens * config.cost_output_nj_per_token) / 1_000_000;

    Ok(ClassifyResult {
        category,
        prompt_tokens,
        completion_tokens,
        cached_tokens: 0,
        cost_urj: input_cost + output_cost,
        failed: false,
        provider: config.model.clone(),
    })
}

/// Classify a batch of passages concurrently.
///
/// Returns results in the same order as the input texts.
/// Failed classifications default to "Statement".
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  texts must be non-empty; config must have valid timeout and concurrency
/// post: returns `Vec<ClassifyResult>` in input order; failed classifications fall back to config.fallback_category; all fallback if no model resolved
#[must_use = "result must be used"]
pub async fn classify_batch(
    texts: &[String],
    config: ClassifierConfig,
    inference_port: Arc<dyn InferencePort>,
) -> Result<Vec<ClassifyResult>, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.classify", operation = "classify_batch", item_count = texts.len(), "REG");

    if config.model.is_empty() {
        // No model resolved — return all fallback category (skip classification)
        let fallback = &config.fallback_category;
        return Ok(texts
            .iter()
            .map(|_| ClassifyResult {
                category: fallback.clone(),
                prompt_tokens: 0,
                completion_tokens: 0,
                cached_tokens: 0,
                cost_urj: 0,
                failed: false,
                provider: String::new(),
            })
            .collect());
    }

    let config = std::sync::Arc::new(config); // share across spawned tasks
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(config.concurrency));
    let mut handles = Vec::with_capacity(texts.len());

    for (i, text) in texts.iter().enumerate() {
        let inference_port = Arc::clone(&inference_port);
        let cfg = Arc::clone(&config);
        let text = text.clone();
        let permit = Arc::clone(&semaphore);

        handles.push(tokio::spawn(async move {
            let _permit = permit.acquire().await;
            let result = classify_one(inference_port.as_ref(), &cfg, &text).await;
            (i, result)
        }));
    }

    let mut results: Vec<Option<ClassifyResult>> = vec![None; texts.len()];
    for handle in handles {
        match handle.await {
            Ok((i, Ok(result))) => {
                results[i] = Some(result);
            }
            Ok((i, Err(e))) => {
                tracing::warn!(index = i, error = %e, "Classifier failed for passage, using fallback");
                results[i] = Some(ClassifyResult {
                    category: config.fallback_category.clone(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cached_tokens: 0,
                    cost_urj: 0,
                    failed: true,
                    provider: config.model.clone(),
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "Classifier task panicked");
            }
        }
    }

    Ok(results
        .into_iter()
        .map(|r| {
            r.unwrap_or(ClassifyResult {
                category: config.fallback_category.clone(),
                prompt_tokens: 0,
                completion_tokens: 0,
                cached_tokens: 0,
                cost_urj: 0,
                failed: true,
                provider: config.model.clone(),
            })
        })
        .collect())
}

// ── HMem Extraction ──────────────────────────────────────────────────

/// Extract semantic h_mems from a batch of passages.
/// Model is determined by `HKASK_CLASSIFIER_MODEL` env var / settings, falling back
/// to the model specified in registry/classify/{name}.yaml.
///
/// Returns results in the same order as the input texts.
/// Failed extractions default to empty TripleExtraction.
/// Graceful degradation: no model resolved → all empty extractions.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  texts must be non-empty; config must have valid timeout and concurrency
/// post: returns `Vec<TripleExtraction>` in input order; failed extractions fall back to empty; all empty if no model resolved
#[must_use = "result must be used"]
pub async fn extract_triples_batch(
    texts: &[String],
    config: &ClassifierConfig,
    inference_port: Arc<dyn InferencePort>,
) -> Result<Vec<TripleExtraction>, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.classify", operation = "extract_triples_batch", item_count = texts.len(), "REG");

    if config.model.is_empty() {
        tracing::info!("No model resolved for h_mem extraction — returning empty extractions");
        return Ok(texts.iter().map(|_| TripleExtraction::default()).collect());
    }

    let config = Arc::new(config.clone());
    let semaphore = Arc::new(tokio::sync::Semaphore::new(config.concurrency));
    let mut handles = Vec::with_capacity(texts.len());

    for (i, text) in texts.iter().enumerate() {
        let inference_port = Arc::clone(&inference_port);
        let cfg = Arc::clone(&config);
        let text = text.clone();
        let permit = Arc::clone(&semaphore);

        handles.push(tokio::spawn(async move {
            let _permit = permit.acquire().await;
            let result = extract_triples_one(inference_port.as_ref(), &cfg, &text).await;
            (i, result)
        }));
    }

    let mut results: Vec<Option<TripleExtraction>> = vec![None; texts.len()];
    for handle in handles {
        match handle.await {
            Ok((i, Ok(result))) => {
                results[i] = Some(result);
            }
            Ok((i, Err(e))) => {
                tracing::warn!(index = i, error = %e, "HMem extraction failed, using empty");
                results[i] = Some(TripleExtraction::default());
            }
            Err(e) => {
                tracing::warn!(error = %e, "HMem extraction task panicked");
            }
        }
    }

    Ok(results.into_iter().map(|r| r.unwrap_or_default()).collect())
}

/// Extract h_mems from a single passage.
async fn extract_triples_one(
    inference_port: &dyn InferencePort,
    config: &ClassifierConfig,
    text: &str,
) -> Result<TripleExtraction, ServiceError> {
    // P3.1: mandatory input guard — scan before sending to any model
    let input_scan = GUARD.scan_input(text);
    if !input_scan.passed {
        return Ok(TripleExtraction::default());
    }

    let parameters = LLMParameters {
        temperature: config.temperature as f32,
        max_tokens: config.max_tokens,
        top_p: 1.0,
        top_k: 0,
        system_prompt: Some(config.system_prompt.clone()),
        ..Default::default()
    };

    let result = inference_port
        .generate_with_model(text, &parameters, Some(&config.model), None)
        .await
        .map_err(|e| {
            let msg = format!("HMem extractor inference error: {e}");
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: msg,
            }
        })?;

    let content = result.text.as_str();

    // P3.1: mandatory output guard — scan model output before parsing
    let output_scan = GUARD.scan_output(content);
    let content = output_scan.output.content(content);

    // Parse the structured JSON from the response
    parse_triple_extraction(content)
}

/// Parse a TripleExtraction from classifier JSON response.
pub fn parse_triple_extraction(content: &str) -> Result<TripleExtraction, ServiceError> {
    // Brace-balanced extraction (RR-0028): the old first-brace to last-brace
    // slice approach silently merged an injected JSON block in the model's
    // reasoning preamble with its real answer. `extract_json_from_response`
    // returns exactly one top-level object, defeating the injection.
    let json_str = llm_json::extract_json_from_response(content);

    let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| {
        let msg = format!(
            "HMem extraction JSON parse error: {e}. Content: {}",
            &json_str[..json_str.len().min(200)]
        );
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            kind: ErrorKind::ServiceUnavailable,
            source: None,
            message: msg,
        }
    })?;

    Ok(TripleExtraction {
        topic: parsed["topic"].as_str().unwrap_or("").to_string(),
        concepts: parsed["concepts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        entities: parsed["entities"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        relationships: parsed["relationships"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        primary_dimension: parsed["primary_dimension"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        quality_flags: parsed["quality_flags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        extra: {
            // Capture any fields not in the standard schema
            let standard = [
                "topic",
                "concepts",
                "entities",
                "relationships",
                "primary_dimension",
                "quality_flags",
            ];
            let mut extra = std::collections::HashMap::new();
            if let Some(obj) = parsed.as_object() {
                for (key, val) in obj {
                    if !standard.contains(&key.as_str()) && !val.is_null() {
                        extra.insert(key.clone(), val.clone());
                    }
                }
            }
            extra
        },
    })
}
