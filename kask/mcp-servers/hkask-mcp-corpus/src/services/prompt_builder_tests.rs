//! Public-tool oracles for compact prepared QA requests.
use crate::CorpusServer;
use crate::services::qa_pipeline::{PreparedQaPrompt, render_prepared_messages};
use crate::tools::corpus::BuildPromptsRequest as ToolRequest;
use hkask_types::corpus::{ClassificationOutcome, TaggedChunk};
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::json;
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
    let canonical = hkask_bridge_ontology::term_resolution::canonicalize_terms(["quantity"]);
    TaggedChunk {
        entity_ref: format!("corpus:test:{reference}"),
        source: source.into(),
        text: format!("Original passage {reference}."),
        classification: ClassificationOutcome::Classified {
            ontology_protocol: hkask_bridge_ontology::term_resolution::TERM_RESOLUTION_PROTOCOL
                .to_string(),
        },
        candidate_terms: canonical.candidate_terms,
        ontology_tags: canonical.ontology_tags,
        concepts: canonical.concepts,
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
                hkask_bridge_ontology::dc_bibo::DOCUMENT,
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
        db_path: None,
        passphrase: None,
        prefix: Some("corpus:test:".into()),
        context_k: 0,
        qa_pairs_per_chunk: 2,
        type_distribution: "1,1,1,1,1".into(),
        max_pairs: 0,
    })
}

async fn build(
    server: &CorpusServer,
    request: ToolRequest,
) -> anyhow::Result<(serde_json::Value, Vec<PreparedQaPrompt>)> {
    let output = request.output.clone();
    let summary = server.corpus_build_prompts(Parameters(request)).await?;
    let summary = hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&summary)?);
    let prompts = std::fs::read_to_string(output)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    Ok((summary, prompts))
}

/// expect: One compact request per chunk carries two Bloom levels, and local
/// model messages never contain canonical source identities.
#[tokio::test]
async fn split_builds_have_stable_ids_and_primary_only_needs_no_db() -> anyhow::Result<()> {
    let directory = fixture()?;
    let chunks = [
        chunk("a", "book-a"),
        chunk("b", "book-a"),
        chunk("c", "book-a"),
        chunk("d", "book-b"),
    ];
    let server = server();
    let (summary, all) = build(&server, request(directory.path(), "all", &chunks)?).await?;
    assert_eq!(summary["prompts_written"], 4);
    assert_eq!(summary["pairs_requested"], 8);
    assert_eq!(summary["context_scope"], "primary_only");
    assert_eq!(summary["stored_passages"], 0);

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
            .await?
            .1,
        );
    }
    let by_id = |rows: Vec<PreparedQaPrompt>| {
        rows.into_iter()
            .map(|row| (row.prompt_id.clone(), row))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let all_by_id = by_id(all);
    let split_by_id = by_id(split);
    assert_eq!(
        all_by_id.keys().collect::<Vec<_>>(),
        split_by_id.keys().collect::<Vec<_>>()
    );
    for prompt in all_by_id.values() {
        assert_eq!(prompt.qa_types.len(), 2);
        assert_eq!(prompt.passages.len(), 1);
        let rendered = render_prepared_messages(prompt)?;
        let messages = rendered
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(messages.contains("p0"));
        assert!(!messages.contains(&prompt.primary().chunk_ref));
        assert!(!messages.contains(&prompt.primary().source));
    }
    Ok(())
}

/// expect: Failed, stale, inconsistent, or duplicate tagged rows are rejected
/// before any compact request file is written.
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
        assert!(error.to_string().contains("not reconciled"));
        assert!(!Path::new(&output).exists());
    }
    let mut inconsistent = chunk("bad-canonical", "book-a");
    inconsistent.ontology_tags.clear();
    let req = request(
        directory.path(),
        "inconsistent",
        &[chunk("a", "book-a"), inconsistent],
    )?;
    assert!(
        server
            .corpus_build_prompts(Parameters(req))
            .await
            .expect_err("derived fields must reconcile")
            .to_string()
            .contains("not reconciled")
    );

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

/// expect: KNN context remains explicit and requires valid DB configuration;
/// primary-only preparation never reads the DB.
#[tokio::test]
async fn context_requires_db_and_preserves_local_identity_mapping() -> anyhow::Result<()> {
    let directory = fixture()?;
    let server = server();
    let chunks = [chunk("a", "book-a"), chunk("b", "book-a")];

    let mut missing = request(directory.path(), "missing-db", &chunks)?;
    missing.context_k = 1;
    assert!(
        server
            .corpus_build_prompts(Parameters(missing))
            .await
            .expect_err("context needs DB")
            .to_string()
            .contains("db_path")
    );

    seed(directory.path(), &chunks)?;
    let mut contextual = request(directory.path(), "context", &chunks[..1])?;
    contextual.context_k = 1;
    contextual.db_path = Some(directory.path().join("memory.db").to_string_lossy().into());
    contextual.passphrase = Some(PASSPHRASE.into());
    let (summary, prompts) = build(&server, contextual).await?;
    assert_eq!(summary["context_scope"], "complete_source");
    assert_eq!(prompts[0].passages.len(), 2);
    assert_eq!(prompts[0].passages[0].local_id, "p0");
    assert_eq!(prompts[0].passages[1].local_id, "p1");
    assert_eq!(prompts[0].passages[1].chunk_ref, chunks[1].entity_ref);
    Ok(())
}
