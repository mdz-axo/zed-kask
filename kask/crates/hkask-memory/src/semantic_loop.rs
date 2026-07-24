//! Semantic Loop — knowledge → store → index → recall → dedup → combine → context (Loop 2b)
//!
//! Wraps `SemanticMemory` and provides two regulatory triggers:
//!
//! 1. **Storage budget** — when h_mem count exceeds the configurable budget
//!    (default 25,000), delete lowest-confidence h_mems to free space.
//!
//! 2. **Consolidation trigger** — review and delete semantic h_mems with
//!    confidence at or below the low-confidence threshold (default 0.33).
//!    These h_mems are too uncertain to be useful and should be pruned.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::semantic::SemanticMemory;
use hkask_regulation::types::loops::{
    ActionType, Deviation, DeviationDirection, LoopId, RegulationLoop, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric,
};
use hkask_storage::HMem;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;

/// Default storage budget for semantic h_mem count.
pub const DEFAULT_SEMANTIC_STORAGE_BUDGET: usize = 25_000;

/// Default low-confidence threshold for the consolidation trigger.
///
/// Semantic h_mems at or below this confidence (0.33 = 33%) are candidates
/// for review and deletion. These h_mems carry insufficient signal to
/// justify their storage cost.
pub const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f64 = 0.33;

/// Default condensation window in days.
///
/// Semantic h_mems with `valid_from` older than this many days are
/// candidates for entity-grouped condensation.
pub const DEFAULT_CONDENSATION_WINDOW_DAYS: u32 = 30;

/// Confidence assigned to condensed summary h_mems.
///
/// Summary h_mems carry 0.6 confidence — lower than directly observed
/// facts (1.0) but higher than the low-confidence threshold (0.33).
pub const CONDENSED_SUMMARY_CONFIDENCE: f64 = 0.6;

/// Semantic Loop — monitors semantic memory with two regulatory triggers.
///
/// Wraps `SemanticMemory` and reads:
/// - `triple_count` — current count vs storage budget
/// - `low_confidence_count` — h_mems at or below the confidence threshold
///
/// When `auto_condense` is enabled (default), the loop also performs
/// entity-grouped condensation: h_mems older than `condensation_window_days`
/// are grouped by entity, the highest-confidence + most recent h_mem is kept,
/// and the rest are soft-deleted with a provenance summary h_mem stored.
///
/// Both the storage budget and low-confidence threshold are configurable
/// per-loop instance, enabling per-user and per-agent customization.
pub struct SemanticLoop {
    memory: Arc<SemanticMemory>,
    storage_budget: usize,
    low_confidence_threshold: f64,
    /// Enable automatic semantic condensation (default: true).
    auto_condense: bool,
    /// Triples older than this many days are condensation candidates.
    condensation_window_days: u32,
}

impl SemanticLoop {
    /// Create a new Semantic Loop with default settings.
    ///
    /// Default storage budget: 25,000 h_mems.
    /// Default low-confidence threshold: 0.33 (33%).
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// \[P3\] Motivating: Generative Space — wraps semantic memory in a regulated knowledge loop
    /// \[P9\] Constraining: Homeostatic Self-Regulation — default budget and low-confidence threshold are set-points
    /// pre:  memory is initialized
    /// post: returns SemanticLoop with DEFAULT_SEMANTIC_STORAGE_BUDGET and DEFAULT_LOW_CONFIDENCE_THRESHOLD
    pub fn new(memory: Arc<SemanticMemory>) -> Self {
        Self {
            memory,
            storage_budget: DEFAULT_SEMANTIC_STORAGE_BUDGET,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            auto_condense: true,
            condensation_window_days: DEFAULT_CONDENSATION_WINDOW_DAYS,
        }
    }

    /// Create a new Semantic Loop with a custom storage budget.
    ///
    /// Use this for per-user or per-agent budget customization.
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// \[P3\] Motivating: Generative Space — customizes storage budget per user or agent
    /// \[P9\] Constraining: Homeostatic Self-Regulation — configurable set-point for memory homeostasis
    /// pre:  memory is initialized, storage_budget > 0
    /// post: returns SemanticLoop with custom budget, default threshold
    pub fn with_budget(memory: Arc<SemanticMemory>, storage_budget: usize) -> Self {
        Self {
            memory,
            storage_budget,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            auto_condense: true,
            condensation_window_days: DEFAULT_CONDENSATION_WINDOW_DAYS,
        }
    }

