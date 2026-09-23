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

async fn project_response(key: Option<&str>, url: &str) -> Result<reqwest::Response, McpToolError> {
    connection_status(key)?;
    let key = key.ok_or_else(|| McpToolError::permission_denied("REDUCT_API_KEY is missing"))?;
    let header = reqwest::header::HeaderValue::from_str(key).map_err(|_| {
        McpToolError::invalid_argument("REDUCT_API_KEY contains invalid HTTP header characters")
    })?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect_policy(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| McpToolError::internal("Could not initialize Reduct HTTP client"))?;
    let response = client
        .get(url)
        .header("x-auth-key", header)
        .send()
        .await
        .map_err(|error| {
            McpToolError::unavailable(format!(
                "Reduct project probe transport failed: {}",
                error.without_url()
            ))
        })?;
    classify_project_probe_status(response.status())?;
    Ok(response)
}

async fn probe_project(key: Option<&str>, url: &str) -> Result<serde_json::Value, McpToolError> {
    let response = project_response(key, url).await?;
    let status = response.status();
    // Never return or log the response body: this check establishes only that
    // the project-read request succeeded, not what projects the account owns.
    Ok(serde_json::json!({
        "probe": "read_only_project_endpoint",
        "http_status": status.as_u16(),
        "provider_connection": "project_read_succeeded",
        "cloud_editing": "not_available",
        "evidence": "Project-read URL is shown by Pipedream and was verified against Reduct; no editing API contract is available."
    }))
}

fn parse_project_snapshot(body: &[u8], limit: usize) -> Result<serde_json::Value, McpToolError> {
    if !(1..=100).contains(&limit) {
        return Err(McpToolError::invalid_argument(
            "limit must be between 1 and 100",
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct project response was not valid JSON")
    })?;
    let projects = response
        .get("project")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            McpToolError::failed_precondition(
                "Reduct project response lacks the observed project map; no results returned",
            )
        })?;
    let mut ordered: Vec<_> = projects.iter().collect();
    ordered.sort_by(|(left, _), (right, _)| left.cmp(right));
    let selected = ordered
        .into_iter()
        .take(limit)
        .map(|(id, project)| {
            let title = project
                .get("title")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    McpToolError::failed_precondition(
                        "Reduct project response contains a project without a title",
                    )
                })?;
            Ok(serde_json::json!({"id": id, "title": title}))
        })
        .collect::<Result<Vec<_>, McpToolError>>()?;
    Ok(serde_json::json!({
        "provider_returned_count": projects.len(),
        "returned_count": selected.len(),
        "truncated": selected.len() < projects.len(),
        "pagination": "unknown",
        "projects": selected,
        "cloud_editing": "not_available"
    }))
}

