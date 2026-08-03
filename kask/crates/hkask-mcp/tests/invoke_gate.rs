//! MCP runtime invoke gate tests — the real OCAP + gas boundary.
//!
//! Exercises `McpRuntime::invoke`'s governance gate: OCAP token verification
//! (`DelegationToken::is_valid_for` / `verify_capability_domain`) and gas
//! gate (`CyberneticsLoop::can_proceed` + `reserve_gas` + `settle_gas`).
//!
//! The lifecycle integration tests cover registration only; these tests cover
//! the invoke path's governance membrane — the surface `.rules` calls "the
//! real OCAP boundary."
//!
//! # Oracle
//! - Invariant: a wrong-token invoke MUST return `CapabilityDenied`
//! - Invariant: a no-budget invoke MUST return `EnergyBudgetExceeded`
//! - Invariant: a valid-token + seeded-budget invoke MUST NOT return a
//!   governance error (the gate allowed it through; any failure is downstream)

use hkask_capability::{
    DelegationAction, DelegationResource, DelegationToken, ToolPort, ToolPortError,
};
use hkask_mcp::{FlatEnergyEstimator, McpRuntime, McpServer, McpTool};
use hkask_regulation::{CyberneticsLoop, GasBudget, GasCost, NoopEventSink, RegulationLedger};
use hkask_test_harness::{test_agent_webid, test_token_for_tool};
use hkask_types::WebID;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Build a `McpRuntime` with governance wired (OCAP + gas + NoopEventSink).
fn governed_runtime() -> McpRuntime {
    let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
    let cybernetics = Arc::new(RwLock::new(CyberneticsLoop::new(ledger)));
    McpRuntime::new().with_governance(
        cybernetics,
        Arc::new(NoopEventSink),
        FlatEnergyEstimator::new(),
    )
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

/// Mint a token for a *different* tool than the one being invoked.
fn wrong_tool_token() -> DelegationToken {
    DelegationToken::new(
        DelegationResource::Tool,
        "wrong_tool".to_string(),
        DelegationAction::Execute,
        WebID::from_persona(b"test-from"),
        test_agent_webid(),
    )
}

#[tokio::test]
async fn governance_denies_wrong_tool_token() {
    let runtime = governed_runtime();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let token = wrong_tool_token();
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

    // Valid token (resource_id matches tool name) but no gas budget seeded.
    let token = test_token_for_tool("test_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    assert!(
        matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "no-budget invoke must be denied by gas gate, got: {result:?}"
    );
}

#[tokio::test]
async fn governance_allows_valid_token_with_budget() {
    let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
    let cybernetics = Arc::new(RwLock::new(CyberneticsLoop::new(ledger)));
    let agent = test_agent_webid();

    // Seed a gas budget large enough for the flat-cost estimator (10 gas/call).
    cybernetics
        .read()
        .await
        .register_gas_budget(agent, GasBudget::new(GasCost(1000)))
        .await;

    let runtime = McpRuntime::new().with_governance(
        cybernetics,
        Arc::new(NoopEventSink),
        FlatEnergyEstimator::new(),
    );
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
        "seeded budget must not be denied by gas gate, got: {result:?}"
    );
}

#[tokio::test]
async fn no_governance_bypasses_gate() {
    let runtime = McpRuntime::new();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    // Any token — governance is None, so the gate is skipped entirely.
    let token = test_token_for_tool("test_tool");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), &token)
        .await;

    assert!(
        !matches!(result, Err(ToolPortError::CapabilityDenied(_))),
        "no-governance runtime must not deny via OCAP, got: {result:?}"
    );
    assert!(
        !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "no-governance runtime must not deny via gas gate, got: {result:?}"
    );
}
