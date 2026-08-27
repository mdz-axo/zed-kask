//! Unified memory store — one store for all h_mems, ontology-discriminated.
//!
//! The ontology blob on each h_mem carries dual-axis anchoring
//! (PKO process axis + DC state axis). A process-anchored h_mem carries
//! PKO procedure/step; a state-anchored h_mem carries DC type/subject.
//!
//! The `perspective` field is provenance (who wrote the memory), not a
//! type classifier. The intended flow is chat stream → chunks → each
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

use hkask_storage::database::value::DbValue;
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
    #[error("No embeddings found for centroid: {0}")]
    NoEmbeddingsForCentroid(String),
}

/// Result of computing a style centroid over a prefix-scoped embedding set.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CentroidResult {
    pub centroid: Vec<f32>,
    pub passage_count: usize,
    pub stored: bool,
}

/// Default memory life in days: 180 days (6 months × 30).
///
/// Wozniak & Gorzelanczyk (1995), equation (3): R(t) = exp(-t/S).
pub(crate) const DEFAULT_MEMORY_LIFE_DAYS: f64 = crate::bayesian::DEFAULT_MEMORY_LIFE_DAYS;

/// Default per-agent storage budget (max h_mems).
pub(crate) const DEFAULT_STORAGE_BUDGET: usize = 10_000;

