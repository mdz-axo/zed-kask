//! Educt transcript LLM passes — the v1 pipeline (design doc §4):
//! schemars-derived schema in a minijinja prompt → `InferencePort` call →
//! `extract_json_from_response` → typed deserialize → per-layer invariants
//! → `LayerProvenance` attached by the pipeline, never claimed by the model.
//!
//! Landed passes: paragraph (slice 3), speaker + correction (slice 4). The
//! speaker pass is the **text-cue attribution** (v1 — works with every
//! catalog model today); the audio-capable-LLM path (the scaffold's
//! primary source, decisions 3/7) requires an audio-input generation method
//! on `InferencePort` — new inference surface, the same class as the v2
//! spike — and will slot in behind the same `SpeakerLayer` record with
//! provenance distinguishing the source.
//!
//! The pass counters are the measured v1 validation failure rate that
//! gates the v2 structured-outputs spike: every pass response carries
//! them, and the design doc's v2 decision reads the accumulated rate.
//!
//! Offload map (design doc §3): the model owns the semantics; the
//! deterministic layer owns everything else — index bounds, validation,
//! provenance, storage. The model never sees a timestamp and cannot emit
//! one.

use crate::transcript::{TimedWord, TranscriptBundle};
use crate::transcript_layers::{
    CorrectionEdit, CorrectionLayer, HighlightEntry, HighlightLayer, LayerProvenance,
    LayerValidationError, ParagraphLayer, SpeakerLayer, SpeakerSpan, TranscriptLayer,
};
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use minijinja::Environment;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Template names (registered in `templates.rs`).
pub const PARAGRAPH_PASS_TEMPLATE: &str = "educt_paragraph_pass";
pub const SPEAKER_PASS_TEMPLATE: &str = "educt_speaker_pass";
pub const SPEAKER_AUDIO_PASS_TEMPLATE: &str = "educt_speaker_audio_pass";
pub const SPEAKER_CORRECTION_PASS_TEMPLATE: &str = "educt_speaker_correction_pass";
pub const CORRECTION_PASS_TEMPLATE: &str = "educt_correction_pass";
pub const HIGHLIGHT_PASS_TEMPLATE: &str = "educt_highlight_pass";

/// The pass call mode — the v2 spike's opt-in instrument.
/// - `PromptSchema` (v1, the default): the schema rides in the prompt;
///   works with every catalog model.
/// - `Structured` (v2): `chat_json` with `response_format: json_schema`
///   (strict) — the provider enforces the schema. The validation gate
///   stays either way. Nothing adopts Structured by default; the A/B
///   between the per-pass totals and the `structured` stats decides
///   adoption (design doc §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassMode {
    PromptSchema,
    Structured,
}

static PARAGRAPH_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PARAGRAPH_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_CORRECTION_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_CORRECTION_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static CORRECTION_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static CORRECTION_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static HIGHLIGHT_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static HIGHLIGHT_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
/// Structured-mode (v2) attempts and rejections across all passes — the
/// mechanism-level A/B measurement against the per-pass v1 totals.
static STRUCTURED_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static STRUCTURED_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);

/// What the model returns for the paragraph pass — the layer shape minus
/// provenance (provenance is attached by the pipeline, never claimed by
/// the model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParagraphPassOutput {
    /// Word indices after which a paragraph break occurs.
    pub breaks_after: Vec<usize>,
}

/// What the model returns for the speaker pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeakerPassOutput {
    /// Speaker spans over word ranges.
    pub spans: Vec<SpeakerSpan>,
}

/// What the model returns for the correction pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CorrectionPassOutput {
    /// Proposed corrections over word ranges.
    pub edits: Vec<CorrectionEdit>,
}

/// What the model returns for the highlight pass — the semantic selection
/// (the agent-as-selection-engine improvement target: a natural-language
/// request resolved to word ranges).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HighlightPassOutput {
    /// The passages matching the selection request.
    pub highlights: Vec<HighlightEntry>,
}

