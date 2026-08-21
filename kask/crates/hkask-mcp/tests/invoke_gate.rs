//! MCP runtime invoke path — call metering, not authorization.
//!
//! `McpRuntime::invoke` performs no per-call capability check. The prior
//! `DelegationToken` gate was removed because every production mint site derived
//! the token's `resource_id` from the same tool name it passed to `invoke`, so
//! the comparison was a value against itself and could not deny. Authority is
//! enforced at the allowlist boundaries (inference IPC `tool_allowlist`, swarm
//! card `mcp_tools`, per-server env allowlists), not here.
//!
//! What remains on this path is the runaway-loop breaker plus the
//! retry-safety classification. The metering tests the regression harness
//! (RR-0056/0057) pins live inline in `runtime.rs::invoke_gate_tests` because
//! the harness runs `cargo test --lib`; these are the public-API companions.

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
/// `InvocationFailed`.
///
/// The distinction is what lets a panel retry: `Unavailable` means the call
/// never reached the tool, so re-issuing it cannot double a side effect.
#[tokio::test]
async fn unreachable_server_reports_unavailable_not_failed() {
    let runtime = McpRuntime::new();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let result = runtime
        .invoke(
            "test-server",
            "test_tool",
            serde_json::json!({}),
            hkask_types::WebID::from_persona(b"any-agent"),
        )
        .await;

    match result {
        Err(ToolPortError::Unavailable(_)) => {}
        other => panic!(
            "a registered-but-unconnected server must report Unavailable so callers \
                 know a retry is safe, got: {other:?}"
        ),
    }
}
/// `Unavailable` is the only retryable classification.
///
/// Pins the predicate panels branch on. If `InvocationFailed` ever became
/// retryable, panels would re-issue state-changing tools whose failure was
/// semantic.
#[test]
fn only_unavailable_is_retryable() {
    assert!(ToolPortError::Unavailable("transport closed".into()).is_retryable());
    assert!(!ToolPortError::InvocationFailed("tool said no".into()).is_retryable());
    assert!(!ToolPortError::EnergyBudgetExceeded("cap".into()).is_retryable());
    assert!(
        !ToolPortError::NotFound(hkask_types::NotFound {
            entity_type: "tool".into(),
            id: "nope".into(),
        })
        .is_retryable()
    );
}
/// A request that was delivered before the connection dropped must NOT be
/// retryable.
///
/// `rmcp` reports both a failed send and a dropped response channel as
/// `ServiceError::TransportClosed` (`service.rs:921` vs `:555,566`), so once
/// a request reaches a live peer, a transport loss is not proof of
/// non-delivery. Auto-retrying would duplicate side effects — two
/// `kanban_task_create`s, or a `swarm_hire` charging credits twice.
#[test]
fn interrupted_is_never_auto_retried() {
    let interrupted = ToolPortError::Interrupted("connection reset".into());
    assert!(
        !interrupted.is_retryable(),
        "an interrupted call has an unknown outcome; retrying it risks applying \
             a state-changing effect twice"
    );
}
/// The two transport classifications are distinct, and the unknown-outcome
/// one says so in the message an operator sees.
#[test]
fn interrupted_and_unavailable_are_distinguishable() {
    let unavailable = ToolPortError::Unavailable("no live connection".into()).to_string();
    let interrupted = ToolPortError::Interrupted("connection reset".into()).to_string();
    assert_ne!(
        unavailable, interrupted,
        "an operator must be able to tell 'never ran' from 'outcome unknown'"
    );
    assert!(
        interrupted.contains("unknown"),
        "the interrupted message must state that the outcome is unknown, got: {interrupted}"
    );
}
/// An unknown tool is `NotFound`, not `Unavailable` — retrying cannot
/// conjure a tool that was never registered.
#[tokio::test]
async fn unknown_tool_is_not_found_not_unavailable() {
    let runtime = McpRuntime::new();
    register_test_tool(&runtime, "test-server", "test_tool").await;

    let result = runtime
        .invoke(
            "test-server",
            "no_such_tool",
            serde_json::json!({}),
            hkask_types::WebID::from_persona(b"any-agent"),
        )
        .await;

    match result {
        Err(ToolPortError::NotFound(_)) => {}
        other => panic!("an unregistered tool name must report NotFound, got: {other:?}"),
    }
}
