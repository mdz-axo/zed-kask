//! Contract tests for hkask-mcp-scenarios — event-tree forecasting invariants.
//!
//! Every test carries the full traceability chain:
//! `UserFunctionalExpectation (expect:) → GoalPrinciple [P{N}] → ConstrainingPrinciple [P{N}] → REQ: → Test`
//!
//! Tested seams:
//! - `ForecastStore` (in-memory, no external dependencies)
//! - Tool-behavior contracts via `Parameters<T>` seam

use hkask_mcp_scenarios::ScenariosServer;
use hkask_mcp_scenarios::superforecast::ForecastStore;
use hkask_mcp_scenarios::{StatusRequest, TriageRequest, UpdateRequest};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

// ── ForecastStore contract tests ────────────────────────────────────────────

#[test]
fn forecast_store_starts_empty() {
    let store = ForecastStore::new(None);
    assert_eq!(store.len(), 0);
}

// ── Tool-behavior contract tests (Parameters<T> seam) ───────────────────────
//
// These exercise the actual MCP tool methods through the public `Parameters<T>`
// seam — the same surface an agent uses. Closes the test-variety gap that hid
// the create-new-file, range-inversion, and multibyte-truncation defects in
// hkask-mcp-filesystem.

/// Construct a ScenariosServer with an in-memory forecast store.
fn test_server() -> ScenariosServer {
    ScenariosServer::new(
        WebID::new(),
        "test-userpod".into(),
        None,
        Arc::new(Mutex::new(ForecastStore::new(None))),
        reqwest::Client::new(),
        Mutex::new(None),
        Mutex::new(HashSet::new()),
    )
}

/// Parse the success envelope `{"content": <value>}`; falls back to the raw
/// value for non-envelope outputs.
fn parse_content(out: &str) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("content").cloned().unwrap_or(v)
}

/// Extract the `kind` field from an error envelope, if present.
fn error_kind(out: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(out).expect("tool output is JSON");
    v.get("kind").and_then(|e| e.as_str()).map(String::from)
}

// REQ: scenario_status returns pipeline overview (P5 Testing Discipline).
// expect: scenario_status returns a JSON object with pipeline info.
#[tokio::test]
async fn scenario_status_returns_overview_via_parameters_seam() {
    let server = test_server();
    let out = server.scenario_status(Parameters(StatusRequest {})).await;
    let content = parse_content(&out);
    assert!(
        content.is_object(),
        "status should return a JSON object: {out}"
    );
}

// REQ: scenario_triage classifies a forecasting question (P5).
// expect: triage returns a difficulty classification for a clear question.
#[tokio::test]
async fn scenario_triage_classifies_question_via_parameters_seam() {
    let server = test_server();
    let req: TriageRequest = serde_json::from_value(serde_json::json!({
        "question": "Will AAPL close above $200 on Dec 31 2026?",
        "has_deadline": true,
        "has_reference_class": true,
        "has_resolution_criteria": true
    }))
    .expect("deserialize TriageRequest");
    let out = server.scenario_triage(Parameters(req)).await;
    let content = parse_content(&out);
    assert!(
        content.get("difficulty").is_some(),
        "should have difficulty: {out}"
    );
    assert!(content.get("scores").is_some(), "should have scores: {out}");
}

// REQ: scenario_update rejects an out-of-range prior probability (P5).
// expect: a prior_probability > 1.0 returns kind=invalid_argument.
#[tokio::test]
async fn scenario_update_rejects_invalid_prior_via_parameters_seam() {
    let server = test_server();
    let req: UpdateRequest = serde_json::from_value(serde_json::json!({
        "forecast_id": "test-fc-1",
        "event_id": "evt-1",
        "prior_probability": 1.5,
        "evidence_likelihood": 0.8,
        "evidence_base_rate": 0.5
    }))
    .expect("deserialize UpdateRequest");
    let out = server.scenario_update(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for invalid prior");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}

// REQ: scenario_update rejects an out-of-range evidence likelihood (P5).
// expect: an evidence_likelihood < 0.0 returns kind=invalid_argument.
#[tokio::test]
async fn scenario_update_rejects_negative_likelihood_via_parameters_seam() {
    let server = test_server();
    let req: UpdateRequest = serde_json::from_value(serde_json::json!({
        "forecast_id": "test-fc-2",
        "event_id": "evt-2",
        "prior_probability": 0.5,
        "evidence_likelihood": -0.1,
        "evidence_base_rate": 0.5
    }))
    .expect("deserialize UpdateRequest");
    let out = server.scenario_update(Parameters(req)).await;
    let kind = error_kind(&out).expect("expected error kind for negative likelihood");
    assert_eq!(kind, "invalid_argument", "got: {out}");
}
