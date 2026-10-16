//! Offline contracts through real tool handlers and provider HTTP normalization.
use super::*;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct FixtureHttp {
    pub(super) origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl FixtureHttp {
    pub(super) async fn start(
        response: impl Fn(&str) -> (u16, Value) + Send + Sync + 'static,
    ) -> Self {
        Self::start_async(move |path| std::future::ready(response(&path))).await
    }

    async fn start_async<F, R>(response: F) -> Self
    where
        F: Fn(String) -> R + Send + Sync + 'static,
        R: std::future::Future<Output = (u16, Value)> + Send + 'static,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let origin = format!("http://{}", listener.local_addr().expect("fixture address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let response = Arc::new(response);
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.expect("connection result").expect("fixture connection task");
                    }
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.expect("accept fixture request");
                        let recorded = recorded.clone();
                        let response = response.clone();
                        connections.spawn(async move {
                            let mut request = Vec::new();
                            loop {
                                let mut buffer = [0; 4096];
                                let length = stream.read(&mut buffer).await.expect("read request");
                                if length == 0 { break; }
                                request.extend_from_slice(&buffer[..length]);
                                if request.windows(4).any(|part| part == b"\r\n\r\n") { break; }
                            }
                            let request = String::from_utf8(request).expect("HTTP request text");
                            let path = request.split_whitespace().nth(1).expect("request path").to_string();
                            recorded.lock().expect("requests lock").push(path.clone());
                            let (status, body) = response(path).await;
                            let body = body.to_string();
                            let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            stream.write_all(response.as_bytes()).await.expect("write response");
                        });
                    }
                }
            }
        });
        Self {
            origin,
            requests,
            task,
        }
    }

    fn count(&self) -> usize {
        self.requests.lock().expect("requests lock").len()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests lock").clone()
    }
}

impl Drop for FixtureHttp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The no-proxy 2s HTTP client every fixture drives.
fn fixture_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .expect("HTTP client")
}

pub(super) fn server(directory: &std::path::Path) -> CompaniesServer {
    CompaniesServer::new(
        hkask_types::WebID::new(),
        fixture_client(),
        "fixture-fmp".into(),
        "fixture-eodhd".into(),
        None,
        None,
        None,
        None,
        ResearchStore::with_dir(directory.join("research")).expect("research fixture"),
        Arc::new(Mutex::new(LearningState::default())),
        superforecast::FermiDefaults::from_env(),
        0.15,
        Some(fibo_cache::FiboDataCache::open(&directory.join("cache.db")).expect("cache fixture")),
        hkask_spreadsheet::WorkbookService::start_with_root(directory.join("spreadsheet"))
            .expect("spreadsheet fixture"),
    )
}

fn fmp_fixture(path: &str) -> (u16, Value) {
    let endpoint = path.split('?').next().expect("endpoint");
    let value = match endpoint {
        "/fmp/key-metrics" => json!([
            {"date":"2025-12-31","fiscalYear":"2025","returnOnInvestedCapital":0.18,"evToEBITDA":12.0},
            {"date":"2024-12-31","fiscalYear":"2024","returnOnInvestedCapital":0.17,"evToEBITDA":11.0}
        ]),
        // Deliberately reverse order: joining by position would corrupt the latest ratios.
        "/fmp/ratios" => json!([
            {"date":"2024-12-31","priceToEarningsRatio":18.0,"grossProfitMargin":0.4},
            {"date":"2025-12-31","priceToEarningsRatio":20.0,"priceToBookRatio":4.0,"priceToSalesRatio":3.0,"grossProfitMargin":0.4}
        ]),
        "/fmp/financial-growth" => json!([
            {"date":"2024-12-31","revenueGrowth":0.08},
            {"date":"2025-12-31","revenueGrowth":0.1}
        ]),
        _ => {
            return (
                404,
                json!({"error":"unexpected fixture endpoint","path":path}),
            );
        }
    };
    (200, value)
}

fn content(output: &str) -> Value {
    serde_json::from_str::<Value>(output).expect("tool JSON")["content"].clone()
}

/// expect: [P5] Metrics completeness must not depend on which tool I call first.
/// dcterms:identifier: CompaniesServer::key_metrics / CompaniesServer::fetch_key_metrics
/// pre: split FMP endpoints, date order differs; post: raw and typed readers contain joined ratios.
#[tokio::test]
async fn raw_first_metrics_are_normalized() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(fmp_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let output = content(
                &server
                    .key_metrics(Parameters(types::SymbolLimitRequest {
                        symbol: "ACME.US".into(),
                        limit: Some(2),
                    }))
                    .await
                    .expect("key metrics tool"),
            );
            assert_eq!(
                output["data"][0]["priceToEarningsRatio"],
                json!(20.0),
                "raw tool must use normalized acquisition"
            );
            let metrics = server
                .fetch_key_metrics("ACME.US", 2)
                .await
                .expect("typed metrics");
            assert_eq!(metrics.pe_ratio(), Some(20.0));
            assert_eq!(metrics.raw(), &output["data"]);
            assert_eq!(output["provider"], "FMP");
            assert_eq!(output["warnings"], json!([]));
            assert_eq!(fixture.count(), 3, "typed read should use cache");
        })
        .await;
}

async fn metrics_tool(server: &CompaniesServer, symbol: &str, limit: u32) -> Value {
    content(
        &server
            .key_metrics(Parameters(types::SymbolLimitRequest {
                symbol: symbol.into(),
                limit: Some(limit),
            }))
            .await
            .expect("metrics tool"),
    )
}

/// expect: [P5] Enriched-first acquisition and a reopened cache preserve the same data and source.
#[tokio::test]
async fn enriched_first_and_reopened_cache_keep_provenance() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(fmp_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let first = server(directory.path());
            let metrics = first
                .fetch_key_metrics("ACME.US", 2)
                .await
                .expect("typed metrics");
            assert_eq!(metrics.pe_ratio(), Some(20.0));
            assert_eq!(metrics.raw()[1]["priceToEarningsRatio"], 18.0);
            assert_eq!(metrics.raw()[0]["roic"], 0.18);
            assert_eq!(metrics.revenue_growth(), Some(0.1));
            let cold = metrics_tool(&first, "ACME.US", 2).await;
            drop(first);
            let reopened = server(directory.path());
            assert_eq!(metrics_tool(&reopened, "ACME.US", 2).await, cold);
            assert_eq!(cold["provider"], "FMP");
            assert_eq!(fixture.count(), 3);
            assert!(
                fixture
                    .requests
                    .lock()
                    .expect("requests")
                    .iter()
                    .all(|request| request.contains("symbol=ACME&")),
                "all FMP endpoints strip .US"
            );
        })
        .await;
}

/// expect: [P5] Old raw-only cache entries cannot poison normalized reads.
#[tokio::test]
async fn legacy_raw_cache_is_rejected_without_deleting_it() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(fmp_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let cache = server.fibo_cache.as_ref().expect("cache");
            let legacy = json!([{"date":"2025-12-31","roic":99.0}]);
            let hash = fibo_cache::hash_params(&[("limit", "2")]);
            cache.store_raw("ACME", "key_metrics", &hash, &legacy, "FMP");
            let output = metrics_tool(&server, "ACME", 2).await;
            assert_eq!(output["data"][0]["roic"], 0.18);
            assert_eq!(output["data"][0]["priceToEarningsRatio"], 20.0);
            assert_eq!(cache.get_raw("ACME", "key_metrics", &hash), Some(legacy));
            assert_eq!(fixture.count(), 3);
        })
        .await;
}

/// expect: [P9] Failed and unmatched supplements remain visibly degraded after reopen.
#[tokio::test]
async fn supplement_failures_and_date_gaps_survive_cache() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/fmp/ratios") {
            return (503, json!({"error":"fixture ratios unavailable"}));
        }
        if path.starts_with("/fmp/financial-growth") {
            return (200, json!([{"date":"1999-12-31","revenueGrowth":9.0}]));
        }
        fmp_fixture(path)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let first = server(directory.path());
            let cold = metrics_tool(&first, "ACME", 2).await;
            assert!(cold["data"][0]["priceToEarningsRatio"].is_null());
            assert!(cold["data"][0]["revenueGrowth"].is_null());
            assert!(
                cold["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .is_some_and(|text| text.contains("ratios") && text.contains("503")))
            );
            assert!(
                cold["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .is_some_and(|text| text.contains("no supplement for date 2025-12-31")))
            );
            drop(first);
            assert_eq!(
                metrics_tool(&server(directory.path()), "ACME", 2).await,
                cold
            );
            assert_eq!(fixture.count(), 3);
        })
        .await;
}

fn financial_fixture(path: &str) -> (u16, Value) {
    let endpoint = path.split('?').next().expect("endpoint");
    if endpoint == "/fmp/profile" {
        return (
            200,
            json!([{"companyName":"Acme","sector":"Technology","industry":"Software","price":30.0,"marketCap":3000000000.0,"sharesOutstanding":100000000.0}]),
        );
    }
    if matches!(
        endpoint,
        "/fmp/income-statement" | "/fmp/balance-sheet-statement" | "/fmp/cash-flow-statement"
    ) {
        let rows: Vec<_> = [("2025", 1000000000.0), ("2024", 1000000000.0 / 1.1), ("2023", 1000000000.0 / 1.1 / 1.08)].into_iter().map(|(year, revenue)| {
            match endpoint {
                "/fmp/income-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"revenue":revenue,"costOfRevenue":revenue*0.6,"grossProfit":revenue*0.4,"depreciationAndAmortization":revenue*0.03,"sellingGeneralAndAdministrativeExpenses":revenue*0.1375,"operatingIncome":revenue*0.2325,"interestExpense":revenue*0.045,"incomeTaxExpense":revenue*0.0375,"incomeBeforeTax":revenue*0.1875,"netIncome":revenue*0.15,"ebitda":revenue*0.2625,"weightedAverageShsOutDil":100000000.0}),
                "/fmp/balance-sheet-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"totalCurrentAssets":revenue*0.3,"totalCurrentLiabilities":revenue*0.15,"cashAndCashEquivalents":revenue*0.05,"longTermDebt":revenue*0.2,"totalStockholdersEquity":revenue*0.75,"totalAssets":revenue*1.1,"totalLiabilities":revenue*0.35}),
                _ => json!({"date":format!("{year}-12-31"),"calendarYear":year,"capitalExpenditure":-revenue*0.04}),
            }
        }).collect();
        return (200, json!(rows));
    }
    fmp_fixture(path)
}

