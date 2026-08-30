//! The ```` ```portfolio ```` block body model + parser.
//!
//! The shape combines the four `companies` MCP server tool responses
//! (`portfolio_list`, `portfolio_returns`, `portfolio_characteristics`,
//! `portfolio_attribution`) into one JSON body emitted by the curator agent.
//! Fields are optional / defaulted so the parser is tolerant of partial bodies
//! and never fails on media-shaped JSON (which has no `viz` field).
//!
//! FIBO concept URI constants anchor displayed metrics to the FIBO ontology;
//! they match the `fibo` map entries the MCP server includes in its responses.
//! Only verified FIBO terms are re-exported — metrics without a real FIBO
//! concept carry no tag rather than an invented URI.

use std::collections::HashMap;

use hkask_tool_invoker::BlockProvenance;
use serde::Deserialize;

// Re-export the verified FIBO constants from the shared
// `hkask_bridge_ontology` crate — the single source of truth — so existing
// call sites (`crate::block::FIBO_*`) keep resolving. These are NOT
// duplicated here.
pub use hkask_bridge_ontology::fibo::{
    INTERNAL_RATE_OF_RETURN as FIBO_INTERNAL_RATE_OF_RETURN, PORTFOLIO as FIBO_PORTFOLIO,
};

/// The discriminator-tagged body of a ```` ```portfolio ```` block.
///
/// `viz` selects the renderer; `"portfolio"` renders the dashboard. The agent
/// (curator) calls the MCP tools and emits the combined result as a fenced
/// block.
#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    /// The portfolio name this dashboard describes (the agent picks the
    /// portfolio; the block body already carries its data).
    #[serde(default)]
    pub portfolio: Option<String>,
    /// Returns summary — mirrors `portfolio_returns`. Optional so bodies
    /// without returns still render the characteristics/attribution sections.
    #[serde(default)]
    pub returns: Option<ReturnsBody>,
    /// Materialized holdings — mirrors `portfolio_snapshot` from
    /// `hkask-mcp-portfolio`. Present for any portfolio type (stock,
    /// prediction-event, CMP index). Optional so bodies without holdings
    /// still render the returns/characteristics sections.
    #[serde(default)]
    pub holdings: Option<HoldingsBody>,
    /// Field-name → characteristic, mirrors `portfolio_characteristics`.
    /// Defaults to empty when absent.
    #[serde(default)]
    pub characteristics: HashMap<String, CharacteristicField>,
    /// Attribution ranking rows, mirrors `portfolio_attribution`. Defaults to
    /// empty when absent.
    #[serde(default)]
    pub attribution: Vec<AttributionRow>,
    /// Server-authoritative provenance for re-issuing the originating MCP tool
    /// with modified args (T5). `#[serde(default)]` so bodies emitted before
    /// provenance landed parse with an empty (non-dispatchable) provenance and
    /// the widget falls back to its read-only display.
    #[serde(default)]
    pub provenance: BlockProvenance,
    /// Ontology concept URI (e.g. `fibo:Portfolio`, `fibo:Corporation`).
    /// Emitted by the companies server as the top-level `"ontology"` key.
    /// Drives the "Explain" affordance's tool selection (the "I" pattern).
    /// `None` on older blocks → the widget falls back to `research_search`.
    #[serde(default)]
    pub ontology: Option<String>,
}

/// Returns summary mirroring the `portfolio_returns` tool response. All numeric
/// fields default so partial bodies parse without error.
#[derive(Debug, Clone, Deserialize)]
pub struct ReturnsBody {
    #[serde(default)]
    pub portfolio: Option<String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub total_return: f64,
    #[serde(default)]
    pub modified_dietz: f64,
    #[serde(default)]
    pub irr: f64,
    #[serde(default)]
    pub irr_converged: bool,
    #[serde(default)]
    pub start_value: f64,
    #[serde(default)]
    pub end_value: f64,
    #[serde(default)]
    pub net_cash_flows: f64,
    #[serde(default)]
    pub cash_flow_count: usize,
    #[serde(default)]
    pub positions_at_start: usize,
    #[serde(default)]
    pub positions_at_end: usize,
}

