//! Tool execution — Regulation span emission, experience recording, and framework-level execution.

use hkask_types::McpErrorKind;
use serde_json::Value;
use std::time::Instant;

use super::error::McpToolError;
use super::http_helpers::McpToolOutput;

/// RAII guard — emits Regulation tool span on drop. Use `span.ok(output)` or `span.error(kind, output)`.
pub struct ToolSpanGuard {
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
/// The `reg.tool` span (emitted by `ToolSpanGuard`) is the production
/// recording surface — it carries tool name, outcome, duration, and caller
/// to the Regulation loop. There is no separate per-tool semantic-memory
/// recording hook; thread-level memory via `RealMemoryPort` (D6) is the
/// richer path, and per-tool debug logging is available via `tracing::debug!`
/// at the call site if a server needs it.
pub trait ToolContext {
    /// The WebID of the caller serving this tool (for Regulation span attribution).
    fn webid(&self) -> &hkask_types::WebID;

    /// The central grounding ledger. Every MCP server carries one —
    /// core-constructed and macro-injected via `mcp_server!`. This is the
    /// unscrambling of the per-server opt-in: grounding is a core capability,
    /// not a leaf-server opt-in. `execute_tool_semantic` calls
    /// `enforce_for_agent` on every tool output through this store.
    fn verification_store(&self) -> &std::sync::Arc<hkask_verification::VerificationStore>;
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
    span.finish(result)
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
    // ── Grounding (Rung 3) ──────────────────────────────────────────
    //
    // Every tool output in every MCP server is grounded through the
    // server's `VerificationStore`. This is the core capability — not a
    // per-server opt-in. The tool name is the contract key; the tool's
    // output JSON is the document; `tool_calls` is empty (the core doesn't
    // have tool-call visibility — the floor is contract-keyed grounding
    // without value-matching; delegating servers keep the richer
    // `enforce_and_stamp` with real `tool_calls`).
    //
    // A tool with no registered contract gets a coverage-gap record
    // (had_contract: false) — the trend query sees the gap, the operator
    // knows which tools need contracts. Silence must not read as a verdict.
    let grounded = match result {
        Ok(value) => {
            let response_str = value.to_string();
            let (_grounding_result, cleaned) = ctx.verification_store().enforce_for_agent(
                // Core-level grounding: the source is prefixed with `core:`
                // so trend queries can distinguish it from the tool's own
                // `enforce_and_stamp` call (which uses the tool name
                // directly). Both records are legitimate — the core grounds
                // every tool output (the floor), and the tool's own call
                // grounds with `tool_calls` visibility (the ceiling).
                &format!("core:{tool_name}"),
                tool_name,
                tool_name,
                &value,
                &[],
                &response_str,
                &[],
            );
            Ok(cleaned)
        }
        Err(e) => Err(e),
    };
    span.finish(grounded)
}
