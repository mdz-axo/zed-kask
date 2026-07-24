use super::handler::DaemonHandler;
use super::listener::DaemonListener;
use super::protocol::{DaemonRequest, DaemonResponse};
use super::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct MockHandler {
    authenticated: bool,
}

#[async_trait::async_trait]
impl DaemonHandler for MockHandler {
    async fn check_auth(&self, userpod: &str) -> (bool, Option<String>) {
        if self.authenticated {
            (true, Some(format!("webid://{userpod}")))
        } else {
            (false, None)
        }
    }

    async fn check_capability(&self, _userpod: &str, _tool: &str) -> bool {
        true
    }

    async fn store_experience(
        &self,
        _userpod: &str,
        _entity: &str,
        _attribute: &str,
        _value: &serde_json::Value,
        _confidence: Option<f64>,
    ) -> (bool, Option<String>, Option<String>) {
        (
            true,
            Some("ep-001".to_string()),
            Some("sem-001".to_string()),
        )
    }

    async fn dispatch_tool(
        &self,
        _userpod: &str,
        _tool: &str,
        _input: &serde_json::Value,
    ) -> (bool, Option<serde_json::Value>, Option<String>) {
        (true, Some(serde_json::json!({"result": "ok"})), None)
    }

    async fn curator_health(&self, _userpod: &str) -> serde_json::Value {
        serde_json::json!({"status": "healthy"})
    }

    async fn reg_status(&self, _userpod: &str, _domain: Option<&str>) -> serde_json::Value {
        serde_json::json!({"variety": 0})
    }
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

async fn setup_test_listener() -> (DaemonListener, std::path::PathBuf) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let sock_path = std::env::temp_dir().join(format!("hkask-daemon-test-{n}.sock"));
    // Clean up any leftover socket
    let _ = std::fs::remove_file(&sock_path);
    let listener = DaemonListener::with_path(sock_path.clone());
    (listener, sock_path)
}

