//! Workflow composer tools — save, list, load, delete media generation pipelines.
//!
//! Fills the OMC `Task` concept with persistent workflow definitions. Workflows
//! are serialized JSON graphs stored in the `gallery_workflow` table. The
//! `media-workflow` skill encodes fixed pipelines as prose; these tools let
//! users save and reload custom pipelines.
use crate::types::{WorkflowDeleteRequest, WorkflowLoadRequest, WorkflowSaveRequest};
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
            let record = self
                .gallery_store
                .record_workflow(&graph_json)
                .map_err(|e| map_media_error(e.into()))?;
            serde_json::to_value(&record)
                .map_err(|e| McpToolError::internal(format!("encode workflow record: {e}"))) // rr0044-ok: serde serialization of own data
        })
        .await
    }

    /// List all saved workflows, newest first.
    #[tool(description = "List all saved media generation workflows, newest first.")]
    pub async fn workflow_list(&self) -> Result<String, McpToolError> {
        execute_tool(self, "workflow_list", async {
            let workflows = self
                .gallery_store
                .list_workflows()
                .map_err(|e| map_media_error(e.into()))?;
            serde_json::to_value(&workflows)
                .map_err(|e| McpToolError::internal(format!("encode workflow list: {e}"))) // rr0044-ok: serde serialization of own data
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
