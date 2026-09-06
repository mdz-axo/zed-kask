//! hKask MCP Companies — Dual-provider abstraction (FMP + EODHD)
//!
//! Routes tool calls to the appropriate provider and normalizes responses
//! so that analysis functions in `analysis.rs` work transparently with
//! either data source.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use serde_json::Value;

// ── Typed projection views ───────────────────────────────────────
//
// Server typed readers project the retained normalized `Value` so
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
    provider: Option<Provider>,
}

impl CompanyProfile {
    /// Wrap a normalized profile payload. The value is expected to be the
    /// FMP-shaped array (`[{"companyName": ...}]`); an empty array means the
    /// provider returned no profile.
    pub fn from_raw(raw: Value) -> Self {
        Self {
            raw,
            provider: None,
        }
    }

    pub(crate) fn from_response(response: ProviderResponse) -> Self {
        Self {
            raw: response.value,
            provider: Some(response.provider),
        }
    }

    pub(crate) fn provider(&self) -> Option<Provider> {
        self.provider
    }

    /// The first profile object in the array, or `None` if the array is empty
    /// or missing (the "no profile" signal — distinct from a present-but-zero
    /// field).
    pub(crate) fn first(&self) -> Option<&Value> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
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

// Test-local HTTP origin substitution leaves routing, requests and normalization intact.
#[cfg(test)]
tokio::task_local! {
    pub(crate) static TEST_HTTP_ORIGIN: String;
}

fn provider_url(base: &str, path: &str) -> String {
    #[cfg(test)]
    if let Ok(origin) = TEST_HTTP_ORIGIN.try_with(Clone::clone) {
        let provider = if base == FMP_BASE_URL { "fmp" } else { "eodhd" };
        return format!("{origin}/{provider}{path}");
    }
    format!("{base}{path}")
}

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

/// FMP addresses US listings by their bare ticker; the qualified "COF.US"
/// form (EODHD style, `resolve_symbol` output) must be stripped before
/// routing to FMP or the endpoint returns an empty result.
fn strip_us_suffix(symbol: &str) -> &str {
    symbol.strip_suffix(".US").unwrap_or(symbol)
}

// ── Provider response with provenance ─────────────────────────────

/// The result of a provider fetch — the raw JSON value plus which provider
/// served it. The cache stores the provider so consumers can distinguish
/// FMP-sourced data from EODHD-sourced data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderResponse {
    pub value: Value,
    pub provider: Provider,
    pub warnings: Vec<String>,
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

    // FMP takes the bare US ticker — strip the qualified form so
    // resolve_symbol's "COF.US" output feeds straight into these tools.
    let fmp_symbol = strip_us_suffix(symbol);

    // Try primary provider
    let primary_result = match primary {
        Provider::Fmp => {
            fmp_get(
                client,
                mapping.fmp_path,
                fmp_api_key,
                fmp_symbol,
                extra_params,
            )
            .await
        }
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
                    warnings: Vec::new(),
                })
            } else {
                Ok(ProviderResponse {
                    value,
                    provider: primary,
                    warnings: Vec::new(),
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
                    fmp_get(
                        client,
                        mapping.fmp_path,
                        fmp_api_key,
                        fmp_symbol,
                        extra_params,
                    )
                    .await
                }
                Provider::Eodhd => {
                    // For FMP→EODHD fallback on plain symbols, try with .US suffix
                    let eodhd_symbol = if !is_international_symbol(symbol) {
                        format!("{}.US", strip_us_suffix(symbol))
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
                            warnings: Vec::new(),
                        })
                    } else {
                        Ok(ProviderResponse {
                            value,
                            provider: secondary,
                            warnings: Vec::new(),
                        })
                    }
                }
                Err(e) => Err(e),
            }
        }
    }
}

