//! Tool-behavior contract tests for `hkask-mcp-curator`.
//!
//! Drives the real `Parameters<T>` tool seam in-process: the server is
//! constructed over an in-memory `SqliteDriver` so the four backing stores
//! (escalation queue, Regulation archive, curator memory) are live, and every
//! tool call goes through `execute_tool` → the `#[tool]` method. This catches
//! wiring regressions that a unit test of the store alone would miss.
//!
//! Covers the testing-standard minimum (docs/reference/mcp-servers/README.md
//! §Testing standard): happy path, invalid input, boundary/edge, and
//! error-specificity — the structured `{"error", "kind"}` envelope must carry
//! the right `McpErrorKind` so callers can route on it.

#![cfg(test)]

use hkask_mcp_curator::types::*;
use hkask_mcp_curator::{CuratorDb, CuratorServer, CuratorStores};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EscalationQueue, HMemStore, RegulationArchive};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

/// Build a `CuratorServer` backed by a single shared in-memory driver, so all
/// four stores see the same data (the production shape — one `curator.db`).
/// Healing is disabled (no path, no passphrase) via `CuratorDb::from_stores`,
/// so the self-heal loop never fires during a test.
fn make_server() -> CuratorServer {
    let driver = SqliteDriver::in_memory_driver();

    // Memory degrades independently — curator recall is entity/EAV based, so
    // the embedding-free constructor matches the production degradation path
    // when an EmbeddingStore is unavailable.
    let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let memory = Arc::new(
        hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store)
            .expect("memory store init"),
    );
    let escalation_queue =
        Arc::new(EscalationQueue::from_driver(driver.clone()).expect("escalation queue init"));
    let regulation_store =
        Arc::new(RegulationArchive::from_driver(driver.clone()).expect("regulation archive init"));

    let stores = CuratorStores {
        escalation_queue: Some(escalation_queue),
        regulation_store: Some(regulation_store),
        memory: Some(memory),
    };
    let database = Arc::new(CuratorDb::from_stores(stores));
    CuratorServer::new(WebID::new(), database)
}

/// Parse a tool output string into JSON, unwrapping the `{"content": ...}`
/// envelope. Error envelopes (`{"error", "kind"}`) have no `content` wrapper
/// and are returned as-is.
fn parse(output: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(output)
        .unwrap_or_else(|| panic!("tool output must be valid JSON, got: {output}"))
}

// ── Liveness ──────────────────────────────────────────────────────────────

/// `curator_ping` returns `status: "ok"` and reports all three stores as live
/// when the DB opened successfully.
#[tokio::test]
async fn ping_reports_store_health() {
    let server = make_server();
    let response = parse(&server.curator_ping(Parameters(PingRequest {})).await);

    assert_eq!(response["status"].as_str(), Some("ok"), "ping must be ok");
    assert_eq!(
        response["stores"]["escalation_queue"].as_bool(),
        Some(true),
        "escalation queue must be live — got: {response}",
    );
    assert_eq!(
        response["stores"]["regulation_store"].as_bool(),
        Some(true),
        "regulation store must be live — got: {response}",
    );
    assert_eq!(
        response["stores"]["memory"].as_bool(),
        Some(true),
        "memory must be live — got: {response}",
    );
}

// ── Escalation management ──────────────────────────────────────────────────

/// `curator_escalations` lists pending escalations — an empty queue is the
/// clean-state happy path.
#[tokio::test]
async fn escalations_lists_pending_empty() {
    let server = make_server();
    let response = parse(&server.curator_escalations(Parameters(PingRequest {})).await);

    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "a fresh queue has zero pending escalations — got: {response}",
    );
    assert!(
        response["escalations"].is_array(),
        "escalations must be an array — got: {response}",
    );
}

/// Resolving a nonexistent escalation must surface a structured `not_found`
/// error, not a generic `internal` or a silent success. The `kind` field is
/// the contract callers route on.
#[tokio::test]
async fn resolve_nonexistent_id_returns_not_found() {
    let server = make_server();
    let response = parse(
        &server
            .curator_escalation_resolve(Parameters(EscalationResolveRequest {
                id: "does-not-exist".to_string(),
                resolution: "tested".to_string(),
            }))
            .await,
    );

    assert_eq!(
        response["kind"].as_str(),
        Some("not_found"),
        "a nonexistent escalation id must classify as not_found, not internal — got: {response}",
    );
    assert!(
        response["error"].as_str().is_some(),
        "the error envelope must carry a message — got: {response}",
    );
}

