//! Gallery storage — SQLite-backed image gallery index.
//!
//! The gallery is a lens over the filesystem, not a copy of it.
//! Images are indexed by path + hash; tags are AI-generated metadata.
//! Schema is flat — one join maximum (image → tags).
//!
//! Tables:
//! - `galleries`: root_path, policy_mode
//! - `images`: path, hash, dimensions, gallery_id
//! - `tags`: image_id, tag_type, value, confidence
//! - `face_registry`: first_name, last_name, image_id, status, notes
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::{define_driver_store, impl_from_db_error};
use hkask_types::InfrastructureError;
use hkask_types::NotFound;
use hkask_types::time::now_rfc3339;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;
#[derive(Debug, Error)]
pub enum GalleryStoreError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("{0}")]
    NotFound(NotFound),
    #[error("Invalid policy mode: {0}")]
    InvalidMode(String),
    #[error("Gallery identity conflict: {0}")]
    Conflict(String),
    #[error("Invalid gallery path: {0}")]
    InvalidPath(String),
}
impl_from_db_error!(GalleryStoreError, Infra);
/// Gallery policy mode — three states, no gray zone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum GalleryMode {
    /// Files are read-only, never modified.
    ReadOnly,
    /// Files can be edited; copies written as new images.
    CopyOnWrite,
    /// Files may be edited in-place; original data may be lost.
    Destructive,
}
impl FromStr for GalleryMode {
    type Err = GalleryStoreError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "read-only" => Ok(Self::ReadOnly),
            "copy-on-write" => Ok(Self::CopyOnWrite),
            "destructive" => Ok(Self::Destructive),
            other => Err(GalleryStoreError::InvalidMode(other.to_string())),
        }
    }
}
impl GalleryMode {
    /// Get the string representation of the face status.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P8\] Motivating: Semantic Grounding — stable gallery mode labels
    /// post: returns "active" or "inactive"
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::CopyOnWrite => "copy-on-write",
            Self::Destructive => "destructive",
        }
    }
}
/// A gallery record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryRecord {
    pub id: String,
    pub root_path: String,
    pub mode: String,
    pub image_count: u32,
    pub total_size_bytes: u64,
    pub created_at: String,
    pub updated_at: String,
}
/// An indexed image entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRecord {
    pub id: String,
    pub gallery_id: String,
    pub relative_path: String,
    pub absolute_path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub size_bytes: u64,
    pub added_at: String,
    /// Media type: "image", "video", or "audio".
    pub media_type: String,
    pub missing: bool,
    pub metadata_stale: bool,
}

/// Physical observations, never annotations. Identity is gallery + canonical path, not hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObservation {
    pub absolute_path: String,
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub size_bytes: u64,
    pub media_type: String,
}

/// Coverage is explicit: errors prohibit absence inference for the entire scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryScan {
    pub root_path: String,
    pub recursive: bool,
    pub extensions: Vec<String>,
    pub entries: Vec<AssetObservation>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconcileResult {
    pub added: u32,
    pub changed: u32,
    pub restored: u32,
    pub missing: u32,
    pub unchanged: u32,
    pub total: u64,
    /// Actual newly added/changed/restored records; callers must not infer positional ranges.
    pub analysis_assets: Vec<ImageRecord>,
}

fn database_error(error: impl std::fmt::Display) -> InfrastructureError {
    InfrastructureError::database(error.to_string())
}

/// Canonicalize existing paths; retain normalized absolute identities when files are offline.
fn asset_path(path: &str) -> Result<PathBuf, GalleryStoreError> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(GalleryStoreError::InvalidPath(path.display().to_string()));
    }
    match path.canonicalize() {
        Ok(path) if path.to_str().is_some() => Ok(path),
        Ok(path) => Err(GalleryStoreError::InvalidPath(format!("Non-UTF-8 canonical path: {}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut normalized = PathBuf::new();
            for component in path.components() {
                match component {
                    std::path::Component::ParentDir => {
                        normalized.pop();
                    }
                    std::path::Component::CurDir => {}
                    other => normalized.push(other.as_os_str()),
                }
            }
            Ok(normalized)
        }
        Err(error) => Err(GalleryStoreError::InvalidPath(format!(
            "{}: {error}",
            path.display()
        ))),
    }
}
/// A tag on an image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRecord {
    pub id: String,
    pub image_id: String,
    pub tag_type: String,
    pub value: String,
    pub confidence: f64,
    pub model_used: String,
    pub created_at: String,
}
/// A registered face in the face registry.
///
/// Maps a reference image to a person's name for facial recognition matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceRegistryRecord {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub image_id: String,
    /// Serialized face descriptor (template-produced) as raw bytes. None if
    /// not yet computed. Compared via cosine similarity on the descriptor
    /// vector for fast matching.
    #[serde(skip)]
    pub embedding: Option<Vec<u8>>,
    pub status: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A persisted workflow graph (WS-2 `WorkflowGraph` serialized to JSON).
/// Linked from `GenerationRecord.workflow_id` so a generated asset can trace
/// back to the exact multi-step pipeline that produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRecord {
    pub id: String,
    /// Serialized `WorkflowGraph` JSON (export / import / re-execute).
    pub graph_json: String,
    pub created_at: String,
}

/// An album — a named grouping of assets within a gallery. Albums are
/// metadata-only (assets stay in place on disk); an asset can be in
/// multiple albums (many-to-many via `gallery_album_members`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumRecord {
    pub id: String,
    pub gallery_id: String,
    pub name: String,
    /// Optional parent album for nested grouping (None = top-level).
    pub parent_id: Option<String>,
    pub created_at: String,
}

