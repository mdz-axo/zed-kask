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
pub enum ArchiveError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Archive is empty — no h_mems to export")]
    Empty,
}
impl From<DbError> for ArchiveError {
    fn from(e: DbError) -> Self {
        ArchiveError::Database(e.to_string())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMeta {
    pub webid: String,
    pub source_server_url: String,
    pub exported_at: String,
    pub triple_count: i64,
    pub schema_version: i32,
}
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MigrationReceipt {
    /// Number of h_mems imported.
    pub triple_count: i64,
}
type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;

pub struct BackupArchive {
    pool: Pool,
    path: PathBuf,
}
impl BackupArchive {
    /// Create a pool for the archive database, handling SQLCipher setup.
    fn open_pool(path: &str, passphrase: &str) -> Result<Pool, ArchiveError> {
        let db = crate::core::database::Database::open(path, passphrase)
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
            return Err(ArchiveError::Database(
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
                    confidence REAL NOT NULL DEFAULT 1.0,
                    perspective TEXT,
                    visibility TEXT NOT NULL DEFAULT 'private',
                    owner_webid TEXT NOT NULL
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
            .query_row("SELECT COUNT(*) FROM hmems", [], |row| row.get(0))
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
        let driver = target.driver();
        for row in rows {
            let owner = owner_webid.to_string();
            driver.execute(
                "INSERT OR REPLACE INTO hmems (id, entity, attribute, value, valid_from, valid_to, confidence, perspective, visibility, owner_webid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                &[
                    DbValue::Text(row.id),
                    DbValue::Text(row.entity),
                    DbValue::Text(row.attribute),
                    DbValue::Text(row.value),
                    DbValue::Text(row.valid_from),
                    row.valid_to.map_or(DbValue::Null, DbValue::Text),
                    DbValue::Real(row.confidence),
                    row.perspective.map_or(DbValue::Null, DbValue::Text),
                    DbValue::Text(row.visibility),
                    DbValue::Text(owner),
                ],
            )?;
        }
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
            "SELECT id, entity, attribute, value, valid_from, valid_to, confidence, perspective, visibility, owner_webid FROM hmems",
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
                confidence: row.get(6)?,
                perspective: row.get(7)?,
                visibility: row.get(8)?,
                owner_webid: row.get(9)?,
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
                "SELECT id, entity, attribute, value, valid_from, valid_to, confidence, perspective, visibility, owner_webid
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
                    confidence: row.get(6)?.as_real()?,
                    perspective: row.get(7)?.as_text().ok().map(|s| s.to_string()),
                    visibility: row.get(8)?.as_text()?.to_string(),
                    owner_webid: row.get(9)?.as_text()?.to_string(),
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
                "INSERT INTO hmems (id, entity, attribute, value, valid_from, valid_to, confidence, perspective, visibility, owner_webid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    row.id,
                    row.entity,
                    row.attribute,
                    row.value,
                    row.valid_from,
                    row.valid_to,
                    row.confidence,
                    row.perspective,
                    row.visibility,
                    row.owner_webid,
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
    confidence: f64,
    perspective: Option<String>,
    visibility: String,
    owner_webid: String,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("hkask-archive-{}-{}", prefix, uuid::Uuid::new_v4()))
    }

    fn setup_h_mem_store() -> (HMemStore, PathBuf, WebID) {
        let pool = SqliteDriver::in_memory_pool().expect("pool");
        let driver: Arc<dyn crate::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool));
        let store = HMemStore::from_driver(driver);
        let webid = WebID::from_persona(b"test-archive-user");
        let driver = store.driver();
        for i in 0..3 {
            driver
                .execute(
                    "INSERT INTO hmems (id, entity, attribute, value, valid_from, confidence, visibility, owner_webid)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    &[
                        DbValue::Text(format!("triple-{}", i)),
                        DbValue::Text("test:entity".to_string()),
                        DbValue::Text("test:attribute".to_string()),
                        DbValue::Text(format!("value-{}", i)),
                        DbValue::Text("2024-01-01T00:00:00Z".to_string()),
                        DbValue::Real(1.0),
                        DbValue::Text("private".to_string()),
                        DbValue::Text(webid.to_string()),
                    ],
                )
                .expect("insert");
        }
        let archive_path = temp_path("test");
        (store, archive_path, webid)
    }

    #[test]
    fn export_creates_archive_with_metadata() {
        let (store, archive_path, webid) = setup_h_mem_store();
        let archive = BackupArchive::create(
            archive_path.clone(),
            "test-passphrase-123",
            &store,
            &webid,
            "https://test.example",
        )
        .expect("create");
        assert!(archive_path.exists(), "archive file should exist");
        let meta = archive.metadata().expect("metadata");
        assert_eq!(meta.webid, webid.to_string());
        assert_eq!(meta.source_server_url, "https://test.example");
        assert_eq!(meta.triple_count, 3);
    }

    #[test]
    fn archive_rejects_wrong_passphrase() {
        let (store, archive_path, webid) = setup_h_mem_store();
        BackupArchive::create(
            archive_path.clone(),
            "test-passphrase-123",
            &store,
            &webid,
            "https://test.example",
        )
        .expect("create");
        let result = BackupArchive::open(archive_path, "wrong-passphrase");
        assert!(result.is_err(), "should reject wrong passphrase");
    }

    #[test]
    fn roundtrip_export_open_preserves_triples() {
        let (store, archive_path, webid) = setup_h_mem_store();
        BackupArchive::create(
            archive_path.clone(),
            "test-passphrase-123",
            &store,
            &webid,
            "https://test.example",
        )
        .expect("create");
        let archive = BackupArchive::open(archive_path, "test-passphrase-123").expect("open");
        let count = archive.triple_count().expect("count");
        assert_eq!(count, 3, "triple count should match");
    }

    #[test]
    fn import_into_empty_target() {
        let (source_store, archive_path, webid) = setup_h_mem_store();
        BackupArchive::create(
            archive_path.clone(),
            "test-passphrase-123",
            &source_store,
            &webid,
            "https://test.example",
        )
        .expect("create");
        let archive = BackupArchive::open(archive_path, "test-passphrase-123").expect("open");

        // Create a fresh empty target
        let pool = SqliteDriver::in_memory_pool().expect("pool");
        let driver: Arc<dyn crate::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool));
        let target = HMemStore::from_driver(driver);
        let receipt = archive.restore_into(&target, &webid).expect("restore");
        assert_eq!(receipt.triple_count, 3, "should import all triples");

        // Verify they landed
        let driver = target.driver();
        let count: i64 = driver
            .query(
                "SELECT COUNT(*) FROM hmems WHERE owner_webid = ?1",
                &[DbValue::Text(webid.to_string())],
            )
            .expect("query")
            .first()
            .and_then(|r| r.get(0).ok()?.as_int().ok())
            .unwrap_or(0);
        assert_eq!(count, 3, "target should have imported triples");
    }

    #[test]
    fn import_is_idempotent() {
        let (source_store, archive_path, webid) = setup_h_mem_store();
        BackupArchive::create(
            archive_path.clone(),
            "test-passphrase-123",
            &source_store,
            &webid,
            "https://test.example",
        )
        .expect("create");
        let archive = BackupArchive::open(archive_path, "test-passphrase-123").expect("open");

        let pool = SqliteDriver::in_memory_pool().expect("pool");
        let driver: Arc<dyn crate::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool));
        let target = HMemStore::from_driver(driver);
        archive
            .restore_into(&target, &webid)
            .expect("first restore");
        let receipt = archive
            .restore_into(&target, &webid)
            .expect("second restore");
        assert_eq!(receipt.triple_count, 3, "idempotent: count unchanged");

        let driver = target.driver();
        let count: i64 = driver
            .query(
                "SELECT COUNT(*) FROM hmems WHERE owner_webid = ?1",
                &[DbValue::Text(webid.to_string())],
            )
            .expect("query")
            .first()
            .and_then(|r| r.get(0).ok()?.as_int().ok())
            .unwrap_or(0);
        assert_eq!(count, 3, "idempotent: still 3 rows");
    }
}
