//! Dual-axis ontology mapping for prediction-market records.
//!
//! Q-O1 resolution (2026-08-05): `hkask-bridge-dublincore` is the canonical
//! vocabulary crate — the `dcterms:*` / `bibo:*` / `pko:*` constants defined
//! there are reused here, not re-declared. Q-O2 resolution: no `hkask:`
//! forecasting namespace exists in the workspace; calibration vocabulary is
//! domain-supplement tier and lives in this module until a second consumer
//! materializes (ADR-042 port-promotion rule).
//!
//! Both the per-record `ontology` blocks and the `market_ontology_map` tool
//! output (T4b) are generated from the constants in this module so they
//! cannot drift.

/// Bumped when the mapping shape changes; consumers compare against the
/// per-record `ontology.mapping_version` to detect evolution.
pub const MAPPING_VERSION: u32 = 1;

/// Polymarket market lifecycle stages (arXiv:2604.20421 §1), mapped onto the
/// PKO process axis. `dispute` is load-bearing: 2604.20421's oracle-risk
/// finding shows markets trade within 24h of a dispute anchor, so consumers
/// must be able to distrust prices at this stage without knowing UMA internals.
/// Used from T4's contract onward; defined now so the mapping document is
/// complete for the T4b tool from day one.
#[allow(dead_code)]
pub const LIFECYCLE_STAGES: [&str; 6] = [
    "creation",
    "trading",
    "oracle_request",
    "proposal",
    "dispute",
    "settlement",
];
