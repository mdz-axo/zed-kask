//! Educt transcript-store tools — persist and recall transcripts + layers.
//!
//! Local implementation of the transcript-as-timeline interaction model:
//! transcripts stop
//! being per-call artifacts and become queryable objects keyed to their media
//! path and optional gallery Asset. Persistence and selection are local; pass
//! tools use configured inference providers, never a hidden cloud path.

use crate::transcript::{TimedWord, TranscriptBundle};
use crate::transcript_layers::{EdlLayer, HighlightEntry, LayerProvenance, TranscriptLayer};
use crate::transcript_pass::{self, PassError, PassMode};
use crate::transcript_select::{
    Edl, EdlEntry, EdlOp, WordRange, edl_to_clip_plan, text_to_word_ranges, union_ranges,
    word_range_to_time_range,
};
use crate::transcript_store::{self, TranscriptFilter, TranscriptStoreError};
use crate::types::{
    EductApplyCorrectionsRequest, EductCorrectionPassRequest, EductDeleteTranscriptRequest,
    EductEdlFromHighlightsRequest, EductExportRequest, EductGetTranscriptRequest,
    EductHighlightPassRequest, EductListLayersRequest, EductLocateRequest,
    EductParagraphPassRequest, EductRealignTranscriptRequest, EductRenderEdlRequest,
    EductSpeakerPassRequest, EductStoreLayerRequest, EductStoreTranscriptRequest,
};
use crate::*;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EductAnnotationCreateRequest {
    pub transcript_id: String,
    /// Inclusive immutable source-word indices, not corrected-text indices.
    pub start_word: usize,
    pub end_word: usize,
    pub label: String,
    pub note: String,
    /// Annotation kind: highlight (default — a labeled selection) or strike
    /// (an unannotated cut over the range; label and note must be blank).
    pub kind: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EductAnnotationListRequest {
    pub transcript_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EductAnnotationUpdateRequest {
    pub transcript_id: String,
    pub annotation_id: String,
    pub start_word: usize,
    pub end_word: usize,
    pub label: String,
    pub note: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EductAnnotationDeleteRequest {
    pub transcript_id: String,
    pub annotation_id: String,
}

fn researcher_highlight(
    start_word: usize,
    end_word: usize,
    label: String,
    note: String,
) -> Result<HighlightEntry, McpToolError> {
    if label.trim().is_empty() || label.len() > 256 || note.len() > 4096 {
        return Err(McpToolError::invalid_argument(
            "label must be nonblank and at most 256 bytes; note at most 4096 bytes",
        ));
    }
    Ok(HighlightEntry {
        start_word,
        end_word,
        label,
        note,
    })
}

/// Validate and build one researcher strike: a single Cut edit decision over
/// the word range. Strikes carry no label or note — they are unannotated
/// cuts; a labeled selection is a highlight.
fn researcher_strike(
    start_word: usize,
    end_word: usize,
    label: String,
    note: String,
) -> Result<EdlEntry, McpToolError> {
    if !label.trim().is_empty() || !note.trim().is_empty() {
        return Err(McpToolError::invalid_argument(
            "a strike carries no label or note — it is an unannotated cut; use kind highlight to carry a label and note",
        ));
    }
    Ok(EdlEntry {
        range: WordRange::new(start_word, end_word),
        op: EdlOp::Cut,
    })
}

/// The researcher-annotation kind of a stored layer, if it is one: a
/// highlight (user researcher_annotation highlight layer — any entry count,
/// preserving the list's original semantics) or a strike (user
/// researcher_annotation EDL with exactly one Cut op). Multi-op EDLs never
/// masquerade as strikes.
fn researcher_annotation_kind(layer: &TranscriptLayer) -> Option<&'static str> {
    match layer {
        TranscriptLayer::Highlight(layer)
            if layer.provenance.model == crate::transcript_layers::RESEARCHER_ANNOTATION_MODEL
                && layer.provenance.prompt_template
                    == crate::transcript_layers::RESEARCHER_ANNOTATION_TEMPLATE =>
        {
            Some("highlight")
        }
        TranscriptLayer::Edl(layer)
            if layer.provenance.model == crate::transcript_layers::RESEARCHER_ANNOTATION_MODEL
                && layer.provenance.prompt_template
                    == crate::transcript_layers::RESEARCHER_ANNOTATION_TEMPLATE
                && layer.ops.len() == 1
                && layer.ops[0].op == EdlOp::Cut =>
        {
            Some("strike")
        }
        _ => None,
    }
}

/// Read one stored layer by annotation id — the dispatch key update and
/// delete share. The mutate functions re-verify identity against their
/// own fresh read, so the dispatch can never write through the wrong arm;
/// a concurrent same-kind edit landing between this read and the mutate
/// is absorbed by the mutate's own re-read (last writer wins on the word
/// range), and only an edit inside the mutate's read-to-write window
/// fails with the named LayerNotFound.
fn annotation_layer(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    transcript_id: &str,
    annotation_id: &str,
) -> Result<TranscriptLayer, McpToolError> {
    transcript_store::list_layers(driver, transcript_id)
        .map_err(map_store_error)?
        .into_iter()
        .find(|record| record.id == annotation_id)
        .map(|record| record.layer)
        .ok_or_else(|| {
            map_store_error(TranscriptStoreError::LayerNotFound {
                transcript_id: transcript_id.to_string(),
                layer_id: annotation_id.to_string(),
            })
        })
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EductSearchTranscriptsRequest {
    /// Literal substring; ASCII case-insensitive, non-ASCII text is exact.
    pub query: String,
    /// Explicit retrieval method. Only lexical is supported; corpus export is the semantic handoff.
    pub mode: String,
    /// Maximum citations, 1..=100 (default 20).
    pub limit: Option<usize>,
    /// Maximum recordings examined in this page, 1..=500 (default 100).
    pub recording_limit: Option<usize>,
    /// Recording-page offset, default 0. Stable order: newest timestamp, then ID.
    pub recording_offset: Option<usize>,
    /// Resume byte offset in the first recording's working text (default 0).
    /// Use next_start_byte with next_recording_offset from the previous page.
    pub start_byte: Option<usize>,
    pub gallery_asset_id: Option<String>,
}

fn validate_transcript_search(
    request: &EductSearchTranscriptsRequest,
) -> Result<(usize, usize, usize), McpToolError> {
    let limit = request.limit.unwrap_or(20);
    let recordings = request.recording_limit.unwrap_or(100);
    let offset = request.recording_offset.unwrap_or(0);
    if request.mode != "lexical" || request.query.trim().is_empty() || request.query.len() > 256 {
        return Err(McpToolError::invalid_argument(
            "mode must be lexical; query must be nonblank and at most 256 UTF-8 bytes",
        ));
    }
    if !(1..=100).contains(&limit)
        || !(1..=500).contains(&recordings)
        || offset > i64::MAX as usize - 501
    {
        return Err(McpToolError::invalid_argument(
            "limit must be 1..=100, recording_limit 1..=500, recording_offset within SQLite integer range",
        ));
    }
    Ok((limit, recordings, offset))
}

// Safety budget for literal matching, independent of recording count or result
// count. Stored bundles/corrections still load one recording at a time; this is
// a scan-work bound, not a bound on the size of an existing stored recording.
const SEARCH_TEXT_BYTE_BUDGET: usize = 4 * 1024 * 1024;

/// Exact excerpts, not summaries. Only match starts in [start_byte, end_byte)
/// are examined; a phrase crossing the end remains eligible (<=256 byte query).
/// The caller validates UTF-8 boundaries. Word indices are inclusive in the
/// working view, never invented from original words for unaligned corrections.
fn lexical_citations(
    working: &WorkingTranscript,
    query: &str,
    start_byte: usize,
    end_byte: usize,
    limit: usize,
) -> Vec<(usize, serde_json::Value)> {
    let mut hits = Vec::new();
    if query.is_empty() || limit == 0 {
        return hits;
    }
    let mut lookahead_end = end_byte
        .saturating_add(query.len() - 1)
        .min(working.text.len());
    while !working.text.is_char_boundary(lookahead_end) {
        lookahead_end -= 1;
    }
    // ASCII folding preserves UTF-8 byte offsets. Fold only this bounded window
    // and use the standard substring search instead of comparing the full query
    // at every input character. Advancing one character preserves overlaps.
    let haystack = working.text[start_byte..lookahead_end].to_ascii_lowercase();
    let needle = query.to_ascii_lowercase();
    let mut search_byte = 0;
    // Each source word is passed at most once, not again for every citation.
    let mut word_cursor = 0;
    let mut word_byte = 0;
    while let Some(relative_match) = haystack[search_byte..].find(&needle) {
        let start = start_byte + search_byte + relative_match;
        if start >= end_byte {
            break;
        }
        let end = start + query.len();
        let matched = &working.text[start..end];
        search_byte = start - start_byte + matched.chars().next().map_or(1, char::len_utf8);
        let range = working.words.as_ref().and_then(|words| {
            while let Some(word) = words.get(word_cursor) {
                if word_byte + word.word.len() > start {
                    break;
                }
                word_byte += word.word.len() + 1;
                word_cursor += 1;
            }
            let mut offset = word_byte;
            let mut first = None;
            let mut last = None;
            for (index, word) in words.iter().enumerate().skip(word_cursor) {
                if offset >= end {
                    break;
                }
                let word_end = offset + word.word.len();
                if word_end > start && !word.word.is_empty() {
                    first.get_or_insert(index);
                    last = Some(index);
                }
                offset = word_end + 1;
            }
            first
                .zip(last)
                .map(|(first, last)| WordRange::new(first, last))
        });
        let times = range.zip(working.words.as_deref()).map(|(range, words)| {
            (
                words[range.start_word].start_ms,
                words[range.end_word].end_ms,
            )
        });
        hits.push((start, serde_json::json!({
            "matched_text": matched, "start_byte": start, "end_byte": end,
            "word_range": range,
            "time_range_ms": times.map(|(start_ms, end_ms)| serde_json::json!({"start_ms": start_ms, "end_ms": end_ms})),
            "alignment": working.alignment, "alignment_error": working.alignment_error,
            "correction_layer_id": working.correction_layer_id,
            "realignment_layer_id": working.realignment_layer_id,
            "text_view": "working_transcript",
            "word_index_basis": "working_transcript",
        })));
        if hits.len() == limit {
            break;
        }
    }
    hits
}

#[derive(serde::Serialize)]
struct EdlRenderEffectiveParams<'a> {
    transcript_id: &'a str,
    edl_layer_id: &'a str,
    source: &'a str,
    clip_plan: &'a [(f64, f64)],
    clips: usize,
}

#[derive(serde::Serialize)]
struct SrtExportEffectiveParams<'a> {
    transcript_id: &'a str,
    cues: usize,
    correction_layer_id: Option<&'a str>,
}

#[derive(serde::Serialize)]
struct HighlightsExportEffectiveParams<'a> {
    transcript_id: &'a str,
    layer_ids: &'a [String],
    rows: usize,
}

#[derive(serde::Serialize)]
struct CorpusExportEffectiveParams<'a> {
    transcript_id: &'a str,
    words: usize,
    word_timings: bool,
    correction_layer_id: Option<&'a str>,
    correction_alignment: WorkingTranscriptAlignment,
    composition: &'static str,
}

#[derive(serde::Serialize)]
struct ReviewCsvExportEffectiveParams<'a> {
    transcript_id: &'a str,
    paragraph_layer_id: Option<&'a str>,
    speaker_layer_id: Option<&'a str>,
    threshold: f64,
    blocks: usize,
    flagged: usize,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkingTranscriptAlignment {
    Original,
    Aligned,
    ReAnchored,
    Unaligned,
    Untimed,
}

#[derive(serde::Serialize)]
struct WorkingTranscript {
    text: String,
    words: Option<Vec<TimedWord>>,
    alignment: WorkingTranscriptAlignment,
    correction_layer_id: Option<String>,
    /// The realignment layer whose decision the projection honors, when
    /// present (the re-anchored path, MF-2).
    realignment_layer_id: Option<String>,
    alignment_error: Option<String>,
}

impl WorkingTranscript {
    fn require_timed_words(&self, operation: &str) -> Result<&[TimedWord], McpToolError> {
        self.words.as_deref().ok_or_else(|| {
            McpToolError::new(
                hkask_types::McpErrorKind::FailedPrecondition,
                format!(
                    "{operation} requires a timing-aligned working transcript; correction layer {} is unaligned: {}. Re-transcribe the corrected range, use one replacement token per original timed word, or store a realignment layer via educt_realign_transcript",
                    self.correction_layer_id.as_deref().unwrap_or("unknown"),
                    self.alignment_error.as_deref().unwrap_or("unknown alignment error")
                ),
            )
        })
    }
}

