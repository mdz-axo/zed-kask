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
    // K2 LANDED: the `parallel` step action now wires concurrency at the
    // per-step level via `input_mapping.concurrency_cap` (bounded
    // `buffer_unordered`). The manifest-level `concurrency` field remains
    // unwired (advisory) — it does not drive the top-level sequential
    // `run_pass`. This test still pins that the manifest-level field has no
    // effect on the bench manifest (which uses compute/choice/loop/abort, no
    // `parallel` step). The regression it guards is silently *dropping* the
    // field's semantics without updating this test.
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

#[test]
fn check_step_cap_rejects_over_cap() {
    // PINS THE HARD GATE FIRES: `check_step_cap` is the shared hard gate
    // called by `execute_manifest_into` (top-level), `execute_flowdef`
    // (sub-cascade), and `execute_parallel` (parallel branch sub-cascade).
    // Previously only the top-level path had the hard gate; the sub-cascade
    // paths got only the advisory `tracing::warn!` from `StepGraph::new`,
    // which was an open loop — a sub-cascade could exceed the cap and run
    // to completion. This test pins that the gate fires for all callers.
    use hkask_templates::step_graph::check_step_cap;

    // At cap — passes.
    assert!(check_step_cap(MAX_STEPS, "test at-cap").is_ok());
    // Over cap — fails with a message naming the context and the cap.
    let err = check_step_cap(MAX_STEPS + 1, "test over-cap").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("test over-cap"),
        "error must name the context: {msg}"
    );
    assert!(
        msg.contains(&MAX_STEPS.to_string()),
        "error must name the cap: {msg}"
    );
    assert!(
        msg.contains(&(MAX_STEPS + 1).to_string()),
        "error must name the actual step count: {msg}"
    );
}

#[tokio::test]
async fn execute_manifest_rejects_over_cap_at_top_level() {
    // PINS THE HARD GATE FIRES AT THE TOP-LEVEL PATH: `execute_manifest_into`
    // returns `Err` for a manifest exceeding `MAX_STEPS`. This is the K5
    // gate; the sub-cascade paths are pinned by `check_step_cap_rejects_over_cap`
    // (unit) and the flowdef/parallel integration tests.
    let over = MAX_STEPS + 1;
    let mut steps_yaml = String::from("manifest:\n  id: overcap-top\n  category: skill\nsteps:\n");
    for ordinal in 1..=over as u32 {
        steps_yaml.push_str(&format!(
            "  - ordinal: {ordinal}\n    action: abort\n    description: x\n"
        ));
    }
    let manifest = load_manifest_from_yaml(&steps_yaml).expect("over-cap manifest must parse");
    let executor = ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    )
    .with_template_base_path(std::path::PathBuf::from("/nonexistent"));
    let result = executor
        .execute_manifest_into(manifest, std::collections::HashMap::new())
        .await;
    assert!(
        result.is_err(),
        "execute_manifest_into must reject a manifest exceeding MAX_STEPS"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("overcap-top"),
        "error must name the manifest id: {err}"
    );
    assert!(
        err.contains(&MAX_STEPS.to_string()),
        "error must name the cap: {err}"
    );
}

// ── K2: parallel action ──────────────────────────────────────────────────

/// Sub-manifest for `parallel` branch A: single compute step, `(+ 1 10)` → 11.
const PARALLEL_BRANCH_A_YAML: &str = "\
manifest:
  id: parallel-branch-a
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-a
    input_mapping:
      form: \"(+ 1 10)\"
";

/// Sub-manifest for `parallel` branch B: single compute step, `(* 2 20)` → 40.
const PARALLEL_BRANCH_B_YAML: &str = "\
manifest:
  id: parallel-branch-b
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-b
    input_mapping:
      form: \"(* 2 20)\"
";

/// Parent manifest: a single `parallel` step with two branches.
const PARALLEL_PARENT_YAML: &str = "\
manifest:
  id: parallel-test
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: parallel
    description: run two branches concurrently
    input_mapping:
      branches:
        - template_ref: \"parallel-branch-a\"
        - template_ref: \"parallel-branch-b\"
      concurrency_cap: 2
      join: list
";

