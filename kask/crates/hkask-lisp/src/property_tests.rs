//! Property layer for the sandboxed evaluator
//! (`kask/docs/reference/testing-protocol.md`). The budget-contract pins
//! (step/depth/output limits) live in `hkask_lisp.rs::tests`; this layer
//! generalizes evaluator correctness over generated programs. Grown in
//! lisp-repair L2 (2026-09-28) from 2 properties to the pass-exit set:
//! - exact-oracle properties over the two lie-class fixes (checked
//!   arithmetic, integer-exact comparison) plus division and string
//!   semantics,
//! - registry + special-form coverage derived from the tree at test time
//!   (a builtin added without a specimen fails here),
//! - byte-identical determinism across repeated evaluation (pillar 2),
//! - a seeded totality fuzz over the numeric-edge atom pool
//!   (i64 bounds, 2^53, multibyte strings) — the crash/lie classes.
//! A shrunk counterexample is a finding to report, never a signal to
//! weaken a property.
//!
//! NOTE: prop_assert!/prop_assert_eq! messages expand through concat!, so
//! inline format captures ("{var}") do not compile there — pass positional
//! args ("{}", var) in macro messages.

use super::*;
use proptest::prelude::*;

/// expect: "Nonfinite literals, evaluated operands, native math and sandbox output fail explicitly" [P8]
#[test]
fn h3_nonfinite_numbers_are_errors_not_null() {
    for token in ["1e309", "-1e309", "NaN", "inf", "-inf"] {
        let error = parse(token).expect_err("nonfinite literal must fail parsing");
        assert!(matches!(error, LispError::Parse(_)), "{token}: {error}");
        assert!(error.to_string().contains(token), "{error}");
    }
    for form in [
        "(+ 1e308 1e308)",
        "(* 1e308 2.0)",
        "(- -1e308 1e308)",
        "(/ 1e308 1e-308)",
        "(list (* 1e308 2.0))",
    ] {
        let error = eval_sandboxed(form, &serde_json::json!({}))
            .expect_err("nonfinite math must not serialize to null");
        assert!(error.to_string().contains("non-finite"), "{form}: {error}");
    }
    let env = Rc::new(RefCell::new(Env::new_root()));
    env.borrow_mut()
        .define("bad".into(), LispValue::Float(f64::NAN));
    assert!(eval(env.clone(), &LispValue::Symbol("bad".into())).is_err());
    assert!(eval(env.clone(), &LispValue::Float(f64::INFINITY)).is_err());
    for (name, builtin) in default_builtins() {
        let args = match name {
            "+" | "-" | "*" | "/" | "=" | "!=" | "<" | "<=" | ">" | ">=" | "min" | "max" => {
                vec![LispValue::Float(f64::INFINITY), LispValue::Int(1)]
            }
            "abs" | "sqrt" => vec![LispValue::Float(f64::NAN)],
            // Nonnumeric operations have their semantic witnesses in the
            // registry specimen test. A new builtin must be classified here.
            "car" | "cdr" | "cons" | "list" | "length" | "nth" | "reverse" | "is_null"
            | "numberp" | "listp" | "stringp" | "assoc" | "assoc!" | "append" | "string="
            | "concat" | "string-contains" | "starts-with" | "ends-with" | "eq" | "member"
            | "boundary-ok" => continue,
            _ => panic!("classify new registry builtin {name} for numeric coverage"),
        };
        let (finite_source, expected) = match name {
            "+" => ("(+ 4 2.0)", serde_json::json!(6.0)),
            "-" => ("(- 4 2.0)", serde_json::json!(2.0)),
            "*" => ("(* 4 2.0)", serde_json::json!(8.0)),
            "/" => ("(/ 4 2.0)", serde_json::json!(2.0)),
            "=" => ("(= 4 2.0)", serde_json::json!(false)),
            "!=" => ("(!= 4 2.0)", serde_json::json!(true)),
            "<" => ("(< 4 2.0)", serde_json::json!(false)),
            "<=" => ("(<= 4 2.0)", serde_json::json!(false)),
            ">" => ("(> 4 2.0)", serde_json::json!(true)),
            ">=" => ("(>= 4 2.0)", serde_json::json!(true)),
            "abs" => ("(abs -4.0)", serde_json::json!(4.0)),
            "sqrt" => ("(sqrt 4.0)", serde_json::json!(2.0)),
            "min" => ("(min 4 2.0)", serde_json::json!(2.0)),
            "max" => ("(max 4 2.0)", serde_json::json!(4)),
            _ => unreachable!("numeric classification above"),
        };
        assert_eq!(
            eval_sandboxed(finite_source, &serde_json::json!({})).expect("finite numeric witness"),
            expected,
            "{name}"
        );
        let error = builtin(&env, &args, &mut EvalBudget::new(100_000, 1024))
            .expect_err("every numeric builtin must reject nonfinite operands");
        let message = error.to_string();
        assert!(
            message.contains(name) && message.contains("non-finite"),
            "{name}: {message}"
        );
    }
    let mut budget = EvalBudget::new(100_000, 1024);
    assert!(
        charge_value(&LispValue::Float(f64::NAN), &mut budget, 0).is_err(),
        "sandbox result validation must reject nonfinite values"
    );
}