/// Generation lineage for a gallery image — the full context that produced
/// the asset (WS-3). Enables `gallery_reproduce` (re-run the stored op+params)
/// and `gallery_variants` (re-run with a new seed). Anti-lock-in: this is the
/// metadata that makes an asset reproducible even if the provider changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRecord {
    pub id: String,
    pub image_id: String,
    /// `MediaOp` string ("generate_image", "image_to_image", ...).
    pub op: String,
    pub prompt: Option<String>,
    pub model: Option<String>,
    /// Provider label ("fal.ai", "openrouter", ... — historical label — the
    /// asset stays viewable if the provider is later removed).
    pub provider: Option<String>,
    pub seed: Option<i64>,
    /// JSON: size, strength, duration, ... (the `MediaGenerateParams` subset
    /// needed to reproduce).
    pub params: Option<String>,
    /// FK to `gallery_workflow` if the asset came from a multi-step workflow.
    pub workflow_id: Option<String>,
    /// Lineage: the gallery image this was derived from (img2img / upscale / ...).
    pub parent_image_id: Option<String>,
    pub created_at: String,
}
define_driver_store!(GalleryStore);
impl GalleryStore {
    /// Initialize gallery tables in the database.
    /// Initialize gallery tables.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — schema for galleries, images, tags, faces
    /// pre:  conn is a valid SQLite connection
    /// post: gallery tables created if not exists
    fn init_schema(
        driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>,
    ) -> Result<(), InfrastructureError> {
        driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS galleries (
                id TEXT PRIMARY KEY,
                root_path TEXT NOT NULL UNIQUE,
                mode TEXT NOT NULL DEFAULT 'read-only',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS gallery_images (
                id TEXT PRIMARY KEY,
                gallery_id TEXT NOT NULL REFERENCES galleries(id) ON DELETE CASCADE,
                relative_path TEXT NOT NULL,
                absolute_path TEXT NOT NULL,
                hash TEXT NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                format TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                added_at TEXT NOT NULL,
                media_type TEXT NOT NULL DEFAULT 'image',
                missing INTEGER NOT NULL DEFAULT 0,
                metadata_stale INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_gallery_images_gallery
                ON gallery_images(gallery_id);
            CREATE INDEX IF NOT EXISTS idx_gallery_images_hash
                ON gallery_images(hash);
            CREATE TABLE IF NOT EXISTS gallery_tags (
                id TEXT PRIMARY KEY,
                image_id TEXT NOT NULL REFERENCES gallery_images(id) ON DELETE CASCADE,
                tag_type TEXT NOT NULL,
                value TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                model_used TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_gallery_tags_image
                ON gallery_tags(image_id);
            CREATE INDEX IF NOT EXISTS idx_gallery_tags_type
                ON gallery_tags(tag_type);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_gallery_tags_unique
                ON gallery_tags(image_id, tag_type, value);
            CREATE TABLE IF NOT EXISTS face_registry (
                id TEXT PRIMARY KEY,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                image_id TEXT NOT NULL REFERENCES gallery_images(id) ON DELETE CASCADE,
                embedding BLOB,
                status TEXT NOT NULL DEFAULT 'pending',
                notes TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(first_name, last_name, image_id)
            );
            CREATE INDEX IF NOT EXISTS idx_face_registry_status
                ON face_registry(status);
                CREATE TABLE IF NOT EXISTS gallery_workflow (id TEXT PRIMARY KEY, graph_json TEXT NOT NULL, created_at TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS gallery_generation (id TEXT PRIMARY KEY, image_id TEXT NOT NULL REFERENCES gallery_images(id) ON DELETE CASCADE, op TEXT NOT NULL, prompt TEXT, model TEXT, provider TEXT, seed INTEGER, params TEXT, workflow_id TEXT REFERENCES gallery_workflow(id) ON DELETE SET NULL, parent_image_id TEXT, created_at TEXT NOT NULL);
                CREATE INDEX IF NOT EXISTS idx_gallery_generation_image ON gallery_generation(image_id);
            CREATE TABLE IF NOT EXISTS gallery_albums (
                id TEXT PRIMARY KEY,
                gallery_id TEXT NOT NULL REFERENCES galleries(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                parent_id TEXT REFERENCES gallery_albums(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_gallery_albums_gallery
                ON gallery_albums(gallery_id);
            CREATE TABLE IF NOT EXISTS gallery_album_members (
                album_id TEXT NOT NULL REFERENCES gallery_albums(id) ON DELETE CASCADE,
                image_id TEXT NOT NULL REFERENCES gallery_images(id) ON DELETE CASCADE,
                added_at TEXT NOT NULL,
                PRIMARY KEY (album_id, image_id)
            );
            CREATE INDEX IF NOT EXISTS idx_gallery_album_members_album
                ON gallery_album_members(album_id);
            CREATE INDEX IF NOT EXISTS idx_gallery_album_members_image
                ON gallery_album_members(image_id);",
        )?;
        // One forward schema update. Never deduplicate by deleting a record: it may
        // own different tags, album memberships, face references or generation lineage.
        let pool = driver
            .sqlite_pool()
            .ok_or_else(|| database_error("GalleryStore requires SQLite"))?;
        let mut connection = pool.get().map_err(database_error)?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let columns = transaction
            .prepare("PRAGMA table_info(gallery_images)")
            .map_err(database_error)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(database_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(database_error)?;
        if !columns.iter().any(|column| column == "missing") {
            transaction
                .execute_batch(
                    "ALTER TABLE gallery_images ADD COLUMN missing INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE gallery_images ADD COLUMN metadata_stale INTEGER NOT NULL DEFAULT 0;",
                )
                .map_err(database_error)?;
            let roots = transaction
                .prepare("SELECT id, root_path FROM galleries")
                .map_err(database_error)?
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(database_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(database_error)?;
            for (gallery_id, root) in roots {
                let root = asset_path(&root).map_err(database_error)?;
                transaction
                    .execute(
                        "UPDATE galleries SET root_path = ?1 WHERE id = ?2",
                        params![root.to_string_lossy(), gallery_id],
                    )
                    .map_err(|error| {
                        database_error(format!(
                            "Gallery root identity conflict; no records removed: {error}"
                        ))
                    })?;
                let images = transaction
                    .prepare("SELECT id, absolute_path FROM gallery_images WHERE gallery_id = ?1")
                    .map_err(database_error)?
                    .query_map([&gallery_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(database_error)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(database_error)?;
                for (image_id, absolute) in images {
                    let absolute = asset_path(&absolute).map_err(database_error)?;
                    let relative = absolute.strip_prefix(&root).unwrap_or(&absolute);
                    transaction.execute("UPDATE gallery_images SET absolute_path = ?1, relative_path = ?2 WHERE id = ?3",
                        params![absolute.to_string_lossy(), relative.to_string_lossy(), image_id]).map_err(database_error)?;
                }
            }
        }
        transaction.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_gallery_images_identity ON gallery_images(gallery_id, absolute_path);")
            .map_err(|error| database_error(format!("Gallery asset identity conflict; resolve duplicate paths explicitly, no records removed: {error}")))?;
        // Counts have one read path: aggregate active rows, not cached counters.
        let columns = transaction
            .prepare("PRAGMA table_info(galleries)")
            .map_err(database_error)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(database_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(database_error)?;
        if columns.iter().any(|column| column == "image_count") {
            transaction.execute_batch("ALTER TABLE galleries DROP COLUMN image_count; ALTER TABLE galleries DROP COLUMN total_size_bytes;").map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;
        Ok(())
    }
    /// expect: Reopening a gallery restores its identity and original permissions.
    /// [P1] Motivating: User ownership survives restarts and canonical aliases.
    /// pre: root is an accessible directory
    /// post: same canonical root returns same ID and stored mode; no implicit escalation
    pub fn open(
        &self,
        root_path: &str,
        mode: GalleryMode,
    ) -> Result<GalleryRecord, GalleryStoreError> {
        let root = Path::new(root_path)
            .canonicalize()
            .map_err(|error| GalleryStoreError::InvalidPath(format!("{root_path}: {error}")))?;
        if !root.is_dir() {
            return Err(GalleryStoreError::InvalidPath(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        std::fs::read_dir(&root).map_err(|error| {
            GalleryStoreError::InvalidPath(format!("{}: {error}", root.display()))
        })?;
        let root = root.to_str().ok_or_else(|| GalleryStoreError::InvalidPath("Non-UTF-8 canonical gallery root".into()))?.to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT INTO galleries (id, root_path, mode, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(root_path) DO NOTHING",
            &[uuid::Uuid::new_v4().to_string().into(), root.clone().into(), mode.as_str().to_string().into(), now.into()],
        )?;
        let id = query_row(
            &*self.driver,
            "SELECT id FROM galleries WHERE root_path = ?1",
            &[root.into()],
            |row| Ok(row.get_str(0)?.to_string()),
        )?
        .ok_or_else(|| GalleryStoreError::Conflict("opened gallery vanished".into()))?;
        self.get(&id)
    }

    pub fn get(&self, gallery_id: &str) -> Result<GalleryRecord, GalleryStoreError> {
        query_row(&*self.driver,
            "SELECT g.id, g.root_path, g.mode, COUNT(i.id), COALESCE(SUM(i.size_bytes), 0), g.created_at, g.updated_at
             FROM galleries g LEFT JOIN gallery_images i ON i.gallery_id = g.id AND i.missing = 0
             WHERE g.id = ?1 GROUP BY g.id", &[gallery_id.to_string().into()], |row| Ok(GalleryRecord {
                id: row.get_str(0)?.into(), root_path: row.get_str(1)?.into(), mode: row.get_str(2)?.into(),
                image_count: row.get_int(3)? as u32, total_size_bytes: row.get_int(4)? as u64,
                created_at: row.get_str(5)?.into(), updated_at: row.get_str(6)?.into(),
             }))?.ok_or_else(|| GalleryStoreError::NotFound(NotFound { entity_type: "gallery".into(), id: gallery_id.into() }))
    }
    /// Add an image to the gallery index.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Add an image to a gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — add image to gallery
    /// pre:  gallery_id is valid, image data is non-empty
    /// post: image stored in gallery
    pub fn add_image(
        &self,
        gallery_id: &str,
        absolute_path: &str,
        hash: &str,
        width: u32,
        height: u32,
        format: &str,
        size_bytes: u64,
    ) -> std::result::Result<ImageRecord, GalleryStoreError> {
        self.add_media(
            gallery_id,
            absolute_path,
            hash,
            width,
            height,
            format,
            size_bytes,
            "image",
        )
    }

    /// Add a media asset (image, video, or audio) to the gallery index.
    ///
    /// For video/audio, width and height may be 0 if unknown.
    /// `media_type` is "image", "video", or "audio".
    pub fn add_media(
        &self,
        gallery_id: &str,
        absolute_path: &str,
        hash: &str,
        width: u32,
        height: u32,
        format: &str,
        size_bytes: u64,
        media_type: &str,
    ) -> std::result::Result<ImageRecord, GalleryStoreError> {
        let gallery = self.get(gallery_id)?;
        let observation = AssetObservation {
            absolute_path: absolute_path.into(),
            hash: hash.into(),
            width,
            height,
            format: format.into(),
            size_bytes,
            media_type: media_type.into(),
        };
        let pool = self
            .driver
            .sqlite_pool()
            .ok_or_else(|| database_error("GalleryStore requires SQLite"))?;
        let mut connection = pool.get().map_err(database_error)?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let record = Self::observe(&transaction, &gallery, &observation)?;
        transaction.commit().map_err(database_error)?;
        Ok(record)
    }
    fn observe(
        transaction: &rusqlite::Transaction<'_>,
        gallery: &GalleryRecord,
        observation: &AssetObservation,
    ) -> Result<ImageRecord, GalleryStoreError> {
        let absolute = asset_path(&observation.absolute_path)?;
        let relative = absolute
            .strip_prefix(&gallery.root_path)
            .unwrap_or(&absolute);
        transaction.execute(
            "INSERT INTO gallery_images (id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(gallery_id, absolute_path) DO UPDATE SET
                relative_path = excluded.relative_path, hash = excluded.hash, width = excluded.width,
                height = excluded.height, format = excluded.format, size_bytes = excluded.size_bytes,
                media_type = excluded.media_type, missing = 0,
                metadata_stale = gallery_images.metadata_stale OR gallery_images.hash != excluded.hash",
            params![uuid::Uuid::new_v4().to_string(), gallery.id, relative.to_string_lossy(), absolute.to_string_lossy(),
                observation.hash, observation.width, observation.height, observation.format, observation.size_bytes,
                now_rfc3339(), observation.media_type],
        ).map_err(database_error)?;
        transaction.query_row(
            "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
             FROM gallery_images WHERE gallery_id = ?1 AND absolute_path = ?2",
            params![gallery.id, absolute.to_string_lossy()], Self::image_from_sql_row,
        ).map_err(|error| database_error(error).into())
    }

    /// expect: A scan never destroys my annotations or guesses that unreadable files were deleted.
    /// [P1] Motivating: Durable path identity and user metadata preservation.
    /// pre: scan observations and coverage describe this gallery's canonical root
    /// post: all observations and safe missing transitions commit together or roll back together
    pub fn reconcile(
        &self,
        gallery_id: &str,
        scan: &GalleryScan,
    ) -> Result<ReconcileResult, GalleryStoreError> {
        let gallery = self.get(gallery_id)?;
        if scan.root_path != gallery.root_path {
            return Err(GalleryStoreError::Conflict(
                "scan root differs from gallery root".into(),
            ));
        }
        let pool = self
            .driver
            .sqlite_pool()
            .ok_or_else(|| database_error("GalleryStore requires SQLite"))?;
        let mut connection = pool.get().map_err(database_error)?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let previous = transaction.prepare(
            "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
             FROM gallery_images WHERE gallery_id = ?1").map_err(database_error)?
            .query_map([gallery_id], Self::image_from_sql_row).map_err(database_error)?
            .collect::<rusqlite::Result<Vec<_>>>().map_err(database_error)?;
        let mut seen = std::collections::HashSet::new();
        let mut result = ReconcileResult::default();
        for observation in &scan.entries {
            let path = asset_path(&observation.absolute_path)?;
            if !path.starts_with(&gallery.root_path) {
                return Err(GalleryStoreError::InvalidPath(format!(
                    "scan path escapes root: {}",
                    path.display()
                )));
            }
            let path = path.to_string_lossy().into_owned();
            if !seen.insert(path.clone()) {
                continue;
            }
            let old = previous.iter().find(|record| record.absolute_path == path);
            let record = Self::observe(&transaction, &gallery, observation)?;
            match old {
                None => result.added += 1,
                Some(old) if old.hash != record.hash => result.changed += 1,
                Some(old) if old.missing => result.restored += 1,
                Some(_) => result.unchanged += 1,
            }
            if old.is_none_or(|old| old.hash != record.hash || old.missing) {
                result.analysis_assets.push(record);
            }
        }
        if scan.errors.is_empty() {
            for record in previous {
                let path = Path::new(&record.absolute_path);
                let Ok(relative) = path.strip_prefix(&gallery.root_path) else {
                    continue;
                };
                let extension = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if record.media_type != "image"
                    || record.missing
                    || seen.contains(&record.absolute_path)
                    || relative
                        .components()
                        .any(|component| component.as_os_str() == ".hkask-gallery")
                    || (!scan.recursive && relative.components().count() != 1)
                    || !scan.extensions.contains(&extension)
                {
                    continue;
                }
                transaction
                    .execute(
                        "UPDATE gallery_images SET missing = 1 WHERE id = ?1",
                        [&record.id],
                    )
                    .map_err(database_error)?;
                result.missing += 1;
            }
        }
        transaction
            .execute(
                "UPDATE galleries SET updated_at = ?1 WHERE id = ?2",
                params![now_rfc3339(), gallery_id],
            )
            .map_err(database_error)?;
        result.total = transaction
            .query_row(
                "SELECT COUNT(*) FROM gallery_images WHERE gallery_id = ?1 AND missing = 0",
                [gallery_id],
                |row| row.get(0),
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(result)
    }

    /// Stable-ID inspection includes missing assets; positional readers never do.
    pub fn get_by_id(
        &self,
        gallery_id: &str,
        image_id: &str,
    ) -> Result<ImageRecord, GalleryStoreError> {
        query_row(&*self.driver,
            "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
             FROM gallery_images WHERE gallery_id = ?1 AND id = ?2",
            &[gallery_id.to_string().into(), image_id.to_string().into()], Self::image_from_row)?
            .ok_or_else(|| GalleryStoreError::NotFound(NotFound { entity_type: "image".into(), id: image_id.into() }))
    }

    /// expect: Analysis of an old revision never tags or certifies a different revision. [P1]
    /// pre: record was captured before inference
    /// post: tags and freshness commit together only while identity/hash still match
    pub fn persist_analysis(&self, record: &ImageRecord, tags: &[(String, String, f64)], model: &str, complete: bool) -> Result<bool, GalleryStoreError> {
        let pool = self.driver.sqlite_pool().ok_or_else(|| database_error("GalleryStore requires SQLite"))?;
        let mut connection = pool.get().map_err(database_error)?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(database_error)?;
        let matches: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM gallery_images WHERE id = ?1 AND gallery_id = ?2 AND hash = ?3 AND missing = 0)",
            params![record.id, record.gallery_id, record.hash], |row| row.get(0)).map_err(database_error)?;
        if !matches { return Ok(false); }
        for (tag_type, value, confidence) in tags {
            transaction.execute("INSERT INTO gallery_tags (id, image_id, tag_type, value, confidence, model_used, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(image_id, tag_type, value) DO NOTHING",
                params![uuid::Uuid::new_v4().to_string(), record.id, tag_type, value, confidence, model, now_rfc3339()]).map_err(database_error)?;
        }
        if complete {
            transaction.execute("UPDATE gallery_images SET metadata_stale = 0 WHERE id = ?1", [&record.id]).map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;
        Ok(true)
    }

    fn image_from_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImageRecord> {
        Ok(ImageRecord {
            id: row.get(0)?,
            gallery_id: row.get(1)?,
            relative_path: row.get(2)?,
            absolute_path: row.get(3)?,
            hash: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            format: row.get(7)?,
            size_bytes: row.get(8)?,
            added_at: row.get(9)?,
            media_type: row.get(10)?,
            missing: row.get(11)?,
            metadata_stale: row.get(12)?,
        })
    }

    /// List gallery assets in index order — 0-based position matching
    /// `get_image`'s `ORDER BY added_at ASC, id ASC` index semantics, so index
    /// `offset + i` in the result is the `image_index` every other gallery
    /// tool accepts. Paginated; no filter (callers filter client-side by
    /// `media_type` — a filtered query would renumber positions and break
    /// the index contract).
    #[must_use = "result must be used"]
    pub fn list_assets(
        &self,
        gallery_id: &str,
        offset: usize,
        limit: usize,
    ) -> std::result::Result<Vec<ImageRecord>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
             FROM gallery_images WHERE gallery_id = ?1 AND missing = 0
             ORDER BY added_at ASC, id ASC LIMIT ?3 OFFSET ?2",
            &[
                DbValue::Text(gallery_id.to_string()),
                DbValue::Integer(offset as i64),
                DbValue::Integer(limit as i64),
            ],
            Self::image_from_row,
        )?)
    }

    /// Count all assets in a gallery.
    #[must_use = "result must be used"]
    pub fn count_assets(&self, gallery_id: &str) -> std::result::Result<u64, GalleryStoreError> {
        query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM gallery_images WHERE gallery_id = ?1 AND missing = 0",
            &[DbValue::Text(gallery_id.to_string())],
            |row| row.get_int(0).map(|count| count as u64),
        )?
        .ok_or_else(|| {
            GalleryStoreError::NotFound(NotFound {
                entity_type: "gallery".to_string(),
                id: gallery_id.to_string(),
            })
        })
    }

    /// Get an image by index (0-based position in gallery) or by hash.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Get an image from a gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — get image by index or hash
    /// pre:  gallery_id is valid
    /// post: returns GalleryImage if found
    pub fn get_image(
        &self,
        gallery_id: &str,
        index: Option<usize>,
        hash: Option<&str>,
    ) -> std::result::Result<ImageRecord, GalleryStoreError> {
        let row = if let Some(h) = hash {
            query_row(
                &*self.driver,
                "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
                 FROM gallery_images WHERE gallery_id = ?1 AND hash = ?2 AND missing = 0 ORDER BY added_at ASC, id ASC LIMIT 1",
                &[
                    DbValue::Text(gallery_id.to_string()),
                    DbValue::Text(h.to_string()),
                ],
                Self::image_from_row,
            )?
            .ok_or_else(|| GalleryStoreError::NotFound(NotFound {
                entity_type: "image".to_string(),
                id: format!("hash={}", h),
            }))?
        } else if let Some(idx) = index {
            query_row(
                &*self.driver,
                "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale
                 FROM gallery_images WHERE gallery_id = ?1 AND missing = 0
                 ORDER BY added_at ASC, id ASC LIMIT 1 OFFSET ?2",
                &[
                    DbValue::Text(gallery_id.to_string()),
                    DbValue::Integer(idx as i64),
                ],
                Self::image_from_row,
            )?
            .ok_or_else(|| GalleryStoreError::NotFound(NotFound {
                entity_type: "image".to_string(),
                id: format!("index={}", idx),
            }))?
        } else {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "image".to_string(),
                id: "Must provide either index or hash".to_string(),
            }));
        };
        Ok(row)
    }
    /// Tag an image with AI-generated metadata.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Tag an image in a gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — tag an image
    /// pre:  gallery_id and image_hash are valid, tag is non-empty
    /// post: tag added to image
    pub fn tag_image(
        &self,
        image_id: &str,
        tag_type: &str,
        value: &str,
        confidence: f64,
        model_used: &str,
    ) -> std::result::Result<TagRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT OR IGNORE INTO gallery_tags (id, image_id, tag_type, value, confidence, model_used, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                DbValue::Text(id),
                DbValue::Text(image_id.to_string()),
                DbValue::Text(tag_type.to_string()),
                DbValue::Text(value.to_string()),
                DbValue::Real(confidence),
                DbValue::Text(model_used.to_string()),
                DbValue::Text(now.clone()),
            ],
        )?;
        // Read back the existing row when insert was ignored
        let existing_id: String = query_row(
            &*self.driver,
            "SELECT id FROM gallery_tags WHERE image_id = ?1 AND tag_type = ?2 AND value = ?3",
            &[
                DbValue::Text(image_id.to_string()),
                DbValue::Text(tag_type.to_string()),
                DbValue::Text(value.to_string()),
            ],
            |row| Ok(row.get_str(0)?.to_string()),
        )?
        .ok_or_else(|| {
            GalleryStoreError::NotFound(NotFound {
                entity_type: "image".to_string(),
                id: "tag vanished".to_string(),
            })
        })?;
        Ok(TagRecord {
            id: existing_id,
            image_id: image_id.to_string(),
            tag_type: tag_type.to_string(),
            value: value.to_string(),
            confidence,
            model_used: model_used.to_string(),
            created_at: now,
        })
    }
    /// Get all tags for an image.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Get tags for an image.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — get tags for an image
    /// pre:  gallery_id and image_hash are valid
    /// post: returns Vec of tags
    #[must_use = "result must be used"]
    pub fn get_tags(
        &self,
        image_id: &str,
    ) -> std::result::Result<Vec<TagRecord>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, image_id, tag_type, value, confidence, model_used, created_at
             FROM gallery_tags WHERE image_id = ?1
             ORDER BY created_at DESC",
            &[DbValue::Text(image_id.to_string())],
            Self::tag_from_row,
        )?)
    }
    /// Get all tags for all images in a gallery.
    ///
    /// Returns tags joined with their image's relative path for search ranking.
    /// expect: "The system provides durable storage for gallery data"
    /// Get all tags across all galleries.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — list all tags across galleries
    /// post: returns Vec of all unique tags
    pub fn get_all_tags(
        &self,
        gallery_id: &str,
    ) -> std::result::Result<Vec<(TagRecord, String)>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT t.id, t.image_id, t.tag_type, t.value, t.confidence, t.model_used, t.created_at, i.relative_path
             FROM gallery_tags t
             JOIN gallery_images i ON t.image_id = i.id
             WHERE i.gallery_id = ?1 AND i.missing = 0
             ORDER BY t.created_at DESC",
            &[DbValue::Text(gallery_id.to_string())],
            |row| {
                let tag = TagRecord {
                    id: row.get_str(0)?.to_string(),
                    image_id: row.get_str(1)?.to_string(),
                    tag_type: row.get_str(2)?.to_string(),
                    value: row.get_str(3)?.to_string(),
                    confidence: row.get_real(4)?,
                    model_used: row.get_str(5)?.to_string(),
                    created_at: row.get_str(6)?.to_string(),
                };
                let relative_path: String = row.get_str(7)?.to_string();
                Ok((tag, relative_path))
            },
        )?)
    }
    /// Register a face in the registry.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Register a face in the gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — register a face
    /// pre:  face data is valid
    /// post: face registered and returned (idempotent — returns existing on duplicate)
    pub fn register_face(
        &self,
        first_name: &str,
        last_name: &str,
        image_id: &str,
        embedding: Option<&[u8]>,
        status: &str,
        notes: &str,
    ) -> std::result::Result<FaceRegistryRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT OR IGNORE INTO face_registry (id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            &[
                DbValue::Text(id),
                DbValue::Text(first_name.to_string()),
                DbValue::Text(last_name.to_string()),
                DbValue::Text(image_id.to_string()),
                embedding.map_or(DbValue::Null, |e| DbValue::Blob(e.to_vec())),
                DbValue::Text(status.to_string()),
                DbValue::Text(notes.to_string()),
                DbValue::Text(now),
            ],
        )?;
        // Read back the existing row when insert was ignored (duplicate) or the new one
        query_row(
            &*self.driver,
            "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
             FROM face_registry WHERE first_name = ?1 AND last_name = ?2 AND image_id = ?3",
            &[
                DbValue::Text(first_name.to_string()),
                DbValue::Text(last_name.to_string()),
                DbValue::Text(image_id.to_string()),
            ],
            Self::face_from_row,
        )?
        .ok_or_else(|| GalleryStoreError::NotFound(NotFound {
            entity_type: "face".to_string(),
            id: "face registration failed".to_string(),
        }))
    }
    /// List all faces in the registry, optionally filtered by status.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// List faces with optional status filter.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — list faces
    /// post: returns Vec of faces, optionally filtered by status
    #[must_use = "result must be used"]
    pub fn list_faces(
        &self,
        status_filter: Option<&str>,
    ) -> std::result::Result<Vec<FaceRegistryRecord>, GalleryStoreError> {
        if let Some(status) = status_filter {
            Ok(query_map(
                &*self.driver,
                "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
                 FROM face_registry WHERE status = ?1
                 ORDER BY created_at DESC",
                &[DbValue::Text(status.to_string())],
                Self::face_from_row,
            )?)
        } else {
            Ok(query_map(
                &*self.driver,
                "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
                 FROM face_registry
                 ORDER BY created_at DESC",
                &[],
                Self::face_from_row,
            )?)
        }
    }
    /// Get a face registry entry by ID.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Get a face by ID.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — get face by ID
    /// pre:  face_id is non-empty
    /// post: returns Face if found
    #[must_use = "result must be used"]
    pub fn get_face(
        &self,
        face_id: &str,
    ) -> std::result::Result<FaceRegistryRecord, GalleryStoreError> {
        query_row(
            &*self.driver,
            "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
             FROM face_registry WHERE id = ?1",
            &[DbValue::Text(face_id.to_string())],
            Self::face_from_row,
        )?
        .ok_or_else(|| GalleryStoreError::NotFound(NotFound {
            entity_type: "face".to_string(),
            id: format!("face_id={}", face_id),
        }))
    }

    /// Get all face registry entries associated with a specific image.
    ///
    /// pre:  image_id is a valid gallery image ID
    /// post: returns Vec of face registry entries for this image
    #[must_use = "result must be used"]
    pub fn get_faces_for_image(
        &self,
        image_id: &str,
    ) -> std::result::Result<Vec<FaceRegistryRecord>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
             FROM face_registry WHERE image_id = ?1
             ORDER BY created_at DESC",
            &[DbValue::Text(image_id.to_string())],
            Self::face_from_row,
        )?)
    }

    /// Remove a face from the registry by ID.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Remove a face from the gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — remove face
    /// pre:  face_id is non-empty
    /// post: face deleted
    pub fn remove_face(&self, face_id: &str) -> std::result::Result<(), GalleryStoreError> {
        let affected = self.driver.execute(
            "DELETE FROM face_registry WHERE id = ?1",
            &[DbValue::Text(face_id.to_string())],
        )?;
        if affected == 0 {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "face".to_string(),
                id: format!("face_id={}", face_id),
            }));
        }
        Ok(())
    }
    /// Update a face registry entry's status and notes.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Update a face's status.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — update face status
    /// pre:  face_id is valid, status is valid
    /// post: face status updated
    pub fn update_face(
        &self,
        face_id: &str,
        status: &str,
        notes: &str,
    ) -> std::result::Result<FaceRegistryRecord, GalleryStoreError> {
        let now = now_rfc3339();
        let affected = self.driver.execute(
            "UPDATE face_registry SET status = ?1, notes = ?2, updated_at = ?3 WHERE id = ?4",
            &[
                DbValue::Text(status.to_string()),
                DbValue::Text(notes.to_string()),
                DbValue::Text(now),
                DbValue::Text(face_id.to_string()),
            ],
        )?;
        if affected == 0 {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "face".to_string(),
                id: format!("face_id={}", face_id),
            }));
        }
        // Read back the updated row
        query_row(
            &*self.driver,
            "SELECT id, first_name, last_name, image_id, embedding, status, notes, created_at, updated_at
             FROM face_registry WHERE id = ?1",
            &[DbValue::Text(face_id.to_string())],
            Self::face_from_row,
        )?
        .ok_or_else(|| GalleryStoreError::NotFound(NotFound {
            entity_type: "face".to_string(),
            id: format!("face_id={}", face_id),
        }))
    }
    // ── Row mappers ──
    fn image_from_row(
        row: &crate::database::value::DbRow,
    ) -> Result<ImageRecord, crate::database::types::DbError> {
        Ok(ImageRecord {
            id: row.get_str(0)?.to_string(),
            gallery_id: row.get_str(1)?.to_string(),
            relative_path: row.get_str(2)?.to_string(),
            absolute_path: row.get_str(3)?.to_string(),
            hash: row.get_str(4)?.to_string(),
            width: row.get_int(5)? as u32,
            height: row.get_int(6)? as u32,
            format: row.get_str(7)?.to_string(),
            size_bytes: row.get_int(8)? as u64,
            added_at: row.get_str(9)?.to_string(),
            media_type: row.get_str(10)?.to_string(),
            missing: row.get_int(11)? != 0,
            metadata_stale: row.get_int(12)? != 0,
        })
    }
    fn tag_from_row(
        row: &crate::database::value::DbRow,
    ) -> Result<TagRecord, crate::database::types::DbError> {
        Ok(TagRecord {
            id: row.get_str(0)?.to_string(),
            image_id: row.get_str(1)?.to_string(),
            tag_type: row.get_str(2)?.to_string(),
            value: row.get_str(3)?.to_string(),
            confidence: row.get_real(4)?,
            model_used: row.get_str(5)?.to_string(),
            created_at: row.get_str(6)?.to_string(),
        })
    }
    fn face_from_row(
        row: &crate::database::value::DbRow,
    ) -> Result<FaceRegistryRecord, crate::database::types::DbError> {
        Ok(FaceRegistryRecord {
            id: row.get_str(0)?.to_string(),
            first_name: row.get_str(1)?.to_string(),
            last_name: row.get_str(2)?.to_string(),
            image_id: row.get_str(3)?.to_string(),
            embedding: match row.get(4)? {
                DbValue::Null => None,
                v => Some(v.as_blob()?.to_vec()),
            },
            status: row.get_str(5)?.to_string(),
            notes: row.get_str(6)?.to_string(),
            created_at: row.get_str(7)?.to_string(),
            updated_at: row.get_str(8)?.to_string(),
        })
    }
}

