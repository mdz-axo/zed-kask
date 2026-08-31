//! Educt transcript store — SQLite persistence for `TranscriptBundle`s and
//! their layers, in the media server's existing DB via the `GalleryStore`
//! driver (`tasks/transcript-store-design.md` §1.4: ground truth and typed
//! records live together, eliminating the orphan-JOIN trap across stores;
//! the corpus server is a derived, rebuildable search index, never the
//! reverse).
//!
//! Tables (owned by this module — the only place their DDL lives, per the
//! hmem schema-drift lesson):
//! - `transcripts`: one row per transcription of a media file — the bundle
//!   JSON, asset linkage, and `words_count` (the layer JOIN key).
//! - `transcript_layers`: one row per layer, keyed by `transcript_id`.
//!
//! Orphan discipline: `delete_transcript` removes layers before the
//! transcript, so a failure can leave a transcript without layers (valid)
//! but never a layer without its transcript; `find_orphan_layers` surfaces
//! anything that arises from out-of-band deletion rather than dropping it.
//!
//! Degradation discipline: a transcript without word timings is stored with
//! `has_word_timings: false` (surfaced in every summary) and every layer
//! store against it is rejected with the named `NoWordTimings` invariant —
//! never an empty success.

use crate::transcript::TranscriptBundle;
use crate::transcript_layers::{LayerValidationError, TranscriptLayer};
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::{DbRow, DbValue};
use serde::Serialize;

/// Schema DDL — idempotent (`IF NOT EXISTS`), run by every public function
/// so the store is bootstrap-free against any driver (file or in-memory).
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    media_path TEXT NOT NULL,
    gallery_asset_id TEXT,
    bundle_json TEXT NOT NULL,
    words_count INTEGER NOT NULL,
    language TEXT,
    model TEXT,
    audio_duration_secs REAL NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transcript_layers (
    id TEXT PRIMARY KEY,
    transcript_id TEXT NOT NULL,
    layer_kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_educt_transcripts_media_path
    ON transcripts(media_path);
CREATE INDEX IF NOT EXISTS idx_educt_transcripts_asset
    ON transcripts(gallery_asset_id);
CREATE INDEX IF NOT EXISTS idx_educt_layers_transcript
    ON transcript_layers(transcript_id);
";

/// Metadata view of a stored transcript (list responses; the bundle is
/// only loaded by `load_transcript`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSummary {
    pub id: String,
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    pub words_count: usize,
    /// False when the STT produced no word-level timings — the surfaced
    /// degradation; layers cannot anchor to such a transcript.
    pub has_word_timings: bool,
    pub language: Option<String>,
    pub model: Option<String>,
    pub audio_duration_secs: f64,
    pub created_at: String,
}

/// A stored layer with its storage keys.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayerRecord {
    pub id: String,
    pub transcript_id: String,
    pub layer: TranscriptLayer,
    pub created_at: String,
}

/// Removal counts from `delete_transcript`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DeleteCounts {
    pub transcripts_removed: usize,
    pub layers_removed: usize,
}

/// Filters for `list_transcripts`.
pub struct TranscriptFilter {
    pub media_path: Option<String>,
    pub gallery_asset_id: Option<String>,
    pub limit: usize,
}

impl Default for TranscriptFilter {
    fn default() -> Self {
        Self {
            media_path: None,
            gallery_asset_id: None,
            limit: 50,
        }
    }
}

/// Named store failures — per-variant, never a blanket internal.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptStoreError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("transcript {transcript_id} not found")]
    TranscriptNotFound { transcript_id: String },
    #[error("layer validation failed: {0}")]
    Validation(#[from] LayerValidationError),
}

/// Create the tables if they do not exist. Called by every public function
/// (idempotent, negligible cost on an existing schema) so the store works
/// against any driver without a bootstrap step.
pub fn ensure_schema(driver: &dyn DatabaseDriver) -> Result<(), TranscriptStoreError> {
    driver.execute_batch(SCHEMA_SQL)?;
    Ok(())
}

