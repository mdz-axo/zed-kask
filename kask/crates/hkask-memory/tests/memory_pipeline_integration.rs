//! Integration test: episodic → recall → consolidate → semantic pipeline.
//!
//! Verifies the full memory lifecycle:
//! 1. Store episodic h_mem (first-person, perspective-bound)
//! 2. Recall episodic (deduped, decayed, temporal attention)
//! 3. Consolidate: episodic → semantic (one-way bridge)
//! 4. Bayesian combination on repeated consolidation of same EAV
//! 5. Semantic recall (deduped, perspective-free)

use hkask_memory::{
    ConsolidationBridge, EpisodicMemory, EpisodicMemoryError, SemanticMemory, SemanticMemoryError,
};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EmbeddingStore, HMem, HMemStore};
use hkask_types::ConsolidationRequest;
use hkask_types::{Confidence, WebID};
use std::sync::Arc;

fn make_driver() -> Arc<dyn hkask_storage::database::driver::DatabaseDriver> {
    Arc::new(SqliteDriver::new(
        SqliteDriver::in_memory_pool().expect("in-memory pool"),
    ))
}

fn setup() -> (Arc<EpisodicMemory>, Arc<SemanticMemory>) {
    let driver = make_driver();
    driver
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS hmems (
                id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                recalled_at TEXT NOT NULL, confidence REAL NOT NULL, perspective TEXT,
                visibility TEXT NOT NULL, owner_webid TEXT NOT NULL, dimension TEXT
            )",
        )
        .expect("init schema");
    let episodic = Arc::new(EpisodicMemory::new(HMemStore::from_driver(Arc::clone(
        &driver,
    ))));
    let semantic = Arc::new(SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(Arc::clone(&driver), 1024),
    ));
    (episodic, semantic)
}

fn test_perspective() -> WebID {
    WebID::from_persona(b"test-agent")
}

// ── Episodic boundary enforcement ──────────────────────────────────────────

#[test]
fn episodic_store_and_recall() {
    let (episodic, _semantic) = setup();
    let perspective = test_perspective();

    let h_mem = HMem::new(
        "test_entity",
        "test_attr",
        serde_json::json!("test_value"),
        perspective,
    )
    .with_perspective(perspective);

    episodic.store(h_mem).expect("store episodic");

    let recalled = episodic
        .query_for_deduped("test_entity", perspective)
        .expect("recall episodic");

    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entity, "test_entity");
    assert_eq!(recalled[0].attribute, "test_attr");
    assert_eq!(recalled[0].value, serde_json::json!("test_value"));
    assert_eq!(recalled[0].access.perspective, Some(perspective));
}

#[test]
fn episodic_rejects_public_visibility() {
    let (episodic, _semantic) = setup();
    let perspective = test_perspective();

    let h_mem = HMem::new("e", "a", serde_json::json!("v"), perspective)
        .with_visibility(hkask_types::Visibility::Public);

    let err = episodic.store(h_mem).unwrap_err();
    assert!(matches!(err, EpisodicMemoryError::InvalidVisibility(_)));
}

#[test]
fn episodic_requires_perspective() {
    let (episodic, _semantic) = setup();
    let perspective = test_perspective();

    let h_mem = HMem::new("e", "a", serde_json::json!("v"), perspective);

    let err = episodic.store(h_mem).unwrap_err();
    assert!(matches!(err, EpisodicMemoryError::MissingPerspective));
}

// ── Semantic boundary enforcement ──────────────────────────────────────────

#[test]
fn semantic_rejects_private_visibility() {
    let (_episodic, semantic) = setup();
    let perspective = test_perspective();

    let h_mem = HMem::new("e", "a", serde_json::json!("v"), perspective);
    let err = semantic.store(h_mem).unwrap_err();
    assert!(matches!(err, SemanticMemoryError::InvalidVisibility(_)));
}

#[test]
fn semantic_store_and_recall_deduped() {
    let (_episodic, semantic) = setup();
    let perspective = test_perspective();

    let h_mem = HMem::new("fact_x", "is", serde_json::json!("true"), perspective)
        .with_visibility(hkask_types::Visibility::Shared);

    semantic.store(h_mem).expect("store semantic");

    let recalled = semantic.query_deduped("fact_x").expect("recall semantic");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entity, "fact_x");
    assert!(recalled[0].is_semantic());
}

// ── Consolidation bridge ───────────────────────────────────────────────────

#[test]
fn consolidation_bridge_counts_candidates() {
    let (episodic, semantic) = setup();
    let bridge = ConsolidationBridge::new(Arc::clone(&episodic), Arc::clone(&semantic));
    let perspective = test_perspective();

    assert_eq!(bridge.consolidation_candidate_count(&perspective), 0);

    let h_mem =
        HMem::new("e", "a", serde_json::json!("v"), perspective).with_perspective(perspective);
    episodic.store(h_mem).expect("store");

    assert_eq!(
        bridge.consolidation_candidate_count(&perspective),
        1,
        "should count stored episodic h_mems"
    );
}

