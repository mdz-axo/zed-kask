//! Request types for hkask-mcp-companies MCP tools.
//!
//! Extracted from main.rs — these are the tool input structs that derive
//! Deserialize + JsonSchema for MCP parameter deserialization.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Financial data request structs ──────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolRequest {
    pub symbol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolLimitRequest {
    pub symbol: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HistoricalRequest {
    pub symbol: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

// ── Portfolio request structs ─────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioNameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransactionNoteRequest {
    pub portfolio: String,
    pub tx_id: String,
    pub note: String,
}

/// Ledger import/export format.
#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    Csv,
    Json,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerImportRequest {
    pub portfolio: String,
    pub format: ImportFormat,
    pub data: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LedgerExportRequest {
    pub portfolio: String,
    pub format: ImportFormat,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioCompareRequest {
    pub portfolio_a: String,
    pub portfolio_b: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AttributionRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CharacteristicsRequest {
    pub portfolio: String,
    pub date: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExpectationsGapRequest {
    pub symbol: String,
    pub target_return: Option<f64>,
    /// Your estimate of sustainable revenue growth (0.0–1.0).
    /// Compared against market-implied growth and management guidance.
    pub growth_estimate: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PortfolioReturnsRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteAddRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteListRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteDeleteRequest {
    pub note_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileAttachRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date: String,
    pub filename: String,
    pub mime_type: String,
    /// Base64-encoded file content
    pub data: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileListRequest {
    pub portfolio: String,
    pub symbol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileDeleteRequest {
    pub file_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResultFeedbackRequest {
    /// Which tool produced the result being rated
    pub tool: String,
    /// The query that was used (symbol, portfolio name, search query, etc.)
    pub query: String,
    /// 1–5 satisfaction score (5 = exceeded expectations, 1 = completely missed)
    /// Omit if you just want to leave comments without a score.
    pub score: Option<u8>,
    /// Free-text comments about what worked, what didn't, or what was missing.
    /// Omit if you just want to leave a score without comments.
    #[serde(default)]
    pub comments: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DcfValuationRequest {
    pub symbol: String,
    /// Optional parent forecast ID for a same-symbol revision.
    pub revision_of: Option<String>,
    /// Stage 1 years (1–3, default 3)
    #[schemars(range(min = 1, max = 3))]
    pub stage1_years: Option<u8>,
    /// Stage 2 years (2–7, default 7)
    #[schemars(range(min = 2, max = 7))]
    pub stage2_years: Option<u8>,
    /// Discount rate / WACC (0.05–0.30, default 0.10)
    #[schemars(range(min = 0.05, max = 0.30))]
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (0.00–0.10, default 0.025; must be below discount rate)
    #[schemars(range(min = 0.0, max = 0.10))]
    pub terminal_growth: Option<f64>,

    /// Override revenue growth rate (-0.50–1.00). Calibrated from history if omitted.
    #[schemars(range(min = -0.50, max = 1.00))]
    pub revenue_growth: Option<f64>,
    /// Override gross margin (0.05–0.95). Calibrated from history if omitted.
    #[schemars(range(min = 0.05, max = 0.95))]
    pub gross_margin: Option<f64>,
    /// Override D&A as % of revenue (0.00–0.20). Calibrated from history if omitted.
    #[schemars(range(min = 0.0, max = 0.20))]
    pub da_to_revenue: Option<f64>,
    /// Override capex as % of revenue (0.00–0.30). Calibrated from history if omitted.
    #[schemars(range(min = 0.0, max = 0.30))]
    pub capex_to_revenue: Option<f64>,
    /// Override NWC as % of revenue (-0.20–0.50). Calibrated from history if omitted.
    #[schemars(range(min = -0.20, max = 0.50))]
    pub nwc_to_revenue: Option<f64>,
    /// Override effective tax rate (0.00–1.00). Calibrated from history if omitted.
    #[schemars(range(min = 0.0, max = 1.0))]
    pub tax_rate: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReverseDcfRequest {
    pub symbol: String,
    /// Stage 1 years (1–3, default 3)
    pub stage1_years: Option<u8>,
    /// Stage 2 years (2–7, default 7)
    pub stage2_years: Option<u8>,
    /// Discount rate / WACC (0.0–0.30, default 0.10)
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (0.0–0.10, default 0.025)
    pub terminal_growth: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScenarioAnalysisRequest {
    pub symbol: String,
    /// Discount rate (default 0.10)
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (default 0.025)
    pub terminal_growth: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrateForecastRequest {
    pub symbol: String,
    /// Optional parent forecast ID for a same-symbol revision.
    pub revision_of: Option<String>,
    /// Your estimate of future revenue growth rate (0.0–1.0).
    /// If omitted, runs Fermi decomposition with default sub-questions.
    pub growth_estimate: Option<f64>,

    /// Your estimate of future profit margin (0.0–1.0).
    /// If omitted, runs Fermi decomposition with default sub-questions.
    pub margin_estimate: Option<f64>,

    /// Override individual Fermi sub-questions for growth.
    /// Each entry: { "estimate": 0.0-1.0, "confidence": 0.0-1.0 }.
    /// Must provide exactly 4 if overriding. Omitted questions use defaults.
    #[serde(default)]
    pub growth_fermi_overrides: Vec<FermiOverride>,
    /// Override individual Fermi sub-questions for margin.
    #[serde(default)]
    pub margin_fermi_overrides: Vec<FermiOverride>,
    /// Reference class for outside view (e.g., "S&P 500 large-cap tech").
    /// Default: "S&P 500 large-cap, 2015-2025"
    pub reference_class: Option<String>,
    /// Number of reference cases for outside view calibration.
    /// Higher N = more weight on base rate. Default: 500.
    pub reference_count: Option<u64>,
    /// Stage 1 years (1–3, default 3)
    pub stage1_years: Option<u8>,
    /// Stage 2 years (2–7, default 7)
    pub stage2_years: Option<u8>,
    /// Discount rate / WACC (default 0.10)
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (default 0.025)
    pub terminal_growth: Option<f64>,
}

/// Override for a single Fermi sub-question estimate.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FermiOverride {
    /// Index of the sub-question to override (0-3).
    pub index: usize,
    /// New estimate (0.0–1.0).
    pub estimate: f64,
    /// New confidence (0.0–1.0).
    pub confidence: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForecastGetRequest {
    pub forecast_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForecastListRequest {
    pub symbol: String,
}

/// Forecast horizon for outcome recording.
#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub enum Horizon {
    #[serde(rename = "3mo")]
    ThreeMo,
    #[serde(rename = "6mo")]
    SixMo,
    #[serde(rename = "1yr")]
    OneYr,
    #[serde(rename = "2yr")]
    TwoYr,
    #[serde(rename = "3yr")]
    ThreeYr,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForecastRecordRequest {
    pub symbol: String,
    /// When the forecast was made (YYYY-MM-DD)
    pub forecast_date: String,
    /// Forecast horizon.
    pub horizon: Horizon,
    /// Forecast valuation multiple (e.g., P/E or EV/FCF)
    pub forecast_multiple: f64,
    /// Forecast price change over the horizon (e.g., 0.10 = 10% return)
    pub forecast_price_change: f64,
    /// Actual outcome date (YYYY-MM-DD)
    pub outcome_date: String,
    /// Actual valuation multiple at outcome date
    pub actual_multiple: f64,
    /// Actual price change from forecast date to outcome date
    pub actual_price_change: f64,
    /// Forecast ID from dcf_valuation or calibrate_forecast.
    /// When provided, looks up the stored projected model and decomposes
    /// the return gap into 11-line-item drivers (revenue growth, gross margin,
    /// D&A, capex, NWC, multiple expansion, net debt).
    pub forecast_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SensitivityAnalysisRequest {
    pub symbol: String,
    pub stage1_years: Option<u8>,
    pub stage2_years: Option<u8>,
    pub discount_rate: Option<f64>,
    pub terminal_growth: Option<f64>,
    pub revenue_growth: Option<f64>,
    pub gross_margin: Option<f64>,
    pub da_to_revenue: Option<f64>,
    pub capex_to_revenue: Option<f64>,
    pub nwc_to_revenue: Option<f64>,
    pub tax_rate: Option<f64>,
    #[serde(default = "default_sensitivity_range")]
    pub range_pct: f64,
}

fn default_sensitivity_range() -> f64 {
    0.10
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MonteCarloDcfRequest {
    pub symbol: String,
    pub stage1_years: Option<u8>,
    pub stage2_years: Option<u8>,
    pub discount_rate: Option<f64>,
    pub terminal_growth: Option<f64>,
    pub revenue_growth: Option<f64>,
    pub gross_margin: Option<f64>,
    pub da_to_revenue: Option<f64>,
    pub capex_to_revenue: Option<f64>,
    pub nwc_to_revenue: Option<f64>,
    pub tax_rate: Option<f64>,
    #[serde(default = "default_mc_simulations")]
    pub simulations: u32,
    #[serde(default = "default_mc_range")]
    pub range_revenue_growth: f64,
    #[serde(default = "default_mc_range")]
    pub range_gross_margin: f64,
    #[serde(default = "default_mc_range_small")]
    pub range_da: f64,
    #[serde(default = "default_mc_range_small")]
    pub range_capex: f64,
    #[serde(default = "default_mc_range")]
    pub range_nwc: f64,
    #[serde(default = "default_mc_range_small")]
    pub range_discount_rate: f64,
}

fn default_mc_simulations() -> u32 {
    1000
}
fn default_mc_range() -> f64 {
    0.03
}
fn default_mc_range_small() -> f64 {
    0.01
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ComparableAnalysisRequest {
    pub symbol: String,
    pub peers: Option<String>,
    /// Discount rate / WACC (0.05–0.30, default 0.10).
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (0.00–0.10 and below discount rate, default 0.025).
    pub terminal_growth: Option<f64>,
}

/// Optional DCF projection inputs shared by all valuation tools.
#[derive(Debug, Default, Clone)]
pub struct ProjectionAssumptionOverrides {
    pub stage1_years: Option<u8>,
    pub stage2_years: Option<u8>,
    pub revenue_growth: Option<f64>,
    pub gross_margin: Option<f64>,
    pub da_to_revenue: Option<f64>,
    pub capex_to_revenue: Option<f64>,
    pub nwc_to_revenue: Option<f64>,
    pub tax_rate: Option<f64>,
    pub discount_rate: Option<f64>,
    pub terminal_growth: Option<f64>,
}

macro_rules! projection_overrides_from_request {
    ($request:ty, $($field:ident),* $(,)?) => {
        impl From<&$request> for ProjectionAssumptionOverrides {
            fn from(request: &$request) -> Self {
                let mut overrides = Self::default();
                $(overrides.$field = request.$field;)*
                overrides
            }
        }
    };
}

projection_overrides_from_request!(
    DcfValuationRequest,
    stage1_years,
    stage2_years,
    revenue_growth,
    gross_margin,
    da_to_revenue,
    capex_to_revenue,
    nwc_to_revenue,
    tax_rate,
    discount_rate,
    terminal_growth,
);
projection_overrides_from_request!(
    ReverseDcfRequest,
    stage1_years,
    stage2_years,
    discount_rate,
    terminal_growth,
);
impl From<&ScenarioAnalysisRequest> for ProjectionAssumptionOverrides {
    fn from(request: &ScenarioAnalysisRequest) -> Self {
        Self {
            discount_rate: request.discount_rate,
            terminal_growth: request.terminal_growth,
            ..Self::default()
        }
    }
}
projection_overrides_from_request!(
    SensitivityAnalysisRequest,
    stage1_years,
    stage2_years,
    revenue_growth,
    gross_margin,
    da_to_revenue,
    capex_to_revenue,
    nwc_to_revenue,
    tax_rate,
    discount_rate,
    terminal_growth,
);
projection_overrides_from_request!(
    MonteCarloDcfRequest,
    stage1_years,
    stage2_years,
    revenue_growth,
    gross_margin,
    da_to_revenue,
    capex_to_revenue,
    nwc_to_revenue,
    tax_rate,
    discount_rate,
    terminal_growth,
);
projection_overrides_from_request!(
    CalibrateForecastRequest,
    stage1_years,
    stage2_years,
    discount_rate,
    terminal_growth,
);
impl From<&ComparableAnalysisRequest> for ProjectionAssumptionOverrides {
    fn from(request: &ComparableAnalysisRequest) -> Self {
        Self {
            discount_rate: request.discount_rate,
            terminal_growth: request.terminal_growth,
            ..Self::default()
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResearchSearchRequest {
    pub symbol: String,
    /// Research query (e.g., "management guidance 2025", "competition market share")
    pub query: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScreenerRequest {
    /// Natural language screening prompt (e.g., "large cap tech stocks with pe under 20 and dividend over 2%")
    pub prompt: String,
    /// Maximum results (default 20)
    #[serde(default = "default_screener_limit")]
    pub limit: u32,
    /// Override specific criteria directly (bypasses prompt parsing for these fields)
    #[serde(default)]
    pub criteria_overrides: serde_json::Value,
}

fn default_screener_limit() -> u32 {
    20
}

// ── Economic Profit valuation request ────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EpValuationRequest {
    pub symbol: String,
    /// Discount rate / WACC (0.0–0.30, default 0.10)
    pub wacc: Option<f64>,
    /// Invested capital growth rate (0.0–0.30, default 0.0).
    /// Models the AFG growth driver: reinvestment that expands the capital base.
    pub ic_growth_rate: Option<f64>,
    /// Competitive fade horizon override.
    /// If omitted, we attempt to derive from moat_result.
    pub moat_override: Option<crate::economic_profit::FadeHorizon>,
    /// Moat classification from moat_check.
    /// Only used when moat_override is not provided.
    pub moat_result: Option<crate::economic_profit::FadeHorizon>,
    /// Stage 1 years: hold current EP constant before fade (1–5, default 3).
    pub stage1_years: Option<u8>,
}
