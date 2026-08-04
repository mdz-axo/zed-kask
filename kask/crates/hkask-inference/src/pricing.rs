//! Inference USD cost — rJoule is USD-denominated (`1 rJoule = $1 USD`).
//!
//! The manifest executor charges each inference call's USD cost to the rJoule
//! budget (see `ManifestExecutor::execute_select`). This module computes that
//! cost from the token usage the provider returns × the model's per-token
//! price, the "API returns cost (via usage × price)" derivation path.
//!
//! # Price source
//!
//! Prices are resolved from an env price table, evaluated once per call (the
//! resolver is a pure function of `model + usage + env`):
//!
//! 1. **Per-model override** — `HKASK_PRICE_PROMPT_PER_1M_<MODEL>` and
//!    `HKASK_PRICE_COMPLETION_PER_1M_<MODEL>` (USD per 1 million tokens). The
//!    model id is sanitized for the env var: uppercase, non-alphanumerics → `_`.
//! 2. **Global fallback** — `HKASK_INFERENCE_PRICE_PROMPT_PER_1M` and
//!    `HKASK_INFERENCE_PRICE_COMPLETION_PER_1M` (USD per 1M tokens, applied to
//!    any model without a per-model override).
//! 3. **Unpriced** — if neither is set, the model is treated as free
//!    (`None` → not charged). Local models (Ollama) and unconfigured cloud
//!    models fall here, so a fresh deployment does not silently charge $0 for
//!    paid models it hasn't priced — it charges nothing until an operator sets
//!    a price, making the gap visible (the "advertised invariants need
//!    enforcement points" rule).
//!
//! # Future enhancement (not in this wiring)
//!
//! OpenRouter's `/v1/models` returns per-token `pricing` on each model
//! (`OpenRouterModel.pricing`). Wiring that cached registry as a third price
//! source (ahead of the env fallback) would auto-price OpenRouter models
//! without per-model env config. Deferred — the env table is the universal
//! source that also covers the bridge path (zed's `LanguageModel` does not
//! expose provider pricing) and non-OpenRouter providers (DeepInfra, Anthropic
//! direct). See `openrouter_backend::OpenRouterPricing`.
//!
//! # Other API service costs (NOT yet wired)
//!
//! This module prices **LLM inference** (`select` steps). MCP `execute` steps
//! that hit paid external APIs (web extraction, doc extraction, hosted
//! inference calls, etc.) are NOT yet charged rJoule through the executor.
//! The `hkask-mcp-media` server already self-gates its own rJoule (USD) budget
//! per call (`MediaBudget` / `charge_budget_gate`) — that per-MCP-server
//! pattern is the template for adding other API service costs later. TODO:
//! either extend the MCP tool response envelope with a `cost_usd` field the
//! executor charges, or have each paid MCP server implement its own
//! `MediaBudget`-style gate.

use hkask_types::InferenceUsage;

