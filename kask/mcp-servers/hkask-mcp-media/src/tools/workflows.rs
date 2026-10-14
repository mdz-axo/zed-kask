//! Workflow composer tools — save, list, load, delete media generation pipelines.
//!
//! Fills the OMC `Task` concept with persistent workflow definitions validated
//! against the ComfyUI / fal.ai DAG model: a workflow is a topologically ordered
//! step DAG (`steps` / `nodes`, 1–64 steps) where each step has a unique `id`,
//! a registered media `tool` in `TOOL_NAMES`, and `params` whose upstream
//! `{"$from": "<step_id>", "field": "<output_field>"}` bindings reference
//! earlier steps.
use crate::types::{
    WorkflowDeleteRequest, WorkflowListRequest, WorkflowLoadRequest, WorkflowSaveRequest,
};
use crate::*;
use std::collections::HashSet;

const MAX_WORKFLOW_STEPS: usize = 64;

/// Canonical workflow DAG definition (ComfyUI node/link DAG + fal.ai
/// parameterized step pipeline).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowGraph {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub inputs: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(alias = "nodes")]
    pub steps: Vec<WorkflowStep>,
}

/// One node in a `WorkflowGraph`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowStep {
    pub id: String,
    pub tool: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
}

fn validate_param_bindings(
    step_id: &str,
    value: &serde_json::Value,
    seen_steps: &HashSet<String>,
) -> Result<(), McpToolError> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(from_val) = map.get("$from") {
                let Some(upstream_id) = from_val.as_str().map(str::trim).filter(|s| !s.is_empty())
                else {
                    return Err(McpToolError::invalid_argument(format!(
                        "workflow step '{step_id}' has a '$from' binding whose target is not a non-empty step id string"
                    )));
                };
                if !seen_steps.contains(upstream_id) {
                    return Err(McpToolError::invalid_argument(format!(
                        "workflow step '{step_id}' references '$from': '{upstream_id}', which is not an earlier step in topological order"
                    )));
                }
                if let Some(field_val) = map.get("field")
                    && field_val.as_str().map(str::trim).is_none_or(str::is_empty)
                {
                    return Err(McpToolError::invalid_argument(format!(
                        "workflow step '{step_id}' '$from' binding has an empty or non-string 'field'"
                    )));
                }
            }
            for child in map.values() {
                validate_param_bindings(step_id, child, seen_steps)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for item in items {
                validate_param_bindings(step_id, item, seen_steps)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Validate that `graph_json` is a well-formed, acyclic ComfyUI/fal.ai-style
/// workflow DAG whose steps reference registered media tools.
pub fn validate_workflow_graph(graph_json: &str) -> Result<WorkflowGraph, McpToolError> {
    let graph: WorkflowGraph = serde_json::from_str(graph_json).map_err(|error| {
        McpToolError::invalid_argument(format!(
            "graph_json must be a valid WorkflowGraph JSON object ({{name?, description?, inputs?, steps: [{{id, tool, params?}}]}}): {error}"
        ))
    })?;
    validate_item_count("steps", graph.steps.len(), 1, MAX_WORKFLOW_STEPS)?;
    let mut seen_steps = HashSet::with_capacity(graph.steps.len());
    for step in &graph.steps {
        let id = step.id.trim();
        if id.is_empty() {
            return Err(McpToolError::invalid_argument(
                "workflow step 'id' must not be empty",
            ));
        }
        if step.tool.starts_with("workflow_") || !crate::TOOL_NAMES.contains(&step.tool.as_str()) {
            return Err(McpToolError::invalid_argument(format!(
                "workflow step '{id}' references unsupported or unknown media tool '{}'",
                step.tool
            )));
        }
        let params_val = serde_json::Value::Object(step.params.clone());
        validate_param_bindings(id, &params_val, &seen_steps)?;
        if !seen_steps.insert(id.to_string()) {
            return Err(McpToolError::invalid_argument(format!(
                "duplicate workflow step id '{id}'"
            )));
        }
    }
    Ok(graph)
}

#[tool_router(router = workflows_router, vis = "pub")]
impl MediaServer {
    /// Save a workflow definition (serialized JSON graph) to the gallery DB.
    /// Returns the workflow ID for later loading.
    #[tool(
        description = "Save a media generation workflow definition (serialized JSON) to the gallery. Returns the workflow ID for later loading or re-execution."
    )]
    pub async fn workflow_save(
        &self,
        Parameters(WorkflowSaveRequest { graph_json }): Parameters<WorkflowSaveRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "workflow_save", async {
            if graph_json.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "graph_json must not be empty",
                ));
            }
            if graph_json.len() > hkask_types::media_limits::MAX_WORKFLOW_GRAPH_BYTES {
                return Err(McpToolError::invalid_argument(format!(
                    "graph_json is {} bytes; maximum is {} bytes",
                    graph_json.len(),
                    hkask_types::media_limits::MAX_WORKFLOW_GRAPH_BYTES
                )));
            }
            validate_workflow_graph(&graph_json)?;
            let record = self
                .gallery_store
                .record_workflow(&graph_json)
                .map_err(|e| map_media_error(e.into()))?;
            serde_json::to_value(&record)
                .map_err(|e| McpToolError::internal(format!("encode workflow record: {e}")))
        })
        .await
    }

    /// List a bounded page of saved workflow summaries, newest first.
    #[tool(
        description = "List saved media generation workflow summaries, newest first. Full graph JSON is available through workflow_load."
    )]
    pub async fn workflow_list(
        &self,
        Parameters(WorkflowListRequest { limit }): Parameters<WorkflowListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "workflow_list", async {
            let limit = limit.unwrap_or(hkask_types::media_limits::DEFAULT_WORKFLOW_LIST_LIMIT);
            validate_item_count(
                "limit",
                limit,
                1,
                hkask_types::media_limits::MAX_WORKFLOW_LIST_LIMIT,
            )?;
            let (workflows, total) = self
                .gallery_store
                .list_workflow_summaries(limit)
                .map_err(|e| map_media_error(e.into()))?;
            Ok(serde_json::json!({
                "workflows": workflows,
                "total": total,
                "has_more": total > limit,
            }))
        })
        .await
    }

    /// Load a saved workflow by ID. Returns the serialized JSON graph.
    #[tool(
        description = "Load a saved media generation workflow by its ID. Returns the serialized JSON graph."
    )]
    pub async fn workflow_load(
        &self,
        Parameters(WorkflowLoadRequest { workflow_id }): Parameters<WorkflowLoadRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "workflow_load", async {
            let record = self
                .gallery_store
                .get_workflow(&workflow_id)
                .map_err(|e| map_media_error(e.into()))?;
            serde_json::to_value(&record)
                .map_err(|e| McpToolError::internal(format!("encode workflow record: {e}")))
        })
        .await
    }

    /// Delete a saved workflow. Assets produced by the workflow are not
    /// affected — only the workflow definition is removed.
    #[tool(
        description = "Delete a saved media generation workflow by its ID. Assets produced by the workflow are not affected."
    )]
    pub async fn workflow_delete(
        &self,
        Parameters(WorkflowDeleteRequest { workflow_id }): Parameters<WorkflowDeleteRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "workflow_delete", async {
            self.gallery_store
                .delete_workflow(&workflow_id)
                .map_err(|e| map_media_error(e.into()))?;
            Ok(serde_json::json!({
                "deleted": true,
                "workflow_id": workflow_id,
            }))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoInference;

    crate::test_support::media_inference_stub! {
        NoInference,
        generate(self, _prompt): {
            panic!("workflow tools must not invoke inference")
        }
    }

    fn make_server() -> Result<MediaServer, Box<dyn std::error::Error>> {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        Ok(MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoInference),
            Arc::new(Mutex::new(None)),
            Arc::new(GalleryStore::from_driver(driver)?),
            crate::templates::create_env()?,
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            None,
        ))
    }

    /// expect: Invalid or oversized workflow definitions fail before persistence.
    #[tokio::test]
    async fn workflow_save_rejects_invalid_json_and_over_cap_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = make_server()?;
        let oversized = format!(
            "\"{}\"",
            "x".repeat(hkask_types::media_limits::MAX_WORKFLOW_GRAPH_BYTES)
        );
        for graph_json in ["not-json".to_string(), oversized] {
            let error = server
                .workflow_save(Parameters(WorkflowSaveRequest { graph_json }))
                .await
                .expect_err("invalid workflow graph must fail");
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        }
        assert_eq!(server.gallery_store.list_workflow_summaries(1)?.1, 0);
        Ok(())
    }

    /// expect: Workflow list pages contain bounded summaries and pagination metadata while load
    /// remains the full-graph interface.
    #[tokio::test]
    async fn workflow_list_is_bounded_summary_and_load_remains_full()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = make_server()?;
        let first_graph = r#"{"name":"first","steps":[{"id":"gen","tool":"generate_image","params":{"prompt":"sunset"}}]}"#;
        let second_graph = r#"{"name":"second","steps":[{"id":"gen","tool":"generate_image","params":{"prompt":"mountains"}}]}"#;
        let first = server.gallery_store.record_workflow(first_graph)?;
        server.gallery_store.record_workflow(second_graph)?;

        let response = server
            .workflow_list(Parameters(WorkflowListRequest { limit: Some(1) }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["total"], 2);
        assert_eq!(payload["has_more"], true);
        assert_eq!(payload["workflows"].as_array().map(Vec::len), Some(1));
        assert!(payload["workflows"][0].get("graph_json").is_none());

        let loaded = server
            .workflow_load(Parameters(WorkflowLoadRequest {
                workflow_id: first.id,
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&loaded)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["graph_json"], first_graph);
        Ok(())
    }

    /// expect: workflow_save accepts valid ComfyUI/fal.ai-style step DAGs and rejects unknown tools,
    /// duplicate step IDs, empty steps, recursive workflow tools, and forward/cyclic $from bindings.
    #[tokio::test]
    async fn workflow_save_validates_dag_steps_tools_and_upstream_links()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = make_server()?;
        let valid_dag = serde_json::json!({
            "name": "product-shot-4k",
            "inputs": { "prompt": "studio watch photo" },
            "steps": [
                {
                    "id": "gen",
                    "tool": "generate_image",
                    "params": { "prompt": "studio watch photo" }
                },
                {
                    "id": "cutout",
                    "tool": "image_remove_background",
                    "params": { "image_index": { "$from": "gen", "field": "image_index" } }
                },
                {
                    "id": "upscale",
                    "tool": "upscale_image",
                    "params": { "image_url": { "$from": "cutout", "field": "output" }, "scale": 4 }
                }
            ]
        })
        .to_string();

        let saved = server
            .workflow_save(Parameters(WorkflowSaveRequest {
                graph_json: valid_dag.clone(),
            }))
            .await?;
        let saved_val =
            hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&saved)?);
        let wf_id = saved_val["id"].as_str().ok_or("missing workflow id")?;
        let loaded = server
            .workflow_load(Parameters(WorkflowLoadRequest {
                workflow_id: wf_id.to_string(),
            }))
            .await?;
        let loaded_val =
            hkask_types::tool_response::unwrap_tool_envelope(serde_json::from_str(&loaded)?);
        assert_eq!(loaded_val["graph_json"], valid_dag);

        for invalid in [
            serde_json::json!({"steps": []}),
            serde_json::json!({"steps": [{"id": "", "tool": "generate_image"}]}),
            serde_json::json!({"steps": [{"id": "s1", "tool": "nonexistent_tool"}]}),
            serde_json::json!({"steps": [{"id": "s1", "tool": "workflow_save"}]}),
            serde_json::json!({
                "steps": [
                    {"id": "dup", "tool": "generate_image"},
                    {"id": "dup", "tool": "upscale_image"}
                ]
            }),
            serde_json::json!({
                "steps": [
                    {"id": "s1", "tool": "upscale_image", "params": {"image_url": {"$from": "s1"}}}
                ]
            }),
            serde_json::json!({
                "steps": [
                    {"id": "s1", "tool": "upscale_image", "params": {"image_url": {"$from": "s2"}}},
                    {"id": "s2", "tool": "generate_image"}
                ]
            }),
            serde_json::json!({
                "steps": [
                    {"id": "s1", "tool": "generate_image"},
                    {"id": "s2", "tool": "upscale_image", "params": {"image_url": {"$from": "s1", "field": "  "}}}
                ]
            }),
        ] {
            let err = server
                .workflow_save(Parameters(WorkflowSaveRequest {
                    graph_json: invalid.to_string(),
                }))
                .await
                .expect_err("invalid DAG must be rejected");
            assert_eq!(
                err.kind,
                hkask_types::McpErrorKind::InvalidArgument,
                "rejected case {invalid}: {err:?}"
            );
        }
        Ok(())
    }

    /// expect: Invalid workflow page sizes fail instead of being clamped.
    #[tokio::test]
    async fn workflow_list_rejects_zero_and_over_cap_limits()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = make_server()?;
        for limit in [0, hkask_types::media_limits::MAX_WORKFLOW_LIST_LIMIT + 1] {
            let error = server
                .workflow_list(Parameters(WorkflowListRequest { limit: Some(limit) }))
                .await
                .expect_err("invalid workflow limit must fail");
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        }
        Ok(())
    }
}
