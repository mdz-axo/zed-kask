//! hKask 6-Loop Architecture — loop type system and channel types.
//!
//! The loop type system (LoopId, Signal, Deviation, RegulatoryAction, etc.)
//! lives here alongside the channel types. The types were previously in
//! `hkask-types::loops` to break a circular dependency that has since been
//! resolved by deleting the Regulation subcrates (storage guard, SLO, seam
//! watcher); they have no Regulation-internal dependencies and their sole
//! consumer is `hkask-regulation`.
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
pub(crate) mod channels;

// Loop type system — actions, core, signals.
pub(crate) mod actions;
pub(crate) mod core;
pub(crate) mod signals;

pub(crate) use actions::{ActionType, RegulatoryAction};
pub(crate) use actions::{BudgetOption, RegulationData, RegulatoryActionParams};
pub use channels::CurationInput;
pub(crate) use core::ImpactReport;
pub(crate) use core::{ActionDecision, LoopId, LoopMetrics, TriggerOrigin};
pub(crate) use signals::Signal;
pub(crate) use signals::{Deviation, DeviationDirection, SignalMetric};
