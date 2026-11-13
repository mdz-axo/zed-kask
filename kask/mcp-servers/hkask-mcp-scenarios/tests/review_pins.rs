//! Pins from the 2026-11-12 mcp-tool-review pass (findings SC-08 onward).
//!
//! Every case builds its request from JSON and calls the tool handler, so
//! the request's deserialization (closed vocabularies, unknown arguments) is
//! part of what is pinned. Stores are in memory or in a temporary directory.

#![cfg(test)]

use hkask_mcp_scenarios::equity_forecast::StoredEquityForecast;
use hkask_mcp_scenarios::{ForecastStore, JournalStore, ProjectStore, ScenariosServer};
use hkask_mcp_test_support::parse_envelope;
use hkask_types::McpErrorKind::{FailedPrecondition, InvalidArgument, NotFound};
use hkask_types::{McpErrorKind, WebID};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};

type Clock = fn() -> chrono::DateTime<chrono::Utc>;
/// `None` in the first slot: the arguments did not deserialize.
type Refusal = (Option<McpErrorKind>, String);

fn before_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00Z".parse().expect("clock")
}
fn a_little_later() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:00.500Z".parse().expect("clock")
}
fn after_cutoff() -> chrono::DateTime<chrono::Utc> {
    "2026-10-20T12:00:02Z".parse().expect("clock")
}
const CUTOFF: &str = "2026-10-20T12:00:01Z";

/// A server over the three store files in `dir` (in memory when `None`).
fn server_at(dir: Option<&Path>, clock: Clock) -> ScenariosServer {
    let file = |name: &str| dir.map(|d| d.join(name));
    let mut forecasts = ForecastStore::new(file("forecasts.json"));
    forecasts.set_clock(clock);
    ScenariosServer::new(
        WebID::new(),
        Arc::new(Mutex::new(forecasts)),
        Arc::new(Mutex::new(JournalStore::<StoredEquityForecast>::new(file(
            "equity_forecasts.json",
        )))),
        Mutex::new(ProjectStore::new(file("projects.json"))),
    )
}

fn server() -> ScenariosServer {
    server_at(None, before_cutoff)
}

/// Call a tool with JSON arguments, as the wire does.
macro_rules! wire {
    ($server:expr, $method:ident, $arguments:expr) => {{
        let outcome: Result<Value, Refusal> = match serde_json::from_value($arguments) {
            Err(error) => Err((None, error.to_string())),
            Ok(request) => $server
                .$method(Parameters(request))
                .await
                .map(|output| parse_envelope(&output))
                .map_err(|error| (Some(error.kind), error.message.clone())),
        };
        outcome
    }};
}

fn refused(outcome: Result<Value, Refusal>, kind: McpErrorKind, names: &str) {
    let (got, message) = outcome.expect_err("the call must be refused");
    assert_eq!(got, Some(kind), "{message}");
    assert!(message.contains(names), "must name '{names}': {message}");
}

fn event(id: &str, probability: f64, depends_on: Value) -> Value {
    json!({
        "id": id, "name": id, "question": format!("Will {id} happen?"),
        "deadline": "2026-10-20", "time_horizon": "tactical",
        "scenario_type": "emerging_economic", "subject": "pins",
        "probability": probability, "depends_on": depends_on,
        "sub_questions": [], "update_count": 0
    })
}

fn parent(ids: &[&str], conditionals: &[f64]) -> Value {
    json!([{ "parent_event_ids": ids, "conditionals": conditionals }])
}

fn declaration(forecast_id: &str, events: &[Value]) -> Value {
    let cutoffs: serde_json::Map<String, Value> = events
        .iter()
        .map(|e| (e["id"].as_str().expect("id").to_owned(), json!(CUTOFF)))
        .collect();
    json!({ "forecast_id": forecast_id, "events": events, "resolution_at": cutoffs, "resolver": "pins" })
}

// ── SC-08: arguments the schema does not name ───────────────────────────

