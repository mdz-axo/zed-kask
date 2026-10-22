//! Tool-behavior contract tests for the scenarios MCP server.
//!
//! Drives the real `#[tool]` methods through their public `Parameters<T>`
//! seam, in-process, with in-memory and temporary file-backed stores. Covers the testing
//! standard minimum (docs/reference/mcp-servers/README.md §Testing standard):
//! happy path, invalid input, boundary/edge cases, and error-specificity.
//!
//! No network: persistence regressions use temporary files and deterministic
//! filesystem failures; other tools operate on caller-supplied inputs.

#![cfg(test)]

use hkask_mcp_scenarios::equity_forecast::StoredEquityForecast;
use hkask_mcp_scenarios::requests::{
    AssessRequest, BrainstormRequest, CalibrateRequest, DeclareRequest, FrameDocumentRequest,
    OutcomeEntry, PosteriorEvidenceEntry, PosteriorsRequest, QuantifyRequest, ScoreRequest,
    StatusRequest, TriageRequest,
};
use hkask_mcp_scenarios::types::{
    EventDependency, ScenarioEvent, ScenarioType, SubQuestion, TimeHorizon,
};
use hkask_mcp_scenarios::{ForecastStore, JournalStore, ProjectStore, ScenariosServer};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// The equity forecast store (ported from the companies server, Phase A P1)
// is in-memory in every behavior test: no test asserts on-disk equity
// persistence yet; None keeps the clean-slate state every test starts from.
fn equity_forecast_store() -> Arc<Mutex<JournalStore<StoredEquityForecast>>> {
    Arc::new(Mutex::new(JournalStore::new(None)))
}

/// Build a server backed by an empty in-memory store and empty caches — the
/// clean-slate state every test starts from.
fn before_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00Z".parse().expect("clock")
}
fn at_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:01Z".parse().expect("clock")
}
fn clocked_store(path: Option<std::path::PathBuf>) -> ForecastStore {
    let mut store = ForecastStore::new(path);
    store.set_clock(at_cutoff);
    store
}
fn cutoffs(
    events: &[ScenarioEvent],
) -> std::collections::BTreeMap<String, chrono::DateTime<chrono::Utc>> {
    events
        .iter()
        .map(|e| {
            (
                e.id.clone(),
                e.deadline
                    .and_hms_opt(12, 0, 1)
                    .expect("cutoff time")
                    .and_utc(),
            )
        })
        .collect()
}
fn set_clock(server: &ScenariosServer, clock: fn() -> chrono::DateTime<chrono::Utc>) {
    server
        .forecast_store
        .lock()
        .expect("forecast store")
        .set_clock(clock);
}

fn make_server() -> ScenariosServer {
    let forecast_store = Arc::new(Mutex::new(clocked_store(None)));
    let tree_cache = Mutex::new(None);
    let project_store = Mutex::new(ProjectStore::new(None));
    ScenariosServer::new(
        WebID::new(),
        forecast_store,
        equity_forecast_store(),
        tree_cache,
        project_store,
    )
}

fn file_backed_server(path: &std::path::Path) -> ScenariosServer {
    ScenariosServer::new(
        WebID::new(),
        Arc::new(Mutex::new(clocked_store(Some(path.to_path_buf())))),
        equity_forecast_store(),
        Mutex::new(None),
        Mutex::new(ProjectStore::new(None)),
    )
}

/// A server whose project store is file-backed at `path` — the restart
/// seam for the durable tree cache (PR-07).
fn project_backed_server(path: &std::path::Path) -> ScenariosServer {
    ScenariosServer::new(
        WebID::new(),
        Arc::new(Mutex::new(clocked_store(None))),
        equity_forecast_store(),
        Mutex::new(None),
        Mutex::new(ProjectStore::new(Some(path.to_path_buf()))),
    )
}

/// The store key `scenario_score` derives for this fixture's single event
/// (`"{forecast_id}:{event_id}"`).
const REGRESSION_KEY: &str = "persistence-regression:event";

async fn declare(server: &ScenariosServer, forecast_id: &str, events: Vec<ScenarioEvent>) {
    set_clock(server, before_cutoff);
    server
        .scenario_declare(Parameters(DeclareRequest {
            forecast_id: forecast_id.into(),
            resolution_at: cutoffs(&events),
            events,
            resolver: "test-source".into(),
        }))
        .await
        .expect("prospective declaration");
    set_clock(server, at_cutoff);
}

fn persistence_score_request(probability: f64, occurred: bool) -> ScoreRequest {
    ScoreRequest {
        resolver: "test-source".into(),
        forecast_id: "persistence-regression".to_string(),
        events: vec![independent_event("event", "durable event", probability)],
        outcomes: vec![OutcomeEntry {
            event_id: "event".to_string(),
            occurred,
        }],
    }
}

/// An `AssessRequest` with every caller metric omitted — the form the
/// derivation tests start from, so every reported metric comes only from
/// the project's stored state.
fn assess_request_all_omitted(project_id: &str) -> AssessRequest {
    AssessRequest {
        project_id: project_id.to_string(),
        perspective_count: None,
        disagreement_score: None,
        event_count: None,
        events_with_dependencies: None,
        strategies_generated: None,
        strategies_implemented: None,
        learning_events: None,
        has_early_warning_indicators: None,
    }
}

/// Parse the journal's newline-delimited entries into (key, record) pairs.
/// Panics on malformed lines so a torn journal fails the test loudly rather
/// than being silently accepted as the expected durable state.
fn parse_journal_entries(
    bytes: &[u8],
) -> Result<Vec<(String, serde_json::Value)>, Box<dyn std::error::Error>> {
    let mut entries = Vec::new();
    for line in std::str::from_utf8(bytes)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: serde_json::Value = serde_json::from_str(line)?;
        let key = entry["key"]
            .as_str()
            .unwrap_or_else(|| panic!("journal entry must carry a key, got: {entry}"))
            .to_string();
        entries.push((key, entry["record"].clone()));
    }
    Ok(entries)
}

/// Assert the recovered store holds exactly the journal's final record:
/// one resolved forecast (probability 0.9, outcome false) with Brier
/// (0.9 - 0)^2 — never a duplicate, the stale snapshot's version, or the
/// journal's pending first entry.
fn assert_recovered_final_record(status: &serde_json::Value) {
    assert_eq!(status["pipeline"]["forecast_count"], 1);
    assert_eq!(status["pipeline"]["resolved_count"], 1);
    assert_eq!(status["pipeline"]["pending_count"], 0);
    let brier = status["pipeline"]["overall_brier"]
        .as_f64()
        .expect("recovered resolved record must be Brier-scored");
    assert!(
        (brier - 0.81).abs() < 1e-9,
        "Brier must be (0.9 - 0)^2, got {brier}"
    );
    let recent = &status["pipeline"]["recent_forecasts"][0];
    assert_eq!(recent["forecast_id"], "persistence-regression");
    assert_eq!(recent["event_id"], "event");
    assert_eq!(recent["probability"], 0.9);
    assert_eq!(recent["outcome"], false);
}

