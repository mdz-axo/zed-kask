//! BackupArchive — portable sovereignty archive for hKask cloud deployment.
//!
//! # REQ: P1-deploy-backup-archive — P1 User Sovereignty: downloadable, passphrase-encrypted h_mem export.
//! expect: "My user data and sovereignty boundaries are stored under my control"
//!
//! Creates a single SQLCipher-encrypted SQLite file containing:
//! 1. A `backup_meta` table with export metadata
//! 2. The user's full live h_mem set from the source HMemStore
//!
//! semantic-graph-audit (M4): this archive covers SQLite + h_mems ONLY.
//! Adapter weight blobs (on disk at `TrainedLoRAAdapter.storage_path`) and GGUFs
//! are NOT backed up by anything today. Do NOT add a third ad-hoc S3 sync path
//! — extend THIS archive (or a sibling `BlobArchive`) to include
//! content-addressed weight blobs, so backup stays under ONE authority.
//! The existing `Checksum` (SHA-256) gives dedup for free.
use super::HMemStore;
use crate::database::types::DbError;
use crate::database::value::DbValue;
use chrono::Utc;
use hkask_types::WebID;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use utoipa::ToSchema;
#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid: {0}")]
    Validation(String),
    #[error("Archive is empty — no h_mems to export")]
    Empty,
}
impl From<DbError> for ArchiveError {
    fn from(e: DbError) -> Self {
        ArchiveError::Database(e.to_string())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BackupMeta {
    pub webid: String,
    pub source_server_url: String,
    pub exported_at: String,
    pub triple_count: i64,
    pub schema_version: i32,
}
#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct MigrationReceipt {
    /// Number of h_mems imported.
    pub triple_count: i64,
}
type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;

pub(crate) struct BackupArchive {
    pool: Pool,
    path: PathBuf,
}
impl BackupArchive {
    /// Create a pool for the archive database, handling SQLCipher setup.
    fn open_pool(path: &str, passphrase: &str) -> Result<Pool, ArchiveError> {
        let db = crate::core::connection::Database::open(path, passphrase)
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        db.sqlite_pool()
            .map_err(|e| ArchiveError::Database(e.to_string()))
    }

