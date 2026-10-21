//! Emission-variant matrix for the `lisp_eval` tool input surface
//! (lisp-repair L0 failure-class inventory).
//!
//! Every variant here is a shape a temperature-0 model has been observed or
//! is plausibly able to emit for `env` / `form` / the budget fields, driven
//! through the REAL deserialization path (`serde_json::from_value` — which
//! exercises `deserialize_env_field` / the `deserialize_teaching_field` core
//! via the serde attribute) and then `evaluate_lisp`. Each test pins the L1
//! outcome: the messages are the L1 TEACHING messages (field + received shape
//! + accepted shape); each comment carries the pre-L1 message as the
//! before/after receipt (lisp-repair L1: an error that does not teach
//! produces identical retries — the 2026-09-28 audit lockout anatomy).
//!
//! Failure classes (lisp-repair contract): `refuse` = typed rejection
//! (acceptable when actionable), `lie` = silent wrong answer (never
//! acceptable), `hang` = budget miss surfaced as a typed error (acceptable),
//! `crash` = panic (never acceptable), `lockout` = framework refusal of a
//! corrected/varying input (never acceptable). The `unteachable` label marks
//! the worst refuse sub-class: a shape that deserializes cleanly but leaves
//! the form's bindings missing, producing an error that does not name the
//! actual defect — the anatomy of the 2026-09-28 audit lockout (5 identical
//! malformed-env retries hard-refused by the per-input tracker). L1 closed
//! the unteachable class: the unbound-symbol error now names the available
//! bindings and the flatten fix.

use serde_json::{Value, json};

use super::lisp_eval_tool::{LispEvalToolInput, evaluate_lisp};

/// The real path: deserialize the tool arguments, then evaluate.
fn run(args: Value) -> Result<Value, String> {
    let input: LispEvalToolInput =
        serde_json::from_value(args).map_err(|e| format!("deserialize: {e}"))?;
    evaluate_lisp(input)
}

/// expect: "Background live decode and evaluation preserve env and budget failures through the erased wire" [H5]
#[gpui::test]
async fn h5_live_run_preserves_input_and_budget_errors(cx: &mut gpui::TestAppContext) {
    use super::lisp_eval_tool::LispEvalTool;
    use crate::{AgentTool, ToolCallEventStream, ToolInput};
    use language_model::LanguageModelToolResultContent;

    for (input, expected) in [
        (json!({"form": "(+ a b)", "env": "{unfinished"}), "env"),
        (json!({"form": "(+ a b)", "env": {"a": 2}}), "env"),
        (json!({"form": "(+ a b)", "env": "[]"}), "env"),
        (
            json!({"form": "(+ 2 3)", "max_steps": 2_000_001}),
            "max_steps",
        ),
        (json!({"form": "(+ 2 3)", "max_depth": 8193}), "max_depth"),
        (json!({"form": "(+ 2 3)", "max_steps": 0}), "max_steps"),
    ] {
        let (events, _rx) = ToolCallEventStream::test();
        let result = cx
            .update(|cx| {
                LispEvalTool
                    .erase()
                    .run(ToolInput::ready(input), events, cx)
            })
            .await;
        let Err(error) = result else {
            panic!("invalid live input must fail")
        };
        let message = error.raw_output["error"]
            .as_str()
            .expect("unchanged error wire");
        assert!(message.contains(expected), "{message}");
        let [LanguageModelToolResultContent::Text(text)] = error.llm_output.as_slice() else {
            panic!("unchanged text error content")
        };
        assert_eq!(text.as_ref(), message);
    }
}