/// Load a stored transcript and enforce the word-timings precondition every
/// timed consumer requires: an unknown ID is the caller's named not-found,
/// and an untimed transcript refuses with the caller's consumer-specific
/// message (each tool names what cannot anchor without timings). Returns the
/// bundle — no current caller reads the summary past the guard.
fn load_timed_transcript(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    transcript_id: &str,
    no_timings_message: &str,
) -> Result<TranscriptBundle, McpToolError> {
    let Some((summary, bundle)) =
        transcript_store::load_transcript(driver, transcript_id).map_err(map_store_error)?
    else {
        return Err(McpToolError::not_found(format!(
            "transcript {transcript_id} not found"
        )));
    };
    if !summary.has_word_timings {
        return Err(McpToolError::invalid_argument(no_timings_message));
    }
    Ok(bundle)
}

/// Select a stored layer of `kind` from the transcript's listed layers:
/// newest first (RFC 3339 timestamps sort lexicographically; the ID breaks
/// ties deterministically, so a tied newest group resolves to its newest
/// member, never an arbitrary one), the caller's `layer_id` when provided,
/// else the newest. An unknown ID (or no layer of the kind) is the named
/// not-found. The caller keeps its typed destructure — the kind filter
/// guarantees the variant, so a mismatch there is an invariant break, never
/// a caller error.
fn select_layer_of_kind(
    layers: &[transcript_store::LayerRecord],
    transcript_id: &str,
    kind: &str,
    layer_id: Option<&str>,
) -> Result<transcript_store::LayerRecord, McpToolError> {
    let mut matching: Vec<_> = layers
        .iter()
        .filter(|record| record.layer.kind() == kind)
        .cloned()
        .collect();
    matching.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    let selected = match layer_id {
        Some(id) => matching.into_iter().find(|record| record.id == id),
        None => matching.into_iter().next(),
    };
    selected.ok_or_else(|| {
        McpToolError::not_found(format!(
            "no {kind} layer found for transcript {transcript_id}"
        ))
    })
}

/// Select the transcript's correction layer — newest, or the caller's
/// `layer_id` — and destructure it: the typed selection the correction
/// consumers (`educt_apply_corrections`, `educt_realign_transcript`)
/// share. Returns the storage record (callers read its id) and the cloned
/// correction layer. The kind filter guarantees the variant, so a mismatch
/// is an invariant break, never a caller error.
fn select_correction_layer(
    layers: &[transcript_store::LayerRecord],
    transcript_id: &str,
    layer_id: Option<&str>,
) -> Result<
    (
        transcript_store::LayerRecord,
        crate::transcript_layers::CorrectionLayer,
    ),
    McpToolError,
> {
    let record = select_layer_of_kind(layers, transcript_id, "correction", layer_id)?;
    let TranscriptLayer::Correction(correction) = &record.layer else {
        return Err(McpToolError::internal(
            "layer kind mismatch after correction filter",
        ));
    };
    let correction = correction.clone();
    Ok((record, correction))
}

fn working_transcript(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    transcript_id: &str,
    bundle: &TranscriptBundle,
) -> Result<WorkingTranscript, McpToolError> {
    let layers = transcript_store::list_layers(driver, transcript_id).map_err(map_store_error)?;
    // The alignment state machine is total: a source with no word timings
    // is untimed regardless of any stored layers. The store refuses
    // corrections over untimed sources (NoWordTimings), so this guard is
    // defense-in-depth for total semantics — every timing projection
    // below is meaningless without source timings.
    if bundle.words.is_empty() {
        return Ok(WorkingTranscript {
            text: bundle.full_text.clone(),
            words: None,
            alignment: WorkingTranscriptAlignment::Untimed,
            correction_layer_id: None,
            realignment_layer_id: None,
            alignment_error: Some("source transcript has no word-level timings".to_string()),
        });
    }
    let mut corrections = layers
        .iter()
        .filter(|record| record.layer.kind() == "correction")
        .collect::<Vec<_>>();
    corrections.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let Some(record) = corrections.pop() else {
        return Ok(WorkingTranscript {
            text: crate::transcript_select::rendered_transcript(&bundle.words),
            words: Some(bundle.words.clone()),
            alignment: WorkingTranscriptAlignment::Original,
            correction_layer_id: None,
            realignment_layer_id: None,
            alignment_error: None,
        });
    };
    let TranscriptLayer::Correction(correction) = &record.layer else {
        return Err(McpToolError::internal(
            "layer kind mismatch after correction filter",
        ));
    };
    // The re-anchored path (MF-2): a realignment layer naming THIS
    // correction records the decision to project with interpolated
    // timings. Newest realignment wins (created_at, then id — the same
    // deterministic tie-break as the correction lookup); a realignment
    // naming an older correction is inert here.
    let mut realignments = layers
        .iter()
        .filter(|candidate| {
            matches!(
                &candidate.layer,
                TranscriptLayer::Realignment(layer)
                    if layer.correction_layer_id == record.id
            )
        })
        .collect::<Vec<_>>();
    realignments.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    if let Some(realignment) = realignments.pop() {
        return match crate::transcript_layers::reanchored_corrected_words(
            &bundle.words,
            &correction.edits,
        ) {
            Ok(words) => Ok(WorkingTranscript {
                text: crate::transcript_select::rendered_transcript(&words),
                words: Some(words),
                alignment: WorkingTranscriptAlignment::ReAnchored,
                correction_layer_id: Some(record.id.clone()),
                realignment_layer_id: Some(realignment.id.clone()),
                alignment_error: None,
            }),
            // Unreachable through validated layers (re-anchoring fails
            // only on an out-of-bounds range); surfaced, never swallowed.
            Err(error) => Ok(WorkingTranscript {
                text: crate::transcript_layers::corrected_text_view(
                    &bundle.words,
                    &correction.edits,
                ),
                words: None,
                alignment: WorkingTranscriptAlignment::Unaligned,
                correction_layer_id: Some(record.id.clone()),
                realignment_layer_id: Some(realignment.id.clone()),
                alignment_error: Some(error.to_string()),
            }),
        };
    }
    match crate::transcript_layers::aligned_corrected_words(&bundle.words, &correction.edits) {
        Ok(words) => Ok(WorkingTranscript {
            text: crate::transcript_select::rendered_transcript(&words),
            words: Some(words),
            alignment: WorkingTranscriptAlignment::Aligned,
            correction_layer_id: Some(record.id.clone()),
            realignment_layer_id: None,
            alignment_error: None,
        }),
        Err(error) => Ok(WorkingTranscript {
            text: crate::transcript_layers::corrected_text_view(&bundle.words, &correction.edits),
            words: None,
            alignment: WorkingTranscriptAlignment::Unaligned,
            correction_layer_id: Some(record.id.clone()),
            realignment_layer_id: None,
            alignment_error: Some(error.to_string()),
        }),
    }
}

/// Map store errors to MCP wire-level kinds per-variant (never a blanket
/// internal): a missing transcript is NotFound; a validation failure is
/// InvalidArgument carrying the named invariant; a DB failure is Internal.
pub(crate) fn map_store_error(error: TranscriptStoreError) -> McpToolError {
    match error {
        TranscriptStoreError::TranscriptNotFound { transcript_id } => {
            McpToolError::not_found(format!("transcript {transcript_id} not found"))
        }
        TranscriptStoreError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        TranscriptStoreError::GalleryAssetNotFound { gallery_asset_id } => {
            McpToolError::not_found(format!("gallery asset {gallery_asset_id} not found"))
        }
        TranscriptStoreError::LayerNotFound {
            transcript_id,
            layer_id,
        } => McpToolError::not_found(format!(
            "layer {layer_id} not found for transcript {transcript_id}"
        )),
        TranscriptStoreError::SourceInspection { path, source } => {
            McpToolError::unavailable(format!("inspect transcript source {path}: {source}"))
        }
        TranscriptStoreError::Serialization(message) => McpToolError::internal(message),
        TranscriptStoreError::Db(error) => {
            McpToolError::internal(format!("transcript store: {error}"))
        }
    }
}

/// Map pass errors per-variant: precondition failures are InvalidArgument
/// (named), unparseable model output is Unavailable (retryable with a
/// different model), inference failures classify through the canonical
/// helper, prompt-construction failures are Internal.
fn map_pass_error(error: PassError) -> McpToolError {
    match error {
        PassError::NoWordTimings => McpToolError::invalid_argument(
            "transcript has no word-level timings; the paragraph pass cannot anchor \
             (NoWordTimings)",
        ),
        PassError::UnparseableOutput { expected, raw } => {
            McpToolError::unavailable(format!("model output failed to parse as {expected}: {raw}"))
        }
        PassError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        PassError::Prompt(message) => {
            McpToolError::internal(format!("prompt construction: {message}"))
        }
        PassError::Inference(error) => classify_inference_error("transcript pass failed", error),
        PassError::NotConfigured { kind, env } => McpToolError::permission_denied(format!(
            "no {kind} model configured — set {env} or pass an explicit model; \
             kask never falls back to a hidden code constant"
        )),
    }
}

/// Normalize a loosely-typed (AnyJsonValue) tool input that may arrive
/// stringified: some MCP transports serialize object params into their
/// JSON-string form. Accept both — the parsed object or the string form —
/// so every client can store transcripts and layers.
pub(crate) fn parse_json_value(
    value: serde_json::Value,
    what: &str,
) -> Result<serde_json::Value, McpToolError> {
    match value {
        serde_json::Value::String(text) => serde_json::from_str(&text).map_err(|e| {
            McpToolError::invalid_argument(format!(
                "{what} must be valid JSON (object or JSON-string form): {e}"
            ))
        }),
        other => Ok(other),
    }
}

/// Whether a media path is an audio file (extension check) — selects the
/// audio trim/concat render path over the video path.
fn is_audio_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".wav", ".mp3", ".flac", ".m4a", ".ogg", ".aac", ".wma"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// Store `bundle` as a transcript record and render the summary JSON with
/// the no-word-timings degradation surfaced when timings are absent — the
/// shared tail of `educt_store_transcript` and audio's
/// `transcribe_and_store`.
pub(crate) fn store_transcript_summary_value(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    bundle: &TranscriptBundle,
    gallery_asset_id: Option<&str>,
) -> Result<serde_json::Value, McpToolError> {
    let summary = transcript_store::store_transcript(driver, bundle, gallery_asset_id)
        .map_err(map_store_error)?;
    let mut result = serde_json::to_value(&summary)
        .map_err(|e| McpToolError::internal(format!("serialize summary: {e}")))?;
    if !summary.has_word_timings {
        result["degradation"] = serde_json::json!(
            "no word-level timings — stored for text/segments only; layers \
             cannot anchor (NoWordTimings)"
        );
    }
    Ok(result)
}

