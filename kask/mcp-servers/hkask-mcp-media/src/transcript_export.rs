//! Educt transcript exports — deterministic projections of the stored
//! transcript to shareable and ingestable formats (slice 7):
//! - **SRT captions** from `TimedWord` — immutable timing ground truth;
//!   segments stay a derived view.
//! - **A CSV of every stored highlight** — with time ranges via the
//!   selection algebra (the only index→time mapping).
//! - **A CSV of derived review flags** (plan §7 Phase 1a) — paragraph
//!   blocks scored by the latest speaker layer's stored attribution
//!   confidences; unscoreable states are named, never a fabricated green.
//! - **The rendered transcript text** for corpus ingestion —
//!   repository-wide semantic search by composition (decision 8: media
//!   owns the artifacts, corpus owns the index). A corpus hit on the
//!   rendered text maps back to word ranges via `text_to_word_ranges`,
//!   closing the cross-recording loop: search finds the passage, the
//!   selection algebra turns it into a media range.

use crate::transcript::TimedWord;
use crate::transcript_layers::{EdlLayer, TranscriptLayer};
use crate::transcript_select::{SelectionError, WordRange, word_range_to_time_range};
use crate::transcript_store::LayerRecord;
use crate::{MediaError, map_media_error};
use hkask_mcp_server::server::McpToolError;
use hkask_storage::database::driver::DatabaseDriver;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TranscriptExportFormat {
    Srt,
    HighlightsCsv,
    ReviewCsv,
    CorpusText,
    WebVtt,
    WebAnnotation,
    Otio,
}

/// Every export format, in the order the tool's schema lists them.
const ALL_EXPORT_FORMATS: [TranscriptExportFormat; 7] = [
    TranscriptExportFormat::Srt,
    TranscriptExportFormat::HighlightsCsv,
    TranscriptExportFormat::ReviewCsv,
    TranscriptExportFormat::CorpusText,
    TranscriptExportFormat::WebVtt,
    TranscriptExportFormat::WebAnnotation,
    TranscriptExportFormat::Otio,
];

/// The names `educt_export` accepts: the schema enumerates this list and
/// `parse` reads the same labels, so a format cannot be accepted and
/// unlisted (as `review_csv` was) or listed and refused.
pub const EXPORT_FORMATS: [&str; 7] = [
    ALL_EXPORT_FORMATS[0].label(),
    ALL_EXPORT_FORMATS[1].label(),
    ALL_EXPORT_FORMATS[2].label(),
    ALL_EXPORT_FORMATS[3].label(),
    ALL_EXPORT_FORMATS[4].label(),
    ALL_EXPORT_FORMATS[5].label(),
    ALL_EXPORT_FORMATS[6].label(),
];

impl TranscriptExportFormat {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        ALL_EXPORT_FORMATS
            .into_iter()
            .find(|format| format.label() == value)
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::HighlightsCsv => "csv",
            Self::ReviewCsv => "csv",
            Self::CorpusText => "txt",
            Self::WebVtt => "vtt",
            Self::WebAnnotation => "json",
            Self::Otio => "otio",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::HighlightsCsv => "highlights_csv",
            Self::ReviewCsv => "review_csv",
            Self::CorpusText => "corpus_text",
            Self::WebVtt => "webvtt",
            Self::WebAnnotation => "web_annotation",
            Self::Otio => "otio",
        }
    }
}

struct DocumentPublicationCleanup {
    staged_dir: std::path::PathBuf,
    published_dir: Option<std::path::PathBuf>,
    committed: bool,
}

impl DocumentPublicationCleanup {
    fn new(staged_dir: std::path::PathBuf) -> Self {
        Self {
            staged_dir,
            published_dir: None,
            committed: false,
        }
    }

    fn mark_published(&mut self, published_dir: std::path::PathBuf) {
        self.published_dir = Some(published_dir);
    }

    fn commit(&mut self) {
        self.committed = true;
    }

    fn remove_dir(path: &std::path::Path) {
        match std::fs::remove_dir_all(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media",
                path = %path.display(),
                %error,
                "Failed to roll back transcript document publication"
            ),
        }
    }
}

impl Drop for DocumentPublicationCleanup {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        Self::remove_dir(&self.staged_dir);
        if let Some(published_dir) = self.published_dir.as_deref() {
            Self::remove_dir(published_dir);
        }
    }
}

fn document_io_error(action: &str, path: &std::path::Path, error: std::io::Error) -> McpToolError {
    map_media_error(MediaError::AssetPersistence(format!(
        "{action} {}: {error}",
        path.display()
    )))
}

