//! Memory consolidator — confidence-based cleanup.
//!
//! Consolidation deletes h_mems by confidence (never by count — budgets are
//! deprecated, operator ruling 2026-09-04; forgetting is time-based and
//! distillation-gated):
//! 1. Deleting h_mems at or below the confidence floor (if specified).
//! 2. Deleting lowest-confidence h_mems down to the caller's explicit
//!    `max_h_mems` (if specified — an explicit instruction, not a hidden cap).
//!
//! All h_mems are unified — the ontology blob carries dual-axis anchoring
//! (PKO process + DC state) but there is no type distinction.

use std::sync::Arc;

use crate::memory_store::MemoryStore;
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

/// Memory consolidator — confidence-based cleanup.
///
/// Deletes low-confidence h_mems and prunes to the caller's explicit cap.
pub struct MemoryConsolidator {
    store: Arc<MemoryStore>,
}

impl MemoryConsolidator {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Execute a consolidation operation — two phases:
    /// 1. Delete h_mems at or below confidence floor (if specified).
    /// 2. Delete lowest-confidence h_mems until within the caller's explicit
    ///    `max_h_mems` (if specified). When omitted, there is no count cap —
    ///    count-based pruning was deprecated (operator ruling 2026-09-04:
    ///    forgetting is time-based and distillation-gated, never count-based;
    ///    the former hidden `storage_budget` fallback is removed).
    pub fn consolidate(
        &self,
        perspective: &WebID,
        request: ConsolidationRequest,
    ) -> anyhow::Result<ConsolidationOutcome> {
        let max_h_mems = request.max_h_mems;

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
