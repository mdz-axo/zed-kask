//! Unified domain error hierarchy for hKask service operations.
//! # REQ: P8 (Semantic Grounding) — every error variant is a distinct semantic state.
//! expect: "Every service error variant represents a distinct semantic state"
//!
//! `ServiceError` composes from all domain crate errors. Surface layers
//! (CLI, API) use `ServiceError` directly — CLI commands return
//! `ServiceError`, API routes return `ServiceErrorResponse` (a newtype
//! implementing Axum's `IntoResponse`). No surface-specific error enums.
//!
//! - CLI: commands return `Result<_, ServiceError>`, rendered via `Display`
//! - API: routes return `Result<_, ServiceErrorResponse>`, mapped to HTTP
//!   status codes via `From<ServiceError> for ApiError`
//!
//! MCP servers continue using `anyhow` for isolated process errors and do
//! NOT depend on this crate.
//!
//! # Design principles
//!
//! - Every variant is either a `#[from]` transparent wrapper around a domain
//!   crate error, or a sentinel String variant for user-facing input errors
//!   that have no upstream typed source.
//! - Surface types (`Json<T>`, HTTP status codes, `println!` formatting)
//!   NEVER appear in `ServiceError` — those belong in surface adapters.
//! - The error hierarchy is flat, not nested: no `ServiceError::Curator(..)`
//!   wrapper around `CuratorError`. Instead, the domain errors that
//!   `CuratorError` wraps appear directly as `ServiceError` variants.
//! - `ServiceError` does NOT depend on surface types (CLI errors, API errors).
//!   Dependency direction: surface → service → domain. Never the reverse.
//!
//! # Module layout
//!
//! - `mod.rs` (this file) — enum definition, `From` impls, `Display`
//! - `retryable.rs` — `is_retryable()` logic

use thiserror::Error;

use hkask_types::InfrastructureError;
use hkask_types::McpErrorKind;
use hkask_types::{EmbeddingGenerationError, InferenceError};

// ── Helper implementation modules ─────────────────────────────────────

mod message_key;
mod retryable;

/// Discriminates error semantics for HTTP status code mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    Conflict,
    Forbidden,
    BadRequest,
    /// Transient infrastructure condition — the service is temporarily
    /// unavailable (e.g., inference provider down, rate-limited).
    ServiceUnavailable,
}

