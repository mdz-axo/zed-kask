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
use hkask_mcp_test_support::{invoke_tool, parse_envelope};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EmbeddingStore, HMemStore, RegulationArchive};
use hkask_types::event::{CyclePhase, Span, SpanNamespace};
use hkask_types::{RegulationRecord, RegulationSink, WebID};
use rmcp::handler::server::wrapper::Parameters;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// The pinned future every test port's arms return — the trait's
/// `Pin<Box<dyn Future<Output = Result<...>> + Send>>` spelling, aliased so
/// each mock's signature carries its parameters and not ten lines of
/// boilerplate.
type CuratorTestFuture<'a> = Pin<
    Box<
        dyn Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
            + Send
            + 'a,
    >,
>;

/// The `generate` stub shared by the test ports: every mock here exercises
/// an embed arm (or nothing); `generate` answers with the stub connection
/// error. Single-sourced so the three mocks cannot drift apart.
fn stub_generate() -> CuratorTestFuture<'static> {
    Box::pin(async { Err(hkask_types::InferenceError::Connection("stub".to_string())) })
}

/// Emit the shared stub `generate` arm inside a test port's
/// `impl InferencePort` block — the arm every mock here repeats.
macro_rules! stub_generate_arm {
    () => {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> CuratorTestFuture<'_> {
            stub_generate()
        }
    };
}

/// Stub inference port whose `embed` is left at the trait default (an
/// error) — pins the degradation path: the semantic tools must fall back
/// to exact-entity lookup (surfaced in the output) rather than erroring
/// or silently returning empty.
struct FailingEmbedPort;

impl hkask_types::InferencePort for FailingEmbedPort {
    stub_generate_arm!();

    fn embed_with_dimensions<'a>(
        &'a self,
        _model: &str,
        _texts: &[String],
        _dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        // The semantic tools call the width-bearing arm with the store's
        // width; fail it explicitly with the same message the width-less
        // default produces, so the degradation path under test (fall back to
        // exact-entity lookup, surfaced) stays the pin — not the trait
        // default's width-discard guard.
        Box::pin(async {
            Err(hkask_types::EmbeddingGenerationError::Connection(
                "embed not supported by this InferencePort".to_string(),
            ))
        })
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
    stub_generate_arm!();

    fn embed<'a>(&'a self, _model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        // Capture only the count (owned, `Copy`) so the future borrows
        // nothing.
        let count = texts.len();
        let dim = test_dim();
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors: (0..count)
                    .map(|_| {
                        let mut vector = vec![0.0f32; dim];
                        vector[0] = 1.0;
                        vector
                    })
                    .collect(),
                requested_model: String::new(),
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        // Explicit width routing (not the trait default): the semantic tools
        // pass the store's width, and this stub embeds at the same resolver
        // (`test_dim`) — a mismatch is a test bug, asserted here.
        if let Some(d) = dimensions {
            debug_assert_eq!(d as usize, test_dim());
        }
        self.embed_with_identity(model, texts)
    }
}

/// A port whose `embed_with_dimensions` reports a provider-confirmed actual
/// model — the shape the model-mismatch sweep's identity probe reads, used
/// to pin the gate's two-form acceptance (a row stored under the actual form
/// is NOT swept).
struct ActualFormEmbedPort;

impl hkask_types::InferencePort for ActualFormEmbedPort {
    stub_generate_arm!();

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
                usage: hkask_types::InferenceUsage::default(),
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

/// Records every `embed_with_dimensions` call's model — the routing
/// falsifier's instrument (D100's named follow-up, 2026-11-06). Returns the
/// constant unit vectors so the search proceeds past the embed.
#[derive(Default)]
struct RecordingEmbedPort {
    models: std::sync::Mutex<Vec<String>>,
}

impl RecordingEmbedPort {
    fn embedded_models(&self) -> Vec<String> {
        self.models
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl hkask_types::InferencePort for RecordingEmbedPort {
    stub_generate_arm!();

    fn embed<'a>(&'a self, _model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        // The ConstantEmbedPort shape: unit vectors at the test width.
        let count = texts.len();
        let dim = test_dim();
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors: (0..count)
                    .map(|_| {
                        let mut vector = vec![0.0f32; dim];
                        vector[0] = 1.0;
                        vector
                    })
                    .collect(),
                requested_model: String::new(),
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
            })
        })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        if let Some(d) = dimensions {
            debug_assert_eq!(d as usize, test_dim());
        }
        self.models
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(model.to_string());
        self.embed_with_identity(model, texts)
    }
}

fn failing_inference_port() -> Arc<dyn hkask_types::InferencePort> {
    Arc::new(FailingEmbedPort)
}

/// Seed one h_mem into the test memory store — the shared construction of
/// every test that plants a row before exercising a read path. Returns the
/// stored row's id (the citation tests cite it as `memory_insert` evidence).
/// The entity, attribute, and value are the test's own variation.
fn seed_h_mem(
    memory: &hkask_memory::MemoryStore,
    entity: &str,
    attribute: &str,
    value: String,
) -> String {
    let h_mem = hkask_storage::HMem::new(
        entity,
        attribute,
        serde_json::Value::String(value),
        WebID::new(),
    );
    let id = h_mem.id.to_string();
    memory.store(h_mem).expect("seed h_mem");
    id
}

/// Seed the unit-vector embedding under `entity` — the KNN shape every
/// semantic test searches with (the `ConstantEmbedPort` embeds queries to
/// the same unit vector, so every seeded row is a guaranteed hit). An
/// entity with an embedding but no h_mem is the orphan-resolution fixture.
fn seed_embedding(memory: &hkask_memory::MemoryStore, entity: &str) {
    seed_embedding_with_model(memory, entity, "test-model");
}

/// The D100-follow-up shape: a row whose embedding lives in a named model's
/// space — `"test-private-model"` seeds the private leg's local space, the
/// space a private call context's query vector matches.
fn seed_embedding_with_model(memory: &hkask_memory::MemoryStore, entity: &str, model: &str) {
    memory
        .store_embedding(entity, &unit_vector(), model, None)
        .expect("seed embedding");
}

/// Seed one aged h_mem — `observed_at` back-dated `age_days` — so the prune
/// valve's age scope has a row to act on. The entity, attribute, value, and
/// age are the test's own variation.
fn seed_aged_h_mem(
    memory: &hkask_memory::MemoryStore,
    entity: &str,
    attribute: &str,
    value: String,
    age_days: i64,
) {
    let mut h_mem = hkask_storage::HMem::new(
        entity,
        attribute,
        serde_json::Value::String(value),
        WebID::new(),
    );
    h_mem.observed_at = chrono::Utc::now() - chrono::Duration::days(age_days);
    memory.store(h_mem).expect("seed aged h_mem");
}

/// The shared `memory_insert` call of the citation tests: insert the
/// default-agent-model memory citing `seed_id` as evidence. The two
/// insert-path tests make this call identically — their assertions (the
/// confidence floor vs the semantic-recall embedding) are each test's own.
async fn insert_citing_seed(server: &CuratorServer, seed_id: String) -> serde_json::Value {
    let request: MemoryInsertRequest = serde_json::from_value(serde_json::json!({
        "entity": "zed-kask",
        "attribute": "default_agent_model",
        "value": serde_json::to_string("qwen3").expect("JSON text for a literal string"),
        "evidence_h_mem_id": seed_id,
        "note": null
    }))
    .expect("decode the model-facing tool request");
    parse_envelope(
        &server
            .memory_insert(Parameters(request))
            .await
            .expect("tool ok"),
    )
}

