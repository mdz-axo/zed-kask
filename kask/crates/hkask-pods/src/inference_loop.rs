//! Inference Loop — prompt → context → model → response → parse → act (Loop 1)
//!
//! Owns its energy budget reservation and tracks the active inference model.
//! Monitors circuit breaker state, gas consumption, and model availability.
//! Lives in `hkask-agents` because domain loops (Inference, Episodic, Semantic,
//! Communication, Curation) are domain logic — they belong with the agents crate.
//! Governance is applied externally via `GovernedTool` (in `hkask-regulation`) before
//! the port is passed to this loop.

use hkask_regulation::sensor_provider::SensorBus;
use hkask_regulation::types::loops::{
    ActionType, Deviation, DeviationDirection, LoopId, RegulationData, RegulationLoop,
    RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::inference_sensors::{
    CircuitBreakerStateSensor, InferenceAvailableSensor, InferenceGasRemainingSensor,
    InferenceModelAvailableSensor, InferenceSensorState,
};

/// Inference Loop — owns energy budget and model selection state.
///
/// Wraps an `InferencePort` and optional `CircuitBreakerPort` to provide
/// loop-level observability. Owns its own energy budget reservation (separate
/// from Cybernetics' global tracking) and tracks the active inference model.
///
/// When the circuit breaker is open or gas is depleted, the loop produces
/// `Throttle`/`AdjustEnergyBudget` actions targeting itself (self-throttle).
/// When the model is unavailable, it produces `Calibrate` to signal that
/// model selection is needed.
///
/// Sensing is delegated to `Sensor` implementations registered in
/// the `sensor_registry`. The loop shares state with its sensors via
/// `Arc<InferenceSensorState>`.
pub struct InferenceLoop {
    /// Shared state between the loop and its sensors.
    sensor_state: Arc<InferenceSensorState>,
    /// Sensor registry — holds the Sensor implementations for this loop.
    sensor_registry: SensorBus,
    /// Currently active inference model (None = not yet selected / unavailable).
    /// Kept in sync with `sensor_state.model_available`.
    current_model: Option<String>,
}

impl InferenceLoop {
    /// Create a new Inference Loop.
    pub fn new() -> Self {
        let sensor_state = Arc::new(InferenceSensorState::new());
        let sensor_registry = SensorBus::new();
        // Register all four inference sensors. The circuit breaker sensors
        // receive None (always closed / always available) — they can be
        // re-registered with a real circuit breaker via `with_circuit_breaker()`.
        sensor_registry.register(Arc::new(CircuitBreakerStateSensor::new(None)));
        sensor_registry.register(Arc::new(InferenceAvailableSensor::new(None)));
        sensor_registry.register(Arc::new(InferenceGasRemainingSensor::new(Arc::clone(
            &sensor_state,
        ))));
        sensor_registry.register(Arc::new(InferenceModelAvailableSensor::new(Arc::clone(
            &sensor_state,
        ))));
        Self {
            sensor_state,
            sensor_registry,
            current_model: None,
        }
    }
}

impl Default for InferenceLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl InferenceLoop {
    /// Set the energy budget for this loop.
    ///
    /// `cap` is the total gas allocation; `remaining` is the current balance.
    /// Both are stored in the shared sensor state so that sensors can emit
    /// the gas-remaining ratio.
    pub fn with_energy_budget(self, cap: u64, remaining: u64) -> Self {
        self.sensor_state.sync_gas(remaining, cap);
        self
    }

    /// Set the active inference model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.current_model = Some(model.into());
        self.sensor_state.set_model_available(true);
        self
    }

    /// Get the current gas remaining value (read-only sense signal).
    #[must_use]
    pub fn gas_remaining(&self) -> u64 {
        self.sensor_state.gas_remaining.load(Ordering::Relaxed)
    }

    /// Read-only accessor for the L1 domain metric.
    ///
    /// Returns `(remaining, cap)` — the loop's token budget state as a
    /// sense signal. The L6 budget (CyberneticsLoop's GasBudget) is the
    /// authoritative regulator; this counter is a read-only mirror.
    #[must_use]
    pub fn token_usage(&self) -> (u64, u64) {
        (
            self.sensor_state.gas_remaining.load(Ordering::Relaxed),
            self.sensor_state.gas_cap.load(Ordering::Relaxed),
        )
    }

    /// Sync this loop's gas counter from the authoritative L6 budget.
    ///
    /// Call after CyberneticsLoop gas operations (reserve, settle, replenish)
    /// to keep the L1 sense signal (`inference_gas_remaining`) in sync with
    /// the L6 regulatory budget. This is the ONLY way external code should
    /// update InferenceLoop's gas counter.
    pub fn sync_gas_state(&self, remaining: u64, cap: u64) {
        self.sensor_state.sync_gas(remaining, cap);
    }

    /// Get the energy budget cap.
    #[must_use]
    pub fn gas_cap(&self) -> u64 {
        self.sensor_state.gas_cap.load(Ordering::Relaxed)
    }

    // Explicit 4-stage cycle: sense → compare → compute → act

    /// **Sense stage** (sense → compare → compute → act):
    /// Read token budget remaining via `token_usage()`, check circuit breaker
    /// state, and verify model availability. Produces afferent signals for
    /// gas remaining ratio, circuit breaker state, and model availability.
    #[must_use]
    pub async fn sense(&self) -> Vec<Signal> {
        <Self as RegulationLoop>::sense(self).await
    }

    /// **Compare stage** (sense → compare → compute → act):
    /// Check if remaining gas is below the set-point ratio (0.2), whether
    /// the circuit breaker is open, or whether the model is unavailable.
    /// Detects deviations from healthy operating set-points.
    #[must_use]
    pub async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        <Self as RegulationLoop>::compare(self, signals).await
    }

    /// **Compute stage** (sense → compare → compute → act):
    /// Determine model selection / gas allocation based on deviations.
    /// Circuit breaker open or inference unavailable → Throttle. Gas below
    /// set-point → AdjustEnergyBudget (self-throttle). Model unavailable →
    /// Calibrate (signal model selection needed).
    #[must_use]
    pub async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        <Self as RegulationLoop>::compute(self, deviations).await
    }

    /// **Act stage** (sense → compare → compute → act):
    /// Execute inference call if budget allows. Logs all regulatory actions
    /// with structured spans. Gas self-throttle and model unavailability
    /// are logged at warn level.
    pub async fn act(&self, actions: &[RegulatoryAction]) {
        <Self as RegulationLoop>::act(self, actions).await
    }
}

