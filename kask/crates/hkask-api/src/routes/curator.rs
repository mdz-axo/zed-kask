//! Curator escalation and metacognition routes

use axum::extract::Extension;
use axum::{Json, extract::Path, extract::State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;

/// Escalation entry — a pending Curator escalation triggered by bot output below
/// confidence threshold or context errors (Pattern C, P12).
///
/// Status: "pending", "resolved", or "dismissed".
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EscalationEntryResponse {
    /// Escalation ID
    pub id: String,
    /// Template that produced the flagged output
    pub template_id: String,
    /// Bot WebID that triggered the escalation
    pub bot_id: String,
    /// Flagged output text
    pub output: String,
    /// Confidence score (0.0–1.0) at time of escalation
    pub confidence: f64,
    /// Number of retry attempts before escalation
    pub retry_count: u32,
    /// Error context or reason for escalation
    pub error_context: String,
    /// ISO 8601 creation timestamp
    pub created_at: String,
    /// Current status: "pending", "resolved", or "dismissed"
    pub status: String,
    /// ISO 8601 resolution timestamp (when resolved/dismissed)
    pub resolved_at: Option<String>,
    /// WebID of the resolver (P12 — accountable identity)
    pub resolved_by: Option<String>,
}

/// List of pending escalations.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ListEscalationsResponse {
    /// Pending escalations in the queue
    pub escalations: Vec<EscalationEntryResponse>,
}

/// Resolve escalation request — P12 (accountable identity).
#[derive(Debug, Deserialize, ToSchema)]
pub struct ResolveEscalationRequest {
    /// WebID of the human or agent resolving the escalation
    pub resolved_by: String,
}

/// Resolve escalation response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ResolveEscalationResponse {
    /// Escalation ID that was resolved
    pub id: String,
    /// New status: "resolved"
    pub status: String,
}

/// Dismiss escalation request — P12 (accountable identity).
#[derive(Debug, Deserialize, ToSchema)]
pub struct DismissEscalationRequest {
    /// WebID of the human or agent dismissing the escalation
    pub dismissed_by: String,
}

/// Dismiss escalation response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DismissEscalationResponse {
    /// Escalation ID that was dismissed
    pub id: String,
    /// New status: "dismissed"
    pub status: String,
}

/// Escalation statistics — aggregate counts across all escalations.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EscalationStatsResponse {
    /// Total escalations ever created
    pub total: i64,
    /// Escalations awaiting resolution
    pub pending: i64,
    /// Escalations that have been resolved
    pub resolved: i64,
    /// Escalations dismissed as non-actionable
    pub dismissed: i64,
}

/// Bot status report — reflects the health of a specific bot agent.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct BotStatusReportResponse {
    /// Bot name
    pub bot_name: String,
    /// Aggregate status: "healthy", "degraded", or "critical"
    pub status: String,
    /// ISO 8601 timestamp of last report (None if never reported)
    pub last_report: Option<String>,
    /// Active issues requiring attention
    pub issues: Vec<String>,
}

/// Curator metacognition status — aggregate view of system health (Pattern C).
///
/// Combines escalation queue statistics with per-bot health reports.
/// This is the primary observability endpoint for the Curator mediation loop.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MetacognitionStatusResponse {
    /// Aggregate escalation statistics
    pub escalation_stats: EscalationStatsResponse,
    /// Per-bot health reports
    pub bot_reports: Vec<BotStatusReportResponse>,
}

/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with curator routes registered
pub fn curator_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_escalations))
        .routes(routes!(resolve_escalation))
        .routes(routes!(dismiss_escalation))
        .routes(routes!(metacognition_status))
}

/// List all pending curator escalations.
#[utoipa::path(
    get, path = "/api/v1/curator/escalations", tag = "curator",
    responses(
        (status = 200, description = "List of pending escalations", body = ListEscalationsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn list_escalations(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<ListEscalationsResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "curator_escalations", "REG");
    let entries = state
        .agent_service
        .governance()
        .list_pending_escalations()?;
    let escalations: Vec<EscalationEntryResponse> = entries
        .into_iter()
        .map(|e| EscalationEntryResponse {
            id: e.id,
            template_id: e.template_id,
            bot_id: e.bot_id,
            output: e.output,
            confidence: e.confidence,
            retry_count: e.retry_count,
            error_context: e.error_context,
            created_at: e.created_at,
            status: e.status,
            resolved_at: e.resolved_at,
            resolved_by: e.resolved_by,
        })
        .collect();
    Ok(Json(ListEscalationsResponse { escalations }))
}

/// Resolve an escalation by marking it handled with the resolver's identity.
#[utoipa::path(
    post, path = "/api/v1/curator/escalations/{id}/resolve", tag = "curator",
    params(
        ("id" = String, Path, description = "Escalation ID"),
    ),
    request_body = ResolveEscalationRequest,
    responses(
        (status = 200, description = "Escalation resolved", body = ResolveEscalationResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Escalation not found"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn resolve_escalation(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(req): Json<ResolveEscalationRequest>,
) -> Result<Json<ResolveEscalationResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "curator_resolve", escalation_id = %id, "REG");
    state
        .agent_service
        .governance()
        .resolve_escalation(&id, &req.resolved_by)?;
    Ok(Json(ResolveEscalationResponse {
        id,
        status: "resolved".into(),
    }))
}

/// Dismiss an escalation as non-actionable, recording who dismissed it.
#[utoipa::path(
    post, path = "/api/v1/curator/escalations/{id}/dismiss", tag = "curator",
    params(
        ("id" = String, Path, description = "Escalation ID"),
    ),
    request_body = DismissEscalationRequest,
    responses(
        (status = 200, description = "Escalation dismissed", body = DismissEscalationResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Escalation not found"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn dismiss_escalation(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(req): Json<DismissEscalationRequest>,
) -> Result<Json<DismissEscalationResponse>, ServiceErrorResponse> {
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "curator_dismiss", escalation_id = %id, "REG");
    state
        .agent_service
        .governance()
        .dismiss_escalation(&id, &req.dismissed_by)?;
    Ok(Json(DismissEscalationResponse {
        id,
        status: "dismissed".into(),
    }))
}

/// Get Curator metacognition status — escalation statistics and bot health reports.
#[utoipa::path(
    get, path = "/api/v1/curator/metacognition", tag = "curator",
    responses(
        (status = 200, description = "Curator metacognition status", body = MetacognitionStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn metacognition_status(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<MetacognitionStatusResponse>, ServiceErrorResponse> {
    let queue = &state.agent_service.governance().escalations;
    let stats = queue.stats()?;
    let escalation_stats = EscalationStatsResponse {
        total: stats.total,
        pending: stats.pending,
        resolved: stats.resolved,
        dismissed: stats.dismissed,
    };
    Ok(Json(MetacognitionStatusResponse {
        escalation_stats,
        bot_reports: Vec::new(),
    }))
}
