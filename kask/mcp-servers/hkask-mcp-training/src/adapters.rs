//! Training job persistence and adapter metrics.
//!
//! Adapter metadata is now stored via `crate::adapter::AdapterStore` (canonical).
//! This module retains `JobStore` (training job persistence) and `AdapterMetrics`
//! (metrics struct serialized into `TrainedLoRAAdapter.expertise.training_source.training_metrics`).

use serde::{Deserialize, Serialize};

// ── Adapter metrics ───────────────────────────────────────────────────────

/// Training metrics recorded at adapter creation time.
/// Serialized into `TrainedLoRAAdapter.expertise.training_source.training_metrics`
/// as a `serde_json::Value`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMetrics {
    /// Final training loss.
    pub loss: Option<f32>,
    /// Perplexity at end of training.
    pub perplexity: Option<f32>,
    /// Training duration in seconds.
    pub training_duration_secs: Option<u64>,
    /// Number of tokens processed.
    pub tokens_processed: Option<u64>,
}

// ── Store errors ───────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AdapterStoreError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

// Helper to execute and discard row count
fn exec_discard(
    conn: &rusqlite::Connection,
    sql: &str,
    params: &[&dyn rusqlite::types::ToSql],
) -> Result<(), AdapterStoreError> {
    conn.execute(sql, params)
        .map(|_| ())
        .map_err(|e| AdapterStoreError::Storage(format!("Execute failed: {}", e)))
}

// ── Job store ───────────────────────────────────────────────────────────

/// Persisted training job record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredJob {
    pub id: String,
    pub base_model: String,
    pub dataset_path: String,
    pub params_json: String,
    pub status: String,
    pub created_at: i64,
    pub host: String,
}