    /// Create a new Semantic Loop with custom storage budget and
    /// low-confidence threshold.
    ///
    /// Use this for full per-user or per-agent customization.
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// \[P3\] Motivating: Generative Space — customizes both budget and cleanup threshold
    /// \[P7\] Constraining: Evolutionary Architecture — thresholds emerge from usage patterns
    /// pre:  memory is initialized, storage_budget > 0
    /// pre:  low_confidence_threshold in [0.0, 1.0]
    /// post: returns SemanticLoop with custom budget and threshold
    pub fn with_budget_and_threshold(
        memory: Arc<SemanticMemory>,
        storage_budget: usize,
        low_confidence_threshold: f64,
    ) -> Self {
        Self {
            memory,
            storage_budget,
            low_confidence_threshold,
            auto_condense: true,
            condensation_window_days: DEFAULT_CONDENSATION_WINDOW_DAYS,
        }
    }

    /// Get the configured storage budget (set-point).
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// \[P3\] Motivating: Generative Space — exposes the semantic storage set-point
    /// \[P9\] Constraining: Homeostatic Self-Regulation — immutable budget reference for regulation
    /// post: returns the storage_budget value set at construction
    pub fn storage_budget(&self) -> usize {
        self.storage_budget
    }

    /// Get the configured low-confidence threshold.
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// \[P3\] Motivating: Generative Space — exposes the low-confidence cleanup set-point
    /// \[P9\] Constraining: Homeostatic Self-Regulation — threshold triggers pruning of uncertain knowledge
    /// post: returns the low_confidence_threshold value set at construction
    pub fn low_confidence_threshold(&self) -> f64 {
        self.low_confidence_threshold
    }

    /// Create with condensation disabled (for testing or manual curation).
    ///
    /// expect: "The system wraps semantic memory in a regulated knowledge loop"
    /// pre:  memory is initialized
    /// post: returns SemanticLoop with auto_condense and window set
    pub fn with_condensation(
        memory: Arc<SemanticMemory>,
        auto_condense: bool,
        condensation_window_days: u32,
    ) -> Self {
        Self {
            memory,
            storage_budget: DEFAULT_SEMANTIC_STORAGE_BUDGET,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            auto_condense,
            condensation_window_days,
        }
    }

    /// Check whether auto-condensation is enabled.
    pub fn auto_condense(&self) -> bool {
        self.auto_condense
    }

    /// Get the condensation window in days.
    pub fn condensation_window_days(&self) -> u32 {
        self.condensation_window_days
    }

    /// Emit a Regulation RegulationRecord through the memory's event sink.
    fn emit_reg(&self, verb: &str, observation: serde_json::Value) {
        if let Some(sink) = self.memory.event_sink() {
            let span = Span::new(
                SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
                verb,
            );
            let event = RegulationRecord::new(
                hkask_types::WebID::new(),
                span,
                CyclePhase::Act,
                observation,
                0,
            );
            let _ = sink.persist(&event);
        }
    }
}

#[async_trait::async_trait]
impl RegulationLoop for SemanticLoop {
    fn id(&self) -> LoopId {
        LoopId::Semantic
    }

    /// Sense: read semantic h_mem count and low-confidence count.
    ///
    /// Produces signals for:
    /// - `triple_count` — current count vs storage budget
    /// - `low_confidence_count` — h_mems at or below confidence threshold
    ///   (set-point = 0, any non-zero count is a deviation)
    async fn sense(&self) -> Vec<Signal> {
        let count = self.memory.h_mem_count().unwrap_or(0);
        let low_count = self
            .memory
            .low_confidence_count(self.low_confidence_threshold)
            .unwrap_or(0);

        vec![
            Signal::new(
                LoopId::Semantic,
                SignalMetric::TripleCount,
                count as f64,
                self.storage_budget as f64,
            ),
            Signal::new(
                LoopId::Semantic,
                SignalMetric::LowConfidenceCount,
                low_count as f64,
                0.0, // set-point = 0: any low-confidence h_mems are a deviation
            ),
        ]
    }

