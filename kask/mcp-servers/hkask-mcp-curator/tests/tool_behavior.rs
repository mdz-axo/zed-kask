//! Tool-behavior contract tests for `hkask-mcp-curator`.
//!
//! Drives the real `Parameters<T>` tool seam in-process: the server is
//! constructed over an in-memory `SqliteDriver` so the Regulation archive
//! and curator memory are live, and every
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
use hkask_storage::{EmbeddingStore, HMemStore, RegulationArchive};
use hkask_types::event::{CyclePhase, Span, SpanNamespace};
use hkask_types::{RegulationRecord, RegulationSink, WebID};
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

/// A port whose `embed_with_dimensions` reports a provider-confirmed actual
/// model — the shape the model-mismatch sweep's identity probe reads, used
/// to pin the gate's two-form acceptance (a row stored under the actual form
/// is NOT swept).
struct ActualFormEmbedPort;

impl hkask_types::InferencePort for ActualFormEmbedPort {
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

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        _dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        // Capture owned values so the future borrows nothing — the
        // ConstantEmbedPort pattern.
        let count = texts.len();
        let dim = test_dim();
        let requested_model = model.to_string();
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors: (0..count)
                    .map(|_| {
                        let mut vector = vec![0.0f32; dim];
                        vector[0] = 1.0;
                        vector
                    })
                    .collect(),
                requested_model,
                actual_model: Some("provider/actual-form".to_string()),
            })
        })
    }
}

/// The unit vector `ConstantEmbedPort` embeds every text to — the query
/// vector the sweep tests search with.
fn unit_vector() -> Vec<f32> {
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    vector
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
            unsafe { std::env::set_var("HKASK_EMBEDDING_MODEL", "test-model") };
        }
    });
}

/// Build a `CuratorServer` backed by a single shared in-memory driver, so all
/// stores see the same data (the production shape — one `curator.db`).
/// Healing is disabled (no path, no passphrase) via `CuratorDb::from_stores`,
/// so the self-heal loop never fires during a test.
fn make_server() -> CuratorServer {
    make_server_with_regulation_archive().0
}

fn make_server_with_regulation_archive() -> (CuratorServer, Arc<RegulationArchive>) {
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
    let regulation_store =
        Arc::new(RegulationArchive::from_driver(driver.clone()).expect("regulation archive init"));

    let stores = CuratorStores {
        regulation_store: Some(regulation_store.clone()),
        memory: Some(memory),
    };
    let database = Arc::new(CuratorDb::from_stores(stores));
    (
        CuratorServer::new(WebID::new(), database, failing_inference_port()),
        regulation_store,
    )
}

/// Build a `CuratorServer` whose memory store carries a live embedding
/// index (in-memory `EmbeddingStore` at the schema dim) and whose
/// inference port embeds every input to the same unit vector — the shape
/// the semantic recall path needs. Returns the server plus its memory
/// store handle so tests can seed h_mems and embeddings directly.
fn make_server_with_embeddings() -> (CuratorServer, Arc<hkask_memory::MemoryStore>) {
    make_server_with_embedding_port(Arc::new(ConstantEmbedPort))
}

/// Like [`make_server_with_embeddings`], with a caller-supplied inference
/// port — the seam the model-mismatch sweep tests use to pin the
/// provider-actual-form acceptance.
fn make_server_with_embedding_port(
    port: Arc<dyn hkask_types::InferencePort>,
) -> (CuratorServer, Arc<hkask_memory::MemoryStore>) {
    ensure_embedding_model_env();
    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let embedding_store =
        EmbeddingStore::from_driver(driver.clone(), test_dim()).expect("embedding store init");
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let regulation_store =
        Arc::new(RegulationArchive::from_driver(driver.clone()).expect("regulation archive init"));

    let stores = CuratorStores {
        regulation_store: Some(regulation_store),
        memory: Some(memory.clone()),
    };
    let database = Arc::new(CuratorDb::from_stores(stores));
    let server = CuratorServer::new(WebID::new(), database, port);
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

/// `curator_ping` returns `status: "ok"` and reports both stores as live
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
    assert!(response["stores"].get("escalation_queue").is_none());
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

/// KNN-orphan trap: an embedding stored under an entity with no h_mem is a
/// hit that fails resolution. When EVERY hit fails, the result must NOT be
/// a bare count:0 — that reads as "genuinely no similar memories" (the
/// empty-result-as-success trap). The note must name the failures.
#[tokio::test]
async fn semantic_search_all_hits_failing_resolution_is_not_reported_as_empty() {
    let (server, memory) = make_server_with_embeddings();
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding("curator:thread:orphaned-hit", &vector, "test-model", None)
        .expect("seed orphan embedding (no h_mem under the entity)");

    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["mode"].as_str(),
        Some("semantic"),
        "the semantic leg ran (embed + KNN found the orphan) — got: {response}",
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "no hit resolved to an h_mem — got: {response}",
    );
    assert!(
        response["note"]
            .as_str()
            .is_some_and(|n| n.contains("1 semantic hits failed h_mem resolution")
                && n.contains("not empty")),
        "an all-hits-failed run must say resolution failed, not imply an empty store — got: {response}",
    );
}