/// expect: "Actual tool run and content conversion enforce pretty bytes without null substitution" [H4B]
#[gpui::test]
async fn h4b_tool_output_document_limit(cx: &mut gpui::TestAppContext) {
    use super::lisp_eval_tool::{LispEvalTool, LispEvalToolOutput};
    use crate::{AgentTool, ToolCallEventStream, ToolInput};
    use language_model::LanguageModelToolResultContent;
    use std::sync::Arc;

    for result in [
        Value::Null,
        json!(-7),
        json!({"é\n": [true, "\0\\\"", 2.5]}),
    ] {
        let expected = serde_json::to_string_pretty(&result).expect("serde oracle");
        let output = LispEvalToolOutput::Success {
            result: result.clone(),
        };
        assert_eq!(
            serde_json::to_value(&output).expect("success shape"),
            json!({"result": result})
        );
        let content: LanguageModelToolResultContent = output.into();
        let LanguageModelToolResultContent::Text(text) = content else {
            panic!("text result")
        };
        assert_eq!(text.as_ref(), expected);
    }
    let error = LispEvalToolOutput::Error {
        error: "visible error".into(),
    };
    assert_eq!(
        serde_json::to_value(&error).expect("error shape"),
        json!({"error": "visible error"})
    );
    let content: LanguageModelToolResultContent = error.into();
    let LanguageModelToolResultContent::Text(text) = content else {
        panic!("text error")
    };
    assert_eq!(text.as_ref(), "visible error");

    let env = json!({"seed": "\0".repeat(174_759)}).to_string();
    assert!(env.len() < hkask_lisp::MAX_ENV_JSON_BYTES);
    for padding in [20, 21] {
        let input: LispEvalToolInput = serde_json::from_value(json!({
            "form": format!("(concat seed \"{}\")", "x".repeat(padding)),
            "env": env,
            "maxSteps": 2_000_000
        }))
        .expect("real JSON-text decoder admits fixture");
        let (events, _rx) = ToolCallEventStream::test();
        let output = cx
            .update(|cx| Arc::new(LispEvalTool).run(ToolInput::resolved(input), events, cx))
            .await;
        let output = if padding == 20 {
            let output = output.expect("exact boundary succeeds");
            assert!(matches!(output, LispEvalToolOutput::Success { .. }));
            assert!(
                serde_json::to_value(&output)
                    .expect("wire output")
                    .get("result")
                    .is_some()
            );
            output
        } else {
            let Err(error) = output else {
                panic!("oversized actual run must return Err")
            };
            assert!(matches!(&error, LispEvalToolOutput::Error { error }
                if error.contains("max_output_bytes") && error.contains("1048576")));
            assert!(
                serde_json::to_value(&error)
                    .expect("wire error")
                    .get("error")
                    .is_some()
            );
            error
        };
        let content: LanguageModelToolResultContent = output.into();
        let LanguageModelToolResultContent::Text(text) = content else {
            panic!("text result")
        };
        if padding == 20 {
            assert_eq!(text.len(), 1_048_576);
            assert_eq!(
                serde_json::from_str::<Value>(&text).expect("legal JSON"),
                json!(format!("{}{}", "\0".repeat(174_759), "x".repeat(padding)))
            );
        } else {
            assert!(text.contains("max_output_bytes"));
        }
    }
    let content: LanguageModelToolResultContent = LispEvalToolOutput::Success {
        result: json!("x".repeat(1_048_575)),
    }
    .into();
    let LanguageModelToolResultContent::Text(text) = content else {
        panic!("text result")
    };
    assert!(
        text.contains("max_output_bytes"),
        "direct conversion must not bypass bound"
    );
    assert_ne!(text.as_ref(), "null");
}

