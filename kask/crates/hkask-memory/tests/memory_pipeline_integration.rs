//! Integration test: store → recall → consolidate pipeline over one store.
//!
//! Verifies the full memory lifecycle:
//! 1. Store a perspective-bound (episodic) h_mem and recall it
//! 2. Store a perspective-free (semantic) h_mem and recall it deduped
//! 3. Consolidate: perspective-bound → shared (one-way bridge)
//! 4. Bayesian combination on repeated consolidation of the same EAV
//! 5. Decay (Wozniak-Gorzelanczyk) applied at recall
//!
//! One `MemoryStore` holds both kinds. The episodic/semantic distinction is
//! carried by the `HMemOntology` blob on each h_mem (P5.4 dual-axis
//! anchoring), not by separate store structs — so `store()` enforces no
//! visibility or perspective invariant. The invariant tests that the legacy
//! `EpisodicMemory`/`SemanticMemory` structs carried are replaced here by
//! tests pinning that both kinds coexist and stay distinguishable.

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{EmbeddingStore, HMem, HMemStore};
use hkask_types::ConsolidationRequest;
use hkask_types::{Confidence, HMemOntology, WebID};
use std::sync::Arc;

fn make_driver() -> Arc<dyn hkask_storage::database::driver::DatabaseDriver> {
    Arc::new(SqliteDriver::new(
        SqliteDriver::in_memory_pool().expect("in-memory pool"),
    ))
}

fn setup_store() -> Arc<MemoryStore> {
    Arc::new(setup_store_owned())
}

fn setup_store_owned() -> MemoryStore {
    let driver = make_driver();
    let h_mem_store = HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store init");
    let embedding_store = EmbeddingStore::from_driver(driver, 1024).expect("embedding store init");
    MemoryStore::new(h_mem_store, embedding_store)
}

fn test_perspective() -> WebID {
    WebID::from_persona(b"test-agent")
}

// ── Episodic recall (perspective-bound, PKO-anchored) ──────────────────────

#[test]
fn episodic_store_and_recall() {
    let store = setup_store();
    let perspective = test_perspective();

    let h_mem = HMem::new(
        "test_entity",
        "test_attr",
        serde_json::json!("test_value"),
        perspective,
    )
    .with_perspective(perspective)
    .with_ontology(HMemOntology::episodic(
        "test-procedure",
        "step-1",
        "session",
    ));

    store.store(h_mem).expect("store episodic");

    let recalled = store
        .query_for_deduped("test_entity", perspective)
        .expect("recall episodic");

    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entity, "test_entity");
    assert_eq!(recalled[0].attribute, "test_attr");
    assert_eq!(recalled[0].value, serde_json::json!("test_value"));
    assert_eq!(recalled[0].access.perspective, Some(perspective));
}

/// A perspective-scoped recall must not surface another agent's h_mems on the
/// same entity. This is the query-axis role `perspective` retains after the
/// store split was removed (P11.1: the DB file is the access boundary;
/// `perspective` is "who wrote this", not an access-control field).
#[test]
fn perspective_scoped_recall_excludes_other_agents() {
    let store = setup_store();
    let mine = test_perspective();
    let theirs = WebID::from_persona(b"other-agent");

    for (perspective, value) in [(mine, "mine"), (theirs, "theirs")] {
        let h_mem = HMem::new(
            "shared_entity",
            "attr",
            serde_json::json!(value),
            perspective,
        )
        .with_perspective(perspective);
        store.store(h_mem).expect("store");
    }

    let recalled = store
        .query_for_deduped("shared_entity", mine)
        .expect("recall");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].value, serde_json::json!("mine"));
}

// ── Semantic recall (perspective-free, DC-anchored) ────────────────────────

#[test]
fn semantic_store_and_recall_deduped() {
    let store = setup_store();
    let owner = test_perspective();

    let h_mem = HMem::new("fact_x", "is", serde_json::json!("true"), owner)
        .with_visibility(hkask_types::Visibility::Shared)
        .with_ontology(HMemOntology::semantic(
            "bibo:Document",
            vec!["truth".to_string()],
            "test",
        ));

    store.store(h_mem).expect("store semantic");

    let recalled = store.query_deduped("fact_x").expect("recall semantic");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].entity, "fact_x");
    assert!(recalled[0].is_semantic());
}

