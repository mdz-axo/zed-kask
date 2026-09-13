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

pub(super) fn server(directory: &std::path::Path) -> CompaniesServer {
    CompaniesServer::new(
        hkask_types::WebID::new(),
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("HTTP client"),
        "fixture-fmp".into(),
        "fixture-eodhd".into(),
        None,
        None,
        None,
        None,
        ResearchStore::with_dir(directory.join("research")).expect("research fixture"),
        Arc::new(Mutex::new(LearningState::default())),
        superforecast::FermiDefaults::from_env(),
        Some(fibo_cache::FiboDataCache::open(&directory.join("cache.db")).expect("cache fixture")),
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
                "/fmp/income-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"revenue":revenue,"costOfRevenue":revenue*0.6,"grossProfit":revenue*0.4,"depreciationAndAmortization":revenue*0.03,"sellingGeneralAndAdministrativeExpenses":revenue*0.1375,"interestExpense":revenue*0.045,"incomeTaxExpense":revenue*0.0375,"incomeBeforeTax":revenue*0.1875,"netIncome":revenue*0.15,"ebitda":revenue*0.2625,"weightedAverageShsOutDil":100000000.0}),
                "/fmp/balance-sheet-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"totalCurrentAssets":revenue*0.3,"totalCurrentLiabilities":revenue*0.15,"cashAndCashEquivalents":revenue*0.05,"longTermDebt":revenue*0.2,"totalStockholdersEquity":revenue*0.75,"totalAssets":revenue*1.1,"totalLiabilities":revenue*0.35}),
                _ => json!({"date":format!("{year}-12-31"),"calendarYear":year,"capitalExpenditure":-revenue*0.04}),
            }
        }).collect();
        return (200, json!(rows));
    }
    fmp_fixture(path)
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

/// expect: [P5] The reverse DCF's current price falls back to the stock
/// quote's close when the profile carries no `price` field (every
/// EODHD-routed, exchange-qualified symbol — live-observed 2026-09-10:
/// `expectations_gap` and `reverse_dcf` returned no market-implied growth
/// for DNB.OL, PKN.WAR, NTDOY on exactly this shape) — the implied growth
/// resolves and the price source is surfaced. (Tested through `reverse_dcf`
/// because `expectations_gap` additionally requires research-search
/// credentials, which the fixture server does not carry.)
/// dcterms:identifier: CompaniesServer::reverse_dcf / forecast::resolve_current_price
#[tokio::test]
async fn reverse_dcf_price_falls_back_to_quote_close() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        let endpoint = path.split('?').next().expect("endpoint");
        if endpoint == "/fmp/profile" {
            // EODHD-routed profile shape: no `price` field.
            return (
                200,
                json!([{"companyName":"Acme","sector":"Technology","industry":"Software","marketCap":3000000000.0,"sharesOutstanding":100000000.0,"currency":"USD"}]),
            );
        }
        if endpoint == "/fmp/quote" {
            return (
                200,
                json!({"symbol":"ACME","price":30.0,"open":29.0,"high":31.0,"low":28.0}),
            );
        }
        if endpoint == "/fmp/income-statement" {
            let (status, mut rows) = financial_fixture(path);
            for row in rows.as_array_mut().expect("income rows") {
                row["reportedCurrency"] = json!("USD");
            }
            return (status, rows);
        }
        financial_fixture(path)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ReverseDcfRequest>(json!({
                "symbol": "ACME"
            }))
            .expect("request");
            let output = content(
                &server
                    .reverse_dcf(Parameters(request))
                    .await
                    .expect("reverse dcf tool"),
            );
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

/// expect: [P5] A quote in pence is converted through pounds into the USD
/// statement currency before reverse DCF; raw GBX is never treated as USD.
/// dcterms:identifier: CompaniesServer::reverse_dcf / CompaniesServer::normalize_price_for_financials
#[tokio::test]
async fn reverse_dcf_normalizes_gbx_quote_to_usd_statements() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        let endpoint = path.split('?').next().expect("endpoint");
        if endpoint == "/fmp/profile" {
            return (
                200,
                json!([{"companyName":"Acme","sector":"Technology","industry":"Software","marketCap":3000000000.0,"sharesOutstanding":100000000.0,"currency":"GBX"}]),
            );
        }
        if endpoint == "/fmp/quote" {
            return (
                200,
                json!({"symbol":"ACME","price":1603.0,"open":1600.0,"high":1620.0,"low":1580.0}),
            );
        }
        if endpoint.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (200, json!([{"date":"2026-09-11","close":0.7396}]));
        }
        if endpoint == "/fmp/income-statement" {
            let (status, mut rows) = financial_fixture(path);
            for row in rows.as_array_mut().expect("income rows") {
                row["reportedCurrency"] = json!("USD");
            }
            return (status, rows);
        }
        financial_fixture(path)
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ReverseDcfRequest>(json!({
                "symbol": "ACME"
            }))
            .expect("request");
            let output = content(
                &server
                    .reverse_dcf(Parameters(request))
                    .await
                    .expect("reverse dcf tool"),
            );
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
                "2025-12-31":{"totalRevenue":"1000000000.00","grossProfit":"400000000.00","costOfRevenue":"600000000.00","netIncome":"150000000.00"},
                "2024-12-31":{"totalRevenue":"900000000.00","grossProfit":"360000000.00","costOfRevenue":"540000000.00","netIncome":"135000000.00"}
            }},
            "Balance_Sheet":{"yearly":{
                "2025-12-31":{"totalAssets":"1200000000.00","totalStockholderEquity":"750000000.00","netInvestedCapital":"900000000.00","netDebt":"150000000.00","accountsPayable":"50000000.00","netReceivables":"100000000.00","inventory":"50000000.00","commonStockSharesOutstanding":"100000000.00"},
                "2024-12-31":{"totalAssets":"1080000000.00","totalStockholderEquity":"675000000.00","netInvestedCapital":"810000000.00","netDebt":"135000000.00"}
            }}
        }
    })
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
                assert_eq!(metrics.raw()[0]["roic"], 0.125);
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
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/fmp/income-statement") || path.starts_with("/eodhd/") {
            (503, json!({"error":"statements unavailable"}))
        } else {
            financial_fixture(path)
        }
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let output = comparable(
                &server(directory.path()),
                json!({"symbol":"ACME","peers":"PEER"}),
            )
            .await;
            assert_eq!(output["comparison"][1]["pe_ratio"], 20.0);
            assert!(output["dcf_overlay"]["error"].is_string());
            assert!(output["dcf_overlay"]["kind"].is_string());
        })
        .await;
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
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/fmp/key-metrics") && path.contains("limit=1") {
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
                json!({"symbol":"ACME","peers":"PEER"}),
            )
            .await;
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
        })
        .await;
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