/// The Acme profile fixture arm — the EODHD-routed shape (no `price`
/// field), varying only by listing currency across the DCF price-fallback
/// tests.
fn acme_profile(currency: &str) -> Value {
    json!([{"companyName":"Acme","sector":"Technology","industry":"Software","marketCap":3000000000.0,"sharesOutstanding":100000000.0,"currency":currency}])
}

/// The Acme quote fixture arm — the full FMP quote shape, varying by prices.
fn acme_quote(price: f64, open: f64, high: f64, low: f64) -> Value {
    json!({"symbol":"ACME","price":price,"open":open,"high":high,"low":low})
}

/// The FOREX rate fixture arm — one dated close, varying by rate (the
/// endpoint condition carries the pair).
fn forex_close(date: &str, close: f64) -> Value {
    json!([{"date":date,"close":close}])
}

/// Serve any `/eodhd/eod/USD<PAIR>.FOREX` route from a (pair, date,
/// close) table — the rate lookups the DCF price-fallback fixture routes.
fn forex_routes(path: &str, rates: &[(&str, &str, f64)]) -> Option<(u16, Value)> {
    rates
        .iter()
        .find(|(pair, _, _)| path.starts_with(&format!("/eodhd/eod/USD{pair}.FOREX")))
        .map(|&(_, date, close)| (200, forex_close(date, close)))
}

/// EODHD screener rows for the provider pagination tests: `count` US rows
/// of $1B market cap, coded `C{offset + index}` — the shape the offset-cap
/// and band-split tests page through.
fn billion_cap_rows(count: usize, code_offset: usize) -> Vec<Value> {
    (0..count)
        .map(|index| {
            json!({
                "code": format!("C{}", code_offset + index),
                "exchange": "US",
                "market_capitalization": 1_000_000_000.0,
            })
        })
        .collect()
}

/// The income-statement arm that pins statement currency to USD —
/// byte-identical across the DCF price-fallback tests.
fn usd_income_statements(path: &str) -> (u16, Value) {
    let (status, mut rows) = financial_fixture(path);
    for row in rows.as_array_mut().expect("income rows") {
        row["reportedCurrency"] = json!("USD");
    }
    (status, rows)
}

/// The DCF price-fallback fixture: an EODHD-routed Acme profile (no
/// `price` field) in `currency`, the `/fmp/quote` body, the FOREX rate
/// table, USD income statements, and the financial fallback — the
/// composition the three price-fallback tests share, varying exactly
/// these inputs.
fn price_fallback_fixture(
    currency: &'static str,
    quote: Value,
    forex: &'static [(&'static str, &'static str, f64)],
) -> impl Fn(&str) -> (u16, Value) + Send + Sync + 'static {
    move |path| {
        let endpoint = path.split('?').next().expect("endpoint");
        if endpoint == "/fmp/profile" {
            return (200, acme_profile(currency));
        }
        if endpoint == "/fmp/quote" {
            return (200, quote.clone());
        }
        if let Some(response) = forex_routes(path, forex) {
            return response;
        }
        if endpoint == "/fmp/income-statement" {
            return usd_income_statements(path);
        }
        financial_fixture(path)
    }
}

/// The reverse-DCF tool call over the ACME fixture symbol — the shared
/// choreography of the two reverse-DCF price-fallback tests.
async fn reverse_dcf_output(server: &CompaniesServer) -> Value {
    let request = serde_json::from_value::<types::ReverseDcfRequest>(json!({
        "symbol": "ACME"
    }))
    .expect("request");
    content(
        &server
            .reverse_dcf(Parameters(request))
            .await
            .expect("reverse dcf tool"),
    )
}

async fn comparable(server: &CompaniesServer, request: Value) -> Value {
    content(
        &server
            .comparable_analysis(Parameters(
                serde_json::from_value(request).expect("comparable request"),
            ))
            .await
            .expect("comparable tool"),
    )
}

/// A fixture responder whose one arm overrides `financial_fixture` — the
/// shape the overlay-error and empty-target tests share, varying exactly
/// the overriding arm (None falls through to the financial fixture).
fn override_financial_fixture(
    arm: impl Fn(&str) -> Option<(u16, Value)> + Send + Sync + 'static,
) -> impl Fn(&str) -> (u16, Value) + Send + Sync + 'static {
    move |path| {
        if let Some(response) = arm(path) {
            return response;
        }
        financial_fixture(path)
    }
}

/// Run one scoped comparable_analysis call over the ACME+PEER request and
/// return its content — the scope + server + call choreography the
/// overlay-error and empty-target tests share (the request is identical).
async fn scoped_comparable_output(directory: &std::path::Path, fixture: &FixtureHttp) -> Value {
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            comparable(&server(directory), json!({"symbol":"ACME","peers":"PEER"})).await
        })
        .await
}

/// expect: [P5] The reverse DCF's current price falls back to the stock
/// quote's close when the profile carries no `price` field (every
/// EODHD-routed, exchange-qualified symbol — live-observed 2026-09-10:
/// `expectations_gap` and `reverse_dcf` returned no market-implied growth
/// for DNB.OL, PKN.WAR, NTDOY on exactly this shape) — the implied growth
/// resolves and the price source is surfaced. (Historically tested through
/// `reverse_dcf` because `expectations_gap` required research-search
/// credentials the fixture server does not carry; since compact mode,
/// `expectations_gap` is fixture-testable directly — see the consolidated
/// EODHD acquisition test.)
/// dcterms:identifier: CompaniesServer::reverse_dcf / forecast::resolve_current_price
#[tokio::test]
async fn reverse_dcf_price_falls_back_to_quote_close() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(price_fallback_fixture(
        "USD",
        acme_quote(30.0, 29.0, 31.0, 28.0),
        &[],
    ))
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let output = reverse_dcf_output(&server).await;
            assert_eq!(
                output["price_source"],
                json!("stock_quote; currency_normalized:USD->USD")
            );
            assert_eq!(output["current_price"], json!(30.0));
            assert!(
                output["implied_growth_rate"]
                    .as_f64()
                    .is_some_and(f64::is_finite),
                "implied growth must resolve with the quote-close price"
            );
        })
        .await;
}

/// expect: [P5] The DCF prices an EODHD-routed profile (no `price` field) from
/// the stock quote's close, converted from the listing currency into the
/// statement currency — live-observed 2026-09-26: VIRI.PA (EUR quote, USD
/// statements) returned "missing or invalid current price for DCF".
/// dcterms:identifier: valuation_service::prepare_dcf
#[tokio::test]
async fn dcf_price_falls_back_to_normalized_quote_close() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(price_fallback_fixture(
        "EUR",
        json!({"symbol":"ACME","close":25.0}),
        &[("EUR", "2026-09-25", 0.8)],
    ))
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let output = standalone_dcf(&server).await;
            assert!(output["error"].is_null(), "{output}");
            assert!(
                output["valuation"]["current_price"]
                    .as_f64()
                    .is_some_and(|price| (price - 31.25).abs() < 1e-6),
                "EUR quote must be converted into USD statement units: {output}"
            );
        })
        .await;
}

/// expect: [P5] A quote in pence is converted through pounds into the USD
/// statement currency before reverse DCF; raw GBX is never treated as USD.
/// dcterms:identifier: CompaniesServer::reverse_dcf / CompaniesServer::normalize_price_for_financials
#[tokio::test]
async fn reverse_dcf_normalizes_gbx_quote_to_usd_statements() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(price_fallback_fixture(
        "GBX",
        acme_quote(1603.0, 1600.0, 1620.0, 1580.0),
        &[("GBP", "2026-09-11", 0.7396)],
    ))
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let output = reverse_dcf_output(&server).await;
            assert_eq!(
                output["price_source"],
                json!("stock_quote; currency_normalized:GBX->USD")
            );
            assert!(
                output["current_price"]
                    .as_f64()
                    .is_some_and(|price| (price - 21.673_877_77).abs() < 1e-6),
                "GBX quote must be converted into USD statement units: {output}"
            );
        })
        .await;
}

/// expect: [P5] Comparable overlay agrees with standalone DCF for identical history and assumptions.
#[tokio::test]
async fn overlay_matches_standalone_dcf() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(financial_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            for overrides in [
                json!({}),
                json!({"discount_rate":0.12,"terminal_growth":0.02}),
            ] {
                let mut request = overrides;
                request["symbol"] = json!("ACME");
                let standalone = content(
                    &server
                        .dcf_valuation(Parameters(
                            serde_json::from_value(request.clone()).expect("DCF request"),
                        ))
                        .await
                        .expect("DCF tool"),
                );
                assert!(
                    standalone["valuation"]["intrinsic_per_share"]
                        .as_f64()
                        .is_some_and(|value| value > 0.0),
                    "{standalone}"
                );
                request["peers"] = json!("PEER");
                let comparison = comparable(&server, request).await;
                for field in ["intrinsic_per_share", "current_price", "margin_of_safety"] {
                    assert_eq!(
                        comparison["dcf_overlay"][field], standalone["valuation"][field],
                        "overlay mismatch in {field}: {comparison}"
                    );
                }
                assert_eq!(comparison["dcf_overlay"]["current_price"], 30.0);
            }
        })
        .await;
}

/// EODHD's real wire shape (verified live 2026-09-10, DNB.OL): Financials
/// monetary values arrive as decimal strings; Highlights and General are
/// JSON numbers. The normalizer coerces the strings to numbers so the
/// normalized payload matches FMP's shape.
fn eodhd_fixture() -> Value {
    json!({
        "General":{"Code":"GLOBAL","Name":"Global","GicSector":"Technology","Industry":"Software"},
        "Highlights":{"MarketCapitalization":3000000000.0,"DividendYield":0.02,"EBITDA":300000000.0},
        "Financials":{
            "Income_Statement":{"yearly":{
                "2025-12-31":{"totalRevenue":"1000000000.00","grossProfit":"400000000.00","costOfRevenue":"600000000.00","sellingGeneralAdministrative":"100000000.00","operatingIncome":"300000000.00","netIncome":"150000000.00"},
                "2024-12-31":{"totalRevenue":"900000000.00","grossProfit":"360000000.00","costOfRevenue":"540000000.00","sellingGeneralAdministrative":"90000000.00","operatingIncome":"270000000.00","netIncome":"135000000.00"}
            }},
            "Balance_Sheet":{"yearly":{
                "2025-12-31":{"totalAssets":"1200000000.00","totalStockholderEquity":"750000000.00","netInvestedCapital":"900000000.00","netDebt":"150000000.00","accountsPayable":"50000000.00","netReceivables":"100000000.00","inventory":"50000000.00","commonStockSharesOutstanding":"100000000.00"},
                "2024-12-31":{"totalAssets":"1080000000.00","totalStockholderEquity":"675000000.00","netInvestedCapital":"810000000.00","netDebt":"135000000.00"}
            }}
        }
    })
}