/// A run where SOME hits resolve and some fail must still surface the
/// failure count — partial results must not mask the degraded recall.
#[tokio::test]
async fn semantic_search_partial_resolution_failure_notes_degradation() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:partial-live";
    let h_mem = hkask_storage::HMem::new(
        entity,
        "turn",
        serde_json::Value::String("live turn content".to_string()),
        WebID::new(),
    );
    memory.store(h_mem).expect("seed h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(entity, &vector, "test-model", None)
        .expect("seed live embedding");
    memory
        .store_embedding("curator:thread:partial-orphan", &vector, "test-model", None)
        .expect("seed orphan embedding");

    let response = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["count"].as_u64(),
        Some(1),
        "the live hit must still resolve — got: {response}",
    );
    assert!(
        response["note"]
            .as_str()
            .is_some_and(|n| n.contains("1 semantic hits failed h_mem resolution")),
        "the failed hit must be named alongside the partial results — got: {response}",
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

/// Consult twin of the KNN-orphan trap: when every semantic hit fails
/// resolution, both scopes must say resolution failed — a bare count:0
/// per scope reads as "the curator has no memory of this".
#[tokio::test]
async fn consult_all_hits_failing_resolution_is_not_reported_as_empty() {
    let (server, memory) = make_server_with_embeddings();
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding("curator:thread:consult-orphan", &vector, "test-model", None)
        .expect("seed orphan embedding (no h_mem under the entity)");

    let response = parse(
        &server
            .curator_consult(Parameters(CuratorConsultRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    for scope in ["entity_wide_fragments", "perspective_scoped_fragments"] {
        assert_eq!(
            response[scope]["count"].as_u64(),
            Some(0),
            "{scope}: no hit resolved — got: {response}",
        );
        assert!(
            response[scope]["note"]
                .as_str()
                .is_some_and(|n| n.contains("1 semantic hits failed h_mem resolution")
                    && n.contains("not empty")),
            "{scope}: an all-hits-failed consult must say resolution failed, not imply an empty store — got: {response}",
        );
    }
}

/// Consult with partial resolution: the resolved scope results must carry
/// the failure count for the hits that did not resolve.
#[tokio::test]
async fn consult_partial_resolution_failure_notes_degradation() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:consult-partial-live";
    let h_mem = hkask_storage::HMem::new(
        entity,
        "turn",
        serde_json::Value::String("live consult turn".to_string()),
        WebID::new(),
    );
    memory.store(h_mem).expect("seed h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(entity, &vector, "test-model", None)
        .expect("seed live embedding");
    memory
        .store_embedding(
            "curator:thread:consult-partial-orphan",
            &vector,
            "test-model",
            None,
        )
        .expect("seed orphan embedding");

    let response = parse(
        &server
            .curator_consult(Parameters(CuratorConsultRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    assert_eq!(
        response["entity_wide_fragments"]["count"].as_u64(),
        Some(1),
        "the live hit must still resolve — got: {response}",
    );
    for scope in ["entity_wide_fragments", "perspective_scoped_fragments"] {
        assert!(
            response[scope]["note"]
                .as_str()
                .is_some_and(|n| n.contains("1 semantic hits failed h_mem resolution")),
            "{scope}: the failed hit must be named alongside the partial results — got: {response}",
        );
    }
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
        "curator:thread:evidence-source",
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
                value: serde_json::json!("qwen3").into(),
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

/// The citation round-trip must complete from the tool surface: an agent
/// reads a fragment via `curator_memory_recall`, extracts the `id` the
/// tool surfaced, and cites it as `memory_insert` evidence. The ID used
/// to live only in the store — every read surface dropped it at
/// serialization — so the evidence requirement was unsatisfiable from
/// the agent side (it was designed against the store API, not the tool
/// surface). Pins the id on all three read surfaces and the round-trip.
#[tokio::test]
async fn memory_citation_round_trip_from_tool_surface() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:evidence-source";
    let seed = hkask_storage::HMem::new(
        entity,
        "chatted",
        serde_json::Value::String("the source turn".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed evidence h_mem");
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(entity, &vector, "test-model", None)
        .expect("seed embedding");

    // Recall via the tool — the cited ID must come from the tool output,
    // not from the store.
    let recall = parse(
        &server
            .curator_memory_recall(Parameters(MemoryRecallRequest {
                entity: entity.to_string(),
                recall_shape: MemoryRecallType::EntityWide,
                ontology_axis: None,
                ontology_value: None,
            }))
            .await
            .expect("recall ok"),
    );
    assert!(
        recall["entity_wide"]["h_mems"][0]["id"]
            .as_str()
            .is_some_and(|id| id == seed_id),
        "recall must surface the seeded h_mem id — got: {recall}",
    );
    let recalled_id = recall["entity_wide"]["h_mems"][0]["id"]
        .as_str()
        .expect("id asserted above")
        .to_string();

    // Consult and semantic search surface the id too.
    let consult = parse(
        &server
            .curator_consult(Parameters(CuratorConsultRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("consult ok"),
    );
    assert!(
        consult["entity_wide_fragments"]["h_mems"][0]["id"]
            .as_str()
            .is_some_and(|id| id == seed_id),
        "consult must surface the h_mem id — got: {consult}",
    );
    let search = parse(
        &server
            .curator_semantic_search(Parameters(SemanticSearchRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("search ok"),
    );
    assert!(
        search["results"][0]["id"]
            .as_str()
            .is_some_and(|id| id == seed_id),
        "semantic search must surface the h_mem id — got: {search}",
    );

    // The round-trip: cite the tool-surfaced ID as insert evidence.
    let response = parse(
        &server
            .memory_insert(Parameters(MemoryInsertRequest {
                entity: "zed-kask".to_string(),
                attribute: "citation_round_trip".to_string(),
                value: serde_json::json!("pinned").into(),
                evidence_h_mem_id: recalled_id,
                note: None,
            }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "an ID obtained from a tool output must be accepted as evidence — got: {response}",
    );
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
            value: serde_json::json!("qwen3").into(),
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
            value: serde_json::json!("qwen3").into(),
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
        "curator:thread:evidence-source",
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
                value: serde_json::json!("qwen3").into(),
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
                failure_origin: SkillUseFailureOrigin::AgentExecution,
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
    assert_eq!(
        response["failure_origin"].as_str(),
        Some("agent_execution"),
        "the tool response must surface typed ownership — got: {response}",
    );
    assert_eq!(
        stored[0].value["failure_origin"].as_str(),
        Some("agent_execution"),
        "the durable report must preserve typed ownership",
    );
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
        "curator:thread:evidence-source",
        "chatted",
        serde_json::Value::String("the source turn".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed evidence h_mem");
    let stores = CuratorStores {
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
                value: serde_json::json!("full surface, no router").into(),
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
                failure_origin: SkillUseFailureOrigin::ToolImplementation,
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
/// Pins both the forget and update_confidence strategies.
#[tokio::test]
async fn resolve_contradiction_finds_target_by_id() {
    let (server, memory) = make_server_with_embeddings();
    let forget_target = hkask_storage::HMem::new(
        "zed-kask/duplicate-ruling",
        "operator_ruling",
        serde_json::Value::String("ruling A".to_string()),
        WebID::new(),
    );
    let forget_id = forget_target.id.to_string();
    memory.store(forget_target).expect("seed forget target");

    let response = parse(
        &server
            .memory_resolve_contradiction(Parameters(MemoryResolveContradictionRequest {
                h_mem_ids: vec!["some-other-id".to_string()],
                strategy: "forget".to_string(),
                target_h_mem_id: forget_id,
                new_confidence: None,
                reason: "duplicate ruling merge".to_string(),
            }))
            .await
            .expect("forget must resolve by ID"),
    );
    assert_eq!(
        response["resolved"].as_bool(),
        Some(true),
        "forget must resolve — got: {response}",
    );
    assert!(
        memory
            .h_mems_by_entity_prefix("zed-kask/duplicate-ruling")
            .expect("query after forget")
            .is_empty(),
        "the forgotten target must be deleted from the database"
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

/// A knowledge row with no embeddable text is named in the dry run — the
/// operator can find the memory semantic recall cannot see.
#[tokio::test]
async fn backfill_names_unsupported_rows() {
    let (server, memory) = make_server_with_embeddings();
    let opaque = hkask_storage::HMem::new(
        "opaque-entity",
        "structured_only",
        serde_json::json!({"count": 3}),
        WebID::new(),
    );
    let id = opaque.id.to_string();
    memory.store(opaque).expect("seed opaque row");

    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
                dry_run: Some(true),
            }))
            .await
            .expect("dry run ok"),
    );
    assert_eq!(dry["unsupported_count"].as_u64(), Some(1));
    assert_eq!(
        dry["unsupported"][0]["h_mem_id"].as_str(),
        Some(id.as_str())
    );
    assert_eq!(
        dry["unsupported"][0]["entity"].as_str(),
        Some("opaque-entity")
    );
}

/// `curator_memory_backfill_embeddings` must embed knowledge-layer h_mems
/// whose entities have no embedding, while excluding turn storage
/// (`curator:thread:`) and distillation watermarks. Also pins dry-run (embeds nothing) and idempotence (a second
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
    let mutable_recall_text = "The server exposes five tools. [mutable state; source: kask/file.rs; version/date: abc123]";
    let mutable_lesson = hkask_storage::HMem::new(
        "server-tool-count",
        "current-count",
        serde_json::json!({
            "text": "The server exposes five tools.",
            "recall_text": mutable_recall_text,
            "mutable_state": true,
            "state_provenance": {
                "source_locator": "kask/file.rs",
                "version_or_date": "abc123"
            }
        }),
        WebID::new(),
    );
    memory.store(mutable_lesson).expect("seed mutable lesson");
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

    // Dry run: lists the ruling and mutable lesson, embeds nothing.
    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
                dry_run: Some(true),
            }))
            .await
            .expect("dry run ok"),
    );
    assert_eq!(dry["dry_run"].as_bool(), Some(true));
    assert_eq!(
        dry["candidate_count"].as_u64(),
        Some(2),
        "both knowledge-layer entities are candidates — got: {dry}",
    );
    assert_eq!(
        memory.embedding_count().expect("count"),
        1,
        "dry run must not embed anything",
    );

    // Real run: both knowledge rows gain canonical embeddings; turns and watermark untouched.
    let run = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
                dry_run: Some(false),
            }))
            .await
            .expect("backfill ok"),
    );
    assert_eq!(
        run["backfilled"].as_u64(),
        Some(2),
        "two entities backfilled — got: {run}",
    );
    assert_eq!(run["failed"].as_u64(), Some(0));
    assert_eq!(
        memory.embedding_count().expect("count"),
        3,
        "the two knowledge embeddings landed; no turn/watermark embeddings added",
    );
    let mutable_passage = memory
        .all_embeddings_with_text()
        .expect("embedding inventory")
        .into_iter()
        .find_map(|(entity, _vector, passage, _model)| {
            (entity == "server-tool-count").then_some(passage).flatten()
        })
        .expect("mutable lesson passage");
    assert_eq!(
        mutable_passage, mutable_recall_text,
        "backfill and production recall must share the provenance-bearing passage"
    );

    // Idempotent: a second run finds no candidates.
    let second = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
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

/// Backfill is passage-scoped, not entity-scoped, and never repairs goal rows.
#[tokio::test]
async fn backfill_is_passage_scoped_and_excludes_goals() {
    let (server, memory) = make_server_with_embeddings();
    let first = hkask_storage::HMem::new(
        "shared-knowledge",
        "first",
        serde_json::json!("first passage"),
        WebID::new(),
    );
    let second = hkask_storage::HMem::new(
        "shared-knowledge",
        "second",
        serde_json::json!("second passage"),
        WebID::new(),
    );
    let goal = hkask_storage::HMem::new(
        "curator:goal:invalid-publication",
        "kanban_goal_score",
        serde_json::json!({
            "content": {"goal_id": "invalid-publication", "brier": null}
        }),
        WebID::new(),
    );
    memory.store(first.clone()).expect("seed first passage");
    memory.store(second.clone()).expect("seed second passage");
    memory.store(goal).expect("seed invalid goal row");
    let first_passage =
        hkask_memory::semantic_passage_for_h_mem(&first).expect("canonical first passage");
    memory
        .store_embedding(
            "shared-knowledge",
            &vec![1.0; test_dim()],
            "test-model",
            Some(&first_passage),
        )
        .expect("seed first embedding");

    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
                dry_run: Some(true),
            }))
            .await
            .expect("dry run"),
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(
        dry["candidates"][0]["attribute"].as_str(),
        Some("second"),
        "the missing sibling passage is selected even though its entity has another vector"
    );

    let run = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: None,
                dry_run: Some(false),
            }))
            .await
            .expect("backfill"),
    );
    assert_eq!(run["backfilled"].as_u64(), Some(1));
    let second_passage =
        hkask_memory::semantic_passage_for_h_mem(&second).expect("canonical second passage");
    assert!(
        memory
            .has_embedding_for_passage("shared-knowledge", &second_passage)
            .expect("second passage coverage")
    );
    assert!(
        memory
            .all_embeddings_with_text()
            .expect("embedding inventory")
            .iter()
            .all(|(entity, _vector, _passage, _model)| entity != "curator:goal:invalid-publication"),
        "backfill must never heal an invalid goal publication"
    )
}

// ── Memory backfill: model-mismatch sweep (the search gate's write side) ──

/// The card 9c17dd5f pin: a row stored under a model the search gate
/// excludes is re-embedded under the current model, and the same search
/// that excluded it before reports zero exclusions after (mixed store →
/// excluded count drops to 0).
#[tokio::test]
async fn model_mismatch_sweep_restores_gated_rows_until_search_excludes_nothing() {
    let (server, memory) = make_server_with_embeddings();
    let current_model =
        std::env::var("HKASK_EMBEDDING_MODEL").unwrap_or_else(|_| "test-model".to_string());

    memory
        .store_embedding(
            "sweep/stale",
            &unit_vector(),
            "legacy-model",
            Some("stale passage"),
        )
        .expect("seed stale-model row");
    memory
        .store_embedding(
            "sweep/current",
            &unit_vector(),
            &current_model,
            Some("current passage"),
        )
        .expect("seed current-model row");

    // BEFORE: the gate excludes the legacy row from the KNN window.
    let before = memory
        .search_similar(&unit_vector(), 10, &current_model, None)
        .expect("search before sweep");
    assert_eq!(
        before.excluded_model_mismatch, 1,
        "the legacy-model row is gated out before the sweep"
    );

    // The dry run names exactly the stale row.
    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: Some("model_mismatch".to_string()),
                store: None,
                dry_run: Some(true),
            }))
            .await
            .expect("dry run ok"),
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(dry["candidates"][0]["entity"].as_str(), Some("sweep/stale"));

    // The sweep re-embeds it under the current model.
    let swept = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: Some("model_mismatch".to_string()),
                store: None,
                dry_run: Some(false),
            }))
            .await
            .expect("sweep ok"),
    );
    assert_eq!(swept["re_embedded"].as_u64(), Some(1));

    // AFTER: the same search excludes nothing and both rows rank.
    let after = memory
        .search_similar(&unit_vector(), 10, &current_model, None)
        .expect("search after sweep");
    assert_eq!(
        after.excluded_model_mismatch, 0,
        "the swept row is recallable under the current model"
    );
    assert_eq!(after.results.len(), 2, "both rows rank after the sweep");
}

