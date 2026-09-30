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
    /// The Lisp form to evaluate. Special forms: `quote`, `if`, `let`,
    /// `lambda`, `define`, `begin`, `and`, `or`, `not`, `cond`. Builtins:
    /// arithmetic (`+`, `-`, `*`, `/`, `=`, `!=`, `<`, `<=`, `>`, `>=`),
    /// `car`, `cdr`, `cons`, `list`, `length`, `nth`, `reverse`, `is_null`,
    /// `numberp`, `listp`, `stringp`, `assoc`, `append`, `member`, `abs`,
    /// `sqrt`, `max`, `min`, `eq`, `string=`, `string-contains`, `concat`.
    /// Integer arithmetic is checked (overflow errors, never wraps silently)
    /// and all-integer comparisons are exact (never coerced through f64).
    /// `/` always returns a Float (`(/ 6 3)` → `2.0`); over 3+ args `!=`
    /// compares adjacent pairs. Bare infix is accepted for the arithmetic
    /// and comparison operators (`a + b` → `(+ a b)`; same-operator chains
    /// fold; mixed operators do not associate; a parenthesized `(a + b)`
    /// is NOT equivalent — it errors).
    #[serde(deserialize_with = "deserialize_form_field")]
    form: String,
    /// JSON object whose keys become top-level Lisp bindings. Values are
    /// converted to Lisp values: objects become association lists, arrays
    /// become lists, numbers stay numbers, strings stay strings.
    ///
    /// Uses `HashMap<String, AnyJsonValue>` (not `serde_json::Value`) so the
    /// generated schema is `{"type":"object","additionalProperties":{}}` — a
    /// bare `AnyJsonValue` emits `{}` (any value), which the model doesn't
    /// populate; a bare `serde_json::Value` emits `true`, which strict-schema
    /// providers reject outright. The `HashMap` shape gives the model a clear
    /// `type: object` signal to send a JSON object.
    ///
    /// `deserialize_env_field` tolerates models that emit `env` as a stringified
    /// JSON string (e.g. `"{}"`) instead of a bare object, and teaches on every
    /// rejection: the error names the field, the received shape, and the
    /// accepted shape (an error that does not teach produces identical
    /// retries — the 2026-09-28 audit lockout anatomy).
    #[serde(default, deserialize_with = "deserialize_env_field")]
    env: std::collections::HashMap<String, hkask_types::AnyJsonValue>,
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
) -> Result<std::collections::HashMap<String, hkask_types::AnyJsonValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    super::deserialize_teaching_field(
        deserializer,
        "env",
        "a JSON object like {\"binding\": value} (a stringified JSON object is also accepted)",
        true,
    )
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
    let env_value = {
        // Canonical order: env keys are sorted before the object is built, so
        // the boundary is deterministic across processes (HashMap iteration
        // order is process-randomized; preserve_order then freezes whatever
        // order arrives first). No form can observe top-level key order today
        // (each key becomes an individual binding — proven lisp-repair L0), so
        // this is defense-in-depth for any future env-introspection surface;
        // the error path sorts independently (bindings list).
        let mut pairs: Vec<(String, serde_json::Value)> =
            input.env.into_iter().map(|(k, v)| (k, v.into())).collect();
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
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::LispEvalToolInput;
    use serde_json::json;

    // The tool dispatches to `hkask_lisp::eval_sandboxed_with_budget` — these
    // tests exercise the interpreter directly along the exact code path the
    // tool uses (same function, same arguments). They verify the skill
    // convergence patterns that `lisp_eval` is designed to support.
    //
    // NOTE: The `env` field went through three iterations:
    //   1. `serde_json::Value` — schemars renders as bare `true`, which
    //      strict-schema providers (Ollama, Gemini) reject outright.
    //   2. `AnyJsonValue` — schema emits `{}` (any value). This avoided the
    //      boolean rejection but the model still didn't populate the parameter
    //      (no `type` signal → env arrives as null → "unbound symbol").
    //   3. `HashMap<String, AnyJsonValue>` — schema emits `{"type":"object",
    //      "additionalProperties":{}}`, giving the model a clear object signal.
    //      This matches the working `render_template` context pattern.
    // These tests verify the interpreter itself is correct (they call
    // `eval_sandboxed_with_budget` directly). The tool wrapper fix (schema
    // shape) needs a process rebuild to take effect live.

    #[test]
    fn test_env_schema_has_type_object() {
        // The env parameter must generate a schema with "type": "object" so the
        // model populates it. This is the root-cause regression test for the
        // "unbound symbol" bug: a bare `AnyJsonValue` emits `{}` (no type) and
        // the model doesn't send the parameter; a bare `serde_json::Value`
        // emits `true` (boolean schema) which strict-schema providers reject.
        // `HashMap<String, AnyJsonValue>` emits `{"type":"object",...}`.
        let schema = schemars::schema_for!(super::LispEvalToolInput);
        let schema_json = serde_json::to_value(&schema).expect("schema is serializable");
        let env_schema = &schema_json["properties"]["env"];
        assert_eq!(
            env_schema["type"], "object",
            "env schema must have type:object so the model populates it, got: {env_schema}"
        );
    }

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

    // Regression: when the model emits `env` as a stringified JSON string
    // (e.g. `"{}"`) instead of a bare object, `deserialize_env_field` parses
    // the string and the tool succeeds. Without it, the deserializer rejects
    // with "invalid type: string, expected a map" and the tool errors out.
    // This is the same pattern `edit_file.edits` uses.
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

    // Positive path: a bare object must still work.
    #[test]
    fn test_env_accepts_bare_object() {
        let input = json!({"form": "(+ 1 2)", "env": {"step_5_result": {"count": 3}}});
        let result: LispEvalToolInput =
            serde_json::from_value(input).expect("bare object must parse");
        assert_eq!(result.env.len(), 1);
        assert!(result.env.contains_key("step_5_result"));
    }

    // ── Canonical skill-form contract tests ─────────────────────────────
    // The grounding-verify skill pins literal lisp_eval forms in its SKILL.md
    // and agents call them verbatim. These tests execute the exact forms so
    // interpreter evolution cannot silently break them — the Step 6 floor
    // form shipped broken (`mapcar` is not a builtin, `min` was not one then,
    // and symbol keys never match JSON string keys) because nothing ran it.

    #[test]
    fn test_canonical_narrative_mark_form() {
        // grounding-verify SKILL.md Step 4 item 1 — the any-verified/mark
        // recursive pair computing narrative blocks' final is_sourced flags
        // from surviving strength-2 claim IDs. Unpinned until 2026-09-28:
        // the same verbatim-pinned, agent-executed, nothing-runs-it risk
        // class that shipped the floor form broken. The base case is `false`,
        // not `nil`: a claimless block is definitively not sourced — `null`
        // would read as unknown under the D/P discipline. The interpreter's
        // `to_json` canonicalizes the pair-list into a JSON object
        // (`{block_name: is_sourced}`), which is the designed output shape.
        let form = r#"(define any-verified (lambda (ids verified) (if (is_null ids) false (or (member (car ids) verified) (any-verified (cdr ids) verified))))) (define mark (lambda (blocks verified) (if (is_null blocks) '() (cons (list (assoc "block_name" (car blocks)) (any-verified (assoc "claim_ids" (car blocks)) verified)) (mark (cdr blocks) verified))))) (mark blocks verified_ids)"#;
        let marked = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"blocks": [
                {"block_name": "financial_profile", "claim_ids": ["c1", "c2"]},
                {"block_name": "management_skill", "claim_ids": ["c3"]},
                {"block_name": "empty_block", "claim_ids": []}
            ], "verified_ids": ["c1", "c2"]}),
            100_000,
            64,
        )
        .expect("mark form must evaluate");
        assert_eq!(
            marked,
            json!({
                "financial_profile": true,
                "management_skill": false,
                "empty_block": false
            }),
            "a block is sourced iff at least one of its claims survived verification — an empty claim list is never sourced, and every flag is a Boolean, never null"
        );
    }

    #[test]
    fn test_canonical_prediction_reconciliation_form() {
        // grounding-verify SKILL.md Step 7 item 0 — the prediction-gap
        // reconciliation: found minus predicted for both counts.
        let form = r#"(list (- found_load_bearing predicted_load_bearing) (- found_failures predicted_failures))"#;
        let gaps = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"found_load_bearing": 12, "predicted_load_bearing": 10, "found_failures": 3, "predicted_failures": 2}),
            100_000,
            64,
        )
        .expect("reconciliation form must evaluate");
        assert_eq!(gaps, json!([2, 1]));
    }

    #[test]
    fn test_canonical_provenance_floor_form() {
        // grounding-verify SKILL.md Step 6 — provenance floor as a recursive
        // min over claim strengths (string keys: JSON objects bind strings).
        let form = r#"(define floor-strength (lambda (cs) (if (= (length cs) 1) (assoc "strength" (nth 0 cs)) (let ((rest_min (floor-strength (cdr cs)))) (let ((this (assoc "strength" (car cs)))) (if (< this rest_min) this rest_min)))))) (floor-strength claims)"#;
        let multi = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"claims": [{"claim_id": "c1", "strength": 2}, {"claim_id": "c2", "strength": 1}, {"claim_id": "c3", "strength": 2}]}),
            100_000,
            64,
        )
        .expect("floor form must evaluate");
        assert_eq!(multi, json!(1), "floor is the weakest claim's strength");

        let single = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"claims": [{"claim_id": "c1", "strength": 2}]}),
            100_000,
            64,
        )
        .expect("single-claim floor must evaluate");
        assert_eq!(single, json!(2));

        // A claim record missing `strength` must fail loudly — an
        // unfinished classification surfaces, it does not silently floor.
        let err = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"claims": [{"claim_id": "c1"}, {"claim_id": "c2", "strength": 1}]}),
            100_000,
            64,
        )
        .expect_err("missing strength must error, not floor");
        assert!(
            matches!(err, hkask_lisp::LispError::TypeError { .. }),
            "got: {err}"
        );
    }

    #[test]
    fn test_canonical_vocabulary_check_form() {
        // grounding-verify SKILL.md Step 2 item 5 — closed-vocabulary count.
        let form = r#"(let ((vocab (list "tool_verified" "platform_derived" "model_inference" "unavailable" "tool_no_match" "pending_check" "rejected")) (bad (lambda (lst) (cond ((is_null lst) 0) ((member (assoc "provenance" (car lst)) vocab) (bad (cdr lst))) (t (+ 1 (bad (cdr lst)))))))) (bad assignments))"#;
        let typo = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"assignments": [{"claim_id": "c1", "provenance": "tool_verrified"}, {"claim_id": "c2", "provenance": "model_inference"}]}),
            100_000,
            64,
        )
        .expect("vocabulary form must evaluate");
        assert_eq!(typo, json!(1), "a planted typo must count as bad");

        let clean = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"assignments": [{"claim_id": "c1", "provenance": "tool_verified"}, {"claim_id": "c2", "provenance": "model_inference"}]}),
            100_000,
            64,
        )
        .expect("clean vocabulary form must evaluate");
        assert_eq!(clean, json!(0));
    }

    #[test]
    fn test_canonical_why_length_check_form() {
        // grounding-verify SKILL.md Step 2 item 5 — why-min-40 count. A
        // missing `why` counts as short (length of nil is 0) — fail-closed.
        let form = r#"(let ((short (lambda (lst) (cond ((is_null lst) 0) ((>= (length (assoc "why" (car lst))) 40) (short (cdr lst))) (t (+ 1 (short (cdr lst)))))))) (short assignments))"#;
        let short = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"assignments": [{"claim_id": "c1", "why": "short one"}, {"claim_id": "c2", "why": "this explanation is definitely longer than forty characters total"}, {"claim_id": "c3"}]}),
            100_000,
            64,
        )
        .expect("why-length form must evaluate");
        assert_eq!(short, json!(2), "short and missing why both count");

        let clean = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"assignments": [{"claim_id": "c1", "why": "this explanation is definitely longer than forty characters total"}]}),
            100_000,
            64,
        )
        .expect("clean why-length form must evaluate");
        assert_eq!(clean, json!(0));
    }

    #[test]
    fn test_canonical_therapy_count_forms() {
        // therapy SKILL.md Phase 4/Phase 5 — approval and failure counts as
        // recursive helpers (the interpreter has no `filter` builtin; the
        // original forms died on `unbound symbol: filter`).
        let approved_form = r#"(define count-approved (lambda (lst) (if (is_null lst) 0 (if (eq (assoc "approved" (car lst)) t) (+ 1 (count-approved (cdr lst))) (count-approved (cdr lst)))))) (count-approved proposals)"#;
        let approved = hkask_lisp::eval_sandboxed_with_budget(
            approved_form,
            &json!({"proposals": [{"approved": true}, {"approved": false}, {"approved": true}]}),
            100_000,
            64,
        )
        .expect("count-approved form must evaluate");
        assert_eq!(approved, json!(2));

        let failed_form = r#"(define count-failed (lambda (lst) (if (is_null lst) 0 (if (eq (assoc "success" (car lst)) nil) (+ 1 (count-failed (cdr lst))) (count-failed (cdr lst)))))) (count-failed results)"#;
        let failed = hkask_lisp::eval_sandboxed_with_budget(
            failed_form,
            &json!({"results": [{"success": true}, {}, {"success": false}]}),
            100_000,
            64,
        )
        .expect("count-failed form must evaluate");
        // A missing `success` counts (assoc → nil); an explicit false does
        // not — same semantics as the original form.
        assert_eq!(failed, json!(1));
    }

    #[test]
    fn test_canonical_step1_structural_form() {
        // grounding-verify SKILL.md Step 1 — zero-claim guard.
        let form = r#"(if (= (length claims) 0) 'no_factual_claims 'ok)"#;
        let ok = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"claims": [{"claim_id": "c1"}]}),
            100_000,
            64,
        )
        .expect("step 1 form must evaluate");
        assert_eq!(ok, json!("ok"));

        let empty =
            hkask_lisp::eval_sandboxed_with_budget(form, &json!({"claims": []}), 100_000, 64)
                .expect("empty-claims form must evaluate");
        assert_eq!(empty, json!("no_factual_claims"));
    }

    #[test]
    fn test_skill_md_pins_canonical_forms() {
        // The forms above are a contract with the skill text: if the SKILL.md
        // drifts from them (or regresses to a non-executable form), this fails
        // until the skill and the tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/grounding-verify/SKILL.md"
        ))
        .expect("grounding-verify SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains(r#"(define floor-strength (lambda (cs)"#),
            "Step 6 floor form must stay pinned in grounding-verify SKILL.md"
        );
        assert!(
            !skill_md.contains("(min (mapcar"),
            "mapcar is not a builtin — the old floor form was broken"
        );
        assert!(
            skill_md.contains(r#"(assoc "provenance" (car lst))"#),
            "vocabulary-check form must stay pinned in grounding-verify SKILL.md"
        );
        assert!(
            skill_md.contains(r#"(assoc "why" (car lst))"#),
            "why-length-check form must stay pinned in grounding-verify SKILL.md"
        );
        assert!(
            skill_md.contains(r#"(if (is_null ids) false"#),
            "narrative-mark form must stay pinned with the false base case — \
             the nil base case shipped null flags where the semantics demand false"
        );
    }

    #[test]
    fn test_therapy_skill_md_pins_count_forms() {
        // therapy SKILL.md pins two count forms; if they drift (or regress to
        // the broken `filter` forms), this fails until skill and tests are
        // reconciled.
        let therapy_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/therapy/SKILL.md"
        ))
        .expect("therapy SKILL.md must exist in the workspace");
        assert!(
            therapy_md.contains("(count-approved proposals)"),
            "Phase 4 count form must stay pinned in therapy SKILL.md"
        );
        assert!(
            therapy_md.contains("(count-failed results)"),
            "Phase 5 count form must stay pinned in therapy SKILL.md"
        );
        assert!(
            !therapy_md.contains("(filter"),
            "filter is not a builtin — the old therapy forms were broken"
        );
    }

    #[test]
    fn test_program_manager_skill_md_pins_closure_ledger_form() {
        // program-manager SKILL.md pins the closure-ledger form; if it
        // drifts — or loses the shape guard that catches object-shaped
        // ledgers silently reading green — this fails until skill and
        // tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/program-manager/SKILL.md"
        ))
        .expect("program-manager SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains(
                r#"(nonflat (lambda (items) (if (= 0 (length items)) 0 (+ (if (listp (car (car items))) 1 0) (nonflat (cdr items)))))))"#
            ),
            "closure-ledger shape guard must stay pinned in program-manager SKILL.md"
        );
        assert!(
            skill_md.contains(r#"(if (> shape 0) (quote red)"#),
            "shape violations must read red, not green"
        );
        assert!(
            skill_md.contains(r#"(member token (car items))"#),
            "the count-token primitive must stay member-based — assoc/eq compare identity and silently miss env-provided strings (the green-wash this form exists to prevent)"
        );

        // The pinned form, executed three ways: green flat ledger, red
        // flat ledger (abandoned + unowned), and the object-shaped entry
        // that the pre-2026-09-28 form silently read green.
        let form = r#"(let ((count-token (lambda (items token) (if (= 0 (length items)) 0 (+ (if (member token (car items)) 1 0) (count-token (cdr items) token))))) (nonflat (lambda (items) (if (= 0 (length items)) 0 (+ (if (listp (car (car items))) 1 0) (nonflat (cdr items))))))) (let ((abandoned (count-token findings "reported-abandoned")) (unowned (count-token findings "owner:none")) (shape (nonflat findings))) (if (> shape 0) (quote red) (if (and (= abandoned 0) (= unowned 0)) (quote green) (quote red)))))"#;

        let green = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"findings": [["f1", "half-edit left in tree", "fixed-verified", "owner:agent"], ["f2", "unrelated bug", "delegated-tracked", "owner:agent"]]}),
            100_000,
            64,
        )
        .expect("green-ledger form must evaluate");
        assert_eq!(green, json!("green"));

        let red = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"findings": [["f1", "half-edit left in tree", "fixed-verified", "owner:agent"], ["f2", "unrelated bug", "reported-abandoned", "owner:none"]]}),
            100_000,
            64,
        )
        .expect("red-ledger form must evaluate");
        assert_eq!(red, json!("red"));

        let malformed = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"findings": [["f1", "half-edit left in tree", "fixed-verified", "owner:agent"], {"id": "f3", "state": "operator-decision", "owner": "operator"}]}),
            100_000,
            64,
        )
        .expect("object-shaped-ledger form must evaluate");
        assert_eq!(
            malformed,
            json!("red"),
            "an object-shaped ledger entry must read red — the pre-2026-09-28 form silently read green"
        );
    }

    #[test]
    fn test_skill_maintenance_skill_md_pins_health_score_form() {
        // skill-maintenance SKILL.md pins the audit health-score form; if it
        // drifts (or the severity weights change silently), this fails until
        // skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/skill-maintenance/SKILL.md"
        ))
        .expect("skill-maintenance SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains(
                r#"(max 0 (- 1 (+ (* 0.50 critical) (* 0.15 high) (* 0.10 medium) (* 0.05 low))))"#
            ),
            "the audit health-score form must stay pinned in skill-maintenance SKILL.md"
        );

        let form =
            r#"(max 0 (- 1 (+ (* 0.50 critical) (* 0.15 high) (* 0.10 medium) (* 0.05 low))))"#;
        let clean = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"critical": 0, "high": 0, "medium": 0, "low": 0}),
            100_000,
            64,
        )
        .expect("no-defect score must evaluate");
        assert_eq!(clean, json!(1.0));

        let one_critical = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"critical": 1, "high": 0, "medium": 0, "low": 0}),
            100_000,
            64,
        )
        .expect("one-critical score must evaluate");
        assert_eq!(one_critical, json!(0.5));

        let floored = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"critical": 2, "high": 1, "medium": 1, "low": 1}),
            100_000,
            64,
        )
        .expect("floor-case score must evaluate");
        assert_eq!(
            floored,
            json!(0),
            "the max-0 floor must hold — a score below zero reads zero, never negative"
        );
    }

    #[test]
    fn test_gradient_hunter_skill_md_pins_convergence_gate() {
        // gradient-hunter SKILL.md pins the Phase 6 stability gate; if it
        // drifts (or the set-comparison vocabulary changes), this fails until
        // skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/gradient-hunter/SKILL.md"
        ))
        .expect("gradient-hunter SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains("(and (eq new_gradient_shapes 0) (eq top_k_stable 1))"),
            "the Phase 6 stability gate must stay pinned in gradient-hunter SKILL.md"
        );

        let gate = r#"(and (eq new_gradient_shapes 0) (eq top_k_stable 1))"#;
        let green = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"new_gradient_shapes": 0, "top_k_stable": 1}),
            100_000,
            64,
        )
        .expect("converged gate env must evaluate");
        assert_eq!(green, json!(true));

        let red = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"new_gradient_shapes": 1, "top_k_stable": 1}),
            100_000,
            64,
        )
        .expect("planted-new-shape gate env must evaluate");
        assert_eq!(
            red,
            json!(false),
            "a planted new gradient shape must fail the stability gate"
        );
    }

    #[test]
    fn test_improv_skill_md_pins_yes_but_form() {
        // improv SKILL.md pins the yes-but forbidden-words check; if it
        // drifts — or the needle-first argument order is "fixed" into
        // reversal — this fails until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/improv/SKILL.md"
        ))
        .expect("improv SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains(r#"(string-contains (car ws) reply)"#),
            "the yes-but check must stay needle-first — the word is the needle, reply the haystack (reversed args silently return false)"
        );

        let form = r#"(begin (define bad (lambda (ws) (if (= (length ws) 0) (list) (if (string-contains (car ws) reply) (cons (car ws) (bad (cdr ws))) (bad (cdr ws)))))) (bad (list " no " " no," "No " "No," "wrong" "can't" "cannot" "impossible")))"#;
        let flagged = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"reply": "No, that can't work — wrong approach"}),
            100_000,
            64,
        )
        .expect("flagged-reply form must evaluate");
        assert_eq!(flagged, json!(["No,", "wrong", "can't"]));

        let clean = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"reply": "Yes — and let's also account for the deployment constraint"}),
            100_000,
            64,
        )
        .expect("clean-reply form must evaluate");
        assert_eq!(
            clean,
            json!([]),
            "an empty list names no literal forbidden words — it does not prove the reply avoids contradiction (that stays the human's judgment)"
        );
    }

    #[test]
    fn test_create_skill_skill_md_pins_convergence_forms() {
        // create-skill SKILL.md pins the Phase 5 convergence gate and the
        // translation reconciliation form; if they drift (or the
        // anchors-first precedence is lost), this fails until skill and
        // tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/create-skill/SKILL.md"
        ))
        .expect("create-skill SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains("(quote reenter-phase-1)"),
            "the Phase 5 gate's anchors-first re-entry clause must stay pinned in create-skill SKILL.md"
        );
        assert!(
            skill_md.contains(r#"(= source_steps (+ steps_mapped (length unresolved_concepts)))"#),
            "the translation reconciliation form must stay pinned in create-skill SKILL.md"
        );

        let gate = r#"(cond ((and (= scaffold 0) (= research 0) (= functional 1)) (quote done)) ((>= reentries 2) (quote stop-and-report)) ((> research 0) (quote reenter-phase-1)) (t (quote reenter-phase-3)))"#;
        let clean = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"scaffold": 0, "research": 0, "functional": 1, "reentries": 0}),
            100_000,
            64,
        )
        .expect("all-clean gate env must evaluate");
        assert_eq!(clean, json!("done"));

        let exhausted = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"scaffold": 2, "research": 0, "functional": 0, "reentries": 2}),
            100_000,
            64,
        )
        .expect("exhausted gate env must evaluate");
        assert_eq!(exhausted, json!("stop-and-report"));

        let anchors_first = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"scaffold": 1, "research": 1, "functional": 0, "reentries": 0}),
            100_000,
            64,
        )
        .expect("mixed-findings gate env must evaluate");
        assert_eq!(
            anchors_first,
            json!("reenter-phase-1"),
            "missing anchors fire before artifact defects — artifact work on missing anchors is wasted"
        );

        let artifact_only = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"scaffold": 1, "research": 0, "functional": 0, "reentries": 0}),
            100_000,
            64,
        )
        .expect("artifact-only gate env must evaluate");
        assert_eq!(artifact_only, json!("reenter-phase-3"));

        let reconcile = r#"(= source_steps (+ steps_mapped (length unresolved_concepts)))"#;
        let complete = hkask_lisp::eval_sandboxed_with_budget(
            reconcile,
            &json!({"source_steps": 3, "steps_mapped": 2, "unresolved_concepts": ["model-scored quality rubric"]}),
            100_000,
            64,
        )
        .expect("reconciliation form must evaluate");
        assert_eq!(complete, json!(true));

        let dropped = hkask_lisp::eval_sandboxed_with_budget(
            reconcile,
            &json!({"source_steps": 3, "steps_mapped": 1, "unresolved_concepts": ["model-scored quality rubric"]}),
            100_000,
            64,
        )
        .expect("dropped-step reconciliation must evaluate");
        assert_eq!(
            dropped,
            json!(false),
            "a silently dropped source step must fail the reconciliation"
        );
    }

    #[test]
    fn test_canonical_superforecasting_forms() {
        // superforecasting SKILL.md stage 4 (Bayes) and stage 5 (MCDA-weighted
        // average) — pinned so the pipeline's probability arithmetic is
        // deterministic, not model-computed.
        let bayes = r#"(/ (* prior likelihood_ratio) (+ (* prior likelihood_ratio) (- 1 prior)))"#;
        let posterior = hkask_lisp::eval_sandboxed_with_budget(
            bayes,
            &json!({"prior": 0.4, "likelihood_ratio": 3}),
            100_000,
            64,
        )
        .expect("Bayes form must evaluate");
        let posterior = posterior.as_f64().expect("posterior is numeric");
        assert!(
            (posterior - 0.6666666666666666).abs() < 1e-9,
            "got {posterior}"
        );

        let mcda = r#"(/ (+ (* m1 c1) (* m2 c2) (* m3 c3)) (+ c1 c2 c3))"#;
        let weighted = hkask_lisp::eval_sandboxed_with_budget(
            mcda,
            &json!({"m1": 0.3, "c1": 2, "m2": 0.5, "c2": 1, "m3": 0.7, "c3": 1}),
            100_000,
            64,
        )
        .expect("MCDA form must evaluate");
        let weighted = weighted.as_f64().expect("weighted average is numeric");
        assert!((weighted - 0.45).abs() < 1e-9, "got {weighted}");
    }

    #[test]
    fn test_canonical_flash_forms() {
        // company-research-flash SKILL.md — alpha score, pt_12m blend,
        // rr/rating/DROP gate, and the ENTER gate dispatch.
        let alpha = hkask_lisp::eval_sandboxed_with_budget(
            r#"(+ (* coverage_gap 0.30) (* market_cap_fit 0.20) (* sector_relevance 0.25) (* valuation_anomaly 0.25))"#,
            &json!({"coverage_gap": 0.8, "market_cap_fit": 0.6, "sector_relevance": 0.9, "valuation_anomaly": 0.7}),
            100_000,
            64,
        )
        .expect("alpha form must evaluate");
        let alpha = alpha.as_f64().expect("alpha is numeric");
        assert!((alpha - 0.76).abs() < 1e-9, "got {alpha}");

        let blend = hkask_lisp::eval_sandboxed_with_budget(
            r#"(/ (+ (* dcf w_dcf) (* comps w_comps) (* scenario_pt w_siv)) (+ w_dcf w_comps w_siv))"#,
            &json!({"dcf": 110, "comps": 105, "scenario_pt": 115, "w_dcf": 0.5, "w_comps": 0.3, "w_siv": 0.2}),
            100_000,
            64,
        )
        .expect("blend form must evaluate");
        let blend = blend.as_f64().expect("blend is numeric");
        assert!((blend - 109.5).abs() < 1e-9, "got {blend}");

        let rating = hkask_lisp::eval_sandboxed_with_budget(
            r#"(let ((rr (/ (- pt_12m market_price) (- market_price bear_case_pt)))) (cond ((>= rr 2) 'BUY) ((>= rr 1) 'HOLD) (t 'UNDERPERFORM)))"#,
            &json!({"pt_12m": 120, "market_price": 100, "bear_case_pt": 80}),
            100_000,
            64,
        )
        .expect("rating form must evaluate");
        assert_eq!(rating, json!("HOLD"));

        let drop_gate = hkask_lisp::eval_sandboxed_with_budget(
            r#"(if (and (< rr 2) (eq rating "UNDERPERFORM")) 'DROP 'PROCEED)"#,
            &json!({"rr": 0.8, "rating": "UNDERPERFORM"}),
            100_000,
            64,
        )
        .expect("DROP gate form must evaluate");
        assert_eq!(drop_gate, json!("DROP"));

        let enter = hkask_lisp::eval_sandboxed_with_budget(
            r#"(let ((n (+ (if edge 1 0) (if new 1 0) (if timely 1 0) (if examples 1 0) (if revealing 1 0)))) (cond ((= n 5) 'PUBLISH) ((= n 4) 'ALERT) (t 'DROP)))"#,
            &json!({"edge": true, "new": true, "timely": true, "examples": true, "revealing": true}),
            100_000,
            64,
        )
        .expect("ENTER gate form must evaluate");
        assert_eq!(enter, json!("PUBLISH"));
    }

    #[test]
    fn test_superforecasting_skill_md_pins_forms() {
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/superforecasting/SKILL.md"
        ))
        .expect("superforecasting SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains(
                "(/ (* prior likelihood_ratio) (+ (* prior likelihood_ratio) (- 1 prior)))"
            ),
            "Bayes form must stay pinned in superforecasting SKILL.md"
        );
        assert!(
            skill_md.contains("(/ (+ (* m1 c1) (* m2 c2) (* m3 c3)) (+ c1 c2 c3))"),
            "MCDA weighted-average form must stay pinned in superforecasting SKILL.md"
        );
    }

    #[test]
    fn test_flash_skill_md_pins_forms() {
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/company-research-flash/SKILL.md"
        ))
        .expect("company-research-flash SKILL.md must exist in the workspace");
        assert!(
            skill_md.contains("(* coverage_gap 0.30)"),
            "alpha-score form must stay pinned in flash SKILL.md"
        );
        assert!(
            skill_md.contains("(/ (- pt_12m market_price) (- market_price bear_case_pt))"),
            "rr form must stay pinned in flash SKILL.md"
        );
        assert!(
            skill_md.contains("((= n 5) 'PUBLISH)"),
            "ENTER gate form must stay pinned in flash SKILL.md"
        );
        assert!(
            !skill_md.contains("reports/company-research/"),
            "the stale reports-path constraint was removed"
        );
        assert!(
            !skill_md.contains("using the market_calibration Brier score"),
            "the stale market-Brier calibration semantics was purged — the analyst's own resolved forecasts are the measurement"
        );

        // The publication gate — the skill's most load-bearing D gate —
        // executed on its three branches: publish, block on adjusted
        // confidence, and the null-input incomplete path.
        let pub_gate = r#"(if (or (is_null enter_eligible) (is_null unadjusted_confidence) (is_null confidence_adjustment) (is_null review_performed) (is_null unresolved_contradictions)) (list nil false) (let ((adjusted (+ unadjusted_confidence confidence_adjustment))) (list (max 0 adjusted) (and enter_eligible (>= adjusted 0.50) review_performed (= unresolved_contradictions 0)))))"#;
        assert!(
            skill_md.contains(pub_gate),
            "publication gate form must stay pinned in flash SKILL.md"
        );
        let publish = hkask_lisp::eval_sandboxed_with_budget(
            pub_gate,
            &json!({"enter_eligible": true, "unadjusted_confidence": 0.55, "confidence_adjustment": 0.0, "review_performed": true, "unresolved_contradictions": 0}),
            100_000,
            64,
        )
        .expect("publication gate must evaluate");
        assert_eq!(publish, json!([0.55, true]));
        let blocked = hkask_lisp::eval_sandboxed_with_budget(
            pub_gate,
            &json!({"enter_eligible": true, "unadjusted_confidence": 0.55, "confidence_adjustment": -0.10, "review_performed": true, "unresolved_contradictions": 0}),
            100_000,
            64,
        )
        .expect("publication gate must evaluate on a penalized review");
        assert_eq!(
            blocked,
            json!([0.45000000000000007, false]),
            "a -0.10 uncorroborated-claim adjustment must block publication below 0.50"
        );
        let incomplete = hkask_lisp::eval_sandboxed_with_budget(
            pub_gate,
            &json!({"enter_eligible": null, "unadjusted_confidence": 0.55, "confidence_adjustment": 0.0, "review_performed": true, "unresolved_contradictions": 0}),
            100_000,
            64,
        )
        .expect("publication gate must evaluate on a null input");
        assert_eq!(
            incomplete,
            json!([null, false]),
            "a missing input is an incomplete data gap, never a publish"
        );

        // The KATA calibration form — four branches: overconfident,
        // no_prediction (broken feedback loop), undetermined (<5),
        // underconfident.
        let kata = r#"(begin (define sum (lambda (l) (if (is_null l) 0 (+ (car l) (sum (cdr l)))))) (define mean (lambda (l) (/ (sum l) (length l)))) (cond ((is_null ps) (list 1.0 "no_prediction")) ((< (length ps) 5) (list nil "undetermined")) (t (let ((d (- (mean ps) (mean os)))) (list (abs d) (if (> d 0) "overconfident" (if (< d 0) "underconfident" "calibrated")))))))"#;
        assert!(
            skill_md.contains(kata),
            "KATA calibration form must stay pinned in flash SKILL.md"
        );
        let over = hkask_lisp::eval_sandboxed_with_budget(
            kata,
            &json!({"ps": [0.8, 0.7, 0.9, 0.6, 0.8], "os": [1, 0, 1, 0, 1]}),
            100_000,
            64,
        )
        .expect("KATA calibration form must evaluate");
        assert_eq!(
            over,
            json!([0.16000000000000003, "overconfident"]),
            "mean stated 0.76 vs observed 0.6 is a 0.16 overconfident gap"
        );
        let no_prediction =
            hkask_lisp::eval_sandboxed_with_budget(kata, &json!({"ps": [], "os": []}), 100_000, 64)
                .expect("KATA calibration form must evaluate on empty forecasts");
        assert_eq!(
            no_prediction,
            json!([1.0, "no_prediction"]),
            "no resolved forecasts is a broken feedback loop (1.0), not neutral"
        );
        let undetermined = hkask_lisp::eval_sandboxed_with_budget(
            kata,
            &json!({"ps": [0.6, 0.55], "os": [1, 0]}),
            100_000,
            64,
        )
        .expect("KATA calibration form must evaluate on a short history");
        assert_eq!(
            undetermined,
            json!([null, "undetermined"]),
            "fewer than 5 resolved forecasts is undetermined (null), never a number"
        );
        let under = hkask_lisp::eval_sandboxed_with_budget(
            kata,
            &json!({"ps": [0.6, 0.55, 0.65, 0.7, 0.5], "os": [1, 1, 1, 1, 1]}),
            100_000,
            64,
        )
        .expect("KATA calibration form must evaluate on an underconfident history");
        assert_eq!(
            under,
            json!([0.3999999999999999, "underconfident"]),
            "mean stated 0.6 vs observed 1.0 is a 0.4 underconfident gap"
        );
    }

    #[test]
    fn test_self_improvement_skill_md_pins_forms() {
        // self-improvement SKILL.md pins six agent-executed lisp_eval gate
        // forms with nothing else running them — the exact risk class that
        // shipped broken before (the grounding-verify floor form). If any
        // drifts, this fails until skill and tests are reconciled. The
        // convergence-gate example near the Fine-tuning Phase 5 text is
        // deliberately NOT pinned: the skill marks it as an example, not
        // policy ("do not use the example as a default policy").
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/self-improvement/SKILL.md"
        ))
        .expect("self-improvement SKILL.md must exist in the workspace");

        // 1. The propose-or-discard acceptance gate.
        assert!(
            skill_md.contains(
                r#"(and (not (member evaluation_method (list "none_available"))) (numberp pass_rate) (numberp baseline_pass_rate) (> pass_rate baseline_pass_rate) (= regressions 0) (= (length safety_violations) 0))"#
            ),
            "acceptance gate must stay pinned in self-improvement SKILL.md"
        );
        let gate = r#"(and (not (member evaluation_method (list "none_available"))) (numberp pass_rate) (numberp baseline_pass_rate) (> pass_rate baseline_pass_rate) (= regressions 0) (= (length safety_violations) 0))"#;
        let pass = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"evaluation_method": "metric-based", "pass_rate": 0.8, "baseline_pass_rate": 0.6, "regressions": 0, "safety_violations": []}),
            100_000,
            64,
        )
        .expect("acceptance gate must evaluate");
        assert_eq!(pass, json!(true));
        let no_method = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"evaluation_method": "none_available", "pass_rate": 0.8, "baseline_pass_rate": 0.6, "regressions": 0, "safety_violations": []}),
            100_000,
            64,
        )
        .expect("acceptance gate must evaluate on none_available");
        assert_eq!(
            no_method,
            json!(false),
            "none_available must fail the gate — no harness, no proposal"
        );

        // 2. The regression-count walker.
        assert!(
            skill_md.contains(
                r#"(begin (define regs (lambda (b a) (if (is_null b) 0 (+ (if (< (car a) (car b)) 1 0) (regs (cdr b) (cdr a)))))) (regs (list b1 b2 ...) (list a1 a2 ...)))"#
            ),
            "regression walker must stay pinned in self-improvement SKILL.md"
        );
        let regs = r#"(begin (define regs (lambda (b a) (if (is_null b) 0 (+ (if (< (car a) (car b)) 1 0) (regs (cdr b) (cdr a)))))) (regs (list 0.9 0.8 0.7) (list 0.85 0.9 0.7)))"#;
        let one_reg = hkask_lisp::eval_sandboxed_with_budget(regs, &json!({}), 100_000, 64)
            .expect("regression walker must evaluate");
        assert_eq!(one_reg, json!(1), "0.9→0.85 is the one regression");

        // 3. The GEPA dominance form with the 10% cost band.
        assert!(
            skill_md.contains(
                r#"(begin (define better-cost (lambda (a b) (< a (* 0.9 b)))) (define dom (lambda (a b) (and (>= (car a) (car b)) (not (better-cost (nth 1 b) (nth 1 a))) (or (> (car a) (car b)) (better-cost (nth 1 a) (nth 1 b)))))) (dom a b))"#
            ),
            "GEPA dominance form must stay pinned in self-improvement SKILL.md"
        );
        let dom = r#"(begin (define better-cost (lambda (a b) (< a (* 0.9 b)))) (define dom (lambda (a b) (and (>= (car a) (car b)) (not (better-cost (nth 1 b) (nth 1 a))) (or (> (car a) (car b)) (better-cost (nth 1 a) (nth 1 b)))))) (dom a b))"#;
        let dominates = hkask_lisp::eval_sandboxed_with_budget(
            dom,
            &json!({"a": [0.8, 1000], "b": [0.8, 1200]}),
            100_000,
            64,
        )
        .expect("dominance form must evaluate");
        assert_eq!(dominates, json!(true), "a 17% cost win is a strict win");
        let tie = hkask_lisp::eval_sandboxed_with_budget(
            dom,
            &json!({"a": [0.8, 100], "b": [0.8, 105]}),
            100_000,
            64,
        )
        .expect("dominance form must evaluate on a within-band pair");
        assert_eq!(
            tie,
            json!(false),
            "a 5% cost gap is a tie, not a strict win — the band the frontier template must match"
        );

        // 4. The GEPA convergence form.
        assert!(
            skill_md.contains("(and (>= iteration 2) (= new_members 0))"),
            "GEPA convergence form must stay pinned in self-improvement SKILL.md"
        );
        let conv = "(and (>= iteration 2) (= new_members 0))";
        let converged = hkask_lisp::eval_sandboxed_with_budget(
            conv,
            &json!({"iteration": 2, "new_members": 0}),
            100_000,
            64,
        )
        .expect("GEPA convergence form must evaluate");
        assert_eq!(converged, json!(true));
        let moving = hkask_lisp::eval_sandboxed_with_budget(
            conv,
            &json!({"iteration": 2, "new_members": 1}),
            100_000,
            64,
        )
        .expect("GEPA convergence form must evaluate on a moving frontier");
        assert_eq!(
            moving,
            json!(false),
            "a single arrival means the frontier is still moving"
        );

        // 5. The noise-floor standard-error form (three branches).
        assert!(
            skill_md.contains(
                r#"(let ((pc (/ kc nc)) (pb (/ kb nb))) (let ((se (sqrt (+ (/ (* pc (- 1 pc)) nc) (/ (* pb (- 1 pb)) nb))))) (cond ((or (< nc 10) (< nb 10)) (list "undetermined" "fewer than 10 held-out examples")) ((= se 0) (list (if (> pc pb) "beyond_noise" "within_noise") 0)) (t (list (if (> (- pc pb) (* 2 se)) "beyond_noise" "within_noise") (- pc pb) (* 2 se))))))"#
            ),
            "noise-floor form must stay pinned in self-improvement SKILL.md"
        );
        let noise = r#"(let ((pc (/ kc nc)) (pb (/ kb nb))) (let ((se (sqrt (+ (/ (* pc (- 1 pc)) nc) (/ (* pb (- 1 pb)) nb))))) (cond ((or (< nc 10) (< nb 10)) (list "undetermined" "fewer than 10 held-out examples")) ((= se 0) (list (if (> pc pb) "beyond_noise" "within_noise") 0)) (t (list (if (> (- pc pb) (* 2 se)) "beyond_noise" "within_noise") (- pc pb) (* 2 se))))))"#;
        let within = hkask_lisp::eval_sandboxed_with_budget(
            noise,
            &json!({"kc": 42, "nc": 50, "kb": 36, "nb": 50}),
            100_000,
            64,
        )
        .expect("noise-floor form must evaluate");
        assert_eq!(
            within,
            json!(["within_noise", 0.12, 0.16395121225535358]),
            "42/50 vs 36/50: a 12-point gain inside a 16-point band — the skill's documented example"
        );
        let beyond = hkask_lisp::eval_sandboxed_with_budget(
            noise,
            &json!({"kc": 48, "nc": 50, "kb": 30, "nb": 50}),
            100_000,
            64,
        )
        .expect("noise-floor form must evaluate on a beyond-noise pair");
        assert_eq!(
            beyond,
            json!(["beyond_noise", 0.36, 0.14923806484942104]),
            "48/50 vs 30/50 clears twice the combined standard error"
        );
        let undetermined = hkask_lisp::eval_sandboxed_with_budget(
            noise,
            &json!({"kc": 5, "nc": 6, "kb": 3, "nb": 8}),
            100_000,
            64,
        )
        .expect("noise-floor form must evaluate on a short held-out set");
        assert_eq!(
            undetermined,
            json!(["undetermined", "fewer than 10 held-out examples"]),
            "fewer than 10 examples per side blocks acceptance"
        );

        // 6. The Improvement Measure convergence form (adjacent differences).
        assert!(
            skill_md.contains(
                "(and (>= (length xs) 3) (< (abs (- (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs))) 0.02) (< (abs (- (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs))) 0.02))"
            ),
            "Improvement Measure convergence form must stay pinned in self-improvement SKILL.md"
        );
        let im = "(and (>= (length xs) 3) (< (abs (- (nth (- (length xs) 1) xs) (nth (- (length xs) 2) xs))) 0.02) (< (abs (- (nth (- (length xs) 2) xs) (nth (- (length xs) 3) xs))) 0.02))";
        let stable = hkask_lisp::eval_sandboxed_with_budget(
            im,
            &json!({"xs": [0.72, 0.73, 0.72]}),
            100_000,
            64,
        )
        .expect("Improvement Measure form must evaluate");
        assert_eq!(stable, json!(true));
        let drifting = hkask_lisp::eval_sandboxed_with_budget(
            im,
            &json!({"xs": [0.5, 0.9, 0.72]}),
            100_000,
            64,
        )
        .expect("Improvement Measure form must evaluate on a drifting sequence");
        assert_eq!(drifting, json!(false), "a 0.18 jump is not convergence");
    }

    #[test]
    fn test_company_research_deep_skill_md_pins_forms() {
        // company-research-deep SKILL.md pins three agent-executed lisp_eval
        // forms with nothing else running them. If any drifts, this fails
        // until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/company-research-deep/SKILL.md"
        ))
        .expect("company-research-deep SKILL.md must exist in the workspace");

        // 1. The claim-source key-closure form: the empty list passes; a
        // nonempty list names unregistered keys.
        let key_closure = r#"(let ((missing_one (lambda (keys known) (if (is_null keys) (list) (if (member (car keys) known) (missing_one (cdr keys) known) (cons (car keys) (missing_one (cdr keys) known))))))) (let ((missing_all (lambda (claims known) (if (is_null claims) (list) (append (missing_one (car claims) known) (missing_all (cdr claims) known)))))) (missing_all claim_source_keys source_keys)))"#;
        assert!(
            skill_md.contains(key_closure),
            "claim-source key-closure form must stay pinned in company-research-deep SKILL.md"
        );
        let clean = hkask_lisp::eval_sandboxed_with_budget(
            key_closure,
            &json!({"claim_source_keys": [["s1", "s2"], ["s2"]], "source_keys": ["s1", "s2"]}),
            100_000,
            64,
        )
        .expect("key-closure form must evaluate");
        assert_eq!(clean, json!([]), "registered keys pass");
        let unregistered = hkask_lisp::eval_sandboxed_with_budget(
            key_closure,
            &json!({"claim_source_keys": [["s1", "s9"]], "source_keys": ["s1", "s2"]}),
            100_000,
            64,
        )
        .expect("key-closure form must evaluate on an unregistered key");
        assert_eq!(
            unregistered,
            json!(["s9"]),
            "an unregistered key is named, never silently passed"
        );

        // 2. The industry-outside-view field check: all ten fields present
        // passes; a missing field is named.
        let field_check = r#"(let ((has (lambda (k entries) (if (is_null entries) false (if (string= k (car (car entries))) true (has k (cdr entries)))))) (missing (lambda (keys) (if (is_null keys) (list) (if (has (car keys) view) (missing (cdr keys)) (cons (car keys) (missing (cdr keys)))))))) (missing (list "industry_drivers" "self_regulatory_bodies" "company_fit" "frame_conflicts" "implication_for_falstaffian" "implication_for_gorilla" "implication_for_thesis" "claim_sources" "excluded_sources" "data_gaps")))"#;
        assert!(
            skill_md.contains(field_check),
            "industry-outside-view field check must stay pinned in company-research-deep SKILL.md"
        );
        let complete = hkask_lisp::eval_sandboxed_with_budget(
            field_check,
            &json!({"view": {"industry_drivers": [], "self_regulatory_bodies": [], "company_fit": [], "frame_conflicts": [], "implication_for_falstaffian": "", "implication_for_gorilla": "", "implication_for_thesis": "", "claim_sources": [], "excluded_sources": [], "data_gaps": []}}),
            100_000,
            64,
        )
        .expect("field check must evaluate");
        assert_eq!(complete, json!([]), "all ten fields present passes");
        let incomplete = hkask_lisp::eval_sandboxed_with_budget(
            field_check,
            &json!({"view": {"industry_drivers": [], "self_regulatory_bodies": [], "frame_conflicts": [], "implication_for_falstaffian": "", "implication_for_gorilla": "", "implication_for_thesis": "", "claim_sources": [], "excluded_sources": [], "data_gaps": []}}),
            100_000,
            64,
        )
        .expect("field check must evaluate on a missing field");
        assert_eq!(
            incomplete,
            json!(["company_fit"]),
            "a missing field is named for re-render"
        );

        // 3. The GORILLA fixed-weight scoring form: three verdict bands plus
        // the maturity-block case (a blocked dimension binds to 0).
        let gorilla = r#"(let ((score (+ (* 0.25 obvious_problem) (* 0.30 invisible_gorilla) (* 0.25 combinatorial_solution) (* 0.20 choke_point)))) (cond ((>= score 75) 'GORILLA) ((>= score 50) 'SMALL_ANIMAL) (t 'PEDESTRIAN)))"#;
        assert!(
            skill_md.contains(gorilla),
            "GORILLA fixed-weight scoring form must stay pinned in company-research-deep SKILL.md"
        );
        let top = hkask_lisp::eval_sandboxed_with_budget(
            gorilla,
            &json!({"obvious_problem": 80, "invisible_gorilla": 85, "combinatorial_solution": 75, "choke_point": 80}),
            100_000,
            64,
        )
        .expect("GORILLA form must evaluate");
        assert_eq!(top, json!("GORILLA"), "80.25 is a GORILLA");
        let mid = hkask_lisp::eval_sandboxed_with_budget(
            gorilla,
            &json!({"obvious_problem": 60, "invisible_gorilla": 55, "combinatorial_solution": 50, "choke_point": 60}),
            100_000,
            64,
        )
        .expect("GORILLA form must evaluate on a mid case");
        assert_eq!(mid, json!("SMALL_ANIMAL"), "56 is a SMALL_ANIMAL");
        let low = hkask_lisp::eval_sandboxed_with_budget(
            gorilla,
            &json!({"obvious_problem": 30, "invisible_gorilla": 40, "combinatorial_solution": 35, "choke_point": 30}),
            100_000,
            64,
        )
        .expect("GORILLA form must evaluate on a low case");
        assert_eq!(low, json!("PEDESTRIAN"), "34.25 is a PEDESTRIAN");
        let blocked = hkask_lisp::eval_sandboxed_with_budget(
            gorilla,
            &json!({"obvious_problem": 80, "invisible_gorilla": 0, "combinatorial_solution": 75, "choke_point": 80}),
            100_000,
            64,
        )
        .expect("GORILLA form must evaluate on a maturity-blocked dimension");
        assert_eq!(
            blocked,
            json!("SMALL_ANIMAL"),
            "a maturity-blocked dimension binds to 0 and drops the verdict from GORILLA to SMALL_ANIMAL"
        );
    }

    #[test]
    fn test_swarm_intelligence_skill_md_pins_forms() {
        // swarm-intelligence pins five SKILL.md lisp_eval forms plus the
        // loop_closure ratio pinned in the check template body — the
        // convergence arithmetic the model never supplies. If any drifts,
        // this fails until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/swarm-intelligence/SKILL.md"
        ))
        .expect("swarm-intelligence SKILL.md must exist in the workspace");

        // 1. The swarm-state distance form: three health axes, plus the
        // task-success fourth axis when an oracle exists.
        let distance = r#"(let ((base (+ (* (- 1 vc) (- 1 vc)) (let ((g (max 0 (- 0.25 div)))) (* g g)) (* (- 1 lc) (- 1 lc))))) (sqrt (if (is_null s) base (+ base (* (- 1 s) (- 1 s))))))"#;
        assert!(
            skill_md.contains(distance),
            "swarm-state distance form must stay pinned in swarm-intelligence SKILL.md"
        );
        let on_target = hkask_lisp::eval_sandboxed_with_budget(
            distance,
            &json!({"vc": 0.95, "div": 0.4, "lc": 1.0, "s": null}),
            100_000,
            64,
        )
        .expect("distance form must evaluate");
        assert_eq!(
            on_target,
            json!(0.050000000000000044),
            "on-target three-axis distance is 0.05"
        );
        let task_failed = hkask_lisp::eval_sandboxed_with_budget(
            distance,
            &json!({"vc": 0.95, "div": 0.4, "lc": 1.0, "s": 0.0}),
            100_000,
            64,
        )
        .expect("distance form must evaluate on a task failure");
        assert_eq!(
            task_failed,
            json!(1.0012492197250393),
            "a healthy swarm that fails the task must NOT converge — the fourth axis dominates"
        );

        // 2. The target-plus-stability gate: the algedonic override and the
        // task-success requirement each block convergence independently.
        let gate = r#"(and (= (length ds) 4) (< (abs (- (nth 1 ds) (nth 0 ds))) 0.03) (< (abs (- (nth 2 ds) (nth 1 ds))) 0.03) (< (abs (- (nth 3 ds) (nth 2 ds))) 0.03) (>= vc 0.9) (>= div 0.25) (= lc 1) coherence_non_decreasing no_algedonic_alert (or (not task_success_required) (and (numberp s) (= s 1))))"#;
        assert!(
            skill_md.contains(gate),
            "target-plus-stability gate must stay pinned in swarm-intelligence SKILL.md"
        );
        let converged = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"ds": [0.05, 0.05, 0.05, 0.05], "vc": 0.95, "div": 0.4, "lc": 1, "coherence_non_decreasing": true, "no_algedonic_alert": true, "task_success_required": true, "s": 1}),
            100_000,
            64,
        )
        .expect("gate form must evaluate");
        assert_eq!(converged, json!(true));
        let algedonic = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"ds": [0.05, 0.05, 0.05, 0.05], "vc": 0.95, "div": 0.4, "lc": 1, "coherence_non_decreasing": true, "no_algedonic_alert": false, "task_success_required": true, "s": 1}),
            100_000,
            64,
        )
        .expect("gate form must evaluate on an algedonic alert");
        assert_eq!(
            algedonic,
            json!(false),
            "a broken algedonic channel is never read as no deviation — the override"
        );

        // 3. The receipt-coverage (same-names) form: order and identity of
        // returned results against the submitted delegations, 1-10 entries.
        let receipts = r#"(begin (define same-names (lambda (a b) (if (= (length a) 0) (= (length b) 0) (if (= (length b) 0) nil (and (string= (car a) (car b)) (same-names (cdr a) (cdr b))))))) (and (> (length expected_names) 0) (<= (length expected_names) 10) (same-names expected_names observed_names)))"#;
        assert!(
            skill_md.contains(receipts),
            "receipt-coverage form must stay pinned in swarm-intelligence SKILL.md"
        );
        let matched = hkask_lisp::eval_sandboxed_with_budget(
            receipts,
            &json!({"expected_names": ["alpha", "beta", "gamma"], "observed_names": ["alpha", "beta", "gamma"]}),
            100_000,
            64,
        )
        .expect("receipt form must evaluate");
        assert_eq!(matched, json!(true));
        let mismatched = hkask_lisp::eval_sandboxed_with_budget(
            receipts,
            &json!({"expected_names": ["alpha", "beta", "gamma"], "observed_names": ["alpha", "delta", "gamma"]}),
            100_000,
            64,
        )
        .expect("receipt form must evaluate on a mismatch");
        assert_eq!(
            mismatched,
            json!(false),
            "a count/order mismatch blocks feedback"
        );

        // 4. The C3 failed-signature comparison: a prior failure blocks an
        // identical repeat; an empty history does not clear the move.
        let c3 = r#"(not (member proposed_signature failed_signatures))"#;
        assert!(
            skill_md.contains(c3),
            "C3 failed-signature form must stay pinned in swarm-intelligence SKILL.md"
        );
        let blocked = hkask_lisp::eval_sandboxed_with_budget(
            c3,
            &json!({"proposed_signature": "hire:analyst|variety_deficit|analyst,researcher", "failed_signatures": ["hire:analyst|variety_deficit|analyst,researcher", "reconfigure:analyst|coherence_deficit|analyst"]}),
            100_000,
            64,
        )
        .expect("C3 form must evaluate");
        assert_eq!(
            blocked,
            json!(false),
            "an identical repeat of a failed signature is blocked"
        );
        let novel = hkask_lisp::eval_sandboxed_with_budget(
            c3,
            &json!({"proposed_signature": "hire:writer|variety_deficit|analyst,researcher", "failed_signatures": ["hire:analyst|variety_deficit|analyst,researcher"]}),
            100_000,
            64,
        )
        .expect("C3 form must evaluate on a novel move");
        assert_eq!(
            novel,
            json!(true),
            "a novel signature is not blocked by C3 (not proof of safety)"
        );

        // 4b. The C7 influence guard: measured negative influence rejects
        // the hire; an absent score is not deterministic rejection.
        let c7 = r#"(or (not (assoc agent_type influence_scores)) (> (assoc agent_type influence_scores) 0))"#;
        assert!(
            skill_md.contains(c7),
            "C7 influence guard form must stay pinned in swarm-intelligence SKILL.md"
        );
        let degraded = hkask_lisp::eval_sandboxed_with_budget(
            c7,
            &json!({"agent_type": "researcher", "influence_scores": {"researcher": -0.2, "writer": 0.5}}),
            100_000,
            64,
        )
        .expect("C7 form must evaluate");
        assert_eq!(
            degraded,
            json!(false),
            "a measured negative influence score rejects the hire"
        );
        let unknown = hkask_lisp::eval_sandboxed_with_budget(
            c7,
            &json!({"agent_type": "novel_agent", "influence_scores": {"researcher": -0.2, "writer": 0.5}}),
            100_000,
            64,
        )
        .expect("C7 form must evaluate on an absent score");
        assert_eq!(
            unknown,
            json!(true),
            "an absent influence score is unknown history, not deterministic rejection"
        );

        // 5. The loop_closure ratio — pinned in the check template body
        // (swarm-check.j2), asserted here so the pin is durable.
        let check_tpl = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../kask/registry/templates/swarm-intelligence/swarm-check.j2"
        ))
        .expect("swarm-check.j2 must exist in the workspace");
        let loop_closure =
            r#"(if (= attempted_delegations 0) 1 (/ ordered_receipts attempted_delegations))"#;
        assert!(
            check_tpl.contains(loop_closure),
            "loop_closure ratio must stay pinned in swarm-check.j2"
        );
        let vacuous = hkask_lisp::eval_sandboxed_with_budget(
            loop_closure,
            &json!({"attempted_delegations": 0, "ordered_receipts": 0}),
            100_000,
            64,
        )
        .expect("loop_closure form must evaluate");
        assert_eq!(
            vacuous,
            json!(1),
            "zero actual delegations is vacuously closed"
        );
        let partial = hkask_lisp::eval_sandboxed_with_budget(
            loop_closure,
            &json!({"attempted_delegations": 4, "ordered_receipts": 2}),
            100_000,
            64,
        )
        .expect("loop_closure form must evaluate on partial receipts");
        assert_eq!(
            partial,
            json!(0.5),
            "partial receipt coverage halves loop_closure"
        );
    }

    #[test]
    fn test_wardley_mapper_skill_md_pins_forms() {
        // wardley-mapper pins one lisp_eval form — the Convergence gate
        // over unclassified components and unresolved dependencies. If it
        // drifts, this fails until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/wardley-mapper/SKILL.md"
        ))
        .expect("wardley-mapper SKILL.md must exist in the workspace");
        let gate = r#"(and (eq (length unclassified_components) 0) (eq (length unresolved_dependencies) 0))"#;
        assert!(
            skill_md.contains(gate),
            "Convergence gate form must stay pinned in wardley-mapper SKILL.md"
        );
        let converged = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"unclassified_components": [], "unresolved_dependencies": []}),
            100_000,
            64,
        )
        .expect("Convergence gate must evaluate");
        assert_eq!(converged, json!(true), "an empty gap list converges");
        let gaps = hkask_lisp::eval_sandboxed_with_budget(
            gate,
            &json!({"unclassified_components": ["kafka-broker"], "unresolved_dependencies": []}),
            100_000,
            64,
        )
        .expect("Convergence gate must evaluate on an unclassified component");
        assert_eq!(
            gaps,
            json!(false),
            "one unclassified component blocks convergence and names the re-entry"
        );
    }

    #[test]
    fn test_upstream_rebase_skill_md_pins_forms() {
        // upstream-rebase pins one lisp_eval form — the per-file strategy
        // decision rule (git-merge vs mapped-reapplication) over the four
        // counts assess.j2 gathers. The SKILL.md documents five tested
        // cases; all five are executed here. If the form drifts, this fails
        // until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/upstream-rebase/SKILL.md"
        ))
        .expect("upstream-rebase SKILL.md must exist in the workspace");
        let rule = r#"(let ((dens (if (= sites 0) 1 (/ markers sites)))) (list dens (if (or (> fork (* 2 upstream)) (< dens 0.5)) (quote mapped-reapplication) (quote git-merge))))"#;
        assert!(
            skill_md.contains(rule),
            "strategy decision rule must stay pinned in upstream-rebase SKILL.md"
        );
        let case = |fork: i64, upstream: i64, markers: i64, sites: i64| {
            hkask_lisp::eval_sandboxed_with_budget(
                rule,
                &json!({"fork": fork, "upstream": upstream, "markers": markers, "sites": sites}),
                100_000,
                64,
            )
            .expect("strategy rule must evaluate")
        };
        // The five documented cases, in the SKILL.md's order.
        assert_eq!(
            case(100, 100, 5, 10),
            json!([0.5, "git-merge"]),
            "density exactly 0.5 merges"
        );
        assert_eq!(
            case(100, 100, 4, 10),
            json!([0.4, "mapped-reapplication"]),
            "density 0.4 re-applies"
        );
        assert_eq!(
            case(201, 100, 10, 10),
            json!([1.0, "mapped-reapplication"]),
            "fork > 2x upstream re-applies regardless of density"
        );
        assert_eq!(
            case(50, 100, 0, 0),
            json!([1, "git-merge"]),
            "zero sites is vacuous density 1, merges"
        );
        assert_eq!(
            case(250, 100, 0, 0),
            json!([1, "mapped-reapplication"]),
            "fork > 2x upstream with zero sites still re-applies"
        );
    }

    #[test]
    fn test_diataxis_diagram_skill_md_pins_forms() {
        // diataxis-diagram pins one lisp_eval form — the six-criterion
        // weighted rubric total, a labelled estimate that chooses
        // refinements and never gates. If it drifts, this fails until
        // skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/diataxis-diagram/SKILL.md"
        ))
        .expect("diataxis-diagram SKILL.md must exist in the workspace");
        let rubric =
            r#"(+ (* c1 0.30) (* c2 0.25) (* c3 0.15) (* c4 0.15) (* c5 0.10) (* c6 0.05))"#;
        assert!(
            skill_md.contains(rubric),
            "weighted rubric form must stay pinned in diataxis-diagram SKILL.md"
        );
        let deficient = hkask_lisp::eval_sandboxed_with_budget(
            rubric,
            &json!({"c1": 0.2, "c2": 0.1, "c3": 0.0, "c4": 0.0, "c5": 0.0, "c6": 0.0}),
            100_000,
            64,
        )
        .expect("rubric form must evaluate");
        assert_eq!(
            deficient,
            json!(0.08499999999999999),
            "c1=0.2, c2=0.1 weighs 0.06 + 0.025 = 0.085"
        );
        let perfect = hkask_lisp::eval_sandboxed_with_budget(
            rubric,
            &json!({"c1": 0.0, "c2": 0.0, "c3": 0.0, "c4": 0.0, "c5": 0.0, "c6": 0.0}),
            100_000,
            64,
        )
        .expect("rubric form must evaluate on a perfect diagram");
        assert_eq!(
            perfect,
            json!(0.0),
            "all-zero criteria weigh 0 — no refinement directives"
        );
    }

    #[test]
    fn test_sankey_flow_skill_md_pins_forms() {
        // sankey-flow pins two lisp_eval forms — the per-node inflow/outflow
        // extraction and the balance check over its rows. The empty-rows
        // case returns true from the form; the SKILL's guard (empty means
        // unverified, never vacuously balanced) is the interpretation
        // discipline. If either form drifts, this fails until skill and
        // tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/sankey-flow/SKILL.md"
        ))
        .expect("sankey-flow SKILL.md must exist in the workspace");

        // 1. The node-rows form: [node, inflow, outflow] per internal node.
        let node_rows_form = r#"(begin
     (define sum-for (lambda (node es field) (if (= (length es) 0) 0 (+ (if (string= (assoc field (car es)) node) (assoc "weight" (car es)) 0) (sum-for node (cdr es) field)))))
     (define has-out (lambda (node es) (if (= (length es) 0) nil (or (string= node (assoc "source" (car es))) (has-out node (cdr es))))))
     (define rows (lambda (rest all) (if (= (length rest) 0) (list) (let ((node (assoc "target" (car rest)))) (if (has-out node all) (cons (list node (sum-for node all "target") (sum-for node all "source")) (rows (cdr rest) all)) (rows (cdr rest) all))))))
     (rows edges edges))"#;
        assert!(
            skill_md.contains(node_rows_form),
            "node-rows form must stay pinned in sankey-flow SKILL.md"
        );
        let pipeline = json!([
            {"source": "Raw Events", "target": "Kafka", "weight": 1200},
            {"source": "Kafka", "target": "Stream Validator", "weight": 1200},
            {"source": "Stream Validator", "target": "Dead Letter Queue", "weight": 80},
            {"source": "Stream Validator", "target": "Enricher", "weight": 1120},
            {"source": "Enricher", "target": "Warehouse", "weight": 1100},
            {"source": "Enricher", "target": "Quarantine", "weight": 20}
        ]);
        let rows = hkask_lisp::eval_sandboxed_with_budget(
            node_rows_form,
            &json!({"edges": pipeline}),
            100_000,
            64,
        )
        .expect("node-rows form must evaluate");
        assert_eq!(
            rows,
            json!([
                ["Kafka", 1200, 1200],
                ["Stream Validator", 1200, 1200],
                ["Enricher", 1120, 1120]
            ]),
            "internal nodes emit [node, inflow, outflow]; terminal nodes are excluded"
        );

        // 2. The balance check over the extracted rows.
        let balanced_form = r#"(begin (define balanced (lambda (rows epsilon) (if (= (length rows) 0) t (and (<= (abs (- (nth 1 (car rows)) (nth 2 (car rows)))) epsilon) (balanced (cdr rows) epsilon))))) (balanced node_rows epsilon))"#;
        assert!(
            skill_md.contains(balanced_form),
            "balance-check form must stay pinned in sankey-flow SKILL.md"
        );
        let balanced = hkask_lisp::eval_sandboxed_with_budget(
            balanced_form,
            &json!({"node_rows": [["Kafka", 1200, 1200], ["Stream Validator", 1200, 1200], ["Enricher", 1120, 1120]], "epsilon": 0.01}),
            100_000,
            64,
        )
        .expect("balance form must evaluate");
        assert_eq!(balanced, json!(true), "balanced rows pass within epsilon");
        let discrepancy = hkask_lisp::eval_sandboxed_with_budget(
            balanced_form,
            &json!({"node_rows": [["Kafka", 1200, 1200], ["Enricher", 1120, 1020]], "epsilon": 0.01}),
            100_000,
            64,
        )
        .expect("balance form must evaluate on a discrepancy");
        assert_eq!(
            discrepancy,
            json!(false),
            "a 100-unit inflow/outflow gap is a discrepancy with node-level values"
        );
        let vacuous = hkask_lisp::eval_sandboxed_with_budget(
            balanced_form,
            &json!({"node_rows": [], "epsilon": 0.01}),
            100_000,
            64,
        )
        .expect("balance form must evaluate on empty rows");
        assert_eq!(
            vacuous,
            json!(true),
            "the form returns true on empty; the SKILL's guard marks it unverified, never vacuously balanced"
        );
    }

    #[test]
    fn test_diagnose_skill_md_pins_forms() {
        // diagnose pins two lisp_eval forms — the hypothesis invariant check
        // (count, completeness, diversity, mutual exclusivity — instrumentation
        // is gated on it) and the convergence score (normalized by 0.85). If
        // either drifts, this fails until skill and tests are reconciled.
        let skill_md = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.agents/skills/diagnose/SKILL.md"
        ))
        .expect("diagnose SKILL.md must exist in the workspace");

        // 1. The hypothesis invariant check.
        let invariant = r#"(let ((hyps hypotheses))
     (begin
       (define walk-check (lambda (hs defects)
         (if (is_null hs)
           defects
           (walk-check (cdr hs)
             (append defects
               (if (is_null (assoc "prediction" (car hs))) (list "missing_prediction") nil)
               (if (is_null (assoc "falsifier" (car hs))) (list "missing_falsifier") nil))))))
       (define get-texts (lambda (hs acc)
         (if (is_null hs) acc (get-texts (cdr hs) (append acc (list (assoc "hypothesis" (car hs))))))))
       (define get-likes (lambda (hs acc)
         (if (is_null hs) acc (get-likes (cdr hs) (append acc (list (assoc "likelihood" (car hs))))))))
       (define has-dupes (lambda (ts)
         (if (is_null ts) nil
           (if (member (car ts) (cdr ts)) (list "duplicate_hypothesis_text") (has-dupes (cdr ts))))))
       (define diff-exists (lambda (ls first)
         (if (is_null ls) nil
           (if (string= (car ls) first) (diff-exists (cdr ls) first) (list "diverse")))))
       (define texts (get-texts hyps nil))
       (define likes (get-likes hyps nil))
       (define defects
         (append
           (if (or (< (length hyps) 3) (> (length hyps) 7)) (list "count_out_of_range_3_to_7") nil)
           (walk-check hyps nil)
           (has-dupes texts)
           (if (is_null likes) nil
             (if (is_null (diff-exists (cdr likes) (car likes))) (list "no_likelihood_diversity") nil))))
       (if (> (length defects) 0) defects 'ok)))"#;
        assert!(
            skill_md.contains(invariant),
            "hypothesis invariant check must stay pinned in diagnose SKILL.md"
        );
        let clean = hkask_lisp::eval_sandboxed_with_budget(
            invariant,
            &json!({"hypotheses": [
                {"hypothesis": "off-by-one in loop bound", "likelihood": "high", "prediction": "changing the bound fixes it", "falsifier": "probe X shows Y"},
                {"hypothesis": "race on shared counter", "likelihood": "medium", "prediction": "locking removes it", "falsifier": "probe Z shows W"},
                {"hypothesis": "stale cache entry", "likelihood": "low", "prediction": "invalidating cache fixes it", "falsifier": "probe Q shows R"}
            ]}),
            100_000,
            64,
        )
        .expect("invariant check must evaluate");
        assert_eq!(clean, json!("ok"), "a clean 3-hypothesis set passes");
        let defective = hkask_lisp::eval_sandboxed_with_budget(
            invariant,
            &json!({"hypotheses": [
                {"hypothesis": "same text", "likelihood": "high", "prediction": "pred A", "falsifier": "probe X"},
                {"hypothesis": "same text", "likelihood": "high", "prediction": "pred B", "falsifier": "probe Y"}
            ]}),
            100_000,
            64,
        )
        .expect("invariant check must evaluate on a defective set");
        assert_eq!(
            defective,
            json!([
                "count_out_of_range_3_to_7",
                "duplicate_hypothesis_text",
                "no_likelihood_diversity"
            ]),
            "count, duplicate text, and uniform likelihood are all named — instrumentation is gated on this"
        );
        let missing_pred = hkask_lisp::eval_sandboxed_with_budget(
            invariant,
            &json!({"hypotheses": [
                {"hypothesis": "h1", "likelihood": "high", "falsifier": "probe X"},
                {"hypothesis": "h2", "likelihood": "medium", "prediction": "pred B", "falsifier": "probe Y"},
                {"hypothesis": "h3", "likelihood": "low", "prediction": "pred C", "falsifier": "probe Z"}
            ]}),
            100_000,
            64,
        )
        .expect("invariant check must evaluate on a missing prediction");
        assert_eq!(
            missing_pred,
            json!(["missing_prediction"]),
            "a hypothesis without a prediction is named before any instrumentation runs"
        );

        // 2. The convergence score (normalized by 0.85).
        let convergence = r#"(/ (+ (* 0.25 a) (* 0.15 r) (* 0.20 f) (* 0.15 e) (* 0.10 c)) 0.85)"#;
        assert!(
            skill_md.contains(convergence),
            "convergence score form must stay pinned in diagnose SKILL.md"
        );
        let all_met = hkask_lisp::eval_sandboxed_with_budget(
            convergence,
            &json!({"a": 0, "r": 0, "f": 0, "e": 0, "c": 0}),
            100_000,
            64,
        )
        .expect("convergence form must evaluate");
        assert_eq!(all_met, json!(0.0), "all met = 0.00 — root cause confirmed");
        let ambiguous = hkask_lisp::eval_sandboxed_with_budget(
            convergence,
            &json!({"a": 1, "r": 0, "f": 0, "e": 0, "c": 0}),
            100_000,
            64,
        )
        .expect("convergence form must evaluate on an ambiguous root cause");
        assert_eq!(
            ambiguous,
            json!(0.29411764705882354),
            "root cause ambiguous alone = 0.25/0.85 — above the 0.25 convergence threshold"
        );
    }
}
