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

pub mod infra_span;
pub mod metacognition;
pub mod qa_span;
pub(crate) mod regulation_policy;
pub mod set_points;
pub mod skill_span;

pub mod sensor_provider;
pub(crate) mod strategy_evaluator;
pub(crate) mod system_simulator;
pub(crate) mod tool_stats;
pub mod types;

pub mod runtime;
pub use algedonic::{AlertEmailSink, AlertEscalationSink, RuntimeAlert};
pub use cybernetics_loop::CyberneticsLoop;
pub use energy::{
    AgentCallCapStatus, CallCap, CallCapError, CallCapManager, CallMeterOutcome,
    DEFAULT_CALL_CAP_ALERT_THRESHOLD, DEFAULT_RUNAWAY_CALL_CEILING,
};
pub use metacognition::{
    AlertEvent, AlertSink, EscalationAlert, EscalationTrigger, HealthSnapshot, MetacognitionConfig,
    MetacognitionLoop,
};

pub use hkask_types::regulation::QueueDepth;
pub use infra_span::InfraSpan;
pub use qa_span::QaSpan;
pub use runtime::NoopEventSink;
pub use runtime::RegulationLedger;
pub use runtime::StoredSkillSpan;
pub use sensor_provider::{
    EnergyBudgetSensor, Sensor, SensorBus, SensorRegistry, ToolReliabilitySensor, VarietySensor,
};
pub use set_points::{
    DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD, DEFAULT_CONNECTOR_LATENCY_MAX_SECS,
    DEFAULT_ENERGY_MIN_REMAINING_RATIO, DEFAULT_ERROR_RATE_MAX, DEFAULT_MAX_ITERATIONS,
    DEFAULT_VARIETY_MAX_DEFICIT, InferenceThrottleMode, SetPoints, SetPointsConfig,
    load_set_points,
};
pub use skill_span::SkillFeedbackSpan;
pub use tool_stats::ToolStats;
pub use types::loops::{CurationInput, ExperienceClassification, RegulatoryAction};
