#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Regulation — Cybernetic Nervous System
//!
//! Homeostatic self-regulation: variety sensing, algedonic alerts, energy budgets,
//! OCAP governance, sovereignty enforcement. Per Ashby's Law of Requisite Variety.

#![allow(unused_crate_dependencies)] // hkask-storage used in wallet_manager.rs #[cfg(test)]

pub mod acp_span;
pub(crate) mod algedonic; // Loop 6 subloop 6.4 — algedonic signal channel
pub mod api_metering; // API key metering — rate limits, Regulation spans, alerts
pub mod calibrated_energy_estimator; // Loop 6 — self-regulating per-server gas estimator
pub(crate) mod calibrator; // Shared calibration loop trait + spawn function
pub mod circuit_breaker; // Loop 6 — regulation
pub mod classify_span;
pub mod composite_energy_estimator; // Composite routing: inference → token-based, others → table
pub mod contract_span;
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
pub mod set_point_calibrator; // Loop 6 — self-tuning regulation thresholds

pub mod seam_span;
pub mod sensor_provider; // Pluggable metric sensors (Fermi Extractor pattern) — public for cross-loop registration
pub mod slo_span;
pub(crate) mod strategy_evaluator; // Loop 6 — multi-model strategy selection (Fermi improvement-loop pattern)
pub(crate) mod system_simulator; // Loop 6 — predictive regulation via digital twin (Fermi dynamics pattern)
pub(crate) mod tool_stats; // Loop 6 — statistical learning for MCP tool costs and reliability
pub mod types; // Loop 6 → Inference energy estimation

pub mod dynamic_gas_table;
pub mod gas_report;
pub mod runtime; // Loop 6 — runtime
pub mod runtime_policy; // Layer 6 — runtime action policy (VeriGuard + AgentGuard)
pub mod seam_types;
pub mod seam_watcher; // Public seam watcher — inventory, drift, Regulation spans
pub mod set_points; // Loop 6 — set-points config & loaders
pub mod slo_manager; // Loop 6 — SLO evaluation, error budgets, breach escalation
pub mod slo_types;
pub(crate) mod snapshot_loop; // Loop 6 — scheduled CAS snapshots
// StorageGuard merged into hkask-services-context::storage_guard module.
// Consumers should use hkask-services-context::storage_guard.
pub mod agent_wallet_store;
pub(crate) mod table_energy_estimator; // Per-server energy cost table
pub mod wallet_budget; // Loop 6 — wallet-backed energy budgets (Phase 5)
pub(crate) mod wallet_energy_estimator; // Loop 6 — wallet-aware energy estimation (Phase 5)
pub mod wallet_gas_calibrator;
pub mod wallet_manager;
pub mod well;
pub use algedonic::{AlertEmailSink, RuntimeAlert};
pub use api_metering::{
    ApiMeter, ApiMeteringAlert, ApiRequestSpan, EndpointWeight, RateLimitStatus, endpoint_weight,
};
pub use calibrated_energy_estimator::{
    CalibratedEnergyEstimator, DEFAULT_CALIBRATION_INTERVAL, DEFAULT_INITIAL_LOOKBACK,
};
pub use circuit_breaker::CircuitBreaker;
pub use composite_energy_estimator::CompositeEnergyEstimator;
pub use set_point_calibrator::{DEFAULT_SET_POINT_CALIBRATION_INTERVAL, SetPointCalibrator};
pub mod contract_events;
pub use acp_span::AcpSpan;
pub use classify_span::ClassifySpan;
pub use contract_events::{
    emit_contract_accepted, emit_contract_proposed, emit_contract_rejected, emit_contract_violated,
};
pub use contract_span::ContractSpan;
pub use cybernetics_loop::CyberneticsLoop;
pub use dynamic_gas_table::DynamicGasTable;
pub use energy::{
    AgentGasStatus, DEFAULT_GAS_ALERT_THRESHOLD, GasBudget, GasCost, GasDelta, GasError,
};
pub use energy_budget_management::GasBudgetManager;
pub use energy_estimator::EnergyEstimator;
pub use gas_report::{AgentGasReport, AgentGasSummary, GasReport, GasTotals, ToolGasBreakdown};

pub use hkask_types::curator::CurationThresholdConfig;
pub use hkask_types::regulation::QueueDepth;
pub use infra_span::InfraSpan;
pub use qa_span::QaSpan;
pub use runtime::NoopEventSink;
pub use runtime::RegulationLedger;
pub use runtime_policy::{DefaultPolicy, PolicyConfig, PolicyVerdict, RuntimePolicy};
pub use seam_span::SeamSpan;
pub use seam_types::{SeamCoverage, SeamInventory};
pub use seam_watcher::{SeamDrift, SeamSummary, SeamWatcher};
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
pub use slo_manager::{SloDataPoint, SloDataProvider, SloManager, SloManagerError};
pub use slo_span::SloSpan;
pub use slo_types::{SloDefinition, SloEvaluation, SloSeverity, seed_slos};
pub use snapshot_loop::{SnapshotLoop, SnapshotLoopConfig};
pub use tool_stats::ToolStats;
pub use types::loops::{CurationInput, ExperienceClassification, RegulationLoop, RegulatoryAction};
pub use wallet_budget::WalletBackedBudget;
pub use wallet_energy_estimator::WalletEnergyEstimator;
pub use wallet_gas_calibrator::{
    DEFAULT_WALLET_CALIBRATION_INTERVAL, DEFAULT_WALLET_INITIAL_LOOKBACK, WalletGasCalibrator,
};
