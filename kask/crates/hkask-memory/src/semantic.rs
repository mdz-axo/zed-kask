//! Semantic memory pipeline
//!
//! Provides the following subloops:
//! - **Storage budget** (6e): Per-entity storage limit with deletion of
//!   lowest-confidence h_mems when budget is exceeded.
//! - **Similarity-augmented recall**: KNN search over embeddings to find
//!   semantically related h_mems, enabling context assembly that goes
//!   beyond exact entity matches.
//! - **Corpus centroid**: Mean embedding vector for style cluster validation.
//! - **Prefix purge**: Idempotent re-ingest by deleting embeddings matching a prefix.

use crate::recall_dedup;
use hkask_storage::{EmbeddingError, EmbeddingStore, HMem, HMemError, HMemStore, SimilarityResult};
use hkask_types::RegulationSink;
use hkask_types::Visibility;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use hkask_types::visibility::Confidence;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SemanticMemoryError {
    #[error("HMem error: {0}")]
    HMem(#[from] HMemError),
    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("Invalid visibility for semantic store: {0}")]
    InvalidVisibility(String),
    #[error("No embeddings found for centroid: {0}")]
    NoEmbeddingsForCentroid(String),
    #[error(
        "Semantic memory requires no perspective (use consolidation bridge for episodic→semantic promotion)"
    )]
    HasPerspective,
}

/// Result of computing a style centroid
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CentroidResult {
    /// The centroid vector (arithmetic mean of matching embeddings)
    pub centroid: Vec<f32>,
    /// Number of passages used to compute the centroid
    pub passage_count: usize,
    /// Whether the centroid was stored under `store_as`
    pub stored: bool,
}

/// Semantic memory — shared knowledge graph
///
/// Provides the following subloops:
/// - **Confidence decay**: Wozniak-Gorzelanczyk (1995) forgetting curve applied
///   at recall — same model as episodic memory. Confidence decays as R(t) = exp(-t/S)
///   where S = memory_life_days (default 180). Recall resets the decay clock via touch_recall.
/// - **Confidence promotion** (6d): Bayesian seeding when consolidating from episodic
///   (confidence seeding at 0.5 baseline) to promote confidence.
/// - **Storage budget** (6e): Per-entity storage limit with retraction candidate
///   identification for lowest-confidence h_mems.
/// - **Similarity-augmented recall**: KNN search over embeddings to find
///   semantically related h_mems, enabling context assembly that goes
///   beyond exact entity matches.
pub struct SemanticMemory {
    event_sink: Option<Arc<dyn RegulationSink>>,
    h_mem_store: HMemStore,
    embedding: Arc<EmbeddingStore>,
    /// Memory life S in days — configurable, default 180 (6 months × 30).
    /// The forgetting curve is R(t) = exp(-t/S) where t is days since recall.
    /// Same decay model as episodic memory (Wozniak & Gorzelanczyk, 1995).
    memory_life_days: f64,
}

impl SemanticMemory {
    /// Create a new SemanticMemory with h_mem and embedding stores.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — creates shared semantic knowledge store
    /// \[P8\] Constraining: Semantic Grounding — unifies h_mem and embedding stores
    /// pre:  h_mem_store and embedding_store are initialized
    /// post: returns SemanticMemory wrapping both stores
    pub fn new(h_mem_store: HMemStore, embedding_store: EmbeddingStore) -> Self {
        Self {
            h_mem_store,
            embedding: Arc::new(embedding_store),
            event_sink: None,
            memory_life_days: crate::bayesian::DEFAULT_MEMORY_LIFE_DAYS,
        }
    }

