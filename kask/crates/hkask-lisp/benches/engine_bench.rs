//! Criterion benches for the sandboxed Lisp evaluator — lisp-repair L3.
//!
//! Workspace bench discipline (gpui-bench skill, plain-criterion path for
//! non-GPUI compute; precedent: `kask/crates/hkask-condenser/benches/`):
//! - production shapes — these are the real skill-gate forms (the audit's
//!   count-verified walker, plain walkers, length, assoc chains,
//!   string-contains citation verification), not synthetic micro-ops;
//! - deterministic fixtures with the expected result asserted once per
//!   fixture OUTSIDE the timed loop (a faster wrong answer is not a win);
//! - bounded runs — sample_size(30) / measurement_time(1s) / warm-up 300ms
//!   keeps the suite far under the five-minute invocation cap;
//! - before/after — run with `--save-baseline` before a change and
//!   `--baseline` after: the same bench code measures both revisions.
//!
//! The walker benches use an EXPLICIT 2,000,000-step budget so the walk
//! completes on both sides of the L3 cost-model fix (pre-fix it needs
//! ~365,000 steps for 600 elements; post-fix ~6,000) — the default-budget
//! outcome is pinned by the matrix test, not benched.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use hkask_lisp::eval_sandboxed_with_budget;
use serde_json::{Value, json};

/// The pinned walker form (the L0 step-cost-anomaly specimen).
const WALKER: &str =
    "(define count (lambda (lst) (if (is_null lst) 0 (+ 1 (count (cdr lst)))))) (count items)";

/// The audit's real gate form (the L2 real-scale specimen): count claims
/// whose provenance contains a marker, over the claim-assignment shape.
const COUNT_VERIFIED: &str = r#"
    (define count-verified
      (lambda (lst)
        (if (< (length lst) 1)
            0
            (+ (if (string-contains "tool_verified" (assoc "provenance" (car lst))) 1 0)
               (count-verified (cdr lst))))))
    (count-verified assignments)
"#;

fn items_env(n: i64) -> Value {
    json!({ "items": (0..n).collect::<Vec<i64>>() })
}

fn assignments_env(n: usize) -> Value {
    let assignments: Vec<Value> = (0..n)
        .map(|i| json!({"claim_id": format!("c{i}"), "provenance": "tool_verified"}))
        .collect();
    json!({ "assignments": assignments })
}

fn haystack_10kb() -> String {
    let mut body = "The claim was verified against its primary source. ".repeat(232);
    body.push_str("marker claim c123 verified at the end");
    body
}

fn configure<'a>(
    group_name: &str,
    c: &'a mut Criterion,
) -> criterion::BenchmarkGroup<'a, criterion::measurement::WallTime> {
    let mut group = c.benchmark_group(group_name);
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(300));
    group
}

fn bench_gate_forms(c: &mut Criterion) {
    let mut group = configure("gate forms", c);

    let env = items_env(100);
    assert_eq!(
        eval_sandboxed_with_budget("(length items)", &env, 100_000, 1024)
            .expect("length fixture evaluates"),
        json!(100)
    );
    group.throughput(Throughput::Elements(100));
    group.bench_function("length over 100-element list", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box("(length items)"),
                black_box(&env),
                100_000,
                1024,
            ))
        })
    });

    let gate_env = assignments_env(100);
    assert_eq!(
        eval_sandboxed_with_budget(COUNT_VERIFIED, &gate_env, 100_000, 1024)
            .expect("gate fixture evaluates"),
        json!(100)
    );
    group.throughput(Throughput::Elements(100));
    group.bench_function("count-verified over 100 assignments", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(COUNT_VERIFIED),
                black_box(&gate_env),
                100_000,
                1024,
            ))
        })
    });

    // The walker scaling curve: 100/300/600 elements at the explicit
    // budget (see the module doc for why the budget is explicit).
    for n in [100usize, 300, 600] {
        let env = items_env(n as i64);
        assert_eq!(
            eval_sandboxed_with_budget(WALKER, &env, 2_000_000, 8192)
                .expect("walker fixture completes"),
            json!(n)
        );
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("walker", n), &env, |b, env| {
            b.iter(|| {
                black_box(eval_sandboxed_with_budget(
                    black_box(WALKER),
                    black_box(env),
                    2_000_000,
                    8192,
                ))
            })
        });
    }
    group.finish();
}