/// Independent oracle: f64 -> i128 has room for every rounded i64 value,
/// including +2^63, so this does not confuse saturation with exactness.
fn exactly_representable_as_f64(value: i64) -> bool {
    (value as f64) as i128 == i128::from(value)
}

/// expect: "Only required, exact integer promotions are admitted; integer math stays checked" [P8]
#[test]
fn h3_promotion_is_exact_and_only_when_needed() {
    let env = serde_json::json!({});
    for operation in [
        "+", "-", "*", "/", "=", "!=", "<", "<=", ">", ">=", "min", "max",
    ] {
        for operands in ["9007199254740993 1.0", "1.0 9007199254740993"] {
            let form = format!("({operation} {operands})");
            let error =
                eval_sandboxed(&form, &env).expect_err("lossy promotion is a numeric error");
            let message = error.to_string();
            assert!(
                message.contains(operation)
                    && message.contains("9007199254740993")
                    && message.contains("exact"),
                "{form}: {message}"
            );
        }
    }
    for form in [
        "(sqrt 9007199254740993)",
        "(/ 9223372036854775807)",
        "(= 9007199254740993 9007199254740992.0)",
        "(+ 9223372036854775807 1 0.0)",
    ] {
        let error = eval_sandboxed(form, &env).expect_err("required lossy conversion fails");
        assert!(error.to_string().contains("exact"), "{form}: {error}");
    }
    for (form, expected) in [
        (
            "(+ 9007199254740994 0.0)",
            serde_json::json!(9007199254740994.0),
        ),
        (
            "(+ -9223372036854775808 0.0)",
            serde_json::json!(-9223372036854775808.0),
        ),
        (
            "(+ 4611686018427387904 4611686018427387904 0.0)",
            serde_json::json!(9223372036854775808.0),
        ),
        (
            "(* 4611686018427387904 2 1.0)",
            serde_json::json!(9223372036854775808.0),
        ),
        (
            "(- -4611686018427387904 4611686018427387904 0.0)",
            serde_json::json!(-9223372036854775808.0),
        ),
        ("(/ 6 3)", serde_json::json!(2.0)),
        ("(/ 5 2)", serde_json::json!(2.5)),
        ("(/ 6)", serde_json::json!(6.0)),
        ("(= 1 1.0)", serde_json::json!(true)),
        ("(eq 1 1.0)", serde_json::json!(false)),
        (
            "(max 9007199254740993 9007199254740992)",
            serde_json::json!(9007199254740993_i64),
        ),
        ("(min 9223372036854775807)", serde_json::json!(i64::MAX)),
        (
            "(max 9007199254740993 9007199254740994 0.0)",
            serde_json::json!(9007199254740994_i64),
        ),
        ("(min 9007199254740993 0 0.0)", serde_json::json!(0)),
        ("(max 1 2.0)", serde_json::json!(2.0)),
        ("(min 1 2.0)", serde_json::json!(1)),
    ] {
        assert_eq!(eval_sandboxed(form, &env).expect(form), expected, "{form}");
    }
    for form in [
        "(+ 9223372036854775807 1 -1)",
        "(* 9223372036854775807 2 0)",
        "(* 1e308 2.0 0.0)",
    ] {
        assert!(
            eval_sandboxed(form, &env).is_err(),
            "checked left accumulation: {form}"
        );
    }
}

/// Edge-weighted i64 pool: the corners where the historical lies lived —
/// i64 bounds (the wrap class), 2^53±1 (the f64-blindness class) — plus
/// arbitrary values.
fn numeric_edge() -> BoxedStrategy<i64> {
    prop_oneof![
        2 => Just(0),
        2 => Just(1),
        2 => Just(-1),
        2 => Just(i64::MAX),
        2 => Just(i64::MIN),
        1 => Just(i64::MAX - 1),
        1 => Just(i64::MIN + 1),
        1 => Just(9_007_199_254_740_992), // 2^53 — f64-blind from here up
        1 => Just(9_007_199_254_740_993), // 2^53+1 — the pinned lie specimen
        1 => Just(-9_007_199_254_740_993),
        1 => any::<i64>(),
    ]
    .boxed()
}

