//! hKask 4-Loop Architecture — loop type system.
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
//! | 2 | Memory | Coordination (unified store) | Domain |
//! | 5 | Curation | Metasystem (observer) | Meta |
//! | 6 | Cybernetics | Homeostatic regulation | Meta |
//!
//! **Bridge:**
//! - Memory consolidation: confidence-based cleanup + budget pruning
//!
//! **Authority DAG:** Curation → Cybernetics → {Inference, Memory}
//! No sideways edges. Authority flows downward.

// Loop type system — actions, core, signals.
pub(crate) mod actions;
pub(crate) mod core;
pub(crate) mod signals;

pub(crate) use actions::{ActionType, RegulatoryAction};
pub(crate) use actions::{BudgetOption, RegulationData, RegulatoryActionParams};
pub use core::CurationInput;
pub use core::{LoopView, Reading, SenseReading, LoopModel, OutcomeTrust, LivenessTrust};
pub(crate) use core::ImpactReport;
pub(crate) use core::{ActionDecision, LoopId, LoopMetrics, TriggerOrigin};
pub(crate) use signals::Signal;
pub(crate) use signals::{Deviation, DeviationDirection, SignalMetric};
