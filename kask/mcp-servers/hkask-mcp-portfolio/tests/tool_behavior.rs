//! Tool-behavior contract tests for the portfolio MCP server.
//!
//! Drives the real `#[tool]` methods through their public `Parameters<T>`
//! seam over a temp-dir SQLite store — the repo testing standard
//! (docs/reference/mcp-servers/README.md §Testing standard). The store-level
//! suite in `src/tests.rs` pins the computation; this suite pins the TOOL
//! contracts: request validation, the seed→returns loop, the missing-price
//! gate (a data gap is an error naming the gap, never a zero valuation),
//! view invalidation, and error specificity.

#![forbid(unsafe_code)]

use hkask_mcp_portfolio::server::{
    LedgerApplyRequest, PortfolioCreateRequest, PortfolioNameRequest, PortfolioReturnsRequest,
    PortfolioServer, PortfolioSnapshotRequest, PriceSeedEntry, PriceSeedRequest,
};
use hkask_mcp_portfolio::{AssetType, PortfolioStore, Transaction, TxType};
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;

/// Extract the MCP tool-result envelope: `{"content": <value>}`.
fn unwrap_content(output: &str) -> serde_json::Value {
    let parsed: serde_json::Value = serde_json::from_str(output)
        .unwrap_or_else(|e| panic!("tool output must be valid JSON, got: {output} ({e})"));
    parsed
        .get("content")
        .cloned()
        .unwrap_or_else(|| panic!("tool output must have 'content' key, got: {parsed}"))
}

fn make_server() -> (PortfolioServer, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "hkask-portfolio-tool-behavior-{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let owner = WebID::new();
    let store = PortfolioStore::with_dir_for_owner(dir.clone(), owner);
    let server = PortfolioServer::new(WebID::new(), store);
    (server, dir)
}

fn transaction(
    date: &str,
    tx_type: TxType,
    symbol: Option<&str>,
    quantity: Option<f64>,
    price: Option<f64>,
    amount: Option<f64>,
) -> Transaction {
    Transaction {
        id: uuid::Uuid::new_v4().to_string(),
        date: date.to_string(),
        tx_type,
        asset_type: AssetType::Stock,
        symbol: symbol.map(str::to_string),
        quantity,
        price,
        commission: Some(0.0),
        amount,
        weight: None,
        currency: "USD".to_string(),
        notes: String::new(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
    }
}

/// The full tool-seam loop: create → apply → batch-seed → returns →
/// materialize → daily_returns. Pins the interaction pattern the
/// portfolio-review skill composes.
#[tokio::test]
async fn create_apply_batch_seed_returns_materialize_loop() {
    let (server, dir) = make_server();

    // Create + list.
    let output = server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "growth".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    assert_eq!(unwrap_content(&output)["status"], "created");
    let list = server.portfolio_list().await.expect("list ok");
    let list_content = unwrap_content(&list);
    let names: Vec<String> = list_content["portfolios"]
        .as_array()
        .expect("portfolios array")
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"growth".to_string()),
        "created portfolio must list, got: {names:?}"
    );

    // Apply a deposit and a buy through the ledger tool.
    for tx in [
        transaction(
            "2026-01-05",
            TxType::Deposit,
            None,
            None,
            None,
            Some(20_000.0),
        ),
        transaction(
            "2026-01-10",
            TxType::Buy,
            Some("AAPL"),
            Some(100.0),
            Some(150.0),
            None,
        ),
    ] {
        let output = server
            .ledger_apply(Parameters(LedgerApplyRequest {
                portfolio: "growth".into(),
                transaction: tx,
            }))
            .await
            .expect("apply ok");
        assert_eq!(unwrap_content(&output)["status"], "applied");
    }

    // Snapshot shows the position and cash.
    let snapshot = server
        .portfolio_snapshot(Parameters(PortfolioSnapshotRequest {
            portfolio: "growth".into(),
            date: "2026-01-10".into(),
        }))
        .await
        .expect("snapshot ok");
    let snapshot_content = unwrap_content(&snapshot);
    assert_eq!(
        snapshot_content["holdings"].as_array().map(Vec::len),
        Some(1),
        "one holding at the snapshot date, got: {snapshot_content}"
    );

    // Batch-seed prices — one call instead of N (the 2026-09-03 addition).
    let seed = server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "growth".into(),
            symbol: None,
            date: None,
            close: None,
            source: None,
            prices: Some(vec![
                PriceSeedEntry {
                    symbol: "AAPL".into(),
                    date: "2026-01-10".into(),
                    close: 150.0,
                    source: Some("test".into()),
                },
                PriceSeedEntry {
                    symbol: "AAPL".into(),
                    date: "2026-02-10".into(),
                    close: 165.0,
                    source: Some("test".into()),
                },
            ]),
        }))
        .await
        .expect("seed ok");
    let seed_content = unwrap_content(&seed);
    assert_eq!(seed_content["seeded_count"], 2, "both prices seeded");

    // Returns now compute (TWR over the seeded window).
    let returns = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-02-10".into(),
        }))
        .await
        .expect("returns ok");
    let returns_content = unwrap_content(&returns);
    let total_return = returns_content["total_return"]
        .as_f64()
        .expect("total_return is a number");
    // 100 shares × (165 − 150) = +1500 on a 15000 start → +10%.
    assert!(
        (total_return - 0.10).abs() < 1e-9,
        "AAPL 150→165 on a 15k start is +10%, got {total_return}: {returns_content}"
    );

    // Materialize + read the daily series.
    server
        .portfolio_materialize_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-01-12".into(),
        }))
        .await
        .expect("materialize ok");
    let daily = server
        .portfolio_daily_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "growth".into(),
            from: "2026-01-10".into(),
            to: "2026-01-12".into(),
        }))
        .await
        .expect("daily ok");
    let daily_content = unwrap_content(&daily);
    let rows = daily_content["rows"]
        .as_array()
        .or_else(|| daily_content["daily_returns"].as_array())
        .unwrap_or_else(|| panic!("daily returns rows, got: {daily_content}"));
    assert_eq!(rows.len(), 3, "three calendar days materialized");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The missing-price gate through the TOOL seam: an unseeded holding errors
