//! Slice G — executor baseline contract, capacity cap, and tail-latency harness.
//!
//! These tests are the measurement scaffolding the refactor plan (slices K1–K5,
//! K2) runs before/after against. They deliberately live in an *external*
//! integration-test file so they pin the public contract without depending on
//! private modules, and so they are untouched by edits to `executor.rs`,
//! `step_actions.rs`, or `step_context.rs`.
//!
//! ## What each test pins
//!
//! - `golden_output_is_stable` — the deterministic compute-only manifest's
//!   `extract_final_step_result` output. Every later slice must keep this
//!   byte-equal; it is the "no behavior change" sentinel for the kernel.
//! - `concurrency_field_has_no_effect_today` — the manifest's `concurrency`
//!   field is declared (default 32, max 128, doc claims `FuturesUnordered`
//!   parallel dispatch) but the kernel is strictly sequential, so `concurrency:
//!   1` and `concurrency: 32` produce identical output. This pins the
//!   "advertised invariant without an enforcement point" finding (`.rules`).
//!   **This test must be deliberately updated in K2** when `parallel` dispatch
//!   is wired — the regression it guards is *removing* the field's effect, not
//!   adding it.
//! - `compute_steps_do_not_invoke_inference` — a manifest of only `compute`
//!   (incl. `lisp.eval`) steps runs to completion against an `InferencePort`
//!   that errors on every call. Success ⇒ no `compute` path reaches inference
//!   (invariant 3: `lisp.eval` stays a deterministic compute step, free of LLM
//!   drift). This is the enforcement point for that invariant; K2 must keep it
//!   green even under `parallel` dispatch.
//! - `max_steps_constant_is_sane` / `over_cap_graph_builds_without_panic` — the
//!   advisory capacity cap (`StepGraph::MAX_STEPS`) is present and non-breaking;
//!   hard enforcement is sequenced for K5.
//! - `baseline_tail_latency` (`#[ignore]`) — opt-in p50/p95 wall-clock over
//!   `SAMPLES` runs of the representative manifest. Run with
//!   `cargo test --test executor_baseline_contract -- --ignored --nocapture`.
//!   This is the "before" number for slices K1/K4; the asserted ceiling is a
//!   regression guard, not a performance target.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hkask_templates::step_graph::{MAX_STEPS, StepGraph};
use hkask_templates::{ManifestExecutor, extract_final_step_result, load_manifest_from_yaml};
use hkask_test_harness::{NoopInferencePort, NoopToolPort};
use hkask_types::template::LLMParameters;
use serde_json::Value;

/// Representative manifest: 6 steps × 3 iterations, fully deterministic
/// (`compute`/`choice`/`loop`/`abort`) — no inference, no template files. This
/// isolates the kernel's dispatch + clone + convergence/budget overhead from
/// inference-IO and template-IO noise, which is exactly what slices K1/K4
/// optimize. `concurrency: 32` is set so the dead-config test can contrast it
/// with `concurrency: 1`.
const BENCH_MANIFEST_YAML_PARALLEL: &str = "\
manifest:
  id: bench-baseline
  category: skill
  concurrency: 32
convergence:
  max_iterations: 3
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-a
    input_mapping:
      form: \"(+ 1 2 3)\"
  - ordinal: 2
    action: compute
    compute_ref: lisp.eval
    description: compute-b
    input_mapping:
      form: \"(* 2 3)\"
  - ordinal: 3
    action: compute
    compute_ref: lisp.eval
    description: compute-c
    input_mapping:
      form: \"(- 10 4)\"
  - ordinal: 4
    action: choice
    description: gate
    input_mapping:
      branches:
        - condition: default
          action: continue
  - ordinal: 5
    action: loop
    description: iterate
    input_mapping:
      loop_target: \"1\"
  - ordinal: 6
    action: abort
    description: exit
";

