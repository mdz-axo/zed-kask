//! Step context — the single typed source of truth for cascade state.
//!
//! (K1) Replaced the denormalized `legacy: HashMap<String, Value>` mirror with
//! typed fields + a small `by_ordinal` index. Templates resolve
//! `{{ step_3_result }}` / `{{ prev_step_1_result }}` / `{{ target }}` through
//! `lookup` (O(1) via the ordinal index) or through the custom `Serialize`
//! impl that minijinja walks directly — no per-render materialization, and no
//! clone-on-write to keep a parallel string-keyed mirror in sync.
//! `store_result`/`snapshot_prev`/merge no longer deep-clone `Value`s into a
//! second map. Protocol keys (`_gas`, `_rjoule`, `_convergence`,
//! `convergence_signal`, `kata_brier`, `input_mapping`-injected bindings) live
//! in `protocol`; step results live once in `results`; user inputs live once
//! in `inputs`.
//!
//! The `ContextLookup`/`ContextMap` traits let convergence/condition/budget
//! stay generic over the backing store: `HashMap<String, Value>` (tests) and
//! `StepContext` (the executor). Two impls, both live (tests use the flat map,
//! the executor uses the typed context) — not the one-impl trap. Naming the
//! trait methods `get`/`insert` keeps call-site bodies unchanged.

use crate::step_graph::StepId;
use hkask_capability::tool_taint::ToolTaint;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ── Context map interface ──────────────────────────────────────────────────
//
// A shared read/write interface so readers (convergence, condition,
// input_mapping, budget) are generic over the backing store: `HashMap<String,
// Value>` (tests, the materialized return map) and `StepContext` (the
// executor's typed context). The trait methods are named `get`/`insert` so
// call-site bodies are unchanged — for a generic `C: ContextLookup`,
// `context.get(k)` resolves to the trait method. Two impls at birth, both
// live (tests use the flat map, the executor uses the typed context), so this
// is not the one-impl speculative-generality trap.

/// Read-only string-key lookup over a context map.
pub trait ContextLookup {
    fn get(&self, key: &str) -> Option<&Value>;

    /// Resolve a string key to its taint label. Default: no taint metadata
    /// available (flat maps don't carry taint); `StepContext` overrides to
    /// read `StepResult.taint` for `step_N_result` / `prev_step_N_result` keys.
    /// Used by `check_untrusted_input` for the FIDES Source→Sink guard.
    fn taint_of_key(&self, _key: &str) -> ToolTaint {
        ToolTaint::Pure
    }
}

/// Mutable string-key map: `ContextLookup` plus `insert`. Writers (convergence
/// `inject_running`/`finalize_report`, budget `inject_into_context`) are
/// generic over this; the `StepContext` impl routes inserts to its `protocol`
/// map, the `HashMap` impl moves.
pub trait ContextMap: ContextLookup {
    fn insert(&mut self, key: String, value: Value);
}

impl ContextLookup for HashMap<String, Value> {
    fn get(&self, key: &str) -> Option<&Value> {
        self.get(key)
    }
}

impl ContextMap for HashMap<String, Value> {
    fn insert(&mut self, key: String, value: Value) {
        self.insert(key, value);
    }
}

/// A step's result, with its taint label inline.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub value: Arc<Value>,
    pub taint: ToolTaint,
    pub step_id: StepId,
    pub ordinal: u32,
}

/// Typed cascade context — the single source of truth. Step results live once
/// (keyed by `StepId`, indexed by ordinal); user inputs once; protocol keys
/// once. No denormalized string-key mirror.
#[derive(Debug, Clone)]
pub struct StepContext {
    /// User-supplied inputs + injected model defaults. Addressed by string
    /// (template authors name these — `{{ target }}`, `{{ quality_criteria }}`).
    pub inputs: HashMap<String, Value>,

    /// Step results, keyed by `StepId`. O(1) lookup, no string parsing.
    results: HashMap<StepId, StepResult>,

    /// Ordinal → `StepId` index, for resolving `{{ step_N_result }}` without
    /// scanning. Built incrementally by `store_result`/`store_named`.
    by_ordinal: HashMap<u32, StepId>,