    /// Compute: produce actions based on deviations.
    ///
    /// - `triple_count` above set-point → Calibrate (budget exceeded)
    ///   If `auto_condense` is enabled, condensation is attempted first.
    /// - `low_confidence_count` above 0 → Calibrate (consolidation trigger)
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        for dev in deviations {
            match dev.signal.metric {
                SignalMetric::TripleCount if dev.direction == DeviationDirection::AboveSetPoint => {
                    // Try condensation first if enabled
                    if self.auto_condense {
                        // Check if there are old h_mems worth condensing
                        if let Ok(old_triples) = self
                            .memory
                            .h_mems_older_than(self.condensation_window_days, 200)
                            && !old_triples.is_empty()
                        {
                            actions.push(RegulatoryAction::new(
                                LoopId::Semantic,
                                ActionType::Calibrate,
                                RegulatoryActionParams::reason("semantic_condense"),
                            ));
                            // Don't also emit budget enforcement — condensation may resolve it
                            continue;
                        }
                    }

                    let _overage = (dev.signal.value - dev.signal.set_point) as usize;
                    actions.push(RegulatoryAction::new(
                        LoopId::Semantic,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("semantic_triple_count_exceeded"),
                    ));
                }
                SignalMetric::LowConfidenceCount
                    if dev.direction == DeviationDirection::AboveSetPoint =>
                {
                    actions.push(RegulatoryAction::new(
                        LoopId::Semantic,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("semantic_low_confidence_review"),
                    ));
                }
                _ => {}
            }
        }

