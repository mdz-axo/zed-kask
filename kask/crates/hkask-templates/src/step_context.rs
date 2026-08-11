//! Step context — typed replacement for the stringly-keyed `HashMap<String, Value>`.
//!
//! The old executor stored everything in one flat `HashMap<String, Value>`:
//! user inputs, step results, loop counters, convergence state, budget
//! snapshots, and taint labels — all addressed by string keys like
//! `step_3_result`, `prev_step_1_result`, `convergence_signal`, `_convergence`.
//! This caused:
//!
//! - `extract_final_step_result` scanning all keys for `step_` prefix and
//!   parsing ordinals out of strings (randomized HashMap order picked
//!   arbitrary steps).
//! - `prev_step_N_result` created by copying every step result into a
//!   parallel key on every loop iteration (30-line block, N lock acquisitions).
//! - FIDES taint tracked in a *parallel* `HashMap<String, ToolTaint>` that had
//!   to be kept in sync with the context map by hand.
//! - `propagate_taint_for_binding` grepping Jinja `{{ }}` expressions with a
//!   hand-rolled tokenizer to figure out which context keys a binding
//!   references.
//!
//! `StepContext` separates these into typed fields. Step results are keyed by
//! `StepId` (O(1) lookup, no string parsing). Taint is a field on `StepResult`,
//! not a parallel map. `prev_results` is populated by the machine on re-enter,
//! not by a block inside the loop arm. The convergence signal is read from
//! the results map directly.
//!
//! The context also carries a `legacy` map for backward compatibility with
//! templates that reference `{{ step_3_result }}` by string. The machine
//! writes to both the typed `results` map and the `legacy` string map so
//! existing Jinja templates keep working without modification.

use crate::step_graph::StepId;
use hkask_capability::tool_taint::ToolTaint;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// A step's result, with its taint label inline.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub value: Arc<Value>,
    pub taint: ToolTaint,
    pub step_id: StepId,
    pub ordinal: u32,
}

/// Typed cascade context. Replaces `HashMap<String, Value>` + the parallel
/// `taint_labels` map.
#[derive(Debug, Clone)]
pub struct StepContext {
    /// User-supplied inputs + injected model defaults. Addressed by string
    /// (template authors name these — `{{ target }}`, `{{ quality_criteria }}`).
    pub inputs: HashMap<String, Value>,

    /// Step results, keyed by `StepId`. O(1) lookup, no string parsing.
    results: HashMap<StepId, StepResult>,

    /// The previous iteration's results, for Self-Refine loops. Populated by
    /// the machine on `Reenter`, not by a block inside the loop arm.
    prev_results: HashMap<StepId, StepResult>,

    /// Legacy string-keyed view of step results, for Jinja templates that
    /// reference `{{ step_3_result }}`. Written in lockstep with `results`.
    /// Read by `input_mapping::resolve_mapping_value` and
    /// `template_renderer::render`.
    legacy: HashMap<String, Value>,

    /// Convergence signal — the scalar the loop step binds via
    /// `convergence_signal: "{{ step_N_result }}"`. Read by the convergence
    /// tracker from the legacy map (the binding resolves through Jinja).
    /// Stored here so the machine can read it without re-parsing strings.
    pub convergence_signal: Option<f64>,
    pub kata_brier: Option<f64>,
}

impl StepContext {
    /// Create a new context from user-supplied inputs.
    pub fn new(inputs: HashMap<String, Value>) -> Self {
        Self {
            inputs,
            results: HashMap::new(),
            prev_results: HashMap::new(),
            legacy: HashMap::new(),
            convergence_signal: None,
            kata_brier: None,
        }
    }

    /// Store a step result. Writes to both the typed `results` map and the
    /// `legacy` string map (as `step_{ordinal}_result`) so Jinja templates
    /// that reference results by string keep working.
    pub fn store_result(&mut self, step_id: StepId, ordinal: u32, value: Value, taint: ToolTaint) {
        let key = format!("step_{ordinal}_result");
        self.legacy.insert(key, value.clone());
        self.results.insert(
            step_id,
            StepResult {
                value: Arc::new(value),
                taint,
                step_id,
                ordinal,
            },
        );
    }

    /// Store a step result under a custom key suffix (e.g. `populated` for
    /// `step_{ordinal}_populated`). Used by `populate` and `render` actions.
    pub fn store_named(
        &mut self,
        step_id: StepId,
        ordinal: u32,
        suffix: &str,
        value: Value,
        taint: ToolTaint,
    ) {
        let key = format!("step_{ordinal}_{suffix}");
        self.legacy.insert(key, value.clone());
        // Named results also go into the typed map under the step_id, so
        // the machine can extract the final result by StepId. The `value`
        // is the rendered output; the taint is propagated from inputs.
        self.results.insert(
            step_id,
            StepResult {
                value: Arc::new(value),
                taint,
                step_id,
                ordinal,
            },
        );
    }

    /// Get a step result by `StepId`. O(1).
    pub fn result(&self, step_id: StepId) -> Option<&StepResult> {
        self.results.get(&step_id)
    }