/// Escape a string into a double-quoted Lisp literal. The tokenizer runs a
/// string token to its closing quote (raw newlines are legal inside it), so
/// only backslash and double-quote need escaping.
fn lisp_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

proptest! {
    /// Hypothesis: the evaluator computes generated integer additions
    /// exactly — arithmetic over generated operands returns the correct
    /// integer result, not merely a result.
    #[test]
    fn evaluator_sums_generated_additions_exactly(
        a in 0u64..=1_000_000,
        b in 0u64..=1_000_000,
    ) {
        let form = format!("(+ {a} {b})");
        let result =
            eval_sandboxed(&form, &serde_json::json!({})).expect("well-formed addition evaluates");
        prop_assert_eq!(result, serde_json::json!(a + b));
    }

    /// Hypothesis: evaluation is total — arbitrary source text either
    /// evaluates or returns a typed error. The step and depth budgets bound
    /// execution, so no input can hang or crash the host.
    #[test]
    fn evaluation_is_total_over_arbitrary_source(source in ".*") {
        let _ = eval_sandboxed(&source, &serde_json::json!({}));
    }

    /// Hypothesis (L2 lie-fix 1): binary Int arithmetic matches the std
    /// checked-op oracle exactly — every representable result is the right
    /// i64, every overflow is a typed Runtime error. Division requires exact
    /// operand promotion, then produces a Float or a division-by-zero error.
    #[test]
    fn checked_binary_arithmetic_matches_std_oracles(
        a in numeric_edge(),
        b in numeric_edge(),
    ) {
        for (op, expected) in [
            ("+", a.checked_add(b)),
            ("-", a.checked_sub(b)),
            ("*", a.checked_mul(b)),
        ] {
            let form = format!("({op} {a} {b})");
            match (expected, eval_sandboxed(&form, &serde_json::json!({}))) {
                (Some(want), Ok(got)) => prop_assert_eq!(got, serde_json::json!(want), "{}", form),
                (None, Err(LispError::Runtime(msg))) => prop_assert!(
                    msg.contains(&format!("integer overflow in {op}")),
                    "{}: {}",
                    form,
                    msg
                ),
                (None, Err(other)) => {
                    prop_assert!(false, "{} should be a typed overflow, got {}", form, other)
                }
                (Some(_), Err(err)) => prop_assert!(false, "{} should evaluate, got {}", form, err),
                (None, Ok(got)) => prop_assert!(false, "{} should overflow, got {}", form, got),
            }
        }
        let div = format!("(/ {a} {b})");
        if !exactly_representable_as_f64(a) || !exactly_representable_as_f64(b) {
            prop_assert!(
                matches!(eval_sandboxed(&div, &serde_json::json!({})), Err(LispError::Runtime(ref msg)) if msg.contains("exact")),
                "{} must refuse lossy operand promotion", div
            );
        } else if b == 0 {
            prop_assert!(
                matches!(
                    eval_sandboxed(&div, &serde_json::json!({})),
                    Err(LispError::Runtime(ref msg)) if msg.contains("division by zero")
                ),
                "{} should be a typed division-by-zero error",
                div
            );
        } else {
            let got = eval_sandboxed(&div, &serde_json::json!({})).expect("division evaluates");
            prop_assert_eq!(got, serde_json::json!((a as f64) / (b as f64)), "{}", div);
        }
    }

    /// expect: "Mixed numeric promotion agrees with an independent widened integer oracle" [P8]
    #[test]
    fn h3_mixed_numeric_domain_matches_independent_oracle(a in numeric_edge()) {
        let env = serde_json::json!({});
        for (op, want) in [
            ("+", serde_json::json!(a as f64 + 1.0)),
            ("-", serde_json::json!(a as f64 - 1.0)),
            ("*", serde_json::json!(a as f64)),
            ("/", serde_json::json!(a as f64)),
            ("=", serde_json::json!(a == 1)),
            ("!=", serde_json::json!(a != 1)),
            ("<", serde_json::json!(a < 1)),
            ("<=", serde_json::json!(a <= 1)),
            (">", serde_json::json!(a > 1)),
            (">=", serde_json::json!(a >= 1)),
            ("min", if a <= 1 { serde_json::json!(a) } else { serde_json::json!(1.0) }),
            ("max", if a >= 1 { serde_json::json!(a) } else { serde_json::json!(1.0) }),
        ] {
            let form = format!("({op} {a} 1.0)");
            let result = eval_sandboxed(&form, &env);
            if exactly_representable_as_f64(a) {
                prop_assert_eq!(result.expect("exact promotion evaluates"), want, "{}", form);
            } else {
                prop_assert!(matches!(result, Err(LispError::Runtime(ref msg)) if msg.contains("exact")), "{} must refuse lossy promotion", form);
            }
        }
        let result = eval_sandboxed(&format!("(sqrt {a})"), &env);
        if exactly_representable_as_f64(a) && a >= 0 {
            prop_assert_eq!(result.expect("finite nonnegative exact operand"), serde_json::json!((a as f64).sqrt()));
        } else {
            prop_assert!(matches!(result, Err(LispError::Runtime(_))));
        }
    }

    /// Hypothesis (L2 lie-fix 1, unary corner): negation and abs match the
    /// std checked oracles — i64::MIN is a typed overflow, never a wrapped
    /// or f64-round-tripped value.
    #[test]
    fn checked_unary_arithmetic_matches_std_oracles(a in numeric_edge()) {
        for (form, expected) in [
            (format!("(- {a})"), a.checked_neg()),
            (format!("(abs {a})"), a.checked_abs()),
        ] {
            match (expected, eval_sandboxed(&form, &serde_json::json!({}))) {
                (Some(want), Ok(got)) => prop_assert_eq!(got, serde_json::json!(want), "{}", form),
                (None, Err(LispError::Runtime(_))) => {}
                (None, Err(other)) => {
                    prop_assert!(false, "{} should be a typed overflow, got {}", form, other)
                }
                (Some(_), Err(err)) => prop_assert!(false, "{} should evaluate, got {}", form, err),
                (None, Ok(got)) => prop_assert!(false, "{} should overflow, got {}", form, got),
            }
        }
    }

    /// Hypothesis (L2 lie-fix 2): ordered comparison, numeric equality and
    /// extrema over Ints are exact i64 — the former f64 coercion misordered
    /// every integer above 2^53 ((= 2^53+1 2^53) silently returned true).
    #[test]
    fn integer_comparisons_match_rust_oracles(
        a in numeric_edge(),
        b in numeric_edge(),
    ) {
        let env = serde_json::json!({});
        for (form, want) in [
            (format!("(= {a} {b})"), a == b),
            (format!("(!= {a} {b})"), a != b),
            (format!("(< {a} {b})"), a < b),
            (format!("(<= {a} {b})"), a <= b),
            (format!("(> {a} {b})"), a > b),
            (format!("(>= {a} {b})"), a >= b),
        ] {
            let got = eval_sandboxed(&form, &env).expect("comparison evaluates");
            prop_assert_eq!(got, serde_json::json!(want), "{}", form);
        }
        for (form, want) in [
            (format!("(max {a} {b})"), a.max(b)),
            (format!("(min {a} {b})"), a.min(b)),
        ] {
            let got = eval_sandboxed(&form, &env).expect("extremum evaluates");
            prop_assert_eq!(got, serde_json::json!(want), "{}", form);
        }
    }

    /// Hypothesis: string semantics match Rust — `length` counts chars
    /// (multibyte is the pinned corner), `string=` is content equality,
    /// `concat` joins, `string-contains` is needle-first containment
    /// with the empty-needle and probable-reversal guards, and
    /// `starts-with`/`ends-with` are directional checks whose only error
    /// case is the empty needle (no length guard — a longer needle is a
    /// legitimate false).
    #[test]
    fn string_semantics_match_rust_oracles(s in ".*", t in ".*") {
        let env = serde_json::json!({});
        let (ls, lt) = (lisp_string_literal(&s), lisp_string_literal(&t));
        for (form, want) in [
            (format!("(string= {ls} {lt})"), serde_json::json!(s == t)),
            (
                format!("(length {ls})"),
                serde_json::json!(s.chars().count() as i64),
            ),
            (
                format!("(concat {ls} {lt})"),
                serde_json::json!(format!("{s}{t}")),
            ),
        ] {
            let got = eval_sandboxed(&form, &env).expect("string op evaluates");
            prop_assert_eq!(got, want, "{}", form);
        }
        let contains = format!("(string-contains {ls} {lt})");
        match eval_sandboxed(&contains, &env) {
            Ok(got) => prop_assert_eq!(got, serde_json::json!(t.contains(&s)), "{}", contains),
            Err(LispError::Runtime(msg)) if s.is_empty() => {
                prop_assert!(msg.contains("non-empty"), "{}: {}", contains, msg)
            }
            Err(LispError::Runtime(msg)) => {
                prop_assert!(
                    s.len() > t.len() && msg.contains("reversed"),
                    "{}: unexpected error {}",
                    contains,
                    msg
                );
            }
            Err(err) => prop_assert!(false, "{}: unexpected error {}", contains, err),
        }
        for (form, want) in [
            (format!("(starts-with {ls} {lt})"), t.starts_with(&s)),
            (format!("(ends-with {ls} {lt})"), t.ends_with(&s)),
        ] {
            match eval_sandboxed(&form, &env) {
                Ok(got) => prop_assert_eq!(got, serde_json::json!(want), "{}", form),
                Err(LispError::Runtime(msg)) if s.is_empty() => {
                    prop_assert!(msg.contains("non-empty"), "{}: {}", form, msg)
                }
                Err(err) => prop_assert!(false, "{}: unexpected error {}", form, err),
            }
        }
    }
}

