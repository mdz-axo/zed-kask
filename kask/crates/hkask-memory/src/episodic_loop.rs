//! Episodic Loop — experience → encode → store → recall → temporal weight → context (Loop 2a)
//!
//! Wraps `EpisodicMemory` and provides loop-level budget observability AND
//! enforcement. Budget regulation is now owned by this loop (Cybernetics
//! concern), not by domain code in `EpisodicMemory`.

use std::sync::Arc;

use crate::consolidation::ConsolidationBridge;
use crate::episodic::EpisodicMemory;
use hkask_regulation::types::loops::{
    ActionType, Deviation, DeviationDirection, LoopId, RegulationLoop, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric,
};
use hkask_types::ConsolidationRequest;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;

/// Episodic Loop — monitors episodic storage usage against budget and enforces limits.
///
/// Wraps `EpisodicMemory` and reads storage usage per perspective. When
/// usage exceeds 80% of budget, it produces `Throttle` actions targeting
/// itself. When usage exceeds 100%, it escalates to the Curation loop
/// and consolidates lowest-confidence episodic h_mems to semantic memory
/// to free storage.
pub struct EpisodicLoop {
    memory: Arc<EpisodicMemory>,
    perspective: WebID,
    storage_budget: usize,
    consolidation: Option<Arc<ConsolidationBridge>>,
}

impl EpisodicLoop {
    /// Create a new Episodic Loop wrapping an EpisodicMemory.
    ///
    /// The `perspective` identifies which agent's episodic storage to monitor.
    /// The `storage_budget` is the set-point for the regulation signal.
    ///
    /// expect: "The system wraps episodic memory in a regulated generative loop"
    /// \[P3\] Motivating: Generative Space — wraps episodic memory in a regulated generative loop
    /// \[P9\] Constraining: Homeostatic Self-Regulation — storage_budget is the cybernetic set-point
    /// pre:  memory is initialized, perspective is valid, storage_budget > 0
    /// post: returns EpisodicLoop without consolidation bridge
    pub fn new(memory: Arc<EpisodicMemory>, perspective: WebID, storage_budget: usize) -> Self {
        Self {
            memory,
            perspective,
            storage_budget,
            consolidation: None,
        }
    }

    /// Create an Episodic Loop with a consolidation bridge.
    pub fn with_consolidation(
        memory: Arc<EpisodicMemory>,
        perspective: WebID,
        storage_budget: usize,
        consolidation: Arc<ConsolidationBridge>,
    ) -> Self {
        Self {
            memory,
            perspective,
            storage_budget,
            consolidation: Some(consolidation),
        }
    }

    /// Get the configured storage budget (set-point).
    ///
    /// expect: "The system wraps episodic memory in a regulated generative loop"
    /// \[P3\] Motivating: Generative Space — exposes the generative budget set-point for context assembly
    /// \[P9\] Constraining: Homeostatic Self-Regulation — budget value is immutable after construction
    /// post: returns the storage_budget value set at construction
    pub fn storage_budget(&self) -> usize {
        self.storage_budget
    }

    /// Emit a Regulation RegulationRecord through the memory's event sink.
    fn emit_reg(&self, verb: &str, observation: serde_json::Value) {
        if let Some(sink) = self.memory.event_sink() {
            let span = Span::new(
                SpanNamespace::try_from(RegulationSpan::MemoryEncode).expect("canonical span"),
                verb,
            );
            let event =
                RegulationRecord::new(self.perspective, span, CyclePhase::Act, observation, 0);
            let _ = sink.persist(&event);
        }
    }
}

#[async_trait::async_trait]
impl RegulationLoop for EpisodicLoop {
    fn id(&self) -> LoopId {
        LoopId::Episodic
    }

    /// Sense: read storage usage and memory life.
    ///
    /// Produces signals for:
    /// - `storage_usage` — current h_mem count vs storage budget
    /// - `memory_life` — memory life S in days (Wozniak-Gorzelanczyk, 1995)
    ///   Default: 180 days (6 months × 30). Configurable via HKASK_MEMORY_LIFE_DAYS.
    async fn sense(&self) -> Vec<Signal> {
        let usage = self.memory.storage_usage(&self.perspective).unwrap_or(0);
        let life_days = self.memory.memory_life_days();

        vec![
            Signal::new(
                LoopId::Episodic,
                SignalMetric::StorageUsage,
                usage as f64,
                self.storage_budget as f64,
            ),
            Signal::new(
                LoopId::Episodic,
                SignalMetric::MemoryLife,
                life_days,
                life_days,
            ),
        ]
    }

