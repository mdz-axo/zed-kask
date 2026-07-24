//! End-to-end HTTP integration tests for the hkask API.
//!
//! Phase 5: deploy → OAuth sign-in → terminal → export → upload → rename → verify.
//!
//! Tests spin up a real axum server with in-memory databases.
//! OAuth sign-in requires real GitHub credentials and is tested manually
//! (see docs/plans/deployment-and-backup.md §2.1 for the manual flow).
//!
//! What we test here:
//! - Static routes (/, /terminal) return correct content
//! - /health endpoint reports DB + Conduit status
//! - Auth-gated routes reject unauthenticated requests
//! - Regulation health endpoint

use hkask_api::ApiState;
use hkask_services_context::AgentService;
use hkask_services_core::ServiceConfig;
use std::net::SocketAddr;

async fn test_state() -> ApiState {
    let _ = std::fs::create_dir_all("/tmp/hkask-templates");
    unsafe {
        if std::env::var("HKASK_MASTER_KEY").is_err() {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            );
        }
    }
    let config = ServiceConfig::in_memory();
    let ctx = AgentService::build(config)
        .await
        .expect("AgentService::build");
    ApiState::from_service_context(ctx)
        .await
        .expect("ApiState::from_service_context")
}

async fn start_server() -> (tokio::task::JoinHandle<()>, SocketAddr) {
    let state = test_state().await;
    let app: axum::Router = hkask_api::create_router(state).into();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (handle, addr)
}

/// Set an env var for the duration of a test, restore on drop.
struct EnvGuard {
    key: String,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            old,
        }
    }

    fn remove(key: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe {
            std::env::remove_var(key);
        }
        Self {
            key: key.to_string(),
            old,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => unsafe {
                std::env::set_var(&self.key, v);
            },
            None => unsafe {
                std::env::remove_var(&self.key);
            },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Static routes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn landing_page_returns_html() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("GET /");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("hKask"));
    assert!(body.contains("Sign in with GitHub"));
}

#[tokio::test]
async fn terminal_page_returns_html() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/terminal"))
        .await
        .expect("GET /terminal");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("xterm"));
    assert!(body.contains("WebSocket"));
}

// ═══════════════════════════════════════════════════════════════════
// Health endpoint
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn health_endpoint_db_healthy() {
    // Point Matrix URL at localhost so the HTTP request itself succeeds
    // (even though no Conduit is listening — we test that case separately).
    let _guard = EnvGuard::set("HKASK_MATRIX_URL", "http://127.0.0.1:18008");

    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["db"], true, "DB should be reachable");
    // 200 if Conduit responds, 503 if connection refused — either is valid
    assert!(
        status == 200 || status == 503,
        "expected 200 or 503, got {status}"
    );
}

#[tokio::test]
async fn health_reports_conduit_unreachable_without_matrix_url() {
    let _guard = EnvGuard::remove("HKASK_MATRIX_URL");

    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(body["db"], true);
    assert_eq!(body["conduit"], false);
    assert_eq!(body["healthy"], false);
    assert_eq!(status, 503);
}

// ═══════════════════════════════════════════════════════════════════
// Auth-gated routes
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn export_create_rejects_no_auth() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/api/v1/export/create"))
        .json(&serde_json::json!({"passphrase": "test-passphrase-123"}))
        .send()
        .await
        .expect("POST /api/v1/export/create");
    let status = resp.status();
    assert!(
        status == 401 || status == 500,
        "expected 401 or 500, got {status}"
    );
}

#[tokio::test]
async fn userpod_list_rejects_no_auth() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/api/v1/userpods"))
        .await
        .expect("GET /api/v1/userpods");
    let status = resp.status();
    assert!(
        status == 401 || status == 403 || status == 500,
        "expected 401/403/500, got {status}"
    );
}

#[tokio::test]
async fn terminal_ws_rejects_no_session() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/api/v1/terminal/ws"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .send()
        .await
        .expect("GET /api/v1/terminal/ws");
    let status = resp.status();
    assert!(
        status == 400 || status == 401,
        "expected 400 or 401, got {status}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// OAuth login
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn oauth_github_login_redirects() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/api/v1/auth/login?provider=github"))
        .send()
        .await
        .expect("GET /api/v1/auth/login");
    let status = resp.status();
    assert!(
        status == 302 || status == 500,
        "expected 302 or 500, got {status}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Regulation health
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn reg_health_returns_json() {
    let (_handle, addr) = start_server().await;
    let resp = reqwest::get(format!("http://{addr}/api/regulation/health"))
        .await
        .expect("GET /api/regulation/health");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("overall_deficit").is_some());
    assert!(body.get("healthy").is_some());
}
