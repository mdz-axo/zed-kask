//! Audit trail types — Loop 5 (Curation): audit logging
//!
//! The Curator maintains the audit trail of all system decisions.
//! Audit entries record who did what, when, and with what outcome.

use chrono::{DateTime, Utc};
use hkask_types::WebID;
use serde::{Deserialize, Serialize};

/// Loop: Curation
/// Unified audit entry for all hKask operations
///
/// This consolidates 5 duplicate audit entry types into a single canonical structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique entry identifier
    pub id: String,
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    /// Actor WebID (who performed the action)
    pub actor: WebID,
    /// Action performed (e.g., "template_dispatch", "tool_invoke")
    pub action: String,
    /// Resource affected (e.g., template ID, tool name)
    pub resource: String,
    /// Outcome (success, failure, denied)
    pub outcome: AuditOutcome,
    /// Additional context/metadata
    pub(crate) context: AuditContext,
}

/// Loop: Curation
/// Audit outcome classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
    Error,
}

impl std::fmt::Display for AuditOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditOutcome::Success => write!(f, "success"),
            AuditOutcome::Failure => write!(f, "failure"),
            AuditOutcome::Denied => write!(f, "denied"),
            AuditOutcome::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for AuditOutcome {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "success" => Ok(AuditOutcome::Success),
            "failure" => Ok(AuditOutcome::Failure),
            "denied" => Ok(AuditOutcome::Denied),
            "error" => Ok(AuditOutcome::Error),
            _ => Err(format!("Invalid audit outcome: {}", s)),
        }
    }
}

/// Loop: Curation
/// Additional context for audit entries
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AuditContext {
    /// Correlation ID for distributed tracing
    pub correlation_id: Option<String>,
    /// Recipient WebID (for message passing)
    pub recipient: Option<WebID>,
    /// IP address (for network operations)
    pub ip_address: Option<String>,
    /// Error message (if outcome is failure/error)
    pub error_message: Option<String>,
    /// Arbitrary metadata
    pub metadata: serde_json::Value,
}

impl AuditEntry {
    /// Create a new audit entry
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  actor is a valid WebID; action and resource are non-empty strings;
    ///       outcome is a valid AuditOutcome variant
    /// post: returns an AuditEntry with a new v4 UUID id, current Utc timestamp,
    ///       and default (empty) AuditContext
    pub fn new(
        actor: WebID,
        action: impl Into<String>,
        resource: impl Into<String>,
        outcome: AuditOutcome,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            actor,
            action: action.into(),
            resource: resource.into(),
            outcome,
            context: AuditContext::default(),
        }
    }

    /// Add correlation ID
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid AuditEntry; correlation_id is a non-empty string
    /// post: returns self with context.correlation_id set to Some(correlation_id)
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.context.correlation_id = Some(correlation_id.into());
        self
    }

    /// Add recipient
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid AuditEntry; recipient is a valid WebID
    /// post: returns self with context.recipient set to Some(recipient)
    pub fn with_recipient(mut self, recipient: WebID) -> Self {
        self.context.recipient = Some(recipient);
        self
    }

    /// Add metadata
    ///
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a valid AuditEntry; metadata is any valid serde_json::Value
    /// post: returns self with context.metadata set to the given value
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.context.metadata = metadata;
        self
    }
}