#[tokio::test]
async fn parallel_step_joins_branch_results_in_order() {
    // K2 enforcement point: the `parallel` action dispatches N sub-cascades
    // with bounded concurrency (`buffer_unordered`), then joins results in
    // `branch_id` order (deterministic, not completion order). This test pins:
    //
    // 1. The action compiles, dispatches, and returns a `Value::Array`.
    // 2. Branch results are in `branch_id` order (0 before 1), not
    //    completion order — the sort by `branch_id` is the deterministic join.
    // 3. Each branch result is the sub-cascade's `extract_final_step_result`
    //    (the compute step's value), not the whole sub-cascade context.
    // 4. Gas is shared (both branches charge the parent's `Arc<AtomicU64>`);
    //    rJoule is settled after the wave (parent charges the sum).

    // Write sub-manifests to a temp dir so `load_from_disk` resolves them.
    let tmp = std::env::temp_dir().join("hkask-parallel-test-");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");
    std::fs::write(tmp.join("parallel-branch-a.yaml"), PARALLEL_BRANCH_A_YAML)
        .expect("write branch-a");
    std::fs::write(tmp.join("parallel-branch-b.yaml"), PARALLEL_BRANCH_B_YAML)
        .expect("write branch-b");

    let manifest = load(PARALLEL_PARENT_YAML);
    let executor = ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    )
    .with_template_base_path(tmp.clone());

    let result = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .expect("parallel manifest must execute");

    let final_value = extract_final_step_result(&result);

    // The joined result is a 2-element array in branch_id order.
    let expected = Value::Array(vec![Value::from(11), Value::from(40)]);
    assert_eq!(
        final_value, expected,
        "parallel step must join branch results in branch_id order, \
         not completion order"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(&tmp);
}

// ── Parallel allSettled: no branch outcome silently dropped ──────────────

/// Sub-manifest for `allSettled` branch 0: compute `(+ 1 10)` → 11.
const ALLSETTLED_BRANCH_0_YAML: &str = "\
manifest:
  id: allsettled-branch-0
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-0
    input_mapping:
      form: \"(+ 1 10)\"
";

/// Sub-manifest for `allSettled` branch 1: compute `(* 2 20)` → 40.
const ALLSETTLED_BRANCH_1_YAML: &str = "\
manifest:
  id: allsettled-branch-1
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-1
    input_mapping:
      form: \"(* 2 20)\"
";

/// Sub-manifest for `allSettled` branch 2: compute `(+ 3 30)` → 33.
const ALLSETTLED_BRANCH_2_YAML: &str = "\
manifest:
  id: allsettled-branch-2
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: compute
    compute_ref: lisp.eval
    description: compute-2
    input_mapping:
      form: \"(+ 3 30)\"
";

/// Parent manifest: a single `parallel` step with 4 branches, one of which
/// (branch 1) references a non-existent sub-manifest and will error with
/// `TemplateError::NotFound`. `join: allSettled` opts into the Promise.allSettled
/// discipline so successful branches are preserved.
const ALLSETTLED_PARENT_YAML: &str = "\
manifest:
  id: allsettled-test
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: parallel
    description: run four branches, one will error
    input_mapping:
      branches:
        - template_ref: \"allsettled-branch-0\"
        - template_ref: \"allsettled-branch-missing\"
        - template_ref: \"allsettled-branch-1\"
        - template_ref: \"allsettled-branch-2\"
      join: allSettled
";

