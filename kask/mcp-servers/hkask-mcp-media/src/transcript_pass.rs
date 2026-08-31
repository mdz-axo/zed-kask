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
    CorrectionEdit, CorrectionLayer, LayerProvenance, LayerValidationError, ParagraphLayer,
    SpeakerLayer, SpeakerSpan, TranscriptLayer,
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
pub const CORRECTION_PASS_TEMPLATE: &str = "educt_correction_pass";

static PARAGRAPH_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PARAGRAPH_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static SPEAKER_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static CORRECTION_PASS_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static CORRECTION_PASS_REJECTIONS: AtomicU64 = AtomicU64::new(0);

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
}

fn pass_stats_json(attempts: &AtomicU64, rejections: &AtomicU64) -> serde_json::Value {
    let attempts = attempts.load(Ordering::Relaxed);
    let rejections = rejections.load(Ordering::Relaxed);
    serde_json::json!({
        "attempts": attempts,
        "rejections": rejections,
        "rejection_rate": if attempts == 0 {
            0.0
        } else {
            (rejections as f64) / (attempts as f64)
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

/// The accumulated correction-pass measurement.
pub fn correction_pass_stats() -> serde_json::Value {
    pass_stats_json(&CORRECTION_PASS_ATTEMPTS, &CORRECTION_PASS_REJECTIONS)
}

/// The shared v1 pass pipeline core: render (schema + indexed words) →
/// model → extract → typed parse. The caller wraps the typed output in
/// its layer, validates, and stores. Counters are per-pass.
async fn run_pass_core<TOutput: serde::de::DeserializeOwned>(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    template_name: &str,
    schema: &str,
    words: &[TimedWord],
    model_override: Option<&str>,
    attempts: &AtomicU64,
    rejections: &AtomicU64,
    expected: &'static str,
) -> Result<(TOutput, String), PassError> {
    let resolved_model = model_override.map(str::to_string).unwrap_or_else(|| {
        hkask_inference::model_constants::resolve(
            crate::models::PASS_ENV,
            crate::models::PASS_DEFAULT,
        )
    });

    let prompt = render_pass_prompt(template_env, template_name, schema, words)?;
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

    let extracted = hkask_types::json_extract::extract_json_from_response(&result.text);
    let output: TOutput = serde_json::from_str(&extracted).map_err(|_| {
        rejections.fetch_add(1, Ordering::Relaxed);
        PassError::UnparseableOutput {
            expected,
            raw: truncate(&extracted, 200),
        }
    })?;
    Ok((output, resolved_model))
}

/// Render a pass prompt: the indexed words plus the pass's schema. All
/// pass templates share the four variables.
fn render_pass_prompt(
    template_env: &Environment<'static>,
    template_name: &str,
    schema: &str,
    words: &[TimedWord],
) -> Result<String, PassError> {
    let indexed_words = indexed_words_line(words);
    let words_count = words.len().to_string();
    let last_word_index = words.len().saturating_sub(1).to_string();
    let mut vars: HashMap<&str, &str> = HashMap::new();
    vars.insert("schema", schema);
    vars.insert("words", indexed_words.as_str());
    vars.insert("words_count", words_count.as_str());
    vars.insert("last_word_index", last_word_index.as_str());
    crate::templates::render(template_env, template_name, &vars)
        .map_err(|e| PassError::Prompt(format!("{e}")))
}

/// Validate a produced layer against the transcript's word count; a
/// failure increments the pass's rejection counter before surfacing.
fn validate_or_reject(
    layer: &TranscriptLayer,
    words_count: usize,
    rejections: &AtomicU64,
) -> Result<(), PassError> {
    if let Err(error) = layer.validate(words_count) {
        rejections.fetch_add(1, Ordering::Relaxed);
        return Err(PassError::Validation(error));
    }
    Ok(())
}

/// Run the paragraph pass: the model identifies paragraph boundaries as
/// word indices. Lowest-risk layer — no speaker inference, no text
/// mutation.
pub async fn run_paragraph_pass(
    inference: &Arc<dyn InferencePort>,
    template_env: &Environment<'static>,
    bundle: &TranscriptBundle,
    model_override: Option<&str>,
) -> Result<ParagraphLayer, PassError> {
    if bundle.words.is_empty() {
        return Err(PassError::NoWordTimings);
    }
    let schema = serde_json::to_string(&schemars::schema_for!(ParagraphPassOutput))
        .map_err(|e| PassError::Prompt(format!("schema serialization: {e}")))?;
    let (output, model) = run_pass_core::<ParagraphPassOutput>(
        inference,
        template_env,
        PARAGRAPH_PASS_TEMPLATE,
        &schema,
        &bundle.words,
        model_override,
        &PARAGRAPH_PASS_ATTEMPTS,
        &PARAGRAPH_PASS_REJECTIONS,
        "ParagraphPassOutput {\"breaks_after\": [usize]}",
    )
    .await?;
    let layer = ParagraphLayer {
        provenance: LayerProvenance {
            model,
            prompt_template: PARAGRAPH_PASS_TEMPLATE.to_string(),
            created_at: hkask_types::time::now_rfc3339(),
        },
        breaks_after: output.breaks_after,
    };
    validate_or_reject(
        &TranscriptLayer::Paragraph(layer.clone()),
        bundle.words.len(),
        &PARAGRAPH_PASS_REJECTIONS,
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
        &bundle.words,
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
        &bundle.words,
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
        for (template, schema_marker) in [
            (PARAGRAPH_PASS_TEMPLATE, "breaks_after"),
            (SPEAKER_PASS_TEMPLATE, "spans"),
            (CORRECTION_PASS_TEMPLATE, "edits"),
        ] {
            let prompt = render_pass_prompt(&env, template, "{\"type\": \"object\"}", &words)
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
    fn pass_stats_shape() {
        for stats in [
            paragraph_pass_stats(),
            speaker_pass_stats(),
            correction_pass_stats(),
        ] {
            assert!(stats.get("attempts").is_some());
            assert!(stats.get("rejections").is_some());
            assert!(stats.get("rejection_rate").is_some());
        }
    }
}
