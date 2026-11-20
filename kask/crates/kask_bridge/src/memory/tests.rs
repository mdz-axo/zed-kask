use super::*;
use hkask_memory::{FederatedSourceSpec, FederatedSourcesManifest};
use hkask_storage::database::sqlite::SqliteDriver;

pub(crate) fn in_memory_port() -> RealMemoryPort {
    in_memory_port_with_cadence(0, 0.3)
}

#[test]
fn connectedness_surfaces_store_errors_instead_of_silent_zero() {
    // A configured-but-broken curator store must not read as "no
    // connections" in recall ranking: the swallow site warns with the
    // entity and the error. Source-structure pin (the D43
    // concat!-needle precedent — the test's own source cannot satisfy
    // it, because the needles exist joined only at runtime).
    let source = include_str!("../memory.rs");
    let body = source
        .split(concat!("pub fn connectedne", "ss("))
        .nth(1)
        .expect("connectedness exists")
        .split("pub fn ")
        .next()
        .expect("connectedness body ends");
    assert!(body.contains(concat!("tracing::wa", "rn!")));
    assert!(body.contains(concat!("lookup fai", "led")));
}

/// The in-memory curator `MemoryStore` — a separate in-memory driver so
/// the curator copy lands in a different DB, mirroring production where
/// the curator has its own `curator.db`.
fn in_memory_curator_store() -> Arc<MemoryStore> {
    let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> = SqliteDriver::in_memory_driver();
    let curator_h_mem_store =
        HMemStore::from_driver(Arc::clone(&curator_driver)).expect("curator hmem store init");
    let curator_embedding_store =
        EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init");
    Arc::new(MemoryStore::new(
        curator_h_mem_store,
        curator_embedding_store,
    ))
}

/// The `RealMemoryPort` literal — the one copy of the seventeen-field
/// construction the port constructors below and the counting-driver
/// tests share.
fn port_with(
    curator_store_inner: Arc<MemoryStore>,
    embedding_port: LanguageModelEmbeddingPort,
    curator_consolidation: Option<Arc<MemoryConsolidator>>,
    consolidation_cadence_secs: u64,
    confidence_floor: f64,
) -> RealMemoryPort {
    RealMemoryPort {
        curator_store: Arc::new(CuratorStore::for_tests(Some(curator_store_inner))),
        private_curator_store: Arc::new(CuratorStore::for_tests(Some(in_memory_curator_store()))),
        embedding_port: Some(embedding_port),
        embedding_model: "test-model".to_string(),
        // The private pair defaults unwired: every existing test's
        // private legs keep the keyword-only degrade (their pinned
        // behavior); the item-3 pins wire the pair explicitly via
        // `in_memory_port_with_private_embed_fn`.
        private_embedding_port: None,
        private_embedding_model: String::new(),
        external_passphrase: String::new(),
        external_sources: Arc::new(hkask_memory::FederatedSourceCache::new()),
        classifier_model: None,
        private_classifier_model: None,
        template_root: std::path::PathBuf::from("test-registry-root"),
        curator_webid: WebID::from_persona(b"curator"),
        curator_consolidation: Arc::new(RwLock::new(curator_consolidation)),
        // The item-4 twin defaults UNBUILT and the forgetting leg
        // disabled — every existing test's legs stay inert (their
        // pinned behavior); the item-4 pins build the twin and set
        // the knob explicitly.
        private_curator_consolidation: Arc::new(RwLock::new(None)),
        consolidation_cadence_secs,
        confidence_floor,
        forgetting_days: 0,
        tokio_handle: tokio::runtime::Handle::current(),
        ingest_semaphore: tokio::sync::Semaphore::new(1),
    }
}

/// The `TurnRecord` skeleton every test in this module builds — only the
/// per-test fields vary; model and title are constant across tests.
fn turn_record(
    thread_id: &str,
    user_input: &str,
    agent_response: &str,
    agent_id: &str,
) -> TurnRecord {
    TurnRecord {
        thread_id: thread_id.to_string(),
        user_input: user_input.to_string(),
        agent_response: agent_response.to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some(agent_id.to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    }
}

/// Borrow the production turn-ingestion configuration for focused write tests.
fn write_context<'a>(
    port: &'a RealMemoryPort,
    embedding_port: Option<&'a LanguageModelEmbeddingPort>,
) -> WriteContext<'a> {
    WriteContext {
        curator_store: &port.curator_store,
        private_curator_store: &port.private_curator_store,
        embedding_port,
        embedding_model: &port.embedding_model,
        private_embedding_port: port.private_embedding_port.as_ref(),
        private_embedding_model: &port.private_embedding_model,
        classifier_model: port.classifier_model.as_deref(),
        private_classifier_model: port.private_classifier_model.as_deref(),
        template_root: port.template_root.as_path(),
        curator_webid: port.curator_webid,
        tokio_handle: &port.tokio_handle,
        curator_consolidation: &port.curator_consolidation,
        private_curator_consolidation: &port.private_curator_consolidation,
        consolidation_cadence_secs: port.consolidation_cadence_secs,
    }
}

/// The below-floor probe h_mem the consolidation tests seed — it must
/// survive ingestion untouched and die only on a consolidation pass.
fn below_floor_probe(webid: WebID) -> hkask_storage::HMem {
    hkask_storage::HMem::new(
        "probe:low",
        "fact",
        serde_json::Value::String("low-confidence probe".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.1))
}

fn in_memory_port_with_embeddings() -> RealMemoryPort {
    in_memory_port_with_embed_fn(Arc::new(|_text: &str| vec![0.25; 1024]))
}

pub(crate) fn in_memory_port_with_embed_fn<F>(embed_fn: Arc<F>) -> RealMemoryPort
where
    F: Fn(&str) -> Vec<f32> + Send + Sync + ?Sized + 'static,
{
    let embedding_port = LanguageModelEmbeddingPort::for_tests_with_embed_fn(
        embed_fn,
        tokio::runtime::Handle::current(),
    );
    port_with(in_memory_curator_store(), embedding_port, None, 0, 0.3)
}

/// The item-3 fixture: the CLOUD pair (as every test wires it) plus a
/// LOCAL pair wired as the private leg — the pins assert private
/// content reaches the local port and never the cloud one.
pub(crate) fn in_memory_port_with_private_embed_fn(
    cloud_embed_fn: Arc<dyn Fn(&str) -> Vec<f32> + Send + Sync>,
    private_embed_fn: Arc<dyn Fn(&str) -> Vec<f32> + Send + Sync>,
) -> RealMemoryPort {
    let embedding_port = LanguageModelEmbeddingPort::for_tests_with_embed_fn(
        cloud_embed_fn,
        tokio::runtime::Handle::current(),
    );
    let private_embedding_port = LanguageModelEmbeddingPort::for_tests_with_embed_fn(
        private_embed_fn,
        tokio::runtime::Handle::current(),
    );
    let mut port = port_with(in_memory_curator_store(), embedding_port, None, 0, 0.3);
    port.private_embedding_port = Some(private_embedding_port);
    port.private_embedding_model = "ollama/test-private-embedding".to_string();
    port
}

pub(crate) fn in_memory_port_with_external_fixture() -> RealMemoryPort {
    let mut port = in_memory_port_with_embed_fn(Arc::new(|_query: &str| {
        let mut vector = vec![0.0; 1024];
        vector[0] = 1.0;
        vector
    }));
    port.external_passphrase = "test-passphrase".to_string();
    port
}

fn in_memory_port_with_cadence(
    consolidation_cadence_secs: u64,
    confidence_floor: f64,
) -> RealMemoryPort {
    let curator_store_inner = in_memory_curator_store();
    // Tests don't call embed — use a stub port with no backing task.
    let embedding_port = LanguageModelEmbeddingPort::for_tests();
    // Curator consolidation service — mirrors the production construction
    // in `RealMemoryPort::new`. Skipped when cadence is 0 (matches
    // production). The curator store is always `Some` in tests.
    let curator_consolidation = build_curator_consolidation(
        consolidation_cadence_secs,
        &Some(Arc::clone(&curator_store_inner)),
    );
    port_with(
        curator_store_inner,
        embedding_port,
        curator_consolidation,
        consolidation_cadence_secs,
        confidence_floor,
    )
}

/// A selected sealed source returns passage provenance; manifest changes
/// revoke cached admission, and unselected broken sources are not opened.
#[tokio::test]
async fn external_search_reads_only_selected_sealed_source_and_revalidates() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "fixture.txt",
        "grounded fixture passage",
        "test-model",
        "test-model",
    )?;
    let mut manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let mut broken = manifest.sources[0].clone();
    broken.id = "unselected-broken".to_string();
    broken.run_identity_path = directory.path().join("missing-identity.json");
    manifest.sources.push(broken);
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let database_path = directory.path().join("reference.db");
    let before = std::fs::read(&database_path)?;
    let port = in_memory_port_with_external_fixture();
    let selected = ["fixture-reference".to_string()];
    let batches = port
        .search_external_passages_at(&manifest_path, "fixture", &selected, 3)
        .await
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        batches.len(),
        1,
        "unselected broken source must not be opened"
    );
    let hit = &batches[0].hits[0];
    assert_eq!(hit.source_id, "fixture-reference");
    assert_eq!(hit.display_name, "Fixture research library");
    assert_eq!(hit.text, "grounded fixture passage");
    assert_eq!(
        hit.entity_ref,
        "calibration:fixture:sealed-v1:reference:utf8-666978747572652e747874:0"
    );
    assert_eq!(hit.model, "test-model");
    assert_eq!(
        hit.run_id,
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(
            directory.path().join("run-identity.json")
        )?)?["run_id"]
    );
    assert!(!hit.text.contains("parataxis_ratio"));
    assert_eq!(batches[0].missing_text, 0);
    let cached = port.external_sources.cached().expect("admitted source");
    port.search_external_passages_at(&manifest_path, "fixture again", &selected, 3)
        .await
        .map_err(anyhow::Error::msg)?;
    let reused = port.external_sources.cached().expect("cached source");
    assert!(
        Arc::ptr_eq(&cached, &reused),
        "unchanged source should reuse the sealed handle"
    );
    manifest
        .sources
        .retain(|source| source.id != "fixture-reference");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let error = port
        .search_external_passages_at(&manifest_path, "fixture", &selected, 3)
        .await
        .expect_err("removed source must be revoked");
    assert!(error.contains("not registered"), "{error}");
    assert!(
        port.external_sources.cached().is_none(),
        "stale handle must be evicted"
    );
    assert_eq!(
        std::fs::read(&database_path)?,
        before,
        "external database must not be changed"
    );
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        assert!(
            !directory
                .path()
                .join(format!("reference.db{suffix}"))
                .exists(),
            "read-only search must not create a sidecar"
        );
    }
    Ok(())
}

#[tokio::test]
async fn external_search_rejects_unregistered_ids_before_opening_sources() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = directory.path().join("federated-sources.json");
    let registered_path = directory.path().join("never-opened.db");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&FederatedSourcesManifest {
            schema_version: 1,
            sources: vec![FederatedSourceSpec {
                id: "registered".to_string(),
                display_name: "Registered".to_string(),
                database_path: registered_path,
                run_identity_path: directory.path().join("identity.json"),
                representations_manifest_path: directory.path().join("representations.json"),
                index_name: "passages".to_string(),
                materialized_provenance: None,
            }],
        })?,
    )?;
    let port = in_memory_port();
    let error = port
        .search_external_passages_at(&manifest_path, "query", &["not-registered".to_string()], 2)
        .await
        .expect_err("unregistered IDs cannot authorize arbitrary paths");
    assert!(error.contains("not registered"), "{error}");
    assert!(port.external_sources.cached().is_none());
    Ok(())
}

#[tokio::test]
async fn external_search_surfaces_sealed_source_failure_without_embedding() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = directory.path().join("federated-sources.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&FederatedSourcesManifest {
            schema_version: 1,
            sources: vec![FederatedSourceSpec {
                id: "sealed".to_string(),
                display_name: "Sealed".to_string(),
                database_path: directory.path().join("source.db"),
                run_identity_path: directory.path().join("missing-identity.json"),
                representations_manifest_path: directory.path().join("representations.json"),
                index_name: "passages".to_string(),
                materialized_provenance: None,
            }],
        })?,
    )?;
    let port = in_memory_port();
    let error = port
        .search_external_passages_at(&manifest_path, "query", &["sealed".to_string()], 2)
        .await
        .expect_err("missing sealed identity must fail closed");
    assert!(error.contains("run identity"), "{error}");
    assert!(port.external_sources.cached().is_none());
    Ok(())
}

