//! Admin API routes — invite management and session listing.
//!
//! All admin routes are gated by the admin middleware (Role::Admin required).
//! These implement the multi-user contracts from FUNCTIONAL_SPECIFICATION.md §3.16.

use crate::middleware::AuthContext;
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use hkask_types::id::UserID;
use hkask_types::identity::Role;
use hkask_types::server_config::{ServerConfig, ServerRegistration};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use utoipa::ToSchema;

use crate::ApiState;

/// POST /api/v1/admin/invite
///
/// expect: "As an admin I can create an invite code for a new member"
#[utoipa::path(
    post,
    path = "/api/v1/admin/invite",
    tag = "admin",
    responses(
        (status = 200, description = "Invite code created successfully", body = InviteResponse),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub async fn create_invite(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let userpod = user_store
        .get_userpod_by_webid(&auth.webid)
        .map_err(|e| (StatusCode::FORBIDDEN, format!("{e}")))?
        .ok_or((StatusCode::FORBIDDEN, "UserPod not found".into()))?;
    let invite = user_store.create_invite(&userpod.user_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Invite creation failed: {e}"),
        )
    })?;
    Ok(Json(InviteResponse { code: invite.code }))
}

/// GET /api/v1/admin/invite
///
/// expect: "As an admin I can see all invites I've sent and their status"
#[utoipa::path(
    get,
    path = "/api/v1/admin/invite",
    tag = "admin",
    responses(
        (status = 200, description = "List of pending invites"),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 500, description = "Internal server error"),
    ),
)]
pub async fn list_invites(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let userpod = user_store
        .get_userpod_by_webid(&auth.webid)
        .map_err(|e| (StatusCode::FORBIDDEN, format!("{e}")))?
        .ok_or((StatusCode::FORBIDDEN, "UserPod not found".into()))?;
    let invites = user_store.list_invites(&userpod.user_id).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("List invites failed: {e}"),
        )
    })?;
    Ok(Json(invites))
}

/// DELETE /api/v1/admin/invite/{code}
///
/// expect: "As an admin I can revoke an invite I've sent"
#[utoipa::path(
    delete,
    path = "/api/v1/admin/invite/{code}",
    tag = "admin",
    params(
        ("code" = String, Path, description = "Invite code to revoke")
    ),
    responses(
        (status = 200, description = "Invite revoked successfully"),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 404, description = "Invite not found or already accepted/expired"),
    ),
)]
pub async fn revoke_invite(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
    Path(code): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let userpod = user_store
        .get_userpod_by_webid(&auth.webid)
        .map_err(|e| (StatusCode::FORBIDDEN, format!("{e}")))?
        .ok_or((StatusCode::FORBIDDEN, "UserPod not found".into()))?;
    let invite = user_store
        .revoke_invite(&code, &userpod.user_id)
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Revoke failed: {e}")))?;
    tracing::info!(
        target = "reg.deploy.invite",
        operation = "invite_revoked",
        code = %invite.code,
        "REG"
    );
    Ok(Json(invite))
}

/// GET /api/v1/admin/sessions
///
/// expect: "As an admin I can see all active sessions on my server"
#[utoipa::path(
    get,
    path = "/api/v1/admin/sessions",
    tag = "admin",
    responses(
        (status = 200, description = "List of active sessions"),
        (status = 403, description = "Forbidden — not an admin"),
    ),
)]
pub async fn list_sessions(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let sessions = user_store.list_all_sessions().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("List sessions failed: {e}"),
        )
    })?;
    Ok(Json(sessions))
}

/// GET /api/v1/admin/members
///
/// expect: "As an admin I can see all members on my server"
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberEntry {
    user_id: String,
    role: String,
    display_name: String,
    created_at: i64,
    last_active: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/members",
    tag = "admin",
    responses(
        (status = 200, description = "List of members"),
        (status = 403, description = "Forbidden — not an admin"),
    ),
)]
pub async fn list_members(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let users = user_store.list_all_users_summary().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("List members failed: {e}"),
        )
    })?;
    let members: Vec<MemberEntry> = users
        .into_iter()
        .map(|(uid, role, name, created, last_active)| MemberEntry {
            user_id: uid.to_string(),
            role,
            display_name: name,
            created_at: created,
            last_active,
        })
        .collect();
    Ok(Json(members))
}

