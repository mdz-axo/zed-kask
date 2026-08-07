//! QA contract tests for hkask-mcp-curator.
//!
//! Instantiates the 7-category contract from
//! kask/docs/qa/per-tool-contracts.md for every tool on the server.
//!
//! Category 7 (adversarial) is N/A for all curator tools — none are LLM
//! I/O boundaries (the server reads the Regulation ledger and memory
//! stores; it does not call an LLM).
//!
//! Category 3 (dependency-denial) is the primary category for curator: every
//! store-backed tool returns `permission_denied` when its store is `None`.
//! This is the store-presence guard pattern — the tool asserts the store is present before
//! proceeding. The tests assert `permission_denied` (not `reg.guard.*` —
//! Gap B, not wired).

#![cfg(test)]

use hkask_mcp_curator::types::*;
use hkask_mcp_curator::{CuratorDb, CuratorServer, CuratorStores};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EscalationQueue, RegulationArchive};
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::regulation::RegulationSpan;
use hkask_types::{BotID, TemplateID, WebID};
use std::sync::Arc;

// ── Test harness ────────────────────────────────────────────────────────────

/// Build a CuratorServer with no stores — every store-backed tool returns
/// permission_denied. This is the dependency-denial fixture.
fn make_server_no_stores() -> CuratorServer {
    CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::for_tests(CuratorStores::empty())),
    )
}

/// Build a CuratorServer with an in-memory EscalationQueue and
/// RegulationArchive. Episodic/Semantic remain None.
fn make_server_with_stores() -> CuratorServer {
    let escalation_queue = Arc::new(
        EscalationQueue::from_driver(SqliteDriver::in_memory_driver()).expect("escalation queue"),
    );
    let pool = SqliteDriver::in_memory_pool().expect("pool");
    let regulation_store = Arc::new(
        RegulationArchive::from_driver(Arc::new(SqliteDriver::new(pool)))
            .expect("regulation archive init"),
    );
    CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::for_tests(CuratorStores {
            escalation_queue: Some(escalation_queue),
            regulation_store: Some(regulation_store),
            ..CuratorStores::empty()
        })),
    )
}

/// Build a CuratorServer whose memory holds one ontology-anchored h_mem:
/// a `bibo:Article` semantic fact subject-tagged `ROIC` and namespace-tagged
/// `fibo`. Used to exercise ontology-axis recall.
fn make_server_with_ontology_h_mem() -> CuratorServer {
    let h_mem_store = hkask_storage::HMemStore::from_driver(SqliteDriver::in_memory_driver())
        .expect("hmem store");
    let memory = Arc::new(
        hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store)
            .expect("embedding-free memory store"),
    );
    let owner = WebID::new();
    let h_mem = hkask_storage::HMem::new(
        "company:Apple",
        "roic",
        serde_json::json!(0.32),
        owner,
    )
    .with_visibility(hkask_types::Visibility::Shared)
    .with_ontology(
        hkask_types::HMemOntology::semantic("bibo:Article", vec!["ROIC".to_string()], "10-K")
            .with_ontology_tag("fibo", "return on invested capital"),
    );
    memory.store(h_mem).expect("store ontology h_mem");
    CuratorServer::new(
        owner,
        Arc::new(CuratorDb::for_tests(CuratorStores {
            memory: Some(memory),
            ..CuratorStores::empty()
        })),
    )
}

/// Build a CuratorServer whose memory store has NO embedding capability —
/// the shape `open_curator_stores` produces when `EmbeddingStore::from_driver`
/// fails. Every curator memory tool recalls by entity/EAV, so all of them must
/// still work.
fn make_server_with_embedding_free_memory() -> CuratorServer {
    let memory = Arc::new(
        hkask_memory::MemoryStore::try_new_without_embeddings(
            hkask_storage::HMemStore::from_driver(SqliteDriver::in_memory_driver())
                .expect("hmem store"),
        )
        .expect("embedding-free memory store"),
    );
    CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::for_tests(CuratorStores {
            memory: Some(memory),
            ..CuratorStores::empty()
        })),
    )
}

