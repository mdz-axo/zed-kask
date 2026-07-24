//! Probabilistic contract verification — (p, δ, k)-satisfaction for
//! non-deterministic contracts (e.g., LLM output validation, style proximity).
//!
//! Based on probabilistic approximately correct (PAC) contract checking:
//! - p = target pass rate (0.0–1.0)
//! - δ = tolerance margin
//! - k = recovery window (extra attempts per trial)

/// Result of a probabilistic contract evaluation.
#[derive(Debug, Clone)]
pub struct ProbContractResult {
    /// Whether the contract passed (actual rate ≥ target rate - margin).
    pub passed: bool,
    /// Number of successful trials.
    pub successes: usize,
    /// Total number of trials attempted.
    pub trials: usize,
    /// Actual observed pass rate (successes / trials).
    pub actual_rate: f64,
    /// Target pass rate that was required.
    pub target_rate: f64,
}

/// Runner for probabilistic contract verification.
///
/// Evaluates a predicate over multiple randomly generated inputs and
/// checks whether the observed pass rate meets the target within tolerance.
/// The recovery window `k` allows up to `k` additional attempts per trial.
pub struct ProbContractRunner {
    target_rate: f64,
    margin: f64,
    recovery_window: u32,
}

impl ProbContractRunner {
    /// Create a new probabilistic contract runner.
    ///
    /// - `target_rate`: desired pass rate (0.0–1.0, e.g., 0.95 for 95%)
    /// - `margin`: acceptable deviation below target (e.g., 0.05)
    /// - `recovery_window`: extra attempts allowed per trial before counting as failure
    pub fn new(target_rate: f64, margin: f64, recovery_window: u32) -> Self {
        assert!(
            (0.0..=1.0).contains(&target_rate),
            "target_rate must be in [0.0, 1.0]"
        );
        assert!(
            (0.0..=1.0).contains(&margin),
            "margin must be in [0.0, 1.0]"
        );
        Self {
            target_rate,
            margin,
            recovery_window,
        }
    }

    /// Evaluate a contract over `trials` generated inputs.
    ///
    /// For each trial, calls `generator` to produce an input, then calls
    /// `predicate` on it. If the predicate fails, up to `recovery_window`
    /// additional attempts are made. The trial passes if any attempt succeeds.
    pub fn evaluate<T, G, P>(&self, trials: usize, generator: G, predicate: P) -> ProbContractResult
    where
        G: Fn() -> T,
        P: Fn(&T) -> bool,
    {
        let max_attempts = 1 + self.recovery_window as usize;
        let mut successes = 0usize;

        for _ in 0..trials {
            let mut passed = false;
            for _ in 0..max_attempts {
                let input = generator();
                if predicate(&input) {
                    passed = true;
                    break;
                }
            }
            if passed {
                successes += 1;
            }
        }

        let actual_rate = if trials > 0 {
            successes as f64 / trials as f64
        } else {
            1.0
        };
        let threshold = self.target_rate - self.margin;
        let passed = actual_rate >= threshold;

        ProbContractResult {
            passed,
            successes,
            trials,
            actual_rate,
            target_rate: self.target_rate,
        }
    }
}
