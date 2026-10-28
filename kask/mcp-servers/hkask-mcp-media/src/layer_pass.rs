//! Media layer LLM pass core — the bounded, resumable inference pipeline
//! every model-produced media layer (t4's vision passes, t5's audio pass)
//! runs over, mirroring `transcript_pass::run_pass_core` for media chunks.
//!
//! Offload map (the transcript-pass discipline): the model owns the
//! semantics; the deterministic layer owns everything else — chunk bounds,
//! typed parsing, validation, provenance, storage. The caller renders its
//! own per-chunk prompts (vision frame sampling and audio wiring are
//! pass-specific and stay OUT of this module), pre-chunks its input, and
//! stores through the t1 layer store.
//!
//! Both inference modes: `PromptSchema` (v1, the default — the schema
//! rides in the prompt; works with every catalog model) and `Structured`
//! (v2 — `chat_json` with a strict `response_format: json_schema`, the
//! provider enforcing the schema). Model resolution follows the shared
//! pass chain: an explicit per-call override, then `HKASK_MEDIA_PASS_MODEL`,
//! then the configured analysis model (`HKASK_MEDIA_VISION_MODEL`) — never
//! a hidden code constant.
//!
//! Failures surface as typed named errors (the unparseable-output class
//! carries the expected shape and the truncated raw text) — never silent
//! empty results.

use crate::transcript_pass::PassMode;
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Structured-mode (v2) attempts and rejections across all layer passes —
/// the mechanism-level A/B measurement against the per-pass v1 totals
/// (mirrors `transcript_pass`'s shared structured counters).
static STRUCTURED_LAYER_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static STRUCTURED_LAYER_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);

