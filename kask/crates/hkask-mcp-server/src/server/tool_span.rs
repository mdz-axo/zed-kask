//! Tool execution — Regulation span emission, experience recording, and framework-level execution.

use hkask_types::McpErrorKind;
use hkask_types::time::now_rfc3339;
use serde_json::Value;
use std::time::Instant;

use super::error::McpToolError;
use super::http_helpers::McpToolOutput;

/// Error healing callback: (error_string, operation_name).
type HealCallback = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Experience recording callback: fires when a span finishes with "success" or "error".
pub type ExperienceCallback = Box<dyn Fn(&str) + Send + Sync>;

/// RAII guard — emits Regulation tool span on drop. Use `span.ok(output)` or `span.error(kind, output)`.
pub struct ToolSpanGuard {
    tool_name: String,
    start: Instant,
    caller: hkask_types::WebID,
    emitted: bool,
    /// Domain ontology concept for type-aware feedback routing (e.g. "pko:ChangeOfStatus").
    ontology: Option<&'static str>,
    /// Optional heal callback: (error_string, operation_name).
    heal_error_cb: Option<HealCallback>,
    /// Optional experience callback: fires on ok/error with "success"/"error".
    experience_cb: Option<ExperienceCallback>,
}

impl ToolSpanGuard {
    /// Create a new tool span guard.
    ///
    /// pre:  tool_name is non-empty, caller is valid
    /// post: returns ToolSpanGuard with start time recorded
    #[must_use]
    pub fn new(tool_name: &str, caller: &hkask_types::WebID) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            start: Instant::now(),
            caller: *caller,
            emitted: false,
            ontology: None,
            heal_error_cb: None,
            experience_cb: None,
        }
    }

    /// Tag this span with a domain ontology concept (e.g. "pko:ChangeOfStatus").
    /// The concept flows into the Regulation span for type-aware feedback routing.
    ///
    /// All hKask bridge crate constants (`hkask-bridge-pko`, `hkask-bridge-dublincore`,
    /// and domain-specific bridges like `hkask-mcp-companies/src/fibo.rs`) are valid
    /// `&'static str` concepts. This function documents the intent: `with_ontology`
    /// accepts ontology concepts, not arbitrary debug strings.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hkask_bridge_dublincore::STEP_EXECUTION;
    /// ToolSpanGuard::new("my_tool", &caller)
    ///     .with_ontology(STEP_EXECUTION);
    /// ```
    #[must_use]
    pub fn with_ontology(mut self, concept: &'static str) -> Self {
        self.ontology = Some(concept);
        self
    }

    /// Attach a self-healing callback for automatic error recovery.
    #[must_use]
    pub fn with_heal_cb(mut self, cb: HealCallback) -> Self {
        self.heal_error_cb = Some(cb);
        self
    }

    /// Attach an experience callback that fires when the span completes.
    ///
    /// The callback receives "success" or "error" based on how the span finishes.
    #[must_use]
    pub fn with_experience(mut self, cb: ExperienceCallback) -> Self {
        self.experience_cb = Some(cb);
        self
    }

    /// Mark span as successful and return output.
    ///
    /// post: Regulation tool span emitted with "ok" status
    /// post: returns output unchanged
    #[must_use]
    pub fn ok(mut self, output: String) -> String {
        self.emitted = true;
        let duration_ms = self.start.elapsed().as_millis() as u64;
        emit_tool_span(
            &self.tool_name,
            "ok",
            duration_ms,
            None,
            Some(&self.caller),
            self.ontology,
        );
        if let Some(ref cb) = self.experience_cb {
            cb("success");
        }
        output
    }

    /// Mark span as error and return output.
    ///
    /// post: Regulation tool span emitted with "error" status and error kind
    /// post: returns output unchanged
    #[must_use]
    pub fn error(mut self, kind: McpErrorKind, output: String) -> String {
        self.emitted = true;
        let duration_ms = self.start.elapsed().as_millis() as u64;
        emit_tool_span(
            &self.tool_name,
            "error",
            duration_ms,
            Some(&kind),
            Some(&self.caller),
            self.ontology,
        );
        if let Some(ref cb) = self.heal_error_cb {
            cb(&output, &self.tool_name);
        }
        if let Some(ref cb) = self.experience_cb {
            cb("error");
        }
        output
    }

    /// Equivalent to `self.ok(McpToolOutput::new(value).to_json_string())`.
    /// Finish span with Ok JSON value.
    ///
    /// post: Regulation tool span emitted with "ok" status
    /// post: returns JSON string of value
    #[must_use]
    pub fn ok_json(self, value: Value) -> String {
        self.ok(McpToolOutput::new(value).to_json_string())
    }

    /// Consume a `Result<Value, McpToolError>` — ok→`ok_json`, err→`error(…)`.
    /// Finish span with a Result.
    ///
    /// post: Regulation tool span emitted with appropriate status
    /// post: returns JSON string of Ok value or error
    #[must_use]
    pub fn finish(self, result: Result<Value, McpToolError>) -> String {
        match result {
            Ok(value) => self.ok_json(value),
            Err(e) => self.error(e.kind, e.to_json_string()),
        }
    }

    /// Produces McpToolError wire format so clients can distinguish errors from successes.
    /// Finish span with an internal error.
    ///
    /// post: Regulation tool span emitted with "error" status
    /// post: returns JSON error string
    #[must_use]
    pub fn internal_error(self, value: Value) -> String {
        let message = match value {
            Value::String(s) => s,
            other => other.to_string(),
        };
        self.error(
            McpErrorKind::Internal,
            McpToolError::internal(message).to_json_string(),
        )
    }
}