/// Publish a transcript projection and its provenance metadata as one durable document unit.
pub(crate) fn publish_document<T: serde::Serialize + ?Sized>(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
    format: TranscriptExportFormat,
    content: &[u8],
    effective_params: &T,
) -> Result<serde_json::Value, McpToolError> {
    let dir = crate::assets::generated_assets_dir().join("transcript-exports");
    let media_label = crate::transcript_store::load_transcript(driver, transcript_id)
        .map_err(crate::tools::educt::map_store_error)?
        .and_then(|(summary, _)| {
            std::path::Path::new(&summary.media_path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "transcript".to_string());
    let label = format!("{media_label} {}", format.label());
    let folder = hkask_types::agent_paths::unique_child_name(
        &dir,
        &hkask_types::agent_paths::functional_dir_name(&label),
    );
    publish_document_in_dir(
        driver,
        &dir,
        &uuid::Uuid::new_v4().to_string(),
        &folder,
        transcript_id,
        format,
        content,
        effective_params,
    )
}

fn publish_document_in_dir<T: serde::Serialize + ?Sized>(
    driver: &dyn DatabaseDriver,
    dir: &std::path::Path,
    export_id: &str,
    folder: &str,
    transcript_id: &str,
    format: TranscriptExportFormat,
    content: &[u8],
    effective_params: &T,
) -> Result<serde_json::Value, McpToolError> {
    std::fs::create_dir_all(dir).map_err(|error| document_io_error("create", dir, error))?;
    let published_dir = dir.join(folder);
    let staged_dir = dir.join(format!(".{folder}.staged"));
    std::fs::create_dir(&staged_dir)
        .map_err(|error| document_io_error("create", &staged_dir, error))?;
    let mut cleanup = DocumentPublicationCleanup::new(staged_dir.clone());
    let output = published_dir.join(format!("document.{}", format.extension()));
    let metadata_path = published_dir.join("metadata.json");
    let staged_output = staged_dir.join(format!("document.{}", format.extension()));
    let staged_metadata = staged_dir.join("metadata.json");

    let mut effective_value = serde_json::to_value(effective_params).map_err(|error| {
        map_media_error(MediaError::AssetPersistence(format!(
            "serialize educt_export effective parameters: {error}"
        )))
    })?;
    let Some(effective_fields) = effective_value.as_object_mut() else {
        return Err(map_media_error(MediaError::AssetPersistence(
            "serialize educt_export effective parameters: expected an object".to_string(),
        )));
    };
    effective_fields.insert(
        "format".to_string(),
        serde_json::Value::String(format.label().to_string()),
    );
    let provenance =
        crate::media_block::provenance_for_tool("educt_export", effective_value.clone(), None);
    let created_at = hkask_types::time::now_rfc3339();
    let metadata = serde_json::json!({
        "export_id": export_id,
        "transcript_id": transcript_id,
        "format": format,
        "output": output,
        "created_at": created_at,
        "provenance": provenance,
        "effective_params": effective_value.clone(),
    });
    let metadata_bytes = serde_json::to_vec_pretty(&metadata).map_err(|error| {
        map_media_error(MediaError::AssetPersistence(format!(
            "serialize transcript export metadata: {error}"
        )))
    })?;

    std::fs::write(&staged_output, content)
        .map_err(|error| document_io_error("stage", &staged_output, error))?;
    std::fs::write(&staged_metadata, metadata_bytes)
        .map_err(|error| document_io_error("stage", &staged_metadata, error))?;
    std::fs::rename(&staged_dir, &published_dir)
        .map_err(|error| document_io_error("publish", &published_dir, error))?;
    cleanup.mark_published(published_dir.clone());
    // A rename success is not existence proof for the operator-facing
    // artifact (observed 2026-11-02: a corpus_text export reported success
    // while the output path never materialized). Verify the published
    // document exists and carries bytes before recording the export — no
    // live export path legitimately produces an empty document (SRT
    // refuses an untimed source; corpus text requires timed words; both
    // CSVs carry a header).
    let published_len = std::fs::metadata(&output)
        .map_err(|error| document_io_error("verify", &output, error))?
        .len();
    if published_len == 0 {
        return Err(map_media_error(MediaError::AssetPersistence(format!(
            "published transcript export is empty: {}",
            output.display()
        ))));
    }
    crate::transcript_store::record_export(
        driver,
        export_id,
        transcript_id,
        format.label(),
        &published_dir,
        &output,
        &metadata_path,
        &created_at,
    )
    .map_err(crate::tools::educt::map_store_error)?;

    let mut result = metadata;
    let Some(result_object) = result.as_object_mut() else {
        return Err(map_media_error(MediaError::AssetPersistence(
            "compose transcript export result: metadata must be an object".to_string(),
        )));
    };
    if let Some(effective_fields) = effective_value.as_object() {
        result_object.extend(effective_fields.clone());
    }
    result_object.insert(
        "metadata_path".to_string(),
        serde_json::json!(metadata_path),
    );
    result_object.insert("status".to_string(), serde_json::json!("exported"));
    cleanup.commit();
    Ok(result)
}

/// Maximum words per caption cue — sentence punctuation splits first; the
/// cap bounds unpunctuated runs.
const MAX_CUE_WORDS: usize = 15;

/// Cue boundary policy shared by SRT and WebVTT (S7): split at
/// sentence-ending punctuation, cap at `MAX_CUE_WORDS` words per cue.
/// Extracted from SRT's inline loop when WebVTT joined (plan S7) so both
/// caption formats cut at the same boundaries.
fn cue_groups<'a>(words: &'a [TimedWord]) -> Vec<Vec<&'a TimedWord>> {
    let mut groups = Vec::new();
    let mut cue_words: Vec<&'a TimedWord> = Vec::new();
    for word in words {
        cue_words.push(word);
        let ends_sentence =
            word.word.ends_with('.') || word.word.ends_with('!') || word.word.ends_with('?');
        if ends_sentence || cue_words.len() >= MAX_CUE_WORDS {
            groups.push(std::mem::take(&mut cue_words));
        }
    }
    if !cue_words.is_empty() {
        groups.push(cue_words);
    }
    groups
}

