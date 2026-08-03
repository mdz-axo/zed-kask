//! Integration test: `lisp.eval` compute primitive through the full executor.
//!
//! Verifies that a manifest with a `compute` step using `compute_ref: "lisp.eval"`
//! executes correctly through `ManifestExecutor::execute_manifest`, not just
//! through the `dispatch_compute` function directly. This catches wiring issues
//! (input_mapping resolution, step result storage, context propagation) that
//! the unit tests in `executor.rs` don't cover.

mod common;

use common::NoopToolPort;
use hkask_templates::executor::ManifestExecutor;
use hkask_templates::load_manifest_from_yaml;
use hkask_types::template::LLMParameters;
use hkask_types::{ChatToolDefinition, InferenceError, InferencePort, InferenceResult};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

struct NoopInference;

impl InferencePort for NoopInference {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async {
            Err(InferenceError::Generation(
                "inference should not be called for compute steps".into(),
            ))
        })
    }
}

fn make_executor() -> ManifestExecutor {
    ManifestExecutor::new(
        Arc::new(NoopInference),
        Arc::new(NoopToolPort),
        LLMParameters::default(),
    )
}

/// Build a minimal FlowDef manifest YAML with one `lisp.eval` compute step.
fn make_manifest_yaml(form: &str, env: Option<&str>) -> String {
    let env_block = match env {
        Some(e) => format!("      env:\n        {}\n", e),
        None => String::new(),
    };
    format!(
        r#"manifest:
  id: test-lisp-eval
  name: Test lisp.eval
  description: Integration test
  version: "0.1.0"
  editor: test
  visibility: Public
  category: skill

steps:
  - ordinal: 1
    action: compute
    description: "lisp.eval test step"
    compute_ref: "lisp.eval"
    gas_cap: 1000
    timeout_seconds: 30
    phase: Core
    input_mapping:
      form: {form}
{env_block}
convergence:
  max_iterations: 1
  threshold: 0.0
  convergence_field: "convergence_metric"
  on_not_reached: "abort"
gas:
  cap: 10000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 0
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
"#
    )
}

#[tokio::test]
async fn lisp_eval_executes_through_executor() {
    let yaml = make_manifest_yaml("\"(+ 1 2 3)\"", None);
    let manifest = load_manifest_from_yaml(&yaml).unwrap();
    let executor = make_executor();
    let result = executor
        .execute_manifest(&manifest, HashMap::new())
        .await
        .unwrap();

    let step_result = result.get("step_1_result").unwrap();
    assert_eq!(*step_result, json!(6));
}

#[tokio::test]
async fn lisp_eval_with_env_from_context() {
    // The env field uses $ref to pull a value directly from the context map.
    // This avoids Jinja string rendering — the value stays a proper JSON object.
    let yaml = make_manifest_yaml(
        "'(assoc \"score\" step_1_result)'",
        Some("step_1_result: { $ref: step_1_result }"),
    );
    let manifest = load_manifest_from_yaml(&yaml).unwrap();
    let executor = make_executor();
    let mut initial_context: HashMap<String, Value> = HashMap::new();
    initial_context.insert(
        "step_1_result".to_string(),
        json!({"score": 0.85, "findings": ["a", "b"]}),
    );

    let result = executor
        .execute_manifest(&manifest, initial_context)
        .await
        .unwrap();

    let step_result = result.get("step_1_result").unwrap();
    assert_eq!(*step_result, json!(0.85));
}

#[tokio::test]
async fn lisp_eval_capability_predicate() {
    // Use a YAML literal block scalar for the form to avoid quoting issues
    // with the nested double-quotes in the Lisp source.
    let yaml = r#"manifest:
  id: test-lisp-eval
  name: Test lisp.eval
  description: Integration test
  version: "0.1.0"
  editor: test
  visibility: Public
  category: skill

steps:
  - ordinal: 1
    action: compute
    description: "lisp.eval recursive capability predicate"
    compute_ref: "lisp.eval"
    gas_cap: 1000
    timeout_seconds: 30
    phase: Core
    input_mapping:
      form: |-
        (begin (define check-cap (lambda (cap) (and (>= (assoc "measured" cap) (assoc "floor" cap)) (<= (assoc "measured" cap) (assoc "ceiling" cap))))) (define check-all (lambda (caps) (if (is_null caps) (list) (cons (check-cap (car caps)) (check-all (cdr caps)))))) (check-all capabilities))
      env:
        capabilities: { $ref: capabilities }
convergence:
  max_iterations: 1
  threshold: 0.0
  convergence_field: "convergence_metric"
  on_not_reached: "abort"
gas:
  cap: 10000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true
rjoule:
  cap: 0
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
    let manifest = load_manifest_from_yaml(yaml).unwrap();
    let executor = make_executor();
    let mut initial_context: HashMap<String, Value> = HashMap::new();
    initial_context.insert(
        "capabilities".to_string(),
        json!([
            {"name": "tool-use", "floor": 0.5, "measured": 0.7, "ceiling": 0.9},
            {"name": "reasoning", "floor": 0.6, "measured": 0.4, "ceiling": 0.95}
        ]),
    );

    let result = executor
        .execute_manifest(&manifest, initial_context)
        .await
        .unwrap();

    let step_result = result.get("step_1_result").unwrap();
    assert_eq!(*step_result, json!([true, false]));
}

#[tokio::test]
async fn lisp_eval_error_propagates() {
    let yaml = make_manifest_yaml("\"(/ 10 0)\"", None);
    let manifest = load_manifest_from_yaml(&yaml).unwrap();
    let executor = make_executor();
    let result = executor.execute_manifest(&manifest, HashMap::new()).await;
    assert!(result.is_err(), "division by zero should error");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("lisp.eval"),
        "error should mention lisp.eval: {err_msg}"
    );
}