/// Decode the `exchange` filter value from a recorded EODHD screener request
/// path, or None when the request carries no exchange filter.
fn decode_screener_exchange(path: &str) -> Option<String> {
    decode_screener_filters(path)?.iter().find_map(|filter| {
        let parts = filter.as_array()?;
        if parts.first()?.as_str()? == "exchange" {
            Some(parts.get(2)?.as_str()?.to_string())
        } else {
            None
        }
    })
}

/// expect: [P5] A multi-geography prompt fans out one EODHD screener query
/// per exchange, interleaves results round-robin, and reports per-exchange
/// match counts.
/// dcterms:identifier: CompaniesServer::company_screener / screener::parse_screening_prompt
#[tokio::test]
async fn screener_fans_out_per_exchange_and_interleaves() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            if let Some(exchange) = decode_screener_exchange(path) {
                let rows: Vec<Value> = (0..2)
                    .map(|index| {
                        json!({
                            "code": format!("{exchange}{index}"),
                            "name": format!("Company {index} of {exchange}"),
                            "exchange": exchange,
                            "market_capitalization": 10_000_000_000.0
                                - f64::from(index) * 1_000_000_000.0,
                        })
                    })
                    .collect();
                return (200, json!({ "data": rows }));
            }
            return (200, json!({ "data": [] }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK listed companies",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );

            let codes = output["parsed_criteria"]["exchanges"]
                .as_array()
                .expect("parsed exchanges");
            assert!(codes.iter().any(|code| code == "US"));
            assert!(codes.iter().any(|code| code == "LSE"));
            assert_eq!(output["exchange_match_counts"]["US"], json!(2));
            assert_eq!(output["exchange_match_counts"]["LSE"], json!(2));
            let results = output["results"].as_array().expect("results");
            assert_eq!(results.len(), 4);
            let exchanges: Vec<&str> = results
                .iter()
                .map(|row| row["exchange"].as_str().expect("exchange"))
                .collect();
            assert_eq!(exchanges, ["US", "LSE", "US", "LSE"]);
            let filters_text = output["screener_filters"].to_string();
            assert!(!filters_text.contains("exchange"));
            assert_eq!(fixture.count(), 2);
        })
        .await;
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
            let rows: Vec<Value> = (0..count)
                .map(|index| {
                    json!({
                        "code": format!("C{}", offset as usize + index),
                        "exchange": "US",
                        "market_capitalization": 1_000_000_000.0,
                    })
                })
                .collect();
            return (200, json!({ "data": rows }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("HTTP client");
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
            let rows: Vec<Value> = (0..500)
                .map(|index| {
                    json!({
                        "code": format!("C{index}"),
                        "exchange": "US",
                        "market_capitalization": 1_000_000_000.0,
                    })
                })
                .collect();
            return (200, json!({ "data": rows }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("HTTP client");
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

/// expect: [P9] A prompt that parses to zero criteria warns loudly instead
/// of silently returning the full universe as if it were a match.
#[tokio::test]
async fn screener_empty_parse_warns_and_lists_universe() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            return (
                200,
                json!({ "data": [{
                    "code": "UNIVERSE",
                    "name": "Universe Row",
                    "exchange": "US",
                    "market_capitalization": 5_000_000_000.0,
                }] }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "show me everything",
                "limit": 20
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert!(
                output["parsed_criteria"]
                    .as_object()
                    .expect("parsed criteria")
                    .is_empty()
            );
            assert!(
                output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .unwrap_or("")
                        .contains("No criteria parsed"))
            );
            assert_eq!(output["count"], json!(1));
            assert_eq!(output["exchange_match_counts"], json!({}));
        })
        .await;
}

/// expect: [P9] One failed exchange keeps the surviving exchanges and names
/// the failure; only a total failure propagates.
#[tokio::test]
async fn screener_partial_exchange_failure_is_surfaced() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("LSE") => (500, json!({ "error": "fixture outage" })),
                Some("US") => (
                    200,
                    json!({ "data": [{
                        "code": "USA1",
                        "name": "US Company",
                        "exchange": "US",
                        "market_capitalization": 5_000_000_000.0,
                    }] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK stocks",
                "limit": 20
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["count"], json!(1));
            assert_eq!(output["exchange_match_counts"]["US"], json!(1));
            assert!(output["exchange_errors"]["LSE"].is_string());
            assert!(
                output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .unwrap_or("")
                        .contains("dropped from results"))
            );
        })
        .await;
}

