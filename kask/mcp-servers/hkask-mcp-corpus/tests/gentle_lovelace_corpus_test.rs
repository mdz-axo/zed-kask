//! Integration tests for EmbedService corpus parsing.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.
//! expect: "Every test verifies a stated behavioral property of a public seam"

use hkask_services_corpus::EmbedService;

/// Parse the Gentle Lovelace corpus config and verify all 11 works,
/// 4 dimension centroids, 4 tag sets, tag_weights, and budget deserialize.
#[test]
fn parse_gentle_lovelace_corpus_yaml() {
    // Resolve from workspace root via CARGO_MANIFEST_DIR
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("../../registry/styles/gentle-lovelace/corpus.yaml");

    let config = EmbedService::parse_config(&config_path)
        .expect("Failed to parse gentle-lovelace corpus.yaml");

    // ── Works ────────────────────────────────────────
    assert_eq!(config.works.len(), 11, "All 11 works must parse");
    let lovelace = config
        .works
        .iter()
        .find(|w| w.slug == "lovelace-notes")
        .expect("Lovelace Notes must be present");
    assert!(
        lovelace.url.contains("fourmilab.ch"),
        "Lovelace source must be fourmilab mirror"
    );
    assert_eq!(lovelace.dimensions, vec!["Lovelace"]);
    assert_eq!(lovelace.mds_categories, vec!["domain", "composition"]);

    let hopper = config
        .works
        .iter()
        .find(|w| w.slug == "hopper-mark1-manual")
        .expect("Hopper manual must be present");
    assert!(hopper.local_path.is_some(), "Hopper must have local_path");
    // All works now reference pre-extracted .txt files with format: text or format: web
    assert_eq!(hopper.dimensions, vec!["Hopper"]);

    let gentle_dlc = config
        .works
        .iter()
        .find(|w| w.slug == "gentle-docs-like-code")
        .expect("Docs Like Code must be present");
    assert_eq!(gentle_dlc.dimensions, vec!["Gentle"]);

    let wtd = config
        .works
        .iter()
        .find(|w| w.slug == "writethedocs-guide")
        .expect("WTD guide must be present");
    assert_eq!(wtd.dimensions, vec!["Schriver"]);

    // ── Dimension centroids ─────────────────────────
    assert_eq!(config.dimension_centroids.len(), 4);
    let gentle_dc = config
        .dimension_centroids
        .iter()
        .find(|dc| dc.name == "Gentle")
        .expect("Gentle dimension must exist");
    assert!(
        (gentle_dc.weight - 0.50).abs() < 0.001,
        "Gentle weight must be 0.50"
    );
    assert!(gentle_dc.ref_name.contains("gentle-centroid"));

    // ── Tag sets ────────────────────────────────────
    assert_eq!(config.tag_sets.len(), 4, "4 orthogonal tag sets");
    let section_type_ts = config
        .tag_sets
        .iter()
        .find(|ts| ts.name == "section_type")
        .expect("section_type tag set must exist");
    assert!(section_type_ts.values.contains(&"Statement".to_string()));

    // ── Tag weights ─────────────────────────────────
    assert!(
        !config.tag_weights.is_empty(),
        "tag_weights must be populated"
    );
    let spec_weights = config
        .tag_weights
        .get("specification")
        .expect("specification tag weights must exist");
    assert!(spec_weights.contains_key("Gentle"));
    assert!(spec_weights.contains_key("Lovelace"));

    // ── Budget ──────────────────────────────────────
    // Budget must deserialize as Flat variant
    let budget_resolved = config.budget.resolve(1000);
    assert!(
        budget_resolved > 0,
        "Budget must resolve to a positive value"
    );

    // ── Methods ─────────────────────────────────────
    assert_eq!(config.methods.len(), 5, "5 declared methods");
    let clarity = config
        .methods
        .iter()
        .find(|m| m.name == "clarity")
        .expect("clarity method must exist");
    assert!(clarity.threshold.is_some(), "clarity must have threshold");
    assert!((clarity.threshold.unwrap() - 0.5).abs() < 0.001);

    // ── Foundational rules ──────────────────────────
    assert_eq!(config.foundational_rules.len(), 7, "7 foundational rules");
    let docs_as_code = config
        .foundational_rules
        .iter()
        .find(|r| r.slug == "docs-as-code")
        .expect("docs-as-code rule must exist");
    assert_eq!(docs_as_code.dimensions, vec!["Gentle"]);
    assert_eq!(docs_as_code.section_type.as_deref(), Some("Statement"));
}
