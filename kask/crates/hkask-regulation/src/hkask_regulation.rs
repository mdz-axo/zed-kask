#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Regulation — Cybernetic Nervous System
//!
//! Homeostatic self-regulation: variety sensing, algedonic alerts, per-agent
//! tool-call caps, OCAP governance, sovereignty enforcement. Per Ashby's Law
//! of Requisite Variety.

pub(crate) mod algedonic;
pub(crate) mod cybernetics_loop;
pub(crate) mod dampener;
pub(crate) mod energy;

pub(crate) mod metacognition;
pub(crate) mod regulation_policy;
pub(crate) mod set_points;

pub(crate) mod loops;
pub(crate) mod sensor_provider;
pub(crate) mod strategy_evaluator;
pub(crate) mod system_simulator;

pub(crate) mod runtime;
pub use algedonic::{AlertEmailSink, AlertEscalationSink, RuntimeAlert};
pub use cybernetics_loop::{CyberneticsLoop, RolloutEventError, RolloutEventSource};
pub use energy::{CallMeterOutcome, DEFAULT_RUNAWAY_CALL_CEILING};
pub use metacognition::{AlertEvent, AlertSink, HealthSnapshot, MetacognitionLoop};

pub use loops::CurationInput;
pub use loops::signals::Signal;
pub use loops::{
    DistinctionState, LivenessTrust, LoopFailureDistinctions, LoopModel, LoopView, OutcomeTrust,
    Reading, SenseReading, StageActions, TriggerOrigin,
};
pub use regulation_policy::alert_condition;
pub use runtime::NoopEventSink;
pub use runtime::RegulationLedger;
pub use sensor_provider::ContextServerHealthSource;
pub use sensor_provider::InferenceHealthSource;
pub use sensor_provider::MemoryHealthSource;
pub use sensor_provider::OcrHealthError;
pub use sensor_provider::OcrHealthSource;
pub use set_points::{DEFAULT_VARIETY_MAX_DEFICIT, SetPoints, load_set_points};
