//! Governance — escalation CRUD and regulation event emission for the curator MCP server.
//!
//! Free functions provide granular access for MCP tool handlers without
//! requiring a consolidated context struct.

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_storage::{EscalationEntry, EscalationQueue};
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use std::sync::Arc;

// ── Escalation response type ──────────────────────────────────────────

/// Response for a single escalation entry.
pub(crate) struct EscalationResponse {
    pub id: String,
    pub template_id: String,
    pub bot_id: String,
    pub output: String,
    pub confidence: f64,
    pub retry_count: u32,
    pub error_context: String,
    pub created_at: String,
    pub status: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
}

impl From<EscalationEntry> for EscalationResponse {
    fn from(e: EscalationEntry) -> Self {
        Self {
            id: e.id.to_string(),
            template_id: e.template_id.to_string(),
            bot_id: e.bot_id.to_string(),
            output: e.output,
            confidence: e.confidence,
            retry_count: e.retry_count,
            error_context: e.error_context,
            created_at: e.created_at.to_rfc3339(),
            status: format!("{:?}", e.status).to_lowercase(),
            resolved_at: e.resolved_at.map(|dt| dt.to_rfc3339()),
            resolved_by: e.resolved_by,
        }
    }
}

// ── Escalation CRUD (free functions for MCP / granular access) ─────────

/// Emit a Regulation regulation record for an escalation operation (resolve/dismiss).
/// `detail` (the operator's resolution note or dismissal reason) is recorded
/// in the observation payload so the audit trail keeps it.
fn emit_escalation_event(
    events: &Arc<dyn RegulationSink>,
    operation: &str,
    actor_key: &str,
    escalation_id: &str,
    actor: &str,
    detail: Option<&str>,
) {
    let namespace = match SpanNamespace::try_from(RegulationSpan::Curation) {
        Ok(ns) => ns,
        Err(e) => {
            tracing::warn!(
                target: "reg.curation",
                error = %e,
                "Curation span namespace not in canonical registry — Regulation event skipped"
            );
            return;
        }
    };
    let span = Span::new(namespace, operation);
    let event = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        span,
        CyclePhase::Act,
        serde_json::json!({
            "escalation_id": escalation_id,
            actor_key: actor,
            "detail": detail,
        }),
        0,
    );
    if let Err(e) = events.persist(&event) {
        tracing::warn!(
            target: "reg.curation",
            escalation_id = %escalation_id,
            error = %e,
            operation = operation,
            "Regulation event persist failed — observability gap"
        );
    }
}

/// List pending escalations.
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: returns all currently pending escalation entries as EscalationResponse records
#[must_use = "result must be used"]
pub(crate) fn list_escalations_direct(
    queue: &EscalationQueue,
) -> Result<Vec<EscalationResponse>, ServiceError> {
    let entries = queue.list_pending().map_err(|e| ServiceError::Domain {
        domain: DomainKind::Curator,
        kind: ErrorKind::ServiceUnavailable,
        source: None,
        message: e.to_string(),
    })?;
    Ok(entries.into_iter().map(EscalationResponse::from).collect())
}

/// Resolve an escalation by ID.
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: marks the escalation as resolved; emits a Regulation regulation record; Err if not found
#[must_use = "result must be used"]
pub(crate) fn resolve_direct(
    queue: &EscalationQueue,
    events: &Arc<dyn RegulationSink>,
    id: &str,
    resolved_by: &str,
    resolution: Option<&str>,
) -> Result<(), ServiceError> {
    emit_escalation_event(
        events,
        "escalation_resolved",
        "resolved_by",
        id,
        resolved_by,
        resolution,
    );

    queue.resolve(id, resolved_by).map_err(|e| match e {
        hkask_storage::EscalationError::NotFound(nf) => ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Curator,
            source: None,
            message: nf.id,
        },
        other => ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Curator,
            source: None,
            message: other.to_string(),
        },
    })
}

/// Dismiss an escalation by ID.
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: marks the escalation as dismissed; emits a Regulation regulation record; Err if not found
#[must_use = "result must be used"]
pub(crate) fn dismiss_direct(
    queue: &EscalationQueue,
    events: &Arc<dyn RegulationSink>,
    id: &str,
    dismissed_by: &str,
    reason: Option<&str>,
) -> Result<(), ServiceError> {
    emit_escalation_event(
        events,
        "escalation_dismissed",
        "dismissed_by",
        id,
        dismissed_by,
        reason,
    );

    queue.dismiss(id, dismissed_by).map_err(|e| match e {
        hkask_storage::EscalationError::NotFound(nf) => ServiceError::Domain {
            kind: ErrorKind::NotFound,
            domain: DomainKind::Curator,
            source: None,
            message: nf.id,
        },
        other => ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::Curator,
            source: None,
            message: other.to_string(),
        },
    })
}
