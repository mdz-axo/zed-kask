use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput, deserialize_maybe_stringified};
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
    /// `numberp`, `listp`, `assoc`, `append`, `member`, `abs`, `sqrt`, `eq`,
    /// `string=`, `string-contains`, `concat`.
    form: String,
    /// JSON object whose keys become top-level Lisp bindings. Values are
    /// converted to Lisp values: objects become association lists, arrays
    /// become lists, numbers stay numbers, strings stay strings.
    ///
    /// Uses `HashMap<String, AnyJsonValue>` (not `serde_json::Value`) so the
    /// generated schema is `{"type":"object","additionalProperties":{}}` —
    /// a bare `AnyJsonValue` emits `{}` (any value), which the model doesn't
    /// populate; a bare `serde_json::Value` emits `true`, which strict-schema
    /// providers reject outright. The `HashMap` shape gives the model a clear
    /// `type: object` signal to send a JSON object.
    ///
    /// `deserialize_maybe_stringified` tolerates models that emit `env` as a
    /// stringified JSON string (e.g. `"{}"`) instead of a bare object — the
    /// same pattern `edit_file.edits` uses. Without it, a stringified `env`
    /// fails with "invalid type: string, expected a map" and the tool errors
    /// out, wasting a turn.
    #[serde(default, deserialize_with = "deserialize_maybe_stringified")]
    env: std::collections::HashMap<String, hkask_types::AnyJsonValue>,
    /// Maximum evaluation steps (default 100000). Prevents infinite loops.
    #[serde(default = "default_max_steps")]
    max_steps: u64,
    /// Maximum evaluation depth (default 1024). Prevents infinite recursion.
    /// Recursive helper forms over lists consume roughly 2–4 depth frames
    /// per element, so the former default of 64 overflowed at ~16 elements —
    /// real-scale validation lists (100+ claims) failed on the first attempt
    /// and wasted turns on retries (observed live: a 134-element list needed
    /// 300). 1024 covers realistic registries out of the box; genuinely
    /// infinite recursion still trips the budget immediately.
    #[serde(default = "default_max_depth")]
    max_depth: u64,
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

            let env_value = serde_json::Value::Object(
                input.env.into_iter().map(|(k, v)| (k, v.into())).collect(),
            );
            let result = hkask_lisp::eval_sandboxed_with_budget(
                &input.form,
                &env_value,
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
    // (e.g. `"{}"`) instead of a bare object, `deserialize_maybe_stringified`
    // parses the string and the tool succeeds. Without it, the deserializer
    // rejects with "invalid type: string, expected a map" and the tool errors
    // out. This is the same pattern `edit_file.edits` uses.
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
    // form shipped broken (`min`/`mapcar` are not builtins, and symbol keys
    // never match JSON string keys) because nothing ran it.

    #[test]
    fn test_canonical_fact_score_form() {
        // grounding-verify SKILL.md Step 5 — fact_score with nil-propagation.
        let form = r#"(if (or (member nil (list sar cvr hfr nlr)) (= claims_checked 0)) 'nil (let ((score (+ (* 0.30 sar) (* 0.25 cvr) (* 0.20 hfr) (* 0.25 nlr)))) score))"#;
        let ok = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"sar": 0.9, "cvr": 0.8, "hfr": 1.0, "nlr": 0.9, "claims_checked": 10}),
            100_000,
            64,
        )
        .expect("fact_score form must evaluate");
        let score = ok.as_f64().expect("happy path returns a number");
        assert!((score - 0.895).abs() < 1e-9, "got {score}");

        // A nil sub-metric must propagate to nil — never a zero-fallback score.
        let nil = hkask_lisp::eval_sandboxed_with_budget(
            form,
            &json!({"sar": null, "cvr": 0.8, "hfr": 1.0, "nlr": 0.9, "claims_checked": 10}),
            100_000,
            64,
        )
        .expect("nil-path form must evaluate");
        assert_eq!(nil, json!(null), "nil sub-metric must yield null, not 0");
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
            skill_md.contains(
                "(if (or (member nil (list sar cvr hfr nlr)) (= claims_checked 0)) 'nil (let ((score (+ (* 0.30 sar) (* 0.25 cvr) (* 0.20 hfr) (* 0.25 nlr)))) score))"
            ),
            "fact_score form must stay pinned in grounding-verify SKILL.md"
        );
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
        assert!(
            !skill_md.contains("combine_tree_probabilities"),
            "combine_tree_probabilities exists nowhere — the phantom tool reference was removed"
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
    }
}
