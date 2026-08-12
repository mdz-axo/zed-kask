use crate::TrainingServer;
use crate::tools::error_mapping::map_host_provider_error;
use crate::types::TrainCancelRequest;
use hkask_mcp_server::server::execute_tool;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde_json::json;

#[tool_router(router = cancel_router, vis = "pub")]
impl TrainingServer {
    #[tool(description = "Cancel a running or queued training job.")]
    pub async fn training_cancel(
        &self,
        Parameters(TrainCancelRequest { job_id }): Parameters<TrainCancelRequest>,
    ) -> String {
        execute_tool(self, "training_cancel", async {
            match self.host.cancel(&job_id).await {
                Ok(()) => Ok(json!({ "job_id": job_id, "status": "cancelled" })),
                Err(e) => Err(map_host_provider_error(e)),
            }
        })
        .await
    }
}
