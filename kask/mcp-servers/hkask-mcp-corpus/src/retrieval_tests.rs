//! Offline retrieval contracts exercised through the real corpus tools.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult, WebID};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

use crate::CorpusServer;
use crate::tools::calibration::EmbeddingInventoryRequest;
use crate::tools::semantic::EmbedRequest;
use crate::tools::storage::{PurgeQaRequest, QueryRequest};

const PASSPHRASE: &str = crate::helpers::TEST_PASSPHRASE;
const ORIGINAL: &str = "The archive records the river flooding in spring.";
const SYNTHESIZED: &str = "The river floods each spring and replenishes the fertile valley.";

struct IdentityPort {
    actual_model: Option<String>,
}

impl InferencePort for IdentityPort {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async {
            Err(InferenceError::NotConfigured(
                "generation unused".to_string(),
            ))
        })
    }

    fn embed_with_identity<'a>(
        &'a self,
        model: &str,
        texts: &[String],
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        let requested_model = model.to_string();
        let actual_model = self.actual_model.clone();
        let count = texts.len();
        Box::pin(async move {
            Ok(hkask_types::EmbeddingBatch {
                vectors: vec![vec![1.0; crate::embedding_dim()]; count],
                requested_model,
                actual_model,
                usage: hkask_types::InferenceUsage::default(),
                cost_usd: None,
            })
        })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        // Explicit width routing (not the trait default): corpus_embed passes
        // the store's width, and this stub embeds at the same resolver — a
        // request mismatch is a test bug, asserted here.
        if let Some(d) = dimensions {
            debug_assert_eq!(d as usize, crate::embedding_dim());
        }
        self.embed_with_identity(model, texts)
    }
}

#[derive(Default)]
struct RecordingPort {
    prompts: Mutex<Vec<String>>,
    generation_models: Mutex<Vec<Option<String>>>,
    embedding_requests: Mutex<Vec<(String, Option<u32>)>>,
    generation_error: Option<InferenceError>,
    embedding_error: Option<hkask_types::EmbeddingGenerationError>,
    inputs: Mutex<Vec<String>>,
    pause: Option<(tokio::sync::Notify, tokio::sync::Notify)>,
    embedding_lifetime: Option<Arc<H8InferenceLifetime>>,
    generation_lifetime: Option<Arc<H8InferenceLifetime>>,
    short: bool,
    wrong_dimension: bool,
    response: Option<String>,
    corrupt_centroid_db: Option<std::path::PathBuf>,
}

impl InferencePort for RecordingPort {
    fn generate(
        &self,
        prompt: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.prompts
            .lock()
            .expect("prompts")
            .push(prompt.to_string());
        let response = self.response.clone().unwrap_or_else(|| SYNTHESIZED.into());
        if let Some(path) = &self.corrupt_centroid_db {
            // Fault after retrieval, before centroid validation: target the lookup,
            // not an earlier KNN/open failure that would leave the bug untested.
            let db = hkask_storage::open_or_repair(&path.to_string_lossy(), PASSPHRASE)
                .expect("fault fixture database");
            db.sqlite_pool().expect("pool").get().expect("connection")
                .execute("UPDATE embeddings SET vector = X'00' WHERE entity_ref = 'style:test:hopper:centroid'", [])
                .expect("corrupt centroid vector");
        }
        Box::pin(async move {
            let _lifetime = if let Some(probe) = &self.generation_lifetime {
                let guard = H8InferenceExit(probe);
                probe.entered.notify_one();
                probe.release.notified().await;
                Some(guard)
            } else {
                None
            };
            if let Some(error) = &self.generation_error {
                return Err(error.clone());
            }
            if let Some(probe) = &self.generation_lifetime {
                probe
                    .completed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            Ok(InferenceResult {
                text: response,
                model: "offline".into(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                    reported: true,
                },
                finish_reason: "stop".into(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            })
        })
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> hkask_types::InferenceFuture<'_> {
        self.generation_models
            .lock()
            .expect("generation models")
            .push(model_override.map(str::to_owned));
        self.generate(prompt, parameters, tools)
    }

    fn embed(&self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'_> {
        self.inputs.lock().expect("inputs").extend_from_slice(texts);
        let count = texts.len();
        let requested_model = model.to_string();
        Box::pin(async move {
            let _lifetime = if let Some(probe) = &self.embedding_lifetime {
                let guard = H8InferenceExit(probe);
                probe.entered.notify_one();
                probe.release.notified().await;
                Some(guard)
            } else {
                None
            };
            if let Some(error) = &self.embedding_error {
                return Err(error.clone());
            }
            if let Some((entered, release)) = &self.pause {
                entered.notify_one();
                release.notified().await;
            }
            if let Some(probe) = &self.embedding_lifetime {
                probe
                    .completed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            Ok(hkask_types::EmbeddingBatch {
                vectors: (0..if self.short {
                    count.saturating_sub(1)
                } else {
                    count
                })
                    .map(|_| {
                        vec![
                            1.0;
                            if self.wrong_dimension {
                                crate::embedding_dim() + 1
                            } else {
                                crate::embedding_dim()
                            }
                        ]
                    })
                    .collect(),
                requested_model,
                actual_model: None,
                usage: hkask_types::InferenceUsage::default(),
                cost_usd: None,
            })
        })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        model: &str,
        texts: &[String],
        dimensions: Option<u32>,
    ) -> hkask_types::EmbedWithIdentityFuture<'a> {
        self.embedding_requests
            .lock()
            .expect("embedding requests")
            .push((model.to_owned(), dimensions));
        // Explicit width routing (not the trait default): corpus_embed and
        // corpus_query pass the store's width, and this stub embeds at the
        // same resolver (its wrong_dimension knob perturbs the response, not
        // the request) — a request mismatch is a test bug, asserted here.
        if let Some(d) = dimensions {
            debug_assert_eq!(d as usize, crate::embedding_dim());
        }
        self.embed_with_identity(model, texts)
    }
}

/// The retrieval fixtures embed under the model name `offline`, while the
/// query path resolves `HKASK_EMBEDDING_MODEL`. Align them once per process
/// so the model gate sees a homogeneous store — the gate's own behavior is
/// pinned in `index.rs` and `hkask-storage` tests with deliberately mixed
/// fixtures. The smoke test isolates its env via `env_clear()` subprocesses
/// and is unaffected.
fn ensure_offline_embedding_model_env() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        if std::env::var("HKASK_EMBEDDING_MODEL").is_err() {
            // SAFETY: test-only env write, executed once before any test
            // body relies on it, always to the same value.
            unsafe { std::env::set_var("HKASK_EMBEDDING_MODEL", "offline") };
        }
    });
}

fn server<T: InferencePort + 'static>(port: Arc<T>) -> CorpusServer {
    ensure_offline_embedding_model_env();
    crate::helpers::corpus_test_server(port)
}

fn fixture() -> tempfile::TempDir {
    let directory = std::env::current_dir()
        .expect("cwd")
        .join("target/retrieval-test");
    std::fs::create_dir_all(&directory).expect("fixture directory");
    tempfile::tempdir_in(directory).expect("isolated fixture")
}

fn content(result: Result<String, hkask_mcp_server::server::McpToolError>) -> Value {
    let value: Value = serde_json::from_str(&result.expect("tool succeeds")).expect("json");
    hkask_types::tool_response::unwrap_tool_envelope(value)
}

/// Write a style fixture (entity_ref + text pairs, style.txt provenance) to
/// style.jsonl and embed it into a fresh style.db under the offline model —
/// the shared setup of the compose/centroid style tests. Returns the
/// database path; the entries are the caller's, everything else is the
/// fixture's.
async fn embed_style_fixture(
    server: &CorpusServer,
    directory: &tempfile::TempDir,
    entries: &[(&str, &str)],
) -> std::path::PathBuf {
    let input = directory.path().join("style.jsonl");
    std::fs::write(
        &input,
        entries
            .into_iter()
            .map(|(entity_ref, text)| {
                json!({"entity_ref":entity_ref,"source":"style.txt","text":text}).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("style input");
    let database = directory.path().join("style.db");
    content(
        server
            .corpus_embed(Parameters(EmbedRequest {
                chunks_jsonl: input.to_string_lossy().into(),
                tagged_jsonl: None,
                db_path: database.to_string_lossy().into(),
                model: Some("offline".into()),
                batch_size: 10,
            }))
            .await,
    );
    database
}

/// expect: "Every tagged passage carries measured how-signals, even if LLM tagging degrades." [P3]
#[tokio::test]
async fn tagging_persists_method_signals_without_trusting_the_model() {
    // Isolate the model setting from parallel tests; inference remains offline.
    if std::env::var_os("KASK_T18_TAG_FIXTURE").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "retrieval_tests::tagging_persists_method_signals_without_trusting_the_model",
                "--nocapture",
            ])
            .env("KASK_T18_TAG_FIXTURE", "1")
            .env("HKASK_CLASSIFIER_MODEL", "offline")
            .env(
                "HKASK_TEMPLATE_ROOT",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry"),
            )
            .output()
            .await
            .expect("run isolated tagging test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    for response in [
        json!([["item-0", [], ["quantity", "process", "assertion"], null]]).to_string(),
        "not JSON".to_string(),
    ] {
        let directory = fixture();
        let port = Arc::new(RecordingPort {
            response: Some(response.clone()),
            ..Default::default()
        });
        let server = server(Arc::clone(&port));
        let input = directory.path().join("chunks.jsonl");
        let output = directory.path().join("tagged.jsonl");
        let text = "He walked and she ran. The beautiful river was silent.";
        std::fs::write(
            &input,
            json!({"entity_ref":"style:test:1", "source":"test.txt", "text":text, "word_count":11})
                .to_string(),
        )
        .expect("write chunks");
        content(
            server
                .corpus_tag_chunks(Parameters(
                    serde_json::from_value(json!({
                        "chunks_jsonl":input, "output":output, "concurrency":1, "tag_batch_size":1
                    }))
                    .expect("tag request"),
                ))
                .await,
        );
        let row: Value =
            serde_json::from_str(&std::fs::read_to_string(output).expect("tagged output"))
                .expect("tagged row");
        assert_eq!(
            row["classification"]["status"],
            if response.starts_with('[') {
                "classified"
            } else {
                "failed"
            }
        );
        let expected = hkask_memory::salience::compute_method_signals(text);
        assert_eq!(
            row["ontology"]["method_signals"],
            serde_json::to_value(expected).expect("signals")
        );
        assert_eq!(row["text"], text);
        assert!(
            row["dimensions"]
                .as_array()
                .expect("dimensions")
                .contains(&json!("how"))
        );
        assert_eq!(
            port.prompts.lock().expect("prompts").len(),
            1,
            "method extraction needs no extra LLM call"
        );
    }
}

/// expect: "Composition can read current passage text and method signals after embedding or replacement." [P3]
#[tokio::test]
async fn durable_embeddings_keep_current_method_signals() {
    let directory = fixture();
    let server = server(Arc::new(RecordingPort::default()));
    for text in [
        "He ran and she walked.",
        "Although it rained, the beautiful river glittered.",
    ] {
        content(
            server
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "methods.db",
                    text,
                )))
                .await,
        );
        let store = crate::helpers::open_memory_store(
            &directory.path().join("methods.db").to_string_lossy(),
            PASSPHRASE,
        )
        .expect("reopen store");
        let records = store
            .query_deduped_untouched("corpus:test:1")
            .expect("metadata");
        let signals = records
            .iter()
            .find(|record| record.attribute == "method_signals")
            .expect("persisted method signals");
        assert_eq!(
            signals.value,
            serde_json::to_value(hkask_memory::salience::compute_method_signals(text))
                .expect("signals")
        );
        assert_eq!(signals.access.owner_webid, server.webid);
        assert_eq!(
            records
                .iter()
                .find(|record| record.attribute == "text")
                .expect("text")
                .value,
            json!(text)
        );
        assert_eq!(
            store.h_mem_count().expect("count"),
            2,
            "replacement must not accumulate stale metrics/text"
        );
    }
}

