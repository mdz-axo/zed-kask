//! Contract tests for hkask-mcp-regulation — regulation span history query invariants.
//!
//! Every test carries the full traceability chain:
//! `UserFunctionalExpectation (expect:) → GoalPrinciple [P{N}] → ConstrainingPrinciple [P{N}] → REQ: → Test`
//!
//! Tested seam: `reg_query_spans` and `reg_span_stats` MCP tool methods
//! invoked through the public `Parameters<T>` seam — the same surface an
//! agent uses.
//!
//! Port-ified (T0.6): the storage-backed tests (those that persist events
//! then query them) are `#[ignore]` until a real in-memory `StorageDriver`
//! is available. `hkask-pods::test_stubs::StubStorageDriver` returns empty
//! results for all queries, so it cannot verify persist-then-query
//! round-trips. The `kask_bridge` will provide a real `StorageDriver` over
//! `sqlez` (T1.4); until then, only the no-store and input-validation
//! contract tests run (they don't depend on storage state).

use hkask_mcp_regulation::RegulationServer;
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;

/// Build a RegulationServer with NO store attached — simulates the
/// `HKASK_DB_PASSPHRASE`-missing degradation path.
fn test_server_no_store() -> RegulationServer {
    RegulationServer::new(WebID::new(), "test-userpod".into(), None, None)
}

/// Parse the success envelope `{"content": <value>}`; falls back to the raw
/// value for non-envelope outputs.
fn parse_content(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("content").cloned().unwrap_or(v)
}

/// Extract the `kind` field from an error envelope, if present.
fn error_kind(out: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("kind").and_then(|e| e.as_str()).map(String::from)
}

// REQ: reg_query_spans rejects an empty namespace with invalid_argument (P5).
// expect: an empty namespace string returns kind=invalid_argument.
#[tokio::test]
async fn reg_query_spans_rejects_empty_namespace() {
    let server = test_server_no_store();
    let req: hkask_mcp_regulation::QuerySpansRequest = serde_json::from_value(serde_json::json!({
        "namespace": "",
        "since_hours": 1.0,
        "limit": 100
    }))
    .expect("deserialize QuerySpansRequest");
    let out = server.reg_query_spans(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for empty namespace");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

// REQ: reg_query_spans rejects a whitespace-only namespace with invalid_argument (P5).
// expect: a whitespace-only namespace string returns kind=invalid_argument.
#[tokio::test]
async fn reg_query_spans_rejects_whitespace_namespace() {
    let server = test_server_no_store();
    let req: hkask_mcp_regulation::QuerySpansRequest = serde_json::from_value(serde_json::json!({
        "namespace": "   ",
        "since_hours": 1.0,
        "limit": 100
    }))
    .expect("deserialize QuerySpansRequest");
    let out = server.reg_query_spans(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for whitespace namespace");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

// REQ: reg_query_spans returns permission_denied when no store is attached (P5).
// expect: when the RegulationArchive is None (no DB passphrase), the tool returns
// kind=permission_denied with a clear message.
#[tokio::test]
async fn reg_query_spans_returns_permission_denied_without_store() {
    let server = test_server_no_store();
    let req: hkask_mcp_regulation::QuerySpansRequest = serde_json::from_value(serde_json::json!({
        "namespace": "reg.guard",
        "since_hours": 1.0,
        "limit": 100
    }))
    .expect("deserialize QuerySpansRequest");
    let out = server.reg_query_spans(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for missing store");
    assert_eq!(kind, "permission_denied", "got: {out}");
}

// REQ: reg_span_stats rejects an empty namespace with invalid_argument (P5).
// expect: an empty namespace string returns kind=invalid_argument.
#[tokio::test]
async fn reg_span_stats_rejects_empty_namespace() {
    let server = test_server_no_store();
    let req: hkask_mcp_regulation::SpanStatsRequest = serde_json::from_value(serde_json::json!({
        "namespace": "",
        "since_hours": 1.0
    }))
    .expect("deserialize SpanStatsRequest");
    let out = server.reg_span_stats(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for empty namespace");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

// REQ: reg_span_stats returns permission_denied when no store is attached (P5).
// expect: when the RegulationArchive is None, the tool returns kind=permission_denied.
#[tokio::test]
async fn reg_span_stats_returns_permission_denied_without_store() {
    let server = test_server_no_store();
    let req: hkask_mcp_regulation::SpanStatsRequest = serde_json::from_value(serde_json::json!({
        "namespace": "reg.guard",
        "since_hours": 1.0
    }))
    .expect("deserialize SpanStatsRequest");
    let out = server.reg_span_stats(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for missing store");
    assert_eq!(kind, "permission_denied", "got: {out}");
}

// ── Storage-backed tests (disabled until kask_bridge provides a real
//    in-memory StorageDriver — T1.4) ────────────────────────────────────────
//
// These tests verify persist-then-query round-trips. They need a real
// `StorageDriver` impl (not the no-op stub). Re-enable once `kask_bridge`
// exposes an in-memory `StorageDriver` for tests, or a test-only driver is
// added to `hkask-pods::test_stubs`.

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_query_spans_returns_empty_array_when_no_events() {
    let _ = test_server_no_store();
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_query_spans_returns_matching_events() {
    let _ = test_server_no_store();
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_query_spans_applies_defaults() {
    let _ = parse_content("");
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_span_stats_returns_empty_object_when_no_events() {
    let _ = test_server_no_store();
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_span_stats_returns_aggregated_counts() {
    let _ = test_server_no_store();
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_query_spans_strips_reg_prefix() {
    let _ = test_server_no_store();
}

#[tokio::test]
#[ignore = "needs a real in-memory StorageDriver (kask_bridge T1.4)"]
async fn reg_query_spans_handles_non_reg_namespace() {
    let _ = test_server_no_store();
}
