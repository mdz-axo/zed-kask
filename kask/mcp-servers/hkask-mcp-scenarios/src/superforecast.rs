//! Superforecasting computation engine (Tetlock GJP methodology).
//!
//! Four-stage pipeline:
//! 1. Fermi decomposition — break forecast into sub-questions
//! 2. Outside view — base rate calibration from reference class
//! 3. Inside view — case-specific adjustments
//! 4. Bayesian updating — revise probabilities as evidence arrives
//!
//! Plus: event tree computation (conditional probability propagation)
//! and Brier scoring for calibration tracking.

use hkask_forecast as forecast;

// ── Re-exports from hkask-forecast (pure pass-throughs eliminated) ───────
pub(crate) use forecast::{
    bayesian_update, brier_interpretation, brier_score, outside_view_adjustment,
};
// R3: shared CMP-index provenance bridge contract — re-exported so the
// `scenario_from_cmp_indices` emitter and the companies `EventTreeProjection`
// deserializer share one type-level source of truth. The pin test
// `scenario_from_cmp_indices_emits_full_cmp_provenance_inside_tree` enforces
// that this emitter populates the full 7-field shape.
pub(crate) use forecast::CmpIndexProvenance;
// ── Forecast math (pure deterministic functions) ───────────────────────────
// Extracted to `superforecast/math.rs` (deep-module split: the pure math — Fermi
// decomposition, event-tree propagation, Brier scoring, sensitivity ranking — is
// independent of the stateful orchestration that remains in this file).
mod math;
pub(crate) use math::brier_score_multi;
pub(crate) use math::{
    auto_update_suggestions, build_event_tree, calibrate_from_fermi, score_forecast,
    sensitivity_ranking, structure_framing_document,
};

// ── Assessment (Chermack + Dragonfly-Eye + Calibration + Triage) ────────────
// Extracted to `superforecast/assess.rs` (deep-module split: the assessment
// concern — project scoring, perspective synthesis, calibration-curve tracking,
// triage — is independent of the forecast math and market composition).
mod assess;
pub(crate) use assess::{
    assess_project, compute_calibration_curve, synthesize_perspectives, triage_question,
};

// ── Persistence ──────────────────────────────────────────────────────────
// Extracted to `superforecast/store.rs` (deep-module split: the persistence
// concern — journal + snapshot compaction — is independent of the forecast
// math and composition concerns that remain in this file).
mod store;
pub(crate) use store::ForecastStore;

// ── Bridge: cross-validation + companies/market conversion ────────────────
// Extracted to `superforecast/bridge.rs` (deep-module split: adapting external
// server outputs into scenario events, and cross-validating estimates).
mod bridge;
pub(crate) use bridge::{convert_market_record, cross_validate};

// ── Composition: market/CMP tree composition + Bayesian propagation ───────
// Extracted to `superforecast/compose.rs` (deep-module split: building event
// trees from market/CMP inputs and propagating prior updates).
mod compose;
pub(crate) use compose::{
    CmpDependencySpec, DependencySpec, compose_cmp_tree, compose_cmp_tree_with_deps,
    compose_market_tree, propagate_prior_update,
};