/// Persist a transcript bundle. Multiple transcriptions of the same media
/// path over time are distinct rows. A bundle without word timings is
/// stored (text/segments remain usable) with `has_word_timings: false` —
/// the surfaced degradation.
pub fn store_transcript(
    driver: &dyn DatabaseDriver,
    bundle: &TranscriptBundle,
    gallery_asset_id: Option<&str>,
) -> Result<TranscriptSummary, TranscriptStoreError> {
    ensure_schema(driver)?;
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = hkask_types::time::now_rfc3339();
    let bundle_json = serde_json::to_string(bundle)
        .map_err(|e| TranscriptStoreError::Serialization(format!("bundle: {e}")))?;
    driver.execute(
        "INSERT INTO transcripts \
         (id, media_path, gallery_asset_id, bundle_json, words_count, language, \
          model, audio_duration_secs, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        &[
            DbValue::Text(id.clone()),
            DbValue::Text(bundle.audio_path.clone()),
            opt_param(gallery_asset_id.map(str::to_string)),
            DbValue::Text(bundle_json),
            DbValue::Integer(bundle.words.len() as i64),
            opt_param(bundle.language.clone()),
            opt_param(bundle.model.clone()),
            DbValue::Real(bundle.audio_duration_secs as f64),
            DbValue::Text(created_at.clone()),
        ],
    )?;
    Ok(TranscriptSummary {
        id,
        media_path: bundle.audio_path.clone(),
        gallery_asset_id: gallery_asset_id.map(str::to_string),
        words_count: bundle.words.len(),
        has_word_timings: !bundle.words.is_empty(),
        language: bundle.language.clone(),
        model: bundle.model.clone(),
        audio_duration_secs: bundle.audio_duration_secs as f64,
        created_at,
    })
}

/// Load one transcript: its summary and the full bundle. `None` when the ID
/// is unknown (the caller surfaces that as a named not-found).
pub fn load_transcript(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
) -> Result<Option<(TranscriptSummary, TranscriptBundle)>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let row = driver.query_optional(
        "SELECT id, media_path, gallery_asset_id, bundle_json, words_count, \
         language, model, audio_duration_secs, created_at \
         FROM transcripts WHERE id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };
    let bundle: TranscriptBundle = row.get_json(3)?;
    let summary = summary_from_row(&row)?;
    Ok(Some((summary, bundle)))
}

/// List transcript summaries, newest first, optionally filtered by media
/// path or gallery asset (the asset JOIN for recall-by-asset).
pub fn list_transcripts(
    driver: &dyn DatabaseDriver,
    filter: &TranscriptFilter,
) -> Result<Vec<TranscriptSummary>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let mut sql = String::from(
        "SELECT id, media_path, gallery_asset_id, words_count, language, \
         model, audio_duration_secs, created_at FROM transcripts",
    );
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<DbValue> = Vec::new();
    if let Some(path) = &filter.media_path {
        params.push(DbValue::Text(path.clone()));
        clauses.push(format!("media_path = ?{}", params.len()));
    }
    if let Some(asset) = &filter.gallery_asset_id {
        params.push(DbValue::Text(asset.clone()));
        clauses.push(format!("gallery_asset_id = ?{}", params.len()));
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    params.push(DbValue::Integer(filter.limit as i64));
    sql.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ?{}",
        params.len()
    ));
    let rows = driver.query(&sql, &params)?;
    rows.iter().map(summary_from_row).collect()
}

/// Delete a transcript and its layers. Layers are removed first: a failure
/// between the two statements can leave a transcript without layers (a
/// valid state) but never a layer without its transcript (an orphan).
pub fn delete_transcript(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
) -> Result<DeleteCounts, TranscriptStoreError> {
    ensure_schema(driver)?;
    let layers_removed = driver.execute(
        "DELETE FROM transcript_layers WHERE transcript_id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    let transcripts_removed = driver.execute(
        "DELETE FROM transcripts WHERE id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    Ok(DeleteCounts {
        transcripts_removed,
        layers_removed,
    })
}