/// `!=` compares adjacent pairs over 3+ args in BOTH numeric paths — the
/// f64 fallback once compared every arg against the first, so
/// `(!= 1.0 2.0 1.0)` returned false while `(!= 1 2 1)` returned true.
/// `=`/`!=` are numeric-only: a non-number anywhere is a type error, never
/// a silent false, even when an earlier pair already decided the result.
#[test]
fn numeric_comparators_pin_adjacent_pairs_and_type_errors() {
    let env = serde_json::json!({});
    for (form, want) in [
        ("(!= 1 2 1)", serde_json::json!(true)),
        ("(!= 1.0 2.0 1.0)", serde_json::json!(true)),
        ("(!= 1.0 1.0 2.0)", serde_json::json!(false)),
        ("(= 1 1 1)", serde_json::json!(true)),
        ("(= 1.0 1.0 2.0)", serde_json::json!(false)),
        ("(= 1.0 2.0 1.0)", serde_json::json!(false)),
    ] {
        let got = eval_sandboxed(form, &env).expect("comparison evaluates");
        assert_eq!(got, want, "{form}");
    }
    for form in ["(= 1 2 \"x\")", "(!= 1.0 2.0 \"x\")"] {
        assert!(
            eval_sandboxed(form, &env).is_err(),
            "{form} must be a type error, never a silent false"
        );
    }
}