/// An `eodhd_fixture()` mutated into one issuer's fundamentals base: the
/// General identity block (code, name, Common Stock, currency, explicit
/// primary ticker, not delisted) and the Financials currency symbols with
/// the Cash_Flow block, all in `currency` — and no identity code (ISIN/LEI):
/// the caller's identity shape is its own. `eodhd_issuer` builds its ISIN
/// form on this base; the GAP.PA per-row-currency and LEI-only variants
/// build their bespoke differences on it too.
fn eodhd_issuer_base(code: &str, name: &str, ticker: &str, currency: &str) -> Value {
    let mut value = eodhd_fixture();
    value["General"]["Code"] = json!(code);
    value["General"]["Name"] = json!(name);
    value["General"]["Type"] = json!("Common Stock");
    value["General"]["CurrencyCode"] = json!(currency);
    value["General"]["PrimaryTicker"] = json!(ticker);
    value["General"]["IsDelisted"] = json!(false);
    value["Financials"]["Income_Statement"]["currency_symbol"] = json!(currency);
    value["Financials"]["Balance_Sheet"]["currency_symbol"] = json!(currency);
    value["Financials"]["Cash_Flow"] = json!({
        "currency_symbol": currency, "yearly": {
            "2025-12-31": {"totalCashFromOperatingActivities":"180000000.00","capitalExpenditures":"-30000000.00","dividendsPaid":"-20000000.00"},
            "2024-12-31": {"totalCashFromOperatingActivities":"170000000.00","capitalExpenditures":"-28000000.00","dividendsPaid":"-18000000.00"}
        }
    });
    value
}
/// expect: [P5] EODHD SG&A reaches the normalized financial model instead of
/// becoming a fabricated zero operating expense.
/// pre: the provider payload uses EODHD's sellingGeneralAdministrative field.
/// post: normalized income statements expose the FMP-compatible SG&A field.
/// [P1] Constraining: provider normalization preserves reported economics.
#[test]
fn eodhd_normalization_preserves_sga() {
    let normalized = providers::normalize_eodhd("income_statement", &eodhd_fixture(), "GLOBAL.LSE");
    assert_eq!(
        normalized[0]["sellingGeneralAndAdministrativeExpenses"],
        100_000_000.0
    );
}

/// expect: [P5] EODHD annual metrics do not need quarterly Earnings.History or FMP supplements.
#[tokio::test]
async fn eodhd_normalization_is_provider_pure_in_both_orders_and_after_reopen() {
    for typed_first in [false, true] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(|path| {
            if path.starts_with("/eodhd/fundamentals/") {
                (200, eodhd_fixture())
            } else {
                (500, json!({"error":"FMP must not supplement EODHD"}))
            }
        })
        .await;
        providers::TEST_HTTP_ORIGIN
            .scope(fixture.origin.clone(), async {
                let first = server(directory.path());
                if typed_first {
                    first
                        .fetch_key_metrics("GLOBAL.LSE", 2)
                        .await
                        .expect("typed metrics");
                }
                let output = metrics_tool(&first, "GLOBAL.LSE", 2).await;
                assert_eq!(output["data"].as_array().expect("annual metrics").len(), 2);
                let metrics = first
                    .fetch_key_metrics("GLOBAL.LSE", 2)
                    .await
                    .expect("typed metrics");
                assert_eq!(metrics.pe_ratio(), Some(20.0));
                assert_eq!(metrics.price_to_book(), Some(4.0));
                assert_eq!(metrics.price_to_sales(), Some(3.0));
                assert_eq!(metrics.ev_to_ebitda(), Some(10.5));
                assert_eq!(metrics.raw()[0]["grossProfitMargin"], 0.4);
                // Operating income 300m after tax (none reported) / net invested capital 900m.
                assert!(
                    (metrics.raw()[0]["roic"].as_f64().expect("roic") - 1.0 / 3.0).abs() < 1e-12
                );
                assert!((metrics.revenue_growth().expect("growth") - 1.0 / 9.0).abs() < 1e-12);
                assert_eq!(output["provider"], "EODHD");
                assert!(output["warnings"].to_string().contains("approximates"));
                drop(first);
                assert_eq!(
                    metrics_tool(&server(directory.path()), "GLOBAL.LSE", 2).await,
                    output
                );
                assert_eq!(fixture.count(), 1);
            })
            .await;
    }
}

/// expect: [P9] An absent EODHD net-debt input does not become a fabricated zero-debt multiple.
#[tokio::test]
async fn eodhd_missing_debt_leaves_ev_multiple_absent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|_| {
        let mut value = eodhd_fixture();
        value["Financials"]["Balance_Sheet"]["yearly"]["2025-12-31"]
            .as_object_mut()
            .expect("balance")
            .remove("netDebt");
        (200, value)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let metrics = server(directory.path())
                .fetch_key_metrics("GLOBAL.LSE", 2)
                .await
                .expect("metrics");
            assert_eq!(metrics.pe_ratio(), Some(20.0));
            assert_eq!(metrics.ev_to_ebitda(), None);
        })
        .await;
}

/// expect: [P5] Targets and peers reuse the same cached data despite later provider changes.
#[tokio::test]
async fn target_and_peers_share_cache_and_learning_policy() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/") {
            (200, eodhd_fixture())
        } else {
            financial_fixture(path)
        }
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let first = server(directory.path());
            let profile = first
                .fetch_profile("PEER")
                .await
                .expect("prime peer profile");
            let metrics = first
                .fetch_key_metrics("PEER", 1)
                .await
                .expect("prime peer metrics");
            // Force subsequent live acquisitions to EODHD: warmed FMP data must
            // remain the same for either role until its TTL expires.
            for symbol in ["PEER", "LEARNED"] {
                for _ in 0..5 {
                    first
                        .learning
                        .lock()
                        .expect("learning")
                        .record(symbol, Provider::Fmp, Some(1));
                }
            }
            let output = comparable(&first, json!({"symbol":"ACME","peers":"PEER,LEARNED"})).await;
            assert_eq!(output["comparison"][1]["price"], json!(profile.price()));
            assert_eq!(
                output["comparison"][1]["pe_ratio"],
                json!(metrics.pe_ratio())
            );
            assert_eq!(output["comparison"][1]["provenance"]["key_metrics"], "FMP");
            assert_eq!(
                output["comparison"][2]["provenance"]["key_metrics"],
                "EODHD"
            );
            assert_eq!(output["comparison"][2]["pe_ratio"], 20.0);
            assert!(
                !fixture
                    .requests
                    .lock()
                    .expect("requests")
                    .iter()
                    .any(|request| request.starts_with("/fmp/")
                        && request.contains("symbol=LEARNED"))
            );
            let peer_requests = fixture
                .requests
                .lock()
                .expect("requests")
                .iter()
                .filter(|request| request.contains("symbol=PEER&"))
                .count();
            assert_eq!(
                peer_requests, 4,
                "peer profile+metrics must be fetched only while priming"
            );
            let calls = fixture.count();
            drop(first);
            let reopened = server(directory.path());
            let warm = comparable(&reopened, json!({"symbol":"ACME","peers":"PEER,LEARNED"})).await;
            assert_eq!(warm, output);
            assert_eq!(
                fixture.count(),
                calls,
                "entire comparison should be warm after reopen"
            );
        })
        .await;
}

/// expect: [P9] Failed peers remain identified; no invented provider or zero multiples.
#[tokio::test]
async fn failed_and_empty_peers_are_visible() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.contains("BROKEN") {
            (503, json!({"error":"fixture peer unavailable"}))
        } else if path.contains("EMPTY") {
            (200, json!([]))
        } else {
            financial_fixture(path)
        }
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let output = comparable(
                &server(directory.path()),
                json!({"symbol":"ACME","peers":"BROKEN,EMPTY,../bad"}),
            )
            .await;
            let rows = output["comparison"].as_array().expect("rows");
            assert_eq!(rows.len(), 4);
            for row in rows.iter().skip(1) {
                assert!(
                    row["errors"]
                        .as_array()
                        .is_some_and(|errors| !errors.is_empty()),
                    "{row}"
                );
                assert!(row["price"].is_null());
                assert!(row["pe_ratio"].is_null());
            }
            assert_eq!(rows[1]["symbol"], "BROKEN");
            assert_eq!(rows[1]["provenance"], json!({}));
            assert!(
                !fixture
                    .requests
                    .lock()
                    .expect("requests")
                    .iter()
                    .any(|path| path.contains("bad"))
            );
        })
        .await;
}

/// expect: [P9] Unsupported sectors and inadequate inputs fail explicitly in both DCF views.
#[tokio::test]
async fn dcf_guards_agree_between_tools() {
    for case in ["sector", "history", "price", "shares"] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(move |path| {
            let (status, mut data) = financial_fixture(path);
            if path.starts_with("/fmp/profile") {
                if case == "sector" {
                    data[0]["sector"] = json!("Financial Services");
                }
                if case == "price" {
                    data[0].as_object_mut().expect("profile").remove("price");
                }
                if case == "shares" {
                    data[0]
                        .as_object_mut()
                        .expect("profile")
                        .remove("sharesOutstanding");
                }
            }
            if path.starts_with("/fmp/income-statement") {
                if case == "history" {
                    data.as_array_mut().expect("income").truncate(1);
                }
                if case == "shares" {
                    for row in data.as_array_mut().expect("income") {
                        row.as_object_mut()
                            .expect("income row")
                            .remove("weightedAverageShsOutDil");
                    }
                }
            }
            (status, data)
        })
        .await;
        providers::TEST_HTTP_ORIGIN
            .scope(fixture.origin.clone(), async {
                let server = server(directory.path());
                let standalone = content(
                    &server
                        .dcf_valuation(Parameters(
                            serde_json::from_value(json!({"symbol":"ACME"})).expect("request"),
                        ))
                        .await
                        .expect("DCF tool"),
                );
                let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
                assert!(standalone["error"].is_string(), "{case}: {standalone}");
                assert_eq!(comparison["dcf_overlay"], standalone, "{case}");
                let expected = match case {
                    "sector" => "financial-sector",
                    "history" => "at least 2 years",
                    "price" => "current price",
                    _ => "shares outstanding",
                };
                assert!(
                    standalone["error"]
                        .as_str()
                        .expect("error text")
                        .contains(expected)
                );
            })
            .await;
    }
}

