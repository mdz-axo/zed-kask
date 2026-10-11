//! Shared fixtures for the portfolio integration tests — single-sourced
//! from the former per-file copies (test-suite cleanup B2; the jscpd
//! clusters between smoke.rs and tool_behavior.rs).

use hkask_mcp_portfolio::PortfolioStore;
use hkask_mcp_portfolio::server::PortfolioServer;
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
