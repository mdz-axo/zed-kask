//! Brooks ingestion requirements exercised at the public tool boundary, offline.
use super::{GroundQaRequest, IngestQaRequest};
use crate::CorpusServer;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

const PASSPHRASE: &str = crate::helpers::TEST_PASSPHRASE;
const ANSWERS: [&str; 4] = ["Thirty", "72 hours", "A collision", "exp(-E/kB T)"];
const PARAPHRASE_ANSWER: &str = "about thirty, give or take";

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
    crate::helpers::seed_test_passphrase();
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
        grounding_manifest: directory
            .join("grounding")
            .join("manifest.json")
            .to_string_lossy()
            .into(),
        source_chunks_jsonl: directory.join("chunks.jsonl").to_string_lossy().into(),
        output: directory.join("training.jsonl").to_string_lossy().into(),
        db_path: directory.join("memory.db").to_string_lossy().into(),
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

/// A canonical source chunk classified under the current published-ontology
/// protocol with reconciling candidate terms — the only source shape the
/// grounding standard admits.
fn tagged_chunk_json(entity_ref: &str, source: &str, text: &str) -> Value {
    let canonical = hkask_bridge_ontology::term_resolution::canonicalize_terms(["test concept"]);
    json!({
        "entity_ref": entity_ref,
        "classification": {"status":"classified","ontology_protocol":"published-term-resolution-v1"},
        "source": source,
        "text": text,
        "candidate_terms": canonical.candidate_terms,
        "ontology_tags": canonical.ontology_tags,
        "concepts": canonical.concepts,
    })
}

/// One canonical chunk per cited chunk_ref, with the evidence quote as its
/// bytes: the fixture answers are then byte-exact inside their own evidence.
fn chunk_rows_from_candidates(rows: &[String]) -> anyhow::Result<Vec<Value>> {
    let mut chunks = std::collections::BTreeMap::new();
    for line in rows {
        let qa = super::parse_qa_record(line).map_err(|error| anyhow::anyhow!("{error:?}"))?;
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
            tagged_chunk_json(chunk_ref, &qa.source, &evidence.quote),
        );
    }
    Ok(chunks.into_values().collect())
}

fn manifest_path(directory: &Path) -> std::path::PathBuf {
    directory.join("grounding").join("manifest.json")
}

fn rows_path(directory: &Path) -> std::path::PathBuf {
    directory.join("grounding").join("grounding-rows.jsonl")
}

