use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ui::SharedString;

/// Evaluate a deterministic Lisp form against a JSON environment.
///
/// This tool provides the deterministic computation layer for skill processes.
/// Use it to check structural invariants, compute convergence signals, and
/// perform arithmetic on structured data that the LLM cannot reliably
/// self-evaluate (counting its own outputs, verifying field presence, scoring).
///
/// The interpreter is sandboxed: no I/O, no `eval`, no `load`, no network.
/// Evaluation is bounded by `max_steps` and `max_depth` to prevent infinite loops.
///
/// JSON objects become association lists — use `(assoc "key" alist)` to access
/// fields. The result is returned as JSON.
///
/// Example: count open threats from a prior step's output:
/// ```text
/// form: "(+ (assoc \"confirmed_bugs\" (assoc \"summary\" step_5_result)) (assoc \"potential_bugs\" (assoc \"summary\" step_5_result)))"
/// env: { "step_5_result": { "summary": { "confirmed_bugs": 3, "potential_bugs": 2 } } }
/// ```
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LispEvalToolInput {
    /// The Lisp form to evaluate. Supports: `quote`, `if`, `let`, `lambda`,
    /// `define`, `begin`, `and`, `or`, `not`, `cond`, arithmetic (`+`, `-`,
    /// `*`, `/`, `<`, `>`, `<=`, `>=`, `=`), `assoc`, `length`, `map`,
    /// `filter`, `reduce`, `string-append`, `string->number`, and more.
    form: String,
    /// JSON object whose keys become top-level Lisp bindings. Values are
    /// converted to Lisp values: objects become association lists, arrays
    /// become lists, numbers stay numbers, strings stay strings.
    ///
    /// Uses `AnyJsonValue` (not `serde_json::Value`) because `schemars`
    /// renders `Value` as bare `true`, which breaks strict-schema providers
    /// (Ollama, Gemini) — the env parameter silently doesn't arrive.
    #[serde(default)]
    env: hkask_types::AnyJsonValue,
    /// Maximum evaluation steps (default 100000). Prevents infinite loops.
    #[serde(default = "default_max_steps")]
    max_steps: u64,
    /// Maximum evaluation depth (default 64). Prevents infinite recursion.
    #[serde(default = "default_max_depth")]
    max_depth: u64,
}

fn default_max_steps() -> u64 {
    100000
}

fn default_max_depth() -> u64 {
    64
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LispEvalToolOutput {
    Success { result: Value },
    Error { error: String },
}

impl From<LispEvalToolOutput> for LanguageModelToolResultContent {
    fn from(value: LispEvalToolOutput) -> Self {
        match value {
            LispEvalToolOutput::Success { result } => serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| "null".into())
                .into(),
            LispEvalToolOutput::Error { error } => error.into(),
        }
    }
}

pub struct LispEvalTool;

impl AgentTool for LispEvalTool {
    type Input = LispEvalToolInput;
    type Output = LispEvalToolOutput;

    const NAME: &'static str = "lisp_eval";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => {
                let form = input.form.chars().take(80).collect::<String>();
                format!("Evaluating: {form}").into()
            }
            Err(_) => "Lisp Evaluation".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_cx| {
            let input = input.recv().await.map_err(|e| LispEvalToolOutput::Error {
                error: format!("failed to receive input: {e}"),
            })?;

            let result = hkask_lisp::eval_sandboxed_with_budget(
                &input.form,
                &input.env,
                input.max_steps,
                input.max_depth,
            )
            .map_err(|e| LispEvalToolOutput::Error {
                error: e.to_string(),
            })?;

            Ok(LispEvalToolOutput::Success { result })
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    // The tool dispatches to `hkask_lisp::eval_sandboxed_with_budget` — these
    // tests exercise the interpreter directly along the exact code path the
    // tool uses (same function, same arguments). They verify the skill
    // convergence patterns that `lisp_eval` is designed to support.
    //
    // NOTE: The `env` field was previously `serde_json::Value`, whose
    // `JsonSchema` renders as bare `true` — strict-schema providers silently
    // dropped the parameter, causing "unbound symbol" errors for any form
    // referencing env variables. Fixed by changing to `AnyJsonValue` (schema
    // emits `{}`). These tests verify the interpreter itself is correct; the
    // tool wrapper fix needs a process rebuild to take effect live.

    #[test]
    fn test_interp_assoc_access_on_json_object() {
        let result = hkask_lisp::eval_sandboxed_with_budget(
            r#"(assoc "count" step_result)"#,
            &json!({ "step_result": { "count": 42, "status": "complete" } }),
            100000,
            64,
        );
        assert!(result.is_ok(), "assoc should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(42));
    }

    #[test]
    fn test_interp_length_on_list() {
        let result = hkask_lisp::eval_sandboxed_with_budget(
            "(length items)",
            &json!({ "items": [1, 2, 3, 4, 5] }),
            100000,
            64,
        );
        assert!(result.is_ok(), "length should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(5));
    }

    #[test]
    fn test_interp_compound_convergence_pattern() {
        // The convergence pattern from the enhanced prompt: extract chunk
        // count, guard against zero, return the count.
        let form = r#"(let ((chunks (assoc "chunks" embed_result))) (if (> chunks 0) chunks 0))"#;
        let result = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({ "embed_result": { "chunks": 127, "model": "text-embedding-3-small" } }),
            100000,
            64,
        );
        assert!(result.is_ok(), "compound form should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(127));
    }

    #[test]
    fn test_interp_zero_count_guard() {
        // Same form, but chunks is 0 — the guard should return 0, not error.
        let form = r#"(let ((chunks (assoc "chunks" embed_result))) (if (> chunks 0) chunks 0))"#;
        let result = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({ "embed_result": { "chunks": 0 } }),
            100000,
            64,
        );
        assert!(result.is_ok(), "zero-count guard should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(0));
    }

    #[test]
    fn test_interp_arithmetic_literal() {
        // The form that was confirmed working via the tool earlier.
        let result = hkask_lisp::eval_sandboxed_with_budget(
            "(+ 1 2 3)",
            &json!({}),
            100000,
            64,
        );
        assert!(result.is_ok(), "arithmetic should succeed: {:?}", result.err());
        assert_eq!(result.expect("checked is_ok above"), json!(6));
    }
}