/// expect: [P9] Zero matches on every exchange warns — a wrong exchange code
/// must be visible, not silent.
#[tokio::test]
async fn screener_all_zero_matches_warns() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/screener") {
            return (200, json!({ "data": [] }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK stocks",
                "limit": 20
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["count"], json!(0));
            assert_eq!(output["total_matches"], json!(0));
            assert_eq!(output["exchange_match_counts"]["US"], json!(0));
            assert_eq!(output["exchange_match_counts"]["LSE"], json!(0));
            assert!(
                output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning.as_str().unwrap_or("").contains("zero matches"))
            );
        })
        .await;
}

/// expect: [P5] criteria_overrides merge over parsed criteria — overrides
/// suppress the empty-parse warning and drive the exchange fan-out.
#[tokio::test]
async fn screener_overrides_merge_over_parsed_criteria() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (200, json!([{"Code": "VN", "Currency": "VND"}]));
        }
        if path.starts_with("/eodhd/eod/USDVND.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 25000.0}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("VN") => (
                    200,
                    json!({ "data": [{
                        "code": "VNM1",
                        "name": "Vietnam Company",
                        "exchange": "VN",
                        "currency_symbol": "₫",
                        "market_capitalization": 50_000_000_000_000.0,
                    }] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "large companies",
                "limit": 20,
                "criteria_overrides": {
                    "exchanges": ["VN"],
                    "market_capitalization_min": 1000000000
                }
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["parsed_criteria"]["exchanges"], json!(["VN"]));
            assert_eq!(
                output["parsed_criteria"]["market_capitalization_min"],
                json!(1_000_000_000)
            );
            assert_eq!(output["count"], json!(1));
            assert!(
                !output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .unwrap_or("")
                        .contains("No criteria parsed"))
            );
            // The override bound reached the VN query unconverted — EODHD's
            // market_capitalization filter is USD-denominated — while the
            // VND rate annotates rows with market_capitalization_usd.
            let vn_bounds = fixture
                .requests()
                .iter()
                .find(|request| decode_screener_exchange(request).as_deref() == Some("VN"))
                .map(|request| decode_screener_cap_bounds(request))
                .unwrap_or_default();
            assert!(
                vn_bounds.contains(&(">=".to_string(), 1_000_000_000.0)),
                "VN lower bound unconverted: {vn_bounds:?}"
            );
            assert_eq!(output["fx"]["usd_rates"]["VND"], json!(25000.0));
            // exchanges-list + USDVND + the VN screener query
            assert_eq!(fixture.count(), 3);
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

/// expect: [P5] USD-stated market-cap bounds are sent UNCONVERTED on every
/// exchange — EODHD's screener market_capitalization filter compares
/// USD-denominated values while its returned field is listing-currency
/// (verified live 2026-09-10: a converted NOK bound behaved as a USD
/// threshold on Oslo) — while rows carry market_capitalization_usd, results
/// rank by USD cap, foreign lines whose home market is also screened are
/// dropped, and the band is enforced client-side.
/// dcterms:identifier: CompaniesServer::company_screener / ScreenerFx / screener_row_currency_pass
#[tokio::test]
async fn screener_sends_usd_bounds_unconverted_per_exchange() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "XETRA", "Currency": "EUR"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDEUR.FOREX") {
            // Out of chronological order — the max-by-date row must win.
            return (
                200,
                json!([
                    {"date": "2026-09-08", "close": 0.8},
                    {"date": "2026-09-07", "close": 0.79}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDJPY.FOREX") {
            // Needed for the XETRA ¥ line (a Japanese company — EODHD has no
            // Japanese exchange, so foreign ¥ lines are the surface).
            return (200, json!([{"date": "2026-09-08", "close": 150.0}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("US") => (
                    200,
                    json!({ "data": [
                        {"code": "US0", "name": "US Zero", "exchange": "US",
                         "currency_symbol": "$", "market_capitalization": 10_000_000_000.0},
                        {"code": "US1", "name": "US One", "exchange": "US",
                         "currency_symbol": "$", "market_capitalization": 9_000_000_000.0}
                    ] }),
                ),
                Some("XETRA") => (
                    200,
                    json!({ "data": [
                        {"code": "EUR1", "name": "Euro One", "exchange": "XETRA",
                         "currency_symbol": "€", "market_capitalization": 3_200_000_000.0},
                        {"code": "JPY1", "name": "Japan One", "exchange": "XETRA",
                         "currency_symbol": "¥", "market_capitalization": 450_000_000_000.0},
                        {"code": "USD1", "name": "US on XETRA", "exchange": "XETRA",
                         "currency_symbol": "$", "market_capitalization": 5_000_000_000.0},
                        {"code": "KR1", "name": "Nordic One", "exchange": "XETRA",
                         "currency_symbol": "kr", "market_capitalization": 10_000_000_000.0},
                        {"code": "EUR2", "name": "Euro Huge", "exchange": "XETRA",
                         "currency_symbol": "€", "market_capitalization": 900_000_000_000.0}
                    ] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and Germany listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );

            // Per-exchange queries carry the raw USD bounds: EODHD's filter
            // is USD-denominated, so XETRA receives the same 2e9/2e11 as US.
            let mut xetra_bounds = Vec::new();
            let mut us_bounds = Vec::new();
            for request_path in fixture.requests() {
                match decode_screener_exchange(&request_path).as_deref() {
                    Some("XETRA") => xetra_bounds = decode_screener_cap_bounds(&request_path),
                    Some("US") => us_bounds = decode_screener_cap_bounds(&request_path),
                    _ => {}
                }
            }
            assert!(
                xetra_bounds.contains(&(">=".to_string(), 2_000_000_000.0)),
                "XETRA lower bound unconverted: {xetra_bounds:?}"
            );
            assert!(
                xetra_bounds.contains(&("<".to_string(), 200_000_000_000.0)),
                "XETRA upper bound unconverted: {xetra_bounds:?}"
            );
            assert!(
                us_bounds.contains(&(">=".to_string(), 2_000_000_000.0)),
                "US lower bound unchanged: {us_bounds:?}"
            );
            assert!(
                us_bounds.contains(&("<".to_string(), 200_000_000_000.0)),
                "US upper bound unchanged: {us_bounds:?}"
            );

            // Row-currency rules, in order of USD cap: US rows (10e9, 9e9),
            // the € row (3.2e9/0.8 = 4e9), the ¥ row (4.5e11/150 = 3e9),
            // then the unconverted kr row last. The $-on-XETRA line is
            // dropped (USD is also screened), and the €9e11 row is out of
            // band.
            let results = output["results"].as_array().expect("results");
            assert_eq!(results.len(), 5);
            let codes: Vec<&str> = results
                .iter()
                .map(|row| row["code"].as_str().expect("code"))
                .collect();
            assert_eq!(codes, ["US0", "US1", "EUR1", "JPY1", "KR1"]);
            assert_eq!(
                results[0]["market_capitalization_usd"],
                json!(10_000_000_000.0)
            );
            assert_eq!(
                results[2]["market_capitalization_usd"],
                json!(4_000_000_000.0)
            );
            assert_eq!(
                results[3]["market_capitalization_usd"],
                json!(3_000_000_000.0)
            );
            assert!(results[4]["market_capitalization_usd"].is_null());

            // Counters surface every drop.
            assert_eq!(output["foreign_lines_dropped"], json!(1));
            assert_eq!(output["out_of_band_dropped"], json!(1));
            assert_eq!(output["unconverted_rows"], json!(1));
            assert_eq!(output["exchange_match_counts"]["US"], json!(2));
            assert_eq!(output["exchange_match_counts"]["XETRA"], json!(3));

            // FX transparency: rates (including the pass-2 JPY rate), as-of
            // date, and the currency map used.
            assert_eq!(output["fx"]["as_of"], json!("2026-09-08"));
            assert_eq!(output["fx"]["usd_rates"]["EUR"], json!(0.8));
            assert_eq!(output["fx"]["usd_rates"]["JPY"], json!(150.0));
            assert_eq!(
                output["fx"]["exchange_currencies"]["XETRA"],
                json!("EUR")
            );
            assert_eq!(output["fx"]["exchange_currencies"]["US"], json!("USD"));

            // exchanges-list + USDEUR + two screener queries + pass-2 USDJPY
            assert_eq!(fixture.count(), 5);
        })
        .await;
}

/// expect: [P5] A mixed-currency exchange (London's IOB hosts ¥ lines) is
/// queried UNBOUNDED — server-side bounds in the exchange's currency would
/// numerically exclude every foreign-currency row — and its rows are
/// selected by the row-currency rules and client-side band enforcement.
/// dcterms:identifier: CompaniesServer::company_screener / MIXED_CURRENCY_EXCHANGES
#[tokio::test]
async fn screener_mixed_currency_exchange_queries_unbounded() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "LSE", "Currency": "GBP"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 0.75}]));
        }
        if path.starts_with("/eodhd/eod/USDJPY.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 150.0}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("US") => (
                    200,
                    json!({ "data": [{
                        "code": "US0", "name": "US Zero", "exchange": "US",
                        "currency_symbol": "$", "market_capitalization": 8_000_000_000.0
                    }] }),
                ),
                Some("LSE") => (
                    200,
                    json!({ "data": [
                        {"code": "GBP1", "name": "UK One", "exchange": "LSE",
                         "currency_symbol": "£", "market_capitalization": 3_000_000_000.0},
                        {"code": "JPY1", "name": "Japan on London", "exchange": "LSE",
                         "currency_symbol": "¥", "market_capitalization": 450_000_000_000.0}
                    ] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );

            // The LSE query carries NO cap bounds (unbounded — a GBP
            // ceiling would exclude the ¥ line); the US query is bounded.
            let mut lse_bounds = Vec::new();
            let mut us_bounds = Vec::new();
            for request_path in fixture.requests() {
                match decode_screener_exchange(&request_path).as_deref() {
                    Some("LSE") => lse_bounds = decode_screener_cap_bounds(&request_path),
                    Some("US") => us_bounds = decode_screener_cap_bounds(&request_path),
                    _ => {}
                }
            }
            assert!(lse_bounds.is_empty(), "LSE must be queried unbounded: {lse_bounds:?}");
            assert!(us_bounds.contains(&(">=".to_string(), 2_000_000_000.0)));

            // Both the £ row (3e9/0.75 = $4B) and the ¥ row (4.5e11/150 =
            // $3B) survive — the Japan path — ranked below the US row.
            let results = output["results"].as_array().expect("results");
            assert_eq!(results.len(), 3);
            let codes: Vec<&str> = results
                .iter()
                .map(|row| row["code"].as_str().expect("code"))
                .collect();
            assert_eq!(codes, ["US0", "GBP1", "JPY1"]);
            assert_eq!(
                results[2]["market_capitalization_usd"],
                json!(3_000_000_000.0)
            );
            assert_eq!(output["fx"]["usd_rates"]["JPY"], json!(150.0));
            // exchanges-list + USDGBP + two screener queries + pass-2 USDJPY
            assert_eq!(fixture.count(), 5);
        })
        .await;
}

