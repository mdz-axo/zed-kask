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
//! - `query_for_deduped_untouched()` — filter by who wrote the memory (the
//!   swarm hive uses this to scope by agent)
//! - Embedding operations (store, search, purge)
//! - Consolidation helpers (update_confidence, delete_h_mem)
//!
//! The decay model (Wozniak-Gorzelanczyk, 1995: R(t) = exp(-t/S)) is applied
//! at recall time.

use std::collections::HashSet;
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

/// Outcome of an age-based prune operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PruneOutcome {
    /// Total h_mems that matched the age cutoff.
    pub candidates: usize,
    /// Successfully deleted.
    pub deleted_count: usize,
    /// Spared because they were recalled within the grace window.
    pub spared_count: usize,
    /// Individual delete failures (counted, not aborting the batch).
    pub failed_count: usize,
}

/// Outcome of a normalized-value dedup operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DedupOutcome {
    /// Total h_mems scanned.
    pub scanned: usize,
    /// Groups that contained 2+ near-duplicate values.
    pub groups_with_dupes: usize,
    /// Successfully deleted (forgotten — the row is removed).
    pub deleted_count: usize,
    /// Individual delete failures (counted, not aborting the batch).
    pub failed_count: usize,
    /// Non-string h_mems skipped (structural dedup is the EAV path's job).
    pub skipped_non_string: usize,
}

/// Normalize a string value for near-duplicate comparison: lowercase,
/// strip leading/trailing whitespace, collapse internal whitespace runs to
/// a single space, and strip common punctuation. Two values that differ only
/// in case, spacing, or trailing punctuation are treated as duplicates.
fn normalize_value(value: &str) -> String {
    let lower = value.to_lowercase();
    let mut result = String::with_capacity(lower.len());
    let mut prev_was_space = false;
    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !prev_was_space && !result.is_empty() {
                result.push(' ');
            }
            prev_was_space = true;
        } else if ch.is_ascii_punctuation() {
            // Skip punctuation — "AAPL." and "aapl" are the same value.
            prev_was_space = false;
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }
    result.trim_end().to_string()
}

/// Default memory life in days: 180 days (6 months × 30).
///
/// Wozniak & Gorzelanczyk (1995), equation (3): R(t) = exp(-t/S).
pub(crate) const DEFAULT_MEMORY_LIFE_DAYS: f64 = crate::bayesian::DEFAULT_MEMORY_LIFE_DAYS;

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
}

impl MemoryStore {
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
        let db = hkask_storage::open_or_repair(db_path, passphrase)?;
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

