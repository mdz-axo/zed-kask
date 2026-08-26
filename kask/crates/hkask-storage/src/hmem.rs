//! Uni-temporal h_mems — entity/attribute/value with observed_at timestamp.

use crate::database::value::{DbRow, DbValue};
use chrono::{DateTime, Utc};
use hkask_types::HMemEntry;
use hkask_types::HMemOntology;
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
    /// Dual-axis ontological anchoring (P5.4): DC+BIBO state axis + PKO process
    /// axis + 5W1H universal ground + open-world domain tags. `None` =
    /// unclassified (the pre-ontology default; legacy h_mems carry no
    /// ontology). Queryable via `json_extract(ontology, ...)` so h_mems and
    /// corpus `TaggedChunk`s share a common substrate for graph reasoning.
    pub ontology: Option<HMemOntology>,
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
            ontology: None,
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
    /// Set the ontological anchoring on a HMem.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set ontology
    /// \[P8\] Constraining: Semantic Grounding — dual-axis anchoring (P5.4)
    /// post: returns Self with ontology set (builder pattern)
    pub fn with_ontology(mut self, ontology: HMemOntology) -> Self {
        self.ontology = Some(ontology);
        self
    }

    /// Set 5W1H dimension on a HMem. Convenience builder that initializes
    /// the ontology blob if absent and adds the dimension to it.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — builder: set dimension
    /// \[P8\] Constraining: Semantic Grounding — anchors to 5W1H ontology tier
    /// post: returns Self with the dimension added to the ontology blob
    pub fn with_dimension(mut self, d: Dimension) -> Self {
        let ont = self.ontology.take().unwrap_or_default();
        self.ontology = Some(ont.with_dimension(d));
        self
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
    ///
    /// The `hmems` table schema is owned by `core/sql/schema.sql`, which
    /// `Database::sqlite_pool` runs on every pool creation (file and
    /// in-memory). This constructor does NOT re-create the table — doing so
    /// would duplicate the schema and drift (the prior `CREATE TABLE IF NOT
    /// EXISTS` here declared `recalled_at TEXT` nullable while `schema.sql`
    /// declared it `NOT NULL DEFAULT`, and the `IF NOT EXISTS` no-op meant
    /// the live schema depended on which ran first).
    pub fn from_driver(
        driver: Arc<dyn crate::database::driver::DatabaseDriver>,
    ) -> Result<Self, InfrastructureError> {
        Ok(Self {
            driver,
            encryptor: None,
        })
    }

    /// Access the underlying driver for bulk operations.
    pub fn driver(&self) -> &Arc<dyn crate::database::driver::DatabaseDriver> {
        &self.driver
    }
}

