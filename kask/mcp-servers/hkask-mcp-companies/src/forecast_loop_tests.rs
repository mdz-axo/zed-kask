//! Loop-closure tests for the durable forecast loop:
//! forecast_persist → forecast_record → read-back.
//!
//! Layer assignment (`kask/docs/reference/testing-protocol.md`): loop-closure
//! — write → observe → recall through the real tool seam over an in-memory
//! store, with degradation surfaced, never empty-equals-success. Phase 0
//! found this persistence loop implemented but untested (a write path with no
//! recall-path test is a loop silently dropped); this suite is Batch 1 of the
//! propagation plan (`tasks/kask-testing-propagation-plan.md`).

use super::acquisition_tests::server;
use super::*;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

fn content(output: &str) -> Value {
    serde_json::from_str::<Value>(output).expect("tool JSON")["content"].clone()
}

fn persist_request(forecast_id: &str) -> types::ForecastPersistRequest {
    types::ForecastPersistRequest {
        symbol: "ACME.US".into(),
        forecast_date: "2026-01-15".into(),
        horizon: types::Horizon::SixMo,
        forecast_price_change: Some(0.10),
        forecast_multiple: Some(12.0),
        forecast_price: Some(110.0),
        current_price: Some(100.0),
        forecast_probability: Some(0.6),
        revision_of: None,
        forecast_id: Some(forecast_id.into()),
    }
}

/// expect: a persisted pre-computed price target reads back complete through
/// forecast_get and forecast_list — every persisted field survives the
/// write→recall round trip.
#[tokio::test]
async fn forecast_persist_records_and_reads_back() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let server = server(directory.path());

    let persisted = content(
        &server
            .forecast_persist(Parameters(persist_request("loop-persist-1")))
            .await
            .expect("forecast_persist tool"),
    );
    assert_eq!(persisted["status"], json!("persisted"));
    assert_eq!(persisted["forecast_id"], json!("loop-persist-1"));

    let read = content(
        &server
            .forecast_get(Parameters(types::ForecastGetRequest {
                forecast_id: "loop-persist-1".into(),
            }))
            .await
            .expect("forecast_get tool"),
    );
    assert_eq!(read["id"], json!("loop-persist-1"));
    assert_eq!(read["symbol"], json!("ACME.US"));
    assert_eq!(read["revision_of"], json!(null));
    assert_eq!(
        read["outcomes"],
        json!([]),
        "freshly persisted forecast has no outcomes yet"
    );
    assert!(read["created_at"].as_str().is_some_and(|s| !s.is_empty()));
    let snapshot = &read["snapshot"];
    assert_eq!(snapshot["kind"], json!("precomputed_price_target"));
    assert_eq!(snapshot["symbol"], json!("ACME.US"));
    assert_eq!(snapshot["forecast_date"], json!("2026-01-15"));
    assert_eq!(snapshot["horizon"], json!("6mo"));
    assert_eq!(snapshot["forecast_multiple"], json!(12.0));
    assert_eq!(snapshot["forecast_price"], json!(110.0));
    assert_eq!(snapshot["current_price"], json!(100.0));
    assert_eq!(snapshot["forecast_price_change"], json!(0.10));
    assert_eq!(snapshot["forecast_probability"], json!(0.6));

    let listed = content(
        &server
            .forecast_list(Parameters(types::ForecastListRequest {
                symbol: "ACME.US".into(),
            }))
            .await
            .expect("forecast_list tool"),
    );
    assert_eq!(listed["symbol"], json!("ACME.US"));
    assert_eq!(
        listed["forecasts"].as_array().map(Vec::len),
        Some(1),
        "the persisted forecast must appear in its symbol's list"
    );
    assert_eq!(listed["forecasts"][0]["id"], json!("loop-persist-1"));
}