/// Ground through the real public tool — the same producer the pipeline uses.
async fn ground(server: &CorpusServer, directory: &Path) -> anyhow::Result<Value> {
    let rows = std::fs::read_to_string(directory.join("generated.jsonl"))?
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let chunks = chunk_rows_from_candidates(&rows)?;
    std::fs::write(
        directory.join("chunks.jsonl"),
        chunks
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    content(
        server
            .corpus_ground_generated_qa(Parameters(GroundQaRequest {
                generated_jsonl: directory.join("generated.jsonl").to_string_lossy().into(),
                source_chunks_jsonl: directory.join("chunks.jsonl").to_string_lossy().into(),
                output_dir: directory.join("grounding").to_string_lossy().into(),
            }))
            .await?,
    )
}

fn read_bundle_rows(directory: &Path) -> anyhow::Result<Vec<Value>> {
    Ok(std::fs::read_to_string(rows_path(directory))?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Write a self-consistent forged bundle: the rows file hash is recomputed, so
/// only re-execution (not the artifact hash) can catch the forgery.
fn write_forged_bundle(directory: &Path, rows: &[Value]) -> anyhow::Result<()> {
    let rows_body = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(rows_path(directory), &rows_body)?;
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path(directory))?)?;
    manifest["artifacts"]["grounding_rows"]["sha256"] = json!(sha256_hex(rows_body.as_bytes()));
    manifest["artifacts"]["grounding_rows"]["rows"] = json!(rows.len());
    std::fs::write(
        manifest_path(directory),
        serde_json::to_string_pretty(&manifest)?,
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

async fn write_candidates_and_ground(
    server: &CorpusServer,
    directory: &Path,
    rows: &[String],
) -> anyhow::Result<()> {
    std::fs::write(directory.join("generated.jsonl"), rows.join("\n"))?;
    ground(server, directory).await?;
    Ok(())
}

/// Ground candidates against caller-supplied canonical chunks — used to build
/// rows whose citations cannot be byte-verified.
async fn write_rows_and_ground_chunks(
    server: &CorpusServer,
    directory: &Path,
    rows: &[String],
    chunks: &[Value],
) -> anyhow::Result<()> {
    std::fs::write(directory.join("generated.jsonl"), rows.join("\n"))?;
    std::fs::write(
        directory.join("chunks.jsonl"),
        chunks
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    content(
        server
            .corpus_ground_generated_qa(Parameters(GroundQaRequest {
                generated_jsonl: directory.join("generated.jsonl").to_string_lossy().into(),
                source_chunks_jsonl: directory.join("chunks.jsonl").to_string_lossy().into(),
                output_dir: directory.join("grounding").to_string_lossy().into(),
            }))
            .await?,
    )?;
    Ok(())
}

/// expect: Clean concise candidates ingest only after complete re-executed
/// source grounding, and the summary carries the gate's identity.
#[tokio::test]
async fn ingest_concise_metadata_counts_and_dry_run() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = ANSWERS
        .iter()
        .enumerate()
        .map(|(index, answer)| flat(index, answer).to_string())
        .collect::<Vec<_>>();
    write_candidates_and_ground(&server, directory.path(), &rows).await?;

    let dry = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), true)))
            .await?,
    )?;
    reconciles(&dry);
    assert_eq!(dry["retained"], 4);
    assert_eq!(dry["grounding_protocol"], "corpus-qa-grounding-v1");
    assert!(
        dry["grounding_manifest_sha256"]
            .as_str()
            .is_some_and(|sha| !sha.is_empty())
    );
    assert_eq!(dry["grounding_rows"], 4);
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
    for (index, row) in training.iter().enumerate() {
        assert_eq!(row["grounding"]["protocol"], "corpus-qa-grounding-v1");
        assert_eq!(
            row["grounding"]["row_key"],
            json!(format!("qa-{index}|factual|line-{}", index + 1))
        );
    }
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
    let row = flat(0, ANSWERS[0]).to_string();
    write_candidates_and_ground(&server, directory.path(), &[row]).await?;
    let mut req = request(directory.path(), false);
    req.output = directory.path().to_string_lossy().into_owned();
    assert!(server.corpus_ingest_qa(Parameters(req)).await.is_err());
    assert!(!directory.path().join("memory.db").exists());
    let req = request(directory.path(), false);
    // A DB opened under a different key than the server-side resolution
    // is an authorization failure — the tool errors and never produces a
    // successful storage summary. (Before F1/F2 this leg cleared a
    // model-supplied `passphrase` field; the credential now resolves
    // server-side, so the honest equivalent pins the same DB-open
    // failure through the repaired seam.)
    crate::helpers::open_memory_store(
        &directory.path().join("memory.db").to_string_lossy(),
        "a-different-key",
    )?;
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
    let server = server();
    let req = request(directory.path(), false);
    let rows = [
        flat(0, ANSWERS[0]).to_string(),
        flat(1, ANSWERS[1]).to_string(),
    ];
    write_candidates_and_ground(&server, directory.path(), &rows).await?;
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
    let result = content(server.corpus_ingest_qa(Parameters(req)).await?)?;
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

