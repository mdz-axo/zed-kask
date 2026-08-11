//! Unified memory store — one store for all h_mems, ontology-discriminated.
//!
//! The episodic/semantic distinction is encoded in the `HMemOntology` blob
//! (P5.4 dual-axis anchoring), not in separate store structs. A semantic fact
//! carries DC+BIBO anchoring (`dc_type`, `dc_subject`, `dc_source`) with no
//! PKO procedure/step. An episodic experience carries PKO anchoring
//! (`pko_procedure`, `pko_step`) with `dc_type = pko:StepExecution`. The
//! ontology blob tells you which kind of memory this is — no separate struct
//! needed.
//!
//! The `perspective` field is provenance (who wrote the memory), not a
//! semantic classifier. The intended flow is chat stream → chunks → each
//! chunk tagged with both the best-fit state axis (Dublin Core) and the
//! best-fit process axis (PKO), so the `HMemOntology` blob is the discriminator.
//!
//! `MemoryStore` wraps `HMemStore` + `EmbeddingStore` and provides:
//! - `store()` — accepts any h_mem (no visibility/perspective invariants; the
//!   ontology blob classifies it)
//! - `query_deduped()` / `query_deduped_untouched()` — recall with decay + dedup
//! - `query_by_perspective()` — filter by who wrote the memory (the swarm
//!   hive uses this to scope by agent)
//! - Embedding operations (store, search, centroid, purge)
//! - Consolidation helpers (find_existing_by_eav, update_confidence,
//!   consolidation_candidates, expire_h_mem)
//!
//! The decay model (Wozniak-Gorzelanczyk, 1995: R(t) = exp(-t/S)) is applied
//! at recall time.

use std::sync::Arc;

use hkask_storage::{EmbeddingError, EmbeddingStore, HMem, HMemError, HMemStore, SimilarityResult};
use hkask_types::RegulationSink;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span};
use hkask_types::visibility::Confidence;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryStoreError {
    #[error("HMem error: {0}")]
    HMem(#[from] HMemError),
    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),
}

/// Default memory life in days: 180 days (6 months × 30).
///
/// Wozniak & Gorzelanczyk (1995), equation (3): R(t) = exp(-t/S).
pub(crate) const DEFAULT_MEMORY_LIFE_DAYS: f64 = crate::bayesian::DEFAULT_MEMORY_LIFE_DAYS;

/// Default per-agent storage budget (max h_mems).
pub(crate) const DEFAULT_STORAGE_BUDGET: usize = 10_000;

/// Unified memory store — one store for all h_mems.
///
/// The episodic/semantic distinction lives in the `HMemOntology` blob on each
/// h_mem, not in the store struct. `store()` accepts any h_mem; the ontology
/// classifies it. Recall queries filter by `perspective` (who wrote this)
/// when needed — the swarm hive uses this to scope by agent.
///
/// Decay (Wozniak-Gorzelanczyk, 1995) is applied at recall time:
/// `R(t) = exp(-t/S)` where `t` is days since `recalled_at` and `S` is
/// `memory_life_days` (default 180). `touch_recall` resets the clock.
///
/// Text chunking (`chunk_text`, `strip_gutenberg_headers`) is exposed as
/// associated functions delegating to [`crate::text_chunking`] — they touch
/// no store state.
pub struct MemoryStore {
    event_sink: Option<Arc<dyn RegulationSink>>,
    h_mem_store: HMemStore,
    embedding: Arc<EmbeddingStore>,
    memory_life_days: f64,
    storage_budget: usize,
}

impl MemoryStore {
    /// The default per-agent storage budget (max shared h_mems before
    /// consolidation prunes). Exposed so callers (`RealMemoryPort::new`)
    /// can fall back to it when `HKASK_MEMORY_STORAGE_BUDGET` is unset or
    /// malformed, keeping the default in one place (the `.rules` "Kask
    /// settings defaults must live in `Default` impls" trap — though this
    /// is a `const`, not a settings struct, the single-source principle
    /// still applies).
    pub fn default_storage_budget() -> usize {
        DEFAULT_STORAGE_BUDGET
    }

