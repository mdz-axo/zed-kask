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

    /// `marketCap` (FMP stable) — the market capitalization.
    /// Also checks `mktCap` (EODHD field name) for EODHD compatibility.
    pub fn market_cap(&self) -> Option<f64> {
        self.first()?
            .get("marketCap")
            .or_else(|| self.first()?.get("mktCap"))
            .and_then(|v| v.as_f64())
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
    /// FMP stable moved this to `/ratios` as `priceToEarningsRatio`;
    /// `fetch_key_metrics` merges ratios fields in, so both names resolve.
    pub fn pe_ratio(&self) -> Option<f64> {
        self.latest()?
            .get("peRatio")
            .or_else(|| self.latest()?.get("priceToEarningsRatio"))
            .and_then(|v| v.as_f64())
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
    /// year — the EV/EBITDA multiple. FMP stable renamed the field to
    /// `evToEBITDA` (uppercase); all three spellings are checked.
    pub fn ev_to_ebitda(&self) -> Option<f64> {
        let latest = self.latest()?;
        latest
            .get("evToEBITDA")
            .or_else(|| latest.get("evToEbitda"))
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

/// Typed view over historical prices (FMP `/historical-price-eod/full` or EODHD
/// `/eod`).
///
/// FMP stable returns a flat array of daily OHLCV bars `[{symbol, date, close, ...}]`.
/// EODHD (normalized) returns the `{symbol, historical: [...]}` envelope.
/// Both shapes are handled — `historical()` returns the bar array from either.
pub(crate) struct HistoricalPriceView {
    raw: Value,
}

impl HistoricalPriceView {
    /// Wrap a normalized historical-price payload.
    pub fn from_raw(raw: Value) -> Self {
        Self { raw }
    }

    /// The array of daily OHLCV bars (newest-first per FMP), or an empty slice
    /// if the payload has no bars. Handles both the FMP stable flat array and
    /// the EODHD `{symbol, historical: [...]}` envelope.
    pub fn historical(&self) -> &[Value] {
        // FMP stable: flat array of bar objects.
        if let Some(arr) = self.raw.as_array() {
            return arr;
        }
        // EODHD normalized: {symbol, historical: [...]}.
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
            fmp_path: "/historical-price-eod/full",
            eodhd_path: "/eod",
            normalize_eodhd: true,
        }),
        "ratios" => Some(EndpointMapping {
            fmp_path: "/ratios",
            eodhd_path: "/fundamentals",
            normalize_eodhd: true,
        }),
        "financial_growth" => Some(EndpointMapping {
            fmp_path: "/financial-growth",
            eodhd_path: "/fundamentals",
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
    // Symbols with an exchange suffix (e.g., VOD.LSE, 0700.HK) are
    // international. The .US suffix is a US listing (FMP primary).
    if let Some(exchange) = symbol.split('.').nth(1) {
        exchange != "US"
    } else {
        // No suffix — plain ticker, assumed US (FMP primary).
        false
    }
}

fn primary_provider(symbol: &str) -> Provider {
    if is_international_symbol(symbol) {
        Provider::Eodhd
    } else {
        Provider::Fmp
    }
}

// ── Provider response with provenance ─────────────────────────────

/// The result of a provider fetch — the raw JSON value plus which provider
/// served it. The cache stores the provider so consumers can distinguish
/// FMP-sourced data from EODHD-sourced data.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub value: Value,
    pub provider: Provider,
}

// ── Main routing function ──────────────────────────────────────────

/// Fetch data for a logical tool endpoint, trying primary provider first
/// then falling back to secondary. Normalizes EODHD responses to match
/// FMP format when needed. Returns the response with the provider that
/// served it so the FIBO cache can tag the provenance.
pub async fn companies_get(
    client: &reqwest::Client,
    tool: &str,
    symbol: &str,
    fmp_api_key: &str,
    eodhd_api_key: &str,
    extra_params: &[(&str, &str)],
    learning: Option<&super::LearningState>,
) -> Result<ProviderResponse, McpToolError> {
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
                Ok(ProviderResponse {
                    value: normalize_eodhd(tool, &value, symbol),
                    provider: Provider::Eodhd,
                })
            } else {
                Ok(ProviderResponse {
                    value,
                    provider: primary,
                })
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
                        Ok(ProviderResponse {
                            value: normalize_eodhd(tool, &value, symbol),
                            provider: Provider::Eodhd,
                        })
                    } else {
                        Ok(ProviderResponse {
                            value,
                            provider: secondary,
                        })
                    }
                }
                Err(e) => Err(e),
            }
        }
    }
}

