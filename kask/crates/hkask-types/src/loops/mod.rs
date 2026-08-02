//! Loop type system — identifiers, signals, actions, and quality telemetry.
//!
//! These types were moved from `hkask-regulation/src/types/loops/` to `hkask-types`
//! to break the circular dependency that prevented extracting Regulation subcrates
//! (storage guard, SLO, seam watcher). They have no Regulation-internal dependencies.
//!

//!
//! Channel types (`CurationInput`, `ToolConsumptionEvent`, etc.) also remain
//! in `hkask-regulation` because they depend on `RuntimeAlert` (Regulation-internal).

pub mod actions;
pub mod core;
pub mod episodic;
pub mod signals;

pub use actions::{
    ActionType, BudgetOption, RegulationData, RegulatoryAction, RegulatoryActionParams,
};
pub use core::{ActionDecision, ImpactReport, LoopId, LoopMetrics, TriggerOrigin};
pub use episodic::ExperienceClassification;
pub use signals::{Deviation, DeviationDirection, Signal, SignalMetric};
