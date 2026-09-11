//! Isolated public-tool oracles for the approved slice4 context contract.
use super::*;
use crate::CorpusServer;
use crate::tools::corpus::BuildPromptsRequest as ToolRequest;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use std::{future::Future, path::Path, pin::Pin, sync::Arc};

const PASSPHRASE: &str = "qa-context-fixture";
struct NoInference;
impl InferencePort for NoInference {
    fn generate(
        &self,
        _: &str,
        _: &LLMParameters,
        _: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        panic!("prompt building must not invoke a model")
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qa-context-tests");
    std::fs::create_dir_all(&root)?;
    Ok(tempfile::tempdir_in(root)?)
}
fn chunk(reference: &str, source: &str) -> TaggedChunk {
    TaggedChunk {
        entity_ref: format!("corpus:test:{reference}"),
        source: source.into(),
        text: format!("Original passage {reference}."),
        classification: ClassificationOutcome::Classified,
        salience: 0.5,
        concepts: vec!["concept".into()],
        ..Default::default()
    }
}
fn seed(directory: &Path, chunks: &[TaggedChunk]) -> anyhow::Result<()> {
    let store = crate::helpers::open_memory_store(
        directory
            .join("memory.db")
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("path"))?,
        PASSPHRASE,
    )?;
    for chunk in chunks {
        store.store(
            hkask_storage::HMem::new(
                &chunk.entity_ref,
                "text",
                json!(chunk.text),
                hkask_types::WebID::new(),
            )
            .with_ontology(hkask_types::HMemOntology::state(
                "bibo:Document",
                chunk.concepts.clone(),
                &chunk.source,
            )),
        )?;
        let mut vector = vec![0.0; crate::embedding_dim()];
        *vector
            .first_mut()
            .ok_or_else(|| anyhow::anyhow!("empty dimension"))? = 1.0;
        store.store_embedding(
            &chunk.entity_ref,
            &vector,
            "offline-vector",
            Some(&chunk.text),
        )?;
    }
    Ok(())
}
fn request(directory: &Path, name: &str, chunks: &[TaggedChunk]) -> anyhow::Result<ToolRequest> {
    let input = directory.join(format!("{name}.jsonl"));
    std::fs::write(
        &input,
        chunks
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n"),
    )?;
    Ok(ToolRequest {
        tagged_jsonl: input.to_string_lossy().into(),
        output: directory
            .join(format!("{name}-prompts.jsonl"))
            .to_string_lossy()
            .into(),
        db_path: directory.join("memory.db").to_string_lossy().into(),
        passphrase: PASSPHRASE.into(),
        prefix: Some("corpus:test:".into()),
        context_k: 1,
        prompts_per_chunk: 3,
        type_distribution: "1,1,1,1,1".into(),
        max_prompts: 0,
        ontology_bloom_overrides: None,
    })
}
async fn build(
    server: &CorpusServer,
    request: ToolRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let output = request.output.clone();
    let summary = server.corpus_build_prompts(Parameters(request)).await?;
    let summary = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&summary)?);
    assert_eq!(summary["context_scope"], "complete_source");
    Ok(std::fs::read_to_string(output)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// expect: Split/reordered builds produce byte-identical per-ID prompts. A tied
/// neighbor outside the input file is retrieved, with its original source/ref.
#[tokio::test]
async fn split_unsplit_context_and_prompt_identity_agree() -> anyhow::Result<()> {
    let directory = fixture()?;
    let chunks = [
        chunk("a", "book-a"),
        chunk("b", "book-a"),
        chunk("c", "book-a"),
        chunk("d", "book-b"),
    ];
    seed(directory.path(), &chunks)?;
    let store = crate::helpers::open_memory_store(
        directory
            .path()
            .join("memory.db")
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("path"))?,
        PASSPHRASE,
    )?;
    for reference in [
        "corpus:test:centroid",
        "corpus:test:lovelace:centroid",
        "corpus:test:rule:precision",
    ] {
        store.store_embedding(
            reference,
            &vec![1.0; crate::embedding_dim()],
            "offline-vector",
            None,
        )?;
    }
    let server = server();
    let all = build(&server, request(directory.path(), "all", &chunks)?).await?;
    assert_eq!(all.len(), 12);
    let mut split = Vec::new();
    for (index, chunk) in chunks.iter().rev().enumerate() {
        split.extend(
            build(
                &server,
                request(
                    directory.path(),
                    &format!("split-{index}"),
                    std::slice::from_ref(chunk),
                )?,
            )
            .await?,
        );
    }
    let by_id = |rows: Vec<serde_json::Value>| {
        rows.into_iter()
            .map(|row| (row["prompt_id"].to_string(), row))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(by_id(all.clone()), by_id(split));
    assert_eq!(
        by_id(all.clone()).len(),
        12,
        "IDs cannot repeat across partitions or QA types"
    );
    let primary = all
        .iter()
        .find(|row| row["chunk_ref"] == chunks[0].entity_ref)
        .ok_or_else(|| anyhow::anyhow!("primary missing"))?;
    let system = primary["system"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("system missing"))?;
    assert!(system.contains("\"chunk_ref\":\"corpus:test:b\""));
    assert!(system.contains("\"source\":\"book-a\""));
    assert!(
        !system.contains("Original passage c."),
        "tie must choose b before c"
    );
    assert!(
        !system.contains("Original passage d."),
        "cross-source candidate excluded"
    );
    let user = primary["user"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("user missing"))?;
    assert!(user.contains("\"chunk_ref\":\"corpus:test:a\""));
    assert!(user.contains("\"source\":\"book-a\""));
    let mut zero = request(directory.path(), "zero", &chunks)?;
    zero.context_k = 0;
    let disabled = build(&server, zero).await?;
    assert!(
        !disabled[0]["system"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("system"))?
            .contains("Original passage b.")
    );
    Ok(())
}

/// expect: Failed, unverified and duplicate refs are errors before output; no
/// apparent success from a fallback tag record, even when max_prompts would cap it.
#[tokio::test]
async fn invalid_tagged_inputs_are_rejected_before_output() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    for outcome in [
        ClassificationOutcome::Unverified,
        ClassificationOutcome::Failed {
            reason: "injected classifier error".into(),
        },
    ] {
        let mut rejected = chunk("bad", "book-a");
        rejected.classification = outcome;
        let req = request(
            directory.path(),
            "invalid",
            &[chunk("a", "book-a"), rejected],
        )?;
        let output = req.output.clone();
        let error = server
            .corpus_build_prompts(Parameters(req))
            .await
            .expect_err("status must fail");
        assert!(error.to_string().contains("not classified"));
        assert!(!Path::new(&output).exists());
    }
    let duplicate = chunk("a", "book-a");
    let req = request(
        directory.path(),
        "duplicate",
        &[duplicate.clone(), duplicate],
    )?;
    assert!(
        server
            .corpus_build_prompts(Parameters(req))
            .await
            .expect_err("duplicate must fail")
            .to_string()
            .contains("Duplicate chunk_ref")
    );
    Ok(())
}

