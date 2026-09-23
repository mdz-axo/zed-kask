use std::io::Read;

use hkask_memory::{
    FederatedHit, FederatedSourceKind, FederatedSourcesManifest, MemoryStore, RankedSourceBatch,
    ReadOnlyPassageSource, interleave_ranked_batches,
};
use hkask_storage::HMem;
use hkask_types::WebID;
use sha2::{Digest, Sha256};

const PASSPHRASE: &str = "test-passphrase";
const SOURCE_ID: &str = "fixture-reference";
const ENTITY_PREFIX: &str = "calibration:fixture:sealed-v1:reference:";
const REQUESTED_MODEL: &str = "provider/fixture-model";
const ACTUAL_MODEL: &str = "fixture-model";

fn sha256_file(path: &std::path::Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn reseal_fixture_identity(identity: &mut serde_json::Value) -> anyhow::Result<()> {
    let index = identity["indexes"]["reference"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing reference digest"))?;
    let representations = identity["representations_manifest_sha256"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing representation digest"))?;
    let canonical = format!(
        "{{\"actual_embedding_model\":\"{ACTUAL_MODEL}\",\"indexes\":{{\"reference\":\"{index}\"}},\"representations_manifest_sha256\":\"{representations}\",\"requested_embedding_model\":\"{REQUESTED_MODEL}\",\"schema_version\":3}}\n"
    );
    identity["run_id"] = serde_json::json!(format!("{:x}", Sha256::digest(canonical.as_bytes())));
    Ok(())
}

fn fixture_vector() -> Vec<f32> {
    let mut vector = vec![0.0; hkask_storage::embedding_dim()];
    vector[0] = 1.0;
    vector
}

fn fixture(directory: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let database_path = directory.join("reference.db");
    let database = database_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 database path"))?;
    {
        let store = MemoryStore::open(database, PASSPHRASE, hkask_storage::embedding_dim())?;
        let entity = format!("{ENTITY_PREFIX}utf8-666978747572652e747874:0");
        store.store(HMem::new(
            &entity,
            "text",
            serde_json::json!("grounded fixture passage"),
            WebID::new(),
        ))?;
        store.store(HMem::new(
            &entity,
            "method_signals",
            serde_json::json!({"parataxis_ratio": 1.0}),
            WebID::new(),
        ))?;
        store.store_embedding(
            &entity,
            &fixture_vector(),
            ACTUAL_MODEL,
            Some("grounded fixture passage"),
        )?;
    }
    {
        let database = hkask_storage::open_or_repair(database, PASSPHRASE)?;
        let pool = database.sqlite_pool()?;
        let connection = pool.get()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        let sidecar = format!("{database}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }

    let digest = sha256_file(&database_path)?;
    let representations_path = directory.join("representations-manifest.json");
    std::fs::write(
        &representations_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "entity_ref_prefix": "calibration:fixture:sealed-v1",
            "boilerplate_exclusion_reports": {
                "fixture.txt": {"input_words": 3, "retained_words": 3, "exclusions": []}
            },
            "validation": {
                "accepted_source_count": 1,
                "boilerplate_filter_applied": true
            }
        }))?,
    )?;
    let manifest_digest = sha256_file(&representations_path)?;
    let run_identity_path = directory.join("run-identity.json");
    let mut identity = serde_json::json!({
        "schema_version": 3,
        "representations_manifest_sha256": manifest_digest,
        "requested_embedding_model": REQUESTED_MODEL,
        "actual_embedding_model": ACTUAL_MODEL,
        "indexes": {"reference": digest}
    });
    reseal_fixture_identity(&mut identity)?;
    std::fs::write(&run_identity_path, serde_json::to_vec_pretty(&identity)?)?;
    let manifest_path = directory.join("federated-sources.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "sources": [{
                "id": SOURCE_ID,
                "display_name": "Fixture research library",
                "database_path": database_path,
                "run_identity_path": run_identity_path,
                "representations_manifest_path": representations_path,
                "index_name": "reference"
            }]
        }))?,
    )?;
    Ok(manifest_path)
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

/// expect: "A self-hashed partial identity cannot impersonate the current producer seal." [P8]
#[test]
fn bound_source_rejects_partial_schema_three_identity() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let manifest_path = fixture(directory.path())?;
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
