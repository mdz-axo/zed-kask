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
use std::sync::Arc;

/// Index into a `StepGraph`'s `steps` vector. Stable for the lifetime of the
/// graph (graphs are immutable once built).
pub type StepId = u32;

/// The entry point of every cascade — always step 0.
pub const ENTRY: StepId = 0;

/// Advisory capacity cap for process-manifest execution. A manifest exceeding
/// this step count is allowed (the warn is non-breaking) but is outside the
/// measured performance envelope — per-step clones in `run_pass` and the
/// per-iteration `snapshot_prev` clone are O(N), so a large N × iterations
/// regressions latency and memory. Hard enforcement (returning an error) is
/// sequenced for the K5 slice, which changes `execute_manifest`'s return type
/// to `Result<CascadeOutcome>`; until then the warn is the diagnostic so an
/// operator can distinguish "measured" from "out of envelope."
///
/// This is *not* the bundle-composition 7-step limit in
/// `BundleManifest::validate` — that applies to composed multi-skill bundles,
/// not single-skill process manifests (e.g. the 10-step `swarm-intelligence`
/// process manifest).
pub const MAX_STEPS: usize = 4096;

/// Hard-enforce the step capacity cap. Returns `Err` if the manifest exceeds
/// `MAX_STEPS` steps. Called by `execute_manifest_into` (the public entry
/// point) and by `execute_flowdef` / `execute_parallel` (the sub-cascade paths)
/// so the gate fires in **all three** orchestration paths, not just the
/// top-level one. Previously the sub-cascade paths got only the advisory
/// `tracing::warn!` from `StepGraph::new`, which was an open loop — a
/// sub-cascade could exceed the cap and run to completion.
///
/// The `context` string names where the gate fired (e.g. `"manifest 'foo'"`
/// or `"Step 5 flowdef sub-manifest 'bar'"`) so the error message tells the
/// operator which path tripped the gate.
pub fn check_step_cap(step_count: usize, context: &str) -> Result<(), crate::ports::TemplateError> {
    if step_count > MAX_STEPS {
        return Err(crate::ports::TemplateError::Manifest(format!(
            "{context} has {step_count} steps — exceeds the capacity cap of {MAX_STEPS}. \
             Remediation: split the manifest, or raise the cap in `step_graph::MAX_STEPS`."
        )));
    }
    Ok(())
}

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
    pub ordinal: u32,
    pub action: Arc<str>,
    pub description: Arc<str>,
    pub renderer: Option<Arc<str>>,
    pub template_ref: Option<Arc<str>>,
    pub mcp: Option<Arc<str>>,
    pub compute_ref: Option<Arc<str>>,
    pub input_mapping: Option<Arc<Value>>,
    pub output_schema: Option<Arc<Value>>,
    pub condition: Option<Arc<str>>,
    pub branching: Option<Arc<HashMap<String, u32>>>,
    pub branching_field: Option<Arc<str>>,
    pub profile: Option<Arc<str>>,
    pub gas_cap: u32,
    pub timeout_seconds: u32,
    pub phase: Arc<str>,
    pub on_complete: ControlFlow,
    /// Optional string identifier (from `id:` field). `None` for skill
    /// manifests that use `ordinal` only.
    pub step_id_name: Option<Arc<str>>,
    /// Shell command for `action: gate` steps.
    pub command: Option<Arc<str>>,
    /// Per-step failure handling.
    pub on_failure: Option<Arc<crate::bundle::manifest::OnFailureConfig>>,
    /// Batch of MCP tool invocations to run concurrently. `None` for steps
    /// that use `mcp` (single call) or non-tool actions.
    pub mcp_batch: Option<Arc<Vec<crate::bundle::manifest::McpBatchEntry>>>,
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
        if steps.len() > MAX_STEPS {
            tracing::warn!(
                target: "hkask.templates.step_graph",
                step_count = steps.len(),
                cap = MAX_STEPS,
                "Manifest has {} steps — exceeds the measured capacity cap of {}. \
                 Execution is allowed (advisory) but latency/memory may regress; \
                 hard enforcement lands when execute_manifest returns Result (K5).",
                steps.len(),
                MAX_STEPS,
            );
        }
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
                action: Arc::from(step.action.clone()),
                description: Arc::from(step.description.clone()),
                renderer: step.renderer.clone().map(Arc::from),
                template_ref: step.template_ref.clone().map(Arc::from),
                mcp: step.mcp.clone().map(Arc::from),
                compute_ref: step.compute_ref.clone().map(Arc::from),
                input_mapping: step.input_mapping.clone().map(Arc::new),
                output_schema: step.output_schema.clone().map(Arc::new),
                condition: step.condition.clone().map(Arc::from),
                branching: step.branching.clone().map(Arc::new),
                branching_field: step.branching_field.clone().map(Arc::from),
                profile: step.profile.clone().map(Arc::from),
                gas_cap: step.gas_cap,
                timeout_seconds: step.timeout_seconds,
                phase: Arc::from(step.phase_str()),
                on_complete,
                step_id_name: step.id.clone().map(Arc::from),
                command: step.command.clone().map(Arc::from),
                on_failure: step.on_failure.clone().map(Arc::new),
                mcp_batch: step.mcp_batch.clone().map(Arc::new),
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
    use crate::bundle::manifest::BundleManifestStep;
    use crate::bundle::manifest::CascadePhase;

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
            id: None,
            command: None,
            on_failure: None,
            mcp_batch: None,
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
