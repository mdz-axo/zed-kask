use hkask_memory::test_support::{reseal_fixture_identity, sha256_file};
use hkask_memory::{
    FederatedHit, FederatedSourceKind, FederatedSourcesManifest, RankedSourceBatch,
    ReadOnlyPassageSource, interleave_ranked_batches,
};

const PASSPHRASE: &str = "test-passphrase";
const SOURCE_ID: &str = "fixture-reference";
const ENTITY_PREFIX: &str = "calibration:fixture:sealed-v1:reference:";
const REQUESTED_MODEL: &str = "provider/fixture-model";
const ACTUAL_MODEL: &str = "fixture-model";

fn fixture(directory: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    hkask_memory::test_support::sealed_federated_fixture(
        directory,
        SOURCE_ID,
        "Fixture research library",
        "fixture.txt",
        "grounded fixture passage",
        REQUESTED_MODEL,
        ACTUAL_MODEL,
    )
}

fn fixture_vector() -> Vec<f32> {
    let mut vector = vec![0.0; hkask_storage::embedding_dim()];
    vector[0] = 1.0;
    vector
}

/// expect: "A configured external source is identity-bound and returns only passage text." [P8]
#[test]
fn bound_source_returns_provenance_without_method_signals() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let before = std::fs::read(directory.path().join("reference.db"))?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let source = ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE)?;
    assert_eq!(source.identity().source_id, SOURCE_ID);
    let run_identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("run-identity.json"))?)?;
    assert_eq!(
        source.identity().run_id,
        run_identity["run_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing run ID"))?
    );
    assert_eq!(source.identity().entity_ref_prefix, ENTITY_PREFIX);
    assert_eq!(source.identity().actual_embedding_model, ACTUAL_MODEL);
    assert_eq!(source.identity().dimensions, hkask_storage::embedding_dim());
    assert_eq!(source.identity().passage_count, 1);

    let batch = source.search(REQUESTED_MODEL, &fixture_vector(), 3)?;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].text, "grounded fixture passage");
    assert_eq!(batch.hits[0].source_id, SOURCE_ID);
    assert_eq!(batch.hits[0].run_id, source.identity().run_id);
    assert_eq!(batch.missing_text, 0);
    assert!(!batch.hits[0].text.contains("parataxis_ratio"));
    drop(source);

    assert_eq!(
        std::fs::read(directory.path().join("reference.db"))?,
        before
    );
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        assert!(
            !directory
                .path()
                .join(format!("reference.db{suffix}"))
                .exists()
        );
    }
    Ok(())
}

/// expect: "A trailing-colon manifest prefix composes the producer's double-colon index prefix and still admits." [P8]
/// The real calibration builder seals refs from the manifest prefix verbatim
/// (`corpus:researcher:` + `:fine:` → `corpus:researcher::fine:`); the consumer
/// must mirror that composition, never normalize it.
#[test]
fn bound_source_admits_trailing_colon_manifest_prefix() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = hkask_memory::test_support::sealed_federated_fixture_indexed(
        directory.path(),
        SOURCE_ID,
        "Fixture research library",
        "fixture.txt",
        "grounded fixture passage",
        REQUESTED_MODEL,
        ACTUAL_MODEL,
        "corpus:researcher:",
        "fine",
    )?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let source = ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE)?;
    assert_eq!(
        source.identity().entity_ref_prefix,
        "corpus:researcher::fine:"
    );
    assert_eq!(source.identity().passage_count, 1);

    let batch = source.search(REQUESTED_MODEL, &fixture_vector(), 3)?;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].text, "grounded fixture passage");
    assert_eq!(
        batch.hits[0].entity_ref,
        "corpus:researcher::fine:utf8-666978747572652e747874:0"
    );
    Ok(())
}

/// expect: "A self-hashed partial identity cannot impersonate the current producer seal." [P8]
#[test]
fn bound_source_rejects_partial_schema_three_identity() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let run_identity_path = directory.path().join("run-identity.json");
    let mut identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&run_identity_path)?)?;
    identity
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("run identity must be an object"))?
        .remove("preseal_run_id");
    reseal_fixture_identity(&mut identity)?;
    std::fs::write(&run_identity_path, serde_json::to_vec_pretty(&identity)?)?;
    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("partial producer identity was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("run identity"));
    Ok(())
}

/// expect: a tampered filter attestation cannot be substituted for the sealed manifest.
#[test]
fn bound_source_rejects_tampered_filter_attestation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let representations_path = directory.path().join("representations-manifest.json");
    let mut representations: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&representations_path)?)?;
    representations["boilerplate_exclusion_reports"]["fixture.txt"]["retained_words"] =
        serde_json::json!(2);
    std::fs::write(&representations_path, serde_json::to_vec(&representations)?)?;
    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("tampered manifest unexpectedly opened"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("digest mismatch"));
    Ok(())
}

/// expect: "A pre-filter representation manifest cannot enter federated retrieval." [P8]
#[test]
fn bound_source_rejects_pre_filter_representation_manifest() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let representations_path = directory.path().join("representations-manifest.json");
    std::fs::write(
        &representations_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "entity_ref_prefix": "calibration:fixture:sealed-v1"
        }))?,
    )?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("pre-filter representation manifest unexpectedly opened"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("digest mismatch"));
    Ok(())
}

