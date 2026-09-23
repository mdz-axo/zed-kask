//! Reduct cloud credential ingress. No provider request is implied by this check.
use crate::*;

fn connection_status(key: Option<&str>) -> Result<serde_json::Value, McpToolError> {
    if key.is_none_or(|key| key.trim().is_empty()) {
        return Err(McpToolError::permission_denied(
            "REDUCT_API_KEY is not configured. Add it in Settings → Kask → Data Services (Reduct.video).",
        ));
    }
    Ok(serde_json::json!({
        "credential": "configured",
        "provider_connection": "not_checked",
        "cloud_operations": "not_yet_available",
        "note": "The API key reached the media MCP child; no Reduct request has been made."
    }))
}

#[tool_router(router = reduct_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Check whether the Reduct.video API key reached the media server. Does not contact Reduct or validate the key; cloud editing operations remain unavailable until their API contracts are verified. Never returns the key."
    )]
    pub async fn reduct_connection_status(&self) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_connection_status", async {
            connection_status(self.reduct_api_key.as_deref())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_permission_denied_not_local_fallback() {
        let error = connection_status(None).expect_err("missing key must be visible");
        assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
        assert!(error.message.contains("REDUCT_API_KEY"));
        assert!(connection_status(Some("  ")).is_err());
    }

    #[test]
    fn configured_key_is_never_returned_or_misrepresented_as_connected() -> Result<(), McpToolError>
    {
        let status = connection_status(Some("test-secret-do-not-echo"))?;
        let encoded = status.to_string();
        assert!(!encoded.contains("test-secret-do-not-echo"));
        assert_eq!(status["provider_connection"], "not_checked");
        assert_eq!(status["cloud_operations"], "not_yet_available");
        Ok(())
    }
}