/// Build SRT captions from the word timings: cues split at
/// sentence-ending punctuation, capped at `MAX_CUE_WORDS` words per cue.
/// A transcript without word timings is a named degradation — captions
/// cannot anchor.
pub fn srt_from_words(words: &[TimedWord]) -> Result<String, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let mut srt = String::new();
    let mut first_word = 0;
    for (index, cue_words) in cue_groups(words).into_iter().enumerate() {
        cue_time_range(first_word, &cue_words)?;
        first_word += cue_words.len();
        append_cue(&mut srt, index + 1, &cue_words);
    }
    Ok(srt)
}

/// A cue's time range, refused when the stored timings put its end before
/// its start: the caption formats cut time ranges too, and a cue running
/// backwards is not a caption any player can show.
fn cue_time_range(
    first_word: usize,
    cue_words: &[&TimedWord],
) -> Result<Option<(u64, u64)>, SelectionError> {
    let (Some(first), Some(last)) = (cue_words.first(), cue_words.last()) else {
        return Ok(None);
    };
    if last.end_ms < first.start_ms {
        return Err(SelectionError::InvertedTimeRange {
            start_word: first_word,
            end_word: first_word + cue_words.len() - 1,
            start_ms: first.start_ms,
            end_ms: last.end_ms,
        });
    }
    Ok(Some((first.start_ms, last.end_ms)))
}

/// Append one numbered SRT cue block (index, timestamp line, text, blank
/// line). Every cue ends with exactly one blank line, so the cue count
/// equals the `"\n\n"` count in the result.
fn append_cue(srt: &mut String, index: usize, cue_words: &[&TimedWord]) {
    let (Some(first), Some(last)) = (cue_words.first(), cue_words.last()) else {
        return;
    };
    let text = cue_words
        .iter()
        .map(|word| word.word.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    srt.push_str(&index.to_string());
    srt.push('\n');
    srt.push_str(&format!(
        "{} --> {}\n",
        srt_timestamp(first.start_ms),
        srt_timestamp(last.end_ms)
    ));
    srt.push_str(&text);
    srt.push_str("\n\n");
}

/// `HH:MM:SS,mmm` — the SRT timestamp form.
fn srt_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// `HH:MM:SS.mmm` — the WebVTT timestamp form (a dot before the
/// milliseconds, unlike SRT's comma).
fn webvtt_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Build WebVTT captions with word-level karaoke timing tags (S7): cue
/// boundaries are the shared `cue_groups` policy, and every word carries
/// its `<HH:MM:SS.mmm>` paint-on timestamp — the standard form of
/// word-timed text (study §4). Same named degradation as SRT without
/// word timings.
pub fn webvtt_from_words(words: &[TimedWord]) -> Result<String, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let mut vtt = String::from("WEBVTT\n\n");
    let mut first_word = 0;
    for (index, cue_words) in cue_groups(words).into_iter().enumerate() {
        let range = cue_time_range(first_word, &cue_words)?;
        first_word += cue_words.len();
        let Some((start_ms, end_ms)) = range else {
            continue;
        };
        vtt.push_str(&(index + 1).to_string());
        vtt.push('\n');
        vtt.push_str(&format!(
            "{} --> {}\n",
            webvtt_timestamp(start_ms),
            webvtt_timestamp(end_ms)
        ));
        let text = cue_words
            .iter()
            .map(|word| format!("<{}>{}", webvtt_timestamp(word.start_ms), word.word))
            .collect::<Vec<_>>()
            .join(" ");
        vtt.push_str(&text);
        vtt.push_str("\n\n");
    }
    Ok(vtt)
}

