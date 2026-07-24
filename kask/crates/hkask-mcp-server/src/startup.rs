//! Shared MCP server startup verification — P4 Gate 1/2/3 enforcement.
//!
//! Every MCP server binary must verify three gates at startup before accepting
//! tool invocations:
//!
//! - **Gate 1 (Authentication):** Is the userpod authenticated via daemon?
//! - **Gate 2 (Assignment):** Is the userpod assigned to this MCP role?
//! - **Gate 3 (Capability):** Does the userpod hold OCAP tokens for the
//!   tools this server exposes?
//!
//! Gate 3 capability denials are non-fatal — the server starts in degraded
//! mode (denied tools unavailable). This matches the research server's behavior
//! and the OCAP principle that tools are individually gated, not the whole server.
//!
//! # Example
//!
//! ```rust,ignore
//! use hkask_mcp::startup::verify_startup_gates;
//!
//! async fn try_daemon_flow(userpod: &str) -> anyhow::Result<()> {
//!     let client = hkask_mcp::DaemonClient::new();
//!     let result = verify_startup_gates(
//!         &client,
//!         userpod,
//!         "condenser",
//!         &["compress", "classify", "persist", "thread_summary"],
//!     ).await?;
//!     tracing::info!(target: "hkask.mcp.condenser",
//!         userpod = userpod,
//!         "P4 gates verified — {} tool(s) denied: {:?}",
//!         result.denied_tools.len(),
//!         result.denied_tools
//!     );
//!     Ok(())
//! }
//! ```

use crate::daemon::{DaemonClient, DaemonResponse};
use crate::server::McpError;

/// Result of startup gate verification.
#[derive(Debug, Clone)]
pub struct StartupGateResult {
    /// Gate 1 passed — userpod is authenticated.
    pub authenticated: bool,
    /// Gate 2 passed — userpod is assigned to the role.
    pub assigned: bool,
    /// Tool names whose capability check was explicitly denied (Gate 3).
    /// Empty if all required tools were granted.
    pub denied_tools: Vec<String>,
}

