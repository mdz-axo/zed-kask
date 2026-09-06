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
    pub(super) async fn start(response: impl Fn(&str) -> (u16, Value) + Send + Sync + 'static) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture");
        let origin = format!("http://{}", listener.local_addr().expect("fixture address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.expect("accept fixture request");
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let length = stream.read(&mut buffer).await.expect("read request");
                    if length == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..length]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).expect("HTTP request text");
                let path = request.split_whitespace().nth(1).expect("request path");
                recorded
                    .lock()
                    .expect("requests lock")
                    .push(path.to_string());
                let (status, body) = response(path);
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
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
        let rows: Vec<_> = [("2025", 1000000000.0), ("2024", 900000000.0), ("2023", 810000000.0)].into_iter().map(|(year, revenue)| {
            match endpoint {
                "/fmp/income-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"revenue":revenue,"costOfRevenue":revenue*0.6,"grossProfit":revenue*0.4,"depreciationAndAmortization":revenue*0.03,"incomeTaxExpense":revenue*0.074,"incomeBeforeTax":revenue*0.37,"weightedAverageShsOutDil":100000000.0}),
                "/fmp/balance-sheet-statement" => json!({"date":format!("{year}-12-31"),"calendarYear":year,"totalCurrentAssets":revenue*0.3,"totalCurrentLiabilities":revenue*0.15,"cashAndCashEquivalents":revenue*0.05,"longTermDebt":revenue*0.2,"totalStockholdersEquity":revenue*0.5}),
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

fn eodhd_fixture() -> Value {
    json!({
        "General":{"Code":"GLOBAL","Name":"Global","GicSector":"Technology","Industry":"Software"},
        "Highlights":{"MarketCapitalization":3000000000.0,"DividendYield":0.02,"EBITDA":300000000.0},
        "Financials":{
            "Income_Statement":{"yearly":{
                "2025-12-31":{"totalRevenue":1000000000.0,"grossProfit":400000000.0,"costOfRevenue":600000000.0,"netIncome":150000000.0},
                "2024-12-31":{"totalRevenue":900000000.0,"grossProfit":360000000.0,"costOfRevenue":540000000.0,"netIncome":135000000.0}
            }},
            "Balance_Sheet":{"yearly":{
                "2025-12-31":{"totalAssets":1200000000.0,"totalStockholderEquity":750000000.0,"netInvestedCapital":900000000.0,"netDebt":150000000.0,"accountsPayable":50000000.0,"netReceivables":100000000.0,"inventory":50000000.0,"commonStockSharesOutstanding":100000000.0},
                "2024-12-31":{"totalAssets":1080000000.0,"totalStockholderEquity":675000000.0,"netInvestedCapital":810000000.0,"netDebt":135000000.0}
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
            if path.starts_with("/eodhd/fundamentals/") { (200, eodhd_fixture()) }
            else { (500, json!({"error":"FMP must not supplement EODHD"})) }
        }).await;
        providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
            let first = server(directory.path());
            if typed_first { first.fetch_key_metrics("GLOBAL.LSE", 2).await.expect("typed metrics"); }
            let output = metrics_tool(&first, "GLOBAL.LSE", 2).await;
            assert_eq!(output["data"].as_array().expect("annual metrics").len(), 2);
            let metrics = first.fetch_key_metrics("GLOBAL.LSE", 2).await.expect("typed metrics");
            assert_eq!(metrics.pe_ratio(), Some(20.0));
            assert_eq!(metrics.price_to_book(), Some(4.0));
            assert_eq!(metrics.price_to_sales(), Some(3.0));
            assert_eq!(metrics.ev_to_ebitda(), Some(10.5));
            assert_eq!(metrics.raw()[0]["grossProfitMargin"], 0.4);
            assert_eq!(metrics.raw()[0]["roic"], 0.125);
            assert!((metrics.revenue_growth().expect("growth") - 1.0/9.0).abs() < 1e-12);
            assert_eq!(output["provider"], "EODHD");
            assert!(output["warnings"].to_string().contains("approximates"));
            drop(first);
            assert_eq!(metrics_tool(&server(directory.path()), "GLOBAL.LSE", 2).await, output);
            assert_eq!(fixture.count(), 1);
        }).await;
    }
}

/// expect: [P9] An absent EODHD net-debt input does not become a fabricated zero-debt multiple.
#[tokio::test]
async fn eodhd_missing_debt_leaves_ev_multiple_absent() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|_| {
        let mut value = eodhd_fixture();
        value["Financials"]["Balance_Sheet"]["yearly"]["2025-12-31"].as_object_mut().expect("balance").remove("netDebt");
        (200, value)
    }).await;
    providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
        let metrics = server(directory.path()).fetch_key_metrics("GLOBAL.LSE", 2).await.expect("metrics");
        assert_eq!(metrics.pe_ratio(), Some(20.0));
        assert_eq!(metrics.ev_to_ebitda(), None);
    }).await;
}

