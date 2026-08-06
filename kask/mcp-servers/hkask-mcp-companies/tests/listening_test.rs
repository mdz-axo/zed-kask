//! Golden-file test for the listening skill (slice 5).
//!
//! Design: `kask/docs/explanation/earnings-transcript-analysis-design.md` §(d) slice 5.
//!
//! Three acceptance criteria:
//! 1. Every verdict cites ≥1 verbatim quote (substring check against source).
//! 2. Horizon-filter: the fixture has a bare short-term guidance change →
//!    output must place it in `ignored_short_term` with no verdict influence.
//! 3. Checkpoint: the fixture has a dated milestone with a nameable strategic
//!    linkage → output must place it on `checkpoint_map` with
//!    `strategic_goal_link` populated.
//!
//! These tests validate the FIXTURE and the TEMPLATE STRUCTURE. The actual
//! LLM golden-file run (template applied to fixture → JSON output → assertions)
//! happens when the skill is invoked via the cascade; here we assert the
//! preconditions that make the golden-file test meaningful:
//! - The fixture contains the required test material.
//! - The template enforces the no-fabrication invariant in its prompt text.
//! - The template's output schema includes the required fields.

use std::path::PathBuf;

/// Path to the listening registry crate, resolved from this crate's
/// manifest dir (the companies server is the natural home for transcript tests;
/// the template lives in the registry).
fn template_crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/templates/listening")
}

fn fixture_path() -> PathBuf {
    template_crate_dir().join("tests/fixtures/sample_transcript.txt")
}

fn template_path() -> PathBuf {
    template_crate_dir().join("apply-template.j2")
}

fn manifest_path() -> PathBuf {
    template_crate_dir().join("manifest.yaml")
}

fn read_fixture() -> String {
    let path = fixture_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()))
}

fn read_template() -> String {
    let path = template_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read template {}: {error}", path.display()))
}

fn read_manifest() -> String {
    let path = manifest_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read manifest {}: {error}", path.display()))
}

// ── Fixture preconditions ───────────────────────────────────────────────────

/// The fixture must contain a bare short-term guidance change (for the
/// horizon-filter test). This is a next-quarter raise with no explicit
/// strategic-path linkage in the same sentence.
#[test]
fn fixture_contains_short_term_guidance_change() {
    let fixture = read_fixture();
    // "raising revenue guidance to $68.5 to $69.1 billion" is a next-quarter
    // guidance change. The horizon-filter test asserts the template places
    // this in ignored_short_term (no strategic linkage in the guidance sentence
    // itself — the strategic linkage is in the separate Germany datacenter
    // checkpoint, not in the guidance raise).
    assert!(
        fixture.contains("raising revenue guidance"),
        "fixture must contain a short-term guidance change for the horizon-filter test"
    );
}

/// The fixture must contain a dated milestone with a nameable strategic
/// linkage (for the checkpoint test). The Germany datacenter in Q3 fiscal 2025
/// links to the "double Azure AI capacity by fiscal 2027" strategic goal.
#[test]
fn fixture_contains_strategic_checkpoint() {
    let fixture = read_fixture();
    assert!(
        fixture.contains("Germany datacenter") && fixture.contains("Q3"),
        "fixture must contain a dated checkpoint (Germany datacenter, Q3)"
    );
    assert!(
        fixture.contains("double Azure AI capacity by fiscal 2027"),
        "fixture must contain the strategic goal the checkpoint links to"
    );
}

/// The fixture must contain speaker markers (probe-verified shape) so the
/// template can attribute claims to speakers.
#[test]
fn fixture_contains_speaker_markers() {
    let fixture = read_fixture();
    assert!(
        fixture.contains("Satya Nadella:"),
        "fixture must have named speakers"
    );
    assert!(
        fixture.contains("Amy Hood:"),
        "fixture must have named speakers"
    );
    assert!(
        fixture.contains("Question-and-Answer Session"),
        "fixture must have the Q&A section marker"
    );
}

// ── Template invariant tests ────────────────────────────────────────────────

/// The template must enforce the no-fabrication invariant in its prompt text.
#[test]
fn template_enforces_no_fabrication_invariant() {
    let template = read_template();
    assert!(
        template.contains("verbatim substring"),
        "template must enforce the no-fabrication invariant (verbatim substring)"
    );
    assert!(
        template.contains("Fabricated quotes fail"),
        "template must warn that fabricated quotes fail the test"
    );
}

/// The template must include the horizon classification (the stance block).
#[test]
fn template_includes_horizon_classification() {
    let template = read_template();
    for class in [
        "seam_checkpoint",
        "tactical_event",
        "strategic_context",
        "short_term_only",
        "speculative_far",
    ] {
        assert!(
            template.contains(class),
            "template must include horizon class '{class}'"
        );
    }
    assert!(
        template.contains("ignored_short_term"),
        "template must include the ignored_short_term output field"
    );
}

/// The template must include the admissibility rule (the linkage bar).
#[test]
fn template_includes_admissibility_rule() {
    let template = read_template();
    assert!(
        template.contains("The linkage, not the calendar date, is the bar"),
        "template must include the admissibility rule"
    );
}

/// The template must include the checkpoint_map in its output schema, with
/// the strategic_goal_link field (the checkpoint test asserts this is populated).
#[test]
fn template_includes_checkpoint_map_schema() {
    let template = read_template();
    assert!(
        template.contains("checkpoint_map"),
        "template must include checkpoint_map in the output schema"
    );
    assert!(
        template.contains("strategic_goal_link"),
        "template must include strategic_goal_link in the checkpoint_map schema"
    );
}

/// The template must include all 7 sections from the design §(b).
#[test]
fn template_includes_all_seven_sections() {
    let template = read_template();
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
            "template must include section '{section}'"
        );
    }
}

/// The template must use the guidebook certainty vocabulary (proximate/probable/
/// possible), not a numeric scale.
#[test]
fn template_uses_guidebook_certainty_vocabulary() {
    let template = read_template();
    for level in ["proximate", "probable", "possible"] {
        assert!(
            template.contains(level),
            "template must use certainty level '{level}'"
        );
    }
}

// ── Manifest validation ────────────────────────────────────────────────────

/// The template crate manifest must exist and reference the apply-template.j2.
#[test]
fn manifest_references_template() {
    let manifest = read_manifest();
    assert!(
        manifest.contains("listening/apply-template"),
        "manifest must reference the apply-template template"
    );
    assert!(
        manifest.contains("apply-template.j2"),
        "manifest must reference the apply-template.j2 file"
    );
}
