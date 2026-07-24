//! Fal workflow execution engine — DAG parsing, topological sort, reference
//! resolution, and multi-node orchestration against Fal's REST API.
//!
//! Migrated from the former `hkask-fal` crate (v0.31.0 consolidation).
//!
//! # Architecture
//!
//! ```text
//! FalBackend::execute_workflow()
//!   ├── parse_workflow_nodes()    — Deserialize flat JSON into typed DAG nodes
//!   ├── validate_workflow_structure() — Must have input, run, display nodes
//!   ├── topological_sort()        — Kahn's algorithm; detects cycles
//!   ├── For each Run node:
//!   │   ├── resolve_references()  — $node.field.path → concrete values
//!   │   └── FalBackend::execute_node() → fal_sync_post()
//!   └── extract_urls()            — Collect media URLs from display output
//! ```

use hkask_types::InferenceError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// ── Workflow types ──────────────────────────────────────────────────────

/// Execution mode for a workflow Run node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Synchronous execution via `fal.run` (default).
    #[default]
    Sync,
    /// Queue-based execution via `queue.fal.run` with polling.
    /// Used for long-running models: video generation, upscaling, etc.
    Queue,
}

/// A node in a Fal workflow plan.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum WorkflowNode {
    #[serde(rename = "input")]
    Input {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        #[serde(default)]
        input: Value,
    },
    #[serde(rename = "run")]
    Run {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        app: String,
        input: Value,
        /// Execution mode (sync or queue). Defaults to sync.
        #[serde(default)]
        mode: ExecutionMode,
    },
    #[serde(rename = "display")]
    Display {
        id: String,
        #[serde(default)]
        depends: Vec<String>,
        fields: Value,
    },
}

impl WorkflowNode {
    pub fn id(&self) -> &str {
        match self {
            WorkflowNode::Input { id, .. } => id,
            WorkflowNode::Run { id, .. } => id,
            WorkflowNode::Display { id, .. } => id,
        }
    }

    pub fn depends(&self) -> &[String] {
        match self {
            WorkflowNode::Input { depends, .. } => depends,
            WorkflowNode::Run { depends, .. } => depends,
            WorkflowNode::Display { depends, .. } => depends,
        }
    }
}

/// Result of executing a full workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[must_use = "WorkflowResult contains output URLs that should not be discarded"]
pub struct WorkflowResult {
    /// URLs of generated outputs from the display node.
    pub output_urls: Vec<String>,
    /// Raw output from the display node's fields.
    pub output_fields: Value,
    /// Per-node execution metadata.
    pub node_results: HashMap<String, Value>,
    /// Total wall-clock time in seconds.
    pub elapsed_seconds: f64,
}

// ── Parsing & validation ────────────────────────────────────────────────

/// Parse a flat workflow JSON object into a list of typed nodes.
pub fn parse_workflow_nodes(workflow: &Value) -> Result<Vec<WorkflowNode>, InferenceError> {
    let obj = workflow
        .as_object()
        .ok_or_else(|| InferenceError::Generation("Workflow must be a JSON object".into()))?;

    let nodes: Result<Vec<_>, _> = obj
        .iter()
        .map(|(_key, node_value)| {
            serde_json::from_value::<WorkflowNode>(node_value.clone())
                .map_err(|e| InferenceError::Json(format!("Workflow node parse: {e}")))
        })
        .collect();

    nodes
}

/// Validate that a workflow has the required node types.
///
/// Every workflow must have: at least one input node, at least one run node,
/// and at least one display node.
pub fn validate_workflow_structure(nodes: &[WorkflowNode]) -> Result<(), InferenceError> {
    let has_input = nodes
        .iter()
        .any(|n| matches!(n, WorkflowNode::Input { .. }));
    let has_run = nodes.iter().any(|n| matches!(n, WorkflowNode::Run { .. }));
    let has_output = nodes
        .iter()
        .any(|n| matches!(n, WorkflowNode::Display { .. }));

    if !has_input || !has_run || !has_output {
        return Err(InferenceError::Generation(
            "Workflow must contain at least one input, run, and display node".into(),
        ));
    }
    Ok(())
}

// ── Topological sort ────────────────────────────────────────────────────