/// expect: Missing provenance, absent stored passages, DB/open/read and output
/// errors remain visible. No real DB or inference is used by this test.
#[tokio::test]
async fn context_and_io_failures_are_not_empty_success() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let chunks = [chunk("a", "book-a")];
    let req = request(directory.path(), "missing", &chunks)?;
    assert!(
        server
            .corpus_build_prompts(Parameters(req))
            .await
            .expect_err("missing passage")
            .to_string()
            .contains("No stored passage")
    );
    seed(directory.path(), &chunks)?;
    let mut req = request(directory.path(), "read-error", &chunks)?;
    req.db_path = directory.path().to_string_lossy().into();
    assert!(server.corpus_build_prompts(Parameters(req)).await.is_err());
    let mut req = request(directory.path(), "input-error", &chunks)?;
    req.tagged_jsonl = directory
        .path()
        .join("absent.jsonl")
        .to_string_lossy()
        .into();
    assert!(server.corpus_build_prompts(Parameters(req)).await.is_err());
    let mut req = request(directory.path(), "output-error", &chunks)?;
    req.output = directory.path().to_string_lossy().into();
    assert!(server.corpus_build_prompts(Parameters(req)).await.is_err());
    let req = request(directory.path(), "source-error", &chunks)?;
    let store = crate::helpers::open_memory_store(&req.db_path, PASSPHRASE)?;
    store.delete_h_mems_by_entity_prefix("corpus:test:")?;
    // Deletion cleans embeddings too; put back a text-bearing embedding with no
    // provenance to exercise the production read rather than a stripped server.
    let mut vector = vec![0.0; crate::embedding_dim()];
    *vector
        .first_mut()
        .ok_or_else(|| anyhow::anyhow!("dimension"))? = 1.0;
    store.store_embedding(
        &chunks[0].entity_ref,
        &vector,
        "offline-vector",
        Some(&chunks[0].text),
    )?;
    assert!(
        server
            .corpus_build_prompts(Parameters(req))
            .await
            .expect_err("source provenance missing")
            .to_string()
            .contains("original source")
    );
    Ok(())
}