/// expect: A missing grounding manifest is rejected before any write, and the
/// superseded self-reported report shape cannot stand in for one.
#[tokio::test]
async fn ingest_requires_grounding_manifest_before_any_write() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let row = flat(0, ANSWERS[0]).to_string();
    std::fs::write(directory.path().join("generated.jsonl"), &row)?;
    let chunks = chunk_rows_from_candidates(std::slice::from_ref(&row))?;
    std::fs::write(
        directory.path().join("chunks.jsonl"),
        chunks
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let mut req = request(directory.path(), false);
    req.grounding_manifest = directory
        .path()
        .join("does-not-exist")
        .join("manifest.json")
        .to_string_lossy()
        .into();
    let error = server
        .corpus_ingest_qa(Parameters(req))
        .await
        .expect_err("missing grounding manifest");
    assert!(
        error.to_string().contains("does-not-exist"),
        "the error must name the missing manifest: {error}"
    );
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());

    // The superseded prepared-qa-grounding-verification-v1 report is not a
    // manifest: a self-reported verdict file cannot open the gate.
    let old_report = json!({
        "protocol":"prepared-qa-grounding-verification-v1",
        "candidate_sha256":"irrelevant",
        "prompt_id":"qa-0","chunk_ref":"corpus:brooks:0","source":"brooks.txt",
        "judgments":[],"fact_score_breakdown":{"sar":1.0,"cvr":1.0,"hfr":1.0,"nlr":1.0,"claims_checked":1},
        "fact_score":1.0,"confidence_band":"high","decoupling":"spawn_agent","verdict":"accept","findings":[]
    });
    let old_path = directory.path().join("old-report.jsonl");
    std::fs::write(&old_path, old_report.to_string())?;
    let mut req = request(directory.path(), false);
    req.grounding_manifest = old_path.to_string_lossy().into();
    let error = server
        .corpus_ingest_qa(Parameters(req))
        .await
        .expect_err("old report shape cannot open the gate");
    assert!(
        error.to_string().contains("corpus-qa-grounding-v1"),
        "{error}"
    );
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: A forged bundle that self-reports a verified answer is caught by
/// re-execution — the artifact's claims cannot substitute for derived facts.
#[tokio::test]
async fn self_reported_verification_cannot_open_ingestion_gate() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let row = json!({"instruction":"Question 0?", "output":PARAPHRASE_ANSWER,
        "qa_type":"factual", "type":"factual", "source":"brooks.txt",
        "chunk_ref":"corpus:brooks:0", "prompt_id":"qa-0",
        "provenance":{"prompt_protocol":"prepared-qa-grounding-candidate-v1","generator_model":"fixture/generator"},
        "evidence_quotes":[{"chunk_ref":"corpus:brooks:0","source":"brooks.txt","quote":"The measured count is thirty."}],
        "concepts":["test concept"], "difficulty":2})
        .to_string();
    write_candidates_and_ground(&server, directory.path(), &[row]).await?;

    // The honest bundle records the paraphrase as model_inference.
    let rows = read_bundle_rows(directory.path())?;
    let answer = rows[0]["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|claim| claim["role"] == "answer")
        .expect("answer claim");
    assert_eq!(answer["provenance"], "model_inference");
    assert_eq!(answer["strength"], 1);

    // Forge: claim tool_verified strength 2 with a fabricated byte span, and
    // rebind the manifest hash so only re-execution can catch it.
    let mut forged = rows;
    for claim in forged[0]["claims"].as_array_mut().expect("claims") {
        if claim["role"] == "answer" {
            claim["provenance"] = json!("tool_verified");
            claim["strength"] = json!(2);
            claim["byte_span"] = json!({"kind":"evidence_quote","evidence_index":0,"offset":0,"length":PARAPHRASE_ANSWER.len()});
        }
    }
    write_forged_bundle(directory.path(), &forged)?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("forged strength cannot open the gate");
    assert!(
        error.to_string().contains("does not match re-execution"),
        "{error}"
    );
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: A bundle whose rows file was modified after writing fails on the
/// manifest hash binding.
#[tokio::test]
async fn grounding_manifest_hash_mismatch_fails_closed() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = [
        flat(0, ANSWERS[0]).to_string(),
        flat(1, ANSWERS[1]).to_string(),
    ];
    write_candidates_and_ground(&server, directory.path(), &rows).await?;
    // Mutate the rows file after the manifest bound its hash.
    let body = std::fs::read_to_string(rows_path(directory.path()))?;
    std::fs::write(rows_path(directory.path()), format!("{body}\n"))?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("modified rows file fails closed");
    assert!(error.to_string().contains("manifest SHA-256"), "{error}");
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: The bundle must cover exactly the ingestible candidates — no
/// missing rows, no duplicates, no extras.
#[tokio::test]
async fn grounding_manifest_requires_bijective_row_coverage() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = [
        flat(0, ANSWERS[0]).to_string(),
        flat(1, ANSWERS[1]).to_string(),
    ];
    write_candidates_and_ground(&server, directory.path(), &rows).await?;

    // A self-consistent bundle that covers only the first candidate.
    let bundle_rows = read_bundle_rows(directory.path())?;
    write_forged_bundle(directory.path(), &bundle_rows[..1])?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("missing coverage fails closed");
    assert!(error.to_string().contains("covers 1 of 2"), "{error}");

    // A bundle that repeats one row_key for two candidates.
    let duplicated = [bundle_rows[0].clone(), bundle_rows[0].clone()];
    write_forged_bundle(directory.path(), &duplicated)?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("duplicate row keys fail closed");
    assert!(error.to_string().contains("repeats row key"), "{error}");
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: Ontology resolutions are recomputed at the gate; a tampered
/// resolution fails closed, while a coarse 5w1h_core ruling is an honest
/// record that ingests.
#[tokio::test]
async fn grounding_gate_recomputes_ontology_resolutions() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = [flat(0, ANSWERS[0]).to_string()];
    write_candidates_and_ground(&server, directory.path(), &rows).await?;

    // The fixture term resolves coarsely — an honest core anchor, not a failure.
    let bundle_rows = read_bundle_rows(directory.path())?;
    let resolution = &bundle_rows[0]["term_resolutions"][0];
    assert_eq!(resolution["tier"], "core");
    assert_eq!(resolution["namespace"], "core");
    assert_eq!(resolution["concept"], "5w1h_core");
    assert_eq!(bundle_rows[0]["candidate_terms"], json!(["test concept"]));

    // Tamper the resolution; the gate recomputes and fails the comparison.
    let mut forged = bundle_rows;
    forged[0]["term_resolutions"][0]["concept"] = json!("http://forged.example/private-concept");
    forged[0]["term_resolutions"][0]["tier"] = json!("domain_supplement");
    write_forged_bundle(directory.path(), &forged)?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("tampered resolution fails closed");
    assert!(
        error.to_string().contains("does not match re-execution"),
        "{error}"
    );
    Ok(())
}