/// naming the (symbol, date) gap — never a zero valuation.
#[tokio::test]
async fn returns_tool_errors_naming_missing_prices() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "gap".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    server
        .ledger_apply(Parameters(LedgerApplyRequest {
            portfolio: "gap".into(),
            transaction: transaction(
                "2026-01-05",
                TxType::Buy,
                Some("MSFT"),
                Some(10.0),
                Some(380.0),
                None,
            ),
        }))
        .await
        .expect("apply ok");

    let error = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "gap".into(),
            from: "2026-01-05".into(),
            to: "2026-02-05".into(),
        }))
        .await
        .expect_err("returns must fail on the missing price");
    let error = error.message;
    assert!(
        error.contains("missing cached prices"),
        "the error must name the gap, got: {error}"
    );
    assert!(
        error.contains("MSFT"),
        "the error must name the symbol, got: {error}"
    );
    assert!(
        error.contains("portfolio_seed_price"),
        "the error must name the remedy, got: {error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Seeding invalidates materialized views from the seeded date forward —
/// materialize-then-seed never serves stale rows (pinned at the tool seam).
#[tokio::test]
async fn seed_tool_invalidates_materialized_views() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "invalidate".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    server
        .ledger_apply(Parameters(LedgerApplyRequest {
            portfolio: "invalidate".into(),
            transaction: transaction(
                "2026-01-05",
                TxType::Buy,
                Some("AAPL"),
                Some(10.0),
                Some(150.0),
                None,
            ),
        }))
        .await
        .expect("apply ok");
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "invalidate".into(),
            symbol: Some("AAPL".into()),
            date: Some("2026-01-05".into()),
            close: Some(150.0),
            source: Some("test".into()),
            prices: None,
        }))
        .await
        .expect("seed ok");
    server
        .portfolio_materialize_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "invalidate".into(),
            from: "2026-01-05".into(),
            to: "2026-01-07".into(),
        }))
        .await
        .expect("materialize ok");

    // Re-seed from 01-06: rows from that date forward are invalidated.
    server
        .portfolio_seed_price(Parameters(PriceSeedRequest {
            portfolio: "invalidate".into(),
            symbol: Some("AAPL".into()),
            date: Some("2026-01-06".into()),
            close: Some(160.0),
            source: Some("test".into()),
            prices: None,
        }))
        .await
        .expect("re-seed ok");
    let daily = server
        .portfolio_daily_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "invalidate".into(),
            from: "2026-01-05".into(),
            to: "2026-01-07".into(),
        }))
        .await
        .expect("daily ok");
    let content = unwrap_content(&daily);
    let rows = content["rows"]
        .as_array()
        .or_else(|| content["daily_returns"].as_array())
        .expect("rows");
    assert!(
        rows.iter().all(|row| {
            row["date"]
                .as_str()
                .map(|d| d < "2026-01-06")
                .unwrap_or(true)
        }),
        "rows from the seeded date forward must be invalidated, got: {rows:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Invalid input through the tool seam: malformed dates are rejected with
/// error-specific messages (never silently substituted).
#[tokio::test]
async fn returns_tool_rejects_malformed_dates() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "dates".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    let error = server
        .portfolio_returns(Parameters(PortfolioReturnsRequest {
            portfolio: "dates".into(),
            from: "not-a-date".into(),
            to: "2026-02-05".into(),
        }))
        .await
        .expect_err("malformed date must fail");
    let error = error.message;
    assert!(
        error.contains("from") || error.contains("date"),
        "the error must name the malformed field, got: {error}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Delete removes the portfolio and its ledger.
#[tokio::test]
async fn delete_tool_removes_portfolio_and_ledger() {
    let (server, dir) = make_server();
    server
        .portfolio_create(Parameters(PortfolioCreateRequest {
            name: "doomed".into(),
            asset_type: AssetType::Stock,
        }))
        .await
        .expect("create ok");
    let output = server
        .portfolio_delete(Parameters(PortfolioNameRequest {
            name: "doomed".into(),
        }))
        .await
        .expect("delete ok");
    assert_eq!(unwrap_content(&output)["status"], "deleted");
    let list = server.portfolio_list().await.expect("list ok");
    let list_content = unwrap_content(&list);
    let names: Vec<String> = list_content["portfolios"]
        .as_array()
        .expect("portfolios")
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        !names.contains(&"doomed".to_string()),
        "deleted portfolio is gone"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
