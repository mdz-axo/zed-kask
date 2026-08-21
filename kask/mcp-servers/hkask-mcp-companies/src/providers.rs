//! hKask MCP Companies — Dual-provider abstraction (FMP + EODHD)
//!
//! Routes tool calls to the appropriate provider and normalizes responses
//! so that analysis functions in `analysis.rs` work transparently with
//! either data source.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use serde_json::Value;

// ── Typed projection views ───────────────────────────────────────
//
// `companies_get` returns these typed views over the retained raw `Value` so
// that field-name knowledge concentrates in one place (the accessor bodies)
// rather than leaking into every tool handler as `v.get("companyName")`.
// Each view holds the normalized raw payload and exposes typed accessors.
// The `raw()` escape hatch preserves Ashby requisite variety — a field the
// struct doesn't carry is still reachable via `raw()`, so new provider fields
// are not silently dropped (the broken-fidelity trap that `unwrap_or(0.0)`
// creates on the untyped path).

/// Typed view over a company profile (FMP `/profile` or EODHD `/fundamentals`).
///
/// The profile is a one-element array after EODHD normalization. Accessors
/// read the first element; a missing field returns `None` (not a silent zero),
// so a missing `mktCap` is distinguishable from a zero market cap.
pub(crate) struct CompanyProfile {
    raw: Value,
}

impl CompanyProfile {
    /// Wrap a normalized profile payload. The value is expected to be the
    /// FMP-shaped array (`[{"companyName": ...}]`); an empty array means the
    /// provider returned no profile.
    pub fn from_raw(raw: Value) -> Self {
        Self { raw }
    }

    /// The first profile object in the array, or `None` if the array is empty
    /// or missing (the "no profile" signal — distinct from a present-but-zero
    /// field).
    fn first(&self) -> Option<&Value> {
        self.raw.as_array().and_then(|a| a.first())
    }

    /// Escape hatch — the retained raw payload, preserving fields the typed
    /// accessors don't yet cover (Ashby requisite variety).
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// `companyName` (FMP) — the legal entity name.
    pub fn company_name(&self) -> Option<&str> {
        self.first()?.get("companyName").and_then(|v| v.as_str())
    }

    /// `sector` (FMP) — the GICS sector.
    pub fn sector(&self) -> Option<&str> {
        self.first()?.get("sector").and_then(|v| v.as_str())
    }

    /// `industry` (FMP) — the GICS industry classification.
    pub fn industry(&self) -> Option<&str> {
        self.first()?.get("industry").and_then(|v| v.as_str())
    }

    /// `price` (FMP) — the latest trade price.
    pub fn price(&self) -> Option<f64> {
        self.first()?.get("price").and_then(|v| v.as_f64())
    }

    /// `mktCap` (FMP) — the market capitalization.
    pub fn market_cap(&self) -> Option<f64> {
        self.first()?.get("mktCap").and_then(|v| v.as_f64())
    }
}

/// Typed view over key metrics (FMP `/key-metrics` or EODHD-derived).
///
/// The raw payload is the FMP-shaped array of yearly metric objects. Per-year
/// accessors (`gross_profit_margin`, `roic`, `days_of_payables_outstanding` …)
/// read from each array element; the latest-year accessor reads `first()`
/// (FMP returns newest-first, and the EODHD normalizer sorts to match).
pub(crate) struct KeyMetrics {
    raw: Value,
}

impl KeyMetrics {
    /// Wrap a normalized key-metrics payload (the FMP-shaped array).
    pub fn from_raw(raw: Value) -> Self {
        Self { raw }
    }

    /// The array of yearly metric objects (newest-first), or an empty slice if
    /// the payload isn't an array.
    pub fn years(&self) -> &[Value] {
        self.raw.as_array().map_or(&[], |v| v)
    }

    /// Escape hatch — the retained raw payload.
    pub fn raw(&self) -> &Value {
        &self.raw
    }

    /// The latest yearly metric object (FMP returns newest-first), or `None`.
    pub fn latest(&self) -> Option<&Value> {
        self.years().first()
    }