/// expect: [P5] The exchanges list and FOREX rates are cached — a second
/// screen the same day re-fetches only the screener queries (including the
/// pass-2 rate for a foreign-currency line).
#[tokio::test]
async fn screener_fx_context_is_cached() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "LSE", "Currency": "GBP"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 0.75}]));
        }
        if path.starts_with("/eodhd/eod/USDJPY.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 150.0}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("US") => (
                    200,
                    json!({ "data": [{
                        "code": "US0", "name": "US Zero", "exchange": "US",
                        "currency_symbol": "$", "market_capitalization": 10_000_000_000.0
                    }] }),
                ),
                Some("LSE") => (
                    200,
                    json!({ "data": [{
                        "code": "JPY1", "name": "Japan One", "exchange": "LSE",
                        "currency_symbol": "¥", "market_capitalization": 450_000_000_000.0
                    }] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let first = content(
                &server
                    .company_screener(Parameters(serde_json::from_value(json!({
                        "prompt": "US and UK listed companies with market capitalization between 2 billion and 200 billion",
                        "limit": 10
                    })).expect("request")))
                    .await
                    .expect("screener tool"),
            );
            // exchanges-list + USDGBP + two screener queries + pass-2 USDJPY
            assert_eq!(fixture.count(), 5);

            let second = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            // Only the two screener queries re-fired — the FX context came
            // from the fibo cache.
            assert_eq!(fixture.count(), 7);
            assert_eq!(second["count"], first["count"]);
            assert_eq!(second["fx"]["usd_rates"]["GBP"], json!(0.75));
            assert_eq!(second["fx"]["usd_rates"]["JPY"], json!(150.0));
        })
        .await;
}

