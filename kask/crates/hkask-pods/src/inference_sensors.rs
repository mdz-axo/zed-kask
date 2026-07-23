//! Inference loop sensors — Sensor implementations for InferenceLoop metrics.
//!
//! These sensors replace the inline `sense()` method in `InferenceLoop`. They share
//! state with the loop via `Arc<InferenceSensorState>`, allowing the sensors to
//! read the loop's state without the loop needing to contain sensing logic.
//!
//! This is part of the unified sensor catalog (ADR-056 Phase 4) — all sensing
//! flows through `Sensor` implementations registered with the
//! `SensorRegistry`, enabling centralized monitoring and management.

use hkask_regulation::sensor_provider::Sensor;
use hkask_regulation::types::loops::{LoopId, Signal, SignalMetric};
use hkask_types::CircuitBreakerPort;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Gas budget set-point: when gas remaining drops below this ratio,
/// the loop self-throttles via `AdjustEnergyBudget`.
pub const GAS_SET_POINT: f64 = 0.2;

/// Shared state between `InferenceLoop` and its sensors.
///
/// The loop writes to this state (via `sync_gas_state`, `set_model`, etc.),
/// and the sensors read from it during `sense()`. This decouples sensing
/// from the loop's domain logic.
#[derive(Default)]
pub struct InferenceSensorState {
    /// Gas remaining in the loop's own budget (atomic for lock-free reads).
    pub gas_remaining: Arc<AtomicU64>,
    /// Gas capacity — the budget cap set at allocation time.
    pub gas_cap: AtomicU64,
    /// Currently active inference model (None = not yet selected / unavailable).
    /// Stored as an atomic flag (false = none, true = set) for lock-free reads.
    pub model_available: std::sync::atomic::AtomicBool,
}

impl InferenceSensorState {
    /// Create new shared state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sync gas state from the authoritative L6 budget.
    pub fn sync_gas(&self, remaining: u64, cap: u64) {
        self.gas_remaining.store(remaining, Ordering::Relaxed);
        self.gas_cap.store(cap, Ordering::Relaxed);
    }

    /// Set whether a model is currently available.
    pub fn set_model_available(&self, available: bool) {
        self.model_available.store(available, Ordering::Relaxed);
    }

    /// Get the gas remaining ratio (0.0–1.0).
    pub fn gas_ratio(&self) -> f64 {
        let cap = self.gas_cap.load(Ordering::Relaxed);
        if cap == 0 {
            return 1.0; // No budget allocated — report full to avoid spurious throttling
        }
        let remaining = self.gas_remaining.load(Ordering::Relaxed);
        remaining as f64 / cap as f64
    }
}

/// Senses circuit breaker state (0.0=closed, 1.0=open, 0.5=half-open).
pub struct CircuitBreakerStateSensor {
    circuit_breaker: Option<Arc<dyn CircuitBreakerPort>>,
}

impl CircuitBreakerStateSensor {
    pub fn new(circuit_breaker: Option<Arc<dyn CircuitBreakerPort>>) -> Self {
        Self { circuit_breaker }
    }
}

#[async_trait::async_trait]
impl Sensor for CircuitBreakerStateSensor {
    async fn sense(&self) -> Option<Signal> {
        let state_value = match &self.circuit_breaker {
            Some(cb) => match cb.state() {
                hkask_types::CircuitState::Closed => 0.0,
                hkask_types::CircuitState::Open => 1.0,
                hkask_types::CircuitState::HalfOpen => 0.5,
            },
            None => 0.0, // No circuit breaker means always closed
        };
        Some(Signal::new(
            LoopId::Inference,
            SignalMetric::CircuitBreakerState,
            state_value,
            0.0,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::CircuitBreakerState)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Inference)
    }
}

/// Senses inference availability (1.0 if circuit breaker allows, 0.0 if not).
pub struct InferenceAvailableSensor {
    circuit_breaker: Option<Arc<dyn CircuitBreakerPort>>,
}

impl InferenceAvailableSensor {
    pub fn new(circuit_breaker: Option<Arc<dyn CircuitBreakerPort>>) -> Self {
        Self { circuit_breaker }
    }
}

