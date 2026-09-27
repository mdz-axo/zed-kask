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

use super::claim_scan;

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
    /// Run folder name under `companies-mcp/research-runs/`: the readable
    /// `{YYYY-MM-DD}-{company-slug}` the skill created (lowercase letters,
    /// digits and hyphens). The research ledger's `run_id` lives inside the
    /// packet, not in the folder name.
    pub run_folder: String,
    /// SHA-256 of the exact packet.json bytes. A changed snapshot fails closed.
    pub expected_sha256: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PacketBuildRequest {
    /// Existing research-run folder with sources.json, drafts/*.md, key-claims.json.
    pub run_folder: String,
}

const HANDOFF: &str =
    include_str!("../../../../registry/templates/company-research/verification-handoff.j2");
const MAX_VERIFICATION_PACKET_BYTES: u64 = 512 * 1024;
// Lisp charges large source strings at admission and again during each source
// lookup. Bound work in proportion to the admitted packet, not a tiny fixed
// budget that rejects a valid full-original 10-K before checking any claim.
const SOURCE_CHECK_WORK_PER_BYTE: u64 = 32;
// Full originals too large to inline travel as hash-bound sibling files in the
// same run directory; the total admitted text (packet plus files) is bounded.
const MAX_ADMITTED_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

/// Replace each `output.text_file` reference with the file's verified text.
/// Returns the admitted file bytes, counting each distinct file once however
/// many records cite it. The file is a relative path that must resolve inside
/// the packet's own run directory (e.g. `originals/urd.txt`) and match
/// `output.text_sha256` exactly.
fn load_referenced_texts(
    run_dir: &Path,
    fields: &mut serde_json::Map<String, Value>,
) -> Result<u64, McpToolError> {
    let mut admitted = 0u64;
    let mut loaded: std::collections::HashMap<std::path::PathBuf, String> =
        std::collections::HashMap::new();
    let Some(sources) = fields
        .get_mut("source_outputs")
        .and_then(Value::as_array_mut)
    else {
        return Ok(0);
    };
    for source in sources {
        let Some(output) = source.get_mut("output").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(name) = output.get("text_file") else {
            continue;
        };
        let name = name
            .as_str()
            .filter(|n| {
                let path = Path::new(n);
                !n.is_empty()
                    && !n.contains('\\')
                    && path.is_relative()
                    && path
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
            })
            .ok_or_else(|| {
                McpToolError::invalid_argument(
                    "output.text_file must be a relative path inside the run directory",
                )
            })?
            .to_string();
        let expected = output
            .get("text_sha256")
            .and_then(Value::as_str)
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| {
                McpToolError::invalid_argument(format!(
                    "text_file {name} requires a 64-hex text_sha256"
                ))
            })?
            .to_ascii_lowercase();
        if output.contains_key("text") {
            return Err(McpToolError::invalid_argument(format!(
                "source with text_file {name} must not also carry inline text"
            )));
        }
        let path = run_dir.join(&name).canonicalize().map_err(|error| {
            McpToolError::not_found(format!("referenced text {name} not found: {error}"))
        })?;
        if !path.starts_with(run_dir) || !path.is_file() {
            return Err(McpToolError::permission_denied(format!(
                "referenced text {name} resolves outside the run directory"
            )));
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            McpToolError::unavailable(format!("cannot read referenced text {name}: {error}"))
        })?;
        // Every citing record is checked against its own declared digest.
        if format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(McpToolError::failed_precondition(format!(
                "referenced text {name} digest changed: rebind the exact source snapshot"
            )));
        }
        let text = match loaded.get(&path) {
            Some(text) => text.clone(),
            None => {
                admitted += bytes.len() as u64;
                if admitted > MAX_ADMITTED_SOURCE_BYTES {
                    return Err(McpToolError::invalid_argument(
                        "referenced source texts exceed the 8 MiB admitted total",
                    ));
                }
                let text = String::from_utf8(bytes).map_err(|_| {
                    McpToolError::invalid_argument(format!("referenced text {name} is not UTF-8"))
                })?;
                loaded.insert(path, text.clone());
                text
            }
        };
        output.insert("text".to_string(), Value::String(text));
    }
    Ok(admitted)
}

