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

/// The running server's artifact owner (e.g. `corpus` for `hkask-mcp-corpus`),
/// recorded once by `run_stdio_server`. Writes into the visible artifacts tree
/// are confined to that server's own `{server}-mcp/` folder so every file in
/// `zk-data` names the server that produced it
/// (`standardized-artifact-storage.md` §0).
static ARTIFACT_OWNER: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Record the server whose artifacts this process writes. `server_name` is the
/// binary name (`hkask-mcp-corpus`) or the bare owner (`corpus`).
pub fn set_artifact_owner(server_name: &str) {
    let owner = server_name
        .strip_prefix("hkask-mcp-")
        .unwrap_or(server_name)
        .to_string();
    if let Err(existing) = ARTIFACT_OWNER.set(owner) {
        tracing::warn!(
            target: "hkask.mcp.paths",
            requested = %existing,
            current = ?ARTIFACT_OWNER.get(),
            "Artifact owner already recorded; keeping the first owner"
        );
    }
}

/// The artifacts-tree root this process may write under: the owner's
/// `{server}-mcp/` folder when an owner is recorded, the whole tree otherwise.
fn artifacts_write_root() -> std::path::PathBuf {
    match ARTIFACT_OWNER.get() {
        Some(owner) => hkask_types::agent_paths::resolve_under_artifacts_dir(
            &hkask_types::agent_paths::mcp_artifacts_subdir(owner, ""),
        ),
        None => hkask_types::agent_paths::resolve_artifacts_dir(),
    }
}

/// Contain `path` under an allowed root. The allowed roots are:
/// 1. The process current working directory (the project root when the MCP
///    server is launched per-project via `ContextServerStore`, or zed's
///    launch cwd for the app-global `McpRuntime` spawn).
/// 2. The kask data directory (`HKASK_DATA_DIR` or `~/.local/share/zed-kask`)
///    — where MCP server DBs and internal artifacts live (D28).
/// 3. The kask artifacts directory (`~/Documents/zk-data`) — where
///    user-facing artifacts live (D28). Reads may use the whole tree (another
///    server's output is valid input); writes are confined to the running
///    server's own `{server}-mcp/` folder (see `set_artifact_owner`).
///
/// Canonicalization collapses symlink escapes. Absolute paths like
/// `/etc/passwd` and traversals like `../../escape` are rejected unless they
/// resolve under one of the allowed roots.
fn contain(path: &std::path::Path, write: bool) -> Result<std::path::PathBuf, McpToolError> {
    let cwd = std::env::current_dir()
        .and_then(|cwd| cwd.canonicalize())
        .map_err(|e| McpToolError::internal(format!("Cannot resolve working directory: {e}")))?;

    // Anchor once: a new basename's parent is otherwise the empty path, not CWD.
    let absolute = cwd.join(path);
    let path = absolute.as_path();
    // Collect all allowed roots: CWD + data dir + artifacts dir.
    let mut allowed_roots = vec![cwd];
    if let Some(data_dir) = hkask_types::agent_paths::resolve_data_dir()
        .canonicalize()
        .ok()
    {
        allowed_roots.push(data_dir);
    }
    let artifacts_root = if write {
        let root = artifacts_write_root();
        // Self-healing: the owner's folder is recreated before it anchors a
        // write, so a user deleting it cannot turn valid writes into rejections.
        if let Err(error) = std::fs::create_dir_all(&root) {
            tracing::warn!(
                target: "hkask.mcp.paths",
                path = %root.display(),
                %error,
                "Cannot create the server's artifacts folder; writes under it will be rejected"
            );
        }
        root
    } else {
        hkask_types::agent_paths::resolve_artifacts_dir()
    };
    if let Some(artifacts_dir) = artifacts_root.canonicalize().ok() {
        allowed_roots.push(artifacts_dir);
    }

    let resolved = if write {
        canonicalize_lenient(path)
    } else {
        path.canonicalize()
    }
    .map_err(|e| {
        McpToolError::invalid_argument(format!("Cannot resolve path '{}': {e}", path.display()))
    })?;

    if !allowed_roots.iter().any(|root| resolved.starts_with(root)) {
        let roots_display: Vec<String> = allowed_roots
            .iter()
            .map(|r| r.display().to_string())
            .collect();
        return Err(rejection(
            path,
            &allowed_roots[0],
            &format!(
                "path escapes all allowed roots: {}",
                roots_display.join(", ")
            ),
        ));
    }
    Ok(resolved)
}