/// expect: "A failed snapshot must not erase my scored forecast." [P1]
/// pre: journal append succeeds but snapshot publication is blocked.
/// post: the journal recovers the complete resolved record after reopening.
#[tokio::test]
async fn snapshot_failure_preserves_journal_for_recovery() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let snapshot_path = directory.path().join("forecasts.json");
    let journal_path = snapshot_path.with_extension("json.journal");
    let server = file_backed_server(&snapshot_path);
    declare(
        &server,
        "persistence-regression",
        vec![independent_event("event", "durable event", 0.9)],
    )
    .await;
    // A directory at the destination rejects both direct writes and atomic
    // replacement, independently of the test runner's filesystem privileges.
    std::fs::create_dir(&snapshot_path)?;
    let result = server
        .scenario_score(Parameters(persistence_score_request(0.9, false)))
        .await;
    assert!(result.is_err(), "persistence failure must reach the caller");
    // The retained journal must be the complete two-entry history for the
    // scored record — the pending insert followed by the resolved update —
    // not merely a non-empty file.
    let entries = parse_journal_entries(&std::fs::read(&journal_path)?)?;
    assert_eq!(entries.len(), 2, "journal must retain insert + resolution");
    assert_eq!(entries[0].0, REGRESSION_KEY);
    assert_eq!(entries[0].1["probability"], 0.9);
    assert_eq!(entries[0].1["outcome"], serde_json::Value::Null);
    assert_eq!(entries[1].0, REGRESSION_KEY);
    assert_eq!(entries[1].1["probability"], 0.9);
    assert_eq!(entries[1].1["outcome"], false);
    assert!(
        entries[1].1["resolved_at"].is_string(),
        "the journaled resolution must carry its resolution date"
    );
    // The failed publication must clean up its temporary snapshot file.
    let residue: Vec<String> = std::fs::read_dir(directory.path())?
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp"))
        .collect();
    assert!(
        residue.is_empty(),
        "failed publication left residue: {residue:?}"
    );
    std::fs::remove_dir(&snapshot_path)?;

    // Recovery must be observable through the read seam BEFORE any retry:
    // a re-score would repair a broken replay and hide the regression.
    let recovered = file_backed_server(&snapshot_path);
    let status = parse(
        &recovered
            .scenario_status(Parameters(StatusRequest {}))
            .await?,
    );
    assert_recovered_final_record(&status);

    // With the blockage cleared, the retry must succeed, publish a snapshot,
    // and only then clear the journal.
    recovered
        .scenario_score(Parameters(persistence_score_request(0.9, false)))
        .await?;
    assert!(std::fs::read(&journal_path)?.is_empty());
    let snapshot: serde_json::Value = serde_json::from_slice(&std::fs::read(&snapshot_path)?)?;
    assert_eq!(snapshot[REGRESSION_KEY]["outcome"], false);
    assert_eq!(snapshot[REGRESSION_KEY]["probability"], 0.9);
    let reopened = file_backed_server(&snapshot_path);
    let status = parse(
        &reopened
            .scenario_status(Parameters(StatusRequest {}))
            .await?,
    );
    assert_eq!(status["pipeline"]["forecast_count"], 1);
    assert_eq!(status["pipeline"]["pending_count"], 0);
    Ok(())
}

/// expect: "A published snapshot with its uncleared journal must recover my
/// forecast exactly once, with the journal's last write winning." [P1]
/// pre: a snapshot is published and the process dies before journal
/// truncation — reproduced by restoring the pre-truncation journal bytes
/// over a published snapshot whose overlapping record carries different
/// values, so a skipped or wrong-order replay is detectable.
/// post: reopening yields exactly one record matching the journal's final
/// entry — never the stale snapshot version, the pending first entry, or a
/// duplicate.
#[tokio::test]
async fn snapshot_with_uncleared_journal_recovers_exactly_once()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let snapshot_path = directory.path().join("forecasts.json");
    let journal_path = snapshot_path.with_extension("json.journal");
    // Force a failed publication to obtain production-written journal
    // entries (probability 0.9, resolved false) that truncation would
    // otherwise discard.
    let server = file_backed_server(&snapshot_path);
    declare(
        &server,
        "persistence-regression",
        vec![independent_event("event", "durable event", 0.9)],
    )
    .await;
    std::fs::create_dir(&snapshot_path)?;
    assert!(
        server
            .scenario_score(Parameters(persistence_score_request(0.9, false)))
            .await
            .is_err()
    );
    let retained_journal = std::fs::read(&journal_path)?;
    std::fs::remove_dir(&snapshot_path)?;

    // Publish a snapshot holding a DIFFERENT version of the same record
    // (probability 0.3, resolved true). The journal is removed first so the
    // publisher starts from an empty store — `scenario_score` never rewrites
    // the probability of a record it already holds.
    std::fs::remove_file(&journal_path)?;
    let publisher = file_backed_server(&snapshot_path);
    declare(
        &publisher,
        "persistence-regression",
        vec![independent_event("event", "durable event", 0.3)],
    )
    .await;
    publisher
        .scenario_score(Parameters(persistence_score_request(0.3, true)))
        .await?;
    assert!(
        std::fs::read(&journal_path)?.is_empty(),
        "successful publication must clear the journal"
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&std::fs::read(&snapshot_path)?)?;
    assert_eq!(snapshot[REGRESSION_KEY]["probability"], 0.3);
    assert_eq!(snapshot[REGRESSION_KEY]["outcome"], true);
    // Simulate the crash window: the journal from before the publication is
    // still present next to the newly published snapshot.
    std::fs::write(&journal_path, &retained_journal)?;

    // Reopen: the journal's last entry (probability 0.9, outcome false) must
    // win over the snapshot without duplicating the record.
    let reopened = file_backed_server(&snapshot_path);
    let status = parse(
        &reopened
            .scenario_status(Parameters(StatusRequest {}))
            .await?,
    );
    assert_recovered_final_record(&status);
    Ok(())
}

/// expect: "A journal failure cannot be reported as a saved forecast." [P1]
/// pre: journal append is blocked, while the snapshot destination is writable.
/// post: the tool errors without publishing or admitting an unsaved record.
#[tokio::test]
async fn journal_failure_is_surfaced_before_memory_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let snapshot_path = directory.path().join("forecasts.json");
    let server = file_backed_server(&snapshot_path);
    std::fs::create_dir(snapshot_path.with_extension("json.journal"))?;
    set_clock(&server, before_cutoff);
    let result = server
        .scenario_declare(Parameters(DeclareRequest {
            forecast_id: "persistence-regression".into(),
            resolution_at: cutoffs(&[independent_event("event", "durable event", 0.9)]),
            events: vec![independent_event("event", "durable event", 0.9)],
            resolver: "test-source".into(),
        }))
        .await;
    assert!(result.is_err(), "journal failure must reach the caller");
    let status = parse(&server.scenario_status(Parameters(StatusRequest {})).await?);
    assert_eq!(status["pipeline"]["forecast_count"], 0);
    assert!(!snapshot_path.exists());
    Ok(())
}

/// Parse a tool output string, unwrapping the `{"content": ...}` envelope.
/// Panics on unparseable output so a malformed envelope fails the test loudly
/// rather than silently returning `None`.
fn parse(output: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(output)
        .unwrap_or_else(|| panic!("tool output must be valid JSON, got: {output}"))
}

/// A minimal valid independent event (no dependencies, no sub-questions) with a
/// caller-chosen probability. Used as the quantification input fixture.
fn independent_event(identifier: &str, name: &str, probability: f64) -> ScenarioEvent {
    ScenarioEvent {
        id: identifier.to_string(),
        name: name.to_string(),
        question: format!("Will {name} occur by 2027-12-31?"),
        deadline: before_cutoff().date_naive(),
        time_horizon: TimeHorizon::Strategic,
        scenario_type: ScenarioType::CompanyAnalysis,
        subject: "ACME".to_string(),
        probability,
        basis: None,
        depends_on: vec![],
        sub_questions: vec![],
        base_rate: None,
        reference_class: None,
        brier_score: None,
        update_count: 0,
    }
}

// ── Happy path ───────────────────────────────────────────────────────────────