#[tokio::test]
async fn daemon_auth_query_authenticated() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: true,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    // Give the listener a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);
    let response = client.auth_query("bob").await.unwrap();
    match response {
        DaemonResponse::AuthResponse {
            authenticated,
            webid,
            ..
        } => {
            assert!(authenticated);
            assert_eq!(webid, Some("webid://bob".to_string()));
        }
        other => panic!("Expected AuthResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn daemon_auth_query_unauthenticated() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: false,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);
    let response = client.auth_query("eve").await.unwrap();
    match response {
        DaemonResponse::AuthResponse {
            authenticated,
            action,
            ..
        } => {
            assert!(!authenticated);
            assert_eq!(action, Some("prompt_user".to_string()));
        }
        other => panic!("Expected AuthResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn daemon_capability_query() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: true,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);
    let response = client.capability_query("bob", "web_search").await.unwrap();
    match response {
        DaemonResponse::CapabilityResponse { granted } => assert!(granted),
        other => panic!("Expected CapabilityResponse, got {other:?}"),
    }
}

#[tokio::test]
async fn daemon_store_experience_dual_encoding() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: true,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);
    let response = client
        .store_experience(
            "bob",
            "topic",
            "label",
            &serde_json::json!("test"),
            Some(0.9),
        )
        .await
        .unwrap();
    match response {
        DaemonResponse::StoreResponse {
            stored,
            episodic_id,
            semantic_id,
        } => {
            assert!(stored);
            assert_eq!(episodic_id, Some("ep-001".to_string()));
            assert_eq!(semantic_id, Some("sem-001".to_string()));
        }
        other => panic!("Expected StoreResponse, got {other:?}"),
    }
}

#[test]
fn request_variants_serialize_to_correct_shape() {
    let auth = serde_json::to_value(DaemonRequest::AuthQuery {
        userpod: "alice".into(),
    })
    .unwrap();
    assert_eq!(auth["type"], "auth_query");
    assert_eq!(auth["userpod"], "alice");

    let cap = serde_json::to_value(DaemonRequest::CapabilityQuery {
        userpod: "alice".into(),
        tool: "web_search".into(),
    })
    .unwrap();
    assert_eq!(cap["type"], "capability_query");
    assert_eq!(cap["tool"], "web_search");

    let store = serde_json::to_value(DaemonRequest::StoreExperience {
        userpod: "alice".into(),
        entity: "e1".into(),
        attribute: "a1".into(),
        value: serde_json::json!("v1"),
        confidence: Some(0.9),
    })
    .unwrap();
    assert_eq!(store["type"], "store_experience");
    assert_eq!(store["confidence"], 0.9);

    let dispatch = serde_json::to_value(DaemonRequest::ToolDispatch {
        userpod: "alice".into(),
        tool: "t1".into(),
        input: serde_json::json!({"k": "v"}),
    })
    .unwrap();
    assert_eq!(dispatch["type"], "tool_dispatch");

    let health = serde_json::to_value(DaemonRequest::CuratorHealthQuery {
        userpod: "alice".into(),
    })
    .unwrap();
    assert_eq!(health["type"], "curator_health_query");

    let reg = serde_json::to_value(DaemonRequest::RegStatusQuery {
        userpod: "alice".into(),
        domain: Some("tool".into()),
    })
    .unwrap();
    assert_eq!(reg["type"], "reg_status_query");
}

#[test]
fn response_variants_serialize_to_correct_shape() {
    let auth = serde_json::to_value(DaemonResponse::AuthResponse {
        authenticated: true,
        webid: Some("web://alice".into()),
        action: None,
    })
    .unwrap();
    assert_eq!(auth["type"], "auth_response");
    assert_eq!(auth["authenticated"], true);

    let cap = serde_json::to_value(DaemonResponse::CapabilityResponse { granted: false }).unwrap();
    assert_eq!(cap["type"], "capability_response");

    let err = serde_json::to_value(DaemonResponse::ErrorResponse {
        message: "oops".into(),
    })
    .unwrap();
    assert_eq!(err["type"], "error");

    let store = serde_json::to_value(DaemonResponse::StoreResponse {
        stored: true,
        episodic_id: Some("ep-1".into()),
        semantic_id: Some("sem-1".into()),
    })
    .unwrap();
    assert_eq!(store["type"], "store_response");

    let dispatch = serde_json::to_value(DaemonResponse::ToolDispatchResponse {
        ok: true,
        output: Some(serde_json::json!({"r": 1})),
        error: None,
    })
    .unwrap();
    assert_eq!(dispatch["type"], "tool_dispatch_response");
}

#[test]
fn forward_compat_unknown_fields_tolerated() {
    let json = r#"{"type":"auth_query","userpod":"bob","new_field":"ignored"}"#;
    let req: DaemonRequest = serde_json::from_str(json).unwrap();
    match req {
        DaemonRequest::AuthQuery { userpod } => assert_eq!(userpod, "bob"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn missing_optional_fields() {
    let json = r#"{"type":"auth_query","userpod":"alice"}"#;
    let req: DaemonRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(req, DaemonRequest::AuthQuery { .. }));

    // Confidence is optional
    let json2 =
        r#"{"type":"store_experience","userpod":"bob","entity":"e","attribute":"a","value":"v"}"#;
    let req2: DaemonRequest = serde_json::from_str(json2).unwrap();
    match req2 {
        DaemonRequest::StoreExperience { confidence, .. } => assert!(confidence.is_none()),
        _ => panic!("wrong variant"),
    }

    // Domain is optional
    let json3 = r#"{"type":"reg_status_query","userpod":"bob"}"#;
    let req3: DaemonRequest = serde_json::from_str(json3).unwrap();
    match req3 {
        DaemonRequest::RegStatusQuery { domain, .. } => assert!(domain.is_none()),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn failure_unknown_type_tag() {
    let json = r#"{"type":"nonexistent","userpod":"bob"}"#;
    let result: Result<DaemonRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn failure_missing_required_field() {
    // Missing 'tool' field from a capability_query
    let json = r#"{"type":"capability_query","userpod":"bob"}"#;
    let result: Result<DaemonRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn failure_malformed_json() {
    let json = r#"not json"#;
    let result: Result<DaemonRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[test]
fn failure_wrong_field_type() {
    let json = r#"{"type":"auth_query","userpod":42}"#;
    let result: Result<DaemonRequest, _> = serde_json::from_str(json);
    assert!(result.is_err());
}

#[tokio::test]
async fn daemon_auth_query_is_idempotent() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: true,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);

    let r1 = client.auth_query("bob").await.unwrap();
    let r2 = client.auth_query("bob").await.unwrap();

    match (r1, r2) {
        (
            DaemonResponse::AuthResponse {
                authenticated: a1, ..
            },
            DaemonResponse::AuthResponse {
                authenticated: a2, ..
            },
        ) => {
            assert!(a1);
            assert!(a2);
        }
        _ => panic!("Expected AuthResponse"),
    }
}

#[tokio::test]
async fn daemon_capability_query_is_idempotent() {
    let (mut listener, sock_path) = setup_test_listener().await;
    listener.bind().await.unwrap();
    let handler = Arc::new(MockHandler {
        authenticated: true,
    });
    tokio::spawn(async move {
        listener.serve(handler).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = DaemonClient::with_path(sock_path);

    let r1 = client.capability_query("bob", "web_search").await.unwrap();
    let r2 = client.capability_query("bob", "web_search").await.unwrap();

    match (r1, r2) {
        (
            DaemonResponse::CapabilityResponse { granted: g1 },
            DaemonResponse::CapabilityResponse { granted: g2 },
        ) => {
            assert!(g1);
            assert!(g2);
        }
        _ => panic!("Expected CapabilityResponse"),
    }
}
