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
use serde::{Deserialize, Serialize};
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
    #[error("Gallery already exists at path: {0}")]
    AlreadyExists(String),
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
define_driver_store!(GalleryStore);
impl GalleryStore {
    /// Initialize gallery tables in the database.
    /// Initialize gallery tables.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — schema for galleries, images, tags, faces
    /// pre:  conn is a valid SQLite connection
    /// post: gallery tables created if not exists
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS galleries (
                id TEXT PRIMARY KEY,
                root_path TEXT NOT NULL UNIQUE,
                mode TEXT NOT NULL DEFAULT 'read-only',
                image_count INTEGER NOT NULL DEFAULT 0,
                total_size_bytes INTEGER NOT NULL DEFAULT 0,
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
                added_at TEXT NOT NULL
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
                ON face_registry(status);",
        );
    }
    /// Create a new gallery. Returns the gallery record.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// Create a new gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — create a gallery
    /// pre:  name is non-empty
    /// post: gallery created and returned
    pub fn create(
        &self,
        root_path: &str,
        mode: GalleryMode,
    ) -> std::result::Result<GalleryRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        // Check for existing gallery at this path
        let existing: Option<String> = query_row(
            &*self.driver,
            "SELECT id FROM galleries WHERE root_path = ?1",
            &[DbValue::Text(root_path.to_string())],
            |row| Ok(row.get_str(0)?.to_string()),
        )?;
        if existing.is_some() {
            return Err(GalleryStoreError::AlreadyExists(root_path.to_string()));
        }
        self.driver.execute(
            "INSERT INTO galleries (id, root_path, mode, image_count, total_size_bytes, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 0, ?4, ?4)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(root_path.to_string()),
                DbValue::Text(mode.as_str().to_string()),
                DbValue::Text(now.clone()),
            ],
        )?;
        Ok(GalleryRecord {
            id,
            root_path: root_path.to_string(),
            mode: mode.as_str().to_string(),
            image_count: 0,
            total_size_bytes: 0,
            created_at: now.clone(),
            updated_at: now,
        })
    }
    /// Add an image to the gallery index.
    ///
    /// expect: "The system provides durable storage for gallery data"
    #[allow(clippy::too_many_arguments)]
    /// Add an image to a gallery.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — add image to gallery
    /// pre:  gallery_id is valid, image data is non-empty
    /// post: image stored in gallery
    pub fn add_image(
        &self,
        gallery_id: &str,
        relative_path: &str,
        absolute_path: &str,
        hash: &str,
        width: u32,
        height: u32,
        format: &str,
        size_bytes: u64,
    ) -> std::result::Result<ImageRecord, GalleryStoreError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        self.driver.execute(
            "INSERT INTO gallery_images (id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            &[
                DbValue::Text(id.clone()),
                DbValue::Text(gallery_id.to_string()),
                DbValue::Text(relative_path.to_string()),
                DbValue::Text(absolute_path.to_string()),
                DbValue::Text(hash.to_string()),
                DbValue::Integer(width as i64),
                DbValue::Integer(height as i64),
                DbValue::Text(format.to_string()),
                DbValue::Integer(size_bytes as i64),
                DbValue::Text(now.clone()),
            ],
        )?;
        // Update gallery counts
        self.driver.execute(
            "UPDATE galleries SET image_count = image_count + 1, total_size_bytes = total_size_bytes + ?1, updated_at = ?2
             WHERE id = ?3",
            &[
                DbValue::Integer(size_bytes as i64),
                DbValue::Text(now.clone()),
                DbValue::Text(gallery_id.to_string()),
            ],
        )?;
        Ok(ImageRecord {
            id,
            gallery_id: gallery_id.to_string(),
            relative_path: relative_path.to_string(),
            absolute_path: absolute_path.to_string(),
            hash: hash.to_string(),
            width,
            height,
            format: format.to_string(),
            size_bytes,
            added_at: now,
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
                "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at
                 FROM gallery_images WHERE gallery_id = ?1 AND hash = ?2",
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
                "SELECT id, gallery_id, relative_path, absolute_path, hash, width, height, format, size_bytes, added_at
                 FROM gallery_images WHERE gallery_id = ?1
                 ORDER BY added_at ASC LIMIT 1 OFFSET ?2",
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
    /// Get gallery record by ID.
    /// Get a gallery by ID.
    ///
    /// expect: "The system provides durable storage for gallery data"
    /// \[P3\] Motivating: Generative Space — get gallery by ID
    /// pre:  gallery_id is valid
    /// post: returns Gallery if found
    #[must_use = "result must be used"]
    pub fn get_gallery(
        &self,
        gallery_id: &str,
    ) -> std::result::Result<GalleryRecord, GalleryStoreError> {
        query_row(
            &*self.driver,
            "SELECT id, root_path, mode, image_count, total_size_bytes, created_at, updated_at
             FROM galleries WHERE id = ?1",
            &[DbValue::Text(gallery_id.to_string())],
            |row| {
                Ok(GalleryRecord {
                    id: row.get_str(0)?.to_string(),
                    root_path: row.get_str(1)?.to_string(),
                    mode: row.get_str(2)?.to_string(),
                    image_count: row.get_int(3)? as u32,
                    total_size_bytes: row.get_int(4)? as u64,
                    created_at: row.get_str(5)?.to_string(),
                    updated_at: row.get_str(6)?.to_string(),
                })
            },
        )?
        .ok_or_else(|| {
            GalleryStoreError::NotFound(NotFound {
                entity_type: "gallery".to_string(),
                id: gallery_id.to_string(),
            })
        })
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
             WHERE i.gallery_id = ?1
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn setup() -> GalleryStore {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = SqliteDriver::new(pool);
        GalleryStore::from_driver(Arc::new(driver))
    }

    #[test]
    fn create_gallery_returns_record() {
        let store = setup();
        let gallery = store.create("/tmp/test", GalleryMode::ReadOnly).unwrap();
        assert!(!gallery.id.is_empty());
        assert_eq!(gallery.root_path, "/tmp/test");
    }

    #[test]
    fn create_duplicate_path_rejected() {
        let store = setup();
        store.create("/tmp/dup", GalleryMode::ReadOnly).unwrap();
        assert!(store.create("/tmp/dup", GalleryMode::ReadOnly).is_err());
    }

    #[test]
    fn add_image_stores_record() {
        let store = setup();
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        store
            .add_image(
                &gallery.id,
                "a.png",
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
                "b.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
        let gallery = store.create("/tmp/g", GalleryMode::ReadOnly).unwrap();
        let img = store
            .add_image(
                &gallery.id,
                "a.png",
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
}