/// expect: "Declared style methods select only matching stored passages; no declaration preserves retrieval." [P3]
#[tokio::test]
async fn composition_filters_durable_passages_by_declared_method() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let paratactic = "The water glittered and the boat drifted.";
    let hypotactic = "Because the water glittered, the boat drifted.";
    let database = embed_style_fixture(
        &server,
        &directory,
        &[("style:test:1", paratactic), ("style:test:2", hypotactic)],
    )
    .await;
    for (method, expected_count) in [
        (Value::Null, 2),
        (
            json!({"name":"parataxis","signal":{"parataxis_ratio_min":0.9}}),
            1,
        ),
    ] {
        let result = crate::compose::ComposeService::compose(composition_request(
            &database,
            Arc::clone(&port),
            method,
        ))
        .await
        .expect("compose");
        assert_eq!(result.exemplar_count, expected_count);
        let prompts = port.prompts.lock().expect("prompts");
        let prompt = prompts.last().expect("generation prompt");
        assert!(prompt.contains(paratactic));
        assert_eq!(prompt.contains(hypotactic), expected_count == 2);
    }
    let error = crate::compose::ComposeService::compose(composition_request(
        &database,
        Arc::clone(&port),
        json!({"name":"legacy","threshold":0.5}),
    ))
    .await
    .err()
    .expect("unsupported shorthand must fail");
    assert!(
        error
            .to_string()
            .contains("declared_method.threshold is unsupported")
    );

    let store =
        crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("store");
    let records = store
        .query_deduped_untouched("style:test:1")
        .expect("metadata");
    let signals = records
        .iter()
        .find(|record| record.attribute == "method_signals")
        .expect("signals");
    store
        .update_confidence(
            &signals.id,
            json!({"parataxis_ratio":"broken"}),
            hkask_types::Confidence::new(1.0),
        )
        .expect("corrupt metadata fixture");
    let method = json!({"name":"parataxis","signal":{"parataxis_ratio_min":0.9}});
    let error = crate::compose::ComposeService::compose(composition_request(
        &database,
        Arc::clone(&port),
        method.clone(),
    ))
    .await
    .err()
    .expect("corrupt metadata must surface");
    assert!(error.to_string().contains("Invalid method signals"));
    let unconstrained = crate::compose::ComposeService::compose(composition_request(
        &database,
        Arc::clone(&port),
        Value::Null,
    ))
    .await
    .expect("unconstrained retrieval");
    assert_eq!(unconstrained.exemplar_count, 2);
    for entity in ["style:test:1", "style:test:2"] {
        let records = store.query_deduped_untouched(entity).expect("records");
        for record in records
            .iter()
            .filter(|record| record.attribute == "method_signals")
        {
            store
                .delete_h_mem(&record.id)
                .expect("legacy metadata fixture");
        }
    }
    let missing =
        crate::compose::ComposeService::compose(composition_request(&database, port, method))
            .await
            .expect("missing metrics excluded");
    assert_eq!(missing.exemplar_count, 0);
    assert_eq!(
        missing.method_signals_missing, 2,
        "missing measurements must be surfaced, not reported as no matches"
    );
}

/// expect: "Compose uses the injected port for configured embedding and exact generation selection, and surfaces generation failures." [P3]
#[tokio::test]
async fn composition_routes_models_through_injected_port_and_surfaces_generation_error() {
    let directory = fixture();
    let database = directory.path().join("compose.db");
    let _store = crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE)
        .expect("empty style database");
    let selected_model = "offline/selected-non-default-generation";
    for generation_error in [false, true] {
        let port = Arc::new(RecordingPort {
            generation_error: generation_error
                .then(|| InferenceError::Generation("offline generation rejected".into())),
            ..Default::default()
        });
        let mut request = composition_request(&database, Arc::clone(&port), Value::Null);
        request.cognition.embedding.model = "offline/selected-embedding".into();
        request.cognition.jinja2_template = Some("{{ author }}: {{ prompt }}".into());
        request.generation_model = selected_model.into();
        let result = crate::compose::ComposeService::compose(request).await;
        if generation_error {
            let error = result.err().expect("generation failure must surface");
            assert!(
                error
                    .to_string()
                    .contains("Generation error: offline generation rejected"),
                "{error}"
            );
        } else {
            let composed = result.expect("compose through injected port");
            assert_eq!(composed.generated_prose, SYNTHESIZED);
            assert_eq!(composed.exemplar_count, 0);
        }
        assert_eq!(
            *port.embedding_requests.lock().expect("embedding requests"),
            vec![(
                "offline/selected-embedding".into(),
                Some(crate::embedding_dim() as u32)
            )]
        );
        assert_eq!(
            *port.inputs.lock().expect("inputs"),
            vec!["Write about a river."]
        );
        assert_eq!(
            *port.generation_models.lock().expect("generation models"),
            vec![Some(selected_model.to_owned())]
        );
        assert_eq!(
            *port.prompts.lock().expect("prompts"),
            vec!["test: Write about a river."]
        );
    }
}

/// Style tools preserve actionable provider failures rather than a generic service error.
#[tokio::test]
async fn style_tools_classify_provider_failures_without_losing_detail() {
    use hkask_types::EmbeddingGenerationError as EmbeddingError;
    use hkask_types::McpErrorKind::{Internal, InvalidArgument, PermissionDenied, Unavailable};
    let directory = fixture();
    let database = directory.path().join("style-errors.db");
    let _store = crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE)
        .expect("style database");
    let config_path = directory.path().join("cognition.yaml");
    let mut config = json!({
        "author":"test", "embedding":{"model":"offline", "dim":crate::embedding_dim(),
        "centroid_entity_ref":"style:test:centroid"},
        "validation":{"centroid_distance_max":1.0}
    });
    std::fs::write(
        &config_path,
        serde_yaml_neo::to_string(&config).expect("cognition YAML"),
    )
    .expect("config file");
    let inference_cases = [
        (
            InferenceError::NotConfigured("provider key missing".into()),
            PermissionDenied,
        ),
        (
            InferenceError::Auth("provider rejected key".into()),
            PermissionDenied,
        ),
        (
            InferenceError::Connection("provider offline".into()),
            Unavailable,
        ),
        (
            InferenceError::CircuitOpen("provider circuit open".into()),
            Unavailable,
        ),
        (
            InferenceError::Timeout("provider timed out".into()),
            Unavailable,
        ),
        (
            InferenceError::Overloaded("provider at capacity".into()),
            Unavailable,
        ),
        (
            InferenceError::Model("invalid model".into()),
            InvalidArgument,
        ),
        (
            InferenceError::Generation("provider generation failed".into()),
            Internal,
        ),
        (
            InferenceError::Json("provider JSON invalid".into()),
            Internal,
        ),
        (
            InferenceError::VisionUnsupported("unsupported provider operation".into()),
            Internal,
        ),
    ];
    let embedding_cases = [
        (
            EmbeddingError::NotConfigured("embedding key missing".into()),
            PermissionDenied,
        ),
        (
            EmbeddingError::Api(401, "embedding key rejected".into()),
            PermissionDenied,
        ),
        (
            EmbeddingError::Api(403, "embedding access denied".into()),
            PermissionDenied,
        ),
        (
            EmbeddingError::Connection("embedding provider offline".into()),
            Unavailable,
        ),
        (
            EmbeddingError::Api(429, "embedding rate limited".into()),
            Unavailable,
        ),
        (
            EmbeddingError::Api(503, "embedding provider unavailable".into()),
            Unavailable,
        ),
        (
            EmbeddingError::Api(400, "embedding request rejected".into()),
            InvalidArgument,
        ),
        (
            EmbeddingError::InvalidRequest("invalid embedding request".into()),
            InvalidArgument,
        ),
        (
            EmbeddingError::DimensionMismatch {
                expected: 3,
                actual: 2,
            },
            InvalidArgument,
        ),
        (EmbeddingError::EmptyResponse, Internal),
        (
            EmbeddingError::Json("embedding JSON invalid".into()),
            Internal,
        ),
    ];
    let cases = inference_cases
        .into_iter()
        .map(|(error, kind)| (Some(error), None, kind))
        .chain(
            embedding_cases
                .into_iter()
                .map(|(error, kind)| (None, Some(error), kind)),
        );
    for (generation_error, embedding_error, expected_kind) in cases {
        let detail = generation_error
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| embedding_error.as_ref().map(ToString::to_string))
            .expect("typed failure");
        let server = server(Arc::new(RecordingPort {
            generation_error,
            embedding_error,
            ..Default::default()
        }));
        for rewrite in [false, true] {
            let result = if rewrite {
                server
                    .corpus_rewrite(Parameters(crate::tools::compose_tools::RewriteRequest {
                        content: "Rewrite a sentence.".into(),
                        author: "test".into(),
                        db_path: database.to_string_lossy().into(),
                        dimension: "hopper".into(),
                        config_path: Some(config_path.to_string_lossy().into()),
                    }))
                    .await
            } else {
                server
                    .corpus_compose(Parameters(crate::tools::compose_tools::ComposeRequest {
                        prompt: "Write a sentence.".into(),
                        author: "test".into(),
                        db_path: database.to_string_lossy().into(),
                        no_validate: false,
                        config_path: Some(config_path.to_string_lossy().into()),
                    }))
                    .await
            };
            let error = result.expect_err("typed provider failure must surface");
            assert_eq!(
                error.kind, expected_kind,
                "rewrite={rewrite}: {detail}: {error}"
            );
            assert!(error.message.contains(&detail), "{error}");
            assert!(
                error.message.starts_with(if rewrite {
                    "Rewrite failed:"
                } else {
                    "Compose failed:"
                }),
                "{error}"
            );
        }
    }
    for (template, stage) in [("{% if %}", "parse"), ("{{ missing_value }}", "render")] {
        config["jinja2_template"] = json!(template);
        std::fs::write(
            &config_path,
            serde_yaml_neo::to_string(&config).expect("cognition YAML"),
        )
        .expect("config file");
        let port = Arc::new(RecordingPort::default());
        let server = server(Arc::clone(&port));
        let error = server
            .corpus_compose(Parameters(crate::tools::compose_tools::ComposeRequest {
                prompt: "Write a sentence.".into(),
                author: "test".into(),
                db_path: database.to_string_lossy().into(),
                no_validate: true,
                config_path: Some(config_path.to_string_lossy().into()),
            }))
            .await
            .expect_err("strict template error");
        assert_eq!(error.kind, Unavailable);
        assert!(
            error.message.contains(&format!("Template {stage} failed")),
            "{error}"
        );
        assert!(port.prompts.lock().expect("prompts").is_empty());
    }
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let error = server
        .corpus_compose(Parameters(crate::tools::compose_tools::ComposeRequest {
            prompt: "x".repeat(50_001),
            author: "test".into(),
            db_path: database.to_string_lossy().into(),
            no_validate: true,
            config_path: Some(config_path.to_string_lossy().into()),
        }))
        .await
        .expect_err("oversized input");
    assert_eq!(error.kind, InvalidArgument);
    assert!(
        error.message.contains("Prompt exceeds 50KB limit"),
        "{error}"
    );
    assert!(
        port.embedding_requests
            .lock()
            .expect("embeddings")
            .is_empty()
    );
}

