//! The ```` ```scenarios ```` block body model + parser.
//!
//! Mirrors the `scenario_status` MCP tool response from `hkask-mcp-scenarios`
//! (pipeline overview, calibration summary, event tree summary, recent
//! forecasts). Fields are optional / defaulted so the parser is tolerant of
//! partial bodies and never fails on other-shaped JSON (which has no `viz`
//! field matching `"scenarios"`).

use hkask_tool_invoker::BlockProvenance;
use serde::Deserialize;

// ── FIBO / methodology anchors ────────────────────────────────────────────
pub const FIBO_FORECAST_ID: &str = "fibo-fbc-fct-ra:ForecastIdentifier";
pub const FIBO_BRIER_SCORE: &str = "fibo-fbc-fct-ra:BrierScore";
pub const FIBO_SCENARIO_PROBABILITY: &str = "fibo-fbc-fct-ra:ScenarioProbability";

/// The discriminator-tagged body of a ```` ```scenarios ```` block.
///
/// `viz` selects the renderer; `"scenarios"` renders the pipeline/matrix/
/// sensitivity/timeline dashboard. The event-tree DAG is handled separately
/// by `hkask-graph-widget` (`viz: "event_tree"`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScenariosBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    #[serde(default)]
    pub pipeline: PipelineOverview,
    #[serde(default)]
    pub calibration: Option<CalibrationSummary>,
    #[serde(default)]
    pub event_tree: Option<EventTreeSummary>,
    #[serde(default)]
    pub recent_forecasts: Vec<RecentForecast>,
    /// Server-authoritative provenance for re-issuing the originating MCP tool
    /// with modified args (T3/T4). `#[serde(default)]` so bodies emitted before
    /// provenance landed parse with an empty (non-dispatchable) provenance and
    /// the widget falls back to its hardcoded dispatch.
    #[serde(default)]
    pub provenance: BlockProvenance,
    /// Ontology anchor emitted by the scenarios server ("pko" or "dublin-core").
    /// The widget carries it so the compose-back body can reference it and a
    /// future "explain this scenario" affordance can dispatch on it. `None`
    /// on older blocks or when the server doesn't emit the anchor.
    #[serde(default)]
    pub ontology_anchor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PipelineOverview {
    #[serde(default)]
    pub forecast_count: usize,
    #[serde(default)]
    pub resolved_count: usize,
    #[serde(default)]
    pub pending_count: usize,
    #[serde(default)]
    pub overall_brier: Option<f64>,
    #[serde(default)]
    pub recent_forecasts: Vec<RecentForecast>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecentForecast {
    #[serde(default)]
    pub forecast_id: String,
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub event_name: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub probability: f64,
    #[serde(default)]
    pub outcome: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalibrationSummary {
    #[serde(default)]
    pub total_forecasts: usize,
    #[serde(default)]
    pub resolved_forecasts: usize,
    #[serde(default)]
    pub overall_brier: Option<f64>,
    #[serde(default)]
    pub overconfidence_score: Option<f64>,
    #[serde(default)]
    pub interpretation: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventTreeSummary {
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub event_count: usize,
    #[serde(default)]
    pub joint_probability: Option<f64>,
    #[serde(default)]
    pub root_ids: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<EventNode>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventNode {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub probability: Option<f64>,
    #[serde(default)]
    pub marginal_probability: Option<f64>,
    #[serde(default)]
    pub certainty_tier: Option<serde_json::Value>,
    #[serde(default)]
    pub basis: Option<String>,
    #[serde(default)]
    pub parent_ids: Vec<String>,
    #[serde(default)]
    pub sub_question_count: Option<usize>,
    #[serde(default)]
    pub has_base_rate: Option<bool>,
    #[serde(default)]
    pub brier_score: Option<f64>,
}

/// Parse a ```` ```scenarios ```` block body. Tolerant: missing `viz`/`nodes`
/// default to `None`/empty rather than erroring, so other-shaped JSON (media,
/// graph) parses (and is then rejected by the renderer on the `viz` check).
pub fn parse_scenarios_body(body: &str) -> anyhow::Result<ScenariosBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_body() {
        let json = r#"{
            "viz": "scenarios",
            "pipeline": {
                "forecast_count": 5, "resolved_count": 2, "pending_count": 3,
                "overall_brier": 0.15, "recent_forecasts": []
            },
            "calibration": {
                "total_forecasts": 5, "resolved_forecasts": 2,
                "overall_brier": 0.15, "overconfidence_score": 0.03,
                "interpretation": "good"
            },
            "event_tree": {
                "subject": "AAPL", "event_count": 2, "joint_probability": 0.12,
                "root_ids": ["e1"],
                "nodes": [
                    {"id":"e1","name":"Rev","probability":0.7,"parent_ids":[]},
                    {"id":"e2","name":"Margin","probability":0.4,"parent_ids":["e1"]}
                ]
            }
        }"#;
        let body = parse_scenarios_body(json).expect("valid body");
        assert_eq!(body.viz.as_deref(), Some("scenarios"));
        assert_eq!(body.pipeline.forecast_count, 5);
        assert!(body.calibration.is_some());
        let tree = body.event_tree.unwrap();
        assert_eq!(tree.nodes.len(), 2);
    }

    #[test]
    fn parses_minimal_body() {
        let json = r#"{"viz":"scenarios"}"#;
        let body = parse_scenarios_body(json).expect("minimal body");
        assert_eq!(body.viz.as_deref(), Some("scenarios"));
        assert_eq!(body.pipeline.forecast_count, 0);
        assert!(body.calibration.is_none());
    }

    #[test]
    fn media_body_has_no_viz_scenarios() {
        let json = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let body = parse_scenarios_body(json).expect("json parses");
        assert_ne!(body.viz.as_deref(), Some("scenarios"));
    }

    #[test]
    fn event_tree_body_not_claimed() {
        let json = r#"{"viz":"event_tree","nodes":[{"id":"e0"}]}"#;
        let body = parse_scenarios_body(json).expect("json parses");
        assert_ne!(body.viz.as_deref(), Some("scenarios"));
    }

    #[test]
    fn non_json_fails() {
        assert!(parse_scenarios_body("not json").is_err());
    }

    #[test]
    fn provenance_defaults_empty_when_absent() {
        // A body emitted before provenance lands has no `provenance` key.
        // Adding the field is non-breaking: provenance defaults empty and is
        // not dispatchable (T3 contract).
        let body = parse_scenarios_body(r#"{"viz":"scenarios"}"#).expect("valid body");
        assert!(!body.provenance.is_dispatchable());
        assert!(body.provenance.tool.is_none());
        assert!(body.provenance.server.is_none());
    }

    #[test]
    fn provenance_parses_when_present() {
        let json = r#"{"viz":"scenarios","provenance":{"tool":"scenario_quantify","server":"hkask-mcp-scenarios","args":{"event_id":"e1"}}}"#;
        let body = parse_scenarios_body(json).expect("valid body");
        assert!(body.provenance.is_dispatchable());
        assert_eq!(body.provenance.tool.as_deref(), Some("scenario_quantify"));
        assert_eq!(
            body.provenance.server.as_deref(),
            Some("hkask-mcp-scenarios")
        );
        assert_eq!(body.provenance.args["event_id"], "e1");
    }
}