/// The source words' rendered text (single-space joined) with each word's
/// start offset in characters — the TextPositionSelector coordinate space.
/// Character (not byte) offsets: the W3C model counts characters.
fn word_char_offsets(words: &[TimedWord]) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut offsets = Vec::with_capacity(words.len());
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            text.push(' ');
        }
        offsets.push(text.chars().count());
        text.push_str(&word.word);
    }
    (text, offsets)
}

/// Bounded quote-selector context: at most this many characters of
/// literal prefix/suffix around the exact quote.
const ANNOTATION_CONTEXT_CHARS: usize = 32;

/// The creator identity the plan pins for researcher-authored highlight
/// exports (study §4: creator `researcher_supplied`).
const ANNOTATION_CREATOR: &str = "researcher_supplied";

/// Build a W3C Web Annotation collection from every stored highlight (S7,
/// study §4, W3C REC 2017-02-23): each annotation carries BOTH selectors —
/// a `TextPositionSelector` (character offsets over the source-words text,
/// the immutable state the brittleness note demands) and a
/// `TextQuoteSelector` (exact plus bounded literal prefix/suffix) — with
/// motivation `highlighting`, plus `tagging` and a `TextualBody` when the
/// highlight carries a label. Each annotation's `created` is its layer
/// record's authoring timestamp; `base_id` is the transcript's URN base and
/// ids derive deterministically from it, so the serializer is pure over
/// its inputs. Non-highlight records are skipped defensively (the caller
/// filters).
pub fn web_annotation_collection(
    words: &[TimedWord],
    records: &[LayerRecord],
    base_id: &str,
) -> Result<serde_json::Value, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let (text, offsets) = word_char_offsets(words);
    let source_id = format!("{base_id}#source-words");
    let context = "http://www.w3.org/ns/anno.jsonld";
    let mut items = Vec::new();
    for record in records {
        let TranscriptLayer::Highlight(highlight) = &record.layer else {
            continue;
        };
        for entry in &highlight.highlights {
            // Range validity through the selection algebra — the one
            // index oracle (bounds and ordering get named failures).
            word_range_to_time_range(words, WordRange::new(entry.start_word, entry.end_word))?;
            let start = offsets[entry.start_word];
            let end = offsets[entry.end_word] + words[entry.end_word].word.chars().count();
            let exact = words[entry.start_word..=entry.end_word]
                .iter()
                .map(|word| word.word.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let prefix_start = start.saturating_sub(ANNOTATION_CONTEXT_CHARS);
            let prefix = text
                .chars()
                .skip(prefix_start)
                .take(start - prefix_start)
                .collect::<String>();
            let suffix = text
                .chars()
                .skip(end)
                .take(ANNOTATION_CONTEXT_CHARS)
                .collect::<String>();
            let has_label = !entry.label.trim().is_empty();
            let motivation = if has_label {
                serde_json::json!(["highlighting", "tagging"])
            } else {
                serde_json::json!(["highlighting"])
            };
            let mut annotation = serde_json::json!({
                "@context": context,
                "id": format!("{base_id}#annotation-{}", items.len()),
                "type": "Annotation",
                "motivation": motivation,
                "creator": {"type": "Agent", "name": ANNOTATION_CREATOR},
                "created": record.created_at,
                "target": {
                    "source": source_id,
                    "selector": [
                        {"type": "TextPositionSelector", "start": start, "end": end},
                        {
                            "type": "TextQuoteSelector",
                            "exact": exact,
                            "prefix": prefix,
                            "suffix": suffix,
                        },
                    ],
                },
            });
            if has_label {
                annotation["body"] = serde_json::json!({
                    "type": "TextualBody",
                    "value": entry.label,
                    "purpose": "tagging",
                });
            }
            items.push(annotation);
        }
    }
    Ok(serde_json::json!({
        "@context": context,
        "type": "AnnotationCollection",
        "id": format!("{base_id}#annotations"),
        "items": items,
    }))
}

/// One OTIO RationalTime: milliseconds as the value at rate 1000 — exact
/// seconds for integer-ms timings, no float loss (plan S7 defers full
/// round-trip fidelity to operator-side OTIO tooling).
fn rational_time_ms(ms: u64) -> serde_json::Value {
    serde_json::json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "value": ms as f64,
        "rate": 1000.0,
    })
}