#[tool_router(router = educt_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Author a manually supplied researcher annotation over inclusive immutable source-word indices. kind highlight (default) carries a label and note; kind strike is an unannotated cut over the range (label and note must be blank) — strikethrough editing, rendered by educt_render_edl as the transcript minus the struck range. Returns stable annotation_id (the annotation layer's ID). Never changes source bytes. Provenance is researcher_supplied, not authenticated human identity; authorization is the existing host boundary."
    )]
    pub async fn educt_annotation_create(
        &self,
        Parameters(request): Parameters<EductAnnotationCreateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_annotation_create", async {
            let driver = &**self.gallery_store.driver();
            let provenance = LayerProvenance {
                model: crate::transcript_layers::RESEARCHER_ANNOTATION_MODEL.to_string(),
                prompt_template: crate::transcript_layers::RESEARCHER_ANNOTATION_TEMPLATE
                    .to_string(),
                created_at: hkask_types::time::now_rfc3339(),
            };
            let layer = match request.kind.as_deref().unwrap_or("highlight") {
                "highlight" => {
                    let entry = researcher_highlight(request.start_word, request.end_word, request.label, request.note)?;
                    TranscriptLayer::Highlight(crate::transcript_layers::HighlightLayer {
                        provenance,
                        highlights: vec![entry],
                    })
                }
                "strike" => {
                    let entry = researcher_strike(request.start_word, request.end_word, request.label, request.note)?;
                    TranscriptLayer::Edl(crate::transcript_layers::EdlLayer {
                        provenance,
                        ops: vec![entry],
                    })
                }
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "kind must be highlight or strike, got {other:?}"
                    )))
                }
            };
            load_timed_transcript(driver, &request.transcript_id, "research annotations require source word timings")?;
            let record = transcript_store::store_layer(driver, &request.transcript_id, &layer).map_err(map_store_error)?;
            Ok(serde_json::json!({"annotation_id": record.id, "transcript_id": request.transcript_id, "layer": record.layer, "provenance": "researcher_supplied", "word_index_basis": "immutable_source"}))
        }).await
    }

    #[tool(
        description = "List researcher-authored annotations with their original payload and inclusive source-word ranges: highlights (label, note) and strikes (single-Cut edit decisions). Excludes model suggestions and multi-op EDLs; existing transcript layers remain available via educt_list_layers."
    )]
    pub async fn educt_annotation_list(
        &self,
        Parameters(request): Parameters<EductAnnotationListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_annotation_list", async {
            let driver = &**self.gallery_store.driver();
            if transcript_store::load_transcript(driver, &request.transcript_id).map_err(map_store_error)?.is_none() {
                return Err(McpToolError::not_found(format!("transcript {} not found", request.transcript_id)));
            }
            let annotations = transcript_store::list_layers(driver, &request.transcript_id).map_err(map_store_error)?.into_iter()
                .filter(|record| researcher_annotation_kind(&record.layer).is_some())
                .collect::<Vec<_>>();
            Ok(serde_json::json!({"transcript_id": request.transcript_id, "annotations": annotations, "provenance": "researcher_supplied", "word_index_basis": "immutable_source"}))
        }).await
    }

    #[tool(
        description = "Update one researcher annotation by stable annotation_id, dispatching on its stored kind: a highlight replaces its source-word range, label, and note; a strike replaces its source-word range with blank label and note. Identity and provenance are preserved. Cannot edit model suggestions or multi-op EDLs."
    )]
    pub async fn educt_annotation_update(
        &self,
        Parameters(request): Parameters<EductAnnotationUpdateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_annotation_update", async {
            let driver = &**self.gallery_store.driver();
            let stored = annotation_layer(driver, &request.transcript_id, &request.annotation_id)?;
            match researcher_annotation_kind(&stored) {
                Some("highlight") => {
                    let entry = researcher_highlight(request.start_word, request.end_word, request.label, request.note)?;
                    transcript_store::mutate_user_highlight(driver, &request.transcript_id, &request.annotation_id, Some(&entry)).map_err(map_store_error)?;
                    Ok(serde_json::json!({"transcript_id": request.transcript_id, "annotation_id": request.annotation_id, "highlight": entry, "provenance": "researcher_supplied", "word_index_basis": "immutable_source"}))
                }
                Some("strike") => {
                    researcher_strike(request.start_word, request.end_word, request.label, request.note)?;
                    transcript_store::mutate_user_strike(driver, &request.transcript_id, &request.annotation_id, Some((request.start_word, request.end_word))).map_err(map_store_error)?;
                    Ok(serde_json::json!({"transcript_id": request.transcript_id, "annotation_id": request.annotation_id, "strike": {"start_word": request.start_word, "end_word": request.end_word}, "provenance": "researcher_supplied", "word_index_basis": "immutable_source"}))
                }
                _ => Err(map_store_error(TranscriptStoreError::LayerNotFound {
                    transcript_id: request.transcript_id.clone(),
                    layer_id: request.annotation_id.clone(),
                })),
            }
        }).await
    }

    #[tool(
        description = "Delete only the named researcher annotation (highlight or strike), preserving all other annotations, model layers, transcript text, and source media."
    )]
    pub async fn educt_annotation_delete(
        &self,
        Parameters(request): Parameters<EductAnnotationDeleteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_annotation_delete", async {
            let driver = &**self.gallery_store.driver();
            let stored = annotation_layer(driver, &request.transcript_id, &request.annotation_id)?;
            match researcher_annotation_kind(&stored) {
                Some("highlight") => transcript_store::mutate_user_highlight(driver, &request.transcript_id, &request.annotation_id, None),
                Some("strike") => transcript_store::mutate_user_strike(driver, &request.transcript_id, &request.annotation_id, None),
                _ => Err(TranscriptStoreError::LayerNotFound {
                    transcript_id: request.transcript_id.clone(),
                    layer_id: request.annotation_id.clone(),
                }),
            }
            .map_err(map_store_error)?;
            Ok(serde_json::json!({"transcript_id": request.transcript_id, "annotation_id": request.annotation_id, "deleted": true}))
        }).await
    }
    #[tool(
        description = "Search stored recording content in lexical mode: ASCII case-insensitive literal substring (non-ASCII exact), no embeddings. Returns bounded exact working-text excerpts with source IDs and available word/time ranges. Matching is capped at 4 MiB per call; one stored recording's projection may be larger. Resume with next_recording_offset and next_start_byte for all limits. Corpus export remains the semantic handoff."
    )]
    pub async fn educt_search_transcripts(
        &self,
        Parameters(request): Parameters<EductSearchTranscriptsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_search_transcripts", async {
            let (limit, recording_limit, offset) = validate_transcript_search(&request)?;
            let driver = &**self.gallery_store.driver();
            let mut summaries = transcript_store::list_transcripts_page(driver, &TranscriptFilter {
                media_path: None, gallery_asset_id: request.gallery_asset_id.clone(), limit: recording_limit + 1,
            }, offset).map_err(map_store_error)?;
            let more_recordings = summaries.len() > recording_limit;
            summaries.truncate(recording_limit);
            if summaries.is_empty() && request.start_byte.unwrap_or(0) != 0 {
                return Err(McpToolError::invalid_argument("start_byte requires a recording at recording_offset"));
            }
            let mut hits = Vec::new();
            let mut scanned = 0;
            let mut text_bytes_scanned = 0;
            let mut result_limit_reached = false;
            let mut scan_byte_limit_reached = false;
            let mut next_cursor = None;
            for (index, summary) in summaries.iter().enumerate() {
                if text_bytes_scanned == SEARCH_TEXT_BYTE_BUDGET {
                    scan_byte_limit_reached = true;
                    next_cursor = Some((offset + index, 0));
                    break;
                }
                let Some((summary, bundle)) = transcript_store::load_transcript(driver, &summary.id).map_err(map_store_error)? else {
                    return Err(McpToolError::not_found(format!("transcript {} disappeared during search", summary.id)));
                };
                let working = working_transcript(driver, &summary.id, &bundle)?;
                let start = if index == 0 { request.start_byte.unwrap_or(0) } else { 0 };
                if !working.text.is_char_boundary(start) {
                    return Err(McpToolError::invalid_argument("start_byte must be a UTF-8 boundary within the first recording's current working text"));
                }
                let mut end = (start + SEARCH_TEXT_BYTE_BUDGET - text_bytes_scanned).min(working.text.len());
                while !working.text.is_char_boundary(end) { end -= 1; }
                let citations = lexical_citations(&working, &request.query, start, end, limit - hits.len() + 1);
                scanned += 1;
                let gallery_id = if !citations.is_empty() && hits.len() < limit {
                    match summary.gallery_asset_id.as_deref() {
                        Some(id) => Some(self.gallery_store.get_by_id(id).map_err(map_gallery_store_error)?.gallery_id),
                        None => None,
                    }
                } else { None };
                for (match_start, mut citation) in citations {
                    if hits.len() == limit {
                        result_limit_reached = true;
                        // Resume AT the first omitted match, not after its recording.
                        next_cursor = Some((offset + index, match_start));
                        end = match_start;
                        break;
                    }
                    citation["transcript_id"] = serde_json::json!(summary.id);
                    citation["gallery_id"] = serde_json::json!(gallery_id);
                    citation["media_path"] = serde_json::json!(summary.media_path);
                    citation["gallery_asset_id"] = serde_json::json!(summary.gallery_asset_id);
                    citation["source_gallery_asset_id"] = serde_json::json!(summary.source_gallery_asset_id);
                    citation["source_link"] = serde_json::json!(summary.source_link);
                    citation["source_availability"] = serde_json::json!(summary.source_availability);
                    hits.push(citation);
                }
                // Count the covered prefix; one <=256-byte lookahead may also
                // have been compared. No full transcript enters the response.
                text_bytes_scanned += end - start;
                if result_limit_reached { break; }
                if end < working.text.len() {
                    scan_byte_limit_reached = true;
                    next_cursor = Some((offset + index, end));
                    break;
                }
            }
            if next_cursor.is_none() && more_recordings {
                next_cursor = Some((offset + scanned, 0));
            }
            Ok(serde_json::json!({
                "mode": "lexical", "matching": "ascii_case_insensitive_literal_substring",
                "hits": hits, "count": hits.len(), "recordings_scanned": scanned,
                "recording_offset": offset, "has_more_recordings": more_recordings || scanned < summaries.len(),
                "has_more": next_cursor.is_some(),
                "next_recording_offset": next_cursor.map(|(recording, _)| recording),
                "next_start_byte": next_cursor.map(|(_, byte)| byte),
                "text_bytes_scanned": text_bytes_scanned, "scan_byte_limit": SEARCH_TEXT_BYTE_BUDGET,
                "scan_byte_limit_reached": scan_byte_limit_reached,
                "result_limit_reached": result_limit_reached,
                "search_complete": next_cursor.is_none(),
                "coverage_note": "Bounded search from the supplied cursor. Resume with next_recording_offset and next_start_byte; offsets refer to the current recording order and working text, not a frozen snapshot. The byte budget bounds matching, not loading/projection of one stored recording.",
            }))
        }).await
    }

    #[tool(
        description = "Persist a TranscriptBundle (exactly as returned by transcribe_bundle) as a queryable transcript record, keyed to its media path and an optional gallery asset. Returns the transcript summary with its ID. Transcripts without word-level timings are stored with a surfaced degradation note — text and segments remain usable, but layers cannot anchor to them."
    )]
    pub async fn educt_store_transcript(
        &self,
        Parameters(EductStoreTranscriptRequest {
            transcript,
            gallery_asset_id,
        }): Parameters<EductStoreTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_store_transcript", async {
            let value = parse_json_value(transcript.0, "transcript")?;
            let bundle: TranscriptBundle = serde_json::from_value(value).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "transcript must be valid TranscriptBundle JSON (as returned \
                         by transcribe_bundle): {e}"
                ))
            })?;
            let driver = &**self.gallery_store.driver();
            store_transcript_summary_value(driver, &bundle, gallery_asset_id.as_deref())
        })
        .await
    }

    #[tool(
        description = "List stored transcripts, optionally filtered by media path or gallery asset ID, newest first. Each summary carries words_count, has_word_timings, and the five-state alignment (original, aligned, re_anchored, unaligned, untimed) so degradations and annotation capability are visible per record — the alignment is the same working-transcript computation the workspace gate uses, never an approximation."
    )]
    pub async fn educt_list_transcripts(
        &self,
        Parameters(EductListTranscriptsRequest {
            media_path,
            gallery_asset_id,
            limit,
        }): Parameters<EductListTranscriptsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_list_transcripts", async {
            let driver = &**self.gallery_store.driver();
            let filter = TranscriptFilter {
                media_path,
                gallery_asset_id,
                limit: limit.unwrap_or(50).min(500) as usize,
            };
            let summaries =
                transcript_store::list_transcripts(driver, &filter).map_err(map_store_error)?;
            let mut transcripts = Vec::with_capacity(summaries.len());
            for summary in summaries {
                // Enrich each row with its alignment — the same
                // working_transcript computation the workspace gate uses,
                // so the browser's flag can never disagree with the opened
                // view (D5: one projection, never a re-implementation). A
                // transcript deleted between the list read and this load
                // drops out of the listing.
                let Some((_, bundle)) = transcript_store::load_transcript(driver, &summary.id)
                    .map_err(map_store_error)?
                else {
                    tracing::info!(
                        target: "hkask.mcp.media",
                        "transcript {} vanished between list and load; row dropped",
                        summary.id
                    );
                    continue;
                };
                let working = working_transcript(driver, &summary.id, &bundle)?;
                let mut row = serde_json::to_value(&summary)
                    .map_err(|error| McpToolError::internal(format!("summary: {error}")))?;
                row["alignment"] = serde_json::to_value(&working.alignment)
                    .map_err(|error| McpToolError::internal(format!("alignment: {error}")))?;
                transcripts.push(row);
            }
            Ok(serde_json::json!({
                "transcripts": transcripts,
                "count": transcripts.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Get a stored transcript by ID: its summary, immutable source TranscriptBundle, timing-aware working transcript after the latest correction, durable exports/renders, and (by default) its layers."
    )]
    pub async fn educt_get_transcript(
        &self,
        Parameters(EductGetTranscriptRequest {
            transcript_id,
            include_layers,
        }): Parameters<EductGetTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_get_transcript", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            let working = working_transcript(driver, &transcript_id, &bundle)?;
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            // Derived review view (task 1a): paragraph blocks scored by the
            // latest speaker layer's stored attribution confidences. A
            // missing layer or uncovered block is named in the view —
            // never a fabricated green.
            let review = crate::transcript_review::derive_review_flags(bundle.words.len(), &layers);
            let mut result = serde_json::json!({
                "summary": summary,
                "transcript": bundle,
                "working_transcript": working,
                "review": review,
            });
            if include_layers.unwrap_or(true) {
                result["layers"] = serde_json::json!(layers);
            }
            let exports =
                transcript_store::list_exports(driver, &transcript_id).map_err(map_store_error)?;
            let renders =
                transcript_store::list_renders(driver, &transcript_id).map_err(map_store_error)?;
            result["exports"] = serde_json::json!(exports);
            result["renders"] = serde_json::json!(renders);
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Delete a stored transcript and its editable layers atomically. Durable SRT/CSV/text exports and rendered EDL gallery media are preserved with detached source identities and returned in the result."
    )]
    pub async fn educt_delete_transcript(
        &self,
        Parameters(EductDeleteTranscriptRequest { transcript_id }): Parameters<
            EductDeleteTranscriptRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_delete_transcript", async {
            let driver = &**self.gallery_store.driver();
            let counts = transcript_store::delete_transcript(driver, &transcript_id)
                .map_err(map_store_error)?;
            if counts.transcripts_removed == 0 && counts.layers_removed == 0 {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            }
            let exports =
                transcript_store::list_exports(driver, &transcript_id).map_err(map_store_error)?;
            let renders =
                transcript_store::list_renders(driver, &transcript_id).map_err(map_store_error)?;
            Ok(serde_json::json!({
                "transcripts_removed": counts.transcripts_removed,
                "layers_removed": counts.layers_removed,
                "exports_preserved": counts.exports_preserved,
                "renders_preserved": counts.renders_preserved,
                "preserved_exports": exports,
                "preserved_renders": renders,
            }))
        })
        .await
    }

    #[tool(
        description = "Store a layer over a transcript's words: {\"kind\": \"speaker\"|\"paragraph\"|\"correction\"|\"highlight\"|\"edl\"|\"realignment\", ...}. The layer is validated against the transcript's word count before storage — a layer that fails validation is rejected with the named failing invariant and nothing is persisted. Layers anchor to word indices, never timestamps."
    )]
    pub async fn educt_store_layer(
        &self,
        Parameters(EductStoreLayerRequest {
            transcript_id,
            layer,
        }): Parameters<EductStoreLayerRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_store_layer", async {
            let value = parse_json_value(layer.0, "layer")?;
            let parsed: TranscriptLayer = serde_json::from_value(value).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "layer must be a tagged layer JSON object {{\"kind\": \
                     \"speaker\"|\"paragraph\"|\"correction\"|\"highlight\"|\"edl\"|\"realignment\", \
                     ...}}: {e}"
                ))
            })?;
            let driver = &**self.gallery_store.driver();
            let record = transcript_store::store_layer(driver, &transcript_id, &parsed)
                .map_err(map_store_error)?;
            Ok(serde_json::json!({ "stored": record }))
        })
        .await
    }

    #[tool(
        description = "List the layers stored over a transcript, oldest first. Each record carries the layer kind, its provenance (model, prompt template, created_at), and the payload."
    )]
    pub async fn educt_list_layers(
        &self,
        Parameters(EductListLayersRequest { transcript_id }): Parameters<EductListLayersRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_list_layers", async {
            let driver = &**self.gallery_store.driver();
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            Ok(serde_json::json!({
                "layers": layers,
                "count": layers.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the paragraph pass over a stored transcript: an LLM identifies paragraph boundaries as word indices (never timestamps — the deterministic layer maps indices to time). The output is validated against the transcript's word count and stored as a provenance-carrying ParagraphLayer. The response carries pass stats (attempts/rejections/rejection_rate) — the measured v1 failure rate."
    )]
    pub async fn educt_paragraph_pass(
        &self,
        Parameters(EductParagraphPassRequest {
            transcript_id,
            model,
            structured,
        }): Parameters<EductParagraphPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_paragraph_pass", async {
            let _admission = self.admit_heavy_operation()?;
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; the paragraph pass cannot \
                 anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                 first",
            )?;
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let layer = transcript_pass::run_paragraph_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Paragraph(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::paragraph_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the speaker pass over a stored transcript. Source \"audio\" (default): an audio-capable model hears the recording and attributes speaker turns as word-index spans — the primary source. Source \"text\": the text-cue pass (works with every model, approximate). Both produce the same validated SpeakerLayer; provenance records which source produced it. correct_existing=true (with source \"text\") runs the gated correction pass instead: it refines the latest stored speaker layer — the locality gate accepts only contiguous changes of at most 5 words and reverts longer rewrites with a surfaced refusal. The response carries pass stats."
    )]
    pub async fn educt_speaker_pass(
        &self,
        Parameters(EductSpeakerPassRequest {
            transcript_id,
            model,
            source,
            structured,
            correct_existing,
        }): Parameters<EductSpeakerPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_speaker_pass", async {
            let _admission = self.admit_heavy_operation()?;
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; the speaker pass cannot \
                 anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                 first",
            )?;
            // Structured outputs apply to the text passes only — the
            // audio path is prompt-schema (rejected here, never a silent
            // no-op).
            let structured_requested = structured.unwrap_or(false);
            if structured_requested && !matches!(source.as_deref(), Some("text")) {
                return Err(McpToolError::invalid_argument(
                    "structured outputs apply to the text passes (source \"text\"); the \
                     audio path is prompt-schema",
                ));
            }
            let mode = if structured_requested {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            // Gated correction mode (plan §7 Phase 1b): refine the latest
            // stored speaker layer instead of attributing from scratch.
            // The locality gate decides what lands — the model only
            // proposes; long rewrites revert with a surfaced refusal.
            if correct_existing.unwrap_or(false) {
                if !matches!(source.as_deref(), Some("text")) {
                    return Err(McpToolError::invalid_argument(
                        "speaker correction mode (correct_existing=true) requires \
                         source \"text\" — it is the lexical post-pass; run the \
                         audio or text pass without correct_existing to attribute \
                         from scratch",
                    ));
                }
                let layers = transcript_store::list_layers(driver, &transcript_id)
                    .map_err(map_store_error)?;
                let Some(existing) = layers.iter().rev().find_map(|record| match &record.layer {
                    TranscriptLayer::Speaker(layer) => Some(layer),
                    _ => None,
                }) else {
                    return Err(McpToolError::not_found(format!(
                        "no speaker layer found for transcript {transcript_id} — the \
                         correction pass refines an existing attribution; run \
                         educt_speaker_pass first"
                    )));
                };
                let proposed = transcript_pass::run_speaker_correction_pass(
                    &self.vision_port,
                    &self.template_env,
                    &bundle,
                    existing,
                    mode,
                    model.as_deref(),
                )
                .await
                .map_err(map_pass_error)?;
                let (gated_spans, gate) = transcript_pass::gate_speaker_corrections(
                    bundle.words.len(),
                    &existing.spans,
                    &proposed.spans,
                );
                let gated = crate::transcript_layers::SpeakerLayer {
                    provenance: proposed.provenance,
                    spans: gated_spans,
                };
                let record = transcript_store::store_layer(
                    driver,
                    &transcript_id,
                    &TranscriptLayer::Speaker(gated),
                )
                .map_err(map_store_error)?;
                let mut result = serde_json::json!({
                    "stored": record,
                    "gate": gate,
                    "pass_stats": transcript_pass::speaker_correction_pass_stats(),
                });
                if let Some(note) = gate.reverted_note() {
                    result["gate_note"] = serde_json::json!(note);
                }
                return Ok(result);
            }
            // Source dispatch: "audio" (default — the scaffold's primary,
            // decision 7) or "text" (the text-cue pass). An audio failure
            // surfaces as its own error — retry with source "text" is the
            // operator's choice, never a silent fallback.
            let layer = match source.as_deref() {
                None | Some("audio") => transcript_pass::run_speaker_pass_audio(
                    &self.vision_port,
                    &self.template_env,
                    &bundle,
                    model.as_deref(),
                )
                .await
                .map_err(map_pass_error)?,
                Some("text") => transcript_pass::run_speaker_pass(
                    &self.vision_port,
                    &self.template_env,
                    &bundle,
                    mode,
                    model.as_deref(),
                )
                .await
                .map_err(map_pass_error)?,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "source must be \"audio\" or \"text\", got \"{other}\""
                    )));
                }
            };
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Speaker(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::speaker_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the correction pass over a stored transcript: an LLM proposes text replacements over word ranges for likely speech-to-text errors (mishearings, homophones, garbled fragments). Timings are never touched. The newest stored correction becomes the working derived transcript used by inspection and downstream timed operations when aligned; educt_apply_corrections returns that derived view explicitly. Original source words stay immutable. The output is validated (disjoint, in-bounds) and stored as a CorrectionLayer. The response carries pass stats."
    )]
    pub async fn educt_correction_pass(
        &self,
        Parameters(EductCorrectionPassRequest {
            transcript_id,
            model,
            structured,
        }): Parameters<EductCorrectionPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_correction_pass", async {
            let _admission = self.admit_heavy_operation()?;
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; the correction pass cannot \
                 anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                 first",
            )?;
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let layer = transcript_pass::run_correction_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Correction(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::correction_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Apply a stored correction layer as a derived working view without modifying source words/timings. Returns aligned working words only when replacement-token cardinality matches the timed source range; otherwise returns an explicit unaligned state. Defaults to the latest correction layer."
    )]
    pub async fn educt_apply_corrections(
        &self,
        Parameters(EductApplyCorrectionsRequest {
            transcript_id,
            layer_id,
        }): Parameters<EductApplyCorrectionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_apply_corrections", async {
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; corrections cannot anchor \
                 (NoWordTimings)",
            )?;
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let (record, correction) =
                select_correction_layer(&layers, &transcript_id, layer_id.as_deref())?;
            let corrected =
                crate::transcript_layers::corrected_text_view(&bundle.words, &correction.edits);
            // Consistency with the working transcript every timed consumer
            // reads: a realignment layer naming THIS correction projects the
            // re-anchored view here too (MF-2) — apply and consumers must
            // never disagree on the same layer.
            let realignment_layer_id = layers
                .iter()
                .filter(|candidate| {
                    matches!(
                        &candidate.layer,
                        TranscriptLayer::Realignment(layer)
                            if layer.correction_layer_id == record.id
                    )
                })
                .max_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                })
                .map(|candidate| candidate.id.clone());
            let (alignment, working_words, alignment_error) = match realignment_layer_id {
                Some(_) => {
                    match crate::transcript_layers::reanchored_corrected_words(
                        &bundle.words,
                        &correction.edits,
                    ) {
                        Ok(words) => (WorkingTranscriptAlignment::ReAnchored, Some(words), None),
                        Err(error) => (
                            WorkingTranscriptAlignment::Unaligned,
                            None,
                            Some(error.to_string()),
                        ),
                    }
                }
                None => {
                    match crate::transcript_layers::aligned_corrected_words(
                        &bundle.words,
                        &correction.edits,
                    ) {
                        Ok(words) => (WorkingTranscriptAlignment::Aligned, Some(words), None),
                        Err(error) => (
                            WorkingTranscriptAlignment::Unaligned,
                            None,
                            Some(error.to_string()),
                        ),
                    }
                }
            };
            Ok(serde_json::json!({
                "corrected_text": corrected,
                "alignment": alignment,
                "alignment_error": alignment_error,
                "realignment_layer_id": realignment_layer_id,
                "working_words": working_words,
                "applied_layer": {
                    "id": record.id,
                    "provenance": record.layer.provenance(),
                    "edits": correction.edits.len(),
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Store a realignment layer recording the decision to project a correction with re-anchored timings: each cardinality-changing edit's replacement tokens are sliced equally across its source range's span, restoring timed consumers (educt_locate, highlighting, SRT) after insert/delete corrections. The immutable source bundle and its timings are never modified; interpolated tokens carry no STT confidence. Refuses an untimed transcript; a no-op on an already-aligned correction. Defaults to the latest correction layer."
    )]
    pub async fn educt_realign_transcript(
        &self,
        Parameters(EductRealignTranscriptRequest {
            transcript_id,
            layer_id,
        }): Parameters<EductRealignTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_realign_transcript", async {
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; a realignment cannot \
                 anchor interpolation on an untimed source (NoWordTimings)",
            )?;
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let (record, correction) =
                select_correction_layer(&layers, &transcript_id, layer_id.as_deref())?;
            // An already-aligned correction needs no re-anchoring — an
            // honest no-op, not an error.
            if crate::transcript_layers::aligned_corrected_words(&bundle.words, &correction.edits)
                .is_ok()
            {
                return Ok(serde_json::json!({
                    "alignment": "aligned",
                    "note": "correction is already one-for-one aligned; no re-anchoring needed",
                    "applied_layer": {"id": record.id},
                }));
            }
            let realignment =
                TranscriptLayer::Realignment(crate::transcript_layers::RealignmentLayer {
                    provenance: LayerProvenance {
                        model: "deterministic".to_string(),
                        prompt_template: "educt_realign_transcript".to_string(),
                        created_at: hkask_types::time::now_rfc3339(),
                    },
                    correction_layer_id: record.id.clone(),
                });
            let stored = transcript_store::store_layer(driver, &transcript_id, &realignment)
                .map_err(map_store_error)?;
            let working_words = crate::transcript_layers::reanchored_corrected_words(
                &bundle.words,
                &correction.edits,
            )
            .map_err(|error| {
                McpToolError::internal(format!("re-anchoring failed on a validated layer: {error}"))
            })?;
            Ok(serde_json::json!({
                "alignment": "re_anchored",
                "working_words": working_words,
                "stored": stored,
                "reanchored": {
                    "correction_layer_id": record.id,
                    "interpolation": "equal-slice across each edited range's source span",
                    "source_timings": "unchanged — the immutable source bundle is never modified",
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Run semantic selection over the timing-aligned working transcript (the latest correction when present). A natural-language request resolves to validated labeled word ranges; unaligned corrections fail visibly before inference."
    )]
    pub async fn educt_highlight_pass(
        &self,
        Parameters(EductHighlightPassRequest {
            transcript_id,
            request,
            model,
            structured,
        }): Parameters<EductHighlightPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_highlight_pass", async {
            let _admission = self.admit_heavy_operation()?;
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; the highlight pass cannot \
                 anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                 first",
            )?;
            let working = working_transcript(driver, &transcript_id, &bundle)?;
            let working_words = working.require_timed_words("educt_highlight_pass")?;
            let mut working_bundle = bundle.clone();
            working_bundle.words = working_words.to_vec();
            working_bundle.full_text = working.text;
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let mut layer = transcript_pass::run_highlight_pass(
                &self.vision_port,
                &self.template_env,
                &working_bundle,
                &request,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            // The pass selects over the working view; highlight layers
            // anchor to immutable SOURCE word indices. Translate through
            // the correction projection whenever a correction layer
            // exists — otherwise a cardinality-changing correction makes
            // pass selections overshoot the source space (observed live
            // 2026-10-25: word 568 against a 558-word source). An index
            // the map cannot place passes through unchanged for the
            // store's own validation to reject by name.
            if let Some(correction_layer_id) = working.correction_layer_id.clone() {
                let layers = transcript_store::list_layers(driver, &transcript_id)
                    .map_err(map_store_error)?;
                let (_record, correction) =
                    select_correction_layer(&layers, &transcript_id, Some(&correction_layer_id))?;
                let index_map = crate::transcript_layers::reanchored_index_map(
                    &bundle.words,
                    &correction.edits,
                )
                .map_err(|error| {
                    McpToolError::internal(format!("re-anchoring index map failed: {error}"))
                })?;
                for highlight in &mut layer.highlights {
                    if let Some(start) = index_map.get(highlight.start_word) {
                        highlight.start_word = *start;
                    }
                    if let Some(end) = index_map.get(highlight.end_word) {
                        highlight.end_word = *end;
                    }
                }
            }
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Highlight(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::highlight_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Compose an EDL (the Reel) from a stored highlight layer, deterministically: each selected highlight becomes a Keep op over its word range, and overlapping selections are union-merged so the Keep ops are disjoint. Optionally filter highlights by exact label; defaults to the latest highlight layer. The composed EDL is validated and stored as an EdlLayer — render it with educt_render_edl."
    )]
    pub async fn educt_edl_from_highlights(
        &self,
        Parameters(EductEdlFromHighlightsRequest {
            transcript_id,
            label,
            layer_id,
        }): Parameters<EductEdlFromHighlightsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_edl_from_highlights", async {
            let driver = &**self.gallery_store.driver();
            load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; an EDL cannot anchor \
                 (NoWordTimings)",
            )?;
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let record =
                select_layer_of_kind(&layers, &transcript_id, "highlight", layer_id.as_deref())?;
            let TranscriptLayer::Highlight(highlight) = &record.layer else {
                return Err(McpToolError::internal(
                    "layer kind mismatch after highlight filter",
                ));
            };
            let selected: Vec<&HighlightEntry> = match label.as_deref() {
                Some(label) => highlight
                    .highlights
                    .iter()
                    .filter(|entry| entry.label == label)
                    .collect(),
                None => highlight.highlights.iter().collect(),
            };
            if selected.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "no highlights{} in layer {}",
                    label
                        .as_deref()
                        .map(|label| format!(" labeled {label:?}"))
                        .unwrap_or_default(),
                    record.id
                )));
            }
            // Union-merge the selected ranges: overlapping highlights are
            // fine as annotations, but EDL Keep ops must be disjoint — the
            // union covers exactly the highlighted words.
            let ranges: Vec<WordRange> = selected
                .iter()
                .map(|entry| WordRange::new(entry.start_word, entry.end_word))
                .collect();
            let merged = union_ranges(&ranges);
            let ranges_merged = merged.len();
            let edl = EdlLayer {
                provenance: LayerProvenance {
                    model: "deterministic".to_string(),
                    prompt_template: "educt_edl_from_highlights".to_string(),
                    created_at: hkask_types::time::now_rfc3339(),
                },
                ops: merged
                    .into_iter()
                    .map(|range| EdlEntry {
                        range,
                        op: EdlOp::Keep,
                    })
                    .collect(),
            };
            let stored =
                transcript_store::store_layer(driver, &transcript_id, &TranscriptLayer::Edl(edl))
                    .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": stored,
                "composed_from": {
                    "layer_id": record.id,
                    "highlights_selected": selected.len(),
                    "ranges_merged": ranges_merged,
                    "label_filter": label,
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Render a stored EDL layer to one canonical durable gallery media asset: word ranges map to time ranges, ffmpeg clips and concatenates them losslessly, and temporary clips are removed. Audio sources publish WAV; other sources publish MP4. Defaults to the latest EDL layer."
    )]
    pub async fn educt_render_edl(
        &self,
        Parameters(EductRenderEdlRequest {
            transcript_id,
            layer_id,
        }): Parameters<EductRenderEdlRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_render_edl", async {
            let _admission = self.admit_heavy_operation()?;
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; an EDL cannot render \
                 (NoWordTimings)",
            )?;
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let mut edl_layers: Vec<_> = layers
                .into_iter()
                .filter(|record| record.layer.kind() == "edl")
                .collect();
            // Newest first with the same ID tie-break as every layer
            // selection in this family (deterministic among ties).
            edl_layers.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
            let record = match layer_id {
                Some(id) => edl_layers.into_iter().find(|record| record.id == id),
                None => edl_layers.into_iter().next(),
            };
            let Some(record) = record else {
                return Err(McpToolError::not_found(format!(
                    "no EDL layer found for transcript {transcript_id}"
                )));
            };
            let TranscriptLayer::Edl(edl) = &record.layer else {
                return Err(McpToolError::internal(
                    "layer kind mismatch after EDL filter",
                ));
            };
            // The slice-1 algebra: EDL → keep ranges → clip plan (the only
            // index→time mapping; the render consumes its output).
            let selection_edl = Edl {
                ops: edl.ops.clone(),
            };
            let plan_ms = edl_to_clip_plan(&bundle.words, &selection_edl).map_err(|error| {
                McpToolError::invalid_argument(format!("EDL rejected: {error}"))
            })?;
            if plan_ms.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "EDL cuts the entire transcript — nothing to render",
                ));
            }
            let plan_secs: Vec<(f64, f64)> = plan_ms
                .iter()
                .map(|(start_ms, end_ms)| (*start_ms as f64 / 1000.0, *end_ms as f64 / 1000.0))
                .collect();

            self.require_ffmpeg()?;
            let media_path = bundle.audio_path.clone();
            let audio = is_audio_path(&media_path);
            let mut intermediates = crate::assets::ScratchIntermediates::new("EDL render");
            let mut clip_paths: Vec<std::path::PathBuf> = Vec::with_capacity(plan_secs.len());
            for (start_sec, end_sec) in &plan_secs {
                let clipped = if audio {
                    self.ffmpeg
                        .audio_trim(&media_path, *start_sec as f32, *end_sec as f32)
                        .await
                } else {
                    self.ffmpeg
                        .clip(&media_path, *start_sec as f32, *end_sec as f32)
                        .await
                }
                .map_err(map_media_error)?;
                intermediates.track(clipped.clone());
                clip_paths.push(clipped);
            }
            let output = if clip_paths.len() == 1 {
                clip_paths.first().cloned().ok_or_else(|| {
                    McpToolError::internal("EDL render produced no clip after a non-empty plan")
                })?
            } else {
                let clip_sources = clip_paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                if audio {
                    self.ffmpeg
                        .audio_concat(&clip_sources)
                        .await
                        .map_err(map_media_error)?
                } else {
                    self.ffmpeg
                        .concat(&clip_sources)
                        .await
                        .map_err(map_media_error)?
                }
            };
            let effective_params = EdlRenderEffectiveParams {
                transcript_id: &transcript_id,
                edl_layer_id: &record.id,
                source: &media_path,
                clip_plan: &plan_secs,
                clips: clip_paths.len(),
            };
            crate::assets::publish_local_media_with_transcript_render(
                &self.gallery_store,
                &output,
                "educt_render_edl",
                "rendered",
                if audio {
                    crate::assets::LocalMediaFormat::Wav
                } else {
                    crate::assets::LocalMediaFormat::Mp4
                },
                &effective_params,
                &transcript_id,
                &record.id,
            )
        })
        .await
    }

    #[tool(
        description = "Publish a durable transcript document with stable export_id and provenance. SRT and mapped corpus text use the timing-aligned working transcript; review_csv derives paragraph-block review flags from the latest speaker layer's stored confidences; unaligned corrected text remains exportable for search with an explicit no-range-mapping degradation. Documents stay outside the playable-media gallery."
    )]
    pub async fn educt_export(
        &self,
        Parameters(EductExportRequest {
            transcript_id,
            format,
        }): Parameters<EductExportRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_export", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            let export_format = crate::transcript_export::TranscriptExportFormat::parse(&format)
                .ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "format must be \"srt\", \"highlights_csv\", \"review_csv\", or \
                         \"corpus_text\", got \"{format}\""
                    ))
                })?;
            match export_format {
                crate::transcript_export::TranscriptExportFormat::Srt => {
                    if !summary.has_word_timings {
                        return Err(McpToolError::invalid_argument(
                            "transcript has no word-level timings; SRT cues cannot anchor \
                             (NoWordTimings)",
                        ));
                    }
                    let working = working_transcript(driver, &transcript_id, &bundle)?;
                    let working_words = working.require_timed_words("SRT export")?;
                    let srt = crate::transcript_export::srt_from_words(working_words).map_err(
                        |error| McpToolError::invalid_argument(format!("SRT export: {error}")),
                    )?;
                    let effective_params = SrtExportEffectiveParams {
                        transcript_id: &transcript_id,
                        cues: srt.matches("\n\n").count(),
                        correction_layer_id: working.correction_layer_id.as_deref(),
                    };
                    crate::transcript_export::publish_document(
                        driver,
                        &transcript_id,
                        export_format,
                        srt.as_bytes(),
                        &effective_params,
                    )
                }
                crate::transcript_export::TranscriptExportFormat::HighlightsCsv => {
                    let layers = transcript_store::list_layers(driver, &transcript_id)
                        .map_err(map_store_error)?;
                    let highlight_records: Vec<_> = layers
                        .into_iter()
                        .filter(|record| record.layer.kind() == "highlight")
                        .collect();
                    if highlight_records.is_empty() {
                        return Err(McpToolError::not_found(format!(
                            "no highlight layers found for transcript {transcript_id}"
                        )));
                    }
                    let rows = highlight_records
                        .iter()
                        .map(|record| match &record.layer {
                            TranscriptLayer::Highlight(highlight) => highlight.highlights.len(),
                            _ => 0,
                        })
                        .sum::<usize>();
                    let csv =
                        crate::transcript_export::highlights_csv(&bundle.words, &highlight_records)
                            .map_err(|error| {
                                McpToolError::invalid_argument(format!("CSV export: {error}"))
                            })?;
                    let layer_ids = highlight_records
                        .iter()
                        .map(|record| record.id.clone())
                        .collect::<Vec<_>>();
                    let effective_params = HighlightsExportEffectiveParams {
                        transcript_id: &transcript_id,
                        layer_ids: &layer_ids,
                        rows,
                    };
                    crate::transcript_export::publish_document(
                        driver,
                        &transcript_id,
                        export_format,
                        csv.as_bytes(),
                        &effective_params,
                    )
                }
                crate::transcript_export::TranscriptExportFormat::ReviewCsv => {
                    let layers = transcript_store::list_layers(driver, &transcript_id)
                        .map_err(map_store_error)?;
                    let review = crate::transcript_review::derive_review_flags(
                        bundle.words.len(),
                        &layers,
                    );
                    if review.paragraph_layer_id.is_none() {
                        return Err(McpToolError::not_found(format!(
                            "no paragraph layer found for transcript {transcript_id} — review \
                             flags attach to paragraph blocks; run educt_paragraph_pass first"
                        )));
                    }
                    let csv = crate::transcript_export::review_csv(&bundle.words, &review)
                        .map_err(|error| {
                            McpToolError::invalid_argument(format!(
                                "review CSV export: {error}"
                            ))
                        })?;
                    let effective_params = ReviewCsvExportEffectiveParams {
                        transcript_id: &transcript_id,
                        paragraph_layer_id: review.paragraph_layer_id.as_deref(),
                        speaker_layer_id: review.speaker_layer_id.as_deref(),
                        threshold: review.threshold,
                        blocks: review.blocks.len(),
                        flagged: review
                            .blocks
                            .iter()
                            .filter(|block| {
                                block.status == crate::transcript_review::ReviewStatus::Flagged
                            })
                            .count(),
                    };
                    crate::transcript_export::publish_document(
                        driver,
                        &transcript_id,
                        export_format,
                        csv.as_bytes(),
                        &effective_params,
                    )
                }
                crate::transcript_export::TranscriptExportFormat::CorpusText => {
                    const ALIGNED_COMPOSITION: &str = "run corpus_convert → corpus_chunk → \
                         corpus_embed on this file; corpus_query hits map back to word ranges via \
                         text_to_word_ranges over the timing-aligned working transcript";
                    const TEXT_ONLY_COMPOSITION: &str = "run corpus_convert → corpus_chunk → \
                         corpus_embed on this file; text remains searchable but cannot map back to \
                         word ranges until the transcript is timing-aligned";
                    let working = if summary.has_word_timings {
                        Some(working_transcript(driver, &transcript_id, &bundle)?)
                    } else {
                        None
                    };
                    let (text, word_timings, correction_layer_id, correction_alignment, degradation) =
                        match working {
                            Some(working) if working.words.is_some() => (
                                working.text,
                                true,
                                working.correction_layer_id,
                                working.alignment,
                                None,
                            ),
                            Some(working) => (
                                working.text,
                                false,
                                working.correction_layer_id,
                                working.alignment,
                                Some(format!(
                                    "working transcript is unaligned — {}. Corpus text was exported, but hits cannot map back to word ranges; store a realignment layer via educt_realign_transcript to restore range mapping",
                                    working.alignment_error.as_deref().unwrap_or("unknown alignment error")
                                )),
                            ),
                            None => (
                                bundle.full_text.clone(),
                                false,
                                None,
                                WorkingTranscriptAlignment::Untimed,
                                Some(
                                    "no word-level timings — exported the provider's full_text; corpus hits cannot map back to word ranges (NoWordTimings)".to_string(),
                                ),
                            ),
                        };
                    let effective_params = CorpusExportEffectiveParams {
                        transcript_id: &transcript_id,
                        words: bundle.words.len(),
                        word_timings,
                        correction_layer_id: correction_layer_id.as_deref(),
                        correction_alignment,
                        composition: if word_timings {
                            ALIGNED_COMPOSITION
                        } else {
                            TEXT_ONLY_COMPOSITION
                        },
                    };
                    let mut result = crate::transcript_export::publish_document(
                        driver,
                        &transcript_id,
                        export_format,
                        text.as_bytes(),
                        &effective_params,
                    )?;
                    if let Some(degradation) = degradation {
                        result["degradation"] = serde_json::json!(degradation);
                    }
                    Ok(result)
                }
            }
        })
        .await
    }

    #[tool(
        description = "Locate a quoted passage in the timing-aligned working transcript, deterministically. Returns every word-aligned match and time range; ambiguity is surfaced, unaligned corrections fail visibly, and no model guesses timestamps."
    )]
    pub async fn educt_locate(
        &self,
        Parameters(EductLocateRequest {
            transcript_id,
            text,
        }): Parameters<EductLocateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_locate", async {
            let driver = &**self.gallery_store.driver();
            let bundle = load_timed_transcript(
                driver,
                &transcript_id,
                "transcript has no word-level timings; a quote cannot map to a media \
                 range (NoWordTimings)",
            )?;
            let working = working_transcript(driver, &transcript_id, &bundle)?;
            let working_words = working.require_timed_words("educt_locate")?;
            let ranges = text_to_word_ranges(working_words, &text);
            if ranges.is_empty() {
                return Ok(serde_json::json!({
                    "status": "no_match",
                    "ranges": [],
                    "count": 0,
                    "note": "no word-aligned match — quote the rendered form exactly \
                             (punctuation included)",
                }));
            }
            let mut located = Vec::with_capacity(ranges.len());
            for range in ranges {
                let (start_ms, end_ms) =
                    word_range_to_time_range(working_words, range).map_err(|error| {
                        McpToolError::internal(format!(
                            "impossible: text_to_word_ranges produced an out-of-bounds \
                             range: {error}"
                        ))
                    })?;
                located.push(serde_json::json!({
                    "start_word": range.start_word,
                    "end_word": range.end_word,
                    "start_ms": start_ms,
                    "end_ms": end_ms,
                }));
            }
            Ok(serde_json::json!({
                "status": "located",
                "ranges": located,
                "count": located.len(),
            }))
        })
        .await
    }
}