/// expect: [P9] An exchange whose USD rate cannot be fetched is dropped and
/// named — never screened with wrong-band bounds.
#[tokio::test]
async fn screener_fx_failure_drops_exchange_loudly() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "LSE", "Currency": "GBP"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (500, json!({ "error": "fixture forex outage" }));
        }
        if path.starts_with("/eodhd/screener") {
            return (
                200,
                json!({ "data": [{
                    "code": "USA1",
                    "name": "US Company",
                    "exchange": "US",
                    "currency_symbol": "$",
                    "market_capitalization": 5_000_000_000.0,
                }] }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["count"], json!(1));
            assert!(
                output["exchange_errors"]["LSE"]
                    .as_str()
                    .expect("LSE error")
                    .contains("FX rate USDGBP unavailable")
            );
            assert!(
                output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .unwrap_or("")
                        .contains("dropped from results"))
            );
            // exchanges-list + failed USDGBP + the US screener query
            assert_eq!(fixture.count(), 3);
        })
        .await;
}

/// expect: [P9] A fan-out exchange code absent from the EODHD exchange list
/// is dropped and named — the runtime check on the geography table's codes.
#[tokio::test]
async fn screener_unknown_exchange_code_is_dropped() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (200, json!([{"Code": "US", "Currency": "USD"}]));
        }
        if path.starts_with("/eodhd/screener") {
            return (
                200,
                json!({ "data": [{
                    "code": "USA1",
                    "name": "US Company",
                    "exchange": "US",
                    "market_capitalization": 5_000_000_000.0,
                }] }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "large companies",
                "limit": 10,
                "criteria_overrides": {
                    "exchanges": ["US", "ZZ"],
                    "market_capitalization_min": 2000000000
                }
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["count"], json!(1));
            assert!(
                output["exchange_errors"]["ZZ"]
                    .as_str()
                    .expect("ZZ error")
                    .contains("no currency mapping")
            );
            // exchanges-list + the US screener query (no forex — USD is
            // skipped)
            assert_eq!(fixture.count(), 2);
        })
        .await;
}

