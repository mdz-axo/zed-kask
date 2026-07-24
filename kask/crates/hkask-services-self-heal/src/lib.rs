#![deny(unsafe_code)]
//! hKask Self-Healing Engine — two-stage autonomous error recovery.
//!
//! Extracted from `hkask-services-core` (see `tasks/plan-core-scope-contraction.md`,
//! Task 1.1). The engine maps error patterns to recovery strategies, executes
//! healing actions, and returns Healed (retry), Degraded (fallback), or
//! Unhealable (escalate to Curator).
//!
//! **Stage 1 (always available):** Deterministic env/config healing — `RunCommand`,
//! `SetEnv`, `LoadDotEnv`, `CreateDefaultFile`, `RetryWithBackoff`, `ProposeCodeChange`.
//! No inference required.
//!
//! **Stage 2 (requires `with_inference()`):** LLM template-assisted healing via
//! `LlmAssisted`. Renders a Jinja2 template from `registry/templates/heal/`,
//! calls the classifier model, parses JSON instructions, executes sub-actions.
//!
//! ## Module Structure
//!
//! - `types` — Core types: outcomes, contexts, strategies, actions
//! - `registry` — `HealRegistry` strategy catalog
//! - `healer` — `SelfHealer` engine
//! - `helpers` — Env value resolution, LLM response parsing

#![allow(private_interfaces)]

mod healer;
mod helpers;
mod registry;
#[cfg(test)]
mod tests;
mod types;

pub use healer::SelfHealer;
pub use registry::HealRegistry;
pub use types::{
    EnvValueSource, HealAction, HealContext, HealError, HealInferenceFn, HealOutcome, HealStrategy,
};