/// Store a layer over a transcript. The layer is validated against the
/// transcript's stored `words_count` first (the layer↔transcript JOIN): a
/// layer whose transcript is gone is refused with a named not-found, never
/// orphaned; a layer that fails validation is rejected with the named
/// invariant and nothing is persisted.
pub fn store_layer(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
    layer: &TranscriptLayer,
) -> Result<LayerRecord, TranscriptStoreError> {
    ensure_schema(driver)?;
    let row = driver.query_optional(
        "SELECT words_count FROM transcripts WHERE id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    let words_count: usize = match row {
        Some(row) => row.get_int(0)? as usize,
        None => {
            return Err(TranscriptStoreError::TranscriptNotFound {
                transcript_id: transcript_id.to_string(),
            });
        }
    };
    layer.validate(words_count)?;
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = hkask_types::time::now_rfc3339();
    let payload_json = serde_json::to_string(layer)
        .map_err(|e| TranscriptStoreError::Serialization(format!("layer: {e}")))?;
    driver.execute(
        "INSERT INTO transcript_layers \
         (id, transcript_id, layer_kind, payload_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        &[
            DbValue::Text(id.clone()),
            DbValue::Text(transcript_id.to_string()),
            DbValue::Text(layer.kind().to_string()),
            DbValue::Text(payload_json),
            DbValue::Text(created_at.clone()),
        ],
    )?;
    Ok(LayerRecord {
        id,
        transcript_id: transcript_id.to_string(),
        layer: layer.clone(),
        created_at,
    })
}

/// List the layers stored over a transcript, oldest first.
pub fn list_layers(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
) -> Result<Vec<LayerRecord>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let rows = driver.query(
        "SELECT id, transcript_id, layer_kind, payload_json, created_at \
         FROM transcript_layers WHERE transcript_id = ?1 ORDER BY created_at",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    rows.iter().map(layer_record_from_row).collect()
}

/// Surface layers whose transcript row is gone (out-of-band deletion) —
/// the diagnostic the recall path uses to report orphans rather than
/// silently dropping them.
pub fn find_orphan_layers(
    driver: &dyn DatabaseDriver,
) -> Result<Vec<LayerRecord>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let rows = driver.query(
        "SELECT l.id, l.transcript_id, l.layer_kind, l.payload_json, l.created_at \
         FROM transcript_layers l \
         LEFT JOIN transcripts t ON t.id = l.transcript_id \
         WHERE t.id IS NULL ORDER BY l.created_at",
        &[],
    )?;
    rows.iter().map(layer_record_from_row).collect()
}

/// Map a row (by named columns — robust against column-order drift between
/// the list and load queries) to a summary.
fn summary_from_row(row: &DbRow) -> Result<TranscriptSummary, TranscriptStoreError> {
    let words_count = row.get_named("words_count")?.as_int()? as usize;
    Ok(TranscriptSummary {
        id: row.get_named("id")?.as_text()?.to_string(),
        media_path: row.get_named("media_path")?.as_text()?.to_string(),
        gallery_asset_id: opt_text(row, "gallery_asset_id")?,
        words_count,
        has_word_timings: words_count > 0,
        language: opt_text(row, "language")?,
        model: opt_text(row, "model")?,
        audio_duration_secs: row.get_named("audio_duration_secs")?.as_real()?,
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
    })
}

/// Map a layer row to a record. The `layer_kind` column and the payload's
/// internal `kind` tag must agree — a mismatch is surfaced as a
/// serialization error, not papered over.
fn layer_record_from_row(row: &DbRow) -> Result<LayerRecord, TranscriptStoreError> {
    let layer: TranscriptLayer = row.get_json(3)?;
    let stored_kind = row.get_str(2)?;
    if stored_kind != layer.kind() {
        return Err(TranscriptStoreError::Serialization(format!(
            "layer kind column says {stored_kind} but payload says {}",
            layer.kind()
        )));
    }
    Ok(LayerRecord {
        id: row.get_str(0)?.to_string(),
        transcript_id: row.get_str(1)?.to_string(),
        layer,
        created_at: row.get_str(4)?.to_string(),
    })
}

