//! Step graph — a validated, addressable IR over a manifest's steps.
//!
//! Replaces the `Vec<BundleManifestStep>` + `step_idx: usize` + O(N)
//! `position(ordinal)` pattern from the old executor. Steps are addressed by
//! `StepId` (a `u32` index into the graph's `steps` vector), not by the
//! user-facing `ordinal`. The `ordinal` is retained on each `StepNode` for
//! error messages and context-key naming (`step_{ordinal}_result`), but the
//! machine never scans for it.
//!
//! Control flow is a property of the node (`on_complete`), not something the
//! interpreter reconstructs from `match` arms. `choice` and `branching` both
//! become `ControlFlow::Jump(StepId)`; `loop` becomes `ControlFlow::Reenter`;
//! the implicit end-of-pass re-entry becomes an explicit `Reenter(entry)`
//! edge on the last step. There is exactly one way to jump.

use crate::bundle::manifest::BundleManifestStep;
use serde_json::Value;
use std::collections::HashMap;

/// Index into a `StepGraph`'s `steps` vector. Stable for the lifetime of the
/// graph (graphs are immutable once built).
pub type StepId = u32;

/// The entry point of every cascade — always step 0.
pub const ENTRY: StepId = 0;

/// How a step hands control to the next step.
///
/// This is the *static* control flow declared by the step's position and
/// action type. The *dynamic* control flow (a `choice` that jumps, a
/// `branching` map that routes, a `loop` that re-enters) is produced by the
/// step's `StepAction::execute` returning an `Effect` that the machine merges
/// with this static flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    /// Continue to the next step (by StepId order). Same iteration.
    Fallthrough,
    /// Jump to a specific step. Same iteration (choice/branching).
    Jump(StepId),
    /// Re-enter the cascade from `target`. New iteration (loop). The machine
    /// runs the convergence check and budget check before re-entering.
    Reenter(StepId),
    /// Exit the cascade. The machine stops.
    Exit(ExitKind),
}

/// Why the cascade exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    /// `abort` action or convergence threshold met — success.
    Converged,
    /// `max_iterations` exhausted or gas/rJoule budget spent.
    MaxedOut,
    /// `escalate` action or `on_not_reached: escalate` — blocked.
    Escalated,
}

/// A validated step node in the graph. The `action` field carries the
/// step's typed configuration; `on_complete` declares the static control
/// flow. Everything the old `run_cascade` reconstructed from `match` arms is
/// now data on the node.
#[derive(Debug, Clone)]
pub struct StepNode {
    pub id: StepId,
    /// User-facing ordinal, for error messages and `step_{ordinal}_result`
    /// context-key naming. NOT used for addressing.
    pub ordinal: u32,
    pub action: String,
    pub description: String,
    pub renderer: Option<String>,
    pub template_ref: Option<String>,
    pub mcp: Option<String>,
    pub compute_ref: Option<String>,
    pub input_mapping: Option<Value>,
    pub output_schema: Option<Value>,
    pub condition: Option<String>,
    pub branching: Option<HashMap<String, u32>>,
    pub branching_field: Option<String>,
    pub profile: Option<String>,
    pub gas_cap: u32,
    pub timeout_seconds: u32,
    pub phase: String,
    /// Static control flow after this step completes. For most steps this is
    /// `Fallthrough`; the last step is `Reenter(ENTRY)` (implicit loop) unless
    /// the manifest declares `max_iterations: 1` (single-pass → `Exit`).
    pub on_complete: ControlFlow,
}

/// A validated, addressable step graph. Built once from a `BundleManifest`;
/// immutable for the lifetime of the cascade.
#[derive(Debug, Clone)]
pub struct StepGraph {
    /// Steps indexed by `StepId` (== vector index).
    steps: Vec<StepNode>,
    /// Maps user-facing ordinals to `StepId` for jump resolution.
    /// Built once at construction; `find` is O(1).
    by_ordinal: HashMap<u32, StepId>,
    /// Whether the cascade loops (re-enters from entry after the last step)
    /// or runs once (exits after the last step).
    loops: bool,
}

