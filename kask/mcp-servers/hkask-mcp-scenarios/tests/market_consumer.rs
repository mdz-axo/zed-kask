//! T7: zero-edit consumption path — a prediction-market record flows into the
//! existing scenarios tools as caller-supplied JSON. No scenarios-server code
//! changes; this test pins that the seams accept market-anchored inputs.

use hkask_mcp_scenarios::superforecast;
use hkask_mcp_scenarios::types::Perspective;

/// A minimal annotated record as returned by hkask-mcp-prediction-markets.
/// Constructed via JSON to exercise the actual wire shape.
fn market_record_json() -> serde_json::Value {
    serde_json::json!({
        "source": "kalshi",
        "event_id": "KXFED-27DEC",
        "market_id": "KXFED-27DEC-H0",
        "question": "Will the Fed hold rates in December 2027?",
        "description": "Resolves per the Federal Reserve statement.",
        "category": "Economics",
        "series": "KXFEDDECISION",
        "deadline": "2027-12-08T18:59:00Z",
        "probability": 0.72,
        "probability_method": "midpoint",
        "spread": 0.02,
        "volume": 250000.0,
        "volume_grain": "market",
        "liquidity": 10000.0,
        "open_interest": 1500.0,
        "last_update": "2026-08-05T00:00:00Z",
        "volatility": {
            "realized_variance": null,
            "structural_flag": "none",
            "interpretation": "low"
        },
        "status": "open",
        "resolved_outcome": null,
        "resolution_source": "kalshi_exchange",
        "calibration": {
            "brier": 0.18,
            "domain_bias": null,
            "sample_size": 42,
            "stale": false
        },
        "reliability_tier": "high",
        "ontology": {
            "process": {
                "type": "pko:ProcedureExecution",
                "stage": "trading",
                "probability_role": "pko:StepExecution.output"
            },
            "state": {
                "identifier": "kalshi:KXFED-27DEC-H0",
                "title": "t",
                "description": "d",
                "temporal": "2027-12-08T18:59:00Z",
                "provenance": "kalshi_exchange"
            },
            "mapping_version": 1
        }
    })
}

#[test]
fn market_probability_feeds_cross_validate_divergence() {
    let record = market_record_json();
    let market_p = record["probability"].as_f64().expect("probability");

    // The agent's LLM forecast vs the market anchor — the quantitative check.
    let result = superforecast::cross_validate(
        "fed-dec-2027",
        "llm_forecast",
        0.55,
        &[],
        "market:kalshi",
        market_p,
        &[],
        None,
    );
    assert!((result.divergence - 0.17).abs() < 1e-9);
    assert!(result.requires_review, "0.17 divergence exceeds the 0.15 threshold");
}

#[test]
fn market_perspective_flows_through_synthesize() {
    let record = market_record_json();
    let market_perspective = Perspective {
        source: format!(
            "market:{}",
            record["source"].as_str().expect("source")
        ),
        probability: record["probability"].as_f64().expect("probability"),
        fermi_sub_questions: vec![],
        base_rate: None,
        reference_class: Some(record["series"].as_str().expect("series").to_string()),
        rationale: Some("market-implied probability".into()),
        historical_brier: record["calibration"]["brier"].as_f64(),
    };
    let llm_perspective = Perspective {
        source: "llm_forecast".into(),
        probability: 0.55,
        fermi_sub_questions: vec![],
        base_rate: Some(0.5),
        reference_class: None,
        rationale: None,
        historical_brier: Some(0.22),
    };

    let synthesis = superforecast::synthesize_perspectives(
        "fed-dec-2027",
        &[llm_perspective, market_perspective],
    )
    .expect("synthesizes");

    // The market's lower Brier (0.18 vs 0.22) must earn it the larger weight.
    let market_weight = synthesis
        .perspective_weights
        .iter()
        .find(|(source, _)| source == "market:kalshi")
        .map(|(_, w)| *w)
        .expect("market perspective has a weight");
    assert!(
        market_weight > 0.5,
        "market with better Brier should dominate, weight {market_weight}"
    );
}
