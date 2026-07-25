//! Error types — Cross-cutting infrastructure
//
//! Infrastructure errors used across all loops. Not loop-specific.!
//! Layered error architecture (Miller separation):
//! 1. `InfrastructureError` — cross-cutting transport errors (Database, Serialization,
//!    LockPoisoned, Io). No domain semantics. Passes through crate boundaries.
//! 2. Domain enums (e.g. `GoalRepositoryError`, `EmbeddingError`) — compose from
//!    `InfrastructureError` via `#[from]` and add only authority-bearing,
//!    recovery-path-significant variants.
//! 3. `ServiceError` (in `hkask-services`) — unified domain error vocabulary
//!    composing all domain errors for surface presentation.
//!
//! Rule: if a variant name appears in 3+ crates with identical semantics, it
//! belongs in `InfrastructureError`. If it carries domain-specific recovery
//! semantics, it stays in the domain enum.

use serde::{Deserialize, Serialize};
use std::sync::PoisonError;
use thiserror::Error;

// InfrastructureError — Cross-Crate Foundation

/// Classification for database errors — enables recovery-path
/// discrimination (retry connection, skip corrupt rows, alert on constraint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DatabaseErrorKind {
    Connection,
    Query,
    Constraint,
    Migration,
    Other,
}

impl std::fmt::Display for DatabaseErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection => write!(f, "connection"),
            Self::Query => write!(f, "query"),
            Self::Constraint => write!(f, "constraint"),
            Self::Migration => write!(f, "migration"),
            Self::Other => write!(f, "other"),
        }
    }
}

// DbProvider — Database provider enum (moved from hkask-database::types)
//
// Relocated to break the circular dependency: hkask-storage -> the wallet types crate
// -> hkask-database -> hkask-storage. DbError/DbProvider are pure types with no
// external deps beyond thiserror + serde (already in hkask-types).

/// Supported database providers.
///
/// New providers are added as enum variants. The `DatabaseDriver` trait
/// dispatches to the correct implementation at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DbProvider {
    /// SQLite / SQLCipher via rusqlite (default, stable).
    Sqlite,
    /// PostgreSQL via sqlx + pgvector.
    Postgres,
}

impl std::fmt::Display for DbProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite => write!(f, "sqlite"),
            Self::Postgres => write!(f, "postgres"),
        }
    }
}

/// Database operation errors — provider-agnostic.
///
/// Moved from hkask-database::types to break the storage/wallet-types/database
/// circular dependency. These types only depend on hkask-types itself.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("database: {0}")]
    Database(String),

    #[error("constraint violation: {0}")]
    Constraint(String),

    #[error("connection: {0}")]
    Connection(String),

    #[error("serialization: {0}")]
    Serialization(String),

    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),

    #[error("migration: {0}")]
    Migration(String),
}

impl From<DbError> for InfrastructureError {
    fn from(e: DbError) -> Self {
        match &e {
            DbError::Connection(_) => InfrastructureError::Database {
                message: e.to_string(),
                kind: DatabaseErrorKind::Connection,
            },
            DbError::Constraint(_) => InfrastructureError::Database {
                message: e.to_string(),
                kind: DatabaseErrorKind::Constraint,
            },
            DbError::Migration(_) => InfrastructureError::Database {
                message: e.to_string(),
                kind: DatabaseErrorKind::Migration,
            },
            _ => InfrastructureError::Database {
                message: e.to_string(),
                kind: DatabaseErrorKind::Other,
            },
        }
    }
}

impl From<InfrastructureError> for DbError {
    fn from(e: InfrastructureError) -> Self {
        DbError::Database(e.to_string())
    }
}

/// Generic infrastructure errors shared by every crate.
///
/// These are transport-layer failures — they carry no domain semantics.
/// Every domain enum may compose from this via `#[from]` to eliminate
/// the 87× repetition of `Database(String)` / `Serialization(String)` /
/// `LockPoisoned(String)` spread across the codebase.
///
/// Design constraint (C5): every variant is a distinct recovery category —
/// no catch-all, no `Other(String)`, no `Internal(String)`.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InfrastructureError {
    #[error("database({kind}): {message}")]
    Database {
        message: String,
        kind: DatabaseErrorKind,
    },

    #[error("serialization: {0}")]
    Serialization(String),

    #[error("lock poisoned")]
    LockPoisoned,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("io: {0}")]
    Io(String),
}

