//! Media layer store — SQLite persistence for time-keyed annotation layers
//! over stored video/audio recordings, in the media server's existing DB via
//! the `GalleryStore` driver.
//!
//! The media sibling of the word-indexed educt transcript layers: transcript
//! layers anchor to word indices over an immutable `words` array, media layers
//! anchor to source-PTS second ranges over an immutable media file. Both
//! carry `LayerProvenance` (model, prompt template, created_at) and both
//! reject a failing layer with the named invariant — never a partial write.
//!
//! Tables (owned by this module — the only place their DDL lives, per the
//! hmem schema-drift lesson):
//! - `media_layers`: one row per layer — media linkage, kind, label,
//!   provenance, creation time.
//! - `media_layer_items`: one row per item — a source-PTS time range,
//!   searchable text, an arbitrary payload, and optional gallery frame
//!   asset references. Deleting a layer cascades its items (the FK is
//!   declared `ON DELETE CASCADE`; pooled connections run with
//!   `PRAGMA foreign_keys = ON`).
//!
//! Immutability discipline: layers are database rows only — no function in
//! this module reads or writes source media bytes. The duration anchor is
//! ffprobe-derived by the caller (the tool layer) and passed in; this module
//! never shells out.
//!
//! Validation is deterministic and total, mirroring
//! `transcript_layers::LayerValidationError`: every variant names the broken
//! invariant and the offending values, and a layer that fails is never
//! stored.

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::{DbRow, DbValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Schema DDL — idempotent (`IF NOT EXISTS`), run by every public function
/// so the store is bootstrap-free against any driver (file or in-memory).
const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS media_layers (
    id TEXT PRIMARY KEY,
    media_path TEXT NOT NULL,
    gallery_asset_id TEXT,
    kind TEXT NOT NULL,
    label TEXT,
    provenance_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    detached_at TEXT
);
CREATE TABLE IF NOT EXISTS media_layer_items (
    id TEXT PRIMARY KEY,
    layer_id TEXT NOT NULL REFERENCES media_layers(id) ON DELETE CASCADE,
    start_sec REAL NOT NULL,
    end_sec REAL,
    searchable_text TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    frame_asset_ids_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_media_layers_media_kind
    ON media_layers(media_path, kind);
CREATE INDEX IF NOT EXISTS idx_media_layer_items_layer
    ON media_layer_items(layer_id);
";

/// The asset-detach trigger — the layer-side mirror of the transcript
/// store's `educt_gallery_asset_detach_transcripts`: a deleted gallery
/// asset severs the layer's asset link and stamps the severance
/// (`detached_at`), the durable-record detach-and-surface contract —
/// never a block, never a cascade delete. The layer survives keyed to
/// its media_path; the severance is surfaced in every list/get.
const ASSET_DETACH_TRIGGER_SQL: &str = "
CREATE TRIGGER IF NOT EXISTS media_layers_gallery_asset_detach
BEFORE DELETE ON gallery_images
BEGIN
    UPDATE media_layers
       SET gallery_asset_id = NULL,
           detached_at = COALESCE(detached_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
     WHERE gallery_asset_id = OLD.id;
END;
";

/// The closed layer-kind vocabulary — what a layer asserts over its media.
/// Shot, semantic, object, ocr and audio_event layers are model-pass
/// products; researcher layers are human-authored annotations; edl layers
/// are composed edit decisions. The wire form is snake_case, matching the
/// stored `kind` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaLayerKind {
    Shot,
    Semantic,
    Object,
    Ocr,
    AudioEvent,
    Researcher,
    Edl,
}

impl MediaLayerKind {
    /// Every valid wire value, in declaration order — the closed set named
    /// in rejection messages.
    pub const VALID_VALUES: &'static [&'static str] = &[
        "shot",
        "semantic",
        "object",
        "ocr",
        "audio_event",
        "researcher",
        "edl",
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            MediaLayerKind::Shot => "shot",
            MediaLayerKind::Semantic => "semantic",
            MediaLayerKind::Object => "object",
            MediaLayerKind::Ocr => "ocr",
            MediaLayerKind::AudioEvent => "audio_event",
            MediaLayerKind::Researcher => "researcher",
            MediaLayerKind::Edl => "edl",
        }
    }

    /// Parse a stored `kind` column (or any wire value) into the enum.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "shot" => Some(MediaLayerKind::Shot),
            "semantic" => Some(MediaLayerKind::Semantic),
            "object" => Some(MediaLayerKind::Object),
            "ocr" => Some(MediaLayerKind::Ocr),
            "audio_event" => Some(MediaLayerKind::AudioEvent),
            "researcher" => Some(MediaLayerKind::Researcher),
            "edl" => Some(MediaLayerKind::Edl),
            _ => None,
        }
    }
}