/// Topological sort of workflow nodes by dependency order (Kahn's algorithm).
///
/// Detects circular dependencies and unknown node references.
pub fn topological_sort(nodes: &[WorkflowNode]) -> Result<Vec<String>, InferenceError> {
    let node_ids: HashSet<&str> = nodes.iter().map(|n| n.id()).collect();

    // Build adjacency: node → nodes that depend on it (forward edges)
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for node in nodes {
        let id = node.id();
        in_degree.entry(id).or_insert(0);
        for dep in node.depends() {
            if !node_ids.contains(dep.as_str()) {
                return Err(InferenceError::Generation(format!(
                    "Node '{id}' depends on unknown node '{dep}'"
                )));
            }
            dependents.entry(dep.as_str()).or_default().push(id);
            *in_degree.entry(id).or_insert(0) += 1;
        }
    }

    // Process nodes with in-degree 0
    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut sorted: Vec<String> = Vec::new();

    while let Some(node) = queue.pop() {
        sorted.push(node.to_string());
        if let Some(deps) = dependents.get(node) {
            for &dependent in deps {
                if let Some(deg) = in_degree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(dependent);
                    }
                }
            }
        }
    }

    if sorted.len() != nodes.len() {
        let unsorted: Vec<_> = nodes
            .iter()
            .filter(|n| !sorted.contains(&n.id().to_string()))
            .map(|n| n.id().to_string())
            .collect();
        return Err(InferenceError::Generation(format!(
            "Circular dependency detected at nodes: {}",
            unsorted.join(", ")
        )));
    }

    Ok(sorted)
}

// ── Reference resolution ────────────────────────────────────────────────

/// Resolve `$node_id.field.path` references in a JSON value.
///
/// Walks the value tree and replaces string references with actual
/// values from previous node results.
pub fn resolve_references(
    value: &Value,
    results: &HashMap<String, Value>,
    depends: &[String],
) -> Result<Value, InferenceError> {
    match value {
        Value::String(s) if s.starts_with('$') => resolve_single_reference(s, results, depends),
        Value::Object(obj) => {
            let mut resolved = serde_json::Map::new();
            for (k, v) in obj {
                resolved.insert(k.clone(), resolve_references(v, results, depends)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(arr) => {
            let resolved: Result<Vec<_>, _> = arr
                .iter()
                .map(|v| resolve_references(v, results, depends))
                .collect();
            Ok(Value::Array(resolved?))
        }
        other => Ok(other.clone()),
    }
}

/// Resolve a single `$node_id.field.subfield` reference.
fn resolve_single_reference(
    reference: &str,
    results: &HashMap<String, Value>,
    depends: &[String],
) -> Result<Value, InferenceError> {
    let path = reference.strip_prefix('$').unwrap_or(reference);

    let (node_id, field_path) = path.split_once('.').ok_or_else(|| {
        InferenceError::Generation(format!("Unresolved reference: '{reference}'"))
    })?;

    if !depends.contains(&node_id.to_string()) {
        return Err(InferenceError::Generation(format!(
            "'{reference}' references node '{node_id}' which is not in depends: {depends:?}"
        )));
    }

    let node_result = results.get(node_id).ok_or_else(|| {
        InferenceError::Generation(format!(
            "'{reference}' — node '{node_id}' has no result yet"
        ))
    })?;

    let mut current = node_result;
    for segment in field_path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx).ok_or_else(|| {
                InferenceError::Generation(format!("'{reference}' — index {idx} out of range"))
            })?;
        } else {
            current = current.get(segment).ok_or_else(|| {
                InferenceError::Generation(format!("'{reference}' — field '{segment}' not found"))
            })?;
        }
    }

    Ok(current.clone())
}

// ── URL extraction ──────────────────────────────────────────────────────

/// Extract media URLs from a resolved output value.
///
/// Walks the value tree and collects string values that look like
/// media URLs (contain typical media extensions or are from Fal's CDN).
pub fn extract_urls(value: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    extract_urls_recursive(value, &mut urls);
    urls
}

