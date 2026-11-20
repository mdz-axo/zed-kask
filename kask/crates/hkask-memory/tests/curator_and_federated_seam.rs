//! Behavioral seam tests for unified Curator sovereign-store self-healing and
//! federated external-source admission (`hkask_memory::curator_store` and
//! `hkask_memory::federated_recall`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use hkask_memory::{
    CuratorOpenConfig, CuratorOpenFailure, CuratorStoreSet, DEFAULT_HEAL_RETRY_INTERVAL,
    FederatedSourceCache, FederatedSourceKind, FederatedSourceSpec, FederatedSourceState,
    FederatedSourcesManifest, MaterializedProvenance, MemoryStore, SelfHealingStore,
    open_curator_store_set, parse_memory_life_days_value,
};
use hkask_types::WebID;

#[test]
fn self_healing_store_enforces_backoff_stays_side_effect_free_on_availability_and_heals()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let db_path = directory.path().join("curator-private.db");
    let db_path_str = db_path.to_string_lossy().into_owned();
    std::fs::write(&db_path, b"not a sqlcipher database")?;

    let open_attempts = Arc::new(AtomicUsize::new(0));
    let config = CuratorOpenConfig {
        embedding_dim: hkask_storage::embedding_dim(),
        memory_life_days: 30.0,
        allow_degraded_embeddings: false,
        journal_open_span: true,
    };

    let attempts_for_closure = Arc::clone(&open_attempts);
    let path_for_closure = db_path_str.clone();
    let reopen = move || {
        attempts_for_closure.fetch_add(1, Ordering::SeqCst);
        open_curator_store_set(Some(&path_for_closure), Some("test-passphrase"), config)
    };

    let initial = reopen();
    assert_eq!(open_attempts.load(Ordering::SeqCst), 1);
    assert!(initial.0.is_down(), "corrupt file must fail initial open");
    assert!(matches!(
        initial.1,
        Some(CuratorOpenFailure::DatabaseOpen(_))
    ));

    let healing = SelfHealingStore::new(
        db_path_str,
        initial.0,
        initial.1,
        true,
        CuratorStoreSet::is_down,
        reopen,
    )
    .with_retry_interval(Duration::from_millis(150));

    assert_eq!(DEFAULT_HEAL_RETRY_INTERVAL, Duration::from_secs(5));

    // Side-effect-free availability probe must NOT trigger reopen.
    assert!(!healing.is_available());
    assert_eq!(
        open_attempts.load(Ordering::SeqCst),
        1,
        "is_available() must never drive reopen"
    );

    // First get() attempts heal (#2) and fails because corruption persists.
    assert!(healing.get().is_down());
    assert_eq!(open_attempts.load(Ordering::SeqCst), 2);
    assert!(healing.has_outage());

    // Remove corruption; immediate get() within retry_interval must NOT storm reopen.
    std::fs::remove_file(&db_path)?;
    assert!(
        healing.get().is_down(),
        "reopen within retry_interval must be throttled"
    );
    assert_eq!(
        open_attempts.load(Ordering::SeqCst),
        2,
        "throttled get() must not invoke reopen"
    );

    // Once retry_interval elapses, get() heals (#3), restores decay setting, and clears outage.
    std::thread::sleep(Duration::from_millis(170));
    let restored = healing.get();
    assert_eq!(open_attempts.load(Ordering::SeqCst), 3);
    assert!(healing.is_available());
    assert!(!healing.has_outage());

    let memory = restored.memory.expect("healed memory store");
    assert_eq!(memory.memory_life_days(), 30.0);

    // Verify Pr1 single-driver ledger sharing and Pr5 role-accurate open span ("curator-private").
    let row = hkask_storage::HMem::new(
        "c1-seam:driver-share",
        "finding",
        serde_json::json!("shared ledger"),
        WebID::from_persona(b"curator"),
    );
    memory.store(row)?;

    let archive = restored.regulation_store.expect("healed regulation store");
    let since = chrono::Utc::now() - chrono::Duration::hours(1);
    let encoded = archive.query_records(since, Some("reg.memory.encode"), 10)?;
    assert!(
        !encoded.is_empty(),
        "MemoryStore must journal reg.memory.encode through shared RegulationArchive"
    );
    let opens = archive.query_records(since, Some("reg.storage.open"), 10)?;
    assert!(
        opens
            .iter()
            .any(|record| record.span.path == "reg.storage.open.curator-private"),
        "open span must carry actual file stem (curator-private), got: {:?}",
        opens.iter().map(|r| &r.span.path).collect::<Vec<_>>()
    );

    Ok(())
}