/// Provenance envelope carried by every media layer — which model, which
/// prompt template, when. Mirrors `transcript_layers::LayerProvenance`; the
/// media layer store owns its own envelope so the two layer families can
/// evolve their provenance independently (the shot pass records its filter
/// and threshold parameters here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerProvenance {
    /// The model that produced the layer (an inference-catalog label, or
    /// "user" for researcher-authored annotations).
    pub model: String,
    /// The prompt template that produced it (template name or inline hash;
    /// deterministic producers use a producer-scoped template label like
    /// "ffmpeg:scdet").
    pub prompt_template: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// Producer parameters (the shot pass's select filter and threshold,
    /// a pass's sampling controls) — absent for researcher-authored layers.
    /// Persisted inside `provenance_json`, so no schema change is needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// One layer item — a time range over the source media in source-PTS
/// seconds, the text the unified search matches, an arbitrary payload, and
/// optional gallery frame asset references (`video_extract_frames` asset
/// ids, whose `source_pts_sec` should lie inside the item's range).
///
/// The payload is the decoded domain form (plain JSON); the wire request
/// carries it as `AnyJsonValue` JSON text and the tool layer decodes before
/// reaching this module.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MediaLayerItem {
    /// Range start in source-PTS seconds.
    pub start_sec: f64,
    /// Range end in source-PTS seconds; `None` is a point-in-time item.
    pub end_sec: Option<f64>,
    /// The text a lexical search matches — never blank.
    pub searchable_text: String,
    /// Arbitrary structured payload (labels, boxes, confidences, OCR text).
    pub payload: serde_json::Value,
    /// Gallery asset ids of frames this item references.
    pub frame_asset_ids: Option<Vec<String>>,
}

/// A media layer pending validation and storage. The caller ffprobes the
/// media file and supplies `duration_secs` — the validation anchor; the
/// store never shells out.
#[derive(Debug, Clone)]
pub struct NewMediaLayer {
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    pub kind: MediaLayerKind,
    pub label: Option<String>,
    pub provenance: LayerProvenance,
    /// The ffprobe-anchored source duration in seconds — the upper bound
    /// every item range is validated against.
    pub duration_secs: f64,
    pub items: Vec<MediaLayerItem>,
}

/// List filter — media path and/or kind, with a bounded limit.
#[derive(Debug, Clone)]
pub struct MediaLayerFilter {
    pub media_path: Option<String>,
    pub kind: Option<MediaLayerKind>,
    pub limit: usize,
}

impl Default for MediaLayerFilter {
    fn default() -> Self {
        Self {
            media_path: None,
            kind: None,
            limit: 50,
        }
    }
}

/// Summary view of a stored layer (list responses; items load via
/// `get_layer`).
#[derive(Debug, Clone, Serialize)]
pub struct MediaLayerSummary {
    pub id: String,
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    /// Stamped when a gallery-asset deletion severed the layer's asset
    /// link (the detach-and-surface contract) — `None` while attached.
    pub detached_at: Option<String>,
    pub kind: MediaLayerKind,
    pub label: Option<String>,
    pub provenance: LayerProvenance,
    pub items_count: usize,
    pub created_at: String,
}

/// One stored item — the storage row form of `MediaLayerItem`, keyed to its
/// layer and carrying its own stable id (the id `layers_export` passages and
/// `layers_get` resolve).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredMediaLayerItem {
    pub id: String,
    pub layer_id: String,
    #[serde(flatten)]
    pub item: MediaLayerItem,
}

/// A full stored layer with its items.
#[derive(Debug, Clone, Serialize)]
pub struct MediaLayerRecord {
    pub id: String,
    pub media_path: String,
    pub gallery_asset_id: Option<String>,
    /// Stamped when a gallery-asset deletion severed the layer's asset
    /// link (the detach-and-surface contract) — `None` while attached.
    pub detached_at: Option<String>,
    pub kind: MediaLayerKind,
    pub label: Option<String>,
    pub provenance: LayerProvenance,
    pub created_at: String,
    /// Ordered by `start_sec`, then id — a stable playback/search order.
    pub items: Vec<StoredMediaLayerItem>,
}