/// expect: "A stored style centroid makes corpus_compose's validation run
/// for real — compose never silently runs unvalidated." [P3]
#[tokio::test]
async fn centroid_tool_stores_style_centroid_and_unblocks_compose_validation() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let database = embed_style_fixture(
        &server,
        &directory,
        &[
            ("style:jb-test:1", "The water glittered."),
            ("style:jb-test:2", "The boat drifted."),
        ],
    )
    .await;

    let centroid = content(
        server
            .corpus_centroid(Parameters(crate::tools::compose_tools::CentroidRequest {
                author: "jb-test".into(),
                db_path: database.to_string_lossy().into(),
                refs_file: None,
                dimension: None,
            }))
            .await,
    );
    assert_eq!(centroid["centroid_entity_ref"], "style:jb-test:centroid");
    assert_eq!(centroid["passage_count"], 2);
    assert_eq!(centroid["stored"], true);
    let dimension = content(
        server
            .corpus_centroid(Parameters(crate::tools::compose_tools::CentroidRequest {
                author: "jb-test".into(),
                db_path: database.to_string_lossy().into(),
                refs_file: None,
                dimension: Some("composite".into()),
            }))
            .await,
    );
    assert_eq!(
        dimension["centroid_entity_ref"],
        "style:jb-test:composite:centroid"
    );
    assert_eq!(
        dimension["passage_count"], 2,
        "dimension without a refs file preserves prefix selection"
    );

    // Idempotency through the tool: the stored centroid is excluded on
    // recompute, so the passage count stays at the seeded set.
    let again = content(
        server
            .corpus_centroid(Parameters(crate::tools::compose_tools::CentroidRequest {
                author: "jb-test".into(),
                db_path: database.to_string_lossy().into(),
                refs_file: None,
                dimension: None,
            }))
            .await,
    );
    assert_eq!(
        again["passage_count"], 2,
        "a stored centroid must never fold into its own successor"
    );

    // With the centroid stored, compose validates for real: the
    // RecordingPort embeds everything as [1.0; dim], so the generated
    // prose sits at cosine distance 0.0 from the all-ones centroid.
    let config = serde_json::from_value(json!({
        "author":"jb-test", "embedding":{"model":"offline", "dim":crate::embedding_dim(),
        "centroid_entity_ref":"style:jb-test:centroid", "retrieval":{"k_max":10}},
        "validation":{"centroid_distance_max":1.0}
    }))
    .expect("cognition config");
    let composed = crate::compose::ComposeService::compose(crate::compose::ComposeRequest {
        prompt: "Write about a river.".into(),
        db_path: database.clone(),
        db_passphrase: PASSPHRASE.into(),
        cognition: config,
        inference: port,
        generation_model: "offline".into(),
        no_validate: false,
    })
    .await
    .expect("compose");
    let validation = composed
        .validation
        .expect("validation must run when a centroid is stored");
    assert!(validation.passed);
    assert!(
        validation.distance.abs() < 1e-9,
        "distance to the all-ones centroid is 0.0, got {}",
        validation.distance
    );
}

/// expect: "Step 6 reuses selected embeddings and validates rewrites against their dimension, not an author fallback." [P3]
#[tokio::test]
async fn dimension_centroid_tool_and_rewrite_validation_round_trip() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let database = directory.path().join("dimension.db");
    let store =
        crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("store");
    for (entity_ref, value) in [
        ("corpus:selected:1", 1.0),
        ("other:selected:2", 1.0),
        ("corpus:unselected", -1.0),
        ("style:step6:centroid", -1.0),
        ("style:step6:gentle:centroid", -1.0),
        ("style:step6:rule:1", -1.0),
    ] {
        store
            .store_embedding(
                entity_ref,
                &vec![value; crate::embedding_dim()],
                "offline",
                None,
            )
            .expect("seed existing embeddings");
    }
    let refs_file = directory.path().join("refs.txt");
    std::fs::write(&refs_file, " corpus:selected:1 \r\nother:selected:2\n\ncorpus:selected:1\nstyle:step6:rule:1\nstyle:step6:centroid\nstyle:step6:hopper:centroid\n")
        .expect("refs file");
    let centroid_request = || crate::tools::compose_tools::CentroidRequest {
        author: "step6".into(),
        db_path: database.to_string_lossy().into(),
        refs_file: Some(refs_file.to_string_lossy().into()),
        dimension: Some(" Hopper ".into()),
    };
    for _ in 0..2 {
        let result = content(server.corpus_centroid(Parameters(centroid_request())).await);
        assert_eq!(result["centroid_entity_ref"], "style:step6:hopper:centroid");
        assert_eq!(result["passage_count"], 2);
        assert_eq!(result["stored"], true);
        assert_eq!(
            store.embedding_count().expect("count"),
            7,
            "only destination added"
        );
    }
    assert!(
        port.inputs.lock().expect("inputs").is_empty(),
        "centroids do not re-embed passages"
    );

    let config_path = directory.path().join("cognition.yaml");
    // Deliberately points at the opposite author centroid: rewrite must override it.
    let config = json!({
        "author":"step6", "embedding":{"model":"offline", "dim":crate::embedding_dim(),
        "centroid_entity_ref":"style:step6:centroid", "retrieval":{"k_max":10}},
        "validation":{"centroid_distance_max":0.25}
    });
    std::fs::write(
        &config_path,
        serde_yaml_neo::to_string(&config).expect("yaml"),
    )
    .expect("config file");
    let rewrite = |dimension: &str| crate::tools::compose_tools::RewriteRequest {
        content: "Make this easier to read.".into(),
        author: "step6".into(),
        db_path: database.to_string_lossy().into(),
        dimension: dimension.into(),
        config_path: Some(config_path.to_string_lossy().into()),
    };
    let result = content(server.corpus_rewrite(Parameters(rewrite("Hopper"))).await);
    assert_eq!(result["centroid_entity_ref"], "style:step6:hopper:centroid");
    assert_eq!(result["centroid_missing"], false);
    assert_eq!(result["style_passed"], true);
    assert!(
        result["centroid_distance"]
            .as_f64()
            .expect("distance")
            .abs()
            < 1e-9
    );
    assert_eq!(
        port.inputs.lock().expect("inputs").len(),
        2,
        "query and generated prose embedded"
    );

    store
        .delete_embeddings_by_entity("style:step6:hopper:centroid")
        .expect("replace target");
    store
        .store_embedding(
            "style:step6:hopper:centroid",
            &vec![-1.0; crate::embedding_dim()],
            "offline",
            None,
        )
        .expect("opposite centroid");
    let result = content(server.corpus_rewrite(Parameters(rewrite("hopper"))).await);
    assert_eq!(result["style_passed"], false);
    assert_eq!(result["centroid_missing"], false);
    assert!((result["centroid_distance"].as_f64().expect("distance") - 2.0).abs() < 1e-9);

    let missing = content(server.corpus_rewrite(Parameters(rewrite("lovelace"))).await);
    assert_eq!(
        missing["centroid_entity_ref"],
        "style:step6:lovelace:centroid"
    );
    assert_eq!(missing["centroid_missing"], true);
    assert!(missing["centroid_distance"].is_null());
    assert!(missing["style_passed"].is_null());
    assert_eq!(missing["rewritten"], SYNTHESIZED);

    let skipped = content(
        server
            .corpus_compose(Parameters(crate::tools::compose_tools::ComposeRequest {
                prompt: "Write a sentence.".into(),
                author: "step6".into(),
                db_path: database.to_string_lossy().into(),
                config_path: Some(config_path.to_string_lossy().into()),
                no_validate: true,
            }))
            .await,
    );
    assert_eq!(skipped["centroid_missing"], false);
    assert!(skipped["centroid_distance"].is_null());

    std::fs::write(&refs_file, "corpus:selected:1\ncorpus:missing\n").expect("missing ref file");
    let error = server
        .corpus_centroid(Parameters(centroid_request()))
        .await
        .expect_err("missing ref");
    assert!(error.to_string().contains("corpus:missing"));
    let stored = store
        .embeddings_by_prefix("style:step6:hopper:centroid")
        .expect("stored target");
    assert_eq!(
        stored.first().expect("target").1,
        vec![-1.0; crate::embedding_dim()],
        "no partial overwrite"
    );
    assert_eq!(store.embedding_count().expect("count"), 7);
}