/// expect: [P5] Targets and peers reuse the same cached data despite later provider changes.
#[tokio::test]
async fn target_and_peers_share_cache_and_learning_policy() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/eodhd/") { (200, eodhd_fixture()) } else { financial_fixture(path) }
    }).await;
    providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
        let first = server(directory.path());
        let profile = first.fetch_profile("PEER").await.expect("prime peer profile");
        let metrics = first.fetch_key_metrics("PEER", 1).await.expect("prime peer metrics");
        // Force subsequent live acquisitions to EODHD: warmed FMP data must
        // remain the same for either role until its TTL expires.
        for symbol in ["PEER", "LEARNED"] {
            for _ in 0..5 { first.learning.lock().expect("learning").record(symbol, Provider::Fmp, Some(1)); }
        }
        let output = comparable(&first, json!({"symbol":"ACME","peers":"PEER,LEARNED"})).await;
        assert_eq!(output["comparison"][1]["price"], json!(profile.price()));
        assert_eq!(output["comparison"][1]["pe_ratio"], json!(metrics.pe_ratio()));
        assert_eq!(output["comparison"][1]["provenance"]["key_metrics"], "FMP");
        assert_eq!(output["comparison"][2]["provenance"]["key_metrics"], "EODHD");
        assert_eq!(output["comparison"][2]["pe_ratio"], 20.0);
        assert!(!fixture.requests.lock().expect("requests").iter().any(|request| request.starts_with("/fmp/") && request.contains("symbol=LEARNED")));
        let peer_requests = fixture.requests.lock().expect("requests").iter().filter(|request| request.contains("symbol=PEER&")).count();
        assert_eq!(peer_requests, 4, "peer profile+metrics must be fetched only while priming");
        let calls = fixture.count();
        drop(first);
        let reopened = server(directory.path());
        let warm = comparable(&reopened, json!({"symbol":"ACME","peers":"PEER,LEARNED"})).await;
        assert_eq!(warm, output);
        assert_eq!(fixture.count(), calls, "entire comparison should be warm after reopen");
    }).await;
}

/// expect: [P9] Failed peers remain identified; no invented provider or zero multiples.
#[tokio::test]
async fn failed_and_empty_peers_are_visible() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.contains("BROKEN") { (503, json!({"error":"fixture peer unavailable"})) }
        else if path.contains("EMPTY") { (200, json!([])) }
        else { financial_fixture(path) }
    }).await;
    providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
        let output = comparable(&server(directory.path()), json!({"symbol":"ACME","peers":"BROKEN,EMPTY,../bad"})).await;
        let rows = output["comparison"].as_array().expect("rows");
        assert_eq!(rows.len(), 4);
        for row in rows.iter().skip(1) {
            assert!(row["errors"].as_array().is_some_and(|errors| !errors.is_empty()), "{row}");
            assert!(row["price"].is_null());
            assert!(row["pe_ratio"].is_null());
        }
        assert_eq!(rows[1]["symbol"], "BROKEN");
        assert_eq!(rows[1]["provenance"], json!({}));
        assert!(!fixture.requests.lock().expect("requests").iter().any(|path| path.contains("bad")));
    }).await;
}

