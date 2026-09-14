//! Educt transcript exports — deterministic projections of the stored
//! transcript to shareable and ingestable formats (slice 7):
//! - **SRT captions** from `TimedWord` — immutable timing ground truth;
//!   segments stay a derived view.
//! - **A CSV of every stored highlight** — with time ranges via the
//!   selection algebra (the only index→time mapping).
//! - **The rendered transcript text** for corpus ingestion —
//!   repository-wide semantic search by composition (decision 8: media
//!   owns the artifacts, corpus owns the index). A corpus hit on the
//!   rendered text maps back to word ranges via `text_to_word_ranges`,
//!   closing the cross-recording loop: search finds the passage, the
//!   selection algebra turns it into a media range.

use crate::transcript::TimedWord;
use crate::transcript_layers::TranscriptLayer;
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
    CorpusText,
}

impl TranscriptExportFormat {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "srt" => Some(Self::Srt),
            "highlights_csv" => Some(Self::HighlightsCsv),
            "corpus_text" => Some(Self::CorpusText),
            _ => None,
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::HighlightsCsv => "csv",
            Self::CorpusText => "txt",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::HighlightsCsv => "highlights_csv",
            Self::CorpusText => "corpus_text",
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
    publish_document_in_dir(
        driver,
        &dir,
        &uuid::Uuid::new_v4().to_string(),
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
    transcript_id: &str,
    format: TranscriptExportFormat,
    content: &[u8],
    effective_params: &T,
) -> Result<serde_json::Value, McpToolError> {
    std::fs::create_dir_all(dir).map_err(|error| document_io_error("create", dir, error))?;
    let published_dir = dir.join(export_id);
    let staged_dir = dir.join(format!(".{export_id}.staged"));
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
        crate::media_block::Provenance::for_tool("educt_export", effective_value.clone(), None);
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

/// Maximum words per SRT cue — sentence punctuation splits first; the cap
/// bounds unpunctuated runs.
const MAX_CUE_WORDS: usize = 15;

/// Build SRT captions from the word timings: cues split at
/// sentence-ending punctuation, capped at `MAX_CUE_WORDS` words per cue.
/// A transcript without word timings is a named degradation — captions
/// cannot anchor.
pub fn srt_from_words(words: &[TimedWord]) -> Result<String, SelectionError> {
    if words.is_empty() {
        return Err(SelectionError::NoWordTimings);
    }
    let mut srt = String::new();
    let mut cue_index: usize = 1;
    let mut cue_words: Vec<&TimedWord> = Vec::new();
    for word in words {
        cue_words.push(word);
        let ends_sentence =
            word.word.ends_with('.') || word.word.ends_with('!') || word.word.ends_with('?');
        if ends_sentence || cue_words.len() >= MAX_CUE_WORDS {
            append_cue(&mut srt, cue_index, &cue_words);
            cue_index += 1;
            cue_words.clear();
        }
    }
    if !cue_words.is_empty() {
        append_cue(&mut srt, cue_index, &cue_words);
    }
    Ok(srt)
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
    use crate::transcript_layers::{HighlightEntry, HighlightLayer, LayerProvenance};
    use hkask_storage::database::sqlite::SqliteDriver;

    fn timed_words(texts: &[&str]) -> Vec<TimedWord> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| TimedWord {
                word: text.to_string(),
                start_ms: index as u64 * 1000,
                end_ms: index as u64 * 1000 + 500,
                confidence: None,
            })
            .collect()
    }

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
}