/// Dismissing a nonexistent escalation must surface a structured `not_found`
/// error — same contract as resolve.
#[tokio::test]
async fn dismiss_nonexistent_id_returns_not_found() {
    let server = make_server();
    let response = parse(
        &server
            .curator_escalation_dismiss(Parameters(EscalationDismissRequest {
                id: "does-not-exist".to_string(),
                reason: "duplicate".to_string(),
            }))
            .await,
    );

    assert_eq!(
        response["kind"].as_str(),
        Some("not_found"),
        "a nonexistent escalation id must classify as not_found — got: {response}",
    );
}

// ── Memory recall — invalid input ──────────────────────────────────────────

/// Naming an `ontology_axis` without an `ontology_value` is a contract
/// violation — the axis is meaningless without a term to match. Must surface
/// `invalid_argument`, not a silent empty result.
#[tokio::test]
async fn memory_recall_ontology_axis_without_value_is_rejected() {
    let server = make_server();
    let response = parse(
        &server
            .curator_memory_recall(Parameters(MemoryRecallRequest {
                entity: "test-entity".to_string(),
                recall_shape: MemoryRecallType::default(),
                ontology_axis: Some("dc_type".to_string()),
                ontology_value: None,
            }))
            .await,
    );

    assert_eq!(
        response["kind"].as_str(),
        Some("invalid_argument"),
        "ontology_axis without ontology_value must be invalid_argument — got: {response}",
    );
}

/// An unknown `ontology_axis` value must be rejected with `invalid_argument`,
/// naming the valid axes, not accepted as a no-op.
#[tokio::test]
async fn memory_recall_rejects_unknown_ontology_axis() {
    let server = make_server();
    let response = parse(
        &server
            .curator_memory_recall(Parameters(MemoryRecallRequest {
                entity: "test-entity".to_string(),
                recall_shape: MemoryRecallType::default(),
                ontology_axis: Some("bogus_axis".to_string()),
                ontology_value: Some("whatever".to_string()),
            }))
            .await,
    );

    assert_eq!(
        response["kind"].as_str(),
        Some("invalid_argument"),
        "an unknown ontology_axis must be invalid_argument — got: {response}",
    );
}

// ── Memory recall — boundary / happy ───────────────────────────────────────

/// Recalling memory for an entity with no stored facts returns a structured
/// response with zero-count sub-objects, not an error. This is the empty-store
/// boundary: "no data" is a valid result, distinct from "store unavailable."
#[tokio::test]
async fn memory_recall_empty_entity_returns_zero_counts() {
    let server = make_server();
    let response = parse(
        &server
            .curator_memory_recall(Parameters(MemoryRecallRequest {
                entity: "never-seen".to_string(),
                recall_shape: MemoryRecallType::Both,
                ontology_axis: None,
                ontology_value: None,
            }))
            .await,
    );

    assert!(
        response.get("error").is_none(),
        "an empty entity is not an error — got: {response}",
    );
    assert_eq!(
        response["perspective_scoped"]["count"].as_u64(),
        Some(0),
        "perspective_scoped count must be zero for an unseen entity — got: {response}",
    );
    assert_eq!(
        response["entity_wide"]["count"].as_u64(),
        Some(0),
        "entity_wide count must be zero for an unseen entity — got: {response}",
    );
}

// ── Semantic search — boundary ──────────────────────────────────────────────

/// A semantic search for a query with no matches returns zero results, not an
/// error. The empty-result boundary.
#[tokio::test]
async fn semantic_search_no_matches_returns_zero() {
    let server = make_server();
    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "no-such-entity".to_string(),
                limit: None,
            }))
            .await,
    );

    assert!(
        response.get("error").is_none(),
        "no matches is not an error — got: {response}",
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "count must be zero for no matches — got: {response}",
    );
    assert!(
        response["results"].is_array(),
        "results must be an array — got: {response}",
    );
}

// ── Algedonic log — happy ───────────────────────────────────────────────────

/// `curator_algedonic_log` returns an empty event list for a fresh archive.
/// The response must carry the window and a (zero-length) events array.
#[tokio::test]
async fn algedonic_log_empty_returns_window_and_events() {
    let server = make_server();
    let response = parse(
        &server
            .curator_algedonic_log(Parameters(AlgedonicLogRequest { hours: Some(24) }))
            .await,
    );

    assert_eq!(
        response["window_hours"].as_u64(),
        Some(24),
        "window_hours must echo the request — got: {response}",
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "a fresh archive has zero algedonic events — got: {response}",
    );
    assert!(
        response["events"].is_array(),
        "events must be an array — got: {response}",
    );
}
