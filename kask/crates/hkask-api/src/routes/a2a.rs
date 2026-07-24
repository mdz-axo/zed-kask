//! A2A registration and listing routes
//!
//! # Service layer depth test
//!
//! A2AService was considered but **rejected** as shallow: every handler is a thin
//! delegation to `A2ARuntime` methods (`register_agent`, `list_agents`,
//! `unregister_agent`) plus HTTP response mapping. No CLI A2A commands exist.
//! An A2AService would just be `self.a2a_runtime().register_agent()` — a pure
//! pass-through that increases interface cost without adding behavior.
//!
//! Decision: Guideline — keep direct `service_context.infra().pods.clone().a2a_runtime()` access.
//! Revisit if A2A policy logic (e.g., capability scoping, agent tier enforcement)
//! grows beyond simple registration/delegation.

use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use axum::{Json, extract::State};
use hkask_types::WebID;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::error::ServiceErrorResponse;
use crate::middleware::AuthContext;

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};

/// Parse a WebID from a string, returning a structured error on failure.
fn parse_webid(raw: &str) -> Result<WebID, ServiceError> {
    uuid::Uuid::parse_str(raw)
        .map(WebID::from_uuid)
        .map_err(|_| ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: format!("Invalid WebID format: {}", raw),
        })
}

/// A2A registration request
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct A2ARegisterRequest {
    /// Agent WebID (UUID string)
    pub webid: String,
    /// Capabilities to grant (e.g., ["tool:execute", "template:render"])
    pub capabilities: Vec<String>,
}

/// A2A registration response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct A2ARegisterResponse {
    /// Granted capability token (HMAC-signed)
    pub token: String,
    /// Registration timestamp (Unix epoch seconds)
    pub registered_at: i64,
    /// Agent WebID
    pub webid: String,
}

/// A2A agent response — registered agent in the ACP capability delegation system (P4 OCAP).
///
/// `capabilities` are the granted capability verbs this agent holds.
/// `active` indicates whether the agent is currently allowed to exercise its capabilities.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct A2AAgentResponse {
    pub webid: String,
    pub capabilities: Vec<String>,
    pub registered_at: i64,
    pub active: bool,
}

/// Agent list response — all A2A-registered agents.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct AgentListResponse {
    pub agents: Vec<A2AAgentResponse>,
}

/// Create A2A router
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with A2A routes registered
pub fn a2a_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .route("/api/v1/a2a/register", axum::routing::post(a2a_register))
        .routes(routes!(a2a_list_agents))
        .routes(routes!(a2a_unregister_agent))
}

/// Register an agent with the A2A runtime
async fn a2a_register(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Json(req): Json<A2ARegisterRequest>,
) -> Result<Json<A2ARegisterResponse>, ServiceErrorResponse> {
    let webid = parse_webid(&req.webid)?;

    if req.capabilities.is_empty() {
        return Err(ServiceError::Domain {
            kind: ErrorKind::BadRequest,
            domain: DomainKind::User,
            source: None,
            message: "At least one capability is required".to_string(),
        }
        .into());
    }

    let a2a = &state.agent_service.governance().a2a;
    let token = a2a.register_agent(webid, req.capabilities).await?;

    Ok(Json(A2ARegisterResponse {
        token: token.id.clone(),
        registered_at: chrono::Utc::now().timestamp(),
        webid: req.webid,
    }))
}

/// List all registered A2A agents
#[utoipa::path(
    get,
    path = "/api/v1/a2a/agents",
    tag = "a2a",
    responses(
        (status = 200, description = "List of registered agents", body = AgentListResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn a2a_list_agents(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<AgentListResponse>, ServiceErrorResponse> {
    let a2a = &state.agent_service.governance().a2a;
    let agents = a2a.list_agents().await;

    let agent_responses: Vec<A2AAgentResponse> = agents
        .into_iter()
        .map(|a| A2AAgentResponse {
            webid: a.webid.to_string(),
            capabilities: a.capabilities,
            registered_at: a.registered_at,
            active: a.active,
        })
        .collect();

    Ok(Json(AgentListResponse {
        agents: agent_responses,
    }))
}

/// Unregister an A2A agent
#[utoipa::path(
    delete,
    path = "/api/v1/a2a/agents/{agent_id}",
    tag = "a2a",
    params(
        ("agent_id" = String, Path, description = "A2A agent ID to unregister"),
    ),
    responses(
        (status = 200, description = "Agent unregistered"),
        (status = 400, description = "Invalid agent ID"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Agent not found"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub(crate) async fn a2a_unregister_agent(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Path(agent_id): Path<String>,
) -> Result<StatusCode, ServiceErrorResponse> {
    use axum::http::StatusCode;

    let webid = parse_webid(&agent_id)?;

    let a2a = &state.agent_service.governance().a2a;
    a2a.unregister_agent(&webid).await?;

    Ok(StatusCode::NO_CONTENT)
}