/// `scenario_status` on a fresh server reports an empty pipeline: zero
/// forecasts, no cached tree, and an empty recent-forecasts list. A non-empty
/// `ontology` anchor confirms the semantic-span wiring is intact.
#[tokio::test]
async fn scenario_status_reports_empty_state() {
    let server = make_server();
    let output = server
        .scenario_status(Parameters(StatusRequest {}))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    let pipeline = parsed
        .get("pipeline")
        .unwrap_or_else(|| panic!("status must carry a pipeline object, got: {parsed}"));
    assert_eq!(
        pipeline["forecast_count"].as_u64(),
        Some(0),
        "a fresh store holds no forecasts"
    );
    assert_eq!(
        pipeline["pending_count"].as_u64(),
        Some(0),
        "with no forecasts none can be pending"
    );
    assert!(
        pipeline["recent_forecasts"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "recent_forecasts must be an empty array, got: {parsed}"
    );
    assert!(
        parsed.get("event_tree").is_some_and(|tree| tree.is_null()),
        "no tree has been quantified yet, so event_tree must be null, got: {parsed}"
    );
    assert!(
        parsed.get("ontology").is_some(),
        "the ontology anchor must be present on status output"
    );
}

/// `scenario_quantify` on a single independent event resolves its marginal to
/// the intrinsic prior and sets the joint probability equal to it (a one-node
/// tree's joint is just the root's probability).
#[tokio::test]
async fn scenario_quantify_resolves_single_independent_event() {
    let server = make_server();
    let event = independent_event("evt-1", "ACME launches product X", 0.3);
    let output = server
        .scenario_quantify(Parameters(QuantifyRequest {
            events: vec![event],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    assert_eq!(
        parsed["event_count"].as_u64(),
        Some(1),
        "a single-event tree has one node"
    );
    assert!(
        (parsed["joint_probability"].as_f64().unwrap_or(-1.0) - 0.3).abs() < 1e-9,
        "joint probability of a one-node tree equals the root prior, got: {parsed}"
    );
    let node = parsed["nodes"]
        .as_array()
        .and_then(|nodes| nodes.first())
        .unwrap_or_else(|| panic!("nodes array must be non-empty, got: {parsed}"));
    assert!(
        (node["marginal_probability"].as_f64().unwrap_or(-1.0) - 0.3).abs() < 1e-9,
        "the root's marginal equals its prior with no parents, got: {parsed}"
    );
    // Quantifying caches the tree so a subsequent status reflects it.
    let status = parse(
        &server
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("tool ok"),
    );
    assert!(
        status.get("event_tree").is_some_and(|tree| !tree.is_null()),
        "scenario_quantify must populate the tree cache, got: {status}"
    );
}

/// `scenario_triage` classifies a well-specified question (deadline, reference
/// class, clear resolution, enough words) as well_specified and forecastable —
/// the top of the Goldilocks triage band.
#[tokio::test]
async fn scenario_triage_classifies_fully_specified_question_as_well_specified() {
    let server = make_server();
    let output = server
        .scenario_triage(Parameters(TriageRequest {
            question: "Will ACME reach $10B revenue by end of 2027?".to_string(),
            has_deadline: Some(true),
            has_reference_class: Some(true),
            has_resolution_criteria: Some(true),
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    assert_eq!(
        parsed["difficulty"].as_str(),
        Some("well_specified"),
        "a fully-specified question is well_specified, got: {parsed}"
    );
    assert_eq!(
        parsed["is_forecastable"].as_bool(),
        Some(true),
        "a well_specified question is forecastable"
    );
    assert_eq!(
        parsed["scores"]["overall"]
            .as_f64()
            .map(|score| score >= 0.7),
        Some(true),
        "overall score must clear the OVERALL_STRONG threshold, got: {parsed}"
    );
}

// ── Boundary / edge cases ────────────────────────────────────────────────────

/// `scenario_triage` with no deadline, no reference class, and a terse question
/// falls below the goldilocks floor into needs_refinement and is not
/// forecastable — the bottom of the triage band.
#[tokio::test]
async fn scenario_triage_classifies_vague_question_as_needs_refinement() {
    let server = make_server();
    let output = server
        .scenario_triage(Parameters(TriageRequest {
            question: "What about the economy?".to_string(),
            has_deadline: Some(false),
            has_reference_class: Some(false),
            has_resolution_criteria: Some(false),
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    assert_eq!(
        parsed["difficulty"].as_str(),
        Some("needs_refinement"),
        "an under-specified question is needs_refinement, got: {parsed}"
    );
    assert_eq!(
        parsed["is_forecastable"].as_bool(),
        Some(false),
        "a needs_refinement question is not forecastable"
    );
}

// ── Slice 1 fidelity pins (PR-03/04/06/12/14) ──────

/// PR-03: the Phase-5 calibration evidence is scoped to the project's
/// subject — a multi-subject store must not leak other projects'
/// forecasts into the assessment. The projects are created by
/// `scenario_frame_document` (PR-01); their ids default to their subjects.
#[tokio::test]
async fn scenario_assess_calibration_curve_is_subject_scoped() {
    let server = make_server();
    // Two projects (one per subject), one resolved forecast each.
    for subject in ["alpha", "beta"] {
        server
            .scenario_frame_document(Parameters(FrameDocumentRequest {
                subject: subject.to_string(),
                project_id: None,
                answers: serde_json::json!({
                    "focal_question": format!("What is next for {subject}?")
                })
                .into(),
            }))
            .await
            .expect("frame_document ok");
        let mut event = independent_event("evt", "durable event", 0.9);
        event.subject = subject.to_string();
        let occurred = subject == "alpha";
        declare(&server, &format!("scope-{subject}"), vec![event.clone()]).await;
        server
            .scenario_score(Parameters(ScoreRequest {
                resolver: "test-source".into(),
                forecast_id: format!("scope-{subject}"),
                events: vec![event],
                outcomes: vec![OutcomeEntry {
                    event_id: "evt".to_string(),
                    occurred,
                }],
            }))
            .await
            .expect("score ok");
    }
    let output = server
        .scenario_assess(Parameters(assess_request_all_omitted("alpha")))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(
        parsed["calibration"]["resolved_forecasts"].as_u64(),
        Some(1),
        "the assessment curve must count only the assessed project's subject records, got: {parsed}"
    );
}

/// PR-06: the brainstorm default research string must not instruct agents to
/// call `scenario_research` — a tool that no longer exists on the surface.
#[tokio::test]
async fn scenario_brainstorm_default_context_never_names_removed_scenario_research() {
    let server = make_server();
    let output = server
        .scenario_brainstorm(Parameters(BrainstormRequest {
            subject: "ACME".to_string(),
            time_horizon: None,
            research_context: None,
            personas: None,
            start_round: None,
        }))
        .await
        .expect("tool ok");
    assert!(
        !output.contains("scenario_research"),
        "the default research string must not name the removed scenario_research tool, got: {output}"
    );
}

/// Caller-supplied trees are valid without preceding scenario-tool calls.
#[tokio::test]
async fn scenario_quantify_called_cold_emits_no_sequence_note() {
    let server = make_server();
    let event = independent_event("evt-cold", "cold entry event", 0.4);
    let output = server
        .scenario_quantify(Parameters(QuantifyRequest {
            events: vec![event],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert!(
        parsed.get("sequence_note").is_none(),
        "a cold quantify with a caller-supplied tree is a legitimate entry — no advisory, got: {parsed}"
    );
}

/// PR-14: the tree-node certainty field is named for what it measures —
/// |P − 0.5| × 2 is a certainty distance (1 = most certain). The old
/// `variance_contribution` misnomer is deleted, not aliased.
#[tokio::test]
async fn scenario_quantify_emits_certainty_distance_not_variance_contribution() {
    let server = make_server();
    let event = independent_event("evt-cd", "certainty distance event", 0.9);
    let output = server
        .scenario_quantify(Parameters(QuantifyRequest {
            events: vec![event],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    let node = &parsed["nodes"][0];
    let distance = node["certainty_distance"]
        .as_f64()
        .unwrap_or_else(|| panic!("the node carries certainty_distance, got: {parsed}"));
    assert!(
        (distance - 0.8).abs() < 1e-9,
        "|0.9 − 0.5| × 2 = 0.8 — the value is unchanged, only the name, got {distance}"
    );
    assert!(
        node.get("variance_contribution").is_none(),
        "the old misnomer is deleted, not aliased, got: {parsed}"
    );
}

/// `scenario_brainstorm` clamps `start_round` into the valid [1, 4] range. A
/// caller passing 5 must not get an out-of-range starting round or an empty
/// protocol — it clamps to 4 and emits the final round only.
#[tokio::test]
async fn scenario_brainstorm_clamps_start_round_into_range() {
    let server = make_server();
    let output = server
        .scenario_brainstorm(Parameters(BrainstormRequest {
            subject: "ACME".to_string(),
            time_horizon: Some("strategic".to_string()),
            research_context: None,
            personas: None,
            start_round: Some(5),
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    assert_eq!(
        parsed["starting_round"].as_u64(),
        Some(4),
        "start_round 5 must clamp to the maximum valid round 4, got: {parsed}"
    );
    assert_eq!(
        parsed["total_rounds"].as_u64(),
        Some(1),
        "only round 4 survives the clamp, so exactly one round is active, got: {parsed}"
    );
}

// ── Invalid input + error-specificity ────────────────────────────────────────

/// `scenario_quantify` with no events returns a structured `invalid_argument`
/// error whose message names the defect ("no events"). Error-specificity: the
/// kind distinguishes a caller-input defect from an internal failure.
#[tokio::test]
async fn scenario_quantify_rejects_empty_events_as_invalid_argument() {
    let server = make_server();
    let error = server
        .scenario_quantify(Parameters(QuantifyRequest { events: vec![] }))
        .await
        .expect_err("empty events must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "missing input is a caller defect, not an internal error, got: {error:?}"
    );
    assert!(
        error.message.contains("no events"),
        "the error message must name the defect, got: {error:?}"
    );
}

/// `scenario_quantify` with an out-of-range probability returns an
/// `invalid_argument` error whose message identifies the offending event and
/// the bad value. This is the error-specificity check: a generic "bad input"
/// message would not let a caller locate the failing event.
#[tokio::test]
async fn scenario_quantify_rejects_out_of_range_probability_as_invalid_argument() {
    let server = make_server();
    let event = independent_event("evt-bad", "impossible event", 1.5);
    let error = server
        .scenario_quantify(Parameters(QuantifyRequest {
            events: vec![event],
        }))
        .await
        .expect_err("an out-of-range probability must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "an invalid probability is a caller-input defect, got: {error:?}"
    );
    assert!(
        error.message.contains("impossible event"),
        "the error must name the offending event, got: {error:?}"
    );
    assert!(
        error.message.contains("not in [0, 1]"),
        "the error must state the probability constraint, got: {error:?}"
    );
}

// Historical fitting remains available in the pure math crate, but is not
// automatically applied to later forecasts without prospective evidence.
#[tokio::test]
async fn scenario_calibrate_withholds_unsupported_historical_correction() {
    let server = make_server();
    let events: Vec<_> = (0..5)
        .map(|i| independent_event(&format!("history-{i}"), "history", 0.8))
        .collect();
    let outcomes = events
        .iter()
        .map(|event| OutcomeEntry {
            event_id: event.id.clone(),
            occurred: true,
        })
        .collect();
    declare(&server, "history", events.clone()).await;
    server
        .scenario_score(Parameters(ScoreRequest {
            forecast_id: "history".into(),
            events,
            outcomes,
            resolver: "test-source".into(),
        }))
        .await
        .expect("resolved history");
    let output = server
        .scenario_calibrate(Parameters(CalibrateRequest {
            question: "Model estimate".into(),
            sub_questions: vec![SubQuestion {
                question: "Evidence".into(),
                estimate: 0.8,
                confidence: 0.9,
            }],
            reference_class: None,
            base_rate: None,
            reference_count: None,
        }))
        .await
        .expect("estimate");
    let body = parse(&output);
    assert!(
        body["adjustment_status"]
            .as_str()
            .expect("withheld reason")
            .contains("withheld")
    );
    assert!(body.get("isotonic_calibrated_probability").is_none());
    assert!(body.get("calibration_adjusted_probability").is_none());
    assert_eq!(
        body["calibrated_probability"], 0.8,
        "history does not correct the estimate"
    );
}

// ── scenario_assess unreported-metrics contract ─────────────────────────────

/// `scenario_assess` with every caller metric omitted must not fabricate
/// measurements: the phases whose scores depend on metrics that are
/// neither supplied nor derivable from the project record are withheld as
/// insufficient data (null score + a gap naming the missing metric), the
/// output names every such metric, and the overall score averages only the
/// reported phases. An unreported metric is not a zero. Preparation is
/// scored from the project's framing document (PR-01) — reported even
/// with every caller metric omitted.
#[tokio::test]
async fn scenario_assess_unreported_metrics_withhold_dependent_phases() {
    let server = make_server();
    // The project exists with a framing document but no quantified tree:
    // event/dependency counts are not derivable, and the framing scores
    // Preparation directly.
    server
        .scenario_frame_document(Parameters(FrameDocumentRequest {
            subject: "ACME".to_string(),
            project_id: None,
            answers: serde_json::json!({
                "focal_question": "What should we watch at ACME over the next year?"
            })
            .into(),
        }))
        .await
        .expect("frame_document ok");
    let output = server
        .scenario_assess(Parameters(assess_request_all_omitted("ACME")))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    // Every metric that is neither supplied nor derivable is named — six
    // of the seven (perspective_count is not required: the framing
    // document scores Preparation).
    let unreported: Vec<String> = parsed["unreported_metrics"]
        .as_array()
        .unwrap_or_else(|| panic!("assessment must carry unreported_metrics, got: {parsed}"))
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    for metric in [
        "disagreement_score",
        "event_count",
        "events_with_dependencies",
        "strategies_generated",
        "strategies_implemented",
        "has_early_warning_indicators",
    ] {
        assert!(
            unreported.contains(&metric.to_string()),
            "unreported_metrics must name {metric}, got: {unreported:?}"
        );
    }
    assert_eq!(
        unreported.len(),
        6,
        "exactly the six non-derivable metrics were omitted, got: {unreported:?}"
    );

    // Preparation is scored from the framing document — reported, not
    // withheld, even with every caller metric omitted.
    assert!(
        parsed["phases"]["preparation"]["score"].is_number(),
        "preparation scores from the framing document, got: {parsed}"
    );

    // Phases 2-4 depend on the unreported metrics: their scores are
    // withheld (null), never zero-derived, and their gaps name the
    // missing metrics.
    let withheld = [
        ("exploration", &["event_count", "disagreement_score"][..]),
        (
            "development",
            &["event_count", "events_with_dependencies"][..],
        ),
        (
            "implementation",
            &[
                "strategies_generated",
                "strategies_implemented",
                "has_early_warning_indicators",
            ][..],
        ),
    ];
    for (phase, missing) in withheld {
        assert_eq!(
            parsed["phases"][phase]["score"],
            serde_json::Value::Null,
            "{phase} depends on unreported metrics — its score must be null (insufficient data), not zero-derived, got: {parsed}"
        );
        assert_eq!(
            parsed["phase_scores"][phase],
            serde_json::Value::Null,
            "phase_scores[{phase}] must be null too, got: {parsed}"
        );
        let gaps = parsed["phases"][phase]["gaps"]
            .as_array()
            .unwrap_or_else(|| panic!("{phase} must carry gaps, got: {parsed}"))
            .iter()
            .filter_map(|g| g.as_str())
            .collect::<String>();
        assert!(
            gaps.contains("insufficient data"),
            "{phase} gap must say insufficient data, got: {gaps}"
        );
        for metric in missing {
            assert!(
                gaps.contains(metric),
                "{phase} gap must name {metric}, got: {gaps}"
            );
        }
    }

    // The overall score averages only the reported phases (preparation
    // from the framing document, project_assessment from learning events
    // + calibration) — never the zero-derived average of withheld phases.
    assert!(
        parsed["overall_score"].is_number(),
        "overall must average only the reported phases, got: {parsed}"
    );
}

// ── Slice 5 posterior-tool pins (PR-08) ──────────

#[tokio::test]
async fn scenario_recompute_posteriors_matches_finite_bayes() {
    let server = make_server();
    let root = independent_event("evt-a", "root event", 0.5);
    let mut child = independent_event("evt-b", "child event", 0.5);
    child.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-a".to_owned()],
        conditionals: vec![0.2, 0.8],
    }];
    for (observed, expected) in [(Some(true), 0.8), (Some(false), 0.2), (None, 0.5)] {
        let evidence = observed
            .map(|occurred| PosteriorEvidenceEntry {
                event_id: "evt-b".to_owned(),
                occurred: Some(occurred),
                observed_probability: None,
                likelihood_ratio: None,
            })
            .into_iter()
            .collect();
        let output = server
            .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
                events: vec![root.clone(), child.clone()],
                evidence,
            }))
            .await
            .expect("valid finite Bayesian model");
        let parsed = parse(&output);
        let a = parsed["nodes"]
            .as_array()
            .expect("node array")
            .iter()
            .find(|node| node["id"] == "evt-a")
            .expect("root present");
        let actual = a["posterior_probability"].as_f64().expect("numeric root");
        assert!(
            (actual - expected).abs() < 1e-12,
            "observed {observed:?}, got {parsed}"
        );
        assert_eq!(parsed["method"], "polytree_backward_inference");
        assert!(parsed["note"].is_null());
    }
}

#[tokio::test]
async fn scenario_recompute_posteriors_surfaces_impossible_evidence() {
    let server = make_server();
    let error = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events: vec![independent_event("evt-a", "impossible event", 0.0)],
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "evt-a".to_owned(),
                occurred: Some(true),
                observed_probability: None,
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect_err("zero-support observations cannot become probabilities");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "got {error:?}"
    );
    assert!(error.to_string().contains("evidence"), "got {error}");
}

/// PR-08 parity: the tool reproduces the shared engine's backward
/// inference with an independent numeric oracle. For a 0.5 → b [0.1, 0.6]
/// → c [0.2, 0.7], fixing P(c)=0.9 yields P(a)=0.64 and P(b)=0.6048.
/// One engine, two surfaces; matching direction alone is insufficient.
#[tokio::test]
async fn scenario_recompute_posteriors_updates_ancestors_on_leaf_evidence() {
    let server = make_server();
    let root = independent_event("evt-a", "root event", 0.5);
    let mut mid = independent_event("evt-b", "middle event", 0.0);
    mid.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-a".to_string()],
        conditionals: vec![0.1, 0.6],
    }];
    let mut leaf = independent_event("evt-c", "leaf event", 0.0);
    leaf.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-b".to_string()],
        conditionals: vec![0.2, 0.7],
    }];
    let output = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events: vec![root, mid, leaf],
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "evt-c".to_string(),
                observed_probability: Some(0.9),
                occurred: None,
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(
        parsed["method"].as_str(),
        Some("polytree_backward_inference"),
        "a chain is a polytree — backward inference runs, got: {parsed}"
    );
    let node = |id: &str| {
        parsed["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("node {id} present, got: {parsed}"))
    };
    for (id, expected) in [("evt-a", 0.64), ("evt-b", 0.6048), ("evt-c", 0.9)] {
        let actual = node(id)["posterior_probability"]
            .as_f64()
            .expect("numeric posterior");
        assert!((actual - expected).abs() < 1e-12, "node {id}: got {parsed}");
    }
}

/// PR-08: a multiply-connected DAG (diamond) degrades to forward-only
/// marginalization with a note naming the limitation — never silently.
#[tokio::test]
async fn scenario_recompute_posteriors_degrades_on_non_polytree_with_note() {
    let server = make_server();
    let a = independent_event("evt-a", "root event", 0.5);
    let mut b = independent_event("evt-b", "left event", 0.0);
    b.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-a".to_string()],
        conditionals: vec![0.1, 0.6],
    }];
    let mut c = independent_event("evt-c", "right event", 0.0);
    c.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-a".to_string()],
        conditionals: vec![0.2, 0.7],
    }];
    let mut d = independent_event("evt-d", "join event", 0.0);
    d.depends_on = vec![
        EventDependency {
            parent_event_ids: vec!["evt-b".to_string()],
            conditionals: vec![0.1, 0.6],
        },
        EventDependency {
            parent_event_ids: vec!["evt-c".to_string()],
            conditionals: vec![0.2, 0.7],
        },
    ];
    let output = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events: vec![a, b, c, d],
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "evt-d".to_string(),
                occurred: Some(true),
                observed_probability: None,
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(
        parsed["method"].as_str(),
        Some("forward_marginalization_only"),
        "a diamond is not a polytree — forward only, got: {parsed}"
    );
    assert_eq!(parsed["polytree"].as_bool(), Some(false));
    assert!(
        parsed["note"]
            .as_str()
            .is_some_and(|n| n.contains("multiply-connected")),
        "the degradation is surfaced with a note, got: {parsed}"
    );
}

/// PR-08: evidence validation — exactly one of the three fields, and the
/// event must be among the request's events. Both/neither and unknown ids
/// are invalid_argument naming the event.
#[tokio::test]
async fn scenario_recompute_posteriors_rejects_malformed_evidence() {
    let server = make_server();
    let events = vec![independent_event("evt-a", "root event", 0.5)];

    // Both observed_probability and occurred set.
    let error = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events: events.clone(),
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "evt-a".to_string(),
                observed_probability: Some(0.9),
                occurred: Some(true),
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect_err("two fields set must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "exactly-one-of-three is closed-set validation, got: {error:?}"
    );

    // Unknown event id.
    let error = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events: events.clone(),
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "ghost".to_string(),
                observed_probability: Some(0.9),
                occurred: None,
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect_err("an unknown evidence event must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "an evidence id outside the request's events is a caller defect, got: {error:?}"
    );
    assert!(
        error.to_string().contains("ghost"),
        "the error names the unknown event, got: {error}"
    );

    // Out-of-range observed probability.
    let error = server
        .scenario_recompute_posteriors(Parameters(PosteriorsRequest {
            events,
            evidence: vec![PosteriorEvidenceEntry {
                event_id: "evt-a".to_string(),
                observed_probability: Some(1.5),
                occurred: None,
                likelihood_ratio: None,
            }],
        }))
        .await
        .expect_err("an out-of-range probability must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "observed_probability must be in [0,1], got: {error:?}"
    );
}

// ── Slice 4 marginal-scoring pin (PR-02) ─────────

/// PR-02: the scored belief is the tree's resolved MARGINAL, not the
/// caller-supplied prior. A dependent event (prior 0.5, conditionals
/// [0.2, 0.8] under a 0.6 parent → marginal 0.56) scores 0.56, and the
/// journal record carries the marginal with the v3 marker.
#[tokio::test]
async fn scenario_score_scores_the_tree_marginal_not_the_prior() {
    let directory = tempfile::tempdir().expect("tempdir");
    let snapshot_path = directory.path().join("forecasts.json");
    let server = file_backed_server(&snapshot_path);

    let root = independent_event("evt-root", "root event", 0.6);
    let mut dependent = independent_event("evt-dep", "dependent event", 0.5);
    dependent.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-root".to_string()],
        conditionals: vec![0.2, 0.8],
    }];
    declare(
        &server,
        "marginal-scoring",
        vec![root.clone(), dependent.clone()],
    )
    .await;
    // marginal(evt-dep) = 0.4·0.2 + 0.6·0.8 = 0.56 ≠ prior 0.5
    let output = server
        .scenario_score(Parameters(ScoreRequest {
            resolver: "test-source".into(),
            forecast_id: "marginal-scoring".to_string(),
            events: vec![root, dependent],
            outcomes: vec![OutcomeEntry {
                event_id: "evt-dep".to_string(),
                occurred: true,
            }],
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);

    // The response scores the marginal: (0.56 − 1)² = 0.1936, not the
    // prior's (0.5 − 1)² = 0.25.
    let scored = &parsed["per_event"][0];
    let prob = scored["forecast_probability"]
        .as_f64()
        .unwrap_or_else(|| panic!("per_event carries forecast_probability, got: {parsed}"));
    assert!(
        (prob - 0.56).abs() < 1e-9,
        "the scored probability is the tree marginal 0.56, not the prior 0.5, got {prob}"
    );
    let brier = scored["brier_score"].as_f64().expect("brier present");
    assert!(
        (brier - 0.1936).abs() < 1e-9,
        "Brier is (0.56 − 1)² = 0.1936, not the prior-based 0.25, got {brier}"
    );
    assert_eq!(
        parsed["scored_quantity"].as_str(),
        Some("frozen_tree_marginal"),
        "the output names what was scored, got: {parsed}"
    );

    // The durable record carries the marginal with the v3 marker — the
    // calibration loop learns from the belief actually forecast. (The
    // handler's closing persist() compacts the journal into the snapshot,
    // so the post-call durable state IS the snapshot.)
    let snapshot: HashMap<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&snapshot_path).expect("snapshot readable"))
            .expect("snapshot parses");
    let resolved = snapshot
        .get("marginal-scoring:evt-dep")
        .unwrap_or_else(|| panic!("evt-dep must be recorded, got: {snapshot:?}"));
    assert_eq!(
        resolved["outcome"].as_bool(),
        Some(true),
        "the resolved entry carries the outcome, got: {resolved}"
    );
    let recorded = resolved["probability"]
        .as_f64()
        .expect("probability present");
    assert!(
        (recorded - 0.56).abs() < 1e-9,
        "the journal records the marginal 0.56, got {recorded}"
    );
    assert_eq!(
        resolved["scored_from_marginal"].as_bool(),
        Some(true),
        "the v3 marker distinguishes marginal records from prior-era records, got: {resolved}"
    );
    assert_eq!(
        resolved["schema_version"].as_u64(),
        Some(5),
        "schema v5, got: {resolved}"
    );
}

// ── Slice 3 project-record pins (PR-01/PR-07) ──────

/// PR-01: the project record is the assessment's anchor — an unknown
/// project id is `not_found` naming it, never a phantom assessment.
#[tokio::test]
async fn scenario_assess_unknown_project_is_not_found() {
    let server = make_server();
    let error = server
        .scenario_assess(Parameters(assess_request_all_omitted("ghost-project")))
        .await
        .expect_err("an unknown project must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::NotFound),
        "an unknown project id is a missing caller-named resource, not an internal error, got: {error:?}"
    );
    assert!(
        error.to_string().contains("ghost-project"),
        "the error must name the missing project, got: {error}"
    );
}

/// PR-01: the project record derives what the caller used to recall —
/// event and dependency counts from the stored tree, Preparation from the
/// framing document. With every caller metric omitted, Development is
/// still scored (derived) and Preparation scores from the framing.
#[tokio::test]
async fn scenario_assess_derives_counts_and_preparation_from_the_project() {
    let server = make_server();
    // A complete framing document: focal question, both-side scope, two
    // stakeholders, an action deadline — Preparation's four components.
    server
        .scenario_frame_document(Parameters(FrameDocumentRequest {
            subject: "ACME".to_string(),
            project_id: None,
            answers: serde_json::json!({
                "focal_question": "What should we watch at ACME over the next year?",
                "in_scope": ["product launches", "margin trajectory"],
                "out_of_scope": ["macro policy"],
                "stakeholders": [
                    {"role": "portfolio manager", "primary_concern": "drawdown"},
                    {"role": "analyst", "primary_concern": "thesis drift"}
                ],
                "action_deadline": "2027-01-31"
            })
            .into(),
        }))
        .await
        .expect("frame_document ok");
    // A two-event tree with one dependency edge: derived counts 2 and 1.
    let root = independent_event("evt-root", "root event", 0.6);
    let mut dependent = independent_event("evt-dep", "dependent event", 0.5);
    dependent.depends_on = vec![EventDependency {
        parent_event_ids: vec!["evt-root".to_string()],
        conditionals: vec![0.2, 0.8],
    }];
    server
        .scenario_quantify(Parameters(QuantifyRequest {
            events: vec![root, dependent],
        }))
        .await
        .expect("quantify ok");
    let output = server
        .scenario_assess(Parameters(assess_request_all_omitted("ACME")))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    // Preparation: scored from the framing document — all four components
    // present, so the full 1.0.
    assert_eq!(
        parsed["phases"]["preparation"]["score"].as_f64(),
        Some(1.0),
        "a complete framing document scores Preparation at 1.0, got: {parsed}"
    );
    // Development: derived from the stored tree — 2 events, 1 with
    // dependencies — so scored, not withheld.
    assert!(
        parsed["phases"]["development"]["score"].is_number(),
        "development derives its counts from the project's tree — reported, not withheld, got: {parsed}"
    );
    // Exploration still withholds: disagreement_score is neither
    // supplied nor derivable.
    assert_eq!(
        parsed["phases"]["exploration"]["score"],
        serde_json::Value::Null,
        "exploration needs disagreement_score — neither supplied nor derivable, got: {parsed}"
    );
}

/// PR-07: the durable tree cache — the last-quantified tree persists with
/// the project record; `ProjectStore::last_tree()` is the documented default
/// a restarted server reads when its memory cache is empty.
#[tokio::test]
async fn quantified_tree_survives_a_server_restart() {
    let directory = tempfile::tempdir().expect("tempdir");
    let projects_path = directory.path().join("projects.json");
    let events = vec![
        independent_event("evt-a", "event a", 0.6),
        independent_event("evt-b", "event b", 0.5),
    ];
    {
        let server = project_backed_server(&projects_path);
        server
            .scenario_quantify(Parameters(QuantifyRequest { events }))
            .await
            .expect("quantify ok");
    }
    // A fresh store on the same path: the persisted tree is the documented
    // default a restarted server reads.
    let restarted_store = ProjectStore::new(Some(projects_path));
    let tree = restarted_store
        .last_tree()
        .expect("the quantified tree persists with the project record");
    assert!(
        (tree.joint_probability - 0.30).abs() < 1e-9,
        "the persisted tree's joint (0.6 × 0.5 = 0.30) survives the restart, got {}",
        tree.joint_probability
    );
}

// Approved forecast cutover: public tools must not manufacture a declaration
// from resolution-time input, and replay cannot change the proposition.
#[tokio::test]
async fn scenario_resolution_requires_frozen_identity_and_unique_outcomes() {
    let server = make_server();
    let event = independent_event("event", "durable event", 0.8);
    let mut req = persistence_score_request(0.8, true);
    assert!(
        server
            .scenario_score(Parameters(req))
            .await
            .expect_err("no declaration")
            .to_string()
            .contains("not declared")
    );
    declare(&server, "persistence-regression", vec![event]).await;
    req = persistence_score_request(0.2, true);
    assert!(
        server
            .scenario_score(Parameters(req))
            .await
            .expect_err("p is frozen")
            .to_string()
            .contains("differs")
    );
    req = persistence_score_request(0.8, true);
    req.resolver = "different-source".into();
    assert!(server.scenario_score(Parameters(req)).await.is_err());
    req = persistence_score_request(0.8, true);
    req.events[0].question = "A different proposition".into();
    assert!(server.scenario_score(Parameters(req)).await.is_err());
    req = persistence_score_request(0.8, true);
    req.outcomes.push(OutcomeEntry {
        event_id: "event".into(),
        occurred: true,
    });
    assert!(
        server
            .scenario_score(Parameters(req))
            .await
            .expect_err("duplicate same outcome")
            .to_string()
            .contains("duplicate")
    );
    let first = parse(
        &server
            .scenario_score(Parameters(persistence_score_request(0.8, true)))
            .await
            .expect("resolve"),
    );
    assert!((first["aggregate"]["brier_score"].as_f64().expect("score") - 0.04).abs() < 1e-12);
    assert!(first.get("calibration_note").is_none());
    assert!(first["aggregate"].get("interpretation").is_none());
    server
        .scenario_score(Parameters(persistence_score_request(0.8, true)))
        .await
        .expect("same replay");
    assert!(
        server
            .scenario_score(Parameters(persistence_score_request(0.8, false)))
            .await
            .expect_err("conflict")
            .to_string()
            .contains("conflicting")
    );
    let status = parse(
        &server
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("status"),
    );
    assert_eq!(
        status["pipeline"]["resolved_count"], 1,
        "replays do not add event weight"
    );
}

#[tokio::test]
async fn scenario_declaration_rejects_late_scaffolds_and_early_outcomes() {
    let server = make_server();
    let mut event = independent_event("evt", "future", 0.5);
    event.deadline = before_cutoff().date_naive() - chrono::Duration::days(1);
    assert!(
        server
            .scenario_declare(Parameters(DeclareRequest {
                forecast_id: "late".into(),
                resolution_at: cutoffs(&[event.clone()]),
                events: vec![event.clone()],
                resolver: "source".into()
            }))
            .await
            .is_err()
    );
    event.deadline = before_cutoff().date_naive() + chrono::Duration::days(1);
    event.basis = Some("prediction_market:kalshi".into());
    assert!(
        server
            .scenario_declare(Parameters(DeclareRequest {
                forecast_id: "scaffold".into(),
                resolution_at: cutoffs(&[event.clone()]),
                events: vec![event.clone()],
                resolver: "source".into()
            }))
            .await
            .is_err()
    );
    event.basis = None;
    declare(&server, "future", vec![event.clone()]).await;
    assert!(
        server
            .scenario_score(Parameters(ScoreRequest {
                forecast_id: "future".into(),
                events: vec![event],
                resolver: "test-source".into(),
                outcomes: vec![OutcomeEntry {
                    event_id: "evt".into(),
                    occurred: true
                }]
            }))
            .await
            .expect_err("early outcome")
            .to_string()
            .contains("deadline")
    );
}

#[tokio::test]
async fn unmatched_outcomes_are_explicit_and_empty_bins_are_insufficient() {
    use hkask_mcp_scenarios::requests::CalibrationRequest;
    let server = make_server();
    declare(
        &server,
        "persistence-regression",
        vec![independent_event("event", "durable event", 0.8)],
    )
    .await;
    let mut req = persistence_score_request(0.8, true);
    req.outcomes.push(OutcomeEntry {
        event_id: "unrelated".into(),
        occurred: false,
    });
    let scored = parse(
        &server
            .scenario_score(Parameters(req))
            .await
            .expect("matching and unmatched"),
    );
    assert_eq!(
        scored["unmatched_outcome_ids"],
        serde_json::json!(["unrelated"])
    );
    assert_eq!(scored["scored_count"], 1);
    let curve = parse(
        &server
            .scenario_calibration(Parameters(CalibrationRequest { subject: None }))
            .await
            .expect("curve"),
    );
    assert_eq!(
        curve["evidence_status"], "insufficient",
        "no bin has five observations"
    );
    assert!(curve["signed_probability_error"].is_null());
    let bins = curve["bins"].as_array().expect("bins");
    let empty = bins.iter().find(|b| b["count"] == 0).expect("empty bin");
    assert_eq!(empty["evidence_status"], "insufficient");
    for field in ["hit_rate", "expected_rate", "signed_probability_error"] {
        assert!(empty[field].is_null(), "empty {field}: {empty}");
    }
    let occupied = bins.iter().find(|b| b["count"] == 1).expect("occupied bin");
    assert_eq!(occupied["hit_rate"], 1.0);
    assert_eq!(occupied["expected_rate"], 0.8);
}

#[tokio::test]
async fn synthesis_equal_weights_do_not_invent_or_reward_historical_loss() {
    use hkask_mcp_scenarios::requests::SynthesizeRequest;
    let req: SynthesizeRequest = serde_json::from_value(serde_json::json!({
        "event_id":"event", "perspectives":[
            {"source":"a", "probability":0.2, "fermi_sub_questions":[], "historical_brier":0.001},
            {"source":"b", "probability":0.8, "fermi_sub_questions":[]}
        ]
    }))
    .expect("request");
    let body = parse(
        &make_server()
            .scenario_synthesize(Parameters(req))
            .await
            .expect("synthesis"),
    );
    assert_eq!(body["aggregated_probability"], 0.5);
    assert!(
        body["perspective_weights"]
            .as_array()
            .expect("weights")
            .iter()
            .all(|w| w["weight"] == 0.5)
    );
    assert!(body["individual_perspectives"][1]["historical_brier"].is_null());
    assert!(
        body["methodology"]["weighting"]
            .as_str()
            .expect("policy")
            .starts_with("equal")
    );
}

#[tokio::test]
async fn signed_cancellation_is_descriptive_not_calibration_certification() {
    use hkask_mcp_scenarios::requests::CalibrationRequest;
    let server = make_server();
    let mut events = Vec::new();
    let mut outcomes = Vec::new();
    for i in 0..10 {
        let id = format!("evt-{i}");
        events.push(independent_event(
            &id,
            "cancel",
            if i < 5 { 0.2 } else { 0.8 },
        ));
        outcomes.push(OutcomeEntry {
            event_id: id,
            occurred: i < 5,
        });
    }
    declare(&server, "cancellation", events.clone()).await;
    server
        .scenario_score(Parameters(ScoreRequest {
            forecast_id: "cancellation".into(),
            events,
            outcomes,
            resolver: "test-source".into(),
        }))
        .await
        .expect("resolve");
    let curve = parse(
        &server
            .scenario_calibration(Parameters(CalibrationRequest { subject: None }))
            .await
            .expect("curve"),
    );
    assert!(
        curve["signed_probability_error"]
            .as_f64()
            .expect("signed bias")
            .abs()
            < 1e-12
    );
    assert_eq!(curve["evidence_status"], "descriptive_only");
    assert!(curve.get("interpretation").is_none());
    assert!((curve["overall_brier"].as_f64().expect("raw loss") - 0.64).abs() < 1e-12);
}

#[tokio::test]
async fn legacy_scenario_history_survives_without_becoming_score_eligible() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("history.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({
        "legacy:event": {"schema_version":3,"forecast_id":"legacy","event_id":"event", "event_name":"durable event", "subject":"ACME", "probability":0.8, "scored_from_marginal":true, "created_at":"2020-01-01", "outcome":null,"resolved_at":null,"category":"company_analysis"}
    })).expect("fixture json")).expect("historical fixture");
    let server = file_backed_server(&path);
    let mut request = persistence_score_request(0.8, true);
    request.forecast_id = "legacy".into();
    let body = parse(
        &server
            .scenario_score(Parameters(request))
            .await
            .expect("record legacy outcome"),
    );
    assert_eq!(body["scored_count"], 0);
    assert!(body["aggregate"]["brier_score"].is_null());
    assert_eq!(body["per_event"][0]["scoring_status"], "unscored");
    let reopened = file_backed_server(&path);
    let status = parse(
        &reopened
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("history after restart"),
    );
    assert_eq!(status["pipeline"]["forecast_count"], 1);
    assert_eq!(status["pipeline"]["resolved_count"], 1);
    assert!(status["pipeline"]["overall_brier"].is_null());
    assert_eq!(
        status["pipeline"]["recent_forecasts"][0]["probability"],
        0.8
    );
    assert_eq!(
        status["pipeline"]["recent_forecasts"][0]["scoring_eligible"],
        false
    );
}