    /// The previous iteration's results, for Self-Refine loops. Populated by
    /// the machine on `Reenter` (shallow after K4: `Arc<Value>` refcount bumps).
    prev_results: HashMap<StepId, StepResult>,

    /// Ordinal → `StepId` index for `{{ prev_step_N_result }}`.
    prev_by_ordinal: HashMap<u32, StepId>,

    /// Named (non-`_result`) step outputs — `step_{ordinal}_populated` etc.
    /// from `populate`/`render` actions. Templates resolve these via `lookup`.
    named: HashMap<String, Arc<Value>>,

    /// Protocol keys: `_gas`, `_rjoule`, `_convergence`, `convergence_signal`,
    /// `kata_brier`, and `input_mapping`-injected bindings. This is the
    /// manifest-author binding protocol, NOT a results duplication.
    protocol: HashMap<String, Value>,

    /// Cached scalar readings of the convergence signal / Brier (set by
    /// `read_convergence_signal` from `protocol`).
    pub convergence_signal: Option<f64>,
    pub kata_brier: Option<f64>,
}

impl StepContext {
    /// Create a new context from user-supplied inputs.
    pub fn new(inputs: HashMap<String, Value>) -> Self {
        Self {
            inputs,
            results: HashMap::new(),
            by_ordinal: HashMap::new(),
            prev_results: HashMap::new(),
            prev_by_ordinal: HashMap::new(),
            named: HashMap::new(),
            protocol: HashMap::new(),
            convergence_signal: None,
            kata_brier: None,
        }
    }