/// expect: [P9] Both DCF tools reject the same invalid assumptions with a typed error.
#[tokio::test]
async fn invalid_dcf_assumptions_agree_between_tools() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(financial_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request =
                json!({"symbol":"ACME","peers":"PEER","discount_rate":0.10,"terminal_growth":0.12});
            let standalone = server
                .dcf_valuation(Parameters(
                    serde_json::from_value(request.clone()).expect("request"),
                ))
                .await
                .expect_err("invalid DCF");
            let comparison = server
                .comparable_analysis(Parameters(
                    serde_json::from_value(request).expect("request"),
                ))
                .await
                .expect_err("invalid overlay");
            assert_eq!(standalone.kind, hkask_types::McpErrorKind::InvalidArgument);
            assert_eq!(comparison.kind, standalone.kind);
            assert_eq!(comparison.message, standalone.message);
        })
        .await;
}

/// expect: [P9] Fallback provenance identifies the provider that actually supplied cached metrics.
#[tokio::test]
async fn fallback_provenance_survives_reopen_in_both_directions() {
    for (symbol, expected_provider) in [("ACME.US", "EODHD"), ("GLOBAL.LSE", "FMP")] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(move |path| {
            if expected_provider == "EODHD" {
                if path.starts_with("/fmp/") {
                    (503, json!({"error":"FMP unavailable"}))
                } else {
                    (200, eodhd_fixture())
                }
            } else if path.starts_with("/eodhd/") {
                (503, json!({"error":"EODHD unavailable"}))
            } else {
                fmp_fixture(path)
            }
        })
        .await;
        providers::TEST_HTTP_ORIGIN
            .scope(fixture.origin.clone(), async {
                let first = server(directory.path());
                let output = metrics_tool(&first, symbol, 2).await;
                assert_eq!(output["provider"], expected_provider);
                assert_eq!(output["data"][0]["priceToEarningsRatio"], 20.0);
                let calls = fixture.count();
                drop(first);
                assert_eq!(
                    metrics_tool(&server(directory.path()), symbol, 2).await,
                    output
                );
                assert_eq!(fixture.count(), calls);
                assert!(
                    !fixture
                        .requests
                        .lock()
                        .expect("requests")
                        .iter()
                        .any(|request| request.contains(".US.US"))
                );
            })
            .await;
    }
}

/// expect: [P9] Malformed supplement payloads are visible, not treated as complete enrichment.
#[tokio::test]
async fn malformed_supplements_are_visible() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/fmp/ratios") || path.starts_with("/fmp/financial-growth") {
            (200, json!({"unexpected":"object"}))
        } else {
            fmp_fixture(path)
        }
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let output = metrics_tool(&server(directory.path()), "ACME", 2).await;
            assert_eq!(output["warnings"].as_array().expect("warnings").len(), 2);
            assert!(output["warnings"].to_string().contains("expected an array"));
            assert!(output["data"][0]["priceToEarningsRatio"].is_null());
            assert!(output["data"][0]["revenueGrowth"].is_null());
        })
        .await;
}

/// expect: [P5] One slow peer does not serialize the remaining peer acquisitions.
#[tokio::test]
async fn peer_acquisition_is_concurrent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let fixture = FixtureHttp::start_async(move |path| {
        let barrier = barrier.clone();
        async move {
            if path.starts_with("/fmp/profile")
                && (path.contains("symbol=FIRST&") || path.contains("symbol=SECOND&"))
            {
                barrier.wait().await;
            }
            financial_fixture(&path)
        }
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let output = comparable(
                &server(directory.path()),
                json!({"symbol":"ACME","peers":"FIRST,SECOND"}),
            )
            .await;
            for row in output["comparison"].as_array().expect("rows") {
                assert_eq!(
                    row["errors"],
                    json!([]),
                    "both peer profiles must reach the barrier concurrently: {row}"
                );
                assert_eq!(row["price"], 30.0);
            }
        })
        .await;
}

/// expect: [P9] An unavailable overlay does not hide usable peer comparisons.
#[tokio::test]
async fn overlay_acquisition_error_preserves_comparison_table() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(override_financial_fixture(|path| {
        if path.starts_with("/fmp/income-statement") || path.starts_with("/eodhd/") {
            return Some((503, json!({"error":"statements unavailable"})));
        }
        None
    }))
    .await;
    let output = scoped_comparable_output(directory.path(), &fixture).await;
    assert_eq!(output["comparison"][1]["pe_ratio"], 20.0);
    assert!(output["dcf_overlay"]["error"].is_string());
    assert!(output["dcf_overlay"]["kind"].is_string());
}

/// expect: [P1] Transport failures must never publish or persist provider credentials.
#[tokio::test]
async fn review_transport_timeout_redacts_key_on_cold_and_warm_cache() {
    const SENTINEL: &str = "fixture-fmp-SENTINEL-not-a-real-secret";
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start_async(|path| async move {
        if path.starts_with("/fmp/ratios") {
            std::future::pending::<()>().await;
        }
        fmp_fixture(&path)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let mut first = server(directory.path());
            first.fmp_api_key = SENTINEL.into();
            let cold = metrics_tool(&first, "ACME", 2).await;
            assert_eq!(cold["data"][0]["roic"], 0.18);
            assert_eq!(cold["data"][0]["revenueGrowth"], 0.1);
            assert!(cold["data"][0]["priceToEarningsRatio"].is_null());
            let serialized_cache: String =
                rusqlite::Connection::open(directory.path().join("cache.db"))
                    .expect("cache DB")
                    .query_row(
                        "SELECT raw_response FROM fibo_raw_cache WHERE endpoint = 'key_metrics' AND params_hash = ?1",
                        [acquisition_cache_key(&[("limit", "2")])],
                        |row| row.get(0),
                    )
                    .expect("persisted cache row");
            let calls = fixture.count();
            drop(first);
            let warm = metrics_tool(&server(directory.path()), "ACME", 2).await;
            assert_eq!(cold, warm);
            assert_eq!(fixture.count(), calls);
            for (surface, text) in [
                ("cold output", cold.to_string()),
                ("persisted cache", serialized_cache),
                ("warm output", warm.to_string()),
            ] {
                assert!(
                    !text.contains(SENTINEL)
                        && !text.contains("apikey")
                        && !text.contains("fixture-fmp"),
                    "{surface} leaked the sentinel credential or its query parameter"
                );
            }
            let warning = cold["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .find_map(|warning| {
                    warning
                        .as_str()
                        .and_then(|text| text.strip_prefix("FMP ratios: "))
                })
                .expect("ratios warning");
            let error: Value = serde_json::from_str(warning).expect("typed warning error");
            assert_eq!(
                error["kind"],
                hkask_types::McpErrorKind::Unavailable.to_string()
            );
            let message = error["error"].as_str().expect("actionable message");
            assert!(
                message.contains("FMP")
                    && message.contains("/ratios")
                    && message.contains("timed out"),
                "{message}"
            );
        })
        .await;
}

async fn standalone_dcf(server: &CompaniesServer) -> Value {
    content(
        &server
            .dcf_valuation(Parameters(
                serde_json::from_value(json!({"symbol":"ACME"})).expect("request"),
            ))
            .await
            .expect("DCF tool"),
    )
}

// ── scenario_impact_valuation presentation (PR-10) ──────────────────────

/// A minimal two-node scenario impact request: the flat tree format the
/// tool normalizes, one growth delta and one margin delta.
fn impact_valuation_request(presentation: Option<Value>) -> Value {
    let mut request = json!({
        "symbol": "ACME",
        "scenario_tree": json!({
            "nodes": [
                {"id": "evt-launch", "marginal_probability": 0.6, "depends_on": []},
                {"id": "evt-scale", "marginal_probability": 0.42, "depends_on": []}
            ],
            "topological_order": ["evt-launch", "evt-scale"]
        }).to_string(),
        "impact_mappings": json!([
            {"node_id": "evt-launch", "yes_deltas": {"revenue_growth": 0.02}},
            {"node_id": "evt-scale", "yes_deltas": {"gross_margin": 0.01}}
        ]).to_string(),
    });
    if let Some(p) = presentation {
        request["presentation"] = p;
    }
    request
}

/// Run one scenario_impact_valuation call over the two-node fixture request
/// and return its content — the choreography both PR-10 presentation tests
/// share, varying exactly the presentation argument.
async fn scenario_impact_output(presentation: Option<Value>) -> Value {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(financial_fixture).await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            content(
                &server
                    .scenario_impact_valuation(Parameters(
                        serde_json::from_value(impact_valuation_request(presentation))
                            .expect("request"),
                    ))
                    .await
                    .expect("scenario_impact_valuation tool"),
            )
        })
        .await
}

/// PR-10: the default presentation is DataOnly — the output carries NO
/// display_hint key. The default path is byte-identical to the
/// pre-PR-10 shape: the presentation branch adds nothing.
#[tokio::test]
async fn scenario_impact_valuation_default_has_no_display_hint() {
    let output = scenario_impact_output(None).await;
    assert!(
        output["probability_weighted_intrinsic"].is_number(),
        "the valuation computes, got: {output}"
    );
    assert!(
        output.get("display_hint").is_none(),
        "the default presentation adds no display_hint key, got: {output}"
    );
}

/// PR-10: the WorkbookWhatIf presentation publishes the path grid as an
/// editable workbook revision and appends its ```spreadsheet display
/// hint — a fenced block whose body carries the published table.
#[tokio::test]
async fn scenario_impact_valuation_workbook_whatif_publishes_the_path_grid() {
    let output = scenario_impact_output(Some(json!("WorkbookWhatIf"))).await;
    let hint = output["display_hint"].as_str().unwrap_or_else(|| {
        panic!("the workbook presentation appends a display_hint, got: {output}")
    });
    assert!(
        hint.starts_with("```spreadsheet\n"),
        "the hint is a fenced spreadsheet block, got: {hint}"
    );
    assert!(
        hint.contains("Scenario impact staging — ACME"),
        "the published workbook carries the path-grid table, got: {hint}"
    );
    // The valuation output itself is unchanged by the presentation —
    // the hint is additive.
    assert!(output["probability_weighted_intrinsic"].is_number());
    assert_eq!(
        output["path_count"],
        json!(4),
        "2 nodes → 4 paths, got: {output}"
    );
}

async fn assert_null_shares_fallback(basic: bool) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(move |path| {
        let (status, mut value) = financial_fixture(path);
        if path.starts_with("/fmp/income-statement") {
            value[0]["weightedAverageShsOutDil"] = Value::Null;
            if basic {
                value[0]["weightedAverageShsOut"] = json!(80000000.0);
            }
        }
        (status, value)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let standalone = standalone_dcf(&server).await;
            assert_eq!(
                standalone["history"]["shares_outstanding"],
                if basic {
                    json!(80000000.0)
                } else {
                    json!(100000000.0)
                },
                "{standalone}"
            );
            let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
            for field in ["intrinsic_per_share", "current_price", "margin_of_safety"] {
                assert!(standalone["valuation"][field].is_number());
                assert_eq!(
                    comparison["dcf_overlay"][field],
                    standalone["valuation"][field]
                );
            }
        })
        .await;
}

