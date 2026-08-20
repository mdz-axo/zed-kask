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
    #[serde(default)]
    env: Value,
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