impl GalleryStore {
    // ── Generation lineage (WS-3) ──────────────────────────────────────────

    /// Persist a workflow graph (serialized `WorkflowGraph` JSON) and return its id.
    pub fn record_workflow(
        &self,
        graph_json: &str,
    ) -> std::result::Result<WorkflowRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT INTO gallery_workflow (id, graph_json, created_at) VALUES (?1, ?2, ?3)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(graph_json.to_string()),
                DbValue::Text(now.clone()),
            ],
        )?;
        Ok(WorkflowRecord {
            id,
            graph_json: graph_json.to_string(),
            created_at: now,
        })
    }

    /// Look up a persisted workflow graph by id.
    #[must_use = "result must be used"]
    pub fn get_workflow(
        &self,
        workflow_id: &str,
    ) -> std::result::Result<WorkflowRecord, GalleryStoreError> {
        query_row(
            &*self.driver,
            "SELECT id, graph_json, created_at FROM gallery_workflow WHERE id = ?1",
            &[DbValue::Text(workflow_id.to_string())],
            Self::workflow_from_row,
        )?
        .ok_or_else(|| {
            GalleryStoreError::NotFound(NotFound {
                entity_type: "workflow".to_string(),
                id: workflow_id.to_string(),
            })
        })
    }

    /// List all saved workflows, newest first.
    #[must_use = "result must be used"]
    pub fn list_workflows(&self) -> std::result::Result<Vec<WorkflowRecord>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, graph_json, created_at FROM gallery_workflow ORDER BY created_at DESC",
            &[],
            Self::workflow_from_row,
        )?)
    }

    /// Delete a workflow by id. Does not delete assets produced by the
    /// workflow — only the workflow definition. Generation records referencing
    /// this workflow have their `workflow_id` set to NULL (ON DELETE SET NULL).
    pub fn delete_workflow(&self, workflow_id: &str) -> std::result::Result<(), GalleryStoreError> {
        let affected = self.driver.execute(
            "DELETE FROM gallery_workflow WHERE id = ?1",
            &[DbValue::Text(workflow_id.to_string())],
        )?;
        if affected == 0 {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "workflow".to_string(),
                id: workflow_id.to_string(),
            }));
        }
        Ok(())
    }

    /// Record the generation lineage for a gallery image. The image must
    /// already exist in `gallery_images` (the FK is enforced).
    pub fn record_generation(
        &self,
        image_id: &str,
        op: &str,
        prompt: Option<&str>,
        model: Option<&str>,
        provider: Option<&str>,
        seed: Option<i64>,
        params: Option<&str>,
        workflow_id: Option<&str>,
        parent_image_id: Option<&str>,
    ) -> std::result::Result<GenerationRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT INTO gallery_generation
             (id, image_id, op, prompt, model, provider, seed, params, workflow_id, parent_image_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(image_id.to_string()),
                DbValue::Text(op.to_string()),
                prompt.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                model.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                provider.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                seed.map(DbValue::Integer).unwrap_or(DbValue::Null),
                params.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                workflow_id.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                parent_image_id.map(|s| DbValue::Text(s.to_string())).unwrap_or(DbValue::Null),
                DbValue::Text(now.clone()),
            ],
        )?;
        Ok(GenerationRecord {
            id,
            image_id: image_id.to_string(),
            op: op.to_string(),
            prompt: prompt.map(String::from),
            model: model.map(String::from),
            provider: provider.map(String::from),
            seed,
            params: params.map(String::from),
            workflow_id: workflow_id.map(String::from),
            parent_image_id: parent_image_id.map(String::from),
            created_at: now,
        })
    }

    /// Look up the most recent generation lineage for an image (None if none
    /// recorded). Used by `gallery_reproduce` / `gallery_variants`.
    #[must_use = "result must be used"]
    pub fn get_generation(
        &self,
        image_id: &str,
    ) -> std::result::Result<Option<GenerationRecord>, GalleryStoreError> {
        Ok(query_row(
            &*self.driver,
            "SELECT id, image_id, op, prompt, model, provider, seed, params, workflow_id, parent_image_id, created_at
             FROM gallery_generation WHERE image_id = ?1 ORDER BY created_at DESC LIMIT 1",
            &[DbValue::Text(image_id.to_string())],
            Self::generation_from_row,
        )?)
    }

    // ── Album CRUD ──────────────────────────────────────────────────────

    /// Create a new album in a gallery.
    pub fn create_album(
        &self,
        gallery_id: &str,
        name: &str,
        parent_id: Option<&str>,
    ) -> std::result::Result<AlbumRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT INTO gallery_albums (id, gallery_id, name, parent_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(gallery_id.to_string()),
                DbValue::Text(name.to_string()),
                parent_id
                    .map(|s| DbValue::Text(s.to_string()))
                    .unwrap_or(DbValue::Null),
                DbValue::Text(now.clone()),
            ],
        )?;
        Ok(AlbumRecord {
            id,
            gallery_id: gallery_id.to_string(),
            name: name.to_string(),
            parent_id: parent_id.map(String::from),
            created_at: now,
        })
    }

    /// List all albums in a gallery.
    #[must_use = "result must be used"]
    pub fn list_albums(
        &self,
        gallery_id: &str,
    ) -> std::result::Result<Vec<AlbumRecord>, GalleryStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, gallery_id, name, parent_id, created_at
             FROM gallery_albums WHERE gallery_id = ?1
             ORDER BY created_at ASC",
            &[DbValue::Text(gallery_id.to_string())],
            Self::album_from_row,
        )?)
    }

    /// Add an image to an album (idempotent — re-adding is a no-op).
    pub fn add_to_album(
        &self,
        album_id: &str,
        image_id: &str,
    ) -> std::result::Result<(), GalleryStoreError> {
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT OR IGNORE INTO gallery_album_members (album_id, image_id, added_at)
             VALUES (?1, ?2, ?3)",
            &[
                DbValue::Text(album_id.to_string()),
                DbValue::Text(image_id.to_string()),
                DbValue::Text(now),
            ],
        )?;
        Ok(())
    }

    /// Remove an image from an album (idempotent — removing a non-member is a no-op).
    pub fn remove_from_album(
        &self,
        album_id: &str,
        image_id: &str,
    ) -> std::result::Result<(), GalleryStoreError> {
        self.driver.execute(
            "DELETE FROM gallery_album_members WHERE album_id = ?1 AND image_id = ?2",
            &[
                DbValue::Text(album_id.to_string()),
                DbValue::Text(image_id.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Delete an album. Assets remain in the gallery — only the album grouping
    /// and its memberships are removed.
    pub fn delete_album(&self, album_id: &str) -> std::result::Result<(), GalleryStoreError> {
        let affected = self.driver.execute(
            "DELETE FROM gallery_albums WHERE id = ?1",
            &[DbValue::Text(album_id.to_string())],
        )?;
        if affected == 0 {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "album".to_string(),
                id: format!("album_id={}", album_id),
            }));
        }
        Ok(())
    }

    /// List all image IDs in an album.
    #[must_use = "result must be used"]
    pub fn list_album_members(
        &self,
        album_id: &str,
    ) -> std::result::Result<Vec<String>, GalleryStoreError> {
        let rows = query_map(
            &*self.driver,
            "SELECT m.image_id FROM gallery_album_members m JOIN gallery_images i ON i.id = m.image_id WHERE m.album_id = ?1 AND i.missing = 0 ORDER BY i.added_at ASC, i.id ASC",
            &[DbValue::Text(album_id.to_string())],
            |row| row.get_str(0).map(String::from),
        )?;
        Ok(rows)
    }

    /// Delete an image record and all its associated data (tags, face
    /// associations, generation lineage). Does NOT delete the underlying
    /// file on disk — only the gallery index entry.
    ///
    /// pre:  image_id is a valid gallery image ID
    /// post: image record, tags, and generation lineage for image_id are deleted
    /// post: returns NotFound if the image_id does not exist
    pub fn delete_image(&self, image_id: &str) -> std::result::Result<(), GalleryStoreError> {
        let affected = self.driver.execute(
            "DELETE FROM gallery_images WHERE id = ?1",
            &[DbValue::Text(image_id.to_string())],
        )?;
        if affected == 0 {
            return Err(GalleryStoreError::NotFound(NotFound {
                entity_type: "image".to_string(),
                id: format!("image_id={}", image_id),
            }));
        }
        // Cascade: delete tags and generation lineage for this image.
        // These are best-effort — if they fail, the image is already deleted
        // and the orphaned tags/lineage are harmless (they reference a
        // non-existent image_id) — but the failure is logged so a lingering
        // orphan is diagnosable instead of silent.
        if let Err(e) = self.driver.execute(
            "DELETE FROM gallery_tags WHERE image_id = ?1",
            &[DbValue::Text(image_id.to_string())],
        ) {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                image_id,
                "failed to cascade-delete gallery tags — orphaned rows are harmless but visible"
            );
        }
        if let Err(e) = self.driver.execute(
            "DELETE FROM gallery_generation WHERE image_id = ?1",
            &[DbValue::Text(image_id.to_string())],
        ) {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                image_id,
                "failed to cascade-delete gallery generation lineage — orphaned rows are harmless but visible"
            );
        }
        Ok(())
    }

    fn workflow_from_row(
        row: &crate::database::value::DbRow,
    ) -> std::result::Result<WorkflowRecord, crate::database::types::DbError> {
        Ok(WorkflowRecord {
            id: row.get_str(0)?.to_string(),
            graph_json: row.get_str(1)?.to_string(),
            created_at: row.get_str(2)?.to_string(),
        })
    }

    fn generation_from_row(
        row: &crate::database::value::DbRow,
    ) -> std::result::Result<GenerationRecord, crate::database::types::DbError> {
        Ok(GenerationRecord {
            id: row.get_str(0)?.to_string(),
            image_id: row.get_str(1)?.to_string(),
            op: row.get_str(2)?.to_string(),
            prompt: match row.get(3)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            model: match row.get(4)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            provider: match row.get(5)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            seed: match row.get(6)? {
                DbValue::Integer(n) => Some(*n),
                _ => None,
            },
            params: match row.get(7)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            workflow_id: match row.get(8)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            parent_image_id: match row.get(9)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            created_at: row.get_str(10)?.to_string(),
        })
    }

    fn album_from_row(
        row: &crate::database::value::DbRow,
    ) -> std::result::Result<AlbumRecord, crate::database::types::DbError> {
        Ok(AlbumRecord {
            id: row.get_str(0)?.to_string(),
            gallery_id: row.get_str(1)?.to_string(),
            name: row.get_str(2)?.to_string(),
            parent_id: match row.get(3)? {
                DbValue::Text(s) => Some(s.to_string()),
                _ => None,
            },
            created_at: row.get_str(4)?.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn setup() -> GalleryStore {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = SqliteDriver::new(pool);
        GalleryStore::from_driver(Arc::new(driver)).expect("gallery store init")
    }

    #[test]
    fn create_gallery_returns_record() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        assert!(!gallery.id.is_empty());
        assert!(Path::new(&gallery.root_path).is_absolute());
    }

    #[test]
    fn reopening_preserves_mode_and_identity() {
        let store = setup();
        let directory = tempfile::tempdir().expect("gallery root");
        let path = directory.path().to_str().expect("UTF-8 root");
        let first = store.open(path, GalleryMode::ReadOnly).expect("open");
        let second = store.open(path, GalleryMode::Destructive).expect("reopen");
        assert_eq!(first.id, second.id);
        assert_eq!(second.mode, "read-only");
    }

    #[test]
    fn add_image_stores_record() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc123",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        assert_eq!(img.hash, "abc123");
        assert_eq!(img.width, 100);
    }

    #[test]
    fn get_image_by_index() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "aaa",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        store
            .add_image(
                &gallery.id,
                "/tmp/g/b.png",
                "bbb",
                300,
                400,
                "png",
                2048,
            )
            .unwrap();
        let img = store.get_image(&gallery.id, Some(0), None).unwrap();
        assert_eq!(img.hash, "aaa");
        let img2 = store.get_image(&gallery.id, Some(1), None).unwrap();
        assert_eq!(img2.hash, "bbb");
    }

    #[test]
    fn get_image_by_hash() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let img = store.get_image(&gallery.id, None, Some("abc")).unwrap();
        assert_eq!(img.hash, "abc");
    }

    #[test]
    fn tag_image_stores_tag() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let tag = store
            .tag_image(&img.id, "color", "red", 0.95, "test-model")
            .unwrap();
        assert_eq!(tag.value, "red");
    }

    #[test]
    fn get_tags_returns_all() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        store
            .tag_image(&img.id, "color", "red", 0.95, "test-model")
            .unwrap();
        store
            .tag_image(&img.id, "style", "abstract", 0.8, "test-model")
            .unwrap();
        let tags = store.get_tags(&img.id).unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[test]
    fn tag_image_ignores_duplicates() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        store
            .tag_image(&img.id, "color", "red", 0.95, "test-model")
            .unwrap();
        store
            .tag_image(&img.id, "color", "red", 0.8, "test-model")
            .unwrap(); // should be ignored
        let tags = store.get_tags(&img.id).unwrap();
        assert_eq!(tags.len(), 1);
    }

    #[test]
    fn register_face_creates_record() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let face = store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        assert_eq!(face.first_name, "John");
        assert_eq!(face.status, "active");
    }

    #[test]
    fn list_faces_returns_all() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        store
            .register_face("Jane", "Smith", &img.id, None, "pending", "")
            .unwrap();
        let faces = store.list_faces(None).unwrap();
        assert_eq!(faces.len(), 2);
    }

    #[test]
    fn list_faces_filters_by_status() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        store
            .register_face("Jane", "Smith", &img.id, None, "pending", "")
            .unwrap();
        let active = store.list_faces(Some("active")).unwrap();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn get_face_returns_record() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let face = store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        let retrieved = store.get_face(&face.id).unwrap();
        assert_eq!(retrieved.first_name, "John");
    }

    #[test]
    fn get_face_unknown_id_errors() {
        let store = setup();
        assert!(store.get_face("nonexistent").is_err());
    }

    #[test]
    fn remove_face_deletes_record() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let face = store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        store.remove_face(&face.id).unwrap();
        assert!(store.get_face(&face.id).is_err());
    }

    #[test]
    fn update_face_changes_status() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/g/a.png",
                "abc",
                100,
                200,
                "png",
                1024,
            )
            .unwrap();
        let face = store
            .register_face("John", "Doe", &img.id, None, "active", "")
            .unwrap();
        let updated = store.update_face(&face.id, "inactive", "retired").unwrap();
        assert_eq!(updated.status, "inactive");
    }

    #[test]
    fn record_and_get_generation_lineage() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/gen/out.png",
                "hash1",
                512,
                512,
                "png",
                2048,
            )
            .unwrap();
        let wf = store.record_workflow("{\"nodes\":[]}").unwrap();
        let gen_rec = store
            .record_generation(
                &img.id,
                "generate_image",
                Some("a cat in space"),
                Some("fal-ai/flux/dev"),
                Some("fal.ai"),
                Some(42),
                Some("{\"size\":\"square\"}"),
                Some(&wf.id),
                None,
            )
            .unwrap();
        assert_eq!(gen_rec.image_id, img.id);
        assert_eq!(gen_rec.op, "generate_image");
        assert_eq!(gen_rec.prompt.as_deref(), Some("a cat in space"));
        assert_eq!(gen_rec.model.as_deref(), Some("fal-ai/flux/dev"));
        assert_eq!(gen_rec.provider.as_deref(), Some("fal.ai"));
        assert_eq!(gen_rec.seed, Some(42));
        assert_eq!(gen_rec.workflow_id.as_deref(), Some(wf.id.as_str()));

        let retrieved = store
            .get_generation(&img.id)
            .unwrap()
            .expect("lineage should exist");
        assert_eq!(retrieved.op, "generate_image");
        assert_eq!(retrieved.prompt.as_deref(), Some("a cat in space"));
        assert_eq!(retrieved.seed, Some(42));
    }

    #[test]
    fn get_generation_returns_none_when_no_lineage() {
        let store = setup();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("gallery root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/gen2/x.png",
                "hash2",
                100,
                100,
                "png",
                512,
            )
            .unwrap();
        assert!(store.get_generation(&img.id).unwrap().is_none());
    }

    #[test]
    fn record_and_get_workflow() {
        let store = setup();
        let wf = store
            .record_workflow("{\"nodes\":[],\"parallel\":false}")
            .unwrap();
        let retrieved = store.get_workflow(&wf.id).unwrap();
        assert_eq!(retrieved.graph_json, "{\"nodes\":[],\"parallel\":false}");
    }

    #[test]
    fn get_workflow_unknown_id_errors() {
        let store = setup();
        assert!(store.get_workflow("nonexistent").is_err());
    }

    /// Durable file-backed gallery DB (G14): lineage written through one
    /// `SqliteDriver` (file) must survive dropping that driver and reopening
    /// the same file with a fresh driver — the contract `HKASK_MEDIA_DB` relies
    /// on. Uses a uuid-named temp dir (no `tempfile` dev-dep in this crate).
    #[test]
    fn gallery_lineage_survives_across_driver_instances() {
        use crate::database::sqlite::SqliteDriver;
        let dir =
            std::env::temp_dir().join(format!("hkask_storage_durability_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("gallery.db");
        let db_path_str = db_path.to_string_lossy().to_string();

        // First instance: create gallery, add image, record lineage.
        let image_id = {
            let driver = Arc::new(SqliteDriver::new_labeled(
                SqliteDriver::file_pool(&db_path_str).unwrap(),
                db_path_str.as_str(),
            ));
            let store = GalleryStore::from_driver(driver).unwrap();
            let gallery = store
                .open(
                    tempfile::tempdir()
                        .expect("gallery root")
                        .path()
                        .to_str()
                        .expect("UTF-8 root"),
                    GalleryMode::ReadOnly,
                )
                .unwrap();
            let img = store
                .add_image(&gallery.id, "/tmp/gal/a.png", "h", 1, 1, "png", 1)
                .unwrap();
            store
                .record_generation(
                    &img.id,
                    "generate_image",
                    Some("p"),
                    None,
                    None,
                    Some(7),
                    None,
                    None,
                    None,
                )
                .unwrap();
            img.id
        };
        // First driver dropped (WAL checkpoints on close). Reopen the same
        // file with a fresh driver — the lineage must persist.
        let driver = Arc::new(SqliteDriver::new_labeled(
            SqliteDriver::file_pool(&db_path_str).unwrap(),
            db_path_str.as_str(),
        ));
        let store = GalleryStore::from_driver(driver).unwrap();
        let lineage = store
            .get_generation(&image_id)
            .unwrap()
            .expect("lineage must persist across restart");
        assert_eq!(lineage.op, "generate_image");
        assert_eq!(lineage.prompt.as_deref(), Some("p"));
        assert_eq!(lineage.seed, Some(7));

        let _ = std::fs::remove_dir_all(&dir);
    }
    /// expect: Same path keeps identity while same-content copies remain separate; tied times have stable indices. [P1]
    #[test]
    fn path_upsert_retains_annotations_and_deterministic_positions() -> Result<(), Box<dyn std::error::Error>> {
        let store = setup();
        let directory = tempfile::tempdir()?;
        let gallery = store.open(directory.path().to_str().expect("UTF-8 root"), GalleryMode::ReadOnly)?;
        let path = directory.path().join("a.png").to_string_lossy().into_owned();
        let first = store.add_image(&gallery.id, &path, "hash", 1, 1, "png", 10)?;
        store.tag_image(&first.id, "caption", "Keep", 1.0, "user")?;
        let album = store.create_album(&gallery.id, "Keep", None)?;
        store.add_to_album(&album.id, &first.id)?;
        let repeated = store.add_image(&gallery.id, &path, "hash", 1, 1, "png", 10)?;
        assert_eq!(first.id, repeated.id); assert_eq!(first.added_at, repeated.added_at);
        assert_eq!(store.get(&gallery.id)?.total_size_bytes, 10);
        let changed = store.add_image(&gallery.id, &path, "changed", 2, 2, "png", 20)?;
        assert!(changed.metadata_stale); assert_eq!(changed.id, first.id);
        assert_eq!(store.get_tags(&first.id)?.len(), 1); assert_eq!(store.list_album_members(&album.id)?, vec![first.id.clone()]);
        let other = directory.path().join("b.png").to_string_lossy().into_owned();
        let copy = store.add_image(&gallery.id, &other, "changed", 2, 2, "png", 20)?;
        assert_ne!(copy.id, first.id);
        store.driver.execute("UPDATE gallery_images SET added_at = 'same-time' WHERE gallery_id = ?1", &[gallery.id.clone().into()])?;
        let listed = store.list_assets(&gallery.id, 0, 10)?;
        assert!(listed[0].id < listed[1].id);
        for (index, image) in listed.iter().enumerate() { assert_eq!(store.get_image(&gallery.id, Some(index), None)?.id, image.id); }
        assert_eq!(store.get(&gallery.id)?.total_size_bytes, 40);
        Ok(())
    }

    /// expect: Forward schema updates preserve metadata, and ambiguous duplicate identities stop explicitly. [P1]
    #[test]
    fn forward_schema_preserves_data_and_refuses_duplicate_identity() -> Result<(), Box<dyn std::error::Error>> {
        let store = setup();
        let directory = tempfile::tempdir()?;
        let gallery = store.open(directory.path().to_str().expect("UTF-8 root"), GalleryMode::ReadOnly)?;
        let path = directory.path().join("a.png").to_string_lossy().into_owned();
        let image = store.add_image(&gallery.id, &path, "hash", 1, 1, "png", 10)?;
        store.tag_image(&image.id, "caption", "Original annotation", 1.0, "user")?;
        store.driver.execute_batch("DROP INDEX idx_gallery_images_identity;
            ALTER TABLE gallery_images DROP COLUMN missing;
            ALTER TABLE gallery_images DROP COLUMN metadata_stale;
            ALTER TABLE galleries ADD COLUMN image_count INTEGER NOT NULL DEFAULT 99;
            ALTER TABLE galleries ADD COLUMN total_size_bytes INTEGER NOT NULL DEFAULT 99;")?;
        let migrated = GalleryStore::from_driver(store.driver.clone())?;
        assert_eq!(migrated.get_tags(&image.id)?.len(), 1);
        assert_eq!(migrated.get(&gallery.id)?.image_count, 1);
        assert_eq!(migrated.get(&gallery.id)?.total_size_bytes, 10);
        migrated.driver.execute_batch("DROP INDEX idx_gallery_images_identity;
            INSERT INTO gallery_images SELECT 'duplicate', gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at, media_type, missing, metadata_stale FROM gallery_images;")?;
        migrated.tag_image("duplicate", "caption", "Conflicting annotation", 1.0, "user")?;
        let error = match GalleryStore::from_driver(migrated.driver.clone()) { Ok(_) => panic!("duplicate identity accepted"), Err(error) => error };
        assert!(error.to_string().contains("identity conflict"), "{error}");
        assert_eq!(migrated.get_tags(&image.id)?.len(), 1);
        assert_eq!(migrated.get_tags("duplicate")?.len(), 1);
        assert_eq!(migrated.count_assets(&gallery.id)?, 2);
        Ok(())
    }

}