/// expect: "A broken centroid lookup cannot masquerade as a missing centroid or a successful rewrite." [P3]
#[tokio::test]
async fn rewrite_centroid_lookup_failure_is_not_missing() {
    let directory = fixture();
    let database = directory.path().join("fault.db");
    let store =
        crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("store");
    store
        .store_embedding(
            "style:test:hopper:centroid",
            &vec![1.0; crate::embedding_dim()],
            "offline",
            None,
        )
        .expect("seed centroid");
    let config = json!({
        "author":"test", "embedding":{"model":"offline", "dim":crate::embedding_dim(),
        "centroid_entity_ref":"style:test:hopper:centroid"},
        "validation":{"centroid_distance_max":0.25}
    });
    let config_path = directory.path().join("cognition.yaml");
    std::fs::write(
        &config_path,
        serde_yaml_neo::to_string(&config).expect("yaml"),
    )
    .expect("config");
    let port = Arc::new(RecordingPort {
        corrupt_centroid_db: Some(database.clone()),
        ..Default::default()
    });
    let server = server(Arc::clone(&port));
    let error = server
        .corpus_rewrite(Parameters(crate::tools::compose_tools::RewriteRequest {
            content: "A short sentence.".into(),
            author: "test".into(),
            dimension: "hopper".into(),
            db_path: database.to_string_lossy().into(),
            config_path: Some(config_path.to_string_lossy().into()),
        }))
        .await
        .expect_err("lookup corruption surfaces");
    assert!(
        error.to_string().contains("Centroid lookup failed"),
        "{error}"
    );
    assert!(error.to_string().contains("Dimension mismatch"), "{error}");
    assert_eq!(
        port.prompts.lock().expect("prompts").len(),
        1,
        "fault occurred after retrieval"
    );
}

/// expect: "A refs file cannot escape the allowed roots, and an empty selection is not a prefix fallback." [P3]
#[cfg(unix)]
#[tokio::test]
async fn centroid_refs_file_containment_and_empty_selection() {
    let directory = fixture();
    let server = server(Arc::new(RecordingPort::default()));
    let refs_file = directory.path().join("empty.txt");
    std::fs::write(&refs_file, " \n\r\n").expect("empty refs");
    let request = |path: &std::path::Path| crate::tools::compose_tools::CentroidRequest {
        author: "test".into(),
        db_path: directory.path().join("unused.db").to_string_lossy().into(),
        refs_file: Some(path.to_string_lossy().into()),
        dimension: Some("hopper".into()),
    };
    let error = server
        .corpus_centroid(Parameters(request(&refs_file)))
        .await
        .expect_err("empty selection");
    assert!(error.to_string().contains("contains no entity refs"));
    assert!(!directory.path().join("unused.db").exists());

    let outside = tempfile::NamedTempFile::new().expect("outside file");
    let link = directory.path().join("escape.txt");
    std::os::unix::fs::symlink(outside.path(), &link).expect("symlink");
    for path in [outside.path(), link.as_path()] {
        let error = server
            .corpus_centroid(Parameters(request(path)))
            .await
            .expect_err("contained read");
        assert!(error.to_string().contains("outside"), "{error}");
    }
}

fn composition_request(
    database: &std::path::Path,
    port: Arc<RecordingPort>,
    method: Value,
) -> crate::compose::ComposeRequest {
    let config = serde_json::from_value(json!({
        "author":"test", "embedding":{"model":"offline", "dim":crate::embedding_dim(),
        "centroid_entity_ref":"style:test:centroid", "retrieval":{"k_max":10,"declared_method":method}},
        "validation":{"centroid_distance_max":1.0}
    })).expect("cognition config");
    crate::compose::ComposeRequest {
        prompt: "Write about a river.".into(),
        db_path: database.into(),
        db_passphrase: PASSPHRASE.into(),
        cognition: config,
        inference: port,
        generation_model: "offline".into(),
        no_validate: true,
    }
}

fn embed_request(directory: &std::path::Path, database: &str, text: &str) -> EmbedRequest {
    let path = directory.join("chunks.jsonl");
    std::fs::write(
        &path,
        json!({"entity_ref":"corpus:test:1", "source":"river.txt", "text":text, "word_count":10})
            .to_string(),
    )
    .expect("chunks");
    EmbedRequest {
        chunks_jsonl: path.to_string_lossy().into(),
        tagged_jsonl: None,
        db_path: directory.join(database).to_string_lossy().into(),
        model: Some("offline".into()),
        batch_size: 10,
    }
}

/// expect: corpus_embed reports requested and provider-confirmed model identities separately.
#[tokio::test]
async fn corpus_embed_surfaces_provider_confirmed_model_identity() {
    for (actual_model, expected_status) in [
        (Some("provider/substituted-model".to_string()), "confirmed"),
        (None, "unavailable"),
    ] {
        let directory = fixture();
        let server = server(Arc::new(IdentityPort {
            actual_model: actual_model.clone(),
        }));
        let summary = content(
            server
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "identity.db",
                    ORIGINAL,
                )))
                .await,
        );
        assert_eq!(summary["model"], "offline");
        assert_eq!(summary["requested_model"], "offline");
        assert_eq!(summary["actual_model"].as_str(), actual_model.as_deref());
        assert_eq!(summary["actual_model_status"], expected_status);
        assert_eq!(
            summary["identity_batches"]["confirmed"],
            usize::from(actual_model.is_some())
        );
        assert_eq!(
            summary["identity_batches"]["missing"],
            usize::from(actual_model.is_none())
        );
    }
}

/// expect: an interrupted calibration can identify only shard refs that are absent or stored under another actual model.
#[tokio::test]
async fn embedding_inventory_returns_exact_missing_only_retry_refs() {
    let directory = fixture();
    let database = directory.path().join("inventory.db");
    let server = server(Arc::new(IdentityPort {
        actual_model: Some("provider/actual-model".into()),
    }));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "inventory.db",
                ORIGINAL,
            )))
            .await,
    );

    let shard = directory.path().join("inventory-shard.jsonl");
    let rows = [
        json!({"entity_ref":"corpus:test:1", "source":"river.txt", "text":ORIGINAL, "word_count":10}),
        json!({"entity_ref":"corpus:test:2", "source":"river.txt", "text":"Missing passage.", "word_count":2}),
    ];
    std::fs::write(
        &shard,
        rows.into_iter()
            .map(|row| row.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("inventory shard");

    let inventory = content(
        server
            .corpus_embedding_inventory(Parameters(EmbeddingInventoryRequest {
                chunks_jsonl: shard.to_string_lossy().into(),
                db_path: database.to_string_lossy().into(),
                expected_model: "provider/actual-model".into(),
            }))
            .await,
    );
    assert_eq!(inventory["requested"], 2);
    assert_eq!(inventory["stored_matching_model"], 1);
    assert_eq!(inventory["missing_entity_refs"], json!(["corpus:test:2"]));
    assert_eq!(inventory["mismatched_model_entity_refs"], json!([]));
    assert_eq!(inventory["retry_entity_refs"], json!(["corpus:test:2"]));
    assert_eq!(inventory["complete"], false);

    let mismatch = content(
        server
            .corpus_embedding_inventory(Parameters(EmbeddingInventoryRequest {
                chunks_jsonl: shard.to_string_lossy().into(),
                db_path: database.to_string_lossy().into(),
                expected_model: "provider/other-model".into(),
            }))
            .await,
    );
    assert_eq!(mismatch["stored_matching_model"], 0);
    assert_eq!(
        mismatch["mismatched_model_entity_refs"],
        json!([{"entity_ref":"corpus:test:1","stored_model":"provider/actual-model"}])
    );
    assert_eq!(
        mismatch["retry_entity_refs"],
        json!(["corpus:test:1", "corpus:test:2"])
    );
}

fn query(database: Option<&std::path::Path>, answer: bool, include_text: bool) -> QueryRequest {
    QueryRequest {
        query: "When does the river flood?".into(),
        top_k: Some(50),
        generate_answer: Some(answer),
        include_text: Some(include_text),
        min_score: Some(0.5),
        db_path: database.map(|path| path.to_string_lossy().into()),
        lisp_probe: None,
    }
}

async fn purge(server: &CorpusServer, database: &std::path::Path) -> Value {
    content(
        server
            .corpus_purge_qa(Parameters(PurgeQaRequest {
                prefix: "corpus:test:".into(),
                db_path: database.to_string_lossy().into(),
            }))
            .await,
    )
}

/// expect: Purged passages stop appearing immediately, without a server restart.
/// [P8] Motivating: retrieval must reflect the durable corpus.
/// pre: a real DB-backed passage is embedded; post: neither warm nor restarted search returns it.
#[tokio::test]
async fn retrieval_purge_invalidates_warm_cache() {
    let directory = fixture();
    let server = server(Arc::new(RecordingPort::default()));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "memory.db",
                ORIGINAL,
            )))
            .await,
    );
    assert_eq!(
        content(
            server
                .corpus_query(Parameters(query(None, false, true)))
                .await
        )["results"]
            .as_array()
            .expect("results")
            .len(),
        1
    );
    purge(&server, &directory.path().join("memory.db")).await;
    let after = content(
        server
            .corpus_query(Parameters(query(
                Some(&directory.path().join("memory.db")),
                false,
                true,
            )))
            .await,
    );
    assert_eq!(after["results"], json!([]));
}

/// expect: Hiding returned passage text does not remove the evidence used to answer.
/// [P8] Motivating: grounded answers; pre: indexed source; post: original context and normalized question reach generation.
#[tokio::test]
async fn retrieval_rag_keeps_context_when_text_hidden() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "memory.db",
                ORIGINAL,
            )))
            .await,
    );
    for lisp in [false, true] {
        let mut request = query(None, true, false);
        if lisp {
            request.query =
                r#"(list (list "query" "When does the river flood?") (list "generate-answer" t))"#
                    .into();
        }
        let answer = content(server.corpus_query(Parameters(request)).await);
        assert!(answer["results"][0].get("text").is_none());
        assert!(answer.get("answer").is_some());
        let prompts = port.prompts.lock().expect("prompts");
        let prompt = prompts.last().expect("generation called");
        assert!(
            prompt.contains(ORIGINAL),
            "source context missing: {prompt}"
        );
        assert!(prompt.contains("When does the river flood?"));
        assert!(!prompt.contains("(list"), "raw Lisp is not the question");
    }
}