/// The gate accepts a row stored under the provider-confirmed actual form
/// (actual == stored); the sweep must accept it too — only rows matching
/// neither form are swept.
#[tokio::test]
async fn model_mismatch_sweep_accepts_the_provider_actual_form() {
    let (server, memory) = make_server_with_embedding_port(Arc::new(ActualFormEmbedPort));

    memory
        .store_embedding(
            "sweep/actual-form",
            &unit_vector(),
            "provider/actual-form",
            Some("actual-form passage"),
        )
        .expect("seed actual-form row");
    memory
        .store_embedding(
            "sweep/legacy",
            &unit_vector(),
            "legacy-model",
            Some("legacy passage"),
        )
        .expect("seed legacy row");

    let dry = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: Some("model_mismatch".to_string()),
                store: None,
                dry_run: Some(true),
            }))
            .await
            .expect("dry run ok"),
    );
    assert_eq!(
        dry["actual_model"].as_str(),
        Some("provider/actual-form"),
        "the identity probe surfaces the provider-confirmed actual form"
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(
        dry["candidates"][0]["entity"].as_str(),
        Some("sweep/legacy"),
        "the actual-form row is NOT swept — the gate accepts it, so the sweep must too"
    );
}

/// `HKASK_SWARM_MEMORY_DB` and `HKASK_DB_PASSPHRASE` are process-global —
/// every test that repoints the tool's swarm path must hold this lock for
/// its full seeded-DB lifetime (across the awaited tool calls) or the env
/// writes race. The tokio mutex is safe to hold across await points.
static SWARM_DB_ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Over the swarm store, missing-mode eligibility mirrors the swarm
/// server's own embed path — delegation response chunks only — and the
/// model-mismatch sweep reaches the swarm DB through the same tool.
#[tokio::test]
async fn backfill_reaches_the_swarm_store_with_response_chunk_eligibility() {
    let (server, _curator_memory) = make_server_with_embeddings();
    let _env_guard = SWARM_DB_ENV_LOCK.lock().await;

    // Route the tool's swarm path at a throwaway DB. The env writes are
    // test-only and serialized on SWARM_DB_ENV_LOCK; the
    // ensure_embedding_model_env precedent allows the unsafe set_var.
    let dir = std::env::temp_dir().join(format!("curator-swarm-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let db_path = dir.join("swarm-test.db");
    unsafe {
        std::env::set_var("HKASK_SWARM_MEMORY_DB", &db_path);
        std::env::set_var("HKASK_DB_PASSPHRASE", "test-passphrase");
    }

    let swarm_memory = hkask_memory::MemoryStore::open(
        &db_path.to_string_lossy(),
        "test-passphrase",
        hkask_storage::embedding_dim(),
    )
    .expect("open swarm test DB");

    // A delegation response chunk (the swarm's recall surface) and a
    // non-chunk row its write path never embeds.
    let chunk = hkask_storage::HMem::new(
        "agent:probe:turn:t1:chunk:0",
        "delegation:response_chunk",
        serde_json::json!({"text": "swarm response chunk passage"}),
        WebID::new(),
    );
    swarm_memory.store(chunk).expect("seed response chunk");
    let other = hkask_storage::HMem::new(
        "agent:probe:knowledge",
        "note",
        serde_json::json!({"text": "never embedded by the swarm write path"}),
        WebID::new(),
    );
    swarm_memory.store(other).expect("seed non-chunk row");
    // A stale-model row the mismatch sweep must retire. Its h_mem exists —
    // the swarm write path stores the chunk before embedding it, and an
    // embedding whose entity has no h_mem is an orphan the open-time
    // sweep retires first.
    let second_chunk = hkask_storage::HMem::new(
        "agent:probe:turn:t1:chunk:1",
        "delegation:response_chunk",
        serde_json::json!({"text": "second chunk passage"}),
        WebID::new(),
    );
    swarm_memory.store(second_chunk).expect("seed second chunk");
    swarm_memory
        .store_embedding(
            "agent:probe:turn:t1:chunk:1",
            &unit_vector(),
            "legacy-model",
            Some("second chunk passage"),
        )
        .expect("seed stale-model row");

    // Missing mode: only the response chunk is eligible.
    let missing = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: Some("swarm".to_string()),
                dry_run: Some(false),
            }))
            .await
            .expect("swarm missing backfill ok"),
    );
    assert_eq!(missing["store"].as_str(), Some("swarm"));
    assert_eq!(
        missing["backfilled"].as_u64(),
        Some(1),
        "only the delegation response chunk is eligible over the swarm store"
    );

    // Mismatch mode: the stale row is re-embedded under the current model.
    let current_model =
        std::env::var("HKASK_EMBEDDING_MODEL").unwrap_or_else(|_| "test-model".to_string());
    let swept = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: Some("model_mismatch".to_string()),
                store: Some("swarm".to_string()),
                dry_run: Some(false),
            }))
            .await
            .expect("swarm sweep ok"),
    );
    assert_eq!(swept["re_embedded"].as_u64(), Some(1));

    let after = swarm_memory
        .search_similar(&unit_vector(), 10, &current_model, None)
        .expect("search after swarm sweep");
    assert_eq!(after.excluded_model_mismatch, 0);
    assert_eq!(
        after.results.len(),
        2,
        "the chunk and the swept row both rank"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup tempdir");
}