#[tokio::test]
async fn an_argument_the_tool_does_not_name_is_refused() {
    let server = server();
    let (kind, message) = wire!(
        server,
        scenario_triage,
        json!({"question": "Will it?", "has_dedline": true})
    )
    .expect_err("a misspelled argument must not be dropped");
    assert_eq!(kind, None);
    assert!(message.contains("has_dedline"), "{message}");

    for tool in ScenariosServer::combined_router().list_all() {
        let schema = serde_json::to_value(&tool.input_schema).expect("schema");
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{} must declare its arguments closed",
            tool.name
        );
    }
}

// ── SC-09: closed vocabularies ─────────────────────────────────────────

#[tokio::test]
async fn a_value_outside_a_closed_vocabulary_is_refused_by_name() {
    let server = server();
    refused(
        wire!(
            server,
            scenario_brainstorm,
            json!({"subject": "pins", "time_horizon": "bogus"})
        ),
        InvalidArgument,
        "tactical, strategic, long_term",
    );
    refused(
        wire!(
            server,
            scenario_build,
            json!({"subject": "pins", "scenario_type": "long_term"})
        ),
        InvalidArgument,
        "company_update, company_analysis, emerging_economic, economic_potential",
    );
    refused(
        wire!(server, scenario_frame, json!({"subject": " "})),
        InvalidArgument,
        "subject",
    );

    let schemas: std::collections::BTreeMap<String, Value> = ScenariosServer::combined_router()
        .list_all()
        .into_iter()
        .map(|tool| {
            (
                tool.name.to_string(),
                serde_json::to_value(&tool.input_schema).expect("schema"),
            )
        })
        .collect();
    let horizons = json!(["tactical", "strategic", "long_term"]);
    for tool in ["scenario_brainstorm", "scenario_build"] {
        assert_eq!(
            schemas[tool]["properties"]["time_horizon"]["anyOf"][0]["enum"], horizons,
            "{tool} enumerates its horizons"
        );
    }
    assert_eq!(
        schemas["scenario_from_markets_set"]["properties"]["match_confidences"]["anyOf"][0]["items"]
            ["anyOf"][0]["enum"],
        json!(["high", "medium", "low"])
    );
}

#[tokio::test]
async fn the_event_template_scenario_build_returns_is_one_scenario_quantify_accepts() {
    let server = server();
    let built = wire!(server, scenario_build, json!({"subject": "pins"})).expect("build");
    let mut template = built["event_template"].clone();
    template["deadline"] = json!("2027-01-01");
    template["basis"] = json!("technical_feasibility");
    let tree = wire!(server, scenario_quantify, json!({"events": [template]}))
        .expect("the server's own template must deserialize as an event");
    assert_eq!(tree["event_count"], 1);
}

