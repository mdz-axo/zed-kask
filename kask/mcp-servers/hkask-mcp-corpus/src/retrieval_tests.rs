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
        Box::pin(async {
            Ok(InferenceResult {
                text: SYNTHESIZED.into(),
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
                            1
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
    let pipeline = Arc::new(crate::ocr::PipelineExecutor::new(Arc::clone(&ocr)));
    CorpusServer::new(
        WebID::new(),
        None,
        port,
        crate::ocr::ThresholdConfig::default(),
        Mutex::new(Vec::new()),
        Default::default(),
        ocr,
        pipeline,
    )
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
    let chunks = ["corpus:test:1", "corpus:test:2"].map(|entity_ref| json!({"entity_ref":entity_ref, "source":"river.txt", "text":ORIGINAL, "word_count":10, "concepts":[], "salience":0.5}));
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
    let request = crate::tools::corpus::ConsolidateChunksRequest {
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
    let summary = content(server.corpus_consolidate_chunks(Parameters(request)).await);
    assert_eq!(summary["reembedded"], 1, "{summary}");
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
    content(server.corpus_embed(Parameters(embed_request(directory.path(), "memory.db", "old text"))).await);
    let mut request = embed_request(directory.path(), "memory.db", ORIGINAL);
    let tags = directory.path().join("tags.jsonl");
    std::fs::write(&tags, json!({"entity_ref":"corpus:test:1", "ontology_tags":{"golem":["river"]}}).to_string()).expect("tags");
    request.tagged_jsonl = Some(tags.to_string_lossy().into());
    content(server.corpus_embed(Parameters(request)).await);
    assert!(port.inputs.lock().expect("inputs").iter().any(|input| input == &format!("[golem: river] {ORIGINAL}")));
    let fresh = self::server(Arc::clone(&port));
    for current in [&server, &fresh] {
        let result = content(current.corpus_query(Parameters(query(Some(&directory.path().join("memory.db")), false, true))).await);
        assert_eq!(result["total_indexed"], 1);
        assert_eq!(result["results"][0]["text"], ORIGINAL);
    }
    let store = crate::helpers::open_memory_store(&directory.path().join("memory.db").to_string_lossy(), PASSPHRASE).expect("DB");
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
    request.db_path = database.strip_prefix(std::env::current_dir().expect("cwd")).expect("relative DB").to_string_lossy().into();
    content(server.corpus_embed(Parameters(request)).await);
    content(server.corpus_embed(Parameters(embed_request(directory.path(), "second.db", "other DB text"))).await);
    crate::services::convert::ConvertService::from_corpus(&server).index_passages(&[("corpus:test:1".into(), "ephemeral text".into())], "river.txt").await.expect("ephemeral index");
    let mut request = query(Some(&directory.path().join("does-not-exist.db")), false, true);
    request.passphrase = Some("wrong but unused".into());
    assert_eq!(content(server.corpus_query(Parameters(request)).await)["total_indexed"], 3, "nonempty index must not consult db_path");
    assert!(!directory.path().join("does-not-exist.db").exists());
    purge(&server, &database).await;
    let retained = content(server.corpus_query(Parameters(query(None, false, true))).await);
    assert_eq!(retained["total_indexed"], 2);
    let texts: Vec<_> = retained["results"].as_array().expect("results").iter().map(|entry| entry["text"].as_str().expect("text")).collect();
    assert!(texts.contains(&"other DB text") && texts.contains(&"ephemeral text"));
    #[cfg(unix)] {
        let alias = directory.path().join("alias.db");
        std::os::unix::fs::symlink(&database, &alias).expect("DB symlink");
        content(server.corpus_embed(Parameters(embed_request(directory.path(), "first.db", ORIGINAL))).await);
        purge(&server, &alias).await;
        assert_eq!(content(server.corpus_query(Parameters(query(None, false, true))).await)["total_indexed"], 2);
    }
}

/// expect: A legacy vector without stored passage text cannot masquerade as grounded context.
/// [P8] Motivating: visible grounding gap; pre: textless DB row; post: no generation call and explicit gap.
#[tokio::test]
async fn retrieval_missing_text_is_visible_without_generation() {
    let directory = fixture();
    let database = directory.path().join("memory.db");
    let store = crate::helpers::open_memory_store(&database.to_string_lossy(), PASSPHRASE).expect("DB");
    store.store_embedding("legacy:1", &vec![1.0; crate::embedding_dim()], "offline", None).expect("legacy row");
    let port = Arc::new(RecordingPort::default());
    let server = server(Arc::clone(&port));
    for _ in 0..2 {
        let result = content(server.corpus_query(Parameters(query(Some(&database), true, false))).await);
        assert_eq!(result["missing_passage_text"], 1);
        assert_eq!(result["results"][0]["text_available"], false);
        assert!(result["answer_error"].as_str().expect("gap").contains("No usable passage text"));
        assert!(result["note"].as_str().expect("note").contains("passage_text"));
    }
    assert!(port.prompts.lock().expect("prompts").is_empty());
}

/// expect: Invalid provider cardinality/dimension cannot report unstored chunks as embedded.
/// [P8] Motivating: honest completion counts; pre: malformed embedding response; post: all rows failed, no writes.
#[tokio::test]
async fn retrieval_invalid_vectors_do_not_publish() {
    for short in [true, false] {
        let directory = fixture();
        let port = Arc::new(RecordingPort { short, wrong_dimension: !short, ..Default::default() });
        let server = server(port);
        let summary = content(server.corpus_embed(Parameters(embed_request(directory.path(), "memory.db", ORIGINAL))).await);
        assert_eq!(summary["total"], 1);
        assert_eq!(summary["embedded"], 0);
        assert_eq!(summary["failed"], 1);
        let store = crate::helpers::open_memory_store(&directory.path().join("memory.db").to_string_lossy(), PASSPHRASE).expect("DB");
        assert_eq!(store.embedding_count().expect("count"), 0);
    }
}
