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

/// Resolve the grounded-tagging model — the tagging-role chain (settings
/// card 9fc54490): an explicit per-call override, then the tagging setting
/// (`HKASK_MEDIA_TAGGING_MODEL`), then the configured analysis model —
/// a vision-capable workload, so the vision default is a legitimate
/// fallback; `None` when nothing is configured (the caller fails visibly
/// naming the envs).
fn resolve_tagging_model(model_override: Option<&str>) -> Result<String, LayerPassError> {
    model_override
        .map(str::to_string)
        .or_else(crate::models::tagging_model)
        .or_else(crate::models::vision_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or(LayerPassError::NotConfigured {
            kind: "grounded tagging pass",
            env: "HKASK_MEDIA_TAGGING_MODEL or HKASK_MEDIA_VISION_MODEL (the tagging model setting or the configured analysis model)",
        })
}

/// Which model chain a layer pass resolves through. The shared chain is
/// per-call → `HKASK_MEDIA_PASS_MODEL` → the configured analysis model;
/// the tagging chain (the grounded vision mode, settings card 9fc54490)
/// is per-call → `HKASK_MEDIA_TAGGING_MODEL` → the configured analysis
/// model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerPassModelChain {
    Shared,
    Tagging,
}

impl LayerPassModelChain {
    /// Resolve the chain's model for one pass call. `pub(crate)` for the
    /// tool surfaces that run a single bounded vision call through the
    /// same chain as the chunked passes (video_caption, T7) — the resolve
    /// stays crate-internal; no `pub` surface is added.
    pub(crate) fn resolve(self, model_override: Option<&str>) -> Result<String, LayerPassError> {
        match self {
            Self::Shared => resolve_layer_model(model_override),
            Self::Tagging => resolve_tagging_model(model_override),
        }
    }
}
/// Resolve the audio-pass model — the audio-role chain (operator steer
/// 2026-10-30, card 9fc54490): an explicit per-call override, then the
/// sound-annotation setting (`HKASK_MEDIA_SOUND_ANNOTATION_MODEL`); `None`
/// when nothing is configured (the caller fails visibly naming the
/// setting). Audio-only models never route via `HKASK_MEDIA_PASS_MODEL` —
/// the shared pass-model env serves the text/vision passes, and routing an
/// audio-only model through it would break them; the vision default is not
/// audio-capable, so it is not a fallback here.
fn resolve_audio_layer_model(model_override: Option<&str>) -> Result<String, LayerPassError> {
    model_override
        .map(str::to_string)
        .or_else(crate::models::sound_annotation_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or(LayerPassError::NotConfigured {
            kind: "media audio layer pass",
            env: "HKASK_MEDIA_SOUND_ANNOTATION_MODEL (the sound annotation model setting)",
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

/// The bounded chunk-plan guard: validates the plan is non-empty, the
/// resume cursor is inside it, and the per-call chunk bound is at least
/// one; returns the slice `[resume_from, end)` the call processes.
fn plan_chunk_range(
    plan_len: usize,
    resume_from: usize,
    max_chunks: usize,
) -> Result<std::ops::Range<usize>, LayerPassError> {
    if plan_len == 0 {
        return Err(LayerPassError::ChunkPlan(
            "no chunks to process — the caller degrades before the core".to_string(),
        ));
    }
    if resume_from > plan_len {
        return Err(LayerPassError::ChunkPlan(format!(
            "resume cursor {resume_from} is past the chunk plan ({plan_len} chunks)"
        )));
    }
    if max_chunks == 0 {
        return Err(LayerPassError::ChunkPlan(
            "max_chunks must be at least 1".to_string(),
        ));
    }
    let end = (resume_from + max_chunks).min(plan_len);
    Ok(resume_from..end)
}

/// Extract and typed-parse one chunk's model output, ticking the
/// rejection counters (and the structured counters in structured mode) on
/// failure — the shared tail of every inference leg.
fn parse_chunk_output<TOutput: serde::de::DeserializeOwned>(
    model_text: &str,
    mode: PassMode,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<TOutput, LayerPassError> {
    let extracted = hkask_types::json_extract::extract_json_from_response(model_text);
    serde_json::from_str(&extracted).map_err(|_| {
        rejections.fetch_add(1, Ordering::Relaxed);
        if matches!(mode, PassMode::Structured) {
            STRUCTURED_LAYER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
        }
        LayerPassError::UnparseableOutput {
            expected,
            raw: truncate(&extracted, 200),
        }
    })
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
    let range = plan_chunk_range(chunk_prompts.len(), resume_from, max_chunks)?;
    let model = resolve_layer_model(model_override)?;
    let strict = match mode {
        PassMode::PromptSchema => None,
        PassMode::Structured => Some(strict_layer_schema(schema)?),
    };

    let mut outputs = Vec::with_capacity(range.len());
    for prompt in &chunk_prompts[range.clone()] {
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
        let output: TOutput = parse_chunk_output(&model_text, mode, rejections, expected)?;
        outputs.push(output);
    }
    Ok(LayerPassResult {
        outputs,
        resume_from: (range.end < chunk_prompts.len()).then_some(range.end),
        model,
    })
}

/// One vision chunk: the rendered prompt (the schema rides in it — vision
/// inputs have no provider-enforced schema path) plus the base64 PNG
/// images the chunk's sampled frames encode to.
pub struct LayerVisionChunk {
    pub prompt: String,
    pub images: Vec<String>,
}

/// Run one bounded slice of a vision layer pass: one `generate_vision`
/// call per chunk (the only image-carrying inference surface), the same
/// chunk/resume plumbing, typed parse errors, and counters as the text
/// leg, with the model chain selected by `chain` (`LayerPassModelChain::Shared`
/// for the standard pass, `::Tagging` for the grounded mode — settings card
/// 9fc54490). In structured mode the structured-output counters tick
/// and the caller's prompt carries the schema — strict provider-side
/// schema enforcement does not exist for image inputs (chat_json is
/// prompt+schema text-only), so the mode is surfaced honestly by the tool
/// rather than silently downgraded here.
pub async fn run_layer_pass_vision<TOutput: serde::de::DeserializeOwned>(
    inference: &Arc<dyn InferencePort>,
    mode: PassMode,
    model_override: Option<&str>,
    chain: LayerPassModelChain,
    chunks: &[LayerVisionChunk],
    resume_from: usize,
    max_chunks: usize,
    attempts: &AtomicU64,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<LayerPassResult<TOutput>, LayerPassError> {
    let range = plan_chunk_range(chunks.len(), resume_from, max_chunks)?;
    let model = chain.resolve(model_override)?;
    let mut outputs = Vec::with_capacity(range.len());
    for chunk in &chunks[range.clone()] {
        let result = inference
            .generate_vision(
                &chunk.prompt,
                &chunk.images,
                &LLMParameters::default(),
                Some(&model),
            )
            .await
            .map_err(LayerPassError::Inference)?;
        attempts.fetch_add(1, Ordering::Relaxed);
        if matches!(mode, PassMode::Structured) {
            STRUCTURED_LAYER_PASS_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        }
        let output: TOutput = parse_chunk_output(&result.text, mode, rejections, expected)?;
        outputs.push(output);
    }
    Ok(LayerPassResult {
        outputs,
        resume_from: (range.end < chunks.len()).then_some(range.end),
        model,
    })
}

/// One audio chunk: the rendered prompt (the schema rides in it — audio
/// inputs have no provider-enforced schema path) plus the audio file path
/// the chunk's extracted span lives at.
pub struct LayerAudioChunk {
    pub prompt: String,
    pub audio_path: String,
}

/// Run one bounded slice of an audio layer pass: one `media_generate`
/// "chat_audio" call per chunk (the speaker-pass audio wiring — the only
/// audio-carrying inference surface), the same chunk/resume plumbing,
/// model chain, typed parse errors, and counters as the text leg. In
/// structured mode the structured-output counters tick and the caller's
/// prompt carries the schema — strict provider-side schema enforcement
/// does not exist for audio inputs (chat_json is text-only), so the mode
/// is surfaced honestly by the tool rather than silently downgraded here.
pub async fn run_layer_pass_audio<TOutput: serde::de::DeserializeOwned>(
    inference: &Arc<dyn InferencePort>,
    mode: PassMode,
    model_override: Option<&str>,
    chunks: &[LayerAudioChunk],
    resume_from: usize,
    max_chunks: usize,
    attempts: &AtomicU64,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<LayerPassResult<TOutput>, LayerPassError> {
    let range = plan_chunk_range(chunks.len(), resume_from, max_chunks)?;
    let model = resolve_audio_layer_model(model_override)?;
    let mut outputs = Vec::with_capacity(range.len());
    for chunk in &chunks[range.clone()] {
        let params = hkask_types::MediaGenerateParams {
            prompt: Some(chunk.prompt.clone()),
            audio_url: Some(chunk.audio_path.clone()),
            model: Some(model.clone()),
            ..Default::default()
        };
        let raw = inference
            .media_generate("chat_audio", &params)
            .await
            .map_err(LayerPassError::Inference)?;
        attempts.fetch_add(1, Ordering::Relaxed);
        if matches!(mode, PassMode::Structured) {
            STRUCTURED_LAYER_PASS_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        }
        let model_text = raw
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                rejections.fetch_add(1, Ordering::Relaxed);
                if matches!(mode, PassMode::Structured) {
                    STRUCTURED_LAYER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
                }
                LayerPassError::UnparseableOutput {
                    expected: "chat_audio response {\"text\": …}",
                    raw: truncate(&raw.to_string(), 200),
                }
            })?
            .to_string();
        let output: TOutput = parse_chunk_output(&model_text, mode, rejections, expected)?;
        outputs.push(output);
    }
    Ok(LayerPassResult {
        outputs,
        resume_from: (range.end < chunks.len()).then_some(range.end),
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