/// PATCH /api/v1/admin/members/{user_id}
///
/// expect: "As an admin I can promote a member to admin or demote to member"
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetRoleRequest {
    pub role: String,
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/members/{user_id}",
    tag = "admin",
    request_body = SetRoleRequest,
    params(
        ("user_id" = String, Path, description = "User ID to modify")
    ),
    responses(
        (status = 200, description = "Role updated"),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 400, description = "Invalid role"),
        (status = 404, description = "User not found"),
    ),
)]
pub async fn set_member_role(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Path(user_id_str): Path<String>,
    Json(body): Json<SetRoleRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let role: Role = body
        .role
        .parse()
        .map_err(|e: String| (StatusCode::BAD_REQUEST, format!("Invalid role: {e}")))?;
    let user_id = UserID::from_str(&user_id_str)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid user ID: {e}")))?;
    let user_store = state.agent_service.storage().users.clone();
    let user_store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    // Guard: prevent demoting the last admin
    if role == Role::Member {
        let users = user_store.list_all_users_summary().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("List members failed: {e}"),
            )
        })?;
        let admin_count = users.iter().filter(|(_, r, _, _, _)| r == "admin").count();
        if admin_count <= 1 {
            return Err((
                StatusCode::BAD_REQUEST,
                "Cannot demote the last admin. Promote another member to admin first.".into(),
            ));
        }
    }
    user_store
        .set_user_role(&user_id, role)
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Set role failed: {e}")))?;
    tracing::info!(
        target = "reg.multi_user.role_changed",
        user_id = %user_id_str,
        role = %body.role,
        "REG"
    );
    Ok(Json(
        serde_json::json!({"status": "ok", "user_id": user_id_str, "role": body.role}),
    ))
}

/// GET /api/v1/admin/config
///
/// expect: "As an admin I can view the server configuration"
#[utoipa::path(
    get,
    path = "/api/v1/admin/config",
    tag = "admin",
    responses(
        (status = 200, description = "Server configuration"),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 500, description = "Failed to load config"),
    ),
)]
pub async fn get_config(
    State(_state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let config = ServerConfig::load().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load config: {e}"),
        )
    })?;
    Ok(Json(config))
}

/// PATCH /api/v1/admin/config
///
/// expect: "As an admin I can change the server registration mode"
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateConfigRequest {
    #[serde(default)]
    pub registration: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/config",
    tag = "admin",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "Configuration updated"),
        (status = 403, description = "Forbidden — not an admin"),
        (status = 400, description = "Invalid request body"),
        (status = 500, description = "Failed to save config"),
    ),
)]
pub async fn update_config(
    State(_state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = ServerConfig::load().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load config: {e}"),
        )
    })?;
    if let Some(ref reg_str) = body.registration {
        config.registration = reg_str.parse::<ServerRegistration>().map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid registration mode: {e}"),
            )
        })?;
    }
    config.save().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save config: {e}"),
        )
    })?;
    Ok(Json(config))
}

/// Build the admin router.
pub fn admin_router() -> utoipa_axum::router::OpenApiRouter<ApiState> {
    use utoipa_axum::routes;
    utoipa_axum::router::OpenApiRouter::new()
        .routes(routes!(create_invite))
        .routes(routes!(list_invites))
        .routes(routes!(revoke_invite))
        .routes(routes!(list_sessions))
        .routes(routes!(list_members))
        .routes(routes!(set_member_role))
        .routes(routes!(get_config))
        .routes(routes!(update_config))
}

#[derive(Serialize, ToSchema)]
pub struct InviteResponse {
    code: String,
}