/// Acquire canonical metrics while retaining the actual provider. FMP stable
/// splits metrics across three endpoints; EODHD derives them from fundamentals
/// and is never mixed with FMP data.
pub async fn fetch_key_metrics(
    client: &reqwest::Client,
    symbol: &str,
    extra: &[(&str, &str)],
    fmp_api_key: &str,
    eodhd_api_key: &str,
    learning: Option<&super::LearningState>,
) -> Result<ProviderResponse, McpToolError> {
    let mut response = companies_get(
        client,
        "key_metrics",
        symbol,
        fmp_api_key,
        eodhd_api_key,
        extra,
        learning,
    )
    .await?;
    if response.provider == Provider::Eodhd {
        response.warnings.push("EODHD derived metrics: ROIC approximates net income / total assets; working-capital ratios use year-end balances".into());
        return Ok(response);
    }
    let symbol = strip_us_suffix(symbol);
    let (ratios, growth) = tokio::join!(
        fmp_get(client, "/ratios", fmp_api_key, symbol, extra),
        fmp_get(client, "/financial-growth", fmp_api_key, symbol, extra),
    );
    let mut supplement = |endpoint: &str, result: Result<Value, McpToolError>| match result {
        Ok(value) if value.is_array() => {
            if let Some(rows) = response.value.as_array() {
                for row in rows {
                    let date = row.get("date").and_then(Value::as_str);
                    if !value.as_array().is_some_and(|entries| {
                        entries.iter().any(|entry| {
                            date.is_some() && entry.get("date").and_then(Value::as_str) == date
                        })
                    }) {
                        response.warnings.push(format!(
                            "FMP {endpoint}: no supplement for date {}",
                            date.unwrap_or("missing")
                        ));
                    }
                }
            }
            Some(value)
        }
        Ok(_) => {
            response
                .warnings
                .push(format!("FMP {endpoint}: expected an array"));
            None
        }
        Err(error) => {
            response
                .warnings
                .push(format!("FMP {endpoint}: {}", error.to_json_string()));
            None
        }
    };
    let ratios = supplement("ratios", ratios);
    let growth = supplement("financial-growth", growth);
    response.value = enrich_key_metrics(response.value, ratios, growth);
    Ok(response)
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
    let url = provider_url(FMP_BASE_URL, path);
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
    let url = provider_url(EODHD_BASE_URL, &format!("{path}/{symbol}"));
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
/// EODHD provides Highlights (latest snapshot), optional quarterly Earnings.History,
/// and Financials (annual balance sheet + income statement). Annual statements set
/// the metric dates; we retain matching earnings fields
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

    // Annual statements define fiscal periods; Earnings.History is quarterly and
    // may be absent. It must not determine the annual metrics timeline.
    let mut items: Vec<Value> = match income_yearly {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(date, _)| {
                let year = date.split('-').next().unwrap_or(date);
                let mut obj = serde_json::json!({
                    "calendarYear": year,
                    "date": date,
                    "period": "FY",
                });

                // Retain any earnings fields for the matching fiscal date only.
                if let Some(obj_map) = obj.as_object_mut()
                    && let Some(e_obj) = earnings_history
                        .and_then(|history| history.get(date))
                        .and_then(Value::as_object)
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
                if let Some(net_debt) = balance_entry
                    .and_then(|balance| balance.get("netDebt"))
                    .and_then(Value::as_f64)
                {
                    let enterprise_value = market_cap + net_debt;
                    f_map.insert(
                        "evToEBITDA".to_string(),
                        Value::from(enterprise_value / ebitda_val),
                    );
                    f_map.insert(
                        "evToEbitda".to_string(),
                        Value::from(enterprise_value / ebitda_val),
                    );
                }
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

/// Inputs for multi-signal symbol resolution: the company name and ticker
/// from the prompt, plus optional exchange / country disambiguators. At
/// least one of `company_name` / `ticker` must be present (enforced by
/// the tool layer).
pub struct ResolveSymbolInput {
    pub company_name: Option<String>,
    pub ticker: Option<String>,
    pub exchange: Option<String>,
    pub country: Option<String>,
}

impl ResolveSymbolInput {
    /// The given signals, for error messages.
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ticker) = &self.ticker {
            parts.push(format!("ticker '{ticker}'"));
        }
        if let Some(company_name) = &self.company_name {
            parts.push(format!("company name '{company_name}'"));
        }
        if let Some(exchange) = &self.exchange {
            parts.push(format!("exchange '{exchange}'"));
        }
        if let Some(country) = &self.country {
            parts.push(format!("country '{country}'"));
        }
        parts.join(", ")
    }
}

/// One EODHD search entry, filtered to a common stock listing.
#[derive(Clone)]
struct SearchCandidate {
    code: String,
    exchange: String,
    name: String,
    country: Option<String>,
    is_primary: bool,
}

/// Resolve a company name and/or ticker to its primary exchange symbol.
///
/// Searches EODHD with the ticker (when given) and the company name (when
/// given), keeps common-stock listings, narrows to the prompt's
/// exchange/country when one is given, then takes the first candidate in
/// preference order: exact ticker match, company-name match on a primary
/// listing, company-name match, primary listing. EODHD's own ordering
/// (popularity) is the tiebreak. EODHD's search matches substrings inside
/// company names, so a bare ticker like "COF" surfaces dozens of name
/// matches ("Swiss Water Decaffeinated Coffee Inc" contains "cof") — the
/// exact-code preference is what picks Capital One's COF.US out of that
/// noise.
///
/// A ticker that already carries an exchange suffix ("VOD.LSE") is its
/// own answer and is returned as-is without a search.
///
/// Returns the resolved EODHD-format symbol and whether it's a US listing
/// (FMP primary) or international (EODHD primary).
pub async fn resolve_symbol(
    client: &reqwest::Client,
    input: &ResolveSymbolInput,
    eodhd_api_key: &str,
) -> Result<ResolvedSymbol, McpToolError> {
    if let Some(ticker) = input.ticker.as_deref() {
        if ticker.contains('.') {
            let exchange = ticker
                .rsplit_once('.')
                .map(|(_, suffix)| suffix)
                .unwrap_or_default();
            return Ok(ResolvedSymbol {
                symbol: ticker.to_string(),
                is_us: exchange.eq_ignore_ascii_case("US"),
                company_name: None,
            });
        }
    }

    let mut entries: Vec<Value> = Vec::new();

    // Ticker search first: when it surfaces the exact code, the ticker is
    // authoritative and a name search cannot improve on it.
    let mut ticker_match_found = false;
    if let Some(ticker) = input.ticker.as_deref() {
        let results = eodhd_search_get(client, ticker, "50", eodhd_api_key).await?;
        ticker_match_found = results.as_array().is_some_and(|array| {
            array.iter().any(|entry| {
                parse_search_candidate(entry)
                    .is_some_and(|candidate| candidate.code.eq_ignore_ascii_case(ticker))
            })
        });
        extend_entries(&mut entries, results);
    }
    if input.company_name.is_some() && !ticker_match_found {
        let results = eodhd_search_get(
            client,
            input.company_name.as_deref().unwrap_or_default(),
            "50",
            eodhd_api_key,
        )
        .await?;
        extend_entries(&mut entries, results);
    }

    let best = select_best_candidate(&entries, input).ok_or_else(|| {
        McpToolError::unavailable(format!(
            "No common stock listing found on EODHD for {}",
            input.describe()
        ))
    })?;

    let is_us = best.exchange == "US";
    Ok(ResolvedSymbol {
        symbol: format!("{}.{}", best.code, best.exchange),
        is_us,
        company_name: if best.name.is_empty() {
            None
        } else {
            Some(best.name)
        },
    })
}

fn extend_entries(entries: &mut Vec<Value>, results: Value) {
    if let Some(array) = results.as_array() {
        entries.extend(array.iter().cloned());
    }
}

/// Parse an EODHD search entry, returning `None` unless it is a common
/// stock listing — ETFs, funds, bonds, and indices share tickers with
/// common stock and must not win resolution.
fn parse_search_candidate(entry: &Value) -> Option<SearchCandidate> {
    let asset_type = entry.get("Type").and_then(Value::as_str)?;
    if asset_type != "Common Stock" {
        return None;
    }
    Some(SearchCandidate {
        code: entry.get("Code").and_then(Value::as_str)?.to_string(),
        exchange: entry.get("Exchange").and_then(Value::as_str)?.to_string(),
        name: entry
            .get("Name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        country: entry
            .get("Country")
            .and_then(Value::as_str)
            .map(String::from),
        is_primary: entry
            .get("isPrimary")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Pick the winning candidate: parse, narrow to the prompt's
/// exchange/country, then take the first match in preference order —
/// exact ticker, company-name match on a primary listing, company-name
/// match, primary listing, first. EODHD's own ordering (popularity) is
/// the tiebreak.
fn select_best_candidate(entries: &[Value], input: &ResolveSymbolInput) -> Option<SearchCandidate> {
    let candidates: Vec<SearchCandidate> =
        entries.iter().filter_map(parse_search_candidate).collect();
    // A market signal that matches nothing (a spelling the alias tables
    // don't cover) is ignored rather than fatal.
    let mut pool: Vec<SearchCandidate> = candidates
        .iter()
        .filter(|candidate| matches_given_market(candidate, input))
        .cloned()
        .collect();
    if pool.is_empty() {
        pool = candidates;
    }
    pool.iter()
        .find(|candidate| {
            input
                .ticker
                .as_deref()
                .is_some_and(|ticker| candidate.code.eq_ignore_ascii_case(ticker))
        })
        .or_else(|| {
            pool.iter()
                .find(|candidate| name_matches(candidate, input) && candidate.is_primary)
        })
        .or_else(|| pool.iter().find(|candidate| name_matches(candidate, input)))
        .or_else(|| pool.iter().find(|candidate| candidate.is_primary))
        .or_else(|| pool.first())
        .cloned()
}

/// Whether a candidate is consistent with an explicitly given
/// exchange/country. A candidate with no Country value passes — a missing
/// EODHD field must not exclude the right listing.
fn matches_given_market(candidate: &SearchCandidate, input: &ResolveSymbolInput) -> bool {
    if let Some(exchange) = input.exchange.as_deref() {
        if canonical_exchange(exchange) != canonical_exchange(&candidate.exchange) {
            return false;
        }
    }
    if let (Some(country), Some(candidate_country)) =
        (input.country.as_deref(), candidate.country.as_deref())
    {
        if canonical_country(country) != canonical_country(candidate_country) {
            return false;
        }
    }
    true
}

/// Whether the candidate's name matches the prompt's company name: every
/// significant word of the query starts a word of the candidate name.
/// Prefix matching absorbs corporate-designation spellings ("Corp" →
/// "Corporation", "Ltd" → "Limited") without a stopword table.
fn name_matches(candidate: &SearchCandidate, input: &ResolveSymbolInput) -> bool {
    let Some(company_name) = input.company_name.as_deref() else {
        return false;
    };
    let candidate_words = name_words(&candidate.name);
    let query_words: Vec<String> = name_words(company_name)
        .into_iter()
        .filter(|word| word.len() > 1)
        .collect();
    !query_words.is_empty()
        && query_words.iter().all(|query_word| {
            candidate_words
                .iter()
                .any(|candidate_word| candidate_word.starts_with(query_word))
        })
}

/// Lowercase words of a name, split on non-alphanumerics.
fn name_words(name: &str) -> Vec<String> {
    name.split(|character: char| !character.is_alphanumeric())
        .map(|word| word.to_lowercase())
        .collect()
}

/// Lowercase alphanumerics only: "U.S.A." → "usa". Used for exchange and
/// country comparison so punctuation variants compare equal.
fn normalize_alphanumeric(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

/// Canonical country for comparison: ISO-2 codes and common names
/// collapse to one form per country ("US", "USA", "United States" →
/// "usa"), so the EODHD `Country` value matches whatever spelling the
/// prompt used. Unknown inputs pass through normalized — an exact
/// EODHD-style spelling still compares equal to itself.
fn canonical_country(input: &str) -> String {
    let normalized = normalize_alphanumeric(input);
    match normalized.as_str() {
        "us" | "usa" | "unitedstates" | "america" => "usa",
        "uk" | "gb" | "unitedkingdom" | "britain" | "england" => "uk",
        "ca" | "canada" => "canada",
        "de" | "germany" => "germany",
        "fr" | "france" => "france",
        "jp" | "japan" => "japan",
        "cn" | "china" => "china",
        "in" | "india" => "india",
        "au" | "australia" => "australia",
        "ch" | "switzerland" => "switzerland",
        "hk" | "hongkong" => "hongkong",
        _ => return normalized,
    }
    .to_string()
}

/// Canonical exchange for comparison. EODHD folds all major US exchanges
/// into code "US", so NYSE/NASDAQ/AMEX map there; TSX/LSE names map to
/// their codes; "PA"/"PAR"/"Paris" collapse to one form so both sides
/// compare equal. Anything else (already an EODHD code like "GER")
/// passes through normalized.
fn canonical_exchange(input: &str) -> String {
    let normalized = normalize_alphanumeric(input);
    match normalized.as_str() {
        "us" | "usa" | "nyse" | "nasdaq" | "amex" => "us",
        "to" | "tsx" | "toronto" => "to",
        "lse" | "london" => "lse",
        "pa" | "par" | "paris" => "par",
        _ => return normalized,
    }
    .to_string()
}

/// Result of symbol resolution: the EODHD-format symbol, whether it's a US
/// listing (FMP primary), and the company name if available.
pub struct ResolvedSymbol {
    pub symbol: String,
    pub is_us: bool,
    pub company_name: Option<String>,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        code: &str,
        exchange: &str,
        name: &str,
        country: Option<&str>,
        is_primary: bool,
    ) -> Value {
        let mut object = serde_json::json!({
            "Code": code,
            "Exchange": exchange,
            "Name": name,
            "Type": "Common Stock",
            "isPrimary": is_primary,
        });
        if let Some(country) = country {
            object["Country"] = Value::String(country.to_string());
        }
        object
    }

    fn input(
        ticker: Option<&str>,
        company_name: Option<&str>,
        exchange: Option<&str>,
        country: Option<&str>,
    ) -> ResolveSymbolInput {
        ResolveSymbolInput {
            ticker: ticker.map(String::from),
            company_name: company_name.map(String::from),
            exchange: exchange.map(String::from),
            country: country.map(String::from),
        }
    }

    // The reported mismatch: EODHD's substring search for "COF" surfaces
    // dozens of name matches ("Swiss Water Decaffeinated Coffee Inc"
    // contains "cof"), and the old max_by_key(isPrimary) pick chose
    // SWP.TO. The exact code match must dominate name-substring noise.
    #[test]
    fn bare_ticker_prefers_exact_code_match() {
        let entries = vec![
            entry(
                "COF",
                "US",
                "Capital One Financial Corp.",
                Some("USA"),
                true,
            ),
            entry(
                "SWP",
                "TO",
                "Swiss Water Decaffeinated Coffee Inc",
                Some("Canada"),
                true,
            ),
        ];
        let best = select_best_candidate(&entries, &input(Some("COF"), None, None, None))
            .expect("an exact code match must win");
        assert_eq!(best.code, "COF");
        assert_eq!(best.exchange, "US");
    }

    #[test]
    fn non_common_stock_entries_are_filtered() {
        let entries = vec![
            serde_json::json!({
                "Code": "COF", "Exchange": "US", "Name": "Capital One ETF",
                "Type": "ETF", "isPrimary": true,
            }),
            entry(
                "COF",
                "US",
                "Capital One Financial Corp.",
                Some("USA"),
                true,
            ),
        ];
        let best = select_best_candidate(&entries, &input(Some("COF"), None, None, None))
            .expect("ETF entries must be skipped");
        assert_eq!(best.code, "COF");
    }

    #[test]
    fn company_name_tokens_beat_substring_noise() {
        let entries = vec![
            entry(
                "COF",
                "US",
                "Capital One Financial Corp.",
                Some("USA"),
                true,
            ),
            entry(
                "SWP",
                "TO",
                "Swiss Water Decaffeinated Coffee Inc",
                Some("Canada"),
                true,
            ),
        ];
        let best = select_best_candidate(
            &entries,
            &input(None, Some("Capital One Financial Corp"), None, None),
        )
        .expect("token overlap must beat substring noise");
        assert_eq!(best.code, "COF");
    }

    #[test]
    fn primary_listing_wins_when_signals_tie() {
        // The EODHD AAPL shape: a US primary plus a Canadian CDR with the
        // same code. With only a name, the primary listing must win.
        let entries = vec![
            entry("AAPL", "US", "Apple Inc", Some("USA"), true),
            entry(
                "AAPL",
                "TO",
                "Apple CDR (CAD Hedged)",
                Some("Canada"),
                false,
            ),
        ];
        let best = select_best_candidate(&entries, &input(None, Some("Apple"), None, None))
            .expect("primary listing must win ties");
        assert_eq!(best.exchange, "US");
    }

    #[test]
    fn country_disambiguates_same_ticker() {
        let entries = vec![
            entry("AAPL", "US", "Apple Inc", Some("USA"), true),
            entry(
                "AAPL",
                "TO",
                "Apple CDR (CAD Hedged)",
                Some("Canada"),
                false,
            ),
        ];
        let best =
            select_best_candidate(&entries, &input(None, Some("Apple"), None, Some("Canada")))
                .expect("explicit country must disambiguate");
        assert_eq!(best.exchange, "TO");
    }

    #[test]
    fn unmatched_market_signal_is_ignored() {
        // A market the company doesn't list on (Apple on LSE) must not
        // empty the candidate set — the disambiguator is ignored rather
        // than fatal.
        let entries = vec![
            entry("AAPL", "US", "Apple Inc", Some("USA"), true),
            entry(
                "AAPL",
                "TO",
                "Apple CDR (CAD Hedged)",
                Some("Canada"),
                false,
            ),
        ];
        let best = select_best_candidate(&entries, &input(Some("AAPL"), None, Some("LSE"), None))
            .expect("unmatched exchange must fall back to the full set");
        assert_eq!(best.code, "AAPL");
        assert_eq!(best.exchange, "US");
    }

    #[test]
    fn exchange_name_maps_to_eodhd_code() {
        let entries = vec![
            entry("AAPL", "US", "Apple Inc", Some("USA"), true),
            entry(
                "AAPL",
                "TO",
                "Apple CDR (CAD Hedged)",
                Some("Canada"),
                false,
            ),
        ];
        let best =
            select_best_candidate(&entries, &input(None, Some("Apple"), Some("NASDAQ"), None))
                .expect("NASDAQ must match EODHD code US");
        assert_eq!(best.exchange, "US");
    }

    #[test]
    fn name_and_ticker_resolve_together() {
        // The primary contract: a research prompt gives both the company
        // name and the ticker.
        let entries = vec![
            entry(
                "COF",
                "US",
                "Capital One Financial Corp.",
                Some("USA"),
                true,
            ),
            entry(
                "SWP",
                "TO",
                "Swiss Water Decaffeinated Coffee Inc",
                Some("Canada"),
                true,
            ),
        ];
        let best = select_best_candidate(
            &entries,
            &input(Some("COF"), Some("Capital One Financial Corp"), None, None),
        )
        .expect("name and ticker must resolve together");
        assert_eq!(best.code, "COF");
        assert_eq!(best.exchange, "US");
    }

    #[test]
    fn no_common_stock_candidates_yields_none() {
        let entries = vec![serde_json::json!({
            "Code": "COF", "Exchange": "US", "Name": "x", "Type": "ETF", "isPrimary": true,
        })];
        assert!(select_best_candidate(&entries, &input(Some("COF"), None, None, None)).is_none());
        assert!(select_best_candidate(&[], &input(Some("COF"), None, None, None)).is_none());
    }

    #[tokio::test]
    async fn dotted_ticker_passes_through_without_search() {
        // An already-qualified ticker is its own answer — no request is
        // made, so an unroutable key proves the passthrough.
        let client = reqwest::Client::new();
        let resolved = resolve_symbol(
            &client,
            &input(Some("VOD.LSE"), None, None, None),
            "unused-key",
        )
        .await
        .expect("qualified ticker resolves without a search");
        assert_eq!(resolved.symbol, "VOD.LSE");
        assert!(!resolved.is_us);

        let resolved = resolve_symbol(
            &client,
            &input(Some("COF.US"), None, None, None),
            "unused-key",
        )
        .await
        .expect("qualified ticker resolves without a search");
        assert_eq!(resolved.symbol, "COF.US");
        assert!(resolved.is_us);
    }

    #[test]
    fn country_and_exchange_aliases_canonicalize() {
        for spelling in ["US", "USA", "U.S.A.", "United States"] {
            assert_eq!(canonical_country(spelling), canonical_country("USA"));
        }
        assert_eq!(canonical_country("GB"), canonical_country("United Kingdom"));
        assert_eq!(canonical_exchange("NASDAQ"), canonical_exchange("US"));
        assert_eq!(canonical_exchange("NYSE"), canonical_exchange("US"));
        assert_eq!(canonical_exchange("Toronto"), canonical_exchange("TO"));
        assert_eq!(canonical_exchange("PA"), canonical_exchange("Paris"));
    }

    // Live-API pins of the ranking against the real EODHD search — the
    // synthetic tests above cover the logic; these catch EODHD-side
    // changes (field renames, new CDR listings) that would silently break
    // resolution. Skipped without HKASK_EODHD_API_KEY.

    fn eodhd_api_key() -> Option<String> {
        std::env::var("HKASK_EODHD_API_KEY")
            .ok()
            .filter(|key| !key.is_empty())
    }

    #[tokio::test]
    async fn live_bare_ticker_picks_exact_code() {
        // Reproduction of the reported mismatch: EODHD's substring search
        // for "COF" returns name matches like SWP.TO ("Swiss Water
        // Decaffeinated Coffee Inc" contains "cof"); the ranking must
        // pick Capital One's COF.US instead.
        let Some(key) = eodhd_api_key() else {
            eprintln!("SKIP: no EODHD API key");
            return;
        };
        let client = reqwest::Client::new();
        let resolved = resolve_symbol(&client, &input(Some("COF"), None, None, None), &key)
            .await
            .expect("COF must resolve to a common stock listing");
        assert_eq!(
            resolved.symbol, "COF.US",
            "bare ticker COF must resolve to Capital One, not a name-substring match"
        );
        assert!(resolved.is_us);
    }

    #[tokio::test]
    async fn live_company_name_only_resolves_primary() {
        let Some(key) = eodhd_api_key() else {
            eprintln!("SKIP: no EODHD API key");
            return;
        };
        let client = reqwest::Client::new();
        let resolved = resolve_symbol(
            &client,
            &input(None, Some("Capital One Financial Corp"), None, None),
            &key,
        )
        .await
        .expect("company name must resolve to a common stock listing");
        assert_eq!(
            resolved.symbol, "COF.US",
            "'Capital One Financial Corp' must resolve to Capital One's US listing"
        );
    }

    #[test]
    fn us_suffix_is_stripped_for_fmp_routing() {
        assert_eq!(strip_us_suffix("COF.US"), "COF");
        assert_eq!(strip_us_suffix("BRK-B.US"), "BRK-B");
        assert_eq!(strip_us_suffix("COF"), "COF");
        assert_eq!(strip_us_suffix("VOD.LSE"), "VOD.LSE");
    }

    fn fmp_api_key() -> Option<String> {
        std::env::var("HKASK_FMP_API_KEY")
            .ok()
            .filter(|key| !key.is_empty())
    }

    #[tokio::test]
    async fn live_qualified_us_symbol_feeds_fmp_profile() {
        // resolve_symbol returns the qualified "COF.US" form; the profile
        // tools must accept it — FMP is addressed with the bare ticker.
        let (Some(fmp_key), Some(eodhd_key)) = (fmp_api_key(), eodhd_api_key()) else {
            eprintln!("SKIP: no FMP/EODHD API key");
            return;
        };
        let client = reqwest::Client::new();
        let response = companies_get(
            &client,
            "company_profile",
            "COF.US",
            &fmp_key,
            &eodhd_key,
            &[],
            None,
        )
        .await
        .expect("qualified US symbol must resolve through companies_get");
        let profile = response
            .value
            .as_array()
            .and_then(|array| array.first())
            .expect("profile for COF.US must be non-empty");
        assert_eq!(
            profile.get("symbol").and_then(|value| value.as_str()),
            Some("COF"),
            "the qualified form must reach FMP as the bare ticker"
        );
    }

    #[tokio::test]
    async fn live_country_picks_local_listing() {
        // AAPL also trades as a Canadian CDR on TO (EODHD's own docs
        // sample shape); an explicit country must pick it over the US
        // primary.
        let Some(key) = eodhd_api_key() else {
            eprintln!("SKIP: no EODHD API key");
            return;
        };
        let client = reqwest::Client::new();
        let resolved = resolve_symbol(
            &client,
            &input(Some("AAPL"), None, None, Some("Canada")),
            &key,
        )
        .await
        .expect("AAPL must resolve to a common stock listing");
        assert_eq!(
            resolved.symbol, "AAPL.TO",
            "country=Canada must pick the Canadian CDR over the US primary"
        );
    }
}