/// Build a CuratorServer with an in-memory EscalationQueue pre-populated
/// with `count` pending escalations, plus a RegulationArchive. Returns the
/// server and the added escalation ids.
fn make_server_with_escalations(count: usize) -> (CuratorServer, Vec<String>) {
    let escalation_queue = Arc::new(
        EscalationQueue::from_driver(SqliteDriver::in_memory_driver()).expect("escalation queue"),
    );
    let mut ids = Vec::new();
    for _ in 0..count {
        let id = escalation_queue
            .add(
                TemplateID::new(),
                BotID::new(),
                "test output".into(),
                0.9,
                0,
                "probe".into(),
            )
            .expect("add escalation");
        ids.push(id.to_string());
    }
    let pool = SqliteDriver::in_memory_pool().expect("pool");
    let regulation_store = Arc::new(
        RegulationArchive::from_driver(Arc::new(SqliteDriver::new(pool)))
            .expect("regulation archive init"),
    );
    let server = CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::for_tests(CuratorStores {
            escalation_queue: Some(escalation_queue),
            regulation_store: Some(regulation_store),
            ..CuratorStores::empty()
        })),
    );
    (server, ids)
}

/// Build a CuratorServer with a RegulationArchive pre-populated with
/// `count` Regulation events.
fn make_server_with_archive_events(count: usize) -> CuratorServer {
    let escalation_queue = Arc::new(
        EscalationQueue::from_driver(SqliteDriver::in_memory_driver()).expect("escalation queue"),
    );
    let pool = SqliteDriver::in_memory_pool().expect("pool");
    let regulation_store = Arc::new(
        RegulationArchive::from_driver(Arc::new(SqliteDriver::new(pool)))
            .expect("regulation archive init"),
    );
    for i in 0..count {
        persist_regulation_event(&regulation_store, &format!("probe_{i}"));
    }
    CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::for_tests(CuratorStores {
            escalation_queue: Some(escalation_queue),
            regulation_store: Some(regulation_store),
            ..CuratorStores::empty()
        })),
    )
}

/// Persist a synthetic Regulation event through the archive's
/// `RegulationSink` surface, as the escalation resolve/dismiss paths do.
/// Uses the `gas` span category so the event is visible to the algedonic
/// replay (`query_algedonic` filters on `ALGEDONIC_SPAN_CATEGORIES`, which
/// does not include `curation`).
fn persist_regulation_event(store: &RegulationArchive, operation: &str) {
    let ns = SpanNamespace::try_from(RegulationSpan::Gas).expect("canonical span");
    let record = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        Span::new(ns, operation),
        CyclePhase::Act,
        serde_json::json!({"probe": operation}),
        0,
    );
    store.persist(&record).expect("regulation event persist");
}

/// Parse a tool's JSON string response, unwrapping the rmcp `content` envelope
/// via the canonical `hkask_types::tool_response::parse_tool_response` seam.
fn parse(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output must be valid JSON")
}

// ── Self-healing ────────────────────────────────────────────────────────────

mod self_healing {
    use super::*;

