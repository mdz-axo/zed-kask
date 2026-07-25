#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Regulation — Cybernetic Nervous System
//!
//! Homeostatic self-regulation: variety sensing, algedonic alerts, energy budgets,
//! OCAP governance, sovereignty enforcement. Per Ashby's Law of Requisite Variety.

#![allow(unused_crate_dependencies)] // hkask-storage used in wallet_manager.rs #[cfg(test)]

pub(crate) mod algedonic; // Loop 6 subloop 6.4 — algedonic signal channel
pub(crate) mod calibrator; // Shared calibration loop trait + spawn function
pub mod cybernetics_loop; // Loop 6
pub(crate) mod dampener; // Loop 6 — regulation
pub mod energy; // Loop 6 — energy budgets (hJoules)
pub mod energy_budget_management; // Loop 6 — energy budget registration/reservation/settlement
pub mod energy_estimator; // Loop 6 — tool cost estimation trait

pub(crate) mod inference_estimator;
pub mod infra_span;
pub mod meta_span;
pub mod qa_span;
pub(crate) mod regulation_policy; // Loop 6 — per-metric regulation rules
pub mod set_points; // Loop 6 — set-points config & loaders

pub mod sensor_provider; // Pluggable metric sensors (Fermi Extractor pattern) — public for cross-loop registration
pub(crate) mod strategy_evaluator; // Loop 6 — multi-model strategy selection (Fermi improvement-loop pattern)
pub(crate) mod system_simulator; // Loop 6 — predictive regulation via digital twin (Fermi dynamics pattern)
pub(crate) mod tool_stats; // Loop 6 — statistical learning for MCP tool costs and reliability
pub mod types; // Loop 6 → Inference energy estimation

pub mod runtime; // Loop 6 — runtime
pub mod runtime_policy; // Layer 6 — runtime action policy (VeriGuard + AgentGuard)
// StorageGuard merged into hkask-services-context::storage_guard module.
// Consumers should use hkask-services-context::storage_guard.
pub mod agent_wallet_store;
pub(crate) mod table_energy_estimator; // Per-server energy cost table
pub mod wallet_budget; // Loop 6 — wallet-backed energy budgets (Phase 5)
pub mod wallet_manager;
pub mod well;
pub use algedonic::{AlertEmailSink, RuntimeAlert};
pub use energy::{AgentGasStatus, DEFAULT_GAS_ALERT_THRESHOLD, GasBudget, GasCost, GasError};
pub use energy_budget_management::GasBudgetManager;
pub use energy_estimator::EnergyEstimator;

pub use hkask_types::regulation::QueueDepth;
pub use infra_span::InfraSpan;
pub use qa_span::QaSpan;
pub use runtime::NoopEventSink;
pub use runtime::RegulationLedger;
pub use runtime_policy::{DefaultPolicy, PolicyConfig, PolicyVerdict, RuntimePolicy};
pub use sensor_provider::{
    EnergyBudgetSensor, Sensor, SensorBus, SensorRegistry, ToolReliabilitySensor, VarietySensor,
    WalletBalanceRatioSensor, WalletKeyHealthSensor,
};
pub use set_points::{
    DEFAULT_COMMUNICATION_BACKPRESSURE_THRESHOLD, DEFAULT_CONNECTOR_LATENCY_MAX_SECS,
    DEFAULT_ENERGY_MIN_REMAINING_RATIO, DEFAULT_ERROR_RATE_MAX, DEFAULT_MAX_ITERATIONS,
    DEFAULT_VARIETY_MAX_DEFICIT, InferenceThrottleMode, SetPoints, SetPointsConfig,
    load_set_points,
};
pub use tool_stats::ToolStats;
pub use types::loops::{CurationInput, ExperienceClassification, RegulationLoop, RegulatoryAction};
pub use wallet_budget::WalletBackedBudget;
