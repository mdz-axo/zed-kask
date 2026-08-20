//! Integration tests guarding company-corpus source manifests
//! (`kask/registry/company-sources/*.yaml`) against the `CompanySourceManifest`
//! consumer struct.
//!
//! Slice 1 of the company-corpus design
//! (`kask/docs/explanation/company-corpus-design.md` §B6): the manifest must
//! parse, and the schema must reject a manifest with tier_3 sell-side/media
//! enabled by default (the MAIA self-description doctrine enforced at parse
//! time, not prompt time).
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_mcp_corpus::corpus::{CompanySourceManifest, ManifestValidationError};

fn manifest_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../registry/company-sources")
        .join(name)
}

#[test]
fn msft_manifest_parses_and_validates() {
    let path = manifest_path("msft.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let manifest = CompanySourceManifest::from_yaml(&text)
        .unwrap_or_else(|e| panic!("msft.yaml failed to parse: {e}"));
    manifest
        .validate()
        .unwrap_or_else(|e| panic!("msft.yaml failed validation: {e}"));

    assert_eq!(manifest.company.symbol, "MSFT");
    assert_eq!(manifest.company.cik, "0000789019");
    assert_eq!(manifest.company.fiscal_year_end_month, Some(6));
    assert!(
        !manifest.source_tiers.tier_1_self_description.is_empty(),
        "tier_1 must be non-empty (the company describing itself)"
    );
    assert!(
        manifest
            .source_tiers
            .tier_1_self_description
            .iter()
            .any(|e| e.kind == "earnings_transcript"),
        "tier_1 must include the earnings_transcript source"
    );
    assert!(
        manifest
            .source_tiers
            .tier_2_executive_voice
            .iter()
            .any(|e| e.kind == "youtube" && !e.channels_allowlist.is_empty()),
        "tier_2 youtube source must carry a channel allowlist"
    );
    assert!(
        manifest.source_tiers.tier_3_external.is_empty(),
        "tier_3 must be empty by default (sell-side/media excluded per MAIA doctrine)"
    );
}

#[test]
fn schema_rejects_tier3_enabled_by_default() {
    let yaml = r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description:
    - kind: sec_filings
      via: sec_edgar
      forms: [10-K]
  tier_2_executive_voice: []
  tier_3_external:
    - kind: sell_side_research
      via: analyst_blogs
provenance_rule: test
exclusion_rule: test
"#;
    let manifest = CompanySourceManifest::from_yaml(yaml).expect("yaml should parse");
    let result = manifest.validate();
    assert!(
        matches!(
            result,
            Err(ManifestValidationError::Tier3EnabledByDefault { .. })
        ),
        "tier_3 enabled by default must be rejected, got {result:?}"
    );
}

#[test]
fn schema_rejects_youtube_without_channel_allowlist() {
    let yaml = r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description:
    - kind: sec_filings
      via: sec_edgar
      forms: [10-K]
  tier_2_executive_voice:
    - kind: youtube
      via: serpapi
      queries: ["ceo keynote"]
  tier_3_external: []
provenance_rule: test
exclusion_rule: test
"#;
    let manifest = CompanySourceManifest::from_yaml(yaml).expect("yaml should parse");
    let result = manifest.validate();
    assert!(
        matches!(
            result,
            Err(ManifestValidationError::MissingField {
                field: "channels_allowlist",
                ..
            })
        ),
        "youtube without channels_allowlist must be rejected, got {result:?}"
    );
}

#[test]
fn schema_rejects_unknown_fields() {
    // deny_unknown_fields: a stale/renamed field must fail loudly, matching
    // the CorpusConfig drift contract (embed/types.rs BUG-001 note).
    let yaml = r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
  ticker: TEST
source_tiers:
  tier_1_self_description: []
  tier_2_executive_voice: []
  tier_3_external: []
provenance_rule: test
exclusion_rule: test
"#;
    assert!(
        CompanySourceManifest::from_yaml(yaml).is_err(),
        "unknown field 'ticker' must be rejected by deny_unknown_fields"
    );
}

#[test]
fn schema_rejects_unknown_ontology_in_tagging() {
    // The tagging.ontologies field is validated against OntologyNamespace::from_str.
    // Unknown namespaces (including the old CogAT) must be rejected — this is
    // the enforcement point for the ontology manifest field.
    let yaml = r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description: []
  tier_2_executive_voice: []
  tier_3_external: []
provenance_rule: test
exclusion_rule: test
ingestion:
  entity_ref_prefix: "company:test"
  tagging:
    ontologies: [fibo, cogat, sumo]
"#;
    let manifest = CompanySourceManifest::from_yaml(yaml).expect("yaml should parse");
    let result = manifest.validate();
    assert!(
        matches!(result, Err(ManifestValidationError::UnknownOntology(ref name)) if name == "cogat"),
        "unknown ontology 'cogat' must be rejected, got {result:?}"
    );
}

#[test]
fn schema_accepts_valid_ontologies_in_tagging() {
    let yaml = r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description: []
  tier_2_executive_voice: []
  tier_3_external: []
provenance_rule: test
exclusion_rule: test
ingestion:
  entity_ref_prefix: "company:test"
  tagging:
    ontologies: [fibo, sumo, eso, golem, mlschema, omc]
"#;
    let manifest = CompanySourceManifest::from_yaml(yaml).expect("yaml should parse");
    manifest
        .validate()
        .expect("all valid ontology namespaces should be accepted");
}

// ── Property tests for manifest validation ──────────────────────────────────
//
// Uses the hkask-test-harness for structured oracle variety + trace emission.

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn trace_dir() -> Option<std::path::PathBuf> {
        std::env::var("HKASK_TRACE_DIR")
            .ok()
            .map(std::path::PathBuf::from)
    }

    fn emit_trace(name: &str, result: &str, oracle_type: &str) {
        let Some(dir) = trace_dir() else { return };
        let entry = TraceEntry {
            kind: "proptest".to_string(),
            name: name.to_string(),
            result: result.to_string(),
            duration_ms: 0,
            shrunk_counterexample: String::new(),
            oracle_type: oracle_type.to_string(),
            metadata: serde_json::json!({
                "crate": "hkask-mcp-corpus",
                "target": "company_manifest"
            }),
        };
        let run_id =
            std::env::var("HKASK_TRACE_RUN_ID").unwrap_or_else(|_| "standalone".to_string());
        if let Err(error) = write_trace(&dir, &run_id, &entry) {
            eprintln!("warn: trace emission failed for {name}: {error}");
        }
    }

    /// A valid manifest body with configurable ontology names.
    fn manifest_with_ontologies(ontologies: &[&str]) -> String {
        let ontology_list: String = ontologies
            .iter()
            .map(|ontology| format!("\"{ontology}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description: []
  tier_2_executive_voice: []
  tier_3_external: []
provenance_rule: test
exclusion_rule: test
ingestion:
  entity_ref_prefix: "company:test"
  tagging:
    ontologies: [{ontology_list}]
"#
        )
    }

    /// All valid OntologyNamespace names (the domain supplements).
    const VALID_ONTOLOGIES: &[&str] = &["fibo", "eso", "golem", "mlschema", "sdmx", "omc", "sumo"];

    proptest! {
        /// P4 (panic-freedom): `from_yaml` + `validate()` never panics on
        /// arbitrary string input.
        #[test]
        fn manifest_validate_never_panics(yaml in r"\PC*") {
            if let Ok(manifest) = CompanySourceManifest::from_yaml(&yaml) {
                let _ = manifest.validate();
            }
            emit_trace("manifest_validate_never_panics", "pass", "");
        }

        /// P1 (invariant): every valid ontology name is accepted by `validate()`.
        #[test]
        fn manifest_accepts_all_valid_ontologies(
            ontology in prop::sample::select(VALID_ONTOLOGIES.to_vec())
        ) {
            let yaml = manifest_with_ontologies(&[ontology]);
            let manifest = CompanySourceManifest::from_yaml(&yaml).expect("parse");
            manifest.validate().expect("valid ontology should be accepted");
            emit_trace("manifest_accepts_all_valid_ontologies", "pass", "invariant");
        }

        /// P1 (invariant): any string that is NOT a valid OntologyNamespace is
        /// rejected by `validate()` with `UnknownOntology`.
        /// Uses `oracle_invariant` — the check is a property of the output.
        #[test]
        fn manifest_rejects_unknown_ontology(
            ontology in "[a-z]{1,10}".prop_filter("not a valid ontology", |s| !VALID_ONTOLOGIES.contains(&s.as_str()))
        ) {
            let oracle = oracle_invariant(|_input: &serde_json::Value, output: &serde_json::Value| {
                let result = output.get("result").and_then(|v| v.as_str()).unwrap_or("");
                if result == "UnknownOntology" {
                    Ok(())
                } else {
                    Err(format!("expected UnknownOntology, got {result}"))
                }
            });

            let yaml = manifest_with_ontologies(&[&ontology]);
            let manifest = CompanySourceManifest::from_yaml(&yaml).expect("parse");
            let result = manifest.validate();
            let output = serde_json::json!({
                "result": match &result {
                    Err(ManifestValidationError::UnknownOntology(_)) => "UnknownOntology".to_string(),
                    Err(other) => other.to_string(),
                    Ok(()) => "Ok".to_string(),
                }
            });
            match oracle.verify(&serde_json::Value::Null, &output) {
                OracleVerdict::Pass => {}
                OracleVerdict::Fail(message) => prop_assert!(false, "{message}"),
                OracleVerdict::Inconclusive => {}
            }
            emit_trace("manifest_rejects_unknown_ontology", "pass", "invariant");
        }

        /// P1 (invariant): a manifest with tier_3 entries is always rejected
        /// (the MAIA self-description doctrine enforced at parse time).
        #[test]
        fn manifest_rejects_tier3_via_any_kind(kind in "[a-z_]{1,20}", via in "[a-z_]{1,20}") {
            let yaml = format!(
                r#"
manifest:
  id: test
  category: company-source-manifest
  name: Test
  version: 0.1.0
company:
  symbol: TEST
  name: Test Co
  cik: "0000000000"
  ir_base: "https://example.com/ir"
source_tiers:
  tier_1_self_description: []
  tier_2_executive_voice: []
  tier_3_external:
    - kind: {kind}
      via: {via}
provenance_rule: test
exclusion_rule: test
"#
            );
            let manifest = CompanySourceManifest::from_yaml(&yaml).expect("parse");
            let result = manifest.validate();
            prop_assert!(
                matches!(result, Err(ManifestValidationError::Tier3EnabledByDefault { .. })),
                "any tier_3 entry must be rejected, got {result:?}"
            );
            emit_trace("manifest_rejects_tier3_via_any_kind", "pass", "invariant");
        }
    }
}