impl InfrastructureError {
    /// Create a Database error from a message string (for callers without rusqlite).
    pub fn database(message: impl Into<String>) -> Self {
        InfrastructureError::Database {
            message: message.into(),
            kind: DatabaseErrorKind::Other,
        }
    }

    /// Create a `NotFound` error with the canonical `NotFound` struct, preserving
    /// entity type and identifier. Prefer this over `NotFound(format!(...))` when
    /// the entity type is known — it enables downstream consumers to pattern-match
    /// on the structured data rather than parsing a string.
    pub fn not_found(entity_type: &'static str, id: impl Into<String>) -> Self {
        NotFound {
            entity_type: entity_type.to_string(),
            id: id.into(),
        }
        .into()
    }
}

// From impls for the canonical error sources.
// Note: the rusqlite From impl requires the "sql" feature (opt-in).
// Downstream crates without rusqlite should manually wrap errors
// into InfrastructureError::database(String).
impl From<serde_json::Error> for InfrastructureError {
    fn from(e: serde_json::Error) -> Self {
        InfrastructureError::Serialization(e.to_string())
    }
}

impl From<std::io::Error> for InfrastructureError {
    fn from(e: std::io::Error) -> Self {
        InfrastructureError::Io(e.to_string())
    }
}

impl<T> From<PoisonError<T>> for InfrastructureError {
    fn from(_: PoisonError<T>) -> Self {
        InfrastructureError::LockPoisoned
    }
}

/// Convert the canonical `NotFound` struct into `InfrastructureError::NotFound`.
///
/// This bridges the rich `NotFound { entity_type, id }` representation with the
/// transport-layer `InfrastructureError::NotFound(String)` variant, preserving
/// the entity type and identifier in the message string.
impl From<NotFound> for InfrastructureError {
    fn from(nf: NotFound) -> Self {
        InfrastructureError::NotFound(format!("{nf}"))
    }
}

/// Convert the canonical `NotFound` struct into `McpErrorKind::NotFound`.
///
/// This bridges the rich `NotFound { entity_type, id }` representation with the
/// MCP error taxonomy's `NotFound` variant.
impl From<NotFound> for McpErrorKind {
    fn from(_: NotFound) -> Self {
        McpErrorKind::NotFound
    }
}

/// Convert the canonical `CapabilityDenied` struct into `McpErrorKind::PermissionDenied`.
impl From<CapabilityDenied> for McpErrorKind {
    fn from(_: CapabilityDenied) -> Self {
        McpErrorKind::PermissionDenied
    }
}

#[cfg(feature = "sql")]
impl From<rusqlite::Error> for InfrastructureError {
    fn from(e: rusqlite::Error) -> Self {
        let kind = match &e {
            rusqlite::Error::SqliteFailure(ffi, _) => match ffi.code {
                rusqlite::ErrorCode::ConstraintViolation => DatabaseErrorKind::Constraint,
                _ => DatabaseErrorKind::Query,
            },
            _ => DatabaseErrorKind::Query,
        };
        InfrastructureError::Database {
            message: e.to_string(),
            kind,
        }
    }
}

// McpErrorKind — Canonical MCP Error Taxonomy

/// Semantic classification of MCP tool errors.
///
/// Inspired by `stack-domain-types::ErrorKind` (ADR-T7) and gRPC status codes.
/// Every MCP error variant maps to one `McpErrorKind`, enabling:
/// - Retry logic (retry on `Timeout`/`Unavailable`, don't on `InvalidArgument`)
/// - User-facing error categorization
/// - Regulation observability bucketing by error class
/// - OCAP policy decisions (distinguish `PermissionDenied` from `NotFound`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum McpErrorKind {
    /// Internal server error (bug, unexpected state).
    Internal,
    /// External service unavailable (network, upstream down).
    Unavailable,
    /// Operation timed out.
    Timeout,
    /// Resource or tool not found.
    NotFound,
    /// Invalid arguments or schema validation failure.
    InvalidArgument,
    /// OCAP capability denied or insufficient permissions.
    PermissionDenied,
    /// Rate limit exceeded (retryable after backoff).
    ///
    /// This is an external API boundary rate limiter — it protects MCP servers
    /// from external client DoS, distinct from internal energy budget tracking.
    RateLimited,
    /// Precondition not met (server not initialized, feature disabled).
    FailedPrecondition,
}

