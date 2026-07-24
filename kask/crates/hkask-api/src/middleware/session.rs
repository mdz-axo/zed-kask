//! Session cookie middleware — authenticates users via `hkask_session` cookie.
//!
//! # REQ: P1-deploy-session-middleware — session cookie auth coexists with capability token auth.
//! expect: "My session cookie coexists with capability token authentication"
//! Runs BEFORE the capability token middleware. If a valid session cookie is found,
//! injects an `AuthContext` into request extensions so the capability token middleware
//! can skip Bearer token verification.

use crate::middleware::auth::AuthContext;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::Response,
};
use cookie::Cookie;
use hkask_storage::user_store::UserStore;
use std::sync::{Arc, Mutex};

/// Extract a cookie value by name from request headers.
pub(crate) fn extract_cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    Cookie::split_parse(cookie_header)
        .filter_map(|c| c.ok())
        .find(|c| c.name() == name)
        .map(|c| c.value().to_owned())
}

/// Session middleware implementation — called from a closure in `create_router`.
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  user_store is a valid ``Arc<Mutex<UserStore>>``
/// post: if valid session cookie → AuthContext injected, next.run called
/// post: if expired session cookie → 401 with clear error
/// post: if missing cookie → pass through (capability token path may handle it)
pub async fn session_middleware_impl(
    user_store: &Arc<Mutex<UserStore>>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Check for session cookie — if absent, pass through to capability token auth
    let session_id = match extract_cookie(req.headers(), "hkask_session") {
        Some(id) => id,
        None => return next.run(req).await,
    };

    // Load session from UserStore — drop lock before any await
    let session_result = {
        let store = user_store.lock();
        match store {
            Ok(s) => s.get_session(&session_id),
            Err(_) => Err(hkask_storage::user_store::UserStoreError::Infra(
                hkask_types::InfrastructureError::LockPoisoned,
            )),
        }
    }; // MutexGuard dropped here

    let session = match session_result {
        Ok(Some(s)) => s,
        _ => {
            // Invalid or missing session — return 401 with clear message
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(
                    header::SET_COOKIE,
                    "hkask_session=; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=0",
                )
                .body(Body::from(
                    "Session invalid or expired. Please sign in again.",
                ))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    // Check expiry — clear expired session cookie and return 401
    let now = chrono::Utc::now().timestamp();
    if session.is_expired(now) {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(
                header::SET_COOKIE,
                "hkask_session=; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=0",
            )
            .body(Body::from(
                "Session invalid or expired. Please sign in again.",
            ))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    }

    // Inject AuthContext
    let webid = session.webid;
    req.extensions_mut()
        .insert(AuthContext::from_session(webid));

    next.run(req).await
}
