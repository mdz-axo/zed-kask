//! Artifact management tools — user-facing report/screen persistence.
//!
//! These tools store user-facing artifacts (research reports, screens) in the
//! visible artifacts directory (`~/Documents/zk-data/companies-mcp/`), NOT in
//! the hidden internal data dir. Users need to find their reports without
//! digging through `~/.local/share/zed-kask/`.
use crate::CompaniesServer;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use hkask_types::agent_paths::resolve_under_artifacts_dir;
use rmcp::{handler::server::wrapper::Parameters, schemars::JsonSchema, tool, tool_router};
use serde::Deserialize;

/// Resolve the artifact subdirectory for a given kind.
fn validate_kind(kind: &str) -> Result<&'static str, McpToolError> {
    match kind {
        "report" => Ok("reports"),
        "screen" => Ok("screens"),
        _ => Err(McpToolError::invalid_argument(format!(
            "kind must be 'report' or 'screen' (got '{kind}')"
        ))),
    }
}

/// Resolve the artifact directory for a given kind, creating it if needed.
fn artifact_dir(kind_label: &str) -> Result<std::path::PathBuf, McpToolError> {
    let dir =
        resolve_under_artifacts_dir(std::path::Path::new(&format!("companies-mcp/{kind_label}")));
    std::fs::create_dir_all(&dir).map_err(|e| {
        McpToolError::internal(format!(
            "Failed to create artifact directory {}: {e}",
            dir.display()
        ))
    })?;
    Ok(dir)
}

/// Sanitize an artifact name for filesystem use. Prevents path traversal.
fn sanitize_artifact_name(name: &str) -> Result<String, McpToolError> {
    if name.is_empty() {
        return Err(McpToolError::invalid_argument("name must not be empty"));
    }
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            other => other,
        })
        .collect();
    if sanitized == "." || sanitized == ".." {
        return Err(McpToolError::invalid_argument(
            "name must not be '.' or '..'",
        ));
    }
    Ok(sanitized)
}

#[derive(Deserialize, JsonSchema)]
pub struct ReportSaveRequest {
    /// Artifact kind: "report" or "screen".
    #[schemars(regex(pattern = r"^(report|screen)$"))]
    pub kind: String,
    /// Artifact name (without extension). Used as the filename stem.
    pub name: String,
    /// JSON payload to persist.
    pub payload: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReportLoadRequest {
    /// Artifact kind: "report" or "screen".
    #[schemars(regex(pattern = r"^(report|screen)$"))]
    pub kind: String,
    /// Artifact name (without extension).
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReportListRequest {
    /// Artifact kind: "report" or "screen".
    #[schemars(regex(pattern = r"^(report|screen)$"))]
    pub kind: String,
}

#[tool_router(router = artifacts_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Persist a JSON artifact (screen or report) produced by the companies server or a skill."
    )]
    pub async fn report_save(&self, Parameters(req): Parameters<ReportSaveRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_save",
            Self::ontology_anchor("report_save"),
            async {
                let kind_label = validate_kind(&req.kind)?;
                let name = sanitize_artifact_name(&req.name)?;
                let dir = artifact_dir(kind_label)?;
                let path = dir.join(format!("{name}.json"));
                let json = serde_json::to_string_pretty(&req.payload).map_err(|e| {
                    McpToolError::invalid_argument(format!("payload is not serializable: {e}"))
                })?;
                std::fs::write(&path, json).map_err(|e| {
                    McpToolError::internal(format!(
                        "Failed to write artifact {}: {e}",
                        path.display()
                    ))
                })?;
                Ok(serde_json::json!({
                    "saved": true,
                    "kind": req.kind,
                    "name": name,
                    "path": path.to_string_lossy(),
                }))
            },
        )
        .await
    }

    #[tool(
        description = "Load a previously saved JSON artifact (screen or report) by name. Returns the full JSON payload. Returns a not-found error if no artifact with that name exists."
    )]
    pub async fn report_load(&self, Parameters(req): Parameters<ReportLoadRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_load",
            Self::ontology_anchor("report_load"),
            async {
                let kind_label = validate_kind(&req.kind)?;
                let name = sanitize_artifact_name(&req.name)?;
                let dir = artifact_dir(kind_label)?;
                let path = dir.join(format!("{name}.json"));
                let content = std::fs::read_to_string(&path).map_err(|_| {
                    McpToolError::not_found(format!(
                        "No {kind_label} artifact named '{name}' at {}",
                        path.display()
                    ))
                })?;
                let payload: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                    McpToolError::internal(format!("Artifact {name} is not valid JSON: {e}"))
                })?;
                Ok(serde_json::json!({
                    "loaded": true,
                    "kind": req.kind,
                    "name": name,
                    "payload": payload,
                }))
            },
        )
        .await
    }

    #[tool(
        description = "List saved artifact names (without extension) for a kind. Use kind='screen' or kind='report'. Returns a JSON array of names sorted alphabetically."
    )]
    pub async fn report_list(&self, Parameters(req): Parameters<ReportListRequest>) -> String {
        execute_tool_semantic(
            self,
            "report_list",
            Self::ontology_anchor("report_list"),
            async {
                let kind_label = validate_kind(&req.kind)?;
                let dir = artifact_dir(kind_label)?;
                let mut names: Vec<String> = std::fs::read_dir(&dir)
                    .map_err(|e| {
                        McpToolError::internal(format!(
                            "Failed to read artifact directory {}: {e}",
                            dir.display()
                        ))
                    })?
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| {
                        let path = entry.path();
                        if path.extension().is_some_and(|ext| ext == "json") {
                            path.file_stem()?.to_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();
                names.sort();
                Ok(serde_json::json!({
                    "kind": req.kind,
                    "count": names.len(),
                    "names": names,
                }))
            },
        )
        .await
    }
}
