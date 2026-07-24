//! Integration tests for hkask-communication public API.
//!
//! Tests the public seams: types (RoomId, UserId, Thread, MatrixMessage),
//! AgentRegistry (WebID↔UserId mapping, thread watchlists), and error types.
//!
//! MatrixTransport tests require a running Conduit homeserver (Docker sidecar).
//! Those are deferred to the integration test suite that runs with `./scripts/conduit-docker.sh start`.
//! See OPEN_QUESTIONS.md §hkask-communication-matrix-transport-tests.

use hkask_communication::agent_registration::{AgentRegistrationError, AgentRegistry};
use hkask_communication::matrix::{MatrixError, MatrixMessage, RoomId, Thread, UserId};
use hkask_types::WebID;

// ── Type tests ───────────────────────────────────────────────────────────────

#[test]
fn room_id_newtype_round_trip() {
    let id = RoomId::new("!abc123:localhost");
    assert_eq!(id.as_str(), "!abc123:localhost");
}

#[test]
fn room_id_equality() {
    let a = RoomId::new("!abc123:localhost");
    let b = RoomId::new("!abc123:localhost");
    let c = RoomId::new("!xyz789:localhost");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn user_id_newtype_round_trip() {
    let id = UserId::new("@agent:localhost");
    assert_eq!(id.as_str(), "@agent:localhost");
}

#[test]
fn user_id_equality() {
    let a = UserId::new("@alice:localhost");
    let b = UserId::new("@alice:localhost");
    let c = UserId::new("@bob:localhost");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn thread_struct_fields() {
    let thread = Thread {
        room_id: RoomId::new("!room1:localhost"),
        title: "Test Thread".to_string(),
        participants: vec![
            UserId::new("@alice:localhost"),
            UserId::new("@bob:localhost"),
        ],
        monitored_by: vec![UserId::new("@curator:localhost")],
        escalated: false,
        created_at: 1000,
    };
    assert_eq!(thread.room_id.as_str(), "!room1:localhost");
    assert_eq!(thread.title, "Test Thread");
    assert_eq!(thread.participants.len(), 2);
    assert_eq!(thread.monitored_by.len(), 1);
    assert!(!thread.escalated);
    assert_eq!(thread.created_at, 1000);
}

#[test]
fn matrix_message_fields() {
    let msg = MatrixMessage {
        event_id: "$event:localhost".to_string(),
        sender: UserId::new("@alice:localhost"),
        body: "Hello, world!".to_string(),
        structured: Some(serde_json::json!({"type": "greeting"})),
        timestamp: 1700000000000,
    };
    assert_eq!(msg.event_id, "$event:localhost");
    assert_eq!(msg.sender.as_str(), "@alice:localhost");
    assert_eq!(msg.body, "Hello, world!");
    assert!(msg.structured.is_some());
    assert_eq!(msg.timestamp, 1700000000000);
}

#[test]
fn matrix_message_no_structured() {
    let msg = MatrixMessage {
        event_id: "$bot-event:localhost".to_string(),
        sender: UserId::new("@bot:localhost"),
        body: "Plain text".to_string(),
        structured: None,
        timestamp: 0,
    };
    assert!(msg.structured.is_none());
}

// ── Error type tests ─────────────────────────────────────────────────────────

#[test]
fn matrix_error_not_logged_in() {
    let err = MatrixError::NotLoggedIn;
    assert!(err.to_string().contains("Not logged in"));
}

#[test]
fn matrix_error_auth_carries_reason() {
    let err = MatrixError::Auth("Invalid password".to_string());
    assert!(err.to_string().contains("Invalid password"));
}

#[test]
fn matrix_error_unavailable_carries_reason() {
    let err = MatrixError::Unavailable("Connection refused".to_string());
    assert!(err.to_string().contains("Connection refused"));
}

#[test]
fn agent_registration_error_not_registered() {
    let err = AgentRegistrationError::NotRegistered("alice-webid".to_string());
    assert!(err.to_string().contains("alice-webid"));
}

// ── AgentRegistry tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn registry_record_and_resolve() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();
    let user_id = UserId::new("@alice:localhost");

    registry.record_mapping(&webid, &user_id).await;

    let resolved = registry.resolve(&webid).await;
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().as_str(), "@alice:localhost");
}