/// Verify all three P4 gates at MCP server startup.
///
/// Performs daemon queries in order:
/// 1. `auth_query` — if unauthenticated, returns an error.
/// 2. ~~`assignment_query`~~ — removed (role assignment tracking deleted).
/// 3. `capability_query` for each tool in `required_tools` — denied tools are
///    collected into [`StartupGateResult::denied_tools`] (non-fatal: the server
///    starts with degraded capabilities).
///
/// This function performs no logging — callers should log using their
/// canonical tracing target (e.g., `"hkask.mcp.memory"`) before and after.
///
/// # Errors
///
/// Returns `McpError` when:
/// - Authentication returns `authenticated: false`
/// - Assignment returns `assigned: false`
/// - The daemon sends an unrecognized response variant
///
/// # Examples
///
/// Full three-gate check with required capability tools and caller-side logging:
/// ```rust,ignore
/// let client = hkask_mcp::DaemonClient::new();
/// let result = hkask_mcp::startup::verify_startup_gates(
///     &client,
///     "alice",
///     "memory",
///     &["episodic_store", "semantic_search", "memory_backup"],
/// ).await?;
/// tracing::info!(target: "hkask.mcp.memory",
///     userpod = "alice",
///     "P4 gates verified — {} tool(s) denied: {:?}",
///     result.denied_tools.len(),
///     result.denied_tools
/// );
/// ```
///
/// Server with no capability-gated tools (Gate 3 is a no-op):
/// ```rust,ignore
/// hkask_mcp::startup::verify_startup_gates(
///     &client,
///     "alice",
///     "condenser",
///     &[],
/// ).await?;
/// ```
#[must_use = "startup gate verification result must be inspected"]
pub async fn verify_startup_gates(
    client: &DaemonClient,
    userpod: &str,
    _role: &str,
    required_tools: &[&str],
) -> Result<StartupGateResult, McpError> {
    // ── Gate 1: Authentication ──────────────────────────────────────────

    let auth = client.auth_query(userpod).await?;
    match auth {
        DaemonResponse::AuthResponse {
            authenticated: true,
            ..
        } => {
            // Authenticated — proceed to Gate 2.
        }
        DaemonResponse::AuthResponse {
            authenticated: false,
            action: Some(ref action),
            ..
        } if action == "prompt_user" => {
            return Err(McpError::Auth {
                userpod: userpod.to_string(),
            });
        }
        other => {
            return Err(McpError::UnexpectedResponse {
                context: "auth".to_string(),
                detail: format!("{:?}", other),
            });
        }
    }

    // Gate 2: Assignment — removed (role assignments deleted in consolidation).
    // Tools are accessed via OCAP capabilities, not role assignments.

    // ── Gate 3: Capability ──────────────────────────────────────────────

    let mut denied_tools = Vec::new();
    for tool in required_tools {
        let cap = client.capability_query(userpod, tool).await?;
        match cap {
            DaemonResponse::CapabilityResponse { granted: true } => {
                // Tool capability granted.
            }
            DaemonResponse::CapabilityResponse { granted: false } => {
                denied_tools.push(tool.to_string());
            }
            other => {
                return Err(McpError::UnexpectedResponse {
                    context: format!("capability({})", tool),
                    detail: format!("{:?}", other),
                });
            }
        }
    }

    Ok(StartupGateResult {
        authenticated: true,
        assigned: true,
        denied_tools,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::{DaemonHandler, DaemonListener};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Configurable mock daemon handler for startup gate tests.
    ///
    /// Each field controls how the mock responds:
    /// - `authenticated`: Gate 1 result
    /// - `assigned`: Gate 2 result (always responds to the queried role)
    /// - `granted_tools`: Set of tool names that should be granted (Gate 3);
    ///   any tool not in this set is denied.
    struct GateMock {
        authenticated: AtomicBool,
        #[allow(dead_code)]
        assigned: AtomicBool,
        granted_tools: Vec<String>,
    }

    #[async_trait::async_trait]
    impl DaemonHandler for GateMock {
        async fn check_auth(&self, userpod: &str) -> (bool, Option<String>) {
            let auth = self.authenticated.load(Ordering::SeqCst);
            (
                auth,
                if auth {
                    Some(format!("webid://{}", userpod))
                } else {
                    None
                },
            )
        }

        async fn check_capability(&self, _userpod: &str, tool: &str) -> bool {
            self.granted_tools.iter().any(|t| t == tool)
        }

        async fn store_experience(
            &self,
            _userpod: &str,
            _entity: &str,
            _attribute: &str,
            _value: &serde_json::Value,
            _confidence: Option<f64>,
        ) -> (bool, Option<String>, Option<String>) {
            (true, Some("ep-001".into()), Some("sem-001".into()))
        }

        async fn dispatch_tool(
            &self,
            _userpod: &str,
            _tool: &str,
            _input: &serde_json::Value,
        ) -> (bool, Option<serde_json::Value>, Option<String>) {
            (true, None, None)
        }

        async fn curator_health(&self, _userpod: &str) -> serde_json::Value {
            serde_json::json!({"reg_health": "healthy"})
        }

        async fn reg_status(&self, _userpod: &str, _domain: Option<&str>) -> serde_json::Value {
            serde_json::json!({"domains": []})
        }
    }

    async fn setup_gate_test(
        authenticated: bool,
        assigned: bool,
        granted_tools: Vec<String>,
    ) -> (DaemonClient, PathBuf) {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "hkask-gate-test-{}-{}.sock",
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_file(&path);
        let mut listener = DaemonListener::with_path(path.clone());
        listener.bind().await.expect("bind test socket");

        let handler = Arc::new(GateMock {
            authenticated: AtomicBool::new(authenticated),
            assigned: AtomicBool::new(assigned),
            granted_tools,
        });
        tokio::spawn(async move {
            let _ = listener.serve(handler).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (DaemonClient::with_path(path.clone()), path)
    }

    // ── Full success tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn all_gates_pass_no_tools() {
        let (client, _path) = setup_gate_test(true, true, vec![]).await;
        let result = verify_startup_gates(&client, "alice", "condenser", &[])
            .await
            .expect("all gates should pass");

        assert!(result.authenticated);
        assert!(result.assigned);
        assert!(result.denied_tools.is_empty());
    }

    #[tokio::test]
    async fn all_gates_pass_with_tools() {
        let (client, _path) = setup_gate_test(
            true,
            true,
            vec!["compress".into(), "classify".into(), "persist".into()],
        )
        .await;
        let result = verify_startup_gates(
            &client,
            "bob",
            "condenser",
            &["compress", "classify", "persist"],
        )
        .await
        .expect("all gates should pass");

        assert!(result.authenticated);
        assert!(result.assigned);
        assert!(result.denied_tools.is_empty());
    }

    // ── Gate 1 failure: authentication ──────────────────────────────────

    #[tokio::test]
    async fn gate1_auth_fails() {
        let (client, _path) = setup_gate_test(false, true, vec![]).await;
        let err = verify_startup_gates(&client, "alice", "condenser", &[])
            .await
            .expect_err("should fail on auth rejection");

        let msg = err.to_string();
        assert!(
            msg.contains("not authenticated"),
            "expected auth error, got: {msg}"
        );
    }

    // ── Gate 2: assignment system was removed — no gate2_assignment_fails test ──

    // ── Gate 3: partial capability denial (non-fatal) ───────────────────

    #[tokio::test]
    async fn gate3_some_capabilities_denied() {
        // Only "compress" is granted; "classify" and "persist" are denied.
        let (client, _path) = setup_gate_test(true, true, vec!["compress".into()]).await;
        let result = verify_startup_gates(
            &client,
            "alice",
            "condenser",
            &["compress", "classify", "persist"],
        )
        .await
        .expect("should succeed even with denied capabilities");

        assert!(result.authenticated);
        assert!(result.assigned);
        assert_eq!(result.denied_tools.len(), 2);
        assert!(result.denied_tools.contains(&"classify".to_string()));
        assert!(result.denied_tools.contains(&"persist".to_string()));
        assert!(!result.denied_tools.contains(&"compress".to_string()));
    }

    #[tokio::test]
    async fn gate3_all_capabilities_denied() {
        // No tools granted.
        let (client, _path) = setup_gate_test(true, true, vec![]).await;
        let result = verify_startup_gates(&client, "alice", "condenser", &["compress", "classify"])
            .await
            .expect("should succeed even with all capabilities denied");

        assert!(result.authenticated);
        assert!(result.assigned);
        assert_eq!(result.denied_tools.len(), 2);
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_required_tools_is_noop() {
        let (client, _path) = setup_gate_test(true, true, vec![]).await;
        let result = verify_startup_gates(&client, "carol", "spec", &[])
            .await
            .expect("empty tools should be no-op");

        assert!(result.authenticated);
        assert!(result.assigned);
        assert!(result.denied_tools.is_empty());
    }
}