/// Every builtin in the registry has a specimen that evaluates to its
/// expected value. The registry is walked at test time — a builtin added
/// without a specimen fails here (the tree-derived coverage rule), and a
/// specimen naming a builtin the registry no longer defines fails too.
#[test]
fn every_registry_builtin_has_a_passing_specimen() {
    let registered: std::collections::HashSet<&str> = default_builtins()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let data_env = serde_json::json!({"data": {"a": 1, "b": "x"}});
    let empty = serde_json::json!({});
    // (builtin, form, env, expected)
    let specimens: Vec<(&str, String, Value, Value)> = vec![
        ("+", "(+ 1 2)".into(), empty.clone(), serde_json::json!(3)),
        ("-", "(- 10 3)".into(), empty.clone(), serde_json::json!(7)),
        ("*", "(* 4 5)".into(), empty.clone(), serde_json::json!(20)),
        ("/", "(/ 6 3)".into(), empty.clone(), serde_json::json!(2.0)),
        (
            "=",
            "(= 2 2)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "!=",
            "(!= 2 3)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "<",
            "(< 1 2)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "<=",
            "(<= 2 2)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            ">",
            "(> 2 1)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            ">=",
            "(>= 2 2)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "car",
            "(car (list 1 2))".into(),
            empty.clone(),
            serde_json::json!(1),
        ),
        (
            "cdr",
            "(cdr (list 1 2))".into(),
            empty.clone(),
            serde_json::json!([2]),
        ),
        (
            "cons",
            "(cons 1 (list 2))".into(),
            empty.clone(),
            serde_json::json!([1, 2]),
        ),
        (
            "list",
            "(list 1 2)".into(),
            empty.clone(),
            serde_json::json!([1, 2]),
        ),
        (
            "length",
            "(length (list 1 2 3))".into(),
            empty.clone(),
            serde_json::json!(3),
        ),
        (
            "nth",
            "(nth 1 (list 5 6))".into(),
            empty.clone(),
            serde_json::json!(6),
        ),
        (
            "reverse",
            "(reverse (list 1 2))".into(),
            empty.clone(),
            serde_json::json!([2, 1]),
        ),
        (
            "is_null",
            "(is_null nil)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "numberp",
            "(numberp 3)".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "listp",
            "(listp (list 1))".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "stringp",
            "(stringp \"x\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "assoc",
            "(assoc \"b\" data)".into(),
            data_env.clone(),
            serde_json::json!("x"),
        ),
        (
            "assoc!",
            "(assoc! \"b\" data)".into(),
            data_env,
            serde_json::json!("x"),
        ),
        (
            "append",
            "(append (list 1) (list 2))".into(),
            empty.clone(),
            serde_json::json!([1, 2]),
        ),
        (
            "string=",
            "(string= \"a\" \"a\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "concat",
            "(concat \"a\" \"b\")".into(),
            empty.clone(),
            serde_json::json!("ab"),
        ),
        (
            "string-contains",
            "(string-contains \"ell\" \"hello\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "starts-with",
            "(starts-with \"hel\" \"hello\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "ends-with",
            "(ends-with \"llo\" \"hello\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "abs",
            "(abs -3)".into(),
            empty.clone(),
            serde_json::json!(3),
        ),
        (
            "sqrt",
            "(sqrt 4)".into(),
            empty.clone(),
            serde_json::json!(2.0),
        ),
        (
            "max",
            "(max 1 5)".into(),
            empty.clone(),
            serde_json::json!(5),
        ),
        (
            "min",
            "(min 1 5)".into(),
            empty.clone(),
            serde_json::json!(1),
        ),
        (
            "eq",
            "(eq \"a\" \"a\")".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "member",
            "(member 2 (list 1 2))".into(),
            empty.clone(),
            serde_json::json!(true),
        ),
        (
            "boundary-ok",
            "(boundary-ok \"record:\" \"record:x\")".into(),
            empty,
            serde_json::json!(true),
        ),
    ];
    let covered: std::collections::HashSet<&str> =
        specimens.iter().map(|(name, _, _, _)| *name).collect();
    for name in &registered {
        assert!(
            covered.contains(name),
            "builtin {name} has no coverage specimen — add one in the same change"
        );
    }
    for name in &covered {
        assert!(
            registered.contains(name),
            "specimen names builtin {name} that the registry does not define — stale specimen"
        );
    }
    for (name, form, env, expected) in &specimens {
        let got =
            eval_sandboxed(form, env).unwrap_or_else(|err| panic!("{name}: {form} errored: {err}"));
        assert_eq!(&got, expected, "{name}: {form}");
    }
}