/// The semantic paths resolve the embedding model from
/// `HKASK_EMBEDDING_MODEL` (an `Option` since the model_constants
/// refactor — unset means the semantic paths fail visibly: search errors
/// with `EmbeddingNotConfigured`, inserts fail with `failed_precondition`).
/// The test binary sets it once so the semantic leg exercises; the
/// write is one-shot and test-only (the crate root allows `unsafe` in
/// test builds for exactly this pattern).
fn ensure_embedding_model_env() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        if std::env::var("HKASK_EMBEDDING_MODEL").is_err() {
            // SAFETY: test-only env write, executed once before any test
            // body relies on it, always to the same value.
            unsafe { std::env::set_var("HKASK_EMBEDDING_MODEL", "test-model") };
        }
        // The private leg's LOCAL model (D100's named follow-up,
        // 2026-11-06): distinct from the configured model so the routing
        // pins can tell the two apart; set in the same one-shot for the
        // same reasons (a later test cannot unset a sibling's write).
        if std::env::var("HKASK_PRIVATE_EMBEDDING_MODEL").is_err() {
            // SAFETY: test-only env write, one-shot, always the same value.
            unsafe { std::env::set_var("HKASK_PRIVATE_EMBEDDING_MODEL", "test-private-model") };
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

// ── Liveness ──────────────────────────────────────────────────────────────

/// `curator_ping` returns `status: "ok"` and reports both stores as live
/// when the DB opened successfully.
#[tokio::test]
async fn ping_reports_store_health() {
    let server = make_server();
    let response = invoke_tool!(server, curator_ping, PingRequest {});

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
            entity: None,
            recall_shape: MemoryRecallType::default(),
            ontology_axis: Some("dc_type".to_string()),
            ontology_value: None,
            limit: None,
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
            entity: None,
            recall_shape: MemoryRecallType::default(),
            ontology_axis: Some("bogus_axis".to_string()),
            ontology_value: Some("whatever".to_string()),
            limit: None,
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
    let response = invoke_tool!(
        server,
        curator_memory_recall,
        MemoryRecallRequest {
            entity: Some("never-seen".to_string()),
            recall_shape: MemoryRecallType::Both,
            ontology_axis: None,
            ontology_value: None,
            limit: None,
        }
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
    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "no-such-entity".to_string(),
            limit: None,
        }
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
    seed_h_mem(&memory, entity, "turn", turn);
    seed_embedding(&memory, entity);

    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "kangaroo wallaby emu cassowary".to_string(),
            limit: None,
        }
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
    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "no-such-entity".to_string(),
            limit: None,
        }
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

    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        }
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
    seed_h_mem(&memory, entity, "turn", "live turn content".to_string());
    seed_embedding(&memory, entity);
    seed_embedding(&memory, "curator:thread:partial-orphan");

    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        }
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
    seed_h_mem(&memory, entity, "turn", turn);
    seed_embedding(&memory, entity);

    let response = invoke_tool!(
        server,
        curator_consult,
        CuratorConsultRequest {
            query: "completely unrelated question words".to_string(),
            limit: None,
        }
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
    seed_embedding(&memory, "curator:thread:consult-orphan");

    let response = invoke_tool!(
        server,
        curator_consult,
        CuratorConsultRequest {
            query: "any question words".to_string(),
            limit: None,
        }
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
    seed_h_mem(&memory, entity, "turn", "live consult turn".to_string());
    seed_embedding(&memory, entity);
    seed_embedding(&memory, "curator:thread:consult-partial-orphan");

    let response = invoke_tool!(
        server,
        curator_consult,
        CuratorConsultRequest {
            query: "any question words".to_string(),
            limit: None,
        }
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
    let seed_id = seed_h_mem(
        &memory,
        "curator:thread:evidence-source",
        "chatted",
        "the source turn".to_string(),
    );

    let response = insert_citing_seed(&server, seed_id).await;

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

/// FR-4 (D-2): `memory_insert` DERIVES the inference-provenance mark from
/// the cited evidence row — the evidence carries the producing thread's
/// routing state (stamped at the turn-completion ingest seam), so a memory
/// distilled from private-marked content stays private. The request schema
/// offers no field to claim: a model-facing surface can never set or clear
/// the mark (P4 — a caller-supplied value is not a boundary). Both
/// directions pinned: private evidence → private row; public evidence →
/// public row.
#[tokio::test]
async fn memory_insert_inherits_the_evidence_inference_provenance() {
    let (server, memory) = make_server_with_embeddings();

    // A private-marked evidence row (the shape the turn-completion ingest
    // writes for a D100-marked thread).
    let private_evidence = hkask_storage::HMem::new(
        "curator:thread:private-source",
        "chatted",
        serde_json::Value::String("a private turn".to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    let private_id = private_evidence.id.to_string();
    memory
        .store(private_evidence)
        .expect("seed private evidence");

    // Q2 (operator ruling 2026-11-07): citing private-marked evidence
    // belongs to a PRIVATE call context — the direct-ID provenance
    // boundary refuses it from a public one, matching the flow this pin
    // models (a private thread's ingest).
    let response = hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        insert_citing_seed(&server, private_id),
    )
    .await;
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "insert citing private evidence must succeed — got: {response}"
    );
    let stored = memory
        .query_deduped_untouched("zed-kask")
        .expect("query should succeed");
    let private_row = stored
        .iter()
        .find(|row| row.attribute == "default_agent_model")
        .expect("the insert landed");
    assert_eq!(
        private_row.inference_provenance,
        hkask_types::InferenceProvenance::PrivateInference,
        "a memory cited against private evidence inherits the private mark"
    );

    // The public direction: a fresh server/store, public evidence → an
    // unmarked row.
    let (server, memory) = make_server_with_embeddings();
    let public_id = seed_h_mem(
        &memory,
        "curator:thread:public-source",
        "chatted",
        "a public turn".to_string(),
    );
    let response = insert_citing_seed(&server, public_id).await;
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "insert citing public evidence must succeed — got: {response}"
    );
    let stored = memory
        .query_deduped_untouched("zed-kask")
        .expect("query should succeed");
    let public_row = stored
        .iter()
        .find(|row| row.attribute == "default_agent_model")
        .expect("the second insert landed");
    assert_eq!(
        public_row.inference_provenance,
        hkask_types::InferenceProvenance::PublicProvider,
        "a memory cited against public evidence stays unmarked"
    );
}

// ── M5: the recall/export filters (FR-4's read side, falsifier (b)) ───────

/// M5 mutation-surface extension (operator ruling 2026-11-07, dual-axis
/// review Q2): a public call context naming a private-marked h_mem ID is
/// refused at the provenance boundary — never an existence oracle with
/// read/write/delete power; a private context flows unchanged.
#[tokio::test]
async fn public_contexts_cannot_mutate_private_marked_rows() {
    let (server, memory) = make_server_with_embeddings();
    let private = hkask_storage::HMem::new(
        "curator:thread:m5-mutation",
        "private",
        serde_json::Value::String("a private turn".to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    let private_id = private.id;
    memory.store(private).expect("seed private row");

    // Public context (the default): update is refused typed.
    let refused = server
        .memory_update(Parameters(MemoryUpdateRequest {
            h_mem_id: private_id.to_string(),
            new_confidence: 0.9,
            new_value: None,
            reason: Some("public mutation probe".to_string()),
        }))
        .await
        .expect_err("a public context cannot update a private-marked row");
    assert!(
        refused.to_string().contains("provenance boundary"),
        "the refusal names the boundary: {refused}"
    );

    // Public context: contradiction-forget is refused too.
    let refused = server
        .memory_resolve_contradiction(Parameters(MemoryResolveContradictionRequest {
            target_h_mem_id: private_id.to_string(),
            h_mem_ids: vec![private_id.to_string()],
            strategy: "forget".to_string(),
            new_confidence: None,
            reason: "public forget probe".to_string(),
        }))
        .await
        .expect_err("a public context cannot delete a private-marked row");
    assert!(
        refused.to_string().contains("provenance boundary"),
        "the refusal names the boundary: {refused}"
    );

    // Private context (the positive control): the same update flows.
    hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.memory_update(Parameters(MemoryUpdateRequest {
            h_mem_id: private_id.to_string(),
            new_confidence: 0.9,
            new_value: None,
            reason: Some("private mutation control".to_string()),
        })),
    )
    .await
    .expect("a private context mutates its private rows");
}

/// Seed the M5 mixed-visibility fixture the private-row legs share: one
/// public row and one private-marked row on the same entity.
fn seed_m5_mixed_public_and_private(memory: &hkask_memory::MemoryStore) {
    seed_h_mem(
        memory,
        "curator:thread:m5-mixed",
        "public",
        "a public turn".to_string(),
    );
    let private = hkask_storage::HMem::new(
        "curator:thread:m5-mixed",
        "private",
        serde_json::Value::String("a private turn".to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    memory.store(private).expect("seed private row");
}

/// FR-4 falsifier (b), the tool leg's public side: a recall in a
/// NON-private call context never returns a private-marked row — and
/// the exclusion is SURFACED (`excluded_private_rows`), never silently
/// dropped (an empty leg with exclusions must read as filtered, not as
/// "no memory found"). The call context here is the default — no
/// `_meta` mark claims provenance, the fail-closed public read.
#[tokio::test]
async fn public_call_contexts_never_receive_private_rows() {
    let (server, memory) = make_server_with_embeddings();
    seed_m5_mixed_public_and_private(&memory);

    let response = invoke_tool!(
        server,
        curator_memory_recall,
        MemoryRecallRequest {
            entity: Some("curator:thread:m5-mixed".to_string()),
            recall_shape: MemoryRecallType::EntityWide,
            ontology_axis: None,
            ontology_value: None,
            limit: None,
        }
    );

    let leg = &response["entity_wide"];
    assert_eq!(
        leg["count"].as_u64(),
        Some(1),
        "a public call context sees only the public row — got: {response}",
    );
    assert_eq!(
        leg["excluded_private_rows"].as_u64(),
        Some(1),
        "the exclusion is surfaced, never silent — got: {response}",
    );
    assert!(
        leg["h_mems"]
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row["attribute"] != "private")),
        "no private-marked row leaks — got: {response}",
    );
}

/// FR-4 falsifier (b), the tool leg's private side — the positive
/// control (the gate must not over-block): a PRIVATE call context (the
/// `_meta` mark the parent sets; simulated via the test seam) recalls
/// its private rows.
#[tokio::test]
async fn private_call_contexts_receive_their_private_rows() {
    let (server, memory) = make_server_with_embeddings();
    seed_m5_mixed_public_and_private(&memory);

    let response = hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.curator_memory_recall(Parameters(MemoryRecallRequest {
            entity: Some("curator:thread:m5-mixed".to_string()),
            recall_shape: MemoryRecallType::EntityWide,
            ontology_axis: None,
            ontology_value: None,
            limit: None,
        })),
    )
    .await
    .expect("recall ok");
    let response = parse_envelope(&response);

    let leg = &response["entity_wide"];
    assert_eq!(
        leg["count"].as_u64(),
        Some(2),
        "a private call context recalls both rows — got: {response}",
    );
    assert!(
        leg.get("excluded_private_rows").is_none(),
        "nothing was excluded — got: {response}",
    );
}

/// The semantic leg filters too (falsifier (b) over the KNN path): the
/// seeded embedding is a guaranteed hit for every query, so the ONLY
/// variable is the row's mark — a public context gets zero results with
/// the exclusion surfaced; a private context gets the fragment.
#[tokio::test]
async fn semantic_search_filters_private_rows_by_call_context() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "curator:thread:m5-semantic";
    let private = hkask_storage::HMem::new(
        entity,
        "turn",
        serde_json::json!({
            "user_input": "a private question",
            "agent_response": "a private answer",
        }),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    memory.store(private).expect("seed private turn");
    seed_embedding(&memory, entity);

    // Public context: the hit is excluded and the exclusion surfaced.
    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        }
    );
    assert_eq!(
        response["count"].as_u64(),
        Some(0),
        "a public context gets no private fragment — got: {response}",
    );
    assert_eq!(
        response["excluded_private_rows"].as_u64(),
        Some(1),
        "the semantic exclusion is surfaced — got: {response}",
    );

    // Private context (the D100-follow-up re-shape, 2026-11-06): the query
    // embeds through the PRIVATE model (`test-private-model`), so the
    // seeded cloud-model embedding gates OUT by the model string — surfaced
    // as `model_mismatch_excluded`, never silently empty — while a
    // private-model embedding on a private row recalls: the KNN positive
    // control in the local-model space.
    let private_entity = "curator:thread:m5-semantic-local";
    let private_local = hkask_storage::HMem::new(
        private_entity,
        "turn",
        serde_json::json!({
            "user_input": "a private question in the local space",
            "agent_response": "a private answer in the local space",
        }),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    memory
        .store(private_local)
        .expect("seed local-space private turn");
    seed_embedding_with_model(&memory, private_entity, "test-private-model");

    let response = hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.curator_semantic_search(Parameters(SemanticSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        })),
    )
    .await
    .expect("search ok");
    let response = parse_envelope(&response);
    assert_eq!(
        response["count"].as_u64(),
        Some(1),
        "a private context recalls the private fragment in the local-model space — got: {response}",
    );
    assert_eq!(
        response["model_mismatch_excluded"].as_u64(),
        Some(1),
        "the cloud-model embedding gates out by the model string, surfaced — got: {response}",
    );
    assert!(response.get("excluded_private_rows").is_none());
}

/// D100's named follow-up (operator-approved 2026-11-06), the routing
/// falsifier: a PRIVATE call context's query embed travels to the PRIVATE
/// model only — the recording port observes zero configured-model embeds
/// and exactly one private-model embed; the same server under a PUBLIC
/// context embeds via the configured model, unchanged.
#[tokio::test]
async fn private_call_query_embeds_via_the_private_model_not_the_configured_embedder() {
    let recording = Arc::new(RecordingEmbedPort::default());
    let (server, _memory) = make_server_with_embedding_port(recording.clone());

    // Private context: one private-model embed, zero configured-model embeds.
    hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.curator_semantic_search(Parameters(SemanticSearchRequest {
            query: "a private question".to_string(),
            limit: None,
        })),
    )
    .await
    .expect("search ok");
    let models = recording.embedded_models();
    assert_eq!(
        models,
        vec!["test-private-model".to_string()],
        "the private call embeds through the private model only — got: {models:?}"
    );

    // Public context on the same server: the configured model, unchanged.
    server
        .curator_semantic_search(Parameters(SemanticSearchRequest {
            query: "a public question".to_string(),
            limit: None,
        }))
        .await
        .expect("search ok");
    let models = recording.embedded_models();
    assert_eq!(
        models,
        vec!["test-private-model".to_string(), "test-model".to_string(),],
        "the public call embeds via the configured model, unchanged — got: {models:?}"
    );
}

/// The federated head embed is a SEPARATE call site — pin the same routing
/// there: a private call context's federated query embeds through the
/// private model only (the sealed-source legs' model gates then surface
/// their per-source exclusions; this server has no sealed sources
/// configured, so the curator leg is the one embed).
#[tokio::test]
async fn private_call_federated_query_embeds_via_the_private_model() {
    let recording = Arc::new(RecordingEmbedPort::default());
    let (server, _memory) = make_server_with_embedding_port(recording.clone());

    hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.curator_federated_search(Parameters(FederatedSearchRequest {
            query: "a private federated question".to_string(),
            limit: None,
        })),
    )
    .await
    .expect("search ok");
    assert_eq!(
        recording.embedded_models(),
        vec!["test-private-model".to_string()],
        "the private federated call embeds through the private model only — got: {:?}",
        recording.embedded_models()
    );
}

