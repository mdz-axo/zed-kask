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
pub mod channels;

// Loop type system — actions, core, signals.
pub mod actions;
pub mod core;
pub mod signals;

pub use actions::{ActionType, RegulatoryAction};
pub(crate) use actions::{BudgetOption, RegulationData, RegulatoryActionParams};
pub use channels::CurationInput;
pub use core::ImpactReport;
pub(crate) use core::{ActionDecision, LoopId, LoopMetrics, TriggerOrigin};
pub use signals::Signal;
pub(crate) use signals::{Deviation, DeviationDirection, SignalMetric};

// Backward-compatible re-export — CuratorDirective was previously re-exported
// from here but lives in hkask_types::curator.
pub use hkask_types::curator::CuratorDirective;