    /// Regression pin for the self-healing curator DB: when the stores are
    /// down, tools return permission_denied; after the stores heal
    /// (simulated via `set_for_tests`), the same server instance serves the
    /// tool without a restart.
    #[tokio::test]
    async fn tools_recover_after_stores_heal() {
        let db = Arc::new(CuratorDb::for_tests(CuratorStores::empty()));
        let server = CuratorServer::new(WebID::new(), db.clone());

        // Down: permission_denied.
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        assert_error_kind(&out, "permission_denied");

        // Heal.
        let escalation_queue = Arc::new(
            EscalationQueue::from_driver(SqliteDriver::in_memory_driver())
                .expect("escalation queue"),
        );
        db.set_for_tests(CuratorStores {
            escalation_queue: Some(escalation_queue),
            ..CuratorStores::empty()
        });

        // Same server instance now serves the tool.
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(0));
    }

    /// Ping reports per-store availability so an operator (or the curator
    /// itself) can distinguish "server up, stores down" from "server down".
    #[tokio::test]
    async fn ping_reports_stores_down_then_up() {
        let db = Arc::new(CuratorDb::for_tests(CuratorStores::empty()));
        let server = CuratorServer::new(WebID::new(), db.clone());

        let out = server
            .curator_ping(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        let stores = v.get("stores").expect("stores field");
        assert_eq!(
            stores.get("escalation_queue").and_then(|s| s.as_bool()),
            Some(false)
        );

        let escalation_queue = Arc::new(
            EscalationQueue::from_driver(SqliteDriver::in_memory_driver())
                .expect("escalation queue"),
        );
        db.set_for_tests(CuratorStores {
            escalation_queue: Some(escalation_queue),
            ..CuratorStores::empty()
        });

        let out = server
            .curator_ping(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        let stores = v.get("stores").expect("stores field");
        assert_eq!(
            stores.get("escalation_queue").and_then(|s| s.as_bool()),
            Some(true)
        );
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

/// Assert the response is NOT a structured McpToolError — the tool succeeded.
fn assert_no_error(out: &str) {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output must be valid JSON");
    assert!(
        v.get("error").is_none(),
        "expected success, got error response: {out}"
    );
}

/// Unwrap the `{"content": <value>}` envelope every tool response is wrapped
/// in. Reading a field off the envelope's top level silently yields `None`
/// (the `.rules` tool-envelope trap), so tests must unwrap first.
fn tool_payload(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output must be valid JSON");
    v.get("content")
        .cloned()
        .unwrap_or_else(|| panic!("expected 'content' envelope, got: {out}"))
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
    async fn denies_without_queue() {
        // REQ: dependency-denial — no EscalationQueue → permission_denied
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

    #[tokio::test]
    async fn happy_with_entries() {
        // REQ: happy — pre-populated queue lists the pending escalations
        let (server, ids) = make_server_with_escalations(2);
        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(2));
        let arr = v
            .get("escalations")
            .and_then(|e| e.as_array())
            .expect("missing array");
        assert_eq!(arr.len(), 2);
        assert!(
            arr.iter()
                .any(|e| e.get("id").and_then(|i| i.as_str()) == Some(ids[0].as_str()))
        );
    }
}

// ── curator_escalation_resolve ─────────────────────────────────────────────

mod curator_escalation_resolve {
    use super::*;

    #[tokio::test]
    async fn denies_without_queue() {
        // REQ: dependency-denial
        let server = make_server_no_stores();
        let req = params::<EscalationResolveRequest>(
            serde_json::json!({"id": "nonexistent", "resolution": "fixed"}),
        );
        let out = server.curator_escalation_resolve(req).await;
        assert_error_kind(&out, "permission_denied");
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_id() {
        // REQ: error-propagation — valid stores, nonexistent escalation id.
        // The storage NotFound kind must survive to the wire as not_found,
        // not be flattened to internal.
        let server = make_server_with_stores();
        let req = params::<EscalationResolveRequest>(
            serde_json::json!({"id": "nonexistent-id", "resolution": "fixed"}),
        );
        let out = server.curator_escalation_resolve(req).await;
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(
            v.get("kind").and_then(|k| k.as_str()),
            Some("not_found"),
            "nonexistent escalation should map to not_found, got: {out}"
        );
    }

    #[tokio::test]
    async fn happy_success() {
        // REQ: happy — add an escalation, resolve it, assert success and the
        // queue draining the pending entry.
        let (server, ids) = make_server_with_escalations(1);
        let req = params::<EscalationResolveRequest>(
            serde_json::json!({"id": ids[0], "resolution": "verified fixed"}),
        );
        let out = server.curator_escalation_resolve(req).await;
        let v = parse(&out);
        assert_eq!(v.get("resolved").and_then(|r| r.as_bool()), Some(true));

        let out = server
            .curator_escalations(params::<PingRequest>(serde_json::json!({})))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(0));
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
    async fn denies_without_queue() {
        // REQ: dependency-denial
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

    #[tokio::test]
    async fn happy_success() {
        // REQ: happy — add an escalation, dismiss it, assert success
        let (server, ids) = make_server_with_escalations(1);
        let req = params::<EscalationDismissRequest>(
            serde_json::json!({"id": ids[0], "reason": "not actionable"}),
        );
        let out = server.curator_escalation_dismiss(req).await;
        let v = parse(&out);
        assert_eq!(v.get("dismissed").and_then(|d| d.as_bool()), Some(true));
    }
}

// ── curator_semantic_search ────────────────────────────────────────────────

mod curator_semantic_search {
    use super::*;

    #[tokio::test]
    async fn denies_without_semantic() {
        // REQ: dependency-denial — no MemoryStore → permission_denied
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

    /// Degradation-boundary pin: an unavailable `EmbeddingStore` must NOT
    /// disable curator memory recall. Every curator memory tool recalls by
    /// entity/EAV, never by vector similarity, so an embedding failure is
    /// orthogonal to their capability. Before the episodic/semantic store
    /// unification the h_mem half survived independently; this asserts the
    /// unified store preserves that boundary instead of coupling all recall
    /// to a capability none of these tools use.
    #[tokio::test]
    async fn recalls_when_embeddings_unavailable() {
        let server = make_server_with_embedding_free_memory();

        let search = server
            .curator_semantic_search(params::<SemanticSearchRequest>(
                serde_json::json!({"query": "anything", "limit": null}),
            ))
            .await;
        assert_no_error(&search);

        let recall = server
            .curator_memory_recall(params::<MemoryRecallRequest>(
                serde_json::json!({"entity": "anything", "memory_type": "both"}),
            ))
            .await;
        assert_no_error(&recall);

        let consult = server
            .curator_consult(params::<CuratorConsultRequest>(
                serde_json::json!({"query": "anything", "limit": null}),
            ))
            .await;
        assert_no_error(&consult);
    }
}

// ── curator_memory_recall: ontology-axis recall (P5.4) ────────────────────

mod curator_memory_recall_ontology {
    use super::*;

    /// Each ontology axis must reach the anchored h_mem. This is the
    /// enforcement point that makes the `HMemOntology` blob a query axis
    /// rather than inert metadata — without a tool exposing it, the storage
    /// query paths would have only test callers.
    #[tokio::test]
    async fn each_axis_recalls_the_anchored_h_mem() {
        let server = make_server_with_ontology_h_mem();
        for (axis, value) in [
            ("dc_type", "bibo:Article"),
            ("dc_subject", "ROIC"),
            ("ontology_namespace", "fibo"),
        ] {
            let out = server
                .curator_memory_recall(params::<MemoryRecallRequest>(serde_json::json!({
                    "entity": "",
                    "memory_type": null,
                    "ontology_axis": axis,
                    "ontology_value": value,
                })))
                .await;
            assert_no_error(&out);
            let payload = tool_payload(&out);
            assert_eq!(
                payload.get("count").and_then(|c| c.as_u64()),
                Some(1),
                "axis '{axis}' must recall the anchored h_mem, got: {out}"
            );
            assert_eq!(
                payload.pointer("/h_mems/0/entity").and_then(|e| e.as_str()),
                Some("company:Apple")
            );
            // The recalled h_mem must carry its ontology blob — an ontology
            // query that drops the anchoring in its output would leave the
            // caller unable to tell WHY the row matched.
            assert!(
                payload.pointer("/h_mems/0/ontology").is_some(),
                "ontology recall must return the ontology blob, got: {out}"
            );
        }
    }

    /// A semantic fact carries no PKO procedure, so the process axis must
    /// return empty rather than matching on the state axis by accident.
    #[tokio::test]
    async fn process_axis_does_not_match_a_semantic_fact() {
        let server = make_server_with_ontology_h_mem();
        let out = server
            .curator_memory_recall(params::<MemoryRecallRequest>(serde_json::json!({
                "entity": "",
                "memory_type": null,
                "ontology_axis": "pko_procedure",
                "ontology_value": "bibo:Article",
            })))
            .await;
        assert_no_error(&out);
        assert_eq!(
            tool_payload(&out).get("count").and_then(|c| c.as_u64()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn unknown_axis_is_invalid_argument() {
        let server = make_server_with_ontology_h_mem();
        let out = server
            .curator_memory_recall(params::<MemoryRecallRequest>(serde_json::json!({
                "entity": "",
                "memory_type": null,
                "ontology_axis": "not_an_axis",
                "ontology_value": "x",
            })))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }

    #[tokio::test]
    async fn axis_without_value_is_invalid_argument() {
        let server = make_server_with_ontology_h_mem();
        let out = server
            .curator_memory_recall(params::<MemoryRecallRequest>(serde_json::json!({
                "entity": "",
                "memory_type": null,
                "ontology_axis": "dc_type",
                "ontology_value": null,
            })))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }

    /// Omitting the ontology fields must preserve the entity-recall behavior
    /// byte-for-byte — the axis is additive, not a replacement.
    #[tokio::test]
    async fn absent_axis_falls_back_to_entity_recall() {
        let server = make_server_with_ontology_h_mem();
        let out = server
            .curator_memory_recall(params::<MemoryRecallRequest>(serde_json::json!({
                "entity": "company:Apple",
                "memory_type": "semantic",
            })))
            .await;
        assert_no_error(&out);
        assert_eq!(
            tool_payload(&out)
                .pointer("/semantic/count")
                .and_then(|c| c.as_u64()),
            Some(1),
            "entity recall must still work: {out}"
        );
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

    #[tokio::test]
    async fn invalid_memory_type_rejected() {
        // REQ: error-propagation — unknown memory_type → invalid_argument,
        // not a silent empty object
        let server = make_server_no_stores();
        let req = params::<MemoryRecallRequest>(
            serde_json::json!({"entity": "test_entity", "memory_type": "bogus"}),
        );
        let out = server.curator_memory_recall(req).await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── curator_algedonic_log ───────────────────────────────────────────────────

mod curator_algedonic_log {
    use super::*;

    #[tokio::test]
    async fn denies_without_store() {
        // REQ: dependency-denial — no RegulationArchive → permission_denied
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

    #[tokio::test]
    async fn happy_with_events() {
        // REQ: happy — persisted Regulation events are visible in the log
        let server = make_server_with_archive_events(1);
        let req = params::<AlgedonicLogRequest>(serde_json::json!({"hours": 24}));
        let out = server.curator_algedonic_log(req).await;
        let v = parse(&out);
        assert_eq!(v.get("count").and_then(|c| c.as_u64()), Some(1));
    }
}

// ── reg_query ───────────────────────────────────────────────────────────────

mod reg_query {
    use super::*;

    #[tokio::test]
    async fn denies_without_store() {
        // REQ: dependency-denial — no RegulationArchive → permission_denied
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
        assert_eq!(v.get("replayed_count").and_then(|t| t.as_u64()), Some(0));
        assert_eq!(v.get("filtered_count").and_then(|c| c.as_u64()), Some(0));
    }

    #[tokio::test]
    async fn happy_with_events() {
        // REQ: happy — persisted Regulation events are replayed; the
        // replayed_count reflects the archive contents
        let server = make_server_with_archive_events(3);
        let req = params::<RegQueryRequest>(
            serde_json::json!({"namespace": null, "window_seconds": 86400, "limit": 100}),
        );
        let out = server.reg_query(req).await;
        let v = parse(&out);
        assert_eq!(v.get("replayed_count").and_then(|t| t.as_u64()), Some(3));
        assert_eq!(v.get("filtered_count").and_then(|c| c.as_u64()), Some(3));
    }
}