/// expect: Synthesized passages are searchable immediately and after restart with identical text.
/// [P8] Motivating: persistent grounding; pre: two overlapping sources; post: synthesis and sources survive.
#[tokio::test]
async fn retrieval_consolidation_survives_restart() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let mut request = embed_request(directory.path(), "memory.db", ORIGINAL);
    let chunks = ["corpus:test:1", "corpus:test:2"].map(|entity_ref| json!({"entity_ref":entity_ref, "classification":{"status":"classified", "ontology_protocol":hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL}, "source":"river.txt", "text":ORIGINAL, "word_count":10, "candidate_terms":["quantity"], "ontology_tags":{"sumo":[hkask_bridge_ontology::sumo::QUANTITY]}, "concepts":[hkask_bridge_ontology::sumo::QUANTITY], "salience":0.5}));
    let path = directory.path().join("tagged.jsonl");
    std::fs::write(
        &path,
        chunks
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("tagged");
    request.chunks_jsonl = path.to_string_lossy().into();
    content(server.corpus_embed(Parameters(request)).await);
    let request = || crate::tools::corpus::ConsolidateChunksRequest {
        tagged_jsonl: path.to_string_lossy().into(),
        output: directory
            .path()
            .join("consolidated.jsonl")
            .to_string_lossy()
            .into(),
        db_path: directory.path().join("memory.db").to_string_lossy().into(),
        prefix: "corpus:test:".into(),
        threshold: 0.75,
        concurrency: 2,
        max_chunks_per_cluster: 10,
        dry_run: false,
    };
    for _ in 0..2 {
        let summary = content(
            server
                .corpus_consolidate_chunks(Parameters(request()))
                .await,
        );
        assert_eq!(summary["reembedded"], 1, "{summary}");
        let consolidated = std::fs::read_to_string(directory.path().join("consolidated.jsonl"))
            .expect("consolidated output");
        for row in consolidated.lines() {
            let chunk: hkask_types::corpus::TaggedChunk = serde_json::from_str(row).expect("chunk");
            assert_eq!(
                chunk.classification,
                hkask_types::corpus::ClassificationOutcome::Unverified
            );
            assert_eq!(
                chunk
                    .ontology
                    .expect("ontology")
                    .method_signals
                    .expect("recomputed signals"),
                hkask_memory::salience::compute_method_signals(&chunk.text)
            );
        }
    }
    assert!(port.inputs.lock().expect("inputs").iter().any(|input| {
        input
            == &format!(
                "[sumo: {}] {SYNTHESIZED}",
                hkask_bridge_ontology::sumo::QUANTITY
            )
    }));
    let fresh = self::server(port);
    for current in [&server, &fresh] {
        let result = content(
            current
                .corpus_query(Parameters(query(
                    Some(&directory.path().join("memory.db")),
                    false,
                    true,
                )))
                .await,
        );
        let results = result["results"].as_array().expect("results");
        assert_eq!(results.len(), 3, "source entities retained: {result}");
        assert!(
            results.iter().any(|entry| entry["text"] == SYNTHESIZED),
            "{result}"
        );
    }
}

/// expect: Ontology instructions influence vectors, never the stored/returned source text.
/// [P8] Motivating: source fidelity; pre: same ref embedded twice; post: one latest passage, warm and cold.
#[tokio::test]
async fn retrieval_upsert_preserves_unannotated_text() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "memory.db",
                "old text",
            )))
            .await,
    );
    let mut request = embed_request(directory.path(), "memory.db", ORIGINAL);
    let tags = directory.path().join("tags.jsonl");
    std::fs::write(
        &tags,
        json!({"entity_ref":"corpus:test:1", "classification":{"status":"classified", "ontology_protocol":hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL}, "source":"river.txt", "text":ORIGINAL, "candidate_terms":[hkask_bridge_ontology::golem::CHARACTER], "ontology_tags":{"golem":[hkask_bridge_ontology::golem::CHARACTER]}, "concepts":[hkask_bridge_ontology::golem::CHARACTER]}).to_string(),
    )
    .expect("tags");
    request.tagged_jsonl = Some(tags.to_string_lossy().into());
    content(server.corpus_embed(Parameters(request)).await);
    assert!(port.inputs.lock().expect("inputs").iter().any(|input| input
        == &format!(
            "[golem: {}] {ORIGINAL}",
            hkask_bridge_ontology::golem::CHARACTER
        )));
    let fresh = self::server(Arc::clone(&port));
    for current in [&server, &fresh] {
        let result = content(
            current
                .corpus_query(Parameters(query(
                    Some(&directory.path().join("memory.db")),
                    false,
                    true,
                )))
                .await,
        );
        assert_eq!(result["total_indexed"], 1);
        assert_eq!(result["results"][0]["text"], ORIGINAL);
    }
    let store = crate::helpers::open_memory_store(
        &directory.path().join("memory.db").to_string_lossy(),
        PASSPHRASE,
    )
    .expect("DB");
    assert_eq!(store.embedding_count().expect("count"), 1);
}

/// expect: Purge affects the named DB/ref only, including equivalent path spellings.
/// [P8] Motivating: provenance isolation; pre: same ref in two DBs and ephemeral; post: other origins survive.
#[tokio::test]
async fn retrieval_origin_isolation_and_path_aliases() {
    let directory = fixture();
    let server = server(Arc::new(RecordingPort::default()));
    let database = directory.path().join("first.db");
    let mut request = embed_request(directory.path(), "first.db", ORIGINAL);
    request.db_path = database
        .strip_prefix(std::env::current_dir().expect("cwd"))
        .expect("relative DB")
        .to_string_lossy()
        .into();
    content(server.corpus_embed(Parameters(request)).await);
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "second.db",
                "other DB text",
            )))
            .await,
    );
    crate::services::convert::ConvertService::from_corpus(&server)
        .index_passages(
            &[("corpus:test:1".into(), "ephemeral text".into())],
            "river.txt",
        )
        .await
        .expect("ephemeral index");
    let request = query(
        Some(&directory.path().join("does-not-exist.db")),
        false,
        true,
    );
    assert_eq!(
        content(server.corpus_query(Parameters(request)).await)["total_indexed"],
        3,
        "nonempty index must not consult db_path"
    );
    assert!(!directory.path().join("does-not-exist.db").exists());
    purge(&server, &database).await;
    let retained = content(
        server
            .corpus_query(Parameters(query(None, false, true)))
            .await,
    );
    assert_eq!(retained["total_indexed"], 2);
    let texts: Vec<_> = retained["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|entry| entry["text"].as_str().expect("text"))
        .collect();
    assert!(texts.contains(&"other DB text") && texts.contains(&"ephemeral text"));
    #[cfg(unix)]
    {
        let alias = directory.path().join("alias.db");
        std::os::unix::fs::symlink(&database, &alias).expect("DB symlink");
        content(
            server
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "first.db",
                    ORIGINAL,
                )))
                .await,
        );
        purge(&server, &alias).await;
        assert_eq!(
            content(
                server
                    .corpus_query(Parameters(query(None, false, true)))
                    .await
            )["total_indexed"],
            2
        );
    }
}

/// expect: A db_path named alongside a nonempty in-memory index is ignored visibly.
/// [P8] Motivating: ignored-input visibility; pre: fresh in-memory publications
/// plus a named db_path; post: note names the in-memory count and the clear-index action.
#[tokio::test]
async fn nonempty_index_surfaces_ignored_db_path_note() {
    let directory = fixture();
    let server = server(Arc::new(RecordingPort::default()));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "memory.db",
                ORIGINAL,
            )))
            .await,
    );
    let result = content(
        server
            .corpus_query(Parameters(query(
                Some(&directory.path().join("does-not-exist.db")),
                false,
                true,
            )))
            .await,
    );
    assert_eq!(result["total_indexed"], 1, "nonempty index owns the query");
    let note = result["note"].as_str().expect("ignored-db_path note");
    assert!(note.contains("db_path ignored"), "{note}");
    assert!(
        note.contains("passages published this process: 1"),
        "{note}"
    );
    assert!(note.contains("corpus_clear_index"), "{note}");
    assert!(
        !directory.path().join("does-not-exist.db").exists(),
        "db_path was not consulted"
    );
}

/// expect: A legacy vector without stored passage text cannot masquerade as grounded context.
/// [P8] Motivating: visible grounding gap; pre: textless DB row; post: no generation call and explicit gap.
#[tokio::test]
async fn retrieval_missing_text_is_visible_without_generation() {
    let directory = fixture();
    let database = directory.path().join("memory.db");
    let store =
        crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
    store
        .store_embedding(
            "legacy:1",
            &vec![1.0; crate::embedding_dim()],
            "offline",
            None,
        )
        .expect("legacy row");
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    for _ in 0..2 {
        let result = content(
            server
                .corpus_query(Parameters(query(Some(&database), true, false)))
                .await,
        );
        assert_eq!(result["missing_passage_text"], 1);
        assert_eq!(result["results"][0]["text_available"], false);
        assert!(
            result["answer_error"]
                .as_str()
                .expect("gap")
                .contains("No usable passage text")
        );
        assert!(
            result["note"]
                .as_str()
                .expect("note")
                .contains("passage_text")
        );
    }
    assert!(port.prompts.lock().expect("prompts").is_empty());
}

/// expect: Invalid provider cardinality/dimension cannot report unstored chunks as embedded.
/// [P8] Motivating: honest completion counts; pre: malformed embedding response; post: all rows failed, no writes.
#[tokio::test]
async fn retrieval_invalid_vectors_do_not_publish() {
    for short in [true, false] {
        let directory = fixture();
        let port = Arc::new(RecordingPort {
            short,
            wrong_dimension: !short,
            ..Default::default()
        });
        let server = server(port);
        let request = embed_request(directory.path(), "memory.db", ORIGINAL);
        let rows = ["corpus:test:1", "corpus:test:2"]
            .map(|entity_ref| json!({"entity_ref":entity_ref,"text":ORIGINAL}).to_string())
            .join("\n");
        std::fs::write(&request.chunks_jsonl, rows).expect("two chunks");
        let summary = content(server.corpus_embed(Parameters(request)).await);
        assert_eq!(summary["total"], 2);
        assert_eq!(summary["embedded"], 0);
        assert_eq!(summary["failed"], 2);
        assert_eq!(summary["failed_entity_refs_complete"], true);
        assert_eq!(
            summary["failed_entity_refs"],
            json!(["corpus:test:1", "corpus:test:2"])
        );
        let store = crate::helpers::open_memory_store(
            &directory.path().join("memory.db").to_string_lossy(),
            PASSPHRASE,
        )
        .expect("DB");
        assert_eq!(store.embedding_count().expect("count"), 0);
    }
}

