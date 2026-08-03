//! Path containment for LLM-controlled file arguments (CWE-22/73/200/400,
//! OWASP LLM06). This server is launched per-project via ContextServerStore
//! with no governance membrane, so containment is enforced here: every
//! caller-supplied path must resolve (after canonicalization, which also
//! collapses symlink escapes) under the process current working directory.
//!
//! Launch-path note: the per-project ContextServerStore spawn sets cwd to the
//! project root (crates/project/src/context_server_store.rs passes root_path),
//! so containment is anchored to the project there. The app-global McpRuntime
//! spawn sets no cwd — the child inherits zed's cwd, and containment anchors
//! to that. In CLI usage (zed started from the project dir) both coincide;
//! a desktop-launched zed anchors the governed path to the launch cwd. That
//! is fail-safe (still confined to a directory the operator chose to launch
//! from) but is not the project root — corpus tools invoked through the
//! governed path should be given explicit paths within the launch cwd. In
//! both cases, absolute paths like `/etc/passwd` and traversals like
//! `../../escape` are rejected.

use hkask_mcp_server::server::McpToolError;
use std::path::{Path, PathBuf};

use crate::helpers::map_corpus_io_error;

/// Default read size cap for `extract_text` (32 MiB).
pub(crate) const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;

fn rejection(path: &Path, root: &Path, reason: &str) -> McpToolError {
    tracing::warn!(
        target: "hkask.mcp.corpus.path_safety",
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

/// Canonicalize `path` for a target that may not exist yet (writes): resolve
/// the nearest existing ancestor, then re-append the remaining components.
fn canonicalize_lenient(path: &Path) -> std::io::Result<PathBuf> {
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

fn contain(path: &Path, write: bool) -> Result<PathBuf, McpToolError> {
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
/// project root. The target need not exist yet.
pub(crate) fn contain_for_write(path: &str) -> Result<PathBuf, McpToolError> {
    contain(Path::new(path), true)
}

/// Resolve a caller-supplied read path, rejecting anything outside the
/// project root. The target must exist.
pub(crate) fn contain_for_read(path: &str) -> Result<PathBuf, McpToolError> {
    contain(Path::new(path), false)
}

/// Read a caller-supplied file with containment and a size cap, so a hostile
/// or mistaken path cannot exfiltrate arbitrary files or exhaust memory.
pub(crate) fn read_capped(path: &str, max_bytes: u64) -> Result<Vec<u8>, McpToolError> {
    let resolved = contain_for_read(path)?;
    let metadata = std::fs::metadata(&resolved).map_err(|e| {
        map_corpus_io_error(e, &format!("Cannot stat file '{}'", resolved.display()))
    })?;
    if metadata.len() > max_bytes {
        tracing::warn!(
            target: "hkask.mcp.corpus.path_safety",
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
        .map_err(|e| map_corpus_io_error(e, &format!("Failed to read file '{}'", path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_to_tmp_escape_rejected() {
        // /tmp is outside any plausible cargo-test cwd (the crate directory).
        let result = contain_for_write("/tmp/hkask-escape-test.jsonl");
        assert!(result.is_err(), "write to /tmp must be rejected");
    }

    #[test]
    fn write_traversal_escape_rejected() {
        let result = contain_for_write("../../hkask-escape-test");
        assert!(result.is_err(), "write via ../.. must be rejected");
    }

    #[test]
    fn write_inside_cwd_accepted() {
        let cwd = std::env::current_dir().expect("cwd");
        let target = cwd.join("hkask-path-safety-test-output.jsonl");
        let resolved =
            contain_for_write(target.to_str().expect("utf8 path")).expect("inside cwd accepted");
        assert!(resolved.starts_with(cwd.canonicalize().expect("canonical cwd")));
    }

    #[test]
    fn read_etc_passwd_rejected() {
        let result = contain_for_read("/etc/passwd");
        assert!(result.is_err(), "read of /etc/passwd must be rejected");
    }

    #[test]
    fn read_oversized_rejected() {
        let cwd = std::env::current_dir().expect("cwd");
        let file_path = cwd.join("hkask-path-safety-oversized-test.bin");
        std::fs::write(&file_path, vec![0u8; 64]).expect("write fixture");
        let result = read_capped(file_path.to_str().expect("utf8 path"), 32);
        let _cleanup = std::fs::remove_file(&file_path);
        assert!(result.is_err(), "file larger than cap must be rejected");
    }

    #[test]
    fn read_inside_cwd_within_cap_accepted() {
        let cwd = std::env::current_dir().expect("cwd");
        let file_path = cwd.join("hkask-path-safety-read-test.txt");
        std::fs::write(&file_path, b"hello").expect("write fixture");
        let bytes = read_capped(file_path.to_str().expect("utf8 path"), 1024).expect("read ok");
        let _cleanup = std::fs::remove_file(&file_path);
        assert_eq!(bytes, b"hello");
    }
}
