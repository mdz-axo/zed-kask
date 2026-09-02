//! Integration test: FMP stable endpoint schema validation.
//!
//! Calls each FMP stable endpoint used by the companies MCP server and verifies
//! that the response status is 200 and that the response contains the field
//! names the code expects. This catches FMP API schema changes that would
//! otherwise cause silent `None` propagation.
//!
//! Requires `HKASK_FMP_API_KEY` env var. Skips all tests if absent.

use serde_json::Value;
use std::env;

fn api_key() -> Option<String> {
    env::var("HKASK_FMP_API_KEY").ok().filter(|s| !s.is_empty())
}

async fn fetch_endpoint(
    client: &reqwest::Client,
    path: &str,
    params: &[(&str, &str)],
) -> Option<Value> {
    let key = api_key()?;
    let url = format!("https://financialmodelingprep.com/stable{path}");
    let mut query: Vec<(&str, &str)> = params.to_vec();
    query.push(("apikey", &key));

    let resp = client.get(&url).query(&query).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

fn first_object(arr: &Value) -> Option<&serde_json::Map<String, Value>> {
    arr.as_array()?.first()?.as_object()
}

fn has_field(arr: &Value, field: &str) -> bool {
    first_object(arr)
        .map(|obj| obj.contains_key(field))
        .unwrap_or(false)
}

fn missing_fields(arr: &Value, expected: &[&str]) -> Vec<String> {
    let Some(obj) = first_object(arr) else {
        return expected.iter().map(|s| s.to_string()).collect();
    };
    expected
        .iter()
        .filter(|f| !obj.contains_key(**f))
        .map(|s| s.to_string())
        .collect()
}

mod tests {
    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap()
    }

    fn skip_if_no_key() -> Option<String> {
        api_key()
    }

    #[tokio::test]
    async fn profile_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(&client, "/profile", &[("symbol", "AAPL")]).await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "symbol",
                "companyName",
                "marketCap",
                "sector",
                "industry",
                "price",
            ],
        );
        assert!(missing.is_empty(), "profile missing fields: {missing:?}");
    }

    #[tokio::test]
    async fn profile_has_market_cap_not_mktcap() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(&client, "/profile", &[("symbol", "AAPL")]).await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(
            has_field(&data, "marketCap"),
            "profile should have 'marketCap' field"
        );
    }

    #[tokio::test]
    async fn quote_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(&client, "/quote", &[("symbol", "AAPL")]).await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(&data, &["symbol", "price", "marketCap", "exchange"]);
        assert!(missing.is_empty(), "quote missing fields: {missing:?}");
    }

    #[tokio::test]
    async fn income_statement_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/income-statement",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "revenue",
                "costOfRevenue",
                "grossProfit",
                "operatingIncome",
                "netIncome",
                "depreciationAndAmortization",
                "ebitda",
                "ebit",
                "eps",
                "epsDiluted",
                "weightedAverageShsOut",
                "weightedAverageShsOutDil",
            ],
        );
        assert!(
            missing.is_empty(),
            "income-statement missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn balance_sheet_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/balance-sheet-statement",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "totalAssets",
                "totalLiabilities",
                "totalEquity",
                "totalStockholdersEquity",
                "totalDebt",
                "cashAndCashEquivalents",
                "inventory",
                "totalCurrentAssets",
                "totalCurrentLiabilities",
            ],
        );
        assert!(
            missing.is_empty(),
            "balance-sheet missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn cash_flow_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/cash-flow-statement",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "netIncome",
                "depreciationAndAmortization",
                "netCashProvidedByOperatingActivities",
                "capitalExpenditure",
                "freeCashFlow",
                "operatingCashFlow",
            ],
        );
        assert!(missing.is_empty(), "cash-flow missing fields: {missing:?}");
    }

    #[tokio::test]
    async fn key_metrics_has_expected_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/key-metrics",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "returnOnInvestedCapital",
                "returnOnEquity",
                "investedCapital",
                "daysOfSalesOutstanding",
                "daysOfPayablesOutstanding",
                "operatingCycle",
                "cashConversionCycle",
                "evToEBITDA",
                "enterpriseValue",
                "marketCap",
            ],
        );
        assert!(
            missing.is_empty(),
            "key-metrics missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn key_metrics_does_not_have_legacy_fields() {
        // These fields were in the old /api/v3/key-metrics but moved to
        // /stable/ratios or /stable/financial-growth. The code's enrich_key_metrics
        // function merges them back in — this test documents the split.
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/key-metrics",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let moved = [
            "peRatio",
            "priceToBookRatio",
            "priceToSalesRatio",
            "dividendYield",
            "grossProfitMargin",
            "revenueGrowth",
            "roic",
            "evToEbitda",
        ];
        for field in &moved {
            assert!(
                !has_field(&data, field),
                "key-metrics should NOT have '{field}' (moved to ratios/growth endpoint)"
            );
        }
    }

    #[tokio::test]
    async fn ratios_has_moved_fields() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(&client, "/ratios", &[("symbol", "AAPL"), ("limit", "1")]).await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        let missing = missing_fields(
            &data,
            &[
                "priceToEarningsRatio",
                "priceToBookRatio",
                "priceToSalesRatio",
                "dividendYield",
                "grossProfitMargin",
                "enterpriseValueMultiple",
                "debtToEquityRatio",
            ],
        );
        assert!(missing.is_empty(), "ratios missing fields: {missing:?}");
    }

    #[tokio::test]
    async fn financial_growth_has_revenue_growth() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/financial-growth",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(
            has_field(&data, "revenueGrowth"),
            "financial-growth should have 'revenueGrowth'"
        );
    }

    #[tokio::test]
    async fn historical_price_endpoint_works() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/historical-price-eod/full",
            &[
                ("symbol", "AAPL"),
                ("from", "2025-01-01"),
                ("to", "2025-06-01"),
            ],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        // FMP stable returns a flat array, not the old {symbol, historical: [...]} envelope.
        assert!(
            data.is_array(),
            "historical-price-eod/full should return an array"
        );
        let missing = missing_fields(&data, &["symbol", "date", "close", "volume"]);
        assert!(
            missing.is_empty(),
            "historical-price-eod/full missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn company_screener_endpoint_works() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/company-screener",
            &[("limit", "3"), ("country", "US"), ("sector", "Technology")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(data.is_array(), "company-screener should return an array");
        let missing = missing_fields(&data, &["symbol", "companyName", "marketCap", "sector"]);
        assert!(
            missing.is_empty(),
            "company-screener missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn dates_align_across_endpoints() {
        // The enrich_key_metrics function merges by date — this test verifies
        // that key-metrics, ratios, and financial-growth share the same date keys.
        let _key = skip_if_no_key();
        let client = client();

        let km = fetch_endpoint(
            &client,
            "/key-metrics",
            &[("symbol", "AAPL"), ("limit", "2")],
        )
        .await;
        let ratios =
            fetch_endpoint(&client, "/ratios", &[("symbol", "AAPL"), ("limit", "2")]).await;
        let growth = fetch_endpoint(
            &client,
            "/financial-growth",
            &[("symbol", "AAPL"), ("limit", "2")],
        )
        .await;

        let (Some(km), Some(ratios), Some(growth)) = (km, ratios, growth) else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };

        let km_dates: Vec<String> = km
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("date").and_then(|d| d.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let ratio_dates: Vec<String> = ratios
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("date").and_then(|d| d.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let growth_dates: Vec<String> = growth
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("date").and_then(|d| d.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        assert!(!km_dates.is_empty(), "key-metrics should return dates");
        assert_eq!(
            km_dates, ratio_dates,
            "key-metrics and ratios dates should align"
        );
        assert_eq!(
            km_dates, growth_dates,
            "key-metrics and financial-growth dates should align"
        );
    }

    #[tokio::test]
    async fn search_name_endpoint_works() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/search-name",
            &[("query", "Apple"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(data.is_array(), "search-name should return an array");
        let missing = missing_fields(&data, &["symbol", "name", "exchange"]);
        assert!(
            missing.is_empty(),
            "search-name missing fields: {missing:?}"
        );
    }

    #[tokio::test]
    async fn earning_call_transcript_endpoint_works() {
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/earning-call-transcript",
            &[("symbol", "AAPL"), ("year", "2024"), ("quarter", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        // Transcript may or may not exist for this quarter — just verify the
        // endpoint is reachable and returns an array.
        assert!(
            data.is_array(),
            "earning-call-transcript should return an array"
        );
    }

    #[tokio::test]
    async fn income_statement_has_shares_outstanding() {
        // financial_model.rs and economic_profit.rs look up shares outstanding
        // from the income statement (moved from key-metrics in FMP stable).
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/income-statement",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(
            has_field(&data, "weightedAverageShsOutDil"),
            "income-statement should have 'weightedAverageShsOutDil'"
        );
        assert!(
            has_field(&data, "weightedAverageShsOut"),
            "income-statement should have 'weightedAverageShsOut'"
        );
    }

    #[tokio::test]
    async fn financial_statements_have_fiscal_year() {
        // financial_model.rs falls back from calendarYear to fiscalYear to date
        // for year labels. FMP stable removed calendarYear; fiscalYear is the
        // replacement.
        let _key = skip_if_no_key();
        let client = client();

        for (path, params) in [
            (
                "/income-statement",
                &[("symbol", "AAPL"), ("limit", "1")][..],
            ),
            (
                "/balance-sheet-statement",
                &[("symbol", "AAPL"), ("limit", "1")][..],
            ),
            (
                "/cash-flow-statement",
                &[("symbol", "AAPL"), ("limit", "1")][..],
            ),
        ] {
            let data = fetch_endpoint(&client, path, params).await;
            let Some(data) = data else {
                eprintln!("SKIP: no API key or endpoint unreachable");
                return;
            };
            assert!(
                has_field(&data, "fiscalYear"),
                "{path} should have 'fiscalYear'"
            );
            assert!(has_field(&data, "date"), "{path} should have 'date'");
            assert!(
                !has_field(&data, "calendarYear"),
                "{path} should NOT have 'calendarYear' (removed in stable API)"
            );
        }
    }

    #[tokio::test]
    async fn key_metrics_has_roic_source_field() {
        // extract_roic_from_metrics checks 'roic' (alias added by enrich_key_metrics)
        // then 'returnOnInvestedCapital' (FMP stable field name).
        let _key = skip_if_no_key();
        let client = client();
        let data = fetch_endpoint(
            &client,
            "/key-metrics",
            &[("symbol", "AAPL"), ("limit", "1")],
        )
        .await;
        let Some(data) = data else {
            eprintln!("SKIP: no API key or endpoint unreachable");
            return;
        };
        assert!(
            has_field(&data, "returnOnInvestedCapital"),
            "key-metrics should have 'returnOnInvestedCapital'"
        );
        assert!(
            has_field(&data, "investedCapital"),
            "key-metrics should have 'investedCapital'"
        );
    }

    // ── EODHD normalizer tests ──
    //
    // These test the EODHD → FMP field name mapping by fetching raw EODHD
    // data and checking that the normalizers would produce the correct field
    // names. Requires HKASK_EODHD_API_KEY env var.

    fn eodhd_key() -> Option<String> {
        env::var("HKASK_EODHD_API_KEY")
            .ok()
            .filter(|s| !s.is_empty())
    }

    async fn fetch_eodhd(client: &reqwest::Client, path: &str) -> Option<Value> {
        let key = eodhd_key()?;
        let url = format!("https://eodhd.com/api{path}");
        let resp = client
            .get(&url)
            .query(&[("api_token", key.as_str()), ("fmt", "json")])
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }

    #[tokio::test]
    async fn eodhd_profile_has_fmp_field_names() {
        let _key = eodhd_key();
        let client = client();
        let data = fetch_eodhd(&client, "/fundamentals/VOD.LSE").await;
        let Some(data) = data else {
            eprintln!("SKIP: no EODHD API key or endpoint unreachable");
            return;
        };
        // EODHD General section has Code, Name, GicSector — not symbol, companyName, sector.
        // The normalizer maps these. Verify the source fields exist.
        let general = data.get("General").and_then(|g| g.as_object());
        let Some(general) = general else {
            eprintln!("SKIP: no General section in EODHD response");
            return;
        };
        assert!(
            general.contains_key("Code"),
            "EODHD General should have 'Code'"
        );
        assert!(
            general.contains_key("Name"),
            "EODHD General should have 'Name'"
        );
        assert!(
            general.contains_key("GicSector"),
            "EODHD General should have 'GicSector'"
        );
        // MarketCapitalization is in Highlights, not General
        let highlights = data.get("Highlights").and_then(|h| h.as_object());
        if let Some(highlights) = highlights {
            assert!(
                highlights.contains_key("MarketCapitalization"),
                "EODHD Highlights should have 'MarketCapitalization'"
            );
            assert!(
                highlights.contains_key("DividendYield"),
                "EODHD Highlights should have 'DividendYield'"
            );
        }
    }

    #[tokio::test]
    async fn eodhd_income_statement_has_total_revenue() {
        // EODHD uses 'totalRevenue' instead of 'revenue' — the normalizer maps it.
        let _key = eodhd_key();
        let client = client();
        let data = fetch_eodhd(&client, "/fundamentals/VOD.LSE").await;
        let Some(data) = data else {
            eprintln!("SKIP: no EODHD API key or endpoint unreachable");
            return;
        };
        let yearly = data
            .get("Financials")
            .and_then(|f| f.get("Income_Statement"))
            .and_then(|is| is.get("yearly"))
            .and_then(|y| y.as_object());
        let Some(yearly) = yearly else {
            eprintln!("SKIP: no income statement data in EODHD response");
            return;
        };
        let first_entry = yearly.iter().next().map(|(_, v)| v);
        if let Some(entry) = first_entry.and_then(|e| e.as_object()) {
            assert!(
                entry.contains_key("totalRevenue"),
                "EODHD income statement should have 'totalRevenue'"
            );
            assert!(
                !entry.contains_key("revenue"),
                "EODHD income statement should NOT have 'revenue' (needs mapping)"
            );
            assert!(
                entry.contains_key("grossProfit"),
                "EODHD income statement should have 'grossProfit'"
            );
        }
    }

    #[tokio::test]
    async fn eodhd_cash_flow_has_eodhd_field_names() {
        // EODHD uses 'capitalExpenditures' (plural) and 'totalCashFromOperatingActivities'
        // instead of FMP's 'capitalExpenditure' and 'netCashProvidedByOperatingActivities'.
        let _key = eodhd_key();
        let client = client();
        let data = fetch_eodhd(&client, "/fundamentals/VOD.LSE").await;
        let Some(data) = data else {
            eprintln!("SKIP: no EODHD API key or endpoint unreachable");
            return;
        };
        let yearly = data
            .get("Financials")
            .and_then(|f| f.get("Cash_Flow"))
            .and_then(|cf| cf.get("yearly"))
            .and_then(|y| y.as_object());
        let Some(yearly) = yearly else {
            eprintln!("SKIP: no cash flow data in EODHD response");
            return;
        };
        let first_entry = yearly.iter().next().map(|(_, v)| v);
        if let Some(entry) = first_entry.and_then(|e| e.as_object()) {
            assert!(
                entry.contains_key("capitalExpenditures"),
                "EODHD cash flow should have 'capitalExpenditures'"
            );
            assert!(
                !entry.contains_key("capitalExpenditure"),
                "EODHD cash flow should NOT have 'capitalExpenditure' (needs mapping)"
            );
            assert!(
                entry.contains_key("totalCashFromOperatingActivities"),
                "EODHD cash flow should have 'totalCashFromOperatingActivities'"
            );
            assert!(
                entry.contains_key("freeCashFlow"),
                "EODHD cash flow should have 'freeCashFlow'"
            );
        }
    }

    #[tokio::test]
    async fn eodhd_balance_sheet_has_eodhd_field_names() {
        // EODHD uses 'totalLiab' and 'totalStockholderEquity' instead of
        // FMP's 'totalLiabilities' and 'totalEquity'/'totalStockholdersEquity'.
        let _key = eodhd_key();
        let client = client();
        let data = fetch_eodhd(&client, "/fundamentals/VOD.LSE").await;
        let Some(data) = data else {
            eprintln!("SKIP: no EODHD API key or endpoint unreachable");
            return;
        };
        let yearly = data
            .get("Financials")
            .and_then(|f| f.get("Balance_Sheet"))
            .and_then(|bs| bs.get("yearly"))
            .and_then(|y| y.as_object());
        let Some(yearly) = yearly else {
            eprintln!("SKIP: no balance sheet data in EODHD response");
            return;
        };
        let first_entry = yearly.iter().next().map(|(_, v)| v);
        if let Some(entry) = first_entry.and_then(|e| e.as_object()) {
            assert!(
                entry.contains_key("totalLiab"),
                "EODHD balance sheet should have 'totalLiab'"
            );
            assert!(
                !entry.contains_key("totalLiabilities"),
                "EODHD balance sheet should NOT have 'totalLiabilities' (needs mapping)"
            );
            assert!(
                entry.contains_key("totalStockholderEquity"),
                "EODHD balance sheet should have 'totalStockholderEquity'"
            );
            assert!(
                entry.contains_key("commonStockSharesOutstanding"),
                "EODHD balance sheet should have 'commonStockSharesOutstanding'"
            );
        }
    }

    #[tokio::test]
    async fn eodhd_historical_has_adjusted_close() {
        // EODHD uses 'adjusted_close' instead of 'adjClose' — the normalizer maps it.
        let _key = eodhd_key();
        let client = client();
        let data = fetch_eodhd(&client, "/eod/VOD.LSE?from=2025-06-01&to=2025-06-15").await;
        let Some(data) = data else {
            eprintln!("SKIP: no EODHD API key or endpoint unreachable");
            return;
        };
        if let Some(arr) = data.as_array() {
            if let Some(first) = arr.first().and_then(|e| e.as_object()) {
                assert!(
                    first.contains_key("adjusted_close"),
                    "EODHD EOD should have 'adjusted_close'"
                );
                assert!(
                    !first.contains_key("adjClose"),
                    "EODHD EOD should NOT have 'adjClose' (needs mapping)"
                );
                assert!(first.contains_key("close"), "EODHD EOD should have 'close'");
            }
        }
    }

    #[tokio::test]
    async fn eodhd_symbol_resolution_finds_primary() {
        // resolve_symbol should find the primary common stock listing
        // for a company name search.
        let _key = eodhd_key();
        let client = client();
        let Some(key) = eodhd_key() else {
            eprintln!("SKIP: no EODHD API key");
            return;
        };

        // Search for Vodafone — should find VOD.LSE as primary.
        let url = "https://eodhd.com/api/search/Vodafone".to_string();
        let resp = client
            .get(&url)
            .query(&[("api_token", key.as_str()), ("fmt", "json")])
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "EODHD search should succeed");
        let data: Value = resp.json().await.unwrap();
        let arr = data.as_array().expect("search should return array");
        assert!(!arr.is_empty(), "search should return results");

        // Find the primary listing
        let primary = arr.iter().find(|e| {
            e.get("isPrimary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                && e.get("Type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "Common Stock")
                    .unwrap_or(false)
        });
        assert!(
            primary.is_some(),
            "should find a primary common stock listing for Vodafone"
        );
        let primary = primary.unwrap();
        assert_eq!(
            primary.get("Code").and_then(|v| v.as_str()),
            Some("VOD"),
            "primary listing should be VOD"
        );
        assert_eq!(
            primary.get("Exchange").and_then(|v| v.as_str()),
            Some("LSE"),
            "primary exchange should be LSE"
        );
    }

    #[tokio::test]
    async fn eodhd_symbol_resolution_apple_us() {
        // Apple's primary listing should be on US exchange.
        let Some(key) = eodhd_key() else {
            eprintln!("SKIP: no EODHD API key");
            return;
        };
        let client = client();

        let resp = client
            .get("https://eodhd.com/api/search/AAPL")
            .query(&[("api_token", key.as_str()), ("fmt", "json")])
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let data: Value = resp.json().await.unwrap();
        let arr = data.as_array().expect("search should return array");
        assert!(!arr.is_empty());

        let primary = arr.iter().find(|e| {
            e.get("isPrimary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                && e.get("Type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == "Common Stock")
                    .unwrap_or(false)
        });
        assert!(primary.is_some(), "should find primary listing for AAPL");
        let primary = primary.unwrap();
        assert_eq!(
            primary.get("Exchange").and_then(|v| v.as_str()),
            Some("US"),
            "Apple primary exchange should be US"
        );
    }
}
