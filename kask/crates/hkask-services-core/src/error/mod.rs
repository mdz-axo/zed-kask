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
//! - `message_key.rs` — i18n `message_key()` logic
//! - `regulation_record.rs` — Regulation regulation record emission logic

use thiserror::Error;

use hkask_types::InfrastructureError;
use hkask_types::McpErrorKind;
use hkask_types::WalletError;
use hkask_types::{EmbeddingGenerationError, InferenceError};

// ── Helper implementation modules ─────────────────────────────────────

mod message_key;
mod regulation_record;
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

    /// Inference / embedding — carries retryability for Regulation gas budget.
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

impl From<WalletError> for ServiceError {
    fn from(e: WalletError) -> Self {
        let msg = e.to_string();
        ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Wallet,
            source: Some(Box::new(e)),
            message: msg,
        }
    }
}

impl<T> From<std::sync::PoisonError<T>> for ServiceError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        ServiceError::Infra(hkask_types::InfrastructureError::LockPoisoned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(msg: &str) -> ServiceError {
        ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Infrastructure,
            source: None,
            message: msg.into(),
        }
    }

    #[test]
    fn domain_classifies_agent_variants() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Agent,
            source: None,
            message: "test".into(),
        };
        assert_eq!(e.domain(), DomainKind::Agent);

        let e = ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Agent,
            source: None,
            message: "test".into(),
        };
        assert_eq!(e.domain(), DomainKind::Agent);
    }

    #[test]
    fn domain_classifies_curator_variants() {
        let e = ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Curator,
            source: None,
            message: "test".into(),
        };
        assert_eq!(e.domain(), DomainKind::Curator);

        let e = ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Curator,
            source: None,
            message: "test".into(),
        };
        assert_eq!(e.domain(), DomainKind::Curator);
    }

    #[test]
    fn domain_classifies_infrastructure() {
        assert_eq!(err("cfg").domain(), DomainKind::Infrastructure);

        let e = ServiceError::Infra(InfrastructureError::LockPoisoned);
        assert_eq!(e.domain(), DomainKind::Infrastructure);
    }

    #[test]
    fn domain_covers_all_variants() {
        // Every variant must return a valid DomainKind (not panic)
        let variants: &[ServiceError] = &[
            ServiceError::Domain {
                kind: ErrorKind::NotFound,
                domain: DomainKind::Agent,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Curator,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Consent,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Storage,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Memory,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::NotFound,
                domain: DomainKind::Pod,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Wallet,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::Forbidden,
                domain: DomainKind::User,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain: DomainKind::Skill,
                source: None,
                message: "".into(),
            },
            ServiceError::Domain {
                kind: ErrorKind::ServiceUnavailable,
                domain: DomainKind::Infrastructure,
                source: None,
                message: "".into(),
            },
            ServiceError::ModelService {
                kind: ErrorKind::BadRequest,
                source: None,
                message: "".into(),
                retryable: false,
            },
            ServiceError::ModelService {
                kind: ErrorKind::BadRequest,
                source: None,
                message: "".into(),
                retryable: true,
            },
            ServiceError::McpTool {
                kind: McpErrorKind::Internal,
                server: "".into(),
                tool: "".into(),
                message: "".into(),
            },
            ServiceError::Infra(InfrastructureError::LockPoisoned),
            ServiceError::InvalidWebID {
                source: None,
                message: "".into(),
            },
        ];
        for (i, e) in variants.iter().enumerate() {
            let d = e.domain();
            let k = e.kind();
            // Call Display to ensure no panics
            let _ = e.to_string();
            // Verify domain returned something valid
            assert!(
                matches!(
                    d,
                    DomainKind::Agent
                        | DomainKind::Consent
                        | DomainKind::Curator
                        | DomainKind::Inference
                        | DomainKind::Infrastructure
                        | DomainKind::Memory
                        | DomainKind::Pod
                        | DomainKind::Storage
                        | DomainKind::User
                        | DomainKind::Wallet
                        | DomainKind::Mcp
                        | DomainKind::Skill
                ),
                "variant {i}: unexpected domain {d:?}"
            );
            let _ = k; // kind() returns any ErrorKind
        }
    }

    #[test]
    fn not_found_variants_map_to_not_found_kind() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Agent,
            source: None,
            message: "".into(),
        };
        assert_eq!(e.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn forbidden_variants_map_to_forbidden_kind() {
        let e = ServiceError::InvalidWebID {
            source: None,
            message: "".into(),
        };
        assert_eq!(e.kind(), ErrorKind::Forbidden);
    }

    #[test]
    fn retryable_inference_is_service_unavailable() {
        let e = ServiceError::ModelService {
            kind: ErrorKind::BadRequest,
            source: None,
            message: "".into(),
            retryable: true,
        };
        assert_eq!(e.kind(), ErrorKind::ServiceUnavailable);
    }

    #[test]
    fn explicit_kind_overrides_inferred() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Consent,
            source: None,
            message: "".into(),
        };
        assert_eq!(e.kind(), ErrorKind::NotFound);
    }

    // ── v0.32: consolidated variant tests ─────────────────────

    #[test]
    fn domain_variant_preserves_domain_and_kind() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Pod,
            source: None,
            message: "pod-123".into(),
        };
        assert_eq!(e.domain(), DomainKind::Pod);
        assert_eq!(e.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn domain_variant_display_includes_kind_and_domain() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Agent,
            source: None,
            message: "agent-42".into(),
        };
        let s = e.to_string();
        assert!(s.contains("NotFound"), "expected NotFound in: {s}");
        assert!(s.contains("Agent"), "expected Agent in: {s}");
        assert!(s.contains("agent-42"), "expected message in: {s}");
    }

    #[test]
    fn model_service_retryable_maps_to_service_unavailable() {
        let e = ServiceError::ModelService {
            kind: ErrorKind::BadRequest,
            source: None,
            message: "timeout".into(),
            retryable: true,
        };
        assert_eq!(e.kind(), ErrorKind::ServiceUnavailable);
        assert_eq!(e.domain(), DomainKind::Inference);
    }

    #[test]
    fn model_service_nonretryable_uses_explicit_kind() {
        let e = ServiceError::ModelService {
            kind: ErrorKind::Forbidden,
            source: None,
            message: "access denied".into(),
            retryable: false,
        };
        assert_eq!(e.kind(), ErrorKind::Forbidden);
        assert_eq!(e.domain(), DomainKind::Inference);
    }

    #[test]
    fn domain_with_service_unavailable_is_retryable() {
        let e = ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Storage,
            source: None,
            message: "db down".into(),
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn domain_with_not_found_is_not_retryable() {
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Agent,
            source: None,
            message: "agent missing".into(),
        };
        assert!(!e.is_retryable());
    }

    #[test]
    fn model_service_retryable_is_retryable() {
        let e = ServiceError::ModelService {
            kind: ErrorKind::BadRequest,
            source: None,
            message: "timeout".into(),
            retryable: true,
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn model_service_nonretryable_is_not_retryable() {
        let e = ServiceError::ModelService {
            kind: ErrorKind::BadRequest,
            source: None,
            message: "bad prompt".into(),
            retryable: false,
        };
        assert!(!e.is_retryable());
    }

    // ── Property-level tests: exhaustive cross-product coverage ────────

    /// Every DomainKind appears in at least one Domain variant, and every
    /// Domain variant returns its domain correctly.
    #[test]
    fn all_domain_kinds_roundtrip_through_domain_variant() {
        let all_domains = [
            DomainKind::Agent,
            DomainKind::Consent,
            DomainKind::Curator,
            DomainKind::Inference,
            DomainKind::Infrastructure,
            DomainKind::Memory,
            DomainKind::Pod,
            DomainKind::Storage,
            DomainKind::User,
            DomainKind::Wallet,
            DomainKind::Mcp,
            DomainKind::Skill,
        ];
        for &domain in &all_domains {
            let e = ServiceError::Domain {
                kind: ErrorKind::BadRequest,
                domain,
                source: None,
                message: "test".into(),
            };
            assert_eq!(
                e.domain(),
                domain,
                "Domain variant must preserve its domain_kind"
            );
        }
    }

    /// `ServiceError::McpTool` must classify as `DomainKind::Mcp`, not `Wallet`
    /// (regression for the systemic skill/mcp-vs-wallet mislabel — review F21).
    #[test]
    fn mcp_tool_classifies_as_mcp_domain_not_wallet() {
        let e = ServiceError::McpTool {
            kind: McpErrorKind::NotFound,
            server: "skill".into(),
            tool: "skill_execute".into(),
            message: "not found".into(),
        };
        assert_eq!(e.domain(), DomainKind::Mcp);
        assert_ne!(e.domain(), DomainKind::Wallet);
    }

    /// `ServiceError::Domain { domain: DomainKind::Skill, .. }` must preserve its
    /// domain (regression for skill service ops mislabeled as Wallet — F6/F19).
    #[test]
    fn skill_domain_variant_preserves_skill_kind() {
        let e = ServiceError::Domain {
            kind: ErrorKind::ServiceUnavailable,
            domain: DomainKind::Skill,
            source: None,
            message: "skill not found".into(),
        };
        assert_eq!(e.domain(), DomainKind::Skill);
        assert_ne!(e.domain(), DomainKind::Wallet);
    }

    /// Every ErrorKind produces the expected retryability in a Domain variant.
    #[test]
    fn all_error_kinds_have_correct_retryability() {
        let retryable_kinds = [ErrorKind::ServiceUnavailable];
        let nonretryable_kinds = [
            ErrorKind::NotFound,
            ErrorKind::Conflict,
            ErrorKind::Forbidden,
            ErrorKind::BadRequest,
        ];
        for &kind in &retryable_kinds {
            let e = ServiceError::Domain {
                kind,
                domain: DomainKind::Storage,
                source: None,
                message: "test".into(),
            };
            assert!(
                e.is_retryable(),
                "{kind:?} must be retryable in Domain variant"
            );
        }
        for &kind in &nonretryable_kinds {
            let e = ServiceError::Domain {
                kind,
                domain: DomainKind::Storage,
                source: None,
                message: "test".into(),
            };
            assert!(
                !e.is_retryable(),
                "{kind:?} must NOT be retryable in Domain variant"
            );
        }
    }

    /// Every variant's Display output is non-empty and contains the message.
    #[test]
    fn all_variants_have_meaningful_display() {
        let test_msg = "unique-test-message-42";
        let variants: [ServiceError; 5] = [
            ServiceError::Domain {
                kind: ErrorKind::NotFound,
                domain: DomainKind::Agent,
                source: None,
                message: test_msg.into(),
            },
            ServiceError::ModelService {
                kind: ErrorKind::BadRequest,
                source: None,
                message: test_msg.into(),
                retryable: true,
            },
            ServiceError::McpTool {
                kind: hkask_types::McpErrorKind::Internal,
                server: "test-srv".into(),
                tool: "test-tool".into(),
                message: test_msg.into(),
            },
            ServiceError::Infra(hkask_types::InfrastructureError::database(test_msg)),
            ServiceError::InvalidWebID {
                source: None,
                message: test_msg.into(),
            },
        ];
        for v in &variants {
            let display = v.to_string();
            assert!(
                !display.is_empty(),
                "every variant must produce non-empty Display"
            );
            assert!(
                display.contains(test_msg),
                "Display must contain the error message; got: {display}"
            );
        }
    }

    /// McpTool variant preserves server, tool, and kind identity.
    #[test]
    fn mcp_tool_preserves_identity() {
        let e = ServiceError::McpTool {
            kind: hkask_types::McpErrorKind::RateLimited,
            server: "mcp-regulation".into(),
            tool: "sense".into(),
            message: "rate limit hit".into(),
        };
        assert_eq!(e.domain(), DomainKind::Mcp);
        assert!(e.to_string().contains("mcp-regulation"));
        assert!(e.to_string().contains("sense"));
        assert!(e.to_string().contains("rate_limited"));
    }

    /// InvalidWebID is always Forbidden and non-retryable.
    #[test]
    fn invalid_webid_is_forbidden_and_not_retryable() {
        let e = ServiceError::InvalidWebID {
            source: None,
            message: "bad-uuid".into(),
        };
        assert_eq!(e.kind(), ErrorKind::Forbidden);
        assert_eq!(e.domain(), DomainKind::User);
        assert!(!e.is_retryable());
    }

    /// Infra variant: retryable only for Io errors.
    #[test]
    fn infra_io_is_retryable_database_is_not() {
        let io_err = ServiceError::Infra(hkask_types::InfrastructureError::Io("disk error".into()));
        assert!(io_err.is_retryable(), "Io Infra should be retryable");

        let db_err = ServiceError::Infra(hkask_types::InfrastructureError::database("locked"));
        assert!(
            !db_err.is_retryable(),
            "Database Infra should NOT be retryable"
        );
        assert_eq!(db_err.kind(), ErrorKind::ServiceUnavailable);
        assert_eq!(db_err.domain(), DomainKind::Infrastructure);
    }

    /// From<InfrastructureError> round-trips: domain, kind, Display, and source.
    #[test]
    fn from_infrastructure_error_roundtrips() {
        let inner = hkask_types::InfrastructureError::Io("disk full".into());
        let e = ServiceError::from(inner);
        assert_eq!(e.domain(), DomainKind::Infrastructure);
        assert_eq!(e.kind(), ErrorKind::ServiceUnavailable);
        assert!(e.to_string().contains("disk full"));
        // Infra is #[error(transparent)] — verify it produces the right variant
        assert!(
            matches!(&e, ServiceError::Infra(_)),
            "From<InfrastructureError> must produce Infra variant"
        );
    }

    /// From<WalletError> round-trips through the Domain variant.
    #[test]
    fn from_wallet_error_produces_domain_with_wallet_kind() {
        use hkask_types::WalletError;
        let inner = WalletError::InsufficientBalance {
            have: hkask_types::RJoule(0),
            need: hkask_types::RJoule(100),
        };
        let e = ServiceError::from(inner);
        assert_eq!(e.domain(), DomainKind::Wallet);
        assert_eq!(e.kind(), ErrorKind::ServiceUnavailable);
        assert!(e.to_string().contains("insufficient"));
    }

    /// Source chains are preserved: every variant that carries source should expose it.
    #[test]
    fn source_chains_are_preserved() {
        // Domain with source
        let e = ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Storage,
            source: Some(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "file missing",
            ))),
            message: "db error".into(),
        };
        let src = std::error::Error::source(&e);
        assert!(src.is_some(), "Domain with source must expose it");

        // ModelService with source
        let e2 = ServiceError::ModelService {
            kind: ErrorKind::BadRequest,
            source: Some(Box::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timeout",
            ))),
            message: "inference timeout".into(),
            retryable: true,
        };
        let src2 = std::error::Error::source(&e2);
        assert!(src2.is_some(), "ModelService with source must expose it");

        // InvalidWebID with source
        let e3 = ServiceError::InvalidWebID {
            source: uuid::Uuid::parse_str("not-a-uuid").err(),
            message: "bad uuid".into(),
        };
        let src3 = std::error::Error::source(&e3);
        assert!(src3.is_some(), "InvalidWebID with source must expose it");
    }
}