/// Named pass failures — per-variant, never blanket.
#[derive(Debug, thiserror::Error)]
pub enum PassError {
    #[error("transcript has no word-level timings; the pass cannot anchor")]
    NoWordTimings,
    #[error("model output failed to parse as {expected}: {raw}")]
    UnparseableOutput { expected: &'static str, raw: String },
    #[error("layer validation failed: {0}")]
    Validation(#[from] LayerValidationError),
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
}

fn pass_stats_json(attempts: &AtomicU64, rejections: &AtomicU64) -> serde_json::Value {
    let attempts = attempts.load(Ordering::Relaxed);
    let rejections = rejections.load(Ordering::Relaxed);
    let structured_attempts = STRUCTURED_PASS_ATTEMPTS.load(Ordering::Relaxed);
    let structured_rejections = STRUCTURED_PASS_REJECTIONS.load(Ordering::Relaxed);
    serde_json::json!({
        "attempts": attempts,
        "rejections": rejections,
        "rejection_rate": if attempts == 0 {
            0.0
        } else {
            (rejections as f64) / (attempts as f64)
        },
        "structured": {
            "attempts": structured_attempts,
            "rejections": structured_rejections,
            "rejection_rate": if structured_attempts == 0 {
                0.0
            } else {
                (structured_rejections as f64) / (structured_attempts as f64)
            },
        },
    })
}

/// The accumulated paragraph-pass measurement — the v1 failure rate that
/// gates the v2 structured-outputs spike (design doc §4).
pub fn paragraph_pass_stats() -> serde_json::Value {
    pass_stats_json(&PARAGRAPH_PASS_ATTEMPTS, &PARAGRAPH_PASS_REJECTIONS)
}

/// The accumulated speaker-pass measurement.
pub fn speaker_pass_stats() -> serde_json::Value {
    pass_stats_json(&SPEAKER_PASS_ATTEMPTS, &SPEAKER_PASS_REJECTIONS)
}

/// The accumulated speaker-correction-pass measurement (the gated
/// lexical post-pass, plan §7 Phase 1b).
pub fn speaker_correction_pass_stats() -> serde_json::Value {
    pass_stats_json(
        &SPEAKER_CORRECTION_PASS_ATTEMPTS,
        &SPEAKER_CORRECTION_PASS_REJECTIONS,
    )
}

/// The accumulated correction-pass measurement.
pub fn correction_pass_stats() -> serde_json::Value {
    pass_stats_json(&CORRECTION_PASS_ATTEMPTS, &CORRECTION_PASS_REJECTIONS)
}

/// The accumulated highlight-pass measurement.
pub fn highlight_pass_stats() -> serde_json::Value {
    pass_stats_json(&HIGHLIGHT_PASS_ATTEMPTS, &HIGHLIGHT_PASS_REJECTIONS)
}

/// The shared v1 pass pipeline core: render (schema + indexed words) →
/// model → extract → typed parse. The caller wraps the typed output in
/// its layer, validates, and stores. Counters are per-pass.
async fn run_pass_core<TOutput: serde::de::DeserializeOwned>(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    template_name: &str,
    schema: &str,
    request: Option<&str>,
    words: &[TimedWord],
    mode: PassMode,
    model_override: Option<&str>,
    attempts: &AtomicU64,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<(TOutput, String), PassError> {
    // Text-layer passes are chat workloads, not speech-to-text: the STT
    // model cannot serve them (verified live 2026-10-25 — a whisper STT
    // model as model_override failed "not found" for the correction
    // pass). Resolution order (operator-authorized 2026-10-25,
    // superseding the 2026-09-09 "passes use the STT model" ruling): an
    // explicit per-call override, then HKASK_MEDIA_PASS_MODEL, then the
    // media server's configured analysis model (HKASK_MEDIA_VISION_MODEL
    // — a chat-capable model, always emitted with a non-reasoning
    // default).
    let resolved_model = model_override
        .map(str::to_string)
        .or_else(crate::models::pass_model)
        .or_else(crate::models::vision_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or(PassError::NotConfigured {
            kind: "text pass",
            env: "HKASK_MEDIA_PASS_MODEL or HKASK_MEDIA_VISION_MODEL (the configured analysis model)",
        })?;

    let prompt = render_pass_prompt(template_env, template_name, schema, request, words)?;
    let model_text = match mode {
        PassMode::PromptSchema => {
            let result = inference
                .generate_with_model(
                    &prompt,
                    &LLMParameters::default(),
                    Some(&resolved_model),
                    None,
                )
                .await
                .map_err(PassError::Inference)?;
            attempts.fetch_add(1, Ordering::Relaxed);
            result.text
        }
        PassMode::Structured => {
            let params = hkask_types::MediaGenerateParams {
                prompt: Some(prompt),
                schema: Some(strict_schema(schema)?),
                model: Some(resolved_model.clone()),
                ..Default::default()
            };
            let raw = inference
                .media_generate("chat_json", &params)
                .await
                .map_err(PassError::Inference)?;
            attempts.fetch_add(1, Ordering::Relaxed);
            STRUCTURED_PASS_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
            raw.get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    rejections.fetch_add(1, Ordering::Relaxed);
                    STRUCTURED_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
                    PassError::UnparseableOutput {
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
            STRUCTURED_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
        }
        PassError::UnparseableOutput {
            expected,
            raw: truncate(&extracted, 200),
        }
    })?;
    Ok((output, resolved_model))
}

/// Render a pass prompt: the indexed words plus the pass's schema, plus
/// the selection request when the pass takes one (the highlight pass).
fn render_pass_prompt(
    template_env: &Environment<'static>,
    template_name: &str,
    schema: &str,
    request: Option<&str>,
    words: &[TimedWord],
) -> Result<String, PassError> {
    let indexed_words = indexed_words_line(words);
    let words_count = words.len().to_string();
    let last_word_index = words.len().saturating_sub(1).to_string();
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("schema", schema);
    if let Some(request) = request {
        vars.insert("request", request);
    }
    vars.insert("words", indexed_words.as_str());
    vars.insert("words_count", words_count.as_str());
    vars.insert("last_word_index", last_word_index.as_str());
    crate::templates::render(template_env, template_name, &vars)
        .map_err(|e| PassError::Prompt(format!("{e}")))
}

/// Validate a produced layer against the transcript's word count; a
/// failure increments the pass's rejection counter (and the structured
/// counter in structured mode) before surfacing.
fn validate_or_reject(
    layer: &TranscriptLayer,
    words_count: usize,
    rejections: &AtomicU64,
    mode: PassMode,
) -> Result<(), PassError> {
    if let Err(error) = layer.validate(words_count) {
        rejections.fetch_add(1, Ordering::Relaxed);
        if matches!(mode, PassMode::Structured) {
            STRUCTURED_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
        }
        return Err(PassError::Validation(error));
    }
    Ok(())
}

/// Normalize a schemars-generated schema (JSON string) for strict-mode
/// structured outputs. Strict providers require `additionalProperties:
/// false` on every object schema — schemars does not emit it (verified by
/// probe 2026-08-31: complete `required` lists and `$defs`/`$ref`, both
/// strict-supported, but no `additionalProperties` and `format`/
/// `$schema` annotations strict providers may reject). Deterministic —
/// the same schema always normalizes identically.
fn strict_schema(schema: &str) -> Result<String, PassError> {
    let value: serde_json::Value = serde_json::from_str(schema)
        .map_err(|e| PassError::Prompt(format!("schema is not valid JSON: {e}")))?;
    let normalized = normalize_schema_object(value);
    serde_json::to_string(&normalized)
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))
}

/// The recursive normalizer: strip strict-incompatible annotations
/// (`format`, root `$schema`), inject `additionalProperties: false` on
/// every object schema, and walk `items`, `properties`, and `$defs`.
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

/// Maximum words per single paragraph-pass call. Models degrade on very
/// long single-shot selections: at 14,266 words one call produced 41
/// breaks with a 10,434-word cliff — structurally valid, semantically
/// broken (observed 2026-10-25, the Dunning long-form sample). Windows
/// keep each call in the range the pass handles densely; the model sees
/// window-local word indices and the returned breaks are offset back to
/// transcript space, so short transcripts take the unchanged single call.
const PARAGRAPH_PASS_WINDOW_WORDS: usize = 2_000;

/// Run the paragraph pass: the model identifies paragraph boundaries as
/// word indices. Lowest-risk layer — no speaker inference, no text
/// mutation.
pub async fn run_paragraph_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    mode: PassMode,
    model_override: Option<&str>,
) -> Result<ParagraphLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let schema = serde_json::to_string(&schemars::schema_for!(ParagraphPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let mut breaks_after = Vec::new();
    let mut model = String::new();
    for (window_index, window) in bundle.words.chunks(PARAGRAPH_PASS_WINDOW_WORDS).enumerate() {
        let offset = window_index * PARAGRAPH_PASS_WINDOW_WORDS;
        let (output, window_model) = run_pass_core::<ParagraphPassOutput>(
            inference,
            template_env,
            PARAGRAPH_PASS_TEMPLATE,
            &schema,
            None,
            window,
            mode,
            model_override,
            &PARAGRAPH_PASS_ATTEMPTS,
            &PARAGRAPH_PASS_REJECTIONS,
            "ParagraphPassOutput {\"breaks_after\": [usize]}",
        )
        .await?;
        model = window_model;
        breaks_after.extend(output.breaks_after.iter().map(|local| local + offset));
    }
    // Windows return disjoint index ranges; sort + dedupe keeps the merged
    // layer canonical regardless of per-window ordering.
    breaks_after.sort_unstable();
    breaks_after.dedup();
    let layer = ParagraphLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: PARAGRAPH_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        breaks_after,
    };
    validate_or_reject(
        &TranscriptLayer::Paragraph(layer.clone()),
        bundle.words.len(),
        &PARAGRAPH_PASS_REJECTIONS,
        mode,
    )?;
    Ok(layer)
}

/// Run the speaker pass (text-cue attribution, v1): the model infers
/// speaker turns from textual cues over indexed words. Approximate by
/// nature — the confidence field carries that honestly, and the layer's
/// provenance names the producing template.
pub async fn run_speaker_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    mode: PassMode,
    model_override: Option<&str>,
) -> Result<SpeakerLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let schema = serde_json::to_string(&schemars::schema_for!(SpeakerPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let (output, model) = run_pass_core::<SpeakerPassOutput>(
        inference,
        template_env,
        SPEAKER_PASS_TEMPLATE,
        &schema,
        None,
        &bundle.words,
        mode,
        model_override,
        &SPEAKER_PASS_ATTEMPTS,
        &SPEAKER_PASS_REJECTIONS,
        "SpeakerPassOutput {\"spans\": [{start_word, end_word, speaker, confidence}]}",
    )
    .await?;
    let layer = SpeakerLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: SPEAKER_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        spans: output.spans,
    };
    validate_or_reject(
        &TranscriptLayer::Speaker(layer.clone()),
        bundle.words.len(),
        &SPEAKER_PASS_REJECTIONS,
        mode,
    )?;
    Ok(layer)
}