/// Orphaned vec0 shadow rows — metadata deleted without vec access, e.g.
/// a therapy-style SQL pass — collide on re-insert at the reused rowid
/// (the 2026-09-28 swarm backfill failed 53/53 on exactly this). The
/// swarm open sweeps them first, so the backfill writes succeed.
#[tokio::test]
async fn swarm_backfill_sweeps_orphaned_vec_rows_at_open() {
    let (server, _curator_memory) = make_server_with_embeddings();
    let _env_guard = SWARM_DB_ENV_LOCK.lock().await;

    let dir = std::env::temp_dir().join(format!("curator-swarm-orphan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let db_path = dir.join("swarm-orphan-test.db");
    unsafe {
        std::env::set_var("HKASK_SWARM_MEMORY_DB", &db_path);
        std::env::set_var("HKASK_DB_PASSPHRASE", "test-passphrase");
    }

    // Seed a live response chunk with its embedding, then delete the
    // metadata row via raw SQL — the vec shadow row stays behind.
    let swarm_memory = hkask_memory::MemoryStore::open(
        &db_path.to_string_lossy(),
        "test-passphrase",
        hkask_storage::embedding_dim(),
    )
    .expect("open swarm test DB");
    let chunk = hkask_storage::HMem::new(
        "agent:probe:turn:t3:chunk:0",
        "delegation:response_chunk",
        serde_json::json!({"text": "orphan collision passage"}),
        WebID::new(),
    );
    swarm_memory.store(chunk).expect("seed response chunk");
    swarm_memory
        .store_embedding(
            "agent:probe:turn:t3:chunk:0",
            &unit_vector(),
            "legacy-model",
            Some("orphan collision passage"),
        )
        .expect("seed embedding");
    drop(swarm_memory);

    {
        let database = hkask_storage::open_or_repair(&db_path.to_string_lossy(), "test-passphrase")
            .expect("open raw handle");
        let conn = database
            .sqlite_pool()
            .expect("pool")
            .get()
            .expect("connection");
        conn.execute("DELETE FROM embeddings", ())
            .expect("delete metadata rows, orphaning the vec shadow row");
    }

    // Without the open-time sweep this fails on the vec_embeddings
    // UNIQUE constraint: the fresh metadata insert reuses rowid 1, which
    // the orphaned shadow row still holds.
    let result = parse(
        &server
            .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
                mode: None,
                store: Some("swarm".to_string()),
                dry_run: Some(false),
            }))
            .await
            .expect("swarm backfill over an orphaned store ok"),
    );
    assert_eq!(result["store"].as_str(), Some("swarm"));
    assert_eq!(
        result["backfilled"].as_u64(),
        Some(1),
        "the orphaned vec row was swept at open, so the write lands"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup tempdir");
}

/// Unknown mode or store values are rejected as invalid_argument — the
/// closed vocabulary never silently defaults a typo.
#[tokio::test]
async fn backfill_rejects_unknown_mode_and_store() {
    let (server, _memory) = make_server_with_embeddings();

    let mode_error = server
        .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
            mode: Some("sideways".to_string()),
            store: None,
            dry_run: Some(true),
        }))
        .await
        .expect_err("unknown mode must be rejected");
    assert!(
        matches!(mode_error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "unknown mode is invalid_argument — got: {mode_error:?}"
    );

    let store_error = server
        .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
            mode: None,
            store: Some("elsewhere".to_string()),
            dry_run: Some(true),
        }))
        .await
        .expect_err("unknown store must be rejected");
    assert!(
        matches!(store_error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "unknown store is invalid_argument — got: {store_error:?}"
    );
}