const MAX_KEY_CLAIMS: usize = 25;

/// Tier 1: scan every cited figure and quote in the target against its source.
/// Tier 2 preparation: check each key claim's evidence quote is verbatim in its
/// cited source, so the independent verifier starts from a mechanically sound
/// list. Neither attests context, materiality or truth.
fn tier_checks(fields: &serde_json::Map<String, Value>) -> Result<(Value, Value), McpToolError> {
    let mut sources = std::collections::HashMap::new();
    for source in fields["source_outputs"].as_array().into_iter().flatten() {
        let key = source.get("output_key").and_then(Value::as_str);
        let output = source.get("output");
        let text = output
            .and_then(|o| o.get("text").or_else(|| o.get("content")))
            .and_then(Value::as_str);
        if let (Some(key), Some(text)) = (key, text) {
            sources.insert(key.to_string(), text.to_string());
        }
    }
    let target = fields["target_text"].as_str().unwrap_or_default();
    let scan = claim_scan::scan(target, &sources);
    let Some(claims) = fields.get("key_claims") else {
        return Ok((
            scan,
            serde_json::json!({"status": "not_supplied", "note": "packet has no key_claims; Tier 2 cannot be scoped"}),
        ));
    };
    let claims = claims
        .as_array()
        .ok_or_else(|| McpToolError::invalid_argument("key_claims must be an array"))?;
    if claims.is_empty() || claims.len() > MAX_KEY_CLAIMS {
        return Err(McpToolError::invalid_argument(format!(
            "key_claims must list 1 to {MAX_KEY_CLAIMS} load-bearing claims"
        )));
    }
    let normalized_target = claim_scan::normalize(target);
    let mut rows = Vec::new();
    let mut problems = 0usize;
    for claim in claims {
        let get = |k: &str| claim.get(k).and_then(Value::as_str).unwrap_or_default();
        let (id, text, key, quote) = (get("id"), get("claim"), get("output_key"), get("quote"));
        if id.is_empty() || text.is_empty() || key.is_empty() || quote.is_empty() {
            return Err(McpToolError::invalid_argument(
                "each key claim needs id, claim, role, output_key and quote",
            ));
        }
        let in_target = normalized_target.contains(&claim_scan::normalize(text));
        let quote_found = sources
            .get(key)
            .map(|s| claim_scan::normalize(s).contains(&claim_scan::normalize(quote)));
        let status = match quote_found {
            Some(true) if in_target => "evidence_found",
            Some(true) => "claim_not_in_target",
            Some(false) => "quote_not_in_source",
            None => "source_not_retained",
        };
        if status != "evidence_found" {
            problems += 1;
        }
        rows.push(
            serde_json::json!({"id": id, "role": get("role"), "output_key": key, "status": status}),
        );
    }
    Ok((
        scan,
        serde_json::json!({
            "status": if problems == 0 { "evidence_found" } else { "problems" },
            "count": rows.len(),
            "problems": problems,
            "claims": rows,
            "note": "Mechanical pre-check only: each quote is verbatim in its cited source and the claim text appears in the target. The independent verifier still judges support, context and materiality for every key claim.",
        }),
    ))
}

/// Build a packet only from files contained in one research run. The manifest
/// declares provenance; this operation binds bytes, not independent authority.
fn run_file(run_dir: &Path, name: &str) -> Result<std::path::PathBuf, McpToolError> {
    let relative = Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || !relative.is_relative()
        || !relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(McpToolError::invalid_argument(
            "source path must be a relative path inside the run directory",
        ));
    }
    let path = run_dir
        .join(relative)
        .canonicalize()
        .map_err(|error| McpToolError::not_found(format!("missing run source {name}: {error}")))?;
    if !path.starts_with(run_dir) || !path.is_file() {
        return Err(McpToolError::permission_denied(format!(
            "source path {name} resolves outside the run directory"
        )));
    }
    Ok(path)
}

