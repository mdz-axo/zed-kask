//! HMemStore — the h_mem storage adapter over `StorageDriver`.
//!
//! Moved from `hkask-storage::hmem` during the T0.6 port-ification. The store
//! is provider-agnostic: it uses the `StorageDriver` port (defined in
//! `hkask_types::storage`) instead of `rusqlite`/`r2d2`. The `kask_bridge`
//! implements `StorageDriver` over zed's `sqlez`.
//!
//! Encryption is handled at the bridge layer (application-layer encryption
//! per the architecture plan), not in this adapter.

use chrono::{DateTime, Utc};
use hkask_types::storage::{DbRow, DbValue, StorageDriver};
use hkask_types::time::now_rfc3339;
use hkask_types::visibility::AccessControl;
use hkask_types::{
    Confidence, HMem, HMemError, HMemId, InfrastructureError, NotFound, Visibility,
    WebID,
};
use serde_json::Value;
use std::sync::Arc;
use tracing;

#[derive(Clone)]
pub struct HMemStore {
    driver: Arc<dyn StorageDriver>,
}

impl HMemStore {
    pub fn from_driver(driver: Arc<dyn StorageDriver>) -> Self {
        let store = Self { driver };
        let _ = store.driver().execute_batch(
            "CREATE TABLE IF NOT EXISTS hmems (
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
                dimension INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_hmems_entity ON hmems(entity);
            CREATE INDEX IF NOT EXISTS idx_hmems_attribute ON hmems(attribute);
            CREATE INDEX IF NOT EXISTS idx_hmems_entity_attribute ON hmems(entity, attribute);",
        );
        store
    }

    pub fn driver(&self) -> &Arc<dyn StorageDriver> {
        &self.driver
    }
}

const HMEM_COLUMNS: &str = "id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, dimension";

struct HMemRow {
    id: HMemId,
    entity: String,
    attribute: String,
    value: String,
    valid_from: String,
    recalled_at: String,
    confidence: Confidence,
    perspective: Option<WebID>,
    visibility: Visibility,
    owner_webid: WebID,
    dimension: Option<String>,
}

impl HMemStore {
    fn exec(&self, sql: &str, params: &[DbValue]) -> Result<usize, HMemError> {
        self.driver
            .execute(sql, params)
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))
    }

    fn query_rows(&self, sql: &str, params: &[DbValue]) -> Result<Vec<HMem>, HMemError> {
        let rows = self
            .driver
            .query(sql, params)
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let mut results = Vec::with_capacity(rows.len());
        for row in &rows {
            match self.row_to_h_mem(row) {
                Ok(h) => results.push(h),
                Err(e) => {
                    tracing::error!(target: "reg.storage.corruption", error = %e, "Corrupted database row — propagating error for regulator visibility");
                    return Err(e);
                }
            }
        }
        Ok(results)
    }

    fn row_to_h_mem(&self, row: &DbRow) -> Result<HMem, HMemError> {
        let value_text = row.get(3)?.as_text()?.to_string();
        let hrow = HMemRow {
            id: row.get(0)?.as_text()?.parse().map_err(|_| {
                HMemError::Infra(InfrastructureError::database("invalid id"))
            })?,
            entity: row.get(1)?.as_text()?.to_string(),
            attribute: row.get(2)?.as_text()?.to_string(),
            value: value_text,
            valid_from: row.get(4)?.as_text()?.to_string(),
            recalled_at: row.get(6)?.as_text()?.to_string(),
            confidence: Confidence::new(row.get(7)?.as_real()?),
            perspective: row.get(8)?.as_text().ok().and_then(|s| s.parse().ok()),
            visibility: match row.get(9)?.as_text().unwrap_or("private") {
                "public" => Visibility::Public,
                "shared" => Visibility::Shared,
                _ => Visibility::Private,
            },
            owner_webid: row.get(10)?.as_text()?.parse().map_err(|_| {
                HMemError::Infra(InfrastructureError::database("invalid webid"))
            })?,
            dimension: row.get(11)?.as_text().ok().map(|s| s.to_string()),
        };
        Self::row_to_triple(hrow)
    }

    fn count_rows(&self, sql: &str, params: &[DbValue]) -> Result<usize, HMemError> {
        let rows = self
            .driver
            .query(sql, params)
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        Ok(rows
            .first()
            .and_then(|r| r.get(0).ok())
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize)
    }

    fn row_to_triple(row: HMemRow) -> Result<HMem, HMemError> {
        let value: Value = serde_json::from_str(&row.value)?;
        let valid_from = DateTime::parse_from_rfc3339(&row.valid_from)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                HMemError::Infra(InfrastructureError::database(format!(
                    "corrupt valid_from timestamp '{}': {}",
                    row.valid_from, e
                )))
            })?;
        let recalled_at = DateTime::parse_from_rfc3339(&row.recalled_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(valid_from);
        Ok(HMem {
            id: row.id,
            entity: row.entity,
            attribute: row.attribute,
            value,
            observed_at: valid_from,
            confidence: row.confidence,
            access: AccessControl {
                perspective: row.perspective,
                visibility: row.visibility,
                owner_webid: row.owner_webid,
            },
            recalled_at,
            dimension: row.dimension.and_then(|s| s.parse().ok()),
        })
    }
}