/// expect: forecast_record closes the loop — the outcome with its Brier score
/// becomes durable and reads back. The Brier must be scored against the
/// forecast's OWN stored probability (0.6 → (1−0.6)² = 0.16), not the 0.7
/// fallback. Falsifier: a silent no-op forecast_record fails the outcomes
/// assertions below.
#[tokio::test]
async fn forecast_record_closes_the_loop() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let server = server(directory.path());
    server
        .forecast_persist(Parameters(persist_request("loop-closure-1")))
        .await
        .expect("persist before record");

    // Actual return equals the forecast — inside the 20% tolerance, so
    // return_accurate is true and Brier = (1 − 0.6)².
    let recorded = content(
        &server
            .forecast_record(Parameters(types::ForecastRecordRequest {
                symbol: "ACME.US".into(),
                forecast_date: "2026-01-15".into(),
                horizon: types::Horizon::SixMo,
                forecast_multiple: 12.0,
                forecast_price_change: 0.10,
                outcome_date: "2026-07-15".into(),
                actual_multiple: 12.5,
                actual_price_change: 0.10,
                forecast_id: Some("loop-closure-1".into()),
            }))
            .await
            .expect("forecast_record tool"),
    );
    assert_eq!(recorded["status"], json!("recorded"));
    assert_eq!(recorded["forecast_id"], json!("loop-closure-1"));
    let brier = recorded["brier"]["return_accuracy"]
        .as_f64()
        .expect("brier score in the record response");
    assert!(
        (brier - 0.16).abs() < 1e-9,
        "brier must be (1−0.6)² = 0.16 against the stored probability, got {brier}"
    );

    let read = content(
        &server
            .forecast_get(Parameters(types::ForecastGetRequest {
                forecast_id: "loop-closure-1".into(),
            }))
            .await
            .expect("read-back after record"),
    );
    let outcomes = read["outcomes"].as_array().expect("outcomes array");
    assert_eq!(
        outcomes.len(),
        1,
        "the recorded outcome must be durable — the loop closes"
    );
    let outcome = &outcomes[0];
    assert_eq!(outcome["actual_price_change"], json!(0.10));
    assert_eq!(outcome["actual_multiple"], json!(12.5));
    assert_eq!(outcome["outcome_date"], json!("2026-07-15"));
    assert_eq!(outcome["horizon"], json!("6mo"));
    let stored_brier = outcome["return_brier"]
        .as_f64()
        .expect("stored brier in the outcome");
    assert!((stored_brier - 0.16).abs() < 1e-9);
    assert!(
        outcome["recorded_at"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
}

/// expect: degradation is surfaced, never silent. forecast_record and
/// forecast_get on an unknown forecast fail loudly with the typed error
/// naming it; forecast_list for a symbol with no forecasts returns surfaced
/// empty — never fabricated zeros or entries.
#[tokio::test]
async fn forecast_degradation_is_surfaced_never_silent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let server = server(directory.path());

    let record_error = server
        .forecast_record(Parameters(types::ForecastRecordRequest {
            symbol: "ACME.US".into(),
            forecast_date: "2026-01-15".into(),
            horizon: types::Horizon::SixMo,
            forecast_multiple: 12.0,
            forecast_price_change: 0.10,
            outcome_date: "2026-07-15".into(),
            actual_multiple: 12.0,
            actual_price_change: 0.10,
            forecast_id: Some("no-such-forecast".into()),
        }))
        .await
        .expect_err("recording against an unknown forecast must fail loudly, not no-op");
    let text = record_error.to_string();
    assert!(
        text.contains("forecast not found"),
        "the typed error must name the missing forecast, got: {text}"
    );

    server
        .forecast_get(Parameters(types::ForecastGetRequest {
            forecast_id: "no-such-forecast".into(),
        }))
        .await
        .expect_err("reading an unknown forecast must surface a typed error");

    let listed = content(
        &server
            .forecast_list(Parameters(types::ForecastListRequest {
                symbol: "NOFC.US".into(),
            }))
            .await
            .expect("forecast_list tool"),
    );
    assert_eq!(listed["symbol"], json!("NOFC.US"));
    assert_eq!(
        listed["forecasts"],
        json!([]),
        "no forecasts is surfaced as an empty list — never fabricated zeros or entries"
    );
}

/// expect: the degraded no-probability path is pinned — a forecast persisted
/// without forecast_probability is surfaced at write time (the persist note)
/// and forecast_record scores against the documented 0.7 fallback prior
/// (Brier (1−0.7)² = 0.09), never an invented probability.
#[tokio::test]
async fn forecast_record_without_probability_uses_pinned_fallback() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let server = server(directory.path());
    let persisted = content(
        &server
            .forecast_persist(Parameters(types::ForecastPersistRequest {
                symbol: "ACME.US".into(),
                forecast_date: "2026-01-15".into(),
                horizon: types::Horizon::SixMo,
                forecast_price_change: Some(0.10),
                forecast_multiple: Some(12.0),
                forecast_probability: None,
                revision_of: None,
                forecast_id: Some("loop-fallback-1".into()),
                forecast_price: None,
                current_price: None,
            }))
            .await
            .expect("forecast_persist tool"),
    );
    let note = persisted["note"].as_str().expect("persist response note");
    assert!(
        note.contains("WITHOUT forecast_probability"),
        "the degradation must be surfaced at write time, got: {note}"
    );

    let recorded = content(
        &server
            .forecast_record(Parameters(types::ForecastRecordRequest {
                symbol: "ACME.US".into(),
                forecast_date: "2026-01-15".into(),
                horizon: types::Horizon::SixMo,
                forecast_multiple: 12.0,
                forecast_price_change: 0.10,
                outcome_date: "2026-07-15".into(),
                actual_multiple: 12.0,
                actual_price_change: 0.10,
                forecast_id: Some("loop-fallback-1".into()),
            }))
            .await
            .expect("forecast_record tool"),
    );
    let brier = recorded["brier"]["return_accuracy"]
        .as_f64()
        .expect("brier on the fallback path");
    assert!(
        (brier - 0.09).abs() < 1e-9,
        "the fallback must be the pinned 0.7 prior: (1−0.7)² = 0.09, got {brier}"
    );
}
