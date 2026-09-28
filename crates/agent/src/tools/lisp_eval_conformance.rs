//! Cross-surface conformance and the audit exit-form receipt — lisp-repair L4.
//!
//! Two concerns live here:
//!
//! 1. CONFORMANCE: the canonical form set (every builtin family, every
//!    special form, the L2 lie-fix behaviors, the L3 walker, bare infix,
//!    and the error classes) must produce byte-identical serialized
//!    results on the lib surface (`hkask_lisp::eval_sandboxed`) and the
//!    tool surface (`evaluate_lisp` — deserialize + env canonicalization +
//!    engine + serialize). `kask_bridge`'s host-port test pins the third
//!    surface (host/lisp_eval dispatch) against the tool path with a
//!    representative subset — together the three surfaces are identical.
//!    Error strings differ BY DESIGN between lib (bare `LispError`
//!    Display) and tool (L1 teaching messages), so errors are matched by
//!    class, not bytes.
//!
//! 2. THE AUDIT EXIT-FORM RECEIPT: the skill audit's per-skill exit form
//!    (`(and (= unlabelled 0) (= d-without-oracle 0) (= p-without-critic 0))`)
//!    is computed IN-FORM over the raw steps JSON — the retirement of the
//!    python-routing workaround (lisp-repair L4). The fixture test pins the
//!    counting; the `#[ignore]` test runs the real audit steps files
//!    through the tool path and is the recorded receipt.

use hkask_lisp::{LispError, eval_sandboxed};
use serde_json::{Value, json};

use super::lisp_eval_tool::{LispEvalToolInput, evaluate_lisp};

/// The real tool path: deserialize the tool arguments (the teaching
/// deserialization), evaluate (env canonicalization + engine), serialize.
fn tool_path(args: &Value) -> Result<String, String> {
    let input: LispEvalToolInput =
        serde_json::from_value(args.clone()).map_err(|e| format!("deserialize: {e}"))?;
    evaluate_lisp(input).map(|v| serde_json::to_string(&v).expect("serialize"))
}

/// The canonical success forms: (name, form, env, expected). Every builtin
/// family and special form is represented, plus the L2 lie-fix behaviors,
/// the L3 walker, and bare infix.
fn success_forms() -> Vec<(&'static str, &'static str, Value, Value)> {
    vec![
        ("arithmetic", "(+ 1 2 3)", json!({}), json!(6)),
        (
            "exact equality above 2^53",
            "(= 9007199254740993 9007199254740992)",
            json!({}),
            json!(false),
        ),
        (
            "ordered comparison above 2^53",
            "(< 9007199254740992 9007199254740993)",
            json!({}),
            json!(true),
        ),
        (
            "extremum above 2^53",
            "(max 9007199254740992 9007199254740993)",
            json!({}),
            json!(9007199254740993i64),
        ),
        ("division is always Float", "(/ 6 3)", json!({}), json!(2.0)),
        (
            "string semantics",
            "(and (string= \"a\" \"a\") (= (length \"héllo\") 5) (string-contains \"é\" \"héllo\"))",
            json!({}),
            json!(true),
        ),
        (
            "assoc over object",
            "(assoc \"b\" data)",
            json!({"data": {"a": 1, "b": "x"}}),
            json!("x"),
        ),
        (
            "nested assoc chain",
            "(assoc \"k\" (assoc \"l2\" (assoc \"l1\" data)))",
            json!({"data": {"l1": {"l2": {"k": "hit"}}}}),
            json!("hit"),
        ),
        (
            "list ops",
            "(list (nth 1 (list 5 6)) (car (append (reverse (list 1 2)) (list 3))))",
            json!({}),
            json!([6, 2]),
        ),
        (
            "predicates",
            "(and (stringp \"x\") (numberp 1) (listp (list)) (not (is_null (list 1))))",
            json!({}),
            json!(true),
        ),
        (
            "special forms",
            "(begin (define sq (lambda (x) (* x x))) (let ((y 3)) (if (and (> y 2) (or false true)) (cond (nil 1) (t (sq y))) 0)))",
            json!({}),
            json!(9),
        ),
        (
            "walker",
            "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)",
            json!({"items": [1, 2, 3, 4, 5]}),
            json!(5),
        ),
        ("bare infix", "z + 1", json!({"z": 41}), json!(42)),
    ]
}