        actions
    }

    /// Act: enforce regulation via deletion.
    ///
    /// Two triggers:
    ///
    /// - `semantic_low_confidence_review`: delete all semantic h_mems at or
    ///   below the low-confidence threshold (default 33%). These h_mems
    ///   carry insufficient signal to justify their storage cost.
    ///
    /// - `semantic_triple_count_exceeded`: delete lowest-confidence semantic
    ///   h_mems to bring count back within budget. Fires after the
    ///   low-confidence review — if budget is still exceeded after clearing
    ///   low-confidence entries, progressively delete the next-lowest.
    async fn act(&self, actions: &[RegulatoryAction]) {
        for action in actions {
            match action.action_type {
                ActionType::Calibrate => {
                    let reason = action.parameters.reason.as_str();
                    match reason {
                        "semantic_low_confidence_review" => {
                            // Use a fixed batch size (was previously extracted from action data).
                            let count = 100_usize;

                            if count == 0 {
                                continue;
                            }

                            // Delete all semantic h_mems at or below the threshold
                            match self
                                .memory
                                .low_confidence_h_mems(self.low_confidence_threshold, count)
                            {
                                Ok(candidates) if !candidates.is_empty() => {
                                    tracing::warn!(
                                        target: "hkask.semantic",
                                        candidates = candidates.len(),
                                        threshold = self.low_confidence_threshold,
                                        "Deleting low-confidence semantic h_mems (consolidation trigger)"
                                    );
                                    for h_mem in &candidates {
                                        if let Err(e) = self.memory.delete_h_mem(&h_mem.id) {
                                            tracing::debug!(
                                                target: "hkask.semantic",
                                                triple_id = %h_mem.id,
                                                entity = %h_mem.entity,
                                                attribute = %h_mem.attribute,
                                                confidence = %h_mem.confidence,
                                                error = %e,
                                                "Failed to delete low-confidence semantic h_mem"
                                            );
                                        }
                                    }
                                }
                                Ok(_) => {
                                    tracing::debug!(
                                        target: "hkask.semantic",
                                        "No low-confidence semantic h_mems found for deletion"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target: "hkask.semantic",
                                        error = %e,
                                        "Failed to query low-confidence semantic h_mems"
                                    );
                                }
                            }
                        }
                        "semantic_triple_count_exceeded" => {
                            // Use a fixed batch size (was previously extracted from action data).
                            let overage = 100_usize;

                            // Delete lowest-confidence h_mems to free budget
                            match self.memory.lowest_confidence_h_mems(overage) {
                                Ok(candidates) if !candidates.is_empty() => {
                                    tracing::warn!(
                                        target: "hkask.semantic",
                                        candidates = candidates.len(),
                                        overage = overage,
                                        "Deleting lowest-confidence semantic h_mems to enforce budget"
                                    );
                                    for h_mem in &candidates {
                                        if let Err(e) = self.memory.delete_h_mem(&h_mem.id) {
                                            tracing::debug!(
                                                target: "hkask.semantic",
                                                triple_id = %h_mem.id,
                                                entity = %h_mem.entity,
                                                attribute = %h_mem.attribute,
                                                error = %e,
                                                "Failed to delete semantic h_mem"
                                            );
                                        }
                                    }
                                }
                                Ok(_) => {
                                    tracing::debug!(
                                        target: "hkask.semantic",
                                        "No low-confidence semantic h_mems found for deletion"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target: "hkask.semantic",
                                        error = %e,
                                        "Failed to query low-confidence semantic h_mems"
                                    );
                                }
                            }
                        }
                        "semantic_condense" => {
                            let window_days = self.condensation_window_days;

                            // Query old h_mems for condensation
                            match self.memory.h_mems_older_than(window_days, 500) {
                                Ok(candidates) if !candidates.is_empty() => {
                                    let total = candidates.len();
                                    // Group by entity
                                    let mut groups: BTreeMap<String, Vec<&HMem>> = BTreeMap::new();
                                    for t in &candidates {
                                        groups.entry(t.entity.clone()).or_default().push(t);
                                    }

                                    let mut condensed_count = 0usize;
                                    let mut summary_count = 0usize;

                                    for (entity, group) in &groups {
                                        if group.len() < 2 {
                                            // Single h_mem per entity — nothing to condense
                                            continue;
                                        }

                                        // Keep the best: highest confidence, then most recent valid_from
                                        let mut sorted: Vec<&&HMem> = group.iter().collect();
                                        sorted.sort_by(|a, b| {
                                            b.confidence
                                                .value()
                                                .partial_cmp(&a.confidence.value())
                                                .unwrap_or(std::cmp::Ordering::Equal)
                                                .then_with(|| b.observed_at.cmp(&a.observed_at))
                                        });

                                        // Best h_mem is kept; rest are soft-deleted
                                        let condensed_ids: Vec<String> =
                                            sorted[1..].iter().map(|t| t.id.to_string()).collect();

                                        for id_str in &condensed_ids {
                                            let tid: hkask_storage::HMemId = match id_str.parse() {
                                                Ok(id) => id,
                                                Err(e) => {
                                                    tracing::debug!(
                                                        target: "hkask.semantic",
                                                        triple_id = %id_str,
                                                        entity = %entity,
                                                        error = %e,
                                                        "Failed to parse condensed h_mem ID"
                                                    );
                                                    continue;
                                                }
                                            };
                                            if let Err(e) = self.memory.close_h_mem(&tid) {
                                                tracing::debug!(
                                                    target: "hkask.semantic",
                                                    triple_id = %id_str,
                                                    entity = %entity,
                                                    error = %e,
                                                    "Failed to soft-delete condensed h_mem"
                                                );
                                            } else {
                                                condensed_count += 1;
                                            }
                                        }

                                        // Store provenance summary
                                        let summary = HMem::new(
                                            entity,
                                            "condensed_summary",
                                            serde_json::json!({
                                                "condensed_from": condensed_ids,
                                                "condensed_at": chrono::Utc::now().to_rfc3339(),
                                                "original_count": group.len(),
                                                "kept_triple_id": sorted[0].id.to_string(),
                                            }),
                                            sorted[0].access.owner_webid,
                                        )
                                        .with_confidence(CONDENSED_SUMMARY_CONFIDENCE);

                                        if let Err(e) = self.memory.store_consolidated(summary) {
                                            tracing::debug!(
                                                target: "hkask.semantic",
                                                entity = %entity,
                                                error = %e,
                                                "Failed to store condensation summary"
                                            );
                                        } else {
                                            summary_count += 1;
                                        }
                                    }

                                    tracing::info!(
                                        target: "hkask.semantic",
                                        total_candidates = total,
                                        condensed = condensed_count,
                                        summaries = summary_count,
                                        entities = groups.len(),
                                        window_days = window_days,
                                        "Semantic condensation completed"
                                    );

                                    self.emit_reg(
                                        "semantic_condensed",
                                        serde_json::json!({
                                            "total_candidates": total,
                                            "condensed": condensed_count,
                                            "summaries_stored": summary_count,
                                            "entity_groups": groups.len(),
                                            "window_days": window_days,
                                        }),
                                    );
                                }
                                Ok(_) => {
                                    tracing::debug!(
                                        target: "hkask.semantic",
                                        window_days = window_days,
                                        "No old semantic h_mems found for condensation"
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target: "hkask.semantic",
                                        error = %e,
                                        "Failed to query old semantic h_mems for condensation"
                                    );
                                }
                            }
                        }
                        _ => {
                            tracing::info!(
                                target: "hkask.semantic",
                                action_type = ?action.action_type,
                                target_loop = %action.target,
                                "Semantic Loop calibration action"
                            );
                        }
                    }
                }
                _ => {
                    tracing::info!(
                        target: "hkask.semantic",
                        action_type = ?action.action_type,
                        target_loop = %action.target,
                        "Semantic Loop regulatory action"
                    );
                }
            }
        }
    }
}
