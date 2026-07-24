//! Internationalization (i18n) message keys for `ServiceError`.
//!
//! Each variant carries a stable, language-independent key that surface
//! adapters can use for translation lookup. The `#[error("...")]` strings
//! are English fallbacks; `message_key()` returns the canonical key.

use super::{DomainKind, ServiceError};

impl ServiceError {
    /// Returns a stable i18n key for this error variant.
    ///
    /// Surface adapters use this key for translation lookup instead of
    /// parsing `Display` strings. Keys follow the pattern
    /// `error.<domain>.<condition>`.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be a valid ServiceError variant
    /// post: returns &'static str i18n key (e.g., "error.curator.escalation_not_found")
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            ServiceError::Domain { domain, .. } => match domain {
                DomainKind::Agent => "error.agent",
                DomainKind::Consent => "error.consent",
                DomainKind::Curator => "error.curator",
                DomainKind::Inference => "error.inference",
                DomainKind::Infrastructure => "error.infra",
                DomainKind::Memory => "error.memory",
                DomainKind::Pod => "error.pod",
                DomainKind::Storage => "error.storage",
                DomainKind::User => "error.user",
                DomainKind::Wallet => "error.wallet",
                DomainKind::Mcp => "error.mcp.tool",
                DomainKind::Skill => "error.skill",
            },
            ServiceError::ModelService { retryable, .. } => {
                if *retryable {
                    "error.inference.model_service_retryable"
                } else {
                    "error.inference.model_service"
                }
            }
            ServiceError::McpTool { .. } => "error.mcp.tool",
            ServiceError::Infra(_) => "error.infra",
            ServiceError::InvalidWebID { .. } => "error.user.invalid_webid",
        }
    }
}
