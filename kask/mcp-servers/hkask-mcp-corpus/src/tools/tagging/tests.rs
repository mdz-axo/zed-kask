//! Approved slice3: public-tool identity and failure accounting, with no live services.
use super::*;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult, WebID};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

struct MockPort {
    response: String,
    panic: bool,
    prompts: Mutex<Vec<String>>,
}

impl InferencePort for MockPort {
    fn generate(
        &self,
        prompt: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.prompts.lock().expect("prompts").push(prompt.into());
        Box::pin(async move {
            assert!(!self.panic, "forced tagging task panic");
            if self.response == "mock inference failure" {
                return Err(InferenceError::Model("mock inference failure".into()));
            }
            Ok(InferenceResult {
                text: self.response.clone(),
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
}

fn server(port: Arc<MockPort>) -> CorpusServer {
    let port: Arc<dyn InferencePort> = port;
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    CorpusServer::new(WebID::new(), None, port, Default::default(), ocr)
}

fn tags(id: &str) -> Value {
    json!({"chunk_ref":id, "dimensions":["what"], "dc_type":"bibo:Document",
        "dc_subject":[], "ontology_tags":{"pko":[id], "sepio":["evidence"], "other":["complexity"]},
        "expertise_level":"analyst"})
}

fn fixture() -> tempfile::TempDir {
    let root = std::env::current_dir()
        .expect("cwd")
        .join("target/tagging-test");
    std::fs::create_dir_all(&root).expect("fixture root");
    tempfile::tempdir_in(root).expect("fixture")
}

async fn run(
    ids: &[&str],
    response: String,
    batch_size: usize,
    panic: bool,
) -> (Value, Vec<Value>, Arc<MockPort>) {
    let directory = fixture();
    let input = directory.path().join("input.jsonl");
    let output = directory.path().join("output.jsonl");
    let rows: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({
        "entity_ref":id, "source":"test.txt", "text":"Evidence explains the method.", "word_count":5
    }).to_string()
        })
        .collect();
    std::fs::write(&input, rows.join("\n")).expect("input");
    let port = Arc::new(MockPort {
        response,
        panic,
        prompts: Mutex::new(Vec::new()),
    });
    let result = server(Arc::clone(&port))
        .corpus_tag_chunks(Parameters(TagChunksRequest {
            chunks_jsonl: input.to_string_lossy().into_owned(),
            output: output.to_string_lossy().into_owned(),
            concurrency: 2,
            tag_batch_size: batch_size,
            dry_run: false,
        }))
        .await
        .expect("tool result");
    let summary = hkask_types::tool_response::unwrap_tool_envelope(
        serde_json::from_str(&result).expect("summary"),
    );
    let rows: Vec<Value> = std::fs::read_to_string(output)
        .expect("output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("row"))
        .collect();
    assert_eq!(rows.len(), ids.len());
    assert_eq!(summary["total_chunks"], ids.len());
    assert_eq!(
        summary["tagged"].as_u64().expect("tagged") + summary["failed"].as_u64().expect("failed"),
        ids.len() as u64
    );
    assert_eq!(
        summary["tagged"],
        rows.iter()
            .filter(|row| row["classification"]["status"] == "classified")
            .count()
    );
    assert_eq!(
        summary["failed"],
        rows.iter()
            .filter(|row| row["classification"]["status"] == "failed")
            .count()
    );
    for (row, id) in rows.iter().zip(ids) {
        let typed: TaggedChunk =
            serde_json::from_value(row.clone()).expect("canonical saved schema");
        assert_eq!(serde_json::to_value(typed).expect("round trip"), *row);
        assert_eq!(row["entity_ref"], *id);
        assert!(row["ontology"]["method_signals"].is_object());
        if row["classification"]["status"] == "failed" {
            assert!(
                row["classification"]["reason"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())
            );
            assert_eq!(row["ontology_tags"], json!({}));
            assert_eq!(row["concepts"], json!([]));
        } else {
            assert_eq!(row["classification"]["status"], "classified");
            assert_eq!(row["ontology_tags"]["pko"], json!([id]));
            assert_eq!(row["ontology_tags"]["sepio"], json!(["evidence"]));
            assert_eq!(row["ontology_tags"]["other"], json!(["complexity"]));
        }
    }
    (summary, rows, port)
}

/// expect: "Each unique input has exactly one ID-correlated, visible terminal outcome, including task panic."
#[tokio::test]
async fn public_tagging_identity_contract() {
    // Set process-wide model/template configuration only in an isolated test process.
    if std::env::var_os("KASK_SLICE3_TAG_TEST").is_none() {
        let registry = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry");
        let output =
            tokio::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "tools::tagging::ops::tests::public_tagging_identity_contract",
                    "--nocapture",
                ])
                .env("KASK_SLICE3_TAG_TEST", "1")
                .env("HKASK_CLASSIFIER_MODEL", "offline")
                .env("HKASK_TEMPLATE_ROOT", registry)
                .output()
                .await
                .expect("isolated test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child must actually run the test"
        );
        return;
    }

    let (summary, _, port) = run(
        &["a", "b"],
        json!([tags("b"), tags("a")]).to_string(),
        2,
        false,
    )
    .await;
    assert_eq!(summary["tagged"], 2);
    let prompt = port.prompts.lock().expect("prompts").join("\n");
    assert!(prompt.contains("chunk_ref"));
    for response in [tags("a"), json!([tags("a")])] {
        let (summary, _, _) = run(&["a"], response.to_string(), 1, false).await;
        assert_eq!(summary["tagged"], 1);
    }

    let (summary, rows, _) = run(&["a", "b", "c"], tags("a").to_string(), 1, false).await;
    assert_eq!(summary["tagged"], 1);
    assert_eq!(summary["failed"], 2);
    assert_eq!(rows[0]["classification"]["status"], "classified");
    assert!(
        rows[1]["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("unknown")
    );

    for response in [
        json!({"dimensions":["what"]}),
        json!([{"dimensions":["what"]}]),
        json!({"chunk_ref":"a"}),
    ] {
        let (summary, _, _) = run(&["a"], response.to_string(), 1, false).await;
        assert_eq!(
            summary["failed"], 1,
            "missing identity or tag fields must not be classified"
        );
    }

    // Whole-batch identity contract: short, long, unknown, duplicate, missing ID,
    // and singleton object for a multi-input request must never be positional matches.
    for (response, reason) in [
        (json!([tags("a")]), "omitted"),
        (json!([tags("a"), tags("b"), tags("extra")]), "unknown"),
        (json!([tags("a"), tags("unknown")]), "unknown"),
        (json!([tags("a"), tags("a")]), "duplicate"),
        (json!([{"dimensions":["what"]}, tags("b")]), "chunk_ref"),
        (tags("a"), "array"),
    ] {
        let (summary, rows, _) = run(&["a", "b"], response.to_string(), 2, false).await;
        assert_eq!(summary["failed"], 2, "{response}");
        for row in rows {
            assert!(
                row["classification"]["reason"]
                    .as_str()
                    .expect("reason")
                    .contains(reason),
                "{row}"
            );
        }
    }
    let (summary, rows, _) = run(&["a", "b", "c"], String::new(), 2, true).await;
    assert_eq!(summary["failed"], 3);
    assert!(rows.iter().all(|row| {
        row["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("join")
    }));

    let (summary, rows, port) = run(&["a", "b"], "mock inference failure".into(), 2, false).await;
    assert_eq!(summary["failed"], 2);
    assert!(rows.iter().all(|row| {
        row["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("mock inference failure")
    }));
    assert_eq!(
        port.prompts.lock().expect("prompts").len(),
        MAX_RETRIES as usize
    );

    // Multibyte strings cross both former byte-slice panic boundaries.
    let (summary, rows, _) = run(&["a"], "界".repeat(200), 1, false).await;
    assert_eq!(summary["failed"], 1);
    assert!(
        rows[0]["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("JSON")
    );
    let mut unicode = tags("a");
    unicode["dc_subject"] = json!(["界".repeat(50)]);
    unicode["ontology_tags"]["custom"] = json!(["界".repeat(50)]);
    let (summary, rows, _) = run(&["a"], json!([unicode]).to_string(), 1, false).await;
    assert_eq!(summary["tagged"], 1);
    for concept in [
        &rows[0]["dc_subject"][0],
        &rows[0]["ontology_tags"]["custom"][0],
    ] {
        let concept = concept.as_str().expect("concept");
        assert!(!concept.is_empty() && concept.len() <= 80);
        assert_eq!(concept, "界".repeat(26));
    }
}

/// expect: "A missing canonical template fails visibly without substituting an inline prompt."
#[tokio::test]
async fn missing_template_never_infers() {
    if std::env::var_os("KASK_SLICE3_MISSING_TEMPLATE").is_none() {
        let directory = fixture();
        let output =
            tokio::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "tools::tagging::ops::tests::missing_template_never_infers",
                    "--nocapture",
                ])
                .env("KASK_SLICE3_MISSING_TEMPLATE", "1")
                .env("HKASK_CLASSIFIER_MODEL", "offline")
                .env("HKASK_TEMPLATE_ROOT", directory.path())
                .output()
                .await
                .expect("isolated test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    let (summary, rows, port) = run(&["a"], tags("a").to_string(), 1, false).await;
    assert_eq!(summary["failed"], 1);
    assert!(port.prompts.lock().expect("prompts").is_empty());
    assert!(
        rows[0]["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("template")
    );
}

/// expect: "Persisted tags without classification status are rejected, not upgraded through compatibility defaults."
#[test]
fn missing_classification_status_is_rejected() {
    let row = json!({"entity_ref":"a", "source":"s", "text":"t"});
    let error =
        serde_json::from_value::<TaggedChunk>(row.clone()).expect_err("missing classification");
    assert!(error.to_string().contains("classification"));
    for classification in [
        json!({}),
        json!({"status":"failed"}),
        json!({"status":"legacy"}),
    ] {
        let mut row = row.clone();
        row["classification"] = classification;
        assert!(serde_json::from_value::<TaggedChunk>(row).is_err());
    }
}

#[test]
fn utf8_preview_respects_byte_budget() {
    let text = "界".repeat(200);
    assert_eq!(utf8_prefix(&text, 500), "界".repeat(166));
    assert_eq!(utf8_prefix("é", 1), "");
    assert_eq!(utf8_prefix("", 500), "");
    assert_eq!(utf8_prefix("short", 500), "short");
}

/// expect: "Duplicate or blank input references are rejected before inference, including dry-run."
#[tokio::test]
async fn invalid_input_refs_never_infer() {
    for ids in [["a", "a"], ["a", "  "], ["a", ""]] {
        for dry_run in [false, true] {
            let directory = fixture();
            let input = directory.path().join("input.jsonl");
            let output = directory.path().join("output.jsonl");
            let lines: Vec<_> = ids
                .iter()
                .map(|id| json!({"entity_ref":id, "source":"s", "text":"t"}).to_string())
                .collect();
            std::fs::write(&input, lines.join("\n")).expect("input");
            let port = Arc::new(MockPort {
                response: String::new(),
                panic: false,
                prompts: Mutex::new(Vec::new()),
            });
            let error = server(Arc::clone(&port))
                .corpus_tag_chunks(Parameters(TagChunksRequest {
                    chunks_jsonl: input.to_string_lossy().into_owned(),
                    output: output.to_string_lossy().into_owned(),
                    concurrency: 1,
                    tag_batch_size: 1,
                    dry_run,
                }))
                .await
                .expect_err("invalid input must fail");
            assert!(error.to_string().contains("entity_ref"), "{error}");
            assert!(port.prompts.lock().expect("prompts").is_empty());
            assert!(!output.exists());
        }
    }
}
