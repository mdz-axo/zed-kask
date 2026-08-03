//! Integration test: the swarm cybernetic compute primitives through the full
//! executor across LOOP iterations.
//!
//! Verifies that `swarm.converge_accumulate` and `swarm.second_order_monitor`
//! execute correctly through `ManifestExecutor::execute_manifest` AND that the
//! accumulators (`iteration_log`, `failed_edits`, `influence_scores`) thread
//! through the loop step's `input_mapping` back into context so the next
//! iteration sees them. This catches wiring issues (input_mapping resolution,
//! step-result storage, context propagation across LOOP) that the unit tests
//! in `compute.rs` don't cover — the largest validation gap identified for the
//! Cybernetic Swarm Plan C1/C3/C7 components.

use hkask_templates::executor::ManifestExecutor;
use hkask_templates::load_manifest_from_yaml;
use hkask_test_harness::{NoopToolPort, PanicInferencePort};
use hkask_types::template::LLMParameters;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

fn make_executor() -> ManifestExecutor {
    ManifestExecutor::new(
        Arc::new(PanicInferencePort),
        Arc::new(NoopToolPort),
        LLMParameters::default(),
    )
}

/// A compute-only manifest mirroring the swarm-intelligence CONVERGE shape:
/// step 1 = converge_accumulate, step 2 = second_order_monitor, step 3 = loop
/// (threads the accumulators back to step 1). `min_iterations` is set above
/// `max_iterations` so `check_met` never short-circuits — the loop runs the
/// full `max_iterations` then exits via the maxed-out path (`on_not_reached:
/// "abort"` breaks and returns Ok, per the executor).
const MANIFEST: &str = r#"manifest:
  id: test-swarm-converge
  name: Test swarm converge cascade
  description: Integration test for C1/C3/C7 accumulator threading
  version: "0.1.0"
  editor: test
  visibility: Public
  category: skill

steps:
  - ordinal: 1
    action: compute
    description: "converge_accumulate"
    compute_ref: "swarm.converge_accumulate"
    gas_cap: 4096
    timeout_seconds: 30
    phase: Core
    input_mapping:
      iteration_log: "{{ iteration_log | default([]) }}"
      failed_edits: "{{ failed_edits | default([]) }}"
      influence_scores: "{{ influence_scores | default({}) }}"
      d: "{{ d | default(0.4) }}"
      task_success: "{{ task_success | default(none) }}"
      deficit_class: "{{ deficit_class | default('variety_deficit') }}"
      decisions: "{{ decisions | default({}) }}"
      swarm_state: "{{ swarm_state | default({}) }}"
  - ordinal: 2
    action: compute
    description: "second_order_monitor"
    compute_ref: "swarm.second_order_monitor"
    gas_cap: 4096
    timeout_seconds: 30
    phase: Core
    input_mapping:
      iteration_log: "{{ step_1_result.iteration_log | default([]) }}"
      loop_window: 3
  - ordinal: 3
    action: loop
    description: "Re-enter with threaded accumulators"
    input_mapping:
      loop_target: "{{ 1 }}"
      iteration_log: "{{ step_1_result.iteration_log | default([]) }}"
      failed_edits: "{{ step_1_result.failed_edits | default([]) }}"
      influence_scores: "{{ step_1_result.influence_scores | default({}) }}"
convergence:
  max_iterations: 3
  min_iterations: 10
  threshold: 0.0
  convergence_field: "convergence_metric"
  on_not_reached: "abort"
gas:
  cap: 100000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 3
  alert_threshold: 0.8
  hard_limit: true
error_handling:
  on_gas_exceeded: "abort"
  on_timeout: "retry"
  max_retries: 0
  retry_backoff_seconds: 1
  on_validation_failure: "abort"
ledger:
  emit_spans: false
  span_namespace: ""
  variety_monitoring: false
  algedonic_threshold: 100
  escalation_target: "Curator"
audit:
  enabled: false
  log_level: "info"
  include_input: false
  include_output: false
  include_gas_cost: false
  include_reg_events: false
"#;

