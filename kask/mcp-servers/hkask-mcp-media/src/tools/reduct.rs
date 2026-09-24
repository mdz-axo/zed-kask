//! Reduct cloud API tools. Local educt remains a separate implementation.
use crate::*;

// The operator-provided Reduct v3 API introduction pins the API root and
// X-Auth-Key; Pipedream's public connector pins the project-read URL.
const API_ROOT: &str = "https://app.reduct.video/api/v3/";
const PROJECT_PROBE_URL: &str = "https://app.reduct.video/api/v3/project";

fn validate_reduct_id(name: &str, id: &str) -> Result<(), McpToolError> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must be a Reduct ID of 1–128 ASCII letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

fn project_detail_url(project_id: &str) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    Ok(format!("{API_ROOT}project/{project_id}"))
}

fn reel_detail_url(project_id: &str, reel_id: &str) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    validate_reduct_id("reel_id", reel_id)?;
    Ok(format!("{API_ROOT}project/{project_id}/reel/{reel_id}"))
}

fn reel_create_url(root: &str, project_id: &str) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    Ok(format!("{root}project/{project_id}/reel"))
}

fn reel_block_url(
    root: &str,
    project_id: &str,
    reel_id: &str,
    block_id: Option<&str>,
) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    validate_reduct_id("reel_id", reel_id)?;
    let mut url = format!("{root}project/{project_id}/reel/{reel_id}/block");
    if let Some(block_id) = block_id {
        validate_reduct_id("block_id", block_id)?;
        url.push('/');
        url.push_str(block_id);
    }
    Ok(url)
}

fn recording_read_url(
    project_id: &str,
    recording_id: &str,
    leaf: &str,
) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    validate_reduct_id("recording_id", recording_id)?;
    if !matches!(
        leaf,
        "status" | "transcript.json" | "transcript.txt" | "highlight"
    ) {
        return Err(McpToolError::invalid_argument(
            "unsupported recording read; use status, transcript.json, transcript.txt, or highlight",
        ));
    }
    Ok(format!(
        "{API_ROOT}project/{project_id}/recording/{recording_id}/{leaf}"
    ))
}

fn recording_create_url(project_id: &str) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    Ok(format!("{API_ROOT}project/{project_id}/recording"))
}

fn media_import_url(project_id: &str, recording_id: &str) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    validate_reduct_id("recording_id", recording_id)?;
    Ok(format!(
        "{API_ROOT}project/{project_id}/recording/{recording_id}/media-import"
    ))
}

fn media_upload_url(
    project_id: &str,
    recording_id: &str,
    filename: &str,
) -> Result<String, McpToolError> {
    validate_reduct_id("project_id", project_id)?;
    validate_reduct_id("recording_id", recording_id)?;
    if filename.is_empty()
        || filename.len() > 255
        || filename
            .chars()
            .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
    {
        return Err(McpToolError::invalid_argument(
            "media upload filename must be 1–255 bytes without path separators or control characters",
        ));
    }
    let mut url = reqwest::Url::parse(&format!(
        "{API_ROOT}project/{project_id}/recording/{recording_id}/media-upload"
    ))
    .map_err(|_| McpToolError::internal("Reduct upload URL is malformed"))?;
    url.query_pairs_mut().append_pair("filename", filename);
    Ok(url.into())
}

fn validate_indexed_upload_bytes(
    expected_hash: &str,
    expected_len: u64,
    bytes: &[u8],
) -> Result<(), McpToolError> {
    use sha2::Digest;
    if bytes.len() as u64 != expected_len
        || format!("{:x}", sha2::Sha256::digest(bytes)) != expected_hash
    {
        return Err(McpToolError::failed_precondition(
            "Indexed media bytes changed; re-index before cloud upload",
        ));
    }
    Ok(())
}

fn parse_reel_create_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct reel creation may have succeeded but returned invalid JSON; inspect reels before retrying")
    })?;
    let id = response.get("reel").and_then(serde_json::Value::as_str).ok_or_else(|| {
        McpToolError::failed_precondition("Reduct reel creation may have succeeded but omitted reel ID; inspect reels before retrying")
    })?;
    validate_reduct_id("reel_id", id).map_err(|_| {
        McpToolError::failed_precondition(
            "Reduct reel creation returned an unusable ID; inspect reels before retrying",
        )
    })?;
    Ok(
        serde_json::json!({"source": "reduct_cloud", "reel_id": id, "state": "submitted; inspect reel detail"}),
    )
}

fn parse_reel_block_create_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct block creation may have succeeded but returned invalid JSON; inspect reel before retrying")
    })?;
    let id = response.get("block").and_then(serde_json::Value::as_str).ok_or_else(|| {
        McpToolError::failed_precondition("Reduct block creation may have succeeded but omitted block ID; inspect reel before retrying")
    })?;
    validate_reduct_id("block_id", id).map_err(|_| {
        McpToolError::failed_precondition(
            "Reduct block creation returned an unusable ID; inspect reel before retrying",
        )
    })?;
    Ok(
        serde_json::json!({"source": "reduct_cloud", "block_id": id, "state": "submitted; inspect reel detail"}),
    )
}

fn parse_reel_block_edit_response(
    body: &[u8],
    block_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct block edit may have succeeded but returned invalid JSON; inspect reel before retrying")
    })?;
    if response
        .get(block_id)
        .is_none_or(serde_json::Value::is_null)
    {
        return Err(McpToolError::failed_precondition(
            "Reduct block edit may have succeeded but omitted the block acknowledgement; inspect reel before retrying",
        ));
    }
    Ok(
        serde_json::json!({"source": "reduct_cloud", "block_id": block_id, "state": "submitted; inspect reel detail"}),
    )
}

fn parse_media_upload_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct media upload may have succeeded but returned invalid JSON; inspect before retrying")
    })?;
    let id = response.get("media_id").and_then(serde_json::Value::as_str).ok_or_else(|| {
        McpToolError::failed_precondition("Reduct media upload may have succeeded but omitted media_id; inspect before retrying")
    })?;
    validate_reduct_id("media_id", id).map_err(|_| {
        McpToolError::failed_precondition("Reduct media upload may have succeeded but returned an unusable media ID; inspect before retrying")
    })?;
    Ok(
        serde_json::json!({"source": "reduct_cloud", "media_id": id, "upload_state": "submitted; check recording status"}),
    )
}

fn parse_recording_create_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct recording creation may have succeeded but returned invalid JSON; inspect before retrying")
    })?;
    let id = response.get("recording").and_then(serde_json::Value::as_str).ok_or_else(|| {
        McpToolError::failed_precondition("Reduct recording creation may have succeeded but omitted recording ID; inspect before retrying")
    })?;
    validate_reduct_id("recording_id", id).map_err(|_| {
        McpToolError::failed_precondition(
            "Reduct recording creation returned an unusable ID; inspect before retrying",
        )
    })?;
    Ok(serde_json::json!({"source": "reduct_cloud", "recording_id": id}))
}

fn parse_media_import_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct media import may have succeeded but returned invalid JSON; inspect before retrying")
    })?;
    let media_ids = response.get("media_ids").and_then(serde_json::Value::as_array).ok_or_else(|| {
        McpToolError::failed_precondition("Reduct media import may have succeeded but omitted media_ids; inspect before retrying")
    })?;
    if media_ids.is_empty() {
        return Err(McpToolError::failed_precondition(
            "Reduct media import returned no media IDs; inspect the recording before retrying",
        ));
    }
    for id in media_ids {
        let id = id.as_str().ok_or_else(|| {
            McpToolError::failed_precondition("Reduct media import may have succeeded but returned an unusable media ID; inspect before retrying")
        })?;
        validate_reduct_id("media_id", id).map_err(|_| {
            McpToolError::failed_precondition("Reduct media import may have succeeded but returned an unusable media ID; inspect before retrying")
        })?;
    }
    Ok(
        serde_json::json!({"source": "reduct_cloud", "media_ids": media_ids, "import_state": "submitted; check recording status"}),
    )
}

fn parse_transcript_body(body: &[u8], format: &str) -> Result<serde_json::Value, McpToolError> {
    let content = match format {
        "json" => serde_json::from_slice(body).map_err(|_| {
            McpToolError::failed_precondition("Reduct returned invalid transcript JSON")
        })?,
        "txt" => serde_json::Value::String(
            std::str::from_utf8(body)
                .map_err(|_| {
                    McpToolError::failed_precondition("Reduct returned non-UTF-8 transcript text")
                })?
                .to_string(),
        ),
        _ => {
            return Err(McpToolError::invalid_argument(
                "transcript format must be json or txt",
            ));
        }
    };
    Ok(serde_json::json!({"source": "reduct_cloud", "format": format, "content": content}))
}