#[test]
fn success_results_are_byte_identical_across_lib_and_tool_surfaces() {
    for (name, form, env, expected) in success_forms() {
        let lib = eval_sandboxed(form, &env)
            .unwrap_or_else(|e| panic!("{name}: lib surface errored: {e}"));
        let lib = serde_json::to_string(&lib).expect("serialize");
        let tool = tool_path(&json!({"form": form, "env": env}))
            .unwrap_or_else(|e| panic!("{name}: tool surface errored: {e}"));
        assert_eq!(&tool, &lib, "{name}: lib and tool surfaces disagree");
        let expected = serde_json::to_string(&expected).expect("serialize");
        assert_eq!(tool, expected, "{name}: wrong result");
    }
}

/// The error forms: (name, form, env, tool-error prefix, lib variant class).
/// Tool and host surfaces are byte-identical (same code path); the lib
/// surface carries the bare LispError, so the class is matched by variant.
fn error_forms() -> Vec<(
    &'static str,
    &'static str,
    Value,
    &'static str,
    &'static str,
)> {
    vec![
        (
            "integer overflow",
            "(+ 9223372036854775807 1)",
            json!({}),
            "runtime error: integer overflow in +",
            "runtime",
        ),
        (
            "unbound symbol",
            "nosuchsymbol",
            json!({}),
            "unbound symbol: nosuchsymbol",
            "unbound",
        ),
        (
            "numeric-only equality",
            "(= 1 \"x\")",
            json!({}),
            "type error: expected number, got string",
            "type",
        ),
        (
            "division by zero",
            "(/ 1 0)",
            json!({}),
            "runtime error: division by zero",
            "runtime",
        ),
    ]
}

#[test]
fn error_classes_correspond_across_surfaces() {
    for (name, form, env, tool_prefix, lib_class) in error_forms() {
        let tool = tool_path(&json!({"form": form, "env": env}))
            .expect_err(&format!("{name}: tool surface should error"));
        assert!(
            tool.starts_with(tool_prefix),
            "{name}: tool error does not match prefix {tool_prefix}: {tool}"
        );
        let lib =
            eval_sandboxed(form, &env).expect_err(&format!("{name}: lib surface should error"));
        let matched = match (lib_class, &lib) {
            ("runtime", LispError::Runtime(_)) => true,
            ("unbound", LispError::UnboundSymbol(_)) => true,
            ("type", LispError::TypeError { .. }) => true,
            _ => false,
        };
        assert!(
            matched,
            "{name}: lib variant does not match class {lib_class}: {lib}"
        );
    }
}

// ── The audit exit-form class (python-routing retirement) ──────────────────

/// The shared defines for the audit exit-form class: a counting walker over
/// the steps array, computing the three audit counts IN-FORM from the raw
/// JSON (the python-routing workaround computed these externally).
const AUDIT_DEFINES: &str = concat!(
    "(define count-if (lambda (pred lst) ",
    "(if (< (length lst) 1) 0 ",
    "(+ (if (pred (car lst)) 1 0) (count-if pred (cdr lst)))))) ",
    "(define unlabelled-step (lambda (s) (is_null (assoc \"dp\" s)))) ",
    "(define d-without-oracle ",
    "(lambda (s) (and (string= (assoc \"dp\" s) \"D\") ",
    "(is_null (assoc \"oracle_or_critic\" s))))) ",
    "(define p-without-critic ",
    "(lambda (s) (and (string= (assoc \"dp\" s) \"P\") ",
    "(is_null (assoc \"oracle_or_critic\" s)))))"
);

/// Returns the three counts as a list: (unlabelled, d-without-oracle,
/// p-without-critic).
const AUDIT_COUNTS_TAIL: &str = concat!(
    "(list (count-if unlabelled-step steps) ",
    "(count-if d-without-oracle steps) ",
    "(count-if p-without-critic steps))"
);

/// The per-skill exit form: green iff all three counts are zero.
const AUDIT_EXIT_TAIL: &str = concat!(
    "(and (= (count-if unlabelled-step steps) 0) ",
    "(= (count-if d-without-oracle steps) 0) ",
    "(= (count-if p-without-critic steps) 0))"
);

