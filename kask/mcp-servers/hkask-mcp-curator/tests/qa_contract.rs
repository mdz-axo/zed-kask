//! QA contract tests for hkask-mcp-curator.
//!
//! Instantiates the 7-category contract from
//! kask/docs/qa/per-tool-contracts.md for every tool on the server.
//!
//! Category 7 (adversarial) is N/A for all curator tools — none are LLM
//! I/O boundaries (the server reads the Regulation ledger and memory
//! stores; it does not call an LLM).
//!
//! Category 3 (ocap-denial) is the primary category for curator: every
//! store-backed tool returns `permission_denied` when its store is `None`.
//! This is the OCAP pattern — the tool asserts the store is present before
//! proceeding. The tests assert `permission_denied` (not `reg.guard.*` —
//! Gap B, not wired).
//!
//! `curator_health` and `curator_reg_status` always return `unavailable`
//! (the daemon was removed) — these are tested as error-propagation.

#![cfg(test)]

use hkask_mcp_curator::CuratorServer;
use hkask_mcp_curator::types::*;
use hkask_storage::EscalationQueue;
use hkask_storage::RegulationArchive;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::WebID;
use std::sync::Arc;

// ── Test harness ────────────────────────────────────────────────────────────

/// Build a CuratorServer with no stores — every store-backed tool returns
/// permission_denied. This is the OCAP-denial fixture.
fn make_server_no_stores() -> CuratorServer {
    CuratorServer::new(WebID::new(), None, None, None, None, None)
}

/// Build a CuratorServer with an in-memory EscalationQueue and
/// RegulationArchive. Episodic/Semantic/TokenRegistry remain None.
fn make_server_with_stores() -> CuratorServer {
    let escalation_queue = Arc::new(
        EscalationQueue::from_driver(SqliteDriver::in_memory_driver()).expect("escalation queue"),
    );
    let pool = SqliteDriver::in_memory_pool().expect("pool");
    let regulation_store = Arc::new(RegulationArchive::from_driver(Arc::new(SqliteDriver::new(
        pool,
    ))));
    CuratorServer::new(
        WebID::new(),
        Some(escalation_queue),
        Some(regulation_store),
        None,
        None,
        None,
    )
}

/// Parse a tool's JSON string response, unwrapping the rmcp `content` envelope.
fn parse(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output must be valid JSON");
    if let Some(content) = v.get("content") {
        content.clone()
    } else {
        v
    }
}

/// Assert the response is a structured McpToolError with the given kind.
fn assert_error_kind(out: &str, expected_kind: &str) {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output must be valid JSON");
    let err = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("expected 'error' field, got: {out}"));
    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or_else(|| panic!("expected 'kind' field, got: {out}"));
    assert!(
        !err.is_empty(),
        "error message must not be empty, got: {out}"
    );
    assert_eq!(
        kind, expected_kind,
        "expected kind '{expected_kind}', got '{kind}' in: {out}"
    );
}

/// Construct a Parameters<T> from a JSON value via deserialization.
fn params<T: serde::de::DeserializeOwned>(
    json: serde_json::Value,
) -> rmcp::handler::server::wrapper::Parameters<T> {
    rmcp::handler::server::wrapper::Parameters(
        serde_json::from_value(json).expect("params JSON must deserialize"),
    )
}

// ── curator_ping ────────────────────────────────────────────────────────────

mod curator_ping {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — ping always works (no store required)
        let server = make_server_no_stores();
        let out = server
            .curator_ping(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
        assert_eq!(
            v.get("server").and_then(|s| s.as_str()),
            Some("hkask-mcp-curator")
        );
        let stores = v.get("stores").expect("missing stores");
        assert_eq!(
            stores.get("escalation_queue").and_then(|s| s.as_bool()),
            Some(false)
        );
    }