/// expect: "Tool numeric errors name the operation/input instead of returning null or rounded success" [P8]
#[test]
fn h3_numeric_failures_surface_through_real_tool_decode() {
    for (form, message) in [
        ("1e309", "1e309"),
        ("NaN", "NaN"),
        ("(* 1e308 2.0)", "*"),
        ("(/ 9223372036854775807 1)", "9223372036854775807"),
        (
            "(= 9007199254740993 9007199254740992.0)",
            "9007199254740993",
        ),
        ("(sqrt 9007199254740993)", "sqrt"),
    ] {
        let error = run(json!({"form": form})).expect_err("unsafe numeric result must fail");
        assert!(error.contains(message), "{form}: {error}");
    }
    let error = run(json!({"form": "(+ n 0.0)", "env": r#"{"n":9007199254740993}"#}))
        .expect_err("exact input integer cannot be rounded during promotion");
    assert!(
        error.contains("9007199254740993") && error.contains("exact"),
        "{error}"
    );
    assert_eq!(
        run(json!({"form": "(+ n 0.0)", "env": r#"{"n":9007199254740994}"#}))
            .expect("representable large integer"),
        json!(9007199254740994.0)
    );
    assert_eq!(
        run(json!({"form": "n", "env": r#"{"n":1e100}"#})).expect("explicit large finite float"),
        json!(1e100)
    );
    assert_eq!(
        run(json!({"form": "(/ 5 2)"})).expect("float division"),
        json!(2.5)
    );
}

/// expect: "Raw env admission counts bytes before structure, numeric, and shared decoding" [H4A]
#[test]
fn h4a_raw_env_byte_admission() {
    for object in [r#"{"n":1}"#, r#"{"é":1}"#] {
        let env = format!("{object}{}", " ".repeat(1_048_576 - object.len()));
        assert_eq!(env.len(), 1_048_576);
        assert_eq!(
            run(json!({"form": "1", "env": env, "max_steps": 2_000_000})),
            Ok(json!(1))
        );
        let malformed = format!("{env}!");
        let error = run(json!({"form": "1", "env": malformed}))
            .expect_err("cap+1 must be refused before parsing invalid tail");
        assert!(
            error.contains("env") && error.contains("1048577") && error.contains("1048576"),
            "{error}"
        );
        let multibyte_over = format!("{object}{}", " ".repeat(1_048_577 - object.len()));
        if !object.is_ascii() {
            assert_eq!(multibyte_over.chars().count(), 1_048_576);
        }
        let error = run(json!({"form": "1", "env": multibyte_over}))
            .expect_err("valid cap+1 must also be refused");
        assert!(
            error.contains("1048577") && error.contains("1048576"),
            "{error}"
        );
    }
}

/// expect: "Explicit maxima survive tool serialization; over-cap snake/camel budgets refuse" [H4A]
#[test]
fn h4a_tool_budget_admission() {
    for (steps_key, depth_key) in [("max_steps", "max_depth"), ("maxSteps", "maxDepth")] {
        let args = json!({"form": "1", steps_key: 2_000_000, depth_key: 8192});
        let input: LispEvalToolInput =
            serde_json::from_value(args).expect("explicit maxima decode");
        let serialized = serde_json::to_value(&input).expect("maxima serialize");
        assert_eq!(serialized["max_steps"], json!(2_000_000));
        assert_eq!(serialized["max_depth"], json!(8192));
        assert_eq!(evaluate_lisp(input), Ok(json!(1)));
        for (key, requested, allowed, field) in [
            (steps_key, 2_000_001, 2_000_000, "max_steps"),
            (steps_key, u64::MAX, 2_000_000, "max_steps"),
            (depth_key, 8193, 8192, "max_depth"),
            (depth_key, u64::MAX, 8192, "max_depth"),
        ] {
            let error = run(json!({"form": "(", key: requested}))
                .expect_err("request ceiling must precede source parse");
            assert!(
                error.contains(field)
                    && error.contains(&requested.to_string())
                    && error.contains(&allowed.to_string()),
                "{error}"
            );
        }
    }
    let defaults: LispEvalToolInput =
        serde_json::from_value(json!({"form": "1"})).expect("defaults decode");
    let defaults = serde_json::to_value(defaults).expect("defaults serialize");
    assert_eq!(defaults["max_steps"], json!(100_000));
    assert_eq!(defaults["max_depth"], json!(1024));
    for key in ["max_steps", "maxSteps", "max_depth", "maxDepth"] {
        let error = run(json!({"form": "(+ 1 2)", key: 0})).expect_err("zero is not defaulted");
        assert!(error.contains("(0)"), "{error}");
    }
}

// ── env: tolerated variants ──────────────────────────────────────────────────

#[test]
fn env_bare_object_is_refused() {
    let out = run(json!({"form": "(+ a 1)", "env": {"a": 1}}));
    assert!(matches!(out, Err(e) if e.contains("env:") && e.contains("JSON text")));
}

#[test]
fn env_json_text_is_decoded() {
    assert_eq!(
        run(json!({"form": "(+ a 1)", "env": "{\"a\": 1}"})),
        Ok(json!(2))
    );
}

#[test]
fn env_raw_integer_beyond_u64_is_refused_before_rounding() {
    let out = run(json!({"form": "n", "env": "{\"n\":184467440737095516160}"}));
    let error = out.expect_err("out-of-i64 integer spelling must not become a float");
    assert!(error.contains("env:"), "{error}");
    assert!(error.contains("184467440737095516160"), "{error}");
    assert!(error.contains("i64"), "{error}");
}

#[test]
fn env_raw_unsafe_integers_are_refused_even_when_projection_discards_them() {
    for integer in [
        "9223372036854775808",
        "18446744073709551615",
        "184467440737095516160",
        "-9223372036854775809",
        "-184467440737095516160",
    ] {
        for env in [
            format!(r#"{{"n":{integer}}}"#),
            format!(r#"{{"data":{{"nested":[0,{integer}]}}}}"#),
            format!(r#"{{"n":{integer},"n":0}}"#),
            format!(r#"{{"n":0,"n":{integer}}}"#),
            format!(r#"{{"data":{{"n":{integer},"n":0}},"data":null}}"#),
        ] {
            // The form ignores env: admission must inspect even unused data.
            let error = run(json!({"form": "42", "env": env}))
                .expect_err("every original integer value must be admitted before projection");
            assert!(error.contains("env:"), "{env}: {error}");
            assert!(error.contains(integer), "{env}: {error}");
            assert!(error.contains("i64"), "{env}: {error}");
        }
    }
}

#[test]
fn env_raw_i64_endpoints_and_neighbors_above_two_to_53_stay_exact() {
    for integer in [i64::MIN, i64::MAX, 9007199254740992, 9007199254740993] {
        assert_eq!(
            run(json!({"form": "n", "env": format!(r#"{{"n":{integer}}}"#)})),
            Ok(json!(integer))
        );
    }
    assert_eq!(
        run(json!({
            "form": "(= a b)",
            "env": r#"{"a":9007199254740992,"b":9007199254740993}"#
        })),
        Ok(json!(false))
    );
}

#[test]
fn env_raw_explicit_float_spellings_stay_floats() {
    for spelling in ["1.0", "1e0", "1E+0", "-2.5e-1", "184467440737095516160.0"] {
        let result = run(json!({"form": "n", "env": format!(r#"{{"n":{spelling}}}"#)}))
            .expect("finite explicit floats are accepted");
        let expected: Value = serde_json::from_str(spelling).expect("finite float oracle");
        assert_eq!(result, expected, "{spelling}");
        assert!(result.is_f64(), "{spelling}: {result}");
    }
}

#[test]
fn env_raw_quoted_numeric_and_nested_json_text_stay_strings() {
    for text in [
        "184467440737095516160",
        "-9223372036854775809",
        "1/3",
        "1.0",
        "1e0",
    ] {
        let data = json!({"nested": [text, {"text": text}], "numeric": text});
        assert_eq!(
            run(json!({"form": "data", "env": json!({"data": data}).to_string()})),
            Ok(data)
        );
    }
    assert_eq!(
        run(json!({
            "form": "n",
            "env": r#"{"n":"{\"n\":184467440737095516160}","discarded":1,"discarded":2}"#
        })),
        Ok(json!(r#"{"n":184467440737095516160}"#))
    );
    assert_eq!(
        run(json!({"form": "n", "env": r#"{"n":1,"n":2}"#})),
        Ok(json!(2))
    );
}

#[test]
fn env_raw_validation_keeps_complete_json_teaching_errors() {
    for env in [
        r#"{"n":01}"#,
        r#"{"n":1e}"#,
        r#"{"n":[1,]}"#,
        r#"{"n":1} trailing"#,
        r#"{"n":1e999}"#,
    ] {
        let error = run(json!({"form": "42", "env": env}))
            .expect_err("malformed env JSON must be rejected");
        assert!(error.contains("env:"), "{env}: {error}");
        assert!(
            error.contains("expected complete JSON text"),
            "{env}: {error}"
        );
        assert!(error.contains("line 1 column"), "{env}: {error}");
    }
}

#[test]
fn env_raw_validation_retains_serde_default_nesting_refusal() {
    let env = format!(r#"{{"n":{}0{}}}"#, "[".repeat(256), "]".repeat(256));
    let error = run(json!({"form": "42", "env": env}))
        .expect_err("RawValue validation must not bypass serde's nesting limit");
    assert!(error.contains("env:"), "{error}");
    assert!(error.contains("recursion limit exceeded"), "{error}");
}

#[test]
fn env_json_text_with_surrounding_whitespace_is_decoded() {
    assert_eq!(
        run(json!({"form": "(+ a 1)", "env": "  {\"a\": 1}  "})),
        Ok(json!(2))
    );
}

#[test]
fn env_missing_defaults_to_empty() {
    // #[serde(default)]: a form with no bindings runs against an empty env.
    assert_eq!(run(json!({"form": "(+ 1 2)"})), Ok(json!(3)));
}

// ── env: refused variants (typed errors — the message is the specimen) ──────

#[test]
fn env_null_is_refused() {
    // before L1: "deserialize: invalid type: null, expected a map" — no field
    // name. After L1: the teaching message (field + received + accepted).
    let out = run(json!({"form": "(+ 1 2)", "env": null}));
    assert_eq!(
        out,
        Err("deserialize: env: expected JSON text encoding an object: invalid type: null, expected a string".to_string())
    );
}

#[test]
fn env_array_is_refused() {
    // before L1: "deserialize: invalid type: sequence, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": [["a", 1]]}));
    assert_eq!(
        out,
        Err("deserialize: env: expected JSON text encoding an object: invalid type: sequence, expected a string".to_string())
    );
}

#[test]
fn env_number_is_refused() {
    // before L1: "deserialize: invalid type: integer `5`, expected a map"
    let out = run(json!({"form": "(+ 1 2)", "env": 5}));
    assert_eq!(
        out,
        Err("deserialize: env: expected JSON text encoding an object: invalid type: integer `5`, expected a string".to_string())
    );
}

#[test]
fn env_double_stringified_is_refused() {
    // The string parses to a JSON string, not a map. before L1 the message
    // carried position noise ("at line 1 column 12"); after L1 the teaching
    // message shows the received string — no position noise.
    let out = run(json!({"form": "(+ a 1)", "env": "\"{\\\"a\\\": 1}\""}));
    assert!(
        matches!(out, Err(e) if e.contains("env:") && e.contains("JSON text encoding an object") && e.contains("expected a map"))
    );
}

#[test]
fn env_invalid_json_string_is_refused() {
    // before L1: "deserialize: key must be a string at line 1 column 2"
    // (position noise, no field name). After L1: the teaching message shows
    // the model its own bytes.
    let out = run(json!({"form": "(+ a 1)", "env": "{a: 1}"}));
    assert!(
        matches!(out, Err(e) if e.contains("env:") && e.contains("expected complete JSON text"))
    );
}

#[test]
fn env_python_repr_string_is_refused() {
    // Python-style single quotes are not JSON — a common temperature-0
    // emission class for models trained on repr() output.
    // before L1: "deserialize: key must be a string at line 1 column 1"
    let out = run(json!({"form": "(+ a 1)", "env": "{'a': 1}"}));
    assert!(
        matches!(out, Err(e) if e.contains("env:") && e.contains("expected complete JSON text"))
    );
}

#[test]
fn env_stringified_array_is_refused() {
    // Parses to a JSON array, not a map.
    // before L1: "deserialize: invalid type: sequence, expected a map at line 1 column 0"
    let out = run(json!({"form": "(+ 1 2)", "env": "[1, 2]"}));
    assert!(matches!(out, Err(e) if e.contains("env:") && e.contains("expected a map")));
}

// ── env: the formerly-unteachable class (L1 closed it) ─────────────────────

#[test]
fn env_nested_under_env_key_error_teaches_the_flatten_fix() {
    // The model wraps env one level too deep: the tool arguments carry
    // {"env": {"env": {...}}}. Deserialization SUCCEEDS — the inner object
    // becomes a single binding named "env" — and pre-L1 the form failed with
    // "unbound symbol: a", an error that did not name the nesting: the anatomy
    // of the 2026-09-28 audit lockout (5 identical retries). L1: the
    // unbound-symbol error names the available bindings and the flatten fix,
    // so the first error changes the next emission.
    let out = run(json!({"form": "(+ a 1)", "env": json!({"env": {"a": 1}}).to_string()}));
    assert_eq!(
        out,
        Err("unbound symbol: a — env bindings: [\"env\"] (an env binding named \"env\" was received — if env was nested one level too deep, flatten it: {\"a\": 1}, not {\"env\": {\"a\": 1}})".to_string())
    );
}

#[test]
fn unbound_symbol_with_empty_env_names_the_empty_binding_list() {
    // The teaching generalizes: any unbound symbol names the available
    // bindings (sorted — byte-identical across processes), so an empty env
    // is visibly empty.
    let out = run(json!({"form": "(+ a 1)", "env": "{}"}));
    assert_eq!(out, Err("unbound symbol: a — env bindings: []".to_string()));
}

// ── env: the wrapped-scalar class (the 2026-10-02 lockout; L3 closes it) ────

#[test]
fn env_wrapped_scalar_type_error_teaches_the_flatten_fix() {
    // The model wraps each scalar in a typed object: {"a": {"n": 1}}
    // instead of {"a": 1}. Deserialization SUCCEEDS — the object becomes
    // an association list — and the form fails with "type error: expected
    // number, got list", an error that named neither the binding nor the
    // fix. Observed live 2026-10-02: three identical retries across two
    // lisp_eval gate forms, including one under explicit deliberation to
    // emit bare scalars (skill_use_issue:hypothesis-framer /
    // skill_use_issue:region-routing) — the wrapping is deterministic for
    // this emitter, so the error must teach on the FIRST failure. L3: the
    // type error names the object-carrying bindings and the flatten fix.
    let out = run(json!({"form": "(+ a 1)", "env": json!({"a": {"n": 1}}).to_string()}));
    assert_eq!(
        out,
        Err("type error: expected number, got list — env bindings carrying objects (objects become association lists): [\"a\"]. If you wrapped a scalar in an object, pass the scalar directly: {\"a\": 1}, not {\"a\": {\"n\": 1}}".to_string())
    );
}

#[test]
fn type_error_from_an_array_binding_keeps_the_plain_message() {
    // An array binding also reaches arithmetic as a list, but the fix is
    // different (do not pass an array to arithmetic) — no object bindings,
    // no wrapped-scalar teaching. The plain message stays.
    let out = run(json!({"form": "(+ a 1)", "env": json!({"a": [1]}).to_string()}));
    assert_eq!(
        out,
        Err("type error: expected number, got list".to_string())
    );
}

#[test]
fn type_error_with_object_bindings_but_non_list_actual_keeps_the_plain_message() {
    // An object binding is present but the failing value is a string, not
    // a list — the objects are not the culprit, so the enrichment stays off
    // (no false teaching: the hint fires only on the wrapped-scalar
    // signature, actual == "list").
    let out = run(json!({"form": "(+ \"x\" 1)", "env": json!({"a": {"n": 1}}).to_string()}));
    assert_eq!(
        out,
        Err("type error: expected number, got string".to_string())
    );
}

// ── form variants ───────────────────────────────────────────────────────────

#[test]
fn form_non_string_is_refused() {
    // before L1: "deserialize: invalid type: integer `5`, expected a string"
    // — no field name (a model that got both form and env wrong could not
    // tell which). After L1: the teaching message.
    let out = run(json!({"form": 5, "env": "{}"}));
    assert_eq!(
        out,
        Err("deserialize: form: received number 5, expected a string containing the Lisp form, e.g. \"(+ 1 2)\"".to_string())
    );
}

#[test]
fn form_empty_evaluates_to_null() {
    // Documented behavior (hkask_lisp.rs:1746-1748): an empty program
    // returns JSON null, not an error.
    assert_eq!(run(json!({"form": "", "env": "{}"})), Ok(Value::Null));
}

#[test]
fn form_whitespace_only_evaluates_to_null() {
    assert_eq!(run(json!({"form": "   ", "env": "{}"})), Ok(Value::Null));
}

#[test]
fn form_unbalanced_parens_is_refused() {
    let out = run(json!({"form": "(+ 1 2", "env": "{}"}));
    assert!(matches!(out, Err(e) if e.contains("parse error")));
}

// ── budget fields ───────────────────────────────────────────────────────

/// A depth-dominant form: nested `(+ 1 ...)` 4000 deep. Evaluation descends
/// one frame per level, so the default max_depth (1024) trips before the
/// step budget (~3 steps/level ≈ 12k). Used to make the budget fields'
/// application OBSERVABLE.
fn nested_add_form(levels: usize) -> String {
    format!("{}1{}", "(+ 1 ".repeat(levels), ")".repeat(levels))
}

#[test]
fn budget_camelcase_max_depth_alias_is_tolerated() {
    // Ratified (L1 / 2026-10-14): camelCase budget fields are tolerated
    // via serde aliases, so an explicit budget is APPLIED rather than silently
    // dropped. Pre-L1 this was a silent-ignore: the camelCase field was an
    // unknown field, serde dropped it, and the eval failed with the DEFAULT
    // limit while the model believed it had raised it — an unteachable
    // failure. The alias makes the emission work as intended; the snake_case
    // control below pins the canonical path.
    let out = run(json!({
        "form": nested_add_form(4000),
        "env": "{}",
        "maxDepth": 8192
    }));
    assert_eq!(out, Ok(json!(4001)));
}

#[test]
fn unknown_junk_fields_are_tolerated() {
    // The complement of the alias decision: unknown fields stay IGNORED (no
    // deny_unknown_fields), so a model adding junk (e.g. a "reasoning" field)
    // is not hard-refused — the camelCase aliases cover exactly the fields
    // with semantic effect.
    let out = run(json!({"form": "(+ 1 2)", "env": "{}", "reasoning": "just checking"}));
    assert_eq!(out, Ok(json!(3)));
}

#[test]
fn budget_snake_case_max_depth_is_applied() {
    // The control for the camelCase test: the snake_case field name works —
    // the same 4000-deep form evaluates to 4001 under max_depth 8192.
    let out = run(json!({
        "form": nested_add_form(4000),
        "env": "{}",
        "max_depth": 8192
    }));
    assert_eq!(out, Ok(json!(4001)));
}

#[test]
fn budget_string_value_is_refused() {
    // before L1: "deserialize: invalid type: string \"100000\", expected u64"
    // — no field name. After L1: the teaching message.
    let out = run(json!({"form": "(+ 1 2)", "env": "{}", "max_steps": "100000"}));
    assert_eq!(
        out,
        Err("deserialize: max_steps: received string \"100000\", expected a non-negative integer (default 100000)".to_string())
    );
}

#[test]
fn walker_600_element_list_completes_at_default_budget() {
    // FIXED (lisp-repair L3): a 600-element list walker completes at the
    // DEFAULT 100,000-step budget. Before: StepLimitExceeded(100000) —
    // charge_native_arguments re-charged every list node on each argument
    // pass, so the O(1) is_null/cdr pair cost O(len) per level: 368,531
    // steps for 600 elements (quadratic; measured by the engine's
    // walker_step_cost_is_linear_and_fits_default_budget binary-search
    // instrument). The per-node charge now lives only in the traversing
    // builtins (length/nth/reverse/append/assoc/member/eq), so the walker
    // is linear and fits the default with an order of magnitude of headroom.
    let form =
        "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)";
    let items: Vec<i32> = (0..600).collect();
    let out =
        run(json!({"form": form, "env": json!({"items": items}).to_string(), "max_depth": 8192}));
    assert_eq!(out, Ok(json!(600)));
}

// ── lie-class fixes landed (L2) — these assertions are the FIXED behavior ─
// Found by the L0 contract critic (session 1deaa9ab-f425-4a94-bb76-60c1c23928f6),
// pinned as wrong-behavior specimens at L0, flipped to the correct behavior
// in the same change that landed the fix (lisp-repair L2: checked arithmetic
// + integer-exact comparison). The pre-fix expectations live in the comments
// as the before/after receipt. Naming: `lie_fix_*` = the corrected behavior
// (was lie_specimen_* / degradation_specimen_* before the 2026-10-14 R-1..R-5
// operator rulings).

#[test]
fn lie_fix_integer_overflow_errors_typed() {
    // FIXED (L2): i64 overflow in + is a typed Runtime error, not a silent
    // wrap. Before: (+ 9223372036854775807 1) → -9223372036854775808
    // (wrapping_add hkask_lisp.rs; also wrapping_sub/wrapping_mul/wrapping_abs,
    // the unary-negation f64 round-trip, and checked_neg/checked_abs for MIN).
    let out = run(json!({"form": "(+ 9223372036854775807 1)", "env": "{}"}));
    assert_eq!(
        out,
        Err(
            "runtime error: integer overflow in +: 9223372036854775807 + 1 exceeds i64".to_string()
        )
    );
    // The whole family: variadic -, *, abs on i64::MIN, unary - on i64::MIN.
    // (A bare 9223372036854775808 literal would parse as a Float and miss the
    // int path — the forms below stay in Int space.)
    assert!(matches!(
        run(json!({"form": "(- 0 9223372036854775807 9223372036854775807)", "env": "{}"})),
        Err(e) if e.contains("integer overflow in -")
    ));
    assert!(matches!(
        run(json!({"form": "(* 9223372036854775807 2)", "env": "{}"})),
        Err(e) if e.contains("integer overflow in *")
    ));
    assert!(matches!(
        run(json!({"form": "(abs -9223372036854775808)", "env": "{}"})),
        Err(e) if e.contains("integer overflow in abs")
    ));
    assert!(matches!(
        run(json!({"form": "(- -9223372036854775808)", "env": "{}"})),
        Err(e) if e.contains("integer overflow in -")
    ));
}

#[test]
fn lie_fix_f64_comparison_is_integer_exact() {
    // FIXED (L2): all-Int comparisons compare as exact i64s. Before (f64
    // coercion, hkask_lisp.rs as_f64): (= 2^53+1 2^53) → true,
    // (< 2^53 2^53+1) → false — every integer above 2^53 miscompared
    // silently. Also covers num_eq's former `.ok()` swallow:
    // (= 1 "x") was silent false; now a type error (= is numeric-only).
    let eq = run(json!({"form": "(= 9007199254740993 9007199254740992)", "env": "{}"}));
    assert_eq!(eq, Ok(json!(false)));
    let lt = run(json!({"form": "(< 9007199254740992 9007199254740993)", "env": "{}"}));
    assert_eq!(lt, Ok(json!(true)));
    // max/min select the correct argument above 2^53 (extremum previously
    // compared through f64 and could return the wrong arg).
    let max = run(json!({"form": "(max 9007199254740993 9007199254740992)", "env": "{}"}));
    assert_eq!(max, Ok(json!(9007199254740993i64)));
    let min = run(json!({"form": "(min 9007199254740992 9007199254740993)", "env": "{}"}));
    assert_eq!(min, Ok(json!(9007199254740992i64)));
    // Type errors propagate: (= 1 "x") is a type error, never silent false.
    let typed = run(json!({"form": "(= 1 \"x\")", "env": "{}"}));
    assert!(matches!(typed, Err(e) if e.contains("type error") && e.contains("expected number")));
}

#[test]
fn lie_fix_string_equals_errors_on_non_string_and_allows_nil() {
    // Operator ruling 2026-10-14 (R-2): string= errors with TypeError on
    // non-string scalars/lists (matching `=`'s L2 lie-fix), while keeping
    // Nil -> false so nullable JSON fields and absent-key `(assoc k obj)`
    // in counting walkers (`d-without-oracle`, `count-sev`) evaluate to false.
    let int_err = run(json!({"form": "(string= 1 \"1\")", "env": "{}"}));
    assert_eq!(
        int_err,
        Err("type error: expected string, got int".to_string())
    );
    let bool_err = run(json!({"form": "(string= \"true\" true)", "env": "{}"}));
    assert_eq!(
        bool_err,
        Err("type error: expected string, got boolean".to_string())
    );
    assert_eq!(
        run(json!({"form": "(string= nil \"D\")", "env": "{}"})),
        Ok(json!(false))
    );
    assert_eq!(
        run(json!({"form": "(string= \"D\" nil)", "env": "{}"})),
        Ok(json!(false))
    );
    assert_eq!(
        run(json!({"form": "(string= nil nil)", "env": "{}"})),
        Ok(json!(false))
    );
}

#[test]
fn lie_fix_nth_negative_index_errors_typed() {
    // Operator ruling 2026-10-14 (R-1): a negative nth index is a typed
    // Runtime error teaching 0-based indexing, never a wrap to usize::MAX
    // that walks the spine and returns null silently. Out-of-bounds
    // non-negative indices continue to return null.
    let neg = run(json!({"form": "(nth -1 (list 1 2 3))", "env": "{}"}));
    assert!(
        matches!(neg, Err(ref e) if e.contains("nth: negative index -1") && e.contains("0-based")),
        "got: {neg:?}"
    );
    assert_eq!(
        run(json!({"form": "(nth 5 (list 1 2 3))", "env": "{}"})),
        Ok(Value::Null)
    );
}

#[test]
fn assoc_strict_and_absent_key_teaching_close_vacuity_and_cdr_traps() {
    // Operator ruling 2026-10-14 (R-3 & R-4):
    // 1. `(assoc! key alist)` returns the value when present and errors naming
    //    available keys in sorted order when absent.
    assert_eq!(
        run(json!({
            "form": "(assoc! \"verdict\" row)",
            "env": json!({"row": {"verdict": "BUG", "contract": "P8.4"}}).to_string()
        })),
        Ok(json!("BUG"))
    );
    let missing = run(json!({
        "form": "(assoc! \"tier\" row)",
        "env": json!({"row": {"verdict": "BUG", "contract": "P8.4", "location": "a.rs:1"}}).to_string()
    }));
    assert_eq!(
        missing,
        Err("runtime error: assoc!: key \"tier\" not found — available keys: [\"contract\", \"location\", \"verdict\"]".to_string())
    );
    // 2. `(assoc key 42)` rejects non-string primitive scalars as the alist.
    let bad_alist = run(json!({"form": "(assoc \"k\" 42)", "env": "{}"}));
    assert!(
        matches!(bad_alist, Err(ref e) if e.contains("type error: expected list, got int")),
        "got: {bad_alist:?}"
    );
    // 3. Numeric comparator on nil teaches the missing-key / assoc! recovery.
    let num_nil = run(json!({
        "form": "(> (assoc \"cnt\" row) 0)",
        "env": json!({"row": {"count": 5}}).to_string()
    }));
    assert!(
        matches!(num_nil, Err(ref e) if e.contains("expected number, got nil") && e.contains("assoc!")),
        "got: {num_nil:?}"
    );
    // 4. Common-Lisp `(cdr (assoc ...))` teaches that assoc returns the value directly.
    let cdr_assoc = run(json!({
        "form": "(cdr (assoc \"count\" row))",
        "env": json!({"row": {"count": 5}}).to_string()
    }));
    assert!(
        matches!(cdr_assoc, Err(ref e) if e.contains("expected list, got int") && e.contains("returns the value directly")),
        "got: {cdr_assoc:?}"
    );
}

// ── L2: boundary determinism pins ──────────────────────────────────────

#[test]
fn l2_repeated_evaluation_is_byte_identical_at_the_tool_boundary() {
    // The evaluate_lisp boundary sorts env keys (the env HashMap's iteration
    // order is process-randomized), so repeated evaluation of the same
    // input — success or typed error — is byte-identical across calls.
    let input = json!({
        "form": "(begin (define total (+ a b c)) (if (> total 5) (assoc \"k\" obj) absent))",
        "env": json!({"c": 3, "a": 1, "b": 2, "obj": {"k": "hit", "j": 9}}).to_string()
    });
    assert_eq!(run(input.clone()), Ok(json!("hit")));
    let mut seen: Option<String> = None;
    for _ in 0..20 {
        let out = run(input.clone());
        let serialized = match out {
            Ok(value) => serde_json::to_string(&value).expect("serialize"),
            Err(err) => format!("error: {err}"),
        };
        match &seen {
            None => seen = Some(serialized),
            Some(previous) => assert_eq!(&serialized, previous, "repeated evaluation drifted"),
        }
    }
    assert!(seen.is_some());
}

#[test]
fn l2_unbound_symbol_error_lists_bindings_sorted_and_stable() {
    // Error-path determinism: the G3 message names the env bindings in
    // sorted order and repeats byte-identically — the multi-key companion
    // to `unbound_symbol_with_empty_env_names_the_empty_binding_list`.
    let input =
        json!({"form": "(+ a missing)", "env": json!({"b": 2, "a": 1, "c": 3}).to_string()});
    let mut seen: Option<String> = None;
    for _ in 0..20 {
        let out = run(input.clone());
        let serialized = match out {
            Err(err) => format!("error: {err}"),
            Ok(value) => serde_json::to_string(&value).expect("serialize"),
        };
        assert!(
            serialized.contains("env bindings: [\"a\", \"b\", \"c\"]"),
            "expected sorted bindings, got: {serialized}"
        );
        match &seen {
            None => seen = Some(serialized),
            Some(previous) => assert_eq!(&serialized, previous, "error message drifted"),
        }
    }
    assert!(seen.is_some());
}
