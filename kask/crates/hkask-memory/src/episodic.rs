//! Episodic memory pipeline — first-person experience
//!
//! Subloops (Loop 2a):
//! - 2a.1 Experience Encoding (FILTER) — filter and classify incoming experience
//! - 2a.2 Temporal Attention (ADAPT) — weight by recency: weight = e^(-λ × time_since_storage)
//! - 2a.3 Confidence Decay (RECONCILE) — confidence decreases over time via Bayesian decay
//! - 2a.4 Episodic Storage Budget (GUARD) — per-agent storage limit, mark oldest for consolidation
//! - 2a.5 Episodic Context Assembly (FILTER+ADAPT) — temporal-ordered, recency-weighted, budget-constrained

use crate::recall_dedup;
use hkask_storage::{HMem, HMemError, HMemStore};
use hkask_types::RegulationSink;
use hkask_types::Visibility;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EpisodicMemoryError {
    #[error("HMem error: {0}")]
    HMem(#[from] HMemError),
    #[error("Invalid visibility for episodic store: {0}")]
    InvalidVisibility(String),
    #[error("Episodic memory requires a perspective (agent WebID)")]
    MissingPerspective,
}

/// Default per-agent storage budget (max h_mems).
pub(crate) const DEFAULT_EPISODIC_BUDGET: usize = 10_000;

/// Default memory life in days: 180 days (6 months × 30).
///
/// Wozniak & Gorzelanczyk (1995), equation (3): R(t) = exp(-t/S).
/// S is memory life in days — configurable by admin. After S days
/// without recall, confidence decays to exp(-1) ≈ 36.8%.
pub(crate) const DEFAULT_MEMORY_LIFE_DAYS: f64 = crate::bayesian::DEFAULT_MEMORY_LIFE_DAYS;

// EpisodicMemory — first-person experience with subloops

/// Episodic memory — first-person experience
///
/// Provides the following subloops:
/// - **Confidence decay** (2a.3): Decays confidence using the Wozniak-Gorzelanczyk
///   (1995) human forgetting curve: R(t) = exp(-t/S) where S is memory life in days.
/// - **Temporal attention** (2a.2): Weights recalled h_mems by recency.
/// - **Storage budget** (2a.4): Per-agent storage limit with consolidation
///   candidate identification (uses decayed confidence for prioritization).
pub struct EpisodicMemory {
    event_sink: Option<Arc<dyn RegulationSink>>,
    h_mem_store: HMemStore,
    /// Memory life S in days — configurable, default 180 (6 months × 30).
    /// The forgetting curve is R(t) = exp(-t/S) where t is days since recall.
    memory_life_days: f64,
    /// Per-agent storage budget (max h_mems). Default: 10,000
    storage_budget: usize,
}

impl EpisodicMemory {
    /// Create a new EpisodicMemory with default memory life and storage budget.
    ///
    /// expect: "I can store first-person experience h_mems in my sovereign episodic memory"
    /// \[P3\] Motivating: Generative Space — creates a sovereign first-person experience store
    /// \[P9\] Constraining: Homeostatic Self-Regulation — default memory life and budget are regulation defaults
    /// pre:  h_mem_store is initialized
    /// post: returns EpisodicMemory with DEFAULT_MEMORY_LIFE_DAYS and DEFAULT_EPISODIC_BUDGET
    pub fn new(h_mem_store: HMemStore) -> Self {
        Self {
            h_mem_store,
            memory_life_days: DEFAULT_MEMORY_LIFE_DAYS,
            storage_budget: DEFAULT_EPISODIC_BUDGET,
            event_sink: None,
        }
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
    /// expect: "I can store first-person experience h_mems in my sovereign episodic memory"
    /// pre:  days > 0
    /// post: self.memory_life_days = days
    pub fn with_memory_life_days(mut self, days: f64) -> Self {
        self.memory_life_days = days;
        self
    }

    /// Access the Regulation event sink for loop-level RegulationRecord emission.
    pub(crate) fn event_sink(&self) -> Option<&Arc<dyn RegulationSink>> {
        self.event_sink.as_ref()
    }

    // Store

    /// Store an episodic h_mem (private by default, with perspective).
    ///
    /// expect: "I can store first-person experience h_mems in my sovereign episodic memory"
    /// \[P3\] Motivating: Generative Space — stores a first-person experience h_mem
    /// \[P1\] Constraining: User Sovereignty — rejects Shared/Public visibility (episodic is sovereign)
    /// \[P4\] Constraining: Clear Boundaries — requires perspective owner
    /// pre:  h_mem.access.visibility is Private (episodic is sovereign)
    /// pre:  h_mem.access.perspective is Some (must have owner)
    /// post: h_mem inserted into h_mem_store
    /// post: returns Err(InvalidVisibility) if visibility is Shared or Public
    /// post: returns Err(MissingPerspective) if perspective is None
    pub fn store(&self, h_mem: HMem) -> Result<(), EpisodicMemoryError> {
        if matches!(
            h_mem.access.visibility,
            Visibility::Shared | Visibility::Public
        ) {
            return Err(EpisodicMemoryError::InvalidVisibility(
                "Episodic memory is sovereign — shared/public h_mems belong in semantic memory"
                    .to_string(),
            ));
        }
        if h_mem.access.perspective.is_none() {
            return Err(EpisodicMemoryError::MissingPerspective);
        }
        self.h_mem_store.insert(&h_mem)?;
        // Regulation: emit RegulationRecord for memory write observability
        if let Some(sink) = &self.event_sink {
            let span = Span::new(
                SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
                "episodic_stored",
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

    // Recall — basic queries

    /// Query by entity for specific perspective with deduplication,
    /// confidence decay, and temporal attention applied (2a.3 + 2a.2).
    ///
    /// Decays confidence based on time since last recall (`recalled_at`) using
    /// `Confidence::decay()`, then deduplicates by EAV hash.
    /// `valid_from` is the creation timestamp — never modified.
    ///
    /// Emits `reg.memory.decay` span for each h_mem that undergoes decay.
    ///
    /// expect: "I can recall deduplicated episodic h_mems with confidence decay"
    /// \[P3\] Motivating: Generative Space — recalls deduplicated episodic h_mems for an entity
    /// \[P9\] Constraining: Homeostatic Self-Regulation — applies confidence decay and temporal attention at recall
    /// pre:  entity is non-empty, perspective is valid
    /// post: returns `Vec<HMem>` filtered by perspective, decayed, deduped, sorted by recency
    /// post: confidence decayed via e^(-λt) for each h_mem
    pub fn query_for_deduped(
        &self,
        entity: &str,
        perspective: WebID,
    ) -> Result<Vec<HMem>, EpisodicMemoryError> {
        let h_mems = self.h_mem_store.query_by_entity(entity)?;
        let mut filtered: Vec<HMem> = h_mems
            .into_iter()
            .filter(|t| t.access.perspective == Some(perspective))
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
                    "Episodic confidence decayed (Wozniak-Gorzelanczyk forgetting curve)"
                );
                t
            })
            .collect();

        // Sort by recency (most recent first) — temporal attention (2a.2)
        filtered.sort_by(|a, b| b.observed_at.cmp(&a.observed_at));

        let deduped = recall_dedup::dedup_h_mems(filtered);

        // Touch recalled_at on each deduped h_mem — resets the decay clock.
        // Memory that gets used stays fresh; memory that doesn't decays.
        for t in &deduped {
            if let Err(e) = self.h_mem_store.touch_recall(&t.id) {
                tracing::warn!(
                    target: "reg.memory.decay",
                    triple_id = %t.id,
                    error = %e,
                    "Failed to touch_recall episodic h_mem — decay clock not reset"
                );
            }
        }

        Ok(deduped)
    }

    // Query — all episodic memories

    // Storage Budget (2a.5) — Cybernetics membrane operations

    /// Get the current storage usage for a perspective (number of h_mems).
    ///
    /// Uses a COUNT query instead of loading all h_mems into memory.
    ///
    /// expect: "I can recall deduplicated episodic h_mems with confidence decay"
    /// \[P3\] Motivating: Generative Space — reports episodic storage usage per perspective
    /// \[P9\] Constraining: Homeostatic Self-Regulation — COUNT query avoids loading full store
    /// pre:  perspective is a valid WebID
    /// post: returns count of h_mems for this perspective
    pub fn storage_usage(&self, perspective: &WebID) -> Result<usize, EpisodicMemoryError> {
        let count = self.h_mem_store.count_by_perspective(perspective)?;
        Ok(count)
    }

    /// Identify h_mems eligible for consolidation (oldest, lowest effective confidence)
    /// when budget is exceeded (2a.4).
    ///
    /// Uses recall-time decayed confidence (not stored confidence) so that
    /// old h_mems with high stored confidence but low effective confidence
    /// are correctly prioritized for consolidation.
    ///
    /// **Membrane-sealed:** Only callable from within this crate.
    pub(crate) fn consolidation_candidates(
        &self,
        perspective: WebID,
        limit: usize,
    ) -> Result<Vec<HMem>, EpisodicMemoryError> {
        let mut h_mems = self.h_mem_store.query_by_perspective(&perspective)?;

        // Sort by decayed confidence ascending, then by valid_from ascending (oldest first)
        // Uses Wozniak-Gorzelanczyk (1995) forgetting curve: R(t) = exp(-t/S)
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

    /// Expire a h_mem by setting its `valid_to` timestamp (soft-delete).
    ///
    /// Used by consolidation to mark episodic h_mems as expired after
    /// they have been promoted to semantic memory. The h_mem remains in
    /// the store for audit but is excluded from all current queries.
    ///
    /// **Membrane-sealed:** Only callable from within this crate.
    pub(crate) fn expire_h_mem(
        &self,
        id: &hkask_storage::HMemId,
    ) -> Result<(), EpisodicMemoryError> {
        self.h_mem_store.close_by_id(id)?;
        tracing::debug!(
            target: "hkask.episodic",
            triple_id = %id.as_uuid(),
            "Episodic h_mem expired (consolidated to semantic memory)"
        );
        Ok(())
    }

    /// Get the configured storage budget.
    ///
    /// expect: "I can recall deduplicated episodic h_mems with confidence decay"
    /// \[P3\] Motivating: Generative Space — exposes the episodic storage set-point
    /// \[P9\] Constraining: Homeostatic Self-Regulation — budget bounds per-agent experience growth
    /// post: returns the storage_budget value set at construction
    pub fn storage_budget(&self) -> usize {
        self.storage_budget
    }

    /// Count consolidation candidates for a perspective.
    ///
    /// Returns the number of episodic h_mems eligible for consolidation
    /// (sorted by decayed confidence, oldest/lowest first). This is the
    /// count-only version of `consolidation_candidates` — safe to expose
    /// publicly because it doesn't return h_mem data.
    ///
    /// expect: "I can recall deduplicated episodic h_mems with confidence decay"
    /// \[P3\] Motivating: Generative Space — reports how many episodic h_mems are eligible for consolidation
    /// \[P9\] Constraining: Homeostatic Self-Regulation — uses decayed confidence for prioritization
    /// pre:  perspective is a valid WebID
    /// post: returns count of h_mems eligible for consolidation
    /// post: returns 0 on error (graceful degradation)
    pub fn consolidation_candidate_count(&self, perspective: &WebID) -> usize {
        match self.consolidation_candidates(*perspective, usize::MAX) {
            Ok(candidates) => candidates.len(),
            Err(_) => 0,
        }
    }

    /// Get the configured memory life S in days.
    ///
    /// Memory life is the time constant of the forgetting curve R(t) = exp(-t/S).
    /// Default: 180 days (6 months × 30). Configurable via ServiceConfig.memory_life_days.
    pub fn memory_life_days(&self) -> f64 {
        self.memory_life_days
    }
}