fn bench_string_ops(c: &mut Criterion) {
    let mut group = configure("string ops", c);

    let env = json!({ "haystack": haystack_10kb() });
    let hit = "(string-contains \"marker claim c123\" haystack)";
    let miss = "(string-contains \"zz-absent-marker-zz\" haystack)";
    assert_eq!(
        eval_sandboxed_with_budget(hit, &env, 100_000, 1024).expect("hit fixture evaluates"),
        json!(true)
    );
    assert_eq!(
        eval_sandboxed_with_budget(miss, &env, 100_000, 1024).expect("miss fixture evaluates"),
        json!(false)
    );
    group.bench_function("string-contains 10kb haystack (hit)", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(hit),
                black_box(&env),
                100_000,
                1024,
            ))
        })
    });
    group.bench_function("string-contains 10kb haystack (miss)", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(miss),
                black_box(&env),
                100_000,
                1024,
            ))
        })
    });
    group.finish();
}

fn bench_assoc(c: &mut Criterion) {
    let mut group = configure("assoc", c);

    let deep = json!({ "data": {"l1": {"l2": {"l3": {"l4": {"k": "hit"}}}}}});
    let chain = "(assoc \"k\" (assoc \"l4\" (assoc \"l3\" (assoc \"l2\" (assoc \"l1\" data)))))";
    assert_eq!(
        eval_sandboxed_with_budget(chain, &deep, 100_000, 1024)
            .expect("nested-chain fixture evaluates"),
        json!("hit")
    );
    group.bench_function("assoc chain through 5-deep object", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(chain),
                black_box(&deep),
                100_000,
                1024,
            ))
        })
    });

    let pairs: Vec<Value> = (0..100).map(|i| json!([format!("k{i}"), i])).collect();
    let long = json!({ "alist": pairs });
    let last = "(assoc \"k99\" alist)";
    assert_eq!(
        eval_sandboxed_with_budget(last, &long, 100_000, 1024)
            .expect("long-alist fixture evaluates"),
        json!(99)
    );
    group.throughput(Throughput::Elements(100));
    group.bench_function("assoc last key of 100-pair alist", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(last),
                black_box(&long),
                100_000,
                1024,
            ))
        })
    });
    group.finish();
}

fn bench_boundary(c: &mut Criterion) {
    let mut group = configure("boundary", c);

    let deep_quote = format!("'{}1{}", "(".repeat(100), ")".repeat(100));
    let empty = json!({});
    // The quoted literal is 100 nested arrays around the innermost Int 1 —
    // walk down and assert the shape (completion check, not just Ok).
    let mut cursor = eval_sandboxed_with_budget(&deep_quote, &empty, 100_000, 1024)
        .expect("deep-quote fixture evaluates");
    let mut depth = 0usize;
    while let Some(inner) = cursor.as_array().and_then(|a| a.first().cloned()) {
        if !inner.is_array() {
            cursor = inner;
            break;
        }
        cursor = inner;
        depth += 1;
    }
    assert_eq!((depth, cursor.as_i64()), (99, Some(1)));
    group.bench_function("100-deep quoted literal", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(&deep_quote),
                black_box(&empty),
                100_000,
                1024,
            ))
        })
    });

    let flat = format!("'({})", "1 ".repeat(10_000));
    assert_eq!(
        eval_sandboxed_with_budget(&flat, &empty, 100_000, 1024)
            .expect("flat-literal fixture evaluates")
            .as_array()
            .map(Vec::len),
        Some(10_000)
    );
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("10,000-element flat literal", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(&flat),
                black_box(&empty),
                100_000,
                1024,
            ))
        })
    });
    group.finish();
}

fn bench_small_forms(c: &mut Criterion) {
    let mut group = configure("small forms", c);

    let empty = json!({});
    let gate_flavored = "(if (and (> 2 1) (string= \"a\" \"a\")) (* 7 6) 0)";
    assert_eq!(
        eval_sandboxed_with_budget("(+ 1 2 3)", &empty, 100_000, 1024)
            .expect("small fixture evaluates"),
        json!(6)
    );
    assert_eq!(
        eval_sandboxed_with_budget(gate_flavored, &empty, 100_000, 1024)
            .expect("gate-flavored fixture evaluates"),
        json!(42)
    );
    group.bench_function("small arithmetic form", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box("(+ 1 2 3)"),
                black_box(&empty),
                100_000,
                1024,
            ))
        })
    });
    group.bench_function("small gate-flavored form", |b| {
        b.iter(|| {
            black_box(eval_sandboxed_with_budget(
                black_box(gate_flavored),
                black_box(&empty),
                100_000,
                1024,
            ))
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_gate_forms,
    bench_string_ops,
    bench_assoc,
    bench_boundary,
    bench_small_forms,
);
criterion_main!(benches);