/// expect: Clear/purge wins over already-started embedding work, with visible cancellation.
/// [P8] Motivating: no resurrection; pre: inference paused; post: no stale store/cache publication.
#[tokio::test]
async fn retrieval_inflight_embed_is_cancelled() {
    for clear in [true, false] {
        let directory = fixture();
        let database = directory.path().join("memory.db");
        let port = Arc::new(RecordingPort {
            pause: Some(Default::default()),
            ..Default::default()
        });
        let server = server(Arc::clone(&port));
        let request = embed_request(directory.path(), "memory.db", ORIGINAL);
        let (entered, release) = port.pause.as_ref().expect("barriers");
        let result = tokio::try_join!(server.corpus_embed(Parameters(request)), async {
            entered.notified().await;
            if clear {
                content(
                    server
                        .corpus_clear_index(Parameters(crate::tools::storage::ClearIndexRequest {}))
                        .await,
                );
            } else {
                purge(&server, &database).await;
            }
            release.notify_one();
            Ok(())
        })
        .expect("embedding and invalidation complete");
        let result = content(Ok(result.0));
        assert_eq!(result["embedded"], 0);
        assert_eq!(result["failed"], 1);
        assert_eq!(result["failed_entity_refs_complete"], true);
        assert_eq!(result["failed_entity_refs"], json!(["corpus:test:1"]));
        assert_eq!(result["cancelled"], 1);
        assert!(
            result["note"]
                .as_str()
                .expect("cancellation reason")
                .contains("cancelled")
        );
        let store =
            crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
        assert_eq!(store.embedding_count().expect("count"), 0);
        release.notify_one();
        let later = content(
            server
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "memory.db",
                    ORIGINAL,
                )))
                .await,
        );
        assert_eq!(
            later["embedded"], 1,
            "writes initiated after invalidation are allowed"
        );
    }
}

/// expect: An in-flight db_path query reads the durable database, not the
/// cache — a cache clear cannot hide a durable row from it, a durable purge
/// is what it sees, and it publishes nothing into the in-memory index.
/// [P8] Motivating: no hidden corpus-sized RAM cache behind a query.
/// pre: cold query blocked at the embedding barrier while clear/purge runs;
/// post: clear leaves the durable row visible and the index empty; purge
/// leaves nothing to read.
#[tokio::test]
async fn retrieval_inflight_db_path_query_reads_durable_state() {
    for clear in [true, false] {
        let directory = fixture();
        let database = directory.path().join("memory.db");
        let writer = server(Arc::new(RecordingPort::default()));
        content(
            writer
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "memory.db",
                    ORIGINAL,
                )))
                .await,
        );
        let port = Arc::new(RecordingPort {
            pause: Some(Default::default()),
            ..Default::default()
        });
        let fresh = server(Arc::clone(&port));
        let (entered, release) = port.pause.as_ref().expect("barriers");
        let result = tokio::try_join!(
            fresh.corpus_query(Parameters(query(Some(&database), true, false))),
            async {
                entered.notified().await;
                if clear {
                    let cleared = content(
                        fresh
                            .corpus_clear_index(Parameters(
                                crate::tools::storage::ClearIndexRequest {},
                            ))
                            .await,
                    );
                    assert_eq!(
                        cleared["cleared"], 0,
                        "the db_path query publishes nothing into the in-memory index"
                    );
                } else {
                    purge(&fresh, &database).await;
                }
                release.notify_one();
                Ok(())
            }
        )
        .expect("query and invalidation complete");
        let result = content(Ok(result.0));
        let store =
            crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
        if clear {
            // Clear is cache-only: the durable row survives and the disk query
            // reads it — its text grounding the generated answer.
            assert_eq!(
                result["results"].as_array().map(Vec::len),
                Some(1),
                "a cache clear cannot hide a durable row from a db_path query"
            );
            assert!(
                result.get("answer").is_some(),
                "the durable passage text grounds the generated answer"
            );
            assert_eq!(
                store.embedding_count().expect("count"),
                1,
                "clear is cache-only"
            );
        } else {
            // Purge is durable and precedes the query's disk read: nothing to read.
            assert_eq!(result["results"], json!([]), "purge precedes the disk read");
            assert!(result.get("answer_error").is_some());
            assert_eq!(store.embedding_count().expect("count"), 0);
        }
        // Either way the query published nothing into the in-memory index —
        // no hidden hydration cache accrues behind db_path queries. Asserted
        // directly via clear()'s returned passage count: a second corpus_query
        // here would deadlock on this fixture's paused embed port (its release
        // barrier is spent), and the count is the same evidence without it.
        assert_eq!(
            fresh.index.clear().expect("index clear"),
            0,
            "a db_path query never publishes into the in-memory index"
        );
    }
}

async fn consolidation_fixture(
    server: &CorpusServer,
    directory: &std::path::Path,
) -> crate::tools::corpus::ConsolidateChunksRequest {
    let mut request = embed_request(directory, "memory.db", ORIGINAL);
    let tagged = directory.join("tagged.jsonl");
    let rows = ["corpus:test:1", "corpus:test:2"].map(|entity_ref| {
        json!({"entity_ref":entity_ref,"classification":{"status":"classified","ontology_protocol":hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL},"source":"river.txt","text":ORIGINAL,"word_count":10,"candidate_terms":["quantity"],"ontology_tags":{"sumo":[hkask_bridge_ontology::sumo::QUANTITY]},"concepts":[hkask_bridge_ontology::sumo::QUANTITY],"salience":0.5}).to_string()
    });
    std::fs::write(&tagged, rows.join("\n")).expect("tagged chunks");
    request.chunks_jsonl = tagged.to_string_lossy().into();
    content(server.corpus_embed(Parameters(request)).await);
    crate::tools::corpus::ConsolidateChunksRequest {
        tagged_jsonl: tagged.to_string_lossy().into(),
        output: directory
            .join("consolidated.jsonl")
            .to_string_lossy()
            .into(),
        db_path: directory.join("memory.db").to_string_lossy().into(),
        prefix: "corpus:test:".into(),
        threshold: 0.75,
        concurrency: 2,
        max_chunks_per_cluster: 10,
        dry_run: false,
    }
}

/// expect: Purge/clear cancels consolidation even between its DB snapshot and inference.
/// [P8] Motivating: a deleted source snapshot cannot be republished as synthesis.
#[tokio::test]
async fn retrieval_consolidation_snapshot_is_protected() {
    for clear in [false, true] {
        let directory = fixture();
        let port = Arc::new(RecordingPort::default());
        let server = server(Arc::clone(&port));
        let request = consolidation_fixture(&server, directory.path()).await;
        let database = directory.path().join("memory.db");
        let pause = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
        let mut service = crate::services::consolidation::ConsolidationService::new(
            Arc::clone(&server.inference_router),
            Arc::clone(&server.index),
            server.webid,
        );
        service.after_snapshot = Some(Arc::clone(&pause));
        let (result, ()) = tokio::join!(
            service.consolidate(crate::services::consolidation::ChunkConsolidationRequest {
                tagged_jsonl: request.tagged_jsonl,
                output: request.output,
                db_path: request.db_path,
                passphrase: PASSPHRASE.into(),
                prefix: request.prefix,
                threshold: request.threshold,
                concurrency: request.concurrency,
                max_chunks_per_cluster: request.max_chunks_per_cluster,
                dry_run: false,
            }),
            async {
                pause.0.notified().await;
                if clear {
                    server.index.clear().expect("clear");
                } else {
                    purge(&server, &database).await;
                }
                pause.1.notify_one();
            }
        );
        let error = result.expect_err("snapshot invalidated before inference");
        assert!(error.message.contains("cancelled"), "{error:?}");
        assert!(port.prompts.lock().expect("prompts").is_empty());
        let fresh = self::server(Arc::new(RecordingPort::default()));
        for current in [&server, &fresh] {
            let output = content(
                current
                    .corpus_query(Parameters(query(Some(&database), false, true)))
                    .await,
            );
            assert!(!output.to_string().contains(SYNTHESIZED), "{output}");
            if !clear {
                assert_eq!(output["results"], json!([]));
            }
        }
    }
}