/// Unified memory store — one store for all h_mems.
///
/// The ontology blob on each h_mem carries dual-axis anchoring. `store()`
/// accepts any h_mem; the ontology classifies it. Recall queries filter by
/// `perspective` (who wrote this) when needed — the swarm hive uses this
/// to scope by agent.
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
    /// default `max_h_mems` cap when the caller omits one — the
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
    /// ontology (state-anchored h_mems get `HMemOntology::state()`,
    /// process-anchored h_mems get `HMemOntology::process()`).
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
    /// This is the recall path for perspective-scoped memory: filter to
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

    /// Query all h_mems by entity prefix, without decay or dedup.
    /// Used by the purge tool to find all h_mems (assertions, QA pairs,
    /// and any other attributes) matching a corpus prefix for deletion.
    pub fn h_mems_by_entity_prefix(&self, prefix: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        self.h_mem_store
            .query_by_entity_prefix(prefix, 100_000)
            .map_err(Into::into)
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
    /// `golem`, …) — the domain-supplement query.
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
        passage_text: Option<&str>,
    ) -> Result<String, MemoryStoreError> {
        Ok(self
            .embedding
            .store(entity_ref, vector, model, passage_text)?)
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

    /// Load all embeddings with passage text for in-memory index hydration.
    /// Returns `(entity_ref, vector, passage_text)` for every stored embedding.
    pub fn all_embeddings_with_text(
        &self,
    ) -> Result<Vec<(String, Vec<f32>, Option<String>)>, MemoryStoreError> {
        Ok(self.embedding.all_with_text()?)
    }

    /// Compute the centroid (mean embedding vector) for embeddings matching a prefix.
    pub fn compute_centroid(
        &self,
        prefix: &str,
        exclude_prefix: &str,
        exclude_ref: &str,
        dim: usize,
        store_as: Option<&str>,
        model: Option<&str>,
    ) -> Result<CentroidResult, MemoryStoreError> {
        let matching_refs: Vec<String> = self
            .embedding
            .query_by_prefix(prefix)?
            .into_iter()
            .filter(|r| !r.starts_with(exclude_prefix) && r != exclude_ref)
            .collect();

        if matching_refs.is_empty() {
            return Err(MemoryStoreError::NoEmbeddingsForCentroid(
                prefix.to_string(),
            ));
        }

        let mut centroid = vec![0.0f32; dim];
        let mut count = 0usize;
        for entity_ref in &matching_refs {
            match self.embedding.get(entity_ref) {
                Ok(emb) => {
                    for (i, v) in emb.vector.iter().enumerate() {
                        if i < dim {
                            centroid[i] += v;
                        }
                    }
                    count += 1;
                }
                Err(e) => tracing::warn!(
                    target: "hkask.memory",
                    error = %e,
                    entity_ref = %entity_ref,
                    "Failed to fetch embedding for centroid computation"
                ),
            }
        }

        if count == 0 {
            return Err(MemoryStoreError::NoEmbeddingsForCentroid(
                prefix.to_string(),
            ));
        }

        let n = count as f32;
        for v in centroid.iter_mut() {
            *v /= n;
        }

        let stored = if let Some(ref_to_store) = store_as {
            if let Some(m) = model {
                let _id = self.embedding.store(ref_to_store, &centroid, m, None)?;
                true
            } else {
                false
            }
        } else {
            false
        };

        Ok(CentroidResult {
            centroid,
            passage_count: count,
            stored,
        })
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
    /// Used by the consolidation bridge and the curator's therapy process
    /// to detect when a memory being promoted or examined matches a fact
    /// already in the store, enabling Bayesian evidence combination rather
    /// than duplicate insertion, and contradiction detection for therapy.
    pub fn find_existing_by_eav(&self, h_mem: &HMem) -> Option<HMem> {
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
    pub fn update_confidence(
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

    /// Identify h_mems eligible for consolidation (oldest, lowest
    /// effective confidence) written by a given perspective. Uses recall-time
    /// decayed confidence.
    pub(crate) fn consolidation_candidates(
        &self,
        perspective: WebID,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let mut h_mems = self.h_mem_store.query_by_perspective(&perspective)?;
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
    pub fn expire_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
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
        Ok(self.h_mem_store.count()?)
    }

    pub fn delete_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
        self.h_mem_store.delete_by_id(id)?;
        Ok(())
    }

    pub fn lowest_confidence_h_mems(&self, limit: usize) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.h_mem_store.query_lowest_confidence(limit)?)
    }

    pub fn low_confidence_count(&self, threshold: f64) -> Result<usize, MemoryStoreError> {
        Ok(self.h_mem_store.count_below_confidence(threshold)?)
    }

    pub fn low_confidence_h_mems(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        Ok(self.h_mem_store.query_below_confidence(threshold, limit)?)
    }

    // ── Co-occurrence connectedness (Priority 3) ────────────────────────
    //
    // When memories are recalled together, their entities are linked.
    // The link count is the `connectedness` signal for recall ranking:
    // a memory referenced by many others has been tested against more
    // contexts. Grounding: Tetlock's dilution effect — connectedness
    // down-weights similar-but-isolated memories (dilution candidates).

    /// Record co-occurrence links between a set of entities recalled in
    /// the same context. For each pair (a, b) where a < b lexicographically,
    /// increment the co-occurrence count.
    ///
    /// Called by the context injector after a successful recall.
    pub fn record_co_occurrence(&self, entities: &[String]) -> Result<(), MemoryStoreError> {
        if entities.len() < 2 {
            return Ok(());
        }
        let driver = self.h_mem_store.driver();
        let mut sorted: Vec<&str> = entities.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        sorted.dedup();
        for i in 0..sorted.len() {
            for j in (i + 1)..sorted.len() {
                let sql = "INSERT INTO memory_links (entity_a, entity_b, co_count, last_linked) \
                           VALUES (?1, ?2, 1, datetime('now')) \
                           ON CONFLICT(entity_a, entity_b) DO UPDATE SET \
                           co_count = co_count + 1, \
                           last_linked = datetime('now')";
                driver
                    .execute(
                        sql,
                        &[
                            DbValue::Text(sorted[i].to_string()),
                            DbValue::Text(sorted[j].to_string()),
                        ],
                    )
                    .map_err(|e| MemoryStoreError::HMem(HMemError::from(e)))?;
            }
        }
        Ok(())
    }

    /// Get the connectedness score for an entity — the total co-occurrence
    /// count across all links. Higher = more connected = more salient.
    ///
    /// Returns 0 for entities with no links (new or isolated memories).
    pub fn connectedness(&self, entity: &str) -> Result<u64, MemoryStoreError> {
        let driver = self.h_mem_store.driver();
        let sql = "SELECT COALESCE(SUM(co_count), 0) FROM memory_links \
                   WHERE entity_a = ?1 OR entity_b = ?1";
        let rows = driver
            .query(sql, &[DbValue::Text(entity.to_string())])
            .map_err(|e| MemoryStoreError::HMem(HMemError::from(e)))?;
        if rows.is_empty() {
            return Ok(0);
        }
        let count = rows[0].get_int(0).unwrap_or(0) as u64;
        Ok(count)
    }
}
