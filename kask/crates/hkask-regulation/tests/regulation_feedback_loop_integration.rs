//! Regulation feedback loop integration test — Wave 3 Task 3.4
//!
//! Verifies closed-loop Regulation behavior: event injection → algedonic response →
//! homeostatic restoration. Uses MockRegulationLedger from hkask-test-harness.
//!
//! # Principle grounding
//! - P9 (Homeostatic Self-Regulation): the Regulation must detect perturbations and restore homeostasis
//! - P8 (Semantic Grounding): every test asserts a stated behavioral property

use hkask_test_harness::{MockRegState, MockRegulationLedger, MockToolState, test_event};
use hkask_types::event::{CyclePhase, Span, SpanNamespace};

// The Regulation detects perturbations and restores homeostasis.

#[test]
fn reg_detects_perturbation() {
    let ledger = MockRegulationLedger::new();
    assert!(ledger.is_homeostatic(), "Ledger should start homeostatic");

    let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
    let event = test_event(span, CyclePhase::Sense, None);
    ledger.inject(event);

    assert!(
        !ledger.is_homeostatic(),
        "Ledger should detect perturbation"
    );
    let signals = ledger.recent_signals();
    assert!(
        signals.iter().any(|s| s.is_negative_valence()),
        "Ledger should emit negative valence signal on perturbation"
    );
}

#[test]
fn reg_restores_homeostasis_after_time() {
    let ledger = MockRegulationLedger::new();

    // Perturb the system
    let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
    ledger.inject(test_event(span, CyclePhase::Sense, None));
    assert!(!ledger.is_homeostatic());

    // Advance time to allow feedback processing
    ledger.advance_time(std::time::Duration::from_secs(10));

    // System should return to homeostasis
    assert!(
        ledger.is_homeostatic(),
        "Ledger should restore homeostasis after sufficient time"
    );
    let signals = ledger.recent_signals();
    assert!(
        signals.iter().any(|s| s.is_positive_valence()),
        "Ledger should emit positive valence signal on homeostasis restoration"
    );
}

#[test]
fn reg_throttles_tool_on_budget_exceeded() {
    let ledger = MockRegulationLedger::with_state(MockRegState::perturbed("tool-x"));

    assert!(!ledger.is_homeostatic());
    assert_eq!(ledger.tool_state("tool-x"), MockToolState::Throttled);
    assert_eq!(ledger.tool_state("tool-y"), MockToolState::Active);
}

#[test]
fn reg_tracks_variety_by_domain() {
    let ledger = MockRegulationLedger::new();

    ledger.record_variety("reg.tool");
    ledger.record_variety("reg.tool");
    ledger.record_variety("reg.inference");

    assert_eq!(ledger.variety_for_domain("reg.tool"), 2);
    assert_eq!(ledger.variety_for_domain("reg.inference"), 1);
    assert_eq!(ledger.variety_for_domain("reg.unknown"), 0);
}

#[test]
fn reg_multiple_perturbations_accumulate_signals() {
    let ledger = MockRegulationLedger::new();

    let span = Span::new(SpanNamespace::new("reg.tool").unwrap(), "invoked");
    ledger.inject(test_event(span, CyclePhase::Sense, None));
    ledger.inject(test_event(
        Span::new(SpanNamespace::new("reg.inference").unwrap(), "error"),
        CyclePhase::Compute,
        None,
    ));

    let signals = ledger.recent_signals();
    assert!(
        signals.len() >= 2,
        "multiple perturbations should produce multiple signals"
    );
    let negative_count = signals.iter().filter(|s| s.is_negative_valence()).count();
    assert!(
        negative_count >= 2,
        "each perturbation should produce a negative signal"
    );
}