    /// `peRatio` from the latest year — the price-to-earnings multiple.
    pub fn pe_ratio(&self) -> Option<f64> {
        self.latest()?.get("peRatio").and_then(|v| v.as_f64())
    }

    /// `priceToBookRatio` from the latest year.
    pub fn price_to_book(&self) -> Option<f64> {
        self.latest()?
            .get("priceToBookRatio")
            .and_then(|v| v.as_f64())
    }

    /// `priceToSalesRatio` from the latest year.
    pub fn price_to_sales(&self) -> Option<f64> {
        self.latest()?
            .get("priceToSalesRatio")
            .and_then(|v| v.as_f64())
    }

    /// `evToEbitda` (preferred) or `enterpriseValueMultiple` from the latest
    /// year — the EV/EBITDA multiple (FMP renamed this field across API
    /// versions; both spellings appear in the wild).
    pub fn ev_to_ebitda(&self) -> Option<f64> {
        let latest = self.latest()?;
        latest
            .get("evToEbitda")
            .or_else(|| latest.get("enterpriseValueMultiple"))
            .and_then(|v| v.as_f64())
    }

    /// `dividendYield` from the latest year.
    pub fn dividend_yield(&self) -> Option<f64> {
        self.latest()?.get("dividendYield").and_then(|v| v.as_f64())
    }

    /// `revenueGrowth` from the latest year.
    pub fn revenue_growth(&self) -> Option<f64> {
        self.latest()?.get("revenueGrowth").and_then(|v| v.as_f64())
    }
}

/// Typed view over historical prices (FMP `/historical-price-full` or EODHD
/// `/eod`). The raw payload is the FMP-shaped `{"symbol": ..., "historical":
/// [...]}` envelope; per-day accessors read from the `historical` array.
pub(crate) struct HistoricalPriceView {
    raw: Value,
}

impl HistoricalPriceView {
    /// Wrap a normalized historical-price payload.
    pub fn from_raw(raw: Value) -> Self {
        Self { raw }
    }

    /// The `historical` array of daily OHLCV bars (newest-first per FMP), or an
    /// empty slice if the envelope is missing the array.
    pub fn historical(&self) -> &[Value] {
        self.raw
            .get("historical")
            .and_then(|v| v.as_array())
            .map_or(&[], |v| v)
    }

    /// The latest day's close price, preferring `close` and falling back to
    /// `adjClose` (the adjusted close — used when the raw close isn't
    /// available, e.g. for split-adjusted EODHD bars).
    pub fn latest_close(&self) -> Option<f64> {
        let day = self.historical().first()?;
        day.get("close")
            .or_else(|| day.get("adjClose"))
            .and_then(|v| v.as_f64())
    }
}

// ── Provider enum ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Fmp,
    Eodhd,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fmp => write!(f, "FMP"),
            Self::Eodhd => write!(f, "EODHD"),
        }
    }
}

// ── Base URLs ──────────────────────────────────────────────────────

const FMP_BASE_URL: &str = "https://financialmodelingprep.com/stable";
const EODHD_BASE_URL: &str = "https://eodhd.com/api";

// ── Endpoint descriptor: maps a logical endpoint to provider-specific paths ──

pub(crate) struct EndpointMapping {
    pub fmp_path: &'static str,
    pub eodhd_path: &'static str,
    /// If true, EODHD response needs normalization to match FMP format
    pub normalize_eodhd: bool,
}

// ── Endpoint registry ──────────────────────────────────────────────

