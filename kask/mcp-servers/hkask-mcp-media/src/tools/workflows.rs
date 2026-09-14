//! Workflow composer tools — save, list, load, delete media generation pipelines.
//!
//! Fills the OMC `Task` concept with persistent workflow definitions. Workflows
//! are serialized JSON graphs stored in the `gallery_workflow` table. The
//! `media-workflow` skill encodes fixed pipelines as prose; these tools let
//! users save and reload custom pipelines.
use crate::types::{
    WorkflowDeleteRequest, WorkflowListRequest, WorkflowLoadRequest, WorkflowSaveRequest,
};
use crate::*;

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
            serde_json::from_str::<serde_json::Value>(&graph_json).map_err(|error| {
                McpToolError::invalid_argument(format!("graph_json must be valid JSON: {error}"))
            })?;
            let record = self
                .gallery_store
                .record_workflow(&graph_json)
                .map_err(|e| map_media_error(e.into()))?;
            serde_json::to_value(&record)
                .map_err(|e| McpToolError::internal(format!("encode workflow record: {e}"))) // rr0044-ok: serde serialization of own data
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
                .map_err(|e| McpToolError::internal(format!("encode workflow record: {e}"))) // rr0044-ok: serde serialization of own data
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

    impl hkask_types::InferencePort for NoInference {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
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
        let first = server
            .gallery_store
            .record_workflow(r#"{"nodes":["first"]}"#)?;
        server
            .gallery_store
            .record_workflow(r#"{"nodes":["second"]}"#)?;

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
        assert_eq!(payload["graph_json"], r#"{"nodes":["first"]}"#);
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