/// Seed context so every iteration of converge_accumulate sees the same inputs
/// (constant d, same deficit+action → a reasoning loop the monitor must flag
/// once the log reaches loop_window entries).
fn seed_context() -> HashMap<String, Value> {
    let mut ctx = HashMap::new();
    ctx.insert("d".to_string(), json!(0.4));
    ctx.insert("deficit_class".to_string(), json!("variety_deficit"));
    ctx.insert(
        "decisions".to_string(),
        json!({"proposed_moves": [{"move_type": "hire", "agent_id_or_type": "researcher"}]}),
    );
    ctx.insert(
        "swarm_state".to_string(),
        json!({"workspace_roster": {"agents": [{"agent_type": "writer"}]}}),
    );
    // task_success absent → s is null (not measured) — d alone is the
    // sensor-truth risk per C3's Needs-C0 note.
    ctx
}

#[tokio::test]
async fn swarm_converge_accumulators_thread_across_loop_iterations() {
    let manifest = load_manifest_from_yaml(MANIFEST).expect("manifest parses");
    let executor = make_executor();
    let result = executor
        .execute_manifest(&manifest, seed_context())
        .await
        .expect("cascade executes (maxed-out exit returns Ok)");

    // The final step_1_result is the last iteration's accumulate output.
    let step_1 = result
        .get("step_1_result")
        .expect("step_1_result present")
        .as_object()
        .unwrap();
    let log = step_1
        .get("iteration_log")
        .and_then(|v| v.as_array())
        .expect("iteration_log present");
    // Three iterations ran → the log carries three entries. This is the
    // core threading assertion: if the loop did not bind iteration_log back
    // into context, each iteration would see an empty log and append only
    // one entry, leaving log.len() == 1.
    assert_eq!(
        log.len(),
        3,
        "iteration_log must accumulate across 3 loop iterations (threading works); got {}",
        log.len()
    );
    // Every entry is the same (deficit, action) with constant d — the
    // reasoning-loop precondition.
    for entry in log {
        assert_eq!(entry["deficit_class"], "variety_deficit");
        assert_eq!(entry["decision_action"], "hire");
        assert_eq!(entry["d"], 0.4);
    }
}

#[tokio::test]
async fn swarm_second_order_monitor_fires_on_reasoning_loop() {
    let manifest = load_manifest_from_yaml(MANIFEST).expect("manifest parses");
    let executor = make_executor();
    let result = executor
        .execute_manifest(&manifest, seed_context())
        .await
        .expect("cascade executes");

    // The final step_2_result is the last iteration's monitor output. With
    // 3 logged iterations, the same (deficit, action), and d not improving,
    // the monitor must flag a reasoning_loop and recommend diversify_action.
    let monitor = result
        .get("step_2_result")
        .expect("step_2_result present")
        .as_object()
        .unwrap();
    assert!(
        monitor
            .get("reasoning_loop")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        "monitor must flag a reasoning_loop after 3 identical iterations; monitor = {monitor:?}"
    );
    assert_eq!(
        monitor.get("recommendation").and_then(|v| v.as_str()),
        Some("diversify_action"),
        "monitor must recommend diversify_action on a reasoning loop"
    );
}

#[tokio::test]
async fn swarm_failed_edits_accumulate_when_d_does_not_improve() {
    // C3: constant d (d_delta = 0 each iteration after the first) and s null
    // (not measured) → every post-first iteration is a failed edit. Pin that
    // the failed_edits set grows across iterations (the anti-loop set DECIDE
    // rejects against).
    let manifest = load_manifest_from_yaml(MANIFEST).expect("manifest parses");
    let executor = make_executor();
    let result = executor
        .execute_manifest(&manifest, seed_context())
        .await
        .expect("cascade executes");
    let failed = result
        .get("step_1_result")
        .and_then(|v| v.get("failed_edits"))
        .and_then(|v| v.as_array())
        .expect("failed_edits present");
    // First iteration: d_delta = 0 (no prior) → recorded. Iterations 2 and 3:
    // d_delta = 0.4 - 0.4 = 0 → recorded. Three failed edits total.
    assert_eq!(
        failed.len(),
        3,
        "failed_edits must accumulate across iterations (C3); got {}",
        failed.len()
    );
    // Each entry carries the deterministic swarm_state_signature.
    let sig = failed[0]["swarm_state_signature"].as_str().unwrap_or("");
    assert!(
        sig.contains("variety_deficit"),
        "swarm_state_signature includes the deficit class; got {sig}"
    );
}