fn endpoint_mapping(tool: &str) -> Option<EndpointMapping> {
    match tool {
        "company_profile" => Some(EndpointMapping {
            fmp_path: "/profile",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "stock_quote" => Some(EndpointMapping {
            fmp_path: "/quote",
            eodhd_path: "/real-time",
            normalize_eodhd: false,
        }),
        "income_statement" => Some(EndpointMapping {
            fmp_path: "/income-statement",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "balance_sheet" => Some(EndpointMapping {
            fmp_path: "/balance-sheet-statement",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "cash_flow_statement" => Some(EndpointMapping {
            fmp_path: "/cash-flow-statement",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "key_metrics" => Some(EndpointMapping {
            fmp_path: "/key-metrics",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "historical_price" => Some(EndpointMapping {
            fmp_path: "/historical-price-full",
            eodhd_path: "/eod",
            normalize_eodhd: true,
        }),
        "symbol_search" => Some(EndpointMapping {
            fmp_path: "/search-name",
            eodhd_path: "/search",
            normalize_eodhd: false,
        }),
        _ => None,
    }
}

// ── Symbol routing ─────────────────────────────────────────────────
//
// Symbols with exchange suffix (e.g., VOD.L, BMW.DE) → EODHD primary.
// Plain symbols (e.g., AAPL) → FMP primary, EODHD fallback.

fn is_international_symbol(symbol: &str) -> bool {
    symbol.contains('.')
}

fn primary_provider(symbol: &str) -> Provider {
    if is_international_symbol(symbol) {
        Provider::Eodhd
    } else {
        Provider::Fmp
    }
}

// ── Main routing function ──────────────────────────────────────────

/// Fetch data for a logical tool endpoint, trying primary provider first
/// then falling back to secondary. Normalizes EODHD responses to match
/// FMP format when needed.
pub async fn companies_get(
    client: &reqwest::Client,
    tool: &str,
    symbol: &str,
    fmp_api_key: &str,
    eodhd_api_key: &str,
    extra_params: &[(&str, &str)],
    learning: Option<&super::LearningState>,
) -> Result<Value, McpToolError> {
    let mapping = endpoint_mapping(tool)
        .ok_or_else(|| McpToolError::invalid_argument(format!("unknown tool: {tool}")))?;
    // Learning-aware routing: feedback state can override default provider.
    let primary = if let Some(learn) = learning {
        learn
            .preferred_provider(symbol, primary_provider(symbol))
            .unwrap_or_else(|| primary_provider(symbol))
    } else {
        primary_provider(symbol)
    };

    // Try primary provider
    let primary_result = match primary {
        Provider::Fmp => fmp_get(client, mapping.fmp_path, fmp_api_key, symbol, extra_params).await,
        Provider::Eodhd => {
            eodhd_get(
                client,
                mapping.eodhd_path,
                eodhd_api_key,
                symbol,
                extra_params,
            )
            .await
        }
    };

    match primary_result {
        Ok(value) => {
            // Normalize EODHD response if needed
            if primary == Provider::Eodhd && mapping.normalize_eodhd {
                emit_provider_reg(tool, symbol, "EODHD", true);
                Ok(normalize_eodhd(tool, &value, symbol))
            } else {
                Ok(value)
            }
        }
        Err(_primary_err) => {
            // Fall back to secondary provider
            let secondary = match primary {
                Provider::Fmp => Provider::Eodhd,
                Provider::Eodhd => Provider::Fmp,
            };

            let fallback_result = match secondary {
                Provider::Fmp => {
                    fmp_get(client, mapping.fmp_path, fmp_api_key, symbol, extra_params).await
                }
                Provider::Eodhd => {
                    // For FMP→EODHD fallback on plain symbols, try with .US suffix
                    let eodhd_symbol = if !is_international_symbol(symbol) {
                        format!("{}.US", symbol)
                    } else {
                        symbol.to_string()
                    };
                    eodhd_get(
                        client,
                        mapping.eodhd_path,
                        eodhd_api_key,
                        &eodhd_symbol,
                        extra_params,
                    )
                    .await
                }
            };

            match fallback_result {
                Ok(value) => {
                    if secondary == Provider::Eodhd && mapping.normalize_eodhd {
                        emit_provider_reg(tool, symbol, "EODHD", true);
                        Ok(normalize_eodhd(tool, &value, symbol))
                    } else {
                        Ok(value)
                    }
                }
                Err(e) => Err(e),
            }
        }
    }
}

/// Fetch a company profile as a typed `CompanyProfile` view over the
/// retained raw payload. The view concentrates field-name knowledge
/// (`companyName`, `mktCap`, `price` …) so tool handlers read typed accessors
/// instead of `v.get("companyName").and_then(|v| v.as_str())`. A missing
/// field is `None`, not a silent zero — the algedonic `tracing::warn!`
/// pattern from `parse_financial_field` extends to the accessors.
pub async fn fetch_company_profile(
    client: &reqwest::Client,
    symbol: &str,
    fmp_api_key: &str,
    eodhd_api_key: &str,
    learning: Option<&super::LearningState>,
) -> Result<CompanyProfile, McpToolError> {
    let raw = companies_get(
        client,
        "company_profile",
        symbol,
        fmp_api_key,
        eodhd_api_key,
        &[],
        learning,
    )
    .await?;
    Ok(CompanyProfile::from_raw(raw))
}

/// Fetch key metrics as a typed `KeyMetrics` view over the retained raw array.
pub async fn fetch_key_metrics(
    client: &reqwest::Client,
    symbol: &str,
    limit: usize,
    fmp_api_key: &str,
    eodhd_api_key: &str,
    learning: Option<&super::LearningState>,
) -> Result<KeyMetrics, McpToolError> {
    let limit_str = limit.to_string();
    let raw = companies_get(
        client,
        "key_metrics",
        symbol,
        fmp_api_key,
        eodhd_api_key,
        &[("limit", &limit_str)],
        learning,
    )
    .await?;
    Ok(KeyMetrics::from_raw(raw))
}

/// Fetch historical prices as a typed `HistoricalPriceView` over the
/// `{"symbol": ..., "historical": [...]}` envelope.
pub async fn fetch_historical_price(
    client: &reqwest::Client,
    symbol: &str,
    from: &str,
    to: &str,
    fmp_api_key: &str,
    eodhd_api_key: &str,
    learning: Option<&super::LearningState>,
) -> Result<HistoricalPriceView, McpToolError> {
    let raw = companies_get(
        client,
        "historical_price",
        symbol,
        fmp_api_key,
        eodhd_api_key,
        &[("from", from), ("to", to)],
        learning,
    )
    .await?;
    Ok(HistoricalPriceView::from_raw(raw))
}

/// Approximated field count for EODHD key_metrics normalization (FinGPT §3.2).
const APPROXIMATED_KEY_METRICS_FIELDS: u32 = 4;

/// Emit a Regulation span for provider quality monitoring (FinGPT §3.2 — data quality feedback loop).
fn emit_provider_reg(tool: &str, symbol: &str, provider: &str, was_normalised: bool) {
    let approx_count = if was_normalised {
        match tool {
            "key_metrics" => APPROXIMATED_KEY_METRICS_FIELDS,
            _ => 0,
        }
    } else {
        0
    };
    tracing::debug!(
        target: "hkask.mcp.companies.data_quality",
        symbol = %symbol,
        tool = %tool,
        provider = %provider,
        was_normalised = %was_normalised,
        approximated_fields = %approx_count,
        "Provider quality: Regulation data_quality span"
    );
}

// ── FMP API caller ─────────────────────────────────────────────────

async fn fmp_get(
    client: &reqwest::Client,
    path: &str,
    api_key: &str,
    symbol: &str,
    extra_params: &[(&str, &str)],
) -> Result<Value, McpToolError> {
    let url = format!("{FMP_BASE_URL}{path}");
    let mut query: Vec<(&str, &str)> = vec![("symbol", symbol), ("apikey", api_key)];
    query.extend_from_slice(extra_params);

    let resp = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("FMP request failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("FMP", status, &body));
    }

    serde_json::from_str(&body)
        .map_err(|e| McpToolError::unavailable(format!("failed to parse FMP response: {e}")))
}

// ── EODHD API caller ───────────────────────────────────────────────

async fn eodhd_get(
    client: &reqwest::Client,
    path: &str,
    api_key: &str,
    symbol: &str,
    extra_params: &[(&str, &str)],
) -> Result<Value, McpToolError> {
    let url = format!("{EODHD_BASE_URL}{path}/{symbol}");
    let mut query: Vec<(&str, &str)> = vec![("api_token", api_key), ("fmt", "json")];
    query.extend_from_slice(extra_params);

    let resp = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("EODHD request failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("EODHD", status, &body));
    }

    serde_json::from_str(&body)
        .map_err(|e| McpToolError::unavailable(format!("failed to parse EODHD response: {e}")))
}

// ── EODHD → FMP format normalizers ─────────────────────────────────
//
// EODHD's /fundamentals/{symbol} returns a deeply nested object.
// These functions extract and reshape data to match FMP's flat array format
// so that analysis.rs functions work unchanged.

/// Normalize EODHD response based on which logical tool endpoint was requested.
fn normalize_eodhd(tool: &str, eodhd_value: &Value, symbol: &str) -> Value {
    match tool {
        "company_profile" => normalize_eodhd_profile(eodhd_value),
        "income_statement" => normalize_eodhd_income_statement(eodhd_value),
        "balance_sheet" => normalize_eodhd_balance_sheet(eodhd_value),
        "cash_flow_statement" => normalize_eodhd_cash_flow(eodhd_value),
        "key_metrics" => normalize_eodhd_key_metrics(eodhd_value),
        "historical_price" => normalize_eodhd_historical(eodhd_value, symbol),
        _ => eodhd_value.clone(),
    }
}

/// Extract company profile from EODHD General section → FMP profile format.
fn normalize_eodhd_profile(fundamentals: &Value) -> Value {
    let general = fundamentals.get("General");
    match general {
        Some(g) => {
            // FMP profile returns an array with one element
            Value::Array(vec![g.clone()])
        }
        None => Value::Array(vec![]),
    }
}

/// Extract income statements from EODHD Financials.Income_Statement.yearly → FMP format.
fn normalize_eodhd_income_statement(fundamentals: &Value) -> Value {
    let yearly = fundamentals
        .get("Financials")
        .and_then(|f| f.get("Income_Statement"))
        .and_then(|is| is.get("yearly"));

    match yearly {
        Some(Value::Object(map)) => {
            let mut items: Vec<Value> = map
                .iter()
                .map(|(date, stmt)| {
                    let mut obj = stmt.clone();
                    // Ensure calendarYear field exists (FMP uses this)
                    if let Some(obj_map) = obj.as_object_mut() {
                        let year = date.split('-').next().unwrap_or(date);
                        obj_map
                            .entry("calendarYear".to_string())
                            .or_insert_with(|| Value::String(year.to_string()));
                        obj_map
                            .entry("date".to_string())
                            .or_insert_with(|| Value::String(date.to_string()));
                    }
                    obj
                })
                .collect();
            // Sort by date descending (newest first, like FMP)
            items.sort_by(|a, b| {
                let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
                let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
                db.cmp(da)
            });
            Value::Array(items)
        }
        _ => Value::Array(vec![]),
    }
}

/// Extract balance sheets from EODHD Financials.Balance_Sheet.yearly → FMP format.
fn normalize_eodhd_balance_sheet(fundamentals: &Value) -> Value {
    let yearly = fundamentals
        .get("Financials")
        .and_then(|f| f.get("Balance_Sheet"))
        .and_then(|bs| bs.get("yearly"));

    match yearly {
        Some(Value::Object(map)) => {
            let mut items: Vec<Value> = map
                .iter()
                .map(|(date, sheet)| {
                    let mut obj = sheet.clone();
                    if let Some(obj_map) = obj.as_object_mut() {
                        let year = date.split('-').next().unwrap_or(date);
                        obj_map
                            .entry("calendarYear".to_string())
                            .or_insert_with(|| Value::String(year.to_string()));
                        obj_map
                            .entry("date".to_string())
                            .or_insert_with(|| Value::String(date.to_string()));
                    }
                    obj
                })
                .collect();
            items.sort_by(|a, b| {
                let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
                let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
                db.cmp(da)
            });
            Value::Array(items)
        }
        _ => Value::Array(vec![]),
    }
}

/// Extract cash flow statements from EODHD Financials.Cash_Flow.yearly → FMP format.
fn normalize_eodhd_cash_flow(fundamentals: &Value) -> Value {
    let yearly = fundamentals
        .get("Financials")
        .and_then(|f| f.get("Cash_Flow"))
        .and_then(|cf| cf.get("yearly"));

    match yearly {
        Some(Value::Object(map)) => {
            let mut items: Vec<Value> = map
                .iter()
                .map(|(date, flow)| {
                    let mut obj = flow.clone();
                    if let Some(obj_map) = obj.as_object_mut() {
                        let year = date.split('-').next().unwrap_or(date);
                        obj_map
                            .entry("calendarYear".to_string())
                            .or_insert_with(|| Value::String(year.to_string()));
                        obj_map
                            .entry("date".to_string())
                            .or_insert_with(|| Value::String(date.to_string()));
                    }
                    obj
                })
                .collect();
            items.sort_by(|a, b| {
                let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
                let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
                db.cmp(da)
            });
            Value::Array(items)
        }
        _ => Value::Array(vec![]),
    }
}

/// Build key-metrics array from EODHD Highlights + Earnings.History + Financials → FMP format.
///
/// FMP key-metrics is an array of yearly objects with fields like:
/// grossProfitMargin, roic, daysOfPayablesOutstanding, daysOfSalesOutstanding,
/// calendarYear, period, etc.
///
/// EODHD provides Highlights (latest snapshot), Earnings.History (yearly earnings),
/// and Financials (yearly balance sheet + income statement). We combine them
/// and compute derived metrics so MAIA analysis functions work.
///
/// Note: EODHD-derived metrics are best-effort approximations. MAIA deep
/// fundamental analysis works best with FMP's native key-metrics endpoint.
fn normalize_eodhd_key_metrics(fundamentals: &Value) -> Value {
    let highlights = fundamentals.get("Highlights");
    let earnings_history = fundamentals.get("Earnings").and_then(|e| e.get("History"));
    let income_yearly = fundamentals
        .get("Financials")
        .and_then(|f| f.get("Income_Statement"))
        .and_then(|is| is.get("yearly"));
    let balance_yearly = fundamentals
        .get("Financials")
        .and_then(|f| f.get("Balance_Sheet"))
        .and_then(|bs| bs.get("yearly"));

    // Build per-year objects from Earnings.History, enriched with computed metrics
    let mut items: Vec<Value> = match earnings_history {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(date, earnings)| {
                let year = date.split('-').next().unwrap_or(date);
                let mut obj = serde_json::json!({
                    "calendarYear": year,
                    "date": date,
                    "period": "FY",
                });

                // Copy earnings fields
                if let Some(obj_map) = obj.as_object_mut()
                    && let Some(e_obj) = earnings.as_object()
                {
                    for (key, value) in e_obj {
                        obj_map.insert(key.clone(), value.clone());
                    }
                }

                // Compute derived metrics from financial statements for this year
                compute_year_metrics(&mut obj, date, income_yearly, balance_yearly);

                obj
            })
            .collect(),
        _ => vec![],
    };

    // Sort by date descending (newest first, like FMP)
    items.sort_by(|a, b| {
        let da = a.get("date").and_then(|v| v.as_str()).unwrap_or("");
        let db = b.get("date").and_then(|v| v.as_str()).unwrap_or("");
        db.cmp(da)
    });

    // Merge Highlights data into the latest year's entry (now first after sort)
    if let (Some(highlights), Some(first)) = (highlights, items.first_mut())
        && let (Some(h_obj), Some(f_map)) = (highlights.as_object(), first.as_object_mut())
    {
        for (key, value) in h_obj {
            f_map.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }

    Value::Array(items)
}

/// Compute derived financial metrics for a single year from EODHD financial statements.
///
/// Looks up the Income_Statement and Balance_Sheet entries matching `date` and computes:
/// - grossProfitMargin = grossProfit / revenue
/// - roic = netIncome / totalAssets (simplified approximation)
/// - daysOfPayablesOutstanding = accountsPayable / (costOfRevenue / 365)
/// - daysOfSalesOutstanding = accountsReceivable / (revenue / 365)
fn compute_year_metrics(
    obj: &mut Value,
    date: &str,
    income_yearly: Option<&Value>,
    balance_yearly: Option<&Value>,
) {
    let income_entry = income_yearly.and_then(|iy| iy.get(date));
    let balance_entry = balance_yearly.and_then(|by| by.get(date));

    let obj_map = match obj.as_object_mut() {
        Some(m) => m,
        None => return,
    };

    // ── grossProfitMargin ──
    if let Some(income) = income_entry {
        let revenue = income.get("revenue").and_then(|v| v.as_f64());
        let gross_profit = income.get("grossProfit").and_then(|v| v.as_f64());
        if let (Some(rev), Some(gp)) = (revenue, gross_profit)
            && rev > 0.0
        {
            obj_map
                .entry("grossProfitMargin".to_string())
                .or_insert(Value::from(gp / rev));
        }

        // ── roic (simplified: netIncome / totalAssets) ──
        let net_income = income.get("netIncome").and_then(|v| v.as_f64());
        if let Some(balance) = balance_entry {
            let total_assets = balance.get("totalAssets").and_then(|v| v.as_f64());
            if let (Some(ni), Some(ta)) = (net_income, total_assets)
                && ta > 0.0
            {
                obj_map
                    .entry("roic".to_string())
                    .or_insert(Value::from(ni / ta));
            }
        }

        // ── daysOfPayablesOutstanding ──
        // DPO = accountsPayable / (costOfRevenue / 365)
        let cost_of_revenue = income
            .get("costOfRevenue")
            .or_else(|| income.get("costOfGoodsSold"))
            .and_then(|v| v.as_f64());
        if let Some(balance) = balance_entry {
            let accounts_payable = balance.get("accountsPayable").and_then(|v| v.as_f64());
            if let (Some(ap), Some(cor)) = (accounts_payable, cost_of_revenue)
                && cor > 0.0
            {
                obj_map
                    .entry("daysOfPayablesOutstanding".to_string())
                    .or_insert(Value::from(ap / (cor / 365.0)));
            }
        }

        // ── daysOfSalesOutstanding ──
        // DSO = accountsReceivable / (revenue / 365)
        if let Some(balance) = balance_entry {
            let accounts_receivable = balance.get("accountsReceivable").and_then(|v| v.as_f64());
            if let (Some(ar), Some(rev)) = (accounts_receivable, revenue)
                && rev > 0.0
            {
                obj_map
                    .entry("daysOfSalesOutstanding".to_string())
                    .or_insert(Value::from(ar / (rev / 365.0)));
            }
        }
    }
}

/// Normalize EODHD /eod/{symbol} historical prices → FMP historical-price-full format.
///
/// EODHD returns an array of {date, open, high, low, close, adjusted_close, volume}.
/// FMP returns {symbol, historical: [{date, open, high, low, close, adjClose, volume, ...}]}.
fn normalize_eodhd_historical(eod_value: &Value, symbol: &str) -> Value {
    let historical = match eod_value {
        Value::Array(arr) => Value::Array(arr.clone()),
        _ => Value::Array(vec![]),
    };

    serde_json::json!({
        "symbol": symbol,
        "historical": historical,
    })
}

// ── Search functions (query-based, not symbol-based) ───────────────

/// FMP symbol search by name query.
pub async fn fmp_search_get(
    client: &reqwest::Client,
    query: &str,
    limit: &str,
    api_key: &str,
) -> Result<Value, McpToolError> {
    let url = format!("{FMP_BASE_URL}/search-name");
    let resp = client
        .get(&url)
        .query(&[("query", query), ("limit", limit), ("apikey", api_key)])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("FMP search failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("FMP", status, &body));
    }

    serde_json::from_str(&body)
        .map_err(|e| McpToolError::unavailable(format!("failed to parse FMP search response: {e}")))
}

/// EODHD symbol search by name query.
pub async fn eodhd_search_get(
    client: &reqwest::Client,
    query: &str,
    limit: &str,
    api_key: &str,
) -> Result<Value, McpToolError> {
    let url = format!("{EODHD_BASE_URL}/search/{query}");
    let resp = client
        .get(&url)
        .query(&[("api_token", api_key), ("limit", limit), ("fmt", "json")])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("EODHD search failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("response body read failed: {e}")))?;
    if !status.is_success() {
        return Err(classify_http_error("EODHD", status, &body));
    }

    serde_json::from_str(&body).map_err(|e| {
        McpToolError::unavailable(format!("failed to parse EODHD search response: {e}"))
    })
}

// ── Tests ──────────────────────────────────────────────────────────