/// expect: "A legacy source schema is rejected even when its sealed digest is valid." [P8]
#[test]
fn bound_source_rejects_non_current_schema_without_a_shim() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let database_path = directory.path().join("reference.db");
    let database_str = database_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 database path"))?;
    {
        let database = hkask_storage::open_or_repair(database_str, PASSPHRASE)?;
        let pool = database.sqlite_pool()?;
        let connection = pool.get()?;
        connection.execute_batch(
            "ALTER TABLE hmems ADD COLUMN abandoned_legacy_state TEXT;
             PRAGMA wal_checkpoint(TRUNCATE);",
        )?;
    }
    let digest = sha256_file(&database_path)?;
    let run_identity_path = directory.path().join("run-identity.json");
    let mut run_identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&run_identity_path)?)?;
    run_identity["indexes"]["reference"] = serde_json::json!(digest);
    reseal_fixture_identity(&mut run_identity)?;
    std::fs::write(
        &run_identity_path,
        serde_json::to_vec_pretty(&run_identity)?,
    )?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("non-current schema unexpectedly opened"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("non-current database schema"));
    Ok(())
}

/// expect: "An altered run ID cannot label unchanged sealed evidence as another run." [P8]
#[test]
fn bound_source_rejects_run_id_that_does_not_match_identity() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let run_identity_path = directory.path().join("run-identity.json");
    let mut identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&run_identity_path)?)?;
    identity["run_id"] = serde_json::json!("0".repeat(64));
    std::fs::write(&run_identity_path, serde_json::to_vec_pretty(&identity)?)?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("altered run ID was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("run identity"));
    Ok(())
}

/// expect: "A sealed source with WAL-resident writes is rejected before immutable reads." [P8]
#[test]
fn bound_source_rejects_nonempty_wal() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let path = directory.path().join("reference.db");
    let database_path = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 database path"))?;
    let database = hkask_storage::open_or_repair(database_path, PASSPHRASE)?;
    let pool = database.sqlite_pool()?;
    let connection = pool.get()?;
    connection.execute_batch("PRAGMA wal_autocheckpoint = 0;")?;
    connection.execute(
        "UPDATE hmems SET value = '\"uncheckpointed\"' WHERE attribute = 'text'",
        [],
    )?;
    let wal = std::path::Path::new(&format!("{database_path}-wal")).to_path_buf();
    assert!(
        std::fs::metadata(&wal)?.len() > 0,
        "fixture must contain WAL-resident writes"
    );

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("uncheckpointed source was accepted"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("WAL"));
    Ok(())
}

/// expect: "A source whose sealed index no longer matches its run identity is rejected." [P8]
#[test]
fn bound_source_rejects_index_digest_mismatch() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let run_identity_path = directory.path().join("run-identity.json");
    let mut run_identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&run_identity_path)?)?;
    run_identity["indexes"]["reference"] = serde_json::json!("0".repeat(64));
    reseal_fixture_identity(&mut run_identity)?;
    std::fs::write(
        &run_identity_path,
        serde_json::to_vec_pretty(&run_identity)?,
    )?;

    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = match ReadOnlyPassageSource::open(&manifest.sources[0], PASSPHRASE) {
        Ok(_) => anyhow::bail!("digest mismatch unexpectedly opened"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("digest"));
    Ok(())
}

fn hit(source_id: &str, kind: FederatedSourceKind, record_id: &str, text: &str) -> FederatedHit {
    FederatedHit {
        source_id: source_id.to_string(),
        source_kind: kind,
        record_id: record_id.to_string(),
        entity_ref: format!("{source_id}:{record_id}"),
        text: text.to_string(),
        run_id: None,
        model: "fixture-model".to_string(),
        confidence: None,
        distance: 0.1,
        source_rank: 1,
        fused_rank: 0,
    }
}

/// expect: "Federation reserves turns for every source and keeps Curator on exact duplicates." [P8]
#[test]
fn ranked_interleave_is_balanced_and_curator_wins_exact_duplicates() {
    let curator = RankedSourceBatch {
        source_id: "curator".to_string(),
        hits: vec![
            hit("curator", FederatedSourceKind::Curator, "c1", "duplicate"),
            hit("curator", FederatedSourceKind::Curator, "c2", "curator two"),
            hit(
                "curator",
                FederatedSourceKind::Curator,
                "c3",
                "curator three",
            ),
        ],
    };
    let corpus = RankedSourceBatch {
        source_id: "corpus".to_string(),
        hits: vec![
            hit("corpus", FederatedSourceKind::Corpus, "p1", "duplicate"),
            hit("corpus", FederatedSourceKind::Corpus, "p2", "corpus two"),
            hit("corpus", FederatedSourceKind::Corpus, "p3", "corpus three"),
        ],
    };

    let fused = interleave_ranked_batches(vec![curator, corpus], 5);
    assert_eq!(
        fused
            .iter()
            .map(|result| result.record_id.as_str())
            .collect::<Vec<_>>(),
        vec!["c1", "p2", "c2", "p3", "c3"]
    );
    assert_eq!(
        fused
            .iter()
            .map(|result| result.fused_rank)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
}
