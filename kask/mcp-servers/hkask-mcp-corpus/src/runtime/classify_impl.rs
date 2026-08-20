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
use std::time::Duration;

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
    /// Provider that served this classification (e.g., "openrouter").
    pub provider: String,
}

/// Semantic h_mem extraction result for a single passage.
/// Produced by the h_mem-extractor classifier.
/// Model configured via `HKASK_CLASSIFIER_MODEL` → `registry/classify/hmem-extractor.yaml`.
#[derive(Debug, Clone, Default)]
pub struct PassageExtraction {
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
    /// Provider-prefixed model id (e.g. `OpenRouter/z-ai/glm-5.2`)
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
    pub temperature: f64,
    #[serde(default = "default_fallback")]
    pub fallback_category: String,
    /// API input token cost in nano-rJ per token (e.g., 30,000 at $0.03/M).
    /// 1 µrJ = 1,000 nJ. Zero means cost tracking disabled.
    #[serde(default)]
    pub cost_input_nj_per_token: u64,
    /// API output token cost in nano-rJ per token (e.g., 60,000 at $0.06/M).
    #[serde(default)]
    pub cost_output_nj_per_token: u64,
    /// API cached input token read cost in nano-rJ per token.
    /// Only charged if the provider supports prompt caching.
    #[serde(default)]
    pub cost_cache_read_nj_per_token: u64,
    /// Disable the model's thinking/reasoning mode for this classifier.
    /// Defaults to `true`: the registry/classify configs emit short, deterministic
    /// structured JSON (temperature 0.0, 15–500 tokens) where thinking tokens add
    /// latency and cost without improving the few-shot-equilibrated output (Martin
    /// et al. arXiv:2603.29878). Set to `false` to opt a classifier into thinking
    /// (e.g. qa-triage root-cause analysis). Maps to `enable_thinking: false` /
    /// `chat_template_kwargs: {"enable_thinking": false}` on the wire; harmless
    /// no-op on models without a thinking mode.
    #[serde(default = "default_disable_thinking")]
    pub disable_thinking: bool,
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
            temperature: 0.0,
            fallback_category: "Statement".to_string(),
            cost_input_nj_per_token: 0,
            cost_output_nj_per_token: 0,
            cost_cache_read_nj_per_token: 0,
            disable_thinking: true,
        }
    }
}

fn default_timeout() -> u64 {
    30
}
fn default_fallback() -> String {
    "Statement".to_string()
}
fn default_disable_thinking() -> bool {
    true
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
    pub system_prompt: String,
    pub concurrency: usize,
    pub timeout: Duration,
    pub temperature: f64,
    pub fallback_category: String,
    pub cost_input_nj_per_token: u64,
    pub cost_output_nj_per_token: u64,
    pub cost_cache_read_nj_per_token: u64,
    pub disable_thinking: bool,
}

impl ClassifierConfig {
    /// Build from a ClassifierDef, resolving the canonical model and auto-deriving token costs.
    pub fn from_def(def: &ClassifierDef) -> Self {
        // Per-token cost rates come from the classifier config
        // (`cost_input_nj_per_token` / `cost_output_nj_per_token`). There is no
        // hardcoded provider price fallback — fabricating per-token rates is the
        // operator-priced anti-pattern; unconfigured classifiers get (0, 0) and
        // their cost tracking is disabled (a warn makes the gap visible).
        let (input_nj, output_nj) = (def.cost_input_nj_per_token, def.cost_output_nj_per_token);
        if input_nj == 0 && output_nj == 0 {
            tracing::warn!(
                target: "hkask.classify",
                provider = %def.provider,
                "Classifier cost tracking disabled — no cost_input_nj_per_token / cost_output_nj_per_token set in the classifier config. Set them to enable nano-rJ cost accounting."
            );
        }
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
            system_prompt: def.system_prompt.clone(),
            concurrency: def.concurrency,
            timeout: Duration::from_secs(def.timeout_secs),
            temperature: def.temperature,
            fallback_category: def.fallback_category.clone(),
            cost_input_nj_per_token: input_nj,
            cost_output_nj_per_token: output_nj,
            cost_cache_read_nj_per_token: 0,
            disable_thinking: def.disable_thinking,
        }
    }
}