#[async_trait::async_trait]
impl RegulationLoop for InferenceLoop {
    fn id(&self) -> LoopId {
        LoopId::Inference
    }

    /// Sense: delegate to the SensorBus.
    ///
    /// All sensing is now done through Sensor implementations:
    /// - `CircuitBreakerStateSensor` — 0.0=closed, 1.0=open, 0.5=half-open
    /// - `InferenceAvailableSensor` — 1.0 if circuit breaker allows, 0.0 if not
    /// - `InferenceGasRemainingSensor` — ratio of gas remaining in loop's budget
    /// - `InferenceModelAvailableSensor` — 1.0 if model is set, 0.0 if not
    async fn sense(&self) -> Vec<Signal> {
        self.sensor_registry.sense_all(LoopId::Inference).await
    }

    /// Compute: produce regulatory actions for detected deviations.
    ///
    /// Handles:
    /// - Circuit breaker open → `Throttle`
    /// - Inference unavailable → `Throttle`
    /// - Gas below set-point → `AdjustEnergyBudget` (self-throttle)
    /// - Model unavailable → `Calibrate` (signal model selection needed)
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        for dev in deviations {
            match dev.signal.metric {
                SignalMetric::CircuitBreakerState
                    if dev.direction == DeviationDirection::AboveSetPoint =>
                {
                    actions.push(RegulatoryAction::new(
                        LoopId::Inference,
                        ActionType::Throttle,
                        RegulatoryActionParams::reason("circuit_breaker_open"),
                    ));
                }
                SignalMetric::InferenceAvailable
                    if dev.direction == DeviationDirection::BelowSetPoint =>
                {
                    actions.push(RegulatoryAction::new(
                        LoopId::Inference,
                        ActionType::Throttle,
                        RegulatoryActionParams::reason("inference_unavailable"),
                    ));
                }
                SignalMetric::InferenceGasRemaining
                    if dev.direction == DeviationDirection::BelowSetPoint =>
                {
                    actions.push(RegulatoryAction::new(
                        LoopId::Inference,
                        ActionType::AdjustEnergyBudget,
                        RegulatoryActionParams::with_data(
                            "gas_below_set_point",
                            RegulationData::EnergyBudgetLow {
                                remaining_ratio: dev.signal.value,
                                set_point: dev.signal.set_point,
                            },
                        ),
                    ));
                }
                SignalMetric::InferenceModelAvailable
                    if dev.direction == DeviationDirection::BelowSetPoint =>
                {
                    actions.push(RegulatoryAction::new(
                        LoopId::Inference,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("model_unavailable"),
                    ));
                }
                _ => {}
            }
        }

        actions
    }

    /// Act: execute regulatory actions.
    ///
    /// Logs all actions with structured spans. Gas self-throttle and model
    /// unavailability are logged at warn level to surface budget depletion
    /// and model selection needs.
    async fn act(&self, actions: &[RegulatoryAction]) {
        for action in actions {
            match action.action_type {
                ActionType::AdjustEnergyBudget => {
                    tracing::warn!(
                        target: "reg.inference",
                        action_type = ?action.action_type,
                        target_loop = %action.target,
                        parameters = ?action.parameters,
                        "Inference Loop self-throttle: energy budget below set-point"
                    );
                }
                ActionType::Calibrate => {
                    tracing::warn!(
                        target: "reg.inference",
                        action_type = ?action.action_type,
                        target_loop = %action.target,
                        parameters = ?action.parameters,
                        "Inference Loop calibrate: model selection needed"
                    );
                }
                _ => {
                    tracing::info!(
                        target: "reg.inference",
                        action_type = ?action.action_type,
                        target_loop = %action.target,
                        "Inference Loop regulatory action"
                    );
                }
            }
        }
    }
}