/// expect: [P5] A null diluted share field must not hide available profile shares.
#[tokio::test]
async fn review_null_diluted_shares_fall_back_to_profile() {
    assert_null_shares_fallback(false).await;
}

/// expect: [P5] Basic shares take precedence over profile shares when diluted is null.
#[tokio::test]
async fn review_null_diluted_shares_fall_back_to_basic() {
    assert_null_shares_fallback(true).await;
}

/// expect: [P9] Missing capex is equally visible in standalone DCF and its comparable overlay.
#[tokio::test]
async fn review_overlay_reports_same_missing_capex_quality() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        let (status, mut value) = financial_fixture(path);
        if path.starts_with("/fmp/cash-flow-statement") {
            for row in value.as_array_mut().expect("cash flow") {
                row.as_object_mut()
                    .expect("cash flow row")
                    .remove("capitalExpenditure");
            }
        }
        (status, value)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let standalone = standalone_dcf(&server).await;
            let quality = &standalone["data_quality"];
            assert_eq!(quality["capex_to_revenue"]["confidence"], 0.0);
            assert!(
                quality["capex_to_revenue"]["confidence_note"]
                    .as_str()
                    .is_some_and(|note| note.contains("missing data"))
            );
            let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
            assert_eq!(&comparison["dcf_overlay"]["data_quality"], quality);
            for field in ["intrinsic_per_share", "current_price", "margin_of_safety"] {
                assert!(standalone["valuation"][field].is_number());
                assert_eq!(
                    comparison["dcf_overlay"][field],
                    standalone["valuation"][field]
                );
            }
        })
        .await;
}

/// expect: [P9] Empty target metrics are identified just like peer metrics, without hiding the overlay.
#[tokio::test]
async fn review_empty_target_metrics_report_endpoint_error() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(override_financial_fixture(|path| {
        if path.starts_with("/fmp/key-metrics") && path.contains("limit=1") {
            return Some((200, json!([])));
        }
        None
    }))
    .await;
    let output = scoped_comparable_output(directory.path(), &fixture).await;
    assert_eq!(output["comparison"][0]["price"], 30.0);
    assert!(output["dcf_overlay"]["intrinsic_per_share"].is_number());
    let errors = &output["comparison"][0]["errors"];
    assert!(
        errors
            .as_array()
            .expect("target errors")
            .iter()
            .any(|error| error["endpoint"] == "key_metrics"),
        "{output}"
    );
    assert_eq!(errors, &output["comparison"][1]["errors"]);
    assert!(output["comparison"][0]["pe_ratio"].is_null());
}

/// expect: [P9] Invalid explicit numeric shares are rejected, not hidden by a valid fallback.
#[tokio::test]
async fn review_nonpositive_shares_do_not_fall_through() {
    for shares in [0.0, -1.0] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(move |path| {
            let (status, mut value) = financial_fixture(path);
            if path.starts_with("/fmp/income-statement") {
                value[0]["weightedAverageShsOutDil"] = json!(shares);
                value[0]["weightedAverageShsOut"] = json!(80000000.0);
            }
            (status, value)
        })
        .await;
        providers::TEST_HTTP_ORIGIN
            .scope(fixture.origin.clone(), async {
                let server = server(directory.path());
                let standalone = standalone_dcf(&server).await;
                assert!(
                    standalone["error"]
                        .as_str()
                        .is_some_and(|error| error.contains("shares outstanding"))
                );
                let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
                assert_eq!(comparison["dcf_overlay"], standalone);
            })
            .await;
    }
}

/// expect: [P5] Numeric metrics shares resolve before profile shares; null metrics diluted uses basic.
#[tokio::test]
async fn review_metrics_shares_fallback_matches_history() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        let (status, mut value) = financial_fixture(path);
        if path.starts_with("/fmp/income-statement") {
            value[0]["weightedAverageShsOutDil"] = Value::Null;
        }
        if path.starts_with("/fmp/key-metrics") {
            value[0]["weightedAverageShsOutDil"] = Value::Null;
            value[0]["weightedAverageShsOut"] = json!(90000000.0);
        }
        (status, value)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let standalone = standalone_dcf(&server).await;
            assert_eq!(standalone["history"]["shares_outstanding"], 90000000.0);
            let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
            assert_eq!(
                comparison["dcf_overlay"]["intrinsic_per_share"],
                standalone["valuation"]["intrinsic_per_share"]
            );
        })
        .await;
}

/// expect: [P5] Other models retain the nominal fallback only when no numeric shares resolve.
#[test]
fn review_history_retains_nominal_missing_shares_fallback() {
    let profile = json!({});
    let history = financial_model::HistoricalSnapshot::from_api_json(&[], &[], &[], &[], &profile);
    assert_eq!(
        financial_model::resolve_shares_outstanding(&[], &[], &profile),
        None
    );
    assert_eq!(history.shares_outstanding, 1000.0);
}

/// expect: [P1] Pre-sanitizer cached warnings must never be returned or logged.
#[tokio::test]
async fn legacy_presanitizer_cache_is_not_returned_or_logged() {
    use tracing::instrument::WithSubscriber;

    const SENTINEL: &str = "fixture-fmp-LEGACY-SENTINEL";
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(fmp_fixture).await;
    let legacy_key = format!(
        "normalized-v1:{}",
        fibo_cache::hash_params(&[("limit", "2")])
    );
    let legacy = json!({
        "value": [{"date":"2025-12-31","roic":99.0}],
        "provider": "FMP",
        "warnings": [format!("FMP ratios: {}", json!({
            "kind":"unavailable",
            "error":format!("FMP request failed: error sending request for url (https://financialmodelingprep.com/stable/ratios?symbol=ACME&apikey={SENTINEL})")
        }))],
    });
    let first = server(directory.path());
    let cache = first.fibo_cache.as_ref().expect("cache");
    cache.store_raw("ACME", "key_metrics", &legacy_key, &legacy, "FMP");
    assert_eq!(
        cache.get_raw("ACME", "key_metrics", &legacy_key),
        Some(legacy.clone()),
        "legacy fixture must be fresh and readable before acquisition"
    );

    let log_path = directory.path().join("acquisition.log");
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(Mutex::new(
            std::fs::File::create(&log_path).expect("log file"),
        ))
        .finish();
    let (cold, warm, current) = providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            tracing::warn!("scoped-acquisition-capture-active");
            let cold = metrics_tool(&first, "ACME", 2).await;
            drop(first);
            let reopened = server(directory.path());
            let warm = metrics_tool(&reopened, "ACME", 2).await;
            let cache = reopened.fibo_cache.as_ref().expect("reopened cache");
            let current = cache
                .get_raw(
                    "ACME",
                    "key_metrics",
                    &acquisition_cache_key(&[("limit", "2")]),
                )
                .expect("current cache entry");
            assert_eq!(
                cache.get_raw("ACME", "key_metrics", &legacy_key),
                Some(legacy),
                "old bytes remain; invalidation must not delete them"
            );
            (cold, warm, current)
        })
        .with_subscriber(subscriber)
        .await;

    let logs = std::fs::read_to_string(&log_path).expect("captured logs");
    assert!(
        logs.contains("scoped-acquisition-capture-active"),
        "scoped tracing must actually capture events"
    );
    assert!(
        !logs.contains(SENTINEL) && !logs.contains("apikey"),
        "pre-sanitizer credentials reached tracing"
    );
    assert_eq!(
        fixture.count(),
        3,
        "must fetch base/ratios/growth, then reuse the safe warm entry"
    );
    assert_eq!(cold, warm);
    assert_eq!(cold["data"][0]["roic"], 0.18);
    assert_eq!(cold["data"][0]["priceToEarningsRatio"], 20.0);
    assert_eq!(cold["warnings"], json!([]));
    for (surface, value) in [
        ("cold output", cold),
        ("warm output", warm),
        ("current cache", current),
    ] {
        let text = value.to_string();
        assert!(
            !text.contains(SENTINEL) && !text.contains("apikey"),
            "{surface} contains pre-sanitizer credentials"
        );
    }
}

// ── Screener fan-out contracts ────────────────────────────────────────────

/// Decode the filters JSON array from a recorded EODHD screener request path
/// (query string included).
fn decode_screener_filters(path: &str) -> Option<Vec<Value>> {
    let query = path.split('?').nth(1)?;
    let filters_parameter = query.split('&').find(|pair| pair.starts_with("filters="))?;
    let encoded = filters_parameter.strip_prefix("filters=")?;
    let decoded = encoded
        .replace("%22", "\"")
        .replace("%5B", "[")
        .replace("%5D", "]")
        .replace("%2C", ",")
        .replace("%3D", "=")
        .replace("%3A", ":")
        .replace("%3E", ">")
        .replace("%3C", "<")
        .replace("%20", " ");
    serde_json::from_str::<Value>(&decoded)
        .ok()?
        .as_array()
        .cloned()
}

/// expect: [P5] The screener provider paginates past the first 500-row page
/// to the offset cap instead of silently truncating at one page.
/// dcterms:identifier: providers::fetch_eodhd_screener
#[tokio::test]
async fn screener_provider_paginates_past_the_first_page() {
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            let offset = path
                .split('&')
                .find_map(|pair| pair.strip_prefix("offset="))
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(0);
            let count = if offset == 0 { 500 } else { 137 };
            return (
                200,
                json!({ "data": billion_cap_rows(count, offset as usize) }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let client = fixture_client();
            let rows = providers::fetch_eodhd_screener(&client, "fixture-eodhd", &[])
                .await
                .expect("screener rows");
            assert_eq!(rows.len(), 637);
            assert_eq!(fixture.count(), 2);
        })
        .await;
}