/// expect: A genuinely paraphrased answer is recorded honestly as
/// model_inference by the grounding tool; the row is admitted on its
/// byte-verified citation and the answer is persisted as model-mediated — it is
/// never relabelled verified.
#[tokio::test]
async fn model_inference_answer_is_admitted_and_persisted_as_model_mediated() -> anyhow::Result<()>
{
    let directory = fixture()?;
    let server = server();
    let row = json!({"instruction":"Question 0?", "output":PARAPHRASE_ANSWER,
        "qa_type":"factual", "type":"factual", "source":"brooks.txt",
        "chunk_ref":"corpus:brooks:0", "prompt_id":"qa-0",
        "provenance":{"prompt_protocol":"prepared-qa-grounding-candidate-v1","generator_model":"fixture/generator"},
        "evidence_quotes":[{"chunk_ref":"corpus:brooks:0","source":"brooks.txt","quote":"The measured count is thirty."}],
        "concepts":["test concept"], "difficulty":2})
        .to_string();
    write_candidates_and_ground(&server, directory.path(), &[row]).await?;
    // The honest bundle records the paraphrase as model_inference and the
    // citation as byte-verified.
    let bundle_rows = read_bundle_rows(directory.path())?;
    assert_eq!(bundle_rows.len(), 1);
    assert!(
        bundle_rows[0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding == "answer_not_source_exact")
    );
    let answer = bundle_rows[0]["claims"]
        .as_array()
        .expect("claims")
        .iter()
        .find(|claim| claim["role"] == "answer")
        .expect("answer claim");
    assert_eq!(answer["provenance"], "model_inference");
    assert_eq!(answer["strength"], 1);

    // The row is admitted on its citation, and the answer provenance persists as
    // model-mediated in the training JSONL.
    let summary = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), false)))
            .await?,
    )?;
    reconciles(&summary);
    assert_eq!(summary["stored"], 1);
    let training = std::fs::read_to_string(directory.path().join("training.jsonl"))?;
    let record: Value = serde_json::from_str(training.lines().next().expect("stored row"))?;
    assert_eq!(record["grounding"]["answer_provenance"], "model_inference");
    Ok(())
}

/// expect: The gate runs before dedup, the dry-run return, output, and DB
/// access — a gate failure (here, a citation that cannot be byte-verified)
/// leaves no side effects in either mode.
#[tokio::test]
async fn grounding_gate_runs_before_output_and_db_open() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let row = flat(0, "Thirty").to_string();
    // The canonical chunk does not contain the cited quote, so the row's only
    // citation cannot be byte-verified and the gate fails closed.
    let chunks = [tagged_chunk_json(
        "corpus:brooks:0",
        "brooks.txt",
        "An unrelated sentence.",
    )];
    write_rows_and_ground_chunks(&server, directory.path(), &[row], &chunks).await?;
    for dry_run in [true, false] {
        assert!(
            server
                .corpus_ingest_qa(Parameters(request(directory.path(), dry_run)))
                .await
                .is_err(),
            "gate failure must not be masked by dry_run={dry_run}"
        );
    }
    assert!(!directory.path().join("training.jsonl").exists());
    assert!(!directory.path().join("memory.db").exists());
    Ok(())
}