fn classify_reduct_status(
    status: reqwest::StatusCode,
    operation: &str,
) -> Result<(), McpToolError> {
    match status.as_u16() {
        200 => Ok(()),
        400 => Err(McpToolError::invalid_argument(format!(
            "Reduct {operation} returned HTTP 400 (invalid path or schema); inspect provider state before retrying a mutation."
        ))),
        401 | 403 => Err(McpToolError::permission_denied(format!(
            "Reduct {operation} returned HTTP {status}; check REDUCT_API_KEY or workspace API access."
        ))),
        404 => Err(McpToolError::not_found(format!(
            "Reduct {operation} returned HTTP 404; resource or endpoint not found."
        ))),
        429 => Err(McpToolError::rate_limited(format!(
            "Reduct {operation} returned HTTP 429."
        ))),
        code if (300..400).contains(&code) => Err(McpToolError::failed_precondition(format!(
            "Reduct {operation} returned HTTP {status} redirect; refusing to forward the API key."
        ))),
        code if (500..600).contains(&code) => Err(McpToolError::unavailable(format!(
            "Reduct {operation} returned HTTP {status}."
        ))),
        _ => Err(McpToolError::failed_precondition(format!(
            "Reduct {operation} returned unexpected HTTP {status}; no success claimed."
        ))),
    }
}

/// A provider error after POST is not proof the mutation was rolled back.
fn classify_reduct_mutation_status(
    status: reqwest::StatusCode,
    operation: &str,
) -> Result<(), McpToolError> {
    match status.as_u16() {
        429 => Err(McpToolError::rate_limited(format!(
            "Reduct {operation} returned HTTP 429; request may have succeeded; inspect before retrying"
        ))),
        code if (500..600).contains(&code) => Err(McpToolError::unavailable(format!(
            "Reduct {operation} returned HTTP {status}; request may have succeeded; inspect before retrying"
        ))),
        _ => classify_reduct_status(status, operation),
    }
}

fn authorized_client(
    key: Option<&str>,
) -> Result<(reqwest::Client, reqwest::header::HeaderValue), McpToolError> {
    authorized_client_with_timeout(key, std::time::Duration::from_secs(15))
}

fn authorized_client_with_timeout(
    key: Option<&str>,
    timeout: std::time::Duration,
) -> Result<(reqwest::Client, reqwest::header::HeaderValue), McpToolError> {
    connection_status(key)?;
    let key = key.ok_or_else(|| McpToolError::permission_denied("REDUCT_API_KEY is missing"))?;
    let header = reqwest::header::HeaderValue::from_str(key).map_err(|_| {
        McpToolError::invalid_argument("REDUCT_API_KEY contains invalid HTTP header characters")
    })?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect_policy(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| McpToolError::internal("Could not initialize Reduct HTTP client"))?;
    Ok((client, header))
}

async fn read_response(
    key: Option<&str>,
    url: &str,
    operation: &str,
) -> Result<reqwest::Response, McpToolError> {
    let (client, header) = authorized_client(key)?;
    let response = client
        .get(url)
        .header("x-auth-key", header)
        .send()
        .await
        .map_err(|error| {
            McpToolError::unavailable(format!(
                "Reduct {operation} transport failed: {}",
                error.without_url()
            ))
        })?;
    classify_reduct_status(response.status(), operation)?;
    Ok(response)
}

async fn post_json_response(
    key: Option<&str>,
    url: &str,
    payload: serde_json::Value,
    operation: &str,
) -> Result<reqwest::Response, McpToolError> {
    let (client, header) = authorized_client(key)?;
    let response = client.post(url).header("x-auth-key", header).json(&payload)
        .send().await.map_err(|error| McpToolError::unavailable(format!(
            "Reduct {operation} transport failed: {}; request may have succeeded; inspect before retrying",
            error.without_url()
        )))?;
    if !matches!(response.status().as_u16(), 200 | 201) {
        classify_reduct_mutation_status(response.status(), operation)?;
        return Err(McpToolError::failed_precondition(format!(
            "Reduct {operation} returned HTTP {}; inspect before retrying",
            response.status()
        )));
    }
    Ok(response)
}

async fn post_binary_response(
    key: Option<&str>,
    url: &str,
    body: reqwest::Body,
    length: u64,
    operation: &str,
) -> Result<reqwest::Response, McpToolError> {
    let (client, header) =
        authorized_client_with_timeout(key, std::time::Duration::from_secs(300))?;
    let response = client.post(url)
        .header("x-auth-key", header)
        .header(reqwest::header::CONTENT_LENGTH, length)
        .body(body).send().await
        .map_err(|error| McpToolError::unavailable(format!(
            "Reduct {operation} transport failed: {}; upload may have succeeded; inspect before retrying",
            error.without_url()
        )))?;
    if !matches!(response.status().as_u16(), 200 | 201) {
        classify_reduct_mutation_status(response.status(), operation)?;
        return Err(McpToolError::failed_precondition(format!(
            "Reduct {operation} returned HTTP {}; inspect before retrying",
            response.status()
        )));
    }
    Ok(response)
}

async fn read_upload_ack(response: reqwest::Response) -> Result<serde_json::Value, McpToolError> {
    let body = read_bounded(response, 64 * 1024).await.map_err(|error| {
        McpToolError::failed_precondition(format!(
            "Reduct upload may have succeeded, but its acknowledgement was unreadable: {error}"
        ))
    })?;
    parse_media_upload_response(&body)
}

async fn upload_local_media(
    key: Option<&str>,
    url: &str,
    path: &std::path::Path,
) -> Result<serde_json::Value, McpToolError> {
    if !path.is_absolute() {
        return Err(McpToolError::invalid_argument(
            "Reduct local upload requires an absolute file path",
        ));
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| McpToolError::not_found("Local media file cannot be opened"))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|_| McpToolError::unavailable("Local media file metadata cannot be read"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(McpToolError::invalid_argument(
            "Reduct local upload requires a nonempty regular file",
        ));
    }
    let probe = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
            "-i",
        ])
        .arg(path)
        .output()
        .await
        .map_err(|_| {
            McpToolError::unavailable("ffprobe is required to check local media before upload")
        })?;
    if !probe.status.success()
        || !probe.stdout.split(|byte| *byte == b'\n').any(|line| {
            matches!(
                line.strip_suffix(b"\r").unwrap_or(line),
                b"audio" | b"video"
            )
        })
    {
        return Err(McpToolError::invalid_argument(
            "Local upload requires a decodable audio or video stream; Reduct determines final format support",
        ));
    }
    let body = reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(file));
    let response = post_binary_response(key, url, body, metadata.len(), "media upload").await?;
    read_upload_ack(response).await
}

async fn probe_project(key: Option<&str>, url: &str) -> Result<serde_json::Value, McpToolError> {
    let response = read_response(key, url, "project read").await?;
    let status = response.status();
    // Never return or log the response body: this check establishes only that
    // the project-read request succeeded, not what projects the account owns.
    Ok(serde_json::json!({
        "probe": "read_only_project_endpoint",
        "http_status": status.as_u16(),
        "provider_connection": "project_read_succeeded",
        "cloud_editing": "not_checked",
        "evidence": "The project-read request succeeded; this call does not create or edit a Reel."
    }))
}

async fn read_bounded(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, McpToolError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        McpToolError::unavailable(format!(
            "Reduct response read failed: {}",
            error.without_url()
        ))
    })? {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(McpToolError::failed_precondition(format!(
                "Reduct response exceeds the {max_bytes}-byte limit; no partial data returned"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
    ordered.sort_by_key(|(id, _)| *id);
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
        "cloud_editing": "not_checked"
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
    let response = read_response(key, url, "project read").await?;
    let body = read_bounded(response, 2 * 1024 * 1024).await?;
    parse_project_snapshot(&body, limit)
}

fn parse_project_titled_collection(
    body: &[u8],
    project_id: &str,
    collection: &str,
    limit: usize,
) -> Result<serde_json::Value, McpToolError> {
    if !(1..=100).contains(&limit) {
        return Err(McpToolError::invalid_argument(
            "limit must be between 1 and 100",
        ));
    }
    if !matches!(collection, "recordings" | "reels") {
        return Err(McpToolError::invalid_argument(
            "unsupported project collection",
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct project detail was not valid JSON")
    })?;
    let records = response
        .get(project_id)
        .and_then(|project| project.get(collection))
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            McpToolError::failed_precondition(format!(
                "Reduct project detail lacks the observed {collection} map"
            ))
        })?;
    let mut ordered: Vec<_> = records.iter().collect();
    ordered.sort_by_key(|(id, _)| *id);
    let selected = ordered
        .into_iter()
        .take(limit)
        .map(|(id, record)| {
            let title = record
                .get("title")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    McpToolError::failed_precondition(format!(
                        "Reduct {collection} entry lacks a title"
                    ))
                })?;
            Ok(serde_json::json!({"id": id, "title": title}))
        })
        .collect::<Result<Vec<_>, McpToolError>>()?;
    let mut result = serde_json::json!({
        "source": "reduct_cloud", "project_id": project_id,
        "provider_returned_count": records.len(), "returned_count": selected.len(),
        "truncated": selected.len() < records.len(), "pagination": "unknown"
    });
    result[collection] = serde_json::Value::Array(selected);
    Ok(result)
}

