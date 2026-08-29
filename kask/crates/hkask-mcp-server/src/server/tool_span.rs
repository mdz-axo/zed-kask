//! Tool execution — Regulation span emission, experience recording, and framework-level execution.

use hkask_types::McpErrorKind;
use serde_json::Value;
use std::time::Instant;

use super::error::McpToolError;

/// RAII guard — emits Regulation tool span on drop. Use `span.ok(output)` or `span.error(kind, output)`.
pub(crate) struct ToolSpanGuard {
    tool_name: String,
    start: Instant,
    caller: hkask_types::WebID,
    emitted: bool,
    /// Domain ontology concept for type-aware feedback routing (e.g. "pko:ChangeOfStatus").
    ontology: Option<&'static str>,
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
        }
    }

    /// Tag this span with a domain ontology concept (e.g. "pko:ChangeOfStatus").
    /// The concept flows into the Regulation span for type-aware feedback routing.
    ///
    /// All hKask bridge crate constants (`hkask-bridge-ontology`,
    /// which owns DC/BIBO/PKO + all domain bridges) are valid
    /// `&'static str` concepts. This function documents the intent: `with_ontology`
    /// accepts ontology concepts, not arbitrary debug strings.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hkask_bridge_ontology::pko::STEP_EXECUTION;
    /// ToolSpanGuard::new("my_tool", &caller)
    ///     .with_ontology(STEP_EXECUTION);
    /// ```
    #[must_use]
    pub fn with_ontology(mut self, concept: &'static str) -> Self {
        self.ontology = Some(concept);
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
        output
    }

    /// Mark span as error, emit it, and return the error for propagation.
    ///
    /// post: Regulation tool span emitted with "error" status and error kind
    /// post: returns the error unchanged — callers `return Err(span.error(e))`
    #[must_use]
    pub fn error(mut self, e: McpToolError) -> McpToolError {
        self.emitted = true;
        let duration_ms = self.start.elapsed().as_millis() as u64;
        emit_tool_span(
            &self.tool_name,
            "error",
            duration_ms,
            Some(&e.kind),
            Some(&self.caller),
            self.ontology,
        );
        e
    }

    /// Finish span with Ok, serializing `value` as the MCP `{"content": ...}`
    /// tool-result envelope.
    ///
    /// post: Regulation tool span emitted with "ok" status
    /// post: returns the `{"content": value}` JSON string
    #[must_use]
    pub fn ok_json(self, value: Value) -> String {
        self.ok(
            serde_json::to_string(&serde_json::json!({"content": value})).unwrap_or_else(|e| {
                serde_json::json!({"content": format!("serialization error: {e}")}).to_string()
            }),
        )
    }

    /// Consume a `Result<Value, McpToolError>` — ok→`ok_json`, err→`error(…)`.
    /// Finish span with a Result, propagating the typed error for the wire.
    ///
    /// post: Regulation tool span emitted with appropriate status
    /// post: returns Ok(envelope string) or Err(the typed tool error —
    ///       rmcp marks the wire result `is_error` and carries the kind in
    ///       `structured_content` via the `IntoCallToolResult` impl)
    #[must_use]
    pub fn finish(self, result: Result<Value, McpToolError>) -> Result<String, McpToolError> {
        match result {
            Ok(value) => Ok(self.ok_json(value)),
            Err(e) => Err(self.error(e)),
        }
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
/// handles Regulation span emission and error serialization automatically.
///
/// The `reg.tool` span (emitted by `ToolSpanGuard`) is an observability
/// signal — it is written to the server process's stderr via `tracing`
/// (target `reg.tool`, message "REG"). It is NOT consumed by the Regulation
/// loop: zed's context-server client logs child stderr at debug level and
/// discards it. Production outcome recording happens client-side — the
/// McpRuntime dispatch path records via `CyberneticsLoop::record_outcome`
/// (`McpRuntime::invoke`), and the agent path records via
/// `agent::record_mcp_tool_outcome` (`ContextServerTool::run`, wired in
/// `main.rs`). There is no separate per-tool semantic-memory recording
/// hook; thread-level memory via `RealMemoryPort` (D6) is the richer path,
/// and per-tool debug logging is available via `tracing::debug!` at the
/// call site if a server needs it.
pub trait ToolContext {
    /// The WebID of the caller serving this tool (for Regulation span attribution).
    fn webid(&self) -> &hkask_types::WebID;
}

/// Execute a tool with automatic Regulation span emission and error serialization.
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
    // The span layer propagates the typed error; this boundary keeps the
    // historical `String` tool signature (every server's `#[tool]` method),
    // mapping an error to the `{"error", "kind"}` envelope the client
    // already parses (`hkask_types::tool_response::parse_tool_error`).
    match span.finish(result) {
        Ok(output) => output,
        Err(e) => e.to_json_string(),
    }
}

/// Like `execute_tool` but tags the Regulation span with a domain ontology concept
/// (e.g. "pko:ChangeOfStatus") for type-aware feedback routing.
///
/// When `ontology` is `None`, emits a `tracing::warn!` naming the tool — the
/// algedonic signal that a registered tool lacks an ontology anchor. This
/// opens the S1→S5 feedback channel: a missing anchor is visible at runtime
/// rather than silently producing an untagged span.
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
    } else {
        tracing::warn!(
            target: "hkask.mcp.ontology",
            tool = %tool_name,
            "execute_tool_semantic called with None ontology for tool '{}'; \
             add an arm to the server's ontology_anchor fn",
            tool_name
        );
    }
    let result = fut.await;
    match span.finish(result) {
        Ok(output) => output,
        Err(e) => e.to_json_string(),
    }
}