/// expect: [P5] A foreign-currency line survives when its home bucket kept
/// zero rows — home coverage counts surviving rows, not screened exchanges
/// (live-observed 2026-09-10: BUD kept 0 rows, so Hungarian companies'
/// London Ft lines were dropped with nothing replacing them).
/// dcterms:identifier: CompaniesServer::screener_row_currency_pass
#[tokio::test]
async fn screener_foreign_line_kept_when_home_bucket_empty() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "LSE", "Currency": "GBP"},
                    {"Code": "BUD", "Currency": "HUF"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 0.75}]));
        }
        if path.starts_with("/eodhd/eod/USDHUF.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 312.92}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("US") => (
                    200,
                    json!({ "data": [{
                        "code": "US0", "name": "US Zero", "exchange": "US",
                        "currency_symbol": "$", "market_capitalization": 8_000_000_000.0
                    }] }),
                ),
                // The home bucket delivers nothing (EODHD's Budapest data is
                // cap-less — the converted bounds filter everything).
                Some("BUD") => (200, json!({ "data": [] })),
                // London is mixed-currency: queried unbounded, so the Ft line
                // arrives despite the GBP-converted bounds.
                Some("LSE") => (
                    200,
                    json!({ "data": [{
                        "code": "HUF1", "name": "Hungary One", "exchange": "LSE",
                        "currency_symbol": "Ft", "market_capitalization": 6_000_000_000_000.0
                    }] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US, UK and Hungary listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            // The Ft line survived the empty home bucket, converted at the
            // HUF rate (6e12 / 312.92 ≈ $19.2B — in band).
            assert_eq!(output["foreign_lines_dropped"], json!(0));
            assert_eq!(output["exchange_match_counts"]["LSE"], json!(1));
            assert_eq!(output["exchange_match_counts"]["BUD"], json!(0));
            let results = output["results"].as_array().expect("results");
            let codes: Vec<&str> = results
                .iter()
                .map(|row| row["code"].as_str().expect("code"))
                .collect();
            assert!(codes.contains(&"HUF1"), "the Ft line must survive: {codes:?}");
            let huf_row = results
                .iter()
                .find(|row| row["code"] == "HUF1")
                .expect("HUF row");
            assert_eq!(
                huf_row["market_capitalization_usd"],
                json!(6_000_000_000_000.0 / 312.92)
            );
            assert_eq!(output["fx"]["usd_rates"]["HUF"], json!(312.92));
        })
        .await;
}

/// expect: [P5] Non-common instruments (ETFs, preferreds, notes, CDRs) are
/// dropped client-side and counted — EODHD's screener has no type filter.
/// ADRs and European dual-class tickers are deliberately kept.
/// dcterms:identifier: CompaniesServer::screener_row_currency_pass / is_non_common_instrument
#[tokio::test]
async fn screener_non_common_instruments_dropped() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (200, json!([{ "Code": "US", "Currency": "USD" }]));
        }
        if path.starts_with("/eodhd/screener") {
            return (
                200,
                json!({ "data": [
                    {"code": "IWD", "name": "iShares Russell 1000 Value ETF",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 48_000_000_000.0},
                    {"code": "MFC-PK", "name": "Manulife Financial Corp Pref K",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 48_000_000_000.0},
                    {"code": "PFH", "name": "Prudential Financial Inc 4.125% Junior Subordinated Notes",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 45_000_000_000.0},
                    {"code": "NKE", "name": "NIKE CDR (CAD Hedged)",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 47_000_000_000.0},
                    {"code": "MAERSK-B", "name": "A.P. Møller - Mærsk A/S",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 48_000_000_000.0},
                    {"code": "SIEGY", "name": "Siemens AG ADR",
                     "exchange": "US", "currency_symbol": "$",
                     "market_capitalization": 46_000_000_000.0}
                ] }),
            );
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert_eq!(output["non_common_dropped"], json!(4));
            assert_eq!(output["count"], json!(2));
            let results = output["results"].as_array().expect("results");
            assert!(results.iter().all(|row| {
                row["market_capitalization_usd"] == row["market_capitalization"]
            }));
            let codes: Vec<&str> = results
                .iter()
                .map(|row| row["code"].as_str().expect("code"))
                .collect();
            // Dual-class and ADR survive; the four wrappers drop.
            assert!(codes.contains(&"MAERSK-B"), "dual-class kept: {codes:?}");
            assert!(codes.contains(&"SIEGY"), "ADR kept: {codes:?}");
            assert!(!codes.contains(&"IWD"));
            assert!(!codes.contains(&"MFC-PK"));
            assert!(!codes.contains(&"PFH"));
            assert!(!codes.contains(&"NKE"));
        })
        .await;
}

/// expect: [P9] A failed exchanges-list fetch degrades the whole conversion
/// loudly — bounds apply unconverted (warned), results still return, and
/// the round-robin interleave is preserved.
#[tokio::test]
async fn screener_fx_list_failure_degrades_loudly() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (500, json!({ "error": "fixture exchanges-list outage" }));
        }
        if path.starts_with("/eodhd/screener") {
            if let Some(exchange) = decode_screener_exchange(path) {
                let rows: Vec<Value> = (0..2)
                    .map(|index| {
                        json!({
                            "code": format!("{exchange}{index}"),
                            "name": format!("Company {index} of {exchange}"),
                            "exchange": exchange,
                            "market_capitalization": 10_000_000_000.0
                                - f64::from(index) * 1_000_000_000.0,
                        })
                    })
                    .collect();
                return (200, json!({ "data": rows }));
            }
            return (200, json!({ "data": [] }));
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US and UK listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            assert!(
                output["warnings"]
                    .as_array()
                    .expect("warnings")
                    .iter()
                    .any(|warning| warning
                        .as_str()
                        .unwrap_or("")
                        .contains("USD conversion unavailable"))
            );
            // Bounds passed through unconverted.
            let lse_bounds = fixture
                .requests()
                .iter()
                .find(|request| decode_screener_exchange(request).as_deref() == Some("LSE"))
                .map(|request| decode_screener_cap_bounds(request))
                .unwrap_or_default();
            assert!(
                lse_bounds.contains(&(">=".to_string(), 2_000_000_000.0)),
                "LSE lower bound unconverted in degraded mode: {lse_bounds:?}"
            );
            // Round-robin preserved (no USD ranking without conversion).
            let results = output["results"].as_array().expect("results");
            assert_eq!(results.len(), 4);
            let exchanges: Vec<&str> = results
                .iter()
                .map(|row| row["exchange"].as_str().expect("exchange"))
                .collect();
            assert_eq!(exchanges, ["US", "LSE", "US", "LSE"]);
            assert!(output["fx"].is_null());
            // failed exchanges-list + two screener queries
            assert_eq!(fixture.count(), 3);
        })
        .await;
}