/// A federated source sealed at a narrower width than the curator store
/// must receive its query embedded at the SOURCE's width — the shipped-
/// bundle case where the bundle's sealed dimensions differ from the
/// store's. Pins the per-source-width embedding in
/// `search_external_passages_at`: before the fix, the query was embedded
/// once at the curator store's width and the narrow source rejected it.
#[tokio::test]
async fn external_search_embeds_the_query_at_each_source_width() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_dimmed(
        directory.path(),
        "dimmed-source",
        "Narrow sealed source",
        "dimmed.txt",
        "width-matched fixture passage",
        "test-model",
        "test-model",
        512,
    )?;
    let embed_port = LanguageModelEmbeddingPort::for_tests_with_dim_embed_fn(
        Arc::new(|_text: &str, dims: Option<u32>| {
            // The test environment's configured width (HKASK_EMBEDDING_DIM
            // unset) is 1024 — the file's existing test convention.
            let width = usize::try_from(dims.unwrap_or(1024)).expect("width");
            let mut vector = vec![0.0; width];
            vector[0] = 1.0;
            vector
        }),
        tokio::runtime::Handle::current(),
    );
    let mut port = port_with(in_memory_curator_store(), embed_port, None, 0, 0.3);
    port.external_passphrase = "test-passphrase".to_string();
    let batches = port
        .search_external_passages_at(&manifest_path, "query", &["dimmed-source".to_string()], 3)
        .await
        .expect("the width-matched search must succeed");
    assert_eq!(batches.len(), 1, "one source, one batch");
    assert_eq!(
        batches[0].hits.len(),
        1,
        "the width-matched passage is found"
    );
    assert_eq!(batches[0].hits[0].text, "width-matched fixture passage");
    Ok(())
}

/// The hybrid-retrieval known-answer fixture: a named document whose
/// passage text never names itself (only the entity_ref filename
/// carries the name), dense-attracting decoys, and the corpus's own
/// MANIFEST metadata chunk whose text contains the named document's
/// slug. Every query embeds as `vector[0] = 1.0`
/// (`in_memory_port_with_external_fixture`), so slot-0 documents sit at
/// dense distance 0 and the slot-1 named document at dense distance 1
/// — dense alone ranks the named document last, which is the live
/// failure the lexical leg exists to fix (2026-10-09: the query
/// "magnifica humanitas" did not recall the charter from zk-ref-open).
fn hybrid_retrieval_documents() -> Vec<hkask_memory::test_support::DocumentFixture> {
    vec![
        hkask_memory::test_support::DocumentFixture {
            filename: "alpha-charter.md".to_string(),
            passages: vec![
                "dignity of work expresses and enhances the lives of persons".to_string(),
                "subsidiarity places decisions at the closest level to the person".to_string(),
                "the limit is positive and systems flourish through restraint".to_string(),
            ],
            vector_slot: 1,
        },
        hkask_memory::test_support::DocumentFixture {
            filename: "decoy-instructions.md".to_string(),
            passages: vec![
                "system prompt instructions template for responding to queries".to_string(),
            ],
            vector_slot: 0,
        },
        hkask_memory::test_support::DocumentFixture {
            filename: "decoy-dialogue.md".to_string(),
            passages: vec![
                "dialogue history and working memory sections of the prompt".to_string(),
            ],
            vector_slot: 0,
        },
        hkask_memory::test_support::DocumentFixture {
            filename: "MANIFEST.json".to_string(),
            passages: vec![
                r#"{"slug": "alpha-charter", "retrieval": {"method": "fixture"}}"#.to_string(),
            ],
            vector_slot: 0,
        },
    ]
}

fn fixture_hex(filename: &str) -> String {
    filename.bytes().map(|byte| format!("{byte:02x}")).collect()
}

async fn hybrid_retrieval_batches(
    manifest_path: &std::path::Path,
    query: &str,
) -> anyhow::Result<Vec<hkask_memory::ExternalPassageBatch>> {
    let port = in_memory_port_with_external_fixture();
    port.search_external_passages_at(manifest_path, query, &["fixture-reference".to_string()], 5)
        .await
        .map_err(anyhow::Error::msg)
}

/// The bare entity-name query must recall the named document's chunks
/// — the live failure class: dense alone ranked them last, and the
/// metadata chunk crowded the window before the penalty.
#[tokio::test]
async fn entity_name_query_recalls_the_named_document() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_documents(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "test-model",
        "test-model",
        &hybrid_retrieval_documents(),
    )?;
    let batches = hybrid_retrieval_batches(&manifest_path, "alpha charter").await?;
    let hits = &batches[0].hits;
    let alpha = fixture_hex("alpha-charter.md");
    let manifest = fixture_hex("MANIFEST.json");
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.entity_ref.contains(&alpha))
            .count(),
        3,
        "all three named-document chunks must surface, including the one outside the dense window"
    );
    assert!(
        hits[0].entity_ref.contains(&alpha) && hits[1].entity_ref.contains(&alpha),
        "the bare entity-name query must rank the named document first"
    );
    assert!(
        hits.iter().all(|hit| !hit.entity_ref.contains(&manifest)),
        "the metadata chunk must not crowd the named document out"
    );
    Ok(())
}

/// A multi-topic prompt that names the document once among mechanics
/// vocabulary must still recall it — the live case was a long recall-
/// mechanics question naming the charter.
#[tokio::test]
async fn multitopic_prompt_recalls_the_named_document() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_documents(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "test-model",
        "test-model",
        &hybrid_retrieval_documents(),
    )?;
    let query = "how does the recall of the alpha charter from the federated source work, \
                 is it chunked and tagged, and were the chunks injected into the context";
    let batches = hybrid_retrieval_batches(&manifest_path, query).await?;
    let hits = &batches[0].hits;
    let alpha = fixture_hex("alpha-charter.md");
    assert!(
        hits[0].entity_ref.contains(&alpha),
        "the named document must lead the fused ranking"
    );
    assert_eq!(
        hits.iter()
            .filter(|hit| hit.entity_ref.contains(&alpha))
            .count(),
        3,
        "all three named-document chunks must surface"
    );
    Ok(())
}

/// The corpus's own build-metadata chunk (MANIFEST.json) is down-ranked:
/// on dense rank parity with content chunks it surfaces last, never
/// crowding content out of the window.
#[tokio::test]
async fn metadata_source_chunks_are_downranked() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_documents(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "test-model",
        "test-model",
        &hybrid_retrieval_documents(),
    )?;
    let query = "system prompt instructions template dialogue history working memory \
                 sections responding queries";
    let batches = hybrid_retrieval_batches(&manifest_path, query).await?;
    let hits = &batches[0].hits;
    let manifest = fixture_hex("MANIFEST.json");
    let manifest_position = hits
        .iter()
        .position(|hit| hit.entity_ref.contains(&manifest))
        .expect("the metadata chunk keeps its candidacy, down-ranked not excluded");
    assert_eq!(
        manifest_position,
        hits.len() - 1,
        "the metadata chunk must rank last, never on parity with content"
    );
    Ok(())
}

/// The shipped distribution copy omits `hmems` (producer-owned; the
/// federated read path never touches it) and is vacuumed. Admission and
/// retrieval must be unaffected: a stripped source serves hits exactly as
/// a full one does.
///
/// expect: "A corpus shipped without its producer-owned tables still answers queries" [P1]
/// pre:  a sealed federated fixture whose hmems rows have been dropped
/// post: admission succeeds and the named document still surfaces
#[tokio::test]
async fn a_stripped_sealed_source_still_serves_retrieval() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_documents(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "test-model",
        "test-model",
        &hybrid_retrieval_documents(),
    )?;
    let source_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("manifest has no parent dir"))?;
    let database_path = source_dir.join("reference.db");
    let database_str = database_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 database path"))?;

    let query = "dignity of work and the positive limit";
    let before = hybrid_retrieval_batches(&manifest_path, query).await?;
    assert!(
        !before[0].hits.is_empty(),
        "the unstripped fixture must return hits before the strip"
    );

    hkask_storage::strip_producer_tables(database_str, "test-passphrase", &["hmems"])
        .map_err(|error| anyhow::anyhow!("strip failed: {error}"))?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = format!("{database_str}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }
    // The strip changes the database bytes, so the run identity's index
    // pin must be re-derived before admission will accept the source —
    // exactly the order the production export path uses (strip, then
    // measure the content pins).
    let identity_path = source_dir.join("run-identity.json");
    let mut identity: serde_json::Value = serde_json::from_slice(&std::fs::read(&identity_path)?)?;
    identity["indexes"]["reference"] =
        serde_json::json!(hkask_memory::test_support::sha256_file(&database_path)?);
    hkask_memory::test_support::reseal_fixture_identity(&mut identity)?;
    std::fs::write(&identity_path, serde_json::to_vec_pretty(&identity)?)?;

    let after = hybrid_retrieval_batches(&manifest_path, query).await?;
    assert_eq!(
        after[0]
            .hits
            .iter()
            .map(|hit| hit.entity_ref.clone())
            .collect::<Vec<_>>(),
        before[0]
            .hits
            .iter()
            .map(|hit| hit.entity_ref.clone())
            .collect::<Vec<_>>(),
        "the stripped source must serve the same hits in the same order"
    );
    Ok(())
}

/// Control: a query with no rare-term signal leaves the dense order
/// among content rows unchanged — the lexical leg is silent without
/// signal, and only the metadata penalty applies.
#[tokio::test]
async fn queries_without_rare_terms_keep_the_dense_order() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_documents(
        directory.path(),
        "fixture-reference",
        "Fixture research library",
        "test-model",
        "test-model",
        &hybrid_retrieval_documents(),
    )?;
    let batches = hybrid_retrieval_batches(&manifest_path, "qqq www unindexed vocabulary").await?;
    let hits = &batches[0].hits;
    let instructions = fixture_hex("decoy-instructions.md");
    let dialogue = fixture_hex("decoy-dialogue.md");
    let alpha = fixture_hex("alpha-charter.md");
    let instructions_position = hits
        .iter()
        .position(|hit| hit.entity_ref.contains(&instructions))
        .expect("dense-attracting decoy present");
    let dialogue_position = hits
        .iter()
        .position(|hit| hit.entity_ref.contains(&dialogue))
        .expect("dense-attracting decoy present");
    for hit in hits.iter().filter(|hit| hit.entity_ref.contains(&alpha)) {
        assert!(
            hit.source_rank > instructions_position + 1 && hit.source_rank > dialogue_position + 1,
            "without rare-term signal the dense order among content rows is unchanged"
        );
    }
    Ok(())
}

#[test]
fn mutable_lesson_projection_requires_and_preserves_provenance_boundary() {
    let mut lesson = hkask_storage::HMem::new(
        "mutable:lesson",
        "status",
        serde_json::json!({
            "text": "The server exposes five tools.",
            "mutable_state": true,
            "state_provenance": {
                "source_locator": "kask/file.rs",
                "version_or_date": "abc123"
            }
        }),
        WebID::new(),
    );
    assert!(
        hkask_memory::semantic_passage_for_h_mem(&lesson).is_none(),
        "mutable claims without provenance-bearing recall_text are not injected"
    );
    lesson.value["recall_text"] = serde_json::json!(
        "The server exposes five tools. [mutable state; source: kask/file.rs; version/date: abc123]"
    );
    let projected = hkask_memory::semantic_passage_for_h_mem(&lesson).expect("bounded recall text");
    assert!(projected.contains("kask/file.rs"));
    assert!(projected.contains("abc123"));
}