/// D100 Phase 2 (operator direction 2026-11-06), the write-side routing
/// falsifier: a PRIVATE-marked row's passage embed travels to the PRIVATE
/// model only — `memory_insert` citing private-marked evidence records one
/// private-model embed and zero configured-model embeds; a subsequent
/// public insert on the same server embeds via the configured model,
/// unchanged.
#[tokio::test]
async fn private_marked_rows_embed_their_passages_via_the_private_model() {
    let recording = Arc::new(RecordingEmbedPort::default());
    let (server, memory) = make_server_with_embedding_port(recording.clone());

    // A private-marked evidence row (the shape a D100-marked thread's
    // turn-completion ingest writes).
    let private_evidence = hkask_storage::HMem::new(
        "curator:thread:private-evidence",
        "chatted",
        serde_json::Value::String("a private source turn".to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    let private_id = private_evidence.id.to_string();
    memory
        .store(private_evidence)
        .expect("seed private evidence");

    // Q2 (operator ruling 2026-11-07): citing private-marked evidence
    // belongs to a PRIVATE call context — the direct-ID provenance
    // boundary refuses it from a public one.
    let response = hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        insert_citing_seed(&server, private_id),
    )
    .await;
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "the private-evidence insert must land — got: {response}",
    );
    assert_eq!(
        recording.embedded_models(),
        vec!["test-private-model".to_string()],
        "a private-marked row's passage embeds through the private model only — got: {:?}",
        recording.embedded_models(),
    );

    // The public side, unchanged: an insert citing public evidence embeds
    // via the configured model.
    let public_seed_id = seed_h_mem(
        &memory,
        "curator:thread:public-evidence",
        "chatted",
        "a public source turn".to_string(),
    );
    let response = insert_citing_seed(&server, public_seed_id).await;
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "the public-evidence insert must land — got: {response}",
    );
    assert_eq!(
        recording.embedded_models(),
        vec!["test-private-model".to_string(), "test-model".to_string(),],
        "the public insert embeds via the configured model, unchanged — got: {:?}",
        recording.embedded_models(),
    );
}

/// The no-field rule (P4), pinned at the schema surface: no tool
/// request exposes a provenance field — the mark is parent-set
/// (`_meta`), and a model-facing surface can never claim it.
#[test]
fn no_tool_request_exposes_a_provenance_field() {
    use schemars::JsonSchema;

    fn assert_no_provenance_property<T: JsonSchema>(name: &str) {
        let schema = schemars::schema_for!(T);
        let json = serde_json::to_value(&schema).expect("the schema serializes");
        let properties: Vec<String> = json
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|map| map.keys().cloned().collect())
            .unwrap_or_default();
        assert!(
            !properties.iter().any(|key| key.contains("provenance")),
            "the {name} request schema must expose no provenance field — \
             the mark is parent-set, never caller-claimed (got: {properties:?})"
        );
    }

    assert_no_provenance_property::<MemoryRecallRequest>("MemoryRecallRequest");
    assert_no_provenance_property::<SemanticSearchRequest>("SemanticSearchRequest");
    assert_no_provenance_property::<FederatedSearchRequest>("FederatedSearchRequest");
    assert_no_provenance_property::<CuratorConsultRequest>("CuratorConsultRequest");
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
    let seed_id = seed_h_mem(&memory, entity, "chatted", "the source turn".to_string());
    seed_embedding(&memory, entity);

    // Recall via the tool — the cited ID must come from the tool output,
    // not from the store.
    let recall = invoke_tool!(
        server,
        curator_memory_recall,
        MemoryRecallRequest {
            entity: Some(entity.to_string()),
            recall_shape: MemoryRecallType::EntityWide,
            ontology_axis: None,
            ontology_value: None,
            limit: None,
        }
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
    let consult = invoke_tool!(
        server,
        curator_consult,
        CuratorConsultRequest {
            query: "any question words".to_string(),
            limit: None,
        }
    );
    assert!(
        consult["entity_wide_fragments"]["h_mems"][0]["id"]
            .as_str()
            .is_some_and(|id| id == seed_id),
        "consult must surface the h_mem id — got: {consult}",
    );
    let search = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        }
    );
    assert!(
        search["results"][0]["id"]
            .as_str()
            .is_some_and(|id| id == seed_id),
        "semantic search must surface the h_mem id — got: {search}",
    );

    // The round-trip: cite the tool-surfaced ID as insert evidence.
    let response = invoke_tool!(
        server,
        memory_insert,
        MemoryInsertRequest {
            entity: "zed-kask".to_string(),
            attribute: "citation_round_trip".to_string(),
            value: serde_json::json!("pinned").into(),
            evidence_h_mem_id: recalled_id,
            note: None,
        }
    );
    assert_eq!(
        response["inserted"].as_bool(),
        Some(true),
        "an ID obtained from a tool output must be accepted as evidence — got: {response}",
    );
}

/// Evidence citations that name no existing h_mem must be rejected as
/// `not_found`, while a malformed ID that cannot parse as a UUID is rejected
/// as `invalid_argument`.
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
        matches!(error.kind, hkask_types::McpErrorKind::NotFound),
        "a missing citation is not_found, not internal — got: {error:?}",
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

// ── Insert-path provenance tier + write-path contradiction check (R5) ──

/// The extraction ceiling, enforced at the tool: the model's insert is
/// `model_inference` — the request schema offers no tier field to claim —
/// and the tier surfaces in the insert receipt and in recall, so a
/// model-synthesized memory never retrieves as tool-observed.
#[tokio::test]
async fn memory_insert_stamps_model_inference_and_recall_surfaces_the_tier() {
    let (server, memory) = make_server_with_embeddings();
    let seed_id = seed_h_mem(&memory, "evidence", "fact", "seed".to_string());
    let inserted = insert_citing_seed(&server, seed_id).await;
    assert_eq!(
        inserted["provenance"].as_str(),
        Some("model_inference"),
        "the insert receipt carries the ceiling stamp — got: {inserted}"
    );

    let recalled = invoke_tool!(
        server,
        curator_memory_recall,
        MemoryRecallRequest {
            entity: Some("zed-kask".to_string()),
            recall_shape: MemoryRecallType::EntityWide,
            ontology_axis: None,
            ontology_value: None,
            limit: None,
        }
    );
    let h_mems = recalled["entity_wide"]["h_mems"]
        .as_array()
        .expect("entity_wide h_mems");
    assert!(!h_mems.is_empty(), "the inserted memory is recalled");
    assert!(
        h_mems
            .iter()
            .all(|h| h["provenance"].as_str() == Some("model_inference")),
        "recall surfaces the tier on every h_mem — got: {recalled}"
    );
}

/// A colliding insert surfaces the contradiction at the write path — never
/// silently coexisting: the refusal names the existing h_mem by id and
/// routes to the resolution paths. `memory_update` (revision by id) and the
/// machine ingestion paths are unaffected — the gate is the deliberate
/// model-facing knowledge write only.
#[tokio::test]
async fn memory_insert_collision_surfaces_the_contradiction_and_routes_to_resolution() {
    let (server, memory) = make_server_with_embeddings();
    let seed_id = seed_h_mem(
        &memory,
        "company:acme",
        "revenue_trend",
        "declining".to_string(),
    );

    let error = server
        .memory_insert(Parameters(MemoryInsertRequest {
            entity: "company:acme".to_string(),
            attribute: "revenue_trend".to_string(),
            value: serde_json::json!("growing").into(),
            evidence_h_mem_id: seed_id.clone(),
            note: None,
        }))
        .await
        .expect_err("a differing value for the same key must be refused");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::FailedPrecondition),
        "a contradiction is a failed precondition, not an argument or internal error — got: {error:?}"
    );
    assert!(
        error.message.contains("contradiction"),
        "the refusal names the contradiction — got: {error:?}"
    );
    assert!(
        error.message.contains(&seed_id),
        "the refusal names the existing h_mem by id — got: {error:?}"
    );
    assert!(
        error.message.contains("memory_resolve_contradiction"),
        "the refusal routes to the resolution path — got: {error:?}"
    );
}

/// A same-value insert is a re-assertion, not a contradiction — it proceeds.
/// The payload comparison strips the derived keys (`recall_text`, `_note`),
/// so a re-assertion whose stored form gained a recall passage still matches.
#[tokio::test]
async fn memory_insert_same_value_reassertion_is_not_a_contradiction() {
    let (server, memory) = make_server_with_embeddings();
    let seed_id = seed_h_mem(&memory, "company:beta", "sector", "software".to_string());

    let ok = server
        .memory_insert(Parameters(MemoryInsertRequest {
            entity: "company:beta".to_string(),
            attribute: "sector".to_string(),
            value: serde_json::json!("software").into(),
            evidence_h_mem_id: seed_id,
            note: None,
        }))
        .await
        .expect("a same-value re-assertion proceeds");
    assert_eq!(
        parse_envelope(&ok)["provenance"].as_str(),
        Some("model_inference"),
        "the re-assertion carries the ceiling stamp"
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
    let seed_id = seed_h_mem(
        &memory,
        "curator:thread:evidence-source",
        "chatted",
        "the source turn".to_string(),
    );

    let response = insert_citing_seed(&server, seed_id).await;
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
    let search = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "what model does the agent use by default".to_string(),
            limit: None,
        }
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

    let response = invoke_tool!(
        server,
        curator_report_skill_use_issue,
        ReportSkillUseIssueRequest {
            skill_name: "grounding-verify".to_string(),
            tool_name: "lisp_eval".to_string(),
            step_ordinal: 2,
            error: "closed-vocabulary validation form errored".to_string(),
            tool_input: None,
            failure_type: None,
            failure_origin: SkillUseFailureOrigin::AgentExecution,
        }
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

/// The insert paths' embedding contract (therapy 2026-10-08 F002, the
/// goal-event precedent): with no embedding store and a failing inference
/// port, the insert FAILS and nothing lands — no knowledge row can exist
/// invisible to `curator_semantic_search`. The prior contract (insert
/// succeeds, degradation stamped in the output) produced the 27-row
/// embedding-invisible class measured 2026-10-08.
#[tokio::test]
async fn insert_path_embedding_failure_fails_the_insert_and_lands_nothing() {
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
        memory: Some(memory.clone()),
    };
    let server = CuratorServer::new(
        WebID::new(),
        Arc::new(CuratorDb::from_stores(stores)),
        failing_inference_port(),
    );

    let error = server
        .memory_insert(Parameters(MemoryInsertRequest {
            entity: "zed-kask".to_string(),
            attribute: "mcp_tool_surface".to_string(),
            value: serde_json::json!("full surface, no router").into(),
            evidence_h_mem_id: seed_id,
            note: None,
        }))
        .await
        .expect_err("embedding failure must fail the insert");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::Unavailable),
        "a provider that did not answer is transient (retry), classified by what \
         failed — got: {error:?}",
    );
    assert!(
        error.message.contains("NOT stored"),
        "the error must say nothing landed — got: {error:?}",
    );
    let landed = memory
        .h_mems_by_entity_prefix("zed-kask")
        .expect("query store")
        .into_iter()
        .filter(|h_mem| h_mem.entity == "zed-kask")
        .count();
    assert_eq!(
        landed, 0,
        "no h_mem may land without its embedding — got: {landed}"
    );

    let error = server
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
        .expect_err("embedding failure must fail the skill-use report");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::Unavailable),
        "a provider that did not answer is transient — got: {error:?}",
    );
    let reported = memory
        .h_mems_by_entity_prefix("skill_use_issue:therapy")
        .expect("query store")
        .into_iter()
        .filter(|h_mem| h_mem.entity == "skill_use_issue:therapy")
        .count();
    assert_eq!(
        reported, 0,
        "no report row may land without its embedding — got: {reported}"
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
        "zed-kask-duplicate-ruling",
        "operator_ruling",
        serde_json::Value::String("ruling A".to_string()),
        WebID::new(),
    );
    let forget_id = forget_target.id.to_string();
    memory.store(forget_target).expect("seed forget target");

    let response = invoke_tool!(
        server,
        memory_resolve_contradiction,
        MemoryResolveContradictionRequest {
            h_mem_ids: vec![forget_id.clone()],
            strategy: "forget".to_string(),
            target_h_mem_id: forget_id,
            new_confidence: None,
            reason: "duplicate ruling merge".to_string(),
        }
    );
    assert_eq!(
        response["resolved"].as_bool(),
        Some(true),
        "forget must resolve — got: {response}",
    );
    assert!(
        memory
            .h_mems_by_entity_prefix("zed-kask-duplicate-ruling")
            .expect("query after forget")
            .is_empty(),
        "the forgotten target must be deleted from the database"
    );

    let confidence_target = hkask_storage::HMem::new(
        "zed-kask-confidence-target",
        "policy",
        serde_json::Value::String("policy text".to_string()),
        WebID::new(),
    );
    let confidence_id = confidence_target.id.to_string();
    memory
        .store(confidence_target)
        .expect("seed confidence target");

    let response = invoke_tool!(
        server,
        memory_resolve_contradiction,
        MemoryResolveContradictionRequest {
            h_mem_ids: vec![confidence_id.clone()],
            strategy: "update_confidence".to_string(),
            target_h_mem_id: confidence_id,
            new_confidence: Some(0.5),
            reason: "floor reset".to_string(),
        }
    );
    assert_eq!(
        response["resolved"].as_bool(),
        Some(true),
        "update_confidence must resolve — got: {response}",
    );
    let updated = memory
        .h_mems_by_entity_prefix("zed-kask-confidence-target")
        .expect("query after update");
    assert_eq!(updated.len(), 1);
    let returned = response["target_h_mem_id"]
        .as_str()
        .expect("replacement handle");
    assert_eq!(
        memory
            .get_by_id(&returned.parse().expect("replacement id"))
            .expect("resolve replacement")
            .expect("row exists")
            .id,
        updated[0].id
    );
    assert!(
        memory
            .has_embedding_for_passage("zed-kask-confidence-target", "policy text")
            .expect("confidence revision remains recallable")
    );
    assert!(
        (updated[0].confidence.value() - 0.5).abs() < 1e-9,
        "update_confidence must set the value — got {}",
        updated[0].confidence.value(),
    );
}

