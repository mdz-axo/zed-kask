//! MCP runtime invoke gate tests — the real OCAP + call-cap boundary.
//!
//! Exercises `McpRuntime::invoke`'s governance gate: OCAP token verification
//! (`DelegationToken::is_valid_for` / `verify_capability_domain`) and the
//! per-agent call cap (`CyberneticsLoop::can_proceed` + `charge_call`).
//!
//! The lifecycle integration tests cover registration only; these tests cover
//! the invoke path's governance membrane — the surface `.rules` calls "the
//! real OCAP boundary."
//!
//! # Oracle
//! - Invariant: a wrong-token invoke MUST return `CapabilityDenied`
//! - Invariant: a no-cap invoke MUST return `EnergyBudgetExceeded`
//! - Invariant: a valid-token + seeded-cap invoke MUST NOT return a
//!   governance error (the gate allowed it through; any failure is downstream)

use hkask_capability::{ToolPort, ToolPortError};
use hkask_mcp::{McpRuntime, McpServer, McpTool};
use hkask_regulation::{CyberneticsLoop, NoopEventSink, RegulationLedger};
use hkask_test_harness::{test_agent_webid, test_token_for_tool};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Build a `McpRuntime` with governance wired (OCAP + call cap + NoopEventSink).
fn governed_runtime() -> McpRuntime {
    let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
    let cybernetics = Arc::new(RwLock::new(CyberneticsLoop::new(ledger)));
    McpRuntime::new().with_governance(cybernetics, Arc::new(NoopEventSink))
}

/// Register a minimal server with one tool so the runtime has metadata.
async fn register_test_tool(runtime: &McpRuntime, server_id: &str, tool_name: &str) {
    let tool = McpTool {
        name: tool_name.to_string(),
        description: "test tool".to_string(),
        input_schema: json!({"type": "object", "properties": {}}),
        server_id: server_id.to_string(),
    };
    let server = McpServer {
        id: server_id.to_string(),
        name: server_id.to_string(),
        tools: vec![tool],
    };
    runtime.register_server(server).await;
}

#[tokio::test]
async fn governance_denies_wrong_tool_token() {
    let runtime = governed_runtime();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let token = test_token_for_tool("wrong_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    assert!(
        matches!(result, Err(ToolPortError::CapabilityDenied(_))),
        "wrong-tool token must be denied, got: {result:?}"
    );
}

#[tokio::test]
async fn governance_denies_no_budget() {
    let runtime = governed_runtime();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    // Valid token (resource_id matches tool name) but no call cap seeded.
    let token = test_token_for_tool("test_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    assert!(
        matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "no-cap invoke must be denied by the call-cap gate, got: {result:?}"
    );
}

#[tokio::test]
async fn governance_allows_valid_token_with_budget() {
    let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
    let cybernetics = Arc::new(RwLock::new(CyberneticsLoop::new(ledger)));
    let agent = test_agent_webid();

    // Seed a call cap large enough for the test's invocations.
    cybernetics
        .read()
        .await
        .register_call_cap(agent, 1000)
        .await;

    let runtime = McpRuntime::new().with_governance(cybernetics, Arc::new(NoopEventSink));
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let token = test_token_for_tool("test_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    // The gate allowed the request through. The failure (if any) is from
    // call_tool_inner (no live connection), NOT from governance.
    assert!(
        !matches!(result, Err(ToolPortError::CapabilityDenied(_))),
        "valid token must not be denied by OCAP, got: {result:?}"
    );
    assert!(
        !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "seeded cap must not be denied by the call-cap gate, got: {result:?}"
    );
}

#[tokio::test]
async fn no_governance_fails_closed() {
    let runtime = McpRuntime::new();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    // Governance is None — invoke must fail closed rather than bypass the
    // OCAP + call-cap membrane. A production embedder that forgets `with_governance`
    // would otherwise silently lose capability verification and cap
    // accounting. See the .rules "Process-global hooks set at runtime need a
    // startup-failure signal" and the OCAP gate trap.
    let token = test_token_for_tool("test_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    assert!(
        matches!(result, Err(ToolPortError::CapabilityDenied(_))),
        "no-governance runtime must fail closed (CapabilityDenied) instead of bypassing the gate, got: {result:?}"
    );
}