const HMEM_COLUMNS: &str = "id, entity, attribute, value, valid_from, valid_to, recalled_at, confidence, perspective, visibility, owner_webid, ontology";

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
                recalled_at: row.get(6)?.as_text().ok().unwrap_or_default().to_string(),
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
                ontology: row.get(11)?.as_text().ok().and_then(|s| {
                    if s.is_empty() {
                        None
                    } else {
                        HMemOntology::from_json_str(s).ok()
                    }
                }),
            };
        Self::row_to_triple(hrow)
    }

    fn count_rows(&self, sql: &str, params: &[DbValue]) -> Result<usize, HMemError> {
        let rows = self
            .driver
            .query(sql, params)
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        // No rows → the legitimate "count is zero" case. A decode or column
        // error on a present row is a real DB failure and must propagate so the
        // consolidation regulation loop sees a stale signal instead of a
        // fabricated Ok(0) that reads as "no deviation from set-point".
        match rows.first() {
            None => Ok(0),
            Some(row) => {
                let value = row
                    .get(0)
                    .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
                let count = value
                    .as_int()
                    .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
                Ok(count as usize)
            }
        }
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
        // Serialize the ontology blob eagerly and propagate failure. A
        // silent `unwrap_or_default()` here would write an empty string into
        // a column the ontology queries feed to `json_extract`, and SQLite
        // raises "malformed JSON" on `''` — which fails the WHOLE query, not
        // just that row. One bad write would blind every ontology recall.
        let ontology = match h_mem.ontology.as_ref() {
            Some(ont) => DbValue::Text(ont.to_json_string()?),
            None => DbValue::Null,
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
                ontology,
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
    /// Query h_mems by entity prefix (LIKE 'prefix%'), bounded by `limit`.
    ///
    /// Used by recall paths that need to load h_mems for a family of
    /// entities (e.g. all `chat:thread:*` entities for keyword
    /// search). The prefix must not contain SQL LIKE wildcards (`%` or `_`)
    /// — they would be interpreted as wildcards.
    ///
    /// The `limit` caps the number of rows loaded — without it, a session
    /// with thousands of past turns would load all of them into memory on
    /// every recall call. The recall path only needs the most recent `limit`
    /// h_mems (ordered by `valid_from DESC`), so the SQL LIMIT is the correct
    /// place to bound this.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// \[P3\] Motivating: Generative Space — query by entity prefix
    /// pre:  prefix is non-empty and contains no LIKE wildcards
    /// pre:  limit > 0
    /// post: returns up to `limit` h_mems whose entity starts with `prefix`,
    ///       ordered by `valid_from DESC` (most recent first)
    #[must_use = "result must be used"]
    pub fn query_by_entity_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE entity LIKE ?1 AND valid_to IS NULL \
                 ORDER BY valid_from DESC LIMIT ?2"
            ),
            &[
                DbValue::Text(format!("{}%", prefix)),
                DbValue::Integer(limit as i64),
            ],
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
        // Hold a single pooled connection for the entire transaction. The
        // prior `execute_batch("BEGIN")` / `execute()` / `execute_batch("COMMIT")
        // pattern acquired a different pool connection per call, so the
        // writes ran outside any transaction (autocommit on conns B/C, COMMIT
        // was a no-op on conn D). A crash between the UPDATE (close old
        // version) and INSERT (new version) left the row closed with no
        // replacement — silent data loss under `max_size > 1`.
        let pool = self.driver.sqlite_pool().ok_or_else(|| {
            HMemError::Infra(InfrastructureError::database(
                "HMemStore::update requires a SqliteDriver",
            ))
        })?;
        let mut conn = pool
            .get()
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let tx = conn
            .transaction()
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        // Close the old version (set valid_to).
        tx.execute(
            "UPDATE hmems SET valid_to = ?1 WHERE id = ?2 AND valid_to IS NULL",
            rusqlite::params![now, id.to_string()],
        )
        .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        // Read the old version's metadata to carry into the new version.
        let row = tx.query_row(
            "SELECT entity, attribute, perspective, visibility, owner_webid, ontology FROM hmems WHERE id = ?1",
            rusqlite::params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        ).map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let (entity, attribute, perspective, visibility, owner_webid, ontology) = row;
        let new_id = HMemId::new();
        tx.execute(
            &format!(
                "INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
            ),
            rusqlite::params![
                new_id.to_string(),
                entity,
                attribute,
                serde_json::to_string(&new_value)?,
                now,
                Option::<String>::None,
                now,
                new_confidence.value(),
                perspective,
                visibility,
                owner_webid,
                ontology,
            ],
        )
        .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        tx.commit()
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        Ok(())
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
    /// Query h_mems with lowest confidence, ordered ASC. Used by consolidation.
    pub fn query_lowest_confidence(&self, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE valid_to IS NULL ORDER BY confidence ASC, valid_from ASC LIMIT ?1"),
            &[DbValue::Integer(limit as i64)],
        )
    }
    /// Count h_mems below confidence threshold. Used by consolidation.
    pub fn count_below_confidence(&self, threshold: f64) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE valid_to IS NULL AND confidence <= ?1",
            &[DbValue::Real(threshold)],
        )
    }
    /// Query h_mems below a confidence threshold, ordered ASC. Used by consolidation.
    pub fn query_below_confidence(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE valid_to IS NULL AND confidence <= ?1 ORDER BY confidence ASC, valid_from ASC LIMIT ?2"),
            &[DbValue::Real(threshold), DbValue::Integer(limit as i64)],
        )
    }
    /// Count all h_mems.
    #[must_use = "result must be used"]
    pub fn count(&self) -> Result<usize, HMemError> {
        self.count_rows("SELECT COUNT(*) FROM hmems WHERE valid_to IS NULL", &[])
    }
    // ── Ontology query paths (P5.4 dual-axis anchoring) ──────────────────
    //
    // The `ontology` column is a JSON blob, so these queries reach into it
    // with SQLite's `json_extract`. Every query is guarded by
    // `ontology_is_json()`: SQLite raises "malformed JSON" on a bad blob,
    // which would abort the whole query and blind every ontology recall
    // rather than just excluding the bad row. The guard makes an unparsable
    // blob mean "unanchored" — the same reading `row_to_h_mem` already gives
    // it — instead of a hard failure.

    /// A predicate that is true only when `ontology` holds parseable JSON.
    /// Rows failing this are treated as unanchored.
    fn ontology_is_json(&self) -> &'static str {
        "json_valid(ontology)"
    }

    /// Scalar extraction of `$.<field>` from `ontology`.
    fn ontology_scalar(&self, field: &str) -> String {
        format!("json_extract(ontology, '$.{field}')")
    }

    /// Text rendering of the JSON sub-document at `$.<field>` (used for
    /// substring matching over an array field).
    fn ontology_json_text(&self, field: &str) -> String {
        format!("json_extract(ontology, '$.{field}')")
    }

    /// Query h_mems whose Dublin Core type (`$.dc_type`) matches exactly.
    ///
    /// The state-axis type query: "give me every `bibo:Article` h_mem".
    #[must_use = "result must be used"]
    pub fn query_by_dc_type(&self, dc_type: &str) -> Result<Vec<HMem>, HMemError> {
        let valid = self.ontology_is_json();
        let extract = self.ontology_scalar("dc_type");
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE {valid} AND {extract} = ?1 AND valid_to IS NULL \
                 ORDER BY valid_from DESC"
            ),
            &[DbValue::Text(dc_type.to_string())],
        )
    }

    /// Query h_mems whose Dublin Core subject list (`$.dc_subject`) contains
    /// the given term as a substring.
    ///
    /// `dc_subject` is an array, so the match runs against its JSON rendering
    /// (`["a","b"]`). Two consequences the caller must know:
    ///
    /// - The subject must not contain SQL LIKE wildcards (`%`, `_`) — they
    ///   would be interpreted as wildcards.
    /// - JSON punctuation is part of the haystack, so a needle containing
    ///   `"`, `[`, `]`, or `,` can match structure rather than content. A
    ///   needle spanning two elements never matches (element boundaries are
    ///   real separators), but `,` alone matches any multi-element row. Pass
    ///   plain concept text.
    #[must_use = "result must be used"]
    pub fn query_by_dc_subject(&self, subject: &str) -> Result<Vec<HMem>, HMemError> {
        let valid = self.ontology_is_json();
        let extract = self.ontology_json_text("dc_subject");
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE {valid} AND {extract} LIKE ?1 AND valid_to IS NULL \
                 ORDER BY valid_from DESC"
            ),
            &[DbValue::Text(format!("%{subject}%"))],
        )
    }

    /// Query h_mems belonging to a PKO procedure (`$.pko_procedure`).
    ///
    /// The process-axis query: "give me every step of `diagnose-bug-123`".
    #[must_use = "result must be used"]
    pub fn query_by_pko_procedure(&self, procedure: &str) -> Result<Vec<HMem>, HMemError> {
        let valid = self.ontology_is_json();
        let extract = self.ontology_scalar("pko_procedure");
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE {valid} AND {extract} = ?1 AND valid_to IS NULL \
                 ORDER BY valid_from DESC"
            ),
            &[DbValue::Text(procedure.to_string())],
        )
    }

    /// Query h_mems carrying at least one tag from an open-world ontology
    /// namespace (`$.ontology_tags.<namespace>`).
    ///
    /// This is what makes adding a domain ontology (FIBO, GOLEM, ESO)
    /// a data change rather than a schema change: the namespace is a key in
    /// the blob, and this query reaches it without a migration.
    ///
    /// The namespace is bound as a parameter — it is caller-supplied and
    /// must not be interpolated into the SQL text. SQLite has no
    /// parameterizable JSON path, so the path is built by concatenation
    /// inside SQL (`'$.ontology_tags.' || ?1`) rather than by Rust string
    /// interpolation — the namespace stays a bound parameter and cannot
    /// inject SQL.
    #[must_use = "result must be used"]
    pub fn query_by_ontology_namespace(&self, namespace: &str) -> Result<Vec<HMem>, HMemError> {
        let predicate = "json_extract(ontology, '$.ontology_tags.' || ?1) IS NOT NULL".to_string();
        let params = vec![DbValue::Text(namespace.to_string())];
        let valid = self.ontology_is_json();
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE {valid} AND {predicate} AND valid_to IS NULL \
                 ORDER BY valid_from DESC"
            ),
            &params,
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
            ontology: row.ontology,
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
            dimension: t
                .ontology
                .as_ref()
                .and_then(|ont| ont.dimensions.first().map(|s| s.clone())),
            ontology: t
                .ontology
                .as_ref()
                .and_then(|ont| ont.to_json_string().ok()),
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
    ontology: Option<HMemOntology>,
}
