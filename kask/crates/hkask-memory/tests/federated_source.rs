use hkask_memory::test_support::{reseal_fixture_identity, sha256_file};
use hkask_memory::{
    BundleAsset, BundleManifest, FederatedHit, FederatedRecallError, FederatedSourceKind,
    FederatedSourcesManifest, MaterializationReceipt, MaterializedProvenance, RankedSourceBatch,
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

    let batch = source.search(REQUESTED_MODEL, "fixture passage", &fixture_vector(), 3)?;
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

    let batch = source.search(REQUESTED_MODEL, "fixture passage", &fixture_vector(), 3)?;
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

// ── Materialized-provenance admission (the shipped-bundle path) ──

/// Build a provenance-carrying spec over the sealed fixture: a bundle
/// manifest (declaring `bundle_key` — the key the fixture database is
/// actually locked with) and materialization receipt written into
/// `directory`, carrying the given pins, with the receipt naming
/// `receipt_database_path`.
fn materialized_spec(
    directory: &std::path::Path,
    manifest_path: &std::path::Path,
    run_id: &str,
    expected_dimensions: usize,
    expected_passage_count: usize,
    asset_sha256: &str,
    bundle_key: &str,
    receipt_database_path: &std::path::Path,
) -> anyhow::Result<hkask_memory::FederatedSourceSpec> {
    let manifest = FederatedSourcesManifest::load(manifest_path)?;
    let mut spec = manifest.sources[0].clone();
    let bundle = BundleManifest {
        schema_version: 1,
        source_id: SOURCE_ID.to_string(),
        display_name: "Fixture research library".to_string(),
        run_id: run_id.to_string(),
        expected_dimensions,
        expected_passage_count,
        bundle_key: bundle_key.to_string(),
        assets: vec![BundleAsset {
            name: "zk-ref-open-part-000".to_string(),
            sha256: asset_sha256.to_string(),
        }],
    };
    let bundle_path = directory.join("bundle.json");
    std::fs::write(&bundle_path, serde_json::to_vec_pretty(&bundle)?)?;
    let receipt = MaterializationReceipt {
        schema_version: 1,
        asset_sha256: vec![asset_sha256.to_string()],
        database_path: receipt_database_path.display().to_string(),
    };
    let receipt_path = directory.join("materialization-receipt.json");
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    spec.materialized_provenance = Some(MaterializedProvenance {
        bundle_manifest_path: bundle_path,
        receipt_path,
    });
    Ok(spec)
}

fn fixture_run_id(directory: &std::path::Path) -> anyhow::Result<String> {
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("run-identity.json"))?)?;
    Ok(identity["run_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing run ID"))?
        .to_string())
}

/// expect: "A materialized source admits by provenance chain — bundle pins,
/// verified receipt, live content — without the passphrase-bound byte
/// digest." [P8]
/// The shipped-bundle case: the database is re-keyed under the target
/// machine's passphrase, so the sealed digest can never match again; the
/// seal's index digest here is deliberately corrupted (and re-sealed) to
/// prove the materialized path never consults it.
#[test]
fn materialized_provenance_admits_without_the_sealed_byte_digest() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let identity_path = directory.path().join("run-identity.json");
    let mut identity: serde_json::Value = serde_json::from_slice(&std::fs::read(&identity_path)?)?;
    identity["indexes"]["reference"] = serde_json::json!("f".repeat(64));
    reseal_fixture_identity(&mut identity)?;
    std::fs::write(&identity_path, serde_json::to_vec_pretty(&identity)?)?;

    let spec = materialized_spec(
        directory.path(),
        &manifest_path,
        &fixture_run_id(directory.path())?,
        hkask_storage::embedding_dim(),
        1,
        &"a".repeat(64),
        PASSPHRASE,
        &directory.path().join("reference.db"),
    )?;
    let source = ReadOnlyPassageSource::open(&spec, PASSPHRASE)?;
    assert_eq!(source.identity().source_id, SOURCE_ID);
    assert_eq!(source.identity().passage_count, 1);
    let batch = source.search(REQUESTED_MODEL, "fixture passage", &fixture_vector(), 3)?;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].text, "grounded fixture passage");
    Ok(())
}