/// Row counts a `delete_layer` removed — the cascade receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DeleteLayerCounts {
    pub layers_removed: usize,
    pub items_removed: usize,
}

/// Named layer-validation failures — every variant names the broken
/// invariant and the offending values (reject-with-named-reason; a failing
/// layer is never partially applied or stored), mirroring
/// `transcript_layers::LayerValidationError`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LayerValidationError {
    /// `end_sec` precedes `start_sec` — the range runs backwards.
    #[error("reversed time range: start_sec {start_sec} > end_sec {end_sec}")]
    ReversedRange { start_sec: f64, end_sec: f64 },
    /// A time bound lies outside `[0, duration]` — `which` names the
    /// offending bound ("start_sec" or "end_sec") and the message carries
    /// the probed duration.
    #[error(
        "time bound out of bounds: {which} {value} is outside the probed media duration {duration_secs}s"
    )]
    TimeOutOfBounds {
        which: &'static str,
        value: f64,
        duration_secs: f64,
    },
    /// A referenced frame asset id does not exist in the gallery.
    #[error("frame asset {frame_asset_id} (item {index}) not found in the gallery")]
    FrameRefNotFound {
        frame_asset_id: String,
        index: usize,
    },
    /// The item's searchable text is blank — there is nothing to match.
    #[error("item {index} has empty searchable_text")]
    EmptySearchableText { index: usize },
}

