//! Brooks ingestion requirements exercised at the public tool boundary, offline.
use super::IngestQaRequest;
use crate::CorpusServer;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

const PASSPHRASE: &str = "ingest-test-passphrase";
const ANSWERS: [&str; 4] = ["Thirty", "72 hours", "A collision", "exp(-E/kB T)"];

struct NoInference;
impl InferencePort for NoInference {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("ingestion must not invoke inference")
    }
}

fn server() -> CorpusServer {
    let port: Arc<dyn InferencePort> = Arc::new(NoInference);
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    CorpusServer::new(
        hkask_types::WebID::new(),
        None,
        port,
        Default::default(),
        ocr,
    )
}

fn fixture() -> anyhow::Result<tempfile::TempDir> {
    // Same contained, RAII-isolated fixture pattern as retrieval_tests.
    let directory = std::env::current_dir()?.join("target/ingest-test");
    std::fs::create_dir_all(&directory)?;
    Ok(tempfile::tempdir_in(directory)?)
}

fn request(directory: &Path, dry_run: bool) -> IngestQaRequest {
    IngestQaRequest {
        generated_jsonl: directory.join("generated.jsonl").to_string_lossy().into(),
        output: directory.join("training.jsonl").to_string_lossy().into(),
        db_path: directory.join("memory.db").to_string_lossy().into(),
        passphrase: PASSPHRASE.into(),
        dry_run,
        dataset: "brooks-test".into(),
        owner: "brooks-test-owner".into(),
    }
}

fn flat(index: usize, answer: &str) -> Value {
    json!({"instruction":format!("Question {index}?"), "output":answer,
        "qa_type":"factual", "type":if index == 3 { "recall" } else { "factual" }, "source":"brooks.txt",
        "chunk_ref":format!("corpus:brooks:{index}"), "evidence_quotes":[answer],
        "concepts":["test concept"], "difficulty":2})
}

fn content(result: String) -> anyhow::Result<Value> {
    Ok(hkask_types::tool_response::unwrap_tool_envelope(
        serde_json::from_str(&result)?,
    ))
}

fn reconciles(value: &Value) {
    let count = |key: &str| value[key].as_u64().expect(key);
    assert_eq!(
        count("total_nonblank_rows"),
        count("generator_errors") + count("malformed") + count("parsed")
    );
    assert_eq!(
        count("parsed"),
        count("filter_drops") + count("duplicates") + count("retained")
    );
    assert_eq!(count("filtered"), count("duplicates") + count("retained"));
    assert_eq!(count("deduped"), count("retained"));
    assert_eq!(count("stored_h_mems"), count("stored"));
    if value["dry_run"] == true {
        assert_eq!(count("stored") + count("failed"), 0);
    } else {
        assert_eq!(count("retained"), count("stored") + count("failed"));
    }
}

/// expect: Concise valid QA survives, first case-insensitive duplicate wins, metadata round-trips, and every nonblank row reconciles.
#[tokio::test]
async fn ingest_concise_metadata_counts_and_dry_run() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let mut rows = Vec::new();
    for (index, answer) in ANSWERS.iter().enumerate() {
        let row = flat(index, answer);
        rows.push(if index % 2 == 0 {
            json!({"response":row, "source":row["source"], "chunk_ref":row["chunk_ref"], "qa_type":row["qa_type"]}).to_string()
        } else {
            row.to_string()
        });
    }
    let mut duplicate = flat(0, "wrong survivor");
    duplicate["instruction"] = json!("QUESTION 0?");
    rows.push(duplicate.to_string());
    for field in ["instruction", "output", "qa_type", "source", "chunk_ref"] {
        let mut missing = flat(5, "answer");
        missing.as_object_mut().expect("object").remove(field);
        rows.push(missing.to_string());
        let mut blank = flat(6, "answer");
        blank[field] = json!(" \t\n ");
        rows.push(blank.to_string());
    }
    rows.extend(["not JSON".into(), "[]".into(), r#"{"response":"not an object"}"#.into(),
        json!({"prompt_id":"failed", "source":"brooks.txt", "chunk_ref":"corpus:brooks:failed", "error":"generator unavailable"}).to_string()]);
    std::fs::write(
        directory.path().join("generated.jsonl"),
        format!("\n  \t\n{}\n\n", rows.join("\n")),
    )?;
    let dry = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), true)))
            .await?,
    )?;
    reconciles(&dry);
    for (key, expected) in [
        ("total_nonblank_rows", 19),
        ("generator_errors", 1),
        ("malformed", 3),
        ("parsed", 15),
        ("filter_drops", 10),
        ("duplicates", 1),
        ("retained", 4),
    ] {
        assert_eq!(dry[key], expected, "{key}");
    }
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());

    let written = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), false)))
            .await?,
    )?;
    reconciles(&written);
    assert_eq!(written["stored"], 4);
    assert_eq!(written["failed"], 0);
    assert_eq!(written["status"], "complete");
    for key in [
        "total_nonblank_rows",
        "parsed",
        "filtered",
        "deduped",
        "generator_errors",
        "malformed",
        "filter_drops",
        "duplicates",
        "retained",
    ] {
        assert_eq!(dry[key], written[key], "{key}");
    }
    let output = std::fs::read_to_string(directory.path().join("training.jsonl"))?;
    let training: Vec<Value> = output
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    assert_eq!(training.len(), ANSWERS.len());
    let store =
        crate::helpers::open_memory_store(&request(directory.path(), false).db_path, PASSPHRASE)?;
    assert_eq!(store.h_mem_count()?, 4);
    for (index, (row, answer)) in training.iter().zip(ANSWERS).enumerate() {
        let expected = flat(index, answer);
        for key in [
            "instruction",
            "output",
            "qa_type",
            "type",
            "source",
            "chunk_ref",
            "evidence_quotes",
            "concepts",
            "difficulty",
        ] {
            assert_eq!(row[key], expected[key], "{key}");
        }
        assert_eq!(row["input"], "");
        let records = store
            .query_deduped_untouched(&format!("training:qa:brooks-test:brooks.txt:{index}"))?;
        assert_eq!(records.len(), 1);
        let record = records.first().expect("stored QA");
        assert_eq!(record.attribute, "training_qa_pair");
        assert_eq!(record.value["question"], expected["instruction"]);
        assert_eq!(record.value["answer"], answer);
        assert_eq!(record.value["bloom_level"], expected["qa_type"]);
        for key in [
            "qa_type",
            "type",
            "source",
            "chunk_ref",
            "evidence_quotes",
            "concepts",
            "difficulty",
        ] {
            assert_eq!(record.value[key], expected[key], "stored {key}");
        }
        assert_eq!(
            record.access.owner_webid,
            crate::owner_webid("brooks-test-owner")
        );
        let ontology = record.ontology.as_ref().expect("queryable ontology");
        assert_eq!(ontology.pko_step.as_deref(), expected["chunk_ref"].as_str());
        assert_eq!(
            ontology.pko_procedure.as_deref(),
            Some("corpus_generate_qa")
        );
    }
    // Dry-run over an existing DB must also leave it untouched.
    let second_dry = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), true)))
            .await?,
    )?;
    reconciles(&second_dry);
    assert_eq!(store.h_mem_count()?, 4);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("training.jsonl"))?,
        output
    );
    Ok(())
}

