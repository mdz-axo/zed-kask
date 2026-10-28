//! Fast, local inference resilience at the dispatch enforcement boundary.
//!
//! The circuit is deliberately a pure state machine. `LanguageModelInferencePort`
//! supplies wall-clock instants and owns synchronization, while tests can advance
//! time deterministically without sleeping.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferenceResilienceConfig {
    pub transient_failure_threshold: u32,
    pub open_duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CircuitState {
    Closed,
    Open { until: Instant },
    HalfOpen { probe_in_flight: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CircuitAdmission {
    Admitted { probe: bool },
    Rejected { retry_after: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CircuitTransition {
    Opened,
    HalfOpened,
    Closed,
    Reopened,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InferenceInterventionReceipt {
    pub id: u64,
    pub transition: CircuitTransition,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

pub(crate) struct InferenceCircuitObservation {
    pub state: CircuitState,
    pub receipts: Vec<InferenceInterventionReceipt>,
    pub permanent_failures: Vec<hkask_regulation::InferencePermanentFailureReceipt>,
    pub next_cursor: u64,
}

pub(crate) struct InferenceCircuit {
    config: InferenceResilienceConfig,
    state: CircuitState,
    consecutive_transient_failures: u32,
    next_event_id: u64,
    receipts: Vec<InferenceInterventionReceipt>,
    permanent_failures: Vec<hkask_regulation::InferencePermanentFailureReceipt>,
}

impl InferenceCircuit {
    pub(crate) fn new(mut config: InferenceResilienceConfig) -> Self {
        config.transient_failure_threshold = config.transient_failure_threshold.max(1);
        Self {
            config,
            state: CircuitState::Closed,
            consecutive_transient_failures: 0,
            next_event_id: 1,
            receipts: Vec::new(),
            permanent_failures: Vec::new(),
        }
    }

    /// expect: "A transient inference storm stops new work before it amplifies the outage"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: `now` is the dispatch layer's monotonic clock reading
    /// post: closed circuits admit; open circuits reject until cooldown; half-open admits one probe
    /// [P2] Constraining: Affirmative Consent — only bounded non-spending safety control is automatic
    pub(crate) fn admit_at(&mut self, now: Instant) -> CircuitAdmission {
        match self.state {
            CircuitState::Closed => CircuitAdmission::Admitted { probe: false },
            CircuitState::Open { until } if now < until => CircuitAdmission::Rejected {
                retry_after: until.saturating_duration_since(now),
            },
            CircuitState::Open { .. } => {
                self.state = CircuitState::HalfOpen {
                    probe_in_flight: true,
                };
                self.push_receipt(CircuitTransition::HalfOpened);
                CircuitAdmission::Admitted { probe: true }
            }
            CircuitState::HalfOpen {
                probe_in_flight: false,
            } => {
                self.state = CircuitState::HalfOpen {
                    probe_in_flight: true,
                };
                CircuitAdmission::Admitted { probe: true }
            }
            CircuitState::HalfOpen {
                probe_in_flight: true,
            } => CircuitAdmission::Rejected {
                retry_after: self.config.open_duration,
            },
        }
    }

    pub(crate) fn record_transient_failure_at(&mut self, now: Instant) {
        match self.state {
            CircuitState::Closed => {
                self.consecutive_transient_failures =
                    self.consecutive_transient_failures.saturating_add(1);
                if self.consecutive_transient_failures >= self.config.transient_failure_threshold {
                    self.state = CircuitState::Open {
                        until: now + self.config.open_duration,
                    };
                    self.consecutive_transient_failures = 0;
                    self.push_receipt(CircuitTransition::Opened);
                }
            }
            CircuitState::HalfOpen { .. } => {
                self.state = CircuitState::Open {
                    until: now + self.config.open_duration,
                };
                self.push_receipt(CircuitTransition::Reopened);
            }
            CircuitState::Open { .. } => {}
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.consecutive_transient_failures = 0;
        if matches!(self.state, CircuitState::HalfOpen { .. }) {
            self.state = CircuitState::Closed;
            self.push_receipt(CircuitTransition::Closed);
        }
    }

    pub(crate) fn cancel_probe(&mut self) {
        if matches!(
            self.state,
            CircuitState::HalfOpen {
                probe_in_flight: true
            }
        ) {
            self.state = CircuitState::HalfOpen {
                probe_in_flight: false,
            };
        }
    }

    /// Return one cursor-consistent view of circuit state and all receipt kinds.
    ///
    /// Both histories share `next_event_id`, so reading them under different lock
    /// acquisitions can advance past an event inserted between those reads.
    pub(crate) fn observe_since(&mut self, cursor: u64) -> InferenceCircuitObservation {
        self.receipts.retain(|receipt| receipt.id > cursor);
        self.permanent_failures
            .retain(|receipt| receipt.id > cursor);
        let receipts = self.receipts.clone();
        let permanent_failures = self.permanent_failures.clone();
        let next_cursor = receipts
            .iter()
            .map(|receipt| receipt.id)
            .chain(permanent_failures.iter().map(|receipt| receipt.id))
            .max()
            .unwrap_or(cursor);
        InferenceCircuitObservation {
            state: self.state,
            receipts,
            permanent_failures,
            next_cursor,
        }
    }

    pub(crate) fn record_permanent_failure(
        &mut self,
        kind: hkask_regulation::InferencePermanentFailureKind,
        detail: String,
    ) {
        let id = self.next_event_id;
        self.next_event_id = self.next_event_id.saturating_add(1);
        self.permanent_failures
            .push(hkask_regulation::InferencePermanentFailureReceipt {
                id,
                kind,
                detail,
                occurred_at: chrono::Utc::now(),
            });
    }

    fn push_receipt(&mut self, transition: CircuitTransition) {
        let id = self.next_event_id;
        self.next_event_id = self.next_event_id.saturating_add(1);
        self.receipts.push(InferenceInterventionReceipt {
            id,
            transition,
            occurred_at: chrono::Utc::now(),
        });
    }
}

#[derive(Clone)]
pub(crate) struct InferenceResilience {
    circuit: Arc<Mutex<InferenceCircuit>>,
}

impl InferenceResilience {
    pub(crate) fn new(config: InferenceResilienceConfig) -> Self {
        Self {
            circuit: Arc::new(Mutex::new(InferenceCircuit::new(config))),
        }
    }

    pub(crate) fn admit(&self) -> Result<InferenceCircuitPermit, Duration> {
        match self.lock().admit_at(Instant::now()) {
            CircuitAdmission::Admitted { probe } => Ok(InferenceCircuitPermit {
                resilience: self.clone(),
                probe,
                completed: false,
            }),
            CircuitAdmission::Rejected { retry_after } => Err(retry_after),
        }
    }

    pub(crate) fn observe_since(&self, cursor: u64) -> InferenceCircuitObservation {
        self.lock().observe_since(cursor)
    }

    fn lock(&self) -> MutexGuard<'_, InferenceCircuit> {
        self.circuit.lock().unwrap_or_else(|error| {
            tracing::warn!(
                target: "hkask.inference",
                "inference resilience mutex poisoned — recovering via into_inner"
            );
            error.into_inner()
        })
    }
}

pub(crate) struct InferenceCircuitPermit {
    resilience: InferenceResilience,
    probe: bool,
    completed: bool,
}

impl InferenceCircuitPermit {
    pub(crate) fn complete(
        mut self,
        succeeded: bool,
        transient_failure: bool,
        permanent_failure: Option<(hkask_regulation::InferencePermanentFailureKind, String)>,
    ) {
        {
            let mut circuit = self.resilience.lock();
            if let Some((kind, detail)) = permanent_failure {
                circuit.record_permanent_failure(kind, detail);
            }
            if succeeded {
                circuit.record_success();
            } else if transient_failure {
                circuit.record_transient_failure_at(Instant::now());
            } else if self.probe {
                circuit.cancel_probe();
            }
        }
        self.completed = true;
    }
}

impl Drop for InferenceCircuitPermit {
    fn drop(&mut self) {
        if self.probe && !self.completed {
            self.resilience.lock().cancel_probe();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CircuitAdmission, CircuitState, InferenceCircuit, InferenceResilienceConfig};
    use std::time::{Duration, Instant};

    /// expect: "A transient inference storm stops new work before it amplifies the outage"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the circuit is closed and the configured transient-failure threshold is reached
    /// post: the circuit is open, admission is rejected, and one intervention receipt exists
    /// [P2] Constraining: Affirmative Consent — only bounded non-spending safety control is automatic
    #[test]
    fn transient_failure_threshold_opens_circuit_and_rejects_admission() {
        let start = Instant::now();
        let mut circuit = InferenceCircuit::new(InferenceResilienceConfig {
            transient_failure_threshold: 3,
            open_duration: Duration::from_secs(30),
        });

        for offset in 0..3 {
            circuit.record_transient_failure_at(start + Duration::from_secs(offset));
        }

        assert!(matches!(
            circuit.observe_since(0).state,
            CircuitState::Open { .. }
        ));
        assert!(matches!(
            circuit.admit_at(start + Duration::from_secs(3)),
            CircuitAdmission::Rejected { .. }
        ));
        assert_eq!(circuit.observe_since(0).receipts.len(), 1);
    }

    /// expect: "Transition and permanent-failure receipts share one lossless observation cursor"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: both receipt kinds were recorded under one monotonically increasing event sequence
    /// post: one observation returns both kinds and advances to their maximum id without skipping either
    #[test]
    fn observation_reconciles_both_receipt_kinds_under_one_cursor() {
        let start = Instant::now();
        let mut circuit = InferenceCircuit::new(InferenceResilienceConfig {
            transient_failure_threshold: 1,
            open_duration: Duration::from_secs(30),
        });
        circuit.record_transient_failure_at(start);
        circuit.record_permanent_failure(
            hkask_regulation::InferencePermanentFailureKind::Authorization,
            "missing key".to_string(),
        );

        let observation = circuit.observe_since(0);

        assert_eq!(
            observation.state,
            CircuitState::Open {
                until: start + Duration::from_secs(30)
            }
        );
        assert_eq!(observation.receipts.len(), 1);
        assert_eq!(observation.permanent_failures.len(), 1);
        assert_eq!(observation.next_cursor, 2);
        let acknowledged = circuit.observe_since(observation.next_cursor);
        assert!(acknowledged.receipts.is_empty());
        assert!(acknowledged.permanent_failures.is_empty());
    }

    /// expect: "Inference recovery is probed once and returns the circuit to normal service"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: an open circuit's cooldown has elapsed
    /// post: one half-open probe is admitted, concurrent probes are rejected, and success closes
    #[test]
    fn half_open_admits_one_probe_and_success_closes_circuit() {
        let start = Instant::now();
        let mut circuit = InferenceCircuit::new(InferenceResilienceConfig {
            transient_failure_threshold: 1,
            open_duration: Duration::from_secs(10),
        });
        circuit.record_transient_failure_at(start);

        assert_eq!(
            circuit.admit_at(start + Duration::from_secs(10)),
            CircuitAdmission::Admitted { probe: true }
        );
        assert!(matches!(
            circuit.admit_at(start + Duration::from_secs(10)),
            CircuitAdmission::Rejected { .. }
        ));

        circuit.record_success();

        assert_eq!(circuit.observe_since(0).state, CircuitState::Closed);
        let transitions: Vec<_> = circuit
            .observe_since(0)
            .receipts
            .into_iter()
            .map(|receipt| receipt.transition)
            .collect();
        assert_eq!(
            transitions,
            vec![
                super::CircuitTransition::Opened,
                super::CircuitTransition::HalfOpened,
                super::CircuitTransition::Closed,
            ]
        );
    }
}