/// expect: A valid earlier QA row cannot be written when a later row fails
/// source grounding; the whole batch is checked before either action mode.
#[tokio::test]
async fn later_ungrounded_citation_blocks_every_batch_write() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let rows = [
        flat(0, ANSWERS[0]).to_string(),
        flat(1, ANSWERS[1]).to_string(),
    ];
    let chunks = [
        tagged_chunk_json("corpus:brooks:0", "brooks.txt", ANSWERS[0]),
        tagged_chunk_json("corpus:brooks:1", "brooks.txt", "An unrelated sentence."),
    ];
    write_rows_and_ground_chunks(&server, directory.path(), &rows, &chunks).await?;

    for dry_run in [true, false] {
        let error = server
            .corpus_ingest_qa(Parameters(request(directory.path(), dry_run)))
            .await
            .expect_err("a later ungrounded citation must reject the entire batch");
        assert!(error.to_string().contains("citation claim"), "{error}");
        assert!(!directory.path().join("training.jsonl").exists());
        assert!(!directory.path().join("memory.db").exists());
    }
    Ok(())
}

/// expect: Every citation must be byte-verified; a row whose only evidence
/// quote is absent from its canonical chunk fails closed regardless of its
/// answer.
#[tokio::test]
async fn citation_not_byte_exact_fails_closed_despite_paraphrase_answer() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let row = json!({"instruction":"Question 0?", "output":PARAPHRASE_ANSWER,
        "qa_type":"factual", "type":"factual", "source":"brooks.txt",
        "chunk_ref":"corpus:brooks:0", "prompt_id":"qa-0",
        "provenance":{"prompt_protocol":"prepared-qa-grounding-candidate-v1","generator_model":"fixture/generator"},
        "evidence_quotes":[{"chunk_ref":"corpus:brooks:0","source":"brooks.txt","quote":"The measured count is thirty."}],
        "concepts":["test concept"], "difficulty":2})
        .to_string();
    let chunks = [tagged_chunk_json(
        "corpus:brooks:0",
        "brooks.txt",
        "An unrelated sentence.",
    )];
    write_rows_and_ground_chunks(&server, directory.path(), &[row], &chunks).await?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("an unverifiable citation fails closed");
    assert!(error.to_string().contains("citation claim"), "{error}");
    assert!(!directory.path().join("training.jsonl").exists());
    Ok(())
}

/// expect: Admission requires at least one byte-verified citation; a row that
/// cites no evidence cannot be admitted however well formed its answer.
#[tokio::test]
async fn row_without_evidence_cannot_be_ingested() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let row = json!({"instruction":"Question 0?", "output":"Thirty",
        "qa_type":"factual", "type":"factual", "source":"brooks.txt",
        "chunk_ref":"corpus:brooks:0", "prompt_id":"qa-0",
        "provenance":{"prompt_protocol":"prepared-qa-grounding-candidate-v1","generator_model":"fixture/generator"},
        "evidence_quotes":[], "concepts":["test concept"], "difficulty":2})
        .to_string();
    // A canonical chunk exists, but the candidate cites nothing from it.
    let chunks = [tagged_chunk_json(
        "corpus:brooks:0",
        "brooks.txt",
        "The measured count is thirty.",
    )];
    write_rows_and_ground_chunks(&server, directory.path(), &[row], &chunks).await?;
    let error = server
        .corpus_ingest_qa(Parameters(request(directory.path(), false)))
        .await
        .expect_err("a row with no grounded citation fails closed");
    assert!(
        error.to_string().contains("cites no strength-2 evidence"),
        "{error}"
    );
    assert!(!directory.path().join("training.jsonl").exists());
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
            // Byte-exact extractive answers: the V1 grounding standard admits
            // only answers that occur verbatim inside the row's own evidence.
            json!([
                {"level":"factual","question":"What is the measured count?","answer":"thirty"},
                {"level":"conceptual","question":"Why does the duration constrain timing?","answer":"The duration of 72 hours constrains when the next step can begin."}
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
                    reported: true,
                },
                finish_reason: "stop".into(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            })
        })
    }
}

