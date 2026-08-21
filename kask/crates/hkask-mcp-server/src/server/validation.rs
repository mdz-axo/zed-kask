//! Input validation — shared sanitization for MCP tool parameters.

use super::error::McpToolError;

/// Validate a string identifier.
/// Validate an identifier (tool name, server name, etc.).
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  name and value are non-empty, max_len > 0
/// post: returns Ok(()) if valid (non-empty, ≤max_len, alphanumeric+hyphen+underscore+dot+colon)
/// post: returns Err if invalid
#[must_use = "result must be used"]
pub fn validate_identifier(name: &str, value: &str, max_len: usize) -> Result<(), McpToolError> {
    if value.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not be empty"
        )));
    }
    if value.len() > max_len {
        return Err(McpToolError::invalid_argument(format!(
            "{name} exceeds maximum length of {max_len} (got {})",
            value.len()
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-' || c == ':')
    {
        return Err(McpToolError::invalid_argument(format!(
            "{name} contains invalid characters (allowed: alphanumeric, _, ., -, :)"
        )));
    }
    Ok(())
}

/// Validate a filesystem path without restricting legitimate filename punctuation.
///
/// expect: "The system validates tool input against safety and length constraints"
/// pre:  name and value are non-empty, max_len > 0
/// post: returns Ok(()) if valid
/// post: returns Err if invalid
#[must_use = "result must be used"]
pub fn validate_path(name: &str, value: &str, max_len: usize) -> Result<(), McpToolError> {
    if value.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not be empty"
        )));
    }
    if value.len() > max_len {
        return Err(McpToolError::invalid_argument(format!(
            "{name} exceeds maximum length of {max_len} (got {})",
            value.len()
        )));
    }
    if value.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(McpToolError::invalid_argument(format!(
            "{name} contains a NUL or control character"
        )));
    }
    if std::path::Path::new(value)
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must not contain parent-directory traversal"
        )));
    }
    Ok(())
}

/// Classify a `std::io::Error` from a caller-facing file operation into the
/// appropriate `McpToolError` kind.
///
/// `NotFound` and `PermissionDenied` are caller-fixable (the user supplied a
/// missing path or lacks access), so they map to `not_found` /
/// `permission_denied` rather than `internal`. Other IO failures remain genuine
/// system errors. This is the canonical per-variant IO-error mapper for MCP
/// tool file operations — reuse it instead of re-implementing
/// `McpToolError::internal(format!("...: {e}"))` (which mis-classifies
/// caller-fixable errors as Internal).
#[must_use = "result must be used"]
pub fn map_io_error(e: std::io::Error, context: &str) -> McpToolError {
    match e.kind() {
        std::io::ErrorKind::NotFound => McpToolError::not_found(format!("{context}: {e}")),
        std::io::ErrorKind::PermissionDenied => {
            McpToolError::permission_denied(format!("{context}: {e}"))
        }
        _ => McpToolError::internal(format!("{context}: {e}")),
    }
}

/// Classify a `tokio::task::JoinError` from a `spawn_blocking` task into the
/// MCP wire-level `McpToolError` kind: cancellation → `unavailable` (the task
/// could not run to completion), panic → `internal` (a bug in the task body).
/// Replaces the blanket `internal(format!("... task failed: {e}"))` that
/// flattened both variants to Internal.
#[must_use = "result must be used"]
pub fn map_join_error(error: tokio::task::JoinError, context: &str) -> McpToolError {
    if error.is_cancelled() {
        McpToolError::unavailable(format!("{context}: task cancelled"))
    } else {
        McpToolError::internal(format!("{context}: {error}"))
    }
}