fn audit_form(tail: &str) -> String {
    format!("{AUDIT_DEFINES} {tail}")
}

#[test]
fn audit_exit_form_counts_a_schema_shaped_fixture() {
    // The counting pin: a steps array with one defect of each class counts
    // (1, 1, 1) and fails the exit form; a clean array counts (0, 0, 0)
    // and passes. Schema: {step, dp, oracle_or_critic, reference} per step
    // (the audit's steps/<skill>.json shape).
    let bad = json!({"steps": [
        {"step": "good D", "dp": "D", "oracle_or_critic": "cargo test"},
        {"step": "unlabelled", "oracle_or_critic": "x"},
        {"step": "D without oracle", "dp": "D"},
        {"step": "good P", "dp": "P", "oracle_or_critic": "critic session"},
        {"step": "P without critic", "dp": "P"},
    ]});
    let counts = tool_path(&json!({"form": audit_form(AUDIT_COUNTS_TAIL), "env": bad}))
        .expect("counts form evaluates");
    assert_eq!(counts, "[1,1,1]", "fixture counts must be (1, 1, 1)");
    let exit = tool_path(&json!({"form": audit_form(AUDIT_EXIT_TAIL), "env": bad}))
        .expect("exit form evaluates");
    assert_eq!(exit, "false", "defective fixture must fail the exit form");

    let good = json!({"steps": [
        {"step": "s1", "dp": "D", "oracle_or_critic": "oracle"},
        {"step": "s2", "dp": "P", "oracle_or_critic": "critic"},
    ]});
    let counts = tool_path(&json!({"form": audit_form(AUDIT_COUNTS_TAIL), "env": good}))
        .expect("counts form evaluates");
    assert_eq!(counts, "[0,0,0]", "clean fixture counts must be (0, 0, 0)");
    let exit = tool_path(&json!({"form": audit_form(AUDIT_EXIT_TAIL), "env": good}))
        .expect("exit form evaluates");
    assert_eq!(exit, "true", "clean fixture must pass the exit form");
}

#[test]
#[ignore = "reads the operator's audit steps dir; set LISP_AUDIT_STEPS_DIR to override"]
fn audit_exit_form_runs_green_over_the_real_steps_files() {
    // THE RECEIPT (lisp-repair L4): the audit's per-skill exit-form class,
    // computed in-form through the repaired tool path over the real
    // steps/<skill>.json files — the python-routing workaround retired.
    // Run with: cargo test -p agent --lib lisp_eval_conformance -- --ignored --nocapture
    let dir = std::env::var("LISP_AUDIT_STEPS_DIR").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/Documents/zk-data/skills/skill-audit/2026-09-28/steps")
    });
    let dir = std::path::Path::new(&dir);
    if !dir.is_dir() {
        eprintln!("SKIPPED: audit steps dir not found at {}", dir.display());
        return;
    }
    let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .and_then(|read| read.collect::<Result<Vec<_>, _>>())
        .unwrap_or_else(|e| panic!("steps dir {}: {e}", dir.display()))
        .into_iter()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    assert!(
        !entries.is_empty(),
        "no steps files found in {}",
        dir.display()
    );
    for path in &entries {
        let raw =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let parsed: Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let steps = parsed
            .get("steps")
            .cloned()
            .unwrap_or_else(|| panic!("{}: no steps array", path.display()));
        let env = json!({ "steps": steps });
        let counts = tool_path(&json!({"form": audit_form(AUDIT_COUNTS_TAIL), "env": env}))
            .unwrap_or_else(|e| panic!("{}: counts form failed: {e}", path.display()));
        assert_eq!(
            counts,
            "[0,0,0]",
            "{}: expected (0, 0, 0), got {counts}",
            path.display()
        );
        let exit = tool_path(&json!({"form": audit_form(AUDIT_EXIT_TAIL), "env": env}))
            .unwrap_or_else(|e| panic!("{}: exit form failed: {e}", path.display()));
        assert_eq!(exit, "true", "{}: exit form not green", path.display());
        eprintln!("{}: counts [0,0,0], exit true", path.display());
    }
    eprintln!(
        "audit exit-form receipt: {} steps files, all green through the tool path",
        entries.len()
    );
}