#[tokio::test]
async fn ingest_turn_writes_single_copy_chunk_h_mems() {
    let port = in_memory_port();
    let record = TurnRecord {
        thread_id: "test-thread".to_string(),
        user_input: "What is Rust?".to_string(),
        agent_response: "Rust is a systems programming language.".to_string(),
        model: "test-model".to_string(),
        thread_title: Some("Rust Discussion".to_string()),
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    let result = port.ingest_turn(record).await;
    assert!(result.is_ok(), "ingest_turn should succeed");

    // Single copy (2026-09-04 ruling): chunks under the shared entity —
    // no curator-perspective duplicate.
    let curator_store = port.curator_store.get().expect("curator store");
    let chunks = curator_store
        .query_deduped_untouched("curator:thread:test-thread")
        .expect("query should succeed");
    assert_eq!(chunks.len(), 1, "one chunk for a short turn");
    assert!(chunks[0].attribute.starts_with("chunk:") && chunks[0].attribute.ends_with(":0"));
    let text = chunks[0].value.as_str().expect("chunk value is plain text");
    assert!(text.contains("user: What is Rust?"));
    assert!(text.contains("assistant: Rust is a systems programming language."));

    // Structural ontology: process-anchored, deterministic dimensions —
    // no classifier model is configured in the test port, so the content
    // pair (what/why) is absent and the structural four are the signal.
    let ontology = chunks[0]
        .ontology
        .as_ref()
        .expect("chunk carries an ontology blob");
    assert_eq!(ontology.pko_procedure.as_deref(), Some("chat"));
    assert_eq!(ontology.pko_step.as_deref(), Some("chunk:0"));
    for dimension in ["how", "when", "who", "where"] {
        assert!(
            ontology.dimensions.contains(&dimension.to_string()),
            "structural dimension {dimension} must be present without an LLM"
        );
    }
}

/// FR-4 (D-2), the ingest seam: a turn from a privacy-marked thread
/// (D100) stamps every chunk h_mem `PrivateInference` — the mark rides the
/// record from the zed side and enters the store with the content, never
/// applied later by a model-facing surface.
#[tokio::test]
async fn ingest_turn_stamps_private_routed_turn_chunks_private() {
    let port = in_memory_port();
    let record = TurnRecord {
        thread_id: "private-thread".to_string(),
        user_input: "What runs on the private leg?".to_string(),
        agent_response: "This content must stay private.".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };

    port.ingest_turn(record).await.expect("ingest succeeds");
    // zed-kask: D100/FR-4 (operator ruling 2026-11-05) — the store
    // split: the private-routed turn lands in the PRIVATE store (marked),
    // and the PUBLIC store never receives a row for the thread.
    let chunks = port
        .private_curator_store
        .get()
        .expect("private curator store")
        .query_deduped_untouched("curator:thread:private-thread")
        .expect("query should succeed");
    assert!(!chunks.is_empty(), "the turn produced chunks");
    for chunk in &chunks {
        assert_eq!(
            chunk.inference_provenance,
            hkask_types::InferenceProvenance::PrivateInference,
            "every chunk of a private-routed turn carries the mark"
        );
    }
    let public_rows = port
        .curator_store
        .get()
        .expect("public curator store")
        .query_deduped_untouched("curator:thread:private-thread")
        .expect("query should succeed");
    assert!(
        public_rows.is_empty(),
        "the public store never receives a private turn's content — \
         the separate private DB is clean-by-construction on the public side"
    );
}

/// The counting embedder the degrade-not-leak legs share — records
/// every embed call, returns a fixed-width zero vector.
fn counting_embed_fn() -> (
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    Arc<dyn Fn(&str) -> Vec<f32> + Send + Sync>,
) {
    let embed_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counting_embed = {
        let embed_calls = embed_calls.clone();
        Arc::new(move |_text: &str| -> Vec<f32> {
            embed_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![0.0_f32; 1024]
        }) as Arc<dyn Fn(&str) -> Vec<f32> + Send + Sync>
    };
    (embed_calls, counting_embed)
}

/// The deterministic two-direction embedder for the KNN-isolation
/// pins: any text containing one of `target_markers` embeds to one
/// fixed direction; every other text embeds to the orthogonal one —
/// so a marker query is KNN-identical to a marker chunk while sharing
/// NO word with it (the keyword leg cannot find it).
fn knn_direction_fixture_embed_fn(
    target_markers: &'static [&'static str],
) -> Arc<dyn Fn(&str) -> Vec<f32> + Send + Sync> {
    Arc::new(move |text: &str| -> Vec<f32> {
        let mut vector = vec![0.0_f32; 1024];
        if target_markers.iter().any(|marker| text.contains(marker)) {
            for entry in vector.iter_mut().take(512) {
                *entry = 1.0;
            }
        } else {
            for entry in vector.iter_mut().skip(512) {
                *entry = 1.0;
            }
        }
        vector
    })
}

/// D100 item 3 (operator 2026-11-05) — the WRITE-side lift: a PRIVATE
/// turn's chunks embed through the LOCAL port (the private pair) and
/// the configured cloud embedder never sees them — zero cloud calls.
/// A public turn's chunks embed through the configured port as before
/// (and add no local calls).
#[tokio::test]
async fn private_turn_chunks_embed_via_the_local_port_not_the_configured_embedder() {
    let (cloud_calls, cloud_embed) = counting_embed_fn();
    let (local_calls, local_embed) = counting_embed_fn();
    let port = in_memory_port_with_private_embed_fn(cloud_embed, local_embed);

    let private_record = TurnRecord {
        thread_id: "local-embed-private".to_string(),
        user_input: "private question that must not cross to the cloud embedder".to_string(),
        agent_response: "private answer embedded locally only".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };
    port.ingest_turn(private_record)
        .await
        .expect("private ingest succeeds");
    assert_eq!(
        cloud_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a private turn never reaches the configured cloud embedder"
    );
    let local_after_private = local_calls.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        local_after_private > 0,
        "a private turn's chunks embed through the local port (item 3's lift)"
    );

    let public_record = TurnRecord {
        thread_id: "cloud-embed-public".to_string(),
        user_input: "public question that may embed".to_string(),
        agent_response: "public answer embedded".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(public_record)
        .await
        .expect("public ingest succeeds");
    assert!(
        cloud_calls.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "a public turn embeds through the configured port as usual"
    );
    assert_eq!(
        local_calls.load(std::sync::atomic::Ordering::SeqCst),
        local_after_private,
        "a public turn adds no local-port calls"
    );
}

/// The WRITE-side degrade PERSISTS when the local pair is unwired (the
/// explicit opt-out or an unset private model): a private turn's chunks
/// stay keyword-recallable only and the configured cloud embedder sees
/// zero calls — the original D100 degrade-not-leak contract, now
/// scoped to the unwired case.
#[tokio::test]
async fn private_turn_chunks_stay_keyword_only_when_no_local_port_is_wired() {
    let (embed_calls, counting_embed) = counting_embed_fn();
    let port = in_memory_port_with_embed_fn(counting_embed);

    let private_record = TurnRecord {
        thread_id: "no-local-port-private".to_string(),
        user_input: "private question that must not embed".to_string(),
        agent_response: "private answer stored keyword-only".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };
    port.ingest_turn(private_record)
        .await
        .expect("private ingest succeeds");
    assert_eq!(
        embed_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an unwired local leg degrades keyword-only — never the cloud embedder"
    );
}

/// D100 item 3 — the READ-side lift: a private-marked context's QUERY
/// embeds through the LOCAL port (zero cloud calls) and the KNN leg is
/// LIVE for private content: a query with NO keyword overlap with the
/// stored chunk still recalls it — the only path to that snippet is
/// the locally-embedded KNN join (the unwired control below finds
/// nothing for the same query).
#[tokio::test]
async fn private_context_recall_embeds_the_query_via_the_local_port() {
    let (cloud_calls, cloud_embed) = counting_embed_fn();
    let local_embed =
        knn_direction_fixture_embed_fn(&["confidential-project-alpha", "vector-probe-xray"]);
    let port = in_memory_port_with_private_embed_fn(cloud_embed, local_embed);

    let private_record = TurnRecord {
        thread_id: "knn-private-thread".to_string(),
        user_input: "private discussion of the confidential-project-alpha roadmap".to_string(),
        agent_response: "private answer about milestones and sequencing".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };
    port.ingest_turn(private_record)
        .await
        .expect("private ingest succeeds");

    // The query shares NO word with the chunk content — only the
    // locally-embedded KNN leg can find it.
    let snippets = port
        .recall_context_curator("vector-probe-xray unrelated phrasing", 10, true)
        .await
        .expect("private recall succeeds");
    assert_eq!(
        cloud_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a private context's prompt never crosses to the configured cloud embedder"
    );
    assert!(
        snippets
            .iter()
            .any(|s| s.text.contains("confidential-project-alpha")),
        "the locally-embedded KNN leg recalls the private chunk: {:?}",
        snippets.iter().map(|s| s.text.as_str()).collect::<Vec<_>>()
    );

    // The falsifier control: the SAME no-overlap query against the
    // SAME content with the local pair UNWIRED finds nothing — proving
    // the snippet above arrived through the local embed, not the
    // keyword leg (and the cloud embedder still saw zero calls).
    let (unwired_calls, unwired_embed) = counting_embed_fn();
    let unwired_port = in_memory_port_with_embed_fn(unwired_embed);
    let unwired_record = TurnRecord {
        thread_id: "knn-unwired-thread".to_string(),
        user_input: "private discussion of the confidential-project-alpha roadmap".to_string(),
        agent_response: "private answer about milestones and sequencing".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };
    unwired_port
        .ingest_turn(unwired_record)
        .await
        .expect("unwired ingest succeeds");
    let empty = unwired_port
        .recall_context_curator("vector-probe-xray unrelated phrasing", 10, true)
        .await
        .expect("unwired recall succeeds");
    assert!(
        empty.is_empty(),
        "unwired, the no-overlap query finds nothing — the recall above was KNN"
    );
    assert_eq!(
        unwired_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "unwired, the cloud embedder still sees zero private calls"
    );
}

/// The READ-side degrade PERSISTS when the local pair is unwired: a
/// private context's query never embeds (zero cloud calls) and the
/// keyword leg still serves the private rows — degrade, not loss —
/// while a public context's query embeds as usual.
#[tokio::test]
async fn private_context_recall_stays_keyword_only_when_no_local_port_is_wired() {
    let (embed_calls, counting_embed) = counting_embed_fn();
    let port = in_memory_port_with_embed_fn(counting_embed);

    let private_record = TurnRecord {
        thread_id: "private-query-thread".to_string(),
        user_input: "a private prompt about zephyr reconnection".to_string(),
        agent_response: "a private answer with a distinctive keyword quokka".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PrivateInference,
    };
    port.ingest_turn(private_record)
        .await
        .expect("private ingest succeeds");

    // A private-context recall still finds its private rows (the
    // keyword leg) while NEVER embedding the query.
    let snippets = port
        .recall_context_curator("zephyr reconnection", 10, true)
        .await
        .expect("private recall succeeds");
    assert_eq!(
        embed_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an unwired local leg never widens a private context's query to the configured embedder"
    );
    assert!(
        snippets.iter().any(|s| s.text.contains("quokka")),
        "the keyword leg still recalls the private row — degrade, not loss: {:?}",
        snippets.iter().map(|s| s.text.as_str()).collect::<Vec<_>>()
    );

    // A public context's query embeds as usual.
    let _ = port
        .recall_context_curator("any public query", 10, false)
        .await
        .expect("public recall succeeds");
    assert!(
        embed_calls.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "a public context's query embeds as usual"
    );
}

/// FR-4, the other direction: public-thread content stays
/// `PublicProvider` — the default is the honest history, and the stamp
/// never widens to unmarked content.
#[tokio::test]
async fn ingest_turn_leaves_public_turn_chunks_public() {
    let port = in_memory_port();
    let record = TurnRecord {
        thread_id: "public-thread".to_string(),
        user_input: "What runs on the public leg?".to_string(),
        agent_response: "This content is unmarked.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    port.ingest_turn(record).await.expect("ingest succeeds");
    let chunks = port
        .curator_store
        .get()
        .expect("curator store")
        .query_deduped_untouched("curator:thread:public-thread")
        .expect("query should succeed");
    assert!(!chunks.is_empty(), "the turn produced chunks");
    for chunk in &chunks {
        assert_eq!(
            chunk.inference_provenance,
            hkask_types::InferenceProvenance::PublicProvider,
            "a public thread's content stays unmarked"
        );
    }
}

/// The adapter pin: the zed-side D100 flag maps to the record's
/// provenance at the bridge seam — the producing thread's routing state
/// is the ONLY source of the mark.
#[tokio::test]
async fn bridge_ingest_turn_maps_the_d100_flag_to_the_mark() {
    use agent::ThreadMemoryPort as _;

    let inner = in_memory_port();
    // Keep a handle to the PRIVATE store the bridge's inner port routes
    // the D100-flagged turn to (the store split — operator ruling
    // 2026-11-05); the inner port moves into the Arc<dyn MemoryPort>.
    let store_handle = inner.private_curator_store.clone();
    let inner: std::sync::Arc<dyn MemoryPort> = std::sync::Arc::new(inner);
    let bridge = BridgeMemoryPort::new(inner);

    let record = agent::ThreadTurnRecord {
        thread_id: "d100-adapter-thread".to_string(),
        user_input: "private thread turn".to_string(),
        agent_response: "produced on the private leg".to_string(),
        model: "private-model".to_string(),
        thread_title: None,
        agent_id: None,
        private_inference: true,
    };
    bridge.ingest_turn(record).await.expect("bridge ingest");

    let chunks = store_handle
        .get()
        .expect("curator store")
        .query_deduped_untouched("curator:thread:d100-adapter-thread")
        .expect("query should succeed");
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert_eq!(
            chunk.inference_provenance,
            hkask_types::InferenceProvenance::PrivateInference,
            "the D100 flag became the stored mark at the bridge seam"
        );
    }
}

