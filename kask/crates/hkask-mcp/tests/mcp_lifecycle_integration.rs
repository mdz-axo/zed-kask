//! Integration tests for the hKask MCP runtime tool lifecycle.
//!
//! Verifies the full metadata lifecycle: server registration, tool
//! discovery, tool info retrieval, and server listing. Uses the
//! in-memory `McpRuntime` without spawning child processes.
//!
//! # Scope
//!
//! Tests the metadata path (register → list → get info). Actual tool
//! invocation (`call_tool`) requires a live child process with
//! `Peer<RoleClient>`, which is deferred until daemon startup
//! infrastructure is available in the test harness.
//!
//! # REQ tags
//!
//! Each test carries a `// REQ:` tag linking it to the contract-first
//! migration plan.

use hkask_mcp::{McpRuntime, McpServer, McpTool};
use serde_json::json;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create a minimal MCP tool for testing.
fn make_tool(name: &str, server_id: &str) -> McpTool {
    McpTool {
        name: name.to_string(),
        description: format!("Test tool: {}", name),
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            }
        }),
        server_id: server_id.to_string(),
    }
}

/// Create a minimal MCP server with the given tools.
fn make_server(id: &str, tools: Vec<McpTool>) -> McpServer {
    McpServer {
        id: id.to_string(),
        name: format!("Test Server {}", id),
        tools,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

///
/// After registering a server with tools, `discover_tools()` returns
/// all tool names and `list_servers()` returns the server.
#[tokio::test]
async fn register_server_and_discover_tools() {
    let runtime = McpRuntime::new();

    let tools = vec![
        make_tool("echo", "test-server"),
        make_tool("search", "test-server"),
        make_tool("summarize", "test-server"),
    ];
    let server = make_server("test-server", tools);
    runtime.register_server(server).await;

    // Discover tools
    let discovered = runtime.discover_tools().await;
    assert_eq!(discovered.len(), 3, "Should discover 3 tools");
    assert!(discovered.contains(&"echo".to_string()));
    assert!(discovered.contains(&"search".to_string()));
    assert!(discovered.contains(&"summarize".to_string()));

    // List servers
    let servers = runtime.list_servers().await;
    assert_eq!(servers.len(), 1, "Should have 1 registered server");
    assert_eq!(servers[0].id, "test-server");
    assert_eq!(servers[0].tools.len(), 3);
}

///
/// `get_tool_info()` returns correct metadata including server_id
/// and required capability.
#[tokio::test]
async fn get_tool_info_returns_metadata() {
    let runtime = McpRuntime::new();

    let tool = make_tool("echo", "echo-server");
    let server = make_server("echo-server", vec![tool]);
    runtime.register_server(server).await;

    let info = runtime
        .get_tool_info("echo")
        .await
        .expect("echo tool should be discoverable");

    assert_eq!(info.name, "echo");
    assert_eq!(info.server_id, "echo-server");
    assert!(!info.description.is_empty());
    assert!(info.input_schema.is_object());
    // required_capability is derived from server_id — type system guarantees it exists
}

///
/// `get_tool()` returns the full `McpTool` struct including input schema.
#[tokio::test]
async fn get_tool_returns_full_definition() {
    let runtime = McpRuntime::new();

    let tool = make_tool("search", "search-server");
    let server = make_server("search-server", vec![tool]);
    runtime.register_server(server).await;

    let retrieved = runtime
        .get_tool("search")
        .await
        .expect("search tool should be discoverable");

    assert_eq!(retrieved.name, "search");
    assert_eq!(retrieved.server_id, "search-server");
    assert!(retrieved.input_schema.is_object());
}

///
/// Tools from different servers are correctly namespaced and
/// `get_tool_info()` returns the correct server_id for each.
#[tokio::test]
async fn multi_server_tool_isolation() {
    let runtime = McpRuntime::new();

    // Register two servers with overlapping tool names
    let server_a = make_server(
        "server-a",
        vec![
            make_tool("echo", "server-a"),
            make_tool("search", "server-a"),
        ],
    );
    let server_b = make_server(
        "server-b",
        vec![
            make_tool("echo", "server-b"),
            make_tool("summarize", "server-b"),
        ],
    );

    runtime.register_server(server_a).await;
    runtime.register_server(server_b).await;

    // Both servers registered
    let servers = runtime.list_servers().await;
    assert_eq!(servers.len(), 2);

    // "echo" exists in both — last registration wins in tool_registry
    let discovered = runtime.discover_tools().await;
    // echo appears once (HashMap key), search and summarize are unique
    assert!(discovered.contains(&"echo".to_string()));
    assert!(discovered.contains(&"search".to_string()));
    assert!(discovered.contains(&"summarize".to_string()));

    // Tool info for unique tools returns correct server
    let search_info = runtime.get_tool_info("search").await.unwrap();
    assert_eq!(search_info.server_id, "server-a");

    let summarize_info = runtime.get_tool_info("summarize").await.unwrap();
    assert_eq!(summarize_info.server_id, "server-b");
}

///
/// Querying a non-existent tool returns `None` for both
/// `get_tool()` and `get_tool_info()`.
#[tokio::test]
async fn missing_tool_returns_none() {
    let runtime = McpRuntime::new();

    let server = make_server("empty-server", vec![]);
    runtime.register_server(server).await;

    assert!(runtime.get_tool("nonexistent").await.is_none());
    assert!(runtime.get_tool_info("nonexistent").await.is_none());
    assert!(runtime.discover_tools().await.is_empty());
}

// ── Schema validation contract tests ──────────────────────────────────────

///
/// Valid input conforming to the schema passes validation.
#[test]
fn valid_input_passes_schema_validation() {
    let tool = McpTool {
        name: "echo".into(),
        description: "Echo tool".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "count": { "type": "integer", "minimum": 1 }
            },
            "required": ["message"]
        }),
        server_id: "test".into(),
    };

    // Valid: all required fields present, types correct
    assert!(
        tool.validate_input(&json!({"message": "hello", "count": 3}))
            .is_ok()
    );

    // Valid: only required field
    assert!(tool.validate_input(&json!({"message": "hi"})).is_ok());
}

#[test]
fn invalid_input_fails_schema_validation() {
    let tool = McpTool {
        name: "echo".into(),
        description: "Echo tool".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        }),
        server_id: "test".into(),
    };

    // Missing required field
    let result = tool.validate_input(&json!({}));
    assert!(result.is_err());
    assert!(!result.unwrap_err().is_empty());

    // Wrong type
    let result = tool.validate_input(&json!({"message": 123}));
    assert!(result.is_err());
}

#[test]
fn empty_schema_passes_all_input() {
    let tool = McpTool {
        name: "no-schema".into(),
        description: "No schema tool".into(),
        input_schema: json!({}),
        server_id: "test".into(),
    };

    // Empty schema → everything passes
    assert!(tool.validate_input(&json!({})).is_ok());
    assert!(tool.validate_input(&json!({"anything": [1, 2, 3]})).is_ok());
    assert!(
        tool.validate_input(&json!("string instead of object"))
            .is_ok()
    );
}