/// expect: Actual public generation → public grounding → public ingest → audit
/// preserves each structured source identity, generator metadata, grounding
/// identity, and canonical ontology. Only byte-exact answers round-trip
/// through the gate; a quote match alone cannot launder prose.
#[tokio::test]
async fn exact_source_grounded_qa_round_trips_with_manifest() -> anyhow::Result<()> {
    use crate::services::qa_pipeline::{PREPARED_QA_PROTOCOL, PreparedQaPassage, PreparedQaPrompt};
    use crate::tools::corpus::QaType;
    use crate::tools::semantic::GenerateQaBatchRequest;
    let directory = fixture()?;
    crate::helpers::seed_test_passphrase();
    let port: Arc<dyn InferencePort> = Arc::new(CitationGeneration);
    let ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&port)));
    let server = CorpusServer::new(
        hkask_types::WebID::new(),
        None,
        port,
        Default::default(),
        ocr,
    );
    let passage = "The measured count is thirty. The duration of 72 hours constrains when the next step can begin.";
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
                text: passage.into(),
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

    // Ground through the real tool over canonical tagged chunks.
    std::fs::write(
        directory.path().join("chunks.jsonl"),
        [
            tagged_chunk_json("corpus:brooks:0", "brooks.txt", passage),
            tagged_chunk_json("corpus:other:1", "other.txt", "72 hours"),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n"),
    )?;
    let grounded = content(
        server
            .corpus_ground_generated_qa(Parameters(GroundQaRequest {
                generated_jsonl: req.generated_jsonl.clone(),
                source_chunks_jsonl: directory
                    .path()
                    .join("chunks.jsonl")
                    .to_string_lossy()
                    .into(),
                output_dir: directory.path().join("grounding").to_string_lossy().into(),
            }))
            .await?,
    )?;
    assert_eq!(grounded["candidates"], 2);
    assert_eq!(grounded["tool_verified_answers"], 2);
    assert_eq!(grounded["model_inference_answers"], 0);

    let manifest_sha256 = grounded["manifest_sha256"].as_str().expect("manifest sha");
    let summary = content(
        server
            .corpus_ingest_qa(Parameters(request(directory.path(), false)))
            .await?,
    )?;
    reconciles(&summary);
    assert_eq!(summary["stored"], 2);
    assert_eq!(summary["grounding_manifest_sha256"], manifest_sha256);
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
        // Grounding identity persists with every stored row.
        assert_eq!(flat["grounding"]["protocol"], "corpus-qa-grounding-v1");
        assert_eq!(flat["grounding"]["manifest_sha256"], manifest_sha256);
        assert_eq!(flat["grounding"]["answer_provenance"], "tool_verified");
        let row_key = flat["grounding"]["row_key"].as_str().expect("row key");
        let prompt_id = envelope["prompt_id"].as_str().expect("prompt id");
        assert!(row_key.starts_with(&format!("{prompt_id}|")));
        assert!(row_key.ends_with(&format!("|line-{}", index + 1)));
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
        assert_eq!(record.value["grounding"]["row_key"], row_key);
        assert_eq!(
            record.value["grounding"]["answer_provenance"],
            "tool_verified"
        );
        // Canonical ontology persists through the shared published resolver.
        let ontology = record
            .ontology
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing ontology"))?;
        assert_eq!(
            ontology.ontology_protocol.as_deref(),
            Some("published-term-resolution-v1")
        );
        assert_eq!(ontology.candidate_terms, vec!["test concept".to_string()]);
        assert!(ontology.ontology_tags.contains_key("core"));
    }
    let sources = directory.path().join("sources.jsonl");
    std::fs::write(
        &sources,
        [
            json!({"entity_ref":"corpus:brooks:0","source":"brooks.txt","text":passage}),
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
    assert_eq!(rows[0]["answer"], "thirty");
    assert_eq!(
        rows[1]["verified_claims"][0]["source_reference"]["source"],
        "brooks.txt"
    );
    assert_eq!(
        rows[1]["answer"],
        "The duration of 72 hours constrains when the next step can begin."
    );
    assert_eq!(rows[1]["fact_score"], Value::Null);
    assert_eq!(flat_report["quality_evidence"]["fact_score"], Value::Null);
    assert_eq!(flat_report["launch_authorized"], false);
    Ok(())
}
