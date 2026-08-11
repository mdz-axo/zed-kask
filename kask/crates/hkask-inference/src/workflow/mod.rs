//! General typed node-graph workflow engine.
//!
//! Subsumes fal.ai's 3-node-type DAG (`Input`/`Run`/`Display`) as a degenerate
//! case: `Source` ≡ `Input`, `Compute` ≡ `Run`, `Sink` ≡ `Display`. The fal.ai
//! shape is preserved by [`super::fal_adapter`], which parses fal.ai workflow
//! JSON into a [`WorkflowGraph`], so `FalBackend::execute_workflow` results
//! stay byte-identical to the pre-refactor implementation.
//!
//! Reuses fal.ai's `$reference` resolution and URL extraction (both are
//! value-tree walks, not fal-specific) from [`crate::fal_workflow`].
//!
//! Additions over the fal.ai engine:
//! - per-node [`FailurePolicy`] (`Abort` default = pre-refactor behavior,
//!   `Skip`, `Retry { n }`);
//! - opt-in [`WorkflowGraph::parallel`] (independent same-level nodes run
//!   concurrently; default sequential preserves pre-refactor behavior);
//! - JSON persistence (`Serialize`/`Deserialize`) for export / import /
//!   re-execute (ComfyUI-style).
//!
//! If zed later adds a media-handling trait to its router, this engine can be
//! driven by a zed-side executor instead of `FalBackend` without changing the
//! graph model — the providers stay behind `MediaProvider` (WS-1), the graph
//! stays provider-agnostic here.

pub mod fal_adapter;

use crate::fal_workflow::{ExecutionMode, WorkflowResult, extract_urls, resolve_references};
use hkask_types::InferenceError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

/// A typed node in the graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GraphNode {
    /// Provides caller values (fal.ai `Input`). `value` is the literal input.
    Source {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        value: Value,
    },
    /// Executes a provider call (fal.ai `Run`).
    Compute {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        app: String,
        /// Unresolved input; `$references` are resolved against prior nodes
        /// before the executor is called.
        input: Value,
        #[serde(default)]
        mode: ExecutionMode,
        #[serde(default)]
        on_failure: FailurePolicy,
    },
    /// Collects final outputs (fal.ai `Display`). `fields` selects what to
    /// emit via `$references`.
    Sink {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        fields: Value,
    },
}

impl GraphNode {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Source { id, .. } | Self::Compute { id, .. } | Self::Sink { id, .. } => id,
        }
    }

    #[must_use]
    pub fn depends(&self) -> &[String] {
        match self {
            Self::Source { depends, .. }
            | Self::Compute { depends, .. }
            | Self::Sink { depends, .. } => depends,
        }
    }
}

/// What to do when a `Compute` node fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailurePolicy {
    /// Abort the whole workflow on failure. Default (pre-refactor behavior).
    #[default]
    Abort,
    /// Skip the failed node (no result inserted); the workflow continues.
    /// Dependents that reference the skipped node fail at `$reference`
    /// resolution unless they also `Skip`.
    Skip,
    /// Retry up to `n` times. On final failure the workflow aborts (Retry
    /// does not compose with Skip in this version — retry-then-skip would
    /// need a richer policy type; deferred).
    Retry { n: u32 },
}

/// Executes a `Compute` node's provider call. The fal.ai adapter implements
/// this with `fal_sync_post` / `fal_queue_post`; a zed-side media executor
/// could implement it the same way without touching the graph model.
pub trait NodeExecutor: Send + Sync {
    fn execute_node<'a>(
        &'a self,
        app: &'a str,
        input: Value,
        mode: ExecutionMode,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>>;
}

/// A typed node graph. Serializes to JSON for export / import / re-execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowGraph {
    pub nodes: Vec<GraphNode>,
    /// Opt-in: execute independent nodes (same topological level)
    /// concurrently. Default `false` (sequential) preserves the pre-refactor
    /// execution order and is byte-identical for existing fal.ai workflows.
    #[serde(default)]
    pub parallel: bool,
}

impl WorkflowGraph {
    #[must_use]
    pub fn new(nodes: Vec<GraphNode>) -> Self {
        Self {
            nodes,
            parallel: false,
        }
    }

