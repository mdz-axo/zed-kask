//! Smoke test for hkask-mcp-portfolio — verifies the server starts, its
//! simplest read tool returns valid JSON, and the CSV import → ledger read
//! round-trip works end-to-end. Catches wiring regressions that a unit test
//! of the store alone would miss.
//!
//! No network, no credentials: the store is backed by a temp-dir SQLite DB.

#![forbid(unsafe_code)]

use hkask_mcp_portfolio::server::{
    ImportFormat, LedgerImportRequest, LedgerReadRequest, PortfolioServer,
};
use hkask_mcp_portfolio::{AssetType, PortfolioStore};
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

#[tokio::test]
async fn portfolio_list_returns_empty_array_on_fresh_store() {
    let dir = std::env::temp_dir().join(format!(
        "hkask-portfolio-smoke-list-{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let owner = WebID::new();
    let store = PortfolioStore::with_dir_for_owner(dir.clone(), owner);
    let server = PortfolioServer::new(WebID::new(), store);

    let output = server.portfolio_list().await;
    let content = unwrap_content(&output);
    let portfolios = content
        .get("portfolios")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!("portfolio_list content must have 'portfolios' array, got: {content}")
        });
    assert!(
        portfolios.is_empty(),
        "fresh store must list zero portfolios"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ledger_import_csv_then_read_round_trips() {
    let dir = std::env::temp_dir().join(format!(
        "hkask-portfolio-smoke-csv-{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let owner = WebID::new();
    let store = PortfolioStore::with_dir_for_owner(dir.clone(), owner);
    let server = PortfolioServer::new(WebID::new(), store);

    // Create a portfolio to import into.
    server
        .store
        .create("csv-test", AssetType::Stock)
        .expect("store.create must succeed on a fresh DB");

    let csv = "\
id,date,type,symbol,quantity,price,amount,currency
tx-001,2026-01-15,buy,AAPL,100,150.00,15000.00,USD
tx-002,2026-02-01,buy,MSFT,50,380.00,19000.00,USD
tx-003,2026-03-10,sell,AAPL,100,175.00,17500.00,USD
";

    let import_output = server
        .ledger_import(Parameters(LedgerImportRequest {
            portfolio: "csv-test".into(),
            asset_type: AssetType::Stock,
            format: ImportFormat::Csv,
            data: csv.into(),
        }))
        .await;
    let import_content = unwrap_content(&import_output);
    assert_eq!(
        import_content["status"], "imported",
        "import status must be 'imported', got: {import_content}"
    );
    assert_eq!(
        import_content["count"], 3,
        "import count must be 3, got: {import_content}"
    );

    // Read back the imported transactions.
    let read_output = server
        .ledger_read(Parameters(LedgerReadRequest {
            portfolio: "csv-test".into(),
            symbol: None,
            tx_type: None,
            asset_type: None,
            from_date: None,
            to_date: None,
        }))
        .await;
    let read_content = unwrap_content(&read_output);
    let count = read_content["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("ledger_read must return count, got: {read_content}"));
    assert_eq!(count, 3, "ledger_read must return 3 transactions");

    let transactions = read_content["transactions"]
        .as_array()
        .unwrap_or_else(|| panic!("transactions must be an array, got: {read_content}"));
    let symbols: Vec<&str> = transactions
        .iter()
        .map(|t| t["symbol"].as_str().unwrap_or(""))
        .collect();
    assert!(
        symbols.contains(&"AAPL") && symbols.contains(&"MSFT"),
        "imported transactions must include AAPL and MSFT, got: {symbols:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