#[tokio::test]
async fn framing_answers_outside_their_closed_sets_are_refused() {
    let server = server();
    let framed = wire!(
        server,
        scenario_frame_document,
        json!({"subject": "pins", "answers": r#"{"time_horizon":"long_term","use_case":"investment_thesis","stakeholders":["board",{"role":"staff"}]}"#})
    )
    .expect("the spellings scenario_frame's template names");
    assert_eq!(framed["framing_document"]["time_horizon"], "long_term");
    assert_eq!(framed["framing_document"]["use_case"], "investment_thesis");
    // A stakeholder given as a bare string keeps its name as the role.
    assert_eq!(
        framed["framing_document"]["stakeholders"][0]["role"],
        "board"
    );
    assert_eq!(
        framed["framing_document"]["stakeholders"][1]["role"],
        "staff"
    );

    refused(
        wire!(
            server,
            scenario_frame_document,
            json!({"subject": "pins", "answers": r#"{"time_horizon":"bogus"}"#})
        ),
        InvalidArgument,
        "tactical, strategic, long_term",
    );
    refused(
        wire!(
            server,
            scenario_frame_document,
            json!({"subject": "pins", "answers": r#"{"use_case":"nonsense"}"#})
        ),
        InvalidArgument,
        "landscape_exploration",
    );
    refused(
        wire!(
            server,
            scenario_frame_document,
            json!({"subject": "pins", "answers": "[1,2]"})
        ),
        InvalidArgument,
        "JSON object",
    );
}

// ── SC-10: scenario_update is Bayes' theorem ───────────────────────────

#[tokio::test]
async fn scenario_update_is_exact_and_refuses_inputs_no_model_admits() {
    let server = server();
    let update = |prior: f64, likelihood: f64, base_rate: f64| {
        json!({"forecast_id": "f", "event_id": "e", "prior_probability": prior,
               "evidence_likelihood": likelihood, "evidence_base_rate": base_rate})
    };
    let posterior = |value: Value| value["posterior_probability"].as_f64().expect("posterior");

    let exact = wire!(server, scenario_update, update(0.3, 0.8, 0.5)).expect("coherent");
    assert!((posterior(exact) - 0.48).abs() < 1e-12);
    // Certainty and impossibility are not moved to 0.99 and 0.01.
    let impossible = wire!(server, scenario_update, update(0.0, 0.8, 0.5)).expect("prior 0");
    assert_eq!(posterior(impossible.clone()), 0.0);
    assert_eq!(impossible["direction"], "unchanged");
    let certain = wire!(server, scenario_update, update(1.0, 0.5, 0.5)).expect("prior 1");
    assert_eq!(posterior(certain.clone()), 1.0);
    assert_eq!(certain["direction"], "unchanged");
    // 0.9 × 0.9 / 0.5 = 1.62 is not a probability.
    refused(
        wire!(server, scenario_update, update(0.9, 0.9, 0.5)),
        InvalidArgument,
        "P(E and H) cannot exceed P(E)",
    );
}

// ── SC-11: numbers that are not probabilities ──────────────────────────

#[tokio::test]
async fn numbers_that_are_not_probabilities_are_refused() {
    let server = server();
    let one = json!([{"question": "q", "estimate": 0.7, "confidence": 1.0}]);
    refused(
        wire!(
            server,
            scenario_calibrate,
            json!({"question": "q", "sub_questions": one, "base_rate": 1.7, "reference_count": 10})
        ),
        InvalidArgument,
        "base_rate must be in [0, 1]",
    );
    refused(
        wire!(
            server,
            scenario_calibrate,
            json!({"question": "q", "sub_questions": one, "base_rate": 0.2})
        ),
        InvalidArgument,
        "go together",
    );
    refused(
        wire!(
            server,
            scenario_calibrate,
            json!({"question": "q", "sub_questions": [{"question": "q", "estimate": 0.9, "confidence": 0.0}]})
        ),
        InvalidArgument,
        "confidence 0",
    );
    let blended = wire!(
        server,
        scenario_calibrate,
        json!({"question": "q", "sub_questions": one, "base_rate": 0.2, "reference_count": 10})
    )
    .expect("base rate and count, no label");
    assert!(
        (blended["calibrated_probability"].as_f64().expect("p") - 0.336_363_636_363_636_3).abs()
            < 1e-12
    );

    let cross = |a: f64, b: f64, threshold: Option<f64>| {
        json!({"event_id": "e", "source_a": "a", "estimate_a": a, "sub_questions_a": [],
               "source_b": "b", "estimate_b": b, "sub_questions_b": [], "review_threshold": threshold})
    };
    refused(
        wire!(server, scenario_cross_validate, cross(1.7, 0.5, None)),
        InvalidArgument,
        "estimate_a",
    );
    refused(
        wire!(server, scenario_cross_validate, cross(0.6, 0.5, Some(-1.0))),
        InvalidArgument,
        "review_threshold",
    );

    let branches = json!([{"probability": 1.0, "branch_return": 0.1}]);
    refused(
        wire!(
            server,
            scenario_risk_measure,
            json!({"branches": branches, "realized_volatility": 0.3, "scenario_weight": 5.0})
        ),
        InvalidArgument,
        "scenario_weight",
    );

    wire!(
        server,
        scenario_frame_document,
        json!({"subject": "pins", "answers": "{}"})
    )
    .expect("project");
    refused(
        wire!(
            server,
            scenario_assess,
            json!({"project_id": "pins", "disagreement_score": 7.0})
        ),
        InvalidArgument,
        "disagreement_score",
    );
    refused(
        wire!(
            server,
            scenario_assess,
            json!({"project_id": "pins", "event_count": 2, "events_with_dependencies": 9})
        ),
        InvalidArgument,
        "cannot exceed event_count",
    );
    refused(
        wire!(
            server,
            scenario_assess,
            json!({"project_id": "pins", "strategies_generated": 1, "strategies_implemented": 5})
        ),
        InvalidArgument,
        "cannot exceed strategies_generated",
    );
}

// ── SC-12: tree structure ──────────────────────────────────────────────

#[tokio::test]
async fn event_ids_are_unique_and_a_group_names_at_most_twenty_parents() {
    let server = server();
    let a = event("a", 0.5, json!([]));
    refused(
        wire!(
            server,
            scenario_quantify,
            json!({"events": [a.clone(), a.clone()]})
        ),
        InvalidArgument,
        "event id 'a'",
    );
    refused(
        wire!(
            server,
            scenario_quantify,
            json!({"events": [event(" ", 0.5, json!([]))]})
        ),
        InvalidArgument,
        "blank",
    );
    let mut events: Vec<Value> = (0..21)
        .map(|i| event(&format!("p{i}"), 0.5, json!([])))
        .collect();
    let ids: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
    events.push(event(
        "child",
        0.5,
        json!([{"parent_event_ids": ids, "conditionals": [0.9]}]),
    ));
    refused(
        wire!(server, scenario_quantify, json!({"events": events})),
        InvalidArgument,
        "exceeds the limit of 20",
    );
}

#[tokio::test]
async fn parents_that_share_an_ancestor_are_named_as_an_approximation() {
    let server = server();
    let a = event("a", 0.5, json!([]));
    let b = event("b", 0.5, parent(&["a"], &[0.2, 0.8]));
    let chain = wire!(
        server,
        scenario_quantify,
        json!({"events": [a.clone(), b.clone()]})
    )
    .expect("chain");
    assert!(chain["approximation_note"].is_null(), "{chain}");
    assert!(
        chain["nodes"][0].get("paths_from_root").is_none(),
        "{chain}"
    );

    let c = event("c", 0.5, parent(&["a"], &[0.3, 0.7]));
    let d = event("d", 0.5, parent(&["b", "c"], &[0.1, 0.2, 0.3, 0.9]));
    let diamond =
        wire!(server, scenario_quantify, json!({"events": [a, b, c, d]})).expect("diamond");
    // Exact P(d) is 0.405; treating b and c as independent gives 0.375.
    let d_marginal = diamond["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "d")
        .expect("d")["marginal_probability"]
        .as_f64()
        .expect("marginal");
    assert!((d_marginal - 0.375).abs() < 1e-12);
    assert!(
        diamond["approximation_note"]
            .as_str()
            .is_some_and(|note| note.contains("share an ancestor")),
        "{diamond}"
    );
}

#[tokio::test]
async fn a_prior_on_an_event_with_parents_is_refused() {
    let server = server();
    let events = json!([
        event("a", 0.6, json!([])),
        event("b", 0.5, parent(&["a"], &[0.2, 0.9]))
    ]);
    refused(
        wire!(
            server,
            scenario_propagate,
            json!({"events": events, "event_id": "b", "new_prior": 0.9})
        ),
        InvalidArgument,
        "has parents",
    );
    let moved = wire!(
        server,
        scenario_propagate,
        json!({"events": events, "event_id": "a", "new_prior": 0.9})
    )
    .expect("a root prior moves");
    assert!((moved["joint_after"].as_f64().expect("joint") - 0.81).abs() < 1e-12);
}

#[tokio::test]
async fn two_evidence_entries_for_one_event_are_refused() {
    let server = server();
    let events = json!([
        event("a", 0.5, json!([])),
        event("b", 0.5, parent(&["a"], &[0.2, 0.8]))
    ]);
    refused(
        wire!(
            server,
            scenario_recompute_posteriors,
            json!({"events": events, "evidence": [
            {"event_id": "b", "occurred": true}, {"event_id": "b", "occurred": false}]})
        ),
        InvalidArgument,
        "more than one evidence entry",
    );
}

// ── SC-13: scenario_status ─────────────────────────────────────────────

#[tokio::test]
async fn status_shows_the_most_recently_stored_tree_after_a_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let mut first = event("x", 0.4, json!([]));
    first["subject"] = json!("first");
    let mut second = event("y", 0.4, json!([]));
    second["subject"] = json!("second");
    {
        let server = server_at(Some(dir.path()), before_cutoff);
        wire!(server, scenario_quantify, json!({"events": [first]})).expect("first");
        wire!(server, scenario_quantify, json!({"events": [second]})).expect("second");
    }
    let restarted = server_at(Some(dir.path()), before_cutoff);
    let status = wire!(restarted, scenario_status, json!({})).expect("status");
    assert_eq!(status["event_tree"]["subject"], "second", "{status}");
    assert_eq!(status["store_errors"], json!([]));
    let node = &status["event_tree"]["nodes"][0];
    assert!(
        node.get("children").is_none() && node.get("brier_score").is_none(),
        "{node}"
    );
}

#[tokio::test]
async fn status_lists_recent_forecasts_newest_declaration_first() {
    let server = server();
    let set_clock = |clock: Clock| {
        server
            .forecast_store
            .lock()
            .expect("store")
            .set_clock(clock)
    };
    wire!(
        server,
        scenario_declare,
        declaration("older", &[event("a", 0.6, json!([]))])
    )
    .expect("older");
    set_clock(a_little_later);
    wire!(
        server,
        scenario_declare,
        declaration("newer", &[event("a", 0.6, json!([]))])
    )
    .expect("newer");
    let status = wire!(server, scenario_status, json!({})).expect("status");
    let order: Vec<&str> = status["pipeline"]["recent_forecasts"]
        .as_array()
        .expect("recent")
        .iter()
        .map(|forecast| forecast["forecast_id"].as_str().expect("id"))
        .collect();
    assert_eq!(order, ["newer", "older"]);
}

// ── SC-14: a store file that does not load is never replaced ───────────

#[tokio::test]
async fn an_unreadable_project_file_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("projects.json");
    let damaged = br#"{"kept":{"project_id":"kept","subject":"kept"}, TRUNCATED"#;
    std::fs::write(&path, damaged).expect("write");
    let server = server_at(Some(dir.path()), before_cutoff);

    refused(
        wire!(server, scenario_assess, json!({"project_id": "kept"})),
        FailedPrecondition,
        "projects.json",
    );
    refused(
        wire!(
            server,
            scenario_frame_document,
            json!({"subject": "new", "answers": "{}"})
        ),
        FailedPrecondition,
        "Repair the file or move it aside",
    );
    refused(
        wire!(
            server,
            scenario_quantify,
            json!({"events": [event("a", 0.5, json!([]))]})
        ),
        FailedPrecondition,
        "projects.json",
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        damaged,
        "the file must be untouched"
    );
    let status = wire!(server, scenario_status, json!({})).expect("status still answers");
    assert_eq!(
        status["store_errors"].as_array().expect("errors").len(),
        1,
        "{status}"
    );
}

#[tokio::test]
async fn an_unreadable_forecast_snapshot_is_never_overwritten() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("forecasts.json");
    let damaged = br#"{"f:a": {"forecast_id": "f", TRUNCATED"#;
    std::fs::write(&path, damaged).expect("write");
    let server = server_at(Some(dir.path()), before_cutoff);
    let events = [event("a", 0.6, json!([]))];

    refused(
        wire!(server, scenario_declare, declaration("g", &events)),
        FailedPrecondition,
        "forecasts.json",
    );
    refused(
        wire!(
            server,
            scenario_score,
            json!({"forecast_id": "f", "events": events, "resolver": "pins", "outcomes": []})
        ),
        FailedPrecondition,
        "forecasts.json",
    );
    refused(
        wire!(server, scenario_calibration, json!({})),
        FailedPrecondition,
        "forecasts.json",
    );
    assert_eq!(
        std::fs::read(&path).expect("read"),
        damaged,
        "the file must be untouched"
    );
    assert!(
        !dir.path().join("forecasts.json.journal").exists(),
        "no journal line may be written"
    );
}

#[tokio::test]
async fn a_damaged_journal_line_blocks_writes_and_an_unfinished_last_line_does_not() {
    let dir = tempfile::tempdir().expect("dir");
    let journal = dir.path().join("forecasts.json.journal");
    let events = [event("a", 0.6, json!([]))];
    {
        let server = server_at(Some(dir.path()), before_cutoff);
        for id in ["one", "two", "three"] {
            wire!(server, scenario_declare, declaration(id, &events)).expect("declare");
        }
    }
    let intact = std::fs::read_to_string(&journal).expect("journal");
    assert_eq!(intact.lines().count(), 3);

    let lines: Vec<&str> = intact.lines().collect();
    let damaged = format!(
        "{}\n{}\n{}\n",
        lines[0],
        lines[1].replacen("\"key\"", "\"kex\"", 1),
        lines[2]
    );
    std::fs::write(&journal, &damaged).expect("damage");
    let server = server_at(Some(dir.path()), before_cutoff);
    refused(
        wire!(server, scenario_declare, declaration("one", &events)),
        FailedPrecondition,
        "line 2",
    );
    assert_eq!(
        std::fs::read_to_string(&journal).expect("journal"),
        damaged,
        "the journal must be untouched"
    );
    assert!(
        !dir.path().join("forecasts.json").exists(),
        "no snapshot may replace the damaged journal"
    );

    // A crash mid-append leaves a last line with no newline: never acknowledged.
    let torn = format!(
        "{}\n{}\n{}",
        lines[0],
        lines[1],
        &lines[2][..lines[2].len() / 2]
    );
    std::fs::write(&journal, torn).expect("tear");
    let server = server_at(Some(dir.path()), before_cutoff);
    let status = wire!(server, scenario_status, json!({})).expect("status");
    assert_eq!(status["pipeline"]["forecast_count"], 2);
    assert_eq!(status["store_errors"], json!([]));
    // The next write must not land after the fragment: a restart then reads
    // all three records and still accepts writes.
    wire!(server, scenario_declare, declaration("four", &events)).expect("write after a torn tail");
    drop(server);
    let server = server_at(Some(dir.path()), before_cutoff);
    let status = wire!(server, scenario_status, json!({})).expect("status");
    assert_eq!(status["pipeline"]["forecast_count"], 3, "{status}");
    assert_eq!(status["store_errors"], json!([]), "{status}");
}

// ── SC-15: records written before this change still read ───────────────

#[tokio::test]
async fn records_written_before_this_change_still_read_and_score() {
    let dir = tempfile::tempdir().expect("dir");
    let mut old_event = event("evt", 0.8, json!([]));
    old_event["brier_score"] = Value::Null;
    old_event["basis"] = Value::Null;
    old_event["base_rate"] = Value::Null;
    old_event["reference_class"] = Value::Null;
    let forecasts = json!({"old:evt": {
        "schema_version": 5, "forecast_id": "old", "event_id": "evt", "event_name": "evt",
        "subject": "pins", "probability": 0.8, "scored_from_marginal": true,
        "created_at": "2026-10-19", "outcome": null, "resolved_at": null,
        "category": "emerging_economic",
        "declaration": {"event": old_event, "resolver": "pins",
                        "declared_at": "2026-10-20T12:00:00+00:00", "resolution_at": CUTOFF}}});
    let projects = json!({"pins": {
        "project_id": "pins", "subject": "pins", "framing_document": null,
        "last_tree": {"subject": "pins", "time_horizon": "tactical", "scenario_type": "emerging_economic",
            "nodes": [{"event": old_event, "marginal_probability": 0.8, "paths": [["evt"]], "certainty_distance": 0.6}],
            "root_ids": ["evt"], "topo_order": ["evt"], "joint_probability": 0.8},
        "assessment_history": [{"date": "2026-10-19", "overall_score": 0.4}],
        "created_at": "2026-10-19", "updated_at": "2026-10-19"}});
    std::fs::write(dir.path().join("forecasts.json"), forecasts.to_string()).expect("forecasts");
    std::fs::write(dir.path().join("projects.json"), projects.to_string()).expect("projects");

    let server = server_at(Some(dir.path()), after_cutoff);
    let status = wire!(server, scenario_status, json!({})).expect("status");
    assert_eq!(status["store_errors"], json!([]), "{status}");
    assert_eq!(status["event_tree"]["subject"], "pins");
    assert_eq!(
        status["pipeline"]["recent_forecasts"][0]["scoring_eligible"],
        true
    );

    let scored = wire!(
        server,
        scenario_score,
        json!({"forecast_id": "old", "events": [event("evt", 0.8, json!([]))], "resolver": "pins",
               "outcomes": [{"event_id": "evt", "occurred": true}]})
    )
    .expect("an older declaration scores against the same event");
    assert!((scored["aggregate"]["brier_score"].as_f64().expect("score") - 0.04).abs() < 1e-12);
}

// ── SC-16: scenario_assess reads; equity records name what they did ────

#[tokio::test]
async fn scenario_assess_leaves_the_project_file_unchanged() {
    let dir = tempfile::tempdir().expect("dir");
    let server = server_at(Some(dir.path()), before_cutoff);
    wire!(
        server,
        scenario_frame_document,
        json!({"subject": "pins", "answers": "{}"})
    )
    .expect("project");
    let path = dir.path().join("projects.json");
    let before = std::fs::read(&path).expect("read");
    let assessed = wire!(server, scenario_assess, json!({"project_id": "pins"})).expect("assess");
    assert!(
        assessed.get("phase_scores").is_none(),
        "one copy of the phase scores: {assessed}"
    );
    assert_eq!(std::fs::read(&path).expect("read"), before);
}

#[tokio::test]
async fn equity_tools_refuse_dangling_references_and_name_what_they_stored() {
    let server = server();
    let persist = |id: Option<&str>, revision_of: Option<&str>, symbol: &str| {
        json!({"symbol": symbol, "forecast_date": "2026-01-02", "horizon": "6mo",
               "forecast_price_change": 0.2, "forecast_id": id, "revision_of": revision_of})
    };
    refused(
        wire!(server, forecast_persist, persist(Some(" "), None, "PINS")),
        InvalidArgument,
        "non-blank",
    );
    refused(
        wire!(
            server,
            forecast_persist,
            persist(Some("r1"), Some("never-stored"), "PINS")
        ),
        NotFound,
        "never-stored",
    );

    let stored =
        wire!(server, forecast_persist, persist(None, None, "PINS")).expect("generated id");
    let generated = stored["forecast_id"].as_str().expect("id").to_owned();
    assert_eq!(generated.len(), 36, "a generated id is a UUID");
    refused(
        wire!(
            server,
            forecast_persist,
            persist(Some("r2"), Some(&generated), "OTHER")
        ),
        InvalidArgument,
        "belongs to symbol 'PINS'",
    );
    wire!(
        server,
        forecast_persist,
        persist(Some("r3"), Some(&generated), "PINS")
    )
    .expect("a revision of a generated id is accepted");

    let record = |id: Option<&str>, outcome_date: &str| {
        json!({"symbol": "PINS", "forecast_date": "2026-01-02", "horizon": "6mo", "forecast_multiple": 10.0,
               "forecast_price_change": 0.2, "outcome_date": outcome_date, "actual_multiple": 11.0,
               "actual_price_change": 0.21, "forecast_id": id})
    };
    let unstored = wire!(server, forecast_record, record(None, "2026-07-02")).expect("no id");
    assert_eq!(unstored["status"], "computed_only");
    let recorded =
        wire!(server, forecast_record, record(Some("r3"), "2026-07-02")).expect("with id");
    assert_eq!(recorded["status"], "recorded");
    refused(
        wire!(server, forecast_record, record(None, "not-a-date")),
        InvalidArgument,
        "outcome_date",
    );
}

// ── SC-24: max_events 0 asks for no events ──────────────────────────────

#[tokio::test]
async fn scenario_build_refuses_max_events_zero() {
    let server = server();
    refused(
        wire!(
            server,
            scenario_build,
            json!({"subject": "pins", "max_events": 0})
        ),
        InvalidArgument,
        "max_events",
    );
    // One is the floor the prompt can honor: "extract up to 1 key future events".
    let one = wire!(
        server,
        scenario_build,
        json!({"subject": "pins", "max_events": 1})
    )
    .expect("max_events 1 is the minimum, not a refusal");
    assert_eq!(one["max_events"], 1);
    // The schema carries the bound the handler enforces.
    let schemas: std::collections::BTreeMap<String, serde_json::Value> =
        ScenariosServer::combined_router()
            .list_all()
            .into_iter()
            .map(|tool| {
                (
                    tool.name.to_string(),
                    serde_json::to_value(&tool.input_schema).expect("schema"),
                )
            })
            .collect();
    assert_eq!(
        schemas["scenario_build"]["properties"]["max_events"]["anyOf"][0]["minimum"], 1,
        "max_events declares its floor"
    );
}

// ── SC-25: the scored equity path runs on the store's clock ────────────

#[tokio::test]
async fn the_scored_equity_path_runs_on_the_stores_clock() {
    fn at(moment: &'static str) -> chrono::DateTime<chrono::Utc> {
        moment.parse().expect("clock")
    }
    let server = server_at(None, before_cutoff);
    server
        .equity_forecast_store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_clock(|| at("2026-01-15T09:00:00Z"));

    // Persisted on the store clock's today with a probability: prospective,
    // and the declaration freezes the horizon from the clock's calendar.
    let persisted = wire!(
        server,
        forecast_persist,
        json!({"symbol": "PINS", "forecast_date": "2026-01-15", "horizon": "3mo",
               "forecast_price_change": 0.10, "forecast_probability": 0.6,
               "forecast_id": "sc-25"})
    )
    .expect("persist answers");
    assert_eq!(persisted["scoring_eligible"], true, "{persisted}");
    assert_eq!(persisted["deadline"], "2026-04-15");

    // Before the frozen deadline the store's clock gates the resolution.
    let early = json!({"symbol": "PINS", "forecast_date": "2026-01-15", "horizon": "3mo",
        "forecast_multiple": 15.0, "forecast_price_change": 0.10,
        "outcome_date": "2026-04-15", "actual_multiple": 14.0,
        "actual_price_change": 0.12, "forecast_id": "sc-25"});
    refused(
        wire!(server, forecast_record, early.clone()),
        InvalidArgument,
        "elapsed",
    );

    // Advance the clock to the deadline: the frozen prior 0.6 is scored.
    server
        .equity_forecast_store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .set_clock(|| at("2026-04-15T17:00:00Z"));
    let recorded = wire!(server, forecast_record, early).expect("record answers");
    assert_eq!(recorded["status"], "recorded", "{recorded}");
    assert_eq!(recorded["scoring_status"], "scored");
    // actual 0.12 is within 20% of forecast 0.10 → accurate → Brier (0.6 − 1)² = 0.16.
    let brier = recorded["brier"]["return_accuracy"]
        .as_f64()
        .expect("scored prior");
    assert!(
        (brier - 0.16).abs() < 1e-9,
        "Brier on the frozen 0.6: {recorded}"
    );
}

// ── D3e (SC-20 annex): the certainty-tier ranges are the pre-review wording ──
//
// 3b155590a7 reworded the ranges to the code's boundaries without a source
// check (the fourth unvetted source edit of SC-20). The operator's D3e
// default restores the wording the review started from; the code's actual
// boundaries (hkask_forecast::certainty_tier: >=0.67, >=0.33) match the
// integer-percent ranges, and the pin keeps the wire text stable.

#[tokio::test]
async fn scenario_build_certainty_tiers_are_the_pre_review_wording() {
    let server = server();
    let scaffold =
        wire!(server, scenario_build, json!({"subject": "pins"})).expect("scenario_build answers");
    assert_eq!(
        scaffold["certainty_tiers"]["proximate"]["range"], "67-100%",
        "proximate range: {scaffold}"
    );
    assert_eq!(
        scaffold["certainty_tiers"]["probable"]["range"], "33-66%",
        "probable range: {scaffold}"
    );
    assert_eq!(
        scaffold["certainty_tiers"]["possible"]["range"], "0-32%",
        "possible range: {scaffold}"
    );
}
