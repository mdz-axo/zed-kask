//! Memory consolidator — confidence-based cleanup + budget pruning.
//!
//! When budget pressure triggers consolidation, h_mems are pruned by:
//! 1. Deleting h_mems at or below the confidence floor (if specified).
//! 2. Deleting lowest-confidence h_mems until within the storage budget.
//!
//! All h_mems are unified — the ontology blob carries dual-axis anchoring
//! (PKO process + DC state) but there is no type distinction.

use std::sync::Arc;

use crate::memory_store::MemoryStore;
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

/// Memory consolidator — confidence-based cleanup.
///
/// Called when budget pressure requires freeing storage. Deletes
/// low-confidence h_mems and prunes to the storage budget.
pub struct MemoryConsolidator {
    store: Arc<MemoryStore>,
}

impl MemoryConsolidator {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Execute a consolidation operation — two phases:
    /// 1. Delete h_mems at or below confidence floor (if specified).
    /// 2. Delete lowest-confidence h_mems until within max count (if
    ///    specified). When the caller omits `max_h_mems`, the store's
    ///    `storage_budget` acts as the default cap.
    pub fn consolidate(
        &self,
        perspective: &WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        let max_h_mems = match request.max_h_mems {
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
            max_h_mems = ?max_h_mems,
            "Consolidation starting"
        );

        let mut deleted_count = 0usize;
        let mut failed_count = 0usize;

        if let Some(floor) = request.confidence_floor {
            match self.store.low_confidence_h_mems(floor, usize::MAX) {
                Ok(candidates) if !candidates.is_empty() => {
                    for h_mem in &candidates {
                        match self.store.delete_h_mem(&h_mem.id) {
                            Ok(()) => deleted_count += 1,
                            Err(e) => {
                                failed_count += 1;
                                tracing::warn!(
                                    target: "reg.consolidation",
                                    error = %e,
                                    h_mem_id = ?h_mem.id,
                                    "Failed to delete low-confidence h_mem during consolidation cleanup"
                                );
                            }
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

        if let Some(max) = max_h_mems {
            match self.store.h_mem_count() {
                Ok(count) if count > max => {
                    match self.store.lowest_confidence_h_mems(count - max) {
                        Ok(candidates) => {
                            for h_mem in &candidates {
                                match self.store.delete_h_mem(&h_mem.id) {
                                    Ok(()) => deleted_count += 1,
                                    Err(e) => {
                                        failed_count += 1;
                                        tracing::warn!(
                                            target: "reg.consolidation",
                                            error = %e,
                                            h_mem_id = ?h_mem.id,
                                            "Failed to delete excess h_mem during consolidation cleanup"
                                        );
                                    }
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
            deleted = deleted_count,
            failed = failed_count,
            "Consolidation complete"
        );

        Ok(ConsolidationOutcome {
            deleted_count,
            failed_count,
        })
    }
}