/// Legacy knowledge rows gain a passage through the same write contract;
/// dry-run identifies the repair without changing the row, and retry is idempotent.
#[tokio::test]
async fn backfill_repairs_legacy_rows_without_canonical_passage() {
    let (server, memory) = make_server_with_embeddings();
    let opaque = hkask_storage::HMem::new(
        "opaque-entity",
        "structured_only",
        serde_json::json!({"count": 3}),
        WebID::new(),
    );
    let id = opaque.id.to_string();
    memory.store(opaque).expect("seed opaque row");

    let dry = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(true),
        }
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(dry["candidates"][0]["h_mem_id"].as_str(), Some(id.as_str()));
    assert_eq!(dry["candidates"][0]["requires_passage_repair"], true);
    assert_eq!(memory.embedding_count().expect("dry count"), 0);
    assert!(
        memory
            .get_by_id(&id.parse().expect("old id"))
            .expect("dry read")
            .is_some()
    );
    let run = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(false),
        }
    );
    assert_eq!(run["backfilled"], 1);
    assert_eq!(run["results"][0]["passage_repaired"], true);
    let written_id = run["results"][0]["h_mem_id"]
        .as_str()
        .expect("replacement id");
    let repaired = memory
        .get_by_id(&written_id.parse().expect("new id"))
        .expect("read repair")
        .expect("repaired row");
    assert_eq!(repaired.value["count"], 3);
    let passage = hkask_memory::semantic_passage_for_h_mem(&repaired).expect("derived passage");
    assert!(
        memory
            .has_embedding_for_passage("opaque-entity", &passage)
            .expect("repair coverage")
    );
    let repeat = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(true),
        }
    );
    assert_eq!(repeat["candidate_count"], 0);
}

/// `curator_memory_backfill_embeddings` must embed knowledge-layer h_mems
/// whose entities have no embedding, while excluding turn storage
/// (`curator:thread:`) and distillation watermarks. Also pins dry-run (embeds nothing) and idempotence (a second
/// run finds no candidates).
#[tokio::test]
async fn backfill_embeddings_covers_knowledge_layer_and_excludes_turns() {
    let (server, memory) = make_server_with_embeddings();

    let ruling = hkask_storage::HMem::new(
        "zed-kask-provider_budget_blocks",
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
    let dry = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(true),
        }
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
    let run = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(false),
        }
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
    let second = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: None,
        }
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
        "current",
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

    let dry = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(true),
        }
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(
        dry["candidates"][0]["attribute"].as_str(),
        Some("second"),
        "the missing sibling passage is selected even though its entity has another vector"
    );

    let run = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: None,
            dry_run: Some(false),
        }
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
    let dry = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: Some("model_mismatch".to_string()),
            dry_run: Some(true),
        }
    );
    assert_eq!(dry["candidate_count"].as_u64(), Some(1));
    assert_eq!(dry["candidates"][0]["entity"].as_str(), Some("sweep/stale"));

    // The sweep re-embeds it under the current model.
    let swept = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: Some("model_mismatch".to_string()),
            dry_run: Some(false),
        }
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

    let dry = invoke_tool!(
        server,
        curator_memory_backfill_embeddings,
        BackfillEmbeddingsRequest {
            mode: Some("model_mismatch".to_string()),
            dry_run: Some(true),
        }
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

/// Unknown mode values are rejected as invalid_argument — the
/// closed vocabulary never silently defaults a typo.
#[tokio::test]
async fn backfill_rejects_unknown_mode() {
    let (server, _memory) = make_server_with_embeddings();

    let mode_error = server
        .curator_memory_backfill_embeddings(Parameters(BackfillEmbeddingsRequest {
            mode: Some("sideways".to_string()),
            dry_run: Some(true),
        }))
        .await
        .expect_err("unknown mode must be rejected");
    assert!(
        matches!(mode_error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "unknown mode is invalid_argument — got: {mode_error:?}"
    );
}

/// `curator_memory_prune` must default to turn-storage scope: aged turn
/// rows are hard-deleted while aged knowledge-layer rows survive.
/// `all_layers=true` is the explicit full-store opt-in. (therapy
/// 2026-09-04, P3 — the valve must fail closed on knowledge rows.)
#[tokio::test]
async fn curator_memory_prune_defaults_to_turn_storage_scope() {
    let (server, memory) = make_server_with_embeddings();

    seed_aged_h_mem(
        &memory,
        "curator:thread:prune-tool-test",
        "turn",
        "aged turn".to_string(),
        100,
    );

    seed_aged_h_mem(
        &memory,
        "zed-kask-prune-scope-test-ruling",
        "operator_ruling",
        "durable ruling".to_string(),
        100,
    );

    // Default scope: turn storage only — the ruling survives.
    let scoped = invoke_tool!(
        server,
        curator_memory_prune,
        MemoryPruneRequest {
            max_age_days: 50,
            spare_recalled_within_days: None,
            all_layers: None,
            prefixes: None,
        }
    );
    assert_eq!(scoped["all_layers"].as_bool(), Some(false));
    assert_eq!(scoped["deleted_count"].as_u64(), Some(1));
    assert_eq!(
        memory
            .h_mems_by_entity_prefix("zed-kask-prune-scope-test-ruling")
            .expect("query ruling")
            .len(),
        1,
        "knowledge row must survive the default scope — got: {scoped}",
    );

    // Explicit opt-in: full store — the ruling is now deleted.
    let full = invoke_tool!(
        server,
        curator_memory_prune,
        MemoryPruneRequest {
            max_age_days: 50,
            spare_recalled_within_days: None,
            all_layers: Some(true),
            prefixes: None,
        }
    );
    assert_eq!(full["all_layers"].as_bool(), Some(true));
    assert_eq!(full["deleted_count"].as_u64(), Some(1));
    assert!(
        memory
            .h_mems_by_entity_prefix("zed-kask-prune-scope-test-ruling")
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

    seed_aged_h_mem(
        &memory,
        "skill_use_issue:prune-scope-test",
        "tool_failure:terminal",
        "aged incident".to_string(),
        100,
    );

    seed_aged_h_mem(
        &memory,
        "curator:thread:prune-prefix-test",
        "turn",
        "aged turn".to_string(),
        100,
    );

    seed_aged_h_mem(
        &memory,
        "zed-kask-prune-prefix-test-ruling",
        "operator_ruling",
        "durable ruling".to_string(),
        100,
    );

    // Scoped valve: only the named prefix is pruned; the response names
    // the scope used.
    let scoped = invoke_tool!(
        server,
        curator_memory_prune,
        MemoryPruneRequest {
            max_age_days: 50,
            spare_recalled_within_days: None,
            all_layers: None,
            prefixes: Some(vec!["skill_use_issue:".to_string()]),
        }
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
            .h_mems_by_entity_prefix("zed-kask-prune-prefix-test-ruling")
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
        "zed-kask-update-target",
        "policy",
        serde_json::Value::String("old value".to_string()),
        WebID::new(),
    );
    let seed_id = seed.id.to_string();
    memory.store(seed).expect("seed update target");

    let response = invoke_tool!(
        server,
        memory_update,
        MemoryUpdateRequest {
            h_mem_id: seed_id,
            new_confidence: 0.6,
            new_value: Some(serde_json::Value::String("new value".to_string()).into()),
            reason: Some("test update".to_string()),
        }
    );
    assert_eq!(
        response["updated"].as_bool(),
        Some(true),
        "update must resolve — got: {response}",
    );

    let updated = memory
        .h_mems_by_entity_prefix("zed-kask-update-target")
        .expect("query after update");
    assert_eq!(updated.len(), 1);
    let returned_id = response["h_mem_id"].as_str().expect("written handle");
    assert_eq!(
        memory
            .get_by_id(&returned_id.parse().expect("valid handle"))
            .expect("resolve returned handle")
            .expect("written row")
            .id,
        updated[0].id
    );
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

#[tokio::test]
async fn memory_update_reports_its_embedding_call_in_usage() {
    let (server, memory) = make_server_with_embeddings();
    let id = seed_h_mem(&memory, "usage:revision", "finding", "original".into());
    let output = server
        .memory_update(Parameters(MemoryUpdateRequest {
            h_mem_id: id,
            new_confidence: 0.6,
            new_value: Some(serde_json::json!({"resolution": "updated finding"}).into()),
            reason: None,
        }))
        .await
        .expect("revision succeeds");
    let envelope: serde_json::Value = serde_json::from_str(&output).expect("tool envelope");
    assert_eq!(
        envelope["usage"]["calls"], 1,
        "embedding inference must not read as a deterministic no-usage tool"
    );
    assert_eq!(
        envelope["usage"]["reported"], false,
        "the stub does not claim provider-reported tokens"
    );
}

#[tokio::test]
async fn memory_update_derives_and_embeds_revised_object() {
    let (server, memory) = make_server_with_embeddings();
    let id = seed_h_mem(&memory, "revision-target", "policy", "old passage".into());
    memory
        .store_embedding(
            "revision-target",
            &unit_vector(),
            "test-model",
            Some("old passage"),
        )
        .expect("seed old embedding");

    server
        .memory_update(Parameters(MemoryUpdateRequest {
            h_mem_id: id.clone(),
            new_confidence: 0.6,
            new_value: Some(
                serde_json::json!({"resolution": "revised telescope calibration"}).into(),
            ),
            reason: None,
        }))
        .await
        .expect("revise object without recall_text");
    let rows = memory
        .query_deduped_untouched("revision-target")
        .expect("read revision");
    assert_eq!(rows.len(), 1);
    assert_ne!(
        rows[0].id.to_string(),
        id,
        "revision still rotates row identity"
    );
    let passage = hkask_memory::semantic_passage_for_h_mem(&rows[0])
        .expect("revision must derive its canonical passage");
    assert!(passage.contains("revised telescope calibration"));
    assert!(
        memory
            .has_embedding_for_passage("revision-target", &passage)
            .expect("coverage")
    );
    assert!(
        !memory
            .has_embedding_for_passage("revision-target", "old passage")
            .expect("old coverage")
    );
    let search = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "optical instrument alignment".into(),
            limit: None,
        }
    );
    assert!(
        search["results"]
            .as_array()
            .expect("results")
            .iter()
            .any(|hit| { hit["entity"] == "revision-target" }),
        "revised object must join back from KNN: {search}"
    );
}

#[tokio::test]
async fn memory_update_embedding_failure_preserves_previous_memory() {
    let (server, memory) = make_server_with_embedding_port(Arc::new(FailingEmbedPort));
    let id = seed_h_mem(&memory, "revision-target", "policy", "old passage".into());
    memory
        .store_embedding(
            "revision-target",
            &unit_vector(),
            "test-model",
            Some("old passage"),
        )
        .expect("seed old embedding");
    let result = server
        .memory_update(Parameters(MemoryUpdateRequest {
            h_mem_id: id.clone(),
            new_confidence: 0.6,
            new_value: Some(serde_json::json!({"resolution": "new passage"}).into()),
            reason: None,
        }))
        .await;
    assert!(
        result.is_err(),
        "embedding failure must refuse the revision"
    );
    let old = memory
        .get_by_id(&id.parse().expect("id"))
        .expect("read prior row")
        .expect("failed revision preserves prior row");
    assert_eq!(old.value, "old passage");
    assert!(
        memory
            .has_embedding_for_passage("revision-target", "old passage")
            .expect("prior embedding")
    );
}

// Memory payloads use the shared JSON-text wire contract, not open schemas.
#[test]
fn memory_insert_request_value_schema_requires_json_text() {
    let schema = serde_json::to_value(schemars::schema_for!(MemoryInsertRequest))
        .expect("schema serializes");
    let value = &schema["properties"]["value"];
    assert!(
        !value.is_boolean(),
        "value must not render as the bare boolean true (Schema::Bool(true)) — got: {value}",
    );
    assert_eq!(value["type"], "string");
    assert!(value.get("properties").is_none());
    assert!(value.get("additionalProperties").is_none());
    assert!(
        hkask_types::find_boolean_schema_positions(&schema).is_empty(),
        "MemoryInsertRequest must carry no bare-boolean schema positions",
    );
}

#[test]
fn memory_update_request_new_value_schema_is_optional_json_text() {
    let schema = serde_json::to_value(schemars::schema_for!(MemoryUpdateRequest))
        .expect("schema serializes");
    let new_value = &schema["properties"]["new_value"];
    assert!(
        !new_value.is_boolean(),
        "new_value must not render as the bare boolean true (Schema::Bool(true)) — got: {new_value}",
    );
    assert_eq!(new_value["type"], serde_json::json!(["string", "null"]));
    assert!(new_value.get("properties").is_none());
    assert!(new_value.get("additionalProperties").is_none());
    assert!(
        hkask_types::find_boolean_schema_positions(&schema).is_empty(),
        "MemoryUpdateRequest must carry no bare-boolean schema positions",
    );
}

#[tokio::test]
async fn semantic_search_resolves_the_named_passage_not_newer_siblings() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "semantic:shared-entity";
    let passage = "the older optical calibration decision";
    let mut target = hkask_storage::HMem::new(
        entity,
        "target",
        serde_json::json!({"recall_text": passage}),
        WebID::new(),
    );
    target.observed_at = chrono::Utc::now() - chrono::Duration::days(30);
    let target_id = target.id.to_string();
    memory.store(target).expect("older named passage");
    for index in 0..4 {
        memory
            .store(hkask_storage::HMem::new(
                entity,
                &format!("newer:{index}"),
                serde_json::json!("unrelated newer sibling"),
                WebID::new(),
            ))
            .expect("newer sibling without a matching passage vector");
    }
    memory
        .store_embedding(entity, &unit_vector(), "test-model", Some(passage))
        .expect("only target vector");
    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "query with no lexical overlap".into(),
            limit: Some(1),
        }
    );
    assert_eq!(
        response["results"][0]["id"], target_id,
        "a named vector must join its passage, not the entity newest rows: {response}"
    );
}