/// expect: [P5] A screen that reaches the 1,000-result offset cap re-queries
/// in market cap bands even when the filters already carry market-cap
/// bounds — the bands REPLACE the user's cap triples (clamped into the
/// user's range) instead of stacking four cap conditions, which live EODHD
/// mishandles.
/// dcterms:identifier: providers::fetch_eodhd_screener / fetch_screener_with_bands
#[tokio::test]
async fn screener_band_split_subdivides_user_cap_range() {
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            // Direct pages carry the user's exact bounds; band queries carry
            // the clamped band's own (different) bounds.
            let bounds = decode_screener_cap_bounds(path);
            let is_direct = bounds.contains(&(">=".to_string(), 2_000_000_000.0))
                && bounds.contains(&("<".to_string(), 200_000_000_000.0));
            if !is_direct {
                return (200, json!({ "data": [] }));
            }
            return (200, json!({ "data": billion_cap_rows(500, 0) }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let client = fixture_client();
            let filters = vec![
                json!(["market_capitalization", ">=", 2_000_000_000.0]),
                json!(["market_capitalization", "<", 200_000_000_000.0]),
            ];
            let rows = providers::fetch_eodhd_screener(&client, "fixture-eodhd", &filters)
                .await
                .expect("screener rows");
            // Band results replace the capped direct pass (band queries are
            // empty here), proving the split fired.
            assert_eq!(rows.len(), 0);
            // Two direct pages (1,000 rows → cap hit) + six clamped band
            // queries: [2e9,5e9), [5e9,1e10), [1e10,2.5e10), [2.5e10,5e10),
            // [5e10,1e11), [1e11,2e11).
            assert_eq!(fixture.count(), 8);
            // Every query carried exactly two cap triples. Direct pages carry
            // the user's exact bounds; band queries carry the clamped band's
            // own bounds, always inside the user's range.
            for request in fixture.requests() {
                let bounds = decode_screener_cap_bounds(&request);
                assert_eq!(
                    bounds.len(),
                    2,
                    "every query must carry exactly two cap triples: {bounds:?}"
                );
                let is_direct = bounds.contains(&(">=".to_string(), 2_000_000_000.0))
                    && bounds.contains(&("<".to_string(), 200_000_000_000.0));
                if is_direct {
                    continue;
                }
                for (operation, value) in &bounds {
                    match operation.as_str() {
                        ">=" => assert!(
                            *value >= 2_000_000_000.0,
                            "band lower bound {value} escaped the user range"
                        ),
                        "<" => assert!(
                            *value <= 200_000_000_000.0,
                            "band upper bound {value} escaped the user range"
                        ),
                        _ => {}
                    }
                }
            }
        })
        .await;
}

/// Decode the market_capitalization bounds from a recorded screener request.
fn decode_screener_cap_bounds(path: &str) -> Vec<(String, f64)> {
    decode_screener_filters(path)
        .unwrap_or_default()
        .iter()
        .filter_map(|filter| {
            let parts = filter.as_array()?;
            if parts.first()?.as_str()? != "market_capitalization" {
                return None;
            }
            Some((parts.get(1)?.as_str()?.to_string(), parts.get(2)?.as_f64()?))
        })
        .collect()
}

/// expect: [P5] A provider Ok-with-zero-rows result surfaces a warning
/// naming the symbol and pointing at resolve_symbol — live-observed
/// 2026-10-05: bare "ATD" returned data:[] with warnings:[], leaving
/// "unresolvable symbol" indistinguishable from "no metrics".
/// dcterms:identifier: CompaniesServer::key_metrics / silent-empty guard
#[tokio::test]
async fn key_metrics_empty_provider_result_warns_naming_symbol() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/fundamentals/EMPTY.PA") {
            return (
                200,
                json!({
                    "General": {"Code": "EMPTY.PA"},
                    "Highlights": {},
                    "Financials": {
                        "Income_Statement": {"yearly": {}},
                        "Balance_Sheet": {"yearly": {}}
                    }
                }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::SymbolLimitRequest>(json!({
                "symbol": "EMPTY.PA", "limit": null
            }))
            .expect("request");
            let output = content(
                &server
                    .key_metrics(Parameters(request))
                    .await
                    .expect("key metrics tool"),
            );
            assert_eq!(output["data"], json!([]));
            let warnings = output["warnings"].as_array().expect("warnings");
            assert!(
                warnings.iter().any(|warning| {
                    warning.as_str().is_some_and(|text| {
                        text.contains("EMPTY.PA") && text.contains("resolve_symbol")
                    })
                }),
                "the silent-empty guard must name the symbol and the fix: {warnings:?}"
            );
        })
        .await;
}

/// Run one scoped expectations_gap call over a request Value and return
/// its content — the scope + server + typed-parse + call choreography both
/// expectations-gap tests share, varying exactly the request body. The
/// fixture's recorded requests stay readable after the scope closes (the
/// scope only routes the call's HTTP).
async fn scoped_expectations_gap_output(
    directory: &std::path::Path,
    fixture: &FixtureHttp,
    request: Value,
) -> Value {
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory);
            content(
                &server
                    .expectations_gap(Parameters(
                        serde_json::from_value::<types::ExpectationsGapRequest>(request)
                            .expect("request"),
                    ))
                    .await
                    .expect("expectations gap tool"),
            )
        })
        .await
}

/// expect: [P5] An EODHD-routed expectations_gap call fetches the
/// fundamentals payload ONCE — EODHD's /fundamentals is one 10-API-call
/// endpoint carrying every statement, and the per-tool path fetched the
/// identical payload five times concurrently (50 units, five multi-MB
/// transfers; the load driver behind the 2026-10-05 batch stalls) — and
/// compact=true skips research entirely (no credentials needed) with
/// narrative_mode="compact".
/// dcterms:identifier: CompaniesServer::expectations_gap / consolidated EODHD acquisition + compact mode
#[tokio::test]
async fn expectations_gap_consolidates_eodhd_fundamentals_and_compact_skips_research() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/fundamentals/GAP.PA") {
            let mut value = eodhd_issuer_base("GAP", "Gap SA", "GAP.PA", "USD");
            // Live EODHD yearly entries carry currency_symbol per row —
            // statement_currency() reads it from the normalized rows.
            value["Financials"]["Income_Statement"]["yearly"]["2025-12-31"]["currency_symbol"] =
                json!("USD");
            value["Financials"]["Income_Statement"]["yearly"]["2024-12-31"]["currency_symbol"] =
                json!("USD");
            return (200, value);
        }
        if path.starts_with("/eodhd/real-time/GAP.PA") {
            return (200, json!({"code": "GAP.PA", "close": 50.0}));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    let output = scoped_expectations_gap_output(
        directory.path(),
        &fixture,
        json!({
            "symbol": "GAP.PA", "compact": true
        }),
    )
    .await;

    // ONE fundamentals request supplies all five statement views
    // (the per-tool path made five); one light /real-time quote.
    let fundamentals_calls = fixture
        .requests()
        .iter()
        .filter(|path| path.starts_with("/eodhd/fundamentals/"))
        .count();
    assert_eq!(
        fundamentals_calls, 1,
        "the consolidated EODHD path must fetch fundamentals once"
    );
    let quote_calls = fixture
        .requests()
        .iter()
        .filter(|path| path.starts_with("/eodhd/real-time/"))
        .count();
    assert_eq!(quote_calls, 1);

    // Compact mode: no research (the fixture server carries no
    // research credentials — a research call would have errored),
    // the mode is surfaced, and the gap math is intact.
    assert_eq!(output["narrative_mode"], json!("compact"));
    assert_eq!(output["context"]["guidance_samples"], json!(0));
    assert_eq!(output["data_quality"]["capability_available"], json!(true));
    assert_eq!(output["data_quality"]["total_research_claims"], json!(0));
    assert!(
        output["signal"].as_str().is_some(),
        "the consolidated views must feed the solve: {output}"
    );
}

/// expect: [P5] Data unavailability is a named-reason insufficient_data
/// report, never a tool error. A failed fundamentals fetch degrades
/// through the empty-payload path
/// (every normalizer's missing-data guard), research is skipped (no
/// dead-symbol spin), and data_quality.unavailable_reason names the
/// failed fetch.
/// dcterms:identifier: CompaniesServer::expectations_gap / data-unavailable degrade
#[tokio::test]
async fn expectations_gap_data_unavailable_is_named_reason_report_not_error() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/fundamentals/DEAD.PA") {
            return (500, json!({"error": "fixture fundamentals outage"}));
        }
        if path.starts_with("/eodhd/real-time/DEAD.PA") {
            return (200, json!({"code": "DEAD.PA", "close": 10.0}));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    let output = scoped_expectations_gap_output(
        directory.path(),
        &fixture,
        json!({
            "symbol": "DEAD.PA"
        }),
    )
    .await;
    assert_eq!(output["signal"], json!("insufficient_data"));
    assert_eq!(output["data_quality"]["capability_available"], json!(false));
    let reason = output["data_quality"]["unavailable_reason"]
        .as_str()
        .expect("unavailable_reason must name the failed fetch");
    assert!(
        reason.contains("EODHD fundamentals fetch failed"),
        "the reason names the failed fetch: {reason}"
    );
    assert_eq!(
        output["narrative_mode"],
        json!("skipped_unavailable_financials")
    );
    assert_eq!(output["context"]["guidance_samples"], json!(0));
}

/// DuPont fixture: three years (provider order, newest first) with constant
/// ratios (NPM 0.10, AT 0.5, EM 2.5, ROE 0.125) and dividends proportional
/// to net income (retention constant), so per-year medians preserve the
/// identity exactly. `dividend_ratio` varies to exercise the retention
/// clamp.
fn dupont_fixture(dividend_ratio: f64) -> Value {
    json!({
        "income": [
            {"calendarYear": "2025", "revenue": 1210.0, "netIncome": 121.0, "sellingGeneralAndAdministrativeExpenses": 121.0, "operatingIncome": 1089.0, "interestExpense": 60.5, "incomeTaxExpense": 24.2, "incomeBeforeTax": 121.0, "weightedAverageShsOut": 100.0},
            {"calendarYear": "2024", "revenue": 1100.0, "netIncome": 110.0, "sellingGeneralAndAdministrativeExpenses": 110.0, "operatingIncome": 990.0, "interestExpense": 55.0},
            {"calendarYear": "2023", "revenue": 1000.0, "netIncome": 100.0, "sellingGeneralAndAdministrativeExpenses": 100.0, "operatingIncome": 900.0, "interestExpense": 50.0},
        ],
        "balance": [
            {"calendarYear": "2025", "totalAssets": 2420.0, "totalStockholdersEquity": 968.0},
            {"calendarYear": "2024", "totalAssets": 2200.0, "totalStockholdersEquity": 880.0},
            {"calendarYear": "2023", "totalAssets": 2000.0, "totalStockholdersEquity": 800.0},
        ],
        "cash_flow": [
            {"calendarYear": "2025", "dividendsPaid": 121.0 * dividend_ratio},
            {"calendarYear": "2024", "dividendsPaid": 110.0 * dividend_ratio},
            {"calendarYear": "2023", "dividendsPaid": 100.0 * dividend_ratio},
        ],
    })
}

fn dupont_snapshot(fixture: &Value) -> financial_model::HistoricalSnapshot {
    financial_model::HistoricalSnapshot::from_api_json(
        fixture["income"].as_array().expect("income"),
        fixture["balance"].as_array().expect("balance"),
        fixture["cash_flow"].as_array().expect("cash flow"),
        &[],
        &json!({}),
    )
}