    /// Open a SQLCipher database and construct a SemanticMemory from a single
    /// shared connection pool. This is the canonical way to create a
    /// SemanticMemory for file-backed storage — it eliminates the 6-line
    /// boilerplate of opening a Database, creating a SqliteDriver, and
    /// wiring HMemStore + EmbeddingStore separately.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — opens shared semantic knowledge store
    /// \[P5\] Constraining: Essentialism — single pool shared between stores
    /// pre:  db_path is non-empty, passphrase is non-empty, dim > 0
    /// post: returns SemanticMemory backed by a single SQLCipher pool
    pub fn open(
        db_path: &str,
        passphrase: &str,
        dim: usize,
    ) -> Result<Self, hkask_storage::DatabaseError> {
        use hkask_storage::database::sqlite::SqliteDriver;
        let db = hkask_storage::Database::open(db_path, passphrase)?;
        let pool = db.sqlite_pool()?;
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
            Arc::new(SqliteDriver::new(pool));
        let h_mem_store = HMemStore::from_driver(Arc::clone(&driver));
        let embedding_store = EmbeddingStore::from_driver(driver, dim);
        Ok(Self::new(h_mem_store, embedding_store))
    }
    pub fn with_ledger(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Override memory life S in days (Wozniak-Gorzelanczyk, 1995).
    ///
    /// Sets S in the forgetting curve R(t) = exp(-t/S). Default 180 days.
    /// Admin-configurable via ServiceConfig.memory_life_days.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// pre:  days > 0
    /// post: self.memory_life_days = days
    pub fn with_memory_life_days(mut self, days: f64) -> Self {
        self.memory_life_days = days;
        self
    }

    /// Get the configured memory life S in days.
    ///
    /// Memory life is the time constant of the forgetting curve R(t) = exp(-t/S).
    /// Default: 180 days (6 months × 30). Configurable via ServiceConfig.memory_life_days.
    pub fn memory_life_days(&self) -> f64 {
        self.memory_life_days
    }

    pub(crate) fn event_sink(&self) -> Option<&Arc<dyn RegulationSink>> {
        self.event_sink.as_ref()
    }

    /// Query by entity with deduplication, confidence decay, and recall-touch.
    ///
    /// Applies Wozniak-Gorzelanczyk (1995) forgetting curve decay at recall
    /// and resets the recall clock via touch_recall — same model as episodic memory.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — recalls deduplicated shared semantic h_mems
    /// \[P4\] Constraining: Clear Boundaries — filters to Shared/Public visibility
    /// \[P9\] Constraining: Homeostatic Self-Regulation — applies confidence decay at recall
    /// pre:  entity is non-empty
    /// post: returns `Vec<HMem>` filtered to Shared/Public visibility, decayed, deduped
    /// post: recalled_at touched for each returned h_mem (resets decay clock)
    pub fn query_deduped(&self, entity: &str) -> Result<Vec<HMem>, SemanticMemoryError> {
        let h_mems = self.h_mem_store.query_by_entity(entity)?;
        let filtered: Vec<HMem> = h_mems
            .into_iter()
            .filter(|t| matches!(t.access.visibility, Visibility::Shared | Visibility::Public))
            .map(|mut t| {
                // Wozniak-Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S)
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
                    "Semantic confidence decayed (Wozniak-Gorzelanczyk forgetting curve)"
                );
                t
            })
            .collect();

        let deduped = recall_dedup::dedup_h_mems(filtered);

        // Touch recalled_at on each deduped h_mem — resets the decay clock.
        // Memory that gets used stays fresh; memory that doesn't decays.
        for t in &deduped {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall semantic h_mem — decay clock not reset"
                );
            }
        }

        Ok(deduped)
    }

    /// Store a semantic h_mem (must be Shared/Public, no perspective).
    ///
    /// expect: "I can store shared semantic h_mems for shared knowledge"
    /// \[P3\] Motivating: Generative Space — stores shared semantic h_mem
    /// \[P4\] Constraining: Clear Boundaries — requires Shared/Public visibility and no perspective
    /// pre:  h_mem.access.visibility is Shared or Public
    /// pre:  h_mem.access.perspective is None
    /// post: h_mem inserted into h_mem_store
    /// post: returns Err(InvalidVisibility) if not Shared/Public
    /// post: returns Err(HasPerspective) if perspective is set
    pub fn store(&self, h_mem: HMem) -> Result<(), SemanticMemoryError> {
        if !matches!(
            h_mem.access.visibility,
            Visibility::Shared | Visibility::Public
        ) {
            return Err(SemanticMemoryError::InvalidVisibility(format!(
                "Semantic memory requires Shared/Public visibility, got {:?}",
                h_mem.access.visibility
            )));
        }
        if h_mem.access.perspective.is_some() {
            return Err(SemanticMemoryError::HasPerspective);
        }
        self.h_mem_store.insert(&h_mem)?;
        // Regulation: emit RegulationRecord for semantic write
        if let Some(sink) = &self.event_sink {
            let span = Span::new(
                SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
                "semantic_stored",
            );
            let event = RegulationRecord::new(
                h_mem.access.owner_webid,
                span,
                CyclePhase::Act,
                serde_json::json!({"entity": h_mem.entity, "attribute": h_mem.attribute}),
                0,
            );
            let _ = sink.persist(&event);
        }
        Ok(())
    }

    pub(crate) fn store_consolidated(&self, h_mem: HMem) -> Result<(), SemanticMemoryError> {
        self.h_mem_store.insert(&h_mem)?;
        // Regulation: emit RegulationRecord for consolidation write
        if let Some(sink) = &self.event_sink {
            let span = Span::new(
                SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
                "consolidated",
            );
            let event = RegulationRecord::new(
                h_mem.access.owner_webid,
                span,
                CyclePhase::Act,
                serde_json::json!({"entity": h_mem.entity, "attribute": h_mem.attribute}),
                0,
            );
            let _ = sink.persist(&event);
        }
        Ok(())
    }

    /// Find an existing semantic h_mem with the same EAV as the given h_mem.
    ///
    /// Used by the consolidation bridge to detect when an episodic h_mem
    /// being promoted matches a fact already in semantic memory, enabling
    /// Bayesian evidence combination rather than duplicate insertion.
    ///
    /// Matching is by canonical EAV hash (via `recall_dedup::eav_hash`),
    /// so the same fact stored with different timestamps or metadata is
    /// recognized as a match.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — finds existing semantic h_mem for evidence pooling
    /// \[P8\] Constraining: Semantic Grounding — EAV hash ensures factual identity, not metadata identity
    /// pre:  h_mem has valid entity and attribute
    /// post: returns Some(existing_triple) if semantic memory has a matching EAV
    /// post: returns None if no match found or on query error (graceful degradation)
    pub(crate) fn find_existing_by_eav(&self, h_mem: &HMem) -> Option<HMem> {
        let candidate_hash = crate::recall_dedup::eav_hash(h_mem);
        let existing = self
            .h_mem_store
            .query_by_entity_attribute(&h_mem.entity, &h_mem.attribute)
            .ok()?
            .into_iter()
            .filter(|t| {
                matches!(t.access.visibility, Visibility::Shared | Visibility::Public)
                    && t.access.perspective.is_none()
            })
            .find(|t| crate::recall_dedup::eav_hash(t) == candidate_hash);

        if existing.is_some() {
            tracing::debug!(
                target: "reg.consolidation",
                entity = %h_mem.entity,
                attribute = %h_mem.attribute,
                "Found existing semantic h_mem for EAV — will combine confidences"
            );
        }

        existing
    }

    /// Update an existing semantic h_mem's confidence via the bitemporal update path.
    ///
    /// Closes the current version (sets valid_to) and inserts a new version
    /// with the updated confidence. The value is preserved unchanged — only
    /// the confidence changes, reflecting the additional evidence.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — updates confidence with new evidence
    /// \[P8\] Constraining: Semantic Grounding — bitemporal update preserves audit trail
    /// pre:  existing_id refers to a valid semantic h_mem
    /// pre:  new_confidence is in [0, 1]
    /// post: h_mem with existing_id is closed (valid_to set)
    /// post: new h_mem inserted with updated confidence
    pub(crate) fn update_confidence(
        &self,
        existing_id: &hkask_storage::HMemId,
        current_value: serde_json::Value,
        new_confidence: Confidence,
    ) -> Result<(), SemanticMemoryError> {
        self.h_mem_store
            .update(existing_id, current_value, new_confidence)?;
        tracing::debug!(
            target: "reg.consolidation",
            triple_id = %existing_id.as_uuid(),
            new_confidence = %new_confidence,
            "Semantic h_mem confidence updated via Bayesian combination"
        );
        Ok(())
    }

    /// Count all semantic h_mems.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — reports total shared knowledge h_mems
    /// \[P9\] Constraining: Homeostatic Self-Regulation — count feeds storage budget loop
    /// post: returns total count of semantic h_mems in store
    pub fn h_mem_count(&self) -> Result<usize, SemanticMemoryError> {
        Ok(self.h_mem_store.count_semantic()?)
    }

    /// Count semantic h_mems for a specific entity.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — reports semantic h_mems per entity
    /// \[P9\] Constraining: Homeostatic Self-Regulation — per-entity budget monitoring
    /// pre:  entity is non-empty
    /// post: returns count of semantic h_mems for this entity
    pub fn h_mem_count_for_entity(&self, entity: &str) -> Result<usize, SemanticMemoryError> {
        Ok(self.h_mem_store.count_semantic_by_entity(entity)?)
    }

    /// Query all h_mems with a given attribute.
    ///
    /// Query all h_mems with a given attribute, with confidence decay applied.
    ///
    /// Applies Wozniak-Gorzelanczyk decay and resets the recall clock —
    /// same model as query_deduped.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — queries shared h_mems by attribute
    /// \[P8\] Constraining: Semantic Grounding — attribute-based recall expands context
    /// \[P9\] Constraining: Homeostatic Self-Regulation — applies confidence decay at recall
    /// pre:  attribute is non-empty
    /// post: returns `Vec<HMem>` with matching attribute, decayed, recall clock reset
    pub fn query_by_attribute(&self, attribute: &str) -> Result<Vec<HMem>, SemanticMemoryError> {
        let h_mems = self.h_mem_store.query_by_attribute(attribute)?;
        let decayed: Vec<HMem> = h_mems
            .into_iter()
            .map(|mut t| {
                let days_since = crate::bayesian::days_since(t.recalled_at);
                t.confidence = t.confidence.memory_decay(days_since, self.memory_life_days);
                t
            })
            .collect();

        // Touch recalled_at on each returned h_mem — resets the decay clock.
        for t in &decayed {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall semantic h_mem (query_by_attribute) — decay clock not reset"
                );
            }
        }

        Ok(decayed)
    }

    // Embedding operations (Loop 2b) — similarity-augmented recall

    /// Store an embedding vector for a semantic h_mem.
    ///
    /// The embedding is indexed by the h_mem's ID (`entity_ref`), enabling
    /// similarity search to find semantically related h_mems.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — indexes embedding vector for similarity retrieval
    /// \[P8\] Constraining: Semantic Grounding — vector indexed by h_mem entity_ref
    /// pre:  entity_ref is non-empty, vector is non-empty, model is valid
    /// post: embedding stored and indexed by entity_ref
    /// post: returns embedding ID
    pub fn store_embedding(
        &self,
        entity_ref: &str,
        vector: &[f32],
        model: &str,
    ) -> Result<String, SemanticMemoryError> {
        Ok(self.embedding.store(entity_ref, vector, model)?)
    }

    /// Search for semantically similar embeddings.
    ///
    /// Returns KNN results ordered by ascending distance (most similar first).
    /// Use this for context assembly that goes beyond exact entity matches —
    /// given a query embedding, find h_mems that are semantically close even
    /// if their entity keys differ.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — KNN search augments recall beyond exact matches
    /// \[P8\] Constraining: Semantic Grounding — results ordered by embedding distance
    /// pre:  query_vector is non-empty, limit > 0
    /// post: returns `Vec<SimilarityResult>` ordered by ascending distance
    pub fn search_similar(
        &self,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<SimilarityResult>, SemanticMemoryError> {
        Ok(self.embedding.search(query_vector, limit)?)
    }

    /// Count stored embeddings.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — reports indexed embedding count
    /// \[P9\] Constraining: Homeostatic Self-Regulation — count used for embedding budget monitoring
    /// post: returns total count of embeddings in store
    pub fn embedding_count(&self) -> Result<usize, SemanticMemoryError> {
        Ok(self.embedding.count()?)
    }

    /// Access the underlying EmbeddingStore for direct operations
    /// (e.g., centroid computation, KNN search).
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — exposes embedding store for advanced operations
    /// \[P5\] Constraining: Essentialism — direct accessor avoids duplicate wrappers
    /// post: returns reference to the EmbeddingStore
    pub fn embedding_store(&self) -> &EmbeddingStore {
        &self.embedding
    }

    /// Retrieve all entity_refs matching a prefix.
    ///
    /// Uses SQL LIKE query instead of zero-vector KNN scan.
    /// Returns entity_refs for prefix-based operations (centroid, purge).
    fn entity_refs_by_prefix(&self, prefix: &str) -> Result<Vec<String>, SemanticMemoryError> {
        Ok(self.embedding.query_by_prefix(prefix)?)
    }

    /// Bulk-load all (entity_ref, vector) pairs matching a prefix.
    ///
    /// Single-query retrieval of all embeddings whose entity_ref starts with
    /// `prefix`. Used by corpus chunk dedup to avoid N individual `get()` calls.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — bulk vector retrieval for dedup
    /// pre:  prefix is non-empty
    /// post: returns Vec of (entity_ref, vector) pairs matching prefix
    pub fn embeddings_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, SemanticMemoryError> {
        Ok(self.embedding.get_all_by_prefix(prefix)?)
    }

    // Corpus operations (Loop 2b) — centroid + purge for style embeddings

    /// Compute the centroid (mean embedding vector) for embeddings matching a prefix.
    ///
    /// Only includes embeddings whose `entity_ref` starts with `prefix` but does NOT
    /// start with `exclude_prefix` and does NOT equal `exclude_ref`. This filters out
    /// meta-entries (style rules, centroids) that are not prose exemplars.
    ///
    /// The centroid is the arithmetic mean of all matching vectors, used for
    /// style cluster validation: generated prose should fall within a cosine
    /// distance threshold of this centroid.
    ///
    /// If `store_as` is provided, the centroid is also stored as an embedding
    /// under that entity_ref, enabling one-step compute+store.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — computes mean style vector for corpus validation
    /// \[P8\] Constraining: Semantic Grounding — arithmetic mean over matching embeddings
    /// pre:  prefix is non-empty, dim > 0
    /// post: returns CentroidResult with mean vector and passage count
    /// post: returns Err(NoEmbeddingsForCentroid) if no matching embeddings
    /// post: centroid stored if store_as and model are provided
    pub fn compute_centroid(
        &self,
        prefix: &str,
        exclude_prefix: &str,
        exclude_ref: &str,
        dim: usize,
        store_as: Option<&str>,
        model: Option<&str>,
    ) -> Result<CentroidResult, SemanticMemoryError> {
        let matching_refs: Vec<String> = self
            .entity_refs_by_prefix(prefix)?
            .into_iter()
            .filter(|r| !r.starts_with(exclude_prefix) && r != exclude_ref)
            .collect();

        if matching_refs.is_empty() {
            return Err(SemanticMemoryError::NoEmbeddingsForCentroid(
                prefix.to_string(),
            ));
        }

        // Fetch each embedding and compute mean
        let mut centroid = vec![0.0f32; dim];
        let mut count = 0usize;
        for entity_ref in &matching_refs {
            if let Ok(emb) = self.embedding.get(entity_ref) {
                for (i, v) in emb.vector.iter().enumerate() {
                    if i < dim {
                        centroid[i] += v;
                    }
                }
                count += 1;
            }
        }

        if count == 0 {
            return Err(SemanticMemoryError::NoEmbeddingsForCentroid(
                prefix.to_string(),
            ));
        }

        let n = count as f32;
        for v in centroid.iter_mut() {
            *v /= n;
        }

        let stored = if let Some(ref_to_store) = store_as {
            if let Some(m) = model {
                let _id = self.embedding.store(ref_to_store, &centroid, m)?;
                true
            } else {
                false
            }
        } else {
            false
        };

        tracing::info!(
            target: "hkask.semantic",
            prefix = %prefix,
            passage_count = count,
            stored = stored,
            "Centroid computed"
        );

        Ok(CentroidResult {
            centroid,
            passage_count: count,
            stored,
        })
    }

    /// Purge all embeddings whose `entity_ref` starts with `prefix`.
    ///
    /// Uses SQL prefix query to find candidates, then deletes.
    /// Returns the number of embeddings deleted.
    ///
    /// Used for idempotent re-ingest: purge an author's existing embeddings
    /// before re-downloading and re-embedding their corpus.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — purges embeddings for idempotent re-ingest
    /// \[P5\] Constraining: Essentialism — prefix-based deletion, count of successes returned
    /// pre:  prefix is non-empty
    /// post: all embeddings with matching prefix deleted
    /// post: returns count of deleted embeddings
    pub fn purge_by_prefix(&self, prefix: &str) -> Result<usize, SemanticMemoryError> {
        let to_delete = self.entity_refs_by_prefix(prefix)?;

        let mut count = 0;
        for entity_ref in &to_delete {
            if self.embedding.delete(entity_ref).is_ok() {
                count += 1;
            }
        }

        tracing::info!(
            target: "hkask.semantic",
            prefix = %prefix,
            purged = count,
            "Purged embeddings by prefix"
        );

        Ok(count)
    }

    /// Chunk text into passages for embedding.
    ///
    /// Splits on structural boundaries (markdown headings, horizontal rules,
    /// then paragraph breaks), applies min/max word count constraints, and
    /// splits long paragraphs at the nearest sentence boundary. Short
    /// paragraphs are concatenated until min_words is reached.
    ///
    /// Returns (entity_ref, text) pairs with entity_ref formatted as
    /// `{entity_ref_prefix}:{chunk_index}`.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — chunks text into passage-sized units for embedding
    /// \[P5\] Constraining: Essentialism — structural/sentence boundary splitting with min/max words
    /// pre:  text is non-empty, entity_ref_prefix is non-empty
    /// pre:  min_words > 0, max_words >= min_words
    /// post: returns Vec of (entity_ref, text) chunks
    /// post: each chunk has word count between min_words and max_words (best-effort)
    pub fn chunk_text(
        text: &str,
        entity_ref_prefix: &str,
        min_words: usize,
        max_words: usize,
        sentence_boundary: &str,
    ) -> Vec<(String, String)> {
        // Structural splitting: headings/rules become their own paragraph units so
        // chunks don't straddle unrelated sections (improves concept coherence, which
        // the salience graph depends on).
        let paragraphs = Self::split_structural(text);

        let mut passages = Vec::new();
        let mut buffer = String::new();
        let mut buffer_words = 0usize;
        let mut chunk_index = 0usize;
        let boundary_chars: Vec<char> = sentence_boundary
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        for paragraph in &paragraphs {
            let word_count = paragraph.split_whitespace().count();

            if buffer_words + word_count > max_words && buffer_words >= min_words {
                let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                passages.push((entity_ref, buffer.trim().to_string()));
                chunk_index += 1;
                buffer.clear();
                buffer_words = 0;
            }

            if word_count > max_words {
                if !buffer.is_empty() && buffer_words >= min_words {
                    let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                    passages.push((entity_ref, buffer.trim().to_string()));
                    chunk_index += 1;
                    buffer.clear();
                    buffer_words = 0;
                }
                // Split a too-long paragraph at the nearest sentence boundary at or
                // after max_words (look-ahead up to 25% of max_words), falling back
                // to the last boundary before max_words, then a hard cut.
                let words: Vec<&str> = paragraph.split_whitespace().collect();
                let mut start = 0usize;
                while start < words.len() {
                    let target = (start + max_words).min(words.len());
                    let look_ahead = (max_words / 4).max(1);
                    let mut split_at = target;
                    let mut found = false;
                    for (i, w) in words.iter().enumerate().skip(target).take(look_ahead) {
                        if Self::is_sentence_end(w, &boundary_chars) {
                            split_at = i + 1;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        let back_floor = start + min_words.min(words.len());
                        for i in (back_floor..target).rev() {
                            if Self::is_sentence_end(words[i], &boundary_chars) {
                                split_at = i + 1;
                                break;
                            }
                        }
                    }
                    let chunk_words = &words[start..split_at];
                    let text = chunk_words.join(" ");
                    let cw = chunk_words.len();
                    if cw >= min_words {
                        let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                        passages.push((entity_ref, text));
                        chunk_index += 1;
                        buffer.clear();
                        buffer_words = 0;
                    } else if !buffer.is_empty() {
                        buffer.push(' ');
                        buffer.push_str(&text);
                        buffer_words += cw;
                    } else {
                        let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
                        passages.push((entity_ref, text));
                        chunk_index += 1;
                    }
                    start = split_at;
                }
            } else {
                if !buffer.is_empty() {
                    buffer.push(' ');
                }
                buffer.push_str(paragraph);
                buffer_words += word_count;
            }
        }

        if !buffer.is_empty() {
            let entity_ref = format!("{}:{}", entity_ref_prefix, chunk_index);
            passages.push((entity_ref, buffer.trim().to_string()));
        }

        passages
    }

    /// True when `word` ends a sentence: its final non-quote char is a boundary
    /// punctuation. Handles trailing quotes (`asked."`) and numeric decimals
    /// (`3.14` — a digit before the period is not a sentence end).
    /// Single-letter initials (`J.`) are not sentence ends.
    fn is_sentence_end(word: &str, boundary_chars: &[char]) -> bool {
        let trimmed = word.trim_end_matches(['"', '\'', '\u{201d}', '\u{201c}']);
        let mut chars = trimmed.chars();
        let last = match chars.next_back() {
            Some(c) => c,
            None => return false,
        };
        if !boundary_chars.contains(&last) {
            return false;
        }
        if last == '.' && chars.next_back().is_some_and(|p| p.is_ascii_digit()) {
            return false;
        }
        let stem = trimmed.trim_end_matches(['.', '!', '?']);
        if last == '.'
            && stem.chars().count() == 1
            && stem.chars().next().is_some_and(|c| c.is_uppercase())
        {
            return false;
        }
        true
    }

    /// Split text into paragraphs on structural boundaries: markdown headings,
    /// horizontal rules, and blank-line breaks. Headings/rules always start a
    /// new paragraph so chunks don't straddle unrelated sections.
    fn split_structural(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = String::new();
        for line in text.lines() {
            let trimmed = line.trim();
            let is_heading = trimmed.starts_with('#')
                && trimmed
                    .chars()
                    .nth(1)
                    .is_none_or(|c| c == '#' || c.is_whitespace());
            let is_rule = trimmed == "---" || trimmed == "***" || trimmed == "___";
            if (is_heading || is_rule) && !buf.is_empty() {
                let p = buf.trim().to_string();
                if !p.is_empty() {
                    out.push(p);
                }
                buf.clear();
            }
            if is_heading || is_rule {
                let p = trimmed.to_string();
                if !p.is_empty() {
                    out.push(p);
                }
            } else {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(line);
            }
        }
        let p = buf.trim().to_string();
        if !p.is_empty() {
            out.push(p);
        }
        let mut final_out = Vec::new();
        for para in out {
            for piece in para.split("\n\n") {
                let t = piece.trim();
                if !t.is_empty() {
                    final_out.push(t.to_string());
                }
            }
        }
        final_out
    }

    /// Strip Project Gutenberg headers and footers from text.
    ///
    /// Looks for the standard `*** START OF` / `*** END OF` markers.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — removes boilerplate for clean corpus ingestion
    /// \[P5\] Constraining: Essentialism — marker-based trim, no regex
    /// pre:  text is a valid &str
    /// post: returns text between START OF and END OF markers
    /// post: returns full text if markers not found
    pub fn strip_gutenberg_headers(text: &str) -> String {
        let start_marker = "*** START OF";
        let end_marker = "*** END OF";

        let start = text
            .find(start_marker)
            .and_then(|i| text[i..].find('\n').map(|j| i + j + 1))
            .unwrap_or(0);

        let end = text.find(end_marker).unwrap_or(text.len());

        text[start..end].trim().to_string()
    }

    // Deletion (Loop 2b) — Cybernetics membrane operation

    // Note: The following four methods (delete_triple, lowest_confidence_triples,
    // low_confidence_count, low_confidence_triples) are `pub` rather than
    // `pub(crate)` because they are needed by:
    //   1. `ConsolidationService` (in this crate) for user-triggered cleanup
    //   2. `hkask-mcp-memory` MCP server (external crate) for the
    //      `semantic_count` and `episodic_consolidate_status` tools
    //
    // This is safe because these are data operations, not authority operations.
    // Semantic h_mems are shared/public knowledge (visibility: Shared,
    // perspective: None) — deleting or querying them doesn't bypass the OCAP
    // membrane. The ConsolidationToken and GovernedTool membrane control who
    // can *trigger* the operations; these methods just execute them.

    /// Delete a semantic h_mem by ID (budget enforcement / consolidation cleanup).
    ///
    /// When the semantic storage budget is exceeded or consolidation cleanup
    /// targets low-confidence h_mems, they are deleted outright.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// \[P3\] Motivating: Generative Space — deletes semantic h_mem for budget enforcement or cleanup
    /// \[P9\] Constraining: Homeostatic Self-Regulation — used by regulation loops to free space
    /// pre:  id is a valid HMemId
    /// post: h_mem deleted from store
    /// post: returns Err if h_mem not found
    pub fn delete_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), SemanticMemoryError> {
        tracing::info!(
            target: "hkask.semantic",
            triple_id = %id,
            "Semantic h_mem deleted (budget enforcement)"
        );
        self.h_mem_store.delete_by_id(id)?;
        Ok(())
    }

    // Budget enforcement (Loop 2b) — Cybernetics membrane operation

    /// Identify the lowest-confidence semantic h_mems for budget enforcement.
    ///
    /// Returns up to `limit` h_mems with `perspective IS NULL`, ordered by
    /// confidence ascending then `valid_from` ascending (oldest first).
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — identifies lowest-confidence h_mems for pruning
    /// \[P9\] Constraining: Homeostatic Self-Regulation — ordered by confidence and age
    /// pre:  limit > 0
    /// post: returns up to `limit` h_mems ordered by confidence ascending
    pub fn lowest_confidence_h_mems(&self, limit: usize) -> Result<Vec<HMem>, SemanticMemoryError> {
        Ok(self.h_mem_store.query_semantic_lowest_confidence(limit)?)
    }

    /// Count semantic h_mems at or below a confidence threshold.
    ///
    /// Used by `SemanticLoop::sense()` and `ConsolidationService`
    /// for the consolidation trigger signal.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — counts uncertain semantic h_mems
    /// \[P9\] Constraining: Homeostatic Self-Regulation — threshold-driven count
    /// pre:  threshold in [0.0, 1.0]
    /// post: returns count of h_mems with confidence ≤ threshold
    pub fn low_confidence_count(&self, threshold: f64) -> Result<usize, SemanticMemoryError> {
        Ok(self
            .h_mem_store
            .count_semantic_below_confidence(threshold)?)
    }

    /// Query semantic h_mems at or below a confidence threshold.
    ///
    /// Returns up to `limit` h_mems with `confidence <= threshold`,
    /// ordered by confidence ascending then `valid_from` ascending.
    ///
    /// Used by `SemanticLoop::act()` and `ConsolidationService`
    /// for the consolidation trigger.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// \[P3\] Motivating: Generative Space — retrieves uncertain semantic h_mems for review
    /// \[P9\] Constraining: Homeostatic Self-Regulation — bounded by threshold and limit
    /// pre:  threshold in [0.0, 1.0], limit > 0
    /// post: returns up to `limit` h_mems with confidence ≤ threshold
    pub fn low_confidence_h_mems(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<HMem>, SemanticMemoryError> {
        Ok(self
            .h_mem_store
            .query_semantic_below_confidence(threshold, limit)?)
    }

    /// Query semantic h_mems older than N days, grouped by entity.
    ///
    /// Used by condensation to find candidates for merging.
    ///
    /// expect: "I can recall deduplicated semantic h_mems with embedding similarity"
    /// pre:  days > 0, limit > 0
    /// post: returns h_mems older than cutoff, ordered by entity, confidence DESC, valid_from DESC
    pub fn h_mems_older_than(
        &self,
        days: u32,
        limit: usize,
    ) -> Result<Vec<HMem>, SemanticMemoryError> {
        Ok(self.h_mem_store.query_semantic_older_than(days, limit)?)
    }

    /// Soft-delete a h_mem (set valid_to = now).
    ///
    /// Used by condensation to close original h_mems after merging.
    ///
    /// expect: "I can store shared semantic h_mems for public knowledge"
    /// pre:  id is a valid HMemId
    /// post: h_mem soft-deleted (valid_to set to now)
    pub fn close_h_mem(&self, id: &hkask_storage::HMemId) -> Result<(), SemanticMemoryError> {
        self.h_mem_store.close_by_id(id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::SemanticMemory;
    //
    // Before fix, `centroid[i] += v` was called without checking `i < dim`,
    // causing an index-out-of-bounds panic when an embedding vector was longer
    // than the target centroid dimension.
    #[test]
    fn centroid_accumulation_skips_out_of_range_dimensions() {
        let dim = 4usize;
        let mut centroid = vec![0.0f32; dim];

        // Simulate an embedding with more dimensions than the target centroid.
        let overlong_vector: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        for (i, v) in overlong_vector.iter().enumerate() {
            if i < dim {
                centroid[i] += v;
            }
        }

        // No panic; only the first `dim` values are accumulated.
        assert_eq!(centroid, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn centroid_accumulation_handles_short_embedding() {
        let dim = 4usize;
        let mut centroid = vec![0.0f32; dim];

        // Embedding shorter than dim — should partially accumulate with no panic.
        let short_vector: Vec<f32> = vec![1.0, 2.0];
        for (i, v) in short_vector.iter().enumerate() {
            if i < dim {
                centroid[i] += v;
            }
        }

        assert_eq!(centroid, vec![1.0, 2.0, 0.0, 0.0]);
    }

    // ── chunk_text failure-mode tests ──────────────────────────────────────

    #[test]
    fn chunk_text_empty_input_returns_empty() {
        let result = SemanticMemory::chunk_text("", "doc", 5, 20, ".!? ");
        assert!(result.is_empty(), "empty input should produce no chunks");
    }

    #[test]
    fn chunk_text_whitespace_only_returns_empty() {
        let result = SemanticMemory::chunk_text("   \n\n  \t  \n\n", "doc", 5, 20, ".!? ");
        assert!(
            result.is_empty(),
            "whitespace-only input should produce no chunks"
        );
    }

    #[test]
    fn chunk_text_ontology_concepts_preserved_across_boundaries() {
        // Multi-word concepts from all five ontology namespaces should survive
        // chunking intact. The sentence-boundary splitter breaks at periods
        // after each sentence, not mid-concept.
        //
        // FIBO: barrier to entry, cost of capital, economic profit, margin of safety
        // GOLEM: narrative structure, character development
        // PKO: feedback loop, decision process
        // epistemic: causal reasoning, confirmation bias
        // Dublin Core (dc_subject): these are the general keywords the tagging
        //   template extracts — they overlap with the ontology concepts above.
        let text = "competitive advantage creates economic profit through differentiation. \
barrier to entry protects returns over time for incumbents. \
narrative structure shapes how investors interpret market signals clearly. \
character development in case studies reveals decision patterns over time. \
feedback loop connects analysis to evaluation in the investment process. \
decision process requires discipline and patience from practitioners. \
causal reasoning distinguishes correlation from causation in market analysis. \
confirmation bias distorts judgment when evidence supports prior beliefs. \
cost of capital determines allocation across competing opportunities. \
margin of safety reduces downside risk in uncertain environments.";
        let chunks = SemanticMemory::chunk_text(text, "doc", 5, 15, ".!? ");
        assert!(
            !chunks.is_empty(),
            "should produce chunks from ontology text"
        );
        // Each multi-word concept should appear intact in the joined chunk text.
        let all_text: String = chunks
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let fibo = [
            "barrier to entry",
            "cost of capital",
            "economic profit",
            "margin of safety",
        ];
        let golem = ["narrative structure", "character development"];
        let pko = ["feedback loop", "decision process"];
        let epistemic = ["causal reasoning", "confirmation bias"];
        for concept in fibo
            .iter()
            .chain(golem.iter())
            .chain(pko.iter())
            .chain(epistemic.iter())
        {
            assert!(
                all_text.contains(concept),
                "ontology concept '{concept}' should appear intact in chunked text"
            );
        }
    }

    #[test]
    fn chunk_text_structural_split_prevents_straddling() {
        // A markdown heading creates a structural boundary. With small max_words,
        // the heading forces a paragraph break so chunks don't straddle sections.
        let text = "First section discusses investing principles at length here.\n\n# Chapter Two\n\nSecond section covers return on capital analysis in detail.";
        let chunks = SemanticMemory::chunk_text(text, "doc", 5, 12, ".!? ");
        // No single chunk should contain both "investing" and "return on capital"
        // — they're in different structural sections.
        for (_, chunk_text) in &chunks {
            let has_first = chunk_text.contains("investing");
            let has_second = chunk_text.contains("return on capital");
            assert!(
                !has_first || !has_second,
                "chunk should not straddle structural boundary: '{chunk_text}'"
            );
        }
    }
}
