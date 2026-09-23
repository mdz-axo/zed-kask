//! Uni-temporal h_mems — entity/attribute/value with observed_at timestamp.

use crate::database::value::{DbRow, DbValue};
use chrono::{DateTime, Utc};

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
/// A memory with observation and recall timestamps; forgetting deletes it.
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
        Ok(Self { driver })
    }

    /// Access the underlying driver for bulk operations.
    pub fn driver(&self) -> &Arc<dyn crate::database::driver::DatabaseDriver> {
        &self.driver
    }
}

const HMEM_COLUMNS: &str = "id, entity, attribute, value, valid_from, recalled_at, confidence, perspective, visibility, owner_webid, ontology";

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
                recalled_at: row.get(5)?.as_text().ok().unwrap_or_default().to_string(),
                confidence: Confidence::new(row.get(6)?.as_real()?),
                perspective: row.get(7)?.as_text().ok().and_then(|s| s.parse().ok()),
                visibility: match row.get(8)?.as_text().unwrap_or("private") {
                    "public" => Visibility::Public,
                    "shared" => Visibility::Shared,
                    _ => Visibility::Private,
                },
                owner_webid: row.get(9)?.as_text()?.parse().map_err(|_| {
                    HMemError::Infra(InfrastructureError::database("invalid webid"))
                })?,
                ontology: row.get(10)?.as_text().ok().and_then(|s| {
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
        let value = serde_json::to_string(&h_mem.value)?;
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
                "INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
            ),
            &[
                DbValue::Text(h_mem.id.to_string()),
                DbValue::Text(h_mem.entity.clone()),
                DbValue::Text(h_mem.attribute.clone()),
                DbValue::Text(value),
                DbValue::Text(h_mem.observed_at.to_rfc3339()),
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

    /// Insert a batch of h_mems atomically on one SQLite connection.
    ///
    /// All values are serialized before the transaction begins. If any insert
    /// fails, dropping the transaction rolls back every earlier insert, so a
    /// trailing commit marker cannot survive without the records it covers.
    pub fn insert_batch_atomic(&self, h_mems: &[HMem]) -> Result<(), HMemError> {
        self.insert_batch_with_controls(h_mems, None, None)
            .map(|_| ())
    }

    /// Atomically replace one EAV key while publishing a related h_mem batch.
    ///
    /// expect: "A newer commit marker replaces the old marker without exposing a partial batch."
    /// \[P5\] Motivating: Organic Growth — control state stays bounded as history grows.
    /// \[P2\] Constraining: Transparent Imperfection — failed publication preserves the prior marker.
    /// pre: h_mems contains the replacement row for entity + attribute
    /// post: lessons and the one replacement row commit together; prior key versions are absent
    pub fn insert_batch_replacing_key_atomic(
        &self,
        h_mems: &[HMem],
        entity: &str,
        attribute: &str,
    ) -> Result<(), HMemError> {
        self.insert_batch_with_controls(h_mems, Some((entity, attribute)), None)
            .map(|_| ())
    }

    /// Publish a batch only while a required EAV key exists.
    ///
    /// expect: "Child records cannot be published after their durable parent is deleted."
    /// [P3] Motivating: Generative Space — aggregate creation retains its parent boundary.
    /// [P2] Constraining: Transparent Imperfection — a missing parent produces no partial write.
    /// pre: required_entity + required_attribute identify the parent key
    /// post: returns true and commits the full batch, or false and commits nothing when the key is absent
    pub fn insert_batch_if_key_exists_atomic(
        &self,
        h_mems: &[HMem],
        required_entity: &str,
        required_attribute: &str,
    ) -> Result<bool, HMemError> {
        self.insert_batch_with_controls(h_mems, None, Some((required_entity, required_attribute)))
    }

    fn insert_batch_with_controls(
        &self,
        h_mems: &[HMem],
        replacement: Option<(&str, &str)>,
        required: Option<(&str, &str)>,
    ) -> Result<bool, HMemError> {
        if h_mems.is_empty() && required.is_none() {
            return Ok(true);
        }
        let rows = h_mems
            .iter()
            .map(|h_mem| {
                let value = serde_json::to_string(&h_mem.value)?;
                let ontology = h_mem
                    .ontology
                    .as_ref()
                    .map(|ontology| ontology.to_json_string())
                    .transpose()?;
                Ok((
                    h_mem.id.to_string(),
                    h_mem.entity.clone(),
                    h_mem.attribute.clone(),
                    value,
                    h_mem.observed_at.to_rfc3339(),
                    h_mem.recalled_at.to_rfc3339(),
                    h_mem.confidence.value(),
                    h_mem.access.perspective.as_ref().map(ToString::to_string),
                    h_mem.access.visibility.to_string(),
                    h_mem.access.owner_webid.to_string(),
                    ontology,
                ))
            })
            .collect::<Result<Vec<_>, HMemError>>()?;
        let pool = self.driver.sqlite_pool().ok_or_else(|| {
            HMemError::Infra(InfrastructureError::database(
                "atomic h_mem batch publication requires a SqliteDriver",
            ))
        })?;
        let mut conn = pool
            .get()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        let transaction = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        if let Some((entity, attribute)) = required {
            let exists = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM hmems WHERE entity = ?1 AND attribute = ?2)",
                    rusqlite::params![entity, attribute],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
            if !exists {
                return Ok(false);
            }
        }
        if let Some((entity, attribute)) = replacement {
            transaction
                .execute(
                    "DELETE FROM hmems WHERE entity = ?1 AND attribute = ?2",
                    rusqlite::params![entity, attribute],
                )
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
        }
        for row in rows {
            transaction
                .execute(
                    &format!(
                        "INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
                    ),
                    rusqlite::params![
                        row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9,
                        row.10
                    ],
                )
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
        }
        transaction
            .commit()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        Ok(true)
    }

    /// Delete a primary EAV row and its related EAV row under one write lock.
    ///
    /// expect: "Deleting related durable records either removes both current keys or preserves both."
    /// [P3] Motivating: Generative Space — compound domain state stays coherent.
    /// [P2] Constraining: Transparent Imperfection — failure preserves the last durable state.
    /// pre: primary key identifies at most one row; related key identifies its dependent records
    /// post: true removes both keys atomically, false leaves state unchanged when the primary is absent
    pub fn delete_related_keys_atomic(
        &self,
        primary_entity: &str,
        primary_attribute: &str,
        related_entity: &str,
        related_attribute: &str,
    ) -> Result<bool, HMemError> {
        let pool = self.driver.sqlite_pool().ok_or_else(|| {
            HMemError::Infra(InfrastructureError::database(
                "atomic related-key deletion requires a SqliteDriver",
            ))
        })?;
        let mut connection = pool
            .get()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        let count = transaction
            .query_row(
                "SELECT COUNT(*) FROM hmems WHERE entity = ?1 AND attribute = ?2",
                rusqlite::params![primary_entity, primary_attribute],
                |row| row.get::<_, usize>(0),
            )
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        if count == 0 {
            return Ok(false);
        }
        if count != 1 {
            return Err(HMemError::Infra(InfrastructureError::database(format!(
                "required primary key {primary_entity}/{primary_attribute} has {count} rows"
            ))));
        }
        for (entity, attribute) in [
            (primary_entity, primary_attribute),
            (related_entity, related_attribute),
        ] {
            transaction
                .execute(
                    "DELETE FROM hmems WHERE entity = ?1 AND attribute = ?2",
                    rusqlite::params![entity, attribute],
                )
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
        }
        transaction
            .commit()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        Ok(true)
    }

    /// Delete every h_mem in one PKO procedure on one transaction.
    ///
    /// expect: "Deleting a procedure removes its root, steps, and indexes as one durable change."
    /// [P3] Motivating: Generative Space — procedure lifecycle owns all process records.
    /// [P2] Constraining: Transparent Imperfection — failure preserves the complete procedure.
    /// pre: procedure is the exact pko_procedure identifier; required key identifies its root
    /// post: Some identities are returned after atomic deletion, None leaves state unchanged when the root is absent
    pub fn delete_by_pko_procedure_if_key_exists_atomic(
        &self,
        procedure: &str,
        required_entity: &str,
        required_attribute: &str,
    ) -> Result<Option<Vec<(String, String)>>, HMemError> {
        let pool = self.driver.sqlite_pool().ok_or_else(|| {
            HMemError::Infra(InfrastructureError::database(
                "atomic procedure deletion requires a SqliteDriver",
            ))
        })?;
        let mut connection = pool
            .get()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        let root_count = transaction
            .query_row(
                "SELECT COUNT(*) FROM hmems WHERE entity = ?1 AND attribute = ?2",
                rusqlite::params![required_entity, required_attribute],
                |row| row.get::<_, usize>(0),
            )
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        if root_count == 0 {
            return Ok(None);
        }
        if root_count != 1 {
            return Err(HMemError::Infra(InfrastructureError::database(format!(
                "required procedure root {required_entity}/{required_attribute} has {root_count} rows"
            ))));
        }
        let anchored = transaction
            .query_row(
                "SELECT COUNT(*) FROM hmems WHERE entity = ?1 AND attribute = ?2
                 AND json_valid(ontology) AND json_extract(ontology, '$.pko_procedure') = ?3",
                rusqlite::params![required_entity, required_attribute, procedure],
                |row| row.get::<_, usize>(0),
            )
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        if anchored != 1 {
            return Err(HMemError::Infra(InfrastructureError::database(format!(
                "required procedure root {required_entity}/{required_attribute} is not anchored to {procedure}"
            ))));
        }
        let mut deleted = Vec::new();
        {
            let mut statement = transaction
                .prepare(
                    "DELETE FROM hmems
                     WHERE json_valid(ontology)
                       AND json_extract(ontology, '$.pko_procedure') = ?1
                     RETURNING entity, attribute",
                )
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
            let rows = statement
                .query_map([procedure], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?;
            for row in rows {
                deleted.push(row.map_err(|error| {
                    HMemError::Infra(InfrastructureError::database(error.to_string()))
                })?);
            }
        }
        transaction
            .commit()
            .map_err(|error| HMemError::Infra(InfrastructureError::database(error.to_string())))?;
        Ok(Some(deleted))
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
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string())],
        )
    }

    /// Query the newest structured memories attributed to one source thread.
    ///
    /// expect: "A later distillation pass can reuse keys learned from the same thread."
    /// \[P8\] Motivating: Semantic Grounding — source_thread provenance defines the exact reuse scope.
    /// pre: source_thread is non-empty and limit > 0
    /// post: returns at most limit JSON-object h_mems, newest first
    pub fn query_by_source_thread(
        &self,
        source_thread: &str,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        if source_thread.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE json_valid(value) AND json_extract(value, '$.source_thread') = ?1 \
                 ORDER BY valid_from DESC LIMIT ?2"
            ),
            &[
                DbValue::Text(source_thread.to_string()),
                DbValue::Integer(limit as i64),
            ],
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
                 WHERE entity LIKE ?1 \
                 ORDER BY valid_from DESC LIMIT ?2"
            ),
            &[
                DbValue::Text(format!("{}%", prefix)),
                DbValue::Integer(limit as i64),
            ],
        )
    }

    /// Query h_mems by entity prefix observed at or after `since`.
    ///
    /// expect: "The system provides durable storage for h_mem data"
    /// pre:  prefix is non-empty; `since` is RFC 3339 in the same format
    ///       `store` writes for `valid_from` (`DateTime::to_rfc3339`), so
    ///       lexicographic order equals chronological order
    /// post: returns Vec of matching h_mems, oldest first — the ascending
    ///       order the distillation pass relies on when advancing its
    ///       per-thread watermark to the newest pending turn
    pub fn query_by_entity_prefix_since(
        &self,
        prefix: &str,
        since: &str,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE entity LIKE ?1 AND valid_from >= ?2 \
                 ORDER BY valid_from ASC LIMIT ?3"
            ),
            &[
                DbValue::Text(format!("{}%", prefix)),
                DbValue::Text(since.to_string()),
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
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE entity = ?1 AND attribute = ?2 ORDER BY valid_from DESC"),
            &[DbValue::Text(entity.to_string()), DbValue::Text(attribute.to_string())],
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
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems WHERE attribute = ?1 ORDER BY valid_from DESC"
            ),
            &[DbValue::Text(attribute.to_string())],
        )
    }
    /// Update a h_mem's value and confidence: delete the current version
    /// and insert the replacement, wrapped in a transaction for atomicity.
    /// The prior version is deleted — the forgetting spec keeps no
    /// superseded state (operator ruling 2026-09-04: memories are
    /// forgotten or deleted, never "expired").
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
        // was a no-op on conn D). A crash between the DELETE and INSERT now
        // rolls back both — no window where the row is gone with no
        // replacement.
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
        // Read the old version's metadata to carry into the replacement —
        // before the delete, because the row is gone afterwards.
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
        // Delete the old version — the forgetting spec keeps no superseded
        // state (operator ruling 2026-09-04: memories are forgotten or
        // deleted, never "expired").
        tx.execute(
            "DELETE FROM hmems WHERE id = ?1",
            rusqlite::params![id.to_string()],
        )
        .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        let new_id = HMemId::new();
        tx.execute(
            &format!(
                "INSERT INTO hmems ({HMEM_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
            ),
            rusqlite::params![
                new_id.to_string(),
                entity,
                attribute,
                serde_json::to_string(&new_value)?,
                now,
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
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE id = ?1"),
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
    /// pre:  id is a valid h_mem ID
    /// post: h_mem's recalled_at updated to current time
    pub fn touch_recall(&self, id: &HMemId) -> Result<(), HMemError> {
        self.exec(
            "UPDATE hmems SET recalled_at = ?1 WHERE id = ?2",
            &[DbValue::Text(now_rfc3339()), DbValue::Text(id.to_string())],
        )?;
        Ok(())
    }
    /// Query h_mems with lowest confidence, ordered ASC. Used by consolidation.
    pub fn query_lowest_confidence(&self, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems ORDER BY confidence ASC, valid_from ASC LIMIT ?1"
            ),
            &[DbValue::Integer(limit as i64)],
        )
    }
    /// Count h_mems below confidence threshold. Used by consolidation.
    pub fn count_below_confidence(&self, threshold: f64) -> Result<usize, HMemError> {
        self.count_rows(
            "SELECT COUNT(*) FROM hmems WHERE confidence <= ?1",
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
            &format!("SELECT {HMEM_COLUMNS} FROM hmems WHERE confidence <= ?1 ORDER BY confidence ASC, valid_from ASC LIMIT ?2"),
            &[DbValue::Real(threshold), DbValue::Integer(limit as i64)],
        )
    }
    /// Count all h_mems.
    #[must_use = "result must be used"]
    pub fn count(&self) -> Result<usize, HMemError> {
        self.count_rows("SELECT COUNT(*) FROM hmems", &[])
    }

    /// Query h_mems whose `valid_from` (observation timestamp) is older than
    /// the given cutoff. Used by age-based pruning — distinct from confidence
    /// decay, which lowers recall weight but never deletes. An h_mem that is
    /// old but frequently recalled (`recalled_at` recent) still qualifies for
    /// age pruning here; the caller decides whether to spare actively-recalled
    /// memories by filtering on `recalled_at` after the query.
    ///
    /// pre:  cutoff is a valid RFC 3339 timestamp
    /// post: returns h_mems with `valid_from < cutoff`, ordered oldest
    ///       first
    #[must_use = "result must be used"]
    pub fn query_older_than(
        &self,
        cutoff: &chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 WHERE valid_from < ?1 \
                 ORDER BY valid_from ASC LIMIT ?2"
            ),
            &[
                DbValue::Text(cutoff.to_rfc3339()),
                DbValue::Integer(limit as i64),
            ],
        )
    }

    /// Query all h_mems, without decay or dedup. Used by the dedup
    /// tool to scan the full memory set for near-duplicate values.
    ///
    /// pre:  limit > 0
    /// post: returns up to `limit` h_mems, ordered newest first
    #[must_use = "result must be used"]
    pub fn query_all(&self, limit: usize) -> Result<Vec<HMem>, HMemError> {
        self.query_rows(
            &format!(
                "SELECT {HMEM_COLUMNS} FROM hmems \
                 ORDER BY valid_from DESC LIMIT ?1"
            ),
            &[DbValue::Integer(limit as i64)],
        )
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
                 WHERE {valid} AND {extract} = ?1 \
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
                 WHERE {valid} AND {extract} LIKE ?1 \
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
                 WHERE {valid} AND {extract} = ?1 \
                 ORDER BY valid_from DESC"
            ),
            &[DbValue::Text(procedure.to_string())],
        )
    }

    /// Query h_mems carrying at least one tag from an open-world ontology
    /// namespace (`$.ontology_tags.<namespace>`).
    ///
    /// This is what makes adding a domain ontology (FIBO, GOLEM, SEPIO)
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
                 WHERE {valid} AND {predicate} \
                 ORDER BY valid_from DESC"
            ),
            &params,
        )
    }

    /// Delete every h_mem under a literal, case-sensitive entity prefix, in one statement.
    /// No query limit applies; `%`, `_`, and backslash are ordinary characters.
    /// Returns the number of rows deleted. The forgetting pass uses this
    /// to forget a distilled thread's shared-copy turns — forgotten rows
    /// are removed from the database (operator ruling 2026-09-04: there
    /// is no "expired" state).
    pub fn delete_by_entity_prefix(&self, prefix: &str) -> Result<usize, HMemError> {
        self.delete_by_entity_prefix_with_entities(prefix)
            .map(|(count, _)| count)
    }

    /// Delete by literal prefix and return the count and distinct affected
    /// entities for coupled-reference cleanup. Identities come from DELETE
    /// RETURNING, not a capped or separately scoped discovery query. Payloads
    /// are not decoded; even a malformed memory must remain deletable.
    pub fn delete_by_entity_prefix_with_entities(
        &self,
        prefix: &str,
    ) -> Result<(usize, std::collections::HashSet<String>), HMemError> {
        let pool = self.driver.sqlite_pool().ok_or_else(|| {
            HMemError::Infra(InfrastructureError::database(
                "HMemStore::delete_by_entity_prefix_with_entities requires a SqliteDriver",
            ))
        })?;
        let mut conn = pool
            .get()
            .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))?;
        // Keep deletion, identity collection, and commit on one connection,
        // as in update(). RAII also rolls back partially executed statements
        // (e.g. a trigger using RAISE(FAIL)) before the connection is reused.
        (|| -> rusqlite::Result<_> {
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let mut count = 0;
            let mut entities = std::collections::HashSet::new();
            {
                let mut stmt = tx.prepare(
                    "DELETE FROM hmems WHERE substr(entity, 1, length(?1)) = ?1 COLLATE BINARY RETURNING entity",
                )?;
                let mut rows = stmt.query(rusqlite::params![prefix])?;
                while let Some(row) = rows.next()? {
                    entities.insert(row.get::<_, String>(0)?);
                    count += 1;
                }
            }
            tx.commit()?;
            Ok((count, entities))
        })()
        .map_err(|e| HMemError::Infra(InfrastructureError::database(e.to_string())))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;

    /// Approved slice 1: identities and deletion commit together; a partially
    /// executed DELETE must roll back before the pool connection is reused.
    #[test]
    fn prefix_deletion_rolls_back_and_returns_exact_distinct_entities() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let first = HMem::new("qa:target", "first", serde_json::json!("one"), WebID::new());
        let second = HMem::new(
            "qa:target",
            "second",
            serde_json::json!("two"),
            WebID::new(),
        );
        let unrelated = HMem::new("keep", "fact", serde_json::json!("keep"), WebID::new());
        for memory in [&first, &second, &unrelated] {
            store.insert(memory)?;
        }
        store.driver().execute_batch(
            "CREATE TRIGGER fail_last_delete BEFORE DELETE ON hmems
             WHEN OLD.entity = 'qa:target' AND (SELECT COUNT(*) FROM hmems WHERE entity = 'qa:target') = 1
             BEGIN SELECT RAISE(FAIL, 'forced partial DELETE failure'); END;",
        )?;
        assert!(store.delete_by_entity_prefix_with_entities("qa:").is_err());
        assert_eq!(
            store.count()?,
            3,
            "even the first deleted row must be restored"
        );
        for memory in [&first, &second, &unrelated] {
            assert!(store.get_by_id(&memory.id)?.is_some());
        }
        store
            .driver()
            .execute_batch("DROP TRIGGER fail_last_delete;")?;
        let (count, entities) = store.delete_by_entity_prefix_with_entities("qa:")?;
        assert_eq!(count, 2);
        assert_eq!(
            entities,
            std::collections::HashSet::from(["qa:target".to_string()])
        );
        assert!(store.get_by_id(&unrelated.id)?.is_some());
        let (count, entities) = store.delete_by_entity_prefix_with_entities("qa:")?;
        assert_eq!(count, 0);
        assert!(entities.is_empty());
        Ok(())
    }

    #[test]
    fn atomic_batch_rolls_back_earlier_inserts_when_commit_marker_fails() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let lesson = HMem::new(
            "lesson:atomic",
            "fact",
            serde_json::json!("durable lesson"),
            WebID::new(),
        );
        let watermark = HMem::new(
            "curator:distilled:atomic",
            "distilled_through",
            serde_json::json!({"through": chrono::Utc::now().to_rfc3339()}),
            WebID::new(),
        );
        store.driver().execute_batch(
            "CREATE TRIGGER fail_atomic_watermark BEFORE INSERT ON hmems
             WHEN NEW.entity = 'curator:distilled:atomic'
             BEGIN SELECT RAISE(FAIL, 'forced watermark failure'); END;",
        )?;

        assert!(
            store
                .insert_batch_atomic(&[lesson.clone(), watermark])
                .is_err()
        );
        assert!(
            store.get_by_id(&lesson.id)?.is_none(),
            "the lesson inserted before the failing watermark must roll back"
        );
        assert_eq!(store.count()?, 0);
        Ok(())
    }

    /// expect: "A failed marker replacement restores the prior marker and publishes no partial lesson."
    /// [P5] Motivating: Organic Growth — latest-only control state must remain crash-safe.
    /// [P2] Constraining: Transparent Imperfection — failure preserves the last proven coverage boundary.
    /// pre: one prior marker exists and the replacement insert is forced to fail
    /// post: prior marker survives; replacement marker and lesson are absent
    #[test]
    fn atomic_replacement_failure_preserves_prior_marker() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let entity = "curator:distilled:replace";
        let old_marker = HMem::new(
            entity,
            "distilled_through",
            serde_json::json!({"through": "2026-09-20T00:00:00Z"}),
            WebID::new(),
        );
        store.insert(&old_marker)?;
        let lesson = HMem::new(
            "lesson:replacement",
            "fact",
            serde_json::json!("new lesson"),
            WebID::new(),
        );
        let new_marker = HMem::new(
            entity,
            "distilled_through",
            serde_json::json!({"through": "2026-09-21T00:00:00Z"}),
            WebID::new(),
        );
        store.driver().execute_batch(
            "CREATE TRIGGER fail_replacement_marker BEFORE INSERT ON hmems
             WHEN NEW.entity = 'curator:distilled:replace'
             BEGIN SELECT RAISE(FAIL, 'forced replacement failure'); END;",
        )?;

        assert!(
            store
                .insert_batch_replacing_key_atomic(
                    &[lesson.clone(), new_marker.clone()],
                    entity,
                    "distilled_through",
                )
                .is_err()
        );
        assert!(store.get_by_id(&old_marker.id)?.is_some());
        assert!(store.get_by_id(&lesson.id)?.is_none());
        assert!(store.get_by_id(&new_marker.id)?.is_none());
        assert_eq!(store.count()?, 1);
        Ok(())
    }

    /// expect: "A commit failure rolls back data and marker on one connection; a later successful batch survives reopen" [P1]
    /// post: after the operation returns, a second pooled connection sees no failed batch records
    #[test]
    fn atomic_batch_commit_failure_is_rolled_back_before_reuse_and_reopen() -> anyhow::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("atomic-batch.sqlite");
        {
            let mut conn = rusqlite::Connection::open(&path)?;
            crate::init_wal_pragmas(&mut conn)?;
            crate::core::connection::init_sqlite_vec_on(&conn)?;
            conn.execute_batch(
                &include_str!("core/sql/schema.sql")
                    .replace("$DIM", &crate::embedding_dim().to_string()),
            )?;
            conn.execute_batch(
                "CREATE TABLE commit_parent (id INTEGER PRIMARY KEY);
                 CREATE TABLE commit_child (parent_id INTEGER REFERENCES commit_parent(id)
                     DEFERRABLE INITIALLY DEFERRED);
                 CREATE TRIGGER fail_at_commit AFTER INSERT ON hmems
                 WHEN NEW.entity = 'batch:marker'
                 BEGIN INSERT INTO commit_child VALUES (1); END;",
            )?;
        }
        let pool = r2d2::Pool::builder().max_size(2).build(
            crate::SqliteConnectionManager::file(&path).with_init(crate::init_wal_pragmas),
        )?;
        // Retain the observer so insert_batch_atomic must use a different
        // connection. Its own leased connection owns BEGIN, writes and COMMIT.
        let observer = pool.get()?;
        let store = HMemStore::from_driver(Arc::new(SqliteDriver::new(pool.clone())))?;
        let owner = WebID::new();
        let records = [
            HMem::new("batch:data", "fact", serde_json::json!("payload"), owner),
            HMem::new(
                "batch:marker",
                "commit",
                serde_json::json!("complete"),
                owner,
            ),
        ];
        let error = store
            .insert_batch_atomic(&records)
            .expect_err("deferred constraint fails at commit");
        assert!(error.to_string().contains("FOREIGN KEY"), "{error}");
        for table in ["hmems", "commit_child"] {
            let count: i64 =
                observer.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
            assert_eq!(count, 0, "failed commit must roll back {table}");
        }
        observer.execute_batch("DROP TRIGGER fail_at_commit;")?;
        store.insert_batch_atomic(&records)?;
        let count: i64 = observer.query_row("SELECT COUNT(*) FROM hmems", [], |r| r.get(0))?;
        assert_eq!(count, 2);
        drop(store);
        drop(observer);
        drop(pool);

        let reopened_pool = r2d2::Pool::builder()
            .max_size(2)
            .build(crate::SqliteConnectionManager::file(path).with_init(crate::init_wal_pragmas))?;
        let reopened = HMemStore::from_driver(Arc::new(SqliteDriver::new(reopened_pool)))?;
        for original in records {
            let stored = reopened
                .get_by_id(&original.id)?
                .expect("committed record survives reopen");
            assert_eq!(stored.value, original.value);
            assert_eq!(stored.access.owner_webid, owner);
        }
        assert_eq!(reopened.count()?, 2);
        Ok(())
    }

    #[test]
    fn update_deletes_prior_row_and_preserves_metadata() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let original = HMem::new("entity", "fact", serde_json::json!("old"), WebID::new())
            .with_perspective(WebID::new())
            .with_visibility(Visibility::Shared)
            .with_ontology(HMemOntology::from_json_str(r#"{"dc_type":"bibo:Note"}"#)?);
        store.insert(&original)?;
        store.update(&original.id, serde_json::json!("replacement"), 0.7)?;

        assert!(store.get_by_id(&original.id)?.is_none());
        assert_eq!(
            store.count()?,
            1,
            "replacement must not retain prior versions"
        );
        let rows = store.query_all(10)?;
        let replacement = rows
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing replacement"))?;
        assert_ne!(replacement.id, original.id);
        assert_eq!(replacement.entity, original.entity);
        assert_eq!(replacement.attribute, original.attribute);
        assert_eq!(replacement.value, serde_json::json!("replacement"));
        assert_eq!(replacement.confidence, Confidence::new(0.7));
        assert_eq!(replacement.access.perspective, original.access.perspective);
        assert_eq!(replacement.access.visibility, original.access.visibility);
        assert_eq!(replacement.access.owner_webid, original.access.owner_webid);
        assert_eq!(
            replacement
                .ontology
                .as_ref()
                .map(HMemOntology::to_json_string)
                .transpose()?,
            original
                .ontology
                .as_ref()
                .map(HMemOntology::to_json_string)
                .transpose()?,
        );
        assert!(replacement.observed_at >= original.observed_at);
        assert_eq!(replacement.recalled_at, replacement.observed_at);
        Ok(())
    }

    #[test]
    fn update_rolls_back_deletion_when_replacement_insert_fails() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let original = HMem::new("entity", "fact", serde_json::json!("old"), WebID::new());
        store.insert(&original)?;
        store.driver().execute_batch(
            "CREATE TRIGGER reject_replacement BEFORE INSERT ON hmems
             BEGIN SELECT RAISE(ABORT, 'injected replacement failure'); END;",
        )?;

        let error = store
            .update(&original.id, serde_json::json!("replacement"), 0.7)
            .expect_err("the injected insert failure must reach the caller");
        assert!(error.to_string().contains("injected replacement failure"));
        assert_eq!(store.count()?, 1);
        let retained = store
            .get_by_id(&original.id)?
            .ok_or_else(|| anyhow::anyhow!("failed update deleted the original"))?;
        assert_eq!(retained.value, original.value);
        assert_eq!(retained.confidence, original.confidence);
        assert_eq!(retained.observed_at, original.observed_at);
        assert_eq!(retained.recalled_at, original.recalled_at);
        Ok(())
    }

    /// expect: Prefix deletion treats wildcards and case as literal entity identity.
    /// [P1] Motivating: deleting one corpus must not delete a similarly named corpus.
    #[test]
    fn delete_prefix_matches_literal_entities() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        for prefix in ["corpus:a_", "corpus:a%", "corpus:a\\", "Corpus:Case"] {
            let target = HMem::new(
                &format!("{prefix}:target"),
                "fact",
                serde_json::json!(1),
                WebID::new(),
            );
            let retained = HMem::new(
                "corpus:ab:retained",
                "fact",
                serde_json::json!(2),
                WebID::new(),
            );
            let lower = HMem::new(
                "corpus:case:retained",
                "fact",
                serde_json::json!(3),
                WebID::new(),
            );
            for memory in [&target, &retained, &lower] {
                store.insert(memory)?;
            }
            assert_eq!(store.delete_by_entity_prefix(prefix)?, 1, "{prefix}");
            assert!(store.get_by_id(&target.id)?.is_none());
            assert!(store.get_by_id(&retained.id)?.is_some());
            assert!(store.get_by_id(&lower.id)?.is_some());
        }
        Ok(())
    }

    #[test]
    fn deletion_removes_rows_without_hiding_them() -> anyhow::Result<()> {
        let store = HMemStore::from_driver(SqliteDriver::in_memory_driver())?;
        let owner = WebID::new();
        let first = HMem::new("thread:first", "chunk:0", serde_json::json!("one"), owner);
        let second = HMem::new("thread:second", "chunk:0", serde_json::json!("two"), owner);
        let retained = HMem::new("knowledge", "fact", serde_json::json!("lesson"), owner);
        for memory in [&first, &second, &retained] {
            store.insert(memory)?;
        }
        store.delete_by_id(&first.id)?;
        assert!(store.get_by_id(&first.id)?.is_none());
        assert_eq!(store.delete_by_entity_prefix("thread:")?, 1);
        assert_eq!(store.delete_by_entity_prefix("thread:")?, 0);
        let raw_count = store
            .driver()
            .query_optional("SELECT count(*) FROM hmems", &[])?
            .ok_or_else(|| anyhow::anyhow!("missing count row"))?
            .get_int(0)?;
        assert_eq!(raw_count, 1);
        assert!(store.get_by_id(&retained.id)?.is_some());
        Ok(())
    }
}