/// `curator_memory_prune` must default to turn-storage scope: aged turn
/// rows are hard-deleted while aged knowledge-layer rows survive.
/// `all_layers=true` is the explicit full-store opt-in. (therapy
/// 2026-09-04, P3 — the valve must fail closed on knowledge rows.)
#[tokio::test]
async fn curator_memory_prune_defaults_to_turn_storage_scope() {
    let (server, memory) = make_server_with_embeddings();

    let mut aged_turn = hkask_storage::HMem::new(
        "curator:thread:prune-tool-test",
        "turn",
        serde_json::Value::String("aged turn".to_string()),
        WebID::new(),
    );
    aged_turn.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    memory.store(aged_turn).expect("seed aged turn");

    let mut aged_ruling = hkask_storage::HMem::new(
        "zed-kask/prune-scope-test-ruling",
        "operator_ruling",
        serde_json::Value::String("durable ruling".to_string()),
        WebID::new(),
    );
    aged_ruling.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    memory.store(aged_ruling).expect("seed aged ruling");

    // Default scope: turn storage only — the ruling survives.
    let scoped = parse(
        &server
            .curator_memory_prune(Parameters(MemoryPruneRequest {
                max_age_days: 50,
                spare_recalled_within_days: None,
                all_layers: None,
                prefixes: None,
            }))
            .await
            .expect("scoped prune ok"),
    );
    assert_eq!(scoped["all_layers"].as_bool(), Some(false));
    assert_eq!(scoped["deleted_count"].as_u64(), Some(1));
    assert_eq!(
        memory
            .h_mems_by_entity_prefix("zed-kask/prune-scope-test-ruling")
            .expect("query ruling")
            .len(),
        1,
        "knowledge row must survive the default scope — got: {scoped}",
    );

    // Explicit opt-in: full store — the ruling is now deleted.
    let full = parse(
        &server
            .curator_memory_prune(Parameters(MemoryPruneRequest {
                max_age_days: 50,
                spare_recalled_within_days: None,
                all_layers: Some(true),
                prefixes: None,
            }))
            .await
            .expect("full prune ok"),
    );
    assert_eq!(full["all_layers"].as_bool(), Some(true));
    assert_eq!(full["deleted_count"].as_u64(), Some(1));
    assert!(
        memory
            .h_mems_by_entity_prefix("zed-kask/prune-scope-test-ruling")
            .expect("query ruling")
            .is_empty(),
        "all_layers=true must reach knowledge rows — got: {full}",
    );
}

