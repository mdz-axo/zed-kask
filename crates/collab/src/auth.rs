use crate::entities::User;
use crate::{AppState, Error, db::UserId, rpc::Principal};
use anyhow::Context as _;
use axum::{
    http::{self, Request, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use cloud_api_types::GetAuthenticatedUserResponse;
pub use rpc::auth::random_token;
use std::sync::Arc;

/// Validates the authorization header and adds an Extension<Principal> to the request.
/// Authorization: <user-id> <token>
///   <token> is the access_token attached to that user.
/// Authorization: "dev-server-token" <token>
pub async fn validate_header<B>(mut req: Request<B>, next: Next<B>) -> impl IntoResponse {
    let mut auth_header = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .ok_or_else(|| {
            Error::http(
                StatusCode::UNAUTHORIZED,
                "missing authorization header".to_string(),
            )
        })?
        .split_whitespace();

    let state = req.extensions().get::<Arc<AppState>>().unwrap();

    let first = auth_header.next().unwrap_or("");
    if first == "dev-server-token" {
        Err(Error::http(
            StatusCode::UNAUTHORIZED,
            "Dev servers were removed in Zed 0.157 please upgrade to SSH remoting".to_string(),
        ))?;
    }

    let user_id = UserId(first.parse().map_err(|_| {
        Error::http(
            StatusCode::BAD_REQUEST,
            "missing user id in authorization header".to_string(),
        )
    })?);

    let access_token = auth_header.next().ok_or_else(|| {
        Error::http(
            StatusCode::BAD_REQUEST,
            "missing access token in authorization header".to_string(),
        )
    })?;

    let http_client = state.http_client.clone().expect("no HTTP client");

    let response = http_client
        .get(format!("{}/client/users/me", state.config.zed_cloud_url()))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("{user_id} {access_token}"))
        .send()
        .await
        .context("failed to validate access token")?;
    if let Ok(response) = response.error_for_status() {
        let response_body: GetAuthenticatedUserResponse = response
            .json()
            .await
            .context("failed to parse response body")?;

        let user = User {
            id: UserId(response_body.user.legacy_user_id),
            username: response_body.user.username,
            github_login: response_body.user.github_login,
            avatar_url: response_body.user.avatar_url,
            name: response_body.user.name,
            admin: response_body.user.is_staff,
            connected_once: response_body.user.has_connected_to_collab_once,
        };

        req.extensions_mut().insert(Principal::User(user));
        return Ok::<_, Error>(next.run(req).await);
    }

    Err(Error::http(
        StatusCode::UNAUTHORIZED,
        "invalid credentials".to_string(),
    ))
}

// zed-kask: D30 — Development-only auth bypass for the kask-skills API router.
//
// In local dev the collab server is often run without a local Zed Cloud
// (`cd ../cloud; cargo make dev`), so `validate_header` cannot reach
// `/client/users/me` to resolve a real `Principal`. Without a `Principal`, the
// upload/vote/delete handlers fail with a missing-extension error before the
// request reaches the handler body, so the publish pipeline is dead.
//
// This middleware runs only when `Config::is_development()`. It parses the
// user id from the `Authorization: <user-id> <token>` header (best-effort —
// the token is not validated; there is no Cloud to validate against) and
// inserts a `Principal::User` with a dev `User` whose `username` is `local-dev`.
// The publisher's S3 namespace is verified separately by the upload/delete
// handlers' dev-mode relaxation of the namespace check (see `api/kask_skills.rs`).
//
// Production deployments never use this — `validate_header` is wired onto the
// kask-skills router only in non-development environments (see `router()` in
// `api/kask_skills.rs`).
pub async fn dev_validate_header<B>(mut req: Request<B>, next: Next<B>) -> impl IntoResponse {
    let state = req.extensions().get::<Arc<AppState>>().cloned();
    let is_dev = state
        .as_ref()
        .map(|s| s.config.is_development())
        .unwrap_or(false);
    match dev_principal(is_dev, req.headers()) {
        Some(principal) => {
            req.extensions_mut().insert(principal);
            Ok::<_, Error>(next.run(req).await)
        }
        None => Err(Error::http(
            StatusCode::NOT_IMPLEMENTED,
            "dev auth bypass is only available in development".to_string(),
        )),
    }
}

/// zed-kask: D30 — Pure decision behind `dev_validate_header`, extracted for
/// testability (a live `AppState` + `Next` is heavy to construct in a test;
/// this helper takes the resolved `is_dev` flag + the headers and returns the
/// `Principal` to insert, mirroring the `kask_skill_table_statements` pure-fn
/// extraction pattern).
///
/// Returns `Some(Principal::User { username: "local-dev", .. })` in
/// development (the bypass synthesizes a dev `Principal` from the
/// `Authorization` header's user-id; the token is not validated — local dev
/// only), and `None` otherwise (the router must not wire this middleware in
/// prod, but fail closed if it does). A missing/malformed header falls back to
/// user-id 0 rather than refusing, so a dev client that hasn't logged in can
/// still exercise the publish path. Never panics on arbitrary header bytes.
pub fn dev_principal(is_dev: bool, headers: &http::HeaderMap) -> Option<Principal> {
    if !is_dev {
        return None;
    }

    // Best-effort user id parse. A missing/malformed header falls back to 0.
    let user_id = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.split_whitespace().next())
        .and_then(|first| first.parse::<i32>().ok())
        .unwrap_or(0);

    let dev_user = User {
        id: UserId(user_id),
        username: "local-dev".to_string(),
        github_login: "local-dev".to_string(),
        avatar_url: String::new(),
        name: Some("Local Dev".to_string()),
        // zed-kask: `admin: false` — no kask-skills handler checks `admin`, so
        // `true` would be a latent privilege grant for any future admin-only
        // path. The dev bypass exists to insert *a* `Principal` so the
        // `Extension<Principal>` extractor doesn't fail; it does not elevate.
        admin: false,
        connected_once: true,
    };
    Some(Principal::User(dev_user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::proptest::prelude::*;

    // zed-kask: D30 — the dev bypass inserts a `Principal` iff `is_dev`,
    // regardless of the `Authorization` header's shape (valid, missing,
    // malformed, arbitrary bytes). Never panics. Pinned by proptest over
    // arbitrary header bytes so a malformed header cannot crash the middleware.
    proptest! {
        #[test]
        fn dev_principal_inserts_only_in_dev_for_any_header(
            is_dev in prop::bool::ANY,
            // Arbitrary bytes for the Authorization header — covers valid
            // (`"42 tok"`), missing (None), non-UTF8, and garbage shapes.
            auth_bytes in prop::collection::vec(prop::num::u8::ANY, 0..32),
        ) {
            let mut headers = http::HeaderMap::new();
            if let Ok(value) = http::HeaderValue::from_bytes(&auth_bytes) {
                headers.insert(http::header::AUTHORIZATION, value);
            }
            // The proptest must not panic on any header shape.
            let principal = dev_principal(is_dev, &headers);
            if is_dev {
                let principal = principal.expect("dev must yield a Principal");
                let user = match principal {
                    Principal::User(u) => u,
                };
                assert_eq!(user.username, "local-dev", "dev principal username is fixed");
                assert_eq!(user.github_login, "local-dev");
                assert!(!user.admin, "dev principal must not be admin (no elevation)");
            } else {
                assert!(principal.is_none(), "prod must not yield a Principal (fail closed)");
            }
        }
    }
}
