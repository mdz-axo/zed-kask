//! Reduct cloud credential ingress and an explicitly experimental read-only probe.
use crate::*;

// Pipedream's public Reduct connector shows this exact project-read URL as an
// API proxy target. It is not a substitute for Reduct's login-gated API spec.
const PROJECT_PROBE_URL: &str = "https://app.reduct.video/api/v3/project";

fn classify_project_probe_status(status: reqwest::StatusCode) -> Result<(), McpToolError> {
    match status.as_u16() {
        200 => Ok(()),
        401 | 403 => Err(McpToolError::permission_denied(format!(
            "Reduct project probe returned HTTP {status}; check account API access and the documented authentication contract. No media was uploaded."
        ))),
        404 => Err(McpToolError::not_found(
            "Reduct project probe returned HTTP 404; the third-party endpoint may be unavailable. No cloud capability has been verified.",
        )),
        429 => Err(McpToolError::rate_limited(
            "Reduct project probe was rate-limited (HTTP 429).",
        )),
        code if (300..400).contains(&code) => Err(McpToolError::failed_precondition(format!(
            "Reduct project probe returned HTTP {status} redirect; refusing to forward the API key."
        ))),
        code if (500..600).contains(&code) => Err(McpToolError::unavailable(format!(
            "Reduct project probe returned HTTP {status}."
        ))),
        _ => Err(McpToolError::failed_precondition(format!(
            "Reduct project probe returned unexpected HTTP {status}; no cloud capability has been verified."
        ))),
    }
}

async fn probe_project(key: Option<&str>) -> Result<serde_json::Value, McpToolError> {
    connection_status(key)?;
    let key = key.ok_or_else(|| McpToolError::permission_denied("REDUCT_API_KEY is missing"))?;
    let header = reqwest::header::HeaderValue::from_str(key).map_err(|_| {
        McpToolError::invalid_argument("REDUCT_API_KEY contains invalid HTTP header characters")
    })?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| McpToolError::internal("Could not initialize Reduct HTTP client"))?;
    let response = client
        .get(PROJECT_PROBE_URL)
        .header("x-auth-key", header)
        .send()
        .await
        .map_err(|error| {
            McpToolError::unavailable(format!(
                "Reduct project probe transport failed: {}",
                error.without_url()
            ))
        })?;
    let status = response.status();
    classify_project_probe_status(status)?;
    // Never return or log the response body: this check establishes only that
    // the project-read request succeeded, not what projects the account owns.
    Ok(serde_json::json!({
        "probe": "read_only_project_endpoint",
        "http_status": status.as_u16(),
        "provider_connection": "project_read_succeeded",
        "cloud_operations": "not_yet_available",
        "evidence": "Pipedream public Reduct connector documents the project-read URL; authentication header is still being verified against Reduct."
    }))
}

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

    #[tool(
        description = "Experimental read-only connection probe: GET Reduct's project endpoint (public Pipedream example), with the stored key. Returns only HTTP outcome, never project data or the key. Does not verify upload, transcript, or edit operations."
    )]
    pub async fn reduct_connection_probe(&self) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_connection_probe", async {
            probe_project(self.reduct_api_key.as_deref()).await
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

    #[tokio::test]
    async fn project_probe_missing_key_never_dispatches_network_request() {
        let error = probe_project(None)
            .await
            .expect_err("missing key must fail locally");
        assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
    }

    #[test]
    fn project_probe_classifies_provider_boundaries_without_echoing_credentials() {
        for (status, expected_kind) in [
            (401, hkask_types::McpErrorKind::PermissionDenied),
            (403, hkask_types::McpErrorKind::PermissionDenied),
            (404, hkask_types::McpErrorKind::NotFound),
            (429, hkask_types::McpErrorKind::RateLimited),
            (500, hkask_types::McpErrorKind::Unavailable),
            (302, hkask_types::McpErrorKind::FailedPrecondition),
        ] {
            let error = classify_project_probe_status(
                reqwest::StatusCode::from_u16(status).expect("valid test status"),
            )
            .expect_err("non-success should be visible");
            assert_eq!(error.kind, expected_kind, "HTTP {status}");
            assert!(!error.message.contains("test-secret-do-not-echo"));
        }
        assert!(classify_project_probe_status(reqwest::StatusCode::OK).is_ok());
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