/// Review e6f64bcc: a calendar date cannot establish that declaration
/// preceded the same-day outcome; no exact cutoff means no eligible intake.
#[tokio::test]
async fn date_only_same_day_declaration_cannot_be_admitted() {
    let server = make_server();
    let request = serde_json::from_value::<DeclareRequest>(serde_json::json!({
        "forecast_id":"date-only", "events":[independent_event("event", "observed", 0.99)],
        "resolver":"test-source"
    }));
    assert!(
        request
            .expect_err("precise cutoff is required")
            .to_string()
            .contains("resolution_at")
    );
    let status = parse(
        &server
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("status"),
    );
    assert_eq!(status["pipeline"]["forecast_count"], 0);
}

fn immediately_before_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00.999999999Z".parse().expect("clock")
}
fn immediately_after_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:01.000000001Z".parse().expect("clock")
}
fn declaration_request(id: &str, events: Vec<ScenarioEvent>) -> DeclareRequest {
    DeclareRequest {
        forecast_id: id.into(),
        resolution_at: cutoffs(&events),
        events,
        resolver: "test-source".into(),
    }
}

#[tokio::test]
async fn exact_cutoff_admits_short_horizon_but_not_hindsight_or_early_resolution() {
    let event = independent_event("event", "durable event", 0.8);
    for clock in [at_cutoff as fn() -> _, immediately_after_cutoff] {
        let server = make_server();
        set_clock(&server, clock);
        assert!(
            server
                .scenario_declare(Parameters(declaration_request("late", vec![event.clone()])))
                .await
                .is_err()
        );
        let status = parse(
            &server
                .scenario_status(Parameters(StatusRequest {}))
                .await
                .expect("status"),
        );
        assert_eq!(status["pipeline"]["forecast_count"], 0);
    }
    let server = make_server();
    set_clock(&server, immediately_before_cutoff);
    server
        .scenario_declare(Parameters(declaration_request(
            "persistence-regression",
            vec![event.clone()],
        )))
        .await
        .expect("one nanosecond is a legitimate prospective horizon");
    assert!(
        server
            .scenario_score(Parameters(persistence_score_request(0.8, true)))
            .await
            .is_err()
    );
    set_clock(&server, at_cutoff);
    let body = parse(
        &server
            .scenario_score(Parameters(persistence_score_request(0.8, true)))
            .await
            .expect("resolution at exact cutoff"),
    );
    assert!((body["aggregate"]["brier_score"].as_f64().expect("score") - 0.04).abs() < 1e-12);
    set_clock(&server, immediately_after_cutoff);
    server
        .scenario_score(Parameters(persistence_score_request(0.8, true)))
        .await
        .expect("later replay");
    // A corrupt/hindsight history row cannot bypass the declaration-time check.
    {
        let mut store = server.forecast_store.lock().expect("store");
        let mut record = store.get(REGRESSION_KEY).expect("record").clone();
        record
            .declaration
            .as_mut()
            .expect("declaration")
            .declared_at = at_cutoff().to_rfc3339();
        store
            .insert(REGRESSION_KEY.into(), record)
            .expect("history fixture");
    }
    assert!(
        server
            .scenario_score(Parameters(persistence_score_request(0.8, true)))
            .await
            .expect_err("declared_at must be checked")
            .to_string()
            .contains("declared_at")
    );
}

