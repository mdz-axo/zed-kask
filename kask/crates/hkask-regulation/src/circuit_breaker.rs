//! Circuit Breaker — Cybernetics Regulation Function
//
//! Circuit breaking is a Regulation regulation mechanism: it enforces homeostatic
//! control over external service calls (e.g. inference) by preventing
//! cascading failures when downstream systems degrade. This is a Cybernetics
//! concern, not a templates concern — the Regulation governs when the system must
//! shed load to preserve stability (Ashby's Law of Requisite Variety).

use hkask_types::CircuitBreakerPort;
use hkask_types::regulation::CircuitState;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{error, info};

/// Default number of consecutive failures before opening the circuit.
pub(crate) const DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD: u32 = 5;

/// Default duration (in seconds) to keep the circuit open before attempting half-open.
pub(crate) const DEFAULT_CIRCUIT_BREAKER_OPEN_TIMEOUT_SECS: u64 = 60;

/// Default number of consecutive successes in half-open state to close the circuit.
pub(crate) const DEFAULT_CIRCUIT_BREAKER_SUCCESS_THRESHOLD: u32 = 2;

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub(crate) struct CircuitBreakerConfig {
    pub(crate) failure_threshold: u32,
    pub(crate) open_timeout: Duration,
    pub(crate) success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: DEFAULT_CIRCUIT_BREAKER_FAILURE_THRESHOLD,
            open_timeout: Duration::from_secs(DEFAULT_CIRCUIT_BREAKER_OPEN_TIMEOUT_SECS),
            success_threshold: DEFAULT_CIRCUIT_BREAKER_SUCCESS_THRESHOLD,
        }
    }
}

/// Circuit breaker for inference calls
pub struct CircuitBreaker {
    state: AtomicU8,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure_time: AtomicU64,
    /// Reference Instant for computing elapsed time (stored as nanos since creation).
    created_at: Instant,
    config: CircuitBreakerConfig,
    name: String,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given name and configuration.
    pub(crate) fn new(name: String, config: CircuitBreakerConfig) -> Self {
        let created_at = Instant::now();
        Self {
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU32::new(0),
            success_count: AtomicU32::new(0),
            last_failure_time: AtomicU64::new(0),
            created_at,
            config,
            name,
        }
    }

    /// Create a circuit breaker with inference-appropriate defaults.
    ///
    /// Suitable for wrapping inference calls: 5 failures to open,
    /// 60s open timeout, 2 successes to close from half-open.
    /// Create a default circuit breaker for inference.
    ///
    /// expect: "The system creates circuit breakers with safe default thresholds for inference calls"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — Regulation regulation loop enforces boundary
    /// \[P4\] Constraining: Clear Boundaries — default thresholds establish failure boundary
    /// pre:  name is non-empty
    /// post: returns CircuitBreaker with default thresholds
    pub fn default_for_inference(name: &str) -> Self {
        Self::new(name.to_string(), CircuitBreakerConfig::default())
    }

    /// Check if a request is allowed through the circuit breaker.
    ///
    /// expect: "I can check whether the circuit allows requests through"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — the check-before-execute gateway
    /// \[P4\] Constraining: Clear Boundaries — state-driven gating enforces the boundary
    /// post: returns true if circuit is closed or half-open, false if open
    #[must_use]
    pub fn allow_request(&self) -> bool {
        let state = self.state();

        let result = match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last_failure_nanos = self.last_failure_time.load(Ordering::Relaxed);
                if last_failure_nanos == 0 {
                    return false;
                }

                // Reconstruct the last-failure Instant from nanos since creation
                let last_failure_instant =
                    self.created_at + Duration::from_nanos(last_failure_nanos);
                let elapsed = Instant::now().duration_since(last_failure_instant);

                if elapsed >= self.config.open_timeout {
                    self.set_state(CircuitState::HalfOpen);
                    self.success_count.store(0, Ordering::Relaxed);
                    info!(
                        target: "hkask.circuit_breaker",
                        name = %self.name,
                        "Circuit transitioned to Half-Open"
                    );
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        };
        debug_assert!(
            result
                == (matches!(state, CircuitState::Closed | CircuitState::HalfOpen)
                    || state == CircuitState::Open),
            "postcondition: result matches circuit state gating"
        );
        result
    }

    /// Record a successful request.
    ///
    /// expect: "The circuit tracks successes and transitions back to closed when healthy"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — success count drives loop closure
    /// \[P4\] Constraining: Clear Boundaries — threshold-based state transition enforces boundary
    /// post: success counted, may transition circuit to closed
    pub fn record_success(&self) {
        let state = self.state();

        match state {
            CircuitState::HalfOpen => {
                let success_count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;

                if success_count >= self.config.success_threshold {
                    self.set_state(CircuitState::Closed);
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.success_count.store(0, Ordering::Relaxed);
                    info!(
                        target: "hkask.circuit_breaker",
                        name = %self.name,
                        "Circuit transitioned to Closed"
                    );
                }
            }
            CircuitState::Closed => {
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {}
        }
    }

    pub fn record_failure(&self) {
        let failure_count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;

        // Store nanoseconds since creation as the failure timestamp.
        // `as_nanos()` returns u128; the `as u64` truncation is safe because
        // u64 overflow at 2^64 nanos ≈ 584 years after creation.
        let elapsed_nanos = Instant::now().duration_since(self.created_at).as_nanos() as u64;
        self.last_failure_time
            .store(elapsed_nanos, Ordering::Relaxed);

        let state = self.state();

        if state == CircuitState::HalfOpen || failure_count >= self.config.failure_threshold {
            self.set_state(CircuitState::Open);
            self.failure_count.store(0, Ordering::Relaxed);
            error!(
                target: "hkask.circuit_breaker",
                name = %self.name,
                "Circuit transitioned to Open (failures: {})",
                self.config.failure_threshold
            );
        }
    }

    #[must_use]
    pub fn state(&self) -> CircuitState {
        match self.state.load(Ordering::Relaxed) {
            x if x == CircuitState::Closed as u8 => CircuitState::Closed,
            x if x == CircuitState::Open as u8 => CircuitState::Open,
            x if x == CircuitState::HalfOpen as u8 => CircuitState::HalfOpen,
            _ => CircuitState::Open, // fail-safe default
        }
    }

    fn set_state(&self, state: CircuitState) {
        self.state.store(state as u8, Ordering::Relaxed);
    }
}

impl CircuitBreakerPort for CircuitBreaker {
    fn allow_request(&self) -> bool {
        CircuitBreaker::allow_request(self)
    }

    fn record_success(&self) {
        CircuitBreaker::record_success(self)
    }

    fn record_failure(&self) {
        CircuitBreaker::record_failure(self)
    }

    fn state(&self) -> CircuitState {
        CircuitBreaker::state(self)
    }
}
