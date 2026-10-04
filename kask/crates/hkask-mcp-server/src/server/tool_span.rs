//! Tool execution — Regulation span emission, experience recording, and framework-level execution.

use hkask_types::McpErrorKind;
use hkask_types::ports::InferenceUsage;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::error::McpToolError;

// ── Per-call inference usage recording (mcp-tool-review S-01) ───────────

/// One inference call's usage, recorded by a tool handler for the envelope.
#[derive(Debug, Clone)]
pub(crate) struct RecordedUsage {
    usage: InferenceUsage,
    cost_usd: Option<f64>,
}

tokio::task_local! {
    /// Per-tool-call usage accumulator. `execute_tool` scopes this around
    /// the handler future; handlers record each inference-mediated call
    /// via [`record_tool_usage`], and the tool-result envelope aggregates
    /// the records into its `usage` field.
    static TOOL_USAGE: Arc<Mutex<Vec<RecordedUsage>>>;
}

/// Record an inference call's token usage into the currently executing
/// tool's result envelope (mcp-tool-review S-01 token half). Call from a
/// tool handler after each inference-port call, passing the
/// `InferenceResult`'s `usage` and `cost_usd`.
///
/// Outside an `execute_tool` scope there is no destination — the record
/// is dropped with a debug log (there is no error to propagate, only no
/// envelope to carry it).
pub fn record_tool_usage(usage: InferenceUsage, cost_usd: Option<f64>) {
    let outcome = TOOL_USAGE.try_with(|cell| {
        cell.lock()
            .map(|mut records| records.push(RecordedUsage { usage, cost_usd }))
            .map_err(|poisoned| format!("usage recorder mutex poisoned: {poisoned}"))
    });
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(poisoned)) => tracing::debug!("{poisoned}"),
        Err(_no_scope) => {
            tracing::debug!("record_tool_usage outside a tool span — record dropped");
        }
    }
}

/// Aggregate recorded inference usage for the envelope. Empty records →
/// `null` — the explicit absent-note for deterministic tools (S-01: never
/// silence; absence is stated, not implied by a missing field).
fn usage_json(records: &[RecordedUsage]) -> Value {
    if records.is_empty() {
        return Value::Null;
    }
    let prompt_tokens: u32 = records.iter().map(|r| r.usage.prompt_tokens).sum();
    let completion_tokens: u32 = records.iter().map(|r| r.usage.completion_tokens).sum();
    let total_tokens: u32 = records.iter().map(|r| r.usage.total_tokens).sum();
    let cost_usd = {
        let reported: Vec<f64> = records.iter().filter_map(|r| r.cost_usd).collect();
        if reported.is_empty() {
            None
        } else {
            Some(reported.iter().sum::<f64>())
        }
    };
    serde_json::json!({
        "calls": records.len(),
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
        // true only when every recorded call actually reported usage — a
        // provider that omits the wire field must not read as zero tokens
        // (InferenceUsage::reported, D20).
        "reported": records.iter().all(|r| r.usage.reported),
        "cost_usd": cost_usd,
    })
}

