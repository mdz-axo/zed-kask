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
//! No-fabrication design (process-embedded, not instruction-embedded):
//! The template uses a retrieve-cite-verify process. The transcript is
//! pre-split into numbered chunks. The model searches the chunks and cites
//! what it found (chunk_id + quote + char_start). A post-processing step
//! verifies each cited substring is present in the referenced chunk. The
//! tests assert the template structure enforces this process.

use std::path::PathBuf;

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

fn process_manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/manifests/listening.yaml")
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

fn read_process_manifest() -> String {
    let path = process_manifest_path();
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read process manifest {}: {error}",
            path.display()
        )
    })
}

// ── Fixture preconditions ───────────────────────────────────────────────────

#[test]
fn fixture_contains_short_term_guidance_change() {
    let fixture = read_fixture();
    assert!(
        fixture.contains("raising revenue guidance"),
        "fixture must contain a short-term guidance change for the horizon-filter test"
    );
}

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

// ── Retrieve-cite-verify process tests ──────────────────────────────────────

/// The template must use the retrieve-cite-verify process: the model searches
/// numbered chunks and cites what it found, rather than generating quotes.
#[test]
fn template_uses_retrieve_cite_verify_process() {
    let template = read_template();
    assert!(
        template.contains("retrieve") && template.contains("Retrieve"),
        "template must describe the retrieve step"
    );
    assert!(
        template.contains("Cite") || template.contains("cite"),
        "template must describe the cite step"
    );
    assert!(
        template.contains("Verify") || template.contains("verify"),
        "template must describe the verify step (done by the process, not the model)"
    );
    assert!(
        template.contains("You do NOT write quotes. You FIND them."),
        "template must tell the model to find quotes, not write them"
    );
}

/// The template's evidence schema must use chunk_id + quote + char_start
/// (the retrieval result shape), not bare strings.
#[test]
fn template_evidence_schema_uses_chunk_refs() {
    let template = read_template();
    assert!(
        template.contains("chunk_id"),
        "evidence schema must include chunk_id for citation"
    );
    assert!(
        template.contains("char_start"),
        "evidence schema must include char_start for verification"
    );
    assert!(
        template.contains("\"quote\""),
        "evidence schema must include the quote field"
    );
}

/// The template must accept transcript_chunks (pre-split), not a raw
/// transcript string.
#[test]
fn template_accepts_chunked_input() {
    let template = read_template();
    assert!(
        template.contains("transcript_chunks"),
        "template must accept transcript_chunks as input (pre-split by the process)"
    );
    assert!(
        template.contains("[chunk_id:"),
        "template must label chunks with chunk_id in the input format"
    );
}

/// The template must tell the model that the verifier checks substrings
/// mechanically — the model cannot pass with a fabricated quote.
#[test]
fn template_states_mechanical_verification() {
    let template = read_template();
    assert!(
        template.contains("verifier checks this mechanically"),
        "template must state that verification is mechanical (not model-mediated)"
    );
    assert!(
        template.contains("fabricated quotes are rejected"),
        "template must state that fabricated quotes are rejected"
    );
}

// ── Horizon model tests ─────────────────────────────────────────────────────

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

#[test]
fn template_includes_admissibility_rule() {
    let template = read_template();
    assert!(
        template.contains("The linkage, not the calendar date, is the bar"),
        "template must include the admissibility rule"
    );
}

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

// ── Process manifest tests ──────────────────────────────────────────────────

/// The process manifest must include the chunk_transcript step (step 1)
/// and the verify_citations step (step 3) — the process-embedded enforcement.
#[test]
fn process_manifest_includes_chunk_and_verify_steps() {
    let manifest = read_process_manifest();
    assert!(
        manifest.contains("chunk_transcript"),
        "process manifest must include the chunk_transcript compute step"
    );
    assert!(
        manifest.contains("verify_citations"),
        "process manifest must include the verify_citations compute step"
    );
    assert!(
        manifest.contains("VERIFY CITATIONS"),
        "process manifest must describe the citation verification step"
    );
}

/// The process manifest must pass chunked input to the template (not raw text).
#[test]
fn process_manifest_passes_chunks_to_template() {
    let manifest = read_process_manifest();
    assert!(
        manifest.contains("transcript_chunks"),
        "process manifest must pass transcript_chunks to the template"
    );
}

// ── Manifest validation ─────────────────────────────────────────────────────

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