/// expect: [P5] DuPont capability uses average beginning/ending assets and
/// equity for each income period; retention derives from dividends and SGR is
/// the robust ROE estimate multiplied by retention.
/// pre: three consecutive annual balance sheets and matching income statements.
/// post: two return periods use average balance-sheet denominators.
/// [P1] Constraining: period-end buybacks must not silently inflate capability.
#[test]
fn dupont_uses_average_balance_sheet_denominators() {
    // 2024 average assets = 2100 and average equity = 840; 2025 uses 2310/924.
    // Both measured years therefore have AT 1100/2100, EM 2.5, ROE 110/840.
    let snapshot = dupont_snapshot(&dupont_fixture(0.4));
    let dupont = snapshot.dupont().expect("dupont");
    assert_eq!(dupont.years, 2);
    assert!((dupont.net_profit_margin - 0.10).abs() < 1e-12);
    assert!((dupont.asset_turnover - (1100.0 / 2100.0)).abs() < 1e-12);
    assert!((dupont.equity_multiplier - 2.50).abs() < 1e-12);
    assert!((dupont.roe - (110.0 / 840.0)).abs() < 1e-12);
    assert!((dupont.retention - 0.60).abs() < 1e-12);
    assert!((dupont.sustainable_growth_rate - (110.0 / 840.0 * 0.60)).abs() < 1e-12);
}

/// expect: [P5] Demonstrated revenue growth is the full-period CAGR over
/// positive reported annual revenue, not a financing-capacity proxy or a
/// fabricated default.
#[test]
fn demonstrated_revenue_growth_is_reported_cagr() {
    let snapshot = dupont_snapshot(&dupont_fixture(0.4));
    let growth = snapshot
        .demonstrated_revenue_cagr()
        .expect("three positive revenue observations");
    assert!((growth - 0.10).abs() < 1e-12);
}

/// expect: [P5] Dividends above earnings demonstrate zero retained funding,
/// never negative — the self-funding growth rate floors at zero.
#[test]
fn dupont_retention_clamps_when_dividends_exceed_earnings() {
    // dividends 200% of net income → retention 0 for every year.
    let snapshot = dupont_snapshot(&dupont_fixture(2.0));
    let dupont = snapshot.dupont().expect("dupont");
    assert_eq!(dupont.retention, 0.0);
    assert_eq!(dupont.sustainable_growth_rate, 0.0);
}

/// expect: [P5] The justified-P/B implied-ROE solve round-trips the identity
/// P/B = (ROE − g)/(COE − g), and refuses non-invertible regimes.
#[test]
fn implied_roe_round_trips_justified_price_to_book() {
    // (0.15 − 0.05)/(0.10 − 0.05) = 2.0 → price 200, book 100.
    let implied =
        financial_model::implied_roe_from_price_to_book(200.0, 100.0, 0.10, 0.05).expect("solve");
    assert!((implied - 0.15).abs() < 1e-12);
    // COE ≤ g is not invertible; price and book must be positive.
    assert!(financial_model::implied_roe_from_price_to_book(200.0, 100.0, 0.05, 0.05).is_none());
    assert!(financial_model::implied_roe_from_price_to_book(200.0, 0.0, 0.10, 0.05).is_none());
    assert!(financial_model::implied_roe_from_price_to_book(0.0, 100.0, 0.10, 0.05).is_none());
}

/// expect: [P5] The implied-NET-margin solve carries every modeled expense:
/// at a price generated from a known net margin, the solver recovers that net
/// margin exactly after SG&A, interest, D&A, and tax. Omitting any expense from
/// either side breaks the round trip. Unbracketed prices return None, never a
/// fabricated margin.
/// pre: history contains positive revenue and explicit SG&A and interest.
/// post: the inverse solve recovers the known net margin used by the forward DCF.
/// [P1] Constraining: reported profitability is the shareholder's net margin.
#[test]
fn implied_net_margin_solve_carries_all_modeled_expenses() {
    let snapshot = dupont_snapshot(&dupont_fixture(0.4));
    // Tax = 20%, SG&A/revenue = 10%, interest/revenue = 5%, D&A = 0%.
    assert!((snapshot.tax_rate - 0.2).abs() < 1e-12);
    assert!((snapshot.sga_to_revenue() - 0.10).abs() < 1e-12);
    let assumptions = financial_model::ProjectionAssumptions::from_history(&snapshot, 0.15)
        .expect("reported operating income reconciles");
    // Known net margin 8%: GM = NM/(1−tax) + SG&A% + interest% + D&A%
    // = 0.08/0.8 + 0.10 + 0.05 = 0.25.
    let intrinsic = financial_model::project_financial_model(
        &snapshot,
        &financial_model::ProjectionAssumptions {
            revenue_growth: 0.05,
            gross_margin: 0.25,
            ..assumptions.clone()
        },
    )
    .expect("canonical projection")
    .intrinsic_per_share;
    assert!(intrinsic.is_finite() && intrinsic > 0.0, "{intrinsic}");
    let solved =
        financial_model::implied_net_margin_at_growth(&snapshot, &assumptions, 0.05, intrinsic)
            .expect("net margin solve");
    assert!(
        (solved - 0.08).abs() < 1e-3,
        "solved {solved} — the forward/inverse expense bridge diverged"
    );
    assert!(
        financial_model::implied_net_margin_at_growth(
            &snapshot,
            &assumptions,
            0.05,
            intrinsic * 100.0
        )
        .is_none()
    );
}

fn hand_built_capability() -> financial_model::DuPontAnalysis {
    financial_model::DuPontAnalysis {
        net_profit_margin: 0.08,
        asset_turnover: 0.60,
        equity_multiplier: 2.20,
        roe: 0.1056,
        retention: 0.65,
        sustainable_growth_rate: 0.06864,
        years: 5,
    }
}

/// expect: [P1] A sustainable-growth estimate outside the reverse DCF's
/// validated growth domain is labeled model-sensitive, never presented as an
/// ordinary complete capability comparison.
/// pre: SGR is at or above the reverse DCF upper bound.
/// post: the report names the violated model domain in data quality.
#[test]
fn extreme_sustainable_growth_is_model_sensitive() {
    let solve = tools::expectations::ExpectationsSolve {
        capability: financial_model::DuPontAnalysis {
            sustainable_growth_rate: financial_model::IMPLIED_GROWTH_HI + 0.01,
            roe: 1.20,
            retention: 0.90,
            ..hand_built_capability()
        },
        headline: "net_margin",
        implied_growth: Some(0.05),
        implied_net_margin_at_demonstrated_growth: None,
        implied_roe: None,
        demonstrated_revenue_growth: Some(0.10),
        growth_gap_pp: Some(-5.0),
        financing_growth_gap_pp: Some(-96.0),
        profitability_gap_pp: None,
        growth_benchmark_unsolved_reason: None,
        book_value_per_share: None,
        sustainable_growth_rate: financial_model::IMPLIED_GROWTH_HI + 0.01,
        investor_target_return: 0.15,
        modified_wacc: 0.12,
        equity_weight: 0.75,
        debt_weight: 0.25,
    };
    let report = tools::expectations::build_gap_report(
        "EXTREME",
        &Some(solve),
        &[],
        0.05,
        &[],
        "excerpts",
        0,
        "fixture",
    );
    assert_eq!(
        report["data_quality"]["capability_status"],
        "model_sensitive"
    );
    assert!(
        report["data_quality"]["capability_flags"]
            .as_array()
            .is_some_and(|flags| flags
                .iter()
                .any(|flag| { flag == "sustainable_growth_outside_reverse_dcf_validated_range" }))
    );
}

/// expect: [P1] The primary growth gap compares like-for-like revenue growth:
/// price-implied growth minus demonstrated CAGR. Higgins SGR remains a separate
/// financing diagnostic; guidance remains context only (operator ruling
/// 2026-09-14).
#[test]
fn expectations_gap_separates_revenue_performance_from_financing_capacity() {
    // Non-financial: both legs solved, both demanding more than demonstrated.
    let non_financial = tools::expectations::ExpectationsSolve {
        capability: hand_built_capability(),
        headline: "net_margin",
        implied_growth: Some(0.12),
        implied_net_margin_at_demonstrated_growth: Some(0.10),
        implied_roe: None,
        demonstrated_revenue_growth: Some(0.08),
        growth_gap_pp: Some((0.12 - 0.08) * 100.0),
        financing_growth_gap_pp: Some((0.12 - 0.06864) * 100.0),
        profitability_gap_pp: Some(1.5),
        growth_benchmark_unsolved_reason: None,
        book_value_per_share: None,
        sustainable_growth_rate: 0.06864,
        investor_target_return: 0.15,
        modified_wacc: 0.12,
        equity_weight: 0.75,
        debt_weight: 0.25,
    };
    let report = tools::expectations::build_gap_report(
        "ACME",
        &Some(non_financial),
        &[0.03, 0.05, 0.07],
        0.05,
        &["guidance narrative".to_string()],
        "excerpts",
        9,
        "stock_quote",
    );
    assert_eq!(
        report["capability"]["headline_measure"],
        json!("net_margin")
    );
    assert_eq!(
        report["capability"]["sustainable_growth_rate"],
        json!(0.06864)
    );
    assert_eq!(report["gaps"]["demonstrated_revenue_growth"], json!(0.08));
    assert_eq!(report["gaps"]["sustainable_growth_rate"], json!(0.06864));
    assert!(
        (report["gaps"]["growth_gap_pp"]
            .as_f64()
            .expect("growth gap")
            - 4.0)
            .abs()
            < 1e-12
    );
    assert!(
        (report["gaps"]["financing_growth_gap_pp"]
            .as_f64()
            .expect("financing gap")
            - (0.12 - 0.06864) * 100.0)
            .abs()
            < 1e-12
    );
    assert_eq!(report["gaps"]["profitability_gap_pp"], json!(1.5));
    assert!(
        report["price_implied"]["implied_net_margin_at_demonstrated_growth"]["value"]
            .as_f64()
            .is_some()
    );
    assert!(
        !report["gaps"]
            .as_object()
            .expect("gaps")
            .contains_key("market_vs_management_pct"),
        "guidance must never be a gap axis"
    );
    assert!(
        !report["gaps"]
            .as_object()
            .expect("gaps")
            .contains_key("market_vs_user_pct")
    );
    // Guidance demoted to context.
    assert!(
        (report["context"]["management_guidance_median"]
            .as_f64()
            .expect("median")
            - 0.05)
            .abs()
            < 1e-12
    );
    assert_eq!(report["context"]["guidance_samples"], json!(3));
    assert_eq!(
        report["signal"],
        json!("price_demands_more_than_demonstrated")
    );

    // Financial: the ROE headline path.
    let financial = tools::expectations::ExpectationsSolve {
        capability: hand_built_capability(),
        headline: "roe",
        implied_growth: None,
        implied_net_margin_at_demonstrated_growth: None,
        implied_roe: Some(0.16),
        demonstrated_revenue_growth: None,
        growth_gap_pp: None,
        financing_growth_gap_pp: None,
        profitability_gap_pp: Some((0.16 - 0.1056) * 100.0),
        growth_benchmark_unsolved_reason: None,
        book_value_per_share: Some(242.0),
        sustainable_growth_rate: 0.06864,
        investor_target_return: 0.15,
        modified_wacc: 0.15,
        equity_weight: 1.0,
        debt_weight: 0.0,
    };
    let report = tools::expectations::build_gap_report(
        "BANK.OL",
        &Some(financial),
        &[],
        0.05,
        &[],
        "excerpts",
        0,
        "stock_quote",
    );
    assert_eq!(report["capability"]["headline_measure"], json!("roe"));
    assert!(
        report["price_implied"]["implied_roe"]["value"]
            .as_f64()
            .is_some()
    );
    assert!(report["price_implied"]["implied_growth"].is_null());
    assert_eq!(
        report["signal"],
        json!("price_demands_more_than_demonstrated")
    );

    // No solve: honest unavailability, both legs reported missing.
    let report = tools::expectations::build_gap_report(
        "BROKEN",
        &None,
        &[],
        0.05,
        &[],
        "excerpts",
        0,
        "unavailable",
    );
    assert_eq!(report["signal"], json!("insufficient_data"));
    assert_eq!(report["data_quality"]["capability_available"], json!(false));
    assert_eq!(report["data_quality"]["growth_leg_available"], json!(false));
    assert_eq!(
        report["data_quality"]["profitability_leg_available"],
        json!(false)
    );
}