impl Drop for ToolSpanGuard {
    fn drop(&mut self) {
        if !self.emitted {
            // Guard dropped without calling ok() or error() — emit a warning span
            let duration_ms = self.start.elapsed().as_millis() as u64;
            emit_tool_span(
                &self.tool_name,
                "dropped",
                duration_ms,
                None,
                Some(&self.caller),
                None,
            );
        }
    }
}

// ── Regulation span emission ─────────────────────────────────────────────────────

/// Emit a Regulation tool span with caller identity (WebID) for observability.
fn emit_tool_span(
    tool_name: &str,
    outcome: &str,
    duration_ms: u64,
    error_kind: Option<&McpErrorKind>,
    caller: Option<&hkask_types::WebID>,
    ontology: Option<&str>,
) {
    tracing::info!(target: "reg.tool", tool = tool_name, outcome = outcome, duration_ms = duration_ms, error_kind = error_kind.map(|k| k.to_string()).as_deref().unwrap_or(""), caller = caller.map(|w| w.to_string()).as_deref().unwrap_or(""), ontology = ontology.unwrap_or(""), "REG");
}

// ── Framework-level tool execution ────────────────────────────────────────

/// Trait for MCP server types that want framework-level tool execution.
///
/// Implement this on your server struct to enable `execute_tool()`, which
/// handles Regulation span emission, error serialization, and semantic memory
/// recording automatically.
///
/// Override `record_tool_outcome` to wire daemon-based experience recording.
/// The default implementation emits a Regulation warning so the Curator knows memory
/// recording is not configured.
pub trait ToolContext {
    /// The WebID of the caller serving this tool (for Regulation span attribution).
    fn webid(&self) -> &hkask_types::WebID;

    /// Record a tool outcome to semantic memory via the daemon.
    /// Override this to wire daemon-based experience recording per Pattern D.
    /// Default: emits a Regulation warning — memory not configured for this server.
    fn record_tool_outcome(&self, tool: &str, outcome: &str) {
        tracing::warn!(target: "reg.memory", tool = %tool, outcome = %outcome,
            "Tool outcome not persisted — ToolContext::record_tool_outcome not overridden");
    }
}

