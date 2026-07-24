//! Governance context — OCAP enforcement, consent management, tool dispatch,
//! agent registration, escalation queue, and curation signal routing.
//!
//! Extracted from `AgentService` as part of the strangler-fig decomposition.
//! Public fields provide direct access to each subsystem; the single behavioral
//! method (`notify_goal_transition`) encapsulates `GoalTransitionEvent` construction.
//!
//! Escalation CRUD lives here — the data and the behavior co-locate.

use hkask_capability::CapabilityChecker;
use hkask_pods::a2a::A2ARuntime;
use hkask_pods::consent::ConsentManager;
use hkask_regulation::types::loops::{CurationInput, GoalTransitionEvent};

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_storage::{EscalationEntry, EscalationQueue};
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use std::sync::Arc;

// ── Escalation response type ──────────────────────────────────────────

/// Response for a single escalation entry.
pub struct EscalationResponse {
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

// ── GovernanceContext ──────────────────────────────────────────────────

/// Consolidated governance context — OCAP, consent, dispatch, agents,
/// escalations, and curation signals.
pub struct GovernanceContext {
    pub checker: Arc<CapabilityChecker>,
    pub consent: Arc<ConsentManager>,

    pub a2a: Arc<A2ARuntime>,
    pub escalations: Arc<EscalationQueue>,
    pub events: Arc<dyn RegulationSink>,
    pub curation_tx: tokio::sync::mpsc::UnboundedSender<CurationInput>,
}

impl GovernanceContext {
    /// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
    /// post: returns a GovernanceContext wired with OCAP checker, consent manager, dispatcher, A2A runtime, escalation queue, event sink, and optional curation channel
    pub fn new(
        checker: Arc<CapabilityChecker>,
        consent: Arc<ConsentManager>,

        a2a: Arc<A2ARuntime>,
        escalations: Arc<EscalationQueue>,
        events: Arc<dyn RegulationSink>,
        curation_tx: tokio::sync::mpsc::UnboundedSender<CurationInput>,
    ) -> Self {
        Self {
            checker,
            consent,
            a2a,
            escalations,
            events,
            curation_tx,
        }
    }

    /// Notify the curation loop of a goal state transition.
    ///
    /// Constructs and sends a `GoalTransitionEvent` through the curation
    /// channel. Silently drops the notification if no channel is configured.
    ///
    /// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
    /// post: sends a GoalTransitionEvent to the curation loop; no-op if unconfigured
    pub fn notify_goal_transition(
        &self,
        goal_id: String,
        from_state: String,
        to_state: String,
        agent: WebID,
    ) {
        let event = CurationInput::GoalTransition(GoalTransitionEvent {
            goal_id,
            from_state,
            to_state,
            agent,
        });
        let _ = self.curation_tx.send(event);
    }

    // ── Escalation CRUD ──────────────────────────────────────────────

    /// List pending escalations.
    ///
    /// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
    /// post: returns all currently pending escalation entries as EscalationResponse records
    #[must_use = "result must be used"]
    pub fn list_pending_escalations(&self) -> Result<Vec<EscalationResponse>, ServiceError> {
        list_escalations_direct(self.escalations.as_ref())
    }

    /// Resolve an escalation by ID.
    ///
    /// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
    /// post: marks the escalation as resolved; emits a Regulation regulation record; Err if not found
    #[must_use = "result must be used"]
    pub fn resolve_escalation(&self, id: &str, resolved_by: &str) -> Result<(), ServiceError> {
        resolve_direct(self.escalations.as_ref(), &self.events, id, resolved_by)
    }