/// The core of the unification: a semantic fact and an episodic step
/// execution live side by side in one store, and the ontology blob — not the
/// store struct — tells them apart. Queryable on both axes.
#[test]
fn episodic_and_semantic_coexist_and_stay_distinguishable() {
    let store = setup_store();
    let perspective = test_perspective();

    let fact = HMem::new(
        "company:Apple",
        "roic",
        serde_json::json!(0.32),
        perspective,
    )
    .with_visibility(hkask_types::Visibility::Shared)
    .with_ontology(HMemOntology::semantic(
        "bibo:Article",
        vec!["ROIC".to_string()],
        "10-K 2025",
    ));
    store.store(fact).expect("store fact");

    let step = HMem::new(
        "chat:thread:abc",
        "chatted",
        serde_json::json!("reproduced the bug"),
        perspective,
    )
    .with_perspective(perspective)
    .with_ontology(HMemOntology::episodic(
        "diagnose-bug-123",
        "reproduce",
        "session-1",
    ));
    store.store(step).expect("store step");

    // State-axis query reaches only the fact.
    let articles = store.query_by_dc_type("bibo:Article").expect("dc_type");
    assert_eq!(articles.len(), 1);
    assert_eq!(articles[0].attribute, "roic");

    // Process-axis query reaches only the step.
    let steps = store
        .query_by_pko_procedure("diagnose-bug-123")
        .expect("pko_procedure");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].entity, "chat:thread:abc");
}

// ── Consolidation ──────────────────────────────────────────────────────────

#[test]
fn consolidator_counts_candidates() {
    let store = setup_store();
    let consolidator = MemoryConsolidator::new(Arc::clone(&store));
    let perspective = test_perspective();

    assert_eq!(consolidator.consolidation_candidate_count(&perspective), 0);

    let h_mem =
        HMem::new("e", "a", serde_json::json!("v"), perspective).with_perspective(perspective);
    store.store(h_mem).expect("store");

    assert_eq!(
        consolidator.consolidation_candidate_count(&perspective),
        1,
        "should count stored perspective-bound h_mems"
    );
}

// ── Memory life and decay ──────────────────────────────────────────────────

#[test]
fn memory_life_default_is_180_days() {
    let store = setup_store_owned();
    assert!((store.memory_life_days() - 180.0).abs() < 0.01);
}

#[test]
fn memory_life_configurable() {
    let store = setup_store_owned().with_memory_life_days(365.0);
    assert!((store.memory_life_days() - 365.0).abs() < 0.01);
}

#[test]
fn memory_decay_formula() {
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
fn storage_budget_default() {
    let store = setup_store_owned();
    assert_eq!(store.storage_budget(), 10_000);
}

#[test]
fn decay_applied_on_recall() {
    let store = setup_store_owned();
    let owner = test_perspective();

    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), owner)
        .with_visibility(hkask_types::Visibility::Shared)
        .with_confidence(Confidence::new(1.0));
    store.store(h_mem).expect("store");

    // Immediately recall — decay at t≈0 should leave confidence near 1.0
    let recalled = store.query_deduped("fact").expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        recalled[0].confidence.value() > 0.99,
        "fresh recall should have near-original confidence, got {}",
        recalled[0].confidence
    );

    // Recall again — touch_recall reset the clock, so this stays fresh too.
    let recalled2 = store.query_deduped("fact").expect("recall2");
    assert_eq!(recalled2.len(), 1);
    assert!(
        recalled2[0].confidence.value() > 0.99,
        "re-recalled h_mem should stay fresh, got {}",
        recalled2[0].confidence
    );
}

#[test]
fn recall_touches_recalled_at() {
    let store = setup_store_owned();
    let owner = test_perspective();

    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), owner)
        .with_visibility(hkask_types::Visibility::Shared);
    store.store(h_mem).expect("store");

    let r1 = store.query_deduped("fact").expect("recall1");
    let recalled_at_1 = r1[0].recalled_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    let r2 = store.query_deduped("fact").expect("recall2");
    let recalled_at_2 = r2[0].recalled_at;

    assert!(
        recalled_at_2 > recalled_at_1,
        "recalled_at should be updated on each recall (touch_recall)"
    );
}

