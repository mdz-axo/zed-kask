//! Educt transcript store — SQLite persistence for `TranscriptBundle`s and
//! their layers, in the media server's existing DB via the `GalleryStore`
//! driver (the local authority selected by
//! `tasks/reduct-video-analysis-scaffold.md`: ground truth and typed records
//! live together, eliminating the orphan-JOIN trap across stores;
//! the corpus server is a derived, rebuildable search index, never the
//! reverse).
//!
//! Tables (owned by this module — the only place their DDL lives, per the
//! hmem schema-drift lesson):
//! - `transcripts`: one row per transcription of a media file — the bundle
//!   JSON, asset linkage, and `words_count` (the layer JOIN key).
//! - `transcript_layers`: editable annotation state owned by one transcript.
//! - `transcript_exports`: durable document outputs with live and immutable
//!   source transcript identities.
//! - `transcript_renders`: canonical gallery outputs with live and immutable
//!   transcript/EDL identities.
//!
//! Aggregate discipline: deleting a gallery Asset detaches transcripts;
//! deleting a transcript atomically removes layers and detaches, but preserves,
//! explicitly published documents and rendered media.
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
    source_gallery_asset_id TEXT,
    detached_at TEXT,
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
CREATE TABLE IF NOT EXISTS transcript_exports (
    export_id TEXT PRIMARY KEY,
    transcript_id TEXT,
    source_transcript_id TEXT NOT NULL,
    format TEXT NOT NULL,
    directory_path TEXT NOT NULL,
    document_path TEXT NOT NULL,
    metadata_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transcript_renders (
    gallery_asset_id TEXT PRIMARY KEY REFERENCES gallery_images(id) ON DELETE CASCADE,
    transcript_id TEXT,
    source_transcript_id TEXT NOT NULL,
    edl_layer_id TEXT,
    source_edl_layer_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_educt_transcripts_media_path
    ON transcripts(media_path);
CREATE INDEX IF NOT EXISTS idx_educt_transcripts_asset
    ON transcripts(gallery_asset_id);
CREATE INDEX IF NOT EXISTS idx_educt_layers_transcript
    ON transcript_layers(transcript_id);
CREATE INDEX IF NOT EXISTS idx_educt_exports_transcript
    ON transcript_exports(transcript_id);
CREATE INDEX IF NOT EXISTS idx_educt_exports_source_transcript
    ON transcript_exports(source_transcript_id);
CREATE INDEX IF NOT EXISTS idx_educt_renders_transcript
    ON transcript_renders(transcript_id);
CREATE INDEX IF NOT EXISTS idx_educt_renders_source_transcript
    ON transcript_renders(source_transcript_id);
CREATE TRIGGER IF NOT EXISTS educt_transcript_delete_relations
BEFORE DELETE ON transcripts
BEGIN
    UPDATE transcript_exports
       SET transcript_id = NULL
     WHERE transcript_id = OLD.id;
    UPDATE transcript_renders
       SET transcript_id = NULL, edl_layer_id = NULL
     WHERE transcript_id = OLD.id;
    DELETE FROM transcript_layers WHERE transcript_id = OLD.id;
END;
";

const POST_MIGRATION_SCHEMA_SQL: &str = "
CREATE INDEX IF NOT EXISTS idx_educt_transcripts_source_asset
    ON transcripts(source_gallery_asset_id);
";

const ASSET_DETACH_TRIGGER_SQL: &str = "
CREATE TRIGGER IF NOT EXISTS educt_gallery_asset_detach_transcripts
BEFORE DELETE ON gallery_images
BEGIN
    UPDATE transcripts
       SET gallery_asset_id = NULL,
           detached_at = COALESCE(detached_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE gallery_asset_id = OLD.id;
END;
";

/// Metadata view of a stored transcript (list responses; the bundle is
/// only loaded by `load_transcript`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSourceLink {
    Unlinked,
    Linked,
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSourceAvailability {
    Available,
    Missing,
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSummary {
    pub id: String,
    pub media_path: String,
    /// The currently-live gallery relationship. Asset deletion clears this
    /// atomically without deleting the transcript.
    pub gallery_asset_id: Option<String>,
    /// Immutable origin identity retained after the live gallery relationship
    /// is detached.
    pub source_gallery_asset_id: Option<String>,
    pub source_link: TranscriptSourceLink,
    pub source_availability: TranscriptSourceAvailability,
    pub detached_at: Option<String>,
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
    pub exports_preserved: usize,
    pub renders_preserved: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptExportRecord {
    pub export_id: String,
    pub transcript_id: Option<String>,
    pub source_transcript_id: String,
    pub format: String,
    pub directory_path: String,
    pub document_path: String,
    pub metadata_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptRenderRecord {
    pub gallery_asset_id: String,
    pub transcript_id: Option<String>,
    pub source_transcript_id: String,
    pub edl_layer_id: Option<String>,
    pub source_edl_layer_id: String,
    pub created_at: String,
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
    #[error("gallery asset {gallery_asset_id} not found")]
    GalleryAssetNotFound { gallery_asset_id: String },
    #[error("layer {layer_id} not found for transcript {transcript_id}")]
    LayerNotFound {
        transcript_id: String,
        layer_id: String,
    },
    #[error("inspect transcript source {path}: {source}")]
    SourceInspection {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Create and forward-update the transcript aggregate schema. Existing rows
/// retain their original gallery identity before the live link becomes
/// detachable.
pub fn ensure_schema(driver: &dyn DatabaseDriver) -> Result<(), TranscriptStoreError> {
    driver.execute_batch(SCHEMA_SQL)?;
    add_column_if_missing(driver, "transcripts", "source_gallery_asset_id", "TEXT")?;
    add_column_if_missing(driver, "transcripts", "detached_at", "TEXT")?;
    driver.execute(
        "UPDATE transcripts SET source_gallery_asset_id = gallery_asset_id \
         WHERE source_gallery_asset_id IS NULL AND gallery_asset_id IS NOT NULL",
        &[],
    )?;
    driver.execute_batch(POST_MIGRATION_SCHEMA_SQL)?;
    if table_exists(driver, "gallery_images")? {
        driver.execute(
            "UPDATE transcripts
                SET gallery_asset_id = NULL,
                    detached_at = COALESCE(detached_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
              WHERE gallery_asset_id IS NOT NULL
                AND NOT EXISTS (
                    SELECT 1 FROM gallery_images WHERE id = transcripts.gallery_asset_id
                )",
            &[],
        )?;
        driver.execute_batch(ASSET_DETACH_TRIGGER_SQL)?;
    }
    Ok(())
}

fn table_exists(driver: &dyn DatabaseDriver, table: &str) -> Result<bool, TranscriptStoreError> {
    Ok(driver
        .query_optional(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            &[DbValue::Text(table.to_string())],
        )?
        .is_some())
}

fn add_column_if_missing(
    driver: &dyn DatabaseDriver,
    table: &str,
    column: &str,
    column_type: &str,
) -> Result<(), TranscriptStoreError> {
    let rows = driver.query(&format!("PRAGMA table_info({table})"), &[])?;
    let exists = rows
        .iter()
        .any(|row| row.get_str(1).is_ok_and(|name| name == column));
    if !exists {
        driver.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {column_type};"
        ))?;
    }
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
    if let Some(gallery_asset_id) = gallery_asset_id {
        let exists = driver
            .query_optional(
                "SELECT 1 FROM gallery_images WHERE id = ?1",
                &[DbValue::Text(gallery_asset_id.to_string())],
            )?
            .is_some();
        if !exists {
            return Err(TranscriptStoreError::GalleryAssetNotFound {
                gallery_asset_id: gallery_asset_id.to_string(),
            });
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = hkask_types::time::now_rfc3339();
    let bundle_json = serde_json::to_string(bundle)
        .map_err(|e| TranscriptStoreError::Serialization(format!("bundle: {e}")))?;
    driver.execute(
        "INSERT INTO transcripts \
         (id, media_path, gallery_asset_id, source_gallery_asset_id, detached_at, \
          bundle_json, words_count, language, model, audio_duration_secs, created_at) \
         VALUES (?1, ?2, ?3, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9)",
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
        source_gallery_asset_id: gallery_asset_id.map(str::to_string),
        source_link: if gallery_asset_id.is_some() {
            TranscriptSourceLink::Linked
        } else {
            TranscriptSourceLink::Unlinked
        },
        source_availability: source_availability(&bundle.audio_path)?,
        detached_at: None,
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
        "SELECT id, media_path, gallery_asset_id, source_gallery_asset_id, detached_at, \
         bundle_json, words_count, language, model, audio_duration_secs, created_at \
         FROM transcripts WHERE id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    let Some(row) = row else {
        return Ok(None);
    };
    let bundle: TranscriptBundle = row.get_json(5)?;
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
        "SELECT id, media_path, gallery_asset_id, source_gallery_asset_id, detached_at, \
         words_count, language, model, audio_duration_secs, created_at FROM transcripts",
    );
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<DbValue> = Vec::new();
    if let Some(path) = &filter.media_path {
        params.push(DbValue::Text(path.clone()));
        clauses.push(format!("media_path = ?{}", params.len()));
    }
    if let Some(asset) = &filter.gallery_asset_id {
        params.push(DbValue::Text(asset.clone()));
        clauses.push(format!(
            "(gallery_asset_id = ?{0} OR source_gallery_asset_id = ?{0})",
            params.len()
        ));
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

pub fn count_linked_transcripts(
    driver: &dyn DatabaseDriver,
    gallery_asset_id: &str,
) -> Result<usize, TranscriptStoreError> {
    ensure_schema(driver)?;
    count_rows(driver, "transcripts", "gallery_asset_id", gallery_asset_id)
}

/// Delete one transcript aggregate. The database trigger deletes layers and
/// detaches durable exports/renders inside the same parent-row statement, so
/// a failure rolls back the complete relationship transition.
pub fn delete_transcript(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
) -> Result<DeleteCounts, TranscriptStoreError> {
    ensure_schema(driver)?;
    let layers_removed = count_rows(driver, "transcript_layers", "transcript_id", transcript_id)?;
    let exports_preserved =
        count_rows(driver, "transcript_exports", "transcript_id", transcript_id)?;
    let renders_preserved =
        count_rows(driver, "transcript_renders", "transcript_id", transcript_id)?;
    let transcripts_removed = driver.execute(
        "DELETE FROM transcripts WHERE id = ?1",
        &[DbValue::Text(transcript_id.to_string())],
    )?;
    Ok(DeleteCounts {
        transcripts_removed,
        layers_removed,
        exports_preserved,
        renders_preserved,
    })
}

fn count_rows(
    driver: &dyn DatabaseDriver,
    table: &str,
    column: &str,
    value: &str,
) -> Result<usize, TranscriptStoreError> {
    let row = driver.query_optional(
        &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
        &[DbValue::Text(value.to_string())],
    )?;
    match row {
        Some(row) => Ok(row.get_int(0)? as usize),
        None => Ok(0),
    }
}

pub fn record_export(
    driver: &dyn DatabaseDriver,
    export_id: &str,
    transcript_id: &str,
    format: &str,
    directory_path: &std::path::Path,
    document_path: &std::path::Path,
    metadata_path: &std::path::Path,
    created_at: &str,
) -> Result<TranscriptExportRecord, TranscriptStoreError> {
    ensure_schema(driver)?;
    require_transcript(driver, transcript_id)?;
    let record = TranscriptExportRecord {
        export_id: export_id.to_string(),
        transcript_id: Some(transcript_id.to_string()),
        source_transcript_id: transcript_id.to_string(),
        format: format.to_string(),
        directory_path: directory_path.to_string_lossy().into_owned(),
        document_path: document_path.to_string_lossy().into_owned(),
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        created_at: created_at.to_string(),
    };
    driver.execute(
        "INSERT INTO transcript_exports \
         (export_id, transcript_id, source_transcript_id, format, directory_path, \
          document_path, metadata_path, created_at) \
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7)",
        &[
            DbValue::Text(record.export_id.clone()),
            DbValue::Text(transcript_id.to_string()),
            DbValue::Text(record.format.clone()),
            DbValue::Text(record.directory_path.clone()),
            DbValue::Text(record.document_path.clone()),
            DbValue::Text(record.metadata_path.clone()),
            DbValue::Text(record.created_at.clone()),
        ],
    )?;
    Ok(record)
}

pub fn list_exports(
    driver: &dyn DatabaseDriver,
    source_transcript_id: &str,
) -> Result<Vec<TranscriptExportRecord>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let rows = driver.query(
        "SELECT export_id, transcript_id, source_transcript_id, format, directory_path, \
         document_path, metadata_path, created_at FROM transcript_exports \
         WHERE source_transcript_id = ?1 ORDER BY created_at, export_id",
        &[DbValue::Text(source_transcript_id.to_string())],
    )?;
    rows.iter().map(export_from_row).collect()
}

pub fn record_render(
    driver: &dyn DatabaseDriver,
    gallery_asset_id: &str,
    transcript_id: &str,
    edl_layer_id: &str,
    created_at: &str,
) -> Result<TranscriptRenderRecord, TranscriptStoreError> {
    ensure_schema(driver)?;
    require_transcript(driver, transcript_id)?;
    let layer = driver.query_optional(
        "SELECT 1 FROM transcript_layers WHERE id = ?1 AND transcript_id = ?2",
        &[
            DbValue::Text(edl_layer_id.to_string()),
            DbValue::Text(transcript_id.to_string()),
        ],
    )?;
    if layer.is_none() {
        return Err(TranscriptStoreError::LayerNotFound {
            transcript_id: transcript_id.to_string(),
            layer_id: edl_layer_id.to_string(),
        });
    }
    let asset = driver.query_optional(
        "SELECT 1 FROM gallery_images WHERE id = ?1",
        &[DbValue::Text(gallery_asset_id.to_string())],
    )?;
    if asset.is_none() {
        return Err(TranscriptStoreError::GalleryAssetNotFound {
            gallery_asset_id: gallery_asset_id.to_string(),
        });
    }
    let record = TranscriptRenderRecord {
        gallery_asset_id: gallery_asset_id.to_string(),
        transcript_id: Some(transcript_id.to_string()),
        source_transcript_id: transcript_id.to_string(),
        edl_layer_id: Some(edl_layer_id.to_string()),
        source_edl_layer_id: edl_layer_id.to_string(),
        created_at: created_at.to_string(),
    };
    driver.execute(
        "INSERT INTO transcript_renders \
         (gallery_asset_id, transcript_id, source_transcript_id, edl_layer_id, \
          source_edl_layer_id, created_at) VALUES (?1, ?2, ?2, ?3, ?3, ?4)",
        &[
            DbValue::Text(gallery_asset_id.to_string()),
            DbValue::Text(transcript_id.to_string()),
            DbValue::Text(edl_layer_id.to_string()),
            DbValue::Text(record.created_at.clone()),
        ],
    )?;
    Ok(record)
}

pub fn list_renders(
    driver: &dyn DatabaseDriver,
    source_transcript_id: &str,
) -> Result<Vec<TranscriptRenderRecord>, TranscriptStoreError> {
    ensure_schema(driver)?;
    let rows = driver.query(
        "SELECT gallery_asset_id, transcript_id, source_transcript_id, edl_layer_id, \
         source_edl_layer_id, created_at FROM transcript_renders \
         WHERE source_transcript_id = ?1 ORDER BY created_at, gallery_asset_id",
        &[DbValue::Text(source_transcript_id.to_string())],
    )?;
    rows.iter().map(render_from_row).collect()
}

pub fn get_render(
    driver: &dyn DatabaseDriver,
    gallery_asset_id: &str,
) -> Result<Option<TranscriptRenderRecord>, TranscriptStoreError> {
    ensure_schema(driver)?;
    driver
        .query_optional(
            "SELECT gallery_asset_id, transcript_id, source_transcript_id, edl_layer_id, \
             source_edl_layer_id, created_at FROM transcript_renders WHERE gallery_asset_id = ?1",
            &[DbValue::Text(gallery_asset_id.to_string())],
        )?
        .as_ref()
        .map(render_from_row)
        .transpose()
}

fn require_transcript(
    driver: &dyn DatabaseDriver,
    transcript_id: &str,
) -> Result<(), TranscriptStoreError> {
    if driver
        .query_optional(
            "SELECT 1 FROM transcripts WHERE id = ?1",
            &[DbValue::Text(transcript_id.to_string())],
        )?
        .is_none()
    {
        return Err(TranscriptStoreError::TranscriptNotFound {
            transcript_id: transcript_id.to_string(),
        });
    }
    Ok(())
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

/// Map a row (by named columns — robust against column-order drift between
/// the list and load queries) to a summary.
fn summary_from_row(row: &DbRow) -> Result<TranscriptSummary, TranscriptStoreError> {
    let words_count = row.get_named("words_count")?.as_int()? as usize;
    let media_path = row.get_named("media_path")?.as_text()?.to_string();
    let gallery_asset_id = opt_text(row, "gallery_asset_id")?;
    let source_gallery_asset_id = opt_text(row, "source_gallery_asset_id")?;
    let source_link = if gallery_asset_id.is_some() {
        TranscriptSourceLink::Linked
    } else if source_gallery_asset_id.is_some() {
        TranscriptSourceLink::Detached
    } else {
        TranscriptSourceLink::Unlinked
    };
    Ok(TranscriptSummary {
        id: row.get_named("id")?.as_text()?.to_string(),
        source_availability: source_availability(&media_path)?,
        media_path,
        gallery_asset_id,
        source_gallery_asset_id,
        source_link,
        detached_at: opt_text(row, "detached_at")?,
        words_count,
        has_word_timings: words_count > 0,
        language: opt_text(row, "language")?,
        model: opt_text(row, "model")?,
        audio_duration_secs: row.get_named("audio_duration_secs")?.as_real()?,
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
    })
}

fn export_from_row(row: &DbRow) -> Result<TranscriptExportRecord, TranscriptStoreError> {
    Ok(TranscriptExportRecord {
        export_id: row.get_named("export_id")?.as_text()?.to_string(),
        transcript_id: opt_text(row, "transcript_id")?,
        source_transcript_id: row
            .get_named("source_transcript_id")?
            .as_text()?
            .to_string(),
        format: row.get_named("format")?.as_text()?.to_string(),
        directory_path: row.get_named("directory_path")?.as_text()?.to_string(),
        document_path: row.get_named("document_path")?.as_text()?.to_string(),
        metadata_path: row.get_named("metadata_path")?.as_text()?.to_string(),
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
    })
}

fn render_from_row(row: &DbRow) -> Result<TranscriptRenderRecord, TranscriptStoreError> {
    Ok(TranscriptRenderRecord {
        gallery_asset_id: row.get_named("gallery_asset_id")?.as_text()?.to_string(),
        transcript_id: opt_text(row, "transcript_id")?,
        source_transcript_id: row
            .get_named("source_transcript_id")?
            .as_text()?
            .to_string(),
        edl_layer_id: opt_text(row, "edl_layer_id")?,
        source_edl_layer_id: row.get_named("source_edl_layer_id")?.as_text()?.to_string(),
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
    })
}

fn source_availability(
    media_path: &str,
) -> Result<TranscriptSourceAvailability, TranscriptStoreError> {
    if media_path.contains("://") && !media_path.starts_with("file://") {
        return Ok(TranscriptSourceAvailability::External);
    }
    let path = media_path.strip_prefix("file://").unwrap_or(media_path);
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(TranscriptSourceAvailability::Available),
        Ok(_) => Ok(TranscriptSourceAvailability::Missing),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(TranscriptSourceAvailability::Missing)
        }
        Err(source) => Err(TranscriptStoreError::SourceInspection {
            path: media_path.to_string(),
            source,
        }),
    }
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

    /// dcterms:identifier: `transcript_store::ensure_schema`
    /// expect: Opening an existing transcript database preserves rows and equips them with immutable source identity.
    /// [P1] Motivating: Lifecycle repair must not erase durable transcripts created before the relationship schema.
    #[test]
    fn forward_schema_preserves_and_backfills_existing_transcript_rows() {
        let driver = driver();
        driver
            .execute_batch(
                "CREATE TABLE transcripts (
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
                INSERT INTO transcripts VALUES (
                    'transcript-1', '/tmp/source.wav', 'asset-1', '{}', 0,
                    NULL, NULL, 0.0, '2026-09-01T00:00:00Z'
                );",
            )
            .expect("old transcript schema");

        ensure_schema(&*driver).expect("forward schema");

        let row = driver
            .query_optional(
                "SELECT gallery_asset_id, source_gallery_asset_id, detached_at \
                 FROM transcripts WHERE id = 'transcript-1'",
                &[],
            )
            .expect("query migrated row")
            .expect("row preserved");
        assert_eq!(row.get_str(0).expect("live asset"), "asset-1");
        assert_eq!(row.get_str(1).expect("source asset"), "asset-1");
        assert!(matches!(row.get(2).expect("detached field"), DbValue::Null));
    }

    #[test]
    fn transcript_round_trips_through_the_store() {
        let driver = driver();
        let original = bundle(3, "/tmp/a.wav");
        let summary = store_transcript(&*driver, &original, None).expect("store");
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
        use hkask_storage::gallery::{GalleryMode, GalleryStore};

        let root = tempfile::tempdir().expect("gallery root");
        let media_path = root.path().join("a.wav");
        std::fs::write(&media_path, b"source audio").expect("source media");
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        let gallery_store = GalleryStore::from_driver(driver.clone()).expect("gallery store");
        let gallery = gallery_store
            .open(
                root.path().to_str().expect("UTF-8 gallery root"),
                GalleryMode::CopyOnWrite,
            )
            .expect("open gallery");
        let asset = gallery_store
            .add_media(
                &gallery.id,
                media_path.to_str().expect("UTF-8 media path"),
                "hash",
                0,
                0,
                "wav",
                12,
                "audio",
            )
            .expect("add asset");
        store_transcript(
            &*driver,
            &bundle(2, media_path.to_str().expect("UTF-8 media path")),
            Some(&asset.id),
        )
        .expect("store a");
        store_transcript(&*driver, &bundle(2, "/tmp/b.wav"), None).expect("store b");

        let by_path = list_transcripts(
            &*driver,
            &TranscriptFilter {
                media_path: Some(media_path.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .expect("list by path");
        assert_eq!(by_path.len(), 1);
        assert_eq!(by_path[0].media_path, media_path.to_string_lossy());

        let by_asset = list_transcripts(
            &*driver,
            &TranscriptFilter {
                gallery_asset_id: Some(asset.id.clone()),
                ..Default::default()
            },
        )
        .expect("list by asset");
        assert_eq!(by_asset.len(), 1);
        assert_eq!(
            by_asset[0].gallery_asset_id.as_deref(),
            Some(asset.id.as_str())
        );

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

    /// dcterms:identifier: `transcript_store::delete_transcript`
    /// expect: Deleting a gallery asset preserves my transcript and marks its source relationship detached.
    /// [P1] Motivating: Durable editorial work survives catalog deletion without pretending its Asset still exists.
    #[test]
    fn gallery_asset_deletion_preserves_and_detaches_transcript() {
        use hkask_storage::gallery::{GalleryMode, GalleryStore};

        let root = tempfile::tempdir().expect("gallery root");
        let media_path = root.path().join("source.wav");
        std::fs::write(&media_path, b"source audio").expect("source media");
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        let gallery_store = GalleryStore::from_driver(driver.clone()).expect("gallery store");
        let gallery = gallery_store
            .open(
                root.path().to_str().expect("UTF-8 gallery root"),
                GalleryMode::CopyOnWrite,
            )
            .expect("open gallery");
        let asset = gallery_store
            .add_media(
                &gallery.id,
                media_path.to_str().expect("UTF-8 media path"),
                "hash",
                0,
                0,
                "wav",
                12,
                "audio",
            )
            .expect("add asset");
        let summary = store_transcript(
            &*driver,
            &bundle(5, media_path.to_str().expect("UTF-8 media path")),
            Some(&asset.id),
        )
        .expect("store transcript");

        gallery_store.delete_image(&asset.id).expect("delete asset");

        let (detached, _) = load_transcript(&*driver, &summary.id)
            .expect("load transcript")
            .expect("transcript preserved");
        assert_eq!(detached.gallery_asset_id, None);
        assert_eq!(
            detached.source_gallery_asset_id.as_deref(),
            Some(asset.id.as_str())
        );
        assert_eq!(detached.source_link, TranscriptSourceLink::Detached);
        assert_eq!(
            detached.source_availability,
            TranscriptSourceAvailability::Available
        );
        assert!(detached.detached_at.is_some());
    }

    /// dcterms:identifier: `transcript_store::delete_transcript`
    /// expect: Deleting a transcript removes editable layers but preserves exported documents and rendered reels with detached origins.
    /// [P1] Motivating: Explicitly published outputs survive project-state deletion without losing provenance.
    #[test]
    fn transcript_deletion_preserves_and_detaches_published_outputs() {
        use hkask_storage::gallery::{GalleryMode, GalleryStore};

        let root = tempfile::tempdir().expect("gallery root");
        let source_path = root.path().join("source.wav");
        let render_path = root.path().join("render.wav");
        std::fs::write(&source_path, b"source audio").expect("source media");
        std::fs::write(&render_path, b"rendered audio").expect("rendered media");
        let export_dir = root.path().join("transcript-exports/export-1");
        std::fs::create_dir_all(&export_dir).expect("export directory");
        let document_path = export_dir.join("document.srt");
        let metadata_path = export_dir.join("metadata.json");
        std::fs::write(&document_path, b"captions").expect("document");
        std::fs::write(&metadata_path, b"{}").expect("metadata");

        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        let gallery_store = GalleryStore::from_driver(driver.clone()).expect("gallery store");
        let gallery = gallery_store
            .open(
                root.path().to_str().expect("UTF-8 gallery root"),
                GalleryMode::CopyOnWrite,
            )
            .expect("open gallery");
        let source_asset = gallery_store
            .add_media(
                &gallery.id,
                source_path.to_str().expect("UTF-8 source path"),
                "source-hash",
                0,
                0,
                "wav",
                12,
                "audio",
            )
            .expect("add source asset");
        let render_asset = gallery_store
            .add_media(
                &gallery.id,
                render_path.to_str().expect("UTF-8 render path"),
                "render-hash",
                0,
                0,
                "wav",
                14,
                "audio",
            )
            .expect("add render asset");
        let summary = store_transcript(
            &*driver,
            &bundle(5, source_path.to_str().expect("UTF-8 source path")),
            Some(&source_asset.id),
        )
        .expect("store transcript");
        let layer = store_layer(&*driver, &summary.id, &highlight(0, 1)).expect("layer");
        record_export(
            &*driver,
            "export-1",
            &summary.id,
            "srt",
            &export_dir,
            &document_path,
            &metadata_path,
            "2026-09-14T00:00:00Z",
        )
        .expect("record export");
        record_render(
            &*driver,
            &render_asset.id,
            &summary.id,
            &layer.id,
            "2026-09-14T00:00:00Z",
        )
        .expect("record render");

        let counts = delete_transcript(&*driver, &summary.id).expect("delete transcript");

        assert_eq!(counts.transcripts_removed, 1);
        assert_eq!(counts.layers_removed, 1);
        assert_eq!(counts.exports_preserved, 1);
        assert_eq!(counts.renders_preserved, 1);
        assert!(export_dir.is_dir());
        assert!(render_path.is_file());
        let exports = list_exports(&*driver, &summary.id).expect("list exports");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].transcript_id, None);
        assert_eq!(exports[0].source_transcript_id, summary.id);
        let render = get_render(&*driver, &render_asset.id)
            .expect("get render")
            .expect("render relationship");
        assert_eq!(render.transcript_id, None);
        assert_eq!(render.edl_layer_id, None);
        assert_eq!(render.source_transcript_id, summary.id);
        assert_eq!(render.source_edl_layer_id, layer.id);
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