#[tokio::test]
async fn sheddable_count_excludes_the_additive_knowledge_layer() {
    use hkask_regulation::MemoryHealthSource as _;

    let port = in_memory_port();
    let store = port.curator_store.get().expect("curator store");
    let webid = port.curator_webid;

    // The additive-only state: 5,000 knowledge rows (no entity prefix —
    // the bare-name / bare-slug knowledge classes), 0 sheddable rows.
    // Above the old whole-store 4,000 ceiling.
    for i in 0..5_000 {
        let entity = format!("knowledge-tier-{i}");
        let knowledge = hkask_storage::HMem::new(
            entity.as_str(),
            "lesson",
            serde_json::Value::String(format!("knowledge lesson {i}")),
            webid,
        );
        store.store(knowledge).expect("seed knowledge h_mem");
    }
    assert_eq!(
        store.h_mem_count().expect("total count"),
        5_000,
        "fixture: the additive-only state is above the old whole-store ceiling"
    );
    assert_eq!(
        port.sheddable_h_mem_count().await,
        Some(0),
        "knowledge-only growth must not reach the growth sensor — \
         distillation working is not a valve stall"
    );

    // Adding sheddable rows moves the sensed count — both prefixes.
    let turn = hkask_storage::HMem::new(
        "curator:thread:shed-thread",
        "chunk:0",
        serde_json::Value::String("turn storage row".to_string()),
        webid,
    );
    store.store(turn).expect("seed turn h_mem");
    let goal = hkask_storage::HMem::new(
        "curator:goal:shed-goal",
        "kanban_goal_create",
        serde_json::json!({"goal_id": "shed-goal"}),
        webid,
    );
    store.store(goal).expect("seed goal h_mem");
    assert_eq!(
        port.sheddable_h_mem_count().await,
        Some(1),
        "only turns have a live forgetting valve; goal projections are not acknowledge-purged"
    );
}