async fn projects_snapshot(
    key: Option<&str>,
    limit: usize,
    url: &str,
) -> Result<serde_json::Value, McpToolError> {
    if !(1..=100).contains(&limit) {
        return Err(McpToolError::invalid_argument(
            "limit must be between 1 and 100",
        ));
    }
    let mut response = project_response(key, url).await?;
    const MAX_BYTES: usize = 2 * 1024 * 1024;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        McpToolError::unavailable(format!(
            "Reduct project response read failed: {}",
            error.without_url()
        ))
    })? {
        if chunk.len() > MAX_BYTES.saturating_sub(body.len()) {
            return Err(McpToolError::failed_precondition(
                "Reduct project response exceeds the 2 MiB limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    parse_project_snapshot(&body, limit)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductProjectsRequest {
    /// Maximum projects to return from the provider response (1–100). This
    /// does not request server-side pagination; that contract is unknown.
    limit: usize,
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
        "cloud_editing": "not_available",
        "note": "The API key reached the media MCP child; no Reduct request has been made. Project reads are available separately."
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
            probe_project(self.reduct_api_key.as_deref(), PROJECT_PROBE_URL).await
        })
        .await
    }

    #[tool(
        description = "List up to limit (1–100) Reduct project IDs and titles from the live project-read endpoint. Returns only provider-supplied subset; server-side pagination is unverified. Does not upload or edit media."
    )]
    pub async fn reduct_projects_snapshot(
        &self,
        Parameters(ReductProjectsRequest { limit }): Parameters<ReductProjectsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_projects_snapshot", async {
            projects_snapshot(self.reduct_api_key.as_deref(), limit, PROJECT_PROBE_URL).await
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
    fn project_snapshot_projects_only_ids_and_titles() -> Result<(), McpToolError> {
        let body = br#"{"project":{"p2":{"title":"Second","member":["private"],"description":"secret"},"p1":{"title":"First","member":["private"]}}}"#;
        let snapshot = parse_project_snapshot(body, 1)?;
        assert_eq!(snapshot["returned_count"], 1);
        assert_eq!(snapshot["provider_returned_count"], 2);
        assert_eq!(snapshot["truncated"], true);
        assert_eq!(snapshot["projects"][0]["id"], "p1");
        assert_eq!(snapshot["projects"][0]["title"], "First");
        assert!(!snapshot.to_string().contains("private"));
        assert!(!snapshot.to_string().contains("secret"));
        assert_eq!(snapshot["pagination"], "unknown");
        assert!(parse_project_snapshot(br#"{"other": []}"#, 1).is_err());
        assert!(parse_project_snapshot(br#"{"project":{"p1":{}}}"#, 1).is_err());
        assert!(parse_project_snapshot(body, 0).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn project_probe_missing_key_never_dispatches_network_request() {
        let error = probe_project(None, PROJECT_PROBE_URL)
            .await
            .expect_err("missing key must fail locally");
        assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
    }

    // Explicitly opt in from the operator's workstation after configuring
    // Settings → Kask → Data Services. Never prints the key or project body.
    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_project_probe_with_stored_key() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 to authorize this read-only probe".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let status = probe_project(Some(key.as_str()), PROJECT_PROBE_URL).await?;
        assert_eq!(status["provider_connection"], "project_read_succeeded");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_project_response_shape_with_stored_key() -> Result<(), Box<dyn std::error::Error>>
    {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 for this read-only check".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let header = reqwest::header::HeaderValue::from_str(key.as_str())
            .map_err(|_| std::io::Error::other("invalid key header"))?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect_policy(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let response = client
            .get(PROJECT_PROBE_URL)
            .header("x-auth-key", header)
            .send()
            .await?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            return Err(format!("Reduct project read returned HTTP {status}").into());
        }
        let body = response.bytes().await?;
        if body.len() > 1024 * 1024 {
            return Err("Project response exceeds the one-MiB inspection cap".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        let (shape, count, first_has_id, first_has_name) = match &value {
            serde_json::Value::Array(items) => (
                "array",
                items.len(),
                items.first().and_then(|item| item.get("id")).is_some(),
                items.first().and_then(|item| item.get("name")).is_some(),
            ),
            serde_json::Value::Object(fields) => (
                "object",
                fields.len(),
                fields.get("id").is_some(),
                fields.get("name").is_some(),
            ),
            _ => ("other", 0, false, false),
        };
        // Counts and field-presence only: never project names, IDs, or content.
        eprintln!(
            "Reduct project response shape={shape}; count={count}; first_has_id={first_has_id}; first_has_name={first_has_name}"
        );
        for field in ["projects", "project", "data", "results", "items", "status"] {
            if let Some(value) = value.get(field) {
                let kind = if value.is_array() {
                    "array"
                } else if value.is_object() {
                    "object"
                } else {
                    "scalar"
                };
                let count = value
                    .as_array()
                    .map(Vec::len)
                    .or_else(|| value.as_object().map(serde_json::Map::len))
                    .unwrap_or(0);
                let has_id = value
                    .as_array()
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("id"))
                    .is_some();
                let has_named_fields = ["id", "name", "description", "recordings", "items"]
                    .into_iter()
                    .filter(|candidate| value.get(*candidate).is_some())
                    .count();
                eprintln!(
                    "Reduct response field={field}; type={kind}; count={count}; first_has_id={has_id}; known_fields={has_named_fields}"
                );
                if field == "project" {
                    let first_value = value.as_object().and_then(|fields| fields.values().next());
                    eprintln!(
                        "Reduct first project entry: object={}; fields={}; has_id={}; has_name={}",
                        first_value.is_some_and(serde_json::Value::is_object),
                        first_value
                            .and_then(serde_json::Value::as_object)
                            .map(serde_json::Map::len)
                            .unwrap_or(0),
                        first_value.and_then(|entry| entry.get("id")).is_some(),
                        first_value.and_then(|entry| entry.get("name")).is_some()
                    );
                    let field_names: Vec<&str> = first_value
                        .and_then(serde_json::Value::as_object)
                        .into_iter()
                        .flat_map(|entry| entry.keys().map(String::as_str))
                        .filter(|name| {
                            name.len() <= 32
                                && name
                                    .chars()
                                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                        })
                        .collect();
                    eprintln!("Reduct project entry field names (not values): {field_names:?}");
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_projects_snapshot_with_stored_key() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 for this read-only check".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let snapshot = projects_snapshot(Some(key.as_str()), 10, PROJECT_PROBE_URL).await?;
        assert!(
            snapshot["returned_count"]
                .as_u64()
                .is_some_and(|count| count <= 10)
        );
        assert!(snapshot["provider_returned_count"].as_u64().is_some());
        assert_eq!(snapshot["cloud_editing"], "not_available");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_api_reference_access_with_stored_key() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 for this read-only check".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let header = reqwest::header::HeaderValue::from_str(key.as_str())
            .map_err(|_| std::io::Error::other("invalid key header"))?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect_policy(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        let response = client
            .get("https://app.reduct.video/backstage/api/")
            .header("x-auth-key", header)
            .send()
            .await?;
        let status = response.status();
        let is_login_page = if status.is_success() {
            response.text().await?.contains("Log in to Reduct")
        } else {
            false
        };
        // Do not print the response body or any account identifiers.
        eprintln!("Reduct API reference: HTTP {status}; login_page={is_login_page}");
        Ok(())
    }

    #[tokio::test]
    async fn project_probe_sends_key_only_in_header_and_discards_private_body()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => return Err(error),
                }
            };
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut bytes = [0_u8; 4096];
            let count = stream.read(&mut bytes)?;
            let request = String::from_utf8_lossy(&bytes[..count]).to_string();
            let body = r#"{"private_project":"do-not-return"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(request)
        });
        let status = probe_project(
            Some("fixture-secret-do-not-echo"),
            &format!("http://{address}/api/v3/project"),
        )
        .await?;
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("probe test server thread panicked"))??;
        assert!(request.starts_with("GET /api/v3/project HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-secret-do-not-echo")
        );
        assert!(!status.to_string().contains("fixture-secret-do-not-echo"));
        assert!(!status.to_string().contains("do-not-return"));
        assert_eq!(status["provider_connection"], "project_read_succeeded");
        assert_eq!(status["cloud_editing"], "not_available");
        Ok(())
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
        assert_eq!(status["cloud_editing"], "not_available");
        Ok(())
    }
}