    /// Compute: produce regulatory actions based on storage usage thresholds.
    ///
    /// - >80% of budget → `Throttle` self (reduce ingestion rate)
    /// - >100% of budget → `Escalate` to Curation AND `Calibrate` self (consolidate h_mems)
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        for dev in deviations {
            if dev.signal.metric == SignalMetric::StorageUsage
                && dev.direction == DeviationDirection::AboveSetPoint
            {
                let ratio = dev.signal.value / dev.signal.set_point;

                if ratio > 1.0 {
                    // Budget exceeded — consolidate to free space
                    let _overage = (dev.signal.value - dev.signal.set_point) as usize;
                    actions.push(RegulatoryAction::new(
                        LoopId::Episodic,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("episodic_budget_exceeded_consolidate"),
                    ));
                    // Also escalate to Curation (budget exceeded)
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Escalate,
                        RegulatoryActionParams::reason("episodic_budget_exceeded"),
                    ));
                } else if ratio > 0.8 {
                    // Approaching budget — throttle self
                    actions.push(RegulatoryAction::new(
                        LoopId::Episodic,
                        ActionType::Throttle,
                        RegulatoryActionParams::reason("episodic_budget_approaching"),
                    ));
                }
            }
        }

        actions
    }

    /// Act: enforce budget regulation via consolidation.
    ///
    /// - `Calibrate` with reason `episodic_budget_exceeded_consolidate`: fire
    ///   the consolidation bridge to promote lowest-confidence, oldest h_mems
    ///   from episodic to semantic memory, freeing storage.
    /// - `Throttle`: log warning (no direct enforcement — ingestion rate
    ///   limiting is handled by the caller checking storage usage).
    /// - `Escalate`: logged (Curation loop handles escalation).
    async fn act(&self, actions: &[RegulatoryAction]) {
        for action in actions {
            match action.action_type {
                ActionType::Calibrate => {
                    let reason = action.parameters.reason.as_str();
                    if reason == "episodic_budget_exceeded_consolidate" {
                        // Consolidate a fixed batch per cycle (overage was previously
                        // extracted from action data — now uses a reasonable default).
                        let overage = 100_usize;

                        // Fire consolidation bridge: promote lowest-confidence,
                        // oldest episodic h_mems to semantic memory
                        if let Some(consolidation) = &self.consolidation {
                            match consolidation.consolidate(
                                self.perspective,
                                ConsolidationRequest {
                                    limit: overage,
                                    ..Default::default()
                                },
                            ) {
                                Ok(outcome) if outcome.consolidated_count > 0 => {
                                    self.emit_reg(
                                        "episodic_consolidated",
                                        serde_json::json!({
                                            "consolidated": outcome.consolidated_count,
                                            "failed": outcome.failed_count,
                                            "reason": "budget_enforcement"
                                        }),
                                    );
                                }
                                Ok(_) => {
                                    // No-op: consolidation fired but no h_mems to consolidate
                                }
                                Err(e) => {
                                    self.emit_reg(
                                        "episodic_consolidation_failed",
                                        serde_json::json!({
                                            "error": e.to_string(),
                                            "reason": "budget_enforcement"
                                        }),
                                    );
                                }
                            }
                        } else {
                            self.emit_reg(
                                "episodic_budget_exceeded_no_bridge",
                                serde_json::json!({
                                    "overage": overage
                                }),
                            );
                        }
                    } else {
                        self.emit_reg(
                            "episodic_calibrate",
                            serde_json::json!({
                                "action_type": format!("{:?}", action.action_type),
                                "target_loop": action.target.to_string()
                            }),
                        );
                    }
                }
                _ => {
                    self.emit_reg(
                        "episodic_regulate",
                        serde_json::json!({
                            "action_type": format!("{:?}", action.action_type),
                            "target_loop": action.target.to_string()
                        }),
                    );
                }
            }
        }
    }
}