/// Serialize a composed reel EDL to OpenTimelineIO (S7, study §4): a
/// Timeline holding one Video track whose clips are the reel's clip plan,
/// the same `edl_to_clip_plan` output `educt_render_edl` cuts, so the
/// timeline describes the media the render produces: `Cut` ops are
/// subtracted from the keeps (or from the whole transcript when the EDL
/// has no `Keep`), and word-adjacent keeps are one clip. Each clip's
/// `media_reference` targets the recording. Structural keys follow the
/// documented OTIO schema (Timeline/Stack/Track/Clip/TimeRange/
/// RationalTime/ExternalReference).
pub fn otio_timeline(
    words: &[TimedWord],
    edl: &EdlLayer,
    media_target: &str,
    name: &str,
) -> Result<serde_json::Value, SelectionError> {
    let plan = crate::transcript_select::edl_to_clip_plan(
        words,
        &crate::transcript_select::Edl {
            ops: edl.ops.clone(),
        },
    )?;
    let mut clips = Vec::new();
    for (start_ms, end_ms) in plan {
        clips.push(serde_json::json!({
            "OTIO_SCHEMA": "Clip.1",
            "name": format!("clip-{}", clips.len() + 1),
            "source_range": {
                "OTIO_SCHEMA": "TimeRange.1",
                "start_time": rational_time_ms(start_ms),
                "duration": rational_time_ms(end_ms - start_ms),
            },
            "media_reference": {
                "OTIO_SCHEMA": "ExternalReference.1",
                "target": media_target,
            },
        }));
    }
    Ok(serde_json::json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": name,
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "tracks",
            "children": [{
                "OTIO_SCHEMA": "Track.1",
                "kind": "Video",
                "name": "reel",
                "children": clips,
            }],
        },
    }))
}

/// Build a CSV of every highlight across the given highlight layers:
/// `layer_id, model, start_word, end_word, start_ms, end_ms, label, note`.
/// Time ranges come from the selection algebra; a validated layer can
/// never fail it, but the error is named if the impossible happens.
/// Non-highlight records are skipped defensively (the caller filters).
pub fn highlights_csv(
    words: &[TimedWord],
    records: &[LayerRecord],
) -> Result<String, SelectionError> {
    let mut csv = String::from("layer_id,model,start_word,end_word,start_ms,end_ms,label,note\n");
    for record in records {
        let TranscriptLayer::Highlight(highlight) = &record.layer else {
            continue;
        };
        for entry in &highlight.highlights {
            let (start_ms, end_ms) =
                word_range_to_time_range(words, WordRange::new(entry.start_word, entry.end_word))?;
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                csv_escape(&record.id),
                csv_escape(&highlight.provenance.model),
                entry.start_word,
                entry.end_word,
                start_ms,
                end_ms,
                csv_escape(&entry.label),
                csv_escape(&entry.note),
            ));
        }
    }
    Ok(csv)
}

/// Build a CSV of every derived review flag (plan §7 Phase 1a):
/// `block_index,start_word,end_word,start_ms,end_ms,min_confidence,status`.
/// Provenance (threshold and the source layer ids) lives in the export's
/// effective params, not the rows. Time ranges come from the selection
/// algebra; blocks derive over the same word count, so a mismatch is a
/// named error, never a silent row.
pub fn review_csv(
    words: &[TimedWord],
    review: &crate::transcript_review::ReviewFlags,
) -> Result<String, SelectionError> {
    let mut csv =
        String::from("block_index,start_word,end_word,start_ms,end_ms,min_confidence,status\n");
    for (index, block) in review.blocks.iter().enumerate() {
        let (start_ms, end_ms) =
            word_range_to_time_range(words, WordRange::new(block.start_word, block.end_word))?;
        let min_confidence = block
            .min_confidence
            .map(|confidence| format!("{confidence}"))
            .unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            index,
            block.start_word,
            block.end_word,
            start_ms,
            end_ms,
            min_confidence,
            block.status.as_str(),
        ));
    }
    Ok(csv)
}