/// Persistent job registry backed by the same SQLite database.
/// Survives server restarts — enables `training_status` to look up
/// original job parameters (and `training_submit` retrain mode to
/// pre-register adapter metadata).
pub struct JobStore {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

impl JobStore {
    /// Create a job store and initialize its owned persistence schema.
    ///
    /// expect: "Training jobs survive server restarts without external migrations."
    /// [P5] Motivating: JobStore owns the minimum schema required by its public operations.
    /// post: the training_jobs table and restart-recovery columns exist.
    /// [P4] Constraining: persistence initialization fails explicitly rather than weakening boundaries.
    pub fn new(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Result<Self, AdapterStoreError> {
        let store = Self { pool };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), AdapterStoreError> {
        let conn = self.lock()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_jobs (
                id TEXT PRIMARY KEY,
                base_model TEXT NOT NULL,
                dataset_path TEXT NOT NULL,
                params_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'queued',
                created_at INTEGER NOT NULL,
                host TEXT NOT NULL,
                artifact_manifest_json TEXT,
                provider_job_id TEXT
            );",
        )
        .map_err(|error| AdapterStoreError::Storage(format!("Initialize schema: {error}")))?;

        for (column, definition) in [
            ("artifact_manifest_json", "TEXT"),
            ("provider_job_id", "TEXT"),
        ] {
            let mut statement = conn
                .prepare("PRAGMA table_info(training_jobs)")
                .map_err(|error| AdapterStoreError::Storage(format!("Inspect schema: {error}")))?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|error| AdapterStoreError::Storage(format!("Inspect schema: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| AdapterStoreError::Storage(format!("Inspect schema: {error}")))?;
            drop(statement);
            if !columns.iter().any(|existing| existing == column) {
                conn.execute_batch(&format!(
                    "ALTER TABLE training_jobs ADD COLUMN {column} {definition};"
                ))
                .map_err(|error| {
                    AdapterStoreError::Storage(format!("Migrate {column}: {error}"))
                })?;
            }
        }
        Ok(())
    }

    fn lock(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, AdapterStoreError>
    {
        self.pool
            .get()
            .map_err(|e| AdapterStoreError::Storage(format!("pool get: {}", e)))
    }

    /// Store a new training job.
    #[allow(clippy::too_many_arguments)]
    pub fn store(
        &self,
        id: &str,
        base_model: &str,
        dataset_path: &str,
        params_json: &str,
        status: &str,
        created_at: i64,
        host: &str,
    ) -> Result<(), AdapterStoreError> {
        let conn = self.lock()?;
        exec_discard(
            &conn,
            "INSERT OR REPLACE INTO training_jobs
             (id, base_model, dataset_path, params_json, status, created_at, host)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                &id as &dyn rusqlite::types::ToSql,
                &base_model as &dyn rusqlite::types::ToSql,
                &dataset_path as &dyn rusqlite::types::ToSql,
                &params_json as &dyn rusqlite::types::ToSql,
                &status as &dyn rusqlite::types::ToSql,
                &created_at as &dyn rusqlite::types::ToSql,
                &host as &dyn rusqlite::types::ToSql,
            ],
        )?;
        Ok(())
    }

    /// Persist the concrete remote-training artifact manifest before dispatch.
    pub fn update_artifacts(
        &self,
        job_id: &str,
        artifacts: &crate::huggingface::TrainingArtifacts,
    ) -> Result<(), AdapterStoreError> {
        let manifest = serde_json::to_string(artifacts)
            .map_err(|error| AdapterStoreError::Serialization(error.to_string()))?;
        let conn = self.lock()?;
        exec_discard(
            &conn,
            "UPDATE training_jobs SET artifact_manifest_json = ?1 WHERE id = ?2",
            &[
                &manifest as &dyn rusqlite::types::ToSql,
                &job_id as &dyn rusqlite::types::ToSql,
            ],
        )
    }

    /// Persist the remote provider's job identifier after successful dispatch.
    pub fn update_provider_job_id(
        &self,
        job_id: &str,
        provider_job_id: &str,
    ) -> Result<(), AdapterStoreError> {
        let conn = self.lock()?;
        exec_discard(
            &conn,
            "UPDATE training_jobs SET provider_job_id = ?1 WHERE id = ?2",
            &[
                &provider_job_id as &dyn rusqlite::types::ToSql,
                &job_id as &dyn rusqlite::types::ToSql,
            ],
        )
    }

    /// Read the concrete artifact manifest for restart-safe provider recovery.
    pub fn artifacts(
        &self,
        job_id: &str,
    ) -> Result<Option<crate::huggingface::TrainingArtifacts>, AdapterStoreError> {
        let conn = self.lock()?;
        let manifest: Option<String> = match conn.query_row(
            "SELECT artifact_manifest_json FROM training_jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0),
        ) {
            Ok(manifest) => manifest,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => {
                return Err(AdapterStoreError::Storage(format!(
                    "Read artifact manifest: {error}"
                )));
            }
        };
        manifest
            .map(|manifest| {
                serde_json::from_str(&manifest)
                    .map_err(|error| AdapterStoreError::Serialization(error.to_string()))
            })
            .transpose()
    }

    /// Update job status.
    pub fn update_status(&self, job_id: &str, status: &str) -> Result<(), AdapterStoreError> {
        let conn = self.lock()?;
        exec_discard(
            &conn,
            "UPDATE training_jobs SET status = ?1 WHERE id = ?2",
            &[
                &status as &dyn rusqlite::types::ToSql,
                &job_id as &dyn rusqlite::types::ToSql,
            ],
        )
    }

    /// Get a job by ID.
    pub fn get(&self, job_id: &str) -> Result<Option<StoredJob>, AdapterStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, base_model, dataset_path, params_json, status, created_at, host
                     FROM training_jobs WHERE id = ?1",
            )
            .map_err(|e| AdapterStoreError::Storage(format!("Query failed: {}", e)))?;

        let result = stmt.query_row(rusqlite::params![job_id], |row| {
            Ok(StoredJob {
                id: row.get(0)?,
                base_model: row.get(1)?,
                dataset_path: row.get(2)?,
                params_json: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                host: row.get(6)?,
            })
        });

        match result {
            Ok(job) => Ok(Some(job)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AdapterStoreError::Storage(format!("Query failed: {}", e))),
        }
    }

