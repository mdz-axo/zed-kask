//! Artifact management tools — user-facing report/screen persistence.
//!
//! These tools store user-facing artifacts (research reports, screens) in the
//! visible artifacts directory (`~/Documents/zk-data/companies-mcp/`), NOT in
//! the hidden internal data dir. Users need to find their reports without
//! digging through `~/.local/share/zed-kask/`.
use crate::CompaniesServer;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::agent_paths::{mcp_artifacts_subdir, resolve_under_artifacts_dir};
use rmcp::{handler::server::wrapper::Parameters, schemars::JsonSchema, tool, tool_router};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

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
    let dir = resolve_under_artifacts_dir(&mcp_artifacts_subdir("companies", kind_label));
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

pub(crate) fn save_json_artifact(
    kind: &str,
    name: &str,
    payload: &serde_json::Value,
) -> Result<std::path::PathBuf, McpToolError> {
    let kind_label = validate_kind(kind)?;
    let name = sanitize_artifact_name(name)?;
    let directory = artifact_dir(kind_label)?;
    let path = directory.join(format!("{name}.json"));
    let content = serde_json::to_string_pretty(payload).map_err(|error| {
        McpToolError::invalid_argument(format!("payload is not serializable: {error}"))
    })?;
    std::fs::write(&path, content).map_err(|error| {
        McpToolError::internal(format!(
            "Failed to write artifact {}: {error}",
            path.display()
        ))
    })?;
    Ok(path)
}

/// Read a SHA-256-pinned source packet from the canonical user-facing research
/// run directory. This changes transport, not authority: the packet's original
/// URLs, discovery completeness and materiality still require independent review.
#[derive(Deserialize, JsonSchema)]
pub struct VerificationPacketRequest {
    /// Research run ID (exactly 16 lowercase hexadecimal characters).
    pub run_id: String,
    /// SHA-256 of the exact packet.json bytes. A changed snapshot fails closed.
    pub expected_sha256: String,
}

const HANDOFF: &str =
    include_str!("../../../../registry/templates/company-research/verification-handoff.j2");
const MAX_VERIFICATION_PACKET_BYTES: u64 = 512 * 1024;

fn evaluate_packet(
    root: &Path,
    run_id: &str,
    expected_sha256: &str,
) -> Result<Value, McpToolError> {
    if run_id.len() != 16
        || !run_id
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    {
        return Err(McpToolError::invalid_argument(
            "run_id must be exactly 16 lowercase hex characters",
        ));
    }
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(McpToolError::invalid_argument(
            "expected_sha256 must be 64 hexadecimal characters",
        ));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        McpToolError::not_found(format!(
            "research-run packet directory unavailable: {error}"
        ))
    })?;
    let file = root.join(run_id).join("packet.json");
    let canonical_file = file.canonicalize().map_err(|error| {
        McpToolError::not_found(format!("research-run packet not found: {error}"))
    })?;
    if !canonical_file.starts_with(&canonical_root) {
        return Err(McpToolError::permission_denied(
            "packet resolves outside the company research-run directory",
        ));
    }
    let metadata = std::fs::metadata(&canonical_file).map_err(|error| {
        McpToolError::unavailable(format!("cannot stat research-run packet: {error}"))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_VERIFICATION_PACKET_BYTES {
        return Err(McpToolError::invalid_argument(
            "packet must be a regular JSON file no larger than 512 KiB",
        ));
    }
    let bytes = std::fs::read(&canonical_file).map_err(|error| {
        McpToolError::unavailable(format!("cannot read research-run packet: {error}"))
    })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(expected_sha256) {
        return Err(McpToolError::failed_precondition(
            "packet digest changed: rebind the exact source and target snapshot",
        ));
    }
    let mut packet: Value = serde_json::from_slice(&bytes)
        .map_err(|error| McpToolError::invalid_argument(format!("invalid packet JSON: {error}")))?;
    let fields = packet
        .as_object_mut()
        .ok_or_else(|| McpToolError::invalid_argument("packet must be a JSON object"))?;
    for field in ["target_text", "issuer_identifier", "as_of_date"] {
        if !fields.get(field).is_some_and(Value::is_string) {
            return Err(McpToolError::invalid_argument(format!(
                "packet is missing string field {field}"
            )));
        }
    }
    for field in [
        "source_outputs",
        "pipeline_tool_log",
        "disclosure_inventory",
    ] {
        if !fields.get(field).is_some_and(Value::is_array) {
            return Err(McpToolError::invalid_argument(format!(
                "packet is missing array field {field}"
            )));
        }
    }
    let disclosures = fields["disclosure_inventory"].clone();
    fields.insert("disclosures".to_string(), disclosures);
    let form = HANDOFF
        .split_once("```source-check-lisp")
        .and_then(|(_, rest)| rest.split_once("```").map(|(form, _)| form.trim()))
        .ok_or_else(|| McpToolError::failed_precondition("shared source-check form missing"))?;
    let mechanical_review = hkask_lisp::eval_sandboxed_with_budget(form, &packet, 1_000_000, 4096)
        .map_err(|error| match error {
            hkask_lisp::LispError::StepLimitExceeded(_)
            | hkask_lisp::LispError::DepthLimitExceeded(_)
            | hkask_lisp::LispError::OutputDepthLimitExceeded(_) => {
                McpToolError::failed_precondition(format!("source check not performed: {error}"))
            }
            _ => McpToolError::invalid_argument(format!("invalid source-check packet: {error}")),
        })?;
    if !mechanical_review
        .as_array()
        .is_some_and(|items| items.len() == 2)
    {
        return Err(McpToolError::failed_precondition(
            "source-check form did not return [coverage_status, known_omission]",
        ));
    }
    Ok(serde_json::json!({
        "run_id": run_id,
        "packet_sha256": digest,
        "mechanical_review": mechanical_review,
        "evidence_mode": "packet_mechanical_only",
        "limitation": "A packet hash does not attest original URL downloads, search completeness, materiality or a verified report; independently reconcile those before binding checked",
    }))
}

#[tool_router(router = artifacts_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Persist a JSON artifact (screen or report) produced by the companies server or a skill."
    )]
    pub async fn report_save(
        &self,
        Parameters(req): Parameters<ReportSaveRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "report_save", async {
            let name = sanitize_artifact_name(&req.name)?;
            let path = save_json_artifact(&req.kind, &name, &req.payload)?;
            Ok(serde_json::json!({
                "saved": true,
                "kind": req.kind,
                "name": name,
                "path": path.to_string_lossy(),
            }))
        })
        .await
    }

    #[tool(
        description = "Load a previously saved JSON artifact (screen or report) by name. Returns the full JSON payload. Returns a not-found error if no artifact with that name exists."
    )]
    pub async fn report_load(
        &self,
        Parameters(req): Parameters<ReportLoadRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "report_load", async {
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
        })
        .await
    }

    #[tool(
        description = "List saved artifact names (without extension) for a kind. Use kind='screen' or kind='report'. Returns a JSON array of names sorted alphabetically."
    )]
    pub async fn report_list(
        &self,
        Parameters(req): Parameters<ReportListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "report_list", async {
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
        })
        .await
    }

    #[tool(
        description = "Execute the shared company-research source check over a SHA-256-pinned packet in companies-mcp/research-runs/{run_id}/packet.json. Mechanical-only: the verifier must independently confirm original downloads, materiality and discovery coverage before using checked."
    )]
    pub async fn company_verification_packet_check(
        &self,
        Parameters(req): Parameters<VerificationPacketRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_verification_packet_check", async move {
            let root =
                resolve_under_artifacts_dir(&mcp_artifacts_subdir("companies", "research-runs"));
            tokio::task::spawn_blocking(move || {
                evaluate_packet(&root, &req.run_id, &req.expected_sha256)
            })
            .await
            .map_err(|error| McpToolError::internal(format!("packet worker failed: {error}")))?
        })
        .await
    }
}

