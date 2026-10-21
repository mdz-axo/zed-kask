//! Whole-tool-path bench for `lisp_eval` — lisp-repair L3.
//!
//! Compares the tool's teaching decode + `evaluate_lisp` path with the engine
//! over the same forms and bindings. Both timings include compact result
//! serialization (not the editor's pretty document or protocol envelope).
//! Production evaluators, no test doubles; each expected result is asserted
//! on both paths outside timing. Semantic fixtures live in the conformance tests;
//! this benchmark keeps four representative workloads, not another copy of that
//! suite. Test-profile measurements are not release-performance or heap-quota claims.

use std::hint::black_box;
use std::time::Duration;

use agent::{LispEvalToolInput, evaluate_lisp};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use hkask_lisp::eval_sandboxed;
use serde_json::{Value, json};

const COUNT_VERIFIED: &str = r#"
    (define count-verified
      (lambda (lst)
        (if (< (length lst) 1)
            0
            (+ (if (string-contains "tool_verified" (assoc "provenance" (car lst))) 1 0)
               (count-verified (cdr lst))))))
    (count-verified assignments)
"#;

fn tool_args(form: &str, env: Value) -> Value {
    json!({ "form": form, "env": serde_json::to_string(&env).expect("encode fixture env as JSON text") })
}

fn assignments_env(n: usize) -> Value {
    let assignments: Vec<Value> = (0..n)
        .map(|i| json!({"claim_id": format!("c{i}"), "provenance": "tool_verified"}))
        .collect();
    json!({ "assignments": assignments })
}

/// The full path once: deserialize → evaluate → serialize. Also the
/// per-fixture correctness check (asserted once outside timing).
fn run_tool_path(args: &Value) -> String {
    let input: LispEvalToolInput =
        serde_json::from_value(args.clone()).expect("fixture deserializes");
    let out = evaluate_lisp(input).expect("fixture evaluates");
    serde_json::to_string(&out).expect("fixture serializes")
}

/// Compare the engine and corrected tool on identical forms and bindings.
fn run_engine_path(args: &Value) -> String {
    let form = args["form"].as_str().expect("fixture form is text");
    let env = serde_json::from_str(args["env"].as_str().expect("fixture env is JSON text"))
        .expect("fixture env decodes");
    let out = eval_sandboxed(form, &env).expect("engine fixture evaluates");
    serde_json::to_string(&out).expect("engine fixture serializes")
}

fn bench_tool_path(c: &mut Criterion) {
    let mut group = c.benchmark_group("lisp_eval tool path");
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(300));

    // (name, args, expected serialized result)
    let mut wide_env = serde_json::Map::new();
    let mut wide_form = String::from("(+");
    for i in 0..50 {
        wide_env.insert(format!("k{i}"), json!(i));
        wide_form.push_str(format!(" k{i}").as_str());
    }
    wide_form.push(')');
    let wide_args = tool_args(&wide_form, Value::Object(wide_env));

    let haystack = format!(
        "{}marker claim c123 verified at the end",
        "The claim was verified against its primary source. ".repeat(232)
    );
    let fixtures: Vec<(&str, Value, String)> = vec![
        (
            "small form",
            tool_args("(+ 1 2 3)", json!({})),
            "6".to_string(),
        ),
        (
            "gate form over 100 assignments",
            tool_args(COUNT_VERIFIED, assignments_env(100)),
            "100".to_string(),
        ),
        ("wide env with 50 bindings", wide_args, "1225".to_string()),
        (
            "string-contains over 10kb haystack",
            tool_args(
                "(string-contains \"marker claim c123\" haystack)",
                json!({ "haystack": haystack }),
            ),
            "true".to_string(),
        ),
    ];

    for (name, args, expected) in &fixtures {
        assert_eq!(&run_tool_path(args), expected, "tool fixture {name}");
        assert_eq!(&run_engine_path(args), expected, "engine fixture {name}");
    }

    for (name, args, _) in &fixtures {
        group.bench_function(format!("tool/{name}"), |b| {
            b.iter_batched(
                || args.clone(),
                |args| black_box(run_tool_path(&args)),
                BatchSize::SmallInput,
            )
        });
    }
    for (name, args, _) in &fixtures {
        // Bindings are decoded outside engine timing; the tool timing includes
        // its production JSON-text decode. Both paths serialize the same result.
        let form = args["form"].as_str().expect("fixture form is text");
        let env = serde_json::from_str(args["env"].as_str().expect("fixture env is JSON text"))
            .expect("fixture env decodes");
        group.bench_function(format!("engine/{name}"), |b| {
            b.iter(|| {
                let out = eval_sandboxed(black_box(form), black_box(&env))
                    .expect("engine fixture evaluates");
                black_box(serde_json::to_string(&out).expect("engine fixture serializes"))
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tool_path);
criterion_main!(benches);