/// `prefixes` narrows the prune valve to the named entity prefixes: aged
/// skill_use_issue rows are deleted while out-of-scope aged turn rows and
/// knowledge rows survive. Empty and all_layers-conflicting scope requests
/// are rejected visibly — a silent no-op or a silently-picked scope is a
/// broken feedback loop. (research card f461b8e5, 2026-09-28.)
#[tokio::test]
async fn curator_memory_prune_scoped_prefixes_prune_only_matching_rows() {
    let (server, memory) = make_server_with_embeddings();

    let mut aged_incident = hkask_storage::HMem::new(
        "skill_use_issue:prune-scope-test",
        "tool_failure:terminal",
        serde_json::Value::String("aged incident".to_string()),
        WebID::new(),
    );
    aged_incident.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    memory.store(aged_incident).expect("seed aged incident");

    let mut aged_turn = hkask_storage::HMem::new(
        "curator:thread:prune-prefix-test",
        "turn",
        serde_json::Value::String("aged turn".to_string()),
        WebID::new(),
    );
    aged_turn.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    memory.store(aged_turn).expect("seed aged turn");

    let mut aged_ruling = hkask_storage::HMem::new(
        "zed-kask/prune-prefix-test-ruling",
        "operator_ruling",
        serde_json::Value::String("durable ruling".to_string()),
        WebID::new(),
    );
    aged_ruling.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    memory.store(aged_ruling).expect("seed aged ruling");

    // Scoped valve: only the named prefix is pruned; the response names
    // the scope used.
    let scoped = parse(
        &server
            .curator_memory_prune(Parameters(MemoryPruneRequest {
                max_age_days: 50,
                spare_recalled_within_days: None,
                all_layers: None,
                prefixes: Some(vec!["skill_use_issue:".to_string()]),
            }))
            .await
            .expect("scoped-prefix prune ok"),
    );
    assert_eq!(scoped["deleted_count"].as_u64(), Some(1));
    assert_eq!(
        scoped["prefixes"].as_array().map(|list| list.len()),
        Some(1),
        "the response must name the scope used — got: {scoped}",
    );
    assert!(
        memory
            .h_mems_by_entity_prefix("skill_use_issue:prune-scope-test")
            .expect("query incident")
            .is_empty(),
        "the aged incident row must be deleted by its prefix scope — got: {scoped}",
    );
    assert_eq!(
        memory
            .h_mems_by_entity_prefix("curator:thread:prune-prefix-test")
            .expect("query turn")
            .len(),
        1,
        "an out-of-scope aged turn row must survive a prefixes-scoped prune — got: {scoped}",
    );
    assert_eq!(
        memory
            .h_mems_by_entity_prefix("zed-kask/prune-prefix-test-ruling")
            .expect("query ruling")
            .len(),
        1,
        "knowledge rows must survive a prefixes-scoped prune — got: {scoped}",
    );

    // Empty scope list: rejected visibly, nothing pruned.
    let empty = server
        .curator_memory_prune(Parameters(MemoryPruneRequest {
            max_age_days: 50,
            spare_recalled_within_days: None,
            all_layers: None,
            prefixes: Some(vec![]),
        }))
        .await;
    assert!(empty.is_err(), "an empty prefixes list must be rejected");

    // Conflicting scopes: rejected visibly rather than silently picking one.
    let conflicting = server
        .curator_memory_prune(Parameters(MemoryPruneRequest {
            max_age_days: 50,
            spare_recalled_within_days: None,
            all_layers: Some(true),
            prefixes: Some(vec!["skill_use_issue:".to_string()]),
        }))
        .await;
    assert!(
        conflicting.is_err(),
        "all_layers + prefixes must conflict visibly, not silently pick one"
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
                new_value: Some(serde_json::Value::String("new value".to_string()).into()),
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

// ── Memory edit tool schemas — strict-provider schema pins ────────────────
//
// schemars renders `serde_json::Value` as the bare boolean `true` in
// schema-valued positions. One boolean property schema gets the whole
// chat-completion rejected by strict-schema-decoding providers (Ollama:
// `400 cannot unmarshal bool into ... api.ToolProperty`; Gemini's protobuf
// `Schema` is the same failure class). The arbitrary-JSON params of the
// memory-edit tools must therefore be `AnyJsonValue`, rendering as the
// object-typed permissive schema tool-call parsers can bind.

/// `memory_insert`'s `value` param must render as the object-typed
/// permissive schema — never the bare boolean `true`.
#[test]
fn memory_insert_request_value_schema_is_object_typed_not_boolean() {
    let schema = serde_json::to_value(schemars::schema_for!(MemoryInsertRequest))
        .expect("schema serializes");
    let value = &schema["properties"]["value"];
    assert!(
        !value.is_boolean(),
        "value must not render as the bare boolean true (Schema::Bool(true)) — got: {value}",
    );
    // Key-level pin (schemars merges the field's doc comment as a
    // `description` key, so exact equality would over-constrain): the
    // property must be the object-typed permissive schema — `type: object`
    // with permissive `properties`/`additionalProperties` — never the
    // permissive-any form (bare `true` / `{}`) schemars emits for
    // `serde_json::Value`, which strict providers reject or drop from
    // tool-call arguments.
    assert_eq!(
        value["type"],
        serde_json::json!("object"),
        "value must be object-typed — got: {value}",
    );
    assert_eq!(value["properties"], serde_json::json!({}));
    assert_eq!(value["additionalProperties"], serde_json::json!({}));
    assert!(
        hkask_types::find_boolean_schema_positions(&schema).is_empty(),
        "MemoryInsertRequest must carry no bare-boolean schema positions",
    );
}

/// `memory_update`'s `new_value` param must render as the null-extended
/// object-typed permissive schema (`Option<AnyJsonValue>` →
/// `type: ["object", "null"]`) — never the bare boolean `true`.
#[test]
fn memory_update_request_new_value_schema_is_object_typed_not_boolean() {
    let schema = serde_json::to_value(schemars::schema_for!(MemoryUpdateRequest))
        .expect("schema serializes");
    let new_value = &schema["properties"]["new_value"];
    assert!(
        !new_value.is_boolean(),
        "new_value must not render as the bare boolean true (Schema::Bool(true)) — got: {new_value}",
    );
    // Key-level pin (the doc-comment `description` merge makes exact
    // equality over-constrained): `Option<AnyJsonValue>` must render as
    // the object-typed permissive schema null-extended to
    // `type: ["object", "null"]` — never the permissive-any form.
    assert_eq!(
        new_value["type"],
        serde_json::json!(["object", "null"]),
        "new_value must be null-extended object-typed — got: {new_value}",
    );
    assert_eq!(new_value["properties"], serde_json::json!({}));
    assert_eq!(new_value["additionalProperties"], serde_json::json!({}));
    assert!(
        hkask_types::find_boolean_schema_positions(&schema).is_empty(),
        "MemoryUpdateRequest must carry no bare-boolean schema positions",
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

// ── Regulation query — namespace, time, and limit semantics ──────────────────

fn regulation_record(
    namespace: &str,
    path: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
    observation: &str,
) -> RegulationRecord {
    let mut record = RegulationRecord::new(
        WebID::new(),
        Span::new(
            SpanNamespace::new(namespace).expect("test namespace must be canonical"),
            path,
        ),
        CyclePhase::Sense,
        serde_json::json!({"observation": observation}),
        0,
    );
    record.timestamp = timestamp;
    record
}

#[tokio::test]
async fn reg_query_filters_namespace_in_sql_before_limit() {
    let (server, archive) = make_server_with_regulation_archive();
    let base = chrono::Utc::now() - chrono::Duration::minutes(10);
    let records = [
        regulation_record("reg.inference", "request", base, "earlier inference"),
        regulation_record(
            "reg.curation",
            "review",
            base + chrono::Duration::seconds(1),
            "earlier curation",
        ),
        regulation_record(
            "reg.skill",
            "program-managerish.operator_feedback",
            base + chrono::Duration::seconds(2),
            "textual prefix collision",
        ),
        regulation_record(
            "reg.skill",
            "program-manager.operator_feedback",
            base + chrono::Duration::seconds(3),
            "genuine rejection",
        ),
    ];
    for record in &records {
        archive.persist(record).expect("seed regulation record");
    }

    let skill_response = parse(
        &server
            .reg_query(Parameters(RegQueryRequest {
                namespace: Some("reg.skill".to_string()),
                window_seconds: Some(3600),
                limit: Some(10),
            }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(skill_response["count"].as_u64(), Some(2));
    assert!(
        skill_response["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["phase"] == "Sense"),
        "sense-phase skill feedback must be visible: {skill_response}",
    );

    let exact_response = parse(
        &server
            .reg_query(Parameters(RegQueryRequest {
                namespace: Some("reg.skill.program-manager".to_string()),
                window_seconds: Some(3600),
                limit: Some(1),
            }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(exact_response["count"].as_u64(), Some(1));
    assert_eq!(
        exact_response["events"][0]["path"].as_str(),
        Some("reg.skill.program-manager.operator_feedback"),
        "the namespace predicate must run before LIMIT and respect dot boundaries: {exact_response}",
    );

    let all_response = parse(
        &server
            .reg_query(Parameters(RegQueryRequest {
                namespace: None,
                window_seconds: Some(3600),
                limit: Some(10),
            }))
            .await
            .expect("tool ok"),
    );
    let all_events = all_response["events"].as_array().expect("events array");
    assert_eq!(all_response["count"].as_u64(), Some(4));
    assert_eq!(
        all_events[0]["observation"]["observation"],
        "earlier inference"
    );
    assert_eq!(
        all_events[3]["observation"]["observation"],
        "genuine rejection"
    );
}

/// A query whose arguments carried no scope at all is the truncation
/// signature (the model's tool-call JSON was cut before dispatch, so every
/// `Option` field landed as `None`). It must fail loud, naming what is
/// missing — never silently default to "all namespaces / last hour" and
/// return wrong-filtered data as a success. `namespace: None` WITH a
/// `window_seconds` remains the documented "all namespaces" query.
#[tokio::test]
async fn reg_query_rejects_scopeless_arguments_instead_of_defaulting() {
    let (server, _archive) = make_server_with_regulation_archive();

    let error = server
        .reg_query(Parameters(RegQueryRequest {
            namespace: None,
            window_seconds: None,
            limit: None,
        }))
        .await
        .expect_err("scopeless arguments must be rejected, not defaulted");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
    assert!(
        error.message.contains("namespace") || error.message.contains("window_seconds"),
        "the rejection must name the missing scope field: {}",
        error.message
    );
}

#[tokio::test]
async fn reg_query_surfaces_unavailable_archive_as_typed_error() {
    let server = CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::from_stores(CuratorStores {
            regulation_store: None,
            memory: None,
        })),
        failing_inference_port(),
    );

    let error = server
        .reg_query(Parameters(RegQueryRequest {
            namespace: Some("reg.skill".to_string()),
            window_seconds: Some(3600),
            limit: Some(10),
        }))
        .await
        .expect_err("missing RegulationArchive must be visible");
    assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
    assert!(error.message.contains("RegulationArchive not available"));
}

// ── Algedonic log — happy ───────────────────────────────────────────────────

/// expect: "The operational algedonic log returns the newest events within its bounded window" [P9]
#[tokio::test]
async fn algedonic_log_returns_newest_events_and_declares_ordering() {
    let (server, archive) = make_server_with_regulation_archive();
    let base = chrono::Utc::now() - chrono::Duration::minutes(3);
    for (offset, label) in [(0, "oldest"), (1, "middle"), (2, "newest")] {
        let mut event = RegulationRecord::new(
            WebID::from_persona(b"regulation"),
            hkask_types::event::Span::from_kind(hkask_types::event::SpanKind::LoopMetricsTelemetry),
            CyclePhase::Act,
            serde_json::json!({"label": label}),
            0,
        );
        event.timestamp = base + chrono::Duration::minutes(offset);
        archive.persist(&event).expect("insert event");
    }

    let response = parse(
        &server
            .curator_algedonic_log(Parameters(AlgedonicLogRequest { hours: Some(1) }))
            .await
            .expect("tool ok"),
    );
    assert_eq!(response["ordering"], "newest_first");
    let labels = response["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["observation"]["label"].as_str().expect("label"))
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["newest", "middle", "oldest"]);

    let replay = archive
        .query_algedonic(base - chrono::Duration::seconds(1), 3)
        .expect("chronological replay");
    let replay_labels = replay
        .iter()
        .map(|event| event.observation["label"].as_str().expect("label"))
        .collect::<Vec<_>>();
    assert_eq!(
        replay_labels,
        vec!["oldest", "middle", "newest"],
        "operational recency must not change chronological replay order"
    );
}

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

fn federated_fixture_vector() -> Vec<f32> {
    let mut vector = vec![0.0; test_dim()];
    vector[0] = 1.0;
    vector
}

fn federated_fixture_sha256(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::Digest as _;
    use std::io::Read as _;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sorted_federated_fixture(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut result = serde_json::Map::new();
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(value) = object.get(key) {
                    result.insert(key.clone(), sorted_federated_fixture(value));
                }
            }
            serde_json::Value::Object(result)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(sorted_federated_fixture).collect())
        }
        other => other.clone(),
    }
}

fn federated_fixture_run_id(
    identity: &serde_json::Value,
) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::Digest as _;
    let mut canonical = serde_json::to_vec(&sorted_federated_fixture(identity))?;
    canonical.push(b'\n');
    Ok(format!("{:x}", sha2::Sha256::digest(&canonical)))
}

fn federated_source_fixture(
    directory: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let database_path = directory.join("reference.db");
    let database = database_path
        .to_str()
        .ok_or_else(|| std::io::Error::other("non-UTF-8 database path"))?;
    let entity_ref = "calibration:fixture:sealed-v1:reference:utf8-65766964656e63652e747874:0";
    {
        let store = hkask_memory::MemoryStore::open(database, "test-passphrase", test_dim())?;
        store.store(hkask_storage::HMem::new(
            entity_ref,
            "text",
            serde_json::json!("external corpus evidence"),
            WebID::new(),
        ))?;
        store.store(hkask_storage::HMem::new(
            entity_ref,
            "method_signals",
            serde_json::json!({"parataxis_ratio": 1.0}),
            WebID::new(),
        ))?;
        store.store_embedding(
            entity_ref,
            &federated_fixture_vector(),
            "test-model",
            Some("external corpus evidence"),
        )?;
    }
    {
        let database = hkask_storage::open_or_repair(database, "test-passphrase")?;
        let pool = database.sqlite_pool()?;
        let connection = pool.get()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        let sidecar = format!("{database}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }
    let digest = federated_fixture_sha256(&database_path)?;
    let representations_path = directory.join("representations-manifest.json");
    std::fs::write(
        &representations_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "entity_ref_prefix": "calibration:fixture:sealed-v1",
            "boilerplate_exclusion_reports": {
                "fixture.txt": {"input_words": 3, "retained_words": 3, "exclusions": []}
            },
            "validation": {
                "accepted_source_count": 1,
                "boilerplate_filter_applied": true
            }
        }))?,
    )?;
    let manifest_digest = federated_fixture_sha256(&representations_path)?;
    let run_identity_path = directory.join("run-identity.json");
    let mut identity = serde_json::json!({
        "schema_version": 3,
        "preseal_run_id": "a".repeat(64),
        "accepted_sources_sha256": "b".repeat(64),
        "run_spec_sha256": "c".repeat(64),
        "queries_sha256": "d".repeat(64),
        "requested_embedding_model": "test-model",
        "actual_embedding_model": "test-model",
        "policies_sha256": "e".repeat(64),
        "retriever_sha256": "f".repeat(64),
        "evaluator_sha256": "0".repeat(64),
        "representations_manifest_sha256": manifest_digest,
        "representations": {
            "reference": "1".repeat(64),
            "current": "2".repeat(64),
            "fine": "3".repeat(64),
            "child_parent_map": "4".repeat(64),
            "parent": "5".repeat(64)
        },
        "indexes": {"reference": digest, "current": "6".repeat(64), "fine": "7".repeat(64)}
    });
    identity["run_id"] = serde_json::json!(federated_fixture_run_id(&identity)?);
    std::fs::write(&run_identity_path, serde_json::to_vec_pretty(&identity)?)?;
    let manifest_path = directory.join("federated-sources.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "sources": [{
                "id": "fixture-corpus",
                "display_name": "Fixture corpus",
                "database_path": database_path,
                "run_identity_path": run_identity_path,
                "representations_manifest_path": representations_path,
                "index_name": "reference"
            }]
        }))?,
    )?;
    Ok(manifest_path)
}

