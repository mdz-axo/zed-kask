//! UserPod API — list, rename, and delete userpods.
//!
//! REQ: P1-deploy-userpod-manage — P1 User Sovereignty: user manages their userpods.
//! expect: "I can manage my userpods through the API"

use axum::{Extension, Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ApiState;
use crate::middleware::AuthContext;

#[derive(Debug, Serialize, ToSchema)]
pub struct UserPodInfo {
    pub name: String,
    pub webid: String,
    pub created_at: i64,
    pub last_login: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserPodListResponse {
    pub userpods: Vec<UserPodInfo>,
    pub active: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

/// GET /api/v1/userpods
#[utoipa::path(
    get,
    path = "/api/v1/userpods",
    tag = "userpods",
    responses(
        (status = 200, description = "List of userpods for the authenticated user", body = UserPodListResponse),
    ),
)]
pub async fn list_userpods(
    State(state): State<ApiState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<UserPodListResponse>, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let session_userpod = store
        .get_userpod_by_webid(&auth.webid)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?
        .ok_or((StatusCode::NOT_FOUND, "UserPod not found".to_string()))?;
    let userpod = store
        .get_userpod_by_user(&session_userpod.user_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let list: Vec<UserPodInfo> = userpod
        .into_iter()
        .map(|r| UserPodInfo {
            name: r.userpod_name,
            webid: r.webid.to_string(),
            created_at: r.created_at,
            last_login: r.last_login,
        })
        .collect();
    let active = list
        .iter()
        .find(|r| r.webid == auth.webid.to_string())
        .map(|r| r.name.clone())
        .unwrap_or_default();
    Ok(Json(UserPodListResponse {
        userpods: list,
        active,
    }))
}

/// POST /api/v1/userpods/rename
#[utoipa::path(
    post,
    path = "/api/v1/userpods/rename",
    tag = "userpods",
    request_body = RenameRequest,
    responses(
        (status = 200, description = "UserPod renamed"),
        (status = 400, description = "Invalid request"),
    ),
)]
pub async fn rename_userpod(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    Json(req): Json<RenameRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    store
        .rename_userpod(&req.from, &req.to)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("{e}")))?;
    Ok(Json(
        serde_json::json!({"status": "renamed", "from": req.from, "to": req.to}),
    ))
}

/// DELETE /api/v1/userpods/{name}
#[utoipa::path(
    delete,
    path = "/api/v1/userpods/{name}",
    tag = "userpods",
    params(
        ("name" = String, Path, description = "UserPod name"),
    ),
    responses(
        (status = 200, description = "UserPod deleted"),
        (status = 400, description = "Invalid request"),
    ),
)]
pub async fn delete_userpod(
    State(state): State<ApiState>,
    Extension(_auth): Extension<AuthContext>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user_store = state.agent_service.storage().users.clone();
    let store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    store
        .delete_userpod(&name)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("{e}")))?;
    Ok(Json(serde_json::json!({"status": "deleted", "name": name})))
}

pub fn userpod_router() -> utoipa_axum::router::OpenApiRouter<ApiState> {
    use utoipa_axum::router::OpenApiRouter;
    use utoipa_axum::routes;
    OpenApiRouter::new().routes(routes!(list_userpods, rename_userpod, delete_userpod))
}