/// RFC 4180 field escaping: quote when the field contains a comma, quote,
/// or newline; double embedded quotes.
fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::timed_words;
    use crate::transcript_layers::{EdlLayer, HighlightEntry, HighlightLayer, LayerProvenance};
    use crate::transcript_select::{EdlEntry, EdlOp};
    use hkask_storage::database::sqlite::SqliteDriver;

    #[test]
    fn srt_splits_cues_at_sentence_punctuation() {
        let words = timed_words(&["Hello", "world.", "Next", "one."]);
        let srt = srt_from_words(&words).expect("srt builds");
        // Two cues: "Hello world." and "Next one."
        assert_eq!(srt.matches("\n\n").count(), 2);
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:01,500\nHello world.\n"));
        assert!(srt.contains("2\n00:00:02,000 --> 00:00:03,500\nNext one.\n"));
    }

    #[test]
    fn srt_caps_unpunctuated_runs() {
        let words = timed_words(&[
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
        ]);
        let srt = srt_from_words(&words).expect("srt builds");
        // 16 words, cap 15 → two cues (15 + 1).
        assert_eq!(srt.matches("\n\n").count(), 2);
    }

    #[test]
    fn srt_timestamp_format() {
        assert_eq!(srt_timestamp(0), "00:00:00,000");
        assert_eq!(srt_timestamp(2_500), "00:00:02,500");
        assert_eq!(srt_timestamp(3_723_450), "01:02:03,450");
    }

    #[test]
    fn srt_without_word_timings_is_a_named_degradation() {
        assert_eq!(srt_from_words(&[]), Err(SelectionError::NoWordTimings));
    }

    #[test]
    fn highlights_csv_rows_carry_time_ranges_and_provenance() {
        let words = timed_words(&["alpha", "beta"]);
        let record = LayerRecord {
            id: "layer-1".to_string(),
            transcript_id: "t-1".to_string(),
            layer: TranscriptLayer::Highlight(HighlightLayer {
                provenance: LayerProvenance {
                    model: "test-model".to_string(),
                    prompt_template: "test".to_string(),
                    created_at: "2026-08-31T00:00:00Z".to_string(),
                },
                highlights: vec![HighlightEntry {
                    start_word: 0,
                    end_word: 1,
                    label: "key, \"argument\"".to_string(),
                    note: "the curve".to_string(),
                }],
            }),
            created_at: "2026-08-31T00:00:00Z".to_string(),
        };
        let csv = highlights_csv(&words, &[record]).expect("csv builds");
        assert!(csv.starts_with("layer_id,model,start_word,end_word,start_ms,end_ms,label,note\n"));
        // The label's comma and quotes are RFC 4180-escaped.
        assert!(csv.contains("\"key, \"\"argument\"\"\""));
        // The time range is the algebra's mapping of words [0,1].
        assert!(csv.contains("layer-1,test-model,0,1,0,1500"));
    }

    #[test]
    fn document_publication_failure_rolls_back_the_staged_unit()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let export_id = "forced-publication-failure";
        let published_dir = dir.path().join(export_id);
        std::fs::create_dir(&published_dir)?;
        std::fs::write(published_dir.join("sentinel"), b"pre-existing")?;
        let driver = SqliteDriver::in_memory_driver();
        let error = publish_document_in_dir(
            &*driver,
            dir.path(),
            "export-1",
            export_id,
            "transcript-1",
            TranscriptExportFormat::Srt,
            b"caption",
            &serde_json::json!({"cues": 1}),
        )
        .expect_err("directory promotion must fail");

        assert!(
            error.to_string().contains("publish") && error.to_string().contains(export_id),
            "filesystem publication cause was lost: {error}"
        );
        assert!(!dir.path().join(format!(".{export_id}.staged")).exists());
        assert!(published_dir.join("sentinel").is_file());
        assert!(!published_dir.join("document.srt").exists());
        assert!(!published_dir.join("metadata.json").exists());
        Ok(())
    }

    #[test]
    fn highlights_csv_skips_non_highlight_records_defensively() {
        let words = timed_words(&["alpha"]);
        let record = LayerRecord {
            id: "layer-1".to_string(),
            transcript_id: "t-1".to_string(),
            layer: TranscriptLayer::Paragraph(crate::transcript_layers::ParagraphLayer {
                provenance: LayerProvenance {
                    model: "m".to_string(),
                    prompt_template: "t".to_string(),
                    created_at: "2026-08-31T00:00:00Z".to_string(),
                },
                breaks_after: vec![0],
            }),
            created_at: "2026-08-31T00:00:00Z".to_string(),
        };
        let csv = highlights_csv(&words, &[record]).expect("csv builds");
        assert_eq!(csv.lines().count(), 1, "header only");
    }

    /// expect: A publish that lands an empty document refuses visibly and
    /// rolls the publication back — the operator is never handed a success
    /// record pointing at nothing (the 2026-11-02 defect).
    #[test]
    fn document_publication_refuses_an_empty_published_document()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let driver = SqliteDriver::in_memory_driver();
        let error = publish_document_in_dir(
            &*driver,
            dir.path(),
            "export-empty",
            "empty-export",
            "transcript-1",
            TranscriptExportFormat::CorpusText,
            b"",
            &serde_json::json!({"words": 3}),
        )
        .expect_err("an empty document must not publish");
        assert!(
            error.to_string().contains("empty"),
            "names the empty publication: {error}"
        );
        assert!(
            !dir.path().join("empty-export").exists(),
            "the failed publication rolled back"
        );
        assert!(!dir.path().join(".empty-export.staged").exists());
        Ok(())
    }

    /// expect: A successful publish leaves a verifiable, non-empty
    /// document at the recorded output path — the existence check the
    /// operator handoff requires (path, bytes, re-runnable read).
    #[test]
    fn document_publication_leaves_a_verifiable_document() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let driver = SqliteDriver::in_memory_driver();
        let mut bundle = crate::transcript::TranscriptBundle::new(
            "/missing/verified.wav".into(),
            2.0,
            "alpha beta".to_string(),
        );
        bundle.words = timed_words(&["alpha", "beta"]);
        let summary = crate::transcript_store::store_transcript(&*driver, &bundle, None)?;
        let result = publish_document_in_dir(
            &*driver,
            dir.path(),
            "export-ok",
            "verified-export",
            &summary.id,
            TranscriptExportFormat::CorpusText,
            b"rendered transcript text",
            &serde_json::json!({"words": 2}),
        )?;
        let output = std::path::Path::new(
            result["output"]
                .as_str()
                .ok_or("published output path in the result")?,
        );
        assert_eq!(std::fs::read(output)?, b"rendered transcript text");
        Ok(())
    }

    // ── S7: WebVTT karaoke captions ────────────────────────────────────

    /// dcterms:identifier: `webvtt_from_words`
    /// expect: Cue boundaries mirror SRT (sentence split, word cap) with
    /// dot-millisecond timestamps and a karaoke tag before every word.
    /// [P1] Motivating: WebVTT is the standard word-timed text interchange
    /// (study §4) — future captions-in-player can reuse it.
    /// pre: timed words on the 1-second fixture spacing.
    /// post: the exact rendered document, byte-for-byte.
    #[test]
    fn webvtt_cues_carry_karaoke_tags_and_dot_millis() {
        let words = timed_words(&["Hello", "world.", "Next", "one."]);
        let vtt = webvtt_from_words(&words).expect("webvtt builds");
        assert_eq!(
            vtt,
            "WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.500\n\
             <00:00:00.000>Hello <00:00:01.000>world.\n\n\
             2\n00:00:02.000 --> 00:00:03.500\n\
             <00:00:02.000>Next <00:00:03.000>one.\n\n"
        );
    }

    #[test]
    fn webvtt_without_word_timings_is_a_named_degradation() {
        assert_eq!(webvtt_from_words(&[]), Err(SelectionError::NoWordTimings));
    }

    // ── S7: W3C Web Annotation JSON ─────────────────────────────────────

    /// dcterms:identifier: `web_annotation_collection`
    /// expect: Every highlight becomes a W3C Annotation carrying BOTH
    /// selectors — TextPositionSelector (character offsets over the
    /// source-words text) and TextQuoteSelector (exact plus bounded literal
    /// prefix/suffix) — motivation `highlighting` plus `tagging` with a body
    /// when labeled, creator `researcher_supplied` (study §4; plan S7).
    /// [P1] Motivating: highlights must interoperate with the annotation
    /// ecosystem, not just this panel.
    /// pre: source words on the fixture spacing; one labeled and one
    /// unlabeled highlight over one highlight layer.
    /// post: the exact collection value — both selectors pinned to computed
    /// offsets ("beta gamma" spans chars 6..16 of "alpha beta gamma delta").
    #[test]
    fn web_annotation_carries_both_selectors_and_motivations() {
        let words = timed_words(&["alpha", "beta", "gamma", "delta"]);
        let layer = TranscriptLayer::Highlight(HighlightLayer {
            provenance: LayerProvenance {
                model: "user".into(),
                prompt_template: "researcher_annotation".into(),
                created_at: "2026-11-06T00:00:00Z".into(),
            },
            highlights: vec![
                HighlightEntry {
                    start_word: 1,
                    end_word: 2,
                    label: "key point".into(),
                    note: String::new(),
                },
                HighlightEntry {
                    start_word: 3,
                    end_word: 3,
                    label: String::new(),
                    note: String::new(),
                },
            ],
        });
        let records = vec![LayerRecord {
            id: "layer-1".into(),
            transcript_id: "t1".into(),
            layer,
            created_at: "2026-11-06T00:00:00Z".into(),
        }];
        let collection = web_annotation_collection(&words, &records, "urn:hkask:transcript:t1")
            .expect("web annotation builds");
        let expected = serde_json::json!({
            "@context": "http://www.w3.org/ns/anno.jsonld",
            "type": "AnnotationCollection",
            "id": "urn:hkask:transcript:t1#annotations",
            "items": [
                {
                    "@context": "http://www.w3.org/ns/anno.jsonld",
                    "id": "urn:hkask:transcript:t1#annotation-0",
                    "type": "Annotation",
                    "motivation": ["highlighting", "tagging"],
                    "creator": {"type": "Agent", "name": "researcher_supplied"},
                    "created": "2026-11-06T00:00:00Z",
                    "target": {
                        "source": "urn:hkask:transcript:t1#source-words",
                        "selector": [
                            {"type": "TextPositionSelector", "start": 6, "end": 16},
                            {
                                "type": "TextQuoteSelector",
                                "exact": "beta gamma",
                                "prefix": "alpha ",
                                "suffix": " delta"
                            },
                        ],
                    },
                    "body": {"type": "TextualBody", "value": "key point", "purpose": "tagging"},
                },
                {
                    "@context": "http://www.w3.org/ns/anno.jsonld",
                    "id": "urn:hkask:transcript:t1#annotation-1",
                    "type": "Annotation",
                    "motivation": ["highlighting"],
                    "creator": {"type": "Agent", "name": "researcher_supplied"},
                    "created": "2026-11-06T00:00:00Z",
                    "target": {
                        "source": "urn:hkask:transcript:t1#source-words",
                        "selector": [
                            {"type": "TextPositionSelector", "start": 17, "end": 22},
                            {
                                "type": "TextQuoteSelector",
                                "exact": "delta",
                                "prefix": "alpha beta gamma ",
                                "suffix": "",
                            },
                        ],
                    },
                },
            ],
        });
        assert_eq!(collection, expected);
    }

    // ── S7: OpenTimelineIO reel serialization ────────────────────────

    /// dcterms:identifier: `otio_timeline`
    /// expect: A composed reel serializes to OTIO's documented schema keys
    /// (Timeline/Stack/Track/Clip/TimeRange/RationalTime/ExternalReference),
    /// one clip per Keep op in op order with source ranges from the
    /// selection algebra; Cut ops produce no clips (they are the gaps).
    /// [P1] Motivating: NLE interchange — Reduct's FCP-XML conform is this
    /// idea in product form (study §4).
    /// pre: 1-second fixture words; a Keep/Cut/Keep reel over them.
    /// post: the exact timeline value — structural keys plus source ranges
    /// (RationalTime milliseconds at rate 1000); the full OTIO round-trip
    /// stays deferred to operator-side tooling (plan S7).
    #[test]
    fn otio_reel_clips_carry_schema_keys_and_source_ranges() {
        let words = timed_words(&["a", "b", "c", "d", "e"]);
        let edl = EdlLayer {
            provenance: LayerProvenance {
                model: "user".into(),
                prompt_template: "researcher_annotation".into(),
                created_at: "2026-11-06T00:00:00Z".into(),
            },
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 1),
                    op: EdlOp::Keep,
                },
                EdlEntry {
                    range: WordRange::new(2, 2),
                    op: EdlOp::Cut,
                },
                EdlEntry {
                    range: WordRange::new(3, 4),
                    op: EdlOp::Keep,
                },
            ],
        };
        let timeline = otio_timeline(&words, &edl, "file:///recording.mp4", "reel-layer-1")
            .expect("otio builds");
        let expected = serde_json::json!({
            "OTIO_SCHEMA": "Timeline.1",
            "name": "reel-layer-1",
            "tracks": {
                "OTIO_SCHEMA": "Stack.1",
                "name": "tracks",
                "children": [{
                    "OTIO_SCHEMA": "Track.1",
                    "kind": "Video",
                    "name": "reel",
                    "children": [
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "clip-1",
                            "source_range": {
                                "OTIO_SCHEMA": "TimeRange.1",
                                "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0.0, "rate": 1000.0},
                                "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 1500.0, "rate": 1000.0},
                            },
                            "media_reference": {
                                "OTIO_SCHEMA": "ExternalReference.1",
                                "target": "file:///recording.mp4",
                            },
                        },
                        {
                            "OTIO_SCHEMA": "Clip.1",
                            "name": "clip-2",
                            "source_range": {
                                "OTIO_SCHEMA": "TimeRange.1",
                                "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 3000.0, "rate": 1000.0},
                                "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 1500.0, "rate": 1000.0},
                            },
                            "media_reference": {
                                "OTIO_SCHEMA": "ExternalReference.1",
                                "target": "file:///recording.mp4",
                            },
                        },
                    ],
                }],
            },
        });
        assert_eq!(timeline, expected);
    }
}
