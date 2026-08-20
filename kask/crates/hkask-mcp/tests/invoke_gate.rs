//! MCP runtime invoke path — call metering, not authorization.
//!
//! `McpRuntime::invoke` performs no per-call capability check. The prior
//! `DelegationToken` gate was removed because every production mint site derived
//! the token's `resource_id` from the same tool name it passed to `invoke`, so
//! the comparison was a value against itself and could not deny. Authority is
//! enforced at the allowlist boundaries (inference IPC `tool_allowlist`, swarm
//! card `mcp_tools`, per-server env allowlists), not here.
//!
//! What remains on this path is the runaway-loop breaker. These tests pin its
//! three behaviors.
//!
//! # Oracle
//! - Invariant: an agent with no registered ceiling is auto-registered and
//!   allowed through (a wiring omission must not fail the call)
//! - Invariant: an agent that exhausts its ceiling MUST get `EnergyBudgetExceeded`
//! - Invariant: a runtime without governance dispatches unmetered rather than
//!   refusing

use hkask_mcp::{McpRuntime, McpServer, McpTool};
use hkask_regulation::{CyberneticsLoop, NoopEventSink, RegulationLedger};
use hkask_tool_port::{ToolPort, ToolPortError};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

fn cybernetics() -> Arc<RwLock<CyberneticsLoop>> {
    let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(10)));
    Arc::new(RwLock::new(CyberneticsLoop::new(ledger)))
}

/// A stable per-suite agent identity for ceiling tests.
fn test_agent_webid() -> hkask_types::WebID {
    hkask_types::WebID::from_persona(b"invoke-gate-test-agent")
}

/// Build a `McpRuntime` with governance (metering + event sink) wired.
fn governed_runtime() -> McpRuntime {
    McpRuntime::new().with_governance(cybernetics(), Arc::new(NoopEventSink))
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

/// An agent the composition root never seeded must NOT be refused. This is the
/// regression for the persona-mismatch bug: `main.rs` seeded only `swarm-panel`,
/// while the IPC dispatch and the cascade used `kask-panel` and
/// `manifest-executor`, so the old fail-closed cap denied every one of their
/// tool calls.
#[tokio::test]
async fn unregistered_agent_is_auto_registered_not_denied() {
    let runtime = governed_runtime();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let unseeded = hkask_types::WebID::from_persona(b"never-registered-persona");
    let result = runtime
        .invoke("test-server", "test_tool", json!({}), unseeded)
        .await;

    assert!(
        !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "an agent with no registered ceiling must be auto-registered and allowed \
         through, not refused (wiring omission != authorization decision), got: {result:?}"
    );
}

/// The one pre-dispatch refusal: a runaway loop that burns its whole per-tick
/// ceiling.
#[tokio::test]
async fn exhausted_ceiling_trips_the_runaway_breaker() {
    let cybernetics = cybernetics();
    let agent = test_agent_webid();
    // Ceiling of 1: the first call consumes it, the second must trip.
    cybernetics.read().await.register_call_cap(agent, 1).await;

    let runtime = McpRuntime::new().with_governance(cybernetics, Arc::new(NoopEventSink));
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let first = runtime
        .invoke("test-server", "test_tool", json!({}), agent)
        .await;
    assert!(
        !matches!(first, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "the first call fits within a ceiling of 1, got: {first:?}"
    );

    let second = runtime
        .invoke("test-server", "test_tool", json!({}), agent)
        .await;
    assert!(
        matches!(second, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "exhausting the per-tick ceiling must trip the runaway-loop breaker, got: {second:?}"
    );
}

/// A seeded agent with headroom is not refused by the meter. Any failure is
/// downstream (no live connection in this test).
#[tokio::test]
async fn metering_allows_agent_with_headroom() {
    let cybernetics = cybernetics();
    let agent = test_agent_webid();
    cybernetics
        .read()
        .await
        .register_call_cap(agent, 1000)
        .await;

    let runtime = McpRuntime::new().with_governance(cybernetics, Arc::new(NoopEventSink));
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let result = runtime
        .invoke("test-server", "test_tool", json!({}), agent)
        .await;

    assert!(
        !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "an agent with headroom must not be refused by the meter, got: {result:?}"
    );
}

/// Metering is accounting, not authorization — its absence must not refuse the
/// call. This inverts the prior `no_governance_fails_closed` expectation: that
/// fail-closed branch made `McpRuntime::new()` unusable for any embedder that
/// did not also wire regulation, while denying nothing an attacker could reach.
#[tokio::test]
async fn no_governance_dispatches_unmetered() {
    let runtime = McpRuntime::new();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let result = runtime
        .invoke("test-server", "test_tool", json!({}), test_agent_webid())
        .await;

    assert!(
        !matches!(result, Err(ToolPortError::EnergyBudgetExceeded(_))),
        "an unmetered runtime must dispatch rather than refuse, got: {result:?}"
    );
}
