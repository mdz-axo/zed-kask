//! Consolidation Service — combined consolidation + cleanup

use std::sync::Arc;

use crate::consolidation::ConsolidationBridge;
use crate::semantic::SemanticMemory;
use hkask_types::WebID;
use hkask_types::{ConsolidationOutcome, ConsolidationRequest};

pub struct ConsolidationService {
    bridge: Arc<ConsolidationBridge>,
    semantic: Arc<SemanticMemory>,
}

impl ConsolidationService {
    pub fn new(bridge: Arc<ConsolidationBridge>, semantic: Arc<SemanticMemory>) -> Self {
        Self { bridge, semantic }
    }

    /// Execute a consolidation operation — three phases:
    /// 1. Promote episodic h_mems to semantic memory (bridge also soft-deletes the
    ///    episodic source h_mems; those expirations are reported separately by the bridge).
    /// 2. Delete semantic h_mems at or below confidence floor (if specified).
    /// 3. Delete lowest-confidence semantic h_mems until within max count (if specified).
    ///
    /// Note: `deleted_count` in the returned outcome counts only the semantic cleanup
    /// deletions performed by this service. The bridge's own `deleted_count` reports
    /// episodic source expirations.
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

        if let Some(floor) = request.confidence_floor
            && let Ok(candidates) = self.semantic.low_confidence_h_mems(floor, usize::MAX)
            && !candidates.is_empty()
        {
            for h_mem in &candidates {
                if self.semantic.delete_h_mem(&h_mem.id).is_ok() {
                    deleted_count += 1;
                }
            }
        }

        if let Some(max) = request.max_semantic_triples
            && let Ok(count) = self.semantic.h_mem_count()
            && count > max
            && let Ok(candidates) = self.semantic.lowest_confidence_h_mems(count - max)
        {
            for h_mem in &candidates {
                if self.semantic.delete_h_mem(&h_mem.id).is_ok() {
                    deleted_count += 1;
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

    pub fn semantic_low_confidence_count(&self, threshold: f64) -> usize {
        self.semantic.low_confidence_count(threshold).unwrap_or(0)
    }

    pub fn semantic_h_mem_count(&self) -> usize {
        self.semantic.h_mem_count().unwrap_or(0)
    }
}