#[tokio::test]
async fn ingest_turn_writes_h_mems_at_confidence_floor() {
    // Turn chunks enter at the 0.5 floor — the same floor
    // `memory_insert` starts distilled memories at — so recall ranking
    // can discriminate and the consolidation floor is reachable. The
    // `HMem::new` default of 1.0 starved both consumers of confidence.
    let port = in_memory_port_with_embeddings();
    let record = TurnRecord {
        thread_id: "confidence-floor-thread".to_string(),
        user_input: "check the write confidence".to_string(),
        agent_response: "written at 0.5".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    port.ingest_turn(record)
        .await
        .expect("ingest should succeed");
    let curator_store = port.curator_store.get().expect("curator store");

    let assert_floor = |h_mems: &[hkask_storage::HMem], label: &str| {
        assert!(!h_mems.is_empty(), "{label} must have been written");
        for h_mem in h_mems {
            assert!(
                (h_mem.confidence.value() - 0.5).abs() < 1e-9,
                "{label} must carry the 0.5 floor"
            );
        }
    };
    let chunks = curator_store
        .query_deduped_untouched("curator:thread:confidence-floor-thread")
        .expect("query should succeed");
    assert_floor(&chunks, "chunk h_mems");
}

/// Two turns of one thread with identical text are two memories, not
/// duplicates: the chunk key is turn-scoped, so neither recall nor
/// value-dedup can collapse them.
#[tokio::test]
async fn repeated_turns_keep_distinct_chunk_keys() {
    let port = in_memory_port();
    let record = || TurnRecord {
        thread_id: "repeat-thread".to_string(),
        user_input: "continue".to_string(),
        agent_response: "Continuing.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record()).await.expect("first turn");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    port.ingest_turn(record()).await.expect("second turn");

    let curator_store = port.curator_store.get().expect("curator store");
    let chunks = curator_store
        .query_deduped_untouched("curator:thread:repeat-thread")
        .expect("query should succeed");
    assert_eq!(chunks.len(), 2, "both turns are recalled: {chunks:?}");
    assert_ne!(chunks[0].attribute, chunks[1].attribute);
}

#[tokio::test]
async fn ingest_turn_chunks_are_process_anchored() {
    let port = in_memory_port();
    let record = TurnRecord {
        thread_id: "test-thread-2".to_string(),
        user_input: "Explain async Rust".to_string(),
        agent_response: "Async Rust uses tokio.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    let result = port.ingest_turn(record).await;
    assert!(result.is_ok());

    // Chunks are process-anchored (PKO): they are steps of the chat
    // procedure, not free-standing documents.
    let curator_store = port.curator_store.get().expect("curator store");
    let h_mems = curator_store
        .query_deduped("curator:thread:test-thread-2")
        .expect("query should succeed");
    assert_eq!(h_mems.len(), 1, "one chunk h_mem should be stored");
    assert!(h_mems[0].attribute.starts_with("chunk:") && h_mems[0].attribute.ends_with(":0"));
    let ontology = h_mems[0]
        .ontology
        .as_ref()
        .expect("chunk carries an ontology blob");
    assert_eq!(ontology.dc_type, hkask_bridge_ontology::pko::STEP_EXECUTION);
    assert_eq!(ontology.pko_procedure.as_deref(), Some("chat"));
}

#[tokio::test]
async fn ingest_turn_stores_shared_copy_for_zed_agent_turn() {
    // Non-Curator (e.g. Zed agent) turns must be ingested into the
    // curator's shared store so the curator can recall what happened
    // across all agents. Previously, the is_curator_turn filter gated
    // all three write steps, so Zed agent work was invisible to the
    // curator — the memory stayed empty.
    let port = in_memory_port();
    let record = TurnRecord {
        thread_id: "zed-agent-thread".to_string(),
        user_input: "Fix the memory ingestion bug".to_string(),
        agent_response: "I narrowed the is_curator_turn filter.".to_string(),
        model: "test-model".to_string(),
        thread_title: Some("Memory fix".to_string()),
        agent_id: Some("Zed Agent".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    let result = port.ingest_turn(record).await;
    assert!(result.is_ok(), "zed agent ingest should succeed");

    // The shared chunks (curator:thread:...) must be present.
    let curator_store = port.curator_store.get().expect("curator store");
    let h_mems = curator_store
        .query_deduped("curator:thread:zed-agent-thread")
        .expect("query should succeed");
    assert_eq!(
        h_mems.len(),
        1,
        "shared chunks must be stored for zed agent turns"
    );
    assert!(h_mems[0].attribute.starts_with("chunk:") && h_mems[0].attribute.ends_with(":0"));
}

#[tokio::test]
async fn ingest_turn_skips_curator_copy_when_store_absent() {
    // Simulate the curator DB being unavailable — the curator store is
    // `None`. Ingestion of a curator turn should still succeed (Ok),
    // but no records are written since there's no store to write to.
    let port = in_memory_port();
    port.curator_store.set_for_tests(None);
    let record = turn_record(
        "test-no-curator",
        "What is memory?",
        "Memory is persistence across time.",
        "Curator",
    );

    let result = port.ingest_turn(record).await;
    assert!(
        result.is_ok(),
        "ingest should succeed without curator store"
    );

    // Nothing was written — the store is still absent.
    assert!(port.curator_store.get().is_none());
}

/// The shared shape of the two P9 embedding-degradation reports: a
/// Curator turn through `write_turn` with the embedding capability in
/// the given state, expecting non-fatal ingestion and a degraded
/// report with one stored, un-embedded chunk. Returns the report for
/// the caller's distinct field assert.
async fn degraded_write_report(
    port: &RealMemoryPort,
    thread_id: &str,
    embedding_port: Option<&LanguageModelEmbeddingPort>,
) -> ingest::IngestionReport {
    let ctx = write_context(port, embedding_port);
    let record = turn_record(
        thread_id,
        "remember this turn",
        "the conversation continues",
        "Curator",
    );
    let report = ingest::write_turn(&ctx, record)
        .await
        .expect("degraded ingestion remains non-fatal");
    assert_eq!(report.attempted, 1);
    assert_eq!(report.stored, 1);
    assert_eq!(report.embedded, 0);
    assert!(report.degraded());
    report
}

/// expect: [P9] A failed embedding is reported as partial ingestion while the stored turn remains non-fatal.
#[tokio::test]
async fn write_turn_reports_non_blocking_partial_embedding_failure() {
    let port = in_memory_port();
    let report = degraded_write_report(
        &port,
        "surfaced-embedding-degradation",
        port.embedding_port.as_ref(),
    )
    .await;
    assert_eq!(report.failed, 1);
}

/// expect: [P9] An unavailable embedding capability is reported as degraded while chunk storage remains non-fatal.
#[tokio::test]
async fn write_turn_reports_unavailable_embedding_capability() {
    let port = in_memory_port();
    let report = degraded_write_report(&port, "unavailable-embedding-capability", None).await;
    assert_eq!(report.degraded, 1);
}

#[tokio::test]
async fn ingest_turn_handles_empty_prompt_gracefully() {
    let port = in_memory_port();
    let record = turn_record("test-empty", "", "Response", "Curator");

    let result = port.ingest_turn(record).await;
    assert!(result.is_ok(), "empty prompt should not fail ingestion");

    let curator_store = port.curator_store.get().expect("curator store");
    let h_mems = curator_store
        .query_deduped_untouched("curator:thread:test-empty")
        .expect("query should succeed");
    assert_eq!(h_mems.len(), 1, "chunk h_mems should still be stored");
}

#[tokio::test]
async fn ingest_turn_embeds_every_chunk_with_passage_text() {
    let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
        let mut vector = vec![0.0f32; 1024];
        vector[0] = 1.0;
        vector
    });
    let port = in_memory_port_with_embed_fn(embed_fn);
    let record = turn_record(
        "embedding-round-trip",
        "check the embedding round trip",
        "the round trip is verified",
        "Curator",
    );
    port.ingest_turn(record).await.expect("ingest succeeds");

    let curator_store = port.curator_store.get().expect("curator store");
    let chunks = curator_store
        .query_deduped_untouched("curator:thread:embedding-round-trip")
        .expect("query should succeed");
    assert_eq!(chunks.len(), 1);
    let chunk_text = chunks[0].value.as_str().expect("chunk text").to_string();

    let embeddings = curator_store
        .all_embeddings_with_text()
        .expect("embeddings query should succeed");
    assert!(
        embeddings.iter().any(|(entity_ref, _, passage, _)| {
            entity_ref == "curator:thread:embedding-round-trip"
                && passage.as_deref() == Some(chunk_text.as_str())
        }),
        "every chunk must have an embedding whose passage_text is the chunk text"
    );
}

/// Chunk values are bounded: a huge turn becomes multiple chunks, each
/// within the word ceiling. The 538KB single-value rows the therapy scan
/// found must not be reproducible.
#[tokio::test]
async fn ingest_turn_chunk_values_are_bounded() {
    let port = in_memory_port();
    let long_response = "word ".repeat(2000);
    let record = turn_record(
        "huge-turn",
        "dump something enormous",
        &long_response,
        "Curator",
    );
    port.ingest_turn(record).await.expect("ingest succeeds");

    let curator_store = port.curator_store.get().expect("curator store");
    let chunks = curator_store
        .query_deduped_untouched("curator:thread:huge-turn")
        .expect("query should succeed");
    assert!(
        chunks.len() > 1,
        "a 2000-word turn must split into multiple chunks"
    );
    // The shared chunk contract enforces the configured ceiling exactly;
    // no structural boundary may make a chunk exceed it.
    let word_ceiling = crate::memory::ingest::MAX_CHUNK_WORDS;
    for (index, chunk) in chunks.iter().enumerate() {
        let text = chunk.value.as_str().expect("chunk text");
        let words = text.split_whitespace().count();
        assert!(
            words <= word_ceiling,
            "chunk {index} has {words} words — the ceiling is {word_ceiling}"
        );
    }
}

#[tokio::test]
async fn ingest_turn_does_not_fire_consolidation() {
    // Consolidation is decoupled from ingestion — it runs only on the
    // background timer. A below-floor h_mem must survive ingestion
    // untouched: only a consolidation pass may delete it, and none fires
    // from this path.
    let port = in_memory_port_with_cadence(1, 0.3);
    let store = port.curator_store.get().expect("curator store");
    store
        .store(below_floor_probe(port.curator_webid))
        .expect("seed below-floor h_mem");

    let record = turn_record(
        "no-consolidation-from-ingest",
        "Tell me about memory consolidation",
        "Consolidation promotes episodic to semantic.",
        "Curator",
    );
    port.ingest_turn(record.clone())
        .await
        .expect("ingest_turn should succeed");
    port.ingest_turn(record)
        .await
        .expect("ingest_turn should succeed");

    assert_eq!(
        store
            .h_mems_by_entity_prefix("probe:low")
            .expect("query")
            .len(),
        1,
        "ingest_turn must not fire a consolidation pass — the below-floor \
         h_mem survives until the timer fires"
    );
}

/// expect: "The production consolidation timer fires real passes — the
/// wiring zed's `main.rs` starts is live behavior, not a started task
/// that can never fire." [P1]
/// pre: an in-memory port (cadence 60s, floor 0.3) whose store holds one
/// below-floor h_mem; the production `start_consolidation_timer` runs on
/// a paused tokio runtime, so the clamped 60s poll elapses instantly.
/// post: the first non-skipped tick fires a consolidation pass and
/// deletes the below-floor h_mem — the timer's observable production
/// effect. A timer whose first fire is unreachable (the pre-fix no-op:
/// `last = None` with a wait-for-last flag that never resolves) fails
/// here against the wall-clock bound instead of hanging forever.
#[tokio::test(start_paused = true)]
async fn consolidation_timer_fires_and_prunes_low_confidence() {
    let port = in_memory_port_with_cadence(60, 0.3);
    let store = port.curator_store.get().expect("curator store");
    store
        .store(below_floor_probe(port.curator_webid))
        .expect("seed below-floor h_mem");
    assert_eq!(store.h_mem_count().expect("count"), 1);

    let timer = port
        .start_consolidation_timer()
        .expect("timer starts for cadence > 0");

    // Paused time auto-advances the interval; the wait is bounded in WALL
    // time so a timer that never fires fails loudly, not hangs.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while store.h_mem_count().expect("count") > 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "consolidation timer never fired — the below-floor h_mem survived"
        );
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
    timer.abort();
}

/// expect: "My two-hour consolidation cadence is not shortened to one hour." [P1]
#[tokio::test(start_paused = true)]
async fn consolidation_timer_respects_cadences_longer_than_one_hour() {
    let port = in_memory_port_with_cadence(7200, 0.3);
    let store = port.curator_store.get().expect("curator store");
    store
        .store(
            hkask_storage::HMem::new(
                "probe:long-cadence",
                "fact",
                serde_json::json!("low confidence"),
                port.curator_webid,
            )
            .with_confidence(hkask_types::Confidence::new(0.1)),
        )
        .expect("seed probe");
    let timer = port.start_consolidation_timer().expect("timer");
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(3600)).await;
    tokio::task::yield_now().await;
    let count_at_one_hour = store.h_mem_count().expect("count");
    tokio::time::advance(Duration::from_secs(3600)).await;
    tokio::task::yield_now().await;
    let count_at_two_hours = store.h_mem_count().expect("count");
    timer.abort();
    assert_eq!(count_at_one_hour, 1, "must wait the configured two hours");
    assert_eq!(count_at_two_hours, 0, "must consolidate at two hours");
}

#[tokio::test]
async fn start_consolidation_timer_is_disabled_at_zero_cadence() {
    // Cadence 0 — consolidation is disabled: no timer is started at all.
    let port = in_memory_port_with_cadence(0, 0.3);
    assert!(
        port.start_consolidation_timer().is_none(),
        "a cadence of 0 must disable consolidation entirely"
    );
}

/// The keyword search loads h_mems once per recall call, not once per
/// query word.
///
/// We can't easily count SQL queries from here, but we can verify the
/// observable consequence: `recall_context_curator` returns snippets that
/// match ANY query word (not just the last one), and the recall completes
/// in reasonable time.
#[tokio::test]
async fn recall_context_matches_any_query_word_single_load() {
    let port = in_memory_port();

    // Ingest two curator turns with distinct keywords so we can verify
    // both match.
    let records = [
        TurnRecord {
            thread_id: "t-rust".to_string(),
            user_input: "Tell me about rust programming".to_string(),
            agent_response: "Rust is a systems language.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
        },
        TurnRecord {
            thread_id: "t-python".to_string(),
            user_input: "Tell me about python programming".to_string(),
            agent_response: "Python is a scripting language.".to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Curator".to_string()),
            inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
        },
    ];
    for record in records {
        port.ingest_turn(record).await.expect("ingest succeeds");
    }

    // Query with two distinct keywords — both should match.
    let snippets = port
        .recall_context_curator("rust python", 10, false)
        .await
        .expect("recall succeeds");

    // Both turns should be recalled — the fix loads h_mems once and
    // checks all query words against each.
    let texts: Vec<&str> = snippets.iter().map(|s| s.text.as_str()).collect();
    let has_rust = texts.iter().any(|t| t.contains("rust"));
    let has_python = texts.iter().any(|t| t.contains("python"));
    assert!(
        has_rust && has_python,
        "recall should match ANY query word, got: {snippets:?}"
    );
}

/// expect: "Keyword recall prefers an episode matching more of my query, not a fixed relevance score." [P1]
#[tokio::test]
async fn recall_ranks_keyword_episodes_by_overlap() {
    let port = in_memory_port();
    let store = port.curator_store.get().expect("store");
    for (entity, text, confidence) in [
        (
            "curator:thread:strong",
            "The orchard harvest was abundant.",
            0.6,
        ),
        ("curator:thread:weak", "The orchard road was repaired.", 1.0),
    ] {
        store
            .store(
                hkask_storage::HMem::new(
                    entity,
                    "chunk",
                    serde_json::json!(text),
                    port.curator_webid,
                )
                .with_confidence(hkask_types::Confidence::new(confidence)),
            )
            .expect("seed episode");
    }
    let snippets = port
        .recall_context_curator("orchard harvest", 2, false)
        .await
        .expect("recall");
    assert_eq!(snippets.len(), 2);
    assert_eq!(
        snippets.first().expect("best").entity,
        "curator:thread:strong"
    );
    assert_eq!(snippets.first().expect("best").relevance_score, 0.5);
    assert_eq!(snippets.last().expect("second").relevance_score, 0.25);
}

/// A failed embedding call must degrade recall to the keyword leg —
/// never error, never lose keyword-matched turns. This pins the
/// behavioral contract of the embed-failure branch in `recall_from`:
/// a dead embedding endpoint costs semantic recall only.
#[tokio::test]
async fn recall_degrades_to_keyword_leg_when_embedding_fails() {
    let port = in_memory_port();

    // Ingest a curator turn whose text contains a distinctive keyword.
    // The `for_tests()` port fails every embed, so the semantic leg is
    // dead and the keyword leg is the only path to this turn.
    port.ingest_turn(TurnRecord {
        thread_id: "embed-failure-degradation".to_string(),
        user_input: "kangaroo wallaby emu cassowary".to_string(),
        agent_response: "distinctive keyword quokka".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    })
    .await
    .expect("ingest succeeds despite embed failure");

    // Recall must succeed (not error) and find the turn via the keyword
    // leg — "quokka" appears in the stored text and passes the >3-char
    // word filter.
    let snippets = port
        .recall_context_curator("quokka habitat", 10, false)
        .await
        .expect("recall must not error when the embedding endpoint is down");
    assert!(
        snippets.iter().any(|s| s.text.contains("quokka")),
        "keyword recall must survive embed failure — got: {snippets:?}"
    );
}

// The embed-failure degradation holds for arbitrary queries: whatever
// the query, recall with a dead embedding endpoint never errors and
// returns only keyword-leg matches (a snippet with no shared >3-char
// word can only come from the semantic leg, which is dead). Each case
// builds its own current-thread runtime — `in_memory_port` captures
// `Handle::current()` for the embed spawn.
mod embed_failure_prop {
    use super::*;
    use proptest::prop_assert;

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 64,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn query_returns_keyword_leg_matches_only(
            query in proptest::string::string_regex(r"[a-z ]{4,40}").unwrap()
        ) {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("test runtime builds");
            let snippets = runtime.block_on(async {
                let port = in_memory_port();
                port.ingest_turn(TurnRecord {
                    thread_id: "prop-embed-failure".to_string(),
                    user_input: "the quick brown fox jumps".to_string(),
                    agent_response: "lazy dog response".to_string(),
                    model: "test-model".to_string(),
                    thread_title: None,
                    agent_id: Some("Curator".to_string()),
                    inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
                })
                .await
                .expect("ingest succeeds");

                port.recall_context_curator(&query, 10, false)
                    .await
                    .expect("recall must never error on embed failure")
            });

            let query_words: Vec<String> = query
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .map(|w| w.to_lowercase())
                .collect();
            for snippet in &snippets {
                let text = snippet.text.to_lowercase();
                let shares_word = query_words.iter().any(|w| text.contains(w));
                prop_assert!(
                    shares_word || query_words.is_empty(),
                    "snippet returned without keyword overlap — the semantic \
                     leg must be dead when embed fails: {snippet:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn object_thread_memory_uses_one_passage_across_recall_legs() {
    let semantic_port = in_memory_port_with_embeddings();
    let keyword_port = in_memory_port();
    let mut passage = String::new();
    for port in [&semantic_port, &keyword_port] {
        let store = port.curator_store.get().expect("curator store");
        let write = store
            .prepare_semantic_write(hkask_storage::HMem::new(
                "curator:thread:structured",
                "finding",
                serde_json::json!({"resolution": "ruby telescope calibration"}),
                port.curator_webid,
            ))
            .expect("prepare structured memory");
        passage = write.passage().to_string();
        write
            .publish(&vec![0.25; 1024], "test-model")
            .expect("publish structured memory");
    }
    let semantic = semantic_port
        .recall_context_curator("wombat guava", 10, false)
        .await
        .expect("KNN recall");
    let keyword = keyword_port
        .recall_context_curator("ruby telescope", 10, false)
        .await
        .expect("keyword fallback");
    let thread = keyword_port
        .recall_thread_curator("structured", 10, false)
        .await
        .expect("exact thread recall");
    for (label, snippets) in [
        ("semantic", semantic),
        ("keyword", keyword),
        ("thread", thread),
    ] {
        assert!(
            snippets.iter().any(|snippet| snippet.text == passage),
            "{label} must read the canonical passage: {snippets:?}"
        );
    }
}

#[tokio::test]
async fn projected_keyword_recall_keeps_thread_scope_and_mutable_boundary() {
    let port = in_memory_port();
    let store = port.curator_store.get().expect("curator store");
    for (entity, attribute, value) in [
        (
            "curator:thread:scope",
            "plain",
            serde_json::json!("quokka legacy passage"),
        ),
        (
            "curator:thread:scope",
            "unsupported",
            serde_json::json!({"mutable_state": true, "text": "quokka unbounded claim"}),
        ),
        (
            "knowledge:outside",
            "finding",
            serde_json::json!({"recall_text": "quokka non-thread knowledge"}),
        ),
    ] {
        store
            .store(hkask_storage::HMem::new(
                entity,
                attribute,
                value,
                port.curator_webid,
            ))
            .expect("seed scope fixture");
    }
    let keyword = port
        .recall_context_curator("quokka", 10, false)
        .await
        .expect("keyword recall");
    let thread = port
        .recall_thread_curator("scope", 10, false)
        .await
        .expect("thread recall");
    for snippets in [keyword, thread] {
        assert_eq!(
            snippets.len(),
            1,
            "scope and unsupported mutable rows must not widen: {snippets:?}"
        );
        assert_eq!(snippets[0].text, "quokka legacy passage");
    }
}

/// The semantic (embedding KNN) recall leg works end-to-end: the
/// embedding's `entity_ref` joins to the turn's h_mem.
///
/// This test isolates the semantic leg from the keyword leg by using a
/// stub embedding function that returns the same unit vector for any
/// non-empty input, so every query is a KNN match for every stored
/// embedding (cosine distance 0). The query shares NO words with the
/// stored turn, so the keyword leg misses — the only path to recall is
/// the semantic leg.
#[tokio::test]
async fn recall_context_finds_turn_by_embedding_only() {
    // Constant embedding: every text maps to the same unit vector. KNN
    // search returns every stored embedding at cosine distance 0, so the
    // semantic leg always finds every stored turn regardless of word
    // overlap. The keyword leg is the only thing that could miss.
    let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    });
    let port = in_memory_port_with_embed_fn(embed_fn);

    // Ingest a curator turn whose text contains none of the query words.
    let record = TurnRecord {
        thread_id: "t-unique-thread-id".to_string(),
        user_input: "alpha beta gamma delta epsilon".to_string(),
        agent_response: "zeta eta theta iota kappa".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record).await.expect("ingest succeeds");

    // Query with words that share NO tokens with the stored turn. All
    // query words are >3 chars (pass the keyword filter) but none appear
    // in the stored text — the keyword leg returns nothing. The semantic
    // leg must find the turn via KNN (constant embedding → distance 0).
    let snippets = port
        .recall_context_curator("kangaroo wallaby emu cassowary", 10, false)
        .await
        .expect("recall succeeds");
    assert!(
        snippets.iter().any(|s| s.text.contains("alpha beta gamma")),
        "semantic-only recall should find the turn despite zero word overlap, got: {snippets:?}"
    );
}

/// Non-curator (zed agent) turns are findable by embedding-only recall.
#[tokio::test]
async fn recall_context_finds_zed_agent_turn_by_embedding_only() {
    // Constant embedding — every query is a KNN match for every stored
    // embedding, so the only path that can miss is the entity_ref join.
    let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    });
    let port = in_memory_port_with_embed_fn(embed_fn);

    // A NON-curator turn — the case whose embedding used to be orphaned.
    let record = TurnRecord {
        thread_id: "t-zed-agent-thread-id".to_string(),
        user_input: "omega psi chi upsilon".to_string(),
        agent_response: "the join now resolves".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Zed Agent".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record).await.expect("ingest succeeds");

    // Zero-overlap query — the only path to the turn is the semantic leg.
    let snippets = port
        .recall_context_curator("kookaburra wombat quoll bilby", 10, false)
        .await
        .expect("recall succeeds");
    assert!(
        snippets.iter().any(|s| s.text.contains("omega psi chi")),
        "semantic-only recall should find the zed-agent turn — an orphan \
         embedding would make it invisible, got: {snippets:?}"
    );
}

/// expect: A held-out follow-up question ranks its relevant thread chunk ahead of a competing passage.
#[tokio::test]
async fn follow_up_question_ranks_expected_thread_chunk_first() {
    let embed_fn = Arc::new(|text: &str| -> Vec<f32> {
        let mut vector = vec![0.0f32; 1024];
        let axis = if text.contains("azure semaphore") || text.contains("zephyr readiness") {
            0
        } else {
            1
        };
        if let Some(value) = vector.get_mut(axis) {
            *value = 1.0;
        }
        vector
    });
    let port = in_memory_port_with_embed_fn(embed_fn);

    for (thread_id, user_input, agent_response) in [
        (
            "bridge-target-thread",
            "Record the release dependency.",
            "The azure semaphore must clear before deployment begins.",
        ),
        (
            "bridge-distractor-thread",
            "Record the catering preference.",
            "The green ledger lists tea and sandwiches for the workshop.",
        ),
    ] {
        port.ingest_turn(TurnRecord {
            thread_id: thread_id.to_string(),
            user_input: user_input.to_string(),
            agent_response: agent_response.to_string(),
            model: "test-model".to_string(),
            thread_title: None,
            agent_id: Some("Zed Agent".to_string()),
            inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
        })
        .await
        .expect("ingest succeeds");
    }

    let snippets = port
        .recall_context_curator("Which contingency governs zephyr readiness?", 2, false)
        .await
        .expect("recall succeeds");

    assert_eq!(snippets.len(), 2, "fixture must retain a competing passage");
    assert_eq!(snippets[0].entity, "curator:thread:bridge-target-thread");
    assert!(snippets[0].text.contains("azure semaphore"));
    assert!(snippets[0].relevance_score > snippets[1].relevance_score);
}

/// Confidence-weighted ranking test (Priority 1): when two memories
/// have similar embedding relevance but different confidence scores,
/// the higher-confidence memory should rank first. Before the fix,
/// the sort used `relevance_score` only — confidence was a threshold
/// filter, not a ranking signal.
///
/// Grounding: Dunning's double curse (`138299529:5`) — the model
/// can't self-evaluate, but confidence calibrated by outcomes IS
/// meaningful. Tetlock (`Superforecasting_tetlock:71`) — confidence
/// is a forecast. Using it as a ranking multiplier means a
/// well-calibrated memory outranks an untested one.
/// A `DatabaseDriver` wrapper that counts queries against a substring —
/// the regression seam for the connectedness-in-the-sort-comparator
/// defect (`recall_context_ranks_by_confidence_weighted_relevance`
/// exercises ranking, but nothing bounded the query count) — and records
/// the thread every statement ran on, the seam for keeping recall's
/// database work off the thread that sends a prompt.
struct QueryCountingDriver {
    inner: Arc<dyn hkask_storage::DatabaseDriver>,
    needle: &'static str,
    count: std::sync::atomic::AtomicUsize,
    statement_threads: std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
}

impl QueryCountingDriver {
    fn observe_statement(&self, sql: &str) {
        if sql.contains(self.needle) {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.statement_threads
            .lock()
            .expect("statement thread set")
            .insert(std::thread::current().id());
    }
}

impl hkask_storage::DatabaseDriver for QueryCountingDriver {
    fn execute(
        &self,
        sql: &str,
        params: &[hkask_storage::database::value::DbValue],
    ) -> Result<usize, hkask_types::DbError> {
        self.observe_statement(sql);
        self.inner.execute(sql, params)
    }
    fn execute_batch(&self, sql: &str) -> Result<(), hkask_types::DbError> {
        self.observe_statement(sql);
        self.inner.execute_batch(sql)
    }
    fn query(
        &self,
        sql: &str,
        params: &[hkask_storage::database::value::DbValue],
    ) -> Result<Vec<hkask_storage::database::value::DbRow>, hkask_types::DbError> {
        self.observe_statement(sql);
        self.inner.query(sql, params)
    }
    fn query_optional(
        &self,
        sql: &str,
        params: &[hkask_storage::database::value::DbValue],
    ) -> Result<Option<hkask_storage::database::value::DbRow>, hkask_types::DbError> {
        self.observe_statement(sql);
        self.inner.query_optional(sql, params)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }
    fn sqlite_pool(&self) -> Option<&r2d2::Pool<hkask_storage::SqliteConnectionManager>> {
        self.inner.sqlite_pool()
    }
    fn transaction(
        &self,
        operations: &mut dyn FnMut(
            &dyn hkask_storage::database::driver::TransactionOps,
        ) -> Result<(), hkask_types::DbError>,
    ) -> Result<(), hkask_types::DbError> {
        self.inner.transaction(operations)
    }
}

/// Connectedness must be fetched once per unique candidate entity, not
/// once per sort comparison. The previous implementation called
/// `connectedness` (a live SQLCipher query) inside `sort_by`'s comparator
/// — O(N log N) encrypted-DB queries on EVERY recall, and recall runs on
/// every prompt. With 20 candidates the comparator issued ~170
/// `memory_links` queries; the bound is 20 (one per unique entity).
#[tokio::test]
async fn recall_connectedness_queries_are_bounded_by_unique_entities() {
    let inner = SqliteDriver::in_memory_driver();
    let counting = Arc::new(QueryCountingDriver {
        inner,
        needle: "FROM memory_links",
        count: std::sync::atomic::AtomicUsize::new(0),
        statement_threads: Default::default(),
    });
    let h_mem_store =
        HMemStore::from_driver(Arc::clone(&counting) as Arc<dyn hkask_storage::DatabaseDriver>)
            .expect("curator hmem store init");
    let embedding_store = EmbeddingStore::from_driver(
        Arc::clone(&counting) as Arc<dyn hkask_storage::DatabaseDriver>,
        1024,
    )
    .expect("embedding store init");
    let store = Arc::new(MemoryStore::new(h_mem_store, embedding_store));

    let port = port_with(
        Arc::clone(&store),
        LanguageModelEmbeddingPort::for_tests(),
        None,
        0,
        0.3,
    );

    // 20 h_mems under distinct curator:thread:* entities (the keyword
    // leg's prefix), all matching the query word, all owned by the curator
    // perspective so the perspective-scoped prefix query returns them.
    let curator_webid = port.curator_webid;
    for index in 0..20 {
        store
            .store(
                hkask_storage::HMem::new(
                    &format!("curator:thread:t{index}"),
                    "chunk:0",
                    serde_json::json!(format!("turn {index} about xvantium")),
                    curator_webid,
                )
                .with_perspective(curator_webid),
            )
            .expect("seed turn");
    }

    let snippets = port
        .recall_context_curator("xvantium", 20, false)
        .await
        .expect("recall succeeds");
    assert_eq!(snippets.len(), 20, "all 20 turns should be recalled");

    let connectedness_queries = counting.count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        connectedness_queries, 20,
        "connectedness must run once per unique entity (20), not per sort \
         comparison (~170 with the comparator-query defect)"
    );
}

/// expect: "Sending a curator prompt never runs memory-database work on
/// the thread that sends it."
///
/// Context injection is awaited from the editor's foreground turn task,
/// so a statement it issues on the calling thread freezes the window for
/// as long as the statement takes (the hang record's 0.2–0.6 s send-time
/// stalls at normal CPU speed, 7–17 s under the 2026-11-10 frequency
/// clamp). Falsifier: any store statement observed on the calling thread.
#[tokio::test]
async fn context_injection_keeps_store_statements_off_the_calling_thread() {
    use agent::ContextInjector as _;

    let observing = Arc::new(QueryCountingDriver {
        inner: SqliteDriver::in_memory_driver(),
        needle: "FROM memory_links",
        count: std::sync::atomic::AtomicUsize::new(0),
        statement_threads: Default::default(),
    });
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = observing.clone();
    let store = Arc::new(MemoryStore::new(
        HMemStore::from_driver(Arc::clone(&driver)).expect("curator hmem store init"),
        EmbeddingStore::from_driver(driver, 1024).expect("embedding store init"),
    ));
    let port = port_with(
        Arc::clone(&store),
        LanguageModelEmbeddingPort::for_tests(),
        None,
        0,
        0.3,
    );

    // Turns on two threads that match the prompt, so prompt recall,
    // thread recall and the co-occurrence write (two distinct entities)
    // all issue statements.
    let curator_webid = port.curator_webid;
    for thread in ["alpha", "beta"] {
        store
            .store(
                hkask_storage::HMem::new(
                    &format!("curator:thread:{thread}"),
                    "chunk:0",
                    serde_json::json!(format!("{thread} turn about xvantium")),
                    curator_webid,
                )
                .with_perspective(curator_webid),
            )
            .expect("seed turn");
    }

    let injector = crate::BridgeContextInjector::new_curator(Arc::new(port), 10, 0.0, true);
    // Seeding ran on this thread; only the injection is under test.
    observing
        .statement_threads
        .lock()
        .expect("statement thread set")
        .clear();

    let messages = injector
        .inject_context(
            "alpha",
            "please explain the xvantium reconnection strategy",
            false,
        )
        .await;

    let injected: String = messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|content| match content {
            language_model_core::MessageContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        injected.contains("xvantium"),
        "the injection must recall the seeded turns: {injected}"
    );
    let threads = observing
        .statement_threads
        .lock()
        .expect("statement thread set");
    assert!(
        !threads.is_empty(),
        "the injection issued no observed store statements"
    );
    assert!(
        !threads.contains(&std::thread::current().id()),
        "memory-database statements ran on the thread that sent the prompt"
    );
}

#[tokio::test]
async fn recall_context_ranks_by_confidence_weighted_relevance() {
    // Constant embedding: both h_mems map to the same unit vector, so
    // they have identical relevance_score (1.0 - distance = 1.0). The
    // only differentiator is confidence.
    let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    });
    let port = in_memory_port_with_embed_fn(embed_fn);
    let curator_webid = port.curator_webid;

    // Store h_mems directly in the curator store with distinct entities
    // and confidence scores. The embedding store uses entity_ref as the
    // key, and query_deduped_untouched queries by entity — so each
    // h_mem needs its own entity, and each embedding's entity_ref must
    // match its h_mem's entity.
    let high_conf = hkask_storage::HMem::new(
        "test:ranking:high",
        "fact",
        serde_json::json!("alpha"),
        curator_webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.99));
    let low_conf = hkask_storage::HMem::new(
        "test:ranking:low",
        "fact",
        serde_json::json!("beta"),
        curator_webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.51));

    let unit_vec = {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    };
    let curator_store = port.curator_store.get().expect("curator store");
    curator_store
        .store_embedding("test:ranking:high", &unit_vec, "test-model", Some("alpha"))
        .expect("store embedding high");
    curator_store
        .store_embedding("test:ranking:low", &unit_vec, "test-model", Some("beta"))
        .expect("store embedding low");

    curator_store.store(high_conf).expect("store high");
    curator_store.store(low_conf).expect("store low");

    // Recall with a query that matches both (constant embedding →
    // both at distance 0 → both have relevance_score 1.0).
    let snippets = port
        .recall_context_curator("matching query", 10, false)
        .await
        .expect("recall succeeds");

    // Both should be recalled.
    assert_eq!(
        snippets.len(),
        2,
        "both h_mems should be recalled, got: {snippets:?}"
    );

    // The high-confidence one ("alpha") should rank first.
    assert_eq!(
        snippets[0].text,
        "alpha",
        "higher-confidence memory should rank first, got order: {:?}",
        snippets.iter().map(|s| &s.text).collect::<Vec<_>>()
    );
    assert_eq!(
        snippets[1].text, "beta",
        "lower-confidence memory should rank second"
    );
}