#[tokio::test]
async fn swarm_influence_scores_accumulate_per_agent_type() {
    // C7: the per-agent_type running sum. With constant d (d_delta = 0 each
    // iteration after the first), the researcher influence score stays at 0
    // (0.0 + 0 + 0). Pin that the key exists and the threading preserves the
    // map across iterations (a dropped binding would reset it each pass).
    let manifest = load_manifest_from_yaml(MANIFEST).expect("manifest parses");
    let executor = make_executor();
    let result = executor
        .execute_manifest(&manifest, seed_context())
        .await
        .expect("cascade executes");
    let influence = result
        .get("step_1_result")
        .and_then(|v| v.get("influence_scores"))
        .and_then(|v| v.as_object())
        .expect("influence_scores present");
    assert!(
        influence.contains_key("researcher"),
        "influence_scores must track the moved agent's type (C7); got {influence:?}"
    );
    let score = influence["researcher"].as_f64().unwrap_or(f64::NAN);
    assert!(
        score.is_finite(),
        "influence score must be finite after accumulation; got {score}"
    );
}

/// A single-pass manifest with just the filter step, to verify the C3/C7
/// enforcement fires through the executor (not just in the unit test).
const FILTER_MANIFEST: &str = r#"manifest:
  id: test-swarm-filter
  name: Test swarm filter enforcement
  description: Integration test for C3/C7 deterministic enforcement
  version: "0.1.0"
  editor: test
  visibility: Public
  category: skill
steps:
  - ordinal: 1
    action: compute
    description: "filter_proposed_moves"
    compute_ref: "swarm.filter_proposed_moves"
    gas_cap: 4096
    timeout_seconds: 30
    phase: Core
    input_mapping:
      proposed_moves: "{{ proposed_moves | default([]) }}"
      failed_edits: "{{ failed_edits | default([]) }}"
      influence_scores: "{{ influence_scores | default({}) }}"
      deficit_class: "{{ deficit_class | default('') }}"
      swarm_state: "{{ swarm_state | default({}) }}"
convergence:
  max_iterations: 1
  min_iterations: 10
  threshold: 0.0
  convergence_field: "convergence_metric"
  on_not_reached: "abort"
gas:
  cap: 100000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 3
  alert_threshold: 0.8
  hard_limit: true
error_handling:
  on_gas_exceeded: "abort"
  on_timeout: "retry"
  max_retries: 0
  retry_backoff_seconds: 1
  on_validation_failure: "abort"
ledger:
  emit_spans: false
  span_namespace: ""
  variety_monitoring: false
  algedonic_threshold: 100
  escalation_target: "Curator"
audit:
  enabled: false
  log_level: "info"
  include_input: false
  include_output: false
  include_gas_cost: false
  include_reg_events: false
"#;

#[tokio::test]
async fn swarm_filter_enforces_failed_edit_guard_in_executor() {
    // Seed a prior failed edit (hire under variety_deficit|writer) and a
    // proposed hire under the same signature → the filter must drop it. This
    // verifies the C3 enforcement fires through ManifestExecutor, not just the
    // dispatch_compute unit test.
    let manifest = load_manifest_from_yaml(FILTER_MANIFEST).expect("manifest parses");
    let executor = make_executor();
    let mut ctx = HashMap::new();
    ctx.insert(
        "proposed_moves".to_string(),
        json!([{"move_type": "hire", "agent_id_or_type": "x"}]),
    );
    ctx.insert(
        "failed_edits".to_string(),
        json!([{"decision_action": "hire", "swarm_state_signature": "variety_deficit|writer", "d_delta": 0.0}]),
    );
    ctx.insert("influence_scores".to_string(), json!({}));
    ctx.insert("deficit_class".to_string(), json!("variety_deficit"));
    ctx.insert(
        "swarm_state".to_string(),
        json!({"workspace_roster": {"agents": [{"agent_type": "writer"}]}}),
    );
    let result = executor
        .execute_manifest(&manifest, ctx)
        .await
        .expect("filter executes");
    let filtered = result
        .get("step_1_result")
        .and_then(|v| v.get("proposed_moves"))
        .and_then(|v| v.as_array())
        .expect("filtered proposed_moves present");
    assert!(
        filtered.is_empty(),
        "C3 enforcement: the matching hire must be dropped through the executor; got {filtered:?}"
    );
    let rejected = result
        .get("step_1_result")
        .and_then(|v| v.get("rejected"))
        .and_then(|v| v.as_array())
        .expect("rejected audit present");
    assert_eq!(
        rejected[0]["reason"], "failed_edit_guard",
        "the rejection reason must name the C3 guard"
    );
}
