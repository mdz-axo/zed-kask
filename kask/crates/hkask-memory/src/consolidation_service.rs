//! Memory consolidator — episodic → semantic promotion + cleanup.
//!
//! When currency pressure triggers consolidation, episodic h_mems (those
//! carrying a PKO procedure in their `HMemOntology` blob) are:
//! 1. Selected via `MemoryStore::consolidation_candidates()` (oldest, lowest
//!    effective confidence for a given perspective)
//! 2. Re-tagged from episodic ontology (PKO process axis) to semantic ontology
//!    (DC+BIBO state axis) via `HMemOntology::to_semantic()`, visibility set
//!    to Shared
//! 3. Checked against existing semantic h_mems with same EAV:
//!    a. **Match found:** Bayesian combine confidences, update existing
//!    b. **No match:** Seed as new semantic h_mem
//! 4. Expired in the source (valid_to set, soft-deleted) to free storage budget
//!
//! This is a ONE-WAY operation: episodic → semantic. No reverse flow.
//!
//! The episodic/semantic distinction is carried by the `HMemOntology` blob
//! (P5.4 dual-axis anchoring): the intended flow is chat stream → chunks →
//! each chunk tagged with both the best-fit state axis (Dublin Core) and the
//! best-fit process axis (PKO). The consolidator selects episodic candidates
//! by the ontology blob (`pko_procedure IS NOT NULL`), not by the deprecated
//! `perspective` field. The `perspective` field is retained as provenance
//! (who wrote the memory) but is no longer the episodic/semantic discriminator.
//!
//! Renamed from `ConsolidationService` to `MemoryConsolidator` to avoid the
//! name collision with `hkask_mcp_corpus::services::consolidation::ConsolidationService`
//! (an unrelated type that does LLM chunk synthesis). The two types have
//! always been in different crates, but the shared name was a latent trap
//! for any file that imports both.

use std::sync::Arc;

use crate::bayesian::combine_confidences;
use crate::memory_store::MemoryStore;
use hkask_storage::{HMem, HMemId};
use hkask_types::Visibility;
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

/// Memory consolidator — perspective-bound → shared promotion + cleanup.
///
/// One-way operation called when budget pressure requires freeing
/// perspective-bound storage. Promotes episodic experiences to shared facts,
/// then optionally prunes shared h_mems by confidence floor or max count.
pub struct MemoryConsolidator {
    store: Arc<MemoryStore>,
}

impl MemoryConsolidator {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Execute a consolidation operation — three phases:
    /// 1. Promote perspective-bound h_mems to shared memory (soft-deletes the
    ///    source h_mems; those expirations are reported in `deleted_count`).
    /// 2. Delete shared h_mems at or below confidence floor (if specified).
    /// 3. Delete lowest-confidence shared h_mems until within max count (if
    ///    specified). When the caller omits `max_semantic_triples`, the
    ///    store's `storage_budget` acts as the default cap — the Ashby
    ///    attenuator for unbounded memory growth. A store over budget gets
    ///    pruned back to the budget; a store under budget is left alone.
    pub fn consolidate(
        &self,
        perspective: &WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        // Resolve the effective max-semantic-triples cap. The caller's explicit
        // request wins; otherwise the store's storage_budget is the default
        // attenuator. Without this, the budget field is dead config (the
        // `.rules` "Advertised invariants need enforcement points" trap).
        let max_semantic_triples = match request.max_semantic_triples {
            Some(max) => Some(max),
            None => {
                let budget = self.store.storage_budget();
                if budget == 0 {
                    None
                } else {
                    match self.store.h_mem_count() {
                        Ok(count) if count > budget => {
                            tracing::info!(
                                target: "reg.consolidation",
                                count,
                                budget,
                                "Store over storage budget — pruning to budget"
                            );
                            Some(budget)
                        }
                        Ok(_) => None,
                        Err(error) => {
                            tracing::warn!(
                                target: "reg.consolidation",
                                %error,
                                "h_mem_count: signal stale, skipping budget prune"
                            );
                            None
                        }
                    }
                }
            }
        };

        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            limit = request.limit,
            confidence_floor = ?request.confidence_floor,
            max_semantic_triples = ?max_semantic_triples,
            "Consolidation starting"
        );

        let promotion_outcome = self.promote_episodic_to_semantic(*perspective, request.limit)?;

        let mut deleted_count = 0usize;