/// Pin that `recall_from` touches only the h_mems that survive the
/// limit, not on every recalled candidate. The previous implementation
/// touched every deduped h_mem inside `query_for_deduped` (removed
/// 2026-11-16), even ones filtered out by `recall_min_confidence` in the injector.
///
/// We verify this by checking that h_mems NOT in the final snippets have
/// their `recalled_at` unchanged after a recall that truncates them.
#[tokio::test]
async fn recall_context_touches_only_injected_h_mems() {
    let port = in_memory_port();

    // Ingest one curator turn.
    port.ingest_turn(TurnRecord {
        thread_id: "touch-test".to_string(),
        user_input: "unique_keyword_xyz".to_string(),
        agent_response: "response".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    })
    .await
    .expect("ingest succeeds");

    let curator_store = port.curator_store.get().expect("curator store");

    // Read the stored recalled_at via the untouched query (no side effects).
    let before = curator_store
        .query_deduped_untouched("curator:thread:touch-test")
        .expect("untouched query succeeds");
    assert_eq!(before.len(), 1);
    let recalled_at_before = before[0].recalled_at;

    // Sleep so a touch would be observable.
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Recall with a query that does NOT match the stored keyword — the
    // h_mem should be loaded as a candidate but NOT injected (no keyword
    // overlap), so its recalled_at should NOT be touched.
    let snippets = port
        .recall_context_curator("completely_different_query", 10, false)
        .await
        .expect("recall succeeds");
    assert!(
        snippets.is_empty(),
        "no snippets should match a non-overlapping query"
    );

    let after = curator_store
        .query_deduped_untouched("curator:thread:touch-test")
        .expect("untouched query succeeds");
    assert_eq!(after.len(), 1);
    assert_eq!(
        after[0].recalled_at, recalled_at_before,
        "recalled_at should be unchanged when the h_mem is not injected"
    );

    // Now recall with a matching query — the h_mem should be injected and
    // its recalled_at should be touched.
    std::thread::sleep(std::time::Duration::from_millis(10));
    let snippets = port
        .recall_context_curator("unique_keyword_xyz", 10, false)
        .await
        .expect("recall succeeds");
    assert_eq!(snippets.len(), 1, "matching query should recall the h_mem");

    let after_match = curator_store
        .query_deduped_untouched("curator:thread:touch-test")
        .expect("untouched query succeeds");
    assert_eq!(after_match.len(), 1);
    assert!(
        after_match[0].recalled_at > recalled_at_before,
        "recalled_at should be updated when the h_mem is injected"
    );
}

