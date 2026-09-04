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

/// The semantic paths resolve the embedding model from
/// `HKASK_EMBEDDING_MODEL` (an `Option` since the model_constants
/// refactor — unset means degraded). The test binary sets it once so the
/// semantic leg exercises; the write is one-shot and test-only (the
/// crate root allows `unsafe` in test builds for exactly this pattern).
fn ensure_embedding_model_env() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        if std::env::var("HKASK_EMBEDDING_MODEL").is_err() {
            // SAFETY: test-only env write, executed once before any test
            // body relies on it, always to the same value.
            unsafe { std::env::set_var("HKASK_EMBEDDING_MODEL", "test-embedding-model") };
        }
    });
}

/// Build a `CuratorServer` backed by a single shared in-memory driver, so all
/// four stores see the same data (the production shape — one `curator.db`).
/// Healing is disabled (no path, no passphrase) via `CuratorDb::from_stores`,
/// so the self-heal loop never fires during a test.
fn make_server() -> CuratorServer {
    ensure_embedding_model_env();
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
    ensure_embedding_model_env();
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

// ── Memory distillation — evidence-grounded insert ─────────────────────

/// `memory_insert` must accept an evidence citation that names an existing
/// h_mem ID. The original lookup passed the UUID to the entity-keyed
/// `query_deduped_untouched` — and no entity is a bare UUID, so every
/// citation was "not found" and the distillation tool could never insert:
/// the store only ever accumulated raw turn dumps. This pins the by-ID
/// lookup and the 0.5 confidence floor.
#[tokio::test]
async fn memory_insert_accepts_existing_h_mem_id_as_evidence() {
    let (server, memory) = make_server_with_embeddings();
    let seed = hkask_storage::HMem::new(
        "chat:thread:evidence-source",
        "chatted",
        serde_json::Value::String("the source turn".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed evidence h_mem");

    let response = parse(
        &server
            .memory_insert(Parameters(MemoryInsertRequest {
                entity: "zed-kask".to_string(),
                attribute: "default_agent_model".to_string(),
                value: serde_json::json!("qwen3"),
                evidence_h_mem_id: seed_id,
                note: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "a citation naming a real h_mem ID must insert — got: {response}",
    );
    assert_eq!(
        response["confidence"].as_f64(),
        Some(0.5),
        "inserts start at the 0.5 floor, not the model's self-assessment — got: {response}",
    );

    // The insert is durable and entity-recallable.
    let stored = memory
        .query_deduped_untouched("zed-kask")
        .expect("query should succeed");
    assert_eq!(stored.len(), 1, "the distilled memory must be stored");
    assert_eq!(stored[0].value, serde_json::json!("qwen3"));
    assert!((stored[0].confidence.value() - 0.5).abs() < 1e-9);
}

/// Evidence citations that name no existing h_mem must be rejected as
/// `invalid_argument` with the reason surfaced — both a well-formed UUID
/// that matches no row and a malformed ID that cannot parse.
#[tokio::test]
async fn memory_insert_rejects_missing_or_malformed_evidence() {
    let server = make_server();

    let error = server
        .memory_insert(Parameters(MemoryInsertRequest {
            entity: "zed-kask".to_string(),
            attribute: "default_agent_model".to_string(),
            value: serde_json::json!("qwen3"),
            evidence_h_mem_id: "00000000-0000-0000-0000-000000000000".to_string(),
            note: None,
        }))
        .await
        .expect_err("a citation matching no h_mem must fail");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "a missing citation is an argument error, not internal — got: {error:?}",
    );
    assert!(
        error.message.contains("not found"),
        "the error must name the missing citation — got: {error:?}",
    );

    let error = server
        .memory_insert(Parameters(MemoryInsertRequest {
            entity: "zed-kask".to_string(),
            attribute: "default_agent_model".to_string(),
            value: serde_json::json!("qwen3"),
            evidence_h_mem_id: "not-a-uuid".to_string(),
            note: None,
        }))
        .await
        .expect_err("a malformed citation must fail");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "a malformed citation is an argument error — got: {error:?}",
    );
    assert!(
        error.message.contains("not-a-uuid"),
        "the error must name the malformed ID — got: {error:?}",
    );
}

// ── Insert-path semantic recallability (the entity_ref invariant) ──────

/// `memory_insert` must embed the inserted memory's text under its entity.
/// The original stored the h_mem with no embedding, so every agent-inserted
/// memory (operator rulings, verified code status — the knowledge layer)
/// was invisible to `curator_semantic_search`: recallable only by exact
/// entity name, and a semantic search that found nothing was read as "no
/// memory exists". Pins the embedding and the end-to-end semantic recall of
/// an inserted memory.
#[tokio::test]
async fn memory_insert_embeds_value_for_semantic_recall() {
    let (server, memory) = make_server_with_embeddings();
    let seed = hkask_storage::HMem::new(
        "chat:thread:evidence-source",
        "chatted",
        serde_json::Value::String("the source turn".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed evidence h_mem");

    let response = parse(
        &server
            .memory_insert(Parameters(MemoryInsertRequest {
                entity: "zed-kask".to_string(),
                attribute: "default_agent_model".to_string(),
                value: serde_json::json!("qwen3"),
                evidence_h_mem_id: seed_id,
                note: None,
            }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "insert must succeed — got: {response}",
    );
    assert_eq!(
        response["semantic_recall"].as_str(),
        Some("embedded"),
        "the embedding must be surfaced as stored — got: {response}",
    );
    assert_eq!(
        memory.embedding_count().expect("embedding count"),
        1,
        "memory_insert must store one embedding under the inserted entity",
    );

    // End-to-end: a natural-language query sharing no tokens with the entity
    // name must find the inserted memory via KNN.
    let search = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "what model does the agent use by default".to_string(),
                limit: None,
            }))
            .await
            .expect("search ok"),
    );
    assert!(
        search["results"].as_array().is_some_and(|results| {
            results
                .iter()
                .any(|r| r["entity"].as_str() == Some("zed-kask"))
        }),
        "the inserted memory must be semantically recallable — got: {search}",
    );
}

/// `curator_report_skill_use_issue` must store at the 0.5 confidence floor
/// — not the `HMem::new` 1.0 default — and embed the report text under the
/// entity. At 1.0, unverified issue reports outranked verified facts in
/// recall ranking (confidence is a ranking multiplier); without an
/// embedding the report was invisible to the semantic search this tool's
/// contract advertises.
#[tokio::test]
async fn skill_use_issue_stores_at_floor_and_is_semantically_recallable() {
    let (server, memory) = make_server_with_embeddings();

    let response = parse(
        &server
            .curator_report_skill_use_issue(Parameters(ReportSkillUseIssueRequest {
                skill_name: "grounding-verify".to_string(),
                tool_name: "lisp_eval".to_string(),
                step_ordinal: 2,
                error: "closed-vocabulary validation form errored".to_string(),
                tool_input: None,
                failure_type: None,
            }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(
        response["reported"].as_bool(),
        Some(true),
        "the report must store — got: {response}",
    );

    let stored = memory
        .query_deduped_untouched("skill_use_issue:grounding-verify")
        .expect("query stored reports");
    assert_eq!(stored.len(), 1, "one report must be stored");
    assert!(
        (stored[0].confidence.value() - 0.5).abs() < 1e-9,
        "issue reports start at the 0.5 floor, not the HMem::new 1.0 default — got {}",
        stored[0].confidence.value(),
    );
    assert_eq!(
        memory.embedding_count().expect("embedding count"),
        1,
        "the report text must be embedded under the report entity",
    );
}

/// The insert paths' embedding degradation contract (write-side invariant
/// 3): with no embedding store and a failing inference port, inserts still
/// succeed and the degradation is surfaced in the output — never a failed
/// insert, never a silent success.
#[tokio::test]
async fn insert_path_embedding_failure_is_non_fatal_and_surfaced() {
    // The degraded shape: a live memory store with NO embedding store and a
    // failing inference port.
    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let memory = Arc::new(
        hkask_memory::MemoryStore::try_new_without_embeddings(h_mem_store)
            .expect("memory store init"),
    );
    let seed = hkask_storage::HMem::new(
        "chat:thread:evidence-source",
        "chatted",
        serde_json::Value::String("the source turn".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed evidence h_mem");
    let stores = CuratorStores {
        escalation_queue: None,
        regulation_store: None,
        memory: Some(memory),
    };
    let server = CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::from_stores(stores)),
        failing_inference_port(),
    );

    let insert = parse(
        &server
            .memory_insert(Parameters(MemoryInsertRequest {
                entity: "zed-kask".to_string(),
                attribute: "mcp_tool_surface".to_string(),
                value: serde_json::json!("full surface, no router"),
                evidence_h_mem_id: seed_id,
                note: None,
            }))
            .await
            .expect("insert must succeed without embeddings"),
    );
    assert_eq!(
        insert["inserted"].as_bool(),
        Some(true),
        "the h_mem is durable SQL — embedding failure must not fail the insert — got: {insert}",
    );
    assert_eq!(
        insert["semantic_recall"].as_str(),
        Some("degraded (embedding unavailable — warn logged)"),
        "the degradation must be surfaced in the output — got: {insert}",
    );

    let report = parse(
        &server
            .curator_report_skill_use_issue(Parameters(ReportSkillUseIssueRequest {
                skill_name: "therapy".to_string(),
                tool_name: "memory_insert".to_string(),
                step_ordinal: 5,
                error: "announce-then-stop".to_string(),
                tool_input: None,
                failure_type: None,
            }))
            .await
            .expect("report must succeed without embeddings"),
    );
    assert_eq!(
        report["reported"].as_bool(),
        Some(true),
        "the report is durable SQL — embedding failure must not fail it — got: {report}",
    );
    assert_eq!(
        report["semantic_recall"].as_str(),
        Some("degraded (embedding unavailable — warn logged)"),
        "the degradation must be surfaced in the output — got: {report}",
    );
}

/// `memory_resolve_contradiction` must resolve its target by h_mem ID. The
/// previous verification used the entity-keyed `query_deduped_untouched`
/// with the bare UUID — and no entity is a bare UUID, so every resolution
/// attempt returned not_found and the tool could never resolve anything
/// (the same bug class memory_insert's evidence check was fixed for).
/// Pins both the expire and update_confidence strategies.
#[tokio::test]
async fn resolve_contradiction_finds_target_by_id() {
    let (server, memory) = make_server_with_embeddings();
    let expire_target = hkask_storage::HMem::new(
        "zed-kask/duplicate-ruling",
        "operator_ruling",
        serde_json::Value::String("ruling A".to_string()),
        WebID::new(),
    );
    let expire_id = expire_target.id.to_string();
    memory.store(expire_target).expect("seed expire target");

    let response = parse(
        &server
            .memory_resolve_contradiction(Parameters(MemoryResolveContradictionRequest {
                h_mem_ids: vec!["some-other-id".to_string()],
                strategy: "expire".to_string(),
                target_h_mem_id: expire_id,
                new_confidence: None,
                reason: "duplicate ruling merge".to_string(),
            }))
            .await
            .expect("expire must resolve by ID"),
    );
    assert_eq!(
        response["resolved"].as_bool(),
        Some(true),
        "expire must resolve — got: {response}",
    );
    assert!(
        memory
            .h_mems_by_entity_prefix("zed-kask/duplicate-ruling")
            .expect("query after expire")
            .is_empty(),
        "the expired target must be soft-deleted (valid_to set)"
    );

    let confidence_target = hkask_storage::HMem::new(
        "zed-kask/confidence-target",
        "policy",
        serde_json::Value::String("policy text".to_string()),
        WebID::new(),
    );
    let confidence_id = confidence_target.id.to_string();
    memory
        .store(confidence_target)
        .expect("seed confidence target");

    let response = parse(
        &server
            .memory_resolve_contradiction(Parameters(MemoryResolveContradictionRequest {
                h_mem_ids: vec![],
                strategy: "update_confidence".to_string(),
                target_h_mem_id: confidence_id,
                new_confidence: Some(0.5),
                reason: "floor reset".to_string(),
            }))
            .await
            .expect("update_confidence must resolve by ID"),
    );
    assert_eq!(
        response["resolved"].as_bool(),
        Some(true),
        "update_confidence must resolve — got: {response}",
    );
    let updated = memory
        .h_mems_by_entity_prefix("zed-kask/confidence-target")
        .expect("query after update");
    assert_eq!(updated.len(), 1);
    assert!(
        (updated[0].confidence.value() - 0.5).abs() < 1e-9,
        "update_confidence must set the value — got {}",
        updated[0].confidence.value(),
    );
}

/// `curator_memory_backfill_embeddings` must embed knowledge-layer h_mems
/// whose entities have no embedding, while excluding turn-storage entities
/// (their embeddings live under the shared copy by design) and distillation
/// watermarks (process markers). The tool exists because h_mems inserted
/// before the 2026-09-04 embedding contract are invisible to semantic
/// search. Also pins dry-run (embeds nothing) and idempotence (a second
/// run finds no candidates).
#[tokio::test]
async fn backfill_embeddings_covers_knowledge_layer_and_excludes_turns() {
    let (server, memory) = make_server_with_embeddings();

    let ruling = hkask_storage::HMem::new(
        "zed-kask/provider_budget_blocks",
        "operator_ruling",
        serde_json::Value::String("do not touch the provider files".to_string()),
        WebID::new(),
    );
    memory.store(ruling).expect("seed ruling");
    let shared_turn = hkask_storage::HMem::new(
        "curator:thread:backfill-test",
        "turn",
        serde_json::Value::String("shared turn content".to_string()),
        WebID::new(),
    );
    memory.store(shared_turn).expect("seed shared turn");
    let watermark = hkask_storage::HMem::new(
        "curator:distilled:backfill-test",
        "distilled_through",
        serde_json::json!({"through": "2026-09-04", "turns": 1}),
        WebID::new(),
    );
    memory.store(watermark).expect("seed watermark");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding("curator:thread:backfill-test", &vector, "test-model", None)
        .expect("seed turn embedding");

    // Dry run: lists the ruling only, embeds nothing.
    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                dry_run: Some(true),
            }))
            .await
            .expect("dry run ok"),
    );
    assert_eq!(dry["dry_run"].as_bool(), Some(true));
    assert_eq!(
        dry["candidate_count"].as_u64(),
        Some(1),
        "only the knowledge-layer entity is a candidate — got: {dry}",
    );
    assert_eq!(
        memory.embedding_count().expect("count"),
        1,
        "dry run must not embed anything",
    );

    // Real run: the ruling gains an embedding; turns and watermark untouched.
    let run = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                dry_run: Some(false),
            }))
            .await
            .expect("backfill ok"),
    );
    assert_eq!(
        run["backfilled"].as_u64(),
        Some(1),
        "one entity backfilled — got: {run}",
    );
    assert_eq!(run["failed"].as_u64(), Some(0));
    assert_eq!(
        memory.embedding_count().expect("count"),
        2,
        "the ruling's embedding landed; no turn/watermark embeddings added",
    );

    // Idempotent: a second run finds no candidates.
    let second = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                dry_run: None,
            }))
            .await
            .expect("second run ok"),
    );
    assert_eq!(
        second["candidate_count"].as_u64(),
        Some(0),
        "entities with embeddings are skipped — the pass is idempotent — got: {second}",
    );
}