#[tokio::test]
async fn compaction_fault_restarts_and_retries_the_complete_101_event_declaration() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("forecasts.json");
    let events: Vec<_> = (0..101)
        .map(|i| {
            independent_event(
                &format!("event-{i}"),
                "batch",
                if i == 100 { 0.3 } else { 0.8 },
            )
        })
        .collect();
    let server = file_backed_server(&path);
    set_clock(&server, before_cutoff);
    // This hits the real >=100 automatic compaction path, not a mocked port.
    std::fs::create_dir(&path).expect("block snapshot publication");
    assert!(
        server
            .scenario_declare(Parameters(declaration_request("complete", events.clone())))
            .await
            .is_err()
    );
    let status = parse(
        &server
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("read after fault"),
    );
    assert_eq!(
        status["pipeline"]["forecast_count"], 101,
        "the durable declaration must never expose a partial membership"
    );
    assert_eq!(status["pipeline"]["pending_count"], 101);
    let journal =
        std::fs::read_to_string(path.with_extension("json.journal")).expect("durable journal");
    assert_eq!(
        journal.lines().count(),
        1,
        "membership must be one atomic journal event, not 101 individually durable rows"
    );
    let event: serde_json::Value =
        serde_json::from_str(journal.trim()).expect("complete journal event");
    assert_eq!(
        event["records"].as_array().expect("batch membership").len(),
        101
    );
    std::fs::remove_dir(&path).expect("clear snapshot fault");
    drop(server);
    // Retry after the cutoff must retain the original admission timestamp,
    // not reject an already admitted forecast or declare it retrospectively.
    let restarted = file_backed_server(&path);
    let status = parse(
        &restarted
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("replayed journal"),
    );
    assert_eq!(status["pipeline"]["forecast_count"], 101);
    let retried = parse(
        &restarted
            .scenario_declare(Parameters(declaration_request("complete", events.clone())))
            .await
            .expect("identical retry completes publication"),
    );
    assert_eq!(retried["replayed"], true);
    let mut changed = events.clone();
    changed[100].probability = 0.99;
    assert!(
        restarted
            .scenario_declare(Parameters(declaration_request("complete", changed)))
            .await
            .is_err()
    );
    let mut missing = events.clone();
    missing.pop();
    assert!(
        restarted
            .scenario_declare(Parameters(declaration_request("complete", missing)))
            .await
            .is_err()
    );
    let snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("snapshot published"))
            .expect("snapshot");
    assert_eq!(snapshot.as_object().expect("members").len(), 101);
    for record in snapshot.as_object().expect("records").values() {
        let declared: chrono::DateTime<chrono::Utc> = record["declaration"]["declared_at"]
            .as_str()
            .expect("timestamp")
            .parse()
            .expect("RFC3339");
        assert_eq!(
            declared,
            before_cutoff(),
            "retry must not re-time the forecast"
        );
    }
    let outcomes = events
        .iter()
        .map(|event| OutcomeEntry {
            event_id: event.id.clone(),
            occurred: false,
        })
        .collect();
    let scored = parse(
        &restarted
            .scenario_score(Parameters(ScoreRequest {
                forecast_id: "complete".into(),
                events: events.clone(),
                resolver: "test-source".into(),
                outcomes,
            }))
            .await
            .expect("score original complete set"),
    );
    assert_eq!(scored["scored_count"], 101);
    let rows = scored["per_event"].as_array().expect("per event");
    assert_eq!(rows.len(), 101);
    for event in &events {
        let row = rows
            .iter()
            .find(|r| r["event_id"] == event.id)
            .expect("every original ID");
        assert_eq!(
            row["forecast_probability"], event.probability,
            "original p is frozen"
        );
    }
    assert!(
        (scored["aggregate"]["brier_score"].as_f64().expect("score") - 64.09 / 101.0).abs() < 1e-12
    );
    let reopened = file_backed_server(&path);
    let status = parse(
        &reopened
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("resolved restart"),
    );
    assert_eq!(status["pipeline"]["forecast_count"], 101);
    assert_eq!(status["pipeline"]["eligible_resolved_count"], 101);
}

