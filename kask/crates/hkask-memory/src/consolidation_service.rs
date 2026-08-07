//! Consolidation Service — combined consolidation + cleanup

use std::sync::Arc;

use crate::consolidation::ConsolidationBridge;
use crate::memory_store::MemoryStore;
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

pub struct ConsolidationService {
    bridge: Arc<ConsolidationBridge>,
    store: Arc<MemoryStore>,
}

impl ConsolidationService {
    pub fn new(bridge: Arc<ConsolidationBridge>, store: Arc<MemoryStore>) -> Self {
        Self { bridge, store }
    }

    /// Execute a consolidation operation — three phases:
    /// 1. Promote perspective-bound h_mems to shared memory (bridge also
    ///    soft-deletes the source h_mems; those expirations are reported
    ///    separately by the bridge).
    /// 2. Delete shared h_mems at or below confidence floor (if specified).
    /// 3. Delete lowest-confidence shared h_mems until within max count (if
    ///    specified).
    ///
    /// Note: `deleted_count` in the returned outcome counts only the cleanup
    /// deletions performed by this service. The bridge's own `deleted_count`
    /// reports source expirations.
    pub fn consolidate(
        &self,
        perspective: &WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            limit = request.limit,
            confidence_floor = ?request.confidence_floor,
            max_semantic_triples = ?request.max_semantic_triples,
            "Consolidation starting"
        );

        let bridge_outcome = self.bridge.consolidate(
            *perspective,
            ConsolidationRequest {
                limit: request.limit,
                confidence_floor: None,
                max_semantic_triples: None,
            },
        )?;

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

        if let Some(max) = request.max_semantic_triples {
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
            consolidated = bridge_outcome.consolidated_count,
            deleted = deleted_count,
            failed = bridge_outcome.failed_count,
            "Consolidation complete"
        );

        Ok(ConsolidationOutcome {
            consolidated_count: bridge_outcome.consolidated_count,
            deleted_count,
            failed_count: bridge_outcome.failed_count,
        })
    }

    pub fn consolidation_candidate_count(&self, perspective: &WebID) -> usize {
        self.bridge.consolidation_candidate_count(perspective)
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