fn extract_urls_recursive(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            let is_media_url = s.starts_with("https://")
                && (s.contains("fal.media")
                    || s.contains(".png")
                    || s.contains(".jpg")
                    || s.contains(".jpeg")
                    || s.contains(".webp")
                    || s.contains(".gif")
                    || s.contains(".mp4")
                    || s.contains(".mp3")
                    || s.contains(".svg")
                    || s.contains(".wav"));
            if is_media_url {
                urls.push(s.clone());
            }
        }
        Value::Object(obj) => {
            // Prefer known image fields first
            for key in &["url", "image", "image_url", "images"] {
                if let Some(v) = obj.get(*key) {
                    extract_urls_recursive(v, urls);
                }
            }
            // Then scan remaining fields
            for (k, v) in obj {
                if !["url", "image", "image_url", "images"].contains(&k.as_str()) {
                    extract_urls_recursive(v, urls);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                extract_urls_recursive(v, urls);
            }
        }
        _ => {}
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topological_sort_linear() {
        let workflow = serde_json::json!({
            "input": { "id": "input", "type": "input", "depends": [], "input": {} },
            "generate": { "id": "generate", "type": "run", "depends": ["input"], "app": "test", "input": {} },
            "output": { "id": "output", "type": "display", "depends": ["generate"], "fields": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let order = topological_sort(&nodes).unwrap();
        assert_eq!(order, vec!["input", "generate", "output"]);
    }

    #[test]
    fn test_topological_sort_parallel_branches() {
        let workflow = serde_json::json!({
            "input": { "id": "input", "type": "input", "depends": [], "input": {} },
            "branch_a": { "id": "branch_a", "type": "run", "depends": ["input"], "app": "a", "input": {} },
            "branch_b": { "id": "branch_b", "type": "run", "depends": ["input"], "app": "b", "input": {} },
            "output": { "id": "output", "type": "display", "depends": ["branch_a", "branch_b"], "fields": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let order = topological_sort(&nodes).unwrap();
        assert_eq!(order[0], "input");
        assert_eq!(order[3], "output");
    }

    #[test]
    fn test_circular_dependency_detected() {
        let workflow = serde_json::json!({
            "input": { "id": "input", "type": "input", "depends": ["output"], "input": {} },
            "output": { "id": "output", "type": "display", "depends": ["input"], "fields": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let result = topological_sort(&nodes);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Circular"));
    }

    #[test]
    fn test_reference_resolution_simple_field() {
        let mut results = HashMap::new();
        results.insert(
            "generate".to_string(),
            serde_json::json!({"seed": 42, "model": "flux"}),
        );
        let input = serde_json::json!("$generate.seed");
        let resolved = resolve_references(&input, &results, &["generate".to_string()]).unwrap();
        assert_eq!(resolved, serde_json::json!(42));
    }

    #[test]
    fn test_reference_resolution_missing_field() {
        let mut results = HashMap::new();
        results.insert("generate".to_string(), serde_json::json!({"seed": 42}));
        let input = serde_json::json!("$generate.nonexistent");
        let result = resolve_references(&input, &results, &["generate".to_string()]);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("'nonexistent' not found")
        );
    }

    #[test]
    fn test_reference_resolution_nested_object() {
        let mut results = HashMap::new();
        results.insert(
            "gen".to_string(),
            serde_json::json!({"outer": {"inner": "value"}}),
        );
        let input = serde_json::json!("$gen.outer.inner");
        let resolved = resolve_references(&input, &results, &["gen".to_string()]).unwrap();
        assert_eq!(resolved, serde_json::json!("value"));
    }

    #[test]
    fn test_reference_resolution_in_object_value() {
        let mut results = HashMap::new();
        results.insert("input".to_string(), serde_json::json!({"text": "a cat"}));
        let input = serde_json::json!({"prompt": "$input.text", "size": "square_hd"});
        let resolved = resolve_references(&input, &results, &["input".to_string()]).unwrap();
        assert_eq!(
            resolved,
            serde_json::json!({"prompt": "a cat", "size": "square_hd"})
        );
    }

    #[test]
    fn test_extract_urls() {
        let value = serde_json::json!({
            "image": "https://v3.fal.media/files/abc.png",
            "metadata": { "seed": 42 },
            "variants": [
                "https://v3.fal.media/files/def.jpg",
                "not-a-url"
            ]
        });
        let urls = extract_urls(&value);
        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_missing_required_nodes_rejected() {
        let workflow = serde_json::json!({
            "input": { "id": "input", "type": "input", "depends": [], "input": {} },
            "output": { "id": "output", "type": "display", "depends": ["input"], "fields": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let result = validate_workflow_structure(&nodes);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must contain at least one")
        );
    }

    #[test]
    fn test_valid_workflow_passes_validation() {
        let workflow = serde_json::json!({
            "input": { "id": "input", "type": "input", "depends": [], "input": {} },
            "generate": { "id": "generate", "type": "run", "depends": ["input"], "app": "test", "input": {} },
            "output": { "id": "output", "type": "display", "depends": ["generate"], "fields": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        assert!(validate_workflow_structure(&nodes).is_ok());
    }

    #[test]
    fn test_run_node_defaults_to_sync_mode() {
        let workflow = serde_json::json!({
            "generate": { "id": "generate", "type": "run", "depends": [], "app": "test", "input": {} }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let run_node = nodes.first().unwrap();
        match run_node {
            WorkflowNode::Run { mode, .. } => {
                assert_eq!(*mode, ExecutionMode::Sync);
            }
            _ => panic!("Expected Run node"),
        }
    }

    #[test]
    fn test_run_node_deserializes_queue_mode() {
        let workflow = serde_json::json!({
            "video": { "id": "video", "type": "run", "depends": [], "app": "fal-ai/minimax/video-01-live", "input": {}, "mode": "queue" }
        });
        let nodes = parse_workflow_nodes(&workflow).unwrap();
        let run_node = nodes.first().unwrap();
        match run_node {
            WorkflowNode::Run { mode, .. } => {
                assert_eq!(*mode, ExecutionMode::Queue);
            }
            _ => panic!("Expected Run node"),
        }
    }
}