#[tokio::test]
async fn complementary_probabilities_report_signed_error_not_overconfidence() {
    use hkask_mcp_scenarios::requests::CalibrationRequest;
    let server = make_server();
    let events: Vec<_> = (0..10)
        .map(|i| {
            independent_event(
                &format!("complement-{i}"),
                "complement",
                if i < 5 { 0.1 } else { 0.9 },
            )
        })
        .collect();
    let outcomes = events
        .iter()
        .enumerate()
        .map(|(i, e)| OutcomeEntry {
            event_id: e.id.clone(),
            occurred: i >= 5,
        })
        .collect();
    declare(&server, "complements", events.clone()).await;
    server
        .scenario_score(Parameters(ScoreRequest {
            forecast_id: "complements".into(),
            events,
            resolver: "test-source".into(),
            outcomes,
        }))
        .await
        .expect("resolve");
    let body = parse(
        &server
            .scenario_calibration(Parameters(CalibrationRequest { subject: None }))
            .await
            .expect("curve"),
    );
    assert!(
        body["signed_probability_error"]
            .as_f64()
            .expect("named signed error")
            .abs()
            < 1e-12
    );
    assert_eq!(body["signed_probability_error_sample_count"], 10);
    assert_eq!(
        body["signed_probability_error_qualifying_bins"]
            .as_array()
            .expect("qualifying bins")
            .len(),
        2
    );
    assert!(body.get("overconfidence_score").is_none());
    let bins = body["bins"].as_array().expect("bins");
    for (probability, expected_error) in [(0.1, 0.1), (0.9, -0.1)] {
        let bin = bins
            .iter()
            .find(|b| b["expected_rate"] == probability)
            .expect("probability bin");
        assert!(
            (bin["signed_probability_error"]
                .as_f64()
                .expect("bin signed error")
                - expected_error)
                .abs()
                < 1e-12
        );
        assert!(bin.get("bias").is_none());
    }
    let status = parse(
        &server
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("status"),
    );
    assert!(status["calibration"].get("overconfidence_score").is_none());
    assert_eq!(
        status["calibration"]["signed_probability_error_sample_count"],
        10
    );
}

