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
        grounding_verification_jsonl: directory.join("grounding.jsonl").to_string_lossy().into(),
        source_chunks_jsonl: directory.join("chunks.jsonl").to_string_lossy().into(),
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
        "chunk_ref":format!("corpus:brooks:{index}"), "prompt_id":format!("qa-{index}"),
        "provenance":{"prompt_protocol":"prepared-qa-grounding-candidate-v1","generator_model":"fixture/generator"},
                "evidence_quotes":[{"chunk_ref":format!("corpus:brooks:{index}"),"source":"brooks.txt","quote":answer}],
        "concepts":["test concept"], "difficulty":2})
}

fn write_grounding(directory: &Path, rows: &[String]) -> anyhow::Result<()> {
    let mut reports = Vec::new();
    let mut chunks = std::collections::BTreeMap::new();
    for line in rows {
        let qa = super::parse_qa_record(line).map_err(|error| anyhow::anyhow!("{error:?}"))?;
        let prompt_id = qa
            .prompt_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("prompt_id"))?;
        let chunk_ref = qa
            .chunk_ref
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("chunk_ref"))?;
        let evidence = qa
            .evidence_quotes
            .first()
            .ok_or_else(|| anyhow::anyhow!("evidence"))?;
        chunks.insert(
            chunk_ref.to_string(),
            json!({"entity_ref":chunk_ref,"source":qa.source,"text":evidence.quote}),
        );
        // The instruction judgment is tool_verified: its cited quote is the
        // candidate's evidence quote, byte-checkable against the canonical
        // chunk below. The output judgment is a semantic entailment and stays
        // model_inference. The gate derives CVR from the tool_verified anchor;
        // a self-scored all-model_inference manifest fails closed.
        let judgment = |field: &str, text: &str, provenance: &str, strength: u8| {
            json!({
                "field":field,
                "text":text,
                "provenance":provenance,
                "strength":strength,
                "entailment":true,
                "why":"The complete candidate field is supported by the cited canonical source evidence.",
                "ontology_anchor":{"term":"claim grounding","tier":"core","namespace":"core","concept":"5w1h_core"},
                "source_reference":{"chunk_ref":evidence.chunk_ref,"source":evidence.source,"quote":evidence.quote}
            })
        };
        reports.push(json!({
            "protocol":"prepared-qa-grounding-verification-v1",
            "candidate_sha256":qa.row_sha256,
            "prompt_id":prompt_id,
            "chunk_ref":chunk_ref,
            "source":qa.source,
            "qa_type":qa.qa_type,
            "judgments":[
                judgment("instruction", &qa.instruction, "tool_verified", 2),
                judgment("output", &qa.output, "model_inference", 1)
            ],
            "fact_score_breakdown":{"sar":1.0,"cvr":1.0,"hfr":1.0,"nlr":1.0,"claims_checked":2},
            "fact_score":1.0,
            "confidence_band":"medium",
            "decoupling":"spawn_agent",
            "verdict":"accept",
            "findings":[]
        }));
    }
    std::fs::write(
        directory.join("grounding.jsonl"),
        reports
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    std::fs::write(
        directory.join("chunks.jsonl"),
        chunks
            .values()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    Ok(())
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

/// expect: Clean concise candidates ingest only after complete source-grounded acceptance.
#[tokio::test]
async fn ingest_concise_metadata_counts_and_dry_run() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = ANSWERS
        .iter()
        .enumerate()
        .map(|(index, answer)| flat(index, answer).to_string())
        .collect::<Vec<_>>();
    std::fs::write(directory.path().join("generated.jsonl"), rows.join("\n"))?;
    write_grounding(directory.path(), &rows)?;

    let dry = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), true)))
            .await?,
    )?;
    reconciles(&dry);
    assert_eq!(dry["retained"], 4);
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
    let output = std::fs::read_to_string(directory.path().join("training.jsonl"))?;
    let training = output
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert_eq!(training.len(), ANSWERS.len());
    let store =
        crate::helpers::open_memory_store(&request(directory.path(), false).db_path, PASSPHRASE)?;
    assert_eq!(store.h_mem_count()?, 4);
    Ok(())
}

