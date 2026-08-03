//! Fal.ai workflow adapter — parses fal.ai workflow JSON (the `Input`/`Run`/
//! `Display` DAG) into a general [`WorkflowGraph`].
//!
//! This is the serialization-alias layer: existing fal.ai workflow JSON (as
//! emitted by `media/workflow-composer`) is parsed into the general graph,
//! executed by [`crate::workflow::WorkflowGraph::execute`], and the
//! [`crate::fal_workflow::WorkflowResult`] is returned unchanged. Existing
//! `FalBackend::execute_workflow` results stay byte-identical.
//!
//! Mapping:
//! - `Input`  → [`GraphNode::Source`]  (`value` = `input`)
//! - `Run`    → [`GraphNode::Compute`] (`app`, `input`, `mode`; `on_failure`
//!   defaults to `Abort`, preserving the pre-refactor abort-on-failure
//!   behavior — fal.ai JSON does not carry a failure policy)
//! - `Display`→ [`GraphNode::Sink`]    (`fields`)
//!
//! Validation (`must contain at least one input, run, and display node`) is
//! preserved from [`crate::fal_workflow::validate_workflow_structure`].

use crate::fal_workflow::{self, WorkflowNode};
use crate::workflow::{FailurePolicy, GraphNode, WorkflowGraph};
use hkask_types::InferenceError;
use serde_json::Value;

/// Parse fal.ai workflow JSON into a general [`WorkflowGraph`].
///
/// expect: "The system maps the fal.ai DAG shape onto the general node graph"
/// pre:  workflow is a fal.ai workflow JSON object with input/run/display nodes
/// post: returns Ok(WorkflowGraph) with Source/Compute/Sink nodes
/// post: if the workflow is malformed → Err(InferenceError::Json/Generation)
pub fn parse_fal_workflow(workflow: &Value) -> Result<WorkflowGraph, InferenceError> {
    let nodes = fal_workflow::parse_workflow_nodes(workflow)?;
    fal_workflow::validate_workflow_structure(&nodes)?;
    let graph_nodes = nodes.into_iter().map(workflow_node_to_graph).collect();
    Ok(WorkflowGraph::new(graph_nodes))
}

/// Map a fal.ai [`WorkflowNode`] to a general [`GraphNode`].
fn workflow_node_to_graph(node: WorkflowNode) -> GraphNode {
    match node {
        WorkflowNode::Input { id, depends, input } => GraphNode::Source {
            id,
            depends,
            value: input,
        },
        WorkflowNode::Run {
            id,
            depends,
            app,
            input,
            mode,
        } => GraphNode::Compute {
            id,
            depends,
            app,
            input,
            mode,
            // fal.ai JSON has no failure policy → default Abort preserves the
            // pre-refactor abort-on-first-failure behavior exactly.
            on_failure: FailurePolicy::Abort,
        },
        WorkflowNode::Display {
            id,
            depends,
            fields,
        } => GraphNode::Sink {
            id,
            depends,
            fields,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fal_workflow::ExecutionMode;
    use crate::workflow::NodeExecutor;
    use std::future::Future;
    use std::pin::Pin;

    /// A fal.ai-shaped workflow JSON (as `workflow-composer.j2` emits it):
    /// input → run (sync) → run (queue, depends on first run) → display.
    fn fal_workflow_json() -> Value {
        serde_json::json!({
            "prompt_node": {
                "type": "input",
                "id": "prompt_node",
                "input": {"prompt": "a serene mountain landscape"}
            },
            "gen": {
                "type": "run",
                "id": "gen",
                "depends": ["prompt_node"],
                "app": "fal-ai/flux/dev",
                "input": {"prompt": "$prompt_node.prompt"}
            },
            "upscale": {
                "type": "run",
                "id": "upscale",
                "depends": ["gen"],
                "app": "fal-ai/seedvr2",
                "mode": "queue",
                "input": {"image_url": "$gen.images.0.url", "scale": 4}
            },
            "output": {
                "type": "display",
                "id": "output",
                "depends": ["upscale", "gen"],
                "fields": {"final_url": "$upscale.output.url", "original": "$gen.images.0.url"}
            }
        })
    }

    /// Mock executor that returns a canned result per app, exercising the
    /// full fal.ai reference chain ($gen.images[0].url, $upscale.output.url).
    struct FalMock;
    impl NodeExecutor for FalMock {
        fn execute_node<'a>(
            &'a self,
            app: &'a str,
            _input: Value,
            _mode: fal_workflow::ExecutionMode,
        ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
            Box::pin(async move {
                match app {
                    "fal-ai/flux/dev" => Ok(serde_json::json!({
                        "images": [{"url": "https://fal.media/gen.png"}]
                    })),
                    "fal-ai/seedvr2" => Ok(serde_json::json!({
                        "output": {"url": "https://fal.media/upscaled.png"}
                    })),
                    other => Err(InferenceError::Connection(format!("unknown app {other}"))),
                }
            })
        }
    }

    #[test]
    fn fal_workflow_parses_to_graph_with_source_compute_sink() {
        let graph = parse_fal_workflow(&fal_workflow_json()).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| matches!(n, GraphNode::Source { .. }))
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| matches!(n, GraphNode::Compute { .. }))
        );
        assert!(
            graph
                .nodes
                .iter()
                .any(|n| matches!(n, GraphNode::Sink { .. }))
        );
        // Default sequential.
        assert!(!graph.parallel);
        // Queue mode preserved on the upscale compute node.
        assert!(graph.nodes.iter().any(|n| matches!(
            n,
            GraphNode::Compute { mode: ExecutionMode::Queue, app, .. } if app == "fal-ai/seedvr2"
        )));
    }

    #[tokio::test]
    async fn fal_adapter_roundtrip_preserves_result() {
        // Byte-identical to the pre-refactor FalBackend::execute_workflow:
        // the sink's output_fields resolve the fal.ai response shapes
        // ($gen.images[0].url, $upscale.output.url) and extract_urls finds them.
        let graph = parse_fal_workflow(&fal_workflow_json()).unwrap();
        let result = graph.execute(&FalMock).await.unwrap();
        assert_eq!(
            result.output_fields["final_url"],
            "https://fal.media/upscaled.png"
        );
        assert_eq!(
            result.output_fields["original"],
            "https://fal.media/gen.png"
        );
        // extract_urls collects both fal.media URLs.
        assert_eq!(result.output_urls.len(), 2);
        assert!(
            result
                .output_urls
                .contains(&"https://fal.media/upscaled.png".to_string())
        );
        assert!(
            result
                .output_urls
                .contains(&"https://fal.media/gen.png".to_string())
        );
        // node_results has input + both runs (display is not in node_results,
        // matching pre-refactor behavior).
        assert!(result.node_results.contains_key("prompt_node"));
        assert!(result.node_results.contains_key("gen"));
        assert!(result.node_results.contains_key("upscale"));
        assert!(!result.node_results.contains_key("output"));
    }

    #[test]
    fn fal_adapter_rejects_missing_display_node() {
        // No display node → validate_workflow_structure fails.
        let bad = serde_json::json!({
            "prompt_node": {"type": "input", "id": "prompt_node", "input": {}},
            "gen": {"type": "run", "id": "gen", "app": "fal-ai/flux/dev", "input": {}}
        });
        assert!(parse_fal_workflow(&bad).is_err());
    }
}