/// Sanitize a model id into an env-var suffix: uppercase, non-alphanumerics → `_`.
///
/// e.g. `"OpenRouter/z-ai/glm-5.2"` → `"OPENROUTER_Z_AI_GLM_5_2"`. Used for the
/// per-model override env vars `HKASK_PRICE_PROMPT_PER_1M_<SUFFIX>`.
fn model_env_suffix(model: &str) -> String {
    model
        .trim()
        .to_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Parse a non-negative finite `f64` from an env var, or `None` if unset,
/// unparseable, negative, or non-finite. Mirrors the media server's `env_f64`
/// discipline (deterministic, env-isolated) but returns `Option` (absent ≠ 0).
fn env_price(key: &str) -> Option<f64> {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
}

/// Resolve the per-1M-token USD prices for a model: `(prompt, completion)`.
///
/// Per-model override (`HKASK_PRICE_PROMPT_PER_1M_<MODEL>` /
/// `HKASK_PRICE_COMPLETION_PER_1M_<MODEL>`) wins over the global fallback
/// (`HKASK_INFERENCE_PRICE_PROMPT_PER_1M` / `HKASK_INFERENCE_PRICE_COMPLETION_PER_1M`).
/// Returns `None` if no prompt OR no completion price is configured for the
/// model — a half-priced model is treated as unpriced (not partially charged),
/// so the operator must set both to enable charging for that model.
pub fn model_price_per_1m_usd(model: &str) -> Option<(f64, f64)> {
    let suffix = model_env_suffix(model);
    let prompt = env_price(&format!("HKASK_PRICE_PROMPT_PER_1M_{suffix}"))
        .or_else(|| env_price("HKASK_INFERENCE_PRICE_PROMPT_PER_1M"))?;
    let completion = env_price(&format!("HKASK_PRICE_COMPLETION_PER_1M_{suffix}"))
        .or_else(|| env_price("HKASK_INFERENCE_PRICE_COMPLETION_PER_1M"))?;
    Some((prompt, completion))
}

/// Compute the USD cost of an inference call from token usage × the model's
/// per-token price. Returns `None` for unpriced models (free — not charged).
///
/// `cost = prompt_tokens × prompt_price/1e6 + completion_tokens × completion_price/1e6`
/// (prices are USD per 1M tokens). This is the value the manifest executor
/// passes to `BudgetTracker::charge_rjoule` (1 rJoule = $1 USD).
///
/// `expect`: "The system attributes USD cost to inference calls"
/// `pre`:  usage is the token counts the provider returned; model is the model id
/// `post`: returns Some(usd) when the model is priced, None when unpriced (free)
pub fn compute_cost_usd(model: &str, usage: &InferenceUsage) -> Option<f64> {
    let (prompt_per_1m, completion_per_1m) = model_price_per_1m_usd(model)?;
    let prompt_cost = (usage.prompt_tokens as f64) * prompt_per_1m / 1_000_000.0;
    let completion_cost = (usage.completion_tokens as f64) * completion_per_1m / 1_000_000.0;
    Some(prompt_cost + completion_cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env-var isolation: each test sets a unique suffix so concurrent test
    // threads don't collide on the global fallback keys. (The per-model keys
    // are unique per test; the global keys are only read when the per-model
    // override is absent, so tests that need the global fallback set it
    // themselves and accept that other threads may also read it — the values
    // they assert on come from their own per-model override, which wins.)
    fn isolate(suffix: &str) {
        // Best-effort: tests use per-model overrides with unique suffixes, so
        // they don't depend on the global keys. Nothing to clear (env is
        // process-global and can't be unset safely mid-test), but unique
        // suffixes make each test independent of the global fallback.
        let _ = suffix;
    }

    #[test]
    fn unpriced_model_returns_none() {
        isolate("UNPRICED");
        // No env vars set for this model → None (free, not charged).
        let usage = InferenceUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
        };
        // Only passes if neither per-model nor global keys are set to this
        // model's suffix. Use a uniquely-absurd suffix to guarantee absence.
        assert_eq!(
            compute_cost_usd("definitely-unpriced-model-xyz", &usage),
            None
        );
    }

    #[test]
    fn per_model_override_computes_cost() {
        // Use a unique suffix so this test doesn't depend on global keys.
        unsafe {
            std::env::set_var("HKASK_PRICE_PROMPT_PER_1M_TESTPRICED", "1.0");
            std::env::set_var("HKASK_PRICE_COMPLETION_PER_1M_TESTPRICED", "4.0");
        }
        let usage = InferenceUsage {
            prompt_tokens: 2_000_000,   // 2M prompt tokens → $2.00
            completion_tokens: 500_000, // 0.5M completion tokens → $2.00
            total_tokens: 2_500_000,
        };
        let cost = compute_cost_usd("testpriced", &usage).expect("priced model must compute cost");
        assert!(
            (cost - 4.0).abs() < 1e-9,
            "2M×$1/1M + 0.5M×$4/1M = $4.00, got {cost}"
        );
        // cleanup so other test runs are unaffected
        unsafe {
            std::env::remove_var("HKASK_PRICE_PROMPT_PER_1M_TESTPRICED");
            std::env::remove_var("HKASK_PRICE_COMPLETION_PER_1M_TESTPRICED");
        }
    }

    #[test]
    fn model_id_is_sanitized_for_env_suffix() {
        // "OpenRouter/z-ai/glm-5.2" → OPENROUTER_Z_AI_GLM_5_2
        assert_eq!(
            model_env_suffix("OpenRouter/z-ai/glm-5.2"),
            "OPENROUTER_Z_AI_GLM_5_2"
        );
        assert_eq!(model_env_suffix("  lower-case  "), "LOWER_CASE");
    }

    #[test]
    fn half_priced_model_is_unpriced() {
        // Only prompt set, no completion → None (don't partially charge).
        unsafe {
            std::env::set_var("HKASK_PRICE_PROMPT_PER_1M_HALFPRICED", "2.0");
            std::env::remove_var("HKASK_PRICE_COMPLETION_PER_1M_HALFPRICED");
        }
        let usage = InferenceUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 0,
            total_tokens: 1_000_000,
        };
        assert_eq!(
            compute_cost_usd("halfpriced", &usage),
            None,
            "a model missing either prompt or completion price must be unpriced (not partially charged)"
        );
        unsafe {
            std::env::remove_var("HKASK_PRICE_PROMPT_PER_1M_HALFPRICED");
        }
    }

    #[test]
    fn negative_or_nonfinite_price_ignored() {
        unsafe {
            std::env::set_var("HKASK_PRICE_PROMPT_PER_1M_BADPRICE", "-1.0");
            std::env::set_var("HKASK_PRICE_COMPLETION_PER_1M_BADPRICE", "NaN");
        }
        let usage = InferenceUsage {
            prompt_tokens: 100,
            completion_tokens: 100,
            total_tokens: 200,
        };
        assert_eq!(
            compute_cost_usd("badprice", &usage),
            None,
            "negative/NaN prices must be ignored (treated as unpriced), not charged"
        );
        unsafe {
            std::env::remove_var("HKASK_PRICE_PROMPT_PER_1M_BADPRICE");
            std::env::remove_var("HKASK_PRICE_COMPLETION_PER_1M_BADPRICE");
        }
    }
}