#[tokio::test]
async fn assertions_remain_chunk_metadata_without_replacing_source_passage() {
    if std::env::var_os("KASK_ASSERTION_ROLE_FIXTURE").is_none() {
        let output = tokio::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", "retrieval_tests::assertions_remain_chunk_metadata_without_replacing_source_passage", "--nocapture"])
            .env("KASK_ASSERTION_ROLE_FIXTURE", "1")
            .env("HKASK_CLASSIFIER_MODEL", "offline")
            .env("HKASK_TEMPLATE_ROOT", std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry"))
            .output().await.expect("isolated assertion fixture");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let directory = fixture();
    let predicate = hkask_bridge_ontology::rdf::TYPE;
    let port = Arc::new(RecordingPort {
        response: Some(json!({"h_mems": [{"subject": "archive", "predicate": predicate, "object": "dataset", "confidence": 0.9}]}).to_string()),
        ..Default::default()
    });
    let server = server(Arc::clone(&port));
    let source_text = "The archive is a dataset.";
    let request = embed_request(directory.path(), "memory.db", source_text);
    let chunks = request.chunks_jsonl.clone();
    content(server.corpus_embed(Parameters(request)).await);
    let database = directory.path().join("memory.db");
    let result = content(server.corpus_extract_assertions(Parameters(serde_json::from_value(json!({
        "chunks_jsonl": chunks, "db_path": database, "owner": "capabilities-reasoner", "concurrency": 1,
    })).expect("assertion request"))).await);
    assert_eq!(result["h_mems_stored"], 1);
    assert_eq!(result["record_role"], "chunk_anchored_assertions");
    assert_eq!(result["embeddings_generated"], 0);
    assert_eq!(port.inputs.lock().expect("embedding calls").len(), 1);
    let store = hkask_memory::MemoryStore::open_existing(
        &database.to_string_lossy(),
        PASSPHRASE,
        crate::embedding_dim(),
    )
    .expect("stored metadata");
    let rows = store
        .query_deduped_untouched("corpus:test:1")
        .expect("chunk metadata");
    assert!(
        rows.iter()
            .any(|row| row.attribute == predicate && row.value["subject"] == "archive")
    );
    assert_eq!(store.embedding_count().expect("vectors"), 1);
    let fresh = self::server(Arc::new(RecordingPort::default()));
    let retrieved = content(
        fresh
            .corpus_query(Parameters(query(Some(&database), false, true)))
            .await,
    );
    assert_eq!(retrieved["total_indexed"], 1);
    assert_eq!(retrieved["results"][0]["text"], source_text);
}

/// expect: Failed replacement names its storage error and preserves prior rows and vectors.
/// [P8] Motivating: a caller can distinguish failed publication from a successful replacement.
#[tokio::test]
async fn retrieval_replacement_error_survives_tool_boundary() {
    for consolidate in [false, true] {
        let directory = fixture();
        let server = server(Arc::new(RecordingPort::default()));
        let request = consolidation_fixture(&server, directory.path()).await;
        let database = directory.path().join("memory.db");
        if consolidate {
            content(
                server
                    .corpus_consolidate_chunks(Parameters(
                        crate::tools::corpus::ConsolidateChunksRequest {
                            tagged_jsonl: request.tagged_jsonl.clone(),
                            output: request.output.clone(),
                            db_path: request.db_path.clone(),
                            prefix: request.prefix.clone(),
                            threshold: request.threshold,
                            concurrency: request.concurrency,
                            max_chunks_per_cluster: request.max_chunks_per_cluster,
                            dry_run: false,
                        },
                    ))
                    .await,
            );
        }
        let memory = hkask_memory::MemoryStore::open_existing(
            &database.to_string_lossy(),
            PASSPHRASE,
            crate::embedding_dim(),
        )
        .expect("memory before failed replacement");
        let rows_before = memory.h_mems_by_entity_prefix("").expect("prior rows");
        let mut embeddings_before = memory.all_embeddings_with_text().expect("prior vectors");
        embeddings_before
            .sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.2.cmp(&right.2)));
        let handle = hkask_storage::Database::open(&database.to_string_lossy(), PASSPHRASE)
            .expect("database");
        let pool = handle.sqlite_pool().expect("pool");
        pool.get().expect("connection").execute_batch("CREATE TRIGGER reject_embedding_insert BEFORE INSERT ON embeddings BEGIN SELECT RAISE(FAIL, 'injected embedding storage failure'); END;").expect("trigger");
        let result = if consolidate {
            server.corpus_consolidate_chunks(Parameters(request)).await
        } else {
            server
                .corpus_embed(Parameters(embed_request(
                    directory.path(),
                    "memory.db",
                    "replacement",
                )))
                .await
        };
        let error = result.expect_err("storage publication failure must reach caller");
        assert!(
            error.message.contains("injected embedding storage failure"),
            "{error:?}"
        );
        assert_eq!(error.kind, hkask_types::McpErrorKind::Internal);
        let mut embeddings_after = memory.all_embeddings_with_text().expect("retained vectors");
        embeddings_after
            .sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.2.cmp(&right.2)));
        assert_eq!(embeddings_after, embeddings_before);
        assert_eq!(
            memory.h_mem_count().expect("retained row count"),
            rows_before.len()
        );
        for previous in rows_before {
            let retained = memory
                .get_by_id(&previous.id)
                .expect("row lookup")
                .expect("prior row retained");
            assert_eq!(retained.value, previous.value);
            assert_eq!(retained.confidence, previous.confidence);
        }
    }
}

/// expect: A late h_mem deletion failure does not keep already-purged embeddings in warm answers.
/// [P8] Motivating: partial failures are visible and known-deleted passages stay invalidated.
/// pre: SQLite trigger rejects h_mem deletion; post: tool error, empty warm cache, embeddings removed.
#[tokio::test]
async fn retrieval_partial_purge_failure_still_invalidates() {
    let directory = fixture();
    let database = directory.path().join("memory.db");
    let server = server(Arc::new(RecordingPort::default()));
    content(
        server
            .corpus_embed(Parameters(embed_request(
                directory.path(),
                "memory.db",
                ORIGINAL,
            )))
            .await,
    );
    let store =
        crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
    store
        .store(hkask_storage::HMem::new(
            "corpus:test:1",
            "assertion",
            json!("source assertion"),
            WebID::new(),
        ))
        .expect("h_mem");
    let database_handle =
        hkask_storage::Database::open(&database.to_string_lossy(), PASSPHRASE).expect("DB handle");
    let pool = database_handle.sqlite_pool().expect("pool");
    pool.get().expect("connection").execute_batch("CREATE TRIGGER reject_h_mem_delete BEFORE DELETE ON hmems BEGIN SELECT RAISE(FAIL, 'injected h_mem deletion failure'); END;").expect("failure trigger");
    let result = server
        .corpus_purge_qa(Parameters(PurgeQaRequest {
            prefix: "corpus:test:".into(),
            db_path: database.to_string_lossy().into(),
        }))
        .await;
    assert!(result.is_err(), "partial purge must not return success");
    assert_eq!(store.embedding_count().expect("count"), 0);
    assert_eq!(
        content(
            server
                .corpus_query(Parameters(query(None, false, true)))
                .await
        )["results"],
        json!([])
    );
}

/// H8: strict Lisp booleans fail before inference and teach the accepted values.
#[tokio::test]
async fn h8_boolean_query_handler_contract() {
    let port = Arc::new(RecordingPort::default());
    let server = server(port.clone());
    for key in ["include-text", "generate-answer"] {
        let mut request = query(None, false, false);
        request.query = format!("(list (list \"query\" \"river\") (list \"{key}\" nil))");
        let error = server
            .corpus_query(Parameters(request))
            .await
            .expect_err("nil is not bool");
        assert!(error.to_string().contains("true/t or false"), "{error}");
    }
    assert!(port.inputs.lock().expect("inputs").is_empty());
}

/// H8: public handler preserves NL/defaults, Lisp overrides, true/t/false and limits.
#[tokio::test]
async fn h8_query_options_and_fixed_budgets() {
    let port = Arc::new(RecordingPort::default());
    let server = server(port.clone());
    let refs: Vec<_> = (0..6).map(|i| format!("h8:{i}")).collect();
    let permit = server
        .index
        .begin_ephemeral("h8", refs.clone())
        .expect("permit");
    let passages: Vec<_> = refs
        .iter()
        .map(|r| (r.clone(), format!("original {r}")))
        .collect();
    server
        .index
        .publish_ephemeral(
            &permit,
            &passages,
            vec![vec![1.0; crate::embedding_dim()]; 6],
            &crate::default_embedding_model().expect("model"),
        )
        .expect("publish");
    let default: QueryRequest =
        serde_json::from_value(json!({"query":"ordinary river"})).expect("request");
    let result = content(server.corpus_query(Parameters(default)).await);
    assert_eq!(result["query"], "ordinary river");
    assert_eq!(result["results"].as_array().expect("results").len(), 5);
    assert!(result["results"][0].get("text").is_none());
    for boolean in ["true", "t", "false"] {
        let mut request = query(None, true, true);
        request.query = format!(
            "  (list (list \"query\" \"normalized river\") (list \"top-k\" 2) (list \"min-score\" 0.5) (list \"include-text\" {boolean}) (list \"generate-answer\" {boolean}))"
        );
        let result = content(server.corpus_query(Parameters(request)).await);
        assert_eq!(result["query"], "normalized river");
        assert_eq!(result["results"].as_array().expect("results").len(), 2);
        assert_eq!(
            result["results"][0].get("text").is_some(),
            boolean != "false"
        );
        assert_eq!(result.get("answer").is_some(), boolean != "false");
    }
    let mut request = query(None, true, true);
    request.query = "(list (list \"query\" \"Lisp defaults\"))".into();
    let result = content(server.corpus_query(Parameters(request)).await);
    assert_eq!(result["results"].as_array().expect("results").len(), 5);
    assert!(result.get("answer").is_none());
    assert!(result["results"][0].get("text").is_none());
    for lisp in [false, true] {
        let mut request = query(None, false, false);
        request.min_score = Some(1.1);
        if lisp {
            request.query = "(list (list \"query\" \"floor\") (list \"min-score\" 1.1))".into();
            request.min_score = Some(0.0);
        }
        let result = content(server.corpus_query(Parameters(request)).await);
        assert!(
            result["results"].as_array().expect("results").is_empty(),
            "caller floor must reach retrieval"
        );
    }
    let calls = port.inputs.lock().expect("inputs").len();
    for (expression, diagnostic) in [
        ("(list (list \"query\" (begin (define walk (lambda (n) (if (= n 0) \"done\" (walk (- n 1))))) (walk 200))))".to_string(), "max_depth (100)"),
        (format!("(list (list \"query\" (begin (define xs (list {})) {} \"done\")))", "1 ".repeat(12_000), "(length xs) ".repeat(10)), "max_steps (100000)"),
    ] {
        let mut request = query(None, false, false);
        request.query = expression;
        let error = server.corpus_query(Parameters(request)).await.expect_err("fixed budget refusal");
        assert!(error.to_string().to_lowercase().contains(diagnostic), "{error}");
    }
    assert_eq!(
        port.inputs.lock().expect("inputs").len(),
        calls,
        "budget refusal precedes inference"
    );
    assert_eq!(port.prompts.lock().expect("prompts").len(), 2);
}

fn h8_held_probe() -> (
    Arc<crate::tools::storage::LispQueryProbe>,
    std::sync::mpsc::Sender<()>,
) {
    let (release, receiver) = std::sync::mpsc::channel();
    let probe = Arc::new(crate::tools::storage::LispQueryProbe::default());
    *probe.release.lock().expect("release") = Some(receiver);
    (probe, release)
}

/// H8: actual handler drop signals a queued/entered real blocking evaluator.
#[tokio::test]
async fn h8_handler_drop_stops_real_worker() {
    let port = Arc::new(RecordingPort::default());
    let server = Arc::new(server(port.clone()));
    let (probe, release) = h8_held_probe();
    let mut request = query(None, false, false);
    request.query = "(list (list \"query\" \"river\"))".into();
    request.lisp_probe = Some(probe.clone());
    let foreground = std::thread::current().id();
    let task = tokio::spawn(async move { server.corpus_query(Parameters(request)).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), probe.entered.notified())
        .await
        .expect("worker entry");
    assert_ne!(
        probe
            .worker_thread
            .lock()
            .expect("thread")
            .expect("entered"),
        foreground
    );
    task.abort();
    assert!(task.await.expect_err("aborted handler").is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(5), probe.dropped.notified())
        .await
        .expect("guard drop");
    let signalled = probe
        .flag
        .lock()
        .expect("flag")
        .as_ref()
        .expect("worker flag")
        .load(std::sync::atomic::Ordering::Acquire);
    release.send(()).expect("release evaluator");
    tokio::time::timeout(std::time::Duration::from_secs(5), probe.exited.notified())
        .await
        .expect("real evaluator exit");
    assert!(signalled, "handler drop must signal the real evaluator");
    assert!(
        probe
            .error
            .lock()
            .expect("error")
            .as_ref()
            .expect("cancel error")
            .contains("cancelled")
    );
    assert!(port.inputs.lock().expect("inputs").is_empty());
}

