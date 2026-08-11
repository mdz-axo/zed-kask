//! Dual-axis ontology mapping for prediction-market records.
//!
//! Q-O1 resolution (2026-08-05): `hkask-bridge-ontology` is the canonical
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
/// Used from T4's contract onward; consumed by `mapping_document()` so the
/// T4b tool output and per-record ontology blocks cannot drift.
pub const LIFECYCLE_STAGES: [&str; 6] = [
    "creation",
    "trading",
    "oracle_request",
    "proposal",
    "dispute",
    "settlement",
];

/// The mapping document returned by the `market_ontology_map` tool (T4b).
/// Built from the same constants that populate per-record `ontology` blocks
/// so the two cannot drift (pinned by test).
pub fn mapping_document() -> serde_json::Value {
    serde_json::json!({
        "mapping_version": MAPPING_VERSION,
        "process_axis": {
            "ontology": "PKO (Procedural Knowledge Ontology)",
            "vocabulary_crate": "hkask-bridge-ontology",
            "record_type": "pko:ProcedureExecution",
            "probability_role": "pko:StepExecution.output",
            "lifecycle_stages": LIFECYCLE_STAGES,
            "stage_note": "distrust prices in the `dispute` stage (arXiv:2604.20421 oracle-risk finding: markets trade within 24h of a dispute anchor)"
        },
        "state_axis": {
            "ontology": "Dublin Core (dcterms)",
            "vocabulary_crate": "hkask-bridge-ontology",
            "fields": {
                "identifier": "dcterms:identifier = {source}:{market_id}",
                "title": "dcterms:title ← market question",
                "description": "dcterms:description ← market description (500-char cap)",
                "temporal": "dcterms:temporal ← deadline (drives horizon effects)",
                "provenance": "dcterms:provenance ← resolution_source (uma_oracle | kalshi_exchange)"
            }
        },
        "domain_supplement": {
            "note": "calibration vocabulary (brier, domain_bias, reliability_tier) is kask domain-supplement tier — no standard ontology covers forecast scoring (Q-O2 resolution 2026-08-05)"
        }
    })
}