// ── Consolidation decay symmetry ──────────────────────────────────────────

#[test]
fn consolidation_combines_both_sides_decayed() {
    let store = setup_store();
    let consolidator = MemoryConsolidator::new(Arc::clone(&store));
    let perspective = test_perspective();

    // Seed a shared h_mem with confidence 0.8
    let shared = HMem::new(
        "tool_x",
        "returns",
        serde_json::json!("type_y"),
        perspective,
    )
    .with_visibility(hkask_types::Visibility::Shared)
    .with_confidence(Confidence::new(0.8));
    store.store(shared).expect("store shared");

    // Store a perspective-bound h_mem with same EAV and confidence 0.8
    let bound = HMem::new(
        "tool_x",
        "returns",
        serde_json::json!("type_y"),
        perspective,
    )
    .with_perspective(perspective)
    .with_confidence(Confidence::new(0.8));
    store.store(bound).expect("store perspective-bound");

    // Consolidate — should Bayesian-combine both sides after decay
    let outcome = consolidator
        .consolidate(
            &perspective,
            ConsolidationRequest {
                limit: 10,
                ..Default::default()
            },
        )
        .expect("consolidate");

    assert_eq!(
        outcome.consolidated_count, 1,
        "one h_mem should be consolidated"
    );
    assert_eq!(outcome.failed_count, 0, "no failures expected");

    // Recalling the shared h_mem should show the combined (strengthened) confidence.
    // Both inputs are 0.8, both near-fresh (decay ≈ 0), Bayesian consensus ≈ 0.941.
    let recalled = store.query_deduped("tool_x").expect("recall shared");
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
fn zero_memory_life_preserves_fresh_semantic_h_mems() {
    let store = setup_store_owned().with_memory_life_days(0.0);
    let owner = test_perspective();

    let h_mem = HMem::new("fact", "is", serde_json::json!("true"), owner)
        .with_visibility(hkask_types::Visibility::Shared)
        .with_confidence(Confidence::new(0.8));
    store.store(h_mem).expect("store");

    // Just-stored: t≈0, so confidence is preserved even with S=0
    let recalled = store.query_deduped("fact").expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        (recalled[0].confidence.value() - 0.8).abs() < 0.01,
        "S=0 with fresh h_mem (t≈0) should preserve confidence, got {}",
        recalled[0].confidence
    );
}

#[test]
fn zero_memory_life_preserves_fresh_episodic_h_mems() {
    let store = setup_store_owned().with_memory_life_days(0.0);
    let perspective = test_perspective();

    let h_mem = HMem::new("event", "happened", serde_json::json!("yes"), perspective)
        .with_perspective(perspective)
        .with_confidence(Confidence::new(0.8));
    store.store(h_mem).expect("store");

    let recalled = store
        .query_for_deduped("event", perspective)
        .expect("recall");
    assert_eq!(recalled.len(), 1);
    assert!(
        (recalled[0].confidence.value() - 0.8).abs() < 0.01,
        "S=0 with fresh episodic h_mem (t≈0) should preserve confidence, got {}",
        recalled[0].confidence
    );
}

/// `try_new_without_embeddings` is the no-embedding constructor. It must still
/// support the full h_mem path — a caller that recalls by entity/EAV and never
/// embeds should not need a working embedding store. The curator depends on
/// this: an `EmbeddingStore` failure must degrade vector similarity only, not
/// disable curator recall entirely.
#[test]
fn store_without_embeddings_supports_h_mem_path() {
    let store = MemoryStore::try_new_without_embeddings(
        HMemStore::from_driver(make_driver()).expect("hmem init"),
    )
    .expect("embedding-free store opens");
    let perspective = test_perspective();

    let h_mem = HMem::new("event", "happened", serde_json::json!("yes"), perspective)
        .with_perspective(perspective)
        .with_ontology(HMemOntology::episodic("proc", "step", "src"));
    store.store(h_mem).expect("store");

    let recalled = store
        .query_for_deduped("event", perspective)
        .expect("recall");
    assert_eq!(recalled.len(), 1);
}
