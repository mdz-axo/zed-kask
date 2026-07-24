//! Handler trait for daemon queries — runs inside hKask.
//!
//! Implemented by the hKask runtime to provide authentication,
//! assignment verification, capability checking, and dual memory encoding.

/// Handler trait for daemon queries.
///
/// Implemented by the hKask runtime to provide authentication,
/// assignment verification, capability checking, and dual memory encoding.
#[async_trait::async_trait]
pub trait DaemonHandler: Send + Sync {
    /// Check if a userpod is authenticated. Returns (authenticated, webid).
    async fn check_auth(&self, userpod: &str) -> (bool, Option<String>);

    /// Check if a userpod holds a capability token for a tool.
    async fn check_capability(&self, userpod: &str, tool: &str) -> bool;

    /// Store an experience in both episodic and semantic memory.
    /// Returns (stored, episodic_triple_id, semantic_triple_id).
    async fn store_experience(
        &self,
        userpod: &str,
        entity: &str,
        attribute: &str,
        value: &serde_json::Value,
        confidence: Option<f64>,
    ) -> (bool, Option<String>, Option<String>);

    /// Dispatch a tool call to an MCP server.
    /// Returns (ok, output, error_message).
    async fn dispatch_tool(
        &self,
        userpod: &str,
        tool: &str,
        input: &serde_json::Value,
    ) -> (bool, Option<serde_json::Value>, Option<String>);

    /// Query curator system health — returns a HealthSnapshot as JSON.
    async fn curator_health(&self, userpod: &str) -> serde_json::Value;

    /// Query live Regulation status — variety per domain, backpressure.
    async fn reg_status(&self, userpod: &str, domain: Option<&str>) -> serde_json::Value;
}
