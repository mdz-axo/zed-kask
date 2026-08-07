//! Consolidation Bridge — perspective-bound → shared (one-way)
//!
//! When currency pressure triggers consolidation, perspective-bound h_mems
//! (episodic experiences) are:
//! 1. Selected via `MemoryStore::consolidation_candidates()` (oldest, lowest
//!    effective confidence for a given perspective)
//! 2. Stripped of perspective (privacy boundary removal), visibility set to
//!    Shared
//! 3. Checked against existing shared h_mems with same EAV:
//!    a. **Match found:** Bayesian combine confidences, update existing
//!    b. **No match:** Seed as new shared h_mem
//! 4. Expired in the source (valid_to set, soft-deleted) to free storage budget
//!
//! This is a ONE-WAY operation: perspective-bound → shared. No reverse flow.
//! The episodic/semantic distinction is now encoded in the `HMemOntology` blob
//! (P5.4), not in separate store structs — but the consolidation logic remains
//! the same: promote first-person experiences to shared facts.

use std::sync::Arc;

use crate::bayesian::combine_confidences;
use crate::memory_store::MemoryStore;
use hkask_storage::{HMem, HMemId};
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

/// Consolidation Bridge — perspective-bound → shared
///
/// One-way operation called when budget pressure requires freeing
/// perspective-bound storage. Promotes episodic experiences to shared facts.
pub struct ConsolidationBridge {
    store: Arc<MemoryStore>,
}

impl ConsolidationBridge {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Consolidate perspective-bound h_mems into shared memory (one-way).
    ///
    /// For each candidate (filtered by `perspective`):
    /// 1. Strip perspective (set to `None`) — removes privacy boundary
    /// 2. Check shared memory for existing h_mem with same EAV hash:
    ///    a. **Match:** Bayesian combine candidate + existing confidence,
    ///       update existing h_mem
    ///    b. **No match:** Insert as new shared h_mem
    /// 3. Expire source h_mem (soft-delete via valid_to)
    #[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
    pub fn consolidate(
        &self,
        perspective: WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        let span = tracing::span!(target: "reg.consolidation", tracing::Level::INFO, "consolidate");
        let _enter = span.enter();

        let candidates = self
            .store
            .consolidation_candidates(perspective, request.limit)
            .map_err(|e| anyhow::anyhow!("Memory store error: {e}"))?;

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
            let candidate_c = h_mem
                .confidence
                .memory_decay(days_since, self.store.memory_life_days());

            if let Some(existing) = self.store.find_existing_by_eav(h_mem) {
                // Decay existing confidence to same temporal reference as candidate.
                // Both sides of the Bayesian combination must use decayed confidence
                // so independent evidence is combined at the same point in time.
                let existing_days_since = crate::bayesian::days_since(existing.recalled_at);
                let existing_c = existing
                    .confidence
                    .memory_decay(existing_days_since, self.store.memory_life_days());
                let combined = combine_confidences(existing_c, candidate_c);

                match self
                    .store
                    .update_confidence(&existing.id, h_mem.value.clone(), combined)
                {
                    Ok(()) => {
                        combined_count += 1;
                        consolidated_count += 1;
                        if let Err(e) = self.store.expire_h_mem(&h_mem.id) {
                            tracing::warn!(target: "reg.consolidation", triple_id = %h_mem.id.as_uuid(), error = %e, "Failed to expire source h_mem");
                        } else {
                            expired_count += 1;
                        }
                        tracing::debug!(
                            target: "reg.consolidation",
                            entity = %h_mem.entity, attribute = %h_mem.attribute,
                            stored = %h_mem.confidence, days_since_recall = days_since,
                            candidate = %candidate_c,
                            existing_stored = %existing.confidence,
                            existing_days = existing_days_since,
                            existing = %existing_c, combined = %combined,
                            "Bayesian combined (both sides decayed)"
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        tracing::warn!(target: "reg.consolidation", entity = %h_mem.entity, error = %e, "Failed to update existing h_mem");
                        continue;
                    }
                }
            } else {
                let promoted = HMem {
                    id: HMemId::new(),
                    entity: h_mem.entity.clone(),
                    attribute: h_mem.attribute.clone(),
                    value: h_mem.value.clone(),
                    observed_at: h_mem.observed_at,
                    confidence: candidate_c,
                    access: h_mem.access.to_semantic(),
                    recalled_at: now,
                    ontology: h_mem.ontology.clone(),
                };

                match self.store.store_consolidated(promoted) {
                    Ok(()) => {
                        consolidated_count += 1;
                        if let Err(e) = self.store.expire_h_mem(&h_mem.id) {
                            tracing::warn!(target: "reg.consolidation", triple_id = %h_mem.id.as_uuid(), error = %e, "Failed to expire source h_mem");
                        } else {
                            expired_count += 1;
                        }
                        tracing::debug!(
                            target: "reg.consolidation",
                            entity = %h_mem.entity, attribute = %h_mem.attribute,
                            stored = %h_mem.confidence, days_since_recall = days_since,
                            candidate = %candidate_c,
                            "New shared h_mem seeded"
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        tracing::warn!(target: "reg.consolidation", entity = %h_mem.entity, error = %e, "Failed to store new shared h_mem");
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
    /// Returns the number of perspective-bound h_mems eligible for
    /// consolidation (sorted by decayed confidence, oldest/lowest first),
    /// not total storage usage.
    pub fn consolidation_candidate_count(&self, perspective: &WebID) -> usize {
        self.store.consolidation_candidate_count(perspective)
    }
}
