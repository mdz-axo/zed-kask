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

use std::collections::HashMap;

use serde::Deserialize;

// ── FIBO concept URIs (from `hkask-mcp-companies/src/fibo.rs`) ────────────
/// Anchors displayed metrics to the FIBO ontology. These match the `fibo` map
/// entries the MCP server includes in its responses.
pub const FIBO_PORTFOLIO: &str = "fibo-sec-sec-ast:Portfolio";
pub const FIBO_TRANSACTION_LEDGER: &str = "fibo-sec-sec-ast:TransactionLedger";
pub const FIBO_TIME_WEIGHTED_RETURN: &str = "fibo-fbc-fct-ra:TimeWeightedReturn";
pub const FIBO_INTERNAL_RATE_OF_RETURN: &str = "fibo-fbc-fct-ra:InternalRateOfReturn";

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
    /// Field-name → characteristic, mirrors `portfolio_characteristics`.
    /// Defaults to empty when absent.
    #[serde(default)]
    pub characteristics: HashMap<String, CharacteristicField>,
    /// Attribution ranking rows, mirrors `portfolio_attribution`. Defaults to
    /// empty when absent.
    #[serde(default)]
    pub attribution: Vec<AttributionRow>,
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

/// A single characteristic field from `portfolio_characteristics`.
#[derive(Debug, Clone, Deserialize)]
pub struct CharacteristicField {
    #[serde(default)]
    pub value: Option<f64>,
    #[serde(default)]
    pub fibo: Option<String>,
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
}
