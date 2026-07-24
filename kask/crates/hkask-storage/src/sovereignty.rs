//! Sovereignty Boundary Store — SQLite persistence for user sovereignty boundaries
//
//! Persists user-configured sovereignty boundaries including:
//! - Sovereign data categories (require explicit consent)
//! - Shared data categories (require consent)
//! - Public data categories (always accessible)
//! - Affirmative consent requirements
use crate::database::driver::query_row;
use crate::database::value::DbValue;
use crate::{define_driver_store, impl_from_db_error};
use hkask_types::InfrastructureError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
/// Sovereignty boundary store errors
#[derive(Debug, Error)]
pub enum SovereigntyStoreError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("UUID parse error: {0}")]
    UuidParse(String),
}
impl_from_db_error!(SovereigntyStoreError, Infra);
/// Stored sovereignty boundary entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereigntyBoundaryEntry {
    pub id: String,
    pub webid: String,
    pub sovereign_categories: Vec<String>,
    pub shared_categories: Vec<String>,
    pub public_categories: Vec<String>,
    pub requires_affirmative_consent: String,
    pub created_at: i64,
    pub updated_at: i64,
}
define_driver_store!(SovereigntyBoundaryStore);
impl SovereigntyBoundaryStore {
    /// Initialize the database schema
    ///
    /// Creates the `sovereignty_boundaries` table if it doesn't exist and
    /// applies any pending migrations (column renames, column drops from
    /// Magna Carta refactoring).
    /// Initialize the sovereignty boundary store schema.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — schema for sovereignty boundaries
    /// post: sovereignty_boundaries table created if not exists
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sovereignty_boundaries (
                id TEXT PRIMARY KEY,
                webid TEXT NOT NULL UNIQUE,
                sovereign_categories TEXT NOT NULL,
                shared_categories TEXT NOT NULL,
                public_categories TEXT NOT NULL,
                requires_affirmative_consent TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sovereignty_webid ON sovereignty_boundaries(webid);
            CREATE INDEX IF NOT EXISTS idx_sovereignty_updated ON sovereignty_boundaries(updated_at);
            "
        );
    }
    /// Store sovereignty boundary for a WebID
    /// Store a sovereignty boundary entry.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — store a sovereignty boundary entry
    /// pre:  entry.webid is non-empty
    /// post: entry inserted or replaced
    pub fn store(&self, entry: &SovereigntyBoundaryEntry) -> Result<(), SovereigntyStoreError> {
        let sovereign_json = serde_json::to_string(&entry.sovereign_categories)
            .map_err(|e| SovereigntyStoreError::Infra(InfrastructureError::from(e)))?;
        let shared_json = serde_json::to_string(&entry.shared_categories)
            .map_err(|e| SovereigntyStoreError::Infra(InfrastructureError::from(e)))?;
        let public_json = serde_json::to_string(&entry.public_categories)
            .map_err(|e| SovereigntyStoreError::Infra(InfrastructureError::from(e)))?;
        let now = chrono::Utc::now().timestamp();
        self.driver.execute(
            "INSERT INTO sovereignty_boundaries
             (id, webid, sovereign_categories, shared_categories, public_categories,
              requires_affirmative_consent, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(webid) DO UPDATE SET
                sovereign_categories = excluded.sovereign_categories,
                shared_categories = excluded.shared_categories,
                public_categories = excluded.public_categories,
                requires_affirmative_consent = excluded.requires_affirmative_consent,
                updated_at = excluded.updated_at",
            &[
                DbValue::Text(entry.id.clone()),
                DbValue::Text(entry.webid.clone()),
                DbValue::Text(sovereign_json),
                DbValue::Text(shared_json),
                DbValue::Text(public_json),
                DbValue::Text(entry.requires_affirmative_consent.clone()),
                DbValue::Integer(entry.created_at),
                DbValue::Integer(now),
            ],
        )?;
        Ok(())
    }
    /// Get sovereignty boundary for a WebID
    /// Get sovereignty boundary entries for a WebID.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — get boundaries for a WebID
    /// pre:  webid is non-empty
    /// post: returns Some(entry) if found, None otherwise
    pub fn get(
        &self,
        webid: &str,
    ) -> Result<Option<SovereigntyBoundaryEntry>, SovereigntyStoreError> {
        Ok(query_row(
            &*self.driver,
            "SELECT id, webid, sovereign_categories, shared_categories, public_categories,
                    requires_affirmative_consent, created_at, updated_at
             FROM sovereignty_boundaries WHERE webid = ?1",
            &[DbValue::Text(webid.to_string())],
            |row| {
                let id: String = row.get_str(0)?.to_string();
                let webid: String = row.get_str(1)?.to_string();
                let sovereign_json: String = row.get_str(2)?.to_string();
                let shared_json: String = row.get_str(3)?.to_string();
                let public_json: String = row.get_str(4)?.to_string();
                let requires_affirmative_consent: String = row.get_str(5)?.to_string();
                let created_at: i64 = row.get_int(6)?;
                let updated_at: i64 = row.get_int(7)?;
                let sovereign_categories: Vec<String> = serde_json::from_str(&sovereign_json)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                let shared_categories: Vec<String> = serde_json::from_str(&shared_json)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                let public_categories: Vec<String> = serde_json::from_str(&public_json)
                    .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
                Ok(SovereigntyBoundaryEntry {
                    id,
                    webid,
                    sovereign_categories,
                    shared_categories,
                    public_categories,
                    requires_affirmative_consent,
                    created_at,
                    updated_at,
                })
            },
        )?)
    }
    /// Delete sovereignty boundary for a WebID
    /// Delete sovereignty boundary entries for a WebID.
    ///
    /// expect: "My user data and sovereignty boundaries are stored under my control"
    /// \[P1\] Motivating: User Sovereignty — delete boundaries for a WebID
    /// pre:  webid is non-empty
    /// post: entries deleted for this WebID
    pub fn delete(&self, webid: &str) -> Result<(), SovereigntyStoreError> {
        self.driver.execute(
            "DELETE FROM sovereignty_boundaries WHERE webid = ?1",
            &[DbValue::Text(webid.to_string())],
        )?;
        Ok(())
    }
}
