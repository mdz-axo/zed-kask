//! Tool-behavior contract tests for the scenarios MCP server.
//!
//! Drives the real `#[tool]` methods through their public `Parameters<T>`
//! seam, in-process, with an in-memory `ForecastStore`. Covers the testing
//! standard minimum (docs/reference/mcp-servers/README.md §Testing standard):
//! happy path, invalid input, boundary/edge cases, and error-specificity.
//!
//! No network: this server has no reqwest dependency, so every tool is pure
//! computation over caller-supplied inputs and the in-memory store.

#![cfg(test)]

use hkask_mcp_scenarios::requests::{
    BrainstormRequest, CalibrateRequest, ContractCoherenceRequest, OutcomeEntry, QuantifyRequest,
    ScoreRequest, StatusRequest, TriageRequest,
};
use hkask_mcp_scenarios::types::{ScenarioEvent, ScenarioType, SubQuestion, TimeHorizon};
use hkask_mcp_scenarios::{ForecastStore, ScenariosServer};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// Build a server backed by an empty in-memory store and empty caches — the
/// clean-slate state every test starts from.
fn make_server() -> ScenariosServer {
    let forecast_store = Arc::new(Mutex::new(ForecastStore::new(None)));
    let tree_cache = Mutex::new(None);
    let called_tools = Mutex::new(HashSet::new());
    ScenariosServer::new(WebID::new(), forecast_store, tree_cache, called_tools)
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
        deadline: chrono::NaiveDate::from_ymd_opt(2027, 12, 31).expect("valid deadline date"),
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
/// class, clear resolution, enough words) as clocklike and forecastable — the
/// top of the Goldilocks triage band.
#[tokio::test]
async fn scenario_triage_marks_well_specified_question_clocklike() {
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
        Some("clocklike"),
        "a fully-specified question is clocklike, got: {parsed}"
    );
    assert_eq!(
        parsed["is_forecastable"].as_bool(),
        Some(true),
        "a clocklike question is forecastable"
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
/// falls below the goldilocks floor into cloudlike and is not forecastable —
/// the bottom of the triage band.
#[tokio::test]
async fn scenario_triage_marks_vague_question_cloudlike() {
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
        Some("cloudlike"),
        "an under-specified question is cloudlike, got: {parsed}"
    );
    assert_eq!(
        parsed["is_forecastable"].as_bool(),
        Some(false),
        "a cloudlike question is not forecastable"
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

// ── contract_price_coherence (R5 / H3 reframed) ───────────────────────────────

/// Happy path: an explicit `tree_implied` within the transaction-cost band is
/// reported coherent, with the divergence and both inputs echoed back.
#[tokio::test]
async fn contract_price_coherence_within_band_is_coherent() {
    let server = make_server();
    let output = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 0.50,
            cost_band: 0.03,
            tree_implied: Some(0.52),
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(parsed["tree_implied"].as_f64(), Some(0.52));
    assert_eq!(parsed["market_price"].as_f64(), Some(0.50));
    assert!((parsed["divergence"].as_f64().unwrap_or(f64::NAN) - 0.02).abs() < 1e-9);
    assert_eq!(parsed["coherent"].as_bool(), Some(true));
}

/// Happy path: divergence beyond the cost band is the arbitrage signal —
/// `coherent` is false and the interpretation names the signal.
#[tokio::test]
async fn contract_price_coherence_beyond_band_is_divergent() {
    let server = make_server();
    let output = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 0.40,
            cost_band: 0.03,
            tree_implied: Some(0.52),
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(parsed["coherent"].as_bool(), Some(false));
    assert_eq!(parsed["divergence"].as_f64(), Some(0.12));
    let interpretation = parsed["interpretation"]
        .as_str()
        .expect("interpretation is a string");
    assert!(
        interpretation.contains("arbitrage signal"),
        "a divergent measure must name the signal, got: {interpretation}"
    );
}

/// Cached-tree path: after `scenario_quantify` populates the tree cache, the
/// tool uses the tree's joint probability without an explicit `tree_implied`.
/// Two independent roots at 0.6 and 0.5 → joint 0.30.
#[tokio::test]
async fn contract_price_coherence_defaults_to_cached_tree_joint() {
    let server = make_server();
    let events = vec![
        independent_event("evt-a", "event a", 0.6),
        independent_event("evt-b", "event b", 0.5),
    ];
    server
        .scenario_quantify(Parameters(QuantifyRequest { events }))
        .await
        .expect("quantify ok");
    let output = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 0.31,
            cost_band: 0.05,
            tree_implied: None,
        }))
        .await
        .expect("tool ok");
    let parsed = parse(&output);
    assert_eq!(parsed["tree_implied"].as_f64(), Some(0.30));
    assert_eq!(parsed["coherent"].as_bool(), Some(true));
}