    /// Store a step result under `step_{ordinal}_result`. No mirror to keep in
    /// sync (K1) — the `by_ordinal` index makes `{{ step_N_result }}` resolve
    /// O(1) without a parallel string-keyed map.
    pub fn store_result(&mut self, step_id: StepId, ordinal: u32, value: Value, taint: ToolTaint) {
        self.by_ordinal.insert(ordinal, step_id);
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
    /// The value is also reachable as `step_{ordinal}_result` via the typed
    /// `results` map (same `step_id`), so `extract_final_step_result`'s
    /// max-ordinal selection still works.
    pub fn store_named(
        &mut self,
        step_id: StepId,
        ordinal: u32,
        suffix: &str,
        value: Value,
        taint: ToolTaint,
    ) {
        let key = format!("step_{ordinal}_{suffix}");
        let arc = Arc::new(value);
        self.named.insert(key, arc.clone());
        self.by_ordinal.insert(ordinal, step_id);
        self.results.insert(
            step_id,
            StepResult {
                value: arc,
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
    /// O(1) — the machine tracks `last_result_step`, no string-key scan.
    pub fn last_result(&self, last_step_id: StepId) -> Option<&StepResult> {
        self.results.get(&last_step_id)
    }

    /// Snapshot the current iteration's results into `prev_results` for
    /// Self-Refine loops. Called by the machine on `Reenter`. Shallow after
    /// K4 (Arc refcount bumps) — and after K1 there are no parallel
    /// `prev_step_N_result` legacy writes either.
    pub fn snapshot_prev(&mut self) {
        self.prev_results = self.results.clone();
        self.prev_by_ordinal = self.by_ordinal.clone();
    }

    /// Get a previous iteration's result by `StepId`.
    pub fn prev_result(&self, step_id: StepId) -> Option<&StepResult> {
        self.prev_results.get(&step_id)
    }

    /// Insert a protocol-key binding (`convergence_signal`, `input_mapping`-
    /// injected keys, etc.). NOT for step results — use `store_result`/
    /// `store_named`.
    pub fn insert_protocol(&mut self, key: String, value: Value) {
        self.protocol.insert(key, value);
    }

    /// Iterate over all typed step results.
    pub fn results_iter(&self) -> impl Iterator<Item = (&StepId, &StepResult)> {
        self.results.iter()
    }

    /// Read a protocol key directly (for callers that know the key is
    /// protocol, not a result).
    pub fn protocol(&self, key: &str) -> Option<&Value> {
        self.protocol.get(key)
    }

    /// Read-only access to the protocol map (e.g. for FlowDef's parent-key
    /// snapshot before a sub-cascade).
    pub fn protocol_map(&self) -> &HashMap<String, Value> {
        &self.protocol
    }

    /// Read-only access to the named-results map.
    pub fn named_map(&self) -> &HashMap<String, Arc<Value>> {
        &self.named
    }

    /// String-key lookup over the whole context: `step_N_result` and
    /// `prev_step_N_result` resolve O(1) via the ordinal index; named results,
    /// inputs, and protocol keys are direct map lookups. This is what
    /// templates (via `Serialize`), `resolve_mapping_value`, and conditions
    /// see as `{{ step_3_result }}` / `{{ target }}` / `{{ convergence_signal }}`.
    pub fn lookup(&self, key: &str) -> Option<&Value> {
        if let Some(rest) = key.strip_prefix("prev_step_")
            && let Some(rest) = rest.strip_suffix("_result")
            && let Ok(ordinal) = rest.parse::<u32>()
        {
            return self
                .prev_by_ordinal
                .get(&ordinal)
                .and_then(|id| self.prev_results.get(id))
                .map(|r| r.value.as_ref());
        }
        if let Some(rest) = key.strip_prefix("step_")
            && let Some(rest) = rest.strip_suffix("_result")
            && let Ok(ordinal) = rest.parse::<u32>()
        {
            return self
                .by_ordinal
                .get(&ordinal)
                .and_then(|id| self.results.get(id))
                .map(|r| r.value.as_ref());
        }
        if let Some(v) = self.named.get(key) {
            return Some(v);
        }
        if let Some(v) = self.inputs.get(key) {
            return Some(v);
        }
        self.protocol.get(key)
    }

    /// Iterate all `(string_key, &Value)` pairs the context exposes — for
    /// `render_inline`'s simple `{{key}}` substitution. Order: results,
    /// prev_results, named, inputs, protocol. No `Value` clones.
    pub fn entries<'a>(&'a self) -> impl Iterator<Item = (String, &'a Value)> + 'a {
        self.results
            .values()
            .map(|r| (format!("step_{}_result", r.ordinal), r.value.as_ref()))
            .chain(
                self.prev_results
                    .values()
                    .map(|r| (format!("prev_step_{}_result", r.ordinal), r.value.as_ref())),
            )
            .chain(self.named.iter().map(|(k, v)| (k.clone(), v.as_ref())))
            .chain(self.inputs.iter().map(|(k, v)| (k.clone(), v)))
            .chain(self.protocol.iter().map(|(k, v)| (k.clone(), v)))
    }

    /// Materialize a flat `HashMap<String, Value>` for the bridge (until K5
    /// returns a typed `CascadeOutcome`). One deep clone per manifest, NOT
    /// per iteration — `extract_final_step_result` reads `step_N_result` keys
    /// from this.
    pub fn materialize(&self) -> HashMap<String, Value> {
        let mut out = HashMap::new();
        for r in self.results.values() {
            out.insert(
                format!("step_{}_result", r.ordinal),
                r.value.as_ref().clone(),
            );
        }
        for r in self.prev_results.values() {
            out.insert(
                format!("prev_step_{}_result", r.ordinal),
                r.value.as_ref().clone(),
            );
        }
        for (k, v) in &self.named {
            out.insert(k.clone(), v.as_ref().clone());
        }
        for (k, v) in &self.inputs {
            out.insert(k.clone(), v.clone());
        }
        for (k, v) in &self.protocol {
            out.insert(k.clone(), v.clone());
        }
        out
    }

    /// Merge a sub-cascade's (FlowDef) updates back into the parent. The
    /// sub-cascade ran on a clone of the parent; only keys that existed in the
    /// parent before the sub-cascade are kept (sub-only keys are dropped). The
    /// parent-key sets are computed by the caller from the pre-sub-cascade
    /// context (so they reflect the parent, not the sub).
    pub fn merge_back_sub_cascade(
        &mut self,
        sub: &StepContext,
        parent_step_ids: &HashSet<StepId>,
        parent_protocol_keys: &[String],
        parent_named_keys: &[String],
    ) {
        for (step_id, result) in sub.results_iter() {
            if parent_step_ids.contains(step_id) {
                self.store_result(
                    *step_id,
                    result.ordinal,
                    result.value.as_ref().clone(),
                    result.taint,
                );
            }
        }
        for key in parent_protocol_keys {
            if let Some(v) = sub.protocol.get(key) {
                self.protocol.insert(key.clone(), v.clone());
            }
        }
        for key in parent_named_keys {
            if let Some(v) = sub.named.get(key) {
                self.named.insert(key.clone(), v.clone());
            }
        }
    }

    /// Read the convergence signal and Brier from `protocol` (the loop step's
    /// `convergence_signal:` binding lands there via `apply_input_mapping` →
    /// `insert_protocol`).
    pub fn read_convergence_signal(&mut self) {
        if let Some(v) = self
            .protocol
            .get("convergence_signal")
            .and_then(|v| v.as_f64())
        {
            self.convergence_signal = Some(v);
        }
        if let Some(v) = self.protocol.get("kata_brier").and_then(|v| v.as_f64()) {
            self.kata_brier = Some(v);
        }
    }
}

// `StepContext` IS the context the renderer serializes (via the custom
// `Serialize` below) and the readers look up (via `ContextLookup`).
impl ContextLookup for StepContext {
    fn get(&self, key: &str) -> Option<&Value> {
        self.lookup(key)
    }

    fn taint_of_key(&self, key: &str) -> ToolTaint {
        // Resolve `step_N_result` → ordinal → StepId → StepResult.taint.
        if let Some(rest) = key.strip_prefix("step_")
            && let Some(rest) = rest.strip_suffix("_result")
            && let Ok(ordinal) = rest.parse::<u32>()
        {
            if let Some(step_id) = self.by_ordinal.get(&ordinal)
                && let Some(result) = self.results.get(step_id)
            {
                return result.taint;
            }
        }
        // Resolve `prev_step_N_result` → prev ordinal → StepId → prev taint.
        if let Some(rest) = key.strip_prefix("prev_step_")
            && let Some(rest) = rest.strip_suffix("_result")
            && let Ok(ordinal) = rest.parse::<u32>()
        {
            if let Some(step_id) = self.prev_by_ordinal.get(&ordinal)
                && let Some(result) = self.prev_results.get(step_id)
            {
                return result.taint;
            }
        }
        ToolTaint::Pure
    }
}

impl ContextMap for StepContext {
    fn insert(&mut self, key: String, value: Value) {
        self.insert_protocol(key, value);
    }
}

/// Serialize as a flat string-keyed map — the shape minijinja expects, so
/// `{{ step_3_result }}` / `{{ prev_step_1_result }}` / `{{ target }}` /
/// `{{ convergence_signal }}` resolve. minijinja's `Value::from_serialize`
/// walks this; no per-render materialization of a `HashMap`, and no
/// `Value` clones in the walk (entries are emitted by reference).
impl Serialize for StepContext {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for r in self.results.values() {
            map.serialize_entry(&format!("step_{}_result", r.ordinal), &*r.value)?;
        }
        for r in self.prev_results.values() {
            map.serialize_entry(&format!("prev_step_{}_result", r.ordinal), &*r.value)?;
        }
        for (k, v) in &self.named {
            map.serialize_entry(k, &**v)?;
        }
        for (k, v) in &self.inputs {
            map.serialize_entry(k, v)?;
        }
        for (k, v) in &self.protocol {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_result_resolves_by_ordinal_via_lookup() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::String("hello".into()), ToolTaint::Pure);

        assert_eq!(
            ctx.result(0).unwrap().value.as_ref(),
            &Value::String("hello".into())
        );
        // `{{ step_1_result }}` resolves O(1) via the by_ordinal index — no
        // parallel string-keyed map.
        assert_eq!(
            ctx.lookup("step_1_result").unwrap(),
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
    fn snapshot_prev_resolves_via_prev_lookup_no_legacy_writes() {
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
        // `{{ prev_step_1_result }}` resolves via prev_by_ordinal — no legacy
        // prev-key writes (the old N deep clones per loop iteration are gone).
        assert_eq!(
            ctx.lookup("prev_step_1_result").unwrap(),
            &Value::String("first".into())
        );
        assert_eq!(
            ctx.lookup("prev_step_2_result").unwrap(),
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

    #[test]
    fn lookup_resolves_inputs_and_protocol() {
        let mut inputs = HashMap::new();
        inputs.insert("target".into(), Value::String("widget".into()));
        let mut ctx = StepContext::new(inputs);
        ctx.insert_protocol("convergence_signal".into(), serde_json::json!(0.42));

        assert_eq!(
            ctx.lookup("target").unwrap(),
            &Value::String("widget".into())
        );
        assert_eq!(
            ctx.lookup("convergence_signal").unwrap(),
            &serde_json::json!(0.42)
        );
        // Unknown key resolves to None.
        assert!(ctx.lookup("nope").is_none());
    }

    #[test]
    fn serialize_emits_flat_map_for_minijinja() {
        let mut inputs = HashMap::new();
        inputs.insert("target".into(), Value::String("x".into()));
        let mut ctx = StepContext::new(inputs);
        ctx.store_result(
            0,
            1,
            Value::Number(serde_json::Number::from(6)),
            ToolTaint::Pure,
        );
        ctx.insert_protocol("convergence_signal".into(), serde_json::json!(0.1));

        let serialized = serde_json::to_value(&ctx).expect("StepContext serializes");
        let obj = serialized.as_object().expect("serializes to a map");
        // Results, inputs, and protocol keys are all present (the shape
        // minijinja sees as `{{ step_1_result }}` / `{{ target }}` /
        // `{{ convergence_signal }}`).
        assert_eq!(obj["step_1_result"], serde_json::json!(6));
        assert_eq!(obj["target"], Value::String("x".into()));
        assert_eq!(obj["convergence_signal"], serde_json::json!(0.1));
    }

    // ── Follow-up #2: taint_of_key (FIDES Source→Sink guard) ──────────────

    #[test]
    fn taint_of_key_resolves_step_result_taint() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::Null, ToolTaint::Pure);
        ctx.store_result(1, 2, Value::Null, ToolTaint::Source);

        // `step_1_result` is Pure, `step_2_result` is Source.
        assert_eq!(
            ContextLookup::taint_of_key(&ctx, "step_1_result"),
            ToolTaint::Pure
        );
        assert_eq!(
            ContextLookup::taint_of_key(&ctx, "step_2_result"),
            ToolTaint::Source
        );
        // Unknown keys default to Pure (no false-positive taint).
        assert_eq!(
            ContextLookup::taint_of_key(&ctx, "step_99_result"),
            ToolTaint::Pure
        );
        // Non-result keys default to Pure.
        assert_eq!(ContextLookup::taint_of_key(&ctx, "target"), ToolTaint::Pure);
    }

    #[test]
    fn taint_of_key_resolves_prev_step_result_taint() {
        let mut ctx = StepContext::new(HashMap::new());
        ctx.store_result(0, 1, Value::Null, ToolTaint::Source);
        ctx.snapshot_prev();
        // Overwrite step 1 with Pure in the current iteration.
        ctx.store_result(0, 1, Value::Null, ToolTaint::Pure);

        // `step_1_result` is Pure (current), `prev_step_1_result` is Source.
        assert_eq!(
            ContextLookup::taint_of_key(&ctx, "step_1_result"),
            ToolTaint::Pure
        );
        assert_eq!(
            ContextLookup::taint_of_key(&ctx, "prev_step_1_result"),
            ToolTaint::Source
        );
    }

    // ── Follow-up #1: merge_back_sub_cascade (FlowDef integration) ────────

    #[test]
    fn merge_back_sub_cascade_keeps_only_parent_keys() {
        // Parent has results at StepIds 0, 1, 2 (ordinals 1, 2, 3).
        let mut parent = StepContext::new(HashMap::new());
        parent.store_result(0, 1, Value::from(10), ToolTaint::Pure);
        parent.store_result(1, 2, Value::from(20), ToolTaint::Pure);
        parent.store_result(2, 3, Value::from(30), ToolTaint::Pure);
        parent.insert_protocol("target".into(), Value::String("x".into()));

        // Sub-cascade is a clone of parent, then step 1 is updated and a
        // sub-only step 3 (StepId 3) is added.
        let mut sub = parent.clone();
        sub.store_result(1, 2, Value::from(99), ToolTaint::Pure); // update
        sub.store_result(3, 4, Value::from(40), ToolTaint::Pure); // sub-only
        sub.insert_protocol("sub_only".into(), Value::from(7)); // sub-only

        let parent_step_ids: HashSet<StepId> = parent.results_iter().map(|(id, _)| *id).collect();
        let parent_protocol_keys: Vec<String> = parent.protocol_map().keys().cloned().collect();
        let parent_named_keys: Vec<String> = Vec::new();

        parent.merge_back_sub_cascade(
            &sub,
            &parent_step_ids,
            &parent_protocol_keys,
            &parent_named_keys,
        );

        // Step 1 updated from sub.
        assert_eq!(parent.result(1).unwrap().value.as_ref(), &Value::from(99));
        // Sub-only StepId 3 is NOT merged.
        assert!(parent.result(3).is_none());
        // Parent's original step 0 and 2 are preserved.
        assert_eq!(parent.result(0).unwrap().value.as_ref(), &Value::from(10));
        assert_eq!(parent.result(2).unwrap().value.as_ref(), &Value::from(30));
        // Sub-only protocol key is NOT merged.
        assert!(parent.protocol("sub_only").is_none());
        // Parent protocol key is preserved.
        assert_eq!(
            parent.protocol("target").unwrap(),
            &Value::String("x".into())
        );
    }

    #[test]
    fn merge_back_sub_cascade_step_id_collision_overwrites_parent() {
        // Documents the known StepId collision limitation: when the parent
        // has gapped ordinals and the sub-cascade has different ordinals,
        // StepIds (vector indices) can collide across manifests even though
        // the ordinals differ. The sub's result overwrites the parent's at
        // the same StepId.
        //
        // Parent: ordinals [1, 3, 5] → StepIds [0, 1, 2].
        let mut parent = StepContext::new(HashMap::new());
        parent.store_result(0, 1, Value::from("parent-ord-1"), ToolTaint::Pure);
        parent.store_result(1, 3, Value::from("parent-ord-3"), ToolTaint::Pure);
        parent.store_result(2, 5, Value::from("parent-ord-5"), ToolTaint::Pure);

        // Sub-cascade starts as a clone of parent, then overwrites StepId 1
        // (which was ordinal 3 in the parent) with its own ordinal 2 result.
        let mut sub = parent.clone();
        sub.store_result(1, 2, Value::from("sub-ord-2"), ToolTaint::Source);

        let parent_step_ids: HashSet<StepId> = parent.results_iter().map(|(id, _)| *id).collect();

        parent.merge_back_sub_cascade(&sub, &parent_step_ids, &[], &[]);

        // StepId 1 is now the sub's ordinal-2 result — the parent's
        // ordinal-3 result is lost. This is the known limitation from
        // bug-hunt finding #1: StepId is a vector index, not a globally
        // unique identifier, so two manifests with different ordinal sets
        // can have StepId collisions.
        assert_eq!(
            parent.result(1).unwrap().value.as_ref(),
            &Value::from("sub-ord-2")
        );
        assert_eq!(parent.result(1).unwrap().ordinal, 2);
        assert_eq!(parent.result(1).unwrap().taint, ToolTaint::Source);
        // StepIds 0 and 2 are unaffected (sub didn't touch them).
        assert_eq!(
            parent.result(0).unwrap().value.as_ref(),
            &Value::from("parent-ord-1")
        );
        assert_eq!(
            parent.result(2).unwrap().value.as_ref(),
            &Value::from("parent-ord-5")
        );
    }
}