/// `memory_update` must resolve its target by h_mem ID. The previous
/// verification used the entity-keyed `query_deduped_untouched` with the
/// bare UUID — and no entity is a bare UUID, so every update attempt
/// returned not_found and the tool never updated anything (the same bug
/// class memory_insert's evidence check and memory_resolve_contradiction
/// were fixed for).
#[tokio::test]
async fn memory_update_finds_target_by_id() {
    let (server, memory) = make_server_with_embeddings();
    let seed = hkask_storage::HMem::new(
        "zed-kask/update-target",
        "policy",
        serde_json::Value::String("old value".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed update target");

    let response = parse(
        &server
            .memory_update(Parameters(MemoryUpdateRequest {
                h_mem_id: seed_id,
                new_confidence: 0.6,
                new_value: Some(serde_json::Value::String("new value".to_string())),
                reason: Some("test update".to_string()),
            }))
            .await
            .expect("update must resolve by ID"),
    );
    assert_eq!(
        response["updated"].as_bool(),
        Some(true),
        "update must resolve — got: {response}",
    );

    let updated = memory
        .h_mems_by_entity_prefix("zed-kask/update-target")
        .expect("query after update");
    assert_eq!(updated.len(), 1);
    assert_eq!(
        updated[0].value,
        serde_json::Value::String("new value".to_string()),
        "new_value must replace the value"
    );
    assert!(
        updated[0].confidence.value() > 0.5,
        "the Bayesian combine must move the confidence off the floor — got {}",
        updated[0].confidence.value(),
    );
}

// ── Semantic search — per-entity flood cap ──────────────────────────────

/// One entity must not flood semantic recall: a thread entity holds one
/// h_mem per turn, so without a per-entity cap a single chatty thread fills
/// the entire result set and every other entity vanishes from recall.
/// Multiple embeddings under one entity must also not yield duplicate
/// fragments.
#[tokio::test]
async fn semantic_search_caps_fragments_per_entity() {
    let (server, memory) = make_server_with_embeddings();
    let flood_entity = "curator:thread:flood-test";
    let quiet_entity = "curator:thread:quiet-test";

    for turn_index in 0..5 {
        let turn = serde_json::json!({
            "user_input": format!("flood turn {turn_index}"),
            "agent_response": "ok",
        })
        .to_string();
        let h_mem = hkask_storage::HMem::new(
            flood_entity,
            "turn",
            serde_json::Value::String(turn),
            WebID::new(),
        );
        memory.store(h_mem).expect("seed flood h_mem");
        let mut vector = vec![0.0f32; test_dim()];
        vector[0] = 1.0;
        memory
            .store_embedding(flood_entity, &vector, "test-model", None)
            .expect("seed flood embedding");
    }
    let turn = serde_json::json!({
        "user_input": "quiet turn",
        "agent_response": "ok",
    })
    .to_string();
    let h_mem = hkask_storage::HMem::new(
        quiet_entity,
        "turn",
        serde_json::Value::String(turn),
        WebID::new(),
    );
    memory.store(h_mem).expect("seed quiet h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(quiet_entity, &vector, "test-model", None)
        .expect("seed quiet embedding");

    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "anything at all".to_string(),
                limit: Some(10),
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["mode"].as_str(),
        Some("semantic"),
        "the semantic leg must serve the query — got: {response}",
    );
    let results = response["results"]
        .as_array()
        .expect("results must be an array");
    let flood_fragments: Vec<&serde_json::Value> = results
        .iter()
        .filter(|r| r["entity"].as_str() == Some(flood_entity))
        .collect();
    assert_eq!(
        flood_fragments.len(),
        2,
        "the flood entity contributes at most MAX_FRAGMENTS_PER_ENTITY fragments, \
         not one per turn — got: {response}",
    );
    assert!(
        results
            .iter()
            .any(|r| r["entity"].as_str() == Some(quiet_entity)),
        "the quiet entity must survive the flood — got: {response}",
    );
    let mut flood_values: Vec<String> = flood_fragments
        .iter()
        .filter_map(|r| r["value"].as_str().map(str::to_string))
        .collect();
    flood_values.sort();
    flood_values.dedup();
    assert_eq!(
        flood_values.len(),
        2,
        "the two flood fragments must be distinct h_mems — got: {response}",
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
