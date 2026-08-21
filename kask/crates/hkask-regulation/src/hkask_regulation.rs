#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Regulation — Cybernetic Nervous System
//!
//! Homeostatic self-regulation: variety sensing, algedonic alerts, per-agent
//! tool-call caps, OCAP governance, sovereignty enforcement. Per Ashby's Law
//! of Requisite Variety.

pub(crate) mod algedonic;
pub mod cybernetics_loop;
pub(crate) mod dampener;
pub mod energy;

pub mod metacognition;
pub(crate) mod regulation_policy;
pub mod set_points;

pub mod loops;
pub mod sensor_provider;
pub(crate) mod strategy_evaluator;
pub(crate) mod system_simulator;
pub(crate) mod tool_stats;

pub mod runtime;
pub use algedonic::{AlertEmailSink, AlertEscalationSink, RuntimeAlert};
pub use cybernetics_loop::{CyberneticsLoop, RolloutEventError, RolloutEventSource};
pub use energy::{CallCap, CallMeterOutcome, DEFAULT_RUNAWAY_CALL_CEILING};
pub use metacognition::{
    AlertEvent, AlertSink, EscalationAlert, HealthSnapshot, MetacognitionLoop,
};

pub use hkask_types::regulation::QueueDepth;
pub use loops::CurationInput;
pub use loops::RegulatoryAction;
pub use runtime::NoopEventSink;
pub use runtime::RegulationLedger;
pub use set_points::{DEFAULT_VARIETY_MAX_DEFICIT, SetPoints, load_set_points};
pub use tool_stats::ToolStats;