/// Every special form has a specimen that evaluates to its expected value —
/// the specials side of the L2 semantics-table inventory.
#[test]
fn every_special_form_has_a_passing_specimen() {
    let env = serde_json::json!({});
    let specimens: &[(&str, &str, Value)] = &[
        ("quote", "(quote (1 2))", serde_json::json!([1, 2])),
        ("if", "(if true 1 2)", serde_json::json!(1)),
        ("let", "(let ((x 2)) (* x x))", serde_json::json!(4)),
        ("lambda", "((lambda (x) (* x 10)) 3)", serde_json::json!(30)),
        ("define", "(define x 5)", serde_json::json!(null)),
        ("begin", "(begin 1 2)", serde_json::json!(2)),
        ("and", "(and 1 2)", serde_json::json!(2)),
        ("or", "(or nil 3)", serde_json::json!(3)),
        ("not", "(not nil)", serde_json::json!(true)),
        ("cond", "(cond (nil 1) (t 2))", serde_json::json!(2)),
    ];
    for (name, form, expected) in specimens {
        let got = eval_sandboxed(form, &env)
            .unwrap_or_else(|err| panic!("{name}: {form} errored: {err}"));
        assert_eq!(&got, expected, "{name}: {form}");
    }
}

/// Pillar 2 (reproducible): the same (form, env) produces byte-identical
/// output across repeated evaluation — success values and typed errors
/// both.
#[test]
fn repeated_evaluation_is_byte_identical() {
    let env = serde_json::json!({"data": {"a": 1, "b": "x"}, "n": 3});
    let cases = [
        "(+ 1 2 3)",
        "(let ((x 2)) (* x 30))",
        "(define f (lambda (n) (if (= n 0) 0 (+ n (f (- n 1)))))) (f 30)",
        "(assoc \"b\" data)",
        "(length \"héllo\")",
        "(string-contains \"é\" \"héllo\")",
        "(max 9007199254740993 9007199254740992)",
        "(max 9007199254740992 9007199254740993)",
        "(reverse (list 1 2 3))",
        "(concat \"a\" \"éllo\")",
        "(nth 2 (list 5 6 7))",
        "(begin (define y 3) (* y y))",
        "(and (> 2 1) (string= \"a\" \"a\") (not (is_null (list))))",
        "data",
        "(+ 9223372036854775807 1)",
        "(= 1 \"x\")",
        "(string-contains \"\" \"x\")",
        "nosuchsymbol",
    ];
    for form in cases {
        let mut seen: Option<String> = None;
        for _ in 0..5 {
            let out = match eval_sandboxed(form, &env) {
                Ok(value) => serde_json::to_string(&value).expect("serialize"),
                Err(err) => format!("error: {err}"),
            };
            match &seen {
                None => seen = Some(out),
                Some(previous) => assert_eq!(&out, previous, "{form} drifted across runs"),
            }
        }
        assert!(seen.is_some());
    }
}

