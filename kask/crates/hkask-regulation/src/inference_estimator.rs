//! InferenceEnergyEstimator — Token-based energy cost estimation for inference
//!
//! Implements `EnergyEstimator` using the token estimation heuristic:
//! prompt characters / 4 + max_tokens.
//! This estimator can be used with `GovernedTool` to govern inference
//! through the unified tool membrane.

use crate::energy_estimator::EnergyEstimator;
use serde_json::Value;

/// Characters per token heuristic (English text ≈ 4 chars/token).
const CHARS_PER_TOKEN: usize = 4;

/// Inference-specific gas estimator.
///
/// Estimates cost based on:
/// - `prompt_tokens` ≈ `prompt.len() / CHARS_PER_TOKEN` (from JSON args)
/// - `max_tokens` from JSON args
///
/// \[NORMATIVE\] For `GovernedTool` usage, the `args` Value should contain: (P8 — Semantic Grounding).
/// ```json
/// { "prompt": "...", "max_tokens": N }
/// ```rust,no_run
///
/// If args don't contain the expected fields, falls back to a flat cost of 1.
pub(crate) struct InferenceEnergyEstimator;

impl EnergyEstimator for InferenceEnergyEstimator {
    fn estimate_cost(&self, _server: &str, _tool: &str, args: &Value) -> u64 {
        // Try to extract prompt and max_tokens from args
        let prompt_chars = args
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);

        let max_tokens = args
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(100); // default max_tokens

        let prompt_tokens = prompt_chars as u64 / CHARS_PER_TOKEN as u64;
        let total = prompt_tokens + max_tokens;

        if total == 0 { 1 } else { total }
    }
}