/// Named store failures — per-variant, never a blanket internal.
#[derive(Debug, thiserror::Error)]
pub enum LayerStoreError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("layer validation failed: {0}")]
    Validation(#[from] LayerValidationError),
    #[error("gallery asset {gallery_asset_id} not found")]
    GalleryAssetNotFound { gallery_asset_id: String },
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Create the media layer schema.
pub fn ensure_schema(driver: &dyn DatabaseDriver) -> Result<(), LayerStoreError> {
    driver.execute_batch(SCHEMA_SQL)?;
    // `detached_at` ships in the CREATE TABLE for every new database; a
    // database whose media_layers table predates the column gains it
    // here — CREATE TABLE IF NOT EXISTS never alters an existing table.
    if !column_exists(driver, "media_layers", "detached_at")? {
        driver.execute("ALTER TABLE media_layers ADD COLUMN detached_at TEXT", &[])?;
    }
    // Install the detach trigger only where the gallery table exists
    // (the same guard the transcript store's ensure_schema uses) — the
    // trigger references gallery_images.
    if table_exists(driver, "gallery_images")? {
        driver.execute_batch(ASSET_DETACH_TRIGGER_SQL)?;
    }
    Ok(())
}

/// Whether `table` exists (the sqlite_master check, mirroring
/// `transcript_store::table_exists`).
fn table_exists(driver: &dyn DatabaseDriver, table: &str) -> Result<bool, LayerStoreError> {
    Ok(driver
        .query_optional(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            &[DbValue::Text(table.to_string())],
        )?
        .is_some())
}

/// Whether `table` carries `column` — the idempotent ALTER guard.
fn column_exists(
    driver: &dyn DatabaseDriver,
    table: &str,
    column: &str,
) -> Result<bool, LayerStoreError> {
    let rows = driver.query(&format!("PRAGMA table_info({table})"), &[])?;
    for row in rows.iter() {
        if row.get_named("name")?.as_text()? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Store one media layer. Every item is validated first (against the
/// caller-supplied ffprobe duration, the gallery's frame assets, and the
/// searchable-text invariant); a layer that fails any check is rejected with
/// the named invariant and nothing is persisted. Insert failures after
/// validation roll back the layer row — the FK cascade removes any items
/// already written, so a failed store leaves no partial layer.
pub fn store_layer(
    driver: &dyn DatabaseDriver,
    layer: &NewMediaLayer,
) -> Result<MediaLayerSummary, LayerStoreError> {
    ensure_schema(driver)?;
    if let Some(gallery_asset_id) = &layer.gallery_asset_id {
        let exists = driver
            .query_optional(
                "SELECT 1 FROM gallery_images WHERE id = ?1",
                &[DbValue::Text(gallery_asset_id.to_string())],
            )?
            .is_some();
        if !exists {
            return Err(LayerStoreError::GalleryAssetNotFound {
                gallery_asset_id: gallery_asset_id.to_string(),
            });
        }
    }
    validate_layer(driver, layer)?;
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = hkask_types::time::now_rfc3339();
    let provenance_json = serde_json::to_string(&layer.provenance)
        .map_err(|e| LayerStoreError::Serialization(format!("provenance: {e}")))?;
    let insert = driver.execute(
        "INSERT INTO media_layers \
         (id, media_path, gallery_asset_id, kind, label, provenance_json, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        &[
            DbValue::Text(id.clone()),
            DbValue::Text(layer.media_path.clone()),
            opt_param(layer.gallery_asset_id.clone()),
            DbValue::Text(layer.kind.as_str().to_string()),
            opt_param(layer.label.clone()),
            DbValue::Text(provenance_json),
            DbValue::Text(created_at.clone()),
        ],
    );
    if let Err(error) = insert {
        return Err(error.into());
    }
    for item in &layer.items {
        let stored = insert_item(driver, &id, item);
        if let Err(error) = stored {
            // Roll the layer row back — the FK cascade removes any items
            // already written, so a failed store never leaves a partial layer.
            if let Err(cleanup) = driver.execute(
                "DELETE FROM media_layers WHERE id = ?1",
                &[DbValue::Text(id.clone())],
            ) {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    layer_id = %id,
                    cleanup_error = %cleanup,
                    "media layer store failed mid-item and the rollback delete also failed"
                );
            }
            return Err(error);
        }
    }
    Ok(MediaLayerSummary {
        id,
        media_path: layer.media_path.clone(),
        gallery_asset_id: layer.gallery_asset_id.clone(),
        detached_at: None,
        kind: layer.kind,
        label: layer.label.clone(),
        provenance: layer.provenance.clone(),
        items_count: layer.items.len(),
        created_at,
    })
}

fn insert_item(
    driver: &dyn DatabaseDriver,
    layer_id: &str,
    item: &MediaLayerItem,
) -> Result<(), LayerStoreError> {
    let payload_json = serde_json::to_string(&item.payload)
        .map_err(|e| LayerStoreError::Serialization(format!("item payload: {e}")))?;
    let frame_asset_ids_json = match &item.frame_asset_ids {
        Some(ids) => Some(
            serde_json::to_string(ids)
                .map_err(|e| LayerStoreError::Serialization(format!("frame ids: {e}")))?,
        ),
        None => None,
    };
    driver.execute(
        "INSERT INTO media_layer_items \
         (id, layer_id, start_sec, end_sec, searchable_text, payload_json, frame_asset_ids_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        &[
            DbValue::Text(uuid::Uuid::new_v4().to_string()),
            DbValue::Text(layer_id.to_string()),
            DbValue::Real(item.start_sec),
            opt_real(item.end_sec),
            DbValue::Text(item.searchable_text.clone()),
            DbValue::Text(payload_json),
            opt_param(frame_asset_ids_json),
        ],
    )?;
    Ok(())
}

/// Validate every item against the probed duration and the gallery — the
/// complete gate runs before any row is written, so a rejected layer is
/// never partially stored.
fn validate_layer(
    driver: &dyn DatabaseDriver,
    layer: &NewMediaLayer,
) -> Result<(), LayerStoreError> {
    for (index, item) in layer.items.iter().enumerate() {
        if item.searchable_text.trim().is_empty() {
            return Err(LayerValidationError::EmptySearchableText { index }.into());
        }
        if !item.start_sec.is_finite()
            || item.start_sec < 0.0
            || item.start_sec > layer.duration_secs
        {
            return Err(LayerValidationError::TimeOutOfBounds {
                which: "start_sec",
                value: item.start_sec,
                duration_secs: layer.duration_secs,
            }
            .into());
        }
        if let Some(end_sec) = item.end_sec {
            if !end_sec.is_finite() {
                return Err(LayerValidationError::TimeOutOfBounds {
                    which: "end_sec",
                    value: end_sec,
                    duration_secs: layer.duration_secs,
                }
                .into());
            }
            if end_sec < item.start_sec {
                return Err(LayerValidationError::ReversedRange {
                    start_sec: item.start_sec,
                    end_sec,
                }
                .into());
            }
            if end_sec > layer.duration_secs {
                return Err(LayerValidationError::TimeOutOfBounds {
                    which: "end_sec",
                    value: end_sec,
                    duration_secs: layer.duration_secs,
                }
                .into());
            }
        }
        for frame_asset_id in item.frame_asset_ids.iter().flatten() {
            let exists = driver
                .query_optional(
                    "SELECT 1 FROM gallery_images WHERE id = ?1",
                    &[DbValue::Text(frame_asset_id.to_string())],
                )?
                .is_some();
            if !exists {
                return Err(LayerValidationError::FrameRefNotFound {
                    frame_asset_id: frame_asset_id.to_string(),
                    index,
                }
                .into());
            }
        }
    }
    Ok(())
}

/// List layer summaries, newest first, optionally filtered by media path
/// and/or kind, with per-layer item counts.
pub fn list_layers(
    driver: &dyn DatabaseDriver,
    filter: &MediaLayerFilter,
) -> Result<Vec<MediaLayerSummary>, LayerStoreError> {
    ensure_schema(driver)?;
    let mut sql = String::from(
        "SELECT id, media_path, gallery_asset_id, detached_at, kind, label, provenance_json, created_at, \
         (SELECT COUNT(*) FROM media_layer_items WHERE layer_id = media_layers.id) AS items_count \
         FROM media_layers",
    );
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<DbValue> = Vec::new();
    if let Some(path) = &filter.media_path {
        params.push(DbValue::Text(path.clone()));
        clauses.push(format!("media_path = ?{}", params.len()));
    }
    if let Some(kind) = filter.kind {
        params.push(DbValue::Text(kind.as_str().to_string()));
        clauses.push(format!("kind = ?{}", params.len()));
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    params.push(DbValue::Integer(filter.limit as i64));
    sql.push_str(&format!(
        " ORDER BY created_at DESC, id DESC LIMIT ?{}",
        params.len()
    ));
    let rows = driver.query(&sql, &params)?;
    rows.iter().map(summary_from_row).collect()
}

/// Load one full layer with its items. `None` when the ID is unknown (the
/// caller surfaces that as a named not-found).
pub fn get_layer(
    driver: &dyn DatabaseDriver,
    layer_id: &str,
) -> Result<Option<MediaLayerRecord>, LayerStoreError> {
    ensure_schema(driver)?;
    let Some(row) = driver.query_optional(
        "SELECT id, media_path, gallery_asset_id, detached_at, kind, label, provenance_json, created_at \
         FROM media_layers WHERE id = ?1",
        &[DbValue::Text(layer_id.to_string())],
    )?
    else {
        return Ok(None);
    };
    let mut record = record_from_row(&row)?;
    let rows = driver.query(
        "SELECT id, layer_id, start_sec, end_sec, searchable_text, payload_json, \
         frame_asset_ids_json FROM media_layer_items WHERE layer_id = ?1 \
         ORDER BY start_sec, id",
        &[DbValue::Text(layer_id.to_string())],
    )?;
    record.items = rows
        .iter()
        .map(stored_item_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(record))
}

/// Delete one layer. The FK cascade removes its items inside the same
/// parent-row statement; the counts are taken first so the receipt reports
/// what the cascade removed.
pub fn delete_layer(
    driver: &dyn DatabaseDriver,
    layer_id: &str,
) -> Result<DeleteLayerCounts, LayerStoreError> {
    ensure_schema(driver)?;
    let items_removed = count_rows(driver, "media_layer_items", "layer_id", layer_id)?;
    let layers_removed = driver.execute(
        "DELETE FROM media_layers WHERE id = ?1",
        &[DbValue::Text(layer_id.to_string())],
    )?;
    Ok(DeleteLayerCounts {
        layers_removed,
        items_removed,
    })
}

fn count_rows(
    driver: &dyn DatabaseDriver,
    table: &str,
    column: &str,
    value: &str,
) -> Result<usize, LayerStoreError> {
    let row = driver.query_optional(
        &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
        &[DbValue::Text(value.to_string())],
    )?;
    match row {
        Some(row) => Ok(row.get_int(0)? as usize),
        None => Ok(0),
    }
}

fn summary_from_row(row: &DbRow) -> Result<MediaLayerSummary, LayerStoreError> {
    let stored_kind = row.get_named("kind")?.as_text()?;
    let kind = MediaLayerKind::parse(stored_kind).ok_or_else(|| {
        LayerStoreError::Serialization(format!(
            "layer kind column carries unknown value {stored_kind:?} (valid: {:?})",
            MediaLayerKind::VALID_VALUES
        ))
    })?;
    Ok(MediaLayerSummary {
        id: row.get_named("id")?.as_text()?.to_string(),
        media_path: row.get_named("media_path")?.as_text()?.to_string(),
        gallery_asset_id: opt_text(row, "gallery_asset_id")?,
        detached_at: opt_text(row, "detached_at")?,
        kind,
        label: opt_text(row, "label")?,
        provenance: get_json_named(row, "provenance_json")?,
        items_count: row.get_named("items_count")?.as_int()? as usize,
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
    })
}

fn record_from_row(row: &DbRow) -> Result<MediaLayerRecord, LayerStoreError> {
    let stored_kind = row.get_named("kind")?.as_text()?;
    let kind = MediaLayerKind::parse(stored_kind).ok_or_else(|| {
        LayerStoreError::Serialization(format!(
            "layer kind column carries unknown value {stored_kind:?} (valid: {:?})",
            MediaLayerKind::VALID_VALUES
        ))
    })?;
    Ok(MediaLayerRecord {
        id: row.get_named("id")?.as_text()?.to_string(),
        media_path: row.get_named("media_path")?.as_text()?.to_string(),
        gallery_asset_id: opt_text(row, "gallery_asset_id")?,
        detached_at: opt_text(row, "detached_at")?,
        kind,
        label: opt_text(row, "label")?,
        provenance: get_json_named(row, "provenance_json")?,
        created_at: row.get_named("created_at")?.as_text()?.to_string(),
        items: Vec::new(),
    })
}

fn stored_item_from_row(row: &DbRow) -> Result<StoredMediaLayerItem, LayerStoreError> {
    let frame_asset_ids = match opt_text(row, "frame_asset_ids_json")? {
        Some(json) => Some(
            serde_json::from_str(&json)
                .map_err(|e| LayerStoreError::Serialization(format!("frame ids: {e}")))?,
        ),
        None => None,
    };
    Ok(StoredMediaLayerItem {
        id: row.get_named("id")?.as_text()?.to_string(),
        layer_id: row.get_named("layer_id")?.as_text()?.to_string(),
        item: MediaLayerItem {
            start_sec: row.get_named("start_sec")?.as_real()?,
            end_sec: opt_real_column(row, "end_sec")?,
            searchable_text: row.get_named("searchable_text")?.as_text()?.to_string(),
            payload: get_json_named(row, "payload_json")?,
            frame_asset_ids,
        },
    })
}

/// Optional value → `DbValue` (Null when absent).
fn opt_param(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

/// Deserialize a named TEXT column as JSON (`DbRow` exposes `get_json` by
/// index only; this resolves the name once).
fn get_json_named<T: serde::de::DeserializeOwned>(row: &DbRow, name: &str) -> Result<T, DbError> {
    let index = row
        .column_names()
        .iter()
        .position(|column| column == name)
        .ok_or_else(|| DbError::Database(format!("column '{name}' not found")))?;
    row.get_json(index)
}

fn opt_real(value: Option<f64>) -> DbValue {
    value.map(DbValue::Real).unwrap_or(DbValue::Null)
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

/// Read an optional REAL column (Null → `None`).
fn opt_real_column(row: &DbRow, name: &str) -> Result<Option<f64>, DbError> {
    match row.get_named(name)? {
        DbValue::Null => Ok(None),
        DbValue::Real(value) => Ok(Some(*value)),
        DbValue::Integer(value) => Ok(Some(*value as f64)),
        other => Err(DbError::Database(format!(
            "column {name}: expected real, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn driver() -> std::sync::Arc<dyn DatabaseDriver> {
        hkask_storage::database::sqlite::SqliteDriver::in_memory_driver()
    }

    /// The gallery-backed test preamble, mirroring `transcript_store`'s:
    /// a fresh tempdir, an in-memory pool, a copy-on-write gallery opened
    /// at the root — the setup frame-asset validation needs.
    fn gallery_root() -> (
        tempfile::TempDir,
        std::sync::Arc<hkask_storage::database::sqlite::SqliteDriver>,
        crate::gallery::GalleryStore,
        crate::gallery::store::GalleryRecord,
    ) {
        use crate::gallery::{GalleryMode, GalleryStore};
        use hkask_storage::database::sqlite::SqliteDriver;

        let root = tempfile::tempdir().expect("gallery root");
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        let gallery_store = GalleryStore::from_driver(driver.clone()).expect("gallery store");
        let gallery = gallery_store
            .open(
                root.path().to_str().expect("UTF-8 gallery root"),
                GalleryMode::CopyOnWrite,
            )
            .expect("open gallery");
        (root, driver, gallery_store, gallery)
    }

    fn add_frame_asset(
        gallery_store: &crate::gallery::GalleryStore,
        gallery_id: &str,
        path: &std::path::Path,
    ) -> crate::gallery::ImageRecord {
        gallery_store
            .add_media(
                gallery_id,
                path.to_str().expect("UTF-8 frame path"),
                "frame-hash",
                64,
                64,
                "jpg",
                12,
                "image",
            )
            .expect("add frame asset")
    }

    fn provenance() -> LayerProvenance {
        LayerProvenance {
            model: "test-model".to_string(),
            prompt_template: "test-template".to_string(),
            created_at: "2026-10-27T00:00:00Z".to_string(),
            parameters: None,
        }
    }

    fn item(start_sec: f64, end_sec: Option<f64>) -> MediaLayerItem {
        MediaLayerItem {
            start_sec,
            end_sec,
            searchable_text: "a labeled moment".to_string(),
            payload: serde_json::json!({"label": "moment"}),
            frame_asset_ids: None,
        }
    }

    fn new_layer(media_path: &str, items: Vec<MediaLayerItem>) -> NewMediaLayer {
        NewMediaLayer {
            media_path: media_path.to_string(),
            gallery_asset_id: None,
            kind: MediaLayerKind::Researcher,
            label: Some("review selections".to_string()),
            provenance: provenance(),
            duration_secs: 2.0,
            items,
        }
    }

    #[test]
    fn kind_vocabulary_round_trips_and_rejects_unknown_values() {
        for value in MediaLayerKind::VALID_VALUES {
            let kind = MediaLayerKind::parse(value)
                .unwrap_or_else(|| panic!("valid kind {value} must parse"));
            assert_eq!(kind.as_str(), *value);
        }
        assert!(MediaLayerKind::parse("cloud").is_none());
        assert_eq!(MediaLayerKind::VALID_VALUES.len(), 7);
    }

    #[test]
    fn layer_round_trips_through_the_store() {
        let (root, driver, gallery_store, gallery) = gallery_root();
        let frame_path = root.path().join("frame-0001.jpg");
        std::fs::write(&frame_path, b"frame bytes").expect("frame fixture");
        let frame = add_frame_asset(&gallery_store, &gallery.id, &frame_path);
        let frame_id = frame.id;
        let item_with_frame = MediaLayerItem {
            frame_asset_ids: Some(vec![frame_id.clone()]),
            ..item(0.0, Some(0.5))
        };
        let stored = store_layer(
            &*driver,
            &new_layer("/tmp/a.mp4", vec![item_with_frame, item(1.0, Some(1.5))]),
        )
        .expect("store");
        assert_eq!(stored.items_count, 2);
        assert_eq!(stored.kind, MediaLayerKind::Researcher);
        let record = get_layer(&*driver, &stored.id)
            .expect("get")
            .expect("present");
        assert_eq!(record.items.len(), 2);
        assert_eq!(record.items[0].item.frame_asset_ids, Some(vec![frame_id]));
        assert_eq!(record.items[1].item, item(1.0, Some(1.5)));
        assert_eq!(record.provenance, provenance());
        let listed = list_layers(
            &*driver,
            &MediaLayerFilter {
                media_path: Some("/tmp/a.mp4".to_string()),
                kind: Some(MediaLayerKind::Researcher),
                limit: 50,
            },
        )
        .expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, stored.id);
        assert_eq!(listed[0].items_count, 2);
    }

    #[test]
    fn invalid_layer_rejected_nothing_persisted() {
        let (root, driver, gallery_store, gallery) = gallery_root();
        let frame_path = root.path().join("frame-0001.jpg");
        std::fs::write(&frame_path, b"frame bytes").expect("frame fixture");
        let frame = add_frame_asset(&gallery_store, &gallery.id, &frame_path);

        let cases: Vec<(&str, MediaLayerItem, LayerValidationError)> = vec![
            (
                "reversed range",
                item(1.5, Some(0.5)),
                LayerValidationError::ReversedRange {
                    start_sec: 1.5,
                    end_sec: 0.5,
                },
            ),
            (
                "end beyond duration",
                item(1.0, Some(3.5)),
                LayerValidationError::TimeOutOfBounds {
                    which: "end_sec",
                    value: 3.5,
                    duration_secs: 2.0,
                },
            ),
            (
                "start beyond duration",
                MediaLayerItem {
                    start_sec: 2.5,
                    end_sec: None,
                    ..item(0.0, None)
                },
                LayerValidationError::TimeOutOfBounds {
                    which: "start_sec",
                    value: 2.5,
                    duration_secs: 2.0,
                },
            ),
            (
                "blank searchable text",
                MediaLayerItem {
                    searchable_text: "  ".to_string(),
                    ..item(0.0, Some(0.5))
                },
                LayerValidationError::EmptySearchableText { index: 0 },
            ),
            (
                "missing frame asset",
                MediaLayerItem {
                    frame_asset_ids: Some(vec!["missing-frame-asset".to_string()]),
                    ..item(0.0, Some(0.5))
                },
                LayerValidationError::FrameRefNotFound {
                    frame_asset_id: "missing-frame-asset".to_string(),
                    index: 0,
                },
            ),
        ];
        for (name, bad_item, expected) in cases {
            let error =
                store_layer(&*driver, &new_layer("/tmp/a.mp4", vec![bad_item])).expect_err(name);
            assert_eq!(
                error.to_string(),
                format!("layer validation failed: {expected}"),
                "{name} names the bound and the invariant"
            );
        }
        // Nothing persisted by any rejected store.
        let listed = list_layers(&*driver, &MediaLayerFilter::default()).expect("list");
        assert!(listed.is_empty(), "no rejected layer is stored");
        // The valid frame asset is untouched by the rejected stores.
        let frame_after = gallery_store
            .get_by_id(&frame.id)
            .expect("frame asset still readable");
        assert!(!frame_after.missing, "the frame asset row survives");
    }

    #[test]
    fn unknown_gallery_asset_is_named_not_found() {
        let (_root, driver, _gallery_store, _gallery) = gallery_root();
        let mut layer = new_layer("/tmp/a.mp4", vec![item(0.0, Some(0.5))]);
        layer.gallery_asset_id = Some("missing-gallery-asset".to_string());
        let error = store_layer(&*driver, &layer).expect_err("unknown asset rejected");
        assert_eq!(
            error.to_string(),
            "gallery asset missing-gallery-asset not found"
        );
    }

    /// A deleted gallery asset detaches the layer — asset link severed,
    /// severance stamped (`detached_at`), layer and items intact — the
    /// transcript store's detach-and-surface asset contract, mirrored
    /// for media layers.
    #[test]
    fn deleting_a_gallery_asset_detaches_layers_and_stamps_the_time() {
        use hkask_storage::database::driver::DatabaseDriver;

        let (root, driver, gallery_store, gallery) = gallery_root();
        let source_path = root.path().join("linked-source.jpg");
        std::fs::write(&source_path, b"source bytes").expect("source fixture");
        let asset = add_frame_asset(&gallery_store, &gallery.id, &source_path);

        let mut layer = new_layer("/tmp/a.mp4", vec![item(0.0, Some(0.5))]);
        layer.gallery_asset_id = Some(asset.id.clone());
        let stored = store_layer(&*driver, &layer).expect("store the linked layer");
        assert_eq!(stored.gallery_asset_id.as_deref(), Some(asset.id.as_str()));
        assert!(
            stored.detached_at.is_none(),
            "freshly stored layers are attached"
        );

        driver
            .execute(
                "DELETE FROM gallery_images WHERE id = ?1",
                &[DbValue::Text(asset.id.clone())],
            )
            .expect("delete the asset row (fires the detach trigger)");

        let detached = get_layer(&*driver, &stored.id)
            .expect("get the detached layer")
            .expect("the layer survives the asset deletion");
        assert_eq!(
            detached.gallery_asset_id, None,
            "the asset link is severed, not left dangling"
        );
        assert!(
            detached.detached_at.is_some(),
            "the severance is stamped: {:?}",
            detached.detached_at
        );
        assert_eq!(detached.items.len(), 1, "items survive with the layer");
    }

    #[test]
    fn deleting_a_layer_cascades_its_items() {
        let driver = driver();
        let stored = store_layer(
            &*driver,
            &new_layer("/tmp/a.mp4", vec![item(0.0, Some(0.5)), item(1.0, None)]),
        )
        .expect("store");
        let counts = delete_layer(&*driver, &stored.id).expect("delete");
        assert_eq!(counts.layers_removed, 1);
        assert_eq!(counts.items_removed, 2);
        assert!(
            get_layer(&*driver, &stored.id)
                .expect("get after delete")
                .is_none()
        );
        let listed =
            list_layers(&*driver, &MediaLayerFilter::default()).expect("list after delete");
        assert!(listed.is_empty());
        let items =
            count_rows(&*driver, "media_layer_items", "layer_id", &stored.id).expect("count items");
        assert_eq!(items, 0, "the cascade removed every item row");
    }
}
