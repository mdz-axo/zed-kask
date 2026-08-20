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
pub enum JobStoreError {
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
) -> Result<(), JobStoreError> {
    conn.execute(sql, params)
        .map(|_| ())
        .map_err(|e| JobStoreError::Storage(format!("Execute failed: {}", e)))
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
    ) -> Result<Self, JobStoreError> {
        let store = Self { pool };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), JobStoreError> {
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
        .map_err(|error| JobStoreError::Storage(format!("Initialize schema: {error}")))?;

        for (column, definition) in [
            ("artifact_manifest_json", "TEXT"),
            ("provider_job_id", "TEXT"),
        ] {
            let mut statement = conn
                .prepare("PRAGMA table_info(training_jobs)")
                .map_err(|error| JobStoreError::Storage(format!("Inspect schema: {error}")))?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|error| JobStoreError::Storage(format!("Inspect schema: {error}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| JobStoreError::Storage(format!("Inspect schema: {error}")))?;
            drop(statement);
            if !columns.iter().any(|existing| existing == column) {
                conn.execute_batch(&format!(
                    "ALTER TABLE training_jobs ADD COLUMN {column} {definition};"
                ))
                .map_err(|error| JobStoreError::Storage(format!("Migrate {column}: {error}")))?;
            }
        }
        Ok(())
    }

    fn lock(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, JobStoreError> {
        self.pool
            .get()
            .map_err(|e| JobStoreError::Storage(format!("pool get: {}", e)))
    }

    /// Store a new training job.
    pub fn store(
        &self,
        id: &str,
        base_model: &str,
        dataset_path: &str,
        params_json: &str,
        status: &str,
        created_at: i64,
        host: &str,
    ) -> Result<(), JobStoreError> {
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
    ) -> Result<(), JobStoreError> {
        let manifest = serde_json::to_string(artifacts)
            .map_err(|error| JobStoreError::Serialization(error.to_string()))?;
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
    ) -> Result<(), JobStoreError> {
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
    ) -> Result<Option<crate::huggingface::TrainingArtifacts>, JobStoreError> {
        let conn = self.lock()?;
        let manifest: Option<String> = match conn.query_row(
            "SELECT artifact_manifest_json FROM training_jobs WHERE id = ?1",
            rusqlite::params![job_id],
            |row| row.get(0),
        ) {
            Ok(manifest) => manifest,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => {
                return Err(JobStoreError::Storage(format!(
                    "Read artifact manifest: {error}"
                )));
            }
        };
        manifest
            .map(|manifest| {
                serde_json::from_str(&manifest)
                    .map_err(|error| JobStoreError::Serialization(error.to_string()))
            })
            .transpose()
    }

    /// Update job status.
    pub fn update_status(&self, job_id: &str, status: &str) -> Result<(), JobStoreError> {
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
    pub fn get(&self, job_id: &str) -> Result<Option<StoredJob>, JobStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, base_model, dataset_path, params_json, status, created_at, host
                     FROM training_jobs WHERE id = ?1",
            )
            .map_err(|e| JobStoreError::Storage(format!("Query failed: {}", e)))?;

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
            Err(e) => Err(JobStoreError::Storage(format!("Query failed: {}", e))),
        }
    }

    /// List all jobs, most recent first.
    pub fn list_all(&self) -> Result<Vec<StoredJob>, JobStoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, base_model, dataset_path, params_json, status, created_at, host
                     FROM training_jobs ORDER BY created_at DESC",
            )
            .map_err(|e| JobStoreError::Storage(format!("Query failed: {}", e)))?;

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
            .map_err(|e| JobStoreError::Storage(format!("Query failed: {}", e)))?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|e| JobStoreError::Storage(format!("Row error: {}", e)))?);
        }
        Ok(jobs)
    }
}