/// Fetch key metrics as a typed `KeyMetrics` view over the retained raw array.
///
/// FMP's stable API split the old key-metrics response across three endpoints:
/// - `/stable/key-metrics` — ROIC, ROE, DSO, DPO, cash conversion cycle, etc.
/// - `/stable/ratios` — P/E, P/B, P/S, dividend yield, gross profit margin, etc.
/// - `/stable/financial-growth` — revenue growth, net income growth, etc.
///
/// This function fetches all three and merges relevant fields into each
/// key-metrics entry (matched by date) so downstream code and typed accessors
/// see the same field set as the old single-endpoint response. Field aliases
/// (`roic` for `returnOnInvestedCapital`, `calendarYear` for `fiscalYear`)
/// are added so existing `.get("fieldName")` calls continue to work.
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

    // Enrich with ratios and financial-growth data from FMP.
    // These supplementary fetches are best-effort — if they fail, the key-metrics
    // data is still returned (with None for the moved fields).
    let ratios_raw = fmp_get(
        client,
        "/ratios",
        fmp_api_key,
        symbol,
        &[("limit", &limit_str)],
    )
    .await
    .ok();
    let growth_raw = fmp_get(
        client,
        "/financial-growth",
        fmp_api_key,
        symbol,
        &[("limit", &limit_str)],
    )
    .await
    .ok();

    let enriched = enrich_key_metrics(raw.value, ratios_raw, growth_raw);
    Ok(KeyMetrics::from_raw(enriched))
}

/// Fetch historical prices as a typed `HistoricalPriceView`.
///
/// FMP stable returns a flat array of daily bars; EODHD (normalized) returns
/// the EODHD `{symbol, historical: [...]}` envelope. `HistoricalPriceView`
/// handles both shapes.
#[allow(dead_code)]
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
    Ok(HistoricalPriceView::from_raw(raw.value))
}

