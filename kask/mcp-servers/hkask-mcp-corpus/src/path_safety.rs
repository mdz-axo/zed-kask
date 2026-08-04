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
        let _cleanup = std::fs::remove_file(&target);
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