    #[tokio::test]
    async fn happy_with_stores() {
        // REQ: happy — ping reports stores present
        let server = make_server_with_stores();
        let out = server
            .curator_ping(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        let stores = v.get("stores").expect("missing stores");
        assert_eq!(
            stores.get("escalation_queue").and_then(|s| s.as_bool()),
            Some(true)
        );
        assert_eq!(
            stores.get("regulation_store").and_then(|s| s.as_bool()),
            Some(true)
        );
    }
}

// ── curator_escalations ────────────────────────────────────────────────────

mod curator_escalations {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — empty queue returns count 0
        let server = make_server_with_stores();
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(0));
        assert!(v.get("escalations").and_then(|e| e.as_array()).is_some());
    }

    #[tokio::test]
    async fn ocap_denial_no_queue() {
        // REQ: ocap-denial — no EscalationQueue → permission_denied
        let server = make_server_no_stores();
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn empty_result() {
        // REQ: empty-result — fresh queue has no escalations
        let server = make_server_with_stores();
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        let arr = v
            .get("escalations")
            .and_then(|e| e.as_array())
            .expect("missing array");
        assert!(arr.is_empty());
    }
}

// ── curator_escalation_resolve ─────────────────────────────────────────────

mod curator_escalation_resolve {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_queue() {
        // REQ: ocap-denial
        let server = make_server_no_stores();
        let req = params::<EscalationResolveRequest>(
            serde_json::json!({"id": "nonexistent", "resolution": "fixed"}),
        );
        let out = server.curator_escalation_resolve(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_id() {
        // REQ: error-propagation — valid stores, nonexistent escalation id
        let server = make_server_with_stores();
        let req = params::<EscalationResolveRequest>(
            serde_json::json!({"id": "nonexistent-id", "resolution": "fixed"}),
        );
        let out = server.curator_escalation_resolve(req).await;
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert!(
            v.get("error").is_some(),
            "nonexistent escalation should produce structured error: {out}"
        );
    }

    #[tokio::test]
    async fn schema_violation_missing_id() {
        // REQ: schema-violation
        let raw = serde_json::json!({"resolution": "fixed"});
        let result: Result<EscalationResolveRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "missing 'id' must fail");
    }
}

// ── curator_escalation_dismiss ──────────────────────────────────────────────

mod curator_escalation_dismiss {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_queue() {
        // REQ: ocap-denial
        let server = make_server_no_stores();
        let req = params::<EscalationDismissRequest>(
            serde_json::json!({"id": "x", "reason": "not actionable"}),
        );
        let out = server.curator_escalation_dismiss(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn schema_violation_missing_reason() {
        // REQ: schema-violation
        let raw = serde_json::json!({"id": "x"});
        let result: Result<EscalationDismissRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "missing 'reason' must fail");
    }
}

// ── curator_health ──────────────────────────────────────────────────────────

mod curator_health {
    use super::*;

    #[tokio::test]
    async fn error_propagation_daemon_unavailable() {
        // REQ: error-propagation — daemon removed, always unavailable
        let server = make_server_no_stores();
        let out = server
            .curator_health(params::<PingRequest>(serde_json::json!({})))
            .await;
        assert_error_kind(&out, "unavailable");
    }
}

// ── curator_reg_status ─────────────────────────────────────────────────────

mod curator_reg_status {
    use super::*;

    #[tokio::test]
    async fn error_propagation_daemon_unavailable() {
        // REQ: error-propagation — daemon removed, always unavailable
        let server = make_server_no_stores();
        let out = server
            .curator_reg_status(params::<RegStatusRequest>(
                serde_json::json!({"domain": null}),
            ))
            .await;
        assert_error_kind(&out, "unavailable");
    }
}

// ── curator_semantic_search ────────────────────────────────────────────────

mod curator_semantic_search {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_semantic() {
        // REQ: ocap-denial — no SemanticMemory → permission_denied
        let server = make_server_no_stores();
        let req =
            params::<SemanticSearchRequest>(serde_json::json!({"query": "test", "limit": null}));
        let out = server.curator_semantic_search(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn schema_violation_missing_query() {
        // REQ: schema-violation
        let raw = serde_json::json!({"limit": 10});
        let result: Result<SemanticSearchRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "missing 'query' must fail");
    }
}

// ── curator_memory_recall ───────────────────────────────────────────────────

mod curator_memory_recall {
    use super::*;