// ── Memory life and decay ──────────────────────────────────────────────────

#[test]
fn memory_life_default_is_180_days() {
    let episodic = EpisodicMemory::new(HMemStore::from_driver(make_driver()));

    assert!((episodic.memory_life_days() - 180.0).abs() < 0.01);
}

#[test]
fn memory_life_configurable() {
    let episodic =
        EpisodicMemory::new(HMemStore::from_driver(make_driver())).with_memory_life_days(365.0);

    assert!((episodic.memory_life_days() - 365.0).abs() < 0.01);
}

#[test]
fn memory_decay_formula() {
    use hkask_types::Confidence;

    let c = Confidence::new(1.0);
    let s = 180.0;

    // t=0: no decay
    assert!((c.memory_decay(0.0, s).value() - 1.0).abs() < 0.001);

    // t=S: R = exp(-1) ≈ 0.3679
    assert!((c.memory_decay(s, s).value() - 0.368).abs() < 0.01);

    // t = S·ln(2) (halflife): R = 0.5
    let h = s * std::f64::consts::LN_2;
    assert!((c.memory_decay(h, s).value() - 0.5).abs() < 0.01);

    // Confidence scales: 0.8 * exp(-1) = 0.8 / e
    let c2 = Confidence::new(0.8);
    assert!((c2.memory_decay(s, s).value() - 0.8 * (-1.0_f64).exp()).abs() < 0.01);
}

#[test]
fn episodic_storage_budget() {
    let episodic = EpisodicMemory::new(HMemStore::from_driver(make_driver()));
    assert_eq!(episodic.storage_budget(), 10_000);
}

// ── Semantic memory decay (parity with episodic) ───────────────────────────

#[test]
fn semantic_memory_life_default_is_180_days() {
    let driver = make_driver();
    let semantic = SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(driver, 1024),
    );

    assert!((semantic.memory_life_days() - 180.0).abs() < 0.01);
}

#[test]
fn semantic_memory_life_configurable() {
    let driver = make_driver();
    let semantic = SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(driver, 1024),
    )
    .with_memory_life_days(365.0);

    assert!((semantic.memory_life_days() - 365.0).abs() < 0.01);
}

#[test]
fn semantic_decay_applied_on_recall() {
    let driver = make_driver();
    let semantic = SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(driver, 1024),
    );
    let perspective = test_perspective();

    // Store a semantic h_mem with high confidence
    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), perspective)
        .with_visibility(hkask_types::Visibility::Shared)
        .with_confidence(Confidence::new(1.0));
    semantic.store(h_mem).expect("store semantic");

    // Immediately recall — decay at t≈0 should leave confidence near 1.0
    let recalled = semantic.query_deduped("fact").expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        recalled[0].confidence.value() > 0.99,
        "fresh recall should have near-original confidence, got {}",
        recalled[0].confidence
    );

    // Recall again — touch_recall should have been called, so second recall also near 1.0
    let recalled2 = semantic.query_deduped("fact").expect("recall2");
    assert_eq!(recalled2.len(), 1);
    assert!(
        recalled2[0].confidence.value() > 0.99,
        "re-recalled h_mem should stay fresh, got {}",
        recalled2[0].confidence
    );
}

#[test]
fn semantic_recall_touches_recalled_at() {
    let driver = make_driver();
    let semantic = SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(driver, 1024),
    );
    let perspective = test_perspective();

    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), perspective)
        .with_visibility(hkask_types::Visibility::Shared);
    semantic.store(h_mem).expect("store");

    // First recall
    let r1 = semantic.query_deduped("fact").expect("recall1");
    let recalled_at_1 = r1[0].recalled_at;

    // Small sleep to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Second recall — recalled_at should be updated
    let r2 = semantic.query_deduped("fact").expect("recall2");
    let recalled_at_2 = r2[0].recalled_at;

    assert!(
        recalled_at_2 > recalled_at_1,
        "recalled_at should be updated on each recall (touch_recall)"
    );
}

// ── Consolidation bridge decay symmetry ──────────────────────────────────