    /// Execute the graph against `executor`.
    ///
    /// expect: "The system executes a media workflow DAG in dependency order"
    /// pre:  the graph is a valid DAG (no cycles, all `depends` reference known nodes)
    /// post: returns Ok(WorkflowResult) with per-node results and sink output
    /// post: default (sequential, Abort) is byte-identical to pre-refactor fal.ai execution
    /// post: a failing Compute node with Abort (default) aborts the workflow (Err)
    /// post: a failing Compute node with Skip is omitted from `node_results`
    pub async fn execute(
        &self,
        executor: &dyn NodeExecutor,
    ) -> Result<WorkflowResult, InferenceError> {
        let start = Instant::now();
        let order = topological_sort_graph(&self.nodes)?;
        let node_map: HashMap<&str, &GraphNode> = self.nodes.iter().map(|n| (n.id(), n)).collect();
        let mut results: HashMap<String, Value> = HashMap::new();
        let mut output_fields = Value::Null;
        let mut output_urls: Vec<String> = Vec::new();

        if self.parallel {
            let levels = group_into_levels(&self.nodes, &order);
            for level in &levels {
                execute_level(
                    level,
                    &node_map,
                    executor,
                    &mut results,
                    &mut output_fields,
                    &mut output_urls,
                )
                .await?;
            }
        } else {
            for node_id in &order {
                let node = node_map
                    .get(node_id.as_str())
                    .expect("topological order references a known node");
                execute_single(
                    node,
                    executor,
                    &mut results,
                    &mut output_fields,
                    &mut output_urls,
                )
                .await?;
            }
        }

        Ok(WorkflowResult {
            output_urls,
            output_fields,
            node_results: results,
            elapsed_seconds: start.elapsed().as_secs_f64(),
        })
    }
}

/// Run one `Compute` node applying its [`FailurePolicy`].
///
/// Returns `Ok(Some(value))` on success, `Ok(None)` if the node was skipped
/// (`Skip` policy after failure), or `Err` if the workflow should abort
/// (`Abort` policy, or `Retry` exhausted).
async fn run_with_policy(
    executor: &dyn NodeExecutor,
    app: &str,
    input: Value,
    mode: ExecutionMode,
    policy: FailurePolicy,
) -> Result<Option<Value>, InferenceError> {
    let attempts: u32 = match policy {
        FailurePolicy::Retry { n } => n.saturating_add(1).max(1),
        _ => 1,
    };
    let mut last_err: Option<InferenceError> = None;
    for attempt in 0..attempts {
        match executor.execute_node(app, input.clone(), mode).await {
            Ok(value) => return Ok(Some(value)),
            Err(err) => {
                if attempt + 1 < attempts {
                    tracing::warn!(
                        target: "hkask.workflow",
                        app,
                        attempt,
                        error = %err,
                        "compute node failed, retrying"
                    );
                }
                last_err = Some(err);
            }
        }
    }
    match policy {
        FailurePolicy::Skip => {
            tracing::warn!(target: "hkask.workflow", app, "compute node skipped after failure");
            Ok(None)
        }
        _ => Err(last_err.unwrap_or_else(|| {
            InferenceError::Connection(format!(
                "compute node '{app}' failed with no error captured"
            ))
        })),
    }
}

/// Execute a single node sequentially.
async fn execute_single(
    node: &GraphNode,
    executor: &dyn NodeExecutor,
    results: &mut HashMap<String, Value>,
    output_fields: &mut Value,
    output_urls: &mut Vec<String>,
) -> Result<(), InferenceError> {
    match node {
        GraphNode::Source { id, value, .. } => {
            results.insert(id.clone(), value.clone());
            Ok(())
        }
        GraphNode::Compute {
            id,
            app,
            input,
            depends,
            mode,
            on_failure,
        } => {
            let resolved = resolve_references(input, results, depends)?;
            if let Some(result) =
                run_with_policy(executor, app, resolved, *mode, *on_failure).await?
            {
                results.insert(id.clone(), result);
            }
            Ok(())
        }
        GraphNode::Sink {
            fields, depends, ..
        } => {
            let resolved = resolve_references(fields, results, depends)?;
            *output_fields = resolved.clone();
            *output_urls = extract_urls(&resolved);
            Ok(())
        }
    }
}