#[cfg(test)]
mod researcher_tests {
    use super::*;
    use crate::tools::gallery::tests::{payload, research_server};

    fn search(query: &str) -> Parameters<EductSearchTranscriptsRequest> {
        Parameters(EductSearchTranscriptsRequest {
            query: query.into(),
            mode: "lexical".into(),
            limit: Some(20),
            recording_limit: Some(100),
            recording_offset: None,
            start_byte: None,
            gallery_asset_id: None,
        })
    }

    fn recording(path: &str, words: &[&str]) -> TranscriptBundle {
        let mut bundle = TranscriptBundle::new(path.into(), words.len() as f32, words.join(" "));
        bundle.words = crate::test_support::timed_words(words);
        bundle
    }

    /// expect: All eight CRUD requests deserialize without self-attested approval
    /// and their generated schemas contain no approval field. [P1]
    #[test]
    fn researcher_crud_schemas_use_host_authorization_not_self_attestation() {
        use crate::tools::gallery::*;
        fn check<T: serde::de::DeserializeOwned + schemars::JsonSchema>(
            request: serde_json::Value,
        ) {
            serde_json::from_value::<T>(request).expect("request without approval field");
            let schema = serde_json::to_value(schemars::schema_for!(T)).expect("schema JSON");
            let properties = schema["properties"]
                .as_object()
                .expect("object request schema");
            assert!(!properties.contains_key("user_approved"));
        }
        check::<GalleryAnnotationCreateRequest>(
            serde_json::json!({"gallery_asset_id":"asset", "kind":"note", "value":"supplied"}),
        );
        check::<GalleryAnnotationListRequest>(serde_json::json!({"gallery_asset_id":"asset"}));
        check::<GalleryAnnotationUpdateRequest>(
            serde_json::json!({"gallery_asset_id":"asset", "annotation_id":"annotation", "value":"updated"}),
        );
        check::<GalleryAnnotationDeleteRequest>(
            serde_json::json!({"gallery_asset_id":"asset", "annotation_id":"annotation"}),
        );
        check::<EductAnnotationCreateRequest>(
            serde_json::json!({"transcript_id":"transcript", "start_word":0, "end_word":1, "label":"finding", "note":"supplied"}),
        );
        check::<EductAnnotationListRequest>(serde_json::json!({"transcript_id":"transcript"}));
        check::<EductAnnotationUpdateRequest>(
            serde_json::json!({"transcript_id":"transcript", "annotation_id":"annotation", "start_word":0, "end_word":1, "label":"finding", "note":"updated"}),
        );
        check::<EductAnnotationDeleteRequest>(
            serde_json::json!({"transcript_id":"transcript", "annotation_id":"annotation"}),
        );
    }