/// expect: Blank identity and escaping, oversized, missing or non-UTF8 input fail before output or DB writes.
#[tokio::test]
async fn ingest_rejects_invalid_requests_and_unsafe_inputs() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let input = directory.path().join("generated.jsonl");
    std::fs::write(&input, flat(0, ANSWERS[0]).to_string())?;
    for dry_run in [true, false] {
        for field in ["dataset", "owner"] {
            let mut req = request(directory.path(), dry_run);
            if field == "dataset" {
                req.dataset = " \t".into();
            } else {
                req.owner = " \n".into();
            }
            let error = server
                .corpus_ingest_qa(Parameters(req))
                .await
                .expect_err("blank identity rejected");
            assert!(error.to_string().contains(field));
        }
        for path in [
            "/etc/passwd".to_string(),
            directory
                .path()
                .join("missing.jsonl")
                .to_string_lossy()
                .into_owned(),
        ] {
            let mut req = request(directory.path(), dry_run);
            req.generated_jsonl = path;
            assert!(server.corpus_ingest_qa(Parameters(req)).await.is_err());
        }
    }
    std::fs::write(&input, [0xff, 0xfe])?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("invalid UTF8 rejected");
    assert!(error.to_string().contains("UTF-8"));
    std::fs::File::create(&input)?.set_len(crate::path_safety::MAX_READ_BYTES + 1)?;
    assert!(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), false)))
            .await
            .is_err()
    );
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: File-write and DB-open failures return errors, never successful storage summaries.
#[tokio::test]
async fn ingest_surfaces_output_and_db_open_errors() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let mut req = request(directory.path(), false);
    std::fs::write(&req.generated_jsonl, flat(0, ANSWERS[0]).to_string())?;
    req.output = directory.path().to_string_lossy().into_owned();
    assert!(server.corpus_ingest_qa(Parameters(req)).await.is_err());
    assert!(!directory.path().join("memory.db").exists());
    let mut req = request(directory.path(), false);
    req.passphrase.clear();
    assert!(server.corpus_ingest_qa(Parameters(req)).await.is_err());
    // File output is deliberately not advertised as atomic with DB writes.
    assert_eq!(
        std::fs::read_to_string(directory.path().join("training.jsonl"))?
            .lines()
            .count(),
        1
    );
    Ok(())
}

/// expect: A real SQLCipher insert failure is visible and is never counted as stored.
#[tokio::test]
async fn ingest_reports_partial_storage_failure() -> anyhow::Result<()> {
    let directory = fixture()?;
    let req = request(directory.path(), false);
    std::fs::write(
        &req.generated_jsonl,
        format!("{}\n{}\n", flat(0, ANSWERS[0]), flat(1, ANSWERS[1])),
    )?;
    let store = crate::helpers::open_memory_store(&req.db_path, PASSPHRASE)?;
    let database = hkask_storage::Database::open(&req.db_path, PASSPHRASE)?;
    database
        .sqlite_pool()
        .expect("SQLCipher pool")
        .get()?
        .execute_batch(
            "CREATE TRIGGER reject_second_qa BEFORE INSERT ON hmems
         WHEN NEW.entity = 'training:qa:brooks-test:brooks.txt:1'
         BEGIN SELECT RAISE(ABORT, 'injected QA storage failure'); END;",
        )?;
    let result = content(server().corpus_ingest_qa(Parameters(req)).await?)?;
    reconciles(&result);
    assert_eq!(result["status"], "partial_failure");
    assert_eq!(result["retained"], 2);
    assert_eq!(result["stored"], 1);
    assert_eq!(result["failed"], 1);
    let errors = result["storage_errors"].as_array().expect("visible errors");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["entity"], "training:qa:brooks-test:brooks.txt:1");
    assert!(
        errors[0]["error"]
            .as_str()
            .expect("reason")
            .contains("injected QA storage failure")
    );
    assert_eq!(store.h_mem_count()?, 1);
    assert!(
        store
            .query_deduped_untouched("training:qa:brooks-test:brooks.txt:1")?
            .is_empty()
    );
    Ok(())
}
