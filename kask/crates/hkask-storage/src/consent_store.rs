//! Consent Store — SQLite persistence for user consent records
//!
//! Persists consent records so they survive restarts, enforcing
//! user sovereignty (Principle 1.3) in the headless system.
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::{define_driver_store, impl_from_db_error};
use hkask_types::InfrastructureError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
/// Consent store errors
#[derive(Debug, Error)]
pub enum ConsentStoreError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
}
impl_from_db_error!(ConsentStoreError, Infra);
/// Persistent consent record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConsentRecord {
    pub id: String,
    pub webid: String,
    pub granted_categories: HashSet<String>,
    pub granted_at: i64,
    pub revoked_at: Option<i64>,
    pub active: bool,
}
define_driver_store!(ConsentStore);
impl ConsentStore {
    /// Initialize the consent_records table
    /// Initialize the consent store schema.
    ///
    /// expect: "My consent records are stored with explicit affirmative consent"
    /// \[P2\] Motivating: Affirmative Consent — schema for consent records
    /// post: consent_records table created if not exists
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS consent_records (
                id TEXT PRIMARY KEY,
                webid TEXT NOT NULL UNIQUE,
                granted_categories TEXT NOT NULL,
                granted_at INTEGER NOT NULL,
                revoked_at INTEGER,
                active INTEGER NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_consent_active ON consent_records(active);
            ",
        );
    }
    /// Store (upsert) a consent record for a WebID
    /// Store a consent record.
    ///
    /// expect: "My consent records are stored with explicit affirmative consent"
    /// \[P2\] Motivating: Affirmative Consent — persist a scoped consent record
    /// pre:  record.webid is non-empty
    /// post: record inserted or replaced in consent_records
    pub fn store(&self, record: &StoredConsentRecord) -> Result<(), ConsentStoreError> {
        let categories_json = serde_json::to_string(&record.granted_categories)
            .map_err(|e| ConsentStoreError::Infra(InfrastructureError::from(e)))?;
        let active_int: i64 = if record.active { 1 } else { 0 };
        self.driver.execute(
            "INSERT INTO consent_records (id, webid, granted_categories, granted_at, revoked_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(webid) DO UPDATE SET
                granted_categories = excluded.granted_categories,
                granted_at = excluded.granted_at,
                revoked_at = excluded.revoked_at,
                active = excluded.active",
            &[
                DbValue::Text(record.id.clone()),
                DbValue::Text(record.webid.clone()),
                DbValue::Text(categories_json),
                DbValue::Integer(record.granted_at),
                record.revoked_at.map_or(DbValue::Null, DbValue::Integer),
                DbValue::Integer(active_int),
            ],
        )?;
        Ok(())
    }
    /// Get the active consent record for a WebID
    /// Get a consent record by WebID.
    ///
    /// expect: "My consent records are stored with explicit affirmative consent"
    /// \[P2\] Motivating: Affirmative Consent — retrieve consent by WebID
    /// pre:  webid is non-empty
    /// post: returns Some(record) if found, None otherwise
    pub fn get(&self, webid: &str) -> Result<Option<StoredConsentRecord>, ConsentStoreError> {
        Ok(query_row(
            &*self.driver,
            "SELECT id, webid, granted_categories, granted_at, revoked_at, active
             FROM consent_records WHERE webid = ?1",
            &[DbValue::Text(webid.to_string())],
            Self::row_to_record,
        )?)
    }
    /// Delete consent record for a WebID
    /// Delete a consent record by WebID.
    ///
    /// expect: "My consent records are stored with explicit affirmative consent"
    /// \[P2\] Motivating: Affirmative Consent — delete a consent record
    /// pre:  webid is non-empty
    /// post: record deleted if existed
    pub fn delete(&self, webid: &str) -> Result<(), ConsentStoreError> {
        self.driver.execute(
            "DELETE FROM consent_records WHERE webid = ?1",
            &[DbValue::Text(webid.to_string())],
        )?;
        Ok(())
    }

    /// List all active consent records.
    pub fn list_active(&self) -> Result<Vec<StoredConsentRecord>, ConsentStoreError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, webid, granted_categories, granted_at, revoked_at, active
             FROM consent_records WHERE active = 1",
            &[],
            Self::row_to_record,
        )?)
    }

    /// Parse a database row into a StoredConsentRecord.
    fn row_to_record(
        row: &crate::database::value::DbRow,
    ) -> Result<StoredConsentRecord, crate::database::types::DbError> {
        let id: String = row.get_str(0)?.to_string();
        let webid: String = row.get_str(1)?.to_string();
        let categories_json: String = row.get_str(2)?.to_string();
        let granted_at: i64 = row.get_int(3)?;
        let revoked_at: Option<i64> = match row.get(4)? {
            DbValue::Null => None,
            v => Some(v.as_int()?),
        };
        let active_int: i64 = row.get_int(5)?;
        let granted_categories: HashSet<String> = serde_json::from_str(&categories_json)
            .map_err(|e| crate::database::types::DbError::Database(e.to_string()))?;
        Ok(StoredConsentRecord {
            id,
            webid,
            granted_categories,
            granted_at,
            revoked_at,
            active: active_int != 0,
        })
    }
}

// ── ConsentPort implementation ───────────────────────────────────────

impl hkask_types::consent_port::ConsentPort for ConsentStore {
    fn initialize_schema(&self) -> Result<(), InfrastructureError> {
        // Schema initialized by from_driver() via init_schema().
        Ok(())
    }

    fn store(
        &self,
        record: &hkask_types::consent_port::StoredConsentRecord,
    ) -> Result<(), InfrastructureError> {
        let local = StoredConsentRecord {
            id: record.id.clone(),
            webid: record.webid.clone(),
            granted_categories: record.granted_categories.clone(),
            granted_at: record.granted_at,
            revoked_at: record.revoked_at,
            active: record.active,
        };
        self.store(&local)
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }

    fn list_active(
        &self,
    ) -> Result<Vec<hkask_types::consent_port::StoredConsentRecord>, InfrastructureError> {
        self.list_active()
            .map(|records| {
                records
                    .into_iter()
                    .map(|r| hkask_types::consent_port::StoredConsentRecord {
                        id: r.id,
                        webid: r.webid,
                        granted_categories: r.granted_categories,
                        granted_at: r.granted_at,
                        revoked_at: r.revoked_at,
                        active: r.active,
                    })
                    .collect()
            })
            .map_err(|e| InfrastructureError::database(e.to_string()))
    }
}
