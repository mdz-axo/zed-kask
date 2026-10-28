//! hkask-mcp-prediction-markets — binary entrypoint.
//!
//! Thin wrapper around the prediction-markets server library.

#[tokio::main]
async fn main() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_prediction_markets::run().await
}