impl HMemStore {
    pub fn insert(&self, h_mem: &HMem) -> Result<(), HMemError> {
        let value_json = serde_json::to_string(&h_mem.value)?;
        self.exec(
            &format!("INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
            &[
                DbValue::Text(h_mem.id.to_string()),
                DbValue::Text(h_mem.entity.clone()),
                DbValue::Text(h_mem.attribute.clone()),
                DbValue::Text(value_json),
                DbValue::Text(h_mem.observed_at.to_rfc3339()),
                DbValue::Null,
                DbValue::Text(h_mem.recalled_at.to_rfc3339()),
                DbValue::Real(h_mem.confidence.value()),
                h_mem.access.perspective.as_ref().map_or(DbValue::Null, |p| DbValue::Text(p.to_string())),
                DbValue::Text(h_mem.access.visibility.to_string()),
                DbValue::Text(h_mem.access.owner_webid.to_string()),
                h_mem.dimension.as_ref().map_or(DbValue::Null, |d| DbValue::Text(d.as_str().to_string())),
            ],
        )?;
        Ok(())
    }

    #[must_use = "result must be used"]
    pub fn query_by_entity(&self, entity: &str) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string())],
        )
    }

    pub fn query_by_entity_attribute(&self, entity: &str, attribute: &str) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 AND attribute = ?2 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string()), DbValue::Text(attribute.to_string())],
        )
    }

    pub fn query_by_perspective(&self, perspective: &WebID) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(perspective.to_string())],
        )
    }

    #[must_use = "result must be used"]
    pub fn query_by_attribute(&self, attribute: &str) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE attribute = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(attribute.to_string())],
        )
    }

    pub fn update(&self, id: &HMemId, new_value: Value, new_confidence: impl Into<Confidence>) -> Result<(), HMemError> {
        let new_confidence = new_confidence.into();
        let now = now_rfc3339();
        self.driver.execute_batch("BEGIN").map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let result = (|| -> Result<(), HMemError> {
            self.driver.execute(
                "UPDATE hmems SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
                &[DbValue::Text(now.clone()), DbValue::Text(id.to_string())],
            ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            let rows = self.driver.query(
                "SELECT entity, attribute, perspective, visibility, owner_webid, dimension FROM hmems WHERE id = ?1",
                &[DbValue::Text(id.to_string())],
            ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            let row = rows.first().ok_or_else(|| HMemError::NotFound(NotFound {
                entity_type: "h_mem".to_string(), id: id.to_string(),
            }))?;
            let entity = row.get(0)?.as_text()?.to_string();
            let attribute = row.get(1)?.as_text()?.to_string();
            let perspective: Option<String> = row.get(2)?.as_text().ok().map(|s| s.to_string());
            let visibility = row.get(3)?.as_text()?.to_string();
            let owner_webid = row.get(4)?.as_text()?.to_string();
            let dimension: Option<String> = row.get(5)?.as_text().ok().map(|s| s.to_string());
            let new_id = HMemId::new();
            self.driver.execute(
                &format!("INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
                &[
                    DbValue::Text(new_id.to_string()), DbValue::Text(entity), DbValue::Text(attribute),
                    DbValue::Text(serde_json::to_string(&new_value)?), DbValue::Text(now.clone()),
                    DbValue::Null, DbValue::Text(now.clone()), DbValue::Real(new_confidence.value()),
                    perspective.map_or(DbValue::Null, DbValue::Text),
                    DbValue::Text(visibility), DbValue::Text(owner_webid),
                    dimension.map_or(DbValue::Null, DbValue::Text),
                ],
            ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            Ok(())
        })();
        match result {
            Ok(()) => { self.driver.execute_batch("COMMIT").map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?; Ok(()) }
            Err(e) => { let _ = self.driver.execute_batch("ROLLBACK"); Err(e) }
        }
    }

    #[must_use = "result must be used"]
    pub fn get_by_id(&self, id: &HMemId) -> Result<Option<HMem>, HMemError> {
        let results = self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE id = ?1 AND valid_to IS NULL"),
            &[DbValue::Text(id.to_string())],
        )?;
        Ok(results.into_iter().next())
    }

    pub fn touch_recall(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "UPDATE hmems SET recalled_at = ?1 WHERE id = ?2 AND valid_to IS NULL",
            &[DbValue::Text(now_rfc3339()), DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }

    pub fn query_semantic_lowest_confidence(&self, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL ORDER BY confidence ASC, valid_from ASC LIMIT ?1"),
            &[DbValue::Integer(limit as i64)],
        )
    }

    pub fn count_semantic_below_confidence(&self, threshold: f64) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND confidence <= ?1",
            &[DbValue::Real(threshold)],
        )
    }

    pub fn query_semantic_below_confidence(&self, threshold: f64, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND confidence <= ?1 ORDER BY confidence ASC, valid_from ASC LIMIT ?2"),
            &[DbValue::Real(threshold), DbValue::Integer(limit as i64)],
        )
    }

    pub fn count_semantic(&self) -> Result<usize, HMemError> {
        self.count_rows("SELECT COUNT(*) FROM hmems WHERE perspective IS NULL AND valid_to IS NULL", &[])
    }

    pub fn count_semantic_by_entity(&self, entity: &str) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE entity = ?1 AND perspective IS NULL AND valid_to IS NULL",
            &[DbValue::Text(entity.to_string())],
        )
    }

    pub fn count_by_perspective(&self, perspective: &WebID) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE perspective = ?1 AND valid_to IS NULL",
            &[DbValue::Text(perspective.to_string())],
        )
    }

    pub fn query_semantic_older_than(&self, days: u32, limit: usize) -> Result<Vec<HMem>, HMemError> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND valid_from < ?1 ORDER BY entity ASC, confidence DESC, valid_from DESC LIMIT ?2"),
            &[DbValue::Text(cutoff), DbValue::Integer(limit as i64)],
        )
    }

    pub fn close_by_id(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "UPDATE hmems SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
            &[DbValue::Text(now_rfc3339()), DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }

    pub fn delete_by_id(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec("DELETE FROM hmems WHERE id = ?1", &[DbValue::Text(id.to_string())])?;
        Ok(())
    }

    pub fn delete_by_entity_prefix(&self, prefix: &str) -> Result<usize, HMemError> {
        self.exec("DELETE FROM hmems WHERE entity LIKE ?1", &[DbValue::Text(format!("{}%", prefix))])
    }
}