/// expect: [P5] The signal classifier is sign-aware: same-direction gap
/// pairs get the direction-consistent verdict when at least one leg is
/// beyond its threshold, opposite-sign pairs keep "mixed", and same-sign
/// pairs within both thresholds read "aligned" — live-observed
/// 2026-10-05: the screen's value candidates (growth −1.8..−2.4pp within
/// the ±3.0 gate, margin −0.6..−1.0pp beyond the −0.5 gate) fell into the
/// old catch-all "mixed" arm with opposite-direction text, mislabeling
/// every both-legs-negative name the screen found.
#[test]
fn expectations_gap_signal_classifier_is_sign_aware() {
    let solve = |growth: f64, profitability: f64| tools::expectations::ExpectationsSolve {
        capability: hand_built_capability(),
        headline: "net_margin",
        implied_growth: Some(0.05),
        implied_net_margin_at_demonstrated_growth: Some(0.05),
        implied_roe: None,
        demonstrated_revenue_growth: Some(0.03),
        growth_gap_pp: Some(growth),
        financing_growth_gap_pp: Some(growth),
        profitability_gap_pp: Some(profitability),
        growth_benchmark_unsolved_reason: None,
        book_value_per_share: None,
        sustainable_growth_rate: 0.06864,
        investor_target_return: 0.15,
        modified_wacc: 0.12,
        equity_weight: 0.75,
        debt_weight: 0.25,
    };
    let signal = |growth: f64, profitability: f64| {
        let report = tools::expectations::build_gap_report(
            "ACME",
            &Some(solve(growth, profitability)),
            &[],
            0.05,
            &[],
            "excerpts",
            0,
            "stock_quote",
        );
        report["signal"].as_str().expect("signal").to_string()
    };
    // Both legs beyond, negative — the strong value-candidate verdict.
    assert_eq!(signal(-4.0, -1.5), "price_demands_less_than_demonstrated");
    // One leg beyond (margin), both negative — the live-observed shape.
    assert_eq!(signal(-2.1, -0.6), "price_demands_less_than_demonstrated");
    // One leg beyond (growth), both positive.
    assert_eq!(signal(3.5, 0.3), "price_demands_more_than_demonstrated");
    // Same sign, both within thresholds — envelope, not zero.
    assert_eq!(signal(-0.2, -0.3), "aligned");
    // Opposite signs — the genuine mixed case.
    assert_eq!(signal(4.0, -0.7), "mixed");
}

/// expect: [P5] Financial-sector profiles route to the equity-based
/// implied-ROE solve (justified P/B) — the FCF reverse DCF never runs for
/// banks, and book value derives from equity over shares without the
/// nominal share-count fallback.
#[test]
fn solve_expectations_financial_sector_uses_roe_path() {
    let fixture = dupont_fixture(0.4);
    let profile = providers::CompanyProfile::from_response(providers::ProviderResponse {
        value: json!([{
            "symbol": "BANK.OL",
            "sector": "Financial Services",
            "industry": "Banks - Regional",
        }]),
        provider: providers::Provider::Fmp,
        warnings: Vec::new(),
    });
    let solve = tools::expectations::solve_expectations(
        &fixture["income"],
        &fixture["balance"],
        &fixture["cash_flow"],
        &json!([]),
        &profile,
        20.0,
        0.15,
    )
    .expect("financial solve");
    assert_eq!(solve.headline, "roe");
    assert!(solve.implied_roe.is_some());
    assert!(solve.implied_growth.is_none());
    assert!(solve.growth_gap_pp.is_none());
    // Equity 968 / shares 100 → BVPS 9.68; P/B = 20/9.68. Demonstrated
    // SGR uses average equity: (110 / ((800 + 880) / 2)) × 60% retention.
    let implied_roe = solve.implied_roe.expect("roe");
    let sustainable_growth = (110.0 / 840.0) * 0.60;
    let expected = (20.0 / 9.68) * (0.15 - sustainable_growth) + sustainable_growth;
    assert!(
        (implied_roe - expected).abs() < 1e-12,
        "{implied_roe} vs {expected}"
    );
    assert_eq!(solve.book_value_per_share, Some(9.68));
}

/// AnalystPrep CFA worked example: NI 50,000, revenue 285,000, average assets
/// 1,000,000, and average equity 600,000 produce NPM 17.54%, turnover 0.285,
/// multiplier 1.667, and ROE 8.33%.
/// Source: https://analystprep.com/cfa-level-1-exam/financial-reporting-and-analysis/dupont-analysis-of-return-on-equity/
#[test]
fn dupont_reproduces_published_cfa_worked_example() {
    let snapshot = financial_model::HistoricalSnapshot::from_api_json(
        &[json!({"calendarYear":"2025","revenue":285000.0,"netIncome":50000.0})],
        &[
            json!({"calendarYear":"2025","totalAssets":1000000.0,"totalStockholdersEquity":600000.0}),
            json!({"calendarYear":"2024","totalAssets":1000000.0,"totalStockholdersEquity":600000.0}),
        ],
        &[json!({"calendarYear":"2025","dividendsPaid":0.0})],
        &[],
        &json!({}),
    );
    let result = snapshot.dupont().expect("published DuPont case");
    assert!((result.net_profit_margin - 0.1754385965).abs() < 1e-9);
    assert!((result.asset_turnover - 0.285).abs() < 1e-12);
    assert!((result.equity_multiplier - 1.6666666667).abs() < 1e-9);
    assert!((result.roe - 0.0833333333).abs() < 1e-9);
}

/// Wall Street Prep Higgins SGR worked example: NI $50m, dividends $25m,
/// average equity $200m => retention 50%, ROE 25%, SGR 12.5%.
/// Source: https://www.wallstreetprep.com/knowledge/sustainable-growth-rate/
#[test]
fn sustainable_growth_reproduces_published_worked_example() {
    let snapshot = financial_model::HistoricalSnapshot::from_api_json(
        &[json!({"calendarYear":"2025","revenue":100.0,"netIncome":50.0})],
        &[
            json!({"calendarYear":"2025","totalAssets":300.0,"totalStockholdersEquity":200.0}),
            json!({"calendarYear":"2024","totalAssets":300.0,"totalStockholdersEquity":200.0}),
        ],
        &[json!({"calendarYear":"2025","dividendsPaid":-25.0})],
        &[],
        &json!({}),
    );
    let result = snapshot.dupont().expect("published Higgins case");
    assert!((result.retention - 0.50).abs() < 1e-12);
    assert!((result.roe - 0.25).abs() < 1e-12);
    assert!((result.sustainable_growth_rate - 0.125).abs() < 1e-12);
}

// ── mcp-tool-review C-02: management_narrative head-excerpt default ─────

/// C-02 pin: a long narrative document is capped to a head excerpt with a
/// marker naming the opt-in; a short document passes through unchanged;
/// the cap is char-based (multi-byte safe — never slices a boundary).
#[test]
fn narrative_capped_to_head_excerpt_by_default() {
    let long: String = "x".repeat(1_000);
    let capped = tools::expectations::cap_narrative_doc(&long);
    let cap = tools::expectations::NARRATIVE_EXCERPT_CHARS;
    assert!(
        capped.starts_with(&"x".repeat(cap)),
        "the head excerpt is the document's first {cap} chars"
    );
    assert!(
        capped.contains("full_narrative: true"),
        "the marker names the opt-in: {capped}"
    );
    assert!(capped.chars().count() < cap + 100, "the cap actually caps");

    let short = "guidance narrative".to_string();
    assert_eq!(
        tools::expectations::cap_narrative_doc(&short),
        short,
        "short documents pass through unchanged"
    );

    let cjk: String = "漢".repeat(500);
    let capped_cjk = tools::expectations::cap_narrative_doc(&cjk);
    assert!(
        capped_cjk.starts_with(&"漢".repeat(cap)),
        "the char-based cap never slices a multi-byte boundary"
    );
}

/// C-02 pin: the report states which narrative mode it used — the consumer
/// distinguishes excerpts from full text without guessing, and the
/// narrative itself passes through `build_gap_report` unchanged (the
/// handler owns the capping; the median computation is untouched).
#[test]
fn narrative_mode_stated_in_report() {
    let report = tools::expectations::build_gap_report(
        "ACME",
        &None,
        &[0.03, 0.05],
        0.05,
        &["doc".to_string()],
        "excerpts",
        2,
        "fixture",
    );
    assert_eq!(report["narrative_mode"], json!("excerpts"));
    assert_eq!(report["management_narrative"][0], json!("doc"));
    assert_eq!(
        report["context"]["management_guidance_median"],
        json!(0.04),
        "the consumed quantity is unchanged by the narrative cap"
    );
}
