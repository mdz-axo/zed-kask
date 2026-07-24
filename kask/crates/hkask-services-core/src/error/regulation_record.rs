//! Regulation regulation record emission for `ServiceError`.
//!
//! Only system-level errors (infrastructure, inference, Regulation, storage)
//! emit regulation records. User-input errors (NotFound, InvalidInput, LoginFailed)
//! are not system conditions — they don't need Regulation observability.

use super::{DomainKind, ServiceError};

impl ServiceError {
    /// Emit a regulation record for Regulation-observable errors.
    ///
    /// Returns `None` for user-input errors that don't represent system
    /// conditions. Returns `Some(RegulationRecord)` for infrastructure, inference,
    /// Regulation, storage, and security errors the Regulation can act on.
    ///
    /// The observer WebID is freshly generated per event — these are
    /// system-level observations, not agent-specific.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  self must be a valid ServiceError variant
    /// post: returns Some(RegulationRecord) for system-level errors (inference, Regulation, storage, infra); None for user-input errors (not-found, validation)
    #[must_use]
    pub fn regulation_record(&self) -> Option<hkask_types::event::RegulationRecord> {
        use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};
        use hkask_types::id::WebID;

        let (namespace, path_suffix, observation) = match self {
            // ── Domain (consolidated) ──────────────────────────────────
            ServiceError::Domain {
                domain, message, ..
            } => match domain {
                DomainKind::Agent => (
                    "reg.pod",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Consent => (
                    "reg.sovereignty",
                    "error.consent",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Curator => (
                    "reg.curation",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Inference => (
                    "reg.inference",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Infrastructure => (
                    "reg.cybernetics",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Memory => (
                    "reg.memory.encode",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Pod => (
                    "reg.pod",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Storage => (
                    "reg.cybernetics",
                    "error.storage",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::User => (
                    "reg.sovereignty",
                    "error.user",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Wallet => (
                    "reg.wallet.balance",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Mcp => (
                    "reg.tool",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
                DomainKind::Skill => (
                    "reg.skill",
                    "error",
                    serde_json::json!({ "message": message }),
                ),
            },
            ServiceError::ModelService { message, .. } => (
                "reg.inference",
                "error.model_service",
                serde_json::json!({ "message": message }),
            ),
            ServiceError::McpTool {
                kind,
                server,
                tool,
                message,
            } => (
                "reg.tool",
                "error",
                serde_json::json!({
                    "kind": kind.to_string(),
                    "server": server,
                    "tool": tool,
                    "message": message,
                }),
            ),
            ServiceError::Infra(e) => (
                "reg.cybernetics",
                "error.infra",
                serde_json::json!({ "error": e.to_string() }),
            ),
            // User-input error — not a system condition
            ServiceError::InvalidWebID { .. } => return None,
        };

        let span = Span::new(
            SpanNamespace::new(namespace).expect("canonical namespace"),
            path_suffix,
        );
        Some(RegulationRecord::new(
            WebID::new(),
            span,
            CyclePhase::Sense,
            observation,
            0,
        ))
    }
}