/// Materialized holdings mirroring the `portfolio_snapshot` tool response
/// from `hkask-mcp-portfolio`. Renders for any portfolio type (stock,
/// prediction-event, CMP index). All fields default so partial bodies parse.
#[derive(Debug, Clone, Deserialize)]
pub struct HoldingsBody {
    #[serde(default)]
    pub portfolio: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub holdings: Vec<HoldingRow>,
    #[serde(default)]
    pub cash_balance: f64,
    #[serde(default)]
    pub transaction_count: usize,
    #[serde(default)]
    pub issues: Vec<String>,
}

/// One holding row in a materialized holdings snapshot. `asset_type`
/// discriminates stocks, prediction contracts, and nested portfolios.
#[derive(Debug, Clone, Deserialize)]
pub struct HoldingRow {
    pub symbol: String,
    #[serde(default)]
    pub asset_type: Option<String>,
    #[serde(default)]
    pub shares: f64,
    #[serde(default)]
    pub total_buys: f64,
    #[serde(default)]
    pub total_sells: f64,
    #[serde(default)]
    pub cost_basis: f64,
}

/// A single characteristic field from `portfolio_characteristics`.
#[derive(Debug, Clone, Deserialize)]
pub struct CharacteristicField {
    #[serde(default)]
    pub value: Option<f64>,
    /// Internal metric identifier (hKask canonical metric name — not an
    /// ontology URI). `alias = "fibo"` keeps blocks emitted before the
    /// 2026-08-29 FIBO remediation parsing.
    #[serde(default, alias = "fibo")]
    pub metric: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub holdings: Option<usize>,
}

/// A single attribution row from `portfolio_attribution`. `symbol` is required
/// (a row without a symbol is meaningless); other fields default so partial
/// rows still render.
#[derive(Debug, Clone, Deserialize)]
pub struct AttributionRow {
    pub symbol: String,
    #[serde(default)]
    pub weight_start_pct: f64,
    #[serde(default)]
    pub weight_end_pct: f64,
    #[serde(default)]
    pub security_return_pct: f64,
    #[serde(default)]
    pub contribution_bps: f64,
    #[serde(default)]
    pub gain_loss: f64,
}

