#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Regulation — Cybernetic Nervous System
//!
//! Homeostatic self-regulation: variety sensing, algedonic alerts, energy budgets,
//! OCAP governance, sovereignty enforcement. Per Ashby's Law of Requisite Variety.

#![allow(unused_crate_dependencies)] // hkask-storage used in wallet_manager.rs #[cfg(test)]

pub(crate) mod algedonic; // Loop 6 subloop 6.4 — algedonic signal channel
pub mod cybernetics_loop; // Loop 6
pub(crate) mod dampener; // Loop 6 — regulation
pub mod energy; // Loop 6 — energy budgets (hJoules)
pub mod energy_budget_management; // Loop 6 — energy budget registration/reservation/settlement

pub mod infra_span;
pub mod metacognition;
pub mod qa_span;
pub(crate) mod regulation_policy; // Loop 6 — per-metric regulation rules
pub mod set_points;
pub mod skill_span; // Unified skill feedback spans (reg.skill.<id>.<phase>) // Loop 6 — set-points config & loaders

pub mod sensor_provider; // Pluggable metric sensors (Fermi Extractor pattern) — public for cross-loop registration
pub(crate) mod strategy_evaluator; // Loop 6 — multi-model strategy selection (Fermi improvement-loop pattern)
pub(crate) mod system_simulator; // Loop 6 — predictive regulation via digital twin (Fermi dynamics pattern)
pub(crate) mod tool_stats; // Loop 6 — statistical learning for MCP tool costs and reliability
pub mod types; // Loop 6 → Inference energy estimation

pub mod agent_wallet_store;
pub mod runtime; // Loop 6 — runtime
pub mod runtime_policy; // Layer 6 — runtime action policy (VeriGuard + AgentGuard)
pub mod wallet_manager;
pub mod well;
pub use algedonic::{AlertEmailSink, RuntimeAlert};
pub use cybernetics_loop::CyberneticsLoop;
pub use energy::{AgentGasStatus, DEFAULT_GAS_ALERT_THRESHOLD, GasBudget, GasCost, GasError};
pub use energy_budget_management::GasBudgetManager;
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
pub use runtime_policy::{DefaultPolicy, PolicyConfig, PolicyVerdict};
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
pub use types::loops::{CurationInput, ExperienceClassification, RegulationLoop, RegulatoryAction};