    /// expect: A bounded page resumes at the first omitted hit, including
    /// overlapping matches in the same recording and lookahead in the next. [P1]
    #[tokio::test]
    async fn researcher_search_cursor_never_skips_or_duplicates_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        for path in ["/missing/one.wav", "/missing/two.wav"] {
            transcript_store::store_transcript(
                driver,
                &recording(path, &["banana", "banana"]),
                None,
            )?;
        }
        let all = payload(&server.educt_search_transcripts(search("ana")).await?);
        assert_eq!(all["count"], 8);
        let expected = all["hits"].as_array().ok_or("all hits")?.clone();
        let mut actual = Vec::new();
        let mut request = search("ana").0;
        request.limit = Some(1);
        for _ in 0..10 {
            let page = payload(&server.educt_search_transcripts(Parameters(request)).await?);
            actual.extend(page["hits"].as_array().ok_or("page hits")?.iter().cloned());
            if page["has_more"] == false {
                assert_eq!(page["search_complete"], true);
                assert!(page["next_recording_offset"].is_null());
                break;
            }
            let recording_offset = page["next_recording_offset"]
                .as_u64()
                .ok_or("recording cursor")? as usize;
            let start_byte = page["next_start_byte"].as_u64().ok_or("byte cursor")? as usize;
            request = EductSearchTranscriptsRequest {
                limit: Some(1),
                recording_offset: Some(recording_offset),
                start_byte: Some(start_byte),
                ..search("ana").0
            };
        }
        assert_eq!(actual, expected);
        Ok(())
    }

    /// expect: Large unmatched content cannot evade the matching budget; a
    /// boundary-crossing phrase is returned exactly once and remaining hits resume. [P1]
    #[tokio::test]
    async fn researcher_search_byte_budget_and_utf8_cursor_are_honest()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let text = format!("{}needle é needle", "x".repeat(SEARCH_TEXT_BYTE_BUDGET - 3));
        transcript_store::store_transcript(
            &**server.gallery_store.driver(),
            &TranscriptBundle::new("/missing/large.wav".into(), 1.0, text),
            None,
        )?;
        let first = payload(&server.educt_search_transcripts(search("needle")).await?);
        assert_eq!(first["count"], 1);
        assert_eq!(first["scan_byte_limit_reached"], true);
        assert_eq!(first["text_bytes_scanned"], SEARCH_TEXT_BYTE_BUDGET);
        assert_eq!(first["next_recording_offset"], 0);
        assert_eq!(first["next_start_byte"], SEARCH_TEXT_BYTE_BUDGET);
        assert_eq!(first["hits"][0]["start_byte"], SEARCH_TEXT_BYTE_BUDGET - 3);
        let second = payload(
            &server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    start_byte: Some(SEARCH_TEXT_BYTE_BUDGET),
                    ..search("needle").0
                }))
                .await?,
        );
        assert_eq!(second["count"], 1);
        assert_eq!(second["search_complete"], true);
        assert!(
            second["hits"][0]["start_byte"]
                .as_u64()
                .ok_or("second offset")?
                > SEARCH_TEXT_BYTE_BUDGET as u64
        );
        assert!(first.get("transcript").is_none());
        // A cursor inside a multibyte scalar is a visible invalid argument.
        let unicode_server = research_server();
        transcript_store::store_transcript(
            &**unicode_server.gallery_store.driver(),
            &TranscriptBundle::new("/missing/unicode.wav".into(), 1.0, "éNEEDLE".into()),
            None,
        )?;
        assert!(
            unicode_server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    start_byte: Some(1),
                    ..search("needle").0
                }))
                .await
                .is_err()
        );
        assert!(
            unicode_server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    start_byte: Some(100),
                    ..search("needle").0
                }))
                .await
                .is_err()
        );
        let valid = payload(
            &unicode_server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    start_byte: Some(2),
                    ..search("needle").0
                }))
                .await?,
        );
        assert_eq!(valid["hits"][0]["matched_text"], "NEEDLE");
        assert_eq!(valid["hits"][0]["start_byte"], 2);
        Ok(())
    }

    /// expect: Late repeated hits retain correct source-word/time citations
    /// with a monotonic word cursor; overlaps and Unicode do not shift indices. [P1]
    #[test]
    fn researcher_search_late_hits_map_without_restarting_word_scan() {
        let mut text = vec!["padding"; 50_000];
        text.extend(["ébanana", "banana", "end"]);
        let words = crate::test_support::timed_words(&text);
        let working = WorkingTranscript {
            text: crate::transcript_select::rendered_transcript(&words),
            words: Some(words),
            alignment: WorkingTranscriptAlignment::Original,
            correction_layer_id: None,
            realignment_layer_id: None,
            alignment_error: None,
        };
        let start = working.text.find("ébanana").expect("late marker");
        let hits = lexical_citations(&working, "ana", start, working.text.len(), 100);
        assert_eq!(hits.len(), 4);
        for (position, (_, hit)) in hits.iter().enumerate() {
            assert_eq!(hit["word_range"]["start_word"], 50_000 + position / 2);
            assert_eq!(
                hit["time_range_ms"]["start_ms"],
                (50_000 + position / 2) * 1000
            );
            assert_eq!(hit["matched_text"], "ana");
        }
    }

    /// expect: Content retrieval finds two independently stored recordings with
    /// exact text and source-linked timed citations, not unrelated transcripts. [P1]
    #[tokio::test]
    async fn researcher_search_two_recordings_with_citations_and_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let dir = tempfile::tempdir()?;
        let store = &server.gallery_store;
        let gallery = store.open(&dir.path().to_string_lossy(), GalleryMode::ReadOnly)?;
        let mut ids = Vec::new();
        for (name, words) in [
            ("one.wav", vec!["We", "found", "fracture", "today"]),
            ("two.wav", vec!["Another", "FRACTURE", "appears"]),
            ("other.wav", vec!["No", "matching", "content"]),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"unchanged source")?;
            let asset = store.add_media(
                &gallery.id,
                &path.to_string_lossy(),
                name,
                0,
                0,
                "wav",
                16,
                "audio",
            )?;
            let summary = transcript_store::store_transcript(
                &**store.driver(),
                &recording(&path.to_string_lossy(), &words),
                Some(&asset.id),
            )?;
            ids.push((summary.id, asset.id));
        }
        let result = payload(&server.educt_search_transcripts(search("fracture")).await?);
        let hits = result["hits"].as_array().ok_or("hits")?;
        assert_eq!(hits.len(), 2);
        assert_eq!(result["search_complete"], true);
        for (transcript_id, asset_id) in &ids[..2] {
            let hit = hits
                .iter()
                .find(|hit| hit["transcript_id"] == *transcript_id)
                .ok_or("expected recording")?;
            assert_eq!(hit["gallery_id"], gallery.id);
            assert_eq!(hit["gallery_asset_id"], *asset_id);
            assert_eq!(hit["source_gallery_asset_id"], *asset_id);
            let index = if transcript_id == &ids[0].0 { 2 } else { 1 };
            assert_eq!(hit["word_range"]["start_word"], index);
            assert_eq!(hit["word_range"]["end_word"], index);
            assert_eq!(hit["time_range_ms"]["start_ms"], index * 1000);
            assert_eq!(hit["time_range_ms"]["end_ms"], index * 1000 + 500);
            assert!(
                hit["matched_text"]
                    .as_str()
                    .ok_or("matched text")?
                    .eq_ignore_ascii_case("fracture")
            );
            assert!(
                hit["media_path"]
                    .as_str()
                    .ok_or("source path")?
                    .ends_with(".wav")
            );
        }
        assert!(!hits.iter().any(|hit| hit["transcript_id"] == ids[2].0));
        assert_eq!(
            payload(
                &server
                    .educt_search_transcripts(search("absent phrase"))
                    .await?
            )["count"],
            0
        );
        let bounded = payload(
            &server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    limit: Some(1),
                    ..search("fracture").0
                }))
                .await?,
        );
        assert_eq!(bounded["count"], 1);
        assert_eq!(bounded["result_limit_reached"], true);
        assert_eq!(bounded["search_complete"], false);
        let page = payload(
            &server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    recording_limit: Some(1),
                    ..search("fracture").0
                }))
                .await?,
        );
        assert_eq!(page["recordings_scanned"], 1);
        assert_eq!(page["has_more_recordings"], true);
        assert_eq!(page["next_recording_offset"], 1);
        let filtered = payload(
            &server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    gallery_asset_id: Some(ids[0].1.clone()),
                    ..search("fracture").0
                }))
                .await?,
        );
        assert_eq!(filtered["count"], 1);
        assert_eq!(filtered["hits"][0]["transcript_id"], ids[0].0);
        assert!(
            server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    limit: Some(0),
                    ..search("fracture").0
                }))
                .await
                .is_err()
        );
        assert!(
            server
                .educt_search_transcripts(Parameters(EductSearchTranscriptsRequest {
                    mode: "semantic".into(),
                    ..search("fracture").0
                }))
                .await
                .is_err()
        );
        Ok(())
    }

    /// expect: Untimed text remains searchable and unaligned corrections never
    /// inherit misleading source-word indices/times; exact corrected text wins. [P1]
    #[tokio::test]
    async fn researcher_search_honest_untimed_and_unaligned_views()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        let untimed = transcript_store::store_transcript(
            driver,
            &TranscriptBundle::new(
                "/missing/untimed.wav".into(),
                1.0,
                "fracture without timings".into(),
            ),
            None,
        )?;
        let corrected = transcript_store::store_transcript(
            driver,
            &recording("/missing/corrected.wav", &["bad", "reading"]),
            None,
        )?;
        transcript_store::store_layer(
            driver,
            &corrected.id,
            &TranscriptLayer::Correction(crate::transcript_layers::CorrectionLayer {
                provenance: LayerProvenance {
                    model: "test".into(),
                    prompt_template: "test".into(),
                    created_at: hkask_types::time::now_rfc3339(),
                },
                edits: vec![crate::transcript_layers::CorrectionEdit {
                    start_word: 0,
                    end_word: 1,
                    replacement: "fracture corrected reading".into(),
                    reason: "test cardinality change".into(),
                }],
            }),
        )?;
        let result = payload(&server.educt_search_transcripts(search("fracture")).await?);
        let hits = result["hits"].as_array().ok_or("hits")?;
        assert_eq!(hits.len(), 2);
        for (id, alignment) in [(&untimed.id, "untimed"), (&corrected.id, "unaligned")] {
            let hit = hits
                .iter()
                .find(|hit| hit["transcript_id"] == *id)
                .ok_or("expected degradation")?;
            assert_eq!(hit["alignment"], alignment);
            assert!(hit["word_range"].is_null());
            assert!(hit["time_range_ms"].is_null());
            assert!(hit["alignment_error"].is_string());
            assert_eq!(hit["matched_text"], "fracture");
        }
        assert_eq!(
            payload(
                &server
                    .educt_search_transcripts(search("bad reading"))
                    .await?
            )["count"],
            0
        );
        Ok(())
    }

    /// expect: Researchers can author/read/update/delete highlights without raw
    /// layer JSON, preserving labels/notes, identity, model layers and source text. [P1]
    #[tokio::test]
    async fn researcher_highlight_crud_preserves_other_layers_and_original()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        let bundle = recording("/missing/source.wav", &["a", "fracture", "here"]);
        let summary = transcript_store::store_transcript(driver, &bundle, None)?;
        let suggested = transcript_store::store_layer(
            driver,
            &summary.id,
            &TranscriptLayer::Highlight(crate::transcript_layers::HighlightLayer {
                provenance: LayerProvenance {
                    model: "vision-model".into(),
                    prompt_template: "highlight_pass".into(),
                    created_at: hkask_types::time::now_rfc3339(),
                },
                highlights: vec![HighlightEntry {
                    start_word: 0,
                    end_word: 2,
                    label: "suggestion".into(),
                    note: "keep this note".into(),
                }],
            }),
        )?;
        // Working-text corrections must not redefine the immutable source-word
        // basis of researcher annotations, even when corrections are unaligned.
        transcript_store::store_layer(
            driver,
            &summary.id,
            &TranscriptLayer::Correction(crate::transcript_layers::CorrectionLayer {
                provenance: LayerProvenance {
                    model: "test".into(),
                    prompt_template: "test".into(),
                    created_at: hkask_types::time::now_rfc3339(),
                },
                edits: vec![crate::transcript_layers::CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "a revised source".into(),
                    reason: "cardinality change".into(),
                }],
            }),
        )?;
        let create = || {
            Parameters(EductAnnotationCreateRequest {
                transcript_id: summary.id.clone(),
                start_word: 1,
                end_word: 1,
                label: "finding".into(),
                note: "researcher's note".into(),
                kind: None,
            })
        };
        let result = payload(&server.educt_annotation_create(create()).await?);
        assert_eq!(result["provenance"], "researcher_supplied");
        assert_eq!(
            result["layer"]["provenance"]["model"],
            crate::transcript_layers::RESEARCHER_ANNOTATION_MODEL
        );
        assert_eq!(result["word_index_basis"], "immutable_source");
        assert_eq!(result["layer"]["highlights"][0]["start_word"], 1);
        let annotation_id = result["annotation_id"].as_str().ok_or("annotation ID")?;
        let other = payload(&server.educt_annotation_create(create()).await?);
        let update = Parameters(EductAnnotationUpdateRequest {
            transcript_id: summary.id.clone(),
            annotation_id: annotation_id.into(),
            start_word: 0,
            end_word: 1,
            label: "reviewed".into(),
            note: "changed note".into(),
        });
        server.educt_annotation_update(update).await?;
        let listed = payload(
            &server
                .educt_annotation_list(Parameters(EductAnnotationListRequest {
                    transcript_id: summary.id.clone(),
                }))
                .await?,
        );
        let annotations = listed["annotations"].as_array().ok_or("annotations")?;
        assert_eq!(annotations.len(), 2);
        let updated = annotations
            .iter()
            .find(|record| record["id"] == annotation_id)
            .ok_or("stable annotation ID")?;
        assert_eq!(updated["layer"]["highlights"][0]["label"], "reviewed");
        assert_eq!(updated["layer"]["highlights"][0]["note"], "changed note");
        assert!(
            server
                .educt_annotation_delete(Parameters(EductAnnotationDeleteRequest {
                    transcript_id: summary.id.clone(),
                    annotation_id: suggested.id.clone(),
                }))
                .await
                .is_err()
        );
        server
            .educt_annotation_delete(Parameters(EductAnnotationDeleteRequest {
                transcript_id: summary.id.clone(),
                annotation_id: annotation_id.into(),
            }))
            .await?;
        let layers = transcript_store::list_layers(driver, &summary.id)?;
        assert_eq!(layers.len(), 3);
        assert!(layers.iter().any(|record| {
            record.id
                == other["annotation_id"]
                    .as_str()
                    .expect("other annotation ID")
        }));
        assert_eq!(
            layers
                .iter()
                .find(|record| record.id == suggested.id)
                .ok_or("retained suggestion")?
                .layer,
            suggested.layer
        );
        let (_, original) =
            transcript_store::load_transcript(driver, &summary.id)?.ok_or("original bundle")?;
        assert_eq!(original.full_text, bundle.full_text);

        Ok(())
    }

    /// expect: Researchers author strikes (single-Cut EDLs) through the same
    /// annotation CRUD surface — blank label/note enforced, listed beside
    /// highlights, update/delete dispatch on the stored kind, and multi-op
    /// researcher-EDLs never masquerade as strikes. [P1]
    #[tokio::test]
    async fn researcher_strike_crud_round_trips_as_a_single_cut_edl()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        let bundle = recording("/missing/source.wav", &["a", "fracture", "here"]);
        let summary = transcript_store::store_transcript(driver, &bundle, None)?;
        // Two EDLs that must stay outside the annotation surface: a
        // model-produced one, and a researcher-EDL with two ops.
        let model_edl = transcript_store::store_layer(
            driver,
            &summary.id,
            &TranscriptLayer::Edl(crate::transcript_layers::EdlLayer {
                provenance: LayerProvenance {
                    model: "test-model".into(),
                    prompt_template: "highlight_pass".into(),
                    created_at: "2026-10-27T00:00:00Z".into(),
                },
                ops: vec![EdlEntry {
                    range: WordRange::new(0, 0),
                    op: EdlOp::Cut,
                }],
            }),
        )?;
        let multi_op = transcript_store::store_layer(
            driver,
            &summary.id,
            &TranscriptLayer::Edl(crate::transcript_layers::EdlLayer {
                provenance: LayerProvenance {
                    model: crate::transcript_layers::RESEARCHER_ANNOTATION_MODEL.into(),
                    prompt_template: crate::transcript_layers::RESEARCHER_ANNOTATION_TEMPLATE
                        .into(),
                    created_at: "2026-10-27T00:00:00Z".into(),
                },
                ops: vec![
                    EdlEntry {
                        range: WordRange::new(0, 0),
                        op: EdlOp::Cut,
                    },
                    EdlEntry {
                        range: WordRange::new(2, 2),
                        op: EdlOp::Cut,
                    },
                ],
            }),
        )?;
        let strike_request =
            |start_word: usize, end_word: usize, label: &str, note: &str, kind: Option<&str>| {
                Parameters(EductAnnotationCreateRequest {
                    transcript_id: summary.id.clone(),
                    start_word,
                    end_word,
                    label: label.into(),
                    note: note.into(),
                    kind: kind.map(str::to_string),
                })
            };
        // Create: a strike is a single-Cut EDL with blank label and note.
        let strike = payload(
            &server
                .educt_annotation_create(strike_request(1, 1, "", "", Some("strike")))
                .await?,
        );
        assert_eq!(strike["provenance"], "researcher_supplied");
        assert_eq!(strike["layer"]["kind"], "edl");
        assert_eq!(strike["layer"]["ops"][0]["op"], "cut");
        assert_eq!(strike["layer"]["ops"][0]["range"]["start_word"], 1);
        assert_eq!(strike["layer"]["ops"][0]["range"]["end_word"], 1);
        let strike_id = strike["annotation_id"]
            .as_str()
            .ok_or("annotation ID")?
            .to_string();
        // Nonblank label or note is refused naming the unannotated-cut
        // contract: strikes are unannotated cuts.
        let refused = server
            .educt_annotation_create(strike_request(1, 1, "finding", "", Some("strike")))
            .await
            .expect_err("a labeled strike is refused");
        assert!(
            refused.message.contains("no label or note"),
            "named: {}",
            refused.message
        );
        let refused = server
            .educt_annotation_create(strike_request(1, 1, "", "note", Some("strike")))
            .await
            .expect_err("a noted strike is refused");
        assert!(
            refused.message.contains("no label or note"),
            "named: {}",
            refused.message
        );
        // An unknown kind is refused naming both valid values.
        let refused = server
            .educt_annotation_create(strike_request(1, 1, "", "", Some("scribble")))
            .await
            .expect_err("an unknown kind is refused");
        assert!(
            refused.message.contains("kind must be highlight or strike"),
            "named: {}",
            refused.message
        );
        // A highlight (kind omitted) coexists.
        let highlight = payload(
            &server
                .educt_annotation_create(strike_request(0, 0, "finding", "note", None))
                .await?,
        );
        let highlight_id = highlight["annotation_id"]
            .as_str()
            .ok_or("highlight ID")?
            .to_string();
        // List: the strike and highlight are annotations; the model EDL and
        // the multi-op researcher-EDL never masquerade as strikes.
        let listed = payload(
            &server
                .educt_annotation_list(Parameters(EductAnnotationListRequest {
                    transcript_id: summary.id.clone(),
                }))
                .await?,
        );
        let annotations = listed["annotations"].as_array().ok_or("annotations")?;
        assert_eq!(annotations.len(), 2);
        assert!(
            annotations
                .iter()
                .any(|record| record["id"] == *strike_id && record["layer"]["kind"] == "edl")
        );
        assert!(
            annotations
                .iter()
                .any(|record| record["id"] == *highlight_id
                    && record["layer"]["kind"] == "highlight")
        );
        assert!(
            annotations
                .iter()
                .all(|record| record["id"] != model_edl.id && record["id"] != multi_op.id),
            "non-annotation EDLs stay outside the annotation list"
        );
        // Update dispatches on the stored kind: the strike's range moves
        // with blank label/note, and a nonblank note is refused.
        server
            .educt_annotation_update(Parameters(EductAnnotationUpdateRequest {
                transcript_id: summary.id.clone(),
                annotation_id: strike_id.clone(),
                start_word: 0,
                end_word: 1,
                label: String::new(),
                note: String::new(),
            }))
            .await?;
        // The update persisted: the stored Cut op moved to the new range.
        let persisted = payload(
            &server
                .educt_annotation_list(Parameters(EductAnnotationListRequest {
                    transcript_id: summary.id.clone(),
                }))
                .await?,
        );
        let moved = persisted["annotations"]
            .as_array()
            .ok_or("annotations")?
            .iter()
            .find(|record| record["id"] == *strike_id)
            .ok_or("updated strike")?;
        assert_eq!(moved["layer"]["ops"][0]["range"]["start_word"], 0);
        assert_eq!(moved["layer"]["ops"][0]["range"]["end_word"], 1);
        let refused = server
            .educt_annotation_update(Parameters(EductAnnotationUpdateRequest {
                transcript_id: summary.id.clone(),
                annotation_id: strike_id.clone(),
                start_word: 0,
                end_word: 1,
                label: String::new(),
                note: "annotated".into(),
            }))
            .await
            .expect_err("strikes carry no note");
        assert!(
            refused.message.contains("no label or note"),
            "named: {}",
            refused.message
        );
        // The multi-op researcher-EDL is not an annotation: update and
        // delete are refused by the stored-kind dispatch, naming the layer.
        let refused = server
            .educt_annotation_update(Parameters(EductAnnotationUpdateRequest {
                transcript_id: summary.id.clone(),
                annotation_id: multi_op.id.clone(),
                start_word: 0,
                end_word: 0,
                label: String::new(),
                note: String::new(),
            }))
            .await
            .expect_err("the multi-op EDL is not an annotation");
        assert!(
            refused.message.contains("not found"),
            "the refusal names the layer: {}",
            refused.message
        );
        let refused = server
            .educt_annotation_delete(Parameters(EductAnnotationDeleteRequest {
                transcript_id: summary.id.clone(),
                annotation_id: multi_op.id.clone(),
            }))
            .await
            .expect_err("the multi-op EDL cannot be deleted as an annotation");
        assert!(
            refused.message.contains("not found"),
            "the refusal names the layer: {}",
            refused.message
        );
        // Delete dispatches on the stored kind: the strike deletes; the
        // highlight and both EDLs remain, and the source text is untouched.
        server
            .educt_annotation_delete(Parameters(EductAnnotationDeleteRequest {
                transcript_id: summary.id.clone(),
                annotation_id: strike_id.clone(),
            }))
            .await?;
        let remaining = transcript_store::list_layers(driver, &summary.id)?;
        assert_eq!(remaining.len(), 3);
        assert!(remaining.iter().any(|record| record.id == highlight_id));
        assert!(remaining.iter().any(|record| record.id == model_edl.id));
        assert!(remaining.iter().any(|record| record.id == multi_op.id));
        let (_, original) =
            transcript_store::load_transcript(driver, &summary.id)?.ok_or("original bundle")?;
        assert_eq!(original.full_text, bundle.full_text);
        Ok(())
    }

    /// expect: An all-cut EDL refuses at the render gate with the named
    /// message — nothing renders, never the full source; the plan check
    /// precedes the ffmpeg requirement, so the refusal needs no runner.
    /// [P1]
    #[tokio::test]
    async fn all_cut_edl_render_refuses_visibly() -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        let bundle = recording("/missing/source.wav", &["a", "b", "c"]);
        let summary = transcript_store::store_transcript(driver, &bundle, None)?;
        let all_cut = TranscriptLayer::Edl(crate::transcript_layers::EdlLayer {
            provenance: LayerProvenance {
                model: "test".into(),
                prompt_template: "test".into(),
                created_at: "2026-10-27T00:00:00Z".into(),
            },
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 0),
                    op: EdlOp::Cut,
                },
                EdlEntry {
                    range: WordRange::new(1, 2),
                    op: EdlOp::Cut,
                },
            ],
        });
        let record = transcript_store::store_layer(driver, &summary.id, &all_cut)?;
        let error = server
            .educt_render_edl(Parameters(crate::types::EductRenderEdlRequest {
                transcript_id: summary.id.clone(),
                layer_id: Some(record.id),
            }))
            .await
            .expect_err("an all-cut EDL must refuse to render");
        assert!(
            error.message.contains("nothing to render"),
            "the refusal names the reason: {}",
            error.message
        );
        Ok(())
    }

    /// expect: Transcript listings carry the five-state alignment per row —
    /// original, aligned (cardinality-preserving correction), unaligned
    /// (cardinality-changing correction), re_anchored (after the
    /// realignment decision), and untimed — computed by the same
    /// working-transcript path the workspace gate uses. [P1]
    #[tokio::test]
    async fn transcript_listings_carry_alignment_for_every_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = research_server();
        let driver = &**server.gallery_store.driver();
        // Original: a timed transcript with no corrections.
        let original = transcript_store::store_transcript(
            driver,
            &recording("/missing/original.wav", &["a", "fracture", "here"]),
            None,
        )?;
        // Untimed: no word timings at all. (A correction layer over an
        // untimed source is refused at store time with NoWordTimings, so
        // the untimed state cannot be corrupted by layers — the
        // working_transcript guard is defense-in-depth.)
        let untimed = transcript_store::store_transcript(
            driver,
            &TranscriptBundle::new("/missing/untimed.wav".into(), 0.0, "no timings".into()),
            None,
        )?;
        // Aligned: a correction whose replacement keeps the word count.
        let aligned = transcript_store::store_transcript(
            driver,
            &recording("/missing/aligned.wav", &["a", "b", "c"]),
            None,
        )?;
        transcript_store::store_layer(
            driver,
            &aligned.id,
            &TranscriptLayer::Correction(crate::transcript_layers::CorrectionLayer {
                provenance: LayerProvenance {
                    model: "test".into(),
                    prompt_template: "correction_pass".into(),
                    created_at: "2026-10-27T00:00:00Z".into(),
                },
                edits: vec![crate::transcript_layers::CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "alpha".into(),
                    reason: "same token count".into(),
                }],
            }),
        )?;
        // Unaligned: a correction that changes the word count.
        let corrected = transcript_store::store_transcript(
            driver,
            &recording("/missing/unaligned.wav", &["a", "b", "c"]),
            None,
        )?;
        let correction = transcript_store::store_layer(
            driver,
            &corrected.id,
            &TranscriptLayer::Correction(crate::transcript_layers::CorrectionLayer {
                provenance: LayerProvenance {
                    model: "test".into(),
                    prompt_template: "correction_pass".into(),
                    created_at: "2026-10-27T00:00:01Z".into(),
                },
                edits: vec![crate::transcript_layers::CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "one two".into(),
                    reason: "cardinality change".into(),
                }],
            }),
        )?;
        let listed = payload(
            &server
                .educt_list_transcripts(Parameters(crate::types::EductListTranscriptsRequest {
                    media_path: None,
                    gallery_asset_id: None,
                    limit: None,
                }))
                .await?,
        );
        let rows = listed["transcripts"].as_array().ok_or("rows")?;
        let alignment_of = |id: &str| -> Result<&str, Box<dyn std::error::Error>> {
            rows.iter()
                .find(|row| row["id"] == id)
                .and_then(|row| row["alignment"].as_str())
                .ok_or_else(|| format!("no alignment for {id}").into())
        };
        assert_eq!(alignment_of(&original.id)?, "original");
        assert_eq!(alignment_of(&untimed.id)?, "untimed");
        assert_eq!(alignment_of(&aligned.id)?, "aligned");
        assert_eq!(alignment_of(&corrected.id)?, "unaligned");
        // The realignment decision moves the row to re_anchored.
        server
            .educt_realign_transcript(Parameters(crate::types::EductRealignTranscriptRequest {
                transcript_id: corrected.id.clone(),
                layer_id: Some(correction.id),
            }))
            .await?;
        let listed = payload(
            &server
                .educt_list_transcripts(Parameters(crate::types::EductListTranscriptsRequest {
                    media_path: None,
                    gallery_asset_id: None,
                    limit: None,
                }))
                .await?,
        );
        let rows = listed["transcripts"].as_array().ok_or("rows")?;
        let realigned = rows
            .iter()
            .find(|row| row["id"] == corrected.id)
            .and_then(|row| row["alignment"].as_str())
            .ok_or("no alignment after realignment")?;
        assert_eq!(realigned, "re_anchored");
        Ok(())
    }
}