    pub fn memory_life_days(&self) -> f64 {
        self.memory_life_days
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

    /// Query by entity prefix, perspective-free. Shared h_mems carry no
    /// perspective, so keyword recall over the shared thread
    /// prefix must not scope by one. Without touching `recalled_at`; caps
    /// rows via SQL LIMIT.
    pub fn query_deduped_untouched_by_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_by_entity_prefix(prefix, limit)?;
        let mut filtered: Vec<HMem> = h_mems
            .into_iter()
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

    /// Fetch a single h_mem by ID, without decay and without touching
    /// `recalled_at`. Deleted h_mems are absent — forgetting removes the
    /// row from the database (there is no "expired" state) — so this is
    /// the "does this citation exist" check `memory_insert` runs on its
    /// evidence, not a recall path.
    pub fn get_by_id(&self, id: &hkask_storage::HMemId) -> Result<Option<HMem>, MemoryStoreError> {
        self.h_mem_store.get_by_id(id).map_err(Into::into)
    }

    /// Query all h_mems by entity prefix, without decay or dedup.
    /// Used by the purge tool to find all h_mems (assertions, QA pairs,
    /// and any other attributes) matching a corpus prefix for deletion.
    pub fn h_mems_by_entity_prefix(&self, prefix: &str) -> Result<Vec<HMem>, MemoryStoreError> {
        self.h_mem_store
            .query_by_entity_prefix(prefix, 100_000)
            .map_err(Into::into)
    }

    /// Query h_mems by entity prefix observed at or after `since`, without
    /// decay or dedup. The curator's distillation pass uses this to find
    /// threads with un-distilled turns without loading the whole store.
    pub fn h_mems_by_prefix_since(
        &self,
        prefix: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<HMem>, MemoryStoreError> {
        self.h_mem_store
            .query_by_entity_prefix_since(prefix, &since.to_rfc3339(), 100_000)
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

    /// Compute the centroid (mean embedding) over a prefix-scoped
    /// embedding set.
    ///
    /// Only refs under `prefix` count; `exclude_ref`, derived `:centroid`
    /// refs and `:rule:` refs are excluded so stored author/dimension
    /// centroids never feed back into the passage mean. When
    /// `store_as` is provided with a `model` name, the centroid is stored
    /// as an embedding under that ref — e.g. `style:{author}:centroid`,
    /// the entity ref `corpus_compose` reads for centroid validation.
    /// `dim` must match the store's configured dimension (the store step
    /// validates it).
    pub fn compute_centroid(
        &self,
        prefix: &str,
        exclude_ref: &str,
        dim: usize,
        store_as: Option<&str>,
        model: Option<&str>,
    ) -> Result<CentroidResult, MemoryStoreError> {
        let matching: Vec<(String, Vec<f32>)> = self
            .embedding
            .get_all_by_prefix(prefix)?
            .into_iter()
            .filter(|(entity_ref, _)| Self::centroid_passage_ref(entity_ref, exclude_ref))
            .collect();
        self.centroid_from_embeddings(&matching, prefix, dim, store_as, model)
    }

    /// Compute a centroid from existing embeddings named by explicit entity refs.
    ///
    /// expect: "I can select passages without copying embeddings or weighting duplicate refs twice." [P3]
    /// pre: every distinct eligible ref exists; dim matches the stored vectors
    /// post: averages each eligible ref once; missing refs fail before any write
    /// Uses the same exclusions as `compute_centroid`; no prefix is imposed.
    pub fn compute_centroid_for_refs(
        &self,
        entity_refs: &[String],
        exclude_ref: &str,
        dim: usize,
        store_as: Option<&str>,
        model: Option<&str>,
    ) -> Result<CentroidResult, MemoryStoreError> {
        let mut seen = std::collections::HashSet::new();
        let mut matching = Vec::new();
        for entity_ref in entity_refs {
            if Self::centroid_passage_ref(entity_ref, exclude_ref) && seen.insert(entity_ref) {
                let embedding = self.embedding.get(entity_ref)?;
                matching.push((entity_ref.clone(), embedding.vector));
            }
        }
        self.centroid_from_embeddings(&matching, "explicit entity refs", dim, store_as, model)
    }

    fn centroid_passage_ref(entity_ref: &str, exclude_ref: &str) -> bool {
        entity_ref != exclude_ref
            && !entity_ref.ends_with(":centroid")
            && !entity_ref.contains(":rule:")
    }

    fn centroid_from_embeddings(
        &self,
        matching: &[(String, Vec<f32>)],
        selection: &str,
        dim: usize,
        store_as: Option<&str>,
        model: Option<&str>,
    ) -> Result<CentroidResult, MemoryStoreError> {
        if matching.is_empty() {
            return Err(MemoryStoreError::NoEmbeddingsForCentroid(
                selection.to_string(),
            ));
        }

        let mut centroid = vec![0.0f32; dim];
        for (_, vector) in matching {
            if vector.len() != dim {
                return Err(EmbeddingError::DimensionMismatch {
                    expected: dim,
                    actual: vector.len(),
                }
                .into());
            }
            for (sum, value) in centroid.iter_mut().zip(vector) {
                *sum += value;
            }
        }

        let count = matching.len();
        let n = count as f32;
        for v in centroid.iter_mut() {
            *v /= n;
        }

        let stored = if let Some(ref_to_store) = store_as {
            match model {
                Some(m) => {
                    // A centroid has one current value, unlike multi-passage entities.
                    self.embedding.replace(ref_to_store, &centroid, m, None)?;
                    true
                }
                None => false,
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

    /// Replace an existing h_mem's value and confidence atomically,
    /// deleting the prior row rather than retaining a superseded version.
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

    /// Delete every h_mem under an entity prefix, in one statement,
    /// then remove the emptied entities' embeddings and memory_links
    /// rows. Returns the number of h_mems deleted. The forgetting pass
    /// uses this to forget a distilled thread's shared-copy turns —
    /// forgotten rows are removed from the database (operator ruling
    /// 2026-09-04: there is no "expired" state).
    pub fn delete_h_mems_by_entity_prefix(&self, prefix: &str) -> Result<usize, MemoryStoreError> {
        // Collect the affected entities before deleting — after the
        // delete there is nothing left to enumerate.
        let affected: HashSet<String> = self
            .h_mem_store
            .query_by_entity_prefix(prefix, 100_000)?
            .into_iter()
            .map(|h_mem| h_mem.entity)
            .collect();
        let count = self.h_mem_store.delete_by_entity_prefix(prefix)?;
        for entity in &affected {
            self.cleanup_orphaned_references_for_entity(entity);
        }
        if count > 0 {
            tracing::debug!(
                target: "hkask.memory",
                prefix,
                count,
                "h_mems deleted by entity prefix (forgotten)"
            );
        }
        Ok(count)
    }

    /// Delete every embedding under an entity — vectors and metadata.
    /// The forgetting pass uses this so a forgotten thread's turn
    /// embeddings stop dominating semantic recall.
    pub fn delete_embeddings_by_entity(&self, entity_ref: &str) -> Result<usize, MemoryStoreError> {
        Ok(self.embedding.delete_all_by_entity_ref(entity_ref)?)
    }

    /// Delete the embedding rows of one entity whose passage text is in
    /// `passages`. The coverage-scoped forgetting pass uses this to remove
    /// only the embeddings of chunks a distillation watermark proves
    /// distilled, keeping newer chunks' embeddings recallable. Rows whose
    /// passage is not listed (including NULL-passage legacy rows) survive —
    /// the caller decides what is covered.
    pub fn delete_embeddings_by_entity_passages(
        &self,
        entity_ref: &str,
        passages: &[String],
    ) -> Result<usize, MemoryStoreError> {
        Ok(self
            .embedding
            .delete_by_entity_ref_and_passages(entity_ref, passages)?)
    }

    /// Delete orphaned rows in all three coupled stores: embeddings whose
    /// entity has no h_mem (the entity_ref join key is broken — KNN
    /// silently drops them, so they are dead weight and a false "memory
    /// exists" signal), vector rows orphaned from their metadata rows,
    /// and memory_links rows referencing entities with no h_mems.
    /// Deletion sites clean their own orphans (`delete_h_mem`, the prune
    /// valve, `delete_h_mems_by_entity_prefix`); this sweep is the
    /// belt-and-suspenders backstop that also catches rows predating the
    /// site-level cleanup (the 57 entity-ref orphans found by the
    /// 2026-09-09 therapy scan) and anything a failed site-level cleanup
    /// left behind. Returns the total number of rows removed.
    pub fn delete_orphaned_embeddings(&self) -> Result<usize, MemoryStoreError> {
        let driver = self.h_mem_store.driver();
        let mut deleted = driver
            .execute(
                "DELETE FROM embeddings WHERE entity_ref NOT IN (SELECT DISTINCT entity FROM hmems)",
                &[],
            )
            .map_err(|e| MemoryStoreError::HMem(HMemError::from(e)))?;
        deleted += driver
            .execute(
                "DELETE FROM memory_links WHERE entity_a NOT IN (SELECT DISTINCT entity FROM hmems) \
                 OR entity_b NOT IN (SELECT DISTINCT entity FROM hmems)",
                &[],
            )
            .map_err(|e| MemoryStoreError::HMem(HMemError::from(e)))?;
        deleted += self.embedding.delete_orphaned_vectors()?;
        Ok(deleted)
    }

    // ── Budget / cleanup ──────────────────────────────────────────────────

    pub fn h_mem_count(&self) -> Result<usize, MemoryStoreError> {
        Ok(self.h_mem_store.count()?)
    }

    /// Delete an h_mem and, when this was the entity's last row, the
    /// entity's embeddings and memory_links rows. The entity_ref is the
    /// join key between embeddings and h_mems (README: "One entity_ref
    /// string links each embedding vector to its relational h_mem row"),
    /// so rows left behind after the entity empties are orphans. The
    /// operator's 2026-09-09 ruling: deletions must clean up what they
    /// orphan — no orphan piles, no compatibility states.
    pub fn delete_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), MemoryStoreError> {
        let entity = self.h_mem_store.get_by_id(id)?.map(|h_mem| h_mem.entity);
        self.h_mem_store.delete_by_id(id)?;
        if let Some(entity) = entity {
            self.cleanup_orphaned_references_for_entity(&entity);
        }
        Ok(())
    }

    /// Remove the embeddings and memory_links rows of an entity whose
    /// h_mems were all deleted. Failures are logged, not propagated:
    /// callers have already deleted the h_mems (the primary operation
    /// succeeded), and the periodic orphan sweep in
    /// [`Self::delete_orphaned_embeddings`] is the backstop that catches
    /// anything a failed cleanup leaves behind.
    fn cleanup_orphaned_references_for_entity(&self, entity: &str) {
        let remaining = match self.h_mem_store.query_by_entity(entity) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    target: "hkask.memory",
                    %error,
                    entity,
                    "Failed to check for orphaned references after deletion — periodic sweep will retry"
                );
                return;
            }
        };
        if !remaining.is_empty() {
            return;
        }
        match self.embedding.delete_all_by_entity_ref(entity) {
            Ok(count) if count > 0 => tracing::debug!(
                target: "hkask.memory",
                entity,
                count,
                "Deleted orphaned embeddings of emptied entity"
            ),
            Ok(_) => {}
            Err(error) => tracing::warn!(
                target: "hkask.memory",
                %error,
                entity,
                "Failed to delete orphaned embeddings of emptied entity — periodic sweep will retry"
            ),
        }
        if let Err(error) = self.h_mem_store.driver().execute(
            "DELETE FROM memory_links WHERE entity_a = ?1 OR entity_b = ?1",
            &[DbValue::Text(entity.to_string())],
        ) {
            tracing::warn!(
                target: "hkask.memory",
                %error,
                entity,
                "Failed to delete orphaned memory_links of emptied entity — periodic sweep will retry"
            );
        }
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

    // ── Age-based pruning ──────────────────────────────────────────────
    //
    // Confidence decay (Wozniak-Gorzelanczyk) lowers recall weight over time
    // but never deletes. Age-based pruning is the complementary operation:
    // it hard-deletes h_mems older than a cutoff, bounding storage growth from
    // the time axis. The caller can spare actively-recalled memories by
    // setting `spare_recalled_within_days` — an h_mem recalled recently stays
    // even if it is old.

    /// Prune h_mems older than `max_age_days`, optionally sparing h_mems
    /// recalled within the last `spare_recalled_within_days` days.
    ///
    /// Full-store scope — every entity is a candidate, including
    /// knowledge-layer rows (operator rulings, verified status, reified
    /// lessons). The curator's forgetting valve uses
    /// [`prune_by_age_in_prefixes`] instead, so durable knowledge survives
    /// episodic turnover.
    ///
    /// Returns the count of deleted h_mems. Failures on individual deletes
    /// are counted but do not abort the batch — a single stale row must not
    /// block pruning of the rest.
    pub fn prune_by_age(
        &self,
        max_age_days: i64,
        spare_recalled_within_days: Option<i64>,
    ) -> Result<PruneOutcome, MemoryStoreError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days);
        let candidates = self.h_mem_store.query_older_than(&cutoff, 100_000)?;
        let spare_cutoff = spare_recalled_within_days
            .map(|days| chrono::Utc::now() - chrono::Duration::days(days));
        Ok(self.prune_candidates(max_age_days, candidates, spare_cutoff))
    }

    /// Prune aged h_mems scoped to entity prefixes — the safe default for
    /// the curator's forgetting valve. Turn storage (the `curator:thread:`
    /// and `chat:thread:` prefixes) is episodic and prunable by age;
    /// knowledge-layer rows outside the prefixes outlive episodic turnover
    /// unless the caller explicitly opts into full-store [`prune_by_age`].
    pub fn prune_by_age_in_prefixes(
        &self,
        prefixes: &[&str],
        max_age_days: i64,
        spare_recalled_within_days: Option<i64>,
    ) -> Result<PruneOutcome, MemoryStoreError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days);
        let candidates = self
            .h_mem_store
            .query_older_than(&cutoff, 100_000)?
            .into_iter()
            .filter(|h_mem| {
                prefixes
                    .iter()
                    .any(|prefix| h_mem.entity.starts_with(prefix))
            })
            .collect();
        let spare_cutoff = spare_recalled_within_days
            .map(|days| chrono::Utc::now() - chrono::Duration::days(days));
        Ok(self.prune_candidates(max_age_days, candidates, spare_cutoff))
    }

    /// Delete each prune candidate, sparing ones recalled more recently
    /// than `spare_cutoff`, then remove the emptied entities' embeddings
    /// and memory_links rows (deletions must clean up what they orphan —
    /// operator ruling 2026-09-09). Individual delete failures are
    /// counted, not propagated — one stuck row must not block pruning of
    /// the rest.
    fn prune_candidates(
        &self,
        max_age_days: i64,
        candidates: Vec<HMem>,
        spare_cutoff: Option<chrono::DateTime<chrono::Utc>>,
    ) -> PruneOutcome {
        let mut deleted_count = 0usize;
        let mut spared_count = 0usize;
        let mut failed_count = 0usize;
        let mut deleted_entities: HashSet<String> = HashSet::new();

        for h_mem in &candidates {
            // Spare actively-recalled memories when the caller requested it.
            if let Some(ref spare_cutoff) = spare_cutoff {
                if h_mem.recalled_at > *spare_cutoff {
                    spared_count += 1;
                    continue;
                }
            }
            match self.h_mem_store.delete_by_id(&h_mem.id) {
                Ok(()) => {
                    deleted_count += 1;
                    deleted_entities.insert(h_mem.entity.clone());
                }
                Err(error) => {
                    failed_count += 1;
                    tracing::warn!(
                        target: "reg.consolidation",
                        %error,
                        h_mem_id = %h_mem.id.as_uuid(),
                        "Failed to delete aged h_mem during prune_by_age"
                    );
                }
            }
        }

        for entity in &deleted_entities {
            self.cleanup_orphaned_references_for_entity(entity);
        }

        tracing::info!(
            target: "reg.consolidation",
            max_age_days,
            candidates = candidates.len(),
            deleted = deleted_count,
            spared = spared_count,
            failed = failed_count,
            "Age-based prune complete"
        );

        PruneOutcome {
            candidates: candidates.len(),
            deleted_count,
            spared_count,
            failed_count,
        }
    }

    // ── Near-duplicate value dedup ─────────────────────────────────
    //
    // The existing dedup layers do NOT cover this case:
    // 1. `recall_dedup::dedup_h_mems` — recall-time exact-EAV hash filter
    //    (first-seen wins, no store mutation, no fuzzy matching).
    // 2. Therapy skill — LLM-driven semantic contradiction resolution
    //    (operator-in-the-loop, handles divergent values, not near-duplicates).
    //
    // This pass is the missing fourth layer: deterministic fuzzy string
    // dedup that deletes stored near-duplicates. It normalizes string
    // values (lowercase, strip punctuation, collapse whitespace) and groups
    // h_mems by (entity, attribute, normalized_value), keeping the
    // highest-confidence one and deleting the rest. Non-string values are
    // skipped — structural dedup is the EAV path's job.

    /// Deduplicate h_mems by normalized string value within each
    /// (entity, attribute) group. For each group of near-duplicate
    /// values, the highest-confidence h_mem is kept and the rest are
    /// deleted. Returns the count of deleted duplicates.
    /// Scans all stored h_mems (no perspective filter)
    /// — the curator's memory is a single store and dedup is global.
    pub fn dedup_by_normalized_value(
        &self,
        limit: usize,
    ) -> Result<DedupOutcome, MemoryStoreError> {
        let h_mems = self.h_mem_store.query_all(limit)?;

        // Group by (entity, attribute, normalized_value) for string values.
        let mut groups: std::collections::HashMap<(String, String, String), Vec<&HMem>> =
            std::collections::HashMap::new();
        let mut skipped_non_string = 0usize;

        for h_mem in &h_mems {
            let Some(value_str) = h_mem.value.as_str() else {
                skipped_non_string += 1;
                continue;
            };
            let normalized = normalize_value(value_str);
            let key = (h_mem.entity.clone(), h_mem.attribute.clone(), normalized);
            groups.entry(key).or_default().push(h_mem);
        }

        let mut deleted_count = 0usize;
        let mut failed_count = 0usize;
        let mut groups_with_dupes = 0usize;

        for (_key, group) in &groups {
            if group.len() < 2 {
                continue;
            }
            groups_with_dupes += 1;
            // Keep the highest-confidence h_mem; delete the rest.
            let mut sorted = group.clone();
            sorted.sort_by(|a, b| {
                b.confidence
                    .value()
                    .partial_cmp(&a.confidence.value())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            // The first one is kept; delete the rest.
            for h_mem in &sorted[1..] {
                match self.h_mem_store.delete_by_id(&h_mem.id) {
                    Ok(()) => deleted_count += 1,
                    Err(error) => {
                        failed_count += 1;
                        tracing::warn!(
                            target: "reg.consolidation",
                            %error,
                            h_mem_id = %h_mem.id.as_uuid(),
                            "Failed to delete duplicate h_mem during dedup_by_normalized_value"
                        );
                    }
                }
            }
        }

        tracing::info!(
            target: "reg.consolidation",
            scanned = h_mems.len(),
            groups_with_dupes,
            deleted = deleted_count,
            failed = failed_count,
            skipped_non_string,
            "Normalized-value dedup complete"
        );

        Ok(DedupOutcome {
            scanned: h_mems.len(),
            groups_with_dupes,
            deleted_count,
            failed_count,
            skipped_non_string,
        })
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
        // A column-read error must surface, not read as 0 (the
        // `.rules` unwrap_or(0) sense-input trap).
        let count = rows[0]
            .get_int(0)
            .map_err(|e| MemoryStoreError::HMem(HMemError::from(e)))? as u64;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::SqliteDriver;
    use hkask_types::Confidence;

    fn test_store() -> MemoryStore {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store =
            hkask_storage::HMemStore::from_driver(Arc::clone(&driver)).expect("h_mem store");
        let embedding_store =
            hkask_storage::EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())
                .expect("embedding store");
        MemoryStore::new(h_mem_store, embedding_store)
    }

    fn store_h_mem(store: &MemoryStore, entity: &str, attribute: &str, value: &str, webid: WebID) {
        let h_mem = hkask_storage::HMem::new(
            entity,
            attribute,
            serde_json::Value::String(value.to_string()),
            webid,
        );
        store.store(h_mem).expect("store h_mem");
    }

    /// expect: "The configured memory life changes how memories decay — the
    /// setting is behavior, not monitoring decoration." [P1]
    /// pre: two stores over in-memory drivers hold an identical h_mem whose
    /// recall clock was last touched 15 days ago; one store uses the default
    /// S=180, the other the operator-configured S=30.
    /// post: querying through the decay-applying path returns measurably
    /// deeper decay under S=30 than S=180 — R(15/30) = exp(-0.5) ≈ 0.607
    /// vs R(15/180) ≈ 0.920 — so `with_memory_life_days` demonstrably
    /// reaches the curve, not just a sensor field.
    #[test]
    fn memory_life_days_controls_decay_depth() {
        let webid = WebID::from_persona(b"curator");
        let driver_short = SqliteDriver::in_memory_driver();
        let h_mem_store_short =
            hkask_storage::HMemStore::from_driver(std::sync::Arc::clone(&driver_short))
                .expect("hmem store");
        let embedding_store_short = hkask_storage::EmbeddingStore::from_driver(
            driver_short,
            hkask_storage::embedding_dim(),
        )
        .expect("embedding store");
        let short_store =
            MemoryStore::new(h_mem_store_short, embedding_store_short).with_memory_life_days(30.0);
        let default_store = test_store();

        for store in [&default_store, &short_store] {
            let mut h_mem = hkask_storage::HMem::new(
                "decay:probe",
                "fact",
                serde_json::Value::String("probe".to_string()),
                webid,
            );
            h_mem.recalled_at = chrono::Utc::now() - chrono::Duration::days(15);
            store.h_mem_store.insert(&h_mem).expect("seed probe");
        }

        let decayed_default = default_store
            .query_by_attribute("fact")
            .expect("query default")
            .remove(0)
            .confidence
            .value();
        let decayed_short = short_store
            .query_by_attribute("fact")
            .expect("query short")
            .remove(0)
            .confidence
            .value();

        assert!(
            decayed_short < decayed_default,
            "S=30 must decay deeper than S=180 for the same recall age: \
             {decayed_short} !< {decayed_default}"
        );
        let expected_default = (-15.0_f64 / 180.0).exp();
        assert!(
            (decayed_default - expected_default).abs() < 1e-9,
            "default store must follow R(t)=exp(-t/180) from full confidence: \
             {decayed_default} vs {expected_default}"
        );
    }

    #[test]
    fn prune_by_age_deletes_old_h_mems() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        // Insert an old h_mem by backdating its observed_at via direct store access.
        let mut old = hkask_storage::HMem::new(
            "company:AAPL",
            "sector",
            serde_json::Value::String("technology".to_string()),
            webid,
        );
        old.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        // Backdate valid_from in the DB by inserting then updating — simpler:
        // insert directly with the backdated timestamp via the store's insert.
        store.h_mem_store.insert(&old).expect("insert old h_mem");

        // Insert a recent h_mem.
        store_h_mem(&store, "company:MSFT", "sector", "technology", webid);

        let outcome = store.prune_by_age(50, None).expect("prune succeeds");
        assert_eq!(outcome.candidates, 1, "one h_mem is older than 50 days");
        assert_eq!(outcome.deleted_count, 1);
        assert_eq!(outcome.spared_count, 0);

        // The recent h_mem survives.
        let remaining = store
            .h_mem_store
            .query_by_entity("company:MSFT")
            .expect("query");
        assert_eq!(remaining.len(), 1);

        // The old h_mem is gone.
        let old_remaining = store
            .h_mem_store
            .query_by_entity("company:AAPL")
            .expect("query");
        assert!(old_remaining.is_empty(), "old h_mem was pruned");
        assert!(store.get_by_id(&old.id).expect("query pruned ID").is_none());
        assert_eq!(store.h_mem_count().expect("row count"), 1);
    }

    #[test]
    fn prune_by_age_in_prefixes_spares_knowledge_rows() {
        // The forgetting valve is turn-scoped by default: aged episodic
        // rows are deleted while knowledge-layer rows (rulings, verified
        // status, lessons) survive regardless of age. Pinning this guards
        // the valve against silently widening to full-store.
        let store = test_store();
        let webid = WebID::from_persona(b"curator");

        let mut aged_turn = hkask_storage::HMem::new(
            "curator:thread:prune-scope-test",
            "turn",
            serde_json::Value::String("aged turn".to_string()),
            webid,
        );
        aged_turn.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        store
            .h_mem_store
            .insert(&aged_turn)
            .expect("insert aged turn");

        let mut aged_ruling = hkask_storage::HMem::new(
            "zed-kask/provider_budget_blocks",
            "operator_ruling",
            serde_json::Value::String("do not touch the provider files".to_string()),
            webid,
        );
        aged_ruling.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        store
            .h_mem_store
            .insert(&aged_ruling)
            .expect("insert aged ruling");

        let outcome = store
            .prune_by_age_in_prefixes(&["curator:thread:", "chat:thread:"], 50, None)
            .expect("scoped prune succeeds");
        assert_eq!(outcome.candidates, 1, "only the turn row is in scope");
        assert_eq!(outcome.deleted_count, 1);

        let ruling = store
            .h_mem_store
            .query_by_entity("zed-kask/provider_budget_blocks")
            .expect("query ruling");
        assert_eq!(ruling.len(), 1, "knowledge row survives the scoped valve");

        let turn = store
            .h_mem_store
            .query_by_entity("curator:thread:prune-scope-test")
            .expect("query turn");
        assert!(turn.is_empty(), "aged turn row was pruned");

        // Explicit full-store opt-in does reach knowledge rows.
        let outcome = store.prune_by_age(50, None).expect("full prune succeeds");
        assert_eq!(
            outcome.deleted_count, 1,
            "the aged knowledge row is deleted"
        );
    }

    #[test]
    fn prune_by_age_sares_recalled_within_grace_window() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        // Old h_mem that was recalled recently.
        let mut old_recalled = hkask_storage::HMem::new(
            "company:AAPL",
            "sector",
            serde_json::Value::String("technology".to_string()),
            webid,
        );
        old_recalled.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        old_recalled.recalled_at = chrono::Utc::now() - chrono::Duration::days(1);
        store.h_mem_store.insert(&old_recalled).expect("insert");

        // Old h_mem that was NOT recalled recently.
        let mut old_stale = hkask_storage::HMem::new(
            "company:GOOG",
            "sector",
            serde_json::Value::String("technology".to_string()),
            webid,
        );
        old_stale.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        old_stale.recalled_at = chrono::Utc::now() - chrono::Duration::days(90);
        store.h_mem_store.insert(&old_stale).expect("insert");

        let outcome = store.prune_by_age(50, Some(30)).expect("prune succeeds");
        assert_eq!(outcome.candidates, 2);
        assert_eq!(outcome.deleted_count, 1, "only the stale one is deleted");
        assert_eq!(
            outcome.spared_count, 1,
            "the recently-recalled one is spared"
        );

        // The recalled one survives.
        let survived = store
            .h_mem_store
            .query_by_entity("company:AAPL")
            .expect("query");
        assert_eq!(survived.len(), 1, "recently-recalled old h_mem was spared");

        // The stale one is gone.
        let gone = store
            .h_mem_store
            .query_by_entity("company:GOOG")
            .expect("query");
        assert!(gone.is_empty(), "stale old h_mem was pruned");
    }

    #[test]
    fn dedup_by_normalized_value_deletes_near_duplicates() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        // Three h_mems for the same entity+attribute with near-duplicate values.
        store_h_mem(&store, "company:AAPL", "ticker", "AAPL", webid);
        store_h_mem(&store, "company:AAPL", "ticker", "aapl.", webid);
        store_h_mem(&store, "company:AAPL", "ticker", " AAPL ", webid);
        // A distinct value that should NOT be deleted.
        store_h_mem(&store, "company:AAPL", "ticker", "MSFT", webid);
        // A non-string value that should be skipped.
        let numeric = hkask_storage::HMem::new(
            "company:AAPL",
            "price",
            serde_json::Value::Number(serde_json::Number::from(150)),
            webid,
        );
        store.store(numeric).expect("store numeric");

        let outcome = store
            .dedup_by_normalized_value(1000)
            .expect("dedup succeeds");
        assert_eq!(outcome.scanned, 5);
        assert_eq!(
            outcome.groups_with_dupes, 1,
            "one group has near-duplicates"
        );
        assert_eq!(outcome.deleted_count, 2, "two near-duplicates deleted");
        assert_eq!(outcome.skipped_non_string, 1, "numeric value skipped");

        // The remaining h_mems for ticker: one of the AAPL variants + MSFT.
        let remaining = store
            .h_mem_store
            .query_by_entity_attribute("company:AAPL", "ticker")
            .expect("query");
        // query_by_entity_attribute returns all current h_mems — forgetting
        // deletes rows; there is no validity filter.
        assert_eq!(
            remaining.len(),
            2,
            "one AAPL variant + MSFT survive; two AAPL variants deleted"
        );
    }

    #[test]
    fn dedup_keeps_highest_confidence() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        // Low-confidence duplicate inserted first.
        let mut low = hkask_storage::HMem::new(
            "company:AAPL",
            "sector",
            serde_json::Value::String("technology".to_string()),
            webid,
        );
        low.confidence = Confidence::new(0.3);
        store.store(low).expect("store");

        // High-confidence duplicate inserted second.
        let mut high = hkask_storage::HMem::new(
            "company:AAPL",
            "sector",
            serde_json::Value::String("Technology".to_string()),
            webid,
        );
        high.confidence = Confidence::new(0.9);
        store.store(high).expect("store");

        let outcome = store
            .dedup_by_normalized_value(1000)
            .expect("dedup succeeds");
        assert_eq!(outcome.deleted_count, 1);

        // The high-confidence one survives.
        let remaining = store
            .h_mem_store
            .query_by_entity_attribute("company:AAPL", "sector")
            .expect("query");
        assert_eq!(remaining.len(), 1);
        assert!((remaining[0].confidence.value() - 0.9).abs() < 1e-9);
        let stored_rows = store
            .h_mem_store
            .driver()
            .query_optional("SELECT count(*) FROM hmems", &[])
            .expect("raw row count")
            .expect("count row")
            .get_int(0)
            .expect("integer count");
        assert_eq!(
            stored_rows, 1,
            "dedup must delete, not hide, the duplicate row"
        );
    }

    #[test]
    fn normalize_value_handles_case_punctuation_and_whitespace() {
        // Direct unit test of the normalization helper.
        assert_eq!(normalize_value("AAPL"), "aapl");
        assert_eq!(normalize_value("AAPL."), "aapl");
        assert_eq!(normalize_value(" aapl "), "aapl");
        assert_eq!(normalize_value("AaPL,  Inc."), "aapl inc");
        assert_eq!(normalize_value("  multiple   spaces  "), "multiple spaces");
        assert_eq!(normalize_value(""), "");
    }

    #[test]
    fn normalize_value_strips_punctuation_from_numeric_strings() {
        // Documents the aggressive punctuation stripping: all ASCII
        // punctuation is removed, not just trailing. This means "3.14" and
        // "314" normalize to the same key. This is intentional for ticker-like
        // values ("AAPL." == "aapl") but may surprise for decimal strings.
        // Non-string JSON values (numbers) are skipped by dedup entirely, so
        // this only affects numeric values stored as JSON strings.
        assert_eq!(normalize_value("3.14"), "314");
        assert_eq!(normalize_value("3,14"), "314");
        assert_eq!(normalize_value("3-14"), "314");
        // Values that differ only in punctuation are treated as duplicates.
        assert_eq!(normalize_value("3.14"), normalize_value("314"));
        // But values with different digits are NOT duplicates.
        assert_ne!(normalize_value("3.14"), normalize_value("3.15"));
    }

    #[test]
    fn h_mems_by_entity_prefix_scopes_shared_thread_chunks() {
        // Curator and non-curator turns use the same shared-copy prefix;
        // extraction scopes that prefix to one thread.
        let store = test_store();
        let webid = WebID::from_persona(b"curator");

        let curator_turn = hkask_storage::HMem::new(
            "curator:thread:curator-thread",
            "chunk:0",
            serde_json::Value::String("curator turn content".to_string()),
            webid,
        );
        store.store(curator_turn).expect("store curator turn");

        let shared_turn = hkask_storage::HMem::new(
            "curator:thread:agent-thread",
            "chunk:0",
            serde_json::Value::String("shared turn content".to_string()),
            webid,
        );
        store.store(shared_turn).expect("store shared turn");

        let curator_results = store
            .h_mems_by_entity_prefix("curator:thread:curator-thread")
            .expect("query curator thread");
        assert_eq!(curator_results.len(), 1);

        let agent_results = store
            .h_mems_by_entity_prefix("curator:thread:agent-thread")
            .expect("query agent thread");
        assert_eq!(agent_results.len(), 1);
        assert_ne!(curator_results[0].entity, agent_results[0].entity);
        assert_eq!(
            store
                .h_mems_by_entity_prefix("curator:thread:")
                .expect("query all threads")
                .len(),
            2
        );
    }

    /// Pin the 2026-09-09 orphan-cleanup ruling (operator: "changes can
    /// orphan things if they delete them — you can't orphan things and
    /// leave them"): deleting the last h_mem of an entity must remove the
    /// entity's embedding and memory_links rows. The entity_ref is the
    /// join key between embeddings and h_mems; once no h_mem backs it,
    /// the remaining rows are orphans KNN silently drops.
    #[test]
    fn delete_h_mem_cleans_references_when_entity_emptied() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        store_h_mem(&store, "thread:a", "turn", "one", webid);
        store_h_mem(&store, "thread:a", "turn", "two", webid);
        store
            .store_embedding(
                "thread:a",
                &vec![0.1; hkask_storage::embedding_dim()],
                "test-model",
                Some("turn text"),
            )
            .expect("seed embedding");
        store
            .record_co_occurrence(&["thread:a".to_string(), "thread:b".to_string()])
            .expect("seed link");

        let first = store
            .h_mem_store
            .query_by_entity("thread:a")
            .expect("query first")
            .remove(0);
        store.delete_h_mem(&first.id).expect("delete one");
        // The entity still holds a row — its references must survive.
        assert_eq!(
            store
                .embedding
                .get_all_by_prefix("thread:a")
                .expect("embeddings survive")
                .len(),
            1
        );
        assert!(store.connectedness("thread:a").expect("link survives") > 0);

        let second = store
            .h_mem_store
            .query_by_entity("thread:a")
            .expect("query second")
            .remove(0);
        store.delete_h_mem(&second.id).expect("delete last");
        assert_eq!(
            store
                .embedding
                .get_all_by_prefix("thread:a")
                .expect("embeddings swept")
                .len(),
            0,
            "deleting the last h_mem of an entity must remove its embeddings"
        );
        assert_eq!(
            store.connectedness("thread:a").expect("links swept"),
            0,
            "deleting the last h_mem of an entity must remove its memory_links"
        );
    }

    /// Pin the prune valve to the same orphan-cleanup ruling: pruning the
    /// aged rows of an entity must not leave its embeddings behind.
    #[test]
    fn prune_by_age_cleans_references_of_emptied_entities() {
        let store = test_store();
        let webid = WebID::from_persona(b"curator");
        let mut aged = hkask_storage::HMem::new(
            "thread:old",
            "turn",
            serde_json::Value::String("aged".to_string()),
            webid,
        );
        aged.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
        store.h_mem_store.insert(&aged).expect("insert aged h_mem");
        store
            .store_embedding(
                "thread:old",
                &vec![0.1; hkask_storage::embedding_dim()],
                "test-model",
                Some("aged turn text"),
            )
            .expect("seed embedding");

        let outcome = store.prune_by_age(50, None).expect("prune succeeds");
        assert!(outcome.deleted_count >= 1, "aged row must be pruned");
        assert_eq!(
            store
                .embedding
                .get_all_by_prefix("thread:old")
                .expect("embeddings swept")
                .len(),
            0,
            "pruning the last h_mem of an entity must remove its embeddings"
        );
    }

    /// Pin the periodic sweep: embeddings whose entity has no h_mem row
    /// (broken join key) and links referencing h_mem-less entities are
    /// removed — the backstop that also catches rows predating the
    /// site-level cleanup (the 57 entity-ref orphans of the 2026-09-09
    /// therapy scan).
    #[test]
    fn delete_orphaned_embeddings_sweeps_entity_ref_and_link_orphans() {
        let store = test_store();
        // An embedding whose entity has no h_mem — the join key is broken.
        store
            .store_embedding(
                "goal:ghost",
                &vec![0.2; hkask_storage::embedding_dim()],
                "test-model",
                Some("ghost text"),
            )
            .expect("seed orphan embedding");
        // A link whose endpoints have no h_mems.
        store
            .record_co_occurrence(&["goal:ghost".to_string(), "goal:missing".to_string()])
            .expect("seed orphan link");

        let deleted = store.delete_orphaned_embeddings().expect("sweep succeeds");
        assert!(
            deleted >= 2,
            "sweep must remove the orphaned embedding and link rows, got {deleted}"
        );
        assert_eq!(
            store
                .embedding
                .get_all_by_prefix("goal:ghost")
                .expect("embeddings swept")
                .len(),
            0
        );
        assert_eq!(store.connectedness("goal:ghost").expect("links swept"), 0);
    }

    /// T09: passage-scoped deletion removes exactly the named passage's
    /// text and vector; sibling passages of the same entity survive and
    /// remain semantically retrievable; the entity's h_mems are untouched.
    #[test]
    fn passage_deletion_removes_only_the_named_passage() {
        let store = test_store();
        let webid = WebID::from_persona(b"tester");
        store_h_mem(&store, "thread:t09", "turn", "turn content", webid);
        let dim = hkask_storage::embedding_dim();
        let one_hot = |index: usize| {
            let mut vector = vec![0.0; dim];
            vector[index % dim] = 1.0;
            vector
        };
        let alpha = one_hot(0);
        let beta = one_hot(1);
        let gamma = one_hot(2);

        store
            .store_embedding("thread:t09", &alpha, "test-model", Some("alpha passage"))
            .expect("seed alpha");
        store
            .store_embedding("thread:t09", &beta, "test-model", Some("beta passage"))
            .expect("seed beta");
        store
            .store_embedding("thread:t09", &gamma, "test-model", Some("gamma passage"))
            .expect("seed gamma");
        assert_eq!(store.embedding_count().expect("count"), 3);

        // Delete only the middle passage.
        let deleted = store
            .delete_embeddings_by_entity_passages("thread:t09", &["beta passage".to_string()])
            .expect("delete passage");
        assert_eq!(deleted, 1, "exactly the named passage's row is deleted");
        assert_eq!(store.embedding_count().expect("count"), 2);

        // The deleted passage is no longer retrievable.
        let hits = store.search_similar(&beta, 3).expect("search");
        assert!(
            hits.iter()
                .all(|hit| hit.embedding.passage_text.as_deref() != Some("beta passage")),
            "the deleted passage must not be retrievable"
        );

        // The siblings are the nearest matches for their own vectors.
        for (vector, name) in [(&alpha, "alpha passage"), (&gamma, "gamma passage")] {
            let hits = store.search_similar(vector, 1).expect("search");
            assert_eq!(
                hits[0].embedding.passage_text.as_deref(),
                Some(name),
                "the surviving sibling must remain semantically retrievable"
            );
        }

        // The entity's h_mems survive — passage deletion is not entity
        // deletion, and the surviving embeddings are not orphans.
        let h_mems = store.query_deduped("thread:t09").expect("query");
        assert!(!h_mems.is_empty(), "the entity's h_mems must survive");
        assert_eq!(
            store.delete_orphaned_embeddings().expect("orphan sweep"),
            0,
            "surviving passages with a live entity are not orphans"
        );
    }

    /// T09 control: a NULL-passage legacy row survives a passage-listed
    /// deletion (the caller decides what is covered — a NULL passage is
    /// never in the list), and deleting an unlisted passage's text deletes
    /// nothing.
    #[test]
    fn passage_deletion_spares_null_passage_and_unlisted_entities() {
        let store = test_store();
        let webid = WebID::from_persona(b"tester");
        store_h_mem(&store, "thread:t09b", "turn", "turn content", webid);
        let dim = hkask_storage::embedding_dim();
        let vector = vec![0.5; dim];

        store
            .store_embedding(
                "thread:t09b",
                &vector,
                "test-model",
                Some("covered passage"),
            )
            .expect("seed covered");
        store
            .store_embedding("thread:t09b", &vector, "test-model", None)
            .expect("seed legacy NULL-passage row");
        assert_eq!(store.embedding_count().expect("count"), 2);

        // Deleting a passage from a DIFFERENT entity removes nothing here.
        assert_eq!(
            store
                .delete_embeddings_by_entity_passages(
                    "thread:other",
                    &["covered passage".to_string()]
                )
                .expect("delete other entity"),
            0
        );
        // Deleting the named passage spares the NULL-passage legacy row.
        assert_eq!(
            store
                .delete_embeddings_by_entity_passages(
                    "thread:t09b",
                    &["covered passage".to_string()]
                )
                .expect("delete covered"),
            1
        );
        assert_eq!(store.embedding_count().expect("count"), 1);
        let hits = store.search_similar(&vector, 1).expect("search");
        assert_eq!(
            hits[0].embedding.passage_text, None,
            "the NULL-passage legacy row must survive"
        );
    }

    // ── compute_centroid ─────────────────────────────────────────────

    #[test]
    fn compute_centroid_averages_prefix_embeddings_and_stores() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        let flat = |x: f32| vec![x; dim];
        store
            .store_embedding("style:test:chunk:1", &flat(0.2), "m", None)
            .expect("seed 1");
        store
            .store_embedding("style:test:chunk:2", &flat(0.4), "m", None)
            .expect("seed 2");
        store
            .store_embedding("style:test:chunk:3", &flat(0.6), "m", None)
            .expect("seed 3");
        // Off-prefix embeddings must not count toward the mean.
        store
            .store_embedding("other:chunk:1", &flat(9.0), "m", None)
            .expect("seed off-prefix");

        let result = store
            .compute_centroid(
                "style:test:",
                "style:test:centroid",
                dim,
                Some("style:test:centroid"),
                Some("m"),
            )
            .expect("centroid");

        assert_eq!(result.passage_count, 3);
        assert!(result.stored);
        for value in &result.centroid {
            assert!(
                (value - 0.4).abs() < 1e-6,
                "mean of 0.2/0.4/0.6 is 0.4, got {value}"
            );
        }

        // Idempotency: the stored centroid is excluded from its own
        // recomputation, so the passage count stays at the seeded set.
        let again = store
            .compute_centroid(
                "style:test:",
                "style:test:centroid",
                dim,
                Some("style:test:centroid"),
                Some("m"),
            )
            .expect("recompute");
        assert_eq!(
            again.passage_count, 3,
            "a stored centroid must never fold into its own successor"
        );
        for value in &again.centroid {
            assert!((value - 0.4).abs() < 1e-6);
        }
    }

    #[test]
    fn compute_centroid_excludes_rule_refs() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        store
            .store_embedding("style:test:chunk:1", &vec![0.5; dim], "m", None)
            .expect("seed passage");
        store
            .store_embedding("style:test:rule:1", &vec![9.0; dim], "m", None)
            .expect("seed rule");

        let result = store
            .compute_centroid("style:test:", "style:test:centroid", dim, None, None)
            .expect("centroid");
        assert_eq!(result.passage_count, 1, "rule refs must be excluded");
        assert!(!result.stored, "no store_as → nothing stored");
        for value in &result.centroid {
            assert!(
                (value - 0.5).abs() < 1e-6,
                "rule ref must not skew the mean"
            );
        }
    }

    #[test]
    fn compute_centroid_store_as_without_model_does_not_store() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        store
            .store_embedding("style:test:chunk:1", &vec![0.5; dim], "m", None)
            .expect("seed passage");
        let result = store
            .compute_centroid(
                "style:test:",
                "style:test:centroid",
                dim,
                Some("style:test:centroid"),
                None,
            )
            .expect("centroid");
        assert_eq!(result.passage_count, 1);
        assert!(
            !result.stored,
            "store_as without a model name must not store"
        );
    }

    /// expect: "Selected refs contribute once, without duplicating or altering source embeddings." [P3]
    #[test]
    fn compute_centroid_for_refs_math_and_exclusions() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        let first: Vec<f32> = (0..dim).map(|i| i as f32).collect();
        let second: Vec<f32> = first.iter().map(|v| v + 4.0).collect();
        for (entity_ref, vector) in [
            ("corpus:first", first.clone()),
            ("other:second", second.clone()),
            ("corpus:unselected", vec![99.0; dim]),
            ("style:test:rule:1", vec![99.0; dim]),
            ("style:test:centroid", vec![99.0; dim]),
        ] {
            store
                .store_embedding(entity_ref, &vector, "m", None)
                .expect("seed");
        }
        let refs: Vec<String> = [
            "corpus:first",
            "other:second",
            "corpus:first",
            "style:test:rule:1",
            "style:test:centroid",
            "style:test:hopper:centroid",
            "excluded:missing",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        for _ in 0..2 {
            let result = store
                .compute_centroid_for_refs(
                    &refs,
                    "excluded:missing",
                    dim,
                    Some("style:test:hopper:centroid"),
                    Some("m"),
                )
                .expect("selected centroid");
            assert_eq!(result.passage_count, 2);
            assert!(result.stored);
            assert_eq!(
                result.centroid,
                first.iter().map(|v| v + 2.0).collect::<Vec<_>>()
            );
            assert_eq!(store.embedding_count().expect("count"), 6);
        }
        assert_eq!(
            store
                .embedding
                .get("corpus:first")
                .expect("original")
                .vector,
            first
        );
        assert_eq!(
            store
                .embedding
                .get("other:second")
                .expect("original")
                .vector,
            second
        );
        let prefix = store.compute_centroid("style:test:", "style:test:centroid", dim, None, None);
        assert!(matches!(
            prefix,
            Err(MemoryStoreError::NoEmbeddingsForCentroid(_))
        ));
    }

    /// expect: "A missing selected passage or invalid dimension cannot silently produce a partial centroid." [P3]
    #[test]
    fn compute_centroid_for_refs_rejects_missing_empty_and_wrong_dimension() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        store
            .store_embedding("corpus:present", &vec![2.0; dim], "m", None)
            .expect("seed");
        let refs = vec!["corpus:present".into(), "corpus:missing".into()];
        let error = store
            .compute_centroid_for_refs(
                &refs,
                "style:test:centroid",
                dim,
                Some("style:test:centroid"),
                Some("m"),
            )
            .expect_err("missing ref");
        assert!(matches!(
            error,
            MemoryStoreError::Embedding(EmbeddingError::NotFound(_))
        ));
        assert_eq!(store.embedding_count().expect("count"), 1);
        for refs in [
            Vec::new(),
            vec![
                "style:test:rule:absent".into(),
                "style:test:centroid".into(),
            ],
        ] {
            assert!(matches!(
                store.compute_centroid_for_refs(&refs, "style:test:centroid", dim, None, None),
                Err(MemoryStoreError::NoEmbeddingsForCentroid(_))
            ));
        }
        assert!(matches!(
            store.compute_centroid_for_refs(&["corpus:present".into()], "", dim + 1, None, None),
            Err(MemoryStoreError::Embedding(
                EmbeddingError::DimensionMismatch { .. }
            ))
        ));
    }

    /// expect: "A failed centroid INSERT preserves the previous value and its KNN index." [P3]
    #[test]
    fn compute_centroid_insert_failure_preserves_original() -> anyhow::Result<()> {
        for trigger in [
            // Fail the metadata INSERT after replacement has deleted the old rows.
            "CREATE TRIGGER fail_centroid_insert BEFORE INSERT ON embeddings
             WHEN NEW.entity_ref = 'style:test:hopper:centroid'
             BEGIN SELECT RAISE(FAIL, 'forced centroid INSERT failure'); END;",
            // Occupy the new rowid so the subsequent vec INSERT actually fails.
            "CREATE TRIGGER fail_centroid_vec_insert AFTER INSERT ON embeddings
             WHEN NEW.entity_ref = 'style:test:hopper:centroid'
             BEGIN INSERT INTO vec_embeddings (rowid, embedding)
                 VALUES (NEW.rowid, NEW.vector); END;",
        ] {
            let driver = SqliteDriver::in_memory_driver();
            let store = MemoryStore::new(
                hkask_storage::HMemStore::from_driver(Arc::clone(&driver))?,
                hkask_storage::EmbeddingStore::from_driver(
                    Arc::clone(&driver),
                    hkask_storage::embedding_dim(),
                )?,
            );
            let dim = hkask_storage::embedding_dim();
            let destination = "style:test:hopper:centroid";
            let source = vec![2.0; dim];
            let original = vec![1.0; dim];
            let source_id = store.store_embedding("corpus:source", &source, "m", Some("source"))?;
            let original_id =
                store.store_embedding(destination, &original, "old", Some("original"))?;
            let pool = driver.sqlite_pool().expect("SQLite pool");
            pool.get()?.execute_batch(trigger)?;

            let error = store
                .compute_centroid_for_refs(
                    &["corpus:source".into()],
                    destination,
                    dim,
                    Some(destination),
                    Some("new"),
                )
                .expect_err("forced INSERT must fail");
            assert!(
                matches!(
                    error,
                    MemoryStoreError::Embedding(EmbeddingError::Storage(_))
                ),
                "{error}"
            );
            let retained = store.embedding.get(destination)?;
            assert_eq!(retained.id, original_id);
            assert_eq!(retained.vector, original);
            assert_eq!(retained.model, "old");
            assert_eq!(retained.passage_text.as_deref(), Some("original"));
            assert_eq!(store.embedding_count()?, 2);
            assert_eq!(store.embedding.get("corpus:source")?.id, source_id);
            assert_eq!(store.embedding.get("corpus:source")?.vector, source);
            let nearest = store.search_similar(&original, 1)?;
            let nearest = nearest.first().expect("old centroid remains indexed");
            assert_eq!(nearest.embedding.id, original_id);
            assert_eq!(nearest.distance, 0.0);
            let vectors: i64 =
                pool.get()?
                    .query_row("SELECT COUNT(*) FROM vec_embeddings", [], |row| row.get(0))?;
            assert_eq!(vectors, 2, "rollback must also remove any new index row");
        }
        Ok(())
    }

    /// expect: "Concurrent recomputations leave one centroid without copying source embeddings." [P3]
    #[test]
    fn compute_centroid_concurrently_keeps_one_destination() -> anyhow::Result<()> {
        let store = Arc::new(test_store());
        let dim = hkask_storage::embedding_dim();
        let destination = "style:test:hopper:centroid";
        let source = vec![2.0; dim];
        let source_id = store.store_embedding("corpus:source", &source, "m", Some("source"))?;
        // Ordinary embedding writes must still append, including old duplicate destinations.
        store.store_embedding(destination, &vec![0.0; dim], "old", None)?;
        store.store_embedding(destination, &vec![1.0; dim], "old", None)?;
        assert_eq!(store.embedding_count()?, 3);
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        for _ in 0..4 {
                            let result = store.compute_centroid_for_refs(
                                &["corpus:source".into(), "corpus:source".into()],
                                destination,
                                dim,
                                Some(destination),
                                Some("new"),
                            )?;
                            assert!(result.stored);
                            assert_eq!(result.passage_count, 1);
                            assert_eq!(result.centroid, source);
                        }
                        Ok::<_, MemoryStoreError>(())
                    })
                })
                .collect();
            for handle in handles {
                handle.join().expect("centroid worker")?;
            }
            Ok::<_, MemoryStoreError>(())
        })?;
        assert_eq!(store.embeddings_by_prefix(destination)?.len(), 1);
        assert_eq!(store.embedding_count()?, 2);
        assert_eq!(store.embedding.get(destination)?.vector, source);
        assert_eq!(store.embedding.get("corpus:source")?.id, source_id);
        assert_eq!(store.embedding.get("corpus:source")?.vector, source);
        assert_eq!(store.search_similar(&source, 10)?.len(), 2);
        Ok(())
    }

    #[test]
    fn compute_centroid_empty_prefix_set_errors() {
        let store = test_store();
        let dim = hkask_storage::embedding_dim();
        let error = store
            .compute_centroid("style:absent:", "style:absent:centroid", dim, None, None)
            .expect_err("no embeddings under prefix");
        assert!(
            matches!(error, MemoryStoreError::NoEmbeddingsForCentroid(ref prefix) if prefix == "style:absent:"),
            "wrong error: {error:?}"
        );
    }
}