/// expect: "A receipt whose asset hashes do not match the bundle manifest
/// fails closed." [P8]
#[test]
fn materialized_receipt_hash_mismatch_is_rejected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let spec = materialized_spec(
        directory.path(),
        &manifest_path,
        &fixture_run_id(directory.path())?,
        hkask_storage::embedding_dim(),
        1,
        &"a".repeat(64),
        PASSPHRASE,
        &directory.path().join("reference.db"),
    )?;
    let receipt_path = directory.path().join("materialization-receipt.json");
    let mut receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(&receipt_path)?)?;
    receipt["asset_sha256"][0] = serde_json::json!("b".repeat(64));
    std::fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt)?)?;
    let error = ReadOnlyPassageSource::open(&spec, PASSPHRASE)
        .err()
        .expect("a receipt that does not match the bundle must fail closed");
    assert!(
        matches!(error, FederatedRecallError::DigestMismatch { .. }),
        "{error}"
    );
    Ok(())
}

/// expect: "A materialized database whose live passage count differs from
/// the bundle pin fails closed." [P8]
#[test]
fn materialized_content_pin_mismatch_is_rejected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let spec = materialized_spec(
        directory.path(),
        &manifest_path,
        &fixture_run_id(directory.path())?,
        hkask_storage::embedding_dim(),
        2,
        &"a".repeat(64),
        PASSPHRASE,
        &directory.path().join("reference.db"),
    )?;
    let error = ReadOnlyPassageSource::open(&spec, PASSPHRASE)
        .err()
        .expect("a passage count that differs from the bundle pin must fail closed");
    assert!(
        matches!(error, FederatedRecallError::SchemaMismatch { .. }),
        "{error}"
    );
    Ok(())
}

/// expect: "A bundle manifest naming a different sealed run than the run
/// identity fails closed." [P8]
#[test]
fn materialized_run_id_mismatch_is_rejected() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let spec = materialized_spec(
        directory.path(),
        &manifest_path,
        &"0".repeat(64),
        hkask_storage::embedding_dim(),
        1,
        &"a".repeat(64),
        PASSPHRASE,
        &directory.path().join("reference.db"),
    )?;
    let error = ReadOnlyPassageSource::open(&spec, PASSPHRASE)
        .err()
        .expect("a bundle pinning a different run must fail closed");
    assert!(
        matches!(error, FederatedRecallError::RunIdentityMismatch { .. }),
        "{error}"
    );
    Ok(())
}

/// expect: "A re-keyed database — the exact production materialization —
/// fails the sealed byte check but admits through the provenance chain
/// under the BUNDLE's declared key, not the caller's passphrase." [P8]
#[test]
fn rekeyed_database_admits_via_provenance_not_the_sealed_digest() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
    let database = directory.path().join("reference.db");
    {
        let opened = hkask_storage::open_or_repair(&database.display().to_string(), PASSPHRASE)?;
        let pool = opened.sqlite_pool()?;
        pool.get()?
            .execute_batch("PRAGMA rekey='materialized-passphrase';")?;
    }
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        let sidecar = directory.path().join(format!("reference.db{suffix}"));
        if sidecar.exists() {
            std::fs::remove_file(sidecar)?;
        }
    }
    let manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let error = ReadOnlyPassageSource::open(&manifest.sources[0], "materialized-passphrase")
        .err()
        .expect("a re-keyed database cannot reproduce the sealed bytes");
    assert!(
        matches!(error, FederatedRecallError::DigestMismatch { .. }),
        "{error}"
    );

    let spec = materialized_spec(
        directory.path(),
        &manifest_path,
        &fixture_run_id(directory.path())?,
        hkask_storage::embedding_dim(),
        1,
        &"a".repeat(64),
        // The bundle declares the key the database is actually locked with.
        "materialized-passphrase",
        &database,
    )?;
    // The caller passes the OLD passphrase — a key this database no longer
    // accepts. Admission must open via the bundle's declared key anyway:
    // the machine passphrase is never used for a materialized source.
    let source = ReadOnlyPassageSource::open(&spec, PASSPHRASE)?;
    assert_eq!(source.identity().passage_count, 1);
    let batch = source.search(REQUESTED_MODEL, "fixture passage", &fixture_vector(), 3)?;
    assert_eq!(batch.hits.len(), 1);
    assert_eq!(batch.hits[0].text, "grounded fixture passage");
    Ok(())
}