#[cfg(test)]
mod verification_packet_tests {
    use super::{CompaniesServer, evaluate_packet};
    use anyhow::{Context, Result, ensure};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    /// expect: A full retained packet is checked by the production handoff
    /// form, while a changed snapshot and a path escape fail closed.
    #[test]
    fn admitted_packet_rechecks_changed_target_without_leaking_paths() -> Result<()> {
        let root = tempfile::tempdir()?;
        let run_id = "0123456789abcdef";
        let directory = root.path().join(run_id);
        std::fs::create_dir(&directory)?;
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../../registry/company-research-fixtures/verification.json"
        ))?;
        let mut packet = fixtures["packet"].clone();
        let file = directory.join("packet.json");
        let bytes = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &bytes)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let checked = evaluate_packet(root.path(), run_id, &digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            checked["mechanical_review"] == json!(["checked", "not_found_in_reviewed"]),
            "unexpected clean packet result: {checked}"
        );
        packet["target_text"] = json!("ExampleCo report omits the announced partnerships.");
        packet["disclosures"] = json!([]); // Untrusted shortcut must not replace inventory.
        let changed = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &changed)?;
        let stale = evaluate_packet(root.path(), run_id, &digest)
            .err()
            .context("changed packet must refuse prior digest")?;
        ensure!(
            stale.to_string().contains("digest"),
            "wrong stale-packet error: {stale}"
        );
        let revised_digest = format!("{:x}", Sha256::digest(&changed));
        let revised = evaluate_packet(root.path(), run_id, &revised_digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            revised["mechanical_review"] == json!(["material_omission", "material_omission"]),
            "changed report inherited old approval: {revised}"
        );
        Ok(())
    }

    #[test]
    fn packet_check_is_exposed_by_the_production_artifacts_router() -> Result<()> {
        ensure!(
            CompaniesServer::artifacts_router()
                .list_all()
                .iter()
                .any(|tool| tool.name == "company_verification_packet_check"),
            "packet check tool missing from the production router"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_run_cannot_read_outside_packet_root() -> Result<()> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        std::fs::write(outside.path().join("packet.json"), "{}")?;
        std::os::unix::fs::symlink(outside.path(), root.path().join("0123456789abcdef"))?;
        let error = evaluate_packet(root.path(), "0123456789abcdef", &"0".repeat(64))
            .err()
            .context("symlink escape should be rejected")?;
        ensure!(
            error.to_string().contains("outside"),
            "wrong containment error: {error}"
        );
        Ok(())
    }
}
