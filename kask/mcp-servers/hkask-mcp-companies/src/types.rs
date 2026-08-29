//! Request types for hkask-mcp-companies MCP tools.
//!
//! Extracted from main.rs — these are the tool input structs that derive
//! Deserialize + JsonSchema for MCP parameter deserialization.

use hkask_mcp_server::AnyJsonValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Financial data request structs ──────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SymbolRequest {
    pub symbol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SymbolLimitRequest {
    pub symbol: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct HistoricalRequest {
    pub symbol: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

// ── Portfolio analytics request structs ──────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AttributionRequest {
    pub portfolio: String,
    pub from: String,
    pub to: String,
}

/// Aggregation method for portfolio characteristic weighted averages.
///
/// Mirrors the methods used by FactSet/Bloomberg/Morningstar for portfolio
/// analytics. The default (`weighted_arithmetic`) matches the original
/// implementation. The alternatives address known biases:
///
/// - `weighted_harmonic`: Correct for averaging ratios (P/E, P/B, P/S).
///   The arithmetic mean is biased upward for ratios because it gives
///   greater weight to high values. Morningstar switched to harmonic
///   weighted averages for P/E, P/B, P/S, and P/CF in 2005. (Agrrawal
///   et al. 2010; CFA Level II Reading 25.)
/// - `weighted_median`: Robust to outliers — the median is unaffected by
///   extreme values. Bloomberg uses this for descriptor distributions.
/// - `winsorized`: Clamp values at the 5th and 95th percentiles before
///   computing the weighted arithmetic mean. Bloomberg winsorizes
///   descriptors at 5/95 for Quality and Value-Growth indices.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AggregationMethod {
    /// Weighted arithmetic mean: Σ(wᵢ × xᵢ). Default; matches the original
    /// implementation. Biased upward for ratios (P/E, P/B, P/S).
    #[default]
    WeightedArithmetic,
    /// Weighted harmonic mean: 1 / Σ(wᵢ / xᵢ). Correct for ratios — gives
    /// equal weight to each unit of the denominator (e.g., equal-dollar
    /// weighting for P/E). Cannot handle zero or negative values; those
    /// holdings are skipped for the affected field.
    WeightedHarmonic,
    /// Weighted median: the value where cumulative weight crosses 50%.
    /// Robust to outliers. Unaffected by extreme P/E values.
    WeightedMedian,
    /// Winsorized weighted mean: clamp values at the 5th and 95th
    /// percentiles, then compute the weighted arithmetic mean. Reduces
    /// the influence of outliers without excluding them entirely.
    Winsorized,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CharacteristicsRequest {
    pub portfolio: String,
    pub date: String,
    /// Aggregation method for weighted averages. Defaults to
    /// `weighted_arithmetic` when omitted.
    #[serde(default)]
    pub aggregation: AggregationMethod,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ExpectationsGapRequest {
    pub symbol: String,
    /// Your estimate of sustainable revenue growth (0.0–1.0).
    /// Compared against market-implied growth and management guidance.
    pub growth_estimate: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NoteAddRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NoteListRequest {
    pub portfolio: String,
    pub symbol: String,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NoteDeleteRequest {
    pub note_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FileAttachRequest {
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
pub(crate) struct FileListRequest {
    pub portfolio: String,
    pub symbol: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FileDeleteRequest {
    pub file_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResultFeedbackRequest {
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
    /// Explicit data provider that produced the result (e.g. "fmp", "eodhd").
    /// When omitted, the provider is inferred from the symbol/query.
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DcfValuationRequest {
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
pub(crate) struct EquityDurationRequest {
    pub symbol: String,
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
pub(crate) struct ReverseDcfRequest {
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
pub(crate) struct ScenarioAnalysisRequest {
    pub symbol: String,
    /// Discount rate (default 0.10)
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (default 0.025)
    pub terminal_growth: Option<f64>,
    /// Optional event tree JSON (from hkask-mcp-scenarios
    /// `scenario_from_markets_set` / `scenario_propagate` output — the
    /// `tree` object). When present, the four quadrant probabilities come
    /// from the tree's root-event marginals (detailed mode); when absent,
    /// the plain 2×2 range is returned without probabilities (simple mode,
    /// the default on-ramp).
    pub event_tree: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CalibrateForecastRequest {
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
pub(crate) struct FermiOverride {
    /// Index of the sub-question to override (0-3).
    pub index: usize,
    /// New estimate (0.0–1.0).
    pub estimate: f64,
    /// New confidence (0.0–1.0).
    pub confidence: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForecastGetRequest {
    pub forecast_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForecastListRequest {
    pub symbol: String,
}

/// Forecast horizon for outcome recording.
#[derive(Debug, Clone, Deserialize, JsonSchema, Serialize)]
pub(crate) enum Horizon {
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
pub(crate) struct ForecastRecordRequest {
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
    /// Forecast ID from dcf_valuation, calibrate_forecast, or forecast_persist.
    /// When provided and the stored snapshot contains a full projected model,
    /// decomposes the return gap into 11-line-item drivers (revenue growth,
    /// gross margin, D&A, capex, NWC, multiple expansion, net debt).
    /// Pre-computed PTs persisted via forecast_persist carry no decomposition
    /// model — Brier scoring still runs, decomposition is skipped.
    pub forecast_id: Option<String>,
}

/// Persist a pre-computed price target for later Brier scoring.
/// Unlike calibrate_forecast (which runs its own Fermi decomposition) and
/// forecast_record (which requires the actual outcome), this tool stores a
/// pending price target without an outcome and without a decomposition model.
/// The stored forecast can later be resolved by forecast_record when the
/// horizon passes — Brier scoring runs on the recorded price change;
/// gap decomposition is unavailable (no projected model).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForecastPersistRequest {
    pub symbol: String,
    /// When the forecast was made (YYYY-MM-DD)
    pub forecast_date: String,
    /// Forecast horizon.
    pub horizon: Horizon,
    /// Forecast price change over the horizon (e.g., 0.10 = 10% return).
    /// The primary Brier signal — forecast_record scores this against the
    /// actual price change within a 20% tolerance band. When omitted, the
    /// tool computes it from `forecast_price` and `current_price`.
    pub forecast_price_change: Option<f64>,
    /// Forecast valuation multiple (e.g., P/E or EV/FCF). Optional —
    /// pre-computed PTs from skill valuation synthesis may not carry a
    /// multiple. When omitted, the multiple-direction Brier score is null.
    pub forecast_multiple: Option<f64>,
    /// Forecast price target (absolute price). When provided with
    /// `current_price`, the tool computes `forecast_price_change` if that
    /// field is omitted.
    pub forecast_price: Option<f64>,
    /// Current market price at forecast date. When provided with
    /// `forecast_price`, the tool computes `forecast_price_change` if that
    /// field is omitted.
    pub current_price: Option<f64>,
    /// Optional parent forecast ID for a same-symbol revision.
    pub revision_of: Option<String>,
    /// Optional caller-supplied forecast ID. When omitted, the server
    /// generates a UUID. Useful for skill steps that need a stable ID
    /// to thread to a later forecast_record call.
    pub forecast_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SensitivityAnalysisRequest {
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
pub(crate) struct MonteCarloDcfRequest {
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
pub(crate) struct ComparableAnalysisRequest {
    pub symbol: String,
    pub peers: Option<String>,
    /// Discount rate / WACC (0.05–0.30, default 0.10).
    pub discount_rate: Option<f64>,
    /// Terminal growth rate (0.00–0.10 and below discount rate, default 0.025).
    pub terminal_growth: Option<f64>,
}

/// Optional DCF projection inputs shared by all valuation tools.
#[derive(Debug, Default, Clone)]
pub(crate) struct ProjectionAssumptionOverrides {
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
projection_overrides_from_request!(
    EquityDurationRequest,
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
projection_overrides_from_request!(
    ScenarioImpactValuationRequest,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ResearchSearchRequest {
    pub symbol: String,
    /// Research query (e.g., "management guidance 2025", "competition market share")
    pub query: String,
}

// ── Scenario impact valuation request ─────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ScenarioImpactValuationRequest {
    pub symbol: String,
    /// JSON string of the resolved scenario event tree from `scenario_quantify`
    /// (hkask-mcp-scenarios). Accepts both the scenario server's native
    /// `EventTree` format (nested `event` fields, `topo_order`) and the
    /// flat format (`nodes` with `id`, `marginal_probability`, `depends_on`
    /// and `topological_order`). Normalization is applied automatically.
    pub scenario_tree: String,
    /// JSON array of per-node impact mappings. Each entry has `node_id`,
    /// `yes_deltas` (additive DCF assumption deltas when the node resolves
    /// Yes), and optional `no_deltas` (deltas when No, default zero).
    pub impact_mappings: String,
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
    /// Optional realized (historical) annual volatility of the equity, as a
    /// decimal (0.35 = 35%). When supplied together with a computable
    /// scenario risk measure, the tool emits `fused_volatility` — the
    /// root-sum-square fusion of realized and scenario-implied σ
    /// (hkask_forecast::fuse_volatility), weighted by the tree's total
    /// probability mass (partial tree coverage down-weights the scenario
    /// channel). When omitted, no fusion is emitted (never fabricated).
    pub realized_volatility: Option<f64>,
}

// ── Company transcript request (earnings + corpus modes) ──────────────

/// Fetch mode for `company_transcript`.
///
/// `earnings` fetches FMP earnings-call transcripts (the existing behavior).
/// `corpus` fetches non-earnings company transcripts (investor-day keynotes,
/// executive interviews) via SerpAPI YouTube, channel-allowlisted per the
/// company manifest. Corpus mode does NOT segment — it normalizes to
/// pipeline-ready records and hands off to the corpus pipeline.
#[derive(Debug, Default, Deserialize, JsonSchema, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TranscriptMode {
    /// FMP earnings-call transcript: fetch + coverage-honest.
    #[default]
    Earnings,
    /// Non-earnings company transcripts via SerpAPI YouTube (channel-allowlisted).
    /// Normalize-only, no segmentation. Pipeline-ready JSONL output.
    Corpus,
}

/// Request for `company_transcript`.
///
/// Temporal key is `(year, quarter)` — the FMP `date` field is unreliable
/// (probe-verified: AAPL 2023Q1 returns `date: "2012-03-19"`). Callers must
/// not rely on `date` for ordering or deduplication.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CompanyTranscriptRequest {
    pub symbol: String,
    /// Fetch mode. `earnings` (default) hits FMP; `corpus` hits SerpAPI YouTube.
    #[serde(default)]
    pub mode: TranscriptMode,
    /// Calendar year (e.g. 2024). Required for `earnings` mode when
    /// `quarters_back` is not used. Ignored for `corpus` mode.
    pub year: Option<u32>,
    /// Calendar quarter 1–4. Required for `earnings` mode when `quarters_back`
    /// is not used. Ignored for `corpus` mode.
    pub quarter: Option<u8>,
    /// Fetch the last N quarters ending at `(year, quarter)`. Default 1.
    /// Per-quarter failures are collected into `coverage.missing`, not
    /// propagated as whole-tool failure. Ignored for `corpus` mode.
    #[serde(default = "default_transcript_quarters_back")]
    pub quarters_back: u32,
    /// Search query for `corpus` mode (e.g. "Satya Nadella keynote").
    /// Required for `corpus` mode; ignored for `earnings` mode.
    #[serde(default)]
    pub query: Option<String>,
    /// Channel allowlist for `corpus` mode (e.g. ["Microsoft", "Microsoft Investor Relations"]).
    /// Videos from channels NOT on this list are excluded and logged, never silently kept.
    /// Required for `corpus` mode; ignored for `earnings` mode.
    #[serde(default)]
    pub channels_allowlist: Vec<String>,
    /// Max results for `corpus` mode (default 5). Ignored for `earnings` mode.
    #[serde(default = "default_corpus_max_results")]
    pub max_results: u32,
}

fn default_transcript_quarters_back() -> u32 {
    1
}

fn default_corpus_max_results() -> u32 {
    5
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ScreenerRequest {
    /// Natural language screening prompt (e.g., "large cap tech stocks with pe under 20 and dividend over 2%")
    pub prompt: String,
    /// Maximum results (default 20). The EODHD screener paginates
    /// automatically to exhaust the full universe, so this is an upper
    /// bound on the returned row count, not a page size.
    #[serde(default = "default_screener_limit")]
    #[allow(dead_code)]
    pub limit: u32,
    /// Override specific criteria directly (bypasses prompt parsing for these fields).
    ///
    /// Accepts arbitrary JSON. Typed as [`AnyJsonValue`] (not `serde_json::Value`)
    /// so the generated tool input schema is the empty object `{}` rather than the
    /// bare boolean `true` schemars emits for `Value` — Ollama rejects boolean
    /// property schemas with `400 cannot unmarshal bool into ... api.ToolProperty`.
    #[serde(default)]
    pub criteria_overrides: AnyJsonValue,
}

fn default_screener_limit() -> u32 {
    20
}

/// Request for the stock_universe tool. Exhaustive bulk listing from EODHD.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct StockUniverseRequest {
    /// Minimum market cap in USD (default $500 million)
    #[serde(default = "default_min_market_cap")]
    pub min_market_cap: f64,
    /// EODHD exchange code (default "US" for all US exchanges)
    #[serde(default = "default_exchange")]
    pub exchange: String,
}

fn default_min_market_cap() -> f64 {
    500_000_000.0
}

fn default_exchange() -> String {
    "US".to_string()
}

// ── Economic Profit valuation request ────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct EpValuationRequest {
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
    /// Risk-free rate for CAPM cost-of-equity calculation (financial-sector firms).
    /// Default: 4.25% (10Y Treasury). Only used for equity-based valuation path.
    pub risk_free_rate: Option<f64>,
    /// Equity risk premium for CAPM cost-of-equity calculation (financial-sector firms).
    /// Default: 4.5% (Damodaran implied ERP). Only used for equity-based valuation path.
    pub equity_risk_premium: Option<f64>,
}

// ── Driver-based forecast request ────────────────────────────────────

/// Request for the driver-based three-statement forecasting tool.
///
/// Projects linked income statement, balance sheet, and cash flow from five
/// key drivers. Each driver supports percent change, percent of revenue, and
/// explicit adjustment. The balance sheet identity (A = L + E) is enforced
/// every period. Financial-sector companies use an equity-based residual
/// income path (ROE/COE) instead of FCF-based DCF.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DriverForecastRequest {
    pub symbol: String,

    // Driver 1: Revenue growth
    /// Revenue growth rate (YoY %, e.g., 0.08 = 8%). Default: historical CAGR.
    pub revenue_growth: Option<f64>,
    /// Explicit revenue adjustment in dollars (e.g., acquisition contribution).
    pub revenue_explicit: Option<f64>,

    // Driver 2: Profit margins
    /// Gross margin (% of revenue, e.g., 0.40 = 40%). Default: historical.
    pub gross_margin: Option<f64>,
    /// SG&A as % of revenue (e.g., 0.15 = 15%). Default: historical.
    pub sga_pct: Option<f64>,
    /// D&A as % of revenue. Default: historical.
    pub da_pct: Option<f64>,
    /// Effective tax rate. Default: historical.
    pub tax_rate: Option<f64>,

    // Driver 3: Capex vs depreciation
    /// Capex as % of revenue. Default: historical.
    pub capex_pct: Option<f64>,
    /// Explicit capex adjustment in dollars.
    pub capex_explicit: Option<f64>,
    /// Capex/D&A ratio target. When set, D&A is derived from capex / ratio.
    pub capex_da_ratio: Option<f64>,

    // Driver 4: Net working capital
    /// NWC method: "days", "percent_of_revenue", or "explicit".
    /// Default: "percent_of_revenue".
    pub nwc_method: Option<String>,
    /// Days sales outstanding (for days method).
    pub dso_days: Option<f64>,
    /// Days inventory outstanding (for days method).
    pub dio_days: Option<f64>,
    /// Days payable outstanding (for days method).
    pub dpo_days: Option<f64>,
    /// NWC as % of revenue (for percent_of_revenue method).
    pub nwc_pct: Option<f64>,
    /// Explicit NWC adjustment in dollars.
    pub nwc_explicit: Option<f64>,

    // Driver 5: Debt/equity issuance
    /// Explicit debt issuance in dollars per period.
    pub debt_issuance: Option<f64>,
    /// Explicit debt repayment in dollars per period.
    pub debt_repayment: Option<f64>,
    /// Target debt-to-equity ratio.
    pub target_debt_equity: Option<f64>,
    /// Interest rate on debt. Default: historical interest/debt.
    pub interest_rate: Option<f64>,
    /// Explicit equity issuance in dollars per period.
    pub equity_issuance: Option<f64>,
    /// Dividend payout ratio (0.0 = full retention, 1.0 = full payout).
    pub dividend_payout_ratio: Option<f64>,

    // Valuation
    /// Discount rate (WACC for industrial, COE for financial-sector).
    /// Default: 0.10.
    pub discount_rate: Option<f64>,
    /// Terminal growth rate. Default: 0.025.
    pub terminal_growth: Option<f64>,
    /// Projection horizon in years. Default: 10.
    pub total_years: Option<u8>,
    /// Cost of equity for financial-sector residual income path. Default: 0.10.
    pub cost_of_equity: Option<f64>,

    // Output
    /// When true, return a Markdown report alongside the JSON.
    /// Default: false.
    #[serde(default)]
    pub markdown_report: bool,
}