/// expect: Malformed, incomplete, or generator-error rows reject the whole ingestion batch.
#[tokio::test]
async fn ingest_rejects_partial_generated_input() -> anyhow::Result<()> {
    let directory = fixture()?;
    let rows = [flat(0, ANSWERS[0]).to_string(), "not JSON".to_string()];
    std::fs::write(directory.path().join("generated.jsonl"), rows.join("\n"))?;
    let error = server()
        .corpus_ingest_qa(Parameters(request(directory.path(), true)))
        .await
        .expect_err("partial input must fail closed");
    assert!(error.to_string().contains("rejects partial input"));
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
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
    let row = flat(0, ANSWERS[0]).to_string();
    std::fs::write(&req.generated_jsonl, &row)?;
    write_grounding(directory.path(), &[row])?;
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
    let rows = [
        flat(0, ANSWERS[0]).to_string(),
        flat(1, ANSWERS[1]).to_string(),
    ];
    std::fs::write(&req.generated_jsonl, rows.join("\n"))?;
    write_grounding(directory.path(), &rows)?;
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

struct CitationGeneration;
impl InferencePort for CitationGeneration {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("prepared generation must use role-aware messages")
    }
    fn generate_with_messages(
        &self,
        messages: &[hkask_types::ChatMessage],
        _: &LLMParameters,
        _: Option<&str>,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        let rendered = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("corpus:brooks:0"));
        assert!(!rendered.contains("brooks.txt"));
        let rows = if rendered.contains("disposition plan") {
            assert!(rendered.contains("evidence IDs"));
            assert!(rendered.contains("conceptual_support_absent"));
            assert!(rendered.contains("primary_passage"));
            assert!(rendered.contains("reviewed_level_mandates"));
            json!([
                "clean",
                [
                    {"level":"factual","disposition":"generate","relation":null,"reason":null,"evidence_ids":["e0"]},
                    {"level":"conceptual","disposition":"generate","relation":"causal_relationship","reason":null,"evidence_ids":["e0"]}
                ]
            ])
        } else {
            assert!(rendered.contains("planned_levels"));
            json!([
                {"level":"factual","question":"What is the measured count?","answer":"Thirty"},
                {"level":"conceptual","question":"Why does the duration constrain timing?","answer":"It determines when the next step can begin."}
            ])
        };
        Box::pin(async move {
            Ok(InferenceResult {
                text: rows.to_string(),
                model: "OpenRouter/offline-model".into(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 4,
                    completion_tokens: 6,
                    total_tokens: 10,
                },
                finish_reason: "stop".into(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            })
        })
    }
}