/// No explicit `tree_implied` and no cached tree is a `failed_precondition` —
/// the error names the tools that populate the cache, not a generic failure.
#[tokio::test]
async fn contract_price_coherence_without_tree_is_failed_precondition() {
    let server = make_server();
    let error = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 0.5,
            cost_band: 0.03,
            tree_implied: None,
        }))
        .await
        .expect_err("no tree and no explicit input must fail");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::FailedPrecondition),
        "a missing cached tree is a precondition failure, got: {error:?}"
    );
    assert!(
        error.message.contains("scenario_quantify"),
        "the error must name the populating tools, got: {error:?}"
    );
}

/// A `market_price` outside [0, 1] is an `invalid_argument` — a coherence
/// measure over an invalid probability is never fabricated.
#[tokio::test]
async fn contract_price_coherence_rejects_invalid_market_price() {
    let server = make_server();
    let error = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 1.2,
            cost_band: 0.03,
            tree_implied: Some(0.5),
        }))
        .await
        .expect_err("an out-of-range market price must be rejected");
    assert!(
        matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
        "an invalid market price is a caller-input defect, got: {error:?}"
    );
    assert!(
        error.message.contains("market_price must be in [0, 1]"),
        "the error must state the constraint, got: {error:?}"
    );
}

// ── scenario_calibrate isotonic channel (arXiv:2604.20421 §6.1) ───────────────

/// Seed the store with resolved forecasts via the real `scenario_score` path
/// (pending insert + outcome resolution), then run `scenario_calibrate` and
/// assert the isotonic channel is emitted with an inspectable fit.
#[tokio::test]
async fn scenario_calibrate_emits_isotonic_channel_after_resolved_forecasts() {
    let server = make_server();
    // Four resolved pairs with a monotone mapping: higher probability →
    // occurred. The PAVA fit over these is a usable step function.
    let events = vec![
        independent_event("iso-a", "low prob event", 0.2),
        independent_event("iso-b", "mid prob event", 0.5),
        independent_event("iso-c", "high prob event", 0.8),
        independent_event("iso-d", "very high prob event", 0.9),
    ];
    let outcomes = vec![
        OutcomeEntry {
            event_id: "iso-a".into(),
            occurred: false,
        },
        OutcomeEntry {
            event_id: "iso-b".into(),
            occurred: false,
        },
        OutcomeEntry {
            event_id: "iso-c".into(),
            occurred: true,
        },
        OutcomeEntry {
            event_id: "iso-d".into(),
            occurred: true,
        },
    ];
    server
        .scenario_score(Parameters(ScoreRequest {
            forecast_id: "iso-fit-test".into(),
            events,
            outcomes,
        }))
        .await
        .expect("score ok");

    let output = server
        .scenario_calibrate(Parameters(CalibrateRequest {
            question: "Will the isotonic channel fire?".into(),
            sub_questions: vec![SubQuestion {
                question: "Does the fit exist?".into(),
                estimate: 0.7,
                confidence: 0.8,
            }],
            reference_class: None,
            base_rate: None,
            reference_count: None,
        }))
        .await
        .expect("calibrate ok");
    let parsed = parse(&output);

    let knots = parsed["isotonic_fit_knots"]
        .as_array()
        .expect("fit knots must be an array when resolved pairs exist");
    assert!(
        !knots.is_empty(),
        "the PAVA fit over 4 resolved pairs must have at least one knot"
    );
    assert_eq!(parsed["isotonic_fit_size"].as_u64(), Some(4));
    let isotonic_probability = parsed["isotonic_calibrated_probability"]
        .as_f64()
        .expect("isotonic probability must be present with a fit");
    assert!(
        (0.0..=1.0).contains(&isotonic_probability),
        "isotonic probability must stay in [0, 1], got {isotonic_probability}"
    );
    // The fit is monotone non-decreasing: each knot's calibrated value must
    // not exceed the next's.
    for window in knots.windows(2) {
        let a = window[0]["calibrated_value"].as_f64().unwrap_or(f64::NAN);
        let b = window[1]["calibrated_value"].as_f64().unwrap_or(f64::NAN);
        assert!(a <= b + 1e-9, "fit must be non-decreasing: {knots:?}");
    }
}