/// Pin the recall injection gate's one-way door and its recovery path —
/// the setting the UI labels "Recall Injection Gate (Decayed
/// Confidence)". A memory at the 0.5 insert floor under a 0.6 gate is
/// excluded from injection (the trap a gate above the floor springs:
/// every fresh memory invisible until updated), stays excluded on a
/// second attempt with no update, and re-enters once an explicit
/// confidence update lifts it above the gate.
///
/// Also pins the touch interaction, correcting the earlier analysis:
/// `recall_from` touches its top-limit survivors BEFORE the injector's
/// confidence filter, so a recalled-but-gated-out memory keeps its decay
/// clock — the door is stable exclusion, not a decaying spiral.
#[tokio::test]
async fn recall_gate_excludes_until_an_explicit_confidence_update_lifts_it() {
    use crate::context_injector::BridgeContextInjector;
    use agent::ContextInjector;
    use language_model_core::MessageContent;

    // Constant embedding: the memory matches every query at distance 0.
    let embed_fn = Arc::new(|_text: &str| -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    });
    let port = in_memory_port_with_embed_fn(embed_fn);
    let curator_webid = port.curator_webid;

    let gated = hkask_storage::HMem::new(
        "test:gate:memory",
        "fact",
        serde_json::json!("gated passage content"),
        curator_webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.5));
    let unit_vec = {
        let mut v = vec![0.0f32; 1024];
        v[0] = 1.0;
        v
    };
    let curator_store = port.curator_store.get().expect("curator store");
    curator_store
        .store_embedding(
            "test:gate:memory",
            &unit_vec,
            "test-model",
            Some("gated passage content"),
        )
        .expect("store embedding");
    curator_store.store(gated).expect("store gated");

    // The gate sits ABOVE the 0.5 insert floor — the configuration the
    // settings UI refuses, exercised here directly to pin its behavior.
    let injector = BridgeContextInjector::new_curator(Arc::new(port), 10, 0.6, true);
    let prompt = "a sufficiently long prompt about gated memory content for the recall gate test";

    let before = curator_store
        .query_deduped_untouched("test:gate:memory")
        .expect("untouched query");
    assert_eq!(before.len(), 1);
    let recalled_at_before = before[0].recalled_at;
    std::thread::sleep(std::time::Duration::from_millis(10));

    let messages = injector
        .inject_context("gate-test-thread", prompt, false)
        .await;
    let content = match &messages[0].content[0] {
        MessageContent::Text(t) => t.as_str(),
        _ => panic!("expected text content"),
    };
    assert!(
        !content.contains("gated passage content"),
        "a below-gate memory must not be injected: {content}"
    );
    assert!(
        content.contains("No relevant memory"),
        "the gate's visible outcome is the absence message: {content}"
    );

    // The touch interaction: recall_from touched its survivor even though
    // the injector gated it out — the decay clock keeps resetting while
    // the memory keeps ranking, so exclusion is stable, not a spiral.
    let after = curator_store
        .query_deduped_untouched("test:gate:memory")
        .expect("untouched query");
    assert_eq!(after.len(), 1);
    assert!(
        after[0].recalled_at > recalled_at_before,
        "a recalled-but-gated-out memory keeps its decay clock \
         (touch happens in recall_from, before the injector's filter)"
    );

    // Second attempt with no update: still excluded — the one-way door.
    let messages_again = injector
        .inject_context("gate-test-thread", prompt, false)
        .await;
    let content_again = match &messages_again[0].content[0] {
        MessageContent::Text(t) => t.as_str(),
        _ => panic!("expected text content"),
    };
    assert!(
        !content_again.contains("gated passage content"),
        "without a confidence update the memory stays below the gate: {content_again}"
    );

    // An explicit memory update can lift confidence above the gate.
    // Goal scores never perform this update.
    let lifted = hkask_storage::HMem::new(
        "test:gate:memory",
        "fact",
        serde_json::json!("gated passage content"),
        curator_webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.7));
    curator_store.store(lifted).expect("store lifted");

    let messages_lifted = injector
        .inject_context("gate-test-thread", prompt, false)
        .await;
    let content_lifted = match &messages_lifted[0].content[0] {
        MessageContent::Text(t) => t.as_str(),
        _ => panic!("expected text content"),
    };
    assert!(
        content_lifted.contains("gated passage content"),
        "a calibrated update above the gate is the recovery path: {content_lifted}"
    );
}