fn read_run_json(run_dir: &Path, name: &str) -> Result<Value, McpToolError> {
    let bytes = std::fs::read(run_file(run_dir, name)?)
        .map_err(|error| McpToolError::unavailable(format!("cannot read {name}: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| McpToolError::invalid_argument(format!("invalid {name}: {error}")))
}

fn build_packet(root: &Path, run_folder: &str) -> Result<Value, McpToolError> {
    // Use the packet check's folder policy before touching any caller path.
    if run_folder.is_empty()
        || run_folder.len() > 80
        || run_folder.starts_with('-')
        || !run_folder
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(McpToolError::invalid_argument(
            "invalid research run_folder",
        ));
    }
    let root = root
        .canonicalize()
        .map_err(|error| McpToolError::not_found(format!("research-run root: {error}")))?;
    let run_dir = root
        .join(run_folder)
        .canonicalize()
        .map_err(|error| McpToolError::not_found(format!("research run not found: {error}")))?;
    if !run_dir.starts_with(&root) || !run_dir.is_dir() {
        return Err(McpToolError::permission_denied(
            "run folder resolves outside research-run root",
        ));
    }
    let manifest = read_run_json(&run_dir, "sources.json")?;
    let issuer = manifest
        .get("issuer_identifier")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| McpToolError::invalid_argument("sources.json needs issuer_identifier"))?;
    let as_of = manifest
        .get("as_of_date")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| McpToolError::invalid_argument("sources.json needs as_of_date"))?;
    let declared = manifest
        .get("sources")
        .and_then(Value::as_array)
        .filter(|sources| !sources.is_empty())
        .ok_or_else(|| {
            McpToolError::invalid_argument("sources.json needs a nonempty sources array")
        })?;
    let mut source_outputs = Vec::new();
    let mut pipeline_tool_log = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for source in declared {
        let required = |field| {
            source
                .get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| McpToolError::invalid_argument(format!("source missing {field}")))
        };
        let key = required("output_key")?;
        if !keys.insert(key) {
            return Err(McpToolError::invalid_argument(format!(
                "duplicate output_key {key}"
            )));
        }
        let name = required("text_file")?;
        let text = std::fs::read(run_file(&run_dir, name)?).map_err(|error| {
            McpToolError::unavailable(format!("cannot read source text {name}: {error}"))
        })?;
        String::from_utf8(text.clone()).map_err(|_| {
            McpToolError::invalid_argument(format!("source text {name} is not UTF-8"))
        })?;
        let method = required("method")?;
        let tool = source
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or(match method {
                "text_extraction" => "corpus_convert",
                "pdftotext" => "pdftotext",
                _ => "retained_tool_output",
            });
        let mut record = serde_json::json!({
            "tool_name":tool, "description":source.get("description").and_then(Value::as_str).unwrap_or("Retained run source"),
            "output_key":key, "source_kind":source.get("source_kind").and_then(Value::as_str).unwrap_or(if method == "tool_response" {"derived"} else {"original"}),
            "url":source.get("url").cloned().unwrap_or(Value::Null),
            "retrieved_at":source.get("retrieved_at").cloned().unwrap_or(Value::Null),
            "period":source.get("period").cloned().unwrap_or(Value::Null),
            "unit":source.get("unit").cloned().unwrap_or(Value::Null),
            "output":{"method":method,"text_file":name,"text_sha256":format!("{:x}", Sha256::digest(&text))}
        });
        let mut log = serde_json::json!({"tool_name":tool,"output_key":key,"status":"ok"});
        if let Some(origin) = source.get("origin_path").and_then(Value::as_str) {
            let path = run_file(&run_dir, origin)?;
            let bytes = std::fs::read(&path).map_err(|error| {
                McpToolError::unavailable(format!("cannot read origin {origin}: {error}"))
            })?;
            let digest = format!("{:x}", Sha256::digest(&bytes));
            let path = path.to_string_lossy().to_string();
            record["origin_path"] = Value::String(path.clone());
            record["source_sha256"] = Value::String(digest.clone());
            log["origin_path"] = Value::String(path);
            log["source_sha256"] = Value::String(digest);
        } else if method == "text_extraction" || method == "pdftotext" {
            return Err(McpToolError::invalid_argument(format!(
                "source {key} requires origin_path"
            )));
        }
        source_outputs.push(record);
        pipeline_tool_log.push(log);
    }
    let draft_dir = run_file_directory(&run_dir, "drafts")?;
    let mut drafts = std::fs::read_dir(&draft_dir)
        .map_err(|error| McpToolError::unavailable(format!("cannot read drafts: {error}")))?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| McpToolError::unavailable(format!("cannot list drafts: {error}")))?;
    drafts.retain(|name| name.ends_with(".md"));
    drafts.sort();
    if drafts.is_empty() {
        return Err(McpToolError::invalid_argument(
            "drafts/ needs at least one .md file",
        ));
    }
    let mut target = String::new();
    for draft in &drafts {
        let name = format!("drafts/{draft}");
        let text = std::fs::read_to_string(run_file(&run_dir, &name)?)
            .map_err(|error| McpToolError::unavailable(format!("cannot read {name}: {error}")))?;
        target.push_str(&format!("\n## {name}\n{text}\n"));
    }
    let key_claims = read_run_json(&run_dir, "key-claims.json")?;
    let optional = |field: &str| -> Result<Value, McpToolError> {
        match manifest.get(field).and_then(Value::as_str) {
            Some(name) => read_run_json(&run_dir, name),
            None => Ok(Value::Array(vec![])),
        }
    };
    let packet = serde_json::json!({
        "issuer_identifier":issuer, "as_of_date":as_of, "target_text":target,
        "source_outputs":source_outputs, "pipeline_tool_log":pipeline_tool_log,
        "key_claims":key_claims, "disclosure_inventory":optional("disclosure_inventory")?,
        "historical_findings":optional("historical_findings")?,
        "congruence_rules":optional("congruence_rules")?, "leak_rules":optional("leak_rules")?,
        "original_forecast":Value::Null, "working_forecast":Value::Null,
        "research_run_id":manifest.get("research_run_id").cloned().unwrap_or(Value::Null),
        "frozen_drafts":drafts
    });
    let bytes = serde_json::to_vec(&packet)
        .map_err(|error| McpToolError::internal(format!("cannot serialize packet: {error}")))?;
    if bytes.len() as u64 > MAX_VERIFICATION_PACKET_BYTES {
        return Err(McpToolError::invalid_argument(
            "packet exceeds 512 KiB; retain source texts in files",
        ));
    }
    let staging = run_dir.join(format!(".packet-{}.tmp", uuid::Uuid::new_v4()));
    let staged = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&staging, run_dir.join("packet.json"))
    })();
    if let Err(error) = staged {
        if let Err(cleanup) = std::fs::remove_file(&staging) {
            if cleanup.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("could not remove failed packet staging file: {cleanup}");
            }
        }
        return Err(McpToolError::unavailable(format!(
            "cannot publish packet: {error}"
        )));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    evaluate_packet(&root, run_folder, &digest)
}