/// Optional text column → `DbValue` (Null when absent).
fn opt_param(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

/// Read an optional TEXT column.
fn opt_text(row: &DbRow, name: &str) -> Result<Option<String>, DbError> {
    match row.get_named(name)? {
        DbValue::Null => Ok(None),
        DbValue::Text(text) => Ok(Some(text.clone())),
        other => Err(DbError::Database(format!(
            "column {name}: expected text, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::TimedWord;
    use crate::transcript_layers::{EdlLayer, HighlightEntry, HighlightLayer, LayerProvenance};
    use crate::transcript_select::{EdlEntry, EdlOp, WordRange};
    use hkask_storage::database::sqlite::SqliteDriver;

    fn driver() -> std::sync::Arc<dyn DatabaseDriver> {
        SqliteDriver::in_memory_driver()
    }

    fn bundle(word_count: usize, path: &str) -> TranscriptBundle {
        let words: Vec<TimedWord> = (0..word_count)
            .map(|index| TimedWord {
                word: format!("w{index}"),
                start_ms: index as u64 * 1000,
                end_ms: index as u64 * 1000 + 500,
                confidence: None,
            })
            .collect();
        TranscriptBundle {
            words,
            ..TranscriptBundle::new(
                path.to_string(),
                word_count as f32,
                format!("{} words", word_count),
            )
        }
    }

    fn provenance() -> LayerProvenance {
        LayerProvenance {
            model: "test-model".to_string(),
            prompt_template: "test-template".to_string(),
            created_at: "2026-08-30T00:00:00Z".to_string(),
        }
    }

    fn highlight(start: usize, end: usize) -> TranscriptLayer {
        TranscriptLayer::Highlight(HighlightLayer {
            provenance: provenance(),
            highlights: vec![HighlightEntry {
                start_word: start,
                end_word: end,
                label: "key moment".to_string(),
                note: String::new(),
            }],
        })
    }

    #[test]
    fn transcript_round_trips_through_the_store() {
        let driver = driver();
        let original = bundle(3, "/tmp/a.wav");
        let summary = store_transcript(&*driver, &original, Some("asset-1")).expect("store");
        let (_, loaded) = load_transcript(&*driver, &summary.id)
            .expect("load")
            .expect("present");
        // TranscriptBundle has no PartialEq — compare serialized forms.
        assert_eq!(
            serde_json::to_value(&original).expect("serialize original"),
            serde_json::to_value(&loaded).expect("serialize loaded")
        );
    }

    #[test]
    fn list_filters_by_media_path_and_asset() {
        let driver = driver();
        store_transcript(&*driver, &bundle(2, "/tmp/a.wav"), Some("asset-1")).expect("store a");
        store_transcript(&*driver, &bundle(2, "/tmp/b.wav"), None).expect("store b");

        let by_path = list_transcripts(
            &*driver,
            &TranscriptFilter {
                media_path: Some("/tmp/a.wav".to_string()),
                ..Default::default()
            },
        )
        .expect("list by path");
        assert_eq!(by_path.len(), 1);
        assert_eq!(by_path[0].media_path, "/tmp/a.wav");

        let by_asset = list_transcripts(
            &*driver,
            &TranscriptFilter {
                gallery_asset_id: Some("asset-1".to_string()),
                ..Default::default()
            },
        )
        .expect("list by asset");
        assert_eq!(by_asset.len(), 1);
        assert_eq!(by_asset[0].gallery_asset_id.as_deref(), Some("asset-1"));

        let all = list_transcripts(&*driver, &TranscriptFilter::default()).expect("list all");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn layer_round_trips_and_joins_to_its_transcript() {
        let driver = driver();
        let summary = store_transcript(&*driver, &bundle(5, "/tmp/a.wav"), None).expect("store");
        let layer = highlight(1, 3);
        let record = store_layer(&*driver, &summary.id, &layer).expect("layer stored");
        assert_eq!(record.transcript_id, summary.id);

        let listed = list_layers(&*driver, &summary.id).expect("list layers");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].layer, layer);
        assert_eq!(listed[0].id, record.id);
    }

    #[test]
    fn invalid_layer_rejected_nothing_persisted() {
        let driver = driver();
        let summary = store_transcript(&*driver, &bundle(5, "/tmp/a.wav"), None).expect("store");
        // end_word 10 is out of bounds for a 5-word transcript.
        let bad = highlight(0, 10);
        let error = store_layer(&*driver, &summary.id, &bad).expect_err("rejected");
        assert!(matches!(
            error,
            TranscriptStoreError::Validation(LayerValidationError::WordIndexOutOfBounds {
                index: 10,
                len: 5
            })
        ));
        assert!(list_layers(&*driver, &summary.id).expect("list").is_empty());
    }

    #[test]
    fn layer_on_missing_transcript_is_named_not_found() {
        let driver = driver();
        let error = store_layer(&*driver, "no-such-id", &highlight(0, 1)).expect_err("rejected");
        assert!(matches!(
            error,
            TranscriptStoreError::TranscriptNotFound { ref transcript_id }
                if transcript_id == "no-such-id"
        ));
    }

    #[test]
    fn delete_cascades_layers_first() {
        let driver = driver();
        let summary = store_transcript(&*driver, &bundle(5, "/tmp/a.wav"), None).expect("store");
        store_layer(&*driver, &summary.id, &highlight(0, 1)).expect("layer 1");
        store_layer(&*driver, &summary.id, &highlight(2, 3)).expect("layer 2");

        let counts = delete_transcript(&*driver, &summary.id).expect("delete");
        assert_eq!(counts.transcripts_removed, 1);
        assert_eq!(counts.layers_removed, 2);
        assert!(
            load_transcript(&*driver, &summary.id)
                .expect("load")
                .is_none()
        );
        assert!(list_layers(&*driver, &summary.id).expect("list").is_empty());
    }

    #[test]
    fn orphan_layers_are_surfaced_not_dropped() {
        let driver = driver();
        let summary = store_transcript(&*driver, &bundle(5, "/tmp/a.wav"), None).expect("store");
        let layer = highlight(0, 1);
        store_layer(&*driver, &summary.id, &layer).expect("layer stored");
        // Out-of-band deletion of the transcript row (bypassing the store's
        // cascade) creates the orphan the diagnostic must surface.
        driver
            .execute(
                "DELETE FROM transcripts WHERE id = ?1",
                &[DbValue::Text(summary.id.clone())],
            )
            .expect("out-of-band delete");

        let orphans = find_orphan_layers(&*driver).expect("orphans");
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].layer, layer);
        assert_eq!(orphans[0].transcript_id, summary.id);
    }

    #[test]
    fn empty_words_transcript_is_surfaced_and_rejects_layers() {
        let driver = driver();
        let degraded = bundle(0, "/tmp/silent.wav");
        let summary = store_transcript(&*driver, &degraded, None).expect("stored");
        assert_eq!(summary.words_count, 0);
        assert!(!summary.has_word_timings);

        // The degradation is named on the layer path — never empty-success.
        let error =
            store_layer(&*driver, &summary.id, &highlight(0, 0)).expect_err("layers cannot anchor");
        assert!(matches!(
            error,
            TranscriptStoreError::Validation(LayerValidationError::NoWordTimings)
        ));
    }

    #[test]
    fn edl_layer_validates_via_the_selection_algebra_at_store_time() {
        let driver = driver();
        let summary = store_transcript(&*driver, &bundle(5, "/tmp/a.wav"), None).expect("store");
        let overlapping_keeps = TranscriptLayer::Edl(EdlLayer {
            provenance: provenance(),
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 3),
                    op: EdlOp::Keep,
                },
                EdlEntry {
                    range: WordRange::new(2, 4),
                    op: EdlOp::Keep,
                },
            ],
        });
        assert!(store_layer(&*driver, &summary.id, &overlapping_keeps).is_err());
        assert!(list_layers(&*driver, &summary.id).expect("list").is_empty());
    }
}
