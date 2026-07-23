//! hKask 6-Loop Architecture — channel types and re-exports.
//!
//! The loop type system (LoopId, Signal, Deviation, RegulatoryAction, Loop trait, etc.)
//! has been moved to `hkask_types::loops` to break the circular dependency
//! that prevented extracting Regulation subcrates.
//!
//! Channel types (`CurationInput`, `ToolConsumptionEvent`, etc.) remain here
//! because they depend on `RuntimeAlert` (Regulation-internal).
//!
//! **Loop Numbering (VSM correspondence):**
//!
//! The numbering follows Stafford Beer's VSM. Loop 3 (Control) is absorbed
//! into Cybernetics — the homeostatic regulator IS the controller.
//! There is no Loop 3; this is intentional, not a gap.
//!
//! | Loop | Name | VSM Role | Category |
//! |------|------|----------|----------|
//! | 1 | Inference | Implementation | Domain |
//! | 2a | Episodic Memory | Coordination (private) | Domain |
//! | 2b | Semantic Memory | Coordination (shared) | Domain |
//! | 5 | Curation | Metasystem (observer) | Meta |
//! | 6 | Cybernetics | Homeostatic regulation | Meta |
//! | 6b | Snapshot | Scheduled CAS snapshots | Meta |
//!
//! **Bridge:**
//! - 2a→2b: Consolidation — episodic → strip perspective → store semantic (one-way)
//!
//! **Authority DAG:** Curation → Cybernetics → {Inference, Episodic, Semantic}
//! No sideways edges. Authority flows downward.

// Channel types stay in hkask-regulation (depend on RuntimeAlert).
pub mod channels;
pub mod loop_trait;

// Re-export the full loop type system from hkask-types.
pub use channels::{
    CommunicationEvent, CurationInput, GoalLifecycle, GoalTransitionEvent, ToolConsumptionEvent,
};
pub use hkask_types::loops::{
    ActionDecision, ActionType, BudgetOption, Deviation, DeviationDirection,
    ExperienceClassification, ImpactReport, LoopId, LoopMetrics, RegulationData, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric, TriggerOrigin,
};

// The Loop trait stays in hkask-regulation (orphan rule — external crates impl it for foreign types).
pub use loop_trait::Loop;

// Backward-compatible alias — old code used `RegulationLoop`.
pub use loop_trait::Loop as RegulationLoop;

// Backward-compatible re-exports — CuratorDirective and CuratorHandle were
// previously re-exported from here but live in hkask_types::curator.
pub use hkask_types::curator::{CuratorDirective, CuratorHandle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_quality_default_is_zero() {
        let q = LoopMetrics::default();
        assert_eq!(q.delay_ms, 0);
        assert_eq!(q.gain, 0.0);
        assert_eq!(q.fidelity_score, 0.0);
    }

    #[test]
    fn loop_id_display_roundtrips() {
        assert_eq!(LoopId::Cybernetics.to_string(), "cybernetics");
        assert_eq!(LoopId::Inference.to_string(), "inference");
    }

    #[test]
    fn signal_metric_as_str_matches_serde() {
        assert_eq!(SignalMetric::EnergyRemaining.as_str(), "energy_remaining");
        assert_eq!(SignalMetric::VarietyDeficit.as_str(), "variety_deficit");
    }

    #[test]
    fn action_type_parse_roundtrips() {
        for variant in [
            ActionType::Throttle,
            ActionType::Escalate,
            ActionType::Calibrate,
            ActionType::CircuitBreak,
            ActionType::AdjustEnergyBudget,
            ActionType::OverrideEnergyBudget,
            ActionType::ReplenishBudget,
            ActionType::Notify,
            ActionType::Prune,
        ] {
            assert_eq!(ActionType::parse(variant.as_str()), Some(variant));
        }
        assert_eq!(ActionType::parse("Nonexistent"), None);
    }

    #[test]
    fn deviation_from_signal_detects_above() {
        let sig = Signal::new(LoopId::Cybernetics, SignalMetric::ErrorRate, 0.5, 0.1);
        let dev = Deviation::from_signal(&sig).expect("deviation should exist");
        assert_eq!(dev.direction, DeviationDirection::AboveSetPoint);
        assert!((dev.magnitude - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn deviation_from_signal_returns_none_at_set_point() {
        let sig = Signal::new(LoopId::Cybernetics, SignalMetric::ErrorRate, 0.1, 0.1);
        assert!(Deviation::from_signal(&sig).is_none());
    }

    #[test]
    fn experience_classification_default_confidence() {
        assert_eq!(ExperienceClassification::Success.default_confidence(), 0.9);
        assert_eq!(ExperienceClassification::Failure.default_confidence(), 0.3);
    }

    #[test]
    fn regulation_data_remaining_ratio_extracts() {
        assert_eq!(
            RegulationData::EnergyBudgetLow {
                remaining_ratio: 0.5,
                set_point: 0.2
            }
            .remaining_ratio(),
            Some(0.5)
        );
        assert_eq!(RegulationData::NoData.remaining_ratio(), None);
    }
}
