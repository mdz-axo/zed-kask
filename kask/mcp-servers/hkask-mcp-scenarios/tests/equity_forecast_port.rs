//! Equity forecast port behavior tests (plan Phase A P1 oracle: a ported
//! tool is verified when it is CALLED in its new server and returns the
//! correct result — not by `cargo check`).
//!
//! The three forecast tools were ported from the companies MCP server
//! (companies/FMP/EODHD/FIBO removal, commit `3439db5e00` — the executed
//! design record is preserved in git history at
//! `kask/docs/plans/companies-fmp-eodhd-fibo-removal.md` §3 P1) with
//! caller-supplied inputs only: the DCF-model gap decomposition was
//! dropped with the providers (its inputs were provider statements);
//! `forecast_record` must degrade VISIBLY on that absence (a named
//! `decomposition` note), never silently. The `scenario_risk_measure`
//! wrapper (operator decision N2) exposes the shared `hkask_forecast`
//! risk functions over caller-supplied branch returns.

use hkask_mcp_scenarios::equity_forecast::{
    BranchSpec, EquityForecastListRequest, EquityForecastPersistRequest,
    EquityForecastRecordRequest, ScenarioRiskRequest,
};
use hkask_mcp_scenarios::{ForecastStore, JournalStore, ProjectStore, ScenariosServer};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;
use std::sync::{Arc, Mutex};

fn server() -> ScenariosServer {
    server_using(equity_forecast_store())
}

fn server_using(
    forecasts: Arc<Mutex<JournalStore<hkask_mcp_scenarios::equity_forecast::StoredEquityForecast>>>,
) -> ScenariosServer {
    ScenariosServer::new(
        WebID::new(),
        Arc::new(Mutex::new(ForecastStore::new(None))),
        forecasts,
        Mutex::new(None),
        Mutex::new(ProjectStore::new(None)),
    )
}

fn equity_forecast_store()
-> Arc<Mutex<JournalStore<hkask_mcp_scenarios::equity_forecast::StoredEquityForecast>>> {
    Arc::new(Mutex::new(JournalStore::new(None)))
}

/// Unwrap the `{"content": ...}` envelope every tool returns.
fn content(output: &str) -> Value {
    let envelope: Value = serde_json::from_str(output).expect("tool output is a JSON envelope");
    envelope
        .get("content")
        .cloned()
        .expect("envelope has content")
}

#[tokio::test]
async fn matured_prospective_forecast_scores_the_frozen_probability() {
    let forecasts = equity_forecast_store();
    let server = server_using(Arc::clone(&forecasts));
    let persisted = server
        .forecast_persist(Parameters(EquityForecastPersistRequest {
            symbol: "TEST".to_string(),
            forecast_date: "2026-01-15".to_string(),
            horizon: "3mo".to_string(),
            forecast_price_change: Some(0.10),
            forecast_multiple: Some(15.0),
            forecast_price: Some(110.0),
            current_price: Some(100.0),
            forecast_probability: Some(0.6),
            revision_of: None,
            forecast_id: Some("pf-1".to_string()),
            scenario_project_id: Some("proj-1".to_string()),
            scenario_tree: None,
            impact_mappings_ref: Some("digest:abc".to_string()),
            fused_volatility: Some(0.28),
        }))
        .await
        .expect("forecast_persist answers");
    let body = content(&persisted);
    assert_eq!(body["status"], "persisted", "persist status: {body}");
    assert_eq!(body["forecast_id"], "pf-1");
    assert_eq!(body["forecast_price_change"], 0.10);

    // Filesystem/store boundary fixture: a declaration originally admitted
    // on 2026-01-15, now matured. Admission at today's real clock is tested
    // separately; no test sleeps three months or fabricates outcome timing.
    {
        let mut store = forecasts.lock().expect("store");
        let mut record = store.get("pf-1").expect("persisted").clone();
        record.schema_version = 2;
        record.created_at = "2026-01-15T00:00:00Z".into();
        record.declaration = Some(hkask_mcp_scenarios::equity_forecast::EquityDeclaration {
            deadline: chrono::NaiveDate::from_ymd_opt(2026, 4, 15).expect("deadline"),
            resolver: "caller_reported_price_return_at_horizon".into(),
            tolerance: 0.20,
        });
        store
            .insert("pf-1".into(), record)
            .expect("fixture journal");
    }
    // Resolution reads the frozen prior (0.6), not a submitted prior.
    let recorded = server
        .forecast_record(Parameters(EquityForecastRecordRequest {
            symbol: "TEST".to_string(),
            forecast_date: "2026-01-15".to_string(),
            horizon: "3mo".to_string(),
            forecast_multiple: 15.0,
            forecast_price_change: 0.10,
            outcome_date: "2026-04-15".to_string(),
            actual_multiple: 14.0,
            actual_price_change: 0.12,
            forecast_id: Some("pf-1".to_string()),
        }))
        .await
        .expect("forecast_record answers");
    let body = content(&recorded);
    assert_eq!(body["status"], "recorded", "record status: {body}");
    // 0.12 is within 20% of 0.10 → accurate → Brier = (0.6 - 1)^2 = 0.16.
    assert!(
        (body["brier"]["return_accuracy"].as_f64().unwrap() - 0.16).abs() < 1e-9,
        "Brier on the stored prior 0.6: {body}"
    );
    assert!(
        body["brier"]["combined"]
            .as_f64()
            .is_some_and(|b| (b - 0.16).abs() < 1e-9),
        "combined Brier: {body}"
    );
    // The decomposition absence is a NAMED note, never silent (plan P1
    // design note — visible degradation).
    assert!(
        body["decomposition"]
            .as_str()
            .is_some_and(|d| d.contains("unavailable")),
        "decomposition degrades visibly: {body}"
    );
}