#[test]
fn consolidation_combines_both_sides_decayed() {
    let (episodic, semantic) = setup();
    let bridge = ConsolidationBridge::new(Arc::clone(&episodic), Arc::clone(&semantic));
    let perspective = test_perspective();

    // Seed a semantic h_mem with confidence 0.8
    let sem_triple = HMem::new(
        "tool_x",
        "returns",
        serde_json::json!("type_y"),
        perspective,
    )
    .with_visibility(hkask_types::Visibility::Shared)
    .with_confidence(Confidence::new(0.8));
    semantic.store(sem_triple.clone()).expect("store semantic");

    // Store an episodic h_mem with same EAV and confidence 0.8
    let epi_triple = HMem::new(
        "tool_x",
        "returns",
        serde_json::json!("type_y"),
        perspective,
    )
    .with_perspective(perspective)
    .with_confidence(Confidence::new(0.8));
    episodic.store(epi_triple.clone()).expect("store episodic");

    // Consolidate — should Bayesian-combine both sides after decay
    let outcome = bridge
        .consolidate(
            perspective,
            ConsolidationRequest {
                limit: 10,
                ..Default::default()
            },
        )
        .expect("consolidate");

    eprintln!(
        "consolidated: {}, combined: {}, failed: {}",
        outcome.consolidated_count, outcome.consolidated_count, outcome.failed_count
    );

    assert_eq!(
        outcome.consolidated_count, 1,
        "one h_mem should be consolidated"
    );
    assert!(outcome.failed_count == 0, "no failures expected");

    // Recalling the semantic h_mem should show the combined (strengthened) confidence.
    // Both inputs are 0.8, both near-fresh (decay ≈ 0), Bayesian consensus ≈ 0.941.
    let recalled = semantic.query_deduped("tool_x").expect("recall semantic");
    assert_eq!(recalled.len(), 1);
    assert!(
        recalled[0].confidence.value() > 0.9,
        "Bayesian combination of two 0.8 confidences should strengthen > 0.9, got {}",
        recalled[0].confidence
    );
}

// ── Memory life edge cases ────────────────────────────────────────────────

#[test]
fn memory_life_zero_preserves_at_t0_decays_at_t1() {
    let c = Confidence::new(0.8);

    // t=0, S=0: no time has passed, preserve original
    let decayed_t0 = c.memory_decay(0.0, 0.0);
    assert!(
        (decayed_t0.value() - 0.8).abs() < 0.01,
        "t=0 with S=0 should preserve original confidence, got {}",
        decayed_t0.value()
    );

    // t=1, S=0: time has passed with zero memory life → complete decay
    let decayed_t1 = c.memory_decay(1.0, 0.0);
    assert!(
        decayed_t1.value() < 0.01,
        "t=1 with S=0 should saturate to near-zero, got {}",
        decayed_t1.value()
    );
}

#[test]
fn memory_life_negative_decays_to_zero() {
    // S<0 with t>0: guard triggers infinite decay → 0.0
    let c = Confidence::new(0.5);
    let decayed = c.memory_decay(10.0, -1.0);
    assert!(
        decayed.value() < 0.01,
        "negative S with elapsed time should decay to near-zero, got {}",
        decayed.value()
    );

    // S<0 with t=0: no time has passed, preserve original
    let decayed_t0 = c.memory_decay(0.0, -1.0);
    assert!(
        (decayed_t0.value() - 0.5).abs() < 0.01,
        "t=0 with negative S should preserve original confidence, got {}",
        decayed_t0.value()
    );
}

#[test]
fn semantic_zero_memory_life_preserves_fresh_triples() {
    let driver = make_driver();
    let semantic = SemanticMemory::new(
        HMemStore::from_driver(Arc::clone(&driver)),
        EmbeddingStore::from_driver(driver, 1024),
    )
    .with_memory_life_days(0.0);
    let perspective = test_perspective();

    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), perspective)
        .with_visibility(hkask_types::Visibility::Shared)
        .with_confidence(Confidence::new(0.8));
    semantic.store(h_mem).expect("store");

    // Just-stored: t≈0, so confidence is preserved even with S=0
    let recalled = semantic.query_deduped("fact").expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        (recalled[0].confidence.value() - 0.8).abs() < 0.01,
        "S=0 with fresh h_mem (t≈0) should preserve confidence, got {}",
        recalled[0].confidence
    );
}

#[test]
fn episodic_zero_memory_life_preserves_fresh_triples() {
    let episodic =
        EpisodicMemory::new(HMemStore::from_driver(make_driver())).with_memory_life_days(0.0);
    let perspective = test_perspective();

    let h_mem = HMem::new("event", "happened", serde_json::json!("yes"), perspective)
        .with_perspective(perspective)
        .with_confidence(Confidence::new(0.8));
    episodic.store(h_mem).expect("store");

    let recalled = episodic
        .query_for_deduped("event", perspective)
        .expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        (recalled[0].confidence.value() - 0.8).abs() < 0.01,
        "S=0 with fresh episodic h_mem (t≈0) should preserve confidence, got {}",
        recalled[0].confidence
    );
}