/// The locality boundary for lexical speaker corrections (plan §7 Phase
/// 1b — the DiarizationLM boundary, plan ref [3]): text-only corrections
/// of contiguous disagreement runs longer than this measurably corrupt
/// multi-speaker attribution (long-monologue speaker collapse), so the
/// gate reverts them to the existing labels.
pub const SPEAKER_CORRECTION_MAX_LOCALITY_WORDS: usize = 5;

/// The gate's accounting, surfaced with every correction result —
/// refusals are never silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SpeakerCorrectionGate {
    pub max_locality_words: usize,
    /// Disagreement runs at or below the locality bound — accepted.
    pub runs_accepted: usize,
    /// Disagreement runs above the locality bound — reverted to the
    /// existing labels.
    pub runs_reverted: usize,
    pub words_accepted: usize,
    pub words_reverted: usize,
}

impl SpeakerCorrectionGate {
    /// The surfaced refusal when any run was reverted — named, never a
    /// silent trim.
    pub fn reverted_note(&self) -> Option<String> {
        (self.runs_reverted > 0).then(|| {
            format!(
                "{} proposed change(s) spanned more than {} consecutive \
                 words and were reverted to the existing labels — the \
                 locality gate: text alone cannot resolve which speaker \
                 holds a long monologue",
                self.runs_reverted, self.max_locality_words
            )
        })
    }
}