async fn project_titled_snapshot(
    key: Option<&str>,
    project_id: &str,
    collection: &str,
    limit: usize,
) -> Result<serde_json::Value, McpToolError> {
    if !(1..=100).contains(&limit) || !matches!(collection, "recordings" | "reels") {
        return Err(McpToolError::invalid_argument(
            "collection must be recordings or reels and limit between 1 and 100",
        ));
    }
    let url = project_detail_url(project_id)?;
    let response = read_response(key, &url, "project detail").await?;
    let body = read_bounded(response, 2 * 1024 * 1024).await?;
    parse_project_titled_collection(&body, project_id, collection, limit)
}

fn redact_share_tokens(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            fields.remove("share_token");
            for child in fields.values_mut() {
                redact_share_tokens(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_share_tokens(item);
            }
        }
        _ => {}
    }
}

fn parse_reel_detail(body: &[u8], reel_id: &str) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| McpToolError::failed_precondition("Reduct reel detail was not valid JSON"))?;
    let reel = response
        .get(reel_id)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            McpToolError::failed_precondition("Reduct reel detail lacks the requested reel ID")
        })?;
    let title = reel
        .get("title")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| McpToolError::failed_precondition("Reduct reel detail lacks a title"))?;
    let mut blocks = match reel.get("block") {
        Some(serde_json::Value::Object(blocks)) => serde_json::Value::Object(blocks.clone()),
        None => serde_json::Value::Null,
        Some(_) => {
            return Err(McpToolError::failed_precondition(
                "Reduct reel block field has an unexpected shape",
            ));
        }
    };
    redact_share_tokens(&mut blocks);
    // The v3 Reel GET contract omits `publish`; only a nonempty share token
    // establishes publication. Never return the token itself.
    let publication_state = match reel.get("share_token") {
        None => "unpublished",
        Some(serde_json::Value::String(token)) if !token.is_empty() => "published",
        Some(_) => "undetermined",
    };
    Ok(serde_json::json!({
        "source": "reduct_cloud", "reel_id": reel_id, "title": title,
        "block_state": if blocks.is_null() { "not_present_in_provider_response" } else { "provider_map" },
        "blocks": blocks, "share_tokens": "redacted", "publication_state": publication_state
    }))
}

async fn reel_detail(
    key: Option<&str>,
    project_id: &str,
    reel_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = reel_detail_url(project_id, reel_id)?;
    reel_detail_at(key, &url, reel_id).await
}

async fn reel_detail_at(
    key: Option<&str>,
    url: &str,
    reel_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let response = read_response(key, url, "reel detail").await?;
    let body = read_bounded(response, 4 * 1024 * 1024).await?;
    parse_reel_detail(&body, reel_id)
}

fn validate_reel_text(value: &str, name: &str) -> Result<(), McpToolError> {
    if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must be nonempty, at most 512 bytes, and contain no control characters"
        )));
    }
    Ok(())
}

fn validate_reel_range(order: f64, start: f64, end: f64) -> Result<(), McpToolError> {
    if !order.is_finite() || !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
        return Err(McpToolError::invalid_argument(
            "reel block order must be finite and start/end must be finite, nonnegative, increasing seconds",
        ));
    }
    Ok(())
}

async fn reel_post_body(
    key: Option<&str>,
    url: &str,
    payload: serde_json::Value,
    operation: &str,
) -> Result<Vec<u8>, McpToolError> {
    let response = post_json_response(key, url, payload, operation).await?;
    read_bounded(response, 64 * 1024).await.map_err(|error| {
        McpToolError::failed_precondition(format!(
            "Reduct {operation} may have succeeded, but its acknowledgement was unreadable: {error}; inspect before retrying"
        ))
    })
}

async fn create_reel(
    key: Option<&str>,
    root: &str,
    project_id: &str,
    title: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = reel_create_url(root, project_id)?;
    validate_reel_text(title, "reel title")?;
    let body = reel_post_body(
        key,
        &url,
        serde_json::json!({"title": title}),
        "reel creation",
    )
    .await?;
    parse_reel_create_response(&body)
}

async fn create_reel_clip(
    key: Option<&str>,
    root: &str,
    project_id: &str,
    reel_id: &str,
    recording_id: &str,
    order: f64,
    start: f64,
    end: f64,
) -> Result<serde_json::Value, McpToolError> {
    let url = reel_block_url(root, project_id, reel_id, None)?;
    validate_reduct_id("recording_id", recording_id)?;
    validate_reel_range(order, start, end)?;
    let body = reel_post_body(
        key,
        &url,
        serde_json::json!({
            "type": "doc-range", "order": order, "recording": recording_id,
            "start": start, "end": end, "strikethrough": {}
        }),
        "reel clip creation",
    )
    .await?;
    parse_reel_block_create_response(&body)
}

async fn create_reel_title(
    key: Option<&str>,
    root: &str,
    project_id: &str,
    reel_id: &str,
    order: f64,
    duration: f64,
    title: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = reel_block_url(root, project_id, reel_id, None)?;
    validate_reel_text(title, "title card text")?;
    if !order.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return Err(McpToolError::invalid_argument(
            "title card order must be finite and duration must be positive finite seconds",
        ));
    }
    let body = reel_post_body(
        key,
        &url,
        serde_json::json!({
            "type": "title", "order": order, "duration": duration, "title": title
        }),
        "reel title card creation",
    )
    .await?;
    parse_reel_block_create_response(&body)
}

async fn edit_reel_clip_range(
    key: Option<&str>,
    root: &str,
    project_id: &str,
    reel_id: &str,
    block_id: &str,
    start: f64,
    end: f64,
) -> Result<serde_json::Value, McpToolError> {
    let url = reel_block_url(root, project_id, reel_id, Some(block_id))?;
    validate_reel_range(0.0, start, end)?;
    let detail_url = format!("{root}project/{project_id}/reel/{reel_id}");
    let current = reel_detail_at(key, &detail_url, reel_id).await?;
    let block = current["blocks"].get(block_id).ok_or_else(|| {
        McpToolError::not_found("Reduct reel detail does not contain the requested block")
    })?;
    if block.get("type").and_then(serde_json::Value::as_str) != Some("doc-range") {
        return Err(McpToolError::failed_precondition(
            "Only doc-range blocks can have their clip range edited",
        ));
    }
    let body = reel_post_body(
        key,
        &url,
        serde_json::json!({"start": start, "end": end}),
        "reel clip edit",
    )
    .await?;
    parse_reel_block_edit_response(&body, block_id)
}

async fn create_recording(
    key: Option<&str>,
    project_id: &str,
    title: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = recording_create_url(project_id)?;
    if title.trim().is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
        return Err(McpToolError::invalid_argument(
            "recording title must be nonempty, at most 512 bytes, and contain no control characters",
        ));
    }
    let response = post_json_response(
        key,
        &url,
        serde_json::json!({"title": title}),
        "recording creation",
    )
    .await?;
    let body = read_bounded(response, 64 * 1024).await.map_err(|error| {
        McpToolError::failed_precondition(format!("Reduct recording creation may have succeeded, but its acknowledgement was unreadable: {error}"))
    })?;
    parse_recording_create_response(&body)
}

async fn import_media(
    key: Option<&str>,
    project_id: &str,
    recording_id: &str,
    source_url: &str,
) -> Result<serde_json::Value, McpToolError> {
    connection_status(key)?;
    let url = media_import_url(project_id, recording_id)?;
    if source_url.len() > 4096 {
        return Err(McpToolError::invalid_argument(
            "media import URL exceeds 4096 bytes",
        ));
    }
    validate_tool_url_with_dns(source_url).await?;
    let response = post_json_response(
        key,
        &url,
        serde_json::json!({"url": source_url}),
        "media import",
    )
    .await?;
    let body = read_bounded(response, 64 * 1024).await.map_err(|error| {
        McpToolError::failed_precondition(format!("Reduct media import may have succeeded, but its acknowledgement was unreadable: {error}"))
    })?;
    parse_media_import_response(&body)
}

fn parse_highlights_response(body: &[u8]) -> Result<serde_json::Value, McpToolError> {
    let response: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        McpToolError::failed_precondition("Reduct highlight response was not valid JSON")
    })?;
    let highlights = response
        .get("highlight")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            McpToolError::failed_precondition(
                "Reduct highlight response lacks the documented highlight map",
            )
        })?;
    Ok(serde_json::json!({
        "source": "reduct_cloud",
        "highlights": highlights,
        "provider_returned_count": highlights.len(),
        "pagination": "unknown"
    }))
}

async fn recording_highlights(
    key: Option<&str>,
    project_id: &str,
    recording_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = recording_read_url(project_id, recording_id, "highlight")?;
    let response = read_response(key, &url, "highlight read").await?;
    let body = read_bounded(response, 2 * 1024 * 1024).await?;
    parse_highlights_response(&body)
}

