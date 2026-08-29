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
use hkask_storage::{EmbeddingStore, EscalationQueue, HMemStore, RegulationArchive};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Stub inference port whose `embed` is left at the trait default (an
/// error) — pins the degradation path: the semantic tools must fall back
/// to exact-entity lookup (surfaced in the output) rather than erroring
/// or silently returning empty.
struct FailingEmbedPort;

impl hkask_types::InferencePort for FailingEmbedPort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Connection("stub".to_string())) })
    }
}

/// Stub inference port whose `embed` returns a constant unit vector for any
/// input — every query is a KNN match for every stored embedding (cosine
/// distance 0), isolating the semantic leg from keyword/entity matching.
/// The vector length reads the same resolver the `EmbeddingStore` schema
/// uses, so the stub stays in sync regardless of `HKASK_EMBEDDING_DIM`.
fn test_dim() -> usize {
    hkask_storage::embedding_dim()
}

struct ConstantEmbedPort;

impl hkask_types::InferencePort for ConstantEmbedPort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async { Err(hkask_types::InferenceError::Connection("stub".to_string())) })
    }

    fn embed<'a>(&'a self, _model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        // Capture only the count (owned, `Copy`) so the future borrows
        // nothing — mirrors the swarm-server test stub pattern.
        let count = texts.len();
        let dim = test_dim();
        Box::pin(async move {
            Ok((0..count)
                .map(|_| {
                    let mut vector = vec![0.0f32; dim];
                    vector[0] = 1.0;
                    vector
                })
                .collect())
        })
    }
}

fn failing_inference_port() -> Arc<dyn hkask_types::InferencePort> {
    Arc::new(FailingEmbedPort)
}

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
    CuratorServer::new(WebID::new(), database, failing_inference_port())
}

/// Build a `CuratorServer` whose memory store carries a live embedding
/// index (in-memory `EmbeddingStore` at the schema dim) and whose
/// inference port embeds every input to the same unit vector — the shape
/// the semantic recall path needs. Returns the server plus its memory
/// store handle so tests can seed h_mems and embeddings directly.
fn make_server_with_embeddings() -> (CuratorServer, Arc<hkask_memory::MemoryStore>) {
    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let embedding_store =
        EmbeddingStore::from_driver(driver.clone(), test_dim()).expect("embedding store init");
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let escalation_queue =
        Arc::new(EscalationQueue::from_driver(driver.clone()).expect("escalation queue init"));
    let regulation_store =
        Arc::new(RegulationArchive::from_driver(driver.clone()).expect("regulation archive init"));

    let stores = CuratorStores {
        escalation_queue: Some(escalation_queue),
        regulation_store: Some(regulation_store),
        memory: Some(memory.clone()),
    };
    let database = Arc::new(CuratorDb::from_stores(stores));
    let server = CuratorServer::new(
        WebID::new(),
        database,
        Arc::new(ConstantEmbedPort) as Arc<dyn hkask_types::InferencePort>,
    );
    (server, memory)
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
    let response = parse(
        &server
            .curator_ping(Parameters(PingRequest {}))
            .await
            .expect("tool ok"),
    );

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
    let response = parse(
        &server
            .curator_escalations(Parameters(PingRequest {}))
            .await
            .expect("tool ok"),
    );

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
    let error = server
        .curator_escalation_resolve(Parameters(EscalationResolveRequest {
            id: "does-not-exist".to_string(),
            resolution: "tested".to_string(),
        }))
        .await
        .expect_err("resolving a nonexistent escalation must fail");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::NotFound),
        "a nonexistent escalation id must classify as not_found, not internal — got: {error:?}",
    );
    assert!(
        error.message.contains("does-not-exist"),
        "the error message must name the missing id — got: {error:?}",
    );
}

/// Dismissing a nonexistent escalation must surface a structured `not_found`
/// error — same contract as resolve.
#[tokio::test]
async fn dismiss_nonexistent_id_returns_not_found() {
    let server = make_server();
    let error = server
        .curator_escalation_dismiss(Parameters(EscalationDismissRequest {
            id: "does-not-exist".to_string(),
            reason: "duplicate".to_string(),
        }))
        .await
        .expect_err("dismissing a nonexistent escalation must fail");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::NotFound),
        "a nonexistent escalation id must classify as not_found — got: {error:?}",
    );
}

// ── Escalation dedup at source ──────────────────────────────────────────────

/// `EscalationQueue::has_pending_with_output` returns false for a fresh queue
/// and true after an escalation with that output is added. This is the
/// dedup primitive that prevents runaway escalation floods.
#[tokio::test]
async fn escalation_queue_has_pending_with_output_detects_duplicates() {
    let driver = SqliteDriver::in_memory_driver();
    let queue = EscalationQueue::from_driver(driver).expect("escalation queue init");

    let output = "Efferent action Throttle (target: inference) recommended but not wired";

    // Fresh queue — no pending escalations.
    assert_eq!(
        queue.has_pending_with_output(output).unwrap(),
        false,
        "fresh queue must have no pending escalations with this output"
    );

    // Add one escalation.
    let template_id = hkask_types::TemplateID::new();
    let bot_id = hkask_types::BotID::new();
    queue
        .add(
            template_id,
            bot_id,
            output.to_string(),
            1.0,
            0,
            "{}".to_string(),
        )
        .unwrap();

    // Now the dedup check must find it.
    assert_eq!(
        queue.has_pending_with_output(output).unwrap(),
        true,
        "after adding an escalation, dedup check must find it"
    );

    // A different output string must not match.
    assert_eq!(
        queue
            .has_pending_with_output("completely different output")
            .unwrap(),
        false,
        "a different output string must not match"
    );
}