/// Same manifest with `concurrency: 1` (and the same `id`, so the only
/// variable between the two is `concurrency`). If the `concurrency` field were
/// wired, this would schedule differently from `concurrency: 32`; today both
/// are sequential so their outputs are identical.
const BENCH_MANIFEST_YAML_SERIAL: &str = "\
manifest:
  id: bench-baseline
  category: skill
  concurrency: 1
convergence:
  max_iterations: 3
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-a
    input_mapping:
      form: \"(+ 1 2 3)\"
  - ordinal: 2
    action: compute
    compute_ref: lisp.eval
    description: compute-b
    input_mapping:
      form: \"(* 2 3)\"
  - ordinal: 3
    action: compute
    compute_ref: lisp.eval
    description: compute-c
    input_mapping:
      form: \"(- 10 4)\"
  - ordinal: 4
    action: choice
    description: gate
    input_mapping:
      branches:
        - condition: default
          action: continue
  - ordinal: 5
    action: loop
    description: iterate
    input_mapping:
      loop_target: \"1\"
  - ordinal: 6
    action: abort
    description: exit
";

/// The expected `extract_final_step_result` for the bench manifest. The
/// highest-ordinal step that stores a result is step 3 (`compute` `(- 10 4)`
/// → 6); step 4 is `choice` (NoOp, no store), step 5 is `loop` (Reenter, no
/// store), step 6 is `abort` (never reached — step 5 re-enters first).
const GOLDEN_FINAL: i64 = 6;

fn build_executor() -> ManifestExecutor {
    ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    )
}

fn load(yaml: &str) -> hkask_templates::BundleManifest {
    load_manifest_from_yaml(yaml).expect("bench manifest must parse")
}

#[tokio::test]
async fn golden_output_is_stable() {
    let manifest = load(BENCH_MANIFEST_YAML_PARALLEL);
    let executor = build_executor();
    let result = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .expect("bench manifest must execute");

    let final_value = extract_final_step_result(&result);
    assert_eq!(
        final_value,
        Value::from(GOLDEN_FINAL),
        "golden final-step result changed — a later slice altered observable behavior"
    );
}

#[tokio::test]
async fn concurrency_field_has_no_effect_today() {
    // PINS THE FINDING: BundleManifest.concurrency (default 32, max 128) is
    // declared and documented as driving FuturesUnordered parallel dispatch,
    // but the kernel's run_pass is a strict sequential loop. Therefore the two
    // manifests below must produce *identical* output maps today.
    //
    // UPDATE IN K2: when execute_parallel / parallel dispatch is wired, this
    // test must be revisited. The regression it guards is silently *dropping*
    // the field's effect, not adding it. If K2 makes concurrency observably
    // schedule steps, replace the equality assertion with one that asserts the
    // SCHEDULED order is deterministic (by StepId, not completion order).
    let manifest_p = load(BENCH_MANIFEST_YAML_PARALLEL);
    let manifest_s = load(BENCH_MANIFEST_YAML_SERIAL);
    let executor = build_executor();

    let result_p = executor
        .execute_manifest(&manifest_p, HashMap::new())
        .await
        .expect("parallel-concurrency manifest must execute");
    let result_s = executor
        .execute_manifest(&manifest_s, HashMap::new())
        .await
        .expect("serial-concurrency manifest must execute");

    assert_eq!(
        result_p.context.materialize(),
        result_s.context.materialize(),
        "concurrency field changed output without any dispatch wiring — \
         either the field is partially wired (a hidden enforcement point) or \
         the test manifest is non-deterministic"
    );
    assert_eq!(
        extract_final_step_result(&result_p),
        Value::from(GOLDEN_FINAL),
    );
}

#[tokio::test]
async fn compute_steps_do_not_invoke_inference() {
    // Invariant 3 enforcement point: a manifest of only `compute` steps
    // (incl. lisp.eval) runs to completion against an InferencePort that
    // returns an error on every call. If any compute path reached inference,
    // execute_manifest would return Err and this test would fail. This pins
    // that `lisp.eval` (and the whole compute dispatch table) stays free of
    // LLM drift — including under any future parallel dispatch (K2), where a
    // race that accidentally routes a compute step through inference would
    // surface here.
    let manifest = load(BENCH_MANIFEST_YAML_PARALLEL);
    let executor = ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    );
    let result = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .expect("compute-only manifest must not call inference");

    assert_eq!(
        extract_final_step_result(&result),
        Value::from(GOLDEN_FINAL),
        "compute-only manifest produced unexpected output"
    );
}