#[async_trait::async_trait]
impl Sensor for InferenceAvailableSensor {
    async fn sense(&self) -> Option<Signal> {
        let available = match &self.circuit_breaker {
            Some(cb) => {
                if cb.allow_request() {
                    1.0
                } else {
                    0.0
                }
            }
            None => 1.0, // No circuit breaker means always available
        };
        Some(Signal::new(
            LoopId::Inference,
            SignalMetric::InferenceAvailable,
            available,
            1.0,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::InferenceAvailable)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Inference)
    }
}

/// Senses inference gas remaining ratio.
pub struct InferenceGasRemainingSensor {
    state: Arc<InferenceSensorState>,
}

impl InferenceGasRemainingSensor {
    pub fn new(state: Arc<InferenceSensorState>) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl Sensor for InferenceGasRemainingSensor {
    async fn sense(&self) -> Option<Signal> {
        Some(Signal::new(
            LoopId::Inference,
            SignalMetric::InferenceGasRemaining,
            self.state.gas_ratio(),
            GAS_SET_POINT,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::InferenceGasRemaining)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Inference)
    }
}

/// Senses inference model availability (1.0 if model is set, 0.0 if not).
pub struct InferenceModelAvailableSensor {
    state: Arc<InferenceSensorState>,
}

impl InferenceModelAvailableSensor {
    pub fn new(state: Arc<InferenceSensorState>) -> Self {
        Self { state }
    }
}

#[async_trait::async_trait]
impl Sensor for InferenceModelAvailableSensor {
    async fn sense(&self) -> Option<Signal> {
        let model_available = if self.state.model_available.load(Ordering::Relaxed) {
            1.0
        } else {
            0.0
        };
        Some(Signal::new(
            LoopId::Inference,
            SignalMetric::InferenceModelAvailable,
            model_available,
            1.0,
        ))
    }

    fn metric(&self) -> Option<SignalMetric> {
        Some(SignalMetric::InferenceModelAvailable)
    }

    fn loop_id(&self) -> Option<LoopId> {
        Some(LoopId::Inference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn circuit_breaker_state_sensor_no_cb() {
        let sensor = CircuitBreakerStateSensor::new(None);
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.metric, SignalMetric::CircuitBreakerState);
        assert_eq!(signal.value, 0.0);
        assert_eq!(signal.source, LoopId::Inference);
    }

    #[tokio::test]
    async fn inference_available_sensor_no_cb() {
        let sensor = InferenceAvailableSensor::new(None);
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.metric, SignalMetric::InferenceAvailable);
        assert_eq!(signal.value, 1.0);
    }

    #[tokio::test]
    async fn inference_gas_remaining_sensor_no_budget() {
        let state = Arc::new(InferenceSensorState::new());
        let sensor = InferenceGasRemainingSensor::new(state);
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.metric, SignalMetric::InferenceGasRemaining);
        // No budget allocated — should report full (1.0)
        assert_eq!(signal.value, 1.0);
    }

    #[tokio::test]
    async fn inference_gas_remaining_sensor_with_budget() {
        let state = Arc::new(InferenceSensorState::new());
        state.sync_gas(50, 100);
        let sensor = InferenceGasRemainingSensor::new(Arc::clone(&state));
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.value, 0.5);
    }

    #[tokio::test]
    async fn inference_model_available_sensor() {
        let state = Arc::new(InferenceSensorState::new());
        state.set_model_available(true);
        let sensor = InferenceModelAvailableSensor::new(state);
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.metric, SignalMetric::InferenceModelAvailable);
        assert_eq!(signal.value, 1.0);
    }

    #[tokio::test]
    async fn inference_model_unavailable_sensor() {
        let state = Arc::new(InferenceSensorState::new());
        state.set_model_available(false);
        let sensor = InferenceModelAvailableSensor::new(state);
        let signal = sensor.sense().await.unwrap();
        assert_eq!(signal.value, 0.0);
    }

    #[tokio::test]
    async fn sensor_metadata_returns_correct_metric_and_loop() {
        let state = Arc::new(InferenceSensorState::new());
        let gas_sensor = InferenceGasRemainingSensor::new(state);
        assert_eq!(
            gas_sensor.metric(),
            Some(SignalMetric::InferenceGasRemaining)
        );
        assert_eq!(gas_sensor.loop_id(), Some(LoopId::Inference));
    }
}
