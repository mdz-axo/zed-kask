//! Cross-document linkage test for the listening RAG skill (slice 7).
//!
//! Design: `kask/docs/explanation/company-corpus-design.md` §B4.1 + §B6 slice 6.
//!
//! Acceptance: a checkpoint from an earnings call linked to its 10-K source.
//! The RAG template reads corpus passages from multiple documents + KG triples
//! and emits verdicts with cross-source citations. The no-fabrication invariant
//! extends to the full corpus: every evidence field is a verbatim substring of
//! one of the source passages.
//!
//! These tests validate the RAG template structure + the cross-document fixture.
//! The actual LLM golden-file run happens when the skill is invoked via the
//! cascade; here we assert the preconditions that make the cross-source test
//! meaningful.

use std::path::PathBuf;

fn template_crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/templates/listening")
}

fn rag_template_path() -> PathBuf {
    template_crate_dir().join("apply-template-rag.j2")
}

fn fixture_dir() -> PathBuf {
    template_crate_dir().join("tests/fixtures")
}

fn read_file(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn read_rag_template() -> String {
    let path = rag_template_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read RAG template {}: {error}", path.display()))
}

// ── Cross-document fixture preconditions ────────────────────────────────────

/// The earnings-call fixture must contain the checkpoint (Germany datacenter,
/// Q3 fiscal 2025) that links to the strategic goal.
#[test]
fn earnings_fixture_contains_checkpoint() {
    let earnings = read_file("cross_doc_earnings.txt");
    assert!(
        earnings.contains("Germany") && earnings.contains("Q3 fiscal 2025"),
        "earnings fixture must contain the Germany datacenter checkpoint"
    );
    assert!(
        earnings.contains("double Azure AI capacity by fiscal 2027"),
        "earnings fixture must reference the strategic goal"
    );
}

/// The 10-K fixture must contain the same strategic goal and the same
/// checkpoint, so the KG triple can link them across documents.
#[test]
fn ten_k_fixture_contains_matching_goal_and_checkpoint() {
    let ten_k = read_file("cross_doc_10k.txt");
    assert!(
        ten_k.contains("double Azure AI infrastructure capacity by fiscal year 2027"),
        "10-K fixture must contain the strategic goal"
    );
    assert!(
        ten_k.contains("Germany region scheduled for Q3 of fiscal year 2025"),
        "10-K fixture must contain the same checkpoint"
    );
    assert!(
        ten_k.contains("gross margins to remain stable"),
        "10-K fixture must contain the margin claim for cross-source citation"
    );
}

/// Both fixtures must have distinct entity refs (the document identity that
/// the KG triple uses to link them).
#[test]
fn fixtures_have_distinct_entity_refs() {
    let earnings = read_file("cross_doc_earnings.txt");
    let ten_k = read_file("cross_doc_10k.txt");
    assert!(
        earnings.contains("company:MSFT:earnings:2024_Q4"),
        "earnings fixture must have its entity ref"
    );
    assert!(
        ten_k.contains("company:MSFT:sec_filing:10-K:2024"),
        "10-K fixture must have its entity ref"
    );
}

/// The earnings call has a short-term guidance change (the raise) that should
/// be filtered to ignored_short_term (no strategic linkage in the raise itself).
#[test]
fn earnings_fixture_contains_short_term_guidance_change() {
    let earnings = read_file("cross_doc_earnings.txt");
    assert!(
        earnings.contains("raising revenue guidance"),
        "earnings fixture must contain a short-term guidance change"
    );
}

// ── RAG template invariant tests ─────────────────────────────────────────────

/// The RAG template must enforce the no-fabrication invariant over corpus
/// passages (not just a single transcript).
#[test]
fn rag_template_enforces_no_fabrication_over_corpus() {
    let template = read_rag_template();
    assert!(
        template.contains("verbatim substring"),
        "RAG template must enforce the no-fabrication invariant"
    );
    assert!(
        template.contains("corpus_passages"),
        "RAG template must reference corpus_passages as the source"
    );
}

/// The RAG template must include cross-source citation in its output schema.
#[test]
fn rag_template_includes_cross_source_citation() {
    let template = read_rag_template();
    assert!(
        template.contains("sources"),
        "RAG template must include a sources field for cross-source citation"
    );
    assert!(
        template.contains("source_documents"),
        "RAG template must include source_documents on the checkpoint_map"
    );
    assert!(
        template.contains("Cross-source citation"),
        "RAG template must document the cross-source citation upgrade"
    );
}

/// The RAG template must accept KG triples as input for cross-document linkage.
#[test]
fn rag_template_accepts_kg_triples() {
    let template = read_rag_template();
    assert!(
        template.contains("kg_triples"),
        "RAG template must accept kg_triples as input"
    );
    assert!(
        template.contains("cross-document linkage"),
        "RAG template must document the KG triple linkage"
    );
}

/// The RAG template must include the checkpoint_drift detection via KG triples
/// (the management_consistency section's cross-quarter capability).
#[test]
fn rag_template_includes_checkpoint_drift_via_triples() {
    let template = read_rag_template();
    assert!(
        template.contains("CHECKPOINT DRIFT"),
        "RAG template must include checkpoint drift detection"
    );
    assert!(
        template.contains("KG triples to detect this"),
        "RAG template must reference KG triples for drift detection"
    );
}

/// The RAG template must include all 7 sections (same as the single-transcript
/// template).
#[test]
fn rag_template_includes_all_seven_sections() {
    let template = read_rag_template();
    for section in [
        "margin_trajectory",
        "working_capital_power",
        "moat_evidence",
        "capital_allocation",
        "expectations_gap_update",
        "guidance_vs_expectations",
        "management_consistency",
    ] {
        assert!(
            template.contains(section),
            "RAG template must include section '{section}'"
        );
    }
}

/// The RAG template must use the guidebook certainty vocabulary.
#[test]
fn rag_template_uses_guidebook_certainty_vocabulary() {
    let template = read_rag_template();
    for level in ["proximate", "probable", "possible"] {
        assert!(
            template.contains(level),
            "RAG template must use certainty level '{level}'"
        );
    }
}

// ── Manifest validation ──────────────────────────────────────────────────────

/// The manifest must reference both templates (single-transcript + RAG).
#[test]
fn manifest_references_both_templates() {
    let manifest_path = template_crate_dir().join("manifest.yaml");
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read manifest: {error}"));
    assert!(
        manifest.contains("listening/apply-template"),
        "manifest must reference the single-transcript template"
    );
    assert!(
        manifest.contains("listening/apply-template-rag"),
        "manifest must reference the RAG template"
    );
    assert!(
        manifest.contains("apply-template-rag.j2"),
        "manifest must reference the RAG template file"
    );
}