/// expect: "One explicit search returns Curator experience and sealed corpus evidence with provenance." [P8]
#[tokio::test]
async fn federated_search_interleaves_sources_without_mutating_corpus()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_embedding_model_env();
    let directory = tempfile::tempdir()?;
    let manifest_path = federated_source_fixture(directory.path())?;
    let database_path = directory.path().join("reference.db");
    let before = std::fs::read(&database_path)?;

    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone())?;
    let embedding_store = EmbeddingStore::from_driver(driver.clone(), test_dim())?;
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let local = hkask_storage::HMem::new(
        "curator:decision:fixture",
        "lesson",
        serde_json::json!("curator experience"),
        WebID::new(),
    );
    memory.store(local.clone())?;
    memory.store_embedding(
        &local.entity,
        &federated_fixture_vector(),
        "test-model",
        Some("curator experience"),
    )?;
    let stores = CuratorStores {
        regulation_store: Some(Arc::new(RegulationArchive::from_driver(driver)?)),
        memory: Some(memory),
    };
    let database = Arc::new(CuratorDb::from_stores_with_federated_manifest(
        stores,
        manifest_path,
        "test-passphrase".to_string(),
    ));
    let server = CuratorServer::new(
        WebID::new(),
        database,
        Arc::new(ConstantEmbedPort) as Arc<dyn hkask_types::InferencePort>,
    );

    let response = parse(
        &server
            .curator_federated_search(Parameters(FederatedSearchRequest {
                query: "compare local experience with external evidence".to_string(),
                limit: Some(4),
            }))
            .await?,
    );
    assert_eq!(response["count"].as_u64(), Some(2));
    assert_eq!(response["results"][0]["source_kind"], "curator");
    assert_eq!(response["results"][0]["text"], "curator experience");
    assert_eq!(response["results"][1]["source_kind"], "corpus");
    assert_eq!(response["results"][1]["text"], "external corpus evidence");
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("run-identity.json"))?)?;
    assert_eq!(response["results"][1]["run_id"], identity["run_id"]);
    assert_eq!(response["sources"][0]["state"], "ready");
    assert_eq!(response["sources"][1]["state"], "ready");
    assert!(!response.to_string().contains("parataxis_ratio"));
    assert_eq!(std::fs::read(&database_path)?, before);
    Ok(())
}

