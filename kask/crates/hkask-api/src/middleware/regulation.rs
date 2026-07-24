//! Regulation span middleware — emits tracing spans on every HTTP request.
//!
//! Applied as the outermost middleware layer so all requests are captured
//! regardless of auth or route-level filtering.

use axum::{body::Body, http::Request, middleware::Next, response::Response};
use std::time::Instant;

/// Regulation middleware — emits `request` and `response` spans for every HTTP request.
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  req is an incoming HTTP request
/// post: reg.api.request span emitted with method + path
/// post: reg.api response span emitted with status + latency_ms
pub async fn regulation_middleware(req: Request<Body>, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "request", method = %method, path = %path, "REG");

    let response = next.run(req).await;

    let status = response.status().as_u16();
    // P9: Regulation span
    tracing::info!(target: "hkask.api", operation = "response", status = status, latency_ms = start.elapsed().as_millis(), "REG");

    response
}