impl McpErrorKind {
    /// Whether errors of this kind are retryable with backoff.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any McpErrorKind variant
    /// post: returns true only for Unavailable, Timeout, and RateLimited;
    ///       returns false for Internal, NotFound, InvalidArgument,
    ///       PermissionDenied, and FailedPrecondition
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::Unavailable | Self::Timeout | Self::RateLimited)
    }

    /// Whether this error requires user/admin intervention.
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is any McpErrorKind variant
    /// post: returns true only for PermissionDenied and FailedPrecondition;
    ///       returns false for Internal, Unavailable, Timeout, NotFound,
    ///       InvalidArgument, and RateLimited
    pub fn requires_intervention(self) -> bool {
        matches!(self, Self::PermissionDenied | Self::FailedPrecondition)
    }
}

impl std::fmt::Display for McpErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal => write!(f, "internal"),
            Self::Unavailable => write!(f, "unavailable"),
            Self::Timeout => write!(f, "timeout"),
            Self::NotFound => write!(f, "not_found"),
            Self::InvalidArgument => write!(f, "invalid_argument"),
            Self::PermissionDenied => write!(f, "permission_denied"),
            Self::RateLimited => write!(f, "rate_limited"),
            Self::FailedPrecondition => write!(f, "failed_precondition"),
        }
    }
}

// Canonical domain error types — shared across all crates.

/// A resource was not found. Canonical across 17+ crates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotFound {
    pub entity_type: String,
    pub id: String,
}

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} not found: {}", self.entity_type, self.id)
    }
}

/// Capability denied — shared across 5+ crates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDenied {
    pub reason: String,
}

impl std::fmt::Display for CapabilityDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capability denied: {}", self.reason)
    }
}

/// Embedding dimension mismatch — duplicated across 2 crates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionMismatch {
    pub expected: usize,
    pub actual: usize,
}

impl std::fmt::Display for DimensionMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "dimension mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcperrorkind_is_retryable() {
        assert!(McpErrorKind::Unavailable.is_retryable());
        assert!(McpErrorKind::Timeout.is_retryable());
        assert!(McpErrorKind::RateLimited.is_retryable());
        assert!(!McpErrorKind::Internal.is_retryable());
        assert!(!McpErrorKind::NotFound.is_retryable());
        assert!(!McpErrorKind::InvalidArgument.is_retryable());
        assert!(!McpErrorKind::PermissionDenied.is_retryable());
        assert!(!McpErrorKind::FailedPrecondition.is_retryable());
    }

    #[test]
    fn mcperrorkind_requires_intervention() {
        assert!(McpErrorKind::PermissionDenied.requires_intervention());
        assert!(McpErrorKind::FailedPrecondition.requires_intervention());
        assert!(!McpErrorKind::Internal.requires_intervention());
        assert!(!McpErrorKind::Unavailable.requires_intervention());
        assert!(!McpErrorKind::Timeout.requires_intervention());
        assert!(!McpErrorKind::NotFound.requires_intervention());
        assert!(!McpErrorKind::InvalidArgument.requires_intervention());
        assert!(!McpErrorKind::RateLimited.requires_intervention());
    }

    #[test]
    fn from_poison_error_produces_lock_poisoned() {
        let mutex = std::sync::Mutex::new(42);
        // Poison the mutex by panicking while holding the lock
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
            panic!("intentional poison");
        }));
        assert!(result.is_err());
        // After the panic, the mutex is poisoned
        let lock_result = mutex.lock();
        assert!(lock_result.is_err());
        let infra: InfrastructureError = lock_result.unwrap_err().into();
        assert_eq!(infra, InfrastructureError::LockPoisoned);
    }

    #[test]
    fn from_serde_error_produces_serialization() {
        let bad_json = "{invalid";
        let result: Result<serde_json::Value, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
        let infra: InfrastructureError = result.unwrap_err().into();
        assert!(matches!(infra, InfrastructureError::Serialization(_)));
    }

    #[test]
    fn infrastructure_error_display_is_readable() {
        assert_eq!(
            InfrastructureError::database("conn refused").to_string(),
            "database(other): conn refused"
        );
        assert_eq!(
            InfrastructureError::LockPoisoned.to_string(),
            "lock poisoned"
        );
        assert_eq!(
            InfrastructureError::NotFound("key".into()).to_string(),
            "not found: key"
        );
    }

    #[test]
    fn mcperrorkind_display_renders_snake_case() {
        assert_eq!(McpErrorKind::Internal.to_string(), "internal");
        assert_eq!(McpErrorKind::Unavailable.to_string(), "unavailable");
        assert_eq!(
            McpErrorKind::PermissionDenied.to_string(),
            "permission_denied"
        );
        assert_eq!(
            McpErrorKind::FailedPrecondition.to_string(),
            "failed_precondition"
        );
    }
}