/// Classify a single passage.
async fn classify_one(
    inference_port: &dyn InferencePort,
    config: &ClassifierConfig,
    text: &str,
) -> Result<ClassifyResult, ServiceError> {
    let parameters = LLMParameters {
        temperature: config.temperature as f32,
        top_p: 1.0,
        top_k: 0,
        system_prompt: Some(config.system_prompt.clone()),
        disable_thinking: config.disable_thinking,
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
    cost_driver: Option<Arc<dyn hkask_storage::database::driver::DatabaseDriver>>,
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

    let results: Vec<ClassifyResult> = results
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
        .collect();

    // Post the aggregate rJoule cost to the ledger so the corpus server can
    // find efficiencies in LLM interactions. 1 rJoule = 1 USD; `cost_urj` is
    // in µrJ. Best-effort: a failed post warns but does not block the result.
    if let Some(driver) = cost_driver {
        let total_cost_urj: u64 = results.iter().map(|r| r.cost_urj).sum();
        if total_cost_urj > 0 {
            let provider = if results
                .first()
                .map(|r| r.provider.as_str())
                .unwrap_or("")
                .is_empty()
            {
                "unknown"
            } else {
                // The provider is the model name's namespace prefix (e.g.
                // "OpenRouter" from "OpenRouter/qwen/qwen3-embedding").
                // Fall back to "classify" if no prefix.
                results
                    .first()
                    .map(|r| r.provider.split('/').next().unwrap_or("classify"))
                    .unwrap_or("classify")
            };
            let reference = format!(
                "classify-batch-{}-{}",
                chrono::Utc::now().timestamp_micros(),
                uuid::Uuid::new_v4()
            );
            crate::cost::record_cost_best_effort(
                &driver,
                provider,
                total_cost_urj as i64,
                &reference,
                &serde_json::json!({
                    "operation": "classify_batch",
                    "item_count": texts.len(),
                    "model": config.model,
                }),
            );
        }
    }

    Ok(results)
}

// ── HMem Extraction ──────────────────────────────────────────────────

/// Extract semantic h_mems from a batch of passages.
/// Model is determined by `HKASK_CLASSIFIER_MODEL` env var / settings, falling back
/// to the model specified in registry/classify/{name}.yaml.
///
/// Returns results in the same order as the input texts.
/// Failed extractions default to empty PassageExtraction.
/// Graceful degradation: no model resolved → all empty extractions.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  texts must be non-empty; config must have valid timeout and concurrency
/// post: returns `Vec<PassageExtraction>` in input order; failed extractions fall back to empty; all empty if no model resolved
#[must_use = "result must be used"]
pub async fn extract_passages_batch(
    texts: &[String],
    config: &ClassifierConfig,
    inference_port: Arc<dyn InferencePort>,
) -> Result<Vec<PassageExtraction>, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.classify", operation = "extract_passages_batch", item_count = texts.len(), "REG");

    if config.model.is_empty() {
        tracing::info!("No model resolved for h_mem extraction — returning empty extractions");
        return Ok(texts.iter().map(|_| PassageExtraction::default()).collect());
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
            let result = extract_passage_one(inference_port.as_ref(), &cfg, &text).await;
            (i, result)
        }));
    }

    let mut results: Vec<Option<PassageExtraction>> = vec![None; texts.len()];
    for handle in handles {
        match handle.await {
            Ok((i, Ok(result))) => {
                results[i] = Some(result);
            }
            Ok((i, Err(e))) => {
                tracing::warn!(index = i, error = %e, "HMem extraction failed, using empty");
                results[i] = Some(PassageExtraction::default());
            }
            Err(e) => {
                tracing::warn!(error = %e, "HMem extraction task panicked");
            }
        }
    }

    Ok(results.into_iter().map(|r| r.unwrap_or_default()).collect())
}

/// Extract h_mems from a single passage.
async fn extract_passage_one(
    inference_port: &dyn InferencePort,
    config: &ClassifierConfig,
    text: &str,
) -> Result<PassageExtraction, ServiceError> {
    let parameters = LLMParameters {
        temperature: config.temperature as f32,
        top_p: 1.0,
        top_k: 0,
        system_prompt: Some(config.system_prompt.clone()),
        disable_thinking: config.disable_thinking,
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

    // Parse the structured JSON from the response
    parse_passage_extraction(content)
}

/// Parse a PassageExtraction from classifier JSON response.
pub fn parse_passage_extraction(content: &str) -> Result<PassageExtraction, ServiceError> {
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

    Ok(PassageExtraction {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The classifier thinking-disabled default is the invariant this change
    /// introduces: registry/classify configs must run with thinking off by
    /// default so short structured-JSON outputs stay prompt and cheap. If this
    /// regresses, every registry/classify/*.yaml silently re-enables thinking.
    #[test]
    fn classifier_def_default_disables_thinking() {
        assert!(ClassifierDef::default().disable_thinking);
    }

    /// `from_def` must propagate the def's `disable_thinking` into the runtime
    /// config — the field is only effective if it reaches `classify_one` /
    /// `extract_passage_one`, which read it off `ClassifierConfig`.
    #[test]
    fn from_def_propagates_disable_thinking_true() {
        let cfg = ClassifierConfig::from_def(&ClassifierDef::default());
        assert!(cfg.disable_thinking);
    }

    /// A YAML that omits `disable_thinking` must deserialize to `true` via the
    /// serde default — the 7 shipped registry/classify configs rely on this.
    #[test]
    fn yaml_without_field_defaults_to_disabled() {
        let yaml = "classifier:\n  name: t\n  concurrency: 1\n  system_prompt: x\n";
        let parsed: ClassifierYaml = serde_yaml_neo::from_str(yaml).unwrap();
        assert!(parsed.classifier.disable_thinking);
    }

    /// A YAML that opts into thinking (`disable_thinking: false`) must be
    /// honored — qa-triage root-cause analysis is the documented opt-in case.
    #[test]
    fn yaml_opt_in_thinking_is_honored() {
        let yaml = "classifier:\n  name: t\n  concurrency: 1\n  system_prompt: x\n  disable_thinking: false\n";
        let parsed: ClassifierYaml = serde_yaml_neo::from_str(yaml).unwrap();
        assert!(!parsed.classifier.disable_thinking);
        let cfg = ClassifierConfig::from_def(&parsed.classifier);
        assert!(!cfg.disable_thinking);
    }
}