/// Parse a ```` ```portfolio ```` block body. Tolerant: missing `viz`/`returns`/
/// `characteristics`/`attribution` default to `None`/empty rather than erroring,
/// so media-shaped JSON parses (and is then rejected by the renderer on the
/// `viz` check) instead of being logged as a malformed portfolio block.
pub fn parse_portfolio_body(body: &str) -> anyhow::Result<PortfolioBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_body() {
        let body = r#"{
            "viz": "portfolio",
            "portfolio": "main",
            "returns": {
                "portfolio": "main",
                "from": "2000-01-01",
                "to": "2024-12-01",
                "total_return": 0.15,
                "modified_dietz": 0.14,
                "irr": 0.12,
                "irr_converged": true,
                "start_value": 100000.0,
                "end_value": 115000.0,
                "net_cash_flows": 5000.0,
                "cash_flow_count": 3,
                "positions_at_start": 0,
                "positions_at_end": 3
            },
            "characteristics": {
                "pe_ratio": { "value": 15.2, "fibo": "fibo-...", "holdings": 3 }
            },
            "attribution": [
                { "symbol": "AAPL", "weight_start_pct": 10.0, "weight_end_pct": 12.0,
                  "security_return_pct": 5.0, "contribution_bps": 60.0, "gain_loss": 600.0 }
            ]
        }"#;
        let parsed = parse_portfolio_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("portfolio"));
        assert_eq!(parsed.portfolio.as_deref(), Some("main"));
        let returns = parsed.returns.expect("returns present");
        assert!((returns.total_return - 0.15).abs() < 1e-9);
        assert!(returns.irr_converged);
        assert_eq!(returns.positions_at_end, 3);
        assert_eq!(parsed.characteristics.len(), 1);
        assert_eq!(parsed.attribution.len(), 1);
        assert_eq!(parsed.attribution[0].symbol, "AAPL");
    }

    #[test]
    fn parses_minimal_body_with_only_viz() {
        let body = r#"{"viz":"portfolio"}"#;
        let parsed = parse_portfolio_body(body).expect("minimal body parses");
        assert_eq!(parsed.viz.as_deref(), Some("portfolio"));
        assert!(parsed.returns.is_none());
        assert!(parsed.characteristics.is_empty());
        assert!(parsed.attribution.is_empty());
    }

    #[test]
    fn media_shaped_body_parses_with_no_viz() {
        // A media-shaped body has no `viz` field → parses (viz None) and is
        // rejected by the renderer on the `viz` check, not logged as malformed.
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = parse_portfolio_body(media).expect("media body parses");
        assert_ne!(parsed.viz.as_deref(), Some("portfolio"));
    }

    #[test]
    fn non_json_body_fails_to_parse() {
        assert!(parse_portfolio_body("not json").is_err());
    }

    #[test]
    fn attribution_row_requires_symbol() {
        // A row without a symbol fails to deserialize.
        let body = r#"{"viz":"portfolio","attribution":[{"weight_start_pct":1.0}]}"#;
        assert!(parse_portfolio_body(body).is_err());
    }

    #[test]
    fn provenance_defaults_empty_when_absent() {
        // A body emitted before provenance lands has no `provenance` key.
        // Adding the field is non-breaking: provenance defaults empty and is
        // not dispatchable (T5 contract).
        let body = parse_portfolio_body(r#"{"viz":"portfolio"}"#).expect("valid body");
        assert!(!body.provenance.is_dispatchable());
        assert!(body.provenance.tool.is_none());
        assert!(body.provenance.server.is_none());
    }

    #[test]
    fn provenance_parses_when_present() {
        let json = r#"{"viz":"portfolio","provenance":{"tool":"portfolio_returns","server":"hkask-mcp-companies","args":{"portfolio":"main","from":"2020-01-01","to":"2024-12-31"}}}"#;
        let body = parse_portfolio_body(json).expect("valid body");
        assert!(body.provenance.is_dispatchable());
        assert_eq!(body.provenance.tool.as_deref(), Some("portfolio_returns"));
        assert_eq!(
            body.provenance.server.as_deref(),
            Some("hkask-mcp-companies")
        );
        assert_eq!(body.provenance.args["portfolio"], "main");
        assert_eq!(body.provenance.args["from"], "2020-01-01");
    }

    #[test]
    fn holdings_body_parses() {
        // A CMP index portfolio body with materialized holdings from
        // `portfolio_snapshot` on `hkask-mcp-portfolio`.
        let json = r#"{
            "viz": "portfolio",
            "portfolio": "cmp:KXFEDDECISION",
            "holdings": {
                "portfolio": "cmp:KXFEDDECISION",
                "date": "2024-01-15",
                "holdings": [
                    {"symbol": "cmp:KXFEDDECISION:30d", "asset_type": "prediction_contract", "shares": 1.0, "cost_basis": 0.58},
                    {"symbol": "cmp:KXFEDDECISION:90d", "asset_type": "prediction_contract", "shares": 1.0, "cost_basis": 0.55}
                ],
                "cash_balance": 0.0,
                "transaction_count": 2,
                "issues": []
            }
        }"#;
        let body = parse_portfolio_body(json).expect("valid body");
        let holdings = body.holdings.expect("holdings present");
        assert_eq!(holdings.holdings.len(), 2);
        assert_eq!(holdings.holdings[0].symbol, "cmp:KXFEDDECISION:30d");
        assert_eq!(
            holdings.holdings[0].asset_type.as_deref(),
            Some("prediction_contract")
        );
        assert!((holdings.holdings[0].cost_basis - 0.58).abs() < 1e-9);
        assert_eq!(holdings.transaction_count, 2);
    }

    #[test]
    fn holdings_body_defaults_empty_when_absent() {
        let body = parse_portfolio_body(r#"{"viz":"portfolio"}"#).expect("valid body");
        assert!(body.holdings.is_none());
    }

    #[test]
    fn block_body_has_ontology_field() {
        // S4 sensor: the companies server emits `"ontology": "fibo:Portfolio"`
        // (or another FIBO concept) on every tool output. The widget's
        // PortfolioBlockBody MUST have an `ontology` field to receive it —
        // if this field is absent, the server's tag is silently dropped.
        let json = r#"{"viz":"portfolio","ontology":"fibo:Portfolio"}"#;
        let body = parse_portfolio_body(json).expect("valid body");
        assert_eq!(
            body.ontology.as_deref(),
            Some("fibo:Portfolio"),
            "PortfolioBlockBody must parse the ontology field the server emits"
        );
    }
}