/// expect: "One unavailable corpus cannot erase another corpus's evidence." [P8]
#[tokio::test]
async fn federated_search_preserves_healthy_source_during_partial_outage()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_embedding_model_env();
    let directory = tempfile::tempdir()?;
    let manifest_path = federated_source_fixture(directory.path())?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    let mut missing = manifest["sources"][0].clone();
    missing["id"] = serde_json::json!("unavailable-corpus");
    missing["database_path"] = serde_json::json!(directory.path().join("missing.db"));
    manifest["sources"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("sources must be an array"))?
        .push(missing);
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone())?;
    let embedding_store = EmbeddingStore::from_driver(driver, test_dim())?;
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let local = hkask_storage::HMem::new(
        "curator:partial-outage",
        "lesson",
        serde_json::json!("local evidence"),
        WebID::new(),
    );
    memory.store(local.clone())?;
    memory.store_embedding(
        &local.entity,
        &federated_fixture_vector(),
        "test-model",
        Some("local evidence"),
    )?;
    let server = CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::from_stores_with_federated_manifest(
            CuratorStores {
                regulation_store: None,
                memory: Some(memory),
            },
            manifest_path,
            "test-passphrase".to_string(),
        )),
        Arc::new(ConstantEmbedPort) as Arc<dyn hkask_types::InferencePort>,
    );
    let output = server
        .curator_federated_search(Parameters(FederatedSearchRequest {
            query: "compare local evidence and external corpus evidence".to_string(),
            limit: Some(4),
        }))
        .await?;
    let response = parse(&output);
    assert_eq!(response["sources"][0]["state"], "ready");
    assert_eq!(response["sources"][1]["state"], "ready");
    assert_eq!(response["sources"][2]["state"], "unavailable");
    assert!(
        response["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().any(|hit| hit["source_id"] == "fixture-corpus"))
    );
    assert!(
        response["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().any(|hit| hit["source_id"] == "curator"))
    );
    Ok(())
}

/// expect: "A source removed after first search cannot remain ready or leak stale corpus hits." [P8]
#[tokio::test]
async fn federated_search_reloads_removed_and_restored_manifest()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_embedding_model_env();
    let directory = tempfile::tempdir()?;
    let manifest_path = federated_source_fixture(directory.path())?;
    let original_manifest = std::fs::read(&manifest_path)?;
    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone())?;
    let embedding_store = EmbeddingStore::from_driver(driver.clone(), test_dim())?;
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let local = hkask_storage::HMem::new(
        "curator:decision:manifest-refresh",
        "lesson",
        serde_json::json!("local knowledge survives corpus outage"),
        WebID::new(),
    );
    memory.store(local.clone())?;
    memory.store_embedding(
        &local.entity,
        &federated_fixture_vector(),
        "test-model",
        Some("local knowledge survives corpus outage"),
    )?;
    let db = Arc::new(CuratorDb::from_stores_with_federated_manifest(
        CuratorStores {
            regulation_store: None,
            memory: Some(memory),
        },
        manifest_path.clone(),
        "test-passphrase".to_string(),
    ));
    let server = CuratorServer::new(
        WebID::new(),
        db,
        Arc::new(ConstantEmbedPort) as Arc<dyn hkask_types::InferencePort>,
    );
    let search = || async {
        let output = server
            .curator_federated_search(Parameters(FederatedSearchRequest {
                query: "compare local knowledge and external corpus evidence".to_string(),
                limit: Some(4),
            }))
            .await?;
        Ok::<_, hkask_mcp_server::server::McpToolError>(parse(&output))
    };

    let first = search().await?;
    assert_eq!(first["sources"][1]["state"], "ready");
    assert!(
        first["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().any(|hit| hit["source_kind"] == "corpus"))
    );

    std::fs::remove_file(&manifest_path)?;
    let absent = search().await?;
    assert_eq!(absent["sources"][1]["state"], "unconfigured");
    assert!(
        absent["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().all(|hit| hit["source_kind"] == "curator"))
    );
    assert_eq!(absent["sources"][0]["state"], "ready");

    std::fs::write(&manifest_path, original_manifest)?;
    let restored = search().await?;
    assert_eq!(restored["sources"][1]["state"], "ready");
    assert!(
        restored["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().any(|hit| hit["source_kind"] == "corpus"))
    );

    let run_identity_path = directory.path().join("run-identity.json");
    let original_identity = std::fs::read(&run_identity_path)?;
    let mut tampered_identity: serde_json::Value = serde_json::from_slice(&original_identity)?;
    tampered_identity["run_id"] = serde_json::json!("0".repeat(64));
    std::fs::write(
        &run_identity_path,
        serde_json::to_vec_pretty(&tampered_identity)?,
    )?;
    let unsealed = search().await?;
    assert_eq!(unsealed["sources"][1]["state"], "incompatible");
    assert!(
        unsealed["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().all(|hit| hit["source_kind"] == "curator"))
    );
    std::fs::write(&run_identity_path, original_identity)?;
    assert_eq!(search().await?["sources"][1]["state"], "ready");

    use std::io::Write as _;
    let source_db_path = directory.path().join("reference.db");
    let original_db = std::fs::read(&source_db_path)?;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&source_db_path)?
        .write_all(b"stale")?;
    let changed_db = search().await?;
    assert_eq!(changed_db["sources"][1]["state"], "incompatible");
    assert!(
        changed_db["results"]
            .as_array()
            .is_some_and(|hits| hits.iter().all(|hit| hit["source_kind"] == "curator"))
    );
    std::fs::write(&source_db_path, original_db)?;
    assert_eq!(search().await?["sources"][1]["state"], "ready");
    Ok(())
}

/// expect: "An unconfigured external source is visible and cannot suppress healthy Curator recall." [P8]
#[tokio::test]
async fn federated_search_surfaces_unconfigured_source_with_curator_results() {
    let (server, memory) = make_server_with_embeddings();
    let local = hkask_storage::HMem::new(
        "curator:decision:unconfigured",
        "lesson",
        serde_json::json!("curator-only evidence"),
        WebID::new(),
    );
    memory.store(local.clone()).expect("store local h_mem");
    memory
        .store_embedding(
            &local.entity,
            &federated_fixture_vector(),
            "test-model",
            Some("curator-only evidence"),
        )
        .expect("store local embedding");

    let response = parse(
        &server
            .curator_federated_search(Parameters(FederatedSearchRequest {
                query: "retrieve available experience with source status".to_string(),
                limit: Some(4),
            }))
            .await
            .expect("tool response"),
    );
    assert_eq!(response["count"].as_u64(), Some(1));
    assert_eq!(response["results"][0]["source_kind"], "curator");
    assert_eq!(response["sources"][0]["state"], "ready");
    assert_eq!(response["sources"][1]["state"], "unconfigured");
    assert!(
        response["sources"][1]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("not configured"))
    );
}

/// Federated twin of the KNN-orphan trap: when every curator hit fails
/// resolution, the curator source status must stay Ready (the leg ran)
/// but carry the failure count in its reason — Ready + result_count 0 +
/// no reason reads as "the curator store is empty".
#[tokio::test]
async fn federated_search_ready_path_carries_resolution_failure_count() {
    let (server, memory) = make_server_with_embeddings();
    let mut vector = vec![0.0f32; test_dim()];
    vector[0] = 1.0;
    memory
        .store_embedding(
            "curator:thread:federated-orphan",
            &vector,
            "test-model",
            None,
        )
        .expect("seed orphan embedding (no h_mem under the entity)");

    let response = parse(
        &server
            .curator_federated_search(Parameters(FederatedSearchRequest {
                query: "any question words".to_string(),
                limit: None,
            }))
            .await
            .expect("tool ok"),
    );

    let curator_status = response["sources"]
        .as_array()
        .unwrap_or_else(|| panic!("sources list missing — got: {response}"))
        .iter()
        .find(|status| status["source_id"] == "curator")
        .cloned()
        .unwrap_or_else(|| panic!("curator source status missing — got: {response}"));
    assert_eq!(
        curator_status["state"], "ready",
        "the curator leg ran — got: {response}",
    );
    assert_eq!(
        curator_status["result_count"].as_u64(),
        Some(0),
        "no hit resolved to an h_mem — got: {response}",
    );
    assert!(
        curator_status["reason"]
            .as_str()
            .is_some_and(|r| r.contains("1 semantic hits failed h_mem resolution")),
        "the Ready status must carry the failure count — got: {response}",
    );
}