/// Execute all nodes in one topological level. `Source` nodes insert
/// synchronously; `Compute` nodes run concurrently (joined); `Sink` nodes
/// resolve and extract after computes finish. Order within a level does not
/// affect correctness (no intra-level dependencies).
async fn execute_level(
    level: &[String],
    node_map: &HashMap<&str, &GraphNode>,
    executor: &dyn NodeExecutor,
    results: &mut HashMap<String, Value>,
    output_fields: &mut Value,
    output_urls: &mut Vec<String>,
) -> Result<(), InferenceError> {
    // Source nodes: insert synchronously (no execution).
    for id in level {
        if let GraphNode::Source { value, .. } = node_map.get(id.as_str()).expect("known node") {
            results.insert(id.clone(), value.clone());
        }
    }

    // Compute nodes: resolve inputs (immutable borrow of `results`), then run
    // concurrently (no borrow of `results`), then insert successful results.
    let mut compute_jobs: Vec<(String, String, Value, ExecutionMode, FailurePolicy)> = Vec::new();
    for id in level {
        if let GraphNode::Compute {
            id: cid,
            app,
            input,
            depends,
            mode,
            on_failure,
        } = node_map.get(id.as_str()).expect("known node")
        {
            let resolved = resolve_references(input, results, depends)?;
            compute_jobs.push((cid.clone(), app.clone(), resolved, *mode, *on_failure));
        }
    }

    let futures: Vec<_> = compute_jobs
        .into_iter()
        .map(|(id, app, input, mode, policy)| {
            Box::pin(async move {
                let opt = run_with_policy(executor, &app, input, mode, policy).await?;
                Ok((id, opt))
            })
                as Pin<
                    Box<
                        dyn Future<Output = Result<(String, Option<Value>), InferenceError>> + Send,
                    >,
                >
        })
        .collect();
    let outcomes = futures_util::future::join_all(futures).await;
    for outcome in outcomes {
        let (id, opt) = outcome?;
        if let Some(value) = opt {
            results.insert(id, value);
        }
    }

    // Sink nodes: resolve + extract synchronously after computes finish.
    for id in level {
        if let GraphNode::Sink {
            fields, depends, ..
        } = node_map.get(id.as_str()).expect("known node")
        {
            let resolved = resolve_references(fields, results, depends)?;
            *output_fields = resolved.clone();
            *output_urls = extract_urls(&resolved);
        }
    }
    Ok(())
}

/// Kahn topological sort over `GraphNode`s, deterministic on declaration
/// order for independent nodes (unlike the pre-refactor HashMap-seeded sort,
/// which was randomized — results are order-independent either way).
pub fn topological_sort_graph(nodes: &[GraphNode]) -> Result<Vec<String>, InferenceError> {
    let ids: HashSet<&str> = nodes.iter().map(GraphNode::id).collect();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        let id = node.id();
        in_degree.entry(id).or_insert(0);
        for dep in node.depends() {
            if !ids.contains(dep.as_str()) {
                return Err(InferenceError::Generation(format!(
                    "Node '{id}' depends on unknown node '{dep}'"
                )));
            }
            dependents.entry(dep.as_str()).or_default().push(id);
            *in_degree.entry(id).or_insert(0) += 1;
        }
    }
    // Seed the ready queue in declaration order for deterministic output.
    let mut ready: VecDeque<&str> = nodes
        .iter()
        .map(GraphNode::id)
        .filter(|id| in_degree.get(id) == Some(&0))
        .collect();
    let mut sorted: Vec<String> = Vec::new();
    while let Some(id) = ready.pop_front() {
        sorted.push(id.to_string());
        if let Some(deps) = dependents.get(id) {
            for &dependent in deps {
                if let Some(deg) = in_degree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        ready.push_back(dependent);
                    }
                }
            }
        }
    }
    if sorted.len() != nodes.len() {
        let unsorted: Vec<_> = nodes
            .iter()
            .filter(|n| !sorted.contains(&n.id().to_string()))
            .map(GraphNode::id)
            .map(String::from)
            .collect();
        return Err(InferenceError::Generation(format!(
            "Circular dependency detected at nodes: {}",
            unsorted.join(", ")
        )));
    }
    Ok(sorted)
}

/// Group node ids into topological levels (a node's deps are all in earlier
/// levels). Order within a level follows `order`.
fn group_into_levels(nodes: &[GraphNode], order: &[String]) -> Vec<Vec<String>> {
    let node_map: HashMap<&str, &GraphNode> = nodes.iter().map(|n| (n.id(), n)).collect();
    let mut level_of: HashMap<String, usize> = HashMap::new();
    for id in order {
        let node = node_map
            .get(id.as_str())
            .expect("order references known node");
        let lvl = node
            .depends()
            .iter()
            .filter_map(|d| level_of.get(d))
            .copied()
            .max()
            .map_or(0, |m| m + 1);
        level_of.insert(id.clone(), lvl);
    }
    let max_level = level_of.values().copied().max().unwrap_or(0);
    let mut levels: Vec<Vec<String>> = (0..=max_level).map(|_| Vec::new()).collect();
    for id in order {
        let lvl = level_of[id];
        levels[lvl].push(id.clone());
    }
    levels
}
