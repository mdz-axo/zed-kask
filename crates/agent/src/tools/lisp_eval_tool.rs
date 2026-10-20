use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v2 as acp;
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
/// env: "{\"step_5_result\":{\"summary\":{\"confirmed_bugs\":3,\"potential_bugs\":2}}}"
/// ```
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LispEvalToolInput {
    /// The Lisp form to evaluate. Special forms: `quote`, `if`, `let`,
    /// `lambda`, `define`, `begin`, `and`, `or`, `not`, `cond`. Builtins:
    /// arithmetic (`+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`),
    /// `car`, `cdr`, `cons`, `list`, `length`, `nth`, `reverse`, `is_null`,
    /// `numberp`, `listp`, `stringp`, `assoc`, `assoc!`, `append`, `member`, `abs`,
    /// `sqrt`, `max`, `min`, `eq`, `string=`, `string-contains`,
    /// `starts-with`, `ends-with`, `concat`, `boundary-ok`.
    /// Integer arithmetic is checked (overflow errors, never wraps silently)
    /// and all-integer comparisons are exact (never coerced through f64).
    /// `/` always returns a Float (`(/ 6 3)` → `2.0`); over 3+ args `!=`
    /// compares adjacent pairs. Bare infix is accepted for the arithmetic
    /// and comparison operators (`a + b` → `(+ a b)`; same-operator chains
    /// fold; mixed operators do not associate; a parenthesized `(a + b)`
    /// is NOT equivalent — it errors).
    #[serde(deserialize_with = "deserialize_form_field")]
    form: String,
    /// JSON text encoding an object whose keys become Lisp bindings, for
    /// example `"{\"a\":1,\"b\":true}"`. Values retain their JSON types:
    /// objects become association lists, arrays become lists, numbers stay
    /// numbers and strings stay strings. Do not wrap scalars in objects.
    /// Malformed JSON or a non-object document is rejected before evaluation.
    #[serde(
        default,
        deserialize_with = "deserialize_env_field",
        serialize_with = "hkask_types::tool_schema::serialize_json_text"
    )]
    #[schemars(schema_with = "hkask_types::tool_schema::json_text_schema")]
    env: std::collections::HashMap<String, serde_json::Value>,
    /// Maximum evaluation steps (default 100000). Prevents infinite loops.
    /// The `maxSteps` alias tolerates camelCase emissions so an explicit budget
    /// is applied rather than silently dropped (a silently-dropped budget was
    /// an unteachable failure: the eval failed with the DEFAULT limit while
    /// the model believed it had raised it).
    #[serde(
        default = "default_max_steps",
        alias = "maxSteps",
        deserialize_with = "deserialize_max_steps_field"
    )]
    max_steps: u64,
    /// Maximum evaluation depth (default 1024). Prevents infinite recursion.
    /// Recursive helper forms over lists consume roughly 2–4 depth frames
    /// per element, so the former default of 64 overflowed at ~16 elements —
    /// real-scale validation lists (100+ claims) failed on the first attempt
    /// and wasted turns on retries (observed live: a 134-element list needed
    /// 300). 1024 covers realistic registries out of the box; genuinely
    /// infinite recursion still trips the budget immediately.
    #[serde(
        default = "default_max_depth",
        alias = "maxDepth",
        deserialize_with = "deserialize_max_depth_field"
    )]
    max_depth: u64,
}

// Per-field teaching deserializers: each names its field, the received shape,
// and the accepted shape on rejection (lisp-repair L1). An error that does
// not teach produces identical retries — the 2026-09-28 audit lockout
// anatomy: 5 identical malformed-env emissions hard-refused by the per-input
// retry tracker.

fn deserialize_form_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "form",
        "a string containing the Lisp form, e.g. \"(+ 1 2)\"",
        false,
    )
}

fn deserialize_env_field<'de, D>(
    deserializer: D,
) -> Result<std::collections::HashMap<String, serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    hkask_types::tool_schema::deserialize_json_text_object(deserializer, "env")
}

fn deserialize_max_steps_field<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "max_steps",
        "a non-negative integer (default 100000)",
        false,
    )
}

fn deserialize_max_depth_field<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "max_depth",
        "a non-negative integer (default 1024)",
        false,
    )
}

fn default_max_steps() -> u64 {
    100000
}

fn default_max_depth() -> u64 {
    // 1024, not 64: recursive helpers consume 2–4 depth frames per list
    // element, so 64 overflowed at ~16 elements and real validation lists
    // (100+) failed their first attempt (observed: 134 elements needed 300).
    // Infinite recursion still trips this immediately — the budget is a
    // guard, not a workload ceiling.
    1024
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

    fn deserialize_replay_input(input: serde_json::Value) -> Result<Self::Input> {
        super::deserialize_saved_json_map(input, "env")
    }

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
            evaluate_lisp(input)
                .map(|result| LispEvalToolOutput::Success { result })
                .map_err(|error| LispEvalToolOutput::Error { error })
        })
    }
}