/// With an empty store (no resolved pairs), the isotonic channel is withheld
/// with a note — never a fabricated fit.
#[tokio::test]
async fn scenario_calibrate_withholds_isotonic_without_resolved_pairs() {
    let server = make_server();
    let output = server
        .scenario_calibrate(Parameters(CalibrateRequest {
            question: "Will the isotonic channel be withheld?".into(),
            sub_questions: vec![SubQuestion {
                question: "Is the store empty?".into(),
                estimate: 0.9,
                confidence: 0.9,
            }],
            reference_class: None,
            base_rate: None,
            reference_count: None,
        }))
        .await
        .expect("calibrate ok");
    let parsed = parse(&output);
    assert!(
        parsed["isotonic_calibrated_probability"].is_null(),
        "no resolved pairs must yield no isotonic probability, got: {parsed}"
    );
    let note = parsed["isotonic_note"]
        .as_str()
        .expect("isotonic note is a string");
    assert!(
        note.contains("fewer than 2 resolved"),
        "the note must name the reason, got: {note}"
    );
}

/// The CMP seam end-to-end: `scenario_from_cmp_indices` composes
/// ProvenancedCmpIndex objects (produced by market_cmp_indices on
/// hkask-mcp-prediction-markets) into an EventTree AND caches it, so
/// `contract_price_coherence`'s `tree_implied` default resolves. Pre-fix,
/// from_cmp_indices never wrote the cache and coherence failed with
/// failed_precondition despite its documented default.
#[tokio::test]
async fn from_cmp_indices_caches_tree_for_coherence_default() {
    use hkask_mcp_scenarios::requests::CmpBridgeRequest;

    let server = make_server();

    // One ProvenancedCmpIndex in its serialized shape (family, venue, and
    // the flattened CmpIndex: bucket, orientation, solved portfolio).
    let cmp_indices = serde_json::json!([{
        "family": "policy_interest_rate",
        "venue": "kalshi",
        "bucket": "one_month",
        "orientation": "increase",
        "portfolio": {
            "constituents": [],
            "weighted_maturity_days": 30.0,
            "maturity_error_days": 0.5,
            "index_probability": 0.55,
            "method": "interpolated",
        }
    }]);
    let request = CmpBridgeRequest {
        cmp_indices: hkask_types::AnyJsonValue(cmp_indices),
        observation_date: "2026-09-03".to_string(),
        dependency_specs: None,
    };

    let output = server
        .scenario_from_cmp_indices(Parameters(request))
        .await
        .expect("composition ok");
    let parsed = parse(&output);
    let joint = parsed["tree"]["joint_probability"]
        .as_f64()
        .expect("joint probability is a number");
    assert!(
        (joint - 0.55).abs() < 1e-9,
        "one root at 0.55 → joint 0.55, got {joint}"
    );

    // The cached tree now feeds contract_price_coherence's default — no
    // explicit tree_implied, no failed_precondition.
    let coherence = server
        .contract_price_coherence(Parameters(ContractCoherenceRequest {
            market_price: 0.70,
            cost_band: 0.02,
            tree_implied: None,
        }))
        .await
        .expect("coherence must resolve the cached tree");
    let coherence_parsed = parse(&coherence);
    assert!(
        coherence_parsed.get("divergence").is_some(),
        "coherence must compute against the cached joint, got: {coherence_parsed}"
    );
}