#[tokio::test]
async fn registry_resolve_unregistered_returns_none() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();

    let resolved = registry.resolve(&webid).await;
    assert!(resolved.is_none());
}

#[tokio::test]
async fn registry_deregister_removes_mapping() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();
    let user_id = UserId::new("@alice:localhost");

    registry.record_mapping(&webid, &user_id).await;
    assert!(registry.resolve(&webid).await.is_some());

    registry.deregister(&webid).await.unwrap();
    assert!(registry.resolve(&webid).await.is_none());
}

#[tokio::test]
async fn registry_deregister_unregistered_is_noop() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();

    let result = registry.deregister(&webid).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn registry_monitor_thread_requires_registration() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();
    let room_id = RoomId::new("!room1:localhost");

    let result = registry.monitor_thread(&webid, &room_id).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AgentRegistrationError::NotRegistered(_) => {} // expected
        other => panic!("Expected NotRegistered, got: {:?}", other),
    }
}

#[tokio::test]
async fn registry_monitor_thread_succeeds_for_registered_agent() {
    let registry = AgentRegistry::new();
    let webid = WebID::new();
    let user_id = UserId::new("@alice:localhost");
    let room_id = RoomId::new("!room1:localhost");

    registry.record_mapping(&webid, &user_id).await;
    let result = registry.monitor_thread(&webid, &room_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn registry_get_watchers_returns_monitoring_agents() {
    let registry = AgentRegistry::new();
    let alice = WebID::new();
    let bob = WebID::new();
    let room_id = RoomId::new("!room1:localhost");

    registry
        .record_mapping(&alice, &UserId::new("@alice:localhost"))
        .await;
    registry
        .record_mapping(&bob, &UserId::new("@bob:localhost"))
        .await;
    registry.monitor_thread(&alice, &room_id).await.unwrap();
    registry.monitor_thread(&bob, &room_id).await.unwrap();

    let watchers = registry.get_watchers(&room_id).await;
    assert_eq!(watchers.len(), 2);
}

#[tokio::test]
async fn registry_get_watchers_empty_for_unmonitored_thread() {
    let registry = AgentRegistry::new();
    let room_id = RoomId::new("!unmonitored:localhost");

    let watchers = registry.get_watchers(&room_id).await;
    assert!(watchers.is_empty());
}

// ── SevenR7Listener Lifecycle Tests ───────────────────────────────────────

#[test]
fn listener_new_creates_without_panic() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let _listener = SevenR7Listener::new(transport, 30);
}

#[test]
fn listener_new_accepts_various_intervals() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let _fast = SevenR7Listener::new(Arc::clone(&transport), 1);
    let _slow = SevenR7Listener::new(Arc::clone(&transport), 3600);
}

#[tokio::test]
async fn listener_start_does_not_panic() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let listener = SevenR7Listener::new(transport, 30);

    listener.start().await;
}

#[tokio::test]
async fn listener_start_is_idempotent() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let listener = SevenR7Listener::new(transport, 30);

    listener.start().await;
    listener.start().await; // second start should be no-op
}

#[tokio::test]
async fn listener_stop_does_not_panic() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let listener = SevenR7Listener::new(transport, 30);

    listener.start().await;
    listener.stop().await;
}

#[tokio::test]
async fn listener_stop_before_start_does_not_panic() {
    use hkask_communication::listener::SevenR7Listener;
    use hkask_communication::matrix::MatrixTransport;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let transport = Arc::new(Mutex::new(MatrixTransport::new("http://localhost:8008")));
    let listener = SevenR7Listener::new(transport, 30);

    listener.stop().await; // stop before start should be safe
}
