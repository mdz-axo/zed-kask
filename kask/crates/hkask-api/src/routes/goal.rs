//! Goal coordination routes — direct calls to goal repository.
//! Formerly delegated to GoalService (removed v0.31.0 per P5).

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, extract::Path, extract::Query, extract::State};
use hkask_types::GoalState;
use hkask_types::visibility::Visibility;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::ApiState;
use crate::middleware::AuthContext;

/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns `OpenApiRouter<ApiState>` with goal routes registered
pub fn goal_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list_goals))
        .routes(routes!(create_goal))
        .routes(routes!(set_goal_state))
}

/// Create goal request — P4 OCAP-gated goal creation.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct CreateGoalRequest {
    pub text: String,
    /// Visibility: "private" or "shared" (defaults to "private")
    pub visibility: Option<String>,
}

/// Set goal state request — state machine transition.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct SetGoalStateRequest {
    pub state: String,
}

/// Goal response — reflects current goal state and visibility (P4, P11).
#[derive(Serialize, Deserialize, ToSchema)]
pub struct GoalResponse {
    pub id: String,
    pub text: String,
    pub state: String,
    pub visibility: String,
}

/// Goal list response.
#[derive(Serialize, Deserialize, ToSchema)]
pub struct GoalListResponse {
    pub goals: Vec<GoalResponse>,
}

/// Create a new goal for the authenticated agent.
#[utoipa::path(
    post, path = "/api/goals", tag = "goals",
    request_body = CreateGoalRequest,
    responses(
        (status = 200, description = "Goal created", body = GoalResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Authority denied"),
    ),
)]
pub(crate) async fn create_goal(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateGoalRequest>,
) -> impl IntoResponse {
    let vis = match Visibility::parse_str(&req.visibility.unwrap_or_else(|| "private".into())) {
        Some(v) => v,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid visibility"})),
            )
                .into_response();
        }
    };
    let repo = state.agent_service.storage().goals.clone();
    match repo.create_goal(&auth.webid, &req.text, vis) {
        Ok(goal) => Json(GoalResponse {
            id: goal.id.to_string(),
            text: goal.text,
            state: goal.state.as_str().to_string(),
            visibility: goal.visibility.as_str().to_string(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// List all goals for the authenticated agent, optionally filtered by state.
#[utoipa::path(
    get, path = "/api/goals", tag = "goals",
    params(("state" = Option<String>, Query, description = "Optional state filter")),
    responses(
        (status = 200, description = "Goals listed", body = GoalListResponse),
        (status = 400, description = "Invalid state filter"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Authority denied"),
    ),
)]
pub(crate) async fn list_goals(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let state_filter = params.get("state").map(|s| s.as_str());
    let filter = match state_filter {
        Some(s) => match GoalState::parse_str(s) {
            Some(gs) => Some(gs),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("Invalid state filter: {s}")})),
                )
                    .into_response();
            }
        },
        None => None,
    };
    let repo = state.agent_service.storage().goals.clone();
    match repo.list_goals(&auth.webid, filter) {
        Ok(goals) => Json(GoalListResponse {
            goals: goals
                .into_iter()
                .map(|g| GoalResponse {
                    id: g.id.to_string(),
                    text: g.text,
                    state: g.state.as_str().to_string(),
                    visibility: g.visibility.as_str().to_string(),
                })
                .collect(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Transition a goal to a new state (legal transitions only).
#[utoipa::path(
    post, path = "/api/goals/{id}/state", tag = "goals",
    params(("id" = String, Path, description = "Goal ID")),
    request_body = SetGoalStateRequest,
    responses(
        (status = 200, description = "Goal state changed"),
        (status = 400, description = "Invalid or illegal transition"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Authority denied"),
        (status = 404, description = "Goal not found"),
    ),
)]
pub(crate) async fn set_goal_state(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(req): Json<SetGoalStateRequest>,
) -> impl IntoResponse {
    let goal_id: hkask_types::id::GoalID = match id.parse() {
        Ok(gid) => gid,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid goal ID: {id}")})),
            )
                .into_response();
        }
    };
    let new_state = match GoalState::parse_str(&req.state) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid state: {}", req.state)})),
            )
                .into_response();
        }
    };
    let repo = state.agent_service.storage().goals.clone();

    let goal = match repo.get_goal(goal_id) {
        Ok(Some(g)) => g,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": format!("Goal not found: {id}")})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    };

    // Ownership check
    if goal.webid != auth.webid {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Not authorized to transition this goal"})),
        )
            .into_response();
    }

    match repo.update_goal_state(goal_id, new_state) {
        Ok(()) => {
            state.agent_service.governance().notify_goal_transition(
                goal_id.to_string(),
                goal.state.as_str().to_string(),
                new_state.as_str().to_string(),
                auth.webid,
            );
            Json(GoalResponse {
                id: goal_id.to_string(),
                text: goal.text,
                state: new_state.as_str().to_string(),
                visibility: goal.visibility.as_str().to_string(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