/// expect: [P5] An ambiguous foreign symbol ("kr" = SEK/NOK/DKK) is dropped
/// when the screen covers a candidate home market (the company arrives via
/// its home exchange), while the same symbol on the home exchange itself is
/// a same-currency row converted at the exchange rate.
#[tokio::test]
async fn screener_ambiguous_kr_line_dropped_when_home_screened() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/exchanges-list") {
            return (
                200,
                json!([
                    {"Code": "US", "Currency": "USD"},
                    {"Code": "LSE", "Currency": "GBP"},
                    {"Code": "ST", "Currency": "SEK"}
                ]),
            );
        }
        if path.starts_with("/eodhd/eod/USDGBP.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 0.75}]));
        }
        if path.starts_with("/eodhd/eod/USDSEK.FOREX") {
            return (200, json!([{"date": "2026-09-08", "close": 10.0}]));
        }
        if path.starts_with("/eodhd/screener") {
            return match decode_screener_exchange(path).as_deref() {
                Some("US") => (
                    200,
                    json!({ "data": [{
                        "code": "US0", "name": "US Zero", "exchange": "US",
                        "currency_symbol": "$", "market_capitalization": 8_000_000_000.0
                    }] }),
                ),
                Some("LSE") => (
                    200,
                    json!({ "data": [{
                        "code": "KRLSE", "name": "Nordic on London", "exchange": "LSE",
                        "currency_symbol": "kr", "market_capitalization": 60_000_000_000.0
                    }] }),
                ),
                Some("ST") => (
                    200,
                    json!({ "data": [{
                        "code": "KRST", "name": "Nordic at Home", "exchange": "ST",
                        "currency_symbol": "kr", "market_capitalization": 60_000_000_000.0
                    }] }),
                ),
                _ => (200, json!({ "data": [] })),
            };
        }
        (404, json!({ "error": "unexpected endpoint", "path": path }))
    })
    .await;
    providers::TEST_HTTP_ORIGIN
        .scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let request = serde_json::from_value::<types::ScreenerRequest>(json!({
                "prompt": "US, UK and Sweden listed companies with market capitalization between 2 billion and 200 billion",
                "limit": 10
            }))
            .expect("request");
            let output = content(
                &server
                    .company_screener(Parameters(request))
                    .await
                    .expect("screener tool"),
            );
            // The London kr line is dropped (SEK is screened — the company
            // arrives via Stockholm); the Stockholm kr line is a
            // same-currency row (6e10 / 10 = $6B).
            assert_eq!(output["foreign_lines_dropped"], json!(1));
            assert_eq!(output["count"], json!(2));
            let codes: Vec<&str> = output["results"]
                .as_array()
                .expect("results")
                .iter()
                .map(|row| row["code"].as_str().expect("code"))
                .collect();
            assert_eq!(codes, ["US0", "KRST"]);
            assert_eq!(
                output["results"][1]["market_capitalization_usd"],
                json!(6_000_000_000.0)
            );
            assert_eq!(output["exchange_match_counts"]["LSE"], json!(0));
            assert_eq!(output["exchange_match_counts"]["ST"], json!(1));
        })
        .await;
}

// ── Expectations gap: DuPont capability axis (operator ruling 2026-09-10) ──