#[test]
fn max_steps_constant_is_sane() {
    // PINS THE CAP EXISTS: the advisory capacity constant is exported and in a
    // sane range. Hard enforcement (returning Err) is sequenced for K5; until
    // then this constant is the documented envelope boundary.
    assert!(
        MAX_STEPS >= 1024,
        "MAX_STEPS ({MAX_STEPS}) is below the sane floor (1024)"
    );
    assert!(
        MAX_STEPS <= 1_000_000,
        "MAX_STEPS ({MAX_STEPS}) is implausibly high — the cap must bound a real envelope"
    );
}

#[test]
fn over_cap_graph_builds_without_panic() {
    // The advisory warn is non-breaking: a manifest exceeding MAX_STEPS still
    // builds a graph (the warn is the diagnostic, not a hard error). Hard
    // enforcement lands in K5 when execute_manifest returns Result<CascadeOutcome>.
    let over = MAX_STEPS + 1;
    let mut steps_yaml = String::from("manifest:\n  id: overcap\n  category: skill\nsteps:\n");
    for ordinal in 1..=over as u32 {
        steps_yaml.push_str(&format!(
            "  - ordinal: {ordinal}\n    action: abort\n    description: x\n"
        ));
    }
    let manifest = load_manifest_from_yaml(&steps_yaml).expect("over-cap manifest must parse");
    let graph = StepGraph::new(&manifest.steps, 1);
    assert_eq!(
        graph.len(),
        over,
        "over-cap graph should build with MAX_STEPS+1 nodes (advisory, non-breaking)"
    );
}

/// Opt-in tail-latency harness. Not run by default (it is slow and measures
/// wall-clock, which is noisy under `cargo test` parallelism). Run explicitly:
///   cargo test --test executor_baseline_contract baseline_tail_latency -- --ignored --nocapture
///
/// Prints p50/p95/mean and asserts a generous p95 ceiling as a regression
/// guard. The ceiling is NOT the performance target — slices K1/K4 are
/// expected to lower the measured p95; raise the ceiling only if a legitimate
/// behavior change increases per-iteration work.
const SAMPLES: usize = 1000;
const P95_CEILING: Duration = Duration::from_millis(8);

#[tokio::test]
#[ignore]
async fn baseline_tail_latency() {
    let manifest = load(BENCH_MANIFEST_YAML_PARALLEL);
    let executor = build_executor();

    // Warm up (first run pays parse/setup costs we don't want in the sample).
    let _ = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .expect("warmup");

    let mut durations_us: Vec<u64> = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let result = executor
            .execute_manifest(&manifest, HashMap::new())
            .await
            .expect("execution");
        let elapsed = start.elapsed();
        // Guard the measurement against a silent behavior break.
        assert_eq!(
            extract_final_step_result(&result),
            Value::from(GOLDEN_FINAL)
        );
        durations_us.push(elapsed.as_micros() as u64);
    }

    durations_us.sort_unstable();
    let p50 = durations_us[SAMPLES / 2];
    let p95 = durations_us[SAMPLES * 95 / 100];
    let mean = durations_us.iter().sum::<u64>() / SAMPLES as u64;
    let max = *durations_us.last().unwrap();

    eprintln!(
        "baseline_tail_latency: n={SAMPLES} p50={p50}µs p95={p95}µs mean={mean}µs max={max}µs"
    );

    assert!(
        Duration::from_micros(p95) < P95_CEILING,
        "p95 ({p95}µs) exceeded regression ceiling ({}µs) — a later slice regressed kernel tail latency",
        P95_CEILING.as_micros(),
    );
}
