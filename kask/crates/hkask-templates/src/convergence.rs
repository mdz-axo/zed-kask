//! Convergence tracking — simplified.
//!
//! After each cascade iteration, reads `convergence_signal` from the context.
//! If signal < threshold, converge. If iteration >= max, maxed out.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceStatus {
    Continue,
    Converged,
    MaxedOut,
    Escalated,
}

pub struct ConvergenceTracker {
    max_iterations: u32,
    min_iterations: u32,
    threshold: f64,
    iteration: u32,
}

impl ConvergenceTracker {
    pub fn new(max_iterations: u32, min_iterations: u32, threshold: f64) -> Self {
        Self {
            max_iterations,
            min_iterations,
            threshold,
            iteration: 0,
        }
    }

    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    pub fn check(&mut self, context: &serde_json::Map<String, Value>) -> ConvergenceStatus {
        self.iteration += 1;

        if self.iteration >= self.max_iterations {
            return ConvergenceStatus::MaxedOut;
        }

        if self.iteration < self.min_iterations {
            return ConvergenceStatus::Continue;
        }

        let signal = context.get("convergence_signal").and_then(|v| v.as_f64());

        match signal {
            Some(s) if s < self.threshold => ConvergenceStatus::Converged,
            _ => ConvergenceStatus::Continue,
        }
    }
}