#[tokio::test]
async fn semantic_search_surfaces_a_named_passage_orphan_without_substituting_a_sibling() {
    let (server, memory) = make_server_with_embeddings();
    let entity = "semantic:shared-entity";
    seed_h_mem(
        &memory,
        entity,
        "survivor",
        "unrelated surviving memory".into(),
    );
    memory
        .store_embedding(
            entity,
            &unit_vector(),
            "test-model",
            Some("deleted passage"),
        )
        .expect("orphaned passage vector under a surviving entity");
    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "any semantic query".into(),
            limit: Some(5),
        }
    );
    assert_eq!(
        response["count"], 0,
        "unrelated rows cannot inherit the named vector distance"
    );
    assert!(
        response["note"]
            .as_str()
            .is_some_and(|note| !note.is_empty()),
        "a named-passage resolution failure must be surfaced: {response}"
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

    let response = invoke_tool!(
        server,
        curator_semantic_search,
        SemanticSearchRequest {
            query: "anything at all".to_string(),
            limit: Some(10),
        }
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

    let skill_response = invoke_tool!(
        server,
        reg_query,
        RegQueryRequest {
            namespace: Some("reg.skill".to_string()),
            window_seconds: Some(3600),
            limit: Some(10),
        }
    );
    assert_eq!(skill_response["count"].as_u64(), Some(2));
    assert!(
        skill_response["events"]
            .as_array()
            .expect("events array")
            .iter()
            .all(|event| event["phase"] == "sense"),
        "sense-phase skill feedback must be visible: {skill_response}",
    );

    let exact_response = invoke_tool!(
        server,
        reg_query,
        RegQueryRequest {
            namespace: Some("reg.skill.program-manager".to_string()),
            window_seconds: Some(3600),
            limit: Some(1),
        }
    );
    assert_eq!(exact_response["count"].as_u64(), Some(1));
    assert_eq!(
        exact_response["events"][0]["path"].as_str(),
        Some("reg.skill.program-manager.operator_feedback"),
        "the namespace predicate must run before LIMIT and respect dot boundaries: {exact_response}",
    );

    let all_response = invoke_tool!(
        server,
        reg_query,
        RegQueryRequest {
            namespace: None,
            window_seconds: Some(3600),
            limit: Some(10),
        }
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
    assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
    assert!(
        error
            .message
            .contains("Regulation archive is not available"),
        "got: {}",
        error.message
    );
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

    let response = invoke_tool!(
        server,
        curator_algedonic_log,
        AlgedonicLogRequest { hours: Some(1) }
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
    let response = invoke_tool!(
        server,
        curator_algedonic_log,
        AlgedonicLogRequest { hours: Some(24) }
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

fn federated_source_fixture(
    directory: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(hkask_memory::test_support::sealed_federated_fixture(
        directory,
        "fixture-corpus",
        "Fixture corpus",
        "evidence.txt",
        "external corpus evidence",
        "test-model",
        "test-model",
    )?)
}

/// Build a federated-search `CuratorServer` over a fresh in-memory memory
/// store seeded with one local lesson (`entity`, `text`) embedded to the
/// federated fixture vector — the shared construction of the three
/// federated-search tests. `regulation` toggles the regulation archive
/// (the interleave test carries one; the outage/reload tests do not);
/// `manifest_path` is the sealed fixture manifest (clone it when the test
/// removes or restores the manifest file after construction). The per-test
/// pre-server work (the corpus-file before-read, the manifest tampering,
/// the original-manifest capture) stays at each site.
fn federated_search_server(
    manifest_path: std::path::PathBuf,
    regulation: bool,
    entity: &str,
    text: &str,
) -> Result<CuratorServer, Box<dyn std::error::Error>> {
    let driver = SqliteDriver::in_memory_driver();
    let h_mem_store = HMemStore::from_driver(driver.clone())?;
    let embedding_store = EmbeddingStore::from_driver(driver.clone(), test_dim())?;
    let memory = Arc::new(hkask_memory::MemoryStore::new(h_mem_store, embedding_store));
    let local = hkask_storage::HMem::new(entity, "lesson", serde_json::json!(text), WebID::new());
    memory.store(local.clone())?;
    memory.store_embedding(
        &local.entity,
        &federated_fixture_vector(),
        "test-model",
        Some(text),
    )?;
    let regulation_store = if regulation {
        Some(Arc::new(RegulationArchive::from_driver(driver)?))
    } else {
        None
    };
    let stores = CuratorStores {
        regulation_store,
        memory: Some(memory),
    };
    let database = Arc::new(CuratorDb::from_stores_with_federated_manifest(
        stores,
        manifest_path,
        "test-passphrase".to_string(),
    ));
    Ok(CuratorServer::new(
        WebID::new(),
        database,
        Arc::new(ConstantEmbedPort) as Arc<dyn hkask_types::InferencePort>,
    ))
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

    let server = federated_search_server(
        manifest_path,
        true,
        "curator:decision:fixture",
        "curator experience",
    )?;

    let response = parse_envelope(
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

    let server = federated_search_server(
        manifest_path,
        false,
        "curator:partial-outage",
        "local evidence",
    )?;
    let output = server
        .curator_federated_search(Parameters(FederatedSearchRequest {
            query: "compare local evidence and external corpus evidence".to_string(),
            limit: Some(4),
        }))
        .await?;
    let response = parse_envelope(&output);
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
    let server = federated_search_server(
        manifest_path.clone(),
        false,
        "curator:decision:manifest-refresh",
        "local knowledge survives corpus outage",
    )?;
    let search = || async {
        let output = server
            .curator_federated_search(Parameters(FederatedSearchRequest {
                query: "compare local knowledge and external corpus evidence".to_string(),
                limit: Some(4),
            }))
            .await?;
        Ok::<_, hkask_mcp_server::server::McpToolError>(parse_envelope(&output))
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

    let response = invoke_tool!(
        server,
        curator_federated_search,
        FederatedSearchRequest {
            query: "retrieve available experience with source status".to_string(),
            limit: Some(4),
        }
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

    let response = invoke_tool!(
        server,
        curator_federated_search,
        FederatedSearchRequest {
            query: "any question words".to_string(),
            limit: None,
        }
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

// ── mcp-tool-review 2026-11-12 pins ─────────────────────────────────────────
//
// Each pin crosses the wire: the request is decoded from the JSON a caller
// sends (the same serde path `Parameters<T>` uses), then handed to the tool.

/// Decode tool arguments the way the server does.
fn wire<T: serde::de::DeserializeOwned>(arguments: serde_json::Value) -> T {
    serde_json::from_value(arguments).expect("arguments decode like a tool call")
}

/// A server whose database exists but does not open: created under one
/// passphrase, opened under another. The temp directory is returned so it
/// outlives the server.
fn server_with_unopenable_database() -> (CuratorServer, tempfile::TempDir) {
    ensure_embedding_model_env();
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory
        .path()
        .join("curator.db")
        .to_string_lossy()
        .into_owned();
    drop(hkask_storage::open_or_repair(&path, "the-right-passphrase").expect("create db"));
    let database = CuratorDb::open_at(
        path,
        Some("a-different-passphrase".to_string()),
        std::path::PathBuf::new(),
        90.0,
    );
    (
        CuratorServer::new(WebID::new(), Arc::new(database), failing_inference_port()),
        directory,
    )
}

/// An inference port whose embedder is not configured (no provider prefix,
/// no credential) — a configuration failure, distinct from an outage.
struct NotConfiguredEmbedPort;

impl hkask_types::InferencePort for NotConfiguredEmbedPort {
    stub_generate_arm!();

    fn embed_with_dimensions<'a>(
        &'a self,
        _model: &str,
        _texts: &[String],
        _dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        Box::pin(async {
            Err(hkask_types::EmbeddingGenerationError::NotConfigured(
                "ZZ_API_KEY not set".to_string(),
            ))
        })
    }
}

fn seed_private_h_mem(memory: &hkask_memory::MemoryStore, entity: &str, text: &str) -> String {
    let h_mem = hkask_storage::HMem::new(
        entity,
        "chunk:1:0",
        serde_json::Value::String(text.to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    let id = h_mem.id.to_string();
    memory.store(h_mem).expect("seed private h_mem");
    id
}

/// CU-01: a lookback amount no calendar offset can hold is refused as
/// invalid_argument. Before, the handler panicked and the request got no
/// response at all (three tools), or the window wrapped negative and an
/// empty success came back (`reg_query`, u64 max).
#[tokio::test]
async fn oversized_lookback_windows_are_refused_not_panicked() {
    let (server, memory) = make_server_with_embeddings();
    seed_aged_h_mem(&memory, "curator:thread:keep", "chunk:1:0", "x".into(), 30);

    let error = server
        .curator_algedonic_log(Parameters(wire(
            serde_json::json!({"hours": 4_000_000_000u64}),
        )))
        .await
        .expect_err("hours beyond the calendar");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
    assert!(error.message.contains("hours"), "got: {}", error.message);

    for window in [u64::MAX, 9_300_000_000_000_000u64] {
        let error = server
            .reg_query(Parameters(wire(
                serde_json::json!({"window_seconds": window}),
            )))
            .await
            .expect_err("window beyond the calendar");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        assert!(
            error.message.contains("window_seconds"),
            "got: {}",
            error.message
        );
    }

    for arguments in [
        serde_json::json!({"max_age_days": 9_000_000_000_000_000_000i64}),
        serde_json::json!({"max_age_days": 1, "spare_recalled_within_days": 9_000_000_000_000_000_000i64}),
        serde_json::json!({"max_age_days": 1, "spare_recalled_within_days": 0}),
    ] {
        let error = server
            .curator_memory_prune(Parameters(wire(arguments.clone())))
            .await
            .expect_err("prune amount out of range");
        assert_eq!(
            error.kind,
            hkask_types::McpErrorKind::InvalidArgument,
            "{arguments}"
        );
    }
    assert_eq!(
        memory
            .h_mems_by_entity("curator:thread:keep")
            .expect("query")
            .len(),
        1,
        "a refused prune deletes nothing"
    );
}

/// CU-02: an argument the tool does not declare is refused. Before, a
/// misspelled scope on `curator_memory_prune` (`prefix` for `prefixes`) was
/// dropped and the prune ran on the default scope.
#[test]
fn undeclared_arguments_are_refused_at_decode() {
    let error = serde_json::from_value::<MemoryPruneRequest>(
        serde_json::json!({"max_age_days": 1, "prefix": ["skill_use_issue:"]}),
    )
    .expect_err("a misspelled scope must not decode");
    assert!(
        error.to_string().contains("unknown field `prefix`"),
        "{error}"
    );

    fn refuses_unknown<T: serde::de::DeserializeOwned + std::fmt::Debug>(
        name: &str,
        mut valid: serde_json::Value,
    ) {
        serde_json::from_value::<T>(valid.clone())
            .unwrap_or_else(|error| panic!("{name}: the valid form must decode: {error}"));
        valid["zz_undeclared"] = serde_json::json!(1);
        let error = serde_json::from_value::<T>(valid).expect_err(name);
        assert!(
            error.to_string().contains("unknown field"),
            "{name}: {error}"
        );
    }
    let id = "33333333-3333-4333-8333-333333333333";
    refuses_unknown::<PingRequest>("PingRequest", serde_json::json!({}));
    refuses_unknown::<SemanticSearchRequest>(
        "SemanticSearchRequest",
        serde_json::json!({"query": "q"}),
    );
    refuses_unknown::<FederatedSearchRequest>(
        "FederatedSearchRequest",
        serde_json::json!({"query": "q"}),
    );
    refuses_unknown::<MemoryRecallRequest>(
        "MemoryRecallRequest",
        serde_json::json!({"entity": "e"}),
    );
    refuses_unknown::<CuratorConsultRequest>(
        "CuratorConsultRequest",
        serde_json::json!({"query": "q"}),
    );
    refuses_unknown::<AlgedonicLogRequest>("AlgedonicLogRequest", serde_json::json!({}));
    refuses_unknown::<RegQueryRequest>(
        "RegQueryRequest",
        serde_json::json!({"window_seconds": 60}),
    );
    refuses_unknown::<ReportSkillUseIssueRequest>(
        "ReportSkillUseIssueRequest",
        serde_json::json!({"skill_name": "s", "tool_name": "t", "step_ordinal": 1, "error": "e", "failure_origin": "unknown"}),
    );
    refuses_unknown::<MemoryInsertRequest>(
        "MemoryInsertRequest",
        serde_json::json!({"entity": "e", "attribute": "a", "value": "\"v\"", "evidence_h_mem_id": id}),
    );
    refuses_unknown::<MemoryUpdateRequest>(
        "MemoryUpdateRequest",
        serde_json::json!({"h_mem_id": id, "new_confidence": 0.5}),
    );
    refuses_unknown::<MemoryResolveContradictionRequest>(
        "MemoryResolveContradictionRequest",
        serde_json::json!({"h_mem_ids": [id], "strategy": "forget", "target_h_mem_id": id, "reason": "r"}),
    );
    refuses_unknown::<MemoryPruneRequest>(
        "MemoryPruneRequest",
        serde_json::json!({"max_age_days": 1}),
    );
    refuses_unknown::<MemoryDedupRequest>("MemoryDedupRequest", serde_json::json!({}));
    refuses_unknown::<BackfillEmbeddingsRequest>(
        "BackfillEmbeddingsRequest",
        serde_json::json!({}),
    );
    refuses_unknown::<MemoryExtractRequest>(
        "MemoryExtractRequest",
        serde_json::json!({"thread_id": "t"}),
    );
}

/// CU-03: `curator_memory_extract` reads one thread, exactly. A blank id is
/// refused (before: it matched every thread's turns), a partial id matches
/// nothing (before: every thread starting with it), and a public call
/// context does not receive a private-marked turn (before: no filter).
#[tokio::test]
async fn extract_reads_exactly_one_thread_and_filters_private_turns() {
    let (server, memory) = make_server_with_embeddings();
    seed_h_mem(
        &memory,
        "curator:thread:t1",
        "chunk:1:0",
        "public t1".into(),
    );
    seed_h_mem(
        &memory,
        "curator:thread:t10",
        "chunk:1:0",
        "public t10".into(),
    );
    seed_private_h_mem(&memory, "curator:thread:t1", "private t1");

    let error = server
        .curator_memory_extract(Parameters(wire(serde_json::json!({"thread_id": ""}))))
        .await
        .expect_err("a blank thread id");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);

    let partial = invoke_tool!(
        server,
        curator_memory_extract,
        wire(serde_json::json!({"thread_id": "t"}))
    );
    assert_eq!(partial["turn_count"], 0, "{partial}");

    let public = invoke_tool!(
        server,
        curator_memory_extract,
        wire(serde_json::json!({"thread_id": "t1"}))
    );
    assert_eq!(public["turn_count"], 1, "{public}");
    assert_eq!(public["candidates"][0]["value"], "public t1");
    assert_eq!(public["excluded_private_rows"], 1, "{public}");
    assert!(public["candidates"][0].get("evidence_citation").is_none());

    let private = hkask_mcp_server::with_call_provenance(
        hkask_types::InferenceProvenance::PrivateInference,
        server.curator_memory_extract(Parameters(wire(serde_json::json!({"thread_id": "t1"})))),
    )
    .await
    .expect("private context");
    let private = parse_envelope(&private);
    assert_eq!(private["turn_count"], 2, "{private}");
    assert!(private.get("excluded_private_rows").is_none());
}

/// CU-04: with the database not open, every store-backed tool returns a
/// typed error that names the cause, and ping says degraded. Before,
/// `curator_consult` and `curator_memory_recall` returned a success holding
/// `{"status":"unavailable"}`, the others said only "MemoryStore not
/// available", and ping said `ok`.
#[tokio::test]
async fn a_database_that_does_not_open_is_a_typed_error_naming_the_cause() {
    let (server, _directory) = server_with_unopenable_database();

    let ping = invoke_tool!(server, curator_ping, wire(serde_json::json!({})));
    assert_eq!(ping["status"], "degraded", "{ping}");
    assert_eq!(ping["stores"]["memory"], false);
    assert!(
        ping["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("HKASK_DB_PASSPHRASE")),
        "{ping}"
    );

    let consult = server
        .curator_consult(Parameters(wire(serde_json::json!({"query": "anything"}))))
        .await
        .expect_err("consult with no store");
    let recall = server
        .curator_memory_recall(Parameters(wire(serde_json::json!({"entity": "anything"}))))
        .await
        .expect_err("recall with no store");
    let search = server
        .curator_semantic_search(Parameters(wire(serde_json::json!({"query": "anything"}))))
        .await
        .expect_err("search with no store");
    let log = server
        .curator_algedonic_log(Parameters(wire(serde_json::json!({}))))
        .await
        .expect_err("log with no store");
    for (tool, error) in [
        ("consult", consult),
        ("recall", recall),
        ("search", search),
        ("log", log),
    ] {
        assert_eq!(
            error.kind,
            hkask_types::McpErrorKind::PermissionDenied,
            "{tool}: {}",
            error.message
        );
        assert!(
            error.message.contains("HKASK_DB_PASSPHRASE"),
            "{tool} must name the variable: {}",
            error.message
        );
    }
}

/// CU-05: `curator_memory_recall` takes an entity or an ontology axis.
/// Before, `entity` was required even for axis recall (where it was
/// ignored), and an `ontology_value` without an axis was silently dropped.
#[tokio::test]
async fn recall_takes_an_entity_or_an_axis_and_refuses_mixed_forms() {
    let server = make_server();

    let by_axis = invoke_tool!(
        server,
        curator_memory_recall,
        wire(serde_json::json!({"ontology_axis": "dc_type", "ontology_value": "zz-type"}))
    );
    assert_eq!(by_axis["count"], 0, "{by_axis}");

    for arguments in [
        serde_json::json!({}),
        serde_json::json!({"entity": ""}),
        serde_json::json!({"entity": "e", "ontology_value": "zz-type"}),
        serde_json::json!({"entity": "e", "ontology_axis": "dc_type", "ontology_value": "zz-type"}),
    ] {
        let error = server
            .curator_memory_recall(Parameters(wire(arguments.clone())))
            .await
            .expect_err("a mixed or empty form");
        assert_eq!(
            error.kind,
            hkask_types::McpErrorKind::InvalidArgument,
            "{arguments}: {}",
            error.message
        );
    }
}

/// CU-06: a confidence outside 0.0-1.0 is refused. Before, it was clamped
/// silently (7 became 1.0, which pooling then treats as near-certainty).
#[tokio::test]
async fn confidence_arguments_outside_the_unit_interval_are_refused() {
    let (server, memory) = make_server_with_embeddings();
    let id = seed_h_mem(&memory, "zz-probe:confidence", "finding", "v".into());

    for value in [7.0, -0.1] {
        let error = server
            .memory_update(Parameters(wire(
                serde_json::json!({"h_mem_id": id, "new_confidence": value}),
            )))
            .await
            .expect_err("out-of-range confidence");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        assert!(
            error.message.contains("new_confidence"),
            "{}",
            error.message
        );
    }
    let error = server
        .memory_resolve_contradiction(Parameters(wire(serde_json::json!({
            "h_mem_ids": [id], "strategy": "update_confidence", "target_h_mem_id": id,
            "new_confidence": 1.5, "reason": "zz"
        }))))
        .await
        .expect_err("out-of-range confidence");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);

    let stored = memory
        .get_by_id(&id.parse().expect("id"))
        .expect("read")
        .expect("the row is still there under its id");
    assert!((stored.confidence.value() - 1.0).abs() < 1e-9);
}

/// CU-07: `memory_resolve_contradiction` checks its arguments before it
/// reads or deletes. Before, a blank reason was accepted (the description
/// says a reason is required) and an unknown strategy was reported only
/// after the target lookup.
#[tokio::test]
async fn resolve_contradiction_refuses_a_blank_reason_and_an_unknown_strategy_first() {
    let (server, memory) = make_server_with_embeddings();
    let id = seed_h_mem(&memory, "zz-probe:resolve", "finding", "v".into());

    let error = server
        .memory_resolve_contradiction(Parameters(wire(serde_json::json!({
            "h_mem_ids": [id], "strategy": "forget", "target_h_mem_id": id, "reason": "  "
        }))))
        .await
        .expect_err("a blank reason");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
    assert!(error.message.contains("reason"), "{}", error.message);

    let unknown = "33333333-3333-4333-8333-333333333333";
    let error = server
        .memory_resolve_contradiction(Parameters(wire(serde_json::json!({
            "h_mem_ids": [unknown], "strategy": "merge", "target_h_mem_id": unknown, "reason": "zz"
        }))))
        .await
        .expect_err("an unknown strategy");
    assert_eq!(
        error.kind,
        hkask_types::McpErrorKind::InvalidArgument,
        "the strategy is checked before the target lookup: {}",
        error.message
    );
    assert!(
        error.message.contains("forget") && error.message.contains("update_confidence"),
        "{}",
        error.message
    );
    assert!(
        memory
            .get_by_id(&id.parse().expect("id"))
            .expect("read")
            .is_some(),
        "a refused call deletes nothing"
    );
}

/// CU-08: `memory_insert` keeps a note on a value that is not an object
/// (before: dropped silently), returns the new row's id (before: no id, so
/// the row could not be updated or cited without a second read), accepts a
/// re-asserted non-object value (before: refused as a contradiction with
/// itself), and does not reset the decay clock of the rows it compares.
#[tokio::test]
async fn insert_keeps_notes_returns_its_id_and_leaves_compared_rows_untouched() {
    let (server, memory) = make_server_with_embeddings();
    let evidence = seed_h_mem(
        &memory,
        "curator:thread:zz-evidence",
        "chunk:1:0",
        "source".into(),
    );

    let mut sibling = hkask_storage::HMem::new(
        "zz-probe:insert",
        "older_fact",
        serde_json::Value::String("older".to_string()),
        WebID::new(),
    );
    let stale = chrono::Utc::now() - chrono::Duration::days(10);
    sibling.recalled_at = stale;
    let sibling_id = sibling.id;
    memory.store(sibling).expect("seed sibling");

    let inserted = invoke_tool!(
        server,
        memory_insert,
        wire(serde_json::json!({
            "entity": "zz-probe:insert", "attribute": "count", "value": "5",
            "evidence_h_mem_id": evidence, "note": "why five"
        }))
    );
    let new_id = inserted["h_mem_id"]
        .as_str()
        .expect("the response names the new row");
    let stored = memory
        .get_by_id(&new_id.parse().expect("id"))
        .expect("read")
        .expect("the returned id resolves");
    assert_eq!(stored.value["value"], 5, "{}", stored.value);
    assert_eq!(stored.value["_note"], "why five", "{}", stored.value);

    let again = invoke_tool!(
        server,
        memory_insert,
        wire(serde_json::json!({
            "entity": "zz-probe:insert", "attribute": "count", "value": "5",
            "evidence_h_mem_id": evidence
        }))
    );
    assert_eq!(
        again["inserted"], true,
        "a re-assertion is not a contradiction: {again}"
    );

    let differing = server
        .memory_insert(Parameters(wire(serde_json::json!({
            "entity": "zz-probe:insert", "attribute": "count", "value": "6",
            "evidence_h_mem_id": evidence
        }))))
        .await
        .expect_err("a differing value");
    assert_eq!(
        differing.kind,
        hkask_types::McpErrorKind::FailedPrecondition
    );

    let sibling = memory
        .get_by_id(&sibling_id)
        .expect("read")
        .expect("sibling");
    assert!(
        (sibling.recalled_at - stale).num_seconds().abs() < 2,
        "the contradiction check is not a recall: recalled_at moved to {}",
        sibling.recalled_at
    );
}

/// CU-09: a skill-use report returns its row id, and that id is accepted
/// as `memory_insert` evidence (the learning loop's own step: report, then
/// record the lesson citing the report). Blank identifying fields are refused.
#[tokio::test]
async fn a_skill_use_report_returns_an_id_that_memory_insert_accepts_as_evidence() {
    let (server, _memory) = make_server_with_embeddings();

    let report = invoke_tool!(
        server,
        curator_report_skill_use_issue,
        wire(serde_json::json!({
            "skill_name": "zz-probe-skill", "tool_name": "zz_tool", "step_ordinal": 1,
            "error": "zz failure", "failure_origin": "unknown"
        }))
    );
    let report_id = report["h_mem_id"]
        .as_str()
        .expect("the report names its row");

    let lesson = invoke_tool!(
        server,
        memory_insert,
        wire(serde_json::json!({
            "entity": "zz-probe:lesson", "attribute": "from_report",
            "value": "\"a lesson\"", "evidence_h_mem_id": report_id
        }))
    );
    assert_eq!(lesson["inserted"], true, "{lesson}");

    for arguments in [
        serde_json::json!({"skill_name": " ", "tool_name": "t", "step_ordinal": 1, "error": "e", "failure_origin": "unknown"}),
        serde_json::json!({"skill_name": "s", "tool_name": "", "step_ordinal": 1, "error": "e", "failure_origin": "unknown"}),
        serde_json::json!({"skill_name": "s", "tool_name": "t", "step_ordinal": 1, "error": "", "failure_origin": "unknown"}),
    ] {
        let error = server
            .curator_report_skill_use_issue(Parameters(wire(arguments.clone())))
            .await
            .expect_err("a blank identifying field");
        assert_eq!(
            error.kind,
            hkask_types::McpErrorKind::InvalidArgument,
            "{arguments}"
        );
    }
}

/// CU-10: an embedding failure is classified by what failed. A provider
/// that is not configured is `permission_denied` naming what is missing; an
/// outage is `unavailable`. Before, the write paths reported every failure
/// as `failed_precondition` and federated search reported every failure as
/// `unavailable`.
#[tokio::test]
async fn embedding_failures_are_classified_by_cause() {
    let (unconfigured, memory) = make_server_with_embedding_port(Arc::new(NotConfiguredEmbedPort));
    let evidence = seed_h_mem(
        &memory,
        "curator:thread:zz-evidence",
        "chunk:1:0",
        "source".into(),
    );
    let insert = serde_json::json!({
        "entity": "zz-probe:classified", "attribute": "a", "value": "\"v\"",
        "evidence_h_mem_id": evidence
    });

    let error = unconfigured
        .memory_insert(Parameters(wire(insert.clone())))
        .await
        .expect_err("insert with an unconfigured embedder");
    assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
    assert!(error.message.contains("ZZ_API_KEY"), "{}", error.message);
    assert!(error.message.contains("NOT stored"), "{}", error.message);
    let error = unconfigured
        .curator_federated_search(Parameters(wire(serde_json::json!({"query": "q"}))))
        .await
        .expect_err("federated search with an unconfigured embedder");
    assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);

    let (offline, memory) = make_server_with_embedding_port(failing_inference_port());
    let evidence = seed_h_mem(
        &memory,
        "curator:thread:zz-evidence",
        "chunk:1:0",
        "source".into(),
    );
    let mut insert = insert;
    insert["evidence_h_mem_id"] = serde_json::json!(evidence);
    let error = offline
        .memory_insert(Parameters(wire(insert)))
        .await
        .expect_err("insert during an outage");
    assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
    let error = offline
        .curator_federated_search(Parameters(wire(serde_json::json!({"query": "q"}))))
        .await
        .expect_err("federated search during an outage");
    assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
    assert!(
        memory
            .h_mems_by_entity("zz-probe:classified")
            .expect("query")
            .is_empty(),
        "no row lands without its vector"
    );
}

/// CU-11: `curator_consult` reports the rows it excluded. Before, it was
/// the one recall tool that dropped private-marked rows (and rows gated out
/// by the embedding model) without saying so, so a filtered result read as
/// "the curator has no memory of this". Blank queries are refused by both
/// query tools.
#[tokio::test]
async fn consult_reports_excluded_rows_and_blank_queries_are_refused() {
    let (server, memory) = make_server_with_embeddings();
    seed_private_h_mem(&memory, "curator:thread:zz-private", "a private turn");
    seed_embedding(&memory, "curator:thread:zz-private");

    let consult = invoke_tool!(
        server,
        curator_consult,
        wire(serde_json::json!({"query": "any question"}))
    );
    assert_eq!(consult["entity_wide_fragments"]["count"], 0, "{consult}");
    assert_eq!(consult["excluded_private_rows"], 1, "{consult}");

    let federated = invoke_tool!(
        server,
        curator_federated_search,
        wire(serde_json::json!({"query": "any question"}))
    );
    let reason = federated["sources"][0]["reason"].as_str().expect("reason");
    assert!(
        reason.starts_with("1 private-marked row(s) excluded"),
        "the reason is a sentence, not a fragment after a separator: {reason}"
    );

    for blank in ["", "   "] {
        let error = server
            .curator_consult(Parameters(wire(serde_json::json!({"query": blank}))))
            .await
            .expect_err("a blank consult query");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        let error = server
            .curator_semantic_search(Parameters(wire(serde_json::json!({"query": blank}))))
            .await
            .expect_err("a blank search query");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
    }
}

/// CU-12: bounded reads say when the bound was reached. Before, a
/// `reg_query` that hit its limit, an algedonic log at 500 and a dedup that
/// scanned only the newest `limit` rows were indistinguishable from complete
/// results; `reg_query` accepted `limit: 0` and returned an empty success.
/// The phase is the stored wire word, not a Rust debug rendering.
#[tokio::test]
async fn bounded_reads_report_when_the_bound_was_reached() {
    let (server, archive) = make_server_with_regulation_archive();
    let base = chrono::Utc::now() - chrono::Duration::minutes(5);
    for offset in 0..3 {
        archive
            .persist(&regulation_record(
                "reg.skill",
                "zz.operator_feedback",
                base + chrono::Duration::seconds(offset),
                "zz",
            ))
            .expect("seed record");
    }

    let cut = invoke_tool!(
        server,
        reg_query,
        wire(serde_json::json!({"window_seconds": 3600, "limit": 2}))
    );
    assert_eq!(cut["count"], 2);
    assert_eq!(cut["limit_reached"], true, "{cut}");
    assert_eq!(cut["ordering"], "oldest_first");
    assert_eq!(cut["events"][0]["phase"], "sense");
    let whole = invoke_tool!(
        server,
        reg_query,
        wire(serde_json::json!({"window_seconds": 3600, "limit": 10}))
    );
    assert_eq!(whole["limit_reached"], false, "{whole}");

    let error = server
        .reg_query(Parameters(wire(
            serde_json::json!({"window_seconds": 3600, "limit": 0}),
        )))
        .await
        .expect_err("limit 0");
    assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);

    let log = invoke_tool!(server, curator_algedonic_log, wire(serde_json::json!({})));
    assert_eq!(log["limit_reached"], false, "{log}");

    let (server, memory) = make_server_with_embeddings();
    for index in 0..3 {
        seed_h_mem(
            &memory,
            "zz-probe:dedup",
            &format!("a{index}"),
            "same".into(),
        );
    }
    let dedup = invoke_tool!(
        server,
        curator_memory_dedup,
        wire(serde_json::json!({"limit": 2}))
    );
    assert_eq!(dedup["scanned"], 2);
    assert_eq!(dedup["limit_reached"], true, "{dedup}");
    let dedup = invoke_tool!(server, curator_memory_dedup, wire(serde_json::json!({})));
    assert_eq!(dedup["limit_reached"], false, "{dedup}");
}

/// CU-13: the model-mismatch sweep. A real sweep whose identity probe fails
/// stops before deleting anything (before: it deleted each stale row and
/// then failed to store its replacement, leaving the rows unembedded). A
/// dry run reports the failed probe, and the probe's embedding call shows
/// in the tool's usage (before: the call was made and reported as no usage).
#[tokio::test]
async fn a_model_mismatch_sweep_with_a_dead_embedder_deletes_nothing() {
    let (offline, memory) = make_server_with_embedding_port(failing_inference_port());
    seed_h_mem(
        &memory,
        "zz-probe:sweep-stale",
        "finding",
        "stale passage".into(),
    );
    memory
        .store_embedding(
            "zz-probe:sweep-stale",
            &unit_vector(),
            "legacy-model",
            Some("stale passage"),
        )
        .expect("seed stale embedding");
    let before = memory.embedding_count().expect("count");

    let error = offline
        .curator_memory_backfill_embeddings(Parameters(wire(
            serde_json::json!({"mode": "model_mismatch"}),
        )))
        .await
        .expect_err("a sweep with no embedder");
    assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
    assert!(
        error.message.contains("nothing was deleted"),
        "{}",
        error.message
    );
    assert_eq!(memory.embedding_count().expect("count"), before);

    let dry = invoke_tool!(
        offline,
        curator_memory_backfill_embeddings,
        wire(serde_json::json!({"mode": "model_mismatch", "dry_run": true}))
    );
    assert!(dry["identity_probe_error"].is_string(), "{dry}");
    assert_eq!(memory.embedding_count().expect("count"), before);

    let (online, memory) = make_server_with_embeddings();
    seed_h_mem(
        &memory,
        "zz-probe:sweep-stale",
        "finding",
        "stale passage".into(),
    );
    let output = online
        .curator_memory_backfill_embeddings(Parameters(wire(
            serde_json::json!({"mode": "model_mismatch", "dry_run": true}),
        )))
        .await
        .expect("dry run");
    let envelope: serde_json::Value = serde_json::from_str(&output).expect("envelope");
    assert_eq!(
        envelope["usage"]["calls"], 1,
        "the dry run's identity probe is an embedding call: {envelope}"
    );
    assert!(envelope["content"]["identity_probe_error"].is_null());
}

/// CU-16: `curator_memory_recall` is bounded and says what the bound cut.
/// Before, both forms returned every match: an axis recall on the value
/// ingest stamps on every turn chunk returned all stored turns in one
/// response. The count of matches is `total`; `limit_reached` is true when
/// the response holds fewer.
#[tokio::test]
async fn recall_is_bounded_and_reports_what_the_bound_cut() {
    let (server, memory) = make_server_with_embeddings();
    for index in 0..3 {
        let h_mem = hkask_storage::HMem::new(
            "zz-probe:bounded",
            &format!("fact{index}"),
            serde_json::Value::String(format!("value {index}")),
            WebID::new(),
        )
        .with_ontology(hkask_types::HMemOntology {
            dc_type: "zz-type".to_string(),
            ..Default::default()
        });
        memory.store(h_mem).expect("seed");
    }

    let by_axis = invoke_tool!(
        server,
        curator_memory_recall,
        wire(
            serde_json::json!({"ontology_axis": "dc_type", "ontology_value": "zz-type", "limit": 2})
        )
    );
    assert_eq!(by_axis["count"], 2, "{by_axis}");
    assert_eq!(by_axis["total"], 3);
    assert_eq!(by_axis["limit_reached"], true);

    let by_entity = invoke_tool!(
        server,
        curator_memory_recall,
        wire(
            serde_json::json!({"entity": "zz-probe:bounded", "recall_shape": "entity_wide", "limit": 2})
        )
    );
    assert_eq!(by_entity["entity_wide"]["count"], 2, "{by_entity}");
    assert_eq!(by_entity["entity_wide"]["total"], 3);
    assert_eq!(by_entity["entity_wide"]["limit_reached"], true);

    let whole = invoke_tool!(
        server,
        curator_memory_recall,
        wire(serde_json::json!({"entity": "zz-probe:bounded", "recall_shape": "entity_wide"}))
    );
    assert_eq!(whole["entity_wide"]["count"], 3);
    assert_eq!(whole["entity_wide"]["limit_reached"], false);
}

/// CU-17: only the rows a recall returns count as recalled. Before, entity
/// recall reset the decay clock of every row under the entity before the
/// provenance filter ran, so a public call refreshed a private-marked row it
/// was never shown (and a row the fallback's limit cut was refreshed too).
#[tokio::test]
async fn recall_resets_the_decay_clock_of_returned_rows_only() {
    let (server, memory) = make_server_with_embeddings();
    let stale = chrono::Utc::now() - chrono::Duration::days(20);
    let mut public = hkask_storage::HMem::new(
        "zz-probe:touch",
        "public_fact",
        serde_json::Value::String("shown".to_string()),
        WebID::new(),
    );
    public.recalled_at = stale;
    let public_id = public.id;
    memory.store(public).expect("seed public");
    let mut private = hkask_storage::HMem::new(
        "zz-probe:touch",
        "private_fact",
        serde_json::Value::String("not shown".to_string()),
        WebID::new(),
    )
    .with_inference_provenance(hkask_types::InferenceProvenance::PrivateInference);
    private.recalled_at = stale;
    let private_id = private.id;
    memory.store(private).expect("seed private");

    let recall = invoke_tool!(
        server,
        curator_memory_recall,
        wire(serde_json::json!({"entity": "zz-probe:touch", "recall_shape": "entity_wide"}))
    );
    assert_eq!(recall["entity_wide"]["count"], 1, "{recall}");
    assert_eq!(recall["entity_wide"]["excluded_private_rows"], 1);

    let recalled_at = |id: &hkask_storage::HMemId| {
        memory
            .get_by_id(id)
            .expect("read")
            .expect("row")
            .recalled_at
    };
    assert!(
        (chrono::Utc::now() - recalled_at(&public_id)).num_seconds() < 60,
        "the returned row was recalled: its clock is reset"
    );
    assert!(
        (recalled_at(&private_id) - stale).num_seconds().abs() < 2,
        "a row the call was not shown is not refreshed by it: {}",
        recalled_at(&private_id)
    );
}

/// CU-18: the pooling rule the description states, at the wire. 0.5 leaves
/// the stored confidence unchanged, a value above 0.5 raises it and a value
/// below lowers it; the result is the log-odds sum.
#[tokio::test]
async fn memory_update_pools_log_odds_as_described() {
    let (server, memory) = make_server_with_embeddings();
    let seed = |attribute: &str| {
        let h_mem = hkask_storage::HMem::new(
            "zz-probe:pooling",
            attribute,
            serde_json::Value::String(format!("value for {attribute}")),
            WebID::new(),
        )
        .with_confidence(hkask_types::Confidence::new(0.8));
        let id = h_mem.id.to_string();
        memory.store(h_mem).expect("seed");
        id
    };
    let pooled = |prior: f64, input: f64| {
        let logit = |p: f64| (p / (1.0 - p)).ln();
        1.0 / (1.0 + (-(logit(prior) + logit(input))).exp())
    };

    for (attribute, input) in [("neutral", 0.5), ("agree", 0.8), ("disagree", 0.2)] {
        let id = seed(attribute);
        let updated = invoke_tool!(
            server,
            memory_update,
            wire(serde_json::json!({"h_mem_id": id, "new_confidence": input}))
        );
        let combined = updated["combined_confidence"].as_f64().expect("combined");
        assert!(
            (combined - pooled(0.8, input)).abs() < 1e-9,
            "{attribute}: got {combined}, expected {}",
            pooled(0.8, input)
        );
        assert_eq!(updated["previous_confidence"], 0.8);
        let replacement = updated["h_mem_id"].as_str().expect("replacement id");
        assert_ne!(replacement, id, "the row is replaced");
        let stored = memory
            .get_by_id(&replacement.parse().expect("id"))
            .expect("read")
            .expect("replacement row");
        assert!((stored.confidence.value() - combined).abs() < 1e-9);
    }
}

/// CU-31: a confidence written at or below the editor's consolidation floor
/// is a deferred delete, and the two tools that can write one say so on the
/// parameter. The editor's consolidation pass
/// (`MemoryConsolidator::consolidate`, every
/// `kask.memory.consolidation_cadence_secs`) deletes every h_mem whose
/// stored confidence is at or below `kask.memory.confidence_floor`
/// (default 0.3). Driven through both handlers and the real consolidator;
/// the schema half is `low_confidence_parameters_name_the_consolidation_floor`
/// in the crate's tool-surface tests.
#[tokio::test]
async fn a_confidence_at_or_below_the_consolidation_floor_is_deleted_by_the_next_pass() {
    const FLOOR: f64 = 0.3;
    let (server, memory) = make_server_with_embeddings();
    let seed = |attribute: &str| {
        let h_mem = hkask_storage::HMem::new(
            "zz-probe:floor",
            attribute,
            serde_json::Value::String(format!("value for {attribute}")),
            WebID::new(),
        )
        .with_confidence(hkask_types::Confidence::new(0.5));
        let id = h_mem.id.to_string();
        memory.store(h_mem).expect("seed");
        id
    };

    // Pooling a stored 0.5 with x gives x.
    let pooled_low = invoke_tool!(
        server,
        memory_update,
        wire(serde_json::json!({"h_mem_id": seed("pooled-low"), "new_confidence": 0.2}))
    );
    let pooled_above = invoke_tool!(
        server,
        memory_update,
        wire(serde_json::json!({"h_mem_id": seed("pooled-above"), "new_confidence": 0.31}))
    );
    let target = seed("set-at-floor");
    let set_at_floor = invoke_tool!(
        server,
        memory_resolve_contradiction,
        wire(serde_json::json!({
            "h_mem_ids": [target],
            "strategy": "update_confidence",
            "target_h_mem_id": target,
            "new_confidence": FLOOR,
            "reason": "superseded by a later observation"
        }))
    );
    let replacement = |response: &serde_json::Value, key: &str| -> hkask_storage::HMemId {
        response[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} in {response}"))
            .parse()
            .expect("id")
    };
    let low_id = replacement(&pooled_low, "h_mem_id");
    let above_id = replacement(&pooled_above, "h_mem_id");
    let floor_id = replacement(&set_at_floor, "target_h_mem_id");
    for id in [&low_id, &above_id, &floor_id] {
        assert!(memory.get_by_id(id).expect("read").is_some());
    }

    let outcome = hkask_memory::MemoryConsolidator::new(Arc::clone(&memory))
        .consolidate(&WebID::new(), FLOOR)
        .expect("consolidation pass");
    assert_eq!(outcome.deleted_count, 2, "{outcome:?}");
    assert!(
        memory.get_by_id(&low_id).expect("read").is_none(),
        "a memory pooled to 0.2 is deleted by the pass"
    );
    assert!(
        memory.get_by_id(&floor_id).expect("read").is_none(),
        "a memory set to exactly the floor is deleted by the pass"
    );
    assert!(
        memory.get_by_id(&above_id).expect("read").is_some(),
        "a memory pooled to 0.31 survives the pass"
    );
}