/// Classify an `hkask_types::InfrastructureError` from a storage-layer query
/// into the MCP wire-level `McpToolError` kind: `NotFound` → `not_found`,
/// database connection failures → `unavailable`, lock poisoning → `internal`
/// (a panic happened while holding the lock), serialization/IO/query failures
/// → `internal`. Replaces the blanket `internal(format!("...: {e}"))` that
/// flattened caller-fixable `NotFound`s and transient connection failures to
/// Internal.
#[must_use = "result must be used"]
pub fn map_infra_error(error: &hkask_types::InfrastructureError, context: &str) -> McpToolError {
    let message = format!("{context}: {error}");
    match error {
        hkask_types::InfrastructureError::NotFound(_) => McpToolError::not_found(message),
        hkask_types::InfrastructureError::Database {
            kind: hkask_types::DatabaseErrorKind::Connection,
            ..
        } => McpToolError::unavailable(message),
        hkask_types::InfrastructureError::Database { .. }
        | hkask_types::InfrastructureError::Serialization(_)
        | hkask_types::InfrastructureError::LockPoisoned
        | hkask_types::InfrastructureError::Io(_) => McpToolError::internal(message),
        // Non-exhaustive enum: future variants stay internal (conservative).
        _ => McpToolError::internal(message),
    }
}

/// Classify a `MemoryStoreError` from a memory-DB operation into the
/// appropriate `McpToolError` kind. Infrastructure variants (HMem/Embedding
/// wrapping an `InfrastructureError`) route through [`map_infra_error`];
/// missing entities and centroid embeddings are `not_found`; remaining
/// embedding failures are `internal`. Canonical mapper shared by the corpus
/// and training servers — reuse it instead of re-implementing per-crate
/// copies.
#[must_use = "result must be used"]
pub fn map_memory_store_error(
    error: hkask_memory::MemoryStoreError,
    context: &str,
) -> McpToolError {
    use hkask_memory::MemoryStoreError;
    match error {
        MemoryStoreError::HMem(hkask_storage::HMemError::NotFound(_)) => {
            McpToolError::not_found(format!("{context}: {error}"))
        }
        MemoryStoreError::HMem(hkask_storage::HMemError::Infra(ref infra)) => {
            map_infra_error(infra, context)
        }
        MemoryStoreError::Embedding(hkask_storage::EmbeddingError::NotFound(_)) => {
            McpToolError::not_found(format!("{context}: {error}"))
        }
        MemoryStoreError::Embedding(hkask_storage::EmbeddingError::Infrastructure(ref infra)) => {
            map_infra_error(infra, context)
        }
        MemoryStoreError::NoEmbeddingsForCentroid(_) => {
            McpToolError::not_found(format!("{context}: {error}"))
        }
        MemoryStoreError::Embedding(_) => McpToolError::internal(format!("{context}: {error}")),
    }
}

/// Default read size cap for [`read_capped`] (32 MiB). Bounds a hostile or
/// mistaken path from exhausting memory (CWE-400).
///
/// Override: `HKASK_MCP_MAX_READ_BYTES` env var (parsed as u64 bytes).
pub const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;

/// Resolve the effective read size cap from env var or default.
///
/// Reads `HKASK_MCP_MAX_READ_BYTES` and parses as u64 bytes. Falls back to
/// `MAX_READ_BYTES` if unset or unparsable (with a `warn!` on parse failure
/// per `.rules`).
#[must_use]
pub fn resolve_max_read_bytes() -> u64 {
    match std::env::var("HKASK_MCP_MAX_READ_BYTES") {
        Ok(val) => match val.parse::<u64>() {
            Ok(0) => {
                tracing::warn!(
                    target: "hkask.mcp_server",
                    env_var = "HKASK_MCP_MAX_READ_BYTES",
                    value = %val,
                    "Parsed as 0 — using default (a zero cap would reject all reads)"
                );
                MAX_READ_BYTES
            }
            Ok(n) => n,
            Err(_) => {
                tracing::warn!(
                    target: "hkask.mcp_server",
                    env_var = "HKASK_MCP_MAX_READ_BYTES",
                    value = %val,
                    "Failed to parse as u64 — using default"
                );
                MAX_READ_BYTES
            }
        },
        Err(_) => MAX_READ_BYTES,
    }
}