/// Merge ratios and financial-growth fields into key-metrics entries by date.
///
/// FMP's stable API split the old key-metrics response across three endpoints.
/// This function merges the moved fields back into each key-metrics entry so
/// downstream code sees the same field set as before. Also adds field aliases
/// for renamed fields (`roic` for `returnOnInvestedCapital`, `calendarYear`
/// for `fiscalYear`) so existing `.get("fieldName")` calls continue to work.
fn enrich_key_metrics(mut raw: Value, ratios: Option<Value>, growth: Option<Value>) -> Value {
    let Some(entries) = raw.as_array_mut() else {
        return raw;
    };

    // Build lookup maps by date for ratios and growth data.
    let ratios_by_date: std::collections::HashMap<String, &Value> = ratios
        .as_ref()
        .and_then(|r| r.as_array())
        .map_or(std::collections::HashMap::new(), |arr| {
            arr.iter()
                .filter_map(|e| {
                    e.get("date")
                        .and_then(|d| d.as_str())
                        .map(|d| (d.to_string(), e))
                })
                .collect()
        });

    let growth_by_date: std::collections::HashMap<String, &Value> = growth
        .as_ref()
        .and_then(|g| g.as_array())
        .map_or(std::collections::HashMap::new(), |arr| {
            arr.iter()
                .filter_map(|e| {
                    e.get("date")
                        .and_then(|d| d.as_str())
                        .map(|d| (d.to_string(), e))
                })
                .collect()
        });

    // Fields from /stable/ratios that were in the old key-metrics response.
    const RATIOS_FIELDS: &[&str] = &[
        "priceToEarningsRatio",
        "priceToBookRatio",
        "priceToSalesRatio",
        "dividendYield",
        "grossProfitMargin",
        "enterpriseValueMultiple",
        "debtToEquityRatio",
        "effectiveTaxRate",
    ];

    // Fields from /stable/financial-growth that were in the old key-metrics response.
    const GROWTH_FIELDS: &[&str] = &[
        "revenueGrowth",
        "grossProfitGrowth",
        "ebitgrowth",
        "operatingIncomeGrowth",
        "netIncomeGrowth",
        "epsgrowth",
        "epsdilutedGrowth",
        "freeCashFlowGrowth",
    ];

    for entry in entries {
        if let Some(obj) = entry.as_object_mut() {
            // Add calendarYear alias for fiscalYear (old field name).
            if !obj.contains_key("calendarYear") {
                if let Some(fy) = obj.get("fiscalYear").and_then(|v| v.as_str()) {
                    obj.insert("calendarYear".to_string(), Value::String(fy.to_string()));
                }
            }

            // Add roic alias for returnOnInvestedCapital (old field name).
            if !obj.contains_key("roic") {
                if let Some(roic) = obj.get("returnOnInvestedCapital") {
                    obj.insert("roic".to_string(), roic.clone());
                }
            }

            // Merge ratios and growth fields by matching date.
            // Extract the date string first to avoid holding an immutable borrow
            // across the mutable `obj.insert` calls below.
            let date_str = obj
                .get("date")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            if let Some(date) = date_str.as_deref() {
                // Merge ratios fields.
                if let Some(ratios_entry) = ratios_by_date.get(date).and_then(|r| r.as_object()) {
                    for field in RATIOS_FIELDS {
                        if !obj.contains_key(*field) {
                            if let Some(val) = ratios_entry.get(*field) {
                                obj.insert(field.to_string(), val.clone());
                            }
                        }
                    }
                }

                // Merge growth fields.
                if let Some(growth_entry) = growth_by_date.get(date).and_then(|g| g.as_object()) {
                    for field in GROWTH_FIELDS {
                        if !obj.contains_key(*field) {
                            if let Some(val) = growth_entry.get(*field) {
                                obj.insert(field.to_string(), val.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    raw
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

/// Copy a field from one key to another if the target is absent.
/// Used to map EODHD field names to FMP field names.
fn map_field(map: &mut serde_json::Map<String, Value>, from: &str, to: &str) {
    if !map.contains_key(to) {
        if let Some(v) = map.get(from) {
            map.insert(to.to_string(), v.clone());
        }
    }
}

/// Normalize EODHD response based on which logical tool endpoint was requested.
fn normalize_eodhd(tool: &str, eodhd_value: &Value, symbol: &str) -> Value {
    match tool {
        "company_profile" => normalize_eodhd_profile(eodhd_value),
        "income_statement" => normalize_eodhd_income_statement(eodhd_value),
        "balance_sheet" => normalize_eodhd_balance_sheet(eodhd_value),
        "cash_flow_statement" => normalize_eodhd_cash_flow(eodhd_value),
        "key_metrics" | "ratios" | "financial_growth" => {
            // EODHD fundamentals is a single endpoint — ratios and growth data
            // are computed from the same financial statements. The key_metrics
            // normalizer already computes grossProfitMargin, roic, DPO, DSO,
            // and merges Highlights fields (dividendYield, marketCap, etc.).
            normalize_eodhd_key_metrics(eodhd_value)
        }
        "historical_price" => normalize_eodhd_historical(eodhd_value, symbol),
        _ => eodhd_value.clone(),
    }
}

/// Extract company profile from EODHD General + Highlights → FMP profile format.
///
/// EODHD's General section uses PascalCase field names (Code, Name,
/// MarketCapitalization) while FMP uses camelCase (symbol, companyName,
/// marketCap). We map the key fields and also pull MarketCapitalization from
/// Highlights since General doesn't include it.
fn normalize_eodhd_profile(fundamentals: &Value) -> Value {
    let general = fundamentals.get("General");
    let highlights = fundamentals.get("Highlights");
    match general {
        Some(g) => {
            let mut obj = g.clone();
            if let Some(map) = obj.as_object_mut() {
                // Map EODHD General fields → FMP profile field names.
                map_field(map, "Code", "symbol");
                map_field(map, "Name", "companyName");
                map_field(map, "GicSector", "sector");
                map_field(map, "Industry", "industry");
                map_field(map, "CurrencyCode", "currency");
                map_field(map, "Exchange", "exchange");
                map_field(map, "CountryISO", "country");
                map_field(map, "FullTimeEmployees", "fullTimeEmployees");
                map_field(map, "Description", "description");
                map_field(map, "WebURL", "website");
                map_field(map, "ISIN", "isin");
                map_field(map, "CIK", "cik");
                map_field(map, "IPODate", "ipoDate");

                // Market cap: EODHD puts this in Highlights, not General.
                if !map.contains_key("marketCap") {
                    if let Some(h) = highlights {
                        if let Some(mc) = h.get("MarketCapitalization") {
                            map.insert("marketCap".to_string(), mc.clone());
                        }
                    }
                }

                // Shares outstanding: EODHD puts this in the latest balance sheet.
                if !map.contains_key("sharesOutstanding") {
                    if let Some(bs) = fundamentals
                        .get("Financials")
                        .and_then(|f| f.get("Balance_Sheet"))
                        .and_then(|bs| bs.get("yearly"))
                        .and_then(|y| y.as_object())
                        .and_then(|m| m.iter().max_by_key(|(k, _)| k.as_str()))
                        .map(|(_, v)| v)
                    {
                        if let Some(shares) = bs.get("commonStockSharesOutstanding") {
                            map.insert("sharesOutstanding".to_string(), shares.clone());
                        }
                    }
                }
            }
            Value::Array(vec![obj])
        }
        None => Value::Array(vec![]),
    }
}

/// Extract income statements from EODHD Financials.Income_Statement.yearly → FMP format.
///
/// Maps EODHD field names to FMP equivalents (e.g. totalRevenue → revenue)
/// and adds calendarYear/date fields for downstream compatibility.
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
                    if let Some(obj_map) = obj.as_object_mut() {
                        let year = date.split('-').next().unwrap_or(date);
                        obj_map
                            .entry("calendarYear".to_string())
                            .or_insert_with(|| Value::String(year.to_string()));
                        obj_map
                            .entry("date".to_string())
                            .or_insert_with(|| Value::String(date.to_string()));
                        // Map EODHD field names → FMP field names
                        map_field(obj_map, "totalRevenue", "revenue");
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
///
/// Maps EODHD field names to FMP equivalents (e.g. totalLiab → totalLiabilities,
/// totalStockholderEquity → totalEquity/totalStockholdersEquity).
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
                        // Map EODHD field names → FMP field names
                        map_field(obj_map, "totalLiab", "totalLiabilities");
                        map_field(obj_map, "totalStockholderEquity", "totalStockholdersEquity");
                        map_field(obj_map, "totalStockholderEquity", "totalEquity");
                        map_field(
                            obj_map,
                            "cashAndShortTermInvestments",
                            "cashAndCashEquivalents",
                        );
                        map_field(obj_map, "commonStockSharesOutstanding", "sharesOutstanding");
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
///
/// Maps EODHD field names to FMP equivalents (e.g. totalCashFromOperatingActivities
/// → netCashProvidedByOperatingActivities, capitalExpenditures → capitalExpenditure,
/// depreciation → depreciationAndAmortization).
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
                        // Map EODHD field names → FMP field names
                        map_field(
                            obj_map,
                            "totalCashFromOperatingActivities",
                            "netCashProvidedByOperatingActivities",
                        );
                        map_field(
                            obj_map,
                            "totalCashflowsFromInvestingActivities",
                            "netCashProvidedByInvestingActivities",
                        );
                        map_field(
                            obj_map,
                            "totalCashFromFinancingActivities",
                            "netCashProvidedByFinancingActivities",
                        );
                        map_field(obj_map, "capitalExpenditures", "capitalExpenditure");
                        map_field(obj_map, "depreciation", "depreciationAndAmortization");
                        map_field(
                            obj_map,
                            "totalCashFromOperatingActivities",
                            "operatingCashFlow",
                        );
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

    // Merge Highlights data into the latest year's entry (now first after sort).
    // Map EODHD Highlights field names → FMP field names.
    if let (Some(highlights), Some(first)) = (highlights, items.first_mut())
        && let (Some(h_obj), Some(f_map)) = (highlights.as_object(), first.as_object_mut())
    {
        // First copy raw Highlights fields.
        for (key, value) in h_obj {
            f_map.entry(key.clone()).or_insert_with(|| value.clone());
        }
        // Then add FMP-compatible aliases.
        map_field(f_map, "MarketCapitalization", "marketCap");
        map_field(f_map, "DividendYield", "dividendYield");
        map_field(f_map, "EarningsShare", "eps");
        map_field(f_map, "DilutedEpsTTM", "epsDiluted");
        map_field(f_map, "ReturnOnEquityTTM", "returnOnEquity");
        map_field(f_map, "ReturnOnAssetsTTM", "returnOnAssets");
        map_field(f_map, "BookValue", "bookValuePerShare");
        map_field(f_map, "RevenuePerShareTTM", "revenuePerShare");
        map_field(f_map, "DividendShare", "dividendPerShare");
        map_field(f_map, "RevenueTTM", "revenueTTM");
        map_field(f_map, "GrossprofitTTM", "grossProfitTTM");
        map_field(f_map, "EBITDA", "ebitdaTTM");
        map_field(f_map, "PEGRatio", "pegRatio");

        // Compute valuation ratios from available data.
        // P/E = MarketCap / netIncome
        // P/B = MarketCap / totalEquity
        // P/S = MarketCap / revenue
        // EV/EBITDA = (MarketCap + netDebt) / EBITDA
        compute_valuation_ratios(f_map, income_yearly, balance_yearly, Some(highlights));
    }

    // Compute revenue growth year-over-year from income statement data.
    compute_revenue_growth(&mut items, income_yearly);

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
        let revenue = income
            .get("revenue")
            .or_else(|| income.get("totalRevenue"))
            .and_then(|v| v.as_f64());
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
                let roic_val = ni / ta;
                obj_map
                    .entry("roic".to_string())
                    .or_insert(Value::from(roic_val));
                // Also add returnOnInvestedCapital alias for code that checks the new field name.
                obj_map
                    .entry("returnOnInvestedCapital".to_string())
                    .or_insert(Value::from(roic_val));
            }

            // ── investedCapital ──
            if let Some(ic) = balance.get("netInvestedCapital").and_then(|v| v.as_f64()) {
                obj_map
                    .entry("investedCapital".to_string())
                    .or_insert(Value::from(ic));
            }

            // ── daysOfInventoryOutstanding ──
            // DIO = inventory / (costOfRevenue / 365)
            let inventory = balance.get("inventory").and_then(|v| v.as_f64());
            let cost_of_revenue = income
                .get("costOfRevenue")
                .or_else(|| income.get("costOfGoodsSold"))
                .and_then(|v| v.as_f64());
            if let (Some(inv), Some(cor)) = (inventory, cost_of_revenue)
                && cor > 0.0
            {
                let dio = inv / (cor / 365.0);
                obj_map
                    .entry("daysOfInventoryOutstanding".to_string())
                    .or_insert(Value::from(dio));
            }
        }

        // ── daysOfPayablesOutstanding ──
        // DPO = accountsPayable / (costOfRevenue / 365)
        let cost_of_revenue = income
            .get("costOfRevenue")
            .or_else(|| income.get("costOfGoodsSold"))
            .and_then(|v| v.as_f64());
        if let Some(balance) = balance_entry {
            let accounts_payable = balance
                .get("accountsPayable")
                .or_else(|| balance.get("accountPayables"))
                .and_then(|v| v.as_f64());
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
            let accounts_receivable = balance
                .get("netReceivables")
                .or_else(|| balance.get("accountsReceivables"))
                .and_then(|v| v.as_f64());
            if let (Some(ar), Some(rev)) = (accounts_receivable, revenue)
                && rev > 0.0
            {
                obj_map
                    .entry("daysOfSalesOutstanding".to_string())
                    .or_insert(Value::from(ar / (rev / 365.0)));
            }
        }
    }

    // ── operatingCycle and cashConversionCycle ──
    // OC = DIO + DSO; CCC = OC - DPO
    if let (Some(dio), Some(dso), Some(dpo)) = (
        obj_map
            .get("daysOfInventoryOutstanding")
            .and_then(|v| v.as_f64()),
        obj_map
            .get("daysOfSalesOutstanding")
            .and_then(|v| v.as_f64()),
        obj_map
            .get("daysOfPayablesOutstanding")
            .and_then(|v| v.as_f64()),
    ) {
        obj_map
            .entry("operatingCycle".to_string())
            .or_insert(Value::from(dio + dso));
        obj_map
            .entry("cashConversionCycle".to_string())
            .or_insert(Value::from(dio + dso - dpo));
    }
}

/// Compute valuation ratios (P/E, P/B, P/S, EV/EBITDA) for the latest year's
/// key-metrics entry from EODHD Highlights + financial statement data.
///
/// These ratios were in FMP's old key-metrics endpoint but moved to /ratios in
/// the stable API. For EODHD-primary symbols, we compute them from the raw
/// data that's already available.
fn compute_valuation_ratios(
    f_map: &mut serde_json::Map<String, Value>,
    income_yearly: Option<&Value>,
    balance_yearly: Option<&Value>,
    highlights: Option<&Value>,
) {
    let market_cap = f_map
        .get("marketCap")
        .or_else(|| f_map.get("MarketCapitalization"))
        .and_then(|v| v.as_f64());

    let Some(market_cap) = market_cap else {
        return;
    };

    // Get the latest date from the entry to look up financial statements.
    let date = f_map.get("date").and_then(|v| v.as_str());
    let income_entry = date.and_then(|d| income_yearly.and_then(|iy| iy.get(d)));
    let balance_entry = date.and_then(|d| balance_yearly.and_then(|by| by.get(d)));

    // P/E = MarketCap / netIncome
    if !f_map.contains_key("peRatio") && !f_map.contains_key("priceToEarningsRatio") {
        if let Some(income) = income_entry {
            let net_income = income.get("netIncome").and_then(|v| v.as_f64());
            if let Some(ni) = net_income {
                if ni > 0.0 {
                    f_map.insert("peRatio".to_string(), Value::from(market_cap / ni));
                    f_map.insert(
                        "priceToEarningsRatio".to_string(),
                        Value::from(market_cap / ni),
                    );
                }
            }
        }
    }

    // P/B = MarketCap / totalEquity
    if !f_map.contains_key("priceToBookRatio") {
        if let Some(balance) = balance_entry {
            let equity = balance
                .get("totalStockholderEquity")
                .or_else(|| balance.get("totalEquity"))
                .or_else(|| balance.get("totalStockholdersEquity"))
                .and_then(|v| v.as_f64());
            if let Some(eq) = equity {
                if eq > 0.0 {
                    f_map.insert("priceToBookRatio".to_string(), Value::from(market_cap / eq));
                }
            }
        }
    }

    // P/S = MarketCap / revenue
    if !f_map.contains_key("priceToSalesRatio") {
        if let Some(income) = income_entry {
            let revenue = income
                .get("revenue")
                .or_else(|| income.get("totalRevenue"))
                .and_then(|v| v.as_f64());
            if let Some(rev) = revenue {
                if rev > 0.0 {
                    f_map.insert(
                        "priceToSalesRatio".to_string(),
                        Value::from(market_cap / rev),
                    );
                }
            }
        }
    }

    // EV/EBITDA = (MarketCap + netDebt) / EBITDA
    if !f_map.contains_key("evToEBITDA") && !f_map.contains_key("evToEbitda") {
        let ebitda = highlights
            .and_then(|h| h.get("EBITDA"))
            .or_else(|| f_map.get("ebitdaTTM"))
            .and_then(|v| v.as_f64());
        if let Some(ebitda_val) = ebitda {
            if ebitda_val > 0.0 {
                let net_debt = balance_entry
                    .and_then(|b| b.get("netDebt"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let ev = market_cap + net_debt;
                f_map.insert("evToEBITDA".to_string(), Value::from(ev / ebitda_val));
                f_map.insert("evToEbitda".to_string(), Value::from(ev / ebitda_val));
            }
        }
    }
}

/// Compute year-over-year revenue growth for each entry in the key-metrics array.
///
/// Looks up revenue from the income statement for each year and computes
/// revenueGrowth[n] = (revenue[n] - revenue[n-1]) / revenue[n-1].
fn compute_revenue_growth(items: &mut [Value], income_yearly: Option<&Value>) {
    let Some(income_yearly) = income_yearly else {
        return;
    };

    // Build a sorted list of (date, revenue) from income statements.
    let mut revenue_by_date: Vec<(String, f64)> = income_yearly.as_object().map_or(vec![], |map| {
        map.iter()
            .filter_map(|(date, stmt)| {
                let rev = stmt
                    .get("revenue")
                    .or_else(|| stmt.get("totalRevenue"))
                    .and_then(|v| v.as_f64())?;
                Some((date.clone(), rev))
            })
            .collect()
    });
    revenue_by_date.sort_by(|a, b| a.0.cmp(&b.0));

    // For each item in the key-metrics array, compute revenue growth.
    for item in items {
        let Some(obj) = item.as_object_mut() else {
            continue;
        };
        if obj.contains_key("revenueGrowth") {
            continue;
        }
        let Some(date) = obj.get("date").and_then(|v| v.as_str()) else {
            continue;
        };
        // Find this year's revenue and the prior year's revenue.
        let idx = revenue_by_date.iter().position(|(d, _)| d == date);
        if let Some(idx) = idx {
            if idx > 0 {
                let (curr, prev) = (revenue_by_date[idx].1, revenue_by_date[idx - 1].1);
                if prev > 0.0 {
                    let growth = (curr - prev) / prev;
                    obj.insert("revenueGrowth".to_string(), Value::from(growth));
                }
            }
        }
    }
}

/// Normalize EODHD /eod/{symbol} historical prices → FMP-compatible format.
///
/// EODHD returns an array of {date, open, high, low, close, adjusted_close, volume}.
/// We wrap in the {symbol, historical: [...]} envelope (HistoricalPriceView
/// handles both envelope and flat array) and map adjusted_close → adjClose
/// so HistoricalPriceView::latest_close() fallback works.
fn normalize_eodhd_historical(eod_value: &Value, symbol: &str) -> Value {
    let historical = match eod_value {
        Value::Array(arr) => {
            let mapped: Vec<Value> = arr
                .iter()
                .map(|bar| {
                    let mut obj = bar.clone();
                    if let Some(map) = obj.as_object_mut() {
                        map_field(map, "adjusted_close", "adjClose");
                    }
                    obj
                })
                .collect();
            Value::Array(mapped)
        }
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

// ── EODHD bulk listing ───────────────────────────────────────────────

/// A flat company listing row — the exhaustive screen output format.
/// Identifier, name, exchange, price, shares outstanding, market cap.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompanyListing {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shares: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap: Option<f64>,
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub country: Option<String>,
}

/// Fetch the EODHD Screener API with arbitrary filter triples, returning
/// the raw screener rows (each row contains all filter field values for
/// the company — symbol, name, exchange, market cap, sector, industry,
/// price, volume, EPS, dividend yield, etc.).
///
/// Paginates with offset (max 999). When a screen exceeds 1,000 results,
/// automatically splits by market cap bands to exhaust the full universe.
///
/// EODHD Screener API:
/// `https://eodhd.com/api/screener?api_token={token}&filters=[...]&sort=market_capitalization.desc&limit=500`
/// See: https://eodhd.com/financial-apis/stock-market-screener-api
pub async fn fetch_eodhd_screener(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    filters: &[serde_json::Value],
) -> Result<Vec<Value>, McpToolError> {
    let filters_json = serde_json::to_string(filters).map_err(|e| {
        McpToolError::internal(format!("failed to serialize screener filters: {e}"))
    })?;

    // First pass: try direct pagination with the given filters.
    let mut all_rows = fetch_screener_page(client, eodhd_api_key, &filters_json, 0).await?;

    // If we hit the offset cap (1,000 results), split by market cap bands.
    // This only applies when the filters don't already include a market cap
    // range narrow enough to stay under 1,000.
    if all_rows.len() >= 1000 {
        // Check if market_capitalization is already in the filters
        let has_market_cap_filter = filters.iter().any(|f| {
            f.as_array()
                .and_then(|a| a.first())
                .and_then(|f| f.as_str())
                .map(|s| s == "market_capitalization")
                .unwrap_or(false)
        });

        if !has_market_cap_filter {
            // Split by market cap bands to exhaust the universe
            all_rows = fetch_screener_with_bands(client, eodhd_api_key, filters).await?;
        }
    }

    // Deduplicate by code (band overlaps create dupes)
    let mut seen = std::collections::HashSet::new();
    all_rows.retain(|row| {
        let code = row.get("code").and_then(|v| v.as_str()).unwrap_or("");
        seen.insert(code.to_string())
    });

    Ok(all_rows)
}

/// Fetch the full universe for an exchange with market cap above a
/// threshold, using market cap band splitting to exhaust the offset limit.
/// Convenience wrapper around [`fetch_eodhd_screener`] for the
/// `stock_universe` tool.
pub async fn fetch_eodhd_screener_listing(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    exchange: &str,
    min_market_cap: f64,
) -> Result<Vec<CompanyListing>, McpToolError> {
    let filters = vec![serde_json::json!([
        "market_capitalization",
        ">=",
        min_market_cap
    ])];

    let rows = fetch_eodhd_screener(client, eodhd_api_key, &filters).await?;

    let mut listings: Vec<CompanyListing> = rows
        .iter()
        .filter_map(|row| parse_screener_row(row))
        .filter(|l| {
            l.exchange.eq_ignore_ascii_case(exchange) || exchange.eq_ignore_ascii_case("US")
        })
        .collect();

    // Sort by market cap descending
    listings.sort_by(|a, b| {
        b.market_cap
            .unwrap_or(0.0)
            .partial_cmp(&a.market_cap.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(listings)
}

/// Fetch the EODHD screener with market cap band splitting to exhaust
/// the 1,000-result offset limit. Adds market_capitalization band filters
/// to the existing filter set.
async fn fetch_screener_with_bands(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    base_filters: &[serde_json::Value],
) -> Result<Vec<Value>, McpToolError> {
    let bands: Vec<(f64, f64)> = vec![
        (0.0, 500_000_000.0),
        (500_000_000.0, 1_000_000_000.0),
        (1_000_000_000.0, 2_000_000_000.0),
        (2_000_000_000.0, 5_000_000_000.0),
        (5_000_000_000.0, 10_000_000_000.0),
        (10_000_000_000.0, 25_000_000_000.0),
        (25_000_000_000.0, 50_000_000_000.0),
        (50_000_000_000.0, 100_000_000_000.0),
        (100_000_000_000.0, 500_000_000_000.0),
        (500_000_000_000.0, 2_000_000_000_000.0),
        (2_000_000_000_000.0, 10_000_000_000_000.0),
        (10_000_000_000_000.0, f64::MAX),
    ];

    let mut all_rows = Vec::new();

    for (lower, upper) in &bands {
        let mut band_filters: Vec<serde_json::Value> = base_filters.to_vec();
        band_filters.push(serde_json::json!(["market_capitalization", ">=", lower]));
        if *upper != f64::MAX {
            band_filters.push(serde_json::json!(["market_capitalization", "<", upper]));
        }

        let filters_json = serde_json::to_string(&band_filters).map_err(|e| {
            McpToolError::internal(format!("failed to serialize band filters: {e}"))
        })?;

        let mut offset = 0u32;
        loop {
            let band_rows =
                fetch_screener_page(client, eodhd_api_key, &filters_json, offset).await?;
            let row_count = band_rows.len();
            all_rows.extend(band_rows);

            if row_count < 500 || offset + 500 > 999 {
                break;
            }
            offset += 500;
        }
    }

    Ok(all_rows)
}

/// Fetch a single page from the EODHD screener (up to 500 rows at the
/// given offset). Returns the raw data rows.
async fn fetch_screener_page(
    client: &reqwest::Client,
    eodhd_api_key: &str,
    filters_json: &str,
    offset: u32,
) -> Result<Vec<Value>, McpToolError> {
    let offset_str = offset.to_string();
    let resp = client
        .get(&format!("{EODHD_BASE_URL}/screener"))
        .query(&[
            ("api_token", eodhd_api_key),
            ("fmt", "json"),
            ("filters", filters_json),
            ("sort", "market_capitalization.desc"),
            ("limit", "500"),
            ("offset", offset_str.as_str()),
        ])
        .send()
        .await
        .map_err(|e| McpToolError::unavailable(format!("EODHD screener request failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| McpToolError::unavailable(format!("EODHD screener body read failed: {e}")))?;

    if !status.is_success() {
        return Err(classify_http_error("EODHD", status, &body));
    }

    let raw: Value = serde_json::from_str(&body).map_err(|e| {
        McpToolError::unavailable(format!("failed to parse EODHD screener response: {e}"))
    })?;

    let rows = raw.get("data").and_then(|d| d.as_array());

    match rows {
        Some(arr) => Ok(arr.clone()),
        None => Ok(Vec::new()),
    }
}

/// Parse a single row from the EODHD screener response into a CompanyListing.
/// The screener returns flat fields (code, name, market_capitalization, etc.)
/// rather than the nested blocks used by the bulk-fundamentals endpoint.
fn parse_screener_row(row: &Value) -> Option<CompanyListing> {
    let symbol = row.get("code").and_then(|v| v.as_str())?.to_string();
    let name = row
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let exchange = row
        .get("exchange")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let price = row.get("adjusted_close").and_then(|v| v.as_f64());
    let market_cap = row.get("market_capitalization").and_then(|v| v.as_f64());
    let sector = row
        .get("sector")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let industry = row
        .get("industry")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    Some(CompanyListing {
        symbol,
        name,
        exchange,
        price,
        shares: None,
        market_cap,
        sector,
        industry,
        country: None,
    })
}

/// Resolve a company name or plain ticker to its primary exchange symbol.
///
/// Searches EODHD for the company, filters for `isPrimary == true` and
/// `Type == "Common Stock"`, and returns the symbol as `{Code}.{Exchange}`.
/// If the input already contains a `.`, it's assumed to be already resolved.
///
/// Returns the resolved EODHD-format symbol and whether it's a US listing
/// (FMP primary) or international (EODHD primary).
pub async fn resolve_symbol(
    client: &reqwest::Client,
    query: &str,
    eodhd_api_key: &str,
) -> Result<ResolvedSymbol, McpToolError> {
    // Already in EXCHANGE.FORMAT — use as-is.
    if query.contains('.') {
        let exchange = query.split('.').nth(1).unwrap_or("");
        let is_us = exchange == "US";
        return Ok(ResolvedSymbol {
            symbol: query.to_string(),
            is_us,
            company_name: None,
        });
    }

    // Search EODHD for the primary listing.
    let results = eodhd_search_get(client, query, "50", eodhd_api_key).await?;

    let arr = results
        .as_array()
        .ok_or_else(|| McpToolError::unavailable("EODHD search returned non-array"))?;

    // Prefer: isPrimary == true AND Type == "Common Stock".
    let best = arr
        .iter()
        .filter(|e| {
            e.get("Type")
                .and_then(|v| v.as_str())
                .map(|t| t == "Common Stock")
                .unwrap_or(false)
        })
        .max_by_key(|e| {
            e.get("isPrimary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });

    let best = best.ok_or_else(|| {
        McpToolError::unavailable(format!(
            "No primary common stock listing found for '{query}'"
        ))
    })?;

    let code = best
        .get("Code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpToolError::unavailable("EODHD search result missing 'Code'"))?;
    let exchange = best
        .get("Exchange")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpToolError::unavailable("EODHD search result missing 'Exchange'"))?;
    let company_name = best.get("Name").and_then(|v| v.as_str()).map(String::from);

    let symbol = format!("{code}.{exchange}");
    let is_us = exchange == "US";

    Ok(ResolvedSymbol {
        symbol,
        is_us,
        company_name,
    })
}

/// Result of symbol resolution: the EODHD-format symbol, whether it's a US
/// listing (FMP primary), and the company name if available.
pub struct ResolvedSymbol {
    pub symbol: String,
    pub is_us: bool,
    pub company_name: Option<String>,
}

// ── Tests ──────────────────────────────────────────────────────────