// ── Pattern-based batch dismiss ─────────────────────────────────────────────

/// `curator_escalation_dismiss_by_pattern` dismisses all pending escalations
/// matching an exact output string and returns the count. This is the escape
/// hatch for clearing runaway floods from a single broken feedback loop.
#[tokio::test]
async fn dismiss_by_pattern_clears_matching_escalations() {
    let driver = SqliteDriver::in_memory_driver();
    let queue =
        Arc::new(EscalationQueue::from_driver(driver.clone()).expect("escalation queue init"));
    let regulation_store =
        Arc::new(RegulationArchive::from_driver(driver.clone()).expect("regulation archive init"));
    let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let memory = Arc::new(
        hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store)
            .expect("memory store init"),
    );

    let stores = CuratorStores {
        escalation_queue: Some(queue.clone()),
        regulation_store: Some(regulation_store),
        memory: Some(memory),
    };
    let database = Arc::new(CuratorDb::from_stores(stores));
    let server = CuratorServer::new(WebID::new(), database, failing_inference_port());

    let flood_output = "Efferent action Throttle (target: inference) recommended but not wired";
    let other_output = "Variety deficit in domain: reasoning";

    // Seed: 5 identical flood escalations + 1 unrelated escalation.
    for _ in 0..5 {
        let template_id = hkask_types::TemplateID::new();
        let bot_id = hkask_types::BotID::new();
        queue
            .add(
                template_id,
                bot_id,
                flood_output.to_string(),
                1.0,
                0,
                "{\"domain\":\"efferent:Throttle\"}".to_string(),
            )
            .unwrap();
    }
    let template_id = hkask_types::TemplateID::new();
    let bot_id = hkask_types::BotID::new();
    queue
        .add(
            template_id,
            bot_id,
            other_output.to_string(),
            0.5,
            0,
            "{\"domain\":\"reasoning\"}".to_string(),
        )
        .unwrap();

    // Verify 6 pending before dismissal.
    let before = parse(
        &server
            .curator_escalations(Parameters(PingRequest {}))
            .await
            .expect("tool ok"),
    );
    assert_eq!(
        before["count"].as_u64(),
        Some(6),
        "must have 6 pending escalations before batch dismiss — got: {before}"
    );

    // Dismiss all 5 flood escalations by pattern.
    let response = parse(
        &server
            .curator_escalation_dismiss_by_pattern(Parameters(EscalationDismissByPatternRequest {
                output: flood_output.to_string(),
                reason: "runaway flood from unwired Throttle action".to_string(),
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["dismissed"].as_bool(),
        Some(true),
        "batch dismiss must return dismissed: true — got: {response}"
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(5),
        "must dismiss exactly 5 matching escalations — got: {response}"
    );

    // The unrelated escalation must remain pending.
    let after = parse(
        &server
            .curator_escalations(Parameters(PingRequest {}))
            .await
            .expect("tool ok"),
    );
    assert_eq!(
        after["count"].as_u64(),
        Some(1),
        "only the unrelated escalation must remain — got: {after}"
    );
    assert_eq!(
        after["escalations"][0]["output"].as_str(),
        Some(other_output),
        "the remaining escalation must be the unrelated one — got: {after}"
    );
}

/// `curator_escalation_dismiss_by_pattern` on an empty queue returns count: 0,
/// not an error. The no-match boundary.
#[tokio::test]
async fn dismiss_by_pattern_no_matches_returns_zero() {
    let server = make_server();
    let response = parse(
        &server
            .curator_escalation_dismiss_by_pattern(Parameters(EscalationDismissByPatternRequest {
                output: "nothing matches this".to_string(),
                reason: "testing no-match boundary".to_string(),
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["dismissed"].as_bool(),
        Some(true),
        "dismissed must be true even with zero matches — got: {response}"
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "count must be zero when no escalations match — got: {response}"
    );
}

// ── Memory recall — invalid input ──────────────────────────────────────────

/// Naming an `ontology_axis` without an `ontology_value` is a contract
/// violation — the axis is meaningless without a term to match. Must surface
/// `invalid_argument`, not a silent empty result.
#[tokio::test]
async fn memory_recall_ontology_axis_without_value_is_rejected() {
    let server = make_server();
    let error = server
        .curator_memory_recall(Parameters(MemoryRecallRequest {
            entity: "test-entity".to_string(),
            recall_shape: MemoryRecallType::default(),
            ontology_axis: Some("dc_type".to_string()),
            ontology_value: None,
        }))
        .await
        .expect_err("ontology_axis without ontology_value must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "ontology_axis without ontology_value must be invalid_argument — got: {error:?}",
    );
    assert!(
        error
            .message
            .contains("ontology_axis requires ontology_value"),
        "the error must name the contract violation — got: {error:?}",
    );
}

/// An unknown `ontology_axis` value must be rejected with `invalid_argument`,
/// naming the valid axes, not accepted as a no-op.
#[tokio::test]
async fn memory_recall_rejects_unknown_ontology_axis() {
    let server = make_server();
    let error = server
        .curator_memory_recall(Parameters(MemoryRecallRequest {
            entity: "test-entity".to_string(),
            recall_shape: MemoryRecallType::default(),
            ontology_axis: Some("bogus_axis".to_string()),
            ontology_value: Some("whatever".to_string()),
        }))
        .await
        .expect_err("an unknown ontology_axis must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "an unknown ontology_axis must be invalid_argument — got: {error:?}",
    );
    assert!(
        error.message.contains("unknown ontology_axis 'bogus_axis'"),
        "the error must name the rejected axis — got: {error:?}",
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
            .await
            .expect("tool ok"),
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
            .await
            .expect("tool ok"),
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

// ── Semantic search — semantic leg regression ──────────────────────────────

/// Seed one turn h_mem + embedding under the shared-copy entity, then query
/// with words that share NO tokens with the stored text and are NOT an entity
/// name. Before the fix, `curator_semantic_search` did exact-entity lookup on
/// the raw query — a natural-language question never matched, so every
/// semantic search returned zero. The semantic leg must find the turn via
/// KNN (constant embedding → distance 0).
#[tokio::test]
async fn semantic_search_matches_question_by_embedding() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:semantic-regression";
    let turn = serde_json::json!({
        "user_input": "alpha beta gamma delta epsilon",
        "agent_response": "zeta eta theta",
    })
    .to_string();
    let h_mem = hkask_storage::HMem::new(
        entity,
        "turn",
        serde_json::Value::String(turn),
        WebID::new(),
    );
    memory.store(h_mem).expect("seed h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(entity, &vector, "test-model", None)
        .expect("seed embedding");

    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "kangaroo wallaby emu cassowary".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert!(
        response.get("error").is_none(),
        "semantic search must not error — got: {response}",
    );
    assert_eq!(
        response["mode"].as_str(),
        Some("semantic"),
        "the semantic leg must serve the query — got: {response}",
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(1),
        "the KNN leg must find the seeded turn despite zero word overlap — got: {response}",
    );
    assert!(
        response["results"][0]["value"]
            .as_str()
            .is_some_and(|v| v.contains("alpha beta gamma")),
        "the recalled fragment must be the seeded turn — got: {response}",
    );
}

/// When the query cannot be embedded (no IPC bridge / embedding provider),
/// the tool must degrade to exact-entity lookup AND say so — the operator
/// must be able to tell "no similar memories" from "semantic recall
/// unavailable" (the unwrap_or(0) trap).
#[tokio::test]
async fn semantic_search_degrades_to_entity_exact_with_note() {
    let server = make_server();
    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "no-such-entity".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["mode"].as_str(),
        Some("entity_exact"),
        "a failed embed must fall back to exact-entity lookup — got: {response}",
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "the fallback finds no entity named 'no-such-entity' — got: {response}",
    );
    assert!(
        response["note"].as_str().is_some_and(|n| !n.is_empty()),
        "the degradation reason must be surfaced, not swallowed — got: {response}",
    );
}

/// `curator_consult` with a natural-language question must return semantic
/// fragments. Before the fix, both consult scopes did exact-entity lookup on
/// the raw question text — every consult returned zero fragments, which is
/// why the curator appeared to have no memory at all.
#[tokio::test]
async fn consult_returns_semantic_fragments_for_question() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:consult-regression";
    let turn = serde_json::json!({
        "user_input": "how do we wire the frobnicator",
        "agent_response": "via the socket",
    })
    .to_string();
    let h_mem = hkask_storage::HMem::new(
        entity,
        "turn",
        serde_json::Value::String(turn),
        WebID::new(),
    );
    memory.store(h_mem).expect("seed h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(entity, &vector, "test-model", None)
        .expect("seed embedding");

    let response = parse(
        &server
            .curator_consult(Parameters(CuratorConsultRequest {
                query: "completely unrelated question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert!(
        response.get("error").is_none(),
        "consult must not error — got: {response}",
    );
    assert_eq!(
        response["entity_wide_fragments"]["count"].as_u64(),
        Some(1),
        "the entity-wide scope must find the seeded turn via KNN — got: {response}",
    );
    assert!(
        response["entity_wide_fragments"]["h_mems"][0]["value"]
            .as_str()
            .is_some_and(|v| v.contains("frobnicator")),
        "the consulted fragment must be the seeded turn — got: {response}",
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
            .await
            .expect("tool ok"),
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