/// Named layer-pass failures — per-variant, never blanket (mirrors
/// `transcript_pass::PassError` over the media layer store's validation).
#[derive(Debug, thiserror::Error)]
pub enum LayerPassError {
    #[error("model output failed to parse as {expected}: {raw}")]
    UnparseableOutput { expected: &'static str, raw: String },
    #[error("layer validation failed: {0}")]
    Validation(#[from] crate::layer_store::LayerValidationError),
    #[error("prompt construction failed: {0}")]
    Prompt(String),
    #[error("inference call failed: {0}")]
    Inference(#[from] hkask_types::InferenceError),
    #[error(
        "no {kind} model configured — set {env} or pass an explicit model; \
             kask never falls back to a hidden code constant"
    )]
    NotConfigured {
        kind: &'static str,
        env: &'static str,
    },
    /// The chunk plan is unusable — a bound violation the caller controls
    /// (zero max chunks, a resume cursor past the plan, an empty plan).
    #[error("invalid chunk plan: {0}")]
    ChunkPlan(String),
}

/// Resolve the pass model — the shared chain every LLM pass follows: an
/// explicit per-call override, then `HKASK_MEDIA_PASS_MODEL`, then the
/// configured analysis model (`HKASK_MEDIA_VISION_MODEL`); `None` when
/// nothing is configured (the caller fails visibly naming the env var).
fn resolve_layer_model(model_override: Option<&str>) -> Result<String, LayerPassError> {
    model_override
        .map(str::to_string)
        .or_else(crate::models::pass_model)
        .or_else(crate::models::vision_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or(LayerPassError::NotConfigured {
            kind: "media layer pass",
            env: "HKASK_MEDIA_PASS_MODEL or HKASK_MEDIA_VISION_MODEL (the configured analysis model)",
        })
}

/// The bounded chunk/resume result: per-chunk typed outputs in chunk
/// order, the chunk index a follow-up call should resume from (`None`
/// when the plan is fully processed), and the resolved model (recorded in
/// the stored layer's provenance).
#[derive(Debug)]
pub struct LayerPassResult<TOutput> {
    pub outputs: Vec<TOutput>,
    pub resume_from: Option<usize>,
    pub model: String,
}

/// Run one bounded slice of a layer pass over pre-rendered chunk prompts:
/// `chunk_prompts[resume_from .. resume_from + max_chunks]`, one inference
/// call per chunk, typed parsing per chunk. When the plan extends past the
/// bound, the result's `resume_from` is the next chunk index — the caller
/// re-invokes with it to continue (the resume plumbing t4's
/// `max_chunks`-bounded passes consume). Attempts/rejections are the
/// caller-owned counters (per-pass measurement, mirroring
/// `transcript_pass::run_pass_core`).
pub async fn run_layer_pass<TOutput: serde::de::DeserializeOwned>(
    inference: &Arc<dyn InferencePort>,
    schema: &str,
    mode: PassMode,
    model_override: Option<&str>,
    chunk_prompts: &[String],
    resume_from: usize,
    max_chunks: usize,
    attempts: &AtomicU64,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<LayerPassResult<TOutput>, LayerPassError> {
    if chunk_prompts.is_empty() {
        return Err(LayerPassError::ChunkPlan(
            "no chunks to process — the caller degrades before the core".to_string(),
        ));
    }
    if resume_from > chunk_prompts.len() {
        return Err(LayerPassError::ChunkPlan(format!(
            "resume cursor {resume_from} is past the chunk plan ({} chunks)",
            chunk_prompts.len()
        )));
    }
    if max_chunks == 0 {
        return Err(LayerPassError::ChunkPlan(
            "max_chunks must be at least 1".to_string(),
        ));
    }
    let model = resolve_layer_model(model_override)?;
    let strict = match mode {
        PassMode::PromptSchema => None,
        PassMode::Structured => Some(strict_layer_schema(schema)?),
    };

    let end = (resume_from + max_chunks).min(chunk_prompts.len());
    let mut outputs = Vec::with_capacity(end - resume_from);
    for prompt in &chunk_prompts[resume_from..end] {
        let model_text = match mode {
            PassMode::PromptSchema => {
                let result = inference
                    .generate_with_model(prompt, &LLMParameters::default(), Some(&model), None)
                    .await
                    .map_err(LayerPassError::Inference)?;
                attempts.fetch_add(1, Ordering::Relaxed);
                result.text
            }
            PassMode::Structured => {
                let params = hkask_types::MediaGenerateParams {
                    prompt: Some(prompt.clone()),
                    schema: strict.clone(),
                    model: Some(model.clone()),
                    ..Default::default()
                };
                let raw = inference
                    .media_generate("chat_json", &params)
                    .await
                    .map_err(LayerPassError::Inference)?;
                attempts.fetch_add(1, Ordering::Relaxed);
                STRUCTURED_LAYER_PASS_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
                raw.get("text")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        rejections.fetch_add(1, Ordering::Relaxed);
                        STRUCTURED_LAYER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
                        LayerPassError::UnparseableOutput {
                            expected: "chat_json response {\"text\": …}",
                            raw: truncate(&raw.to_string(), 200),
                        }
                    })?
                    .to_string()
            }
        };
        let extracted = hkask_types::json_extract::extract_json_from_response(&model_text);
        let output: TOutput = serde_json::from_str(&extracted).map_err(|_| {
            rejections.fetch_add(1, Ordering::Relaxed);
            if matches!(mode, PassMode::Structured) {
                STRUCTURED_LAYER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
            }
            LayerPassError::UnparseableOutput {
                expected,
                raw: truncate(&extracted, 200),
            }
        })?;
        outputs.push(output);
    }
    Ok(LayerPassResult {
        outputs,
        resume_from: (end < chunk_prompts.len()).then_some(end),
        model,
    })
}

/// Normalize a schemars-generated schema for strict-mode structured
/// outputs: strict providers require `additionalProperties: false` on
/// every object schema, which schemars does not emit (verified by probe
/// 2026-08-31). Mirrors `transcript_pass::normalize_schema_object` — the
/// normalization contract is shared, the implementation is local so the
/// layer-pass core does not edit the concurrently-evolving transcript pass
/// module; unify when that surface settles. Deterministic — the same
/// schema always normalizes identically.
fn strict_layer_schema(schema: &str) -> Result<String, LayerPassError> {
    let value: serde_json::Value = serde_json::from_str(schema)
        .map_err(|e| LayerPassError::Prompt(format!("schema is not valid JSON: {e}")))?;
    let normalized = normalize_schema_object(value);
    serde_json::to_string(&normalized)
        .map_err(|e| LayerPassError::Prompt(format!("schema serialization: {e}")))
}

fn normalize_schema_object(mut value: serde_json::Value) -> serde_json::Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    object.remove("$schema");
    object.remove("format");
    if object.get("type") == Some(&serde_json::json!("object")) {
        object.insert("additionalProperties".to_string(), serde_json::json!(false));
    }
    if let Some(items) = object.get_mut("items") {
        *items = normalize_schema_object(std::mem::take(items));
    }
    if let Some(properties) = object.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for (_key, property) in properties.iter_mut() {
            *property = normalize_schema_object(std::mem::take(property));
        }
    }
    if let Some(defs) = object.get_mut("$defs").and_then(|d| d.as_object_mut()) {
        for (_name, definition) in defs.iter_mut() {
            *definition = normalize_schema_object(std::mem::take(definition));
        }
    }
    value
}

/// Bound a raw model output embedded in a typed error (the
/// `transcript_pass::truncate` contract).
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}…")
}
