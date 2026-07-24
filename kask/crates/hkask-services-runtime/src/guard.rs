//! Content safety guard — re-exported from `hkask-guard`.
//!
//! The canonical implementation lives in `crates/hkask-guard` and is aligned
//! with OWASP LLM Top 10. This module provides the re-export for consumers
//! that depend on `hkask-services-runtime`.
//!
//! P3.1 Social Generativity: core controls are mandatory at every LLM boundary.

pub use hkask_guard::{ContentGuard, GuardResult, GuardViolation};
