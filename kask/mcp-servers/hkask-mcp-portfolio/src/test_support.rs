//! Shared fixtures for the portfolio integration tests — the lib-crate
//! test seam (single-sourced from the former per-file copies, test-suite
//! cleanup B2; the jscpd clusters between smoke.rs and tool_behavior.rs).
//! Lives in the lib — not `tests/common/mod.rs` — because the fork surface
//! forbids `mod.rs` (CI: `check-no-mod-rs.sh`); `#[doc(hidden)]` keeps it
//! out of the public docs while the integration test binaries (separate
//! crates) reach it through the lib.

use crate::server::PortfolioServer;
use crate::{AssetType, PortfolioStore, Transaction, TxType};
use hkask_types::WebID;

/// Build a server over a fresh temp-dir SQLite store, returning the server
/// and its directory so a test can clean up after itself.
pub fn make_server() -> (PortfolioServer, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("hkask-portfolio-tests-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&dir);
    let owner = WebID::new();
    let store = PortfolioStore::with_dir_for_owner(dir.clone(), owner);
    let spreadsheet =
        hkask_spreadsheet::WorkbookService::start_with_root(dir.join("spreadsheet-workbooks"))
            .expect("spreadsheet engine actor");
    let server = PortfolioServer::new(WebID::new(), store, spreadsheet);
    (server, dir)
}

/// Extract the MCP tool-result content via the canonical envelope helper
/// (`hkask_types::tool_response::parse_tool_response` — parse + unwrap in
/// one call; the former per-file copies re-implemented it).
pub fn unwrap_content(output: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(output).expect("tool output must be valid JSON")
}

/// Build a stock `Transaction` fixture — the field defaults every test
/// builder repeated (Stock asset type, zero commission, no weight, USD,
/// empty notes), with `created_at` derived from the date. Integration
/// suites wrap this with their own id policy (the tool-behavior suite
/// mints a uuid per transaction).
pub fn transaction(
    id: &str,
    date: &str,
    tx_type: TxType,
    symbol: Option<&str>,
    quantity: Option<f64>,
    price: Option<f64>,
    amount: Option<f64>,
) -> Transaction {
    Transaction {
        id: id.to_string(),
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
        created_at: format!("{date}T00:00:00Z"),
    }
}