/// Pin: no branch outcome is silently dropped. 4 branches, branch 1 errors
/// (missing sub-manifest → `TemplateError::NotFound`), branches 0/2/3 succeed.
/// Under `join: allSettled`, the 3 successful results MUST be present in the
/// output (under `results`), and the `errors` sidecar MUST record the failure.
/// This guards against the first-error-abort regression (audit finding B1):
/// the historical `list` mode dropped sibling outcomes on first `Err`.
#[tokio::test]
async fn parallel_allsettled_preserves_successful_branches_when_one_errors() {
    // Write the 3 successful sub-manifests to a temp dir. Branch 1
    // ("allsettled-branch-missing") is deliberately NOT written → its
    // `load_from_disk` / `template_file` lookups fail → `TemplateError::NotFound`.
    let tmp = std::env::temp_dir().join("hkask-parallel-allsettled-test-");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");
    std::fs::write(
        tmp.join("allsettled-branch-0.yaml"),
        ALLSETTLED_BRANCH_0_YAML,
    )
    .expect("write branch-0");
    std::fs::write(
        tmp.join("allsettled-branch-1.yaml"),
        ALLSETTLED_BRANCH_1_YAML,
    )
    .expect("write branch-1");
    std::fs::write(
        tmp.join("allsettled-branch-2.yaml"),
        ALLSETTLED_BRANCH_2_YAML,
    )
    .expect("write branch-2");

    let manifest = load(ALLSETTLED_PARENT_YAML);
    let executor = ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    )
    .with_template_base_path(tmp.clone());

    let result = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .expect("allSettled manifest must execute despite one branch error");

    let final_value = extract_final_step_result(&result);

    // Under allSettled, the stored value is an object with `results` (the
    // successful branch outcomes in branch_id order) and `errors` (the
    // failure summaries). The 3 successful results MUST be present — this is
    // the core invariant: no branch outcome is silently dropped.
    let obj = final_value
        .as_object()
        .expect("allSettled result must be a Value::Object with results + errors");
    let results = obj
        .get("results")
        .and_then(|v| v.as_array())
        .expect("allSettled result must have a `results` array");
    let errors = obj
        .get("errors")
        .and_then(|v| v.as_array())
        .expect("allSettled result must have an `errors` array");

    // 3 successful branches → 3 results, in branch_id order (0, 2, 3).
    // The successful branches are 0, 2, 3 (branch 1 errored). Their results
    // are the compute values: 11, 40, 33.
    assert_eq!(
        results.len(),
        3,
        "allSettled must preserve all 3 successful branch results, got {results:?}"
    );
    assert_eq!(results[0], Value::from(11), "branch 0 result must be 11");
    assert_eq!(results[1], Value::from(40), "branch 2 result must be 40");
    assert_eq!(results[2], Value::from(33), "branch 3 result must be 33");

    // 1 failed branch → 1 error summary with a stable code.
    assert_eq!(
        errors.len(),
        1,
        "allSettled must record exactly 1 error for the failed branch, got {errors:?}"
    );
    let err_code = errors[0]
        .as_object()
        .and_then(|o| o.get("code"))
        .and_then(|v| v.as_str())
        .expect("error summary must have a `code` string");
    assert_eq!(
        err_code, "HKASK-SKILL-001",
        "failed branch error code must be HKASK-SKILL-001 (NotFound)"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Pin: under the default `join: list` mode, the first branch error still
/// aborts the step (backward-compat contract). This guards against the
/// allSettled migration accidentally changing the default behavior.
#[tokio::test]
async fn parallel_list_mode_aborts_on_first_error_backward_compat() {
    let tmp = std::env::temp_dir().join("hkask-parallel-list-compat-test-");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");
    std::fs::write(
        tmp.join("allsettled-branch-0.yaml"),
        ALLSETTLED_BRANCH_0_YAML,
    )
    .expect("write branch-0");
    // branch-1 deliberately missing → NotFound
    std::fs::write(
        tmp.join("allsettled-branch-2.yaml"),
        ALLSETTLED_BRANCH_2_YAML,
    )
    .expect("write branch-2");

    // Same as ALLSETTLED_PARENT_YAML but `join: list` (the default).
    let list_parent_yaml = "\
manifest:
  id: list-compat-test
  category: skill
convergence:
  max_iterations: 1
  threshold: 0.5
  convergence_field: convergence_signal
  on_not_reached: abort
rjoule:
  cap: 10000
steps:
  - ordinal: 1
    action: parallel
    description: run branches, one will error, list mode aborts
    input_mapping:
      branches:
        - template_ref: \"allsettled-branch-0\"
        - template_ref: \"allsettled-branch-missing\"
        - template_ref: \"allsettled-branch-2\"
      join: list
";
    let manifest = load(list_parent_yaml);
    let executor = ManifestExecutor::new(
        Arc::new(NoopInferencePort),
        Arc::new(NoopToolPort::new()),
        LLMParameters::default(),
    )
    .with_template_base_path(tmp.clone());

    let result = executor.execute_manifest(&manifest, HashMap::new()).await;

    // list mode: first Err aborts → the manifest execution returns Err.
    assert!(
        result.is_err(),
        "list mode must abort on first branch error (backward-compat), got Ok: {:?}",
        result.ok()
    );

    let _ = std::fs::remove_dir_all(&tmp);
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
