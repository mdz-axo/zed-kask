//! Uni-temporal h_mems — entity/attribute/value with observed_at timestamp.
pub mod archive;

use crate::database::value::{DbRow, DbValue};
use chrono::{DateTime, Utc};
use hkask_types::git_cas::HMemEntry;
use hkask_types::id::{HMemId, WebID};
use hkask_types::time::now_rfc3339;
use hkask_types::visibility::AccessControl;
use hkask_types::{Confidence, Dimension, InfrastructureError, NotFound, Visibility};
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;
#[derive(Error, Debug)]
pub enum HMemError {
    #[error(transparent)]
    Infra(#[from] InfrastructureError),
    #[error("{0}")]
    NotFound(NotFound),
}

impl From<NotFound> for HMemError {
    fn from(nf: NotFound) -> Self {
        HMemError::NotFound(nf)
    }
}

impl From<crate::database::types::DbError> for HMemError {
    fn from(e: crate::database::types::DbError) -> Self {
        HMemError::Infra(InfrastructureError::from(e))
    }
}

impl From<serde_json::Error> for HMemError {
    fn from(e: serde_json::Error) -> Self {
        HMemError::Infra(InfrastructureError::from(e))
    }
}
/// Bitemporal h_mem
#[derive(Debug, Clone)]
pub struct HMem {
    pub id: HMemId,
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    /// When this memory was formed (observation timestamp).
    pub observed_at: DateTime<Utc>,
    pub confidence: Confidence,
    pub access: AccessControl,
    /// Last time this h_mem was recalled. Starts at creation time.
    /// Updated on each recall — resets the decay clock.
    pub recalled_at: DateTime<Utc>,
    /// 5W1H dimension — which curator ontology category this h_mem belongs to.
    /// Maps to `OntologyAnchor::Core` (universal ground). None = unclassified.
    pub dimension: Option<Dimension>,
}
impl HMem {
    /// Create a new HMem with required fields.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — create a h_mem
    /// \[P1\] Constraining: User Sovereignty — owner_webid carries ownership
    /// pre:  entity and attribute are non-empty, owner_webid is valid
    /// post: returns HMem with defaults for temporal, confidence, access
    pub fn new(entity: &str, attribute: &str, value: Value, owner_webid: WebID) -> Self {
        let now = Utc::now();
        Self {
            id: HMemId::new(),
            entity: entity.to_string(),
            attribute: attribute.to_string(),
            value,
            observed_at: now,
            confidence: Confidence::full(),
            access: AccessControl::new(owner_webid),
            recalled_at: now,
            dimension: None,
        }
    }
    /// Set confidence on a HMem.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set confidence
    /// post: returns Self with confidence set (builder pattern)
    pub fn with_confidence(mut self, c: impl Into<Confidence>) -> Self {
        self.confidence = c.into();
        self
    }
    /// Set perspective on a HMem.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set perspective
    /// post: returns Self with perspective set (builder pattern)
    pub fn with_perspective(mut self, p: WebID) -> Self {
        self.access = self.access.with_perspective(p);
        self
    }
    /// Set visibility on a HMem.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set visibility
    /// post: returns Self with visibility set (builder pattern)
    pub fn with_visibility(mut self, v: Visibility) -> Self {
        self.access = self.access.with_visibility(v);
        self
    }
    /// Set 5W1H dimension on a HMem.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set dimension
    /// \[P8\] Constraining: Semantic Grounding — anchors to 5W1H ontology tier
    /// post: returns Self with dimension set (builder pattern)
    pub fn with_dimension(mut self, d: Dimension) -> Self {
        self.dimension = Some(d);
        self
    }
    /// Check if this is an episodic h_mem (has perspective).
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — predicate for episodic
    /// post: returns true iff perspective is Some
    pub fn is_episodic(&self) -> bool {
        self.access.is_episodic()
    }
    /// Check if this is a semantic h_mem (public, no perspective).
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — predicate for semantic
    /// post: returns true iff visibility is Public and perspective is None
    pub fn is_semantic(&self) -> bool {
        self.access.is_semantic()
    }
}
/// HMem store — backed by a provider-agnostic DatabaseDriver.
#[derive(Clone)]
pub struct HMemStore {
    driver: Arc<dyn crate::database::driver::DatabaseDriver>,
    encryptor: Option<Arc<crate::database::encrypt::Encryptor>>,
}

impl HMemStore {
    /// Create from a DatabaseDriver — provider-agnostic constructor.
    pub fn from_driver(driver: Arc<dyn crate::database::driver::DatabaseDriver>) -> Self {
        let store = Self {
            driver,
            encryptor: None,
        };
        // Best-effort schema init — idempotent CREATE TABLE IF NOT EXISTS
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

    /// Attach an encryptor for value encryption (passphrase-derived).
    pub fn with_passphrase(mut self, passphrase: &str) -> Self {
        self.encryptor = Some(Arc::new(
            crate::database::encrypt::Encryptor::from_passphrase(passphrase),
        ));
        self
    }

    /// Access the underlying driver for bulk operations.
    pub fn driver(&self) -> &Arc<dyn crate::database::driver::DatabaseDriver> {
        &self.driver
    }
}

const HMEM_COLUMNS: &str = "id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, dimension";

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
        let value_text = if let Some(ref enc) = self.encryptor {
            enc.decrypt(&value_text)
        } else {
            value_text
        };
        let hrow =
            HMemRow {
                id: row
                    .get(0)?
                    .as_text()?
                    .parse()
                    .map_err(|_| HMemError::Infra(InfrastructureError::database("invalid id")))?,
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
}

impl HMemStore {
    /// Insert a h_mem into the store.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — insert h_mem into store
    /// pre:  h_mem has valid entity, attribute, value
    /// post: h_mem inserted
    pub fn insert(&self, h_mem: &HMem) -> Result<(), HMemError> {
        let value_json = serde_json::to_string(&h_mem.value)?;
        let value = if let Some(ref enc) = self.encryptor {
            enc.encrypt(&value_json)
        } else {
            value_json
        };
        self.exec(
            &format!(
                "INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
            ),
            &[
                DbValue::Text(h_mem.id.to_string()),
                DbValue::Text(h_mem.entity.clone()),
                DbValue::Text(h_mem.attribute.clone()),
                DbValue::Text(value),
                DbValue::Text(h_mem.observed_at.to_rfc3339()),
                DbValue::Null,
                DbValue::Text(h_mem.recalled_at.to_rfc3339()),
                DbValue::Real(h_mem.confidence.value()),
                h_mem
                    .access
                    .perspective
                    .as_ref()
                    .map_or(DbValue::Null, |p| DbValue::Text(p.to_string())),
                DbValue::Text(h_mem.access.visibility.to_string()),
                DbValue::Text(h_mem.access.owner_webid.to_string()),
                h_mem
                    .dimension
                    .as_ref()
                    .map_or(DbValue::Null, |d| DbValue::Text(d.as_str().to_string())),
            ],
        )?;
        Ok(())
    }
    /// Query h_mems by entity.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query by entity
    /// pre:  entity is non-empty
    /// post: returns Vec of h_mems matching entity
    #[must_use = "result must be used"]
    pub fn query_by_entity(&self, entity: &str) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string())],
        )
    }
    /// Query h_mems by entity and attribute.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query by entity + attribute
    /// pre:  entity and attribute are non-empty
    /// post: returns Vec of matching h_mems
    pub fn query_by_entity_attribute(
        &self,
        entity: &str,
        attribute: &str,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 AND attribute = ?2 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string()), DbValue::Text(attribute.to_string())],
        )
    }
    /// Query h_mems by perspective.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query by perspective
    /// pre:  perspective is valid
    /// post: returns Vec of h_mems for this perspective
    pub fn query_by_perspective(&self, perspective: &WebID) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(perspective.to_string())],
        )
    }
    /// Query all h_mems with a given attribute, regardless of entity.
    /// Query h_mems by attribute.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query by attribute
    /// pre:  attribute is non-empty
    /// post: returns Vec of h_mems matching attribute
    #[must_use = "result must be used"]
    pub fn query_by_attribute(&self, attribute: &str) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE attribute = ?1 AND valid_to IS NULL ORDER BY valid_from DESC"),
            &[DbValue::Text(attribute.to_string())],
        )
    }
    /// Update a h_mem's value (close current version, insert new).
    /// Wrapped in a transaction for atomicity.
    /// Update a h_mem's value and confidence.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — update value and confidence
    /// pre:  id is valid
    /// post: h_mem value and confidence updated
    pub fn update(
        &self,
        id: &HMemId,
        new_value: Value,
        new_confidence: impl Into<Confidence>,
    ) -> Result<(), HMemError> {
        let new_confidence = new_confidence.into();
        let now = now_rfc3339();
        self.driver
            .execute_batch("BEGIN")
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let result = (|| -> Result<(), HMemError> {
            self.driver
                .execute(
                    "UPDATE hmems SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
                    &[DbValue::Text(now.clone()), DbValue::Text(id.to_string())],
                )
                .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            let rows = self.driver.query(
                "SELECT entity, attribute, perspective, visibility, owner_webid, dimension FROM hmems WHERE id = ?1",
                &[DbValue::Text(id.to_string())],
            ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            let row = rows.first().ok_or_else(|| {
                HMemError::NotFound(NotFound {
                    entity_type: "h_mem".to_string(),
                    id: id.to_string(),
                })
            })?;
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
                    DbValue::Null, DbValue::Text(now.clone()),
                    DbValue::Real(new_confidence.value()),
                    perspective.map_or(DbValue::Null, DbValue::Text),
                    DbValue::Text(visibility), DbValue::Text(owner_webid),
                    dimension.map_or(DbValue::Null, DbValue::Text),
                ],
            ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.driver
                    .execute_batch("COMMIT")
                    .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
                Ok(())
            }
            Err(e) => {
                let _ = self.driver.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
    /// Get a h_mem by ID.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — get h_mem by ID
    /// pre:  id is valid
    /// post: returns Some(HMem) if found, None otherwise
    #[must_use = "result must be used"]
    pub fn get_by_id(&self, id: &HMemId) -> Result<Option<HMem>, HMemError> {
        let results = self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE id = ?1 AND valid_to IS NULL"),
            &[DbValue::Text(id.to_string())],
        )?;
        Ok(results.into_iter().next())
    }

    /// Touch a h_mem's recalled_at timestamp to now — resets the decay clock.
    ///
    /// Called on recall so that actively-used memories don't decay.
    /// Unused memories continue their natural decay toward the half-life.
    /// `valid_from` is never modified — it remains the creation timestamp.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// pre:  id is a valid, non-expired h_mem ID
    /// post: h_mem's recalled_at updated to current time
    pub fn touch_recall(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "UPDATE hmems SET recalled_at = ?1 WHERE id = ?2 AND valid_to IS NULL",
            &[DbValue::Text(now_rfc3339()), DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }
    /// Semantic h_mems with lowest confidence, ordered ASC. Used by consolidation.
    /// Query lowest-confidence semantic h_mems.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — low-confidence semantic h_mems
    /// pre:  limit > 0
    /// post: returns up to limit h_mems ordered by confidence ascending
    pub fn query_semantic_lowest_confidence(&self, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL ORDER BY confidence ASC, valid_from ASC LIMIT ?1"),
            &[DbValue::Integer(limit as i64)],
        )
    }
    /// Count semantic h_mems below confidence threshold. Used by consolidation.
    /// Count semantic h_mems below a confidence threshold.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — count below threshold
    /// pre:  threshold in [0.0, 1.0]
    /// post: returns count of h_mems with confidence ≤ threshold
    pub fn count_semantic_below_confidence(&self, threshold: f64) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND confidence <= ?1",
            &[DbValue::Real(threshold)],
        )
    }
    /// Semantic h_mems below confidence threshold, ordered ASC. Used by consolidation.
    /// Query semantic h_mems below a confidence threshold.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query below threshold
    /// pre:  threshold in [0.0, 1.0], limit > 0
    /// post: returns up to limit h_mems with confidence ≤ threshold
    pub fn query_semantic_below_confidence(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND confidence <= ?1 ORDER BY confidence ASC, valid_from ASC LIMIT ?2"),
            &[DbValue::Real(threshold), DbValue::Integer(limit as i64)],
        )
    }
    /// Count semantic h_mems (perspective IS NULL, valid_to IS NULL).
    /// Count all semantic h_mems.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — count semantic h_mems
    /// post: returns total count of semantic h_mems
    #[must_use = "result must be used"]
    pub fn count_semantic(&self) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE perspective IS NULL AND valid_to IS NULL",
            &[],
        )
    }
    /// Count semantic h_mems for a given entity.
    /// Count semantic h_mems for an entity.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — count per entity
    /// pre:  entity is non-empty
    /// post: returns count for entity
    pub fn count_semantic_by_entity(&self, entity: &str) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE entity = ?1 AND perspective IS NULL AND valid_to IS NULL",
            &[DbValue::Text(entity.to_string())],
        )
    }
    /// Count h_mems for a given perspective (episodic).
    /// Count h_mems by perspective.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P8\] Motivating: Semantic Grounding — count per perspective
    /// pre:  perspective is valid
    /// post: returns count for perspective
    pub fn count_by_perspective(&self, perspective: &WebID) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE perspective = ?1 AND valid_to IS NULL",
            &[DbValue::Text(perspective.to_string())],
        )
    }
    /// Query semantic h_mems older than N days, grouped by entity for condensation.
    ///
    /// Returns h_mems with `perspective IS NULL AND valid_to IS NULL` and
    /// `valid_from` earlier than the cutoff date, ordered by entity then
    /// confidence descending (best first), then valid_from descending (most recent first).
    /// This ordering enables the condensation loop to identify the best candidate
    /// to keep per entity group (first in each entity group).
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query old h_mems for condensation
    /// \[P9\] Constraining: Homeostatic Self-Regulation — enables semantic condensation trigger
    /// pre:  days > 0, limit > 0
    /// post: returns up to limit h_mems older than cutoff, ordered by entity, confidence DESC, valid_from DESC
    pub fn query_semantic_older_than(
        &self,
        days: u32,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE perspective IS NULL AND valid_to IS NULL AND valid_from < ?1 ORDER BY entity ASC, confidence DESC, valid_from DESC LIMIT ?2"),
            &[DbValue::Text(cutoff), DbValue::Integer(limit as i64)],
        )
    }

    /// Soft-delete: set valid_to to close a h_mem.
    /// Soft-delete a h_mem by setting valid_to.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — soft-delete h_mem
    /// pre:  id is valid
    /// post: h_mem's valid_to set to now (soft-delete)
    pub fn close_by_id(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "UPDATE hmems SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
            &[DbValue::Text(now_rfc3339()), DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }
    /// Hard-delete a h_mem row entirely.
    /// Hard-delete a h_mem by ID.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — hard-delete h_mem
    /// pre:  id is valid
    /// post: h_mem permanently deleted
    pub fn delete_by_id(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "DELETE FROM hmems WHERE id = ?1",
            &[DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }
    /// Hard-delete all h_mems whose entity starts with the given prefix.
    /// Returns the number of rows deleted.
    /// Delete h_mems by entity prefix.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — delete by entity prefix
    /// pre:  prefix is non-empty
    /// post: matching h_mems deleted
    /// post: returns count of deleted h_mems
    pub fn delete_by_entity_prefix(&self, prefix: &str) -> Result<usize, HMemError> {
        self.exec(
            "DELETE FROM hmems WHERE entity LIKE ?1",
            &[DbValue::Text(format!("{}%", prefix))],
        )
    }
    /// HMemRow → HMem: parse timestamps + JSON value.
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
/// HMem -> HMemEntry: lossy (flattens access control for CAS storage).
impl From<&HMem> for HMemEntry {
    fn from(t: &HMem) -> Self {
        Self {
            id: t.id.to_string(),
            entity: t.entity.clone(),
            attribute: t.attribute.clone(),
            value: t.value.clone(),
            valid_from: t.observed_at.to_rfc3339(),
            valid_to: None,
            confidence: t.confidence.value(),
            perspective: t
                .access
                .perspective
                .map(|wid| wid.to_string())
                .unwrap_or_default(),
            visibility: t.access.visibility.as_str().to_string(),
            dimension: t.dimension.map(|d| d.as_str().to_string()),
        }
    }
}
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use crate::database::value::DbValue;
    fn make_store() -> HMemStore {
        let driver = SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver);
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS hmems (
                    id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                    value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                    recalled_at TEXT NOT NULL DEFAULT (datetime('now')),
                    confidence REAL NOT NULL, perspective TEXT, visibility TEXT NOT NULL,
                    owner_webid TEXT NOT NULL, dimension TEXT
                )",
            )
            .expect("create hmems table");
        store
    }
    //
    // Before fix, a corrupt valid_from was silently replaced with Utc::now(),
    // returning a h_mem with a fabricated temporal validity bound.
    // Now it propagates an Infra error, and the driver's row mapping skips it.
    #[test]
    fn corrupt_valid_from_propagates_infra_error() {
        let store = make_store();
        let webid = WebID::new();
        let id = HMemId::new();
        // Insert a h_mem with a garbage timestamp that cannot be parsed as RFC3339.
        store
            .driver()
            .execute(
                "INSERT INTO hmems (id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, datetime('now'), ?6, NULL, ?7, ?8)",
                &[
                    DbValue::Text(id.to_string()),
                    DbValue::Text("test-entity".into()),
                    DbValue::Text("attr".into()),
                    DbValue::Text(serde_json::to_string(&serde_json::json!("val")).unwrap()),
                    DbValue::Text("not-a-timestamp".into()),
                    DbValue::Real(1.0),
                    DbValue::Text("private".into()),
                    DbValue::Text(webid.to_string()),
                ],
            )
            .unwrap();
        // Query should return an Infra error (row is logged and error propagated by query_rows).
        let result = store.query_by_entity("test-entity");
        assert!(
            result.is_err(),
            "corrupt timestamp row should produce an error, not silently ignored"
        );
    }
    #[test]
    fn valid_from_round_trips_correctly() {
        let store = make_store();
        let webid = WebID::new();
        let h_mem = HMem::new("entity", "attr", serde_json::json!("val"), webid);
        store.insert(&h_mem).unwrap();
        let h_mems = store.query_by_entity("entity").unwrap();
        assert_eq!(h_mems.len(), 1);
        // observed_at should match the original to second precision.
        let delta = (h_mems[0].observed_at - h_mem.observed_at)
            .num_seconds()
            .abs();
        assert!(delta < 2, "valid_from should survive a round-trip");
    }
    #[test]
    fn get_by_id_missing_returns_none() {
        let store = make_store();
        let missing = HMemId::new();
        let result = store.get_by_id(&missing).unwrap();
        assert!(result.is_none());
    }
}