#[test]
fn parse_memory_life_days_validates_positive_finite_numbers() {
    assert_eq!(parse_memory_life_days_value("30"), 30.0);
    assert_eq!(parse_memory_life_days_value(" 45.5 "), 45.5);
    let default = MemoryStore::default_memory_life_days();
    assert_eq!(parse_memory_life_days_value(""), default);
    assert_eq!(parse_memory_life_days_value("soon"), default);
    assert_eq!(parse_memory_life_days_value("-5"), default);
    assert_eq!(parse_memory_life_days_value("0"), default);
    assert_eq!(parse_memory_life_days_value("inf"), default);
}

#[test]
fn federated_cache_unifies_admit_all_and_admit_selected_and_watches_materialized_provenance()
-> anyhow::Result<()> {
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

    // Add an unselected broken source to verify admit_selected ignores unselected entries
    // while admit_all surfaces per-source status without failing the whole call.
    let mut manifest = FederatedSourcesManifest::load(&manifest_path)?;
    let mut broken = manifest.sources[0].clone();
    broken.id = "unselected-broken".to_string();
    broken.run_identity_path = directory.path().join("missing-identity.json");
    manifest.sources.push(broken);
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

    // 1. admit_all: Ready for fixture-reference, Unavailable for unselected-broken.
    // Because one source is Unavailable, unchanged() must return false so transient outages retry.
    let all_cache = FederatedSourceCache::new();
    let admitted_all = all_cache.admit_all(&manifest_path, Some("test-passphrase"));
    assert_eq!(admitted_all.sources().len(), 1);
    let statuses = admitted_all.statuses();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses[0].source_kind, FederatedSourceKind::Corpus);
    assert_eq!(statuses[0].state, FederatedSourceState::Ready);
    assert_eq!(statuses[1].state, FederatedSourceState::Unavailable);
    assert!(
        !admitted_all.unchanged(),
        "Unavailable source must force retry on next admit_all"
    );

    // 2. admit_selected: opens only fixture-reference, reuses Arc on repeat, evicts on error.
    let selected_cache = FederatedSourceCache::new();
    let selected_ids = vec!["fixture-reference".to_string()];
    let first =
        selected_cache.admit_selected(&manifest_path, selected_ids.clone(), "test-passphrase")?;
    let second = selected_cache.admit_selected(&manifest_path, selected_ids, "test-passphrase")?;
    assert!(
        Arc::ptr_eq(&first, &second),
        "unchanged selected source must reuse cached Arc<AdmittedSourceSet>"
    );

    // Unregistered ID fails closed and evicts cached handle.
    let err = selected_cache
        .admit_selected(
            &manifest_path,
            vec!["unregistered-id".to_string()],
            "test-passphrase",
        )
        .expect_err("unregistered ID must fail closed");
    assert!(
        matches!(
            err,
            hkask_memory::FederatedAdmissionError::UnregisteredSource { .. }
        ),
        "unexpected variant: {err:?}"
    );
    assert!(err.to_string().contains("not registered"), "{err}");
    assert!(
        selected_cache.cached().is_none(),
        "failed admission must evict stale cached handle"
    );

    // 3. MaterializedProvenance paths must be watched for mid-search / post-admission drift.
    let bundle_manifest = directory.path().join("bundle.json");
    let receipt = directory.path().join("receipt.json");
    std::fs::write(&bundle_manifest, b"{}")?;
    std::fs::write(&receipt, b"{}")?;

    let mut healthy_only = FederatedSourcesManifest::load(&manifest_path)?;
    healthy_only.sources.retain(|s| s.id == "fixture-reference");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&healthy_only)?)?;

    let healthy_set = all_cache.admit_all(&manifest_path, Some("test-passphrase"));
    assert!(healthy_set.unchanged(), "all-Ready set must cache cleanly");

    // Verify watched_paths includes MaterializedProvenance artifacts when present.
    let spec_with_mat = FederatedSourceSpec {
        id: "mat".to_string(),
        display_name: "Materialized".to_string(),
        database_path: directory.path().join("reference.db"),
        run_identity_path: directory.path().join("run-identity.json"),
        representations_manifest_path: directory.path().join("representations.json"),
        index_name: "fine".to_string(),
        materialized_provenance: Some(MaterializedProvenance {
            bundle_manifest_path: bundle_manifest.clone(),
            receipt_path: receipt.clone(),
        }),
    };
    let paths =
        hkask_memory::federated_recall::watched_paths_for_specs(&manifest_path, &[&spec_with_mat]);
    assert!(paths.contains(&bundle_manifest));
    assert!(paths.contains(&receipt));

    Ok(())
}
