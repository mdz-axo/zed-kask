//! Admin role-gating middleware — restricts admin endpoints to Admin role.
//!
//! REQ: P1-multi-admin-middleware — Admin role gates admin endpoints.

use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use hkask_capability::auth::AuthContext;
use hkask_storage::user_store::UserStore;
use hkask_types::identity::Role;
use std::sync::{Arc, Mutex};

const ADMIN_PATH_PREFIXES: &[&str] = &["/api/v1/admin"];

/// Admin middleware: reject non-Admin requests to admin endpoints.
///
/// expect: "As an admin I am the only one who can access admin configuration"
pub async fn admin_middleware(
    store: Arc<Mutex<UserStore>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let is_admin_path = ADMIN_PATH_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !is_admin_path {
        return Ok(next.run(req).await);
    }
    let auth = req.extensions().get::<AuthContext>();
    let webid = match auth {
        Some(ctx) => ctx.webid,
        None => return Err(StatusCode::UNAUTHORIZED),
    };
    let is_admin = {
        let store = store
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let userpod = store
            .get_userpod_by_webid(&webid)
            .map_err(|_| StatusCode::FORBIDDEN)?
            .ok_or(StatusCode::FORBIDDEN)?;
        let user = store
            .get_user(&userpod.user_id)
            .map_err(|_| StatusCode::FORBIDDEN)?;
        user.role == Role::Admin
    };
    if !is_admin {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(req).await)
}