/// Execute a tool with automatic Regulation span emission, error serialization,
/// and optional semantic memory recording via [`ToolContext`].
///
/// The tool's business logic goes in the `fut` async block, which returns
/// `Result<Value, McpToolError>`. The framework handles everything else.
///
/// # Example
/// ```ignore
/// #[tool(description = "...")]
/// async fn my_tool(&self, params: Parameters<MyRequest>) -> String {
///     execute_tool(self, "my_tool", async {
///         // validation...
///         // business logic...
///         Ok(serde_json::json!({"result": "success"}))
///     }).await
/// }
/// ```
#[must_use]
pub async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    fut: impl std::future::Future<Output = Result<Value, McpToolError>>,
) -> String {
    let span = ToolSpanGuard::new(tool_name, ctx.webid());
    let result = fut.await;
    match &result {
        Ok(_) => ctx.record_tool_outcome(tool_name, "success"),
        Err(_) => ctx.record_tool_outcome(tool_name, "error"),
    }
    span.finish(result)
}

/// Like `execute_tool` but tags the Regulation span with a domain ontology concept
/// (e.g. "pko:ChangeOfStatus") for type-aware feedback routing.
#[must_use]
pub async fn execute_tool_semantic<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    ontology: Option<&'static str>,
    fut: impl std::future::Future<Output = Result<Value, McpToolError>>,
) -> String {
    let mut span = ToolSpanGuard::new(tool_name, ctx.webid());
    if let Some(concept) = ontology {
        span = span.with_ontology(concept);
    }
    let result = fut.await;
    match &result {
        Ok(_) => ctx.record_tool_outcome(tool_name, "success"),
        Err(_) => ctx.record_tool_outcome(tool_name, "error"),
    }
    span.finish(result)
}

/// Record a tool outcome to the daemon for semantic memory encoding.
///
/// Standard fire-and-forget pattern used by all MCP servers that have
/// daemon access. Call this from your `ToolContext::record_tool_outcome`
/// implementation.
pub fn record_via_daemon(
    daemon: &Option<crate::daemon::DaemonClient>,
    userpod: &str,
    tool: &str,
    outcome: &str,
) {
    if let Some(daemon) = daemon.as_ref() {
        let value = serde_json::json!({
            "tool": tool,
            "outcome": outcome,
            "timestamp": now_rfc3339(),
        });
        let daemon = daemon.clone();
        let userpod = userpod.to_string();
        let tool_name = tool.to_string();
        let _outcome = outcome.to_string();
        tokio::spawn(async move {
            match daemon
                .store_experience(&userpod, "mcp_session", "observed", &value, Some(0.85))
                .await
            {
                Ok(crate::daemon::DaemonResponse::StoreResponse { stored: true, .. }) => {
                    tracing::debug!(target: "reg.memory", tool = %tool_name, "Experience stored via daemon");
                }
                Ok(other) => {
                    tracing::warn!(target: "reg.memory", tool = %tool_name, response = ?other, "Unexpected daemon response")
                }
                Err(e) => {
                    tracing::warn!(target: "reg.memory", tool = %tool_name, error = %e, "Failed to store experience");
                    tracing::warn!(target: "hkask.experience_drop", tool = %tool_name, "Regulation experience-drop signal: tool outcome not persisted to daemon");
                }
            }
        });
    } else {
        tracing::warn!(target: "reg.memory", tool = %tool, outcome = %outcome, "Experience not persisted — daemon unavailable");
    }
}

// ── Convenience helpers ────────────────────────────────────────────────────

/// Convenience: produce an internal error response for a named failed operation.
///
/// Combines `context` ("what failed") and `e` into a standard `{"error": "Failed to ...: ..."}` JSON
/// body, eliminating the repeated `span.internal_error(json!({...}))` pattern across servers.
/// Produce a JSON-RPC error response for internal tool errors.
///
/// pre:  message is non-empty
/// post: returns JSON string with error object
#[must_use]
pub fn tool_internal_error(
    span: ToolSpanGuard,
    context: &str,
    e: impl std::fmt::Display,
) -> String {
    span.internal_error(serde_json::json!({"error": format!("Failed to {context}: {e}")}))
}