impl StepGraph {
    /// Build a step graph from a manifest's steps and convergence config.
    ///
    /// `max_iterations` controls whether the last step re-enters the cascade
    /// (looping manifests) or exits (single-pass manifests with
    /// `max_iterations: 1`).
    pub fn new(steps: &[BundleManifestStep], max_iterations: u32) -> Self {
        let loops = max_iterations != 1;
        let mut nodes: Vec<StepNode> = Vec::with_capacity(steps.len());
        let mut by_ordinal: HashMap<u32, StepId> = HashMap::with_capacity(steps.len());

        for (idx, step) in steps.iter().enumerate() {
            let id = idx as StepId;
            by_ordinal.insert(step.ordinal, id);

            // Determine static control flow for this step.
            let is_last = idx == steps.len() - 1;
            let on_complete = if is_last {
                if loops {
                    ControlFlow::Reenter(ENTRY)
                } else {
                    ControlFlow::Exit(ExitKind::Converged)
                }
            } else {
                ControlFlow::Fallthrough
            };

            nodes.push(StepNode {
                id,
                ordinal: step.ordinal,
                action: step.action.clone(),
                description: step.description.clone(),
                renderer: step.renderer.clone(),
                template_ref: step.template_ref.clone(),
                mcp: step.mcp.clone(),
                compute_ref: step.compute_ref.clone(),
                input_mapping: step.input_mapping.clone(),
                output_schema: step.output_schema.clone(),
                condition: step.condition.clone(),
                branching: step.branching.clone(),
                branching_field: step.branching_field.clone(),
                profile: step.profile.clone(),
                gas_cap: step.gas_cap,
                timeout_seconds: step.timeout_seconds,
                phase: step.phase_str().to_string(),
                on_complete,
            });
        }

        Self {
            steps: nodes,
            by_ordinal,
            loops,
        }
    }

    /// Look up a node by `StepId`. O(1).
    pub fn step(&self, id: StepId) -> &StepNode {
        &self.steps[id as usize]
    }

    /// Resolve a user-facing ordinal to a `StepId`. O(1).
    /// Returns `None` if the ordinal doesn't exist in the graph.
    pub fn find(&self, ordinal: u32) -> Option<StepId> {
        self.by_ordinal.get(&ordinal).copied()
    }

    /// The entry step (always step 0).
    pub fn entry(&self) -> StepId {
        ENTRY
    }

    /// The number of steps in the graph.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the cascade loops after the last step.
    pub fn loops(&self) -> bool {
        self.loops
    }

    /// Iterate over all step nodes in order.
    pub fn iter(&self) -> impl Iterator<Item = &StepNode> {
        self.steps.iter()
    }

    /// The last step's `StepId`.
    pub fn last_step_id(&self) -> StepId {
        (self.steps.len() - 1) as StepId
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::cascade::CascadePhase;
    use crate::bundle::manifest::BundleManifestStep;

    fn step(ordinal: u32, action: &str) -> BundleManifestStep {
        BundleManifestStep {
            ordinal,
            action: action.to_string(),
            description: String::new(),
            renderer: None,
            template_ref: None,
            mcp: None,
            compute_ref: None,
            gas_cap: 0,
            timeout_seconds: 0,
            input_mapping: None,
            output_schema: None,
            phase: CascadePhase::default(),
            condition: None,
            branching: None,
            branching_field: None,
            profile: None,
        }
    }

    #[test]
    fn graph_addresses_by_step_id_not_ordinal() {
        let steps = vec![
            step(1, "select"),
            step(3, "select"), // gap in ordinals
            step(7, "abort"),
        ];
        let graph = StepGraph::new(&steps, 10);

        assert_eq!(graph.step(0).ordinal, 1);
        assert_eq!(graph.step(1).ordinal, 3);
        assert_eq!(graph.step(2).ordinal, 7);
    }

    #[test]
    fn find_resolves_ordinal_to_step_id_in_o1() {
        let steps = vec![step(1, "select"), step(3, "select"), step(7, "abort")];
        let graph = StepGraph::new(&steps, 10);

        assert_eq!(graph.find(1), Some(0));
        assert_eq!(graph.find(3), Some(1));
        assert_eq!(graph.find(7), Some(2));
        assert_eq!(graph.find(2), None); // gap
    }

    #[test]
    fn looping_manifest_reenters_after_last_step() {
        let steps = vec![step(1, "select"), step(2, "abort")];
        let graph = StepGraph::new(&steps, 10);

        assert!(graph.loops());
        assert_eq!(graph.step(0).on_complete, ControlFlow::Fallthrough);
        assert_eq!(graph.step(1).on_complete, ControlFlow::Reenter(ENTRY));
    }

    #[test]
    fn single_pass_manifest_exits_after_last_step() {
        let steps = vec![step(1, "select"), step(2, "abort")];
        let graph = StepGraph::new(&steps, 1);

        assert!(!graph.loops());
        assert_eq!(graph.step(0).on_complete, ControlFlow::Fallthrough);
        assert_eq!(
            graph.step(1).on_complete,
            ControlFlow::Exit(ExitKind::Converged)
        );
    }

    #[test]
    fn single_step_manifest_exits_immediately() {
        let steps = vec![step(1, "abort")];
        let graph = StepGraph::new(&steps, 1);

        assert_eq!(
            graph.step(0).on_complete,
            ControlFlow::Exit(ExitKind::Converged)
        );
    }
}