        if let Some(floor) = request.confidence_floor {
            match self.store.low_confidence_h_mems(floor, usize::MAX) {
                Ok(candidates) if !candidates.is_empty() => {
                    for h_mem in &candidates {
                        match self.store.delete_h_mem(&h_mem.id) {
                            Ok(()) => deleted_count += 1,
                            Err(e) => tracing::warn!(
                                target: "reg.consolidation",
                                error = %e,
                                h_mem_id = ?h_mem.id,
                                "Failed to delete low-confidence h_mem during consolidation cleanup"
                            ),
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        target: "reg.consolidation",
                        %error,
                        "low_confidence_h_mems: signal stale, skipping cleanup"
                    );
                }
            }
        }

        if let Some(max) = max_semantic_triples {
            match self.store.h_mem_count() {
                Ok(count) if count > max => {
                    match self.store.lowest_confidence_h_mems(count - max) {
                        Ok(candidates) => {
                            for h_mem in &candidates {
                                match self.store.delete_h_mem(&h_mem.id) {
                                    Ok(()) => deleted_count += 1,
                                    Err(e) => tracing::warn!(
                                        target: "reg.consolidation",
                                        error = %e,
                                        h_mem_id = ?h_mem.id,
                                        "Failed to delete excess h_mem during consolidation cleanup"
                                    ),
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                target: "reg.consolidation",
                                %error,
                                "lowest_confidence_h_mems: signal stale, skipping cleanup"
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        target: "reg.consolidation",
                        %error,
                        "h_mem_count: signal stale, skipping cleanup"
                    );
                }
            }
        }

        tracing::info!(
            target: "reg.consolidation",
            consolidated = promotion_outcome.consolidated_count,
            deleted = deleted_count,
            failed = promotion_outcome.failed_count,
            "Consolidation complete"
        );

        Ok(ConsolidationOutcome {
            consolidated_count: promotion_outcome.consolidated_count,
            deleted_count,
            failed_count: promotion_outcome.failed_count,
        })
    }

    /// Promote episodic h_mems to semantic memory (one-way).
    ///
    /// For each candidate (episodic h_mems with a PKO procedure, filtered by
    /// `perspective` to scope by who wrote them):
    /// 1. Re-tag the ontology blob from episodic (PKO) to semantic (DC+BIBO)
    ///    via `HMemOntology::to_semantic()`
    /// 2. Set visibility to Shared (the access-control aspect of promotion)
    /// 3. Check semantic memory for existing h_mem with same EAV hash:
    ///    a. **Match:** Bayesian combine candidate + existing confidence,
    ///       update existing h_mem
    ///    b. **No match:** Insert as new semantic h_mem
    /// 4. Expire source h_mem (soft-delete via valid_to)
    #[allow(clippy::doc_lazy_continuation, clippy::doc_overindented_list_items)]
    fn promote_episodic_to_semantic(
        &self,
        perspective: WebID,
        limit: usize,
    ) -> anyhow::Result<ConsolidationOutcome> {
        let span = tracing::span!(target: "reg.consolidation", tracing::Level::INFO, "consolidate");
        let _enter = span.enter();

        let candidates = self
            .store
            .consolidation_candidates(perspective, limit)
            .map_err(|e| anyhow::anyhow!("Memory store error: {e}"))?;

        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            candidate_count = candidates.len(),
            limit,
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
                    access: h_mem.access.clone().with_visibility(Visibility::Shared),
                    recalled_at: now,
                    ontology: h_mem.ontology.as_ref().map(|o| o.to_semantic()),
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

    /// Count shared h_mems at or below a confidence threshold.
    ///
    /// Returns 0 on storage error as documented degradation — the
    /// consolidation loop must not hard-fail on a transient DB error, but
    /// the operator sees a `tracing::warn!` so a stale signal is
    /// distinguishable from a measured zero.
    pub fn semantic_low_confidence_count(&self, threshold: f64) -> usize {
        match self.store.low_confidence_count(threshold) {
            Ok(count) => count,
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    "semantic_low_confidence_count: signal stale, returning 0"
                );
                0
            }
        }
    }

    /// Count all shared h_mems.
    ///
    /// Returns 0 on storage error as documented degradation — see
    /// `semantic_low_confidence_count` for the rationale.
    pub fn semantic_h_mem_count(&self) -> usize {
        match self.store.h_mem_count() {
            Ok(count) => count,
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    "semantic_h_mem_count: signal stale, returning 0"
                );
                0
            }
        }
    }
}