/// Top-level env keys become individual bindings, so a form cannot observe
/// their JSON key order: two env objects with identical content in
/// different insertion orders evaluate identically (nested-object key order
/// round-trips by design — documented in the L2 semantics table).
#[test]
fn env_key_order_does_not_change_evaluation_results() {
    let mut env_a = serde_json::Map::new();
    env_a.insert("a".to_string(), serde_json::json!(1));
    env_a.insert("b".to_string(), serde_json::json!(2));
    env_a.insert("c".to_string(), serde_json::json!(3));
    env_a.insert("obj".to_string(), serde_json::json!({"k": 7, "j": 8}));
    let mut env_b = serde_json::Map::new();
    env_b.insert("obj".to_string(), serde_json::json!({"j": 8, "k": 7}));
    env_b.insert("c".to_string(), serde_json::json!(3));
    env_b.insert("b".to_string(), serde_json::json!(2));
    env_b.insert("a".to_string(), serde_json::json!(1));
    let (env_a, env_b) = (Value::Object(env_a), Value::Object(env_b));
    for form in [
        "(+ a b c)",
        "(assoc \"k\" obj)",
        "(member 2 (list 1 2))",
        "(cons obj (list))",
    ] {
        let a = eval_sandboxed(form, &env_a).unwrap_or_else(|err| panic!("{form}: {err}"));
        let b = eval_sandboxed(form, &env_b).unwrap_or_else(|err| panic!("{form}: {err}"));
        assert_eq!(a, b, "{form} differs across env key orders");
    }
}

/// Wire-format corners pinned for the L2 semantics table: Symbols
/// serialize as strings, a bare lambda as "<lambda>", the empty list as [],
/// and an alist as a JSON object.
#[test]
fn output_wire_format_corners_are_pinned() {
    let env = serde_json::json!({});
    assert_eq!(
        eval_sandboxed("(car (quote (sym x)))", &env).expect("symbol car"),
        serde_json::json!("sym")
    );
    assert_eq!(
        eval_sandboxed("(lambda (x) x)", &env).expect("bare lambda value"),
        serde_json::json!("<lambda>")
    );
    assert_eq!(
        eval_sandboxed("(list)", &env).expect("empty list"),
        serde_json::json!([])
    );
    assert_eq!(
        eval_sandboxed("(list (list \"a\" 1) (list \"b\" 2))", &env).expect("alist"),
        serde_json::json!({"a": 1, "b": 2})
    );
}

/// Infix is token-level rewriting (all five behaviors verified live
/// 2026-09-28 and pinned as current behavior — changing any of them is the
/// operator's L4 call): bare `a + b` expands; chained same-operator folds;
/// MIXED operators do not associate (`1 + 2 * 3` parses as three
/// top-level forms and the last one's value, 3, wins); a parenthesized
/// `(a + b)` double-wraps to `((+ a b))` and errors — NOT equivalent to
/// `(+ a b)` despite the doc's "equivalent" phrasing (an L4 divergence
/// item); and the rewriter also fires inside prefix forms — `(- 5 - 3)`
/// becomes `(- (- 5 3))` → -2.
#[test]
fn infix_expansion_behavior_is_pinned() {
    let env = serde_json::json!({"z": 1});
    assert_eq!(
        eval_sandboxed("z + 1", &env).expect("bare infix"),
        serde_json::json!(2)
    );
    assert_eq!(
        eval_sandboxed("1 + 2 + 3", &env).expect("chained same-operator"),
        serde_json::json!(6)
    );
    assert_eq!(
        eval_sandboxed("1 + 2 * 3", &env).expect("mixed: last top-level form wins"),
        serde_json::json!(3)
    );
    let parenthesized = eval_sandboxed("(z + 1)", &env).expect_err("parenthesized double-wraps");
    assert!(
        matches!(parenthesized, LispError::TypeError { .. }),
        "{parenthesized}"
    );
    assert_eq!(
        eval_sandboxed("(- 5 - 3)", &env).expect("rewrite inside a prefix form"),
        serde_json::json!(-2)
    );
}