/// expect: Actual public generation -> public ingest -> audit preserves each
/// structured source identity and generator metadata, including short answers.
/// A quote match cannot launder prose, wrong-source attribution or fabrication.
#[tokio::test]
async fn generation_ingest_audit_metadata_roundtrip() -> anyhow::Result<()> {
    use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage, PreparedQaPrompt};
    use crate::tools::corpus::QaType;
    use crate::tools::semantic::GenerateQaBatchRequest;
    let directory = fixture()?;
    let port: Arc<dyn InferencePort> = Arc::new(CitationGeneration);
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    let server = CorpusServer::new(
        hkask_types::WebID::new(),
        None,
        port,
        Default::default(),
        ocr,
    );
    let prompt = PreparedQaPrompt {
        prompt_id: hkask_types::corpus::qa_prompt_id(
            "brooks.txt",
            "corpus:brooks:0",
            "factual+conceptual",
            0,
        ),
        protocol: PREPARED_QA_PROTOCOL.into(),
        passages: vec![
            PreparedQaPassage {
                local_id: "p0".into(),
                chunk_ref: "corpus:brooks:0".into(),
                source: "brooks.txt".into(),
                text: "The measured count is thirty. The duration of 72 hours constrains when the next step can begin.".into(),
            },
            PreparedQaPassage {
                local_id: "p1".into(),
                chunk_ref: "corpus:other:1".into(),
                source: "other.txt".into(),
                text: "72 hours".into(),
            },
        ],
        candidate_terms: vec!["test concept".into()],
        qa_types: vec![QaType::Factual, QaType::Conceptual],
    };
    let prompts = directory.path().join("prompts.jsonl");
    std::fs::write(&prompts, serde_json::to_string(&prompt)?)?;
    // Generation is reviewed-decisions-only: the manifest admits the passage
    // and mandates both ordered levels, matching the stub plan's relations.
    let adjudications = directory.path().join("adjudications.jsonl");
    std::fs::write(
        &adjudications,
        serde_json::to_string(&json!({
            "protocol": crate::services::qa_adjudication::QA_ADJUDICATION_PROTOCOL,
            "prompt_id": prompt.prompt_id,
            "chunk_ref": "corpus:brooks:0",
            "source": "brooks.txt",
            "passage": {"decision":"admit","reason":null},
            "levels": [
                {"level":"factual","decision":"generate","relation":null,"reason":null},
                {"level":"conceptual","decision":"generate","relation":"causal_relationship","reason":null}
            ],
        }))?,
    )?;
    let req = request(directory.path(), false);
    let generated = content(
        server
            .corpus_generate_qa_batch(Parameters(GenerateQaBatchRequest {
                prompts_jsonl: prompts.to_string_lossy().into(),
                quality_adjudications_jsonl: adjudications.to_string_lossy().into(),
                output: req.generated_jsonl.clone(),
                concurrency: 1,
                model: Some("OpenRouter/offline-model".into()),
            }))
            .await?,
    )?;
    assert_eq!(generated["qa_rows_written"], 2);
    assert_eq!(generated["prompts_failed"], 0);
    let generated_lines = std::fs::read_to_string(&req.generated_jsonl)?
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    write_grounding(directory.path(), &generated_lines)?;
    let summary = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), false)))
            .await?,
    )?;
    reconciles(&summary);
    assert_eq!(summary["stored"], 2);
    let generated_rows: Vec<Value> = std::fs::read_to_string(&req.generated_jsonl)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let training: Vec<Value> = std::fs::read_to_string(&req.output)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let store = crate::helpers::open_memory_store(&req.db_path, PASSPHRASE)?;
    for (index, (envelope, flat)) in generated_rows.iter().zip(&training).enumerate() {
        for key in ["chunk_ref", "source", "qa_type", "prompt_id", "provenance"] {
            assert_eq!(flat[key], envelope[key], "{key}");
        }
        assert_eq!(
            flat["evidence_quotes"],
            envelope["response"]["evidence_quotes"]
        );
        let records = store
            .query_deduped_untouched(&format!("training:qa:brooks-test:brooks.txt:{index}"))?;
        let record = records
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing stored QA"))?;
        for key in [
            "chunk_ref",
            "source",
            "qa_type",
            "prompt_id",
            "provenance",
            "evidence_quotes",
        ] {
            assert_eq!(record.value[key], flat[key], "memory {key}");
        }
    }
    let sources = directory.path().join("sources.jsonl");
    std::fs::write(
        &sources,
        [
            json!({"entity_ref":"corpus:brooks:0","source":"brooks.txt","text":"The measured count is thirty. The duration of 72 hours constrains when the next step can begin."}),
            json!({"entity_ref":"corpus:other:1","source":"other.txt","text":"72 hours"}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n"),
    )?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/audit-qa-quality.sh");
    let audit = async |input: &str| -> anyhow::Result<Value> {
        let mut command = tokio::process::Command::new("bash");
        command
            .arg(&script)
            .arg(input)
            .arg(&sources)
            .kill_on_drop(true);
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await??;
        anyhow::ensure!(
            output.status.code() == Some(2),
            "audit status {:?}: stderr={} stdout={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    let envelope_report = audit(&req.generated_jsonl).await?;
    let flat_report = audit(&req.output).await?;
    assert_eq!(
        envelope_report["rows"], flat_report["rows"],
        "audit must normalize both legitimate shapes identically"
    );
    let rows = flat_report["rows"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("audit rows"))?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["fact_score"], Value::Null);
    assert_eq!(rows[0]["verified_claims"][0]["provenance"], "tool_verified");
    assert_eq!(
        rows[0]["verified_claims"][1]["provenance"],
        "model_inference"
    );
    assert_eq!(rows[0]["answer"], "Thirty");
    assert_eq!(
        rows[1]["verified_claims"][0]["source_reference"]["source"],
        "brooks.txt"
    );
    assert_eq!(
        rows[1]["answer"],
        "It determines when the next step can begin."
    );
    assert_eq!(rows[1]["fact_score"], Value::Null);
    assert_eq!(flat_report["quality_evidence"]["fact_score"], Value::Null);
    assert_eq!(flat_report["launch_authorized"], false);
    Ok(())
}
