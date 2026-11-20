//! Content-addressed blob storage on disk + SQLite reference-counted
//! metadata index (`blobs` + `owners`).
//!
//! Blobs live at `<store_dir>/blobs/<aa>/<sha256>` and are always written via a
//! staging temp file under `<store_dir>/tmp/` before atomic rename.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlobRecord {
    pub sha256: String,
    pub mime_type: String,
    pub size: u64,
    pub uploaded_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageStats {
    pub blob_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug)]
pub enum DeleteOutcome {
    NotFound,
    NotOwner,
    /// Removed this author's ownership ref; other owners still retain the blob.
    RefRemoved {
        remaining_owners: u64,
    },
    /// Removed the last owner and unlinked the blob file.
    BlobDeleted,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    Db {
        context: &'static str,
        #[source]
        source: rusqlite::Error,
    },
    #[error(
        "local Blossom quota exceeded ({current_bytes} + {incoming_bytes} > {max_bytes} bytes)"
    )]
    QuotaExceeded {
        current_bytes: u64,
        incoming_bytes: u64,
        max_bytes: u64,
    },
}

impl StorageError {
    fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    fn db(context: &'static str) -> impl FnOnce(rusqlite::Error) -> Self {
        move |source| Self::Db { context, source }
    }
}

pub struct BlobStore {
    root: PathBuf,
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for BlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobStore")
            .field("root", &self.root)
            .finish()
    }
}