    /// Dismiss an escalation by ID.
    ///
    /// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
    /// post: marks the escalation as dismissed; emits a Regulation regulation record; Err if not found
    #[must_use = "result must be used"]
    pub fn dismiss_escalation(&self, id: &str, dismissed_by: &str) -> Result<(), ServiceError> {
        dismiss_direct(self.escalations.as_ref(), &self.events, id, dismissed_by)
    }
}

// ── Escalation CRUD (free functions for MCP / granular access) ─────────

/// Emit a Regulation regulation record for an escalation operation (resolve/dismiss).
fn emit_escalation_event(
    events: &Arc<dyn RegulationSink>,
    operation: &str,
    actor_key: &str,
    escalation_id: &str,
    actor: &str,
) {
    let span = Span::new(
        SpanNamespace::try_from(RegulationSpan::Curation).expect("canonical span"),
        operation,
    );
    let event = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        span,
        CyclePhase::Act,
        serde_json::json!({
            "escalation_id": escalation_id,
            actor_key: actor,
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

/// List pending escalations (granular — no `GovernanceContext` required).
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: returns all currently pending escalation entries as EscalationResponse records
#[must_use = "result must be used"]
pub fn list_escalations_direct(
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

/// Resolve an escalation by ID (granular — no `GovernanceContext` required).
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: marks the escalation as resolved; emits a Regulation regulation record; Err if not found
#[must_use = "result must be used"]
pub fn resolve_direct(
    queue: &EscalationQueue,
    events: &Arc<dyn RegulationSink>,
    id: &str,
    resolved_by: &str,
) -> Result<(), ServiceError> {
    emit_escalation_event(
        events,
        "escalation_resolved",
        "resolved_by",
        id,
        resolved_by,
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

/// Dismiss an escalation by ID (granular — no `GovernanceContext` required).
///
/// expect: "The system enforces affirmative consent and capability boundaries for agent operations"
/// post: marks the escalation as dismissed; emits a Regulation regulation record; Err if not found
#[must_use = "result must be used"]
pub fn dismiss_direct(
    queue: &EscalationQueue,
    events: &Arc<dyn RegulationSink>,
    id: &str,
    dismissed_by: &str,
) -> Result<(), ServiceError> {
    emit_escalation_event(
        events,
        "escalation_dismissed",
        "dismissed_by",
        id,
        dismissed_by,
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

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::EscalationID;
    use uuid::Uuid;

    const FIXED_UUID_1: &str = "00000000-0000-0000-0000-000000000001";
    const FIXED_UUID_2: &str = "00000000-0000-0000-0000-000000000002";

    #[test]
    fn escalation_entry_to_response_maps_fields() {
        let entry = EscalationEntry {
            id: EscalationID::from_uuid(Uuid::parse_str(FIXED_UUID_1).unwrap()),
            template_id: hkask_types::TemplateID::new(),
            bot_id: hkask_types::BotID::new(),
            output: "test output".into(),
            confidence: 0.85,
            retry_count: 2,
            error_context: "some error".into(),
            created_at: chrono::Utc::now(),
            status: hkask_storage::EscalationStatus::Pending,
            resolved_at: None,
            resolved_by: None,
        };
        let resp = EscalationResponse::from(entry);
        assert_eq!(resp.id, FIXED_UUID_1);
        assert_eq!(resp.output, "test output");
        assert!((resp.confidence - 0.85).abs() < 0.001);
        assert_eq!(resp.retry_count, 2);
        assert_eq!(resp.status, "pending");
        assert!(resp.resolved_at.is_none());
    }

    #[test]
    fn escalation_entry_resolved_maps_resolution_fields() {
        let now = chrono::Utc::now();
        let entry = EscalationEntry {
            id: EscalationID::from_uuid(Uuid::parse_str(FIXED_UUID_2).unwrap()),
            template_id: hkask_types::TemplateID::new(),
            bot_id: hkask_types::BotID::new(),
            output: "done".into(),
            confidence: 1.0,
            retry_count: 0,
            error_context: String::new(),
            created_at: now,
            status: hkask_storage::EscalationStatus::Resolved,
            resolved_at: Some(now),
            resolved_by: Some("admin".into()),
        };
        let resp = EscalationResponse::from(entry);
        assert_eq!(resp.status, "resolved");
        assert!(resp.resolved_at.is_some());
        assert_eq!(resp.resolved_by, Some("admin".into()));
    }
}