#[tokio::test]
async fn exact_cutoff_map_must_match_membership_and_retries_cannot_change_it() {
    let server = make_server();
    set_clock(&server, before_cutoff);
    let event = independent_event("event", "durable event", 0.8);
    let mut request = declaration_request("persistence-regression", vec![event.clone()]);
    request.resolution_at.clear();
    assert!(server.scenario_declare(Parameters(request)).await.is_err());
    let mut request = declaration_request("persistence-regression", vec![event.clone()]);
    request.resolution_at.insert("extra".into(), at_cutoff());
    assert!(server.scenario_declare(Parameters(request)).await.is_err());
    server
        .scenario_declare(Parameters(declaration_request(
            "persistence-regression",
            vec![event.clone()],
        )))
        .await
        .expect("precise declaration");
    let mut request = declaration_request("persistence-regression", vec![event.clone()]);
    request
        .resolution_at
        .insert("event".into(), immediately_after_cutoff());
    assert!(server.scenario_declare(Parameters(request)).await.is_err());
    let mut request = declaration_request("persistence-regression", vec![event.clone()]);
    request.resolver = "another-source".into();
    assert!(server.scenario_declare(Parameters(request)).await.is_err());
    set_clock(&server, immediately_after_cutoff);
    let replay = parse(
        &server
            .scenario_declare(Parameters(declaration_request(
                "persistence-regression",
                vec![event],
            )))
            .await
            .expect("identical normal retry after cutoff"),
    );
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["event_count"], 1);
}

