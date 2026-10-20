//! Shared MCP response boundary: elapsed time, isolated inference usage, and typed errors.

use hkask_types::ports::InferenceUsage;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::error::McpToolError;

struct RecordedUsage {
    usage: InferenceUsage,
    cost_usd: Option<f64>,
}

tokio::task_local! {
    static TOOL_USAGE: Arc<Mutex<Vec<RecordedUsage>>>;
}

/// Record inference usage for the current tool call's response envelope.
/// Outside an execution scope there is no response to attach it to.
pub fn record_tool_usage(usage: InferenceUsage, cost_usd: Option<f64>) {
    match TOOL_USAGE.try_with(|records| {
        records
            .lock()
            .map(|mut records| records.push(RecordedUsage { usage, cost_usd }))
            .map_err(|error| error.to_string())
    }) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "Tool usage recorder poisoned"),
        Err(_) => {
            tracing::debug!("record_tool_usage outside tool execution — no response envelope")
        }
    }
}

fn usage_json(records: &[RecordedUsage]) -> Value {
    if records.is_empty() {
        return Value::Null;
    }
    let prompt_tokens: u64 = records
        .iter()
        .map(|record| u64::from(record.usage.prompt_tokens))
        .sum();
    let completion_tokens: u64 = records
        .iter()
        .map(|record| u64::from(record.usage.completion_tokens))
        .sum();
    let total_tokens: u64 = records
        .iter()
        .map(|record| u64::from(record.usage.total_tokens))
        .sum();
    let costs: Vec<f64> = records
        .iter()
        .filter_map(|record| record.cost_usd)
        .collect();
    let cost_usd = (!costs.is_empty()).then(|| costs.iter().sum::<f64>());
    serde_json::json!({
        "calls": records.len(),
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
        "reported": records.iter().all(|record| record.usage.reported),
        "cost_usd": cost_usd,
    })
}

/// Server-local response annotation used at the shared tool boundary.
/// Identity and outcomes are recorded by the caller-side runtime, not a child-side guard.
pub trait ToolContext {
    fn annotate_response(&self, _tool_name: &str, body: Value) -> Value {
        body
    }
}

/// Run a handler with isolated usage accounting and preserve its typed error.
/// Successful responses carry content, elapsed milliseconds, and usage (null
/// when the handler recorded no inference). Annotation applies exactly once.
pub async fn execute_tool<C: ToolContext>(
    ctx: &C,
    tool_name: &str,
    fut: impl std::future::Future<Output = Result<Value, McpToolError>>,
) -> Result<String, McpToolError> {
    let start = Instant::now();
    let records = Arc::new(Mutex::new(Vec::new()));
    let result = TOOL_USAGE.scope(Arc::clone(&records), fut).await;
    let body = ctx.annotate_response(tool_name, result?);
    let records = records.lock().map_err(|error| {
        McpToolError::internal(format!("Tool usage recorder poisoned: {error}"))
    })?;
    Ok(serde_json::json!({
        "content": body,
        "duration_ms": start.elapsed().as_millis() as u64,
        "usage": usage_json(&records),
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Context;

    impl ToolContext for Context {
        fn annotate_response(&self, tool_name: &str, mut body: Value) -> Value {
            body["annotated_by"] = tool_name.into();
            body
        }
    }

    fn usage(tokens: u32, reported: bool) -> InferenceUsage {
        InferenceUsage {
            prompt_tokens: tokens,
            completion_tokens: 2,
            total_tokens: tokens + 2,
            reported,
        }
    }

    #[tokio::test]
    async fn success_envelope_preserves_content_duration_and_null_usage() {
        let context = Context;
        let output = execute_tool(&context, "example", async {
            Ok(serde_json::json!({"message": "short output"}))
        })
        .await
        .expect("tool succeeds");
        let value: Value = serde_json::from_str(&output).expect("response JSON");
        assert_eq!(value["content"]["message"], "short output");
        assert_eq!(value["content"]["annotated_by"], "example");
        assert!(value["duration_ms"].as_u64().is_some());
        assert!(value.get("usage").is_some_and(Value::is_null));
    }

    #[tokio::test]
    async fn typed_error_is_not_wrapped_as_success_content() {
        let context = Context;
        let error = execute_tool(&context, "missing", async {
            Err(McpToolError::not_found("fixture not found"))
        })
        .await
        .expect_err("typed error propagates");
        assert_eq!(error.kind, hkask_types::McpErrorKind::NotFound);
        assert_eq!(error.message, "fixture not found");
    }

    #[tokio::test]
    async fn concurrent_calls_do_not_mix_recorded_usage() {
        let context = Context;
        let (first, second) = tokio::join!(
            execute_tool(&context, "first", async {
                record_tool_usage(usage(10, true), Some(0.01));
                tokio::task::yield_now().await;
                record_tool_usage(usage(7, false), None);
                Ok(serde_json::json!({}))
            }),
            execute_tool(&context, "second", async {
                tokio::task::yield_now().await;
                record_tool_usage(usage(3, true), None);
                Ok(serde_json::json!({}))
            }),
        );
        let first: Value = serde_json::from_str(&first.expect("first succeeds")).expect("JSON");
        let second: Value = serde_json::from_str(&second.expect("second succeeds")).expect("JSON");
        assert_eq!(first["usage"]["calls"], 2);
        assert_eq!(first["usage"]["prompt_tokens"], 17);
        assert_eq!(first["usage"]["completion_tokens"], 4);
        assert_eq!(first["usage"]["total_tokens"], 21);
        assert_eq!(first["usage"]["reported"], false);
        assert_eq!(first["usage"]["cost_usd"], 0.01);
        assert_eq!(second["usage"]["calls"], 1);
        assert_eq!(second["usage"]["total_tokens"], 5);
        assert!(second["usage"]["cost_usd"].is_null());
    }
}