/// Expand validated spans to per-word labels; the clamp is insurance
/// against a mismatched words_count, never a correctness path (stored
/// layers are in-bounds and disjoint).
fn spans_to_word_labels(words_count: usize, spans: &[SpeakerSpan]) -> Vec<Option<(String, f64)>> {
    let mut labels = vec![None; words_count];
    for span in spans {
        for word in span.start_word..=span.end_word.min(words_count.saturating_sub(1)) {
            labels[word] = Some((span.speaker.clone(), span.confidence));
        }
    }
    labels
}

/// Enforce the locality gate over proposed speaker corrections: expand
/// the existing and proposed spans to per-word labels, diff into maximal
/// disagreement runs, accept each run at or below the bound, revert
/// longer runs to the existing labels, and rebuild disjoint spans from
/// the merged per-word labels (a rebuilt span carries the minimum
/// confidence of the words it covers — the span's claim is its weakest
/// word). Pure and deterministic: the model proposes, this decides.
pub fn gate_speaker_corrections(
    words_count: usize,
    existing: &[SpeakerSpan],
    proposed: &[SpeakerSpan],
) -> (Vec<SpeakerSpan>, SpeakerCorrectionGate) {
    let existing_words = spans_to_word_labels(words_count, existing);
    let proposed_words = spans_to_word_labels(words_count, proposed);
    let mut take_proposed = vec![false; words_count];
    let mut gate = SpeakerCorrectionGate {
        max_locality_words: SPEAKER_CORRECTION_MAX_LOCALITY_WORDS,
        runs_accepted: 0,
        runs_reverted: 0,
        words_accepted: 0,
        words_reverted: 0,
    };
    // Maximal disagreement runs: a run opens at the first word whose
    // speaker label differs between the two views and closes at the
    // first word that agrees again (the sentinel iteration at
    // `words_count` closes a trailing run).
    let mut run_start = 0usize;
    let mut in_run = false;
    for word in 0..=words_count {
        let differs = word < words_count
            && existing_words[word]
                .as_ref()
                .map(|(label, _)| label.as_str())
                != proposed_words[word]
                    .as_ref()
                    .map(|(label, _)| label.as_str());
        if differs {
            if !in_run {
                run_start = word;
                in_run = true;
            }
        } else if in_run {
            in_run = false;
            let run_len = word - run_start;
            if run_len <= gate.max_locality_words {
                gate.runs_accepted += 1;
                gate.words_accepted += run_len;
                for accepted in &mut take_proposed[run_start..word] {
                    *accepted = true;
                }
            } else {
                gate.runs_reverted += 1;
                gate.words_reverted += run_len;
            }
        }
    }
    let merged: Vec<Option<(String, f64)>> = (0..words_count)
        .map(|word| {
            if take_proposed[word] {
                proposed_words[word].clone()
            } else {
                existing_words[word].clone()
            }
        })
        .collect();
    let mut spans: Vec<SpeakerSpan> = Vec::new();
    let mut index = 0usize;
    while index < words_count {
        let Some((speaker, confidence)) = merged[index].clone() else {
            index += 1;
            continue;
        };
        let start = index;
        let mut min_confidence = confidence;
        index += 1;
        while index < words_count
            && merged[index]
                .as_ref()
                .is_some_and(|(next, _)| next == &speaker)
        {
            if let Some((_, next_confidence)) = merged[index].as_ref() {
                min_confidence = min_confidence.min(*next_confidence);
            }
            index += 1;
        }
        spans.push(SpeakerSpan {
            start_word: start,
            end_word: index - 1,
            speaker,
            confidence: min_confidence,
        });
    }
    (spans, gate)
}

