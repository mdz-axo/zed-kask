//! Consolidation Bridge — Episodic → Semantic (one-way)
//!
//! When currency pressure triggers consolidation, episodic h_mems are:
//! 1. Selected via `EpisodicMemory::consolidation_candidates()` (oldest, lowest effective confidence)
//! 2. Stripped of perspective (privacy boundary removal)
//! 3. Checked against existing semantic h_mems with same EAV:
//!    a. **Match found:** Bayesian combine confidences, update existing
//!    b. **No match:** Seed as new semantic h_mem
//! 4. Expired in episodic memory (valid_to set, soft-deleted) to free storage budget
//!
//! This is a ONE-WAY operation: Episodic → Semantic. No reverse flow.

use std::sync::Arc;

use crate::bayesian::combine_confidences;
use crate::episodic::EpisodicMemory;
use crate::semantic::SemanticMemory;
use hkask_storage::{HMem, HMemId};
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

/// Consolidation Bridge — Episodic → Semantic
///
/// One-way operation called from `EpisodicLoop::act()` when budget pressure
/// requires freeing episodic storage.
pub struct ConsolidationBridge {
    episodic: Arc<EpisodicMemory>,
    semantic: Arc<SemanticMemory>,
}

impl ConsolidationBridge {
    pub fn new(episodic: Arc<EpisodicMemory>, semantic: Arc<SemanticMemory>) -> Self {
        Self { episodic, semantic }
    }

    /// Consolidate episodic h_mems into semantic memory (one-way).
    ///
    /// For each candidate:
    /// 1. Strip perspective (set to `None`) — removes privacy boundary
    /// 2. Check semantic memory for existing h_mem with same EAV hash:
    ///    a. **Match:** Bayesian combine episodic + semantic confidence,
    ///       update existing semantic h_mem
    ///    b. **No match:** Insert as new semantic h_mem
    /// 3. Expire episodic source (soft-delete via valid_to)
    #[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
    pub fn consolidate(
        &self,
        perspective: WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        let span = tracing::span!(target: "reg.consolidation", tracing::Level::INFO, "consolidate");
        let _enter = span.enter();

        let candidates = self
            .episodic
            .consolidation_candidates(perspective, request.limit)
            .map_err(|e| anyhow::anyhow!("Episodic error: {e}"))?;

        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            candidate_count = candidates.len(),
            limit = request.limit,
            "Starting consolidation"
        );

        let mut consolidated_count = 0usize;
        let mut combined_count = 0usize;
        let mut expired_count = 0usize;
        let mut failed_count = 0usize;

        let now = chrono::Utc::now();
        for h_mem in &candidates {
            let days_since = crate::bayesian::days_since(h_mem.recalled_at);
            let episodic_c = h_mem
                .confidence
                .memory_decay(days_since, self.episodic.memory_life_days());

            if let Some(existing) = self.semantic.find_existing_by_eav(h_mem) {
                // Decay semantic confidence to same temporal reference as episodic.
                // Both sides of the Bayesian combination must use decayed confidence
                // so independent evidence is combined at the same point in time.
                let semantic_days_since = crate::bayesian::days_since(existing.recalled_at);
                let semantic_c = existing
                    .confidence
                    .memory_decay(semantic_days_since, self.semantic.memory_life_days());
                let combined = combine_confidences(semantic_c, episodic_c);

                match self
                    .semantic
                    .update_confidence(&existing.id, h_mem.value.clone(), combined)
                {
                    Ok(()) => {
                        combined_count += 1;
                        consolidated_count += 1;
                        if let Err(e) = self.episodic.expire_h_mem(&h_mem.id) {
                            tracing::warn!(target: "reg.consolidation", triple_id = %h_mem.id.as_uuid(), error = %e, "Failed to expire episodic h_mem");
                        } else {
                            expired_count += 1;
                        }
                        tracing::debug!(
                            target: "reg.consolidation",
                            entity = %h_mem.entity, attribute = %h_mem.attribute,
                            stored = %h_mem.confidence, days_since_recall = days_since,
                            episodic = %episodic_c,
                            semantic_stored = %existing.confidence,
                            semantic_days = semantic_days_since,
                            semantic = %semantic_c, combined = %combined,
                            "Bayesian combined (both sides decayed)"
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        tracing::warn!(target: "reg.consolidation", entity = %h_mem.entity, error = %e, "Failed to update semantic h_mem");
                        continue;
                    }
                }
            } else {
                let semantic_triple = HMem {
                    id: HMemId::new(),
                    entity: h_mem.entity.clone(),
                    attribute: h_mem.attribute.clone(),
                    value: h_mem.value.clone(),
                    observed_at: h_mem.observed_at,
                    confidence: episodic_c,
                    access: h_mem.access.to_semantic(),
                    recalled_at: now,
                    dimension: h_mem.dimension,
                };

                match self.semantic.store_consolidated(semantic_triple) {
                    Ok(()) => {
                        consolidated_count += 1;
                        if let Err(e) = self.episodic.expire_h_mem(&h_mem.id) {
                            tracing::warn!(target: "reg.consolidation", triple_id = %h_mem.id.as_uuid(), error = %e, "Failed to expire episodic h_mem");
                        } else {
                            expired_count += 1;
                        }
                        tracing::debug!(
                            target: "reg.consolidation",
                            entity = %h_mem.entity, attribute = %h_mem.attribute,
                            stored = %h_mem.confidence, days_since_recall = days_since,
                            episodic = %episodic_c,
                            "New semantic h_mem seeded"
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        tracing::warn!(target: "reg.consolidation", entity = %h_mem.entity, error = %e, "Failed to store new semantic h_mem");
                        continue;
                    }
                }
            }
        }

        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            consolidated_count, combined_count,
            newly_seeded = consolidated_count - combined_count,
            expired_count, failed_count,
            "Consolidation complete"
        );

        Ok(ConsolidationOutcome {
            consolidated_count,
            deleted_count: expired_count,
            failed_count,
        })
    }

    /// Count consolidation candidates for a perspective.
    ///
    /// Returns the number of episodic h_mems eligible for consolidation
    /// (sorted by decayed confidence, oldest/lowest first), not total storage usage.
    pub fn consolidation_candidate_count(&self, perspective: &WebID) -> usize {
        self.episodic.consolidation_candidate_count(perspective)
    }
}