    /// Get the taint label of a step result by `StepId`. O(1).
    pub fn taint_of(&self, step_id: StepId) -> ToolTaint {
        self.results
            .get(&step_id)
            .map(|r| r.taint)
            .unwrap_or(ToolTaint::Pure)
    }

    /// The last step result that stored a value (highest StepId with a result).
    /// O(1) — the machine tracks this, no string-key scan.
    pub fn last_result(&self, last_step_id: StepId) -> Option<&StepResult> {
        self.results.get(&last_step_id)
    }

    /// Snapshot the current iteration's results into `prev_results`. Called
    /// by the machine on `Reenter`, replacing the 30-line block in the old
    /// loop arm that copied every `step_N_result` into `prev_step_N_result`.
    pub fn snapshot_prev(&mut self) {
        // Shallow after K4: `StepResult.value` is `Arc<Value>`, so cloning the
        // results map is N refcount bumps, not N deep Value-tree clones. The
        // legacy prev-key writes below remain deep clones (the legacy map owns
        // `Value`); K1 removes them when the legacy results-mirror is deleted.
        self.prev_results = self.results.clone();
        for (_step_id, result) in &self.results {
            let prev_key = format!("prev_step_{}_result", result.ordinal);
            self.legacy.insert(prev_key, result.value.as_ref().clone());
        }
    }

    /// Get a previous iteration's result by `StepId`.
    pub fn prev_result(&self, step_id: StepId) -> Option<&StepResult> {
        self.prev_results.get(&step_id)
    }

    /// Insert a value into the legacy string-keyed map (for bindings like
    /// `convergence_signal`, `prior_probability`, etc. that aren't step
    /// results but need to be visible to Jinja templates).
    pub fn insert_legacy(&mut self, key: String, value: Value) {
        self.legacy.insert(key, value);
    }

    /// Iterate over all typed step results.
    pub fn results_iter(&self) -> impl Iterator<Item = (&StepId, &StepResult)> {
        self.results.iter()
    }

    /// Read a value from the legacy string-keyed map.
    pub fn legacy(&self, key: &str) -> Option<&Value> {
        self.legacy.get(key)
    }

    /// Get the full legacy map for Jinja template rendering. This is the
    /// `HashMap<String, Value>` that `TemplateRenderer::render` and
    /// `resolve_mapping_value` consume.
    pub fn legacy_map(&self) -> &HashMap<String, Value> {
        &self.legacy
    }

    /// Get a mutable reference to the legacy map (for the budget tracker's
    /// `inject_into_context` and the convergence tracker's
    /// `inject_running` / `finalize_report`).
    pub fn legacy_map_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.legacy
    }

    /// Merge user inputs into the legacy map so templates can reference them
    /// by name (`{{ target }}`).
    pub fn merge_inputs_into_legacy(&mut self) {
        for (key, value) in &self.inputs {
            self.legacy.insert(key.clone(), value.clone());
        }
    }

    /// Read the convergence signal from the legacy map. The loop step's
    /// `convergence_signal:` binding resolves through Jinja into the legacy
    /// map under the key `convergence_signal`.
    pub fn read_convergence_signal(&mut self) {
        if let Some(v) = self
            .legacy
            .get("convergence_signal")
            .and_then(|v| v.as_f64())
        {
            self.convergence_signal = Some(v);
        }
        if let Some(v) = self.legacy.get("kata_brier").and_then(|v| v.as_f64()) {
            self.kata_brier = Some(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_result_writes_both_typed_and_legacy() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::String("hello".into()), ToolTaint::Pure);

        assert_eq!(
            ctx.result(0).unwrap().value.as_ref(),
            &Value::String("hello".into())
        );
        assert_eq!(
            ctx.legacy("step_1_result").unwrap(),
            &Value::String("hello".into())
        );
    }

    #[test]
    fn taint_is_inline_on_result() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::Null, ToolTaint::Source);

        assert_eq!(ctx.taint_of(0), ToolTaint::Source);
    }

    #[test]
    fn snapshot_prev_copies_results_and_writes_legacy_prev_keys() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::String("first".into()), ToolTaint::Pure);
        ctx.store_result(1, 2, Value::String("second".into()), ToolTaint::Source);

        ctx.snapshot_prev();

        assert_eq!(
            ctx.prev_result(0).unwrap().value.as_ref(),
            &Value::String("first".into())
        );
        assert_eq!(
            ctx.prev_result(1).unwrap().value.as_ref(),
            &Value::String("second".into())
        );
        assert_eq!(
            ctx.legacy("prev_step_1_result").unwrap(),
            &Value::String("first".into())
        );
        assert_eq!(
            ctx.legacy("prev_step_2_result").unwrap(),
            &Value::String("second".into())
        );
    }

    #[test]
    fn last_result_is_o1_by_step_id() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::String("a".into()), ToolTaint::Pure);
        ctx.store_result(1, 3, Value::String("b".into()), ToolTaint::Pure);
        ctx.store_result(2, 7, Value::String("c".into()), ToolTaint::Pure);

        let last = ctx.last_result(2).unwrap();
        assert_eq!(last.value.as_ref(), &Value::String("c".into()));
        assert_eq!(last.ordinal, 7);
    }
}