    pub fn create(
        output_path: PathBuf,
        passphrase: &str,
        source: &HMemStore,
        owner_webid: &WebID,
        source_server_url: &str,
    ) -> Result<Self, ArchiveError> {
        if passphrase.len() < 8 {
            return Err(ArchiveError::Validation(
                "Passphrase must be at least 8 characters".to_string(),
            ));
        }
        let path_str = output_path.to_str().ok_or_else(|| {
            ArchiveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid output path",
            ))
        })?;
        let pool = Self::open_pool(path_str, passphrase)?;
        // Create archive schema
        {
            let conn = pool
                .get()
                .map_err(|e| ArchiveError::Database(e.to_string()))?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS backup_meta (
                    webid TEXT NOT NULL,
                    source_server_url TEXT NOT NULL,
                    exported_at TEXT NOT NULL,
                    triple_count INTEGER NOT NULL,
                    schema_version INTEGER NOT NULL DEFAULT 1
                );
                CREATE TABLE IF NOT EXISTS h_mems (
                    id TEXT PRIMARY KEY,
                    entity TEXT NOT NULL,
                    attribute TEXT NOT NULL,
                    value TEXT NOT NULL,
                    valid_from TEXT NOT NULL,
                    valid_to TEXT,
                    recalled_at TEXT,
                    confidence REAL NOT NULL DEFAULT 1.0,
                    perspective TEXT,
                    visibility TEXT NOT NULL DEFAULT 'private',
                    owner_webid TEXT NOT NULL,
                    ontology TEXT
                );",
            )
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        }
        let triple_count = Self::copy_triples(&pool, source, owner_webid)?;
        let meta = BackupMeta {
            webid: owner_webid.to_string(),
            source_server_url: source_server_url.to_string(),
            exported_at: Utc::now().to_rfc3339(),
            triple_count,
            schema_version: 1,
        };
        {
            let conn = pool
                .get()
                .map_err(|e| ArchiveError::Database(e.to_string()))?;
            conn.execute(
                "INSERT INTO backup_meta (webid, source_server_url, exported_at, triple_count, schema_version)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    meta.webid,
                    meta.source_server_url,
                    meta.exported_at,
                    meta.triple_count,
                    meta.schema_version,
                ],
            )
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        }
        Ok(Self {
            pool,
            path: output_path,
        })
    }
    pub fn open(path: PathBuf, passphrase: &str) -> Result<Self, ArchiveError> {
        let path_str = path.to_str().ok_or_else(|| {
            ArchiveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid path",
            ))
        })?;
        let pool = Self::open_pool(path_str, passphrase)?;
        Ok(Self { pool, path })
    }
    pub fn metadata(&self) -> Result<BackupMeta, ArchiveError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        conn.query_row(
            "SELECT webid, source_server_url, exported_at, triple_count, schema_version FROM backup_meta LIMIT 1",
            [],
            |row| {
                Ok(BackupMeta {
                    webid: row.get(0)?,
                    source_server_url: row.get(1)?,
                    exported_at: row.get(2)?,
                    triple_count: row.get(3)?,
                    schema_version: row.get(4)?,
                })
            },
        )
        .map_err(|e| ArchiveError::Database(e.to_string()))
    }
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
    pub fn triple_count(&self) -> Result<i64, ArchiveError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM h_mems", [], |row| row.get(0))
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        Ok(count)
    }
    /// Restore h_mems from this archive into a target HMemStore.
    ///
    /// Simple idempotent insert — no collision handling, no renaming.
    /// expect: "The system provides durable storage for archival data"
    /// pre:  archive is open; target is a live HMemStore
    /// post: all h_mems upserted into target
    pub fn restore_into(
        &self,
        target: &HMemStore,
        owner_webid: &WebID,
    ) -> Result<MigrationReceipt, ArchiveError> {
        let rows = self.read_triples()?;
        let total = rows.len() as i64;
        // Hold a single pooled connection for the entire import so the target
        // is either fully imported or unchanged. The prior per-row
        // `driver.execute()` pattern acquired a separate pool connection per
        // row (autocommit), so a failure mid-loop left the target half-imported.
        let pool = target.driver().sqlite_pool().ok_or_else(|| {
            ArchiveError::Database("restore_into requires a SqliteDriver".to_string())
        })?;
        let mut conn = pool
            .get()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        let owner = owner_webid.to_string();
        for row in &rows {
            tx.execute(
                "INSERT OR REPLACE INTO hmems (id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, ontology)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    row.id,
                    row.entity,
                    row.attribute,
                    row.value,
                    row.valid_from,
                    row.valid_to,
                    row.recalled_at,
                    row.confidence,
                    row.perspective,
                    row.visibility,
                    owner,
                    row.ontology,
                ],
            ).map_err(|e| ArchiveError::Database(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        Ok(MigrationReceipt {
            triple_count: total,
        })
    }
    /// Read all h_mems from this archive.
    fn read_triples(&self) -> Result<Vec<HMemRow>, ArchiveError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, ontology FROM h_mems",
        )
        .map_err(|e| ArchiveError::Database(e.to_string()))?;
        stmt.query_map([], |row| {
            Ok(HMemRow {
                id: row.get(0)?,
                entity: row.get(1)?,
                attribute: row.get(2)?,
                value: row.get(3)?,
                valid_from: row.get(4)?,
                valid_to: row.get(5)?,
                recalled_at: row.get(6)?,
                confidence: row.get(7)?,
                perspective: row.get(8)?,
                visibility: row.get(9)?,
                owner_webid: row.get(10)?,
                ontology: row.get(11)?,
            })
        })
        .map_err(|e| ArchiveError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ArchiveError::Database(e.to_string()))
    }
    fn copy_triples(
        archive_pool: &Pool,
        source: &HMemStore,
        owner_webid: &WebID,
    ) -> Result<i64, ArchiveError> {
        let webid_str = owner_webid.to_string();
        let driver = source.driver();
        let rows: Vec<HMemRow> = driver
            .query(
                "SELECT id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, ontology
                 FROM hmems WHERE owner_webid = ?1",
                &[DbValue::Text(webid_str)],
            )?
            .iter()
            .map(|row| {
                Ok(HMemRow {
                    id: row.get(0)?.as_text()?.to_string(),
                    entity: row.get(1)?.as_text()?.to_string(),
                    attribute: row.get(2)?.as_text()?.to_string(),
                    value: row.get(3)?.as_text()?.to_string(),
                    valid_from: row.get(4)?.as_text()?.to_string(),
                    valid_to: row.get(5)?.as_text().ok().map(|s| s.to_string()),
                    recalled_at: row.get(6)?.as_text().ok().map(|s| s.to_string()),
                    confidence: row.get(7)?.as_real()?,
                    perspective: row.get(8)?.as_text().ok().map(|s| s.to_string()),
                    visibility: row.get(9)?.as_text()?.to_string(),
                    owner_webid: row.get(10)?.as_text()?.to_string(),
                    ontology: row.get(11)?.as_text().ok().map(|s| s.to_string()),
                })
            })
            .collect::<Result<Vec<_>, DbError>>()?;
        let count = rows.len() as i64;
        if count == 0 {
            return Err(ArchiveError::Empty);
        }
        let archive_conn = archive_pool
            .get()
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        for row in &rows {
            archive_conn.execute(
                "INSERT INTO h_mems (id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, ontology)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    row.id,
                    row.entity,
                    row.attribute,
                    row.value,
                    row.valid_from,
                    row.valid_to,
                    row.recalled_at,
                    row.confidence,
                    row.perspective,
                    row.visibility,
                    row.owner_webid,
                    row.ontology,
                ],
            )
            .map_err(|e| ArchiveError::Database(e.to_string()))?;
        }
        Ok(count)
    }
}
struct HMemRow {
    id: String,
    entity: String,
    attribute: String,
    value: String,
    valid_from: String,
    valid_to: Option<String>,
    recalled_at: Option<String>,
    confidence: f64,
    perspective: Option<String>,
    visibility: String,
    owner_webid: String,
    ontology: Option<String>,
}
