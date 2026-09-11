//! Path containment for LLM-controlled file arguments (CWE-22/73/200/400,
//! OWASP LLM06). This server is launched per-project via ContextServerStore
//! with no governance membrane, so containment is enforced here: every
//! caller-supplied path must resolve (after canonicalization, which also
//! collapses symlink escapes) under the process current working directory.
//!
//! The implementation lives in `hkask_mcp_server::server` (shared with the
//! other MCP servers) and is re-exported here so in-crate call sites keep
//! the `crate::path_safety::` path. The test surface is preserved so the
//! `path_safety` cargo-test pattern pinned by RR-0032 keeps matching.
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

pub(crate) use hkask_mcp_server::server::{
    MAX_READ_BYTES, contain_for_read, contain_for_write, read_capped,
};

/// Resolve a QA destination before opening it, including a symlink whose final
/// target does not exist yet. The shared lenient resolver alone leaves such a
/// symlink unresolved, which would give two writers different ownership keys.
pub(crate) fn distinct_output_path(
    input: &str,
    output: &str,
) -> Result<std::path::PathBuf, hkask_mcp_server::server::McpToolError> {
    use hkask_mcp_server::server::McpToolError;
    use std::collections::HashSet;
    use std::io::ErrorKind;

    let input = contain_for_read(input)?;
    let mut output = contain_for_write(output)?;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(output.clone()) {
            return Err(McpToolError::invalid_argument("Output symlink cycle"));
        }
        match std::fs::symlink_metadata(&output) {
            Ok(metadata) if metadata.is_symlink() => {
                let target = std::fs::read_link(&output).map_err(|error| {
                    crate::helpers::map_corpus_io_error(error, "Cannot resolve output symlink")
                })?;
                let parent = output.parent().ok_or_else(|| {
                    McpToolError::invalid_argument("Output has no parent directory")
                })?;
                output = contain_for_write(&parent.join(target).to_string_lossy())?;
            }
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::NotFound => break,
            Err(error) => {
                return Err(crate::helpers::map_corpus_io_error(
                    error,
                    "Cannot inspect output path",
                ));
            }
        }
    }
    let mut aliases_input = input == output;
    // Canonical names cover symlinks; inode identity also protects hard-linked
    // inputs from truncation on the Unix hosts that support these aliases.
    #[cfg(unix)]
    if !aliases_input {
        use std::os::unix::fs::MetadataExt;
        let input_metadata = std::fs::metadata(&input).map_err(|error| {
            crate::helpers::map_corpus_io_error(error, "Cannot inspect input path")
        })?;
        match std::fs::metadata(&output) {
            Ok(metadata) => {
                aliases_input = metadata.dev() == input_metadata.dev()
                    && metadata.ino() == input_metadata.ino();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(crate::helpers::map_corpus_io_error(
                    error,
                    "Cannot inspect output path",
                ));
            }
        }
    }
    if aliases_input {
        return Err(McpToolError::invalid_argument(format!(
            "QA output '{}' aliases prompts_jsonl; refusing to truncate input",
            output.display()
        )));
    }
    Ok(output)
}