/// Pin that the ingestion semaphore serializes concurrent ingestions.
/// Two concurrent ingestions should both complete successfully, but the
/// second should wait for the first to release its permit.
#[tokio::test]
async fn ingestion_semaphore_serializes_concurrent_ingestions() {
    let port = std::sync::Arc::new(in_memory_port());

    let port1 = port.clone();
    let port2 = port.clone();

    let record1 = TurnRecord {
        thread_id: "sem-1".to_string(),
        user_input: "first".to_string(),
        agent_response: "response1".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    let record2 = TurnRecord {
        thread_id: "sem-2".to_string(),
        user_input: "second".to_string(),
        agent_response: "response2".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };

    // Spawn both ingestions concurrently.
    let (r1, r2) = tokio::join!(
        async move { port1.ingest_turn(record1).await },
        async move { port2.ingest_turn(record2).await }
    );

    assert!(r1.is_ok(), "first ingestion should succeed: {r1:?}");
    assert!(r2.is_ok(), "second ingestion should succeed: {r2:?}");

    // Both turns should be stored in the curator store.
    let curator_store = port.curator_store.get().expect("curator store");
    let h1 = curator_store
        .query_deduped_untouched("curator:thread:sem-1")
        .expect("query succeeds");
    let h2 = curator_store
        .query_deduped_untouched("curator:thread:sem-2")
        .expect("query succeeds");
    assert_eq!(h1.len(), 1, "first turn should be stored");
    assert_eq!(h2.len(), 1, "second turn should be stored");
}

/// `recall_context_curator` should recall from the curator's stores, not
/// the user's. This pins the curator recall path that the
/// `BridgeCuratorContextInjector` delegates to.
#[tokio::test]
async fn recall_context_curator_reads_curator_store() {
    let port = in_memory_port();

    // Ingest a curator turn.
    let record = TurnRecord {
        thread_id: "curator-recall-test".to_string(),
        user_input: "regulation_status_check_keyword".to_string(),
        agent_response: "All regulation systems are operational.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record)
        .await
        .expect("ingestion should succeed");

    // Recall from the curator's store — the keyword should match.
    let snippets = port
        .recall_context_curator("regulation_status_check_keyword", 5, false)
        .await
        .expect("curator recall should succeed");
    assert!(
        !snippets.is_empty(),
        "curator recall should find the ingested curator turn"
    );
}

/// `recall_thread_curator` should recall a thread's prior turns by exact
/// entity match, not by content similarity. This pins the thread-scoped
/// recall fix — the previous `inject_context` passed the `thread_id` UUID
/// as the query to `recall_context`, which never matched stored turn text
/// (the stored embeddings are of `user_input`, not the thread_id), so
/// thread-scoped recall was dead code.
#[tokio::test]
async fn recall_thread_recalls_thread_by_entity() {
    let port = in_memory_port();
    let thread_id = "curator-thread-recall-by-entity";

    // Ingest a curator turn.
    let record = TurnRecord {
        thread_id: thread_id.to_string(),
        user_input: "how do I configure the embedding model".to_string(),
        agent_response: "Set kask.corpus.embedding_dim in settings.json.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record)
        .await
        .expect("ingestion should succeed");

    // Recall by thread_id — should find the turn via exact entity match.
    let snippets = port
        .recall_thread_curator(thread_id, 10, false)
        .await
        .expect("thread recall should succeed");
    assert!(
        !snippets.is_empty(),
        "recall_thread_curator should find the ingested turn by entity, not content"
    );
}

/// `recall_thread_curator` should recall the curator's prior turns on a
/// thread from the curator's sovereign stores. Mirrors the user-side test.
#[tokio::test]
async fn recall_thread_curator_recalls_curator_thread() {
    let port = in_memory_port();
    let thread_id = "curator-thread-recall-test";

    // Ingest a curator turn.
    let record = TurnRecord {
        thread_id: thread_id.to_string(),
        user_input: "regulation status check".to_string(),
        agent_response: "All regulation systems are operational.".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    };
    port.ingest_turn(record)
        .await
        .expect("ingestion should succeed");

    // Recall by thread_id from the curator's stores.
    let snippets = port
        .recall_thread_curator(thread_id, 10, false)
        .await
        .expect("curator thread recall should succeed");
    assert!(
        !snippets.is_empty(),
        "recall_thread_curator should find the ingested curator turn by entity"
    );
}

/// A consolidation pass prunes below-floor h_mems from the curator's
/// store and leaves above-floor content alone — the deterministic effect
/// the timer exists to produce. Fired directly through the pass seam the
/// timer calls; the timer's own scheduling is pinned by
/// `consolidation_timer_fires_and_prunes_low_confidence`.
#[tokio::test]
async fn consolidation_pass_prunes_low_confidence_and_keeps_the_rest() {
    let port = in_memory_port_with_cadence(60, 0.3);
    let store = port.curator_store.get().expect("curator store");
    let webid = port.curator_webid;

    let below_floor = hkask_storage::HMem::new(
        "probe:low",
        "fact",
        serde_json::Value::String("low-confidence probe".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.1));
    store.store(below_floor).expect("seed below-floor h_mem");

    let above_floor = hkask_storage::HMem::new(
        "probe:high",
        "fact",
        serde_json::Value::String("high-confidence probe".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.9));
    store.store(above_floor).expect("seed above-floor h_mem");

    let consolidator = port
        .curator_consolidation
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .expect("consolidator is built for cadence > 0");
    fire_curator_consolidation_pass(&consolidator, webid, 0.3, "consolidation_pass_test");

    assert!(
        store
            .h_mems_by_entity_prefix("probe:low")
            .expect("query")
            .is_empty(),
        "the below-floor h_mem is pruned by the pass"
    );
    assert_eq!(
        store
            .h_mems_by_entity_prefix("probe:high")
            .expect("query")
            .len(),
        1,
        "the above-floor h_mem survives the pass"
    );
}

/// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
/// the consolidation TWIN over the private store: the private pass prunes
/// its own below-floor rows and spares its above-floor rows while the
/// PUBLIC store is untouched (the split holds under the mirror), and the
/// public pass never touches the private store. Both passes are the same
/// deletion-only sweep (`MemoryConsolidator::consolidate`) over their own
/// store — no inference, no cloud surface (the forward local-routing
/// rule's current bound).
#[tokio::test]
async fn private_consolidation_twin_prunes_low_confidence_across_the_split() {
    let mut port = in_memory_port_with_cadence(300, 0.3);
    let webid = WebID::from_persona(b"curator");

    // Seed BOTH sides of the split with below- and above-floor rows.
    let public_store = port.curator_store.get().expect("public store");
    let public_low = hkask_storage::HMem::new(
        "probe:public-low",
        "turn",
        serde_json::Value::String("public below floor".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.1));
    public_store.store(public_low).expect("seed public low");
    let public_high = hkask_storage::HMem::new(
        "probe:public-high",
        "turn",
        serde_json::Value::String("public above floor".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.9));
    public_store.store(public_high).expect("seed public high");

    let private_store = in_memory_curator_store();
    let private_low = hkask_storage::HMem::new(
        "probe:private-low",
        "turn",
        serde_json::Value::String("private below floor".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.1));
    private_store.store(private_low).expect("seed private low");
    let private_high = hkask_storage::HMem::new(
        "probe:private-high",
        "turn",
        serde_json::Value::String("private above floor".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.9));
    private_store
        .store(private_high)
        .expect("seed private high");
    port.private_curator_store =
        Arc::new(CuratorStore::for_tests(Some(Arc::clone(&private_store))));
    // The twin, built the way `RealMemoryPort::new` builds it.
    port.private_curator_consolidation = Arc::new(RwLock::new(build_curator_consolidation(
        300,
        &Some(Arc::clone(&private_store)),
    )));

    // The private twin fires: its own below-floor dies, its above-floor
    // survives, the PUBLIC rows are all untouched.
    let twin = port
        .private_curator_consolidation
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .expect("the private twin is built");
    fire_curator_consolidation_pass(&twin, webid, 0.3, "private_twin_test");
    assert!(
        private_store
            .h_mems_by_entity_prefix("probe:private-low")
            .expect("query private low")
            .is_empty(),
        "the private below-floor row dies on the twin's pass"
    );
    assert_eq!(
        private_store
            .h_mems_by_entity_prefix("probe:private-high")
            .expect("query private high")
            .len(),
        1,
        "the private above-floor row survives"
    );
    assert_eq!(
        public_store
            .h_mems_by_entity_prefix("probe:public-low")
            .expect("query public low")
            .len(),
        1,
        "the private twin never touches the public store"
    );

    // The public pass fires: its below-floor dies — and a freshly-seeded
    // private below-floor row SURVIVES it (the public pass never crosses
    // the split).
    let fresh_private_low = hkask_storage::HMem::new(
        "probe:private-low-2",
        "turn",
        serde_json::Value::String("fresh private below floor".to_string()),
        webid,
    )
    .with_confidence(hkask_types::Confidence::new(0.1));
    private_store
        .store(fresh_private_low)
        .expect("seed fresh private low");
    let public_pass = port
        .curator_consolidation
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .expect("the public consolidator is built for cadence > 0");
    fire_curator_consolidation_pass(&public_pass, webid, 0.3, "public_pass_test");
    assert!(
        public_store
            .h_mems_by_entity_prefix("probe:public-low")
            .expect("query public low")
            .is_empty(),
        "the public below-floor row dies on the public pass"
    );
    assert_eq!(
        private_store
            .h_mems_by_entity_prefix("probe:private-low-2")
            .expect("query fresh private low")
            .len(),
        1,
        "the public pass never crosses the split — the private below-floor row survives it"
    );
}

/// zed-kask: D100 (operator ruling 2026-11-06, the review's item 4) —
/// the private forgetting leg: the age-gated sweep is TURN-STORAGE-ONLY
/// over the private store (the `curator_memory_prune` valve's fail-closed
/// default scope): an aged `curator:thread:` row dies, an aged
/// knowledge-layer row survives, a recently-RECALLED aged turn row is
/// spared (the decay-clock discipline), and `forgetting_days == 0`
/// disables the leg entirely. The store-level scope/spare semantics are
/// pinned in hkask-memory (`prune_by_age_in_prefixes_spares_knowledge_rows`);
/// this pin holds the bridge-side SEAM: the leg's arguments, its own store,
/// and its disable knob.
#[tokio::test]
async fn private_forgetting_leg_prunes_aged_turn_storage_only() {
    let mut port = in_memory_port();
    let webid = WebID::from_persona(b"curator");

    let private_store = in_memory_curator_store();
    let mut aged_turn = hkask_storage::HMem::new(
        "curator:thread:forget-test",
        "turn",
        serde_json::Value::String("aged turn".to_string()),
        webid,
    );
    // Both stamps age: `HMem::new` stamps `recalled_at` to now, and the
    // spare path compares `recalled_at` — a fresh recall-spare would
    // block the prune the pin must observe.
    aged_turn.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    aged_turn.recalled_at = chrono::Utc::now() - chrono::Duration::days(100);
    private_store.store(aged_turn).expect("seed aged turn");

    let mut aged_ruling = hkask_storage::HMem::new(
        "forget-test-ruling",
        "operator_ruling",
        serde_json::Value::String("a durable ruling".to_string()),
        webid,
    );
    aged_ruling.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    aged_ruling.recalled_at = chrono::Utc::now() - chrono::Duration::days(100);
    private_store.store(aged_ruling).expect("seed aged ruling");

    let mut recalled_turn = hkask_storage::HMem::new(
        "curator:thread:recall-spared",
        "turn",
        serde_json::Value::String("old but recently recalled".to_string()),
        webid,
    );
    // `observed_at` aged out, `recalled_at` fresh (HMem::new's stamp) —
    // the decay clock was reset, so the leg spares it.
    recalled_turn.observed_at = chrono::Utc::now() - chrono::Duration::days(100);
    private_store
        .store(recalled_turn)
        .expect("seed recall-spared turn");

    port.private_curator_store = Arc::new(CuratorStore::for_tests(Some(private_store.clone())));

    // 0 disables the leg — nothing moves.
    fire_private_forgetting_pass(&port.private_curator_store, 0);
    assert_eq!(
        private_store
            .h_mems_by_entity_prefix("curator:thread:forget-test")
            .expect("query aged turn")
            .len(),
        1,
        "forgetting_days == 0 disables the leg"
    );

    // A 50-day window: the aged turn dies; the aged knowledge row and
    // the recently-recalled aged turn survive.
    fire_private_forgetting_pass(&port.private_curator_store, 50);
    assert!(
        private_store
            .h_mems_by_entity_prefix("curator:thread:forget-test")
            .expect("query aged turn")
            .is_empty(),
        "the aged turn row is pruned"
    );
    assert_eq!(
        private_store
            .h_mems_by_entity_prefix("forget-test-ruling")
            .expect("query aged ruling")
            .len(),
        1,
        "knowledge-layer rows survive the turn-scoped leg"
    );
    assert_eq!(
        private_store
            .h_mems_by_entity_prefix("curator:thread:recall-spared")
            .expect("query recall-spared")
            .len(),
        1,
        "a recently-recalled row is spared (the decay-clock discipline)"
    );
}

#[tokio::test]
async fn memory_health_json_reports_degraded_without_healing() {
    let port = in_memory_port();

    let healthy = port.memory_health_json();
    assert_eq!(healthy["curator_store"], true);
    // `degraded` follows the curator store alone — the bridge-side
    // swarm store was removed with the swarm capability (2026-10-28);
    // there is no second managed memory DB.
    assert_eq!(healthy["degraded"], false);

    // Simulate a curator outage. Healing is disabled in test handles, so
    // if the probe attempted a heal it would fail — the point is it must
    // not attempt one at all.
    port.curator_store.set_for_tests(None);
    let degraded = port.memory_health_json();
    assert_eq!(degraded["curator_store"], false);
    assert_eq!(degraded["degraded"], true);

    // Store still down after the probe — the probe didn't heal.
    assert!(port.curator_store.get().is_none());
}

/// Self-healing pin: when the curator store is down, `get()` returns
/// `None` without healing (heal disabled in tests), and after
/// `set_for_tests` restores it, subsequent reads see the healed
/// store. This mirrors the production heal path where a failed open is
/// retried on the next access.
#[tokio::test]
async fn curator_store_heals_after_outage() {
    let port = in_memory_port();

    // Simulate an outage — the store goes None.
    port.curator_store.set_for_tests(None);
    assert!(port.curator_store.get().is_none(), "store down");

    // Ingestion during the outage still succeeds (curator writes skip
    // — there's no store to write to).
    port.ingest_turn(TurnRecord {
        thread_id: "outage-test".to_string(),
        user_input: "during outage".to_string(),
        agent_response: "response".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    })
    .await
    .expect("ingestion during outage succeeds");

    // Heal: restore a fresh in-memory store and verify reads see it.
    let curator_driver: Arc<dyn hkask_storage::DatabaseDriver> = SqliteDriver::in_memory_driver();
    let healed = Arc::new(MemoryStore::new(
        HMemStore::from_driver(Arc::clone(&curator_driver)).expect("hmem init"),
        EmbeddingStore::from_driver(curator_driver, 1024).expect("embedding store init"),
    ));
    port.curator_store.set_for_tests(Some(Arc::clone(&healed)));

    assert!(port.curator_store.get().is_some(), "store healed");

    // Post-heal ingestion writes curator records again.
    port.ingest_turn(TurnRecord {
        thread_id: "post-heal-test".to_string(),
        user_input: "after heal".to_string(),
        agent_response: "response".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: Some("Curator".to_string()),
        inference_provenance: hkask_types::InferenceProvenance::PublicProvider,
    })
    .await
    .expect("post-heal ingestion succeeds");
    let curator_record = healed
        .query_deduped_untouched("curator:thread:post-heal-test")
        .expect("curator query");
    assert_eq!(curator_record.len(), 1, "curator record written after heal");
}

/// T7b wiring pin: `BridgeMemoryPort::ingest_turn` must call the reask
/// correlator (`hkask_tool_invoker::correlate_reask`) without panicking.
/// The correlator drains the process-global render buffer that
/// provenance-carrying widgets populate via `record_render`. We cannot
/// easily assert the `reg.widget.reask` tracing span fired without a
/// capture subscriber (the repo has none); the leaf's unit tests cover the
/// correlator state machine. This test pins the call path is wired: record
/// a render, then ingest a user-message turn, and assert no error.
#[tokio::test]
async fn bridge_ingest_turn_calls_reask_correlator() {
    use agent::ThreadMemoryPort as _;

    // Record a widget render so the correlator has state to drain.
    hkask_tool_invoker::record_render(Some("portfolio_returns".into()), None);

    // Construct a BridgeMemoryPort over the in-memory RealMemoryPort.
    let inner: std::sync::Arc<dyn MemoryPort> = std::sync::Arc::new(in_memory_port());
    let bridge = BridgeMemoryPort::new(inner);

    // Ingest a user-message turn (non-empty user_input). The correlator
    // fires inside ingest_turn before the inner call.
    let record = agent::ThreadTurnRecord {
        thread_id: "reask-wiring-test".to_string(),
        user_input: "now show me the portfolio returns".to_string(),
        agent_response: "here are the returns".to_string(),
        model: "test-model".to_string(),
        thread_title: None,
        agent_id: None,
        private_inference: false,
    };
    let result = bridge.ingest_turn(record).await;
    assert!(
        result.is_ok(),
        "ingest_turn with correlator wired should succeed: {result:?}"
    );
}