    #[tokio::test]
    async fn happy_no_stores() {
        // REQ: happy — with no episodic/semantic, returns unavailable status
        // for each (not a panic, not permission_denied — the tool gracefully
        // reports the missing store)
        let server = make_server_no_stores();
        let req = params::<MemoryRecallRequest>(
            serde_json::json!({"entity": "test_entity", "memory_type": "both"}),
        );
        let out = server.curator_memory_recall(req).await;
        let v = parse(&out);
        let episodic = v.get("episodic").expect("missing episodic");
        assert_eq!(
            episodic.get("status").and_then(|s| s.as_str()),
            Some("unavailable")
        );
        let semantic = v.get("semantic").expect("missing semantic");
        assert_eq!(
            semantic.get("status").and_then(|s| s.as_str()),
            Some("unavailable")
        );
    }

    #[tokio::test]
    async fn schema_violation_missing_entity() {
        // REQ: schema-violation
        let raw = serde_json::json!({"memory_type": "both"});
        let result: Result<MemoryRecallRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "missing 'entity' must fail");
    }
}

// ── curator_algedonic_log ───────────────────────────────────────────────────

mod curator_algedonic_log {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_store() {
        // REQ: ocap-denial — no RegulationArchive → permission_denied
        let server = make_server_no_stores();
        let req = params::<AlgedonicLogRequest>(serde_json::json!({"hours": 24}));
        let out = server.curator_algedonic_log(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn happy_empty() {
        // REQ: happy / empty-result — empty archive returns count 0
        let server = make_server_with_stores();
        let req = params::<AlgedonicLogRequest>(serde_json::json!({"hours": 24}));
        let out = server.curator_algedonic_log(req).await;
        let v = parse(&out);
        assert_eq!(v.get("window_hours").and_then(|h| h.as_u64()), Some(24));
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(0));
    }
}

// ── reg_query ───────────────────────────────────────────────────────────────

mod reg_query {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_store() {
        // REQ: ocap-denial — no RegulationArchive → permission_denied
        let server = make_server_no_stores();
        let req = params::<RegQueryRequest>(
            serde_json::json!({"namespace": null, "window_seconds": null, "limit": null}),
        );
        let out = server.reg_query(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn happy_empty() {
        // REQ: happy / empty-result — empty archive returns 0 events
        let server = make_server_with_stores();
        let req = params::<RegQueryRequest>(
            serde_json::json!({"namespace": null, "window_seconds": 3600, "limit": 100}),
        );
        let out = server.reg_query(req).await;
        let v = parse(&out);
        assert_eq!(v.get("total_events").and_then(|t| t.as_u64()), Some(0));
        assert_eq!(v.get("filtered_count").and_then(|c| c.as_u64()), Some(0));
    }
}

// ── list_tokens ────────────────────────────────────────────────────────────

mod list_tokens {
    use super::*;

    #[tokio::test]
    async fn ocap_denial_no_registry() {
        // REQ: ocap-denial — no TokenRegistry → permission_denied
        let server = make_server_no_stores();
        let req = params::<TokenListRequest>(
            serde_json::json!({"window_seconds": null, "issuer": null, "recipient": null}),
        );
        let out = server.list_tokens(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn schema_violation_extra_unknown_field() {
        // REQ: schema-violation (c) — extra field ignored by serde
        let raw = serde_json::json!({"window_seconds": null, "issuer": null, "recipient": null, "extra": 42});
        let result: Result<TokenListRequest, _> = serde_json::from_value(raw);
        assert!(result.is_ok(), "unknown fields should be ignored");
    }
}