/// DuPont fixture: three years (provider order, newest first) with constant
/// ratios (NPM 0.10, AT 0.5, EM 2.5, ROE 0.125) and dividends proportional
/// to net income (retention constant), so per-year medians preserve the
/// identity exactly. `dividend_ratio` varies to exercise the retention
/// clamp.
fn dupont_fixture(dividend_ratio: f64) -> Value {
    json!({
        "income": [
            {"calendarYear": "2025", "revenue": 1210.0, "netIncome": 121.0, "interestExpense": 60.5, "incomeTaxExpense": 24.2, "incomeBeforeTax": 121.0, "weightedAverageShsOut": 100.0},
            {"calendarYear": "2024", "revenue": 1100.0, "netIncome": 110.0, "interestExpense": 55.0},
            {"calendarYear": "2023", "revenue": 1000.0, "netIncome": 100.0, "interestExpense": 50.0},
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

/// expect: [P5] DuPont capability: the identity NPM × AT × EM = ROE holds on
/// medians, retention derives from dividends, and SGR = ROE × retention.
#[test]
fn dupont_identity_and_sustainable_growth() {
    // dividends 40% of net income → retention 0.6; SGR = 0.125 × 0.6.
    let snapshot = dupont_snapshot(&dupont_fixture(0.4));
    let dupont = snapshot.dupont().expect("dupont");
    assert_eq!(dupont.years, 3);
    assert!((dupont.net_profit_margin - 0.10).abs() < 1e-12);
    assert!((dupont.asset_turnover - 0.50).abs() < 1e-12);
    assert!((dupont.equity_multiplier - 2.50).abs() < 1e-12);
    assert!((dupont.roe - 0.125).abs() < 1e-12);
    let product = dupont.net_profit_margin * dupont.asset_turnover * dupont.equity_multiplier;
    assert!((product - dupont.roe).abs() < 1e-12, "identity: {product}");
    assert!(
        (dupont.retention - 0.60).abs() < 1e-12,
        "retention {}",
        dupont.retention
    );
    assert!((dupont.sustainable_growth_rate - 0.075).abs() < 1e-12);
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

/// expect: [P5] The implied-NET-margin solve carries the interest burden: at
/// a price generated from a known net margin, the solver recovers that net
/// margin exactly — interest at the demonstrated interest-to-revenue level
/// and tax at the demonstrated rate included. A pre-interest solve would
/// return 0.04, not 0.08, so this test fails if interest or tax drops out
/// of the NM ↔ GM conversion (operator ruling 2026-09-10). Unbracketed
/// prices return None, never a fabricated margin.
#[test]
fn implied_net_margin_solve_carries_interest_burden() {
    let snapshot = dupont_snapshot(&dupont_fixture(0.4));
    // tax_rate = 24.2/121.0 = 0.2; latest interest/revenue = 60.5/1210 = 5%.
    assert!((snapshot.tax_rate - 0.2).abs() < 1e-12);
    let assumptions = financial_model::ProjectionAssumptions::from_history(&snapshot);
    // Known net margin 8%: through the identity GM = NM/(1−tax) + interest%
    // + D&A% (D&A 0 here) → GM = 0.08/0.8 + 0.05 = 0.15, and
    // NI/revenue = (0.15 − 0.05) × 0.8 = 0.08 exactly.
    let intrinsic = financial_model::project_model(
        &snapshot,
        &financial_model::ProjectionAssumptions {
            revenue_growth: 0.05,
            gross_margin: 0.15,
            ..assumptions
        },
        100.0,
    )
    .intrinsic_per_share;
    assert!(intrinsic.is_finite() && intrinsic > 0.0, "{intrinsic}");
    let solved =
        financial_model::implied_net_margin_at_growth(&snapshot, &assumptions, 0.05, intrinsic)
            .expect("net margin solve");
    assert!(
        (solved - 0.08).abs() < 1e-3,
        "solved {solved} — pre-interest would be 0.04"
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

/// expect: [P1] The gap axis is price-implied vs demonstrated DuPont
/// capability — the guidance-gap fields never reappear; guidance is a
/// context annotation (operator ruling 2026-09-10).
#[test]
fn expectations_gap_axis_is_capability_not_guidance() {
    // Non-financial: both legs solved, both demanding more than demonstrated.
    let non_financial = tools::expectations::ExpectationsSolve {
        capability: hand_built_capability(),
        headline: "net_margin",
        implied_growth: Some(0.12),
        implied_net_margin_at_sgr: Some(0.10),
        implied_roe: None,
        growth_gap_pp: Some((0.12 - 0.06864) * 100.0),
        profitability_gap_pp: Some(1.5),
        book_value_per_share: None,
        sustainable_growth_rate: 0.06864,
    };
    let report = tools::expectations::build_gap_report(
        "ACME",
        &Some(non_financial),
        &[0.03, 0.05, 0.07],
        0.05,
        &["guidance narrative".to_string()],
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
    // SGR is the anchor of the growth leg — surfaced beside the gap.
    assert_eq!(report["gaps"]["sustainable_growth_rate"], json!(0.06864));
    assert!(report["gaps"]["growth_gap_pp"].as_f64().is_some());
    assert_eq!(report["gaps"]["profitability_gap_pp"], json!(1.5));
    assert!(
        report["price_implied"]["implied_net_margin_at_sustainable_growth"]["value"]
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
        implied_net_margin_at_sgr: None,
        implied_roe: Some(0.16),
        growth_gap_pp: None,
        profitability_gap_pp: Some((0.16 - 0.1056) * 100.0),
        book_value_per_share: Some(242.0),
        sustainable_growth_rate: 0.06864,
    };
    let report = tools::expectations::build_gap_report(
        "BANK.OL",
        &Some(financial),
        &[],
        0.05,
        &[],
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
    let report =
        tools::expectations::build_gap_report("BROKEN", &None, &[], 0.05, &[], 0, "unavailable");
    assert_eq!(report["signal"], json!("insufficient_data"));
    assert_eq!(report["data_quality"]["capability_available"], json!(false));
    assert_eq!(report["data_quality"]["growth_leg_available"], json!(false));
    assert_eq!(
        report["data_quality"]["profitability_leg_available"],
        json!(false)
    );
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
    )
    .expect("financial solve");
    assert_eq!(solve.headline, "roe");
    assert!(solve.implied_roe.is_some());
    assert!(solve.implied_growth.is_none());
    assert!(solve.growth_gap_pp.is_none());
    // equity 968 / shares 100 → BVPS 9.68; P/B = 20/9.68
    let implied_roe = solve.implied_roe.expect("roe");
    let expected = (20.0 / 9.68) * (0.10 - 0.075) + 0.075;
    assert!(
        (implied_roe - expected).abs() < 1e-12,
        "{implied_roe} vs {expected}"
    );
    assert_eq!(solve.book_value_per_share, Some(9.68));
}