#[tokio::test]
async fn date_only_v4_declaration_is_preserved_but_never_reclassified_as_prospective() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("forecasts.json");
    let event = independent_event("event", "durable event", 0.8);
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({
        "date-only:event": {"schema_version":4,"forecast_id":"date-only","event_id":"event", "event_name":"durable event", "subject":"ACME", "probability":0.8, "scored_from_marginal":true,"created_at":"2026-10-20","outcome":null,"resolved_at":null,"declaration":{"event":event,"resolver":"test-source","declared_at":"2026-10-20T12:00:00Z"}}
    })).expect("fixture")).expect("historical snapshot");
    let server = file_backed_server(&path);
    assert!(
        server
            .scenario_declare(Parameters(declaration_request(
                "date-only",
                vec![event.clone()]
            )))
            .await
            .is_err(),
        "no retrospective cutoff can be attached to history"
    );
    let mut request = persistence_score_request(0.8, true);
    request.forecast_id = "date-only".into();
    let scored = parse(
        &server
            .scenario_score(Parameters(request))
            .await
            .expect("record unscored history"),
    );
    assert_eq!(scored["scored_count"], 0);
    assert_eq!(scored["per_event"][0]["scoring_status"], "unscored");
    let restarted = file_backed_server(&path);
    let status = parse(
        &restarted
            .scenario_status(Parameters(StatusRequest {}))
            .await
            .expect("historical restart"),
    );
    assert_eq!(status["pipeline"]["resolved_count"], 1);
    assert_eq!(status["pipeline"]["eligible_resolved_count"], 0);
    assert!(status["pipeline"]["overall_brier"].is_null());
    assert_eq!(
        status["pipeline"]["recent_forecasts"][0]["scoring_eligible"],
        false
    );
}