    /// The default memory life in days (the decay constant S in
    /// R(t) = exp(-t/S), Wozniak-Gorzelanczyk 1995). Exposed so
    /// `RealMemoryPort::new` can fall back to it when
    /// `HKASK_MEMORY_LIFE_DAYS` is unset or malformed.
    pub fn default_memory_life_days() -> f64 {
        DEFAULT_MEMORY_LIFE_DAYS
    }

    /// Create a new `MemoryStore` from h_mem and embedding stores.
    pub fn new(h_mem_store: HMemStore, embedding_store: EmbeddingStore) -> Self {
        Self {
            h_mem_store,
            embedding: Arc::new(embedding_store),
            event_sink: None,
            memory_life_days: DEFAULT_MEMORY_LIFE_DAYS,
            storage_budget: DEFAULT_STORAGE_BUDGET,
        }
    }

    /// Open a SQLCipher database and construct a `MemoryStore` from a single
    /// shared connection pool. Canonical constructor for file-backed storage.
    pub fn open(
        db_path: &str,
        passphrase: &str,
        dim: usize,
    ) -> Result<Self, hkask_storage::DatabaseError> {
        use hkask_storage::database::sqlite::SqliteDriver;
        let db = hkask_storage::Database::open(db_path, passphrase)?;
        let pool = db.sqlite_pool()?;
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool, db_path));
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver))
            .map_err(|e| hkask_storage::DatabaseError::SqlCipher(e.to_string()))?;
        let embedding_store = EmbeddingStore::from_driver(driver, dim)
            .map_err(|e| hkask_storage::DatabaseError::SqlCipher(e.to_string()))?;
        Ok(Self::new(h_mem_store, embedding_store))
    }

    /// Create an `MemoryStore` with no usable embedding capability.
    ///
    /// For callers that recall by entity/EAV only and never embed (the
    /// condenser's episodic path; the curator when its `EmbeddingStore`
    /// could not be opened). Embedding calls on the returned store will
    /// fail at the storage layer rather than being silently accepted.
    ///
    /// Fallible because `EmbeddingStore::from_driver` can fail for a
    /// driver-shape reason (a `Sqlite` provider whose `sqlite_pool()` is
    /// `None`) that has nothing to do with the dimension. A caller reaching
    /// here *because* its own `from_driver` call already failed would panic
    /// on an `expect`, turning a degraded-memory path into a crash.
    pub fn try_new_without_embeddings(h_mem_store: HMemStore) -> Result<Self, MemoryStoreError> {
        let embedding_store = EmbeddingStore::from_driver(
            Arc::clone(h_mem_store.driver()),
            1, // dim=1 — never used; this store does not embed
        )?;
        Ok(Self::new(h_mem_store, embedding_store))
    }

    pub fn with_ledger(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    pub fn with_memory_life_days(mut self, days: f64) -> Self {
        self.memory_life_days = days;
        self
    }

    /// Set the storage budget (max shared h_mems before consolidation prunes).
    /// The budget is enforced inside `MemoryConsolidator::consolidate` as the
    /// default `max_semantic_triples` cap when the caller omits one — the
    /// Ashby attenuator for unbounded memory growth. `RealMemoryPort::new`
    /// wires this from `HKASK_MEMORY_STORAGE_BUDGET` (default 10_000).
    pub fn with_storage_budget(mut self, budget: usize) -> Self {
        self.storage_budget = budget;
        self
    }

    pub fn memory_life_days(&self) -> f64 {
        self.memory_life_days
    }

    pub fn storage_budget(&self) -> usize {
        self.storage_budget
    }

    /// Access the underlying `EmbeddingStore` for direct operations.
    pub fn embedding_store(&self) -> &EmbeddingStore {
        &self.embedding
    }

    // ── Store ──────────────────────────────────────────────────────────────

    /// Store any h_mem. No visibility/perspective invariants — the ontology
    /// blob classifies the memory. The caller is responsible for setting the
    /// ontology (semantic facts get `HMemOntology::semantic()`, episodic
    /// experiences get `HMemOntology::episodic()`).
    ///
    /// Emits a `reg.memory.encode` span for observability.
    pub fn store(&self, h_mem: HMem) -> Result<(), MemoryStoreError> {
        self.h_mem_store.insert(&h_mem)?;
        if let Some(sink) = &self.event_sink {
            let span = Span::new(crate::MEMORY_ENCODE_SPAN.clone(), "stored");
            let event = RegulationRecord::new(
                h_mem.access.owner_webid,
                span,
                CyclePhase::Act,
                serde_json::json!({"entity": h_mem.entity, "attribute": h_mem.attribute}),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.memory", error = %e, "Failed to persist reg.memory span");
            }
        }
        Ok(())
    }

    /// Store a h_mem as a consolidation product (internal write, no
    /// visibility check). Used by the consolidation bridge.
    pub(crate) fn store_consolidated(&self, h_mem: HMem) -> Result<(), MemoryStoreError> {
        self.h_mem_store.insert(&h_mem)?;
        if let Some(sink) = &self.event_sink {
            let span = Span::new(crate::MEMORY_ENCODE_SPAN.clone(), "consolidated");
            let event = RegulationRecord::new(
                h_mem.access.owner_webid,
                span,
                CyclePhase::Act,
                serde_json::json!({"entity": h_mem.entity, "attribute": h_mem.attribute}),
                0,
            );
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.memory", error = %e, "Failed to persist reg.memory span");
            }
        }
        Ok(())
    }

    // ── Recall ─────────────────────────────────────────────────────────────

    /// Query by entity with deduplication, confidence decay, and recall-touch.
    ///
    /// Applies Wozniak-Gorzelanczyk (1995) forgetting curve decay at recall
    /// and resets the recall clock via `touch_recall`.
    pub fn query_deduped(&self, entity: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        let deduped = self.query_deduped_untouched(entity)?;
        for t in &deduped {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall h_mem — decay clock not reset"
                );
            }
        }
        Ok(deduped)
    }

    /// Query by entity with deduplication and confidence decay, **without**
    /// touching `recalled_at`. Use for recall paths that inspect many
    /// candidates but only act on a few.
    pub fn query_deduped_untouched(&self, entity: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_by_entity(entity)?;
        let decayed: Vec<HMem> = h_mems
            .into_iter()
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                let original_confidence = t.confidence;
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                tracing::debug!(
                    target: "reg.memory.decay",
                    entity = %t.entity,
                    attribute = %t.attribute,
                    original_confidence = %original_confidence,
                    decayed_confidence = %t.confidence,
                    days_since_recall = days_since,
                    memory_life_days = self.memory_life_days,
                    "Confidence decayed (Wozniak-Gorzelanczyk forgetting curve)"
                );
                t
            })
            .collect();
        Ok(crate::recall_dedup::dedup_h_mems(decayed))
    }

    /// Query by entity for a specific perspective (who wrote this), with
    /// deduplication and decay, **without** touching `recalled_at`.
    ///
    /// This is the recall path for first-person episodic memory: filter to
    /// the memories written by a specific agent/user. The swarm hive uses
    /// this to scope by agent.
    pub fn query_for_deduped_untouched(
        &self,
        entity: &str,
        perspective: WebID,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_by_entity(entity)?;
        let mut filtered: Vec<HMem> = h_mems
            .into_iter()
            .filter(|t| t.access.perspective == Some(perspective))
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                t
            })
            .collect();
        filtered.sort_by_key(|b| std::cmp::Reverse(b.observed_at));
        Ok(crate::recall_dedup::dedup_h_mems(filtered))
    }

    /// Query by entity for a specific perspective, with deduplication and
    /// decay, touching `recalled_at` on every survivor.
    ///
    /// The touching variant of [`Self::query_for_deduped_untouched`]. Prefer
    /// the untouched variant for recall paths that inspect many candidates
    /// but only act on a few — touching every recalled h_mem turns recall
    /// into a write storm under concurrent load (one UPDATE per row per call).
    pub fn query_for_deduped(
        &self,
        entity: &str,
        perspective: WebID,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let deduped = self.query_for_deduped_untouched(entity, perspective)?;
        for t in &deduped {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall h_mem — decay clock not reset"
                );
            }
        }
        Ok(deduped)
    }

    /// Query by entity prefix for a perspective, without touching
    /// `recalled_at`. Caps rows via SQL LIMIT.
    pub fn query_for_deduped_untouched_by_prefix(
        &self,
        prefix: &str,
        perspective: WebID,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_by_entity_prefix(prefix, limit)?;
        let mut filtered: Vec<HMem> = h_mems
            .into_iter()
            .filter(|t| t.access.perspective == Some(perspective))
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                t
            })
            .collect();
        filtered.sort_by_key(|b| std::cmp::Reverse(b.observed_at));
        Ok(crate::recall_dedup::dedup_h_mems(filtered))
    }

    /// Touch `recalled_at` on a single h_mem, resetting its decay clock.
    pub fn touch_recall(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
        self.h_mem_store.touch_recall(id).map_err(Into::into)
    }

    /// Query by attribute, with confidence decay applied.
    pub fn query_by_attribute(&self, attribute: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_by_attribute(attribute)?;
        let decayed: Vec<HMem> = h_mems
            .into_iter()
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                t
            })
            .collect();
        for t in &decayed {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall h_mem (query_by_attribute) — decay clock not reset"
                );
            }
        }
        Ok(decayed)
    }

    // ── Ontology recall (P5.4 dual-axis anchoring) ───────────────────────
    //
    // These are what make the ontology blob load-bearing rather than
    // decorative: an h_mem's dual-axis anchoring is a query axis, not just
    // metadata. All four apply decay without touching `recalled_at` — an
    // ontology sweep inspects many h_mems and should not reset their decay
    // clocks wholesale (the same write-storm reasoning as
    // `query_deduped_untouched`).

    /// Recall by Dublin Core type (`dc_type`) — the state-axis type query.
    pub fn query_by_dc_type(&self, dc_type: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.decayed(self.h_mem_store.query_by_dc_type(dc_type)?))
    }

    /// Recall by Dublin Core subject substring — the state-axis topic query.
    pub fn query_by_dc_subject(&self, subject: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.decayed(self.h_mem_store.query_by_dc_subject(subject)?))
    }

    /// Recall every step of a PKO procedure — the process-axis query.
    pub fn query_by_pko_procedure(&self, procedure: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.decayed(self.h_mem_store.query_by_pko_procedure(procedure)?))
    }

    /// Recall h_mems tagged by an open-world ontology namespace (`fibo`,
    /// `golem`, `omc`, …) — the domain-supplement query.
    pub fn query_by_ontology_namespace(
        &self,
        namespace: &str,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.decayed(self.h_mem_store.query_by_ontology_namespace(namespace)?))
    }

    /// Apply the Wozniak-Gorzelanczyk forgetting curve to a recalled batch
    /// without touching `recalled_at`.
    fn decayed(&self, h_mems: Vec<HMem>) -> Vec<HMem> {
        h_mems
            .into_iter()
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                t
            })
            .collect()
    }

    // ── Embedding operations ────────────────────────────────────────

    pub fn store_embedding(
        &self,
        entity_ref: &str,
        vector: &[f32],
        model: &str,
    ) -> Result<String, MemoryStoreError> {
        Ok(self.embedding.store(entity_ref, vector, model)?)
    }

    pub fn search_similar(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<SimilarityResult>, MemoryStoreError> {
        Ok(self.embedding.search(query_vector, limit)?)
    }

    pub fn embedding_count(&self) -> Result<usize, MemoryStoreError> {
        Ok(self.embedding.count()?)
    }

    pub fn embeddings_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, MemoryStoreError> {
        Ok(self.embedding.get_all_by_prefix(prefix)?)
    }

    pub fn purge_by_prefix(&self, prefix: &str) -> Result<usize, MemoryStoreError> {
        let to_delete = self.embedding.query_by_prefix(prefix)?;
        let mut count = 0;
        for entity_ref in &to_delete {
            match self.embedding.delete(entity_ref) {
                Ok(()) => count += 1,
                Err(e) => tracing::warn!(
                    target: "hkask.memory",
                    error = %e,
                    entity_ref = %entity_ref,
                    "Failed to delete embedding during purge_by_prefix"
                ),
            }
        }
        Ok(count)
    }

    // ── Consolidation helpers ─────────────────────────────────────────────

    /// Find an existing h_mem with the same EAV as the given h_mem.
    ///
    /// Used by the consolidation bridge to detect when a memory being
    /// promoted matches a fact already in the store, enabling Bayesian
    /// evidence combination rather than duplicate insertion.
    pub(crate) fn find_existing_by_eav(&self, h_mem: &HMem) -> Option<HMem> {
        let candidate_hash = crate::recall_dedup::eav_hash(h_mem);
        let existing = match self
            .h_mem_store
            .query_by_entity_attribute(&h_mem.entity, &h_mem.attribute)
        {
            Ok(rows) => rows
                .into_iter()
                .find(|t| crate::recall_dedup::eav_hash(t) == candidate_hash),
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    entity = %h_mem.entity,
                    attribute = %h_mem.attribute,
                    "find_existing_by_eav: query failed, returning None (may seed duplicate)"
                );
                return None;
            }
        };

        if existing.is_some() {
            tracing::debug!(
                target: "reg.consolidation",
                entity = %h_mem.entity,
                attribute = %h_mem.attribute,
                "Found existing h_mem for EAV — will combine confidences"
            );
        }

        existing
    }

    /// Update an existing h_mem's confidence via the bitemporal update path.
    pub(crate) fn update_confidence(
        &self,
        existing_id: &hkask_storage::HMemId,
        current_value: serde_json::Value,
        new_confidence: Confidence,
    ) -> Result<(), MemoryStoreError> {
        self.h_mem_store
            .update(existing_id, current_value, new_confidence)?;
        tracing::debug!(
            target: "reg.consolidation",
            triple_id = %existing_id.as_uuid(),
            new_confidence = %new_confidence,
            "h_mem confidence updated via Bayesian combination"
        );
        Ok(())
    }

    /// Identify episodic h_mems eligible for consolidation (oldest, lowest
    /// effective confidence) written by a given perspective. Uses recall-time
    /// decayed confidence. The episodic/semantic distinction is carried by the
    /// `HMemOntology` blob (P5.4) — only h_mems with a PKO procedure are
    /// candidates for promotion to semantic memory.
    pub(crate) fn consolidation_candidates(
        &self,
        perspective: WebID,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let mut h_mems = self
            .h_mem_store
            .query_episodic_by_perspective(&perspective)?;
        h_mems.sort_by(|a, b| {
            let a_effective = a
                .confidence
                .memory_decay(
                    crate::bayesian::days_since(a.recalled_at),
                    self.memory_life_days,
                )
                .value();
            let b_effective = b
                .confidence
                .memory_decay(
                    crate::bayesian::days_since(b.recalled_at),
                    self.memory_life_days,
                )
                .value();
            a_effective
                .partial_cmp(&b_effective)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.observed_at.cmp(&b.observed_at))
        });
        h_mems.truncate(limit);
        Ok(h_mems)
    }

    /// Expire a h_mem by setting its `valid_to` (soft-delete).
    pub(crate) fn expire_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
        self.h_mem_store.close_by_id(id)?;
        tracing::debug!(
            target: "hkask.memory",
            triple_id = %id.as_uuid(),
            "h_mem expired (soft-delete via valid_to)"
        );
        Ok(())
    }

    pub fn consolidation_candidate_count(&self, perspective: &WebID) -> usize {
        match self.consolidation_candidates(*perspective, usize::MAX) {
            Ok(candidates) => candidates.len(),
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    "consolidation_candidate_count: signal stale, returning 0"
                );
                0
            }
        }
    }

    // ── Budget / cleanup ──────────────────────────────────────────────────

    pub fn h_mem_count(&self) -> Result<usize, MemoryStoreError> {
        Ok(self.h_mem_store.count_semantic()?)
    }

    pub fn delete_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
        self.h_mem_store.delete_by_id(id)?;
        Ok(())
    }

    pub fn lowest_confidence_h_mems(&self, limit: usize) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.h_mem_store.query_semantic_lowest_confidence(limit)?)
    }

    pub fn low_confidence_count(&self, threshold: f64) -> Result<usize, MemoryStoreError> {
        Ok(self
            .h_mem_store
            .count_semantic_below_confidence(threshold)?)
    }

    pub fn low_confidence_h_mems(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self
            .h_mem_store
            .query_semantic_below_confidence(threshold, limit)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::HMemOntology;
    use hkask_types::Visibility;

    fn make_store() -> MemoryStore {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store = HMemStore::from_driver(driver).expect("hmem store init");
        let embedding_store =
            EmbeddingStore::from_driver(h_mem_store.driver().clone(), 4).expect("embedding init");
        MemoryStore::new(h_mem_store, embedding_store)
    }

    #[test]
    fn store_and_recall_with_ontology() {
        let store = make_store();
        let webid = WebID::new();
        let ont = HMemOntology::semantic("bibo:Article", vec!["ROIC".to_string()], "10-K 2025");
        let h_mem = HMem::new("company:Apple", "roic", serde_json::json!(0.32), webid)
            .with_visibility(Visibility::Shared)
            .with_ontology(ont);
        store.store(h_mem).unwrap();

        let results = store.query_deduped("company:Apple").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].ontology.is_some());
        assert_eq!(
            results[0].ontology.as_ref().unwrap().dc_type,
            "bibo:Article"
        );
    }

    #[test]
    fn episodic_and_semantic_coexist_in_one_store() {
        // A semantic fact and an episodic experience can both live in the same
        // store — the ontology blob distinguishes them, not the store struct.
        let store = make_store();
        let user = WebID::new();

        // Semantic fact (DC-anchored, no perspective)
        let semantic_ont = HMemOntology::semantic("bibo:Article", vec!["ROIC".to_string()], "10-K");
        let semantic_h_mem = HMem::new("company:Apple", "roic", serde_json::json!(0.32), user)
            .with_visibility(Visibility::Shared)
            .with_ontology(semantic_ont);
        store.store(semantic_h_mem).unwrap();

        // Episodic experience (PKO-anchored, with perspective)
        let episodic_ont = HMemOntology::episodic("diagnose-bug-123", "reproduce", "session-1");
        let episodic_h_mem = HMem::new(
            "chat:thread:abc",
            "chatted",
            serde_json::json!("reproduced the bug"),
            user,
        )
        .with_perspective(user)
        .with_visibility(Visibility::Private)
        .with_ontology(episodic_ont);
        store.store(episodic_h_mem).unwrap();

        // Both are in the store
        let all_apple = store.query_deduped("company:Apple").unwrap();
        assert_eq!(all_apple.len(), 1);
        assert!(
            all_apple[0]
                .ontology
                .as_ref()
                .unwrap()
                .pko_procedure
                .is_none()
        );

        let all_thread = store.query_deduped("chat:thread:abc").unwrap();
        assert_eq!(all_thread.len(), 1);
        assert_eq!(
            all_thread[0].ontology.as_ref().unwrap().pko_procedure,
            Some("diagnose-bug-123".to_string())
        );
    }

    #[test]
    fn decay_applied_on_recall() {
        // With memory_life_days = 0, a freshly-stored h_mem (t≈0) preserves
        // confidence (exp(0/0) = exp(0) = 1.0). The decay only kicks in after
        // time passes.
        let store = make_store().with_memory_life_days(0.0);
        let webid = WebID::new();
        let h_mem = HMem::new("test:entity", "attr", serde_json::json!("val"), webid)
            .with_visibility(Visibility::Shared)
            .with_confidence(Confidence::new(0.8));
        store.store(h_mem).unwrap();

        let results = store.query_deduped_untouched("test:entity").unwrap();
        assert_eq!(results.len(), 1);
        // Just-stored: t≈0, so confidence is preserved even with S=0
        assert!((results[0].confidence.value() - 0.8).abs() < 0.01);
    }

    #[test]
    fn perspective_filter_works() {
        let store = make_store();
        let user1 = WebID::new();
        let user2 = WebID::new();

        let h1 = HMem::new("shared:entity", "attr", serde_json::json!("v1"), user1)
            .with_perspective(user1)
            .with_visibility(Visibility::Private);
        store.store(h1).unwrap();

        let h2 = HMem::new("shared:entity", "attr", serde_json::json!("v2"), user2)
            .with_perspective(user2)
            .with_visibility(Visibility::Private);
        store.store(h2).unwrap();

        let user1_results = store
            .query_for_deduped_untouched("shared:entity", user1)
            .unwrap();
        assert_eq!(user1_results.len(), 1);
        assert_eq!(user1_results[0].value, serde_json::json!("v1"));

        let user2_results = store
            .query_for_deduped_untouched("shared:entity", user2)
            .unwrap();
        assert_eq!(user2_results.len(), 1);
        assert_eq!(user2_results[0].value, serde_json::json!("v2"));
    }

    /// Populate a store with three ontology-anchored h_mems: two semantic
    /// facts (one FIBO-tagged) and one episodic step execution.
    fn store_with_ontologies() -> (MemoryStore, WebID) {
        let store = make_store();
        let user = WebID::new();

        let roic = HMem::new("company:Apple", "roic", serde_json::json!(0.32), user)
            .with_visibility(Visibility::Shared)
            .with_ontology(
                HMemOntology::semantic("bibo:Article", vec!["ROIC".to_string()], "10-K 2025")
                    .with_ontology_tag("fibo", "return on invested capital"),
            );
        store.store(roic).expect("store roic");

        let moat = HMem::new("company:Apple", "moat", serde_json::json!("brand"), user)
            .with_visibility(Visibility::Shared)
            .with_ontology(HMemOntology::semantic(
                "bibo:Document",
                vec!["competitive advantage".to_string()],
                "analyst note",
            ));
        store.store(moat).expect("store moat");

        let step = HMem::new(
            "chat:thread:abc",
            "chatted",
            serde_json::json!("reproduced"),
            user,
        )
        .with_perspective(user)
        .with_visibility(Visibility::Private)
        .with_ontology(HMemOntology::episodic(
            "diagnose-bug-123",
            "reproduce",
            "session-1",
        ));
        store.store(step).expect("store step");

        (store, user)
    }

    #[test]
    fn query_by_dc_type_selects_only_that_state_axis_type() {
        let (store, _) = store_with_ontologies();

        let articles = store.query_by_dc_type("bibo:Article").expect("query");
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].attribute, "roic");

        let steps = store.query_by_dc_type("pko:StepExecution").expect("query");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].entity, "chat:thread:abc");

        assert!(
            store
                .query_by_dc_type("dcterms:Dataset")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn query_by_dc_subject_matches_a_term_inside_the_subject_array() {
        let (store, _) = store_with_ontologies();

        let roic = store.query_by_dc_subject("ROIC").expect("query");
        assert_eq!(roic.len(), 1);
        assert_eq!(roic[0].attribute, "roic");

        // Substring match, not exact: "competitive" hits "competitive advantage".
        let moat = store.query_by_dc_subject("competitive").expect("query");
        assert_eq!(moat.len(), 1);
        assert_eq!(moat[0].attribute, "moat");

        assert!(
            store
                .query_by_dc_subject("nonexistent-subject")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn query_by_pko_procedure_selects_only_process_axis_steps() {
        let (store, _) = store_with_ontologies();

        let steps = store
            .query_by_pko_procedure("diagnose-bug-123")
            .expect("query");
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0]
                .ontology
                .as_ref()
                .and_then(|o| o.pko_step.as_deref()),
            Some("reproduce")
        );

        // Semantic facts carry no PKO procedure, so they never match.
        assert!(
            store
                .query_by_pko_procedure("bibo:Article")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn query_by_ontology_namespace_reaches_the_open_world_map() {
        let (store, _) = store_with_ontologies();

        let fibo = store.query_by_ontology_namespace("fibo").expect("query");
        assert_eq!(fibo.len(), 1);
        assert_eq!(fibo[0].attribute, "roic");
        assert_eq!(
            fibo[0]
                .ontology
                .as_ref()
                .map(|o| o.ontology_concepts("fibo")),
            Some(&["return on invested capital".to_string()][..])
        );

        // An unpopulated namespace yields nothing rather than erroring — the
        // open-world map has no schema constraint on which keys exist.
        assert!(
            store
                .query_by_ontology_namespace("golem")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn ontology_queries_skip_h_mems_with_no_ontology_blob() {
        // A h_mem written before the ontology column existed (or by a caller
        // that sets none) must not match an ontology query — a NULL blob is
        // "unanchored", not "matches everything".
        let store = make_store();
        let user = WebID::new();
        let bare = HMem::new("bare:entity", "attr", serde_json::json!("v"), user)
            .with_visibility(Visibility::Shared);
        store.store(bare).expect("store bare");

        assert!(
            store
                .query_by_dc_type("bibo:Article")
                .expect("query")
                .is_empty()
        );
        assert!(
            store
                .query_by_dc_subject("anything")
                .expect("query")
                .is_empty()
        );
        assert!(
            store
                .query_by_pko_procedure("any")
                .expect("query")
                .is_empty()
        );
        assert!(
            store
                .query_by_ontology_namespace("fibo")
                .expect("query")
                .is_empty()
        );
    }

    /// `reg.memory.encode` span emission is the enforcement point for the
    /// `MemoryEncode` canonical span. Without a `RegulationSink` wired via
    /// `with_ledger`, the span emitter in `store()` is dead code (the `.rules`
    /// "Advertised invariants need enforcement points" trap). This test pins
    /// that wiring a `RegulationArchive` causes `store()` to persist a span
    /// queryable by the `memory` namespace prefix.
    #[test]
    fn store_persists_reg_memory_encode_span_when_ledger_wired() {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store init");
        let embedding_store =
            EmbeddingStore::from_driver(Arc::clone(&driver), 4).expect("embedding init");
        let archive = hkask_storage::RegulationArchive::from_driver(Arc::clone(&driver))
            .expect("regulation archive init");
        let store = MemoryStore::new(h_mem_store, embedding_store).with_ledger(Arc::new(archive));

        let owner = WebID::new();
        let h_mem = HMem::new("span:entity", "attr", serde_json::json!("v"), owner)
            .with_visibility(Visibility::Shared);
        store.store(h_mem).expect("store");

        // Re-open the archive on the same driver to read back the persisted span.
        // The span_category column stores the short name ("memory.encode"),
        // so the "memory" prefix matches it.
        let archive = hkask_storage::RegulationArchive::from_driver(driver)
            .expect("regulation archive re-open");
        let since = chrono::Utc::now() - chrono::Duration::seconds(60);
        let events = archive
            .query_by_namespace("memory", since, 100)
            .expect("query_by_namespace");
        assert_eq!(
            events.len(),
            1,
            "store() should persist exactly one reg.memory.encode span"
        );
        assert_eq!(events[0].span.namespace.short_name(), "memory.encode");
        assert_eq!(events[0].span.path.as_str(), "reg.memory.encode.stored");
    }

    /// The storage budget is the Ashby attenuator for unbounded memory growth:
    /// when `h_mem_count` exceeds `storage_budget` and the caller omits
    /// `max_semantic_triples`, `MemoryConsolidator::consolidate` prunes the
    /// lowest-confidence shared h_mems back to the budget. Without this the
    /// budget field would be dead config (the `.rules` "Advertised invariants
    /// need enforcement points" trap). This test pins the trigger.
    #[test]
    fn storage_budget_triggers_pruning_when_over_budget() {
        use crate::MemoryConsolidator;
        use hkask_types::ConsolidationRequest;

        let store = Arc::new(make_store().with_storage_budget(2));
        let owner = WebID::new();

        // Store three semantic h_mems with distinct confidences; budget is 2.
        for (entity, confidence) in [("high", 0.9), ("mid", 0.5), ("low", 0.1)] {
            let h_mem = HMem::new(entity, "is", serde_json::json!("v"), owner)
                .with_visibility(Visibility::Shared)
                .with_confidence(Confidence::new(confidence))
                .with_ontology(HMemOntology::semantic("bibo:Document", vec![], "test"));
            store.store(h_mem).expect("store");
        }
        assert_eq!(store.h_mem_count().expect("count"), 3);

        let consolidator = MemoryConsolidator::new(Arc::clone(&store));
        let outcome = consolidator
            .consolidate(
                &owner,
                ConsolidationRequest {
                    limit: 100,
                    confidence_floor: None,
                    max_semantic_triples: None,
                },
            )
            .expect("consolidate");

        // The budget trigger should have pruned 1 h_mem (3 → 2).
        assert_eq!(
            outcome.deleted_count, 1,
            "budget trigger should prune the excess h_mem"
        );
        assert_eq!(store.h_mem_count().expect("count after"), 2);

        // The lowest-confidence h_mem ("low", 0.1) should be the one pruned.
        let remaining: Vec<String> = store
            .query_deduped("low")
            .expect("recall")
            .into_iter()
            .map(|h| h.entity)
            .collect();
        assert!(
            remaining.is_empty(),
            "lowest-confidence h_mem should have been pruned, got {remaining:?}"
        );
    }
}