/// RAII guard — emits Regulation tool span on drop. Use `span.ok(output)` or `span.error(kind, output)`.
pub(crate) struct ToolSpanGuard {
    tool_name: String,
    start: Instant,
    caller: hkask_types::WebID,
    emitted: bool,
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
        }
    }

    /// Mark span as successful and return output.
    ///
    /// post: Regulation tool span emitted with "ok" status
    /// post: returns output unchanged
    #[must_use]
    pub fn ok(mut self, output: String) -> String {
        self.emitted = true;
        let duration_ms = self.start.elapsed().as_millis() as u64;
        emit_tool_span(&self.tool_name, "ok", duration_ms, None, Some(&self.caller));
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
        );
        e
    }

    /// Finish span with Ok, serializing `value` as the MCP tool-result
    /// envelope. The envelope carries `duration_ms` and `usage` alongside
    /// `content` — the measurements the caller-side efficiency axis needs
    /// (mcp-tool-review S-01): duration previously lived only in a stderr
    /// tracing span that zed discards, and billed tokens lived only in the
    /// inference port's return value, which the envelope never saw.
    /// `usage` is `null` when the tool execution made no inference calls
    /// (deterministic tools) and an aggregated object when handlers
    /// recorded via [`record_tool_usage`]. Consumers unwrap `content` via
    /// `unwrap_tool_envelope`, which ignores the extra fields.
    ///
    /// post: Regulation tool span emitted with "ok" status
    /// post: returns the `{"content": value, "duration_ms": N, "usage": …}`
    ///       JSON string
    #[must_use]
    pub fn ok_json(self, value: Value, usage: &[RecordedUsage]) -> String {
        let duration_ms = self.start.elapsed().as_millis() as u64;
        self.ok(serde_json::to_string(&serde_json::json!({
            "content": value,
            "duration_ms": duration_ms,
            "usage": usage_json(usage),
        }))
        .unwrap_or_else(|e| {
            serde_json::json!({
                "content": format!("serialization error: {e}"),
                "duration_ms": duration_ms,
                "usage": usage_json(usage),
            })
            .to_string()
        }))
    }

    /// Consume a `Result<Value, McpToolError>` — ok→`ok_json`, err→`error(…)`.
    /// Finish span with a Result, propagating the typed error for the wire.
    /// Usage recorded before an error is not carried on the error wire —
    /// the typed error is the signal there; the ok path carries usage.
    ///
    /// post: Regulation tool span emitted with appropriate status
    /// post: returns Ok(envelope string) or Err(the typed tool error —
    ///       rmcp marks the wire result `is_error` and carries the kind in
    ///       `structured_content` via the `IntoCallToolResult` impl)
    #[must_use]
    pub fn finish(
        self,
        result: Result<Value, McpToolError>,
        usage: &[RecordedUsage],
    ) -> Result<String, McpToolError> {
        match result {
            Ok(value) => Ok(self.ok_json(value, usage)),
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
) {
    tracing::info!(target: "reg.tool", tool = tool_name, outcome = outcome, duration_ms = duration_ms, error_kind = error_kind.map(|k| k.to_string()).as_deref().unwrap_or(""), caller = caller.map(|w| w.to_string()).as_deref().unwrap_or(""), "REG");
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
) -> Result<String, McpToolError> {
    let span = ToolSpanGuard::new(tool_name, ctx.webid());
    // Scope the per-call usage accumulator around the handler future so
    // `record_tool_usage` calls inside the handler land in THIS call's
    // envelope (per-call isolation: concurrent tool calls never mix).
    let records = Arc::new(Mutex::new(Vec::<RecordedUsage>::new()));
    let result = TOOL_USAGE.scope(records.clone(), fut).await;
    let drained = match records.lock() {
        Ok(mut guarded) => std::mem::take(&mut *guarded),
        Err(poisoned) => {
            tracing::debug!("tool usage accumulator poisoned: {poisoned}");
            Vec::new()
        }
    };
    span.finish(result, &drained)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S-01 pin (mcp-tool-review): the tool-result envelope carries
    /// `duration_ms` alongside `content` — the caller-side efficiency axis
    /// depends on it, and `unwrap_tool_envelope` ignores the extra field.
    #[test]
    fn ok_json_envelope_carries_duration_ms() {
        let guard = ToolSpanGuard::new("pin-test", &hkask_types::WebID::new());
        let output = guard.ok_json(serde_json::json!({"x": 1}), &[]);
        let value: Value = serde_json::from_str(&output).expect("envelope is valid JSON");
        assert!(
            value.get("content").is_some(),
            "the envelope still carries content: {output}"
        );
        assert!(
            value.get("duration_ms").and_then(Value::as_u64).is_some(),
            "the envelope carries a numeric duration_ms: {output}"
        );
    }

    /// S-01 pin (token half): no recorded inference calls → an explicit
    /// `usage: null` absent-note in the envelope, never a missing field —
    /// deterministic tools state their absence.
    #[test]
    fn ok_json_envelope_usage_null_without_records() {
        let guard = ToolSpanGuard::new("pin-test", &hkask_types::WebID::new());
        let output = guard.ok_json(serde_json::json!({"x": 1}), &[]);
        let value: Value = serde_json::from_str(&output).expect("envelope is valid JSON");
        assert!(
            value.get("usage").is_some_and(Value::is_null),
            "usage is an explicit null absent-note: {output}"
        );
    }

    /// S-01 pin (token half): recorded inference usage aggregates into the
    /// envelope's `usage` object — call count, summed tokens, the reported
    /// flag, and summed cost over the calls that reported one.
    #[tokio::test]
    async fn ok_json_envelope_aggregates_recorded_usage() {
        let records = Arc::new(Mutex::new(Vec::<RecordedUsage>::new()));
        TOOL_USAGE
            .scope(records.clone(), async {
                record_tool_usage(
                    InferenceUsage {
                        prompt_tokens: 10,
                        completion_tokens: 5,
                        total_tokens: 15,
                        reported: true,
                    },
                    Some(0.01),
                );
                record_tool_usage(
                    InferenceUsage {
                        prompt_tokens: 7,
                        completion_tokens: 3,
                        total_tokens: 10,
                        reported: true,
                    },
                    None,
                );
            })
            .await;
        let drained = records.lock().expect("accumulator unpoisoned").clone();
        let guard = ToolSpanGuard::new("pin-test", &hkask_types::WebID::new());
        let output = guard.ok_json(serde_json::json!({"x": 1}), &drained);
        let value: Value = serde_json::from_str(&output).expect("envelope is valid JSON");
        let usage = value.get("usage").expect("usage object present");
        assert_eq!(usage.get("calls").and_then(Value::as_u64), Some(2));
        assert_eq!(usage.get("prompt_tokens").and_then(Value::as_u64), Some(17));
        assert_eq!(
            usage.get("completion_tokens").and_then(Value::as_u64),
            Some(8)
        );
        assert_eq!(usage.get("total_tokens").and_then(Value::as_u64), Some(25));
        assert_eq!(usage.get("reported").and_then(Value::as_bool), Some(true));
        assert_eq!(usage.get("cost_usd").and_then(Value::as_f64), Some(0.01));
    }

    /// S-01 pin: a handler that records usage inside `execute_tool`'s
    /// future has it aggregated into its own envelope — the accumulator
    /// is scoped per call, so the wiring (scope → drain → finish) is
    /// exercised end-to-end, not just the aggregation helper.
    #[tokio::test]
    async fn execute_tool_carries_handler_recorded_usage() {
        struct TestCtx(hkask_types::WebID);
        impl ToolContext for TestCtx {
            fn webid(&self) -> &hkask_types::WebID {
                &self.0
            }
        }
        let ctx = TestCtx(hkask_types::WebID::new());
        let output = execute_tool(&ctx, "pin-test", async {
            record_tool_usage(
                InferenceUsage {
                    prompt_tokens: 100,
                    completion_tokens: 40,
                    total_tokens: 140,
                    reported: true,
                },
                Some(0.02),
            );
            Ok(serde_json::json!({"ok": true}))
        })
        .await
        .expect("tool call succeeds");
        let value: Value = serde_json::from_str(&output).expect("envelope is valid JSON");
        let usage = value.get("usage").expect("usage object present");
        assert_eq!(usage.get("calls").and_then(Value::as_u64), Some(1));
        assert_eq!(usage.get("total_tokens").and_then(Value::as_u64), Some(140));
    }
}
