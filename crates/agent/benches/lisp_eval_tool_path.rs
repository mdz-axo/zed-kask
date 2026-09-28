//! Whole-tool-path bench for `lisp_eval` — lisp-repair L3.
//!
//! Measures the exact per-call sequence `LispEvalTool` performs: deserialize
//! the tool arguments through the teaching deserialization path
//! (`LispEvalToolInput` via `serde_json::from_value`), evaluate
//! (`evaluate_lisp` — env canonicalization + the engine), and serialize the
//! result. Production path, no test doubles; fixtures are the real gate
//! shapes, and each expected result is asserted once outside timing.

use std::hint::black_box;
use std::time::Duration;

use agent::{LispEvalToolInput, evaluate_lisp};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
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
    json!({ "form": form, "env": env })
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
    let fixtures: Vec<(&str, Value, &str)> = vec![
        ("small form", tool_args("(+ 1 2 3)", json!({})), "6"),
        (
            "gate form over 100 assignments",
            tool_args(COUNT_VERIFIED, assignments_env(100)),
            "100",
        ),
        ("wide env with 50 bindings", wide_args, "1225"),
        (
            "string-contains over 10kb haystack",
            tool_args(
                "(string-contains \"marker claim c123\" haystack)",
                json!({ "haystack": haystack }),
            ),
            "true",
        ),
    ];
    for (name, args, expected) in &fixtures {
        assert_eq!(&run_tool_path(args), expected, "fixture {name}");
    }

    for (name, args, _) in &fixtures {
        group.bench_function(*name, |b| {
            b.iter_batched(
                || args.clone(),
                |args| black_box(run_tool_path(&args)),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, bench_tool_path);
criterion_main!(benches);