/// The tool's evaluation, shared with delegated `host/lisp_eval` dispatch.
pub fn evaluate_lisp(input: LispEvalToolInput) -> Result<Value, String> {
    // Sorted for byte-identical error messages across processes (HashMap
    // iteration order is process-randomized — the L2 canonicalization rule
    // applied to the error path too).
    let mut env_bindings: Vec<String> = input.env.keys().cloned().collect();
    env_bindings.sort();
    let nested_env = input
        .env
        .get("env")
        .is_some_and(|value| value.as_object().is_some());
    // Bindings whose value is a JSON object — the wrapped-scalar signature.
    // An object becomes an association list, so a scalar wrapped as
    // `{"n": 1}` reaches a scalar position as a list and fails with
    // "got list" — an error that (pre-L3) named neither the binding nor
    // the fix. Observed live 2026-10-02: three identical retries across two
    // lisp_eval gate forms, including one under explicit deliberation to
    // emit bare scalars — the wrapping is deterministic for this emitter,
    // so the error must teach on the FIRST failure (lisp-repair L1).
    let mut object_bindings: Vec<String> = input
        .env
        .iter()
        .filter(|(_, value)| value.as_object().is_some())
        .map(|(key, _)| key.clone())
        .collect();
    object_bindings.sort();
    let env_value = {
        // Canonical order: env keys are sorted before the object is built, so
        // the boundary is deterministic across processes (HashMap iteration
        // order is process-randomized; preserve_order then freezes whatever
        // order arrives first). No form can observe top-level key order today
        // (each key becomes an individual binding — proven lisp-repair L0), so
        // this is defense-in-depth for any future env-introspection surface;
        // the error path sorts independently (bindings list).
        let mut pairs: Vec<(String, serde_json::Value)> = input.env.into_iter().collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        serde_json::Value::Object(pairs.into_iter().collect())
    };
    hkask_lisp::eval_sandboxed_with_budget(
        &input.form,
        &env_value,
        input.max_steps,
        input.max_depth,
    )
    .map_err(|error| match error {
        // The unbound-symbol error teaches: it names the available bindings,
        // so a model that nested env one level too deep (the unteachable class
        // — the emission deserialized cleanly, the error never named the
        // nesting) sees the stray "env" binding and the flatten fix.
        hkask_lisp::LispError::UnboundSymbol(name) => {
            let mut message = format!("unbound symbol: {name} — env bindings: {env_bindings:?}");
            if nested_env {
                message.push_str(
                    " (an env binding named \"env\" was received — if env was nested one \
                     level too deep, flatten it: {\"a\": 1}, not {\"env\": {\"a\": 1}})",
                );
            }
            message
        }
        // The type error teaches when the wrapped-scalar signature is
        // present: an object-valued binding reached a scalar position
        // (surfacing as "got list"). Names the suspects and the fix so the
        // first error changes the next emission; a type error without
        // object bindings (or with a non-list actual — the objects are not
        // the culprit) keeps the plain message.
        hkask_lisp::LispError::TypeError { expected, actual }
            if actual == "list" && !object_bindings.is_empty() =>
        {
            format!(
                "type error: expected {expected}, got {actual} — env bindings \
                 carrying objects (objects become association lists): \
                 {object_bindings:?}. If you wrapped a scalar in an object, \
                 pass the scalar directly: {{\"a\": 1}}, not {{\"a\": {{\"n\": 1}}}}"
            )
        }
        hkask_lisp::LispError::TypeError { expected, actual }
            if expected == "number" && actual == "nil" =>
        {
            "type error: expected number, got nil (if this value came from \
             (assoc \"key\" obj), the key is missing or null — use (assoc! \
             \"key\" obj) to list available keys, or guard with (is_null ...))"
                .to_string()
        }
        hkask_lisp::LispError::TypeError { expected, actual }
            if expected == "list"
                && actual != "list"
                && (input.form.contains("(assoc ") || input.form.contains("(assoc! "))
                && (input.form.contains("(cdr ") || input.form.contains("(car ")) =>
        {
            format!(
                "type error: expected {expected}, got {actual} (note: in lisp_eval, \
                 (assoc key obj) returns the value directly, not a (key . value) \
                 cons pair — omit (cdr ...) / (car ...) around assoc)"
            )
        }
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::LispEvalToolInput;
    use serde_json::json;

    #[test]
    fn env_wire_schema_and_decoder_preserve_scalar_bindings() {
        let schema =
            serde_json::to_value(schemars::schema_for!(LispEvalToolInput)).expect("schema");
        assert_eq!(schema["properties"]["env"]["type"], "string");
        assert!(hkask_types::find_boolean_schema_positions(&schema).is_empty());
        let input: LispEvalToolInput = serde_json::from_value(json!({
            "form": "(+ count 1)",
            "env": json!({"count": 7, "literal": "{\"not\":\"an object\"}"}).to_string()
        }))
        .expect("JSON-text environment");
        assert_eq!(input.env["count"], json!(7));
        assert!(input.env["literal"].is_string());
        let wire = serde_json::to_value(&input).expect("serialize native input");
        assert!(wire["env"].is_string());
        assert_eq!(
            super::evaluate_lisp(input).expect("evaluate decoded bindings"),
            json!(8)
        );
    }

    #[test]
    fn saved_env_migration_preserves_bindings_without_relaxing_live_inputs() {
        use crate::AgentTool;

        let bindings = json!({
            "count": 7,
            "ready": true,
            "items": [1, null, {"nested": 2}],
            "literal": "{\"not\":\"an object\"}"
        });
        let saved = json!({"form": "(+ count 1)", "env": bindings});
        assert!(serde_json::from_value::<LispEvalToolInput>(saved.clone()).is_err());
        for env in [bindings.clone(), json!(bindings.to_string())] {
            let mut input = saved.clone();
            input["env"] = env;
            let decoded = super::LispEvalTool::deserialize_replay_input(input)
                .expect("decode saved environment");
            assert_eq!(
                serde_json::to_value(&decoded.env).expect("bindings"),
                bindings
            );
            assert_eq!(
                super::evaluate_lisp(decoded).expect("scalar arithmetic"),
                json!(8)
            );
        }
        let omitted = super::LispEvalTool::deserialize_replay_input(json!({"form": "(+ 1 2)"}))
            .expect("omitted environment keeps its default");
        assert_eq!(
            super::evaluate_lisp(omitted).expect("constant arithmetic"),
            json!(3)
        );
    }

    // The following interpreter tests cover evaluation independently of the
    // model-facing JSON-text transport; the wire round-trip is tested above.

    #[test]
    fn default_max_depth_covers_real_scale_lists() {
        // The regression pin for the 64→1024 raise: deserializing a call that
        // omits max_depth must yield the raised default — the observed live
        // case (a 134-element recursive helper needing ~300) failed its first
        // attempt at 64.
        let input: LispEvalToolInput =
            serde_json::from_str(r#"{"form": "(+ 1 2)"}"#).expect("deserializes");
        assert_eq!(input.max_depth, 1024);
    }

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
        assert!(
            result.is_ok(),
            "compound form should succeed: {:?}",
            result.err()
        );
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
        assert!(
            result.is_ok(),
            "zero-count guard should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.expect("checked is_ok above"), json!(0));
    }

    #[test]
    fn test_interp_arithmetic_literal() {
        // The form that was confirmed working via the tool earlier.
        let result = hkask_lisp::eval_sandboxed_with_budget("(+ 1 2 3)", &json!({}), 100000, 64);
        assert!(
            result.is_ok(),
            "arithmetic should succeed: {:?}",
            result.err()
        );
        assert_eq!(result.expect("checked is_ok above"), json!(6));
    }

    // JSON text is the wire representation, including for an empty environment.
    #[test]
    fn test_env_accepts_stringified_json() {
        let input = json!({"form": "(+ 1 2)", "env": "{}"});
        let result: LispEvalToolInput =
            serde_json::from_value(input).expect("stringified env must be accepted");
        assert!(
            result.env.is_empty(),
            "stringified empty object must parse to empty map"
        );
    }

    // Regression: a valid stringified JSON object must also parse.
    #[test]
    fn test_env_accepts_stringified_json_object() {
        let input = json!({"form": "(+ 1 2)", "env": "{\"step_5_result\": {\"count\": 3}}"});
        let result: LispEvalToolInput =
            serde_json::from_value(input).expect("stringified env object must be accepted");
        assert_eq!(result.env.len(), 1);
        assert!(result.env.contains_key("step_5_result"));
    }

    #[test]
    fn test_env_rejects_unencoded_object() {
        let input = json!({"form": "(+ 1 2)", "env": {"step_5_result": {"count": 3}}});
        let error = serde_json::from_value::<LispEvalToolInput>(input)
            .expect_err("unencoded map is not the wire contract");
        assert!(error.to_string().contains("env:"));
        assert!(error.to_string().contains("JSON text"));
    }
}