// Decorate only incoming call extensions to attach per-request observations.
// All IO/handshake/cancellation remains the locked production rmcp transport.
struct H8ObservedTransport<T> {
    inner: T,
    probe: Arc<crate::tools::storage::LispQueryProbe>,
}

impl<T: rmcp::transport::Transport<rmcp::RoleServer>> rmcp::transport::Transport<rmcp::RoleServer>
    for H8ObservedTransport<T>
{
    type Error = T::Error;
    fn send(
        &mut self,
        message: rmcp::service::TxJsonRpcMessage<rmcp::RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(message)
    }
    async fn receive(&mut self) -> Option<rmcp::service::RxJsonRpcMessage<rmcp::RoleServer>> {
        let Some(mut message) = self.inner.receive().await else {
            if let Some(context) = self.probe.context.lock().expect("context").as_ref() {
                let signalled = self
                    .probe
                    .flag
                    .lock()
                    .expect("flag")
                    .as_ref()
                    .expect("flag captured")
                    .load(std::sync::atomic::Ordering::Acquire);
                println!(
                    "H8 at transport EOF: context_cancelled={}, worker_signalled={signalled}",
                    context.ct.is_cancelled()
                );
            }
            return None;
        };
        if let rmcp::model::JsonRpcMessage::Request(request) = &mut message
            && let rmcp::model::ClientRequest::CallToolRequest(call) = &mut request.request
        {
            call.extensions.insert(self.probe.clone());
        }
        Some(message)
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

async fn h8_wire_send<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: Value) {
    use tokio::io::AsyncWriteExt;
    writer
        .write_all(format!("{value}\n").as_bytes())
        .await
        .expect("wire write");
    writer.flush().await.expect("wire flush");
}

/// H8: production rmcp notification cancellation reaches context, drops handler,
/// and signals the real worker; transport EOF is measured independently.
#[tokio::test]
async fn h8_rmcp_request_cancel_and_disconnect_capture() {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, BufReader};
    for disconnect in [false, true] {
        let port = Arc::new(RecordingPort::default());
        let server = server(port.clone());
        let (probe, release) = h8_held_probe();
        let (server_io, client_io) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(server_io);
        let transport = H8ObservedTransport {
            inner: rmcp::transport::async_rw::AsyncRwTransport::<rmcp::RoleServer, _, _>::new(
                read, write,
            ),
            probe: probe.clone(),
        };
        let serving = tokio::spawn(async move {
            server
                .serve(transport)
                .await
                .expect("serve")
                .waiting()
                .await
                .expect("service exit")
        });
        let (read, mut write) = tokio::io::split(client_io);
        let mut read = BufReader::new(read);
        h8_wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"h8-probe","version":"1"}}})).await;
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_line(&mut response),
        )
        .await
        .expect("handshake bound")
        .expect("handshake");
        assert!(
            serde_json::from_str::<Value>(&response)
                .expect("handshake json")
                .get("result")
                .is_some(),
            "{response}"
        );
        h8_wire_send(
            &mut write,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        h8_wire_send(&mut write, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"corpus_query","arguments":{"query":"(list (list \"query\" \"river\"))"}}})).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), probe.entered.notified())
            .await
            .expect("routed real worker");
        let context = probe
            .context
            .lock()
            .expect("context")
            .as_ref()
            .expect("production context")
            .clone();
        assert!(!context.ct.is_cancelled());
        if disconnect {
            drop(read);
            drop(write);
            // Observe EOF while the evaluator is parked, not helper-drop.
            // rmcp drains detached handlers for up to five seconds; waiting's
            // owner drop may subsequently cancel the root token.
            let reason = tokio::time::timeout(std::time::Duration::from_secs(7), serving)
                .await
                .expect("EOF service bound")
                .expect("serve task");
            println!(
                "H8 EOF service reason={reason:?}, context_cancelled={}",
                context.ct.is_cancelled()
            );
        } else {
            h8_wire_send(&mut write, json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2,"reason":"H8 capture"}})).await;
            tokio::time::timeout(std::time::Duration::from_secs(5), context.ct.cancelled())
                .await
                .expect("rmcp context cancellation");
            // Capture handler drop before EOF, so disconnect cannot explain it.
            tokio::time::timeout(std::time::Duration::from_secs(5), probe.dropped.notified())
                .await
                .expect("notification drops handler before EOF");
            assert!(
                probe
                    .flag
                    .lock()
                    .expect("flag")
                    .as_ref()
                    .expect("flag captured")
                    .load(std::sync::atomic::Ordering::Acquire)
            );
            // Restore the permit for the common event check below.
            probe.dropped.notify_one();
            drop(read);
            drop(write);
            tokio::time::timeout(std::time::Duration::from_secs(7), serving)
                .await
                .expect("service bound")
                .expect("serve task");
        }
        let cancelled = context.ct.is_cancelled();
        if cancelled {
            tokio::time::timeout(std::time::Duration::from_secs(5), probe.dropped.notified())
                .await
                .expect("routed handler guard drop");
        }
        let signalled = probe
            .flag
            .lock()
            .expect("flag")
            .as_ref()
            .expect("worker flag")
            .load(std::sync::atomic::Ordering::Acquire);
        println!(
            "H8 disconnect={disconnect}, context_cancelled={cancelled}, worker_signalled={signalled}"
        );
        release.send(()).expect("release real evaluator");
        tokio::time::timeout(std::time::Duration::from_secs(5), probe.exited.notified())
            .await
            .expect("real worker exit");
        if cancelled {
            assert!(signalled);
            assert!(
                probe
                    .error
                    .lock()
                    .expect("error")
                    .as_ref()
                    .expect("cancel error")
                    .contains("cancelled")
            );
            assert!(port.inputs.lock().expect("inputs").is_empty());
        } else {
            assert!(disconnect, "request cancellation must propagate");
            println!(
                "H8 client-disconnect guarantee BLOCKED: no rmcp token propagation; worker exits by fixed budget/normal completion only"
            );
        }
        // Break the observer's retained peer after all captured events.
        probe.context.lock().expect("context").take();
    }
}

#[derive(Default)]
struct H8InferenceLifetime {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    exited: tokio::sync::Notify,
    completed: std::sync::atomic::AtomicBool,
}

struct H8InferenceExit<'a>(&'a H8InferenceLifetime);

impl Drop for H8InferenceExit<'_> {
    fn drop(&mut self) {
        self.0.exited.notify_one();
    }
}

/// H8: after Lisp joins, rmcp cancellation must not newly drop downstream
/// embedding or generation; rmcp response suppression remains its own policy.
#[tokio::test]
async fn h8_rmcp_post_lisp_cancellation_preserves_inference() {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, BufReader};
    for generation in [false, true] {
        let lifetime = Arc::new(H8InferenceLifetime::default());
        let port = Arc::new(RecordingPort {
            embedding_lifetime: (!generation).then(|| lifetime.clone()),
            generation_lifetime: generation.then(|| lifetime.clone()),
            ..Default::default()
        });
        let server = server(port.clone());
        let permit = server
            .index
            .begin_ephemeral("h8-lifecycle", vec!["h8:original".into()])
            .expect("permit");
        server
            .index
            .publish_ephemeral(
                &permit,
                &[("h8:original".into(), ORIGINAL.into())],
                vec![vec![1.0; crate::embedding_dim()]],
                &crate::default_embedding_model().expect("model"),
            )
            .expect("original passage");
        let probe = Arc::new(crate::tools::storage::LispQueryProbe::default());
        let (server_io, client_io) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(server_io);
        let transport = H8ObservedTransport {
            inner: rmcp::transport::async_rw::AsyncRwTransport::<rmcp::RoleServer, _, _>::new(
                read, write,
            ),
            probe: probe.clone(),
        };
        let serving = tokio::spawn(async move {
            server
                .serve(transport)
                .await
                .expect("serve")
                .waiting()
                .await
                .expect("service exit")
        });
        let (read, mut write) = tokio::io::split(client_io);
        let mut read = BufReader::new(read);
        h8_wire_send(&mut write, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"h8-post-lisp","version":"1"}}})).await;
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read.read_line(&mut response),
        )
        .await
        .expect("handshake bound")
        .expect("handshake");
        assert!(
            serde_json::from_str::<Value>(&response)
                .expect("handshake json")
                .get("result")
                .is_some(),
            "{response}"
        );
        h8_wire_send(
            &mut write,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        let expression =
            format!("(list (list \"query\" \"river\") (list \"generate-answer\" {generation}))");
        h8_wire_send(&mut write, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"corpus_query","arguments":{"query":expression}}})).await;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            lifetime.entered.notified(),
        )
        .await
        .expect("functioning inference entered");
        // Provider entry proves the real Lisp worker has joined, not just started.
        tokio::time::timeout(std::time::Duration::from_secs(5), probe.exited.notified())
            .await
            .expect("real Lisp evaluator returned");
        assert!(probe.error.lock().expect("error").is_none());
        let context = probe
            .context
            .lock()
            .expect("context")
            .as_ref()
            .expect("actual context")
            .clone();
        h8_wire_send(&mut write, json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2,"reason":"after Lisp"}})).await;
        tokio::time::timeout(std::time::Duration::from_secs(5), context.ct.cancelled())
            .await
            .expect("actual rmcp cancellation");
        lifetime.release.notify_one();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            lifetime.exited.notified(),
        )
        .await
        .expect("inference completed or dropped");
        let completed = lifetime
            .completed
            .load(std::sync::atomic::Ordering::Acquire);
        // Keep IO open until inference exit: EOF cannot cause the observation.
        drop(read);
        drop(write);
        tokio::time::timeout(std::time::Duration::from_secs(7), serving)
            .await
            .expect("service bound")
            .expect("service task");
        probe.context.lock().expect("context").take();
        println!(
            "H8 post-Lisp generation={generation}, context_cancelled=true, inference_completed={completed}"
        );
        assert!(
            completed,
            "rmcp cancellation after Lisp must not drop downstream inference (generation={generation})"
        );
        assert_eq!(port.inputs.lock().expect("inputs").as_slice(), ["river"]);
        if generation {
            let prompts = port.prompts.lock().expect("prompts");
            assert_eq!(prompts.len(), 1);
            assert!(prompts[0].contains(ORIGINAL));
        }
    }
}