/// expect: [P9] Unsupported sectors and inadequate inputs fail explicitly in both DCF views.
#[tokio::test]
async fn dcf_guards_agree_between_tools() {
    for case in ["sector", "history", "price", "shares"] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(move |path| {
            let (status, mut data) = financial_fixture(path);
            if path.starts_with("/fmp/profile") {
                if case == "sector" { data[0]["sector"] = json!("Financial Services"); }
                if case == "price" { data[0].as_object_mut().expect("profile").remove("price"); }
                if case == "shares" { data[0].as_object_mut().expect("profile").remove("sharesOutstanding"); }
            }
            if path.starts_with("/fmp/income-statement") {
                if case == "history" { data.as_array_mut().expect("income").truncate(1); }
                if case == "shares" { for row in data.as_array_mut().expect("income") { row.as_object_mut().expect("income row").remove("weightedAverageShsOutDil"); } }
            }
            (status, data)
        }).await;
        providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
            let server = server(directory.path());
            let standalone = content(&server.dcf_valuation(Parameters(serde_json::from_value(json!({"symbol":"ACME"})).expect("request"))).await.expect("DCF tool"));
            let comparison = comparable(&server, json!({"symbol":"ACME","peers":"PEER"})).await;
            assert!(standalone["error"].is_string(), "{case}: {standalone}");
            assert_eq!(comparison["dcf_overlay"], standalone, "{case}");
            let expected = match case { "sector" => "financial-sector", "history" => "at least 2 years", "price" => "current price", _ => "shares outstanding" };
            assert!(standalone["error"].as_str().expect("error text").contains(expected));
        }).await;
    }
}

/// expect: [P9] Both DCF tools reject the same invalid assumptions with a typed error.
#[tokio::test]
async fn invalid_dcf_assumptions_agree_between_tools() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(financial_fixture).await;
    providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
        let server = server(directory.path());
        let request = json!({"symbol":"ACME","peers":"PEER","discount_rate":0.10,"terminal_growth":0.12});
        let standalone = server.dcf_valuation(Parameters(serde_json::from_value(request.clone()).expect("request"))).await.expect_err("invalid DCF");
        let comparison = server.comparable_analysis(Parameters(serde_json::from_value(request).expect("request"))).await.expect_err("invalid overlay");
        assert_eq!(standalone.kind, hkask_types::McpErrorKind::InvalidArgument);
        assert_eq!(comparison.kind, standalone.kind);
        assert_eq!(comparison.message, standalone.message);
    }).await;
}

/// expect: [P9] Fallback provenance identifies the provider that actually supplied cached metrics.
#[tokio::test]
async fn fallback_provenance_survives_reopen_in_both_directions() {
    for (symbol, expected_provider) in [("ACME.US", "EODHD"), ("GLOBAL.LSE", "FMP")] {
        let directory = tempfile::tempdir().expect("temporary directory");
        let fixture = FixtureHttp::start(move |path| {
            if expected_provider == "EODHD" {
                if path.starts_with("/fmp/") { (503, json!({"error":"FMP unavailable"})) }
                else { (200, eodhd_fixture()) }
            } else if path.starts_with("/eodhd/") { (503, json!({"error":"EODHD unavailable"})) }
            else { fmp_fixture(path) }
        }).await;
        providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
            let first = server(directory.path());
            let output = metrics_tool(&first, symbol, 2).await;
            assert_eq!(output["provider"], expected_provider);
            assert_eq!(output["data"][0]["priceToEarningsRatio"], 20.0);
            let calls = fixture.count();
            drop(first);
            assert_eq!(metrics_tool(&server(directory.path()), symbol, 2).await, output);
            assert_eq!(fixture.count(), calls);
            assert!(!fixture.requests.lock().expect("requests").iter().any(|request| request.contains(".US.US")));
        }).await;
    }
}

/// expect: [P9] Malformed supplement payloads are visible, not treated as complete enrichment.
#[tokio::test]
async fn malformed_supplements_are_visible() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let fixture = FixtureHttp::start(|path| {
        if path.starts_with("/fmp/ratios") || path.starts_with("/fmp/financial-growth") { (200, json!({"unexpected":"object"})) }
        else { fmp_fixture(path) }
    }).await;
    providers::TEST_HTTP_ORIGIN.scope(fixture.origin.clone(), async {
        let output = metrics_tool(&server(directory.path()), "ACME", 2).await;
        assert_eq!(output["warnings"].as_array().expect("warnings").len(), 2);
        assert!(output["warnings"].to_string().contains("expected an array"));
        assert!(output["data"][0]["priceToEarningsRatio"].is_null());
        assert!(output["data"][0]["revenueGrowth"].is_null());
    }).await;
}