fn run_file_directory(run_dir: &Path, name: &str) -> Result<std::path::PathBuf, McpToolError> {
    let path = run_dir
        .join(name)
        .canonicalize()
        .map_err(|error| McpToolError::not_found(format!("missing {name}: {error}")))?;
    if !path.starts_with(run_dir) || !path.is_dir() {
        return Err(McpToolError::permission_denied(format!(
            "{name} resolves outside the run directory"
        )));
    }
    Ok(path)
}

fn evaluate_packet(
    root: &Path,
    run_folder: &str,
    expected_sha256: &str,
) -> Result<Value, McpToolError> {
    if run_folder.is_empty()
        || run_folder.len() > 80
        || run_folder.starts_with('-')
        || !run_folder
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(McpToolError::invalid_argument(
            "run_folder must be the readable run folder name: lowercase letters, digits and hyphens, e.g. 2026-09-26-viridien",
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
    let file = root.join(run_folder).join("packet.json");
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
    let run_dir = canonical_file
        .parent()
        .ok_or_else(|| McpToolError::internal("packet has no run directory"))?;
    let referenced_bytes = load_referenced_texts(run_dir, fields)?;
    let work_ceiling = (metadata.len() + referenced_bytes) * SOURCE_CHECK_WORK_PER_BYTE
        + MAX_VERIFICATION_PACKET_BYTES * SOURCE_CHECK_WORK_PER_BYTE;
    let (claim_scan, key_claims) = tier_checks(fields)?;
    let disclosures = fields["disclosure_inventory"].clone();
    fields.insert("disclosures".to_string(), disclosures);
    let form = HANDOFF
        .split_once("```source-check-lisp")
        .and_then(|(_, rest)| rest.split_once("```").map(|(form, _)| form.trim()))
        .ok_or_else(|| McpToolError::failed_precondition("shared source-check form missing"))?;
    let mechanical_review = hkask_lisp::eval_sandboxed_with_budget(
        form,
        &packet,
        work_ceiling,
        4096,
    )
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
        "run_folder": run_folder,
        "packet_sha256": digest,
        "referenced_text_bytes": referenced_bytes,
        "mechanical_review": mechanical_review,
        "tier1_claim_scan": claim_scan,
        "tier2_key_claims": key_claims,
        "evidence_mode": "packet_mechanical_only",
        "source_review_status": "not_checked",
        "independent_source_review_required": true,
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
        description = "Build a hash-bound research packet from sources.json, drafts/*.md and key-claims.json in an existing run folder; return its digest and the mechanical packet check in one call."
    )]
    pub async fn company_research_packet_build(
        &self,
        Parameters(req): Parameters<PacketBuildRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_research_packet_build", async move {
            let root =
                resolve_under_artifacts_dir(&mcp_artifacts_subdir("companies", "research-runs"));
            tokio::task::spawn_blocking(move || build_packet(&root, &req.run_folder))
                .await
                .map_err(|error| McpToolError::internal(format!("packet worker failed: {error}")))?
        })
        .await
    }

    #[tool(
        description = "Execute the shared company-research source check over a SHA-256-pinned packet in companies-mcp/research-runs/{run_folder}/packet.json, where run_folder is the readable {YYYY-MM-DD}-{company} folder name. Mechanical-only: the verifier must independently confirm original downloads, materiality and discovery coverage before using checked."
    )]
    pub async fn company_verification_packet_check(
        &self,
        Parameters(req): Parameters<VerificationPacketRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_verification_packet_check", async move {
            let root =
                resolve_under_artifacts_dir(&mcp_artifacts_subdir("companies", "research-runs"));
            tokio::task::spawn_blocking(move || {
                evaluate_packet(&root, &req.run_folder, &req.expected_sha256)
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
        let run_id = "2026-09-26-exampleco";
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

    /// expect: A valid full-original packet near the admitted size limit can
    /// complete the mechanical source check; its result never attests the URL.
    #[test]
    fn full_original_packet_reaches_mechanical_review_without_attesting_source() -> Result<()> {
        let root = tempfile::tempdir()?;
        let run_id = "2026-09-26-exampleco";
        let directory = root.path().join(run_id);
        std::fs::create_dir(&directory)?;
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../../registry/company-research-fixtures/verification.json"
        ))?;
        let mut packet = fixtures["packet"].clone();
        let original = "This is synthetic filing context. ".repeat(13_000);
        let original = format!("{original}We announced partnerships.");
        packet["source_outputs"][0]["tool_name"] = json!("corpus_convert");
        packet["source_outputs"][0]["output"] =
            json!({"method": "text_extraction", "text": original});
        packet["pipeline_tool_log"][0]["tool_name"] = json!("corpus_convert");
        let source_path = directory.join("synthetic-source.docx");
        std::fs::write(&source_path, b"Synthetic source bytes, not a real DOCX")?;
        let source_sha = format!("{:x}", Sha256::digest(std::fs::read(&source_path)?));
        let source_path = source_path.to_string_lossy().into_owned();
        packet["source_outputs"][0]["origin_path"] = json!(source_path);
        packet["source_outputs"][0]["source_sha256"] = json!(source_sha);
        packet["pipeline_tool_log"][0]["origin_path"] = json!(source_path);
        packet["pipeline_tool_log"][0]["source_sha256"] = json!(source_sha);
        let file = directory.join("packet.json");
        let bytes = serde_json::to_vec(&packet)?;
        ensure!(
            bytes.len() > 400_000 && bytes.len() <= 512 * 1024,
            "fixture must exercise a full-sized admitted packet: {} bytes",
            bytes.len()
        );
        std::fs::write(&file, &bytes)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let checked = evaluate_packet(root.path(), run_id, &digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            checked["mechanical_review"] == json!(["checked", "not_found_in_reviewed"])
                && checked["evidence_mode"] == "packet_mechanical_only"
                && checked["source_review_status"] == "not_checked",
            "full-sized packet did not receive bounded mechanical-only review: {checked}"
        );
        packet["disclosure_inventory"][0]["quote"] = json!("Quote absent from source");
        let absent = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &absent)?;
        let absent_digest = format!("{:x}", Sha256::digest(&absent));
        let unchecked = evaluate_packet(root.path(), run_id, &absent_digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            unchecked["mechanical_review"] == json!(["not_checked", "not_checked"]),
            "absent original quote was falsely checked: {unchecked}"
        );
        Ok(())
    }

    /// expect: Full originals larger than the inline packet limit travel as
    /// hash-bound run-directory files, are checked in full, and a changed or
    /// escaping file fails closed.
    #[test]
    fn hash_bound_text_file_carries_full_original_beyond_packet_limit() -> Result<()> {
        let root = tempfile::tempdir()?;
        let run_id = "2026-09-26-exampleco";
        let directory = root.path().join(run_id);
        std::fs::create_dir(&directory)?;
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../../registry/company-research-fixtures/verification.json"
        ))?;
        let mut packet = fixtures["packet"].clone();
        let original = format!(
            "{}We announced partnerships.",
            "This is synthetic filing context. ".repeat(20_000)
        );
        ensure!(
            original.len() > 512 * 1024,
            "fixture must exceed inline limit"
        );
        let text_path = directory.join("original.txt");
        std::fs::write(&text_path, &original)?;
        let text_sha = format!("{:x}", Sha256::digest(original.as_bytes()));
        let source_path = directory.join("synthetic-source.docx");
        std::fs::write(&source_path, b"Synthetic source bytes")?;
        let source_sha = format!("{:x}", Sha256::digest(std::fs::read(&source_path)?));
        let source_path = source_path.to_string_lossy().into_owned();
        packet["source_outputs"][0]["tool_name"] = json!("corpus_convert");
        packet["source_outputs"][0]["output"] = json!({"method": "text_extraction",
            "text_file": "original.txt", "text_sha256": text_sha});
        packet["source_outputs"][0]["origin_path"] = json!(source_path);
        packet["source_outputs"][0]["source_sha256"] = json!(source_sha);
        packet["pipeline_tool_log"][0]["tool_name"] = json!("corpus_convert");
        packet["pipeline_tool_log"][0]["origin_path"] = json!(source_path);
        packet["pipeline_tool_log"][0]["source_sha256"] = json!(source_sha);
        let file = directory.join("packet.json");
        let bytes = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &bytes)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let checked = evaluate_packet(root.path(), run_id, &digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            checked["mechanical_review"] == json!(["checked", "not_found_in_reviewed"])
                && checked["referenced_text_bytes"] == json!(original.len())
                && checked["source_review_status"] == "not_checked",
            "file-referenced original was not fully checked: {checked}"
        );
        std::fs::write(&text_path, format!("{original} tampered"))?;
        let tampered = evaluate_packet(root.path(), run_id, &digest)
            .err()
            .context("changed referenced text must fail closed")?;
        ensure!(
            tampered.to_string().contains("digest"),
            "wrong error: {tampered}"
        );
        // A retained original under a subfolder, cited by two records, is
        // admitted and counted once toward the cap.
        std::fs::write(&text_path, &original)?;
        std::fs::create_dir(directory.join("originals"))?;
        std::fs::rename(&text_path, directory.join("originals/original.txt"))?;
        packet["source_outputs"][0]["output"]["text_file"] = json!("originals/original.txt");
        let second = packet["source_outputs"][0].clone();
        packet["source_outputs"]
            .as_array_mut()
            .context("source outputs")?
            .push(second);
        let nested = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &nested)?;
        let nested_digest = format!("{:x}", Sha256::digest(&nested));
        let nested_checked = evaluate_packet(root.path(), run_id, &nested_digest)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        ensure!(
            nested_checked["referenced_text_bytes"] == json!(original.len()),
            "a file cited twice must be counted once: {nested_checked}"
        );
        packet["source_outputs"][0]["output"]["text_file"] = json!("../escape.txt");
        let escaping = serde_json::to_vec(&packet)?;
        std::fs::write(&file, &escaping)?;
        let escaping_digest = format!("{:x}", Sha256::digest(&escaping));
        ensure!(
            evaluate_packet(root.path(), run_id, &escaping_digest).is_err(),
            "path-escaping text_file must be rejected"
        );
        Ok(())
    }

    /// expect: rebuilding a frozen pair of drafts binds the exact changed words
    /// to a new packet digest and returns the same mechanical check as the verifier.
    #[test]
    fn packet_builder_rebinds_changed_draft() -> Result<()> {
        let root = tempfile::tempdir()?;
        let run = "2026-09-26-exampleco";
        let dir = root.path().join(run);
        std::fs::create_dir_all(dir.join("drafts"))?;
        std::fs::write(dir.join("original.txt"), "ExampleCo revenue was 100 USD.")?;
        std::fs::write(
            dir.join("drafts/report.md"),
            "Revenue was 100 USD. [filing]",
        )?;
        std::fs::write(dir.join("drafts/summary.md"), "ExampleCo summary.")?;
        std::fs::write(dir.join("key-claims.json"), json!([{"id":"revenue","claim":"Revenue was 100 USD","role":"valuation_input","output_key":"filing","quote":"revenue was 100 USD"}]).to_string())?;
        std::fs::write(dir.join("sources.json"), json!({
            "issuer_identifier":"ExampleCo", "as_of_date":"2026-09-26",
            "sources":[{"output_key":"filing", "url":"https://example.invalid/filing", "origin_path":"original.txt", "text_file":"original.txt", "method":"text_extraction", "period":"FY2025"}]
        }).to_string())?;
        let first = super::build_packet(root.path(), run).map_err(|e| anyhow::anyhow!("{e}"))?;
        ensure!(
            first["packet_sha256"].as_str().is_some(),
            "no digest: {first}"
        );
        ensure!(
            first["tier2_key_claims"]["status"] == "evidence_found",
            "missing precheck: {first}"
        );
        std::fs::write(dir.join("drafts/summary.md"), "ExampleCo changed summary.")?;
        let second = super::build_packet(root.path(), run).map_err(|e| anyhow::anyhow!("{e}"))?;
        ensure!(
            first["packet_sha256"] != second["packet_sha256"],
            "changed draft kept digest"
        );
        ensure!(
            std::fs::read_to_string(dir.join("packet.json"))?.contains("changed summary"),
            "new draft absent"
        );
        Ok(())
    }

    /// expect: missing or escaping source paths never create a checkable packet.
    #[test]
    fn packet_builder_rejects_missing_and_escaping_text() -> Result<()> {
        let root = tempfile::tempdir()?;
        let run = "2026-09-26-exampleco";
        let dir = root.path().join(run);
        std::fs::create_dir_all(dir.join("drafts"))?;
        std::fs::write(dir.join("drafts/report.md"), "ExampleCo report.")?;
        std::fs::write(dir.join("key-claims.json"), "[]")?;
        let manifest = |name: &str| {
            json!({"issuer_identifier":"ExampleCo", "as_of_date":"2026-09-26", "sources":[{"output_key":"filing", "url":"https://example.invalid", "origin_path":name, "text_file":name, "method":"text_extraction", "period":"FY2025"}]}).to_string()
        };
        std::fs::write(dir.join("sources.json"), manifest("missing.txt"))?;
        let missing = super::build_packet(root.path(), run)
            .err()
            .context("missing text accepted")?;
        ensure!(
            missing.to_string().contains("missing"),
            "unexpected error: {missing}"
        );
        ensure!(
            !dir.join("packet.json").exists(),
            "partial packet left after failure"
        );
        std::fs::write(root.path().join("outside.txt"), "ExampleCo")?;
        std::fs::write(dir.join("sources.json"), manifest("../outside.txt"))?;
        let escape = super::build_packet(root.path(), run)
            .err()
            .context("outside text accepted")?;
        ensure!(
            escape.to_string().contains("inside the run"),
            "unexpected error: {escape}"
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