async fn recording_status(
    key: Option<&str>,
    project_id: &str,
    recording_id: &str,
) -> Result<serde_json::Value, McpToolError> {
    let url = recording_read_url(project_id, recording_id, "status")?;
    let response = read_response(key, &url, "recording status").await?;
    let body = read_bounded(response, 512 * 1024).await?;
    let status: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
        McpToolError::failed_precondition("Reduct returned invalid recording-status JSON")
    })?;
    Ok(serde_json::json!({"source": "reduct_cloud", "status": status}))
}

async fn recording_transcript(
    key: Option<&str>,
    project_id: &str,
    recording_id: &str,
    format: &str,
) -> Result<serde_json::Value, McpToolError> {
    if !matches!(format, "json" | "txt") {
        return Err(McpToolError::invalid_argument(
            "transcript format must be json or txt",
        ));
    }
    let url = recording_read_url(project_id, recording_id, &format!("transcript.{format}"))?;
    let response = read_response(key, &url, "transcript read").await?;
    let body = read_bounded(response, 8 * 1024 * 1024).await?;
    parse_transcript_body(&body, format)
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductRecordingRequest {
    project_id: String,
    recording_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductTranscriptRequest {
    project_id: String,
    recording_id: String,
    /// json (timing-bearing provider structure) or txt (plain text).
    format: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductProjectsRequest {
    /// Maximum projects to return from the provider response (1–100). This
    /// does not request server-side pagination; that contract is unknown.
    limit: usize,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductProjectItemsRequest {
    project_id: String,
    limit: usize,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductReelRequest {
    project_id: String,
    reel_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductCreateRecordingRequest {
    project_id: String,
    title: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductCreateReelRequest {
    project_id: String,
    title: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductCreateReelClipRequest {
    project_id: String,
    reel_id: String,
    recording_id: String,
    /// Position among the reel's blocks (Reduct accepts a finite float or integer).
    order: f64,
    start: f64,
    end: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductCreateReelTitleRequest {
    project_id: String,
    reel_id: String,
    order: f64,
    duration: f64,
    title: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductEditReelClipRequest {
    project_id: String,
    reel_id: String,
    block_id: String,
    start: f64,
    end: f64,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductImportMediaRequest {
    project_id: String,
    recording_id: String,
    url: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductUploadMediaRequest {
    project_id: String,
    recording_id: String,
    /// Stable ID of an indexed gallery video/audio asset; never an arbitrary path.
    gallery_asset_id: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ReductUploadLocalMediaRequest {
    project_id: String,
    recording_id: String,
    /// Absolute path to a local audio/video file. The file is sent to Reduct, not indexed in the gallery.
    path: String,
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
        "cloud_editing": "not_checked",
        "note": "The API key reached the media MCP child; this call did not contact Reduct. Use the separate read or explicit cloud tools to make requests."
    }))
}

#[tool_router(router = reduct_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Check whether the Reduct.video API key reached the media server. Does not contact Reduct or verify cloud edits; never returns the key."
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

    #[tool(
        description = "List up to limit (1–100) recording IDs and titles from an existing Reduct project. Only returns the provider-supplied subset; no pagination claim."
    )]
    pub async fn reduct_recordings_snapshot(
        &self,
        Parameters(ReductProjectItemsRequest { project_id, limit }): Parameters<
            ReductProjectItemsRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_recordings_snapshot", async {
            project_titled_snapshot(
                self.reduct_api_key.as_deref(),
                &project_id,
                "recordings",
                limit,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "List up to limit (1–100) reel IDs and titles from an existing Reduct project. Read-only, excludes private share tokens, and does not claim server-side pagination or render state."
    )]
    pub async fn reduct_reels_snapshot(
        &self,
        Parameters(ReductProjectItemsRequest { project_id, limit }): Parameters<
            ReductProjectItemsRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_reels_snapshot", async {
            project_titled_snapshot(self.reduct_api_key.as_deref(), &project_id, "reels", limit)
                .await
        })
        .await
    }

    #[tool(
        description = "Read an existing Reduct reel's title, provider-native blocks and publication state (up to 4 MiB). Removes all share_token fields; never publishes, changes, or renders the reel."
    )]
    pub async fn reduct_reel_detail(
        &self,
        Parameters(ReductReelRequest {
            project_id,
            reel_id,
        }): Parameters<ReductReelRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_reel_detail", async {
            reel_detail(self.reduct_api_key.as_deref(), &project_id, &reel_id).await
        })
        .await
    }

    #[tool(
        description = "Create an empty Reduct cloud reel with a title in an existing project. Returns its provider ID as submitted; read reel detail before composing. Never creates a local educt reel."
    )]
    pub async fn reduct_create_reel(
        &self,
        Parameters(ReductCreateReelRequest { project_id, title }): Parameters<
            ReductCreateReelRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_create_reel", async {
            create_reel(
                self.reduct_api_key.as_deref(),
                API_ROOT,
                &project_id,
                &title,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Add an ordered doc-range clip from a Reduct recording to an existing cloud reel. start/end are seconds; returns a submitted block ID. Read reel detail to verify composition; does not edit local educt."
    )]
    pub async fn reduct_add_reel_clip(
        &self,
        Parameters(ReductCreateReelClipRequest {
            project_id,
            reel_id,
            recording_id,
            order,
            start,
            end,
        }): Parameters<ReductCreateReelClipRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_add_reel_clip", async {
            create_reel_clip(
                self.reduct_api_key.as_deref(),
                API_ROOT,
                &project_id,
                &reel_id,
                &recording_id,
                order,
                start,
                end,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Add an ordered text title card to an existing Reduct cloud reel. Duration is in seconds; returns a submitted block ID. Read reel detail to verify composition."
    )]
    pub async fn reduct_add_reel_title(
        &self,
        Parameters(ReductCreateReelTitleRequest {
            project_id,
            reel_id,
            order,
            duration,
            title,
        }): Parameters<ReductCreateReelTitleRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_add_reel_title", async {
            create_reel_title(
                self.reduct_api_key.as_deref(),
                API_ROOT,
                &project_id,
                &reel_id,
                order,
                duration,
                &title,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Edit the start/end seconds of an existing Reduct cloud doc-range block. Reads reel detail first to refuse a title block. POST may overwrite those fields; inspect reel detail after submission and before retrying."
    )]
    pub async fn reduct_edit_reel_clip_range(
        &self,
        Parameters(ReductEditReelClipRequest {
            project_id,
            reel_id,
            block_id,
            start,
            end,
        }): Parameters<ReductEditReelClipRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_edit_reel_clip_range", async {
            edit_reel_clip_range(
                self.reduct_api_key.as_deref(),
                API_ROOT,
                &project_id,
                &reel_id,
                &block_id,
                start,
                end,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Create a Reduct cloud recording in an existing project (POST title). This mutates the Reduct workspace; returns the provider's recording ID. Never creates a local educt transcript."
    )]
    pub async fn reduct_create_recording(
        &self,
        Parameters(ReductCreateRecordingRequest { project_id, title }): Parameters<
            ReductCreateRecordingRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_create_recording", async {
            create_recording(self.reduct_api_key.as_deref(), &project_id, &title).await
        })
        .await
    }

    #[tool(
        description = "Submit a public URL to Reduct to import video or audio into an existing recording. This mutates the cloud workspace and causes Reduct to fetch the URL. Returns media IDs as submitted, not a completed transcription; check recording status."
    )]
    pub async fn reduct_import_media(
        &self,
        Parameters(ReductImportMediaRequest {
            project_id,
            recording_id,
            url,
        }): Parameters<ReductImportMediaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_import_media", async {
            import_media(
                self.reduct_api_key.as_deref(),
                &project_id,
                &recording_id,
                &url,
            )
            .await
        })
        .await
    }

    /// Upload only an indexed video/audio asset whose bytes still match its
    /// gallery identity. A replaced file must be re-indexed before transfer.
    async fn upload_gallery_media(
        &self,
        project_id: &str,
        recording_id: &str,
        gallery_asset_id: &str,
    ) -> Result<serde_json::Value, McpToolError> {
        connection_status(self.reduct_api_key.as_deref())?;
        let gallery = self.access_gallery().map_err(map_media_error)?;
        let asset = self
            .gallery_store
            .get_by_id(&gallery.gallery_id, gallery_asset_id)
            .map_err(map_gallery_store_error)?;
        if asset.missing || !matches!(asset.media_type.as_str(), "video" | "audio") {
            return Err(McpToolError::failed_precondition(
                "Reduct upload requires an available indexed video or audio asset",
            ));
        }
        let original = std::path::Path::new(&asset.absolute_path);
        let path = tokio::fs::canonicalize(original)
            .await
            .map_err(|_| McpToolError::not_found("Indexed media file no longer exists"))?;
        if path != original {
            return Err(McpToolError::failed_precondition(
                "Indexed media path changed; re-index before cloud upload",
            ));
        }
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|_| McpToolError::not_found("Indexed media file cannot be read"))?;
        const MAX_UPLOAD_BYTES: u64 = 128 * 1024 * 1024;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_UPLOAD_BYTES {
            return Err(McpToolError::failed_precondition(
                "Cloud upload accepts regular nonempty gallery media up to 128 MiB; use Reduct media-import for larger remote files",
            ));
        }
        let filename = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| McpToolError::invalid_argument("Indexed media filename is not UTF-8"))?;
        let url = media_upload_url(project_id, recording_id, filename)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| McpToolError::unavailable("Indexed media file could not be read"))?;
        validate_indexed_upload_bytes(&asset.hash, metadata.len(), &bytes)?;
        let response = post_binary_response(
            self.reduct_api_key.as_deref(),
            &url,
            bytes.into(),
            metadata.len(),
            "media upload",
        )
        .await?;
        read_upload_ack(response).await
    }

    #[tool(
        description = "Upload an indexed gallery video/audio asset (up to 128 MiB) to an existing Reduct recording. Mutates the cloud workspace, verifies file hash before transfer, and reports submission rather than completed transcription. Never uploads an arbitrary filesystem path."
    )]
    pub async fn reduct_upload_gallery_media(
        &self,
        Parameters(ReductUploadMediaRequest {
            project_id,
            recording_id,
            gallery_asset_id,
        }): Parameters<ReductUploadMediaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_upload_gallery_media", async {
            self.upload_gallery_media(&project_id, &recording_id, &gallery_asset_id)
                .await
        })
        .await
    }

    #[tool(
        description = "Upload a local audio/video file by absolute path to an existing Reduct recording, without gallery indexing. Sends file bytes to Reduct; Reduct determines format support. Streams the file without an in-memory size cap. Mutates the cloud workspace; check recording status before any retry."
    )]
    pub async fn reduct_upload_local_media(
        &self,
        Parameters(ReductUploadLocalMediaRequest {
            project_id,
            recording_id,
            path,
        }): Parameters<ReductUploadLocalMediaRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_upload_local_media", async {
            connection_status(self.reduct_api_key.as_deref())?;
            let path = std::path::Path::new(&path);
            if !path.is_absolute() {
                return Err(McpToolError::invalid_argument(
                    "Reduct local upload requires an absolute file path",
                ));
            }
            let filename = path
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .ok_or_else(|| {
                    McpToolError::invalid_argument("Local media filename is not UTF-8")
                })?;
            let url = media_upload_url(&project_id, &recording_id, filename)?;
            upload_local_media(self.reduct_api_key.as_deref(), &url, path).await
        })
        .await
    }

    #[tool(
        description = "Read the provider's ID-keyed highlight JSON for one Reduct recording (up to 2 MiB). Keeps labels, text and timing fields as returned; does not mutate reels or create local educt layers."
    )]
    pub async fn reduct_recording_highlights(
        &self,
        Parameters(ReductRecordingRequest {
            project_id,
            recording_id,
        }): Parameters<ReductRecordingRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_recording_highlights", async {
            recording_highlights(self.reduct_api_key.as_deref(), &project_id, &recording_id).await
        })
        .await
    }

    #[tool(
        description = "Get Reduct's JSON transcription/recording status for a known project and recording ID. Read-only; no local educt fallback."
    )]
    pub async fn reduct_recording_status(
        &self,
        Parameters(ReductRecordingRequest {
            project_id,
            recording_id,
        }): Parameters<ReductRecordingRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_recording_status", async {
            recording_status(self.reduct_api_key.as_deref(), &project_id, &recording_id).await
        })
        .await
    }

    #[tool(
        description = "Read an existing Reduct recording transcript in json or txt format (up to 8 MiB). The provider response is not re-timed or silently substituted with local educt data."
    )]
    pub async fn reduct_recording_transcript(
        &self,
        Parameters(ReductTranscriptRequest {
            project_id,
            recording_id,
            format,
        }): Parameters<ReductTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reduct_recording_transcript", async {
            recording_transcript(
                self.reduct_api_key.as_deref(),
                &project_id,
                &recording_id,
                &format,
            )
            .await
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

    // This fixture exercises the production URL builder, request body, header and
    // acknowledgement parser together without sending any cloud mutation.
    async fn reel_post_fixture<F, Fut>(
        expected_path: &str,
        expected_body: serde_json::Value,
        response_body: &str,
        call: F,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>>
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<serde_json::Value, McpToolError>>,
    {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let root = format!("http://{}/", listener.local_addr()?);
        let response_body = response_body.to_string();
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut request = String::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let n = stream.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other(
                        "incomplete or oversized fixture request",
                    ));
                }
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if request.split_once("\r\n\r\n").is_some_and(|(_, body)| {
                    serde_json::from_str::<serde_json::Value>(body).is_ok()
                }) {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            )?;
            Ok(request)
        });
        let result = call(root).await?;
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("fixture server panicked"))??;
        assert!(request.starts_with(&format!("POST {expected_path} HTTP/1.1")));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        let (_, body) = request
            .split_once("\r\n\r\n")
            .ok_or("missing fixture request body")?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body)?,
            expected_body
        );
        assert!(!body.contains("fixture-key"));
        Ok(result)
    }

    #[tokio::test]
    async fn documented_reel_writes_send_only_the_pdf_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let reel = reel_post_fixture(
            "/project/p_fixture/reel",
            serde_json::json!({"title": "Fixture reel"}),
            r#"{"reel":"reel_fixture"}"#,
            |root| async move {
                create_reel(Some("fixture-key"), &root, "p_fixture", "Fixture reel").await
            },
        )
        .await?;
        assert_eq!(reel["reel_id"], "reel_fixture");
        let clip = reel_post_fixture(
            "/project/p_fixture/reel/reel_fixture/block",
            serde_json::json!({"type":"doc-range","order":1.0,"recording":"recording_fixture","start":2.5,"end":4.0,"strikethrough":{}}),
            r#"{"block":"block_fixture"}"#,
            |root| async move { create_reel_clip(Some("fixture-key"), &root, "p_fixture", "reel_fixture", "recording_fixture", 1.0, 2.5, 4.0).await },
        ).await?;
        assert_eq!(clip["block_id"], "block_fixture");
        let title = reel_post_fixture(
            "/project/p_fixture/reel/reel_fixture/block",
            serde_json::json!({"type":"title","order":2.0,"duration":3.0,"title":"Fixture card"}),
            r#"{"block":"title_fixture"}"#,
            |root| async move {
                create_reel_title(
                    Some("fixture-key"),
                    &root,
                    "p_fixture",
                    "reel_fixture",
                    2.0,
                    3.0,
                    "Fixture card",
                )
                .await
            },
        )
        .await?;
        assert_eq!(title["block_id"], "title_fixture");
        Ok(())
    }

    #[tokio::test]
    async fn reel_clip_edit_reads_type_then_posts_only_range()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let root = format!("http://{}/", listener.local_addr()?);
        let peer = std::thread::spawn(move || -> std::io::Result<(String, String)> {
            let (mut get, _) = listener.accept()?;
            get.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut buffer = [0_u8; 4096];
            let n = get.read(&mut buffer)?;
            let get_request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let body = r#"{"reel_fixture":{"title":"Fixture","block":{"clip_fixture":{"type":"doc-range","start":1.0,"end":2.0}}}}"#;
            write!(
                get,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            drop(get);
            let (mut post, _) = listener.accept()?;
            post.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut request = String::new();
            loop {
                let n = post.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other("incomplete clip edit fixture"));
                }
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if request.split_once("\r\n\r\n").is_some_and(|(_, body)| {
                    serde_json::from_str::<serde_json::Value>(body).is_ok()
                }) {
                    break;
                }
            }
            let ack = r#"{"clip_fixture":{"start":1.5,"end":2.5}}"#;
            write!(
                post,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ack}",
                ack.len()
            )?;
            Ok((get_request, request))
        });
        let result = edit_reel_clip_range(
            Some("fixture-key"),
            &root,
            "p_fixture",
            "reel_fixture",
            "clip_fixture",
            1.5,
            2.5,
        )
        .await?;
        assert_eq!(result["state"], "submitted; inspect reel detail");
        let (get, post) = peer
            .join()
            .map_err(|_| std::io::Error::other("fixture server panicked"))??;
        assert!(get.starts_with("GET /project/p_fixture/reel/reel_fixture HTTP/1.1"));
        assert!(
            post.starts_with(
                "POST /project/p_fixture/reel/reel_fixture/block/clip_fixture HTTP/1.1"
            )
        );
        assert!(
            post.to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        let (_, body) = post.split_once("\r\n\r\n").ok_or("missing edit body")?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body)?,
            serde_json::json!({"start":1.5,"end":2.5})
        );
        Ok(())
    }

    /// expect: An unsuccessful cloud POST never invites a blind retry after an uncertain provider outcome. [P1]
    #[tokio::test]
    async fn reel_post_429_and_5xx_preserve_kind_and_warn_to_inspect()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        for (status_line, kind) in [
            (
                "429 Too Many Requests",
                hkask_types::McpErrorKind::RateLimited,
            ),
            (
                "503 Service Unavailable",
                hkask_types::McpErrorKind::Unavailable,
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let root = format!("http://{}/", listener.local_addr()?);
            let peer = std::thread::spawn(move || -> std::io::Result<()> {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
                let mut buffer = [0_u8; 4096];
                let mut request = String::new();
                loop {
                    let n = stream.read(&mut buffer)?;
                    if n == 0 || request.len() > 16 * 1024 {
                        return Err(std::io::Error::other("incomplete POST fixture"));
                    }
                    request.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    if request.contains("\r\n\r\n") && request.contains("Fixture reel") {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 {status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )?;
                Ok(())
            });
            let error = create_reel(Some("fixture-key"), &root, "p_fixture", "Fixture reel")
                .await
                .expect_err("non-successful POST must not claim Reel creation");
            peer.join()
                .map_err(|_| std::io::Error::other("fixture server panicked"))??;
            assert_eq!(error.kind, kind, "{status_line}");
            assert!(
                error.message.contains("may have succeeded"),
                "{status_line}: {}",
                error.message
            );
            assert!(
                error.message.contains("inspect before retrying"),
                "{status_line}: {}",
                error.message
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn reel_post_http_error_is_classified_and_never_reported_as_created()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let root = format!("http://{}/", listener.local_addr()?);
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut buffer = [0_u8; 4096];
            let mut request = String::new();
            loop {
                let n = stream.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other("incomplete error fixture"));
                }
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
                if request.contains("\r\n\r\n") && request.contains("Fixture reel") {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
            Ok(request)
        });
        let error = create_reel(Some("fixture-key"), &root, "p_fixture", "Fixture reel")
            .await
            .expect_err("provider refusal is not a created reel");
        assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("fixture server panicked"))??;
        assert!(request.starts_with("POST /project/p_fixture/reel HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        Ok(())
    }

    #[test]
    fn reel_writes_reject_bad_ranges_ids_and_ambiguous_acknowledgements() -> Result<(), McpToolError>
    {
        assert_eq!(
            reel_create_url(API_ROOT, "p_fixture")?,
            format!("{API_ROOT}project/p_fixture/reel")
        );
        assert_eq!(
            reel_block_url(API_ROOT, "p_fixture", "reel_fixture", Some("b_fixture"))?,
            format!("{API_ROOT}project/p_fixture/reel/reel_fixture/block/b_fixture")
        );
        assert!(reel_block_url(API_ROOT, "p/invalid", "r", None).is_err());
        assert!(validate_reel_range(1.0, 4.0, 2.0).is_err());
        assert!(validate_reel_range(f64::NAN, 0.0, 2.0).is_err());
        assert!(validate_reel_range(1.0, f64::INFINITY, 2.0).is_err());
        assert!(parse_reel_create_response(br#"{"reel":"r_fixture"}"#).is_ok());
        for bad in [b"not json".as_slice(), br#"{}"#, br#"{"reel":"../bad"}"#] {
            assert!(
                parse_reel_create_response(bad)
                    .expect_err("uncertain reel acknowledgement")
                    .message
                    .contains("inspect")
            );
        }
        assert!(
            parse_reel_block_create_response(br#"{}"#)
                .expect_err("uncertain block acknowledgement")
                .message
                .contains("inspect")
        );
        assert!(
            parse_reel_block_edit_response(br#"{}"#, "b_fixture")
                .expect_err("uncertain edit acknowledgement")
                .message
                .contains("inspect")
        );
        assert_eq!(
            parse_reel_block_edit_response(br#"{"b_fixture":{"start":2.0}}"#, "b_fixture")?["state"],
            "submitted; inspect reel detail"
        );
        assert_eq!(
            classify_reduct_status(reqwest::StatusCode::BAD_REQUEST, "reel creation")
                .expect_err("400 must fail")
                .kind,
            hkask_types::McpErrorKind::InvalidArgument
        );
        assert_eq!(
            classify_reduct_status(reqwest::StatusCode::FORBIDDEN, "reel creation")
                .expect_err("403 must fail")
                .kind,
            hkask_types::McpErrorKind::PermissionDenied
        );
        Ok(())
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
        assert_eq!(snapshot["cloud_editing"], "not_checked");
        assert!(parse_project_snapshot(br#"{"other": []}"#, 1).is_err());
        assert!(parse_project_snapshot(br#"{"project":{"p1":{}}}"#, 1).is_err());
        assert!(parse_project_snapshot(body, 0).is_err());
        Ok(())
    }

    #[test]
    fn changed_gallery_bytes_are_refused_before_upload() -> Result<(), McpToolError> {
        use sha2::Digest;
        let hash = format!("{:x}", sha2::Sha256::digest(b"original"));
        validate_indexed_upload_bytes(&hash, 8, b"original")?;
        assert!(validate_indexed_upload_bytes(&hash, 8, b"modified").is_err());
        assert!(validate_indexed_upload_bytes(&hash, 7, b"original").is_err());
        Ok(())
    }

    /// expect: A binary cloud upload that receives HTTP 5xx stays an uncertain, visible failure. [P1]
    #[tokio::test]
    async fn binary_upload_post_5xx_keeps_uncertain_write_warning()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut request = String::new();
            let mut buffer = [0_u8; 4096];
            while !request.contains("fixture-binary") {
                let n = stream.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other("incomplete binary fixture"));
                }
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
            }
            write!(
                stream,
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )?;
            Ok(request)
        });
        let url = format!(
            "http://{address}/project/p_fixture/recording/r_fixture/media-upload?filename=fixture.mp4"
        );
        let error = post_binary_response(
            Some("fixture-key"),
            &url,
            b"fixture-binary".to_vec().into(),
            b"fixture-binary".len() as u64,
            "media upload",
        )
        .await
        .expect_err("HTTP 503 cannot claim successful upload");
        assert_eq!(error.kind, hkask_types::McpErrorKind::Unavailable);
        assert!(error.message.contains("may have succeeded"));
        assert!(error.message.contains("inspect before retrying"));
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("fixture server panicked"))??;
        assert!(request.starts_with(
            "POST /project/p_fixture/recording/r_fixture/media-upload?filename=fixture.mp4 HTTP/1.1"
        ));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        Ok(())
    }

    /// expect: A local media file outside any gallery can be submitted without indexing, while missing files fail before POST. [P1]
    #[tokio::test]
    async fn local_file_upload_streams_without_gallery_and_rejects_missing_input()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sample clip.wav");
        let mut payload = Vec::from(&b"RIFF"[..]);
        payload.extend_from_slice(&38_u32.to_le_bytes());
        payload.extend_from_slice(b"WAVEfmt ");
        payload.extend_from_slice(&16_u32.to_le_bytes());
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&8000_u32.to_le_bytes());
        payload.extend_from_slice(&16000_u32.to_le_bytes());
        payload.extend_from_slice(&2_u16.to_le_bytes());
        payload.extend_from_slice(&16_u16.to_le_bytes());
        payload.extend_from_slice(b"data");
        payload.extend_from_slice(&2_u32.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        tokio::fs::write(&path, &payload).await?;
        let peer_payload = payload.clone();
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request
                .windows(peer_payload.len())
                .any(|window| window == peer_payload)
            {
                let n = stream.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other("incomplete upload fixture"));
                }
                request.extend_from_slice(&buffer[..n]);
            }
            let response_body = r#"{"media_id":"m1"}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            )?;
            let end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .ok_or_else(|| std::io::Error::other("missing request headers"))?;
            String::from_utf8(request[..end].to_vec()).map_err(std::io::Error::other)
        });
        let url = format!(
            "http://{address}/project/p_fixture/recording/r_fixture/media-upload?filename=sample+clip.wav"
        );
        let result = upload_local_media(Some("fixture-key"), &url, &path).await;
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("fixture peer panicked"))??;
        let result = result?;
        assert_eq!(result["media_id"], "m1");
        assert!(request.starts_with("POST /project/p_fixture/recording/r_fixture/media-upload?filename=sample+clip.wav HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        assert!(!request.contains(&path.to_string_lossy().to_string()));
        assert!(request.to_ascii_lowercase().contains("content-length: 46"));
        assert!(
            !request
                .to_ascii_lowercase()
                .contains("transfer-encoding: chunked")
        );
        let invalid_path = dir.path().join("not-media.wav");
        tokio::fs::write(&invalid_path, b"not audio").await?;
        let invalid = upload_local_media(Some("fixture-key"), &url, &invalid_path)
            .await
            .expect_err("non-media bytes must not be sent");
        assert_eq!(invalid.kind, hkask_types::McpErrorKind::InvalidArgument);
        let missing =
            upload_local_media(Some("fixture-key"), &url, &dir.path().join("missing.wav"))
                .await
                .expect_err("missing local file must not be sent");
        assert_eq!(missing.kind, hkask_types::McpErrorKind::NotFound);
        let directory = upload_local_media(Some("fixture-key"), &url, dir.path())
            .await
            .expect_err("directory must not be sent");
        assert_eq!(directory.kind, hkask_types::McpErrorKind::InvalidArgument);
        Ok(())
    }

    #[test]
    fn documented_binary_upload_contract_encodes_filename_and_requires_media_id()
    -> Result<(), McpToolError> {
        assert_eq!(
            media_upload_url("p1", "r2", "meeting clip.mp4")?,
            "https://app.reduct.video/api/v3/project/p1/recording/r2/media-upload?filename=meeting+clip.mp4"
        );
        assert!(media_upload_url("p1", "r2", "").is_err());
        let uploaded = parse_media_upload_response(br#"{"media_id":"m1"}"#)?;
        assert_eq!(uploaded["media_id"], "m1");
        assert_eq!(
            uploaded["upload_state"],
            "submitted; check recording status"
        );
        assert!(parse_media_upload_response(br#"{}"#).is_err());
        let invalid_id = parse_media_upload_response(br#"{"media_id":"bad/id"}"#)
            .expect_err("invalid upload acknowledgement is an uncertain write");
        assert!(invalid_id.message.contains("may have succeeded"));
        assert!(invalid_id.message.contains("inspect before retrying"));
        Ok(())
    }

    #[test]
    fn documented_mutation_contracts_validate_ids_and_acknowledgements() -> Result<(), McpToolError>
    {
        assert_eq!(
            recording_create_url("p1")?,
            "https://app.reduct.video/api/v3/project/p1/recording"
        );
        assert_eq!(
            media_import_url("p1", "r2")?,
            "https://app.reduct.video/api/v3/project/p1/recording/r2/media-import"
        );
        assert!(media_import_url("p1", "../private").is_err());
        let created = parse_recording_create_response(br#"{"recording":"r2"}"#)?;
        assert_eq!(created["recording_id"], "r2");
        let imported = parse_media_import_response(br#"{"media_ids":["m1","m2"]}"#)?;
        assert_eq!(imported["media_ids"][0], "m1");
        assert!(parse_recording_create_response(br#"{"recording":{}}"#).is_err());
        assert!(parse_media_import_response(br#"{"media_ids":"m1"}"#).is_err());
        assert!(parse_media_import_response(br#"{"media_ids":[]}"#).is_err());
        let invalid_id = parse_media_import_response(br#"{"media_ids":["bad/id"]}"#)
            .expect_err("invalid import acknowledgement is an uncertain write");
        assert!(invalid_id.message.contains("may have succeeded"));
        assert!(invalid_id.message.contains("inspect before retrying"));
        Ok(())
    }

    #[test]
    fn recording_snapshot_projects_ids_and_titles_from_project_detail() -> Result<(), McpToolError>
    {
        let body = br#"{"p1":{"recordings":{"r2":{"title":"Second","member":["secret"]},"r1":{"title":"First","description":"private"}}}}"#;
        let result = parse_project_titled_collection(body, "p1", "recordings", 1)?;
        assert_eq!(result["provider_returned_count"], 2);
        assert_eq!(result["recordings"][0]["id"], "r1");
        assert_eq!(result["recordings"][0]["title"], "First");
        assert_eq!(result["truncated"], true);
        assert!(!result.to_string().contains("secret"));
        assert!(!result.to_string().contains("private"));
        assert!(parse_project_titled_collection(body, "unknown", "recordings", 1).is_err());
        assert!(
            parse_project_titled_collection(
                br#"{"p1":{"recordings":{"r1":{}}}}"#,
                "p1",
                "recordings",
                1
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn documented_recording_reads_validate_ids_and_formats() -> Result<(), McpToolError> {
        assert_eq!(
            recording_read_url("p-id", "r_1", "status")?,
            "https://app.reduct.video/api/v3/project/p-id/recording/r_1/status"
        );
        assert_eq!(
            recording_read_url("p-id", "r_1", "transcript.json")?,
            "https://app.reduct.video/api/v3/project/p-id/recording/r_1/transcript.json"
        );
        assert_eq!(
            recording_read_url("p-id", "r_1", "highlight")?,
            "https://app.reduct.video/api/v3/project/p-id/recording/r_1/highlight"
        );
        assert!(recording_read_url("../other", "r_1", "status").is_err());
        assert!(recording_read_url("p-id", "r/2", "status").is_err());
        assert!(recording_read_url("p-id", "r_1", "transcript.docx").is_err());
        assert!(recording_read_url("p-id", "r_1", "unknown").is_err());
        Ok(())
    }

    #[test]
    fn reel_detail_keeps_blocks_but_never_exposes_share_token() -> Result<(), McpToolError> {
        assert_eq!(
            reel_detail_url("p1", "reel1")?,
            "https://app.reduct.video/api/v3/project/p1/reel/reel1"
        );
        assert!(reel_detail_url("p1", "../other").is_err());
        let body = br#"{"reel1":{"title":"Clip reel","share_token":"private-token","block":{"b1":{"type":"doc-range","start":1.0,"share_token":"nested-private"}}}}"#;
        let result = parse_reel_detail(body, "reel1")?;
        assert_eq!(result["title"], "Clip reel");
        assert_eq!(result["blocks"]["b1"]["type"], "doc-range");
        assert_eq!(result["publication_state"], "published");
        assert!(!result.to_string().contains("private-token"));
        assert!(!result.to_string().contains("nested-private"));
        assert!(parse_reel_detail(br#"{"wrong":{"title":"X"}}"#, "reel1").is_err());
        let empty = parse_reel_detail(br#"{"reel1":{"title":"New"}}"#, "reel1")?;
        assert_eq!(empty["block_state"], "not_present_in_provider_response");
        assert_eq!(empty["publication_state"], "unpublished");
        assert!(empty["blocks"].is_null());
        let ambiguous =
            parse_reel_detail(br#"{"reel1":{"title":"New","share_token":null}}"#, "reel1")?;
        assert_eq!(ambiguous["publication_state"], "undetermined");
        Ok(())
    }

    #[test]
    fn reel_snapshot_preserves_only_id_and_title() -> Result<(), McpToolError> {
        let body = br#"{"p1":{"reels":{"reel2":{"title":"Second","share_token":"private"},"reel1":{"title":"First","blocks":{"sensitive":true}}}}}"#;
        let result = parse_project_titled_collection(body, "p1", "reels", 1)?;
        assert_eq!(result["reels"][0]["id"], "reel1");
        assert_eq!(result["reels"][0]["title"], "First");
        assert_eq!(result["provider_returned_count"], 2);
        assert_eq!(result["truncated"], true);
        assert!(!result.to_string().contains("private"));
        assert!(!result.to_string().contains("sensitive"));
        assert!(parse_project_titled_collection(br#"{"p1":{}}"#, "p1", "reels", 1).is_err());
        Ok(())
    }

    #[test]
    fn highlight_read_preserves_provider_fields_and_rejects_wrong_shape() -> Result<(), McpToolError>
    {
        let body =
            br#"{"highlight":{"h1":{"labels":["clip"],"start":1.25,"end":3.5,"text":"quote"}}}"#;
        let result = parse_highlights_response(body)?;
        assert_eq!(result["source"], "reduct_cloud");
        assert_eq!(result["provider_returned_count"], 1);
        assert_eq!(result["highlights"]["h1"]["labels"][0], "clip");
        assert_eq!(result["highlights"]["h1"]["start"], 1.25);
        assert_eq!(result["pagination"], "unknown");
        assert!(parse_highlights_response(br#"{"recording":{}}"#).is_err());
        Ok(())
    }

    #[test]
    fn transcript_response_keeps_format_and_does_not_fake_timings() -> Result<(), McpToolError> {
        let json = parse_transcript_body(br#"{"wdlist":[]}"#, "json")?;
        assert_eq!(json["source"], "reduct_cloud");
        assert_eq!(json["content"]["wdlist"], serde_json::json!([]));
        let txt = parse_transcript_body(b"hello", "txt")?;
        assert_eq!(txt["content"], "hello");
        assert!(parse_transcript_body(b"not-json", "json").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn recording_post_sends_documented_json_with_key_only_in_header()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let peer = std::thread::spawn(move || -> std::io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
            let mut request = String::new();
            let mut buffer = [0_u8; 4096];
            while !request.contains("\"title\":\"Fixture\"") {
                let n = stream.read(&mut buffer)?;
                if n == 0 || request.len() > 16 * 1024 {
                    return Err(std::io::Error::other(
                        "incomplete or oversized test request",
                    ));
                }
                request.push_str(&String::from_utf8_lossy(&buffer[..n]));
            }
            let body = r#"{"recording":"r_fixture"}"#;
            write!(
                stream,
                "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            Ok(request)
        });
        let url = format!("http://{address}/project/p_fixture/recording");
        let response = post_json_response(
            Some("fixture-key"),
            &url,
            serde_json::json!({"title":"Fixture"}),
            "recording creation",
        )
        .await?;
        let body = read_bounded(response, 64 * 1024).await?;
        assert_eq!(
            parse_recording_create_response(&body)?["recording_id"],
            "r_fixture"
        );
        let request = peer
            .join()
            .map_err(|_| std::io::Error::other("test server panicked"))??;
        assert!(request.starts_with("POST /project/p_fixture/recording HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-auth-key: fixture-key")
        );
        assert!(request.contains("\"title\":\"Fixture\""));
        assert!(!request.contains("fixture-key\"}"));
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
        assert_eq!(snapshot["cloud_editing"], "not_checked");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_reel_metadata_shape() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 for read-only inspection".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let response = read_response(Some(key.as_str()), PROJECT_PROBE_URL, "project list").await?;
        let body = read_bounded(response, 2 * 1024 * 1024).await?;
        let projects: serde_json::Value = serde_json::from_slice(&body)?;
        let ids = projects
            .get("project")
            .and_then(serde_json::Value::as_object)
            .ok_or("project list missing project map")?;
        let mut inspected = 0;
        let mut observed = false;
        for project_id in ids.keys().take(12) {
            let response = read_response(
                Some(key.as_str()),
                &project_detail_url(project_id)?,
                "project detail",
            )
            .await?;
            let body = read_bounded(response, 2 * 1024 * 1024).await?;
            let detail: serde_json::Value = serde_json::from_slice(&body)?;
            inspected += 1;
            if let Some(reels) = detail
                .get(project_id)
                .and_then(|p| p.get("reels"))
                .and_then(serde_json::Value::as_object)
            {
                observed = true;
                let first = reels.values().next();
                let fields: Vec<&str> = first
                    .and_then(serde_json::Value::as_object)
                    .into_iter()
                    .flat_map(|record| record.keys().map(String::as_str))
                    .filter(|name| {
                        name.len() <= 32
                            && name
                                .chars()
                                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    })
                    .collect();
                eprintln!(
                    "Reduct reel metadata: count={}; first-field-names={fields:?}",
                    reels.len()
                );
                let snapshot =
                    project_titled_snapshot(Some(key.as_str()), project_id, "reels", 10).await?;
                assert_eq!(snapshot["provider_returned_count"], reels.len());
                assert!(snapshot["returned_count"].as_u64().is_some_and(|n| n <= 10));
                if let Some(reel_id) = reels.keys().next() {
                    validate_reduct_id("reel_id", reel_id)?;
                    let url = format!("{API_ROOT}project/{project_id}/reel/{reel_id}");
                    let response = read_response(Some(key.as_str()), &url, "reel detail").await?;
                    let body = read_bounded(response, 2 * 1024 * 1024).await?;
                    let detail: serde_json::Value = serde_json::from_slice(&body)?;
                    let fields: Vec<&str> = detail
                        .get(reel_id)
                        .and_then(serde_json::Value::as_object)
                        .into_iter()
                        .flat_map(|record| record.keys().map(String::as_str))
                        .filter(|name| {
                            name.len() <= 32
                                && name
                                    .chars()
                                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                        })
                        .collect();
                    let block = detail.get(reel_id).and_then(|record| record.get("block"));
                    eprintln!(
                        "Reduct reel detail: id-keyed={}; top-level-field-count={}; fields={fields:?}; block-map={}; block-count={}",
                        detail.get(reel_id).is_some(),
                        detail.as_object().map(serde_json::Map::len).unwrap_or(0),
                        block.is_some_and(serde_json::Value::is_object),
                        block
                            .and_then(serde_json::Value::as_object)
                            .map(serde_json::Map::len)
                            .unwrap_or(0)
                    );
                    let projection = reel_detail(Some(key.as_str()), project_id, reel_id).await?;
                    assert_eq!(projection["source"], "reduct_cloud");
                    assert!(projection["title"].is_string());
                    assert_eq!(projection["share_tokens"], "redacted");
                    assert!(projection.get("share_token").is_none());
                }
                break;
            }
        }
        eprintln!("Reduct reel metadata observed={observed} in first {inspected} project details");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HKASK_REDUCT_LIVE_PROBE=1 and a configured OS keychain"]
    async fn live_recording_status_from_project() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var("HKASK_REDUCT_LIVE_PROBE").as_deref() != Ok("1") {
            return Err("set HKASK_REDUCT_LIVE_PROBE=1 for this read-only check".into());
        }
        let key = hkask_keystore::Keychain.retrieve_by_url("kask://credentials/reduct_api_key")?;
        let response = read_response(Some(key.as_str()), PROJECT_PROBE_URL, "project read").await?;
        let body = read_bounded(response, 2 * 1024 * 1024).await?;
        let projects: serde_json::Value = serde_json::from_slice(&body)?;
        let first_project = projects["project"]
            .as_object()
            .and_then(|map| map.keys().next())
            .ok_or("provider returned no projects")?;
        let url = project_detail_url(first_project)?;
        let response = read_response(Some(key.as_str()), &url, "project detail").await?;
        let body = read_bounded(response, 2 * 1024 * 1024).await?;
        let detail: serde_json::Value = serde_json::from_slice(&body)?;
        let snapshot = parse_project_titled_collection(&body, first_project, "recordings", 5)?;
        assert!(
            snapshot["returned_count"]
                .as_u64()
                .is_some_and(|count| count <= 5)
        );
        eprintln!(
            "Reduct project detail top-level map size={}; keyed_by_requested_project_id={}",
            detail.as_object().map(serde_json::Map::len).unwrap_or(0),
            detail.get(first_project).is_some()
        );
        for field in ["recording", "project", "data", "recordings"] {
            if let Some(value) = detail.get(field) {
                eprintln!(
                    "Reduct detail field={field}; object={}; count={}",
                    value.is_object(),
                    value.as_object().map(serde_json::Map::len).unwrap_or(0)
                );
            }
        }
        let project = detail
            .get(first_project)
            .ok_or("provider project detail omitted requested ID")?;
        eprintln!(
            "Reduct project detail reels: present={}; count={}",
            project
                .get("reels")
                .is_some_and(serde_json::Value::is_object),
            project
                .get("reels")
                .and_then(serde_json::Value::as_object)
                .map(serde_json::Map::len)
                .unwrap_or(0)
        );
        let fields: Vec<&str> = project
            .as_object()
            .into_iter()
            .flat_map(|object| object.keys().map(String::as_str))
            .filter(|name| {
                name.len() <= 32
                    && name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            })
            .collect();
        eprintln!(
            "Reduct project detail nested object={}; recording_map={}; field names (not values)={fields:?}",
            project.is_object(),
            project.get("recording").is_some()
        );
        let recordings = project
            .get("recordings")
            .ok_or("project detail lacks recordings field")?;
        eprintln!(
            "Reduct project detail recordings: array={}; object={}; count={}",
            recordings.is_array(),
            recordings.is_object(),
            recordings
                .as_array()
                .map(Vec::len)
                .or_else(|| recordings.as_object().map(serde_json::Map::len))
                .unwrap_or(0)
        );
        if let Some((recording_id, entry)) = recordings
            .as_object()
            .and_then(|entries| entries.iter().next())
        {
            eprintln!(
                "Reduct first recording entry object={}; has_title={}",
                entry.is_object(),
                entry.get("title").is_some()
            );
            let status = recording_status(Some(key.as_str()), first_project, recording_id).await?;
            assert_eq!(status["source"], "reduct_cloud");
            let transcript =
                recording_transcript(Some(key.as_str()), first_project, recording_id, "json")
                    .await?;
            assert_eq!(transcript["source"], "reduct_cloud");
            assert_eq!(transcript["format"], "json");
            let plain =
                recording_transcript(Some(key.as_str()), first_project, recording_id, "txt")
                    .await?;
            assert_eq!(plain["format"], "txt");
            let highlights =
                recording_highlights(Some(key.as_str()), first_project, recording_id).await?;
            assert_eq!(highlights["source"], "reduct_cloud");
            eprintln!(
                "Reduct highlight GET count={}",
                highlights["provider_returned_count"]
            );
            // Never print transcript words, highlight content, or project/recording IDs.
            eprintln!(
                "Reduct recording status response type={}",
                if status["status"].is_object() {
                    "object"
                } else {
                    "other"
                }
            );
        }
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
        assert_eq!(status["cloud_editing"], "not_checked");
        assert!(
            !status["evidence"]
                .as_str()
                .unwrap_or_default()
                .contains("no editing API contract")
        );
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
            let error = classify_reduct_status(
                reqwest::StatusCode::from_u16(status).expect("valid test status"),
                "project read",
            )
            .expect_err("non-success should be visible");
            assert_eq!(error.kind, expected_kind, "HTTP {status}");
            assert!(!error.message.contains("test-secret-do-not-echo"));
        }
        assert!(classify_reduct_status(reqwest::StatusCode::OK, "project read").is_ok());
    }

    #[test]
    fn configured_key_is_never_returned_or_misrepresented_as_connected() -> Result<(), McpToolError>
    {
        let status = connection_status(Some("test-secret-do-not-echo"))?;
        let encoded = status.to_string();
        assert!(!encoded.contains("test-secret-do-not-echo"));
        assert_eq!(status["provider_connection"], "not_checked");
        assert_eq!(status["cloud_editing"], "not_checked");
        Ok(())
    }
}