/// Seeded (deterministic) fuzz for the crash and lie classes: bounded
/// random forms over the numeric-edge atom pool must terminate with Ok or
/// a typed error (totality — a panic fails the test) and evaluate
/// byte-identically on a second pass (determinism).
#[test]
fn seeded_random_forms_are_total_and_deterministic() {
    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }
    let heads = [
        "+",
        "-",
        "*",
        "/",
        "=",
        "!=",
        "<",
        "<=",
        ">",
        ">=",
        "car",
        "cdr",
        "cons",
        "list",
        "length",
        "nth",
        "reverse",
        "is_null",
        "numberp",
        "listp",
        "stringp",
        "assoc",
        "append",
        "string=",
        "concat",
        "string-contains",
        "abs",
        "sqrt",
        "max",
        "min",
        "eq",
        "member",
        "quote",
        "if",
        "let",
        "lambda",
        "define",
        "begin",
        "and",
        "or",
        "not",
        "cond",
        "frobnicate",
        "t",
        "zz",
    ];
    let atoms: Vec<String> = [
        "0",
        "1",
        "-1",
        "9223372036854775807",
        "-9223372036854775808",
        "9007199254740992",
        "9007199254740993",
        "1.5",
        "9007199254740992.0",
        "0.0",
        "\"\"",
        "\"a\"",
        "\"héllo→\"",
        "\"quoted\\\"x\"",
        "true",
        "false",
        "nil",
        "t",
        "zz",
        "data",
    ]
    .iter()
    .map(|atom| (*atom).to_string())
    .collect();
    let env = serde_json::json!({"data": {"a": 1, "b": "x"}});
    for seed in [0x9E37_79B9_7F4A_7C15u64, 0xD1CE_BA5E_5EED_0001u64] {
        let mut rng = XorShift(seed);
        for _ in 0..500 {
            let head = heads[rng.below(heads.len())];
            let arity = rng.below(5);
            let mut args: Vec<String> = Vec::new();
            for _ in 0..arity {
                if rng.below(4) == 0 {
                    let sub_head = heads[rng.below(heads.len())];
                    let atom = &atoms[rng.below(atoms.len())];
                    args.push(format!("({sub_head} {atom})"));
                } else {
                    args.push(atoms[rng.below(atoms.len())].clone());
                }
            }
            let form = if args.is_empty() {
                format!("({head})")
            } else {
                format!("({head} {})", args.join(" "))
            };
            let first = match eval_sandboxed(&form, &env) {
                Ok(value) => serde_json::to_string(&value).expect("serialize"),
                Err(err) => format!("error: {err}"),
            };
            let second = match eval_sandboxed(&form, &env) {
                Ok(value) => serde_json::to_string(&value).expect("serialize"),
                Err(err) => format!("error: {err}"),
            };
            assert_eq!(first, second, "nondeterministic: {form}");
        }
    }
}

proptest! {
    /// expect: "An unset invocation flag preserves default results, exactness and budget failures" [H6A]
    #[test]
    fn h6a_unset_flag_preserves_pure_entry(
        values in prop::collection::vec(-1000i64..1000, 0..80),
        steps in prop_oneof![Just(0u64), Just(100_000), 1u64..2000],
    ) {
        let env = serde_json::json!({"xs": values});
        for form in [
            "(reverse xs)", "(eq xs xs)", "(member 7 xs)", "(length xs)",
            "(assoc! \"missing\" xs)", "(/ 9007199254740993 1)",
            "(define f (lambda (x) (if (= x 0) 42 (f (- x 1))))) (f 10)",
        ] {
            let ordinary = eval_sandboxed_with_budget(form, &env, steps, 1024);
            let flag = Arc::new(AtomicBool::new(false));
            let controlled = eval_sandboxed_with_cancellation(form, &env, steps, 1024, flag.clone());
            match (ordinary, controlled) {
                (Ok(left), Ok(right)) => prop_assert_eq!(left, right),
                (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
                (left, right) => prop_assert!(false, "entry mismatch: {:?} versus {:?}", left, right),
            }
            prop_assert!(!flag.load(Ordering::Acquire));
        }
    }

    /// expect: "Integer literal admission matches the signed i64 domain without float fallback" [P8]
    #[test]
    fn integer_literal_admission_matches_rust_domain(
        value in prop_oneof![numeric_edge().prop_map(i128::from), any::<i128>()],
    ) {
        let result = eval_sandboxed(&value.to_string(), &serde_json::json!({}));
        match i64::try_from(value) {
            Ok(expected) => prop_assert_eq!(result.expect("i64 literal evaluates"), serde_json::json!(expected)),
            Err(_) => prop_assert!(matches!(result, Err(LispError::Parse(_)))),
        }
    }
}

/// expect: "Operand validation is not hidden by an earlier overflow or false comparison" [P8]
#[test]
fn numeric_validation_precedes_folding() {
    let env = serde_json::json!({});
    for form in ["(+ 1e308 1e308 \"x\")", "(= 1.0 2.0 \"x\")"] {
        assert!(
            matches!(eval_sandboxed(form, &env), Err(LispError::TypeError { .. })),
            "{form} must report the invalid operand"
        );
    }
    let error = eval_sandboxed("(= 1 2 9007199254740993 0.0)", &env)
        .expect_err("false comparison must still reject lossy promotion");
    assert!(error.to_string().contains("exactly"));
}
