//! Property tests for `StepGraph` — the validated, addressable IR over a
//! manifest's steps.
//!
//! The in-crate `step_graph::tests` module has example tests (one case per
//! behavior: addressing by StepId, `find`, looping vs single-pass, single
//! step). These property tests drive the same `StepGraph` API across a
//! generated input space to pin the invariants that hold for *all* graphs,
//! not just the hand-picked examples.
//!
//! `BundleManifestStep` is `#[non_exhaustive]`, so external crates cannot
//! construct it with a struct literal. These tests build steps via
//! `load_manifest_from_yaml` (the same path the executor uses), then construct
//! `StepGraph` from `manifest.steps` — exactly as `executor_baseline_contract`
//! does.
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant` — never-panic (catch_unwind), structural invariants.
//! - `oracle_reference` — `find`/`entry`/`loops`/`last_step_id` match a
//!   trusted reference built from the same construction.
//!
//! # Principle grounding
//! - P4 (Clear Boundaries): `StepGraph::new` is total — for any step list and
//!   any `max_iterations` it builds a graph, never panics (the over-cap case
//!   warns, does not error).
//! - P1 (Correctness): `find(ordinal)` returns `Some(id)` iff
//!   `step(id).ordinal == ordinal`; `entry() == 0`; `last_step_id == len-1`;
//!   `loops() == (max_iterations != 1)`.
//! - Determinism: the graph is immutable once built; all accessors are pure.

use hkask_templates::load_manifest_from_yaml;
use hkask_templates::step_graph::{ControlFlow, ENTRY, ExitKind, StepGraph};
use proptest::prelude::*;
use std::collections::HashMap;

// ──────────────────────────────────────────────────────────────────────────
// Input strategies
// ──────────────────────────────────────────────────────────────────────────

/// A small set of valid step actions (the graph stores the action string but
/// does not validate it — any string is accepted).
fn arb_action() -> impl Strategy<Value = &'static str> {
    prop::sample::select(&[
        "select", "execute", "compute", "gate", "abort", "escalate", "halt",
    ])
}

/// A manifest YAML with `count` steps, each with a distinct ascending ordinal
/// (1..=count) and the given actions. `max_iterations` controls looping.
fn manifest_yaml(actions: &[&str], max_iterations: u32) -> String {
    let mut yaml = String::from("manifest:\n  id: prop-test\n  category: skill\n");
    yaml.push_str(&format!(
        "convergence:\n  max_iterations: {max_iterations}\n  threshold: 0.5\n  convergence_field: convergence_signal\n  on_not_reached: abort\nsteps:\n"
    ));
    for (i, action) in actions.iter().enumerate() {
        yaml.push_str(&format!(
            "  - ordinal: {}\n    action: {}\n    description: x\n",
            i + 1,
            action
        ));
    }
    yaml
}

/// A manifest YAML with arbitrary (possibly duplicate, possibly gapped)
/// ordinals — exercises the `by_ordinal` map's collision and gap behavior.
fn manifest_yaml_arbitrary_ordinals(pairs: &[(String, u32)], max_iterations: u32) -> String {
    let mut yaml = String::from("manifest:\n  id: prop-test\n  category: skill\n");
    yaml.push_str(&format!(
        "convergence:\n  max_iterations: {max_iterations}\n  threshold: 0.5\n  convergence_field: convergence_signal\n  on_not_reached: abort\nsteps:\n"
    ));
    for (action, ordinal) in pairs {
        yaml.push_str(&format!(
            "  - ordinal: {ordinal}\n    action: {action}\n    description: x\n"
        ));
    }
    yaml
}

/// A list of actions (1..8) for the ascending-ordinal manifest strategy.
fn arb_action_list() -> impl Strategy<Value = Vec<&'static str>> {
    prop::collection::vec(arb_action(), 1..8)
}

/// A list of (action, ordinal) pairs for the arbitrary-ordinal strategy.
fn arb_arbitrary_ordinal_list() -> impl Strategy<Value = Vec<(String, u32)>> {
    prop::collection::vec((arb_action().prop_map(String::from), 1u32..20), 1..8)
}

/// A `max_iterations` value: 1 (single-pass) or >1 (looping).
fn arb_max_iterations() -> impl Strategy<Value = u32> {
    prop_oneof![Just(1u32), 2u32..50,]
}

// ──────────────────────────────────────────────────────────────────────────
// 1. StepGraph::new — totality
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `StepGraph::new` never panics on any step list and any
    /// `max_iterations` — it is total (the over-cap case warns, not errors).
    #[test]
    fn step_graph_new_never_panics(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("generated manifest must parse");
        let result = std::panic::catch_unwind(|| {
            StepGraph::new(&manifest.steps, max_iterations)
        });
        prop_assert!(result.is_ok(), "panicked on actions={:?} max={}", actions, max_iterations);
    }

    /// `StepGraph::new` on an empty step list never panics. (The graph has no
    /// steps; `last_step_id` would underflow, but construction itself is safe.)
    #[test]
    fn step_graph_new_empty_never_panics(max_iterations in arb_max_iterations()) {
        let yaml = "manifest:\n  id: empty\n  category: skill\nconvergence:\n  max_iterations: 1\n  threshold: 0.5\n  convergence_field: convergence_signal\n  on_not_reached: abort\nsteps: []\n";
        let manifest = load_manifest_from_yaml(yaml).expect("empty manifest must parse");
        let result = std::panic::catch_unwind(|| {
            StepGraph::new(&manifest.steps, max_iterations)
        });
        prop_assert!(result.is_ok(), "panicked on empty steps max={}", max_iterations);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 2. Structural invariants — entry, len, last_step_id, loops
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `entry()` is always `ENTRY` (0), and `len()` equals the input step
    /// count, for any graph.
    #[test]
    fn step_graph_entry_and_len_match_construction(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        prop_assert_eq!(graph.entry(), ENTRY, "entry != ENTRY");
        prop_assert_eq!(graph.len(), actions.len(), "len != step count");
    }

    /// `last_step_id() == len - 1` for any non-empty graph.
    #[test]
    fn step_graph_last_step_id_is_len_minus_one(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        prop_assert_eq!(
            graph.last_step_id(),
            (graph.len() - 1) as u32,
            "last_step_id != len-1"
        );
    }

    /// `loops() == (max_iterations != 1)`. This is the documented construction
    /// contract: `max_iterations: 1` is single-pass (exits after last step);
    /// any other value loops (re-enters from entry).
    #[test]
    fn step_graph_loops_iff_max_iterations_not_one(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        prop_assert_eq!(
            graph.loops(),
            max_iterations != 1,
            "loops() != (max_iterations != 1) for max={}", max_iterations
        );
    }

    /// `iter()` yields exactly `len()` nodes in StepId order, and each node's
    /// `id` matches its position in the iteration.
    #[test]
    fn step_graph_iter_yields_all_nodes_in_order(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        let collected: Vec<_> = graph.iter().collect();
        prop_assert_eq!(collected.len(), graph.len(), "iter yielded wrong count");
        for (idx, node) in collected.iter().enumerate() {
            prop_assert_eq!(node.id, idx as u32, "node id != position at idx {}", idx);
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 3. find() — ordinal-to-StepId resolution
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `find(ordinal)` returns `Some(id)` iff `step(id).ordinal == ordinal`.
    /// This is the reference oracle: the `by_ordinal` map is a bijection
    /// between present ordinals and StepIds.
    #[test]
    fn step_graph_find_is_inverse_of_step_ordinal(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        for (idx, node) in graph.iter().enumerate() {
            let found = graph.find(node.ordinal);
            prop_assert_eq!(
                found,
                Some(idx as u32),
                "find({}) != Some({})", node.ordinal, idx
            );
            // Round-trip: find(ordinal) -> id; step(id).ordinal == ordinal.
            if let Some(id) = found {
                prop_assert_eq!(
                    graph.step(id).ordinal,
                    node.ordinal,
                    "round-trip find/step failed for ordinal {}", node.ordinal
                );
            }
        }
    }

    /// `find` on an ordinal not present in any step returns `None`.
    #[test]
    fn step_graph_find_absent_ordinal_is_none(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
        absent in 1000u32..2000,
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        prop_assert_eq!(
            graph.find(absent),
            None,
            "find({}) returned Some for a graph with {} steps", absent, actions.len()
        );
    }

    /// With arbitrary (possibly duplicate) ordinals, `find(ordinal)` returns
    /// `Some(id)` for the *last* step with that ordinal. NOTE: the manifest
    /// loader sorts steps by ordinal (`manifest.steps.sort_by_key`), so the
    /// `by_ordinal` map's "last wins" collision behavior is determined by the
    /// *sorted* order. For duplicate ordinals, Rust's stable sort preserves
    /// input order among equal keys, so the last input step with a given
    /// ordinal is also the last in sorted order — and thus wins the
    /// `HashMap::insert` overwrite. The reference builds `last_by_ordinal`
    /// from the manifest's (sorted) `steps`, matching the loader's behavior.
    #[test]
    fn step_graph_find_duplicate_ordinal_resolves_to_last(
        pairs in arb_arbitrary_ordinal_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml_arbitrary_ordinals(&pairs, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        // The manifest loader sorts steps by ordinal; build the reference from
        // the sorted steps (the same list the graph was built from).
        let mut last_by_ordinal: HashMap<u32, u32> = HashMap::new();
        for (idx, step) in manifest.steps.iter().enumerate() {
            last_by_ordinal.insert(step.ordinal, idx as u32);
        }
        for (&ordinal, &expected_id) in &last_by_ordinal {
            prop_assert_eq!(
                graph.find(ordinal),
                Some(expected_id),
                "find({}) != last step id {}", ordinal, expected_id
            );
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 4. Control flow — last step on_complete matches loops()
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// The last step's `on_complete` is `Reenter(ENTRY)` iff `loops()` is true,
    /// else `Exit(ExitKind::Converged)`. This pins the static control-flow
    /// construction contract.
    #[test]
    fn step_graph_last_step_on_complete_matches_loops(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        let last_id = graph.last_step_id();
        let last = graph.step(last_id);
        if graph.loops() {
            prop_assert_eq!(
                last.on_complete,
                ControlFlow::Reenter(ENTRY),
                "looping graph last step did not Reenter"
            );
        } else {
            prop_assert_eq!(
                last.on_complete,
                ControlFlow::Exit(ExitKind::Converged),
                "single-pass graph last step did not Exit(Converged)"
            );
        }
    }

    /// Every non-last step's `on_complete` is `Fallthrough`, regardless of
    /// `loops()`. This pins the static control-flow for intermediate steps.
    #[test]
    fn step_graph_non_last_steps_fallthrough(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        let last_id = graph.last_step_id();
        for node in graph.iter() {
            if node.id != last_id {
                prop_assert_eq!(
                    node.on_complete,
                    ControlFlow::Fallthrough,
                    "non-last step {} did not Fallthrough", node.id
                );
            }
        }
    }

    /// A single-step graph: the only step is the last step, so its
    /// `on_complete` is `Reenter(ENTRY)` (looping) or `Exit(Converged)`
    /// (single-pass) — never `Fallthrough`.
    #[test]
    fn step_graph_single_step_never_fallthrough(max_iterations in arb_max_iterations()) {
        let yaml = manifest_yaml(&["abort"], max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        prop_assert_eq!(graph.len(), 1);
        let only = graph.step(0);
        prop_assert_ne!(only.on_complete, ControlFlow::Fallthrough);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// 5. step() — StepId indexing
// ──────────────────────────────────────────────────────────────────────────

proptest! {
    /// `step(id).ordinal` matches the step at position `id` in the input list
    /// (for ascending-ordinal lists, `id == ordinal - 1`). This pins the
    /// `step(StepId)` O(1) indexing contract. `action` is compared as `&str`
    /// (the node stores `Arc<str>`; deref-coerce to `&str` for the compare).
    #[test]
    fn step_graph_step_id_indexes_input_position(
        actions in arb_action_list(),
        max_iterations in arb_max_iterations(),
    ) {
        let yaml = manifest_yaml(&actions, max_iterations);
        let manifest = load_manifest_from_yaml(&yaml).expect("manifest must parse");
        let graph = StepGraph::new(&manifest.steps, max_iterations);
        for (idx, input_action) in actions.iter().enumerate() {
            let node = graph.step(idx as u32);
            // Ascending ordinals: position idx -> ordinal idx+1.
            prop_assert_eq!(
                node.ordinal, (idx + 1) as u32,
                "step({}).ordinal != input ordinal", idx
            );
            prop_assert_eq!(
                &*node.action, *input_action,
                "step({}).action != input action", idx
            );
        }
    }
}