impl BlobStore {
    pub fn open(root: PathBuf) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root.join("blobs"))
            .map_err(StorageError::io("failed to create blobs dir"))?;
        std::fs::create_dir_all(root.join("tmp"))
            .map_err(StorageError::io("failed to create tmp dir"))?;
        let db_path = root.join("index.db");
        let conn =
            Connection::open(&db_path).map_err(StorageError::db("failed to open index.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS blobs (
                 sha256 TEXT PRIMARY KEY,
                 mime_type TEXT NOT NULL,
                 size INTEGER NOT NULL,
                 uploaded_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS owners (
                 sha256 TEXT NOT NULL REFERENCES blobs(sha256) ON DELETE CASCADE,
                 pubkey_hex TEXT NOT NULL,
                 PRIMARY KEY (sha256, pubkey_hex)
             );
             CREATE INDEX IF NOT EXISTS idx_owners_pubkey ON owners(pubkey_hex);",
        )
        .map_err(StorageError::db("failed to initialize Blossom schema"))?;
        Ok(Self {
            root,
            conn: Mutex::new(conn),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn blob_path(&self, sha256: &str) -> PathBuf {
        let prefix = &sha256[..2];
        self.root.join("blobs").join(prefix).join(sha256)
    }

    pub fn temp_path(&self, nonce: u64) -> PathBuf {
        self.root.join("tmp").join(format!("upload-{nonce}.part"))
    }

    pub fn stats(&self) -> Result<StorageStats, StorageError> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let (count, total): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM blobs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(StorageError::db("stats query failed"))?;
        Ok(StorageStats {
            blob_count: count.max(0) as u64,
            total_bytes: total.max(0) as u64,
        })
    }

    pub fn get_blob(&self, sha256: &str) -> Result<Option<BlobRecord>, StorageError> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.query_row(
            "SELECT sha256, mime_type, size, uploaded_at FROM blobs WHERE sha256 = ?1",
            params![sha256],
            |row| {
                Ok(BlobRecord {
                    sha256: row.get(0)?,
                    mime_type: row.get(1)?,
                    size: row.get::<_, i64>(2)?.max(0) as u64,
                    uploaded_at: row.get::<_, i64>(3)?.max(0) as u64,
                })
            },
        )
        .optional()
        .map_err(StorageError::db("get_blob failed"))
    }

    pub fn list_by_owner(
        &self,
        pubkey_hex: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BlobRecord>, StorageError> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let mut out = Vec::new();
        if let Some(pk) = pubkey_hex {
            let mut stmt = conn
                .prepare(
                    "SELECT b.sha256, b.mime_type, b.size, b.uploaded_at
                     FROM blobs b
                     INNER JOIN owners o ON o.sha256 = b.sha256
                     WHERE o.pubkey_hex = ?1
                     ORDER BY b.uploaded_at DESC, b.sha256 ASC
                     LIMIT ?2",
                )
                .map_err(StorageError::db("list_by_owner prepare failed"))?;
            let rows = stmt
                .query_map(params![pk, limit as i64], |row| {
                    Ok(BlobRecord {
                        sha256: row.get(0)?,
                        mime_type: row.get(1)?,
                        size: row.get::<_, i64>(2)?.max(0) as u64,
                        uploaded_at: row.get::<_, i64>(3)?.max(0) as u64,
                    })
                })
                .map_err(StorageError::db("list_by_owner query failed"))?;
            for row in rows {
                out.push(row.map_err(StorageError::db("row decode failed"))?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT sha256, mime_type, size, uploaded_at
                     FROM blobs
                     ORDER BY uploaded_at DESC, sha256 ASC
                     LIMIT ?1",
                )
                .map_err(StorageError::db("list_all prepare failed"))?;
            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    Ok(BlobRecord {
                        sha256: row.get(0)?,
                        mime_type: row.get(1)?,
                        size: row.get::<_, i64>(2)?.max(0) as u64,
                        uploaded_at: row.get::<_, i64>(3)?.max(0) as u64,
                    })
                })
                .map_err(StorageError::db("list_all query failed"))?;
            for row in rows {
                out.push(row.map_err(StorageError::db("row decode failed"))?);
            }
        }
        Ok(out)
    }

    /// Check whether admitting `incoming_bytes` for `sha256` (if not already
    /// stored) fits within `max_bytes` (`0` = unlimited).
    pub fn check_quota(
        &self,
        sha256: Option<&str>,
        incoming_bytes: u64,
        max_bytes: u64,
    ) -> Result<(), StorageError> {
        if max_bytes == 0 {
            return Ok(());
        }
        if let Some(hash) = sha256
            && self.get_blob(hash)?.is_some()
        {
            // Already stored on disk; adding an owner ref uses zero extra blob bytes.
            return Ok(());
        }
        let stats = self.stats()?;
        if stats.total_bytes.saturating_add(incoming_bytes) > max_bytes {
            return Err(StorageError::QuotaExceeded {
                current_bytes: stats.total_bytes,
                incoming_bytes,
                max_bytes,
            });
        }
        Ok(())
    }

    /// Commit a verified staged temp file into `<root>/blobs/<aa>/<sha256>`
    /// and record `(sha256, pubkey_hex)` ownership.
    pub fn commit_blob(
        &self,
        temp_file: &Path,
        sha256: &str,
        mime_type: &str,
        size: u64,
        pubkey_hex: &str,
        max_bytes: u64,
    ) -> Result<BlobRecord, StorageError> {
        let dest = self.blob_path(sha256);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(StorageError::io("failed to create blob prefix dir"))?;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let existing: Option<BlobRecord> = conn
            .query_row(
                "SELECT sha256, mime_type, size, uploaded_at FROM blobs WHERE sha256 = ?1",
                params![sha256],
                |row| {
                    Ok(BlobRecord {
                        sha256: row.get(0)?,
                        mime_type: row.get(1)?,
                        size: row.get::<_, i64>(2)?.max(0) as u64,
                        uploaded_at: row.get::<_, i64>(3)?.max(0) as u64,
                    })
                },
            )
            .optional()
            .map_err(StorageError::db("lookup failed"))?;

        let record = if let Some(rec) = existing {
            let _ = std::fs::remove_file(temp_file);
            rec
        } else {
            if max_bytes > 0 {
                let total: i64 = conn
                    .query_row("SELECT COALESCE(SUM(size), 0) FROM blobs", [], |r| r.get(0))
                    .map_err(StorageError::db("quota query failed"))?;
                let current_bytes = total.max(0) as u64;
                if current_bytes.saturating_add(size) > max_bytes {
                    let _ = std::fs::remove_file(temp_file);
                    return Err(StorageError::QuotaExceeded {
                        current_bytes,
                        incoming_bytes: size,
                        max_bytes,
                    });
                }
            }
            std::fs::rename(temp_file, &dest)
                .map_err(StorageError::io("failed to move staged blob into place"))?;
            conn.execute(
                "INSERT INTO blobs (sha256, mime_type, size, uploaded_at) VALUES (?1, ?2, ?3, ?4)",
                params![sha256, mime_type, size as i64, now as i64],
            )
            .map_err(StorageError::db("failed to insert blob row"))?;
            BlobRecord {
                sha256: sha256.to_string(),
                mime_type: mime_type.to_string(),
                size,
                uploaded_at: now,
            }
        };

        conn.execute(
            "INSERT OR IGNORE INTO owners (sha256, pubkey_hex) VALUES (?1, ?2)",
            params![sha256, pubkey_hex],
        )
        .map_err(StorageError::db("failed to record blob owner"))?;

        Ok(record)
    }

    /// Remove `pubkey_hex`'s ownership of `sha256`. Unlinks the blob file and
    /// deletes the `blobs` row only when zero owners remain.
    pub fn delete_owner(
        &self,
        sha256: &str,
        pubkey_hex: &str,
    ) -> Result<DeleteOutcome, StorageError> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM blobs WHERE sha256 = ?1)",
                params![sha256],
                |row| row.get(0),
            )
            .map_err(StorageError::db("exists query failed"))?;
        if !exists {
            return Ok(DeleteOutcome::NotFound);
        }
        let deleted_refs = conn
            .execute(
                "DELETE FROM owners WHERE sha256 = ?1 AND pubkey_hex = ?2",
                params![sha256, pubkey_hex],
            )
            .map_err(StorageError::db("delete owner failed"))?;
        if deleted_refs == 0 {
            return Ok(DeleteOutcome::NotOwner);
        }
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM owners WHERE sha256 = ?1",
                params![sha256],
                |row| row.get(0),
            )
            .map_err(StorageError::db("remaining owners query failed"))?;
        if remaining == 0 {
            conn.execute("DELETE FROM blobs WHERE sha256 = ?1", params![sha256])
                .map_err(StorageError::db("delete blob row failed"))?;
            let path = self.blob_path(sha256);
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            Ok(DeleteOutcome::BlobDeleted)
        } else {
            Ok(DeleteOutcome::RefRemoved {
                remaining_owners: remaining as u64,
            })
        }
    }
}