#[tokio::test]
async fn forecast_record_without_a_persisted_probability_is_explicitly_unscored() {
    let server = server();
    server
        .forecast_persist(Parameters(EquityForecastPersistRequest {
            symbol: "NOFB".to_string(),
            forecast_date: "2026-01-15".to_string(),
            horizon: "1yr".to_string(),
            forecast_price_change: Some(0.0),
            forecast_multiple: None,
            forecast_price: Some(100.0),
            current_price: Some(100.0),
            forecast_probability: None,
            revision_of: None,
            forecast_id: Some("pf-nofb".to_string()),
            scenario_project_id: None,
            scenario_tree: None,
            impact_mappings_ref: None,
            fused_volatility: None,
        }))
        .await
        .expect("persist answers");
    let recorded = server
        .forecast_record(Parameters(EquityForecastRecordRequest {
            symbol: "NOFB".to_string(),
            forecast_date: "2026-01-15".to_string(),
            horizon: "1yr".to_string(),
            forecast_multiple: 10.0,
            forecast_price_change: 0.0,
            outcome_date: "2027-01-15".to_string(),
            actual_multiple: 10.0,
            actual_price_change: 0.5,
            forecast_id: Some("pf-nofb".to_string()),
        }))
        .await
        .expect("record answers");
    let body = content(&recorded);
    assert!(body["brier"]["combined"].is_null(), "no declared p: {body}");
    assert_eq!(body["scoring_status"], "unscored");
    assert!(body["unscored_reason"].as_str().is_some(), "reason: {body}");
}

#[tokio::test]
async fn forecast_list_returns_the_symbol_persisted_forecasts() {
    let server = server();
    for id in ["pf-a", "pf-b"] {
        server
            .forecast_persist(Parameters(EquityForecastPersistRequest {
                symbol: "LIST".to_string(),
                forecast_date: "2026-02-01".to_string(),
                horizon: "6mo".to_string(),
                forecast_price_change: Some(0.05),
                forecast_multiple: None,
                forecast_price: None,
                current_price: None,
                forecast_probability: Some(0.55),
                revision_of: None,
                forecast_id: Some(id.to_string()),
                scenario_project_id: None,
                scenario_tree: None,
                impact_mappings_ref: None,
                fused_volatility: None,
            }))
            .await
            .expect("persist answers");
    }
    server
        .forecast_persist(Parameters(EquityForecastPersistRequest {
            symbol: "OTHER".to_string(),
            forecast_date: "2026-02-01".to_string(),
            horizon: "6mo".to_string(),
            forecast_price_change: Some(0.05),
            forecast_multiple: None,
            forecast_price: None,
            current_price: None,
            forecast_probability: Some(0.55),
            revision_of: None,
            forecast_id: Some("pf-other".to_string()),
            scenario_project_id: None,
            scenario_tree: None,
            impact_mappings_ref: None,
            fused_volatility: None,
        }))
        .await
        .expect("persist answers");

    let listed = server
        .forecast_list(Parameters(EquityForecastListRequest {
            symbol: "LIST".to_string(),
        }))
        .await
        .expect("list answers");
    let body = content(&listed);
    let forecasts = body["forecasts"].as_array().expect("forecasts array");
    assert_eq!(forecasts.len(), 2, "only the symbol's forecasts: {body}");
    assert!(
        forecasts.iter().all(|f| f["symbol"] == "LIST"),
        "every listed record matches the symbol: {body}"
    );
}

#[tokio::test]
async fn scenario_risk_measure_computes_over_caller_supplied_branches() {
    let server = server();
    let measured = server
        .scenario_risk_measure(Parameters(ScenarioRiskRequest {
            branches: vec![
                BranchSpec {
                    probability: 0.5,
                    branch_return: 0.2,
                },
                BranchSpec {
                    probability: 0.5,
                    branch_return: -0.1,
                },
            ],
            realized_volatility: Some(0.30),
            scenario_weight: Some(1.0),
        }))
        .await
        .expect("risk measure answers");
    let body = content(&measured);
    // Expected return = 0.5*0.2 + 0.5*(-0.1) = 0.05.
    assert!(
        (body["risk_measure"]["expected_return"].as_f64().unwrap() - 0.05).abs() < 1e-9,
        "expected return: {body}"
    );
    // sigma = sqrt(0.5*(0.2-0.05)^2 + 0.5*(-0.1-0.05)^2) = 0.15.
    assert!(
        (body["risk_measure"]["sigma_scenario"].as_f64().unwrap() - 0.15).abs() < 1e-9,
        "scenario sigma: {body}"
    );
    // Fused = sqrt(0.30^2 + 0.15^2).
    let fused = body["fused_volatility"].as_f64().unwrap();
    let expected_fused = (0.30_f64 * 0.30 + 0.15 * 0.15).sqrt();
    assert!(
        (fused - expected_fused).abs() < 1e-9,
        "fused volatility: {body}"
    );
}