    /// List all jobs, most recent first.
    pub fn list_all(&self) -> Result<Vec<StoredJob>, AdapterStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, base_model, dataset_path, params_json, status, created_at, host
                     FROM training_jobs ORDER BY created_at DESC",
            )
            .map_err(|e| AdapterStoreError::Storage(format!("Query failed: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(StoredJob {
                    id: row.get(0)?,
                    base_model: row.get(1)?,
                    dataset_path: row.get(2)?,
                    params_json: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                    host: row.get(6)?,
                })
            })
            .map_err(|e| AdapterStoreError::Storage(format!("Query failed: {}", e)))?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|e| AdapterStoreError::Storage(format!("Row error: {}", e)))?);
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::Database;

    fn setup_db() -> Database {
        Database::in_memory().expect("in-memory db")
    }

    /// expect: JobStore creates and upgrades the schema it owns.
    /// [P5] Motivating: training persistence must not depend on an ambient migration.
    /// post: legacy tables gain artifact and provider recovery columns.
    #[test]
    fn job_store_initializes_and_migrates_owned_schema() {
        let db = setup_db();
        let pool = db.sqlite_pool().expect("pool");
        let conn = pool.get().expect("connection");
        conn.execute_batch(
            "CREATE TABLE training_jobs (
                id TEXT PRIMARY KEY,
                base_model TEXT NOT NULL,
                dataset_path TEXT NOT NULL,
                params_json TEXT NOT NULL DEFAULT '{}',
                status TEXT NOT NULL DEFAULT 'queued',
                created_at INTEGER NOT NULL,
                host TEXT NOT NULL
            );",
        )
        .expect("legacy schema");
        drop(conn);

        let _store = JobStore::new(pool.clone()).expect("migrate job store");
        let conn = pool.get().expect("connection");
        let mut statement = conn
            .prepare("PRAGMA table_info(training_jobs)")
            .expect("inspect schema");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect columns");
        assert!(
            columns
                .iter()
                .any(|column| column == "artifact_manifest_json")
        );
        assert!(columns.iter().any(|column| column == "provider_job_id"));
    }

    #[test]
    fn job_store_store_and_get() {
        let db = setup_db();
        let pool = db.sqlite_pool().expect("pool");
        let store = JobStore::new(pool).expect("initialize job store");

        store
            .store(
                "job-1",
                "Qwen3.5-9B",
                "/data/train.jsonl",
                "{}",
                "queued",
                1000,
                "runpod",
            )
            .expect("store");

        let job = store.get("job-1").expect("get").expect("found");
        assert_eq!(job.base_model, "Qwen3.5-9B");
        assert_eq!(job.status, "queued");
        assert_eq!(job.host, "runpod");
    }

    #[test]
    fn job_store_update_status() {
        let db = setup_db();
        let pool = db.sqlite_pool().expect("pool");
        let store = JobStore::new(pool).expect("initialize job store");

        store
            .store(
                "job-2",
                "model",
                "/data.jsonl",
                "{}",
                "queued",
                2000,
                "axolotl",
            )
            .expect("store");

        store.update_status("job-2", "running").expect("update");

        let job = store.get("job-2").expect("get").expect("found");
        assert_eq!(job.status, "running");
    }

    #[test]
    fn job_store_list_all() {
        let db = setup_db();
        let pool = db.sqlite_pool().expect("pool");
        let store = JobStore::new(pool).expect("initialize job store");

        store
            .store("j1", "m1", "/d1", "{}", "queued", 100, "t")
            .expect("store");
        store
            .store("j2", "m2", "/d2", "{}", "completed", 200, "t")
            .expect("store");

        let all = store.list_all().expect("list");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "j2");
        assert_eq!(all[1].id, "j1");
    }

    #[test]
    fn job_store_missing_returns_none() {
        let db = setup_db();
        let pool = db.sqlite_pool().expect("pool");
        let store = JobStore::new(pool).expect("initialize job store");

        let result = store.get("nonexistent").expect("get");
        assert!(result.is_none());
    }
}