/// Unified domain error for all service operations.
///
/// This replaces the 7 CLI error enums and the API `ApiError` as the single
/// canonical error type for business logic. Surface adapters translate
/// `ServiceError` into presentation format (terminal output, HTTP response).
///
/// In v0.32, the 46 single-domain variants were consolidated into 5
/// general-purpose variants:
/// - `Domain` for typed domain errors with semantic `ErrorKind` + `DomainKind`
/// - `ModelService` for inference/embedding errors with retryability
/// - `McpTool` for out-of-process MCP tool failures
/// - `Infra` for infrastructure errors (IO, lock poisoning)
/// - `InvalidWebID` for malformed WebID identifiers
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Typed domain error with semantic ErrorKind + origin DomainKind.
    ///
    /// Surface layers map `(domain, kind)` to HTTP status codes, CLI
    /// formatting, and Regulation regulation record emission.
    #[error("{kind:?} ({domain:?}): {message}")]
    Domain {
        kind: ErrorKind,
        domain: DomainKind,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Inference / embedding — carries retryability for Regulation energy budget.
    ///
    /// When `retryable` is true, `kind()` returns `ServiceUnavailable`
    /// regardless of the explicit `kind` field.
    #[error("Model service error: {message}")]
    ModelService {
        kind: ErrorKind,
        message: String,
        retryable: bool,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// MCP tool call failed. Carries the semantic error kind for retryability
    /// and Regulation observability. The `server` and `tool` fields identify the
    /// failing MCP server and tool for debugging.
    #[error("{kind}: {message} (server={server}, tool={tool})")]
    McpTool {
        kind: McpErrorKind,
        server: String,
        tool: String,
        message: String,
    },

    /// Upstream infrastructure error (lock poisoning, IO, etc.).
    #[error(transparent)]
    Infra(#[from] InfrastructureError),

    /// Invalid UUID format for WebID parsing.
    #[error("Invalid WebID: {message}")]
    InvalidWebID {
        #[source]
        source: Option<uuid::Error>,
        message: String,
    },
}

// ── Domain classification ────────────────────────────────────────────

/// Top-level domain for error routing and observability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainKind {
    Agent,
    Consent,
    Curator,
    Inference,
    Infrastructure,
    Memory,
    Pod,
    Storage,
    User,
    Wallet,
    /// MCP tool invocations (out-of-process tool servers). Distinct from `Skill`
    /// (agent capability management) and `Wallet` (economic balance).
    Mcp,
    /// Skill registry operations: discovery, publishing, auditing, bundle composition.
    Skill,
}

impl ServiceError {
    /// Classify this error into its top-level domain.
    pub fn domain(&self) -> DomainKind {
        match self {
            ServiceError::Domain { domain, .. } => *domain,
            ServiceError::ModelService { .. } => DomainKind::Inference,
            ServiceError::McpTool { .. } => DomainKind::Mcp,
            ServiceError::Infra(_) => DomainKind::Infrastructure,
            ServiceError::InvalidWebID { .. } => DomainKind::User,
        }
    }

    /// Return the semantic ErrorKind for HTTP status mapping.
    pub fn kind(&self) -> ErrorKind {
        match self {
            ServiceError::Domain { kind, .. } => *kind,
            ServiceError::ModelService {
                retryable: true, ..
            } => ErrorKind::ServiceUnavailable,
            ServiceError::ModelService { kind, .. } => *kind,
            ServiceError::McpTool { kind, .. } => match kind {
                McpErrorKind::NotFound => ErrorKind::NotFound,
                McpErrorKind::PermissionDenied => ErrorKind::Forbidden,
                McpErrorKind::Unavailable | McpErrorKind::Timeout | McpErrorKind::RateLimited => {
                    ErrorKind::ServiceUnavailable
                }
                _ => ErrorKind::BadRequest,
            },
            ServiceError::Infra(_) => ErrorKind::ServiceUnavailable,
            ServiceError::InvalidWebID { .. } => ErrorKind::Forbidden,
        }
    }
}

// ── From impls ──────────────────────────────────────────────────────
//
// Domain crate error conversions use explicit ServiceError::Variant
// construction rather than blanket From impls, keeping hkask-services-core
// decoupled from domain crates.

impl From<InferenceError> for ServiceError {
    fn from(e: InferenceError) -> Self {
        let retryable = matches!(
            e,
            InferenceError::Connection(_) | InferenceError::CircuitOpen(_)
        );
        let kind = if retryable {
            ErrorKind::ServiceUnavailable
        } else {
            ErrorKind::BadRequest
        };
        ServiceError::ModelService {
            kind,
            source: None,
            message: e.to_string(),
            retryable,
        }
    }
}
impl From<EmbeddingGenerationError> for ServiceError {
    fn from(e: EmbeddingGenerationError) -> Self {
        let retryable = matches!(
            e,
            EmbeddingGenerationError::Connection(_) | EmbeddingGenerationError::Api(..)
        );
        let kind = if retryable {
            ErrorKind::ServiceUnavailable
        } else {
            ErrorKind::BadRequest
        };
        ServiceError::ModelService {
            kind,
            source: None,
            message: e.to_string(),
            retryable,
        }
    }
}

impl From<uuid::Error> for ServiceError {
    fn from(e: uuid::Error) -> Self {
        let msg = e.to_string();
        ServiceError::InvalidWebID {
            source: Some(e),
            message: msg,
        }
    }
}

impl<T> From<std::sync::PoisonError<T>> for ServiceError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        ServiceError::Infra(hkask_types::InfrastructureError::LockPoisoned)
    }
}
