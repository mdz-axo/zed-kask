//! Retryability semantics for `ServiceError`.
//!
//! The Regulation energy budget needs to know whether retrying an operation will
//! consume gas for a potentially successful retry or waste gas on a
//! guaranteed failure. This module provides that signal.

use super::{ErrorKind, ServiceError};

impl ServiceError {
    /// Whether this error represents a transient condition that may succeed
    /// on retry (with backoff). Used by the Regulation gas budget to decide whether
    /// to allow retry loops.
    ///
    /// Retryable: network I/O, inference connection/timeout, circuit breaker
    /// open, rate limiting, external service unavailable.
    ///
    /// Non-retryable: not-found, invalid input, permission denied, database
    /// corruption, encryption failures, lock poisoning.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be a valid ServiceError variant
    /// post: returns true for retryable errors (network, rate-limit, keystore); false for non-retryable (not-found, validation, permission)
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            ServiceError::Domain { kind, .. } => {
                matches!(kind, ErrorKind::ServiceUnavailable)
            }
            ServiceError::ModelService { retryable, .. } => *retryable,
            ServiceError::McpTool { kind, .. } => kind.is_retryable(),
            ServiceError::Infra(e) => {
                matches!(e, hkask_types::InfrastructureError::Io(_))
            }
            ServiceError::InvalidWebID { .. } => false,
        }
    }
}
