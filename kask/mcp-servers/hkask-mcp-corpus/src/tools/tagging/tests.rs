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
                cost_usd: Some(0.01),
            })
        })
    }
}

fn server(port: Arc<MockPort>) -> CorpusServer {
    let port: Arc<dyn InferencePort> = port;
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    CorpusServer::new(WebID::new(), None, port, Default::default(), ocr)
}

fn tags(correlation_id: &str) -> Value {
    json!([
        correlation_id,
        ["who", "why"],
        ["procedure", "assertion", "complexity"]
    ])
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
            assert_eq!(row["candidate_terms"], json!([]));
            assert_eq!(row["ontology_tags"], json!({}));
            assert_eq!(row["concepts"], json!([]));
        } else {
            assert_eq!(row["classification"]["status"], "classified");
            assert_eq!(
                row["classification"]["ontology_protocol"],
                TERM_RESOLUTION_PROTOCOL
            );
            let candidate_terms = row["candidate_terms"].as_array().expect("candidate terms");
            assert!(
                candidate_terms.starts_with(
                    json!(["procedure", "assertion", "complexity"])
                        .as_array()
                        .expect("expected terms")
                ),
                "{row}"
            );
            assert_eq!(
                row["ontology_tags"]["pko"],
                json!([hkask_bridge_ontology::pko::PROCEDURE])
            );
            assert_eq!(
                row["ontology_tags"]["sepio"],
                json!([hkask_bridge_ontology::sepio::ASSERTION])
            );
            assert_eq!(row["ontology_tags"]["core"], json!(["5w1h_core"]));
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

    let long_ref = "style:test:utf8-4170706c6965642d436f72706f726174652d46696e616e63652d612d55736572732d4d616e75616c:1777";
    let (summary, rows, port) = run(
        &[long_ref, "b"],
        json!([tags("item-1"), tags("item-0")]).to_string(),
        2,
        false,
    )
    .await;
    assert_eq!(summary["tagged"], 2);
    assert_eq!(summary["planned_batches"], 1);
    assert_eq!(summary["provider_responses"], 1);
    assert_eq!(summary["successful_response_usage"]["total_tokens"], 2);
    assert_eq!(summary["reported_cost_usd"], 0.01);
    assert_eq!(summary["cost_reporting_complete"], true);
    assert_eq!(rows[0]["entity_ref"], long_ref);
    let prompt = port.prompts.lock().expect("prompts").join("\n");
    assert!(prompt.contains("Passage 1 (item-0)"));
    assert!(!prompt.contains(long_ref));
    assert!(!prompt.contains("source:"));
    let (summary, _, _) = run(&["a"], json!([tags("item-0")]).to_string(), 1, false).await;
    assert_eq!(summary["tagged"], 1);

    let (summary, rows, _) = run(
        &["a", "b", "c"],
        json!([tags("item-0")]).to_string(),
        1,
        false,
    )
    .await;
    assert_eq!(summary["tagged"], 3);
    assert_eq!(summary["failed"], 0);
    assert!(
        rows.iter()
            .all(|row| row["classification"]["status"] == "classified")
    );

    for response in [
        json!({"dimensions":["what"]}),
        json!([{"dimensions":["what"]}]),
        json!({"correlation_id":"item-0"}),
        json!({
            "correlation_id":"item-0",
            "dimensions":["what"],
            "dc_type":hkask_bridge_ontology::dc_bibo::DOCUMENT,
            "dc_subject":[],
            "ontology_tags":{"fibo":["corporation"]},
            "expertise_level":"analyst"
        }),
    ] {
        let (summary, _, _) = run(&["a"], response.to_string(), 1, false).await;
        assert_eq!(
            summary["failed"], 1,
            "missing identity or tag fields must not be classified"
        );
    }

    // Whole-batch identity contract: unknown, duplicate, missing correlation ID,
    // and singleton object for a multi-input request must never be positional matches.
    for (response, reason) in [
        (json!([tags("item-0")]), "omitted"),
        (
            json!([tags("item-0"), tags("item-1"), tags("item-extra")]),
            "unknown",
        ),
        (json!([tags("item-0"), tags("item-unknown")]), "unknown"),
        (json!([tags("item-0"), tags("item-0")]), "duplicate"),
        (
            json!([{"dimensions":["what"]}, tags("item-1")]),
            "invalid tagging JSON entry",
        ),
        (tags("item-0"), "invalid tagging JSON entry"),
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
    assert_eq!(summary["provider_responses"], 0);
    assert_eq!(summary["reported_cost_usd"], Value::Null);
    assert_eq!(summary["cost_reporting_complete"], false);
    assert_eq!(port.prompts.lock().expect("prompts").len(), 1);

    // Multibyte strings cross both former byte-slice panic boundaries.
    let (summary, rows, _) = run(&["a"], "界".repeat(200), 1, false).await;
    assert_eq!(summary["failed"], 1);
    assert!(
        rows[0]["classification"]["reason"]
            .as_str()
            .expect("reason")
            .contains("JSON")
    );
    let mut unicode = tags("item-0");
    unicode[2] = json!(["procedure", "assertion", "complexity", "界".repeat(50)]);
    let (summary, rows, _) = run(&["a"], json!([unicode]).to_string(), 1, false).await;
    assert_eq!(summary["tagged"], 1);
    for term in [&rows[0]["dc_subject"][3], &rows[0]["candidate_terms"][3]] {
        let term = term.as_str().expect("term");
        assert!(!term.is_empty() && term.len() <= 80);
        assert_eq!(term, "界".repeat(26));
    }
    assert_eq!(rows[0]["ontology_tags"]["core"], json!(["5w1h_core"]));
}

#[test]
fn salience_uses_descriptive_candidates_when_anchors_are_coarse() {
    let tagged = [
        vec!["alpha", "bridge"],
        vec!["bridge", "gamma"],
        vec!["gamma", "delta"],
        vec!["isolated"],
    ]
    .map(|candidate_terms| TaggedChunk {
        candidate_terms: candidate_terms.into_iter().map(String::from).collect(),
        concepts: vec!["5w1h_core".to_string()],
        ..Default::default()
    });

    let scores = compute_salience(&tagged);
    assert!(scores[0] > 0.0);
    assert!(scores[1] > 0.0);
    assert!(scores[2] > 0.0);
    assert_eq!(scores[3], 0.0);
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
    let (summary, rows, port) = run(&["a"], tags("item-0").to_string(), 1, false).await;
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
