use crate::TrainingServer;
use crate::types::TrainCancelRequest;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::tool;
use serde_json::json;

impl TrainingServer {
    #[tool(description = "Cancel a running or queued training job.")]
    pub async fn training_cancel(
        &self,
        Parameters(TrainCancelRequest { job_id }): Parameters<TrainCancelRequest>,
    ) -> String {
        execute_tool(self, "training_cancel", async {
            match self.host.cancel(&job_id).await {
                Ok(()) => Ok(json!({ "job_id": job_id, "status": "cancelled" })),
                Err(e) => Err(McpToolError::internal(format!(
                    "Cancellation failed: {}",
                    e
                ))),
            }
        })
        .await
    }
}