/// Render the current attribution for the correction prompt: one line
/// per span, the form the model re-reads.
fn current_attribution_request(spans: &[SpeakerSpan]) -> String {
    spans
        .iter()
        .map(|span| {
            format!(
                "{}..={}: {} (confidence {:.2})",
                span.start_word, span.end_word, span.speaker, span.confidence
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run the speaker correction pass (the gated lexical post-pass, plan
/// §7 Phase 1b): the model re-reads the transcript and the CURRENT
/// attribution, then proposes corrections. Locality is never trusted to
/// the model — the caller applies [`gate_speaker_corrections`] before
/// storing.
pub async fn run_speaker_correction_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    existing: &SpeakerLayer,
    mode: PassMode,
    model_override: Option<&str>,
) -> Result<SpeakerLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let schema = serde_json::to_string(&schemars::schema_for!(SpeakerPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let current = current_attribution_request(&existing.spans);
    let (output, model) = run_pass_core::<SpeakerPassOutput>(
        inference,
        template_env,
        SPEAKER_CORRECTION_PASS_TEMPLATE,
        &schema,
        Some(&current),
        &bundle.words,
        mode,
        model_override,
        &SPEAKER_CORRECTION_PASS_ATTEMPTS,
        &SPEAKER_CORRECTION_PASS_REJECTIONS,
        "SpeakerPassOutput {\"spans\": [{start_word, end_word, speaker, confidence}]}",
    )
    .await?;
    let layer = SpeakerLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: SPEAKER_CORRECTION_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        spans: output.spans,
    };
    validate_or_reject(
        &TranscriptLayer::Speaker(layer.clone()),
        bundle.words.len(),
        &SPEAKER_CORRECTION_PASS_REJECTIONS,
        mode,
    )?;
    Ok(layer)
}

/// Run the speaker pass over the audio itself (the scaffold's primary
/// source, decisions 3/7): an audio-capable model hears the recording AND
/// reads the indexed transcript, then attributes speaker turns as word
/// spans. The model cannot know the word indices from audio alone — the
/// indexed transcript in the prompt is what makes the output anchorable.
/// Routes through `media_generate("chat_audio", …)` (child-local provider
/// keys, the OpenAI `input_audio` content-part format) — not the IPC
/// bridge, whose request surface has no audio parts.
pub async fn run_speaker_pass_audio(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    model_override: Option<&str>,
) -> Result<SpeakerLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let resolved_model = model_override
        .map(str::to_string)
        .or_else(crate::models::pass_model)
        .or_else(crate::models::vision_model)
        .filter(|model| !model.trim().is_empty())
        .ok_or(PassError::NotConfigured {
            kind: "speaker-audio pass",
            env: "HKASK_MEDIA_PASS_MODEL or HKASK_MEDIA_VISION_MODEL (the configured analysis model)",
        })?;
    let schema = serde_json::to_string(&schemars::schema_for!(SpeakerPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let prompt = render_pass_prompt(
        template_env,
        SPEAKER_AUDIO_PASS_TEMPLATE,
        &schema,
        None,
        &bundle.words,
    )?;

    let params = hkask_types::MediaGenerateParams {
        prompt: Some(prompt),
        audio_url: Some(bundle.audio_path.clone()),
        model: Some(resolved_model.clone()),
        ..Default::default()
    };
    let raw = inference
        .media_generate("chat_audio", &params)
        .await
        .map_err(PassError::Inference)?;
    SPEAKER_PASS_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

    let model_text = raw
        .get("text")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            SPEAKER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
            PassError::UnparseableOutput {
                expected: "chat_audio response {\"text\": …}",
                raw: truncate(&raw.to_string(), 200),
            }
        })?;
    let extracted = hkask_types::json_extract::extract_json_from_response(model_text);
    let output: SpeakerPassOutput = serde_json::from_str(&extracted).map_err(|_| {
        SPEAKER_PASS_REJECTIONS.fetch_add(1, Ordering::Relaxed);
        PassError::UnparseableOutput {
            expected: "SpeakerPassOutput {\"spans\": [...]}",
            raw: truncate(&extracted, 200),
        }
    })?;

    let layer = SpeakerLayer {
        provenance: LayerProvenance {
            model: resolved_model,
            prompt_template: SPEAKER_AUDIO_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        spans: output.spans,
    };
    validate_or_reject(
        &TranscriptLayer::Speaker(layer.clone()),
        bundle.words.len(),
        &SPEAKER_PASS_REJECTIONS,
        // The audio path is prompt-schema by definition (the structured
        // mode applies to the text passes; the tool rejects the combo).
        PassMode::PromptSchema,
    )?;
    Ok(layer)
}

/// Run the correction pass: the model proposes text replacements over
/// word ranges (mishearings, homophones, garbled fragments). Edits are
/// proposals — applying them (via `corrected_text_view`) produces a
/// derived text view while `words` stays immutable.
pub async fn run_correction_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    mode: PassMode,
    model_override: Option<&str>,
) -> Result<CorrectionLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let schema = serde_json::to_string(&schemars::schema_for!(CorrectionPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let (output, model) = run_pass_core::<CorrectionPassOutput>(
        inference,
        template_env,
        CORRECTION_PASS_TEMPLATE,
        &schema,
        None,
        &bundle.words,
        mode,
        model_override,
        &CORRECTION_PASS_ATTEMPTS,
        &CORRECTION_PASS_REJECTIONS,
        "CorrectionPassOutput {\"edits\": [{start_word, end_word, replacement, reason}]}",
    )
    .await?;
    let layer = CorrectionLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: CORRECTION_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        edits: output.edits,
    };
    validate_or_reject(
        &TranscriptLayer::Correction(layer.clone()),
        bundle.words.len(),
        &CORRECTION_PASS_REJECTIONS,
        mode,
    )?;
    Ok(layer)
}

/// Run the highlight pass — the semantic selection: a natural-language
/// request ("find where he explains the Cinderella curve") resolved to
/// word ranges with labels. This is the agent-as-selection-engine
/// improvement target: Reduct's paradigm with the reading automated.
pub async fn run_highlight_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    request: &str,
    mode: PassMode,
    model_override: Option<&str>,
) -> Result<HighlightLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    if request.trim().is_empty() {
        return Err(PassError::Prompt(
            "selection request must not be empty".to_string(),
        ));
    }
    let schema = serde_json::to_string(&schemars::schema_for!(HighlightPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let (output, model) = run_pass_core::<HighlightPassOutput>(
        inference,
        template_env,
        HIGHLIGHT_PASS_TEMPLATE,
        &schema,
        Some(request),
        &bundle.words,
        mode,
        model_override,
        &HIGHLIGHT_PASS_ATTEMPTS,
        &HIGHLIGHT_PASS_REJECTIONS,
        "HighlightPassOutput {\"highlights\": [{start_word, end_word, label, note}]}",
    )
    .await?;
    let layer = HighlightLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: HIGHLIGHT_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        highlights: output.highlights,
    };
    validate_or_reject(
        &TranscriptLayer::Highlight(layer.clone()),
        bundle.words.len(),
        &HIGHLIGHT_PASS_REJECTIONS,
        mode,
    )?;
    Ok(layer)
}

/// The indexed-words rendering the model reads: `0:The 1:quick 2:brown`.
/// Index legibility is the load-bearing property — the model cites these
/// indices and the deterministic layer owns the only index→time mapping.
pub fn indexed_words_line(words: &[TimedWord]) -> String {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| format!("{index}:{}", word.word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Char-boundary-safe truncation for error payloads (model output can be
/// arbitrarily long; the named error must not be).
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut truncated: String = text.chars().take(max_chars).collect();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pass-prompt fixture: a compiled template environment plus the
    /// two-word timing-aligned transcript (alpha 0-500, beta 500-900) the
    /// prompt-embedding tests share. Timings stay exact — not the
    /// test_support 1-second-boundary fixture.
    fn prompt_env_and_words() -> (minijinja::Environment<'static>, Vec<TimedWord>) {
        let env = crate::templates::create_env().expect("media templates must compile");
        let words = vec![
            TimedWord {
                word: "alpha".to_string(),
                start_ms: 0,
                end_ms: 500,
                confidence: None,
            },
            TimedWord {
                word: "beta".to_string(),
                start_ms: 500,
                end_ms: 900,
                confidence: None,
            },
        ];
        (env, words)
    }

    #[test]
    fn indexed_words_line_formats_indices() {
        let words = vec![
            TimedWord {
                word: "The".to_string(),
                start_ms: 0,
                end_ms: 100,
                confidence: None,
            },
            TimedWord {
                word: "quick".to_string(),
                start_ms: 100,
                end_ms: 300,
                confidence: None,
            },
        ];
        assert_eq!(indexed_words_line(&words), "0:The 1:quick");
    }

    #[test]
    fn every_pass_prompt_embeds_schema_and_indexed_words() {
        let (env, words) = prompt_env_and_words();
        for (template, schema_marker) in [
            (PARAGRAPH_PASS_TEMPLATE, "breaks_after"),
            (SPEAKER_PASS_TEMPLATE, "spans"),
            (SPEAKER_AUDIO_PASS_TEMPLATE, "spans"),
            (CORRECTION_PASS_TEMPLATE, "edits"),
            (HIGHLIGHT_PASS_TEMPLATE, "highlights"),
        ] {
            let prompt = render_pass_prompt(&env, template, "{\"type\": \"object\"}", None, &words)
                .expect("prompt renders");
            assert!(
                prompt.contains("0:alpha 1:beta"),
                "{template}: indexed words present"
            );
            assert!(
                prompt.contains(schema_marker),
                "{template}: schema embedded in prompt"
            );
        }
    }

    #[test]
    fn highlight_prompt_embeds_the_selection_request() {
        let (env, words) = prompt_env_and_words();
        let prompt = render_pass_prompt(
            &env,
            HIGHLIGHT_PASS_TEMPLATE,
            "{\"type\": \"object\"}",
            Some("where he explains the curve"),
            &words,
        )
        .expect("prompt renders");
        assert!(
            prompt.contains("where he explains the curve"),
            "the selection request must reach the model"
        );
        assert!(prompt.contains("0:alpha 1:beta"));
    }

    #[test]
    fn pass_stats_shape() {
        for stats in [
            paragraph_pass_stats(),
            speaker_pass_stats(),
            correction_pass_stats(),
            highlight_pass_stats(),
        ] {
            assert!(stats.get("attempts").is_some());
            assert!(stats.get("rejections").is_some());
            assert!(stats.get("rejection_rate").is_some());
            assert!(stats.get("structured").is_some(), "the A/B measurement");
        }
    }

    fn gate_span(
        start_word: usize,
        end_word: usize,
        speaker: &str,
        confidence: f64,
    ) -> SpeakerSpan {
        SpeakerSpan {
            start_word,
            end_word,
            speaker: speaker.to_string(),
            confidence,
        }
    }

    /// dcterms:identifier: `gate_speaker_corrections`
    /// expect: A short disagreement run (≤ the locality bound) is
    /// accepted and a long one reverted — the DiarizationLM boundary
    /// enforced deterministically, with the refusal counted and named.
    #[test]
    fn gate_accepts_short_runs_and_reverts_long_runs() {
        let existing = vec![gate_span(0, 11, "speaker-1", 0.9)];
        let proposed = vec![
            gate_span(0, 0, "speaker-2", 0.8),
            gate_span(1, 1, "speaker-1", 0.95),
            gate_span(2, 11, "speaker-2", 0.7),
        ];
        let (spans, gate) = gate_speaker_corrections(12, &existing, &proposed);
        assert_eq!(
            gate,
            SpeakerCorrectionGate {
                max_locality_words: 5,
                runs_accepted: 1,
                runs_reverted: 1,
                words_accepted: 1,
                words_reverted: 10,
            }
        );
        // Word 0 takes the proposed label; the long rewrite reverts to
        // the existing labels.
        assert_eq!(
            spans,
            vec![
                gate_span(0, 0, "speaker-2", 0.8),
                gate_span(1, 11, "speaker-1", 0.9),
            ]
        );
        assert!(
            gate.reverted_note()
                .is_some_and(|note| note.contains("reverted"))
        );
    }

    #[test]
    fn gate_boundary_run_of_exactly_five_is_accepted() {
        let existing = vec![gate_span(0, 11, "speaker-1", 0.9)];
        let proposed = vec![
            gate_span(0, 4, "speaker-2", 0.8),
            gate_span(5, 11, "speaker-1", 0.9),
        ];
        let (spans, gate) = gate_speaker_corrections(12, &existing, &proposed);
        assert_eq!(gate.runs_accepted, 1);
        assert_eq!(gate.runs_reverted, 0);
        assert_eq!(
            spans,
            vec![
                gate_span(0, 4, "speaker-2", 0.8),
                gate_span(5, 11, "speaker-1", 0.9),
            ]
        );
        assert_eq!(gate.reverted_note(), None);
    }

    #[test]
    fn gate_coverage_changes_are_locality_gated_too() {
        // Proposed drops words 2..=4 entirely: a coverage disagreement,
        // short enough to accept — the rebuilt spans leave the uncovered
        // gap unattributed (gaps are legal; review flags surface them).
        let existing = vec![gate_span(0, 4, "speaker-1", 0.9)];
        let proposed = vec![gate_span(0, 1, "speaker-1", 0.9)];
        let (spans, gate) = gate_speaker_corrections(5, &existing, &proposed);
        assert_eq!(gate.runs_accepted, 1);
        assert_eq!(gate.words_accepted, 3);
        assert_eq!(spans, vec![gate_span(0, 1, "speaker-1", 0.9)]);
    }

    #[test]
    fn gate_rebuilt_spans_take_the_weakest_word_confidence() {
        // An accepted run joining proposed words of differing confidence
        // yields one span carrying the minimum — the span's claim is its
        // weakest word.
        let existing = vec![gate_span(0, 2, "speaker-1", 0.9)];
        let proposed = vec![
            gate_span(0, 0, "speaker-2", 0.8),
            gate_span(1, 2, "speaker-2", 0.6),
        ];
        let (spans, gate) = gate_speaker_corrections(3, &existing, &proposed);
        assert_eq!(gate.runs_accepted, 1);
        assert_eq!(spans, vec![gate_span(0, 2, "speaker-2", 0.6)]);
    }

    #[test]
    fn gate_with_no_disagreement_returns_existing_labels() {
        let existing = vec![gate_span(0, 2, "speaker-1", 0.9)];
        let (spans, gate) = gate_speaker_corrections(3, &existing, &existing);
        assert_eq!(spans, existing);
        assert_eq!(gate.runs_accepted + gate.runs_reverted, 0);
    }

    #[test]
    fn gate_with_nothing_at_all_is_empty_and_zeroed() {
        let (spans, gate) = gate_speaker_corrections(0, &[], &[]);
        assert!(spans.is_empty());
        assert_eq!(
            gate,
            SpeakerCorrectionGate {
                max_locality_words: 5,
                runs_accepted: 0,
                runs_reverted: 0,
                words_accepted: 0,
                words_reverted: 0,
            }
        );
    }

    #[test]
    fn strict_schema_normalizes_schemars_output_for_strict_mode() {
        // SpeakerPassOutput is the $defs case (nested SpeakerSpan); the
        // probe (2026-08-31) showed schemars emits complete `required`
        // lists and `$defs`/`$ref` (both strict-supported) but NO
        // `additionalProperties` and `format`/`$schema` annotations.
        let raw =
            serde_json::to_string(&schemars::schema_for!(SpeakerPassOutput)).expect("serializes");
        let normalized: serde_json::Value =
            serde_json::from_str(&strict_schema(&raw).expect("normalizes"))
                .expect("normalized schema is valid JSON");

        // Strict-mode requirements now hold at the root…
        assert_eq!(
            normalized["additionalProperties"],
            serde_json::json!(false),
            "root object gets additionalProperties: false"
        );
        assert!(normalized.get("$schema").is_none(), "$schema stripped");
        // …and inside $defs (the nested type).
        let defs = normalized["$defs"].as_object().expect("$defs preserved");
        let span = defs.get("SpeakerSpan").expect("nested type preserved");
        assert_eq!(
            span["additionalProperties"],
            serde_json::json!(false),
            "$defs entries get additionalProperties: false"
        );
        // The `format` annotations schemars adds to properties are
        // stripped (strict providers may reject unsupported keywords).
        let start_word = span["properties"]["start_word"]
            .as_object()
            .expect("property preserved");
        assert!(start_word.get("format").is_none(), "format stripped");
        // Required completeness is preserved untouched.
        assert_eq!(normalized["required"], serde_json::json!(["spans"]));
        assert_eq!(
            span["required"],
            serde_json::json!(["start_word", "end_word", "speaker", "confidence"])
        );
    }
}