#[tokio::test]
async fn scenario_risk_measure_never_fabricates_a_measure_over_zero_mass() {
    let server = server();
    let result = server
        .scenario_risk_measure(Parameters(ScenarioRiskRequest {
            branches: vec![BranchSpec {
                probability: 0.0,
                branch_return: 0.1,
            }],
            realized_volatility: None,
            scenario_weight: None,
        }))
        .await;
    let err = result.expect_err("zero mass must error");
    assert!(
        err.to_string().contains("never fabricated"),
        "the zero-mass degradation names itself: {err}"
    );
}

fn persist_request(id: &str, date: &str, probability: Option<f64>) -> EquityForecastPersistRequest {
    serde_json::from_value(serde_json::json!({"symbol":"TEST", "forecast_id":id, "forecast_date":date, "horizon":"3mo", "forecast_multiple":15.0, "forecast_price_change":0.1, "forecast_probability":probability})).expect("persist request")
}

fn record_request(id: Option<&str>, date: &str, outcome_date: &str) -> EquityForecastRecordRequest {
    serde_json::from_value(serde_json::json!({"symbol":"TEST", "forecast_id":id, "forecast_date":date, "horizon":"3mo", "forecast_multiple":15.0, "forecast_price_change":0.1, "outcome_date":outcome_date, "actual_multiple":14.0, "actual_price_change":0.12})).expect("record request")
}

#[tokio::test]
async fn equity_admission_freezes_horizon_and_early_resolution_is_rejected() {
    let server = server();
    let today = chrono::Utc::now().date_naive();
    let date = today.to_string();
    let deadline = today
        .checked_add_months(chrono::Months::new(3))
        .expect("deadline")
        .to_string();
    let persisted = content(
        &server
            .forecast_persist(Parameters(persist_request("future", &date, Some(0.8))))
            .await
            .expect("declare"),
    );
    assert_eq!(persisted["scoring_eligible"], true);
    assert_eq!(persisted["deadline"], deadline);
    let error = server
        .forecast_record(Parameters(record_request(Some("future"), &date, &deadline)))
        .await
        .expect_err("early outcome");
    assert!(error.to_string().contains("horizon"));
    let mut request = record_request(Some("future"), &date, &deadline);
    request.forecast_price_change = 0.5;
    assert!(
        server
            .forecast_record(Parameters(request))
            .await
            .expect_err("target frozen")
            .to_string()
            .contains("frozen")
    );
    let mut request = record_request(Some("future"), &date, &deadline);
    request.horizon = "1yr".into();
    assert!(server.forecast_record(Parameters(request)).await.is_err());
    let listed = content(
        &server
            .forecast_list(Parameters(EquityForecastListRequest {
                symbol: "TEST".into(),
            }))
            .await
            .expect("history"),
    );
    assert!(listed["forecasts"][0]["outcome"].is_null());
    assert_eq!(listed["forecasts"][0]["forecast_probability"], 0.8);
}

#[tokio::test]
async fn equity_backdated_and_identityless_observations_are_unscored() {
    let server = server();
    let admitted = content(
        &server
            .forecast_persist(Parameters(persist_request(
                "legacy",
                "2020-01-15",
                Some(0.8),
            )))
            .await
            .expect("history"),
    );
    assert_eq!(admitted["scoring_eligible"], false);
    for id in [Some("legacy"), None] {
        let body = content(
            &server
                .forecast_record(Parameters(record_request(id, "2020-01-15", "2020-04-15")))
                .await
                .expect("observation"),
        );
        assert_eq!(body["scoring_status"], "unscored");
        assert!(body["brier"]["combined"].is_null());
        assert!(body["unscored_reason"].is_string());
        assert!(body["gaps"]["return_gap"].is_number());
    }
    // An identityless observation does not manufacture a declared forecast.
    let history = content(
        &server
            .forecast_list(Parameters(EquityForecastListRequest {
                symbol: "TEST".into(),
            }))
            .await
            .expect("list"),
    );
    assert_eq!(history["forecasts"].as_array().expect("history").len(), 1);
    server
        .forecast_record(Parameters(record_request(
            Some("legacy"),
            "2020-01-15",
            "2020-04-15",
        )))
        .await
        .expect("idempotent replay");
    let mut conflicting = record_request(Some("legacy"), "2020-01-15", "2020-04-15");
    conflicting.actual_price_change = -0.4;
    assert!(
        server
            .forecast_record(Parameters(conflicting))
            .await
            .expect_err("conflicting replay")
            .to_string()
            .contains("conflicting")
    );
}