/// Resolve a caller-supplied write target, rejecting anything outside the
/// allowed roots (CWE-73). The target need not exist yet; relative paths,
/// including new basenames, are resolved against the server's CWD.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "New relative outputs work without weakening path containment" [P4]
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "isolated synchronous test subprocess; never runs on GPUI"
    )]
    fn relative_write_paths_preserve_containment() -> Result<(), Box<dyn std::error::Error>> {
        const CHILD: &str = "HKASK_RELATIVE_WRITE_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let cwd = std::env::current_dir()?.canonicalize()?;
            for name in ["output.txt", "new/nested/output.txt"] {
                let destination = contain_for_write(name).expect("new relative output");
                assert_eq!(destination, cwd.join(name));
                assert_eq!(
                    destination,
                    contain_for_write(&format!("./{name}")).expect("dot-prefixed output")
                );
                assert_eq!(
                    destination,
                    contain_for_write(cwd.join(name).to_str().expect("UTF-8 test path"))
                        .expect("absolute output")
                );
                std::fs::create_dir_all(destination.parent().expect("parent"))?;
                std::fs::write(&destination, "output")?;
                assert_eq!(
                    contain_for_write(name).expect("existing output"),
                    destination
                );
                assert_eq!(contain_for_read(name).expect("existing input"), destination);
            }
            assert!(contain_for_write("../outside/escape.txt").is_err());
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink("../outside", "escape")?;
                assert!(contain_for_write("escape/output.txt").is_err());
                assert!(contain_for_write("escape/nested/output.txt").is_err());
            }
            return Ok(());
        }
        // Only the child changes CWD/env; other tests retain their process state.
        let directory = tempfile::tempdir()?;
        for name in ["work", "outside", "data", "artifacts"] {
            std::fs::create_dir(directory.path().join(name))?;
        }
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "server::validation::tests::relative_write_paths_preserve_containment",
            ])
            .current_dir(directory.path().join("work"))
            .env(CHILD, "1")
            .env("HKASK_DATA_DIR", directory.path().join("data"))
            .env("HKASK_ARTIFACTS_DIR", directory.path().join("artifacts"))
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    /// A server may write only inside its own `{server}-mcp/` folder of the
    /// visible artifacts tree; the tree's top level and other servers' folders
    /// are rejected, while reads across the whole tree stay allowed
    /// (standardized-artifact-storage §0, operator ruling 2026-09-24).
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "isolated synchronous test subprocess; never runs on GPUI"
    )]
    fn artifact_writes_are_confined_to_the_owning_server() -> Result<(), Box<dyn std::error::Error>>
    {
        const CHILD: &str = "HKASK_ARTIFACT_OWNER_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let artifacts = hkask_types::agent_paths::resolve_artifacts_dir();
            let other = artifacts.join("media-mcp").join("generated");
            std::fs::create_dir_all(&other)?;
            std::fs::write(other.join("input.txt"), "input")?;
            set_artifact_owner("hkask-mcp-corpus");

            let own = artifacts.join("corpus-mcp").join("qa").join("out.jsonl");
            assert!(
                contain_for_write(own.to_str().ok_or("utf-8")?).is_ok(),
                "the owner's folder accepts writes, and is created on demand"
            );
            for path in [
                artifacts.join("loose-folder").join("out.jsonl"),
                artifacts.join("out.jsonl"),
                other.join("out.jsonl"),
            ] {
                assert!(
                    contain_for_write(path.to_str().ok_or("utf-8")?).is_err(),
                    "write outside corpus-mcp/ must be rejected: {}",
                    path.display()
                );
            }
            assert!(
                contain_for_read(other.join("input.txt").to_str().ok_or("utf-8")?).is_ok(),
                "reads may use another server's output"
            );
            return Ok(());
        }
        let directory = tempfile::tempdir()?;
        for name in ["work", "data", "artifacts"] {
            std::fs::create_dir(directory.path().join(name))?;
        }
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "server::validation::tests::artifact_writes_are_confined_to_the_owning_server",
            ])
            .current_dir(directory.path().join("work"))
            .env(CHILD, "1")
            .env("HKASK_DATA_DIR", directory.path().join("data"))
            .env("HKASK_ARTIFACTS_DIR", directory.path().join("artifacts"))
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    /// The containment must accept paths under the CWD (the project root
    /// when launched per-project). This is the pre-existing behavior the
    /// multi-root change must preserve.
    #[test]
    fn contain_accepts_cwd_relative_path() {
        // The test binary's CWD is the crate root — a relative path under it
        // must resolve and be accepted.
        let result = contain_for_read("Cargo.toml");
        assert!(result.is_ok(), "relative path under CWD must be accepted");
    }

    /// The containment must accept absolute paths under the CWD.
    #[test]
    fn contain_accepts_absolute_cwd_path() {
        let cwd = std::env::current_dir().expect("cwd must resolve");
        let target = cwd.join("Cargo.toml");
        let result = contain_for_read(target.to_str().expect("utf-8 path"));
        assert!(result.is_ok(), "absolute path under CWD must be accepted");
    }

    /// The containment must accept paths under the kask data dir (D28 —
    /// where MCP server DBs live). This is the new behavior.
    #[test]
    fn contain_accepts_data_dir_path() {
        let data_dir = hkask_types::agent_paths::resolve_data_dir();
        // The data dir itself may not exist in the test environment; create
        // a temp marker file to canonicalize against.
        std::fs::create_dir_all(&data_dir).expect("data dir must be creatable");
        let marker = data_dir.join(".containment-test-marker");
        std::fs::write(&marker, b"test").expect("marker must be writable");
        let result = contain_for_read(marker.to_str().expect("utf-8 path"));
        std::fs::remove_file(&marker).ok();
        assert!(result.is_ok(), "path under data dir must be accepted");
    }

    /// The containment must accept paths under the kask artifacts dir (D28 —
    /// where user-facing artifacts live). This is the new behavior.
    #[test]
    fn contain_accepts_artifacts_dir_path() {
        let artifacts_dir = hkask_types::agent_paths::resolve_artifacts_dir();
        std::fs::create_dir_all(&artifacts_dir).expect("artifacts dir must be creatable");
        let marker = artifacts_dir.join(".containment-test-marker");
        std::fs::write(&marker, b"test").expect("marker must be writable");
        let result = contain_for_read(marker.to_str().expect("utf-8 path"));
        std::fs::remove_file(&marker).ok();
        assert!(result.is_ok(), "path under artifacts dir must be accepted");
    }

    /// The containment must reject paths outside all allowed roots —
    /// e.g. `/etc/passwd` (CWE-22).
    #[test]
    fn contain_rejects_etc_passwd() {
        let result = contain_for_read("/etc/passwd");
        assert!(result.is_err(), "/etc/passwd must be rejected");
    }

    /// The containment must reject traversal escapes (CWE-73).
    #[test]
    fn contain_rejects_traversal_escape() {
        // `..` from the CWD lands outside the CWD; unless the parent happens
        // to be the data dir or artifacts dir (it is not — it's the workspace
        // root containing all crates), this must be rejected.
        let result = contain_for_read("../../../etc/passwd");
        assert!(result.is_err(), "traversal escape must be rejected");
    }

    /// The containment must reject nonexistent paths (read mode requires
    /// the target to exist for canonicalization).
    #[test]
    fn contain_rejects_nonexistent_read_path() {
        let result = contain_for_read("/definitely/does/not/exist.txt");
        assert!(result.is_err(), "nonexistent read path must be rejected");
    }
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