/// Canonicalize `path` for a target that may not exist yet (writes): resolve
/// the nearest existing ancestor, then re-append the remaining components.
/// Reads use `Path::canonicalize` directly (the target must exist).
fn canonicalize_lenient(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Ok(canonical),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut ancestor = path;
            let mut suffix: Vec<&std::ffi::OsStr> = Vec::new();
            loop {
                match ancestor.parent() {
                    Some(parent) => {
                        if let Some(name) = ancestor.file_name() {
                            suffix.push(name);
                        }
                        ancestor = parent;
                        if ancestor.exists() {
                            break;
                        }
                    }
                    None => return Err(e),
                }
            }
            let mut resolved = ancestor.canonicalize()?;
            for component in suffix.iter().rev() {
                resolved.push(component);
            }
            Ok(resolved)
        }
        Err(e) => Err(e),
    }
}

fn rejection(path: &std::path::Path, root: &std::path::Path, reason: &str) -> McpToolError {
    tracing::warn!(
        target: "hkask.mcp.path_safety",
        path = %path.display(),
        root = %root.display(),
        reason = %reason,
        "Path rejected by containment check — refusing file operation outside the project root"
    );
    McpToolError::invalid_argument(format!(
        "Path '{}' is outside the allowed root '{}': {}",
        path.display(),
        root.display(),
        reason
    ))
}

/// Contain `path` under the process current working directory (the project
/// root when the MCP server is launched per-project via `ContextServerStore`,
/// or zed's launch cwd for the app-global `McpRuntime` spawn — fail-safe in
/// both cases). Canonicalization collapses symlink escapes. Absolute paths
/// like `/etc/passwd` and traversals like `../../escape` are rejected.
fn contain(path: &std::path::Path, write: bool) -> Result<std::path::PathBuf, McpToolError> {
    let root = std::env::current_dir()
        .and_then(|cwd| cwd.canonicalize())
        .map_err(|e| McpToolError::internal(format!("Cannot resolve working directory: {e}")))?;

    let resolved = if write {
        canonicalize_lenient(path)
    } else {
        path.canonicalize()
    }
    .map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot resolve path '{}': {e}", path.display()))
    })?;

    if !resolved.starts_with(&root) {
        return Err(rejection(path, &root, "path escapes the project root"));
    }
    Ok(resolved)
}

/// Resolve a caller-supplied write target, rejecting anything outside the
/// project root (CWE-73). The target need not exist yet.
#[must_use = "result must be used"]
pub fn contain_for_write(path: &str) -> Result<std::path::PathBuf, McpToolError> {
    contain(std::path::Path::new(path), true)
}

/// Resolve a caller-supplied read path, rejecting anything outside the
/// project root (CWE-22/CWE-200). The target must exist.
#[must_use = "result must be used"]
pub fn contain_for_read(path: &str) -> Result<std::path::PathBuf, McpToolError> {
    contain(std::path::Path::new(path), false)
}

/// Read a caller-supplied file with containment and a size cap, so a hostile
/// or mistaken path cannot exfiltrate arbitrary files (CWE-200) or exhaust
/// memory (CWE-400). Combines [`contain_for_read`] with a metadata size check
/// before the read.
#[must_use = "result must be used"]
pub fn read_capped(path: &str, max_bytes: u64) -> Result<Vec<u8>, McpToolError> {
    let resolved = contain_for_read(path)?;
    let metadata = std::fs::metadata(&resolved)
        .map_err(|e| map_io_error(e, &format!("Cannot stat file '{}'", resolved.display())))?;
    if metadata.len() > max_bytes {
        tracing::warn!(
            target: "hkask.mcp.path_safety",
            path = %resolved.display(),
            size = metadata.len(),
            cap = max_bytes,
            "Read rejected — file exceeds size cap"
        );
        return Err(McpToolError::invalid_argument(format!(
            "File '{}' is {} bytes, exceeding the {} byte read cap",
            resolved.display(),
            metadata.len(),
            max_bytes
        )));
    }
    std::fs::read(&resolved)
        .map_err(|e| map_io_error(e, &format!("Failed to read file '{}'", path)))
}
