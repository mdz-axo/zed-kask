//! Offline retrieval contracts exercised through the real corpus tools.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatToolDefinition, EmbeddingGenerationError, InferenceError, InferencePort, InferenceResult,
    WebID,
};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

use crate::CorpusServer;
use crate::tools::semantic::EmbedRequest;
use crate::tools::storage::{PurgeQaRequest, QueryRequest};

const PASSPHRASE: &str = "retrieval-test-passphrase";
const ORIGINAL: &str = "The archive records the river flooding in spring.";
const SYNTHESIZED: &str = "The river floods each spring and replenishes the fertile valley.";

#[derive(Default)]
struct RecordingPort {
    prompts: Mutex<Vec<String>>,
    inputs: Mutex<Vec<String>>,
    pause: Option<(tokio::sync::Notify, tokio::sync::Notify)>,
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
            Ok(InferenceResult {
                text: response,
                model: "offline".into(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
                finish_reason: "stop".into(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            })
        })
    }

    fn embed(
        &self,
        _: &str,
        texts: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbeddingGenerationError>> + Send + '_>>
    {
        self.inputs.lock().expect("inputs").extend_from_slice(texts);
        let count = texts.len();
        Box::pin(async move {
            if let Some((entered, release)) = &self.pause {
                entered.notify_one();
                release.notified().await;
            }
            Ok((0..if self.short {
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
                .collect())
        })
    }
}

fn server(port: Arc<RecordingPort>) -> CorpusServer {
    let port: Arc<dyn InferencePort> = port;
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    CorpusServer::new(WebID::new(), None, port, Default::default(), ocr)
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
        json!([{"correlation_id":"item-0", "dimensions":["what"], "dc_type":"bibo:Document", "dc_subject":[], "ontology_tags":{}, "expertise_level":"analyst", "method_signals":{"word_count":999}}])
            .to_string(),
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
        assert_eq!(row["classification"]["status"], if response.starts_with('[') { "classified" } else { "failed" });
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
    let input = directory.path().join("style.jsonl");
    std::fs::write(
        &input,
        [("style:test:1", paratactic), ("style:test:2", hypotactic)]
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
                passphrase: PASSPHRASE.into(),
                model: Some("offline".into()),
                batch_size: 10,
            }))
            .await,
    );
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

/// expect: "A stored style centroid makes corpus_compose's validation run
/// for real — compose never silently runs unvalidated." [P3]
#[tokio::test]
async fn centroid_tool_stores_style_centroid_and_unblocks_compose_validation() {
    let directory = fixture();
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    let input = directory.path().join("style.jsonl");
    std::fs::write(
        &input,
        [
            ("style:jb-test:1", "The water glittered."),
            ("style:jb-test:2", "The boat drifted."),
        ]
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
                passphrase: PASSPHRASE.into(),
                model: Some("offline".into()),
                batch_size: 10,
            }))
            .await,
    );

    let centroid = content(
        server
            .corpus_centroid(Parameters(crate::tools::compose_tools::CentroidRequest {
                author: "jb-test".into(),
                db_path: database.to_string_lossy().into(),
                passphrase: PASSPHRASE.into(),
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
                passphrase: PASSPHRASE.into(),
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
                passphrase: PASSPHRASE.into(),
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
        inference_ctx: crate::inference_svc::InferenceContext::from_parts(Some(port), "offline"),
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
        passphrase: PASSPHRASE.into(),
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
        passphrase: PASSPHRASE.into(),
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
                passphrase: PASSPHRASE.into(),
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
            passphrase: PASSPHRASE.into(),
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
        passphrase: PASSPHRASE.into(),
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
        inference_ctx: crate::inference_svc::InferenceContext::from_parts(Some(port), "offline"),
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
        passphrase: PASSPHRASE.into(),
        model: Some("offline".into()),
        batch_size: 10,
    }
}

fn query(database: Option<&std::path::Path>, answer: bool, include_text: bool) -> QueryRequest {
    QueryRequest {
        query: "When does the river flood?".into(),
        top_k: Some(50),
        generate_answer: Some(answer),
        include_text: Some(include_text),
        min_score: Some(0.5),
        db_path: database.map(|path| path.to_string_lossy().into()),
        passphrase: Some(PASSPHRASE.into()),
    }
}

async fn purge(server: &CorpusServer, database: &std::path::Path) -> Value {
    content(
        server
            .corpus_purge_qa(Parameters(PurgeQaRequest {
                prefix: "corpus:test:".into(),
                db_path: database.to_string_lossy().into(),
                passphrase: PASSPHRASE.into(),
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
    let chunks = ["corpus:test:1", "corpus:test:2"].map(|entity_ref| json!({"entity_ref":entity_ref, "classification":{"status":"classified"}, "source":"river.txt", "text":ORIGINAL, "word_count":10, "concepts":[], "salience":0.5}));
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
        passphrase: PASSPHRASE.into(),
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
    assert!(
        port.inputs
            .lock()
            .expect("inputs")
            .iter()
            .any(|input| input == &format!("[unclassified] {SYNTHESIZED}"))
    );
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
        json!({"entity_ref":"corpus:test:1", "ontology_tags":{"golem":["river"]}}).to_string(),
    )
    .expect("tags");
    request.tagged_jsonl = Some(tags.to_string_lossy().into());
    content(server.corpus_embed(Parameters(request)).await);
    assert!(
        port.inputs
            .lock()
            .expect("inputs")
            .iter()
            .any(|input| input == &format!("[golem: river] {ORIGINAL}"))
    );
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
    let mut request = query(
        Some(&directory.path().join("does-not-exist.db")),
        false,
        true,
    );
    request.passphrase = Some("wrong but unused".into());
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

/// expect: A query loading a cold DB cannot restore a cleared/purged cache after its inference finishes.
/// [P8] Motivating: no stale hydration; pre: cold query blocked at embedding barrier; post: invalidation wins.
#[tokio::test]
async fn retrieval_inflight_hydration_cannot_republish() {
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
                        cleared["cleared"], 1,
                        "hydration linearized before inference"
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
        assert_eq!(result["results"], json!([]));
        assert!(result.get("answer_error").is_some());
        assert!(port.prompts.lock().expect("prompts").is_empty());
        let store =
            crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
        assert_eq!(
            store.embedding_count().expect("count"),
            usize::from(clear),
            "clear is cache-only"
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
        json!({"entity_ref":entity_ref,"classification":{"status":"classified"},"source":"river.txt","text":ORIGINAL,"word_count":10,"concepts":[],"salience":0.5}).to_string()
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
        passphrase: PASSPHRASE.into(),
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
                passphrase: request.passphrase,
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

/// expect: Failed replacement names its storage error and potential loss of the prior embedding.
/// [P8] Motivating: a failed-row count alone cannot disclose destructive partial application.
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
                            passphrase: request.passphrase.clone(),
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
        assert!(
            error
                .message
                .contains("prior embedding may have been removed"),
            "{error:?}"
        );
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
            passphrase: PASSPHRASE.into(),
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
