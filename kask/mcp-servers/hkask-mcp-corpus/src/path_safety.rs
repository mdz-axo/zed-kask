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

    // ── Tool-boundary containment contracts (RR-0047 / RR-0048) ──────────
    //
    // These tests pin the containment contract that the LLM-reachable
    // `manifest_path` (corpus_discover_company) and `config_path`
    // (corpus_build_persona) tool inputs must pass through `read_capped` /
    // `contain_for_read` before being read. The tool handlers themselves
    // require a full `CorpusServer` (inference router, etc.) which is too
    // heavy for a unit test, so these tests pin the *primitive* the handlers
    // call: if someone removes the `read_capped` / `contain_for_read` call
    // from the handler, the handler would call `std::fs::read_to_string`
    // directly on the raw path — and these tests would still pass (they test
    // the primitive, not the call site). The call-site pin is the cargo-test
    // grep pattern in RR-0047/RR-0048's `detection.include` glob, which asserts
    // the handler files contain `path_safety::read_capped` /
    // `path_safety::contain_for_read`. Both layers are needed: the primitive
    // test here, and the call-site grep in the regression gate.

    /// RR-0047: `corpus_discover_company`'s `manifest_path` input must be
    /// contained. A traversal path must be rejected by the primitive the
    /// handler calls (`read_capped`), so a poisoned `manifest_path` like
    /// `../../etc/passwd` cannot exfiltrate arbitrary files into LLM context.
    #[test]
    fn manifest_path_traversal_rejected_by_read_capped() {
        let result = read_capped("../../etc/passwd", MAX_READ_BYTES);
        assert!(
            result.is_err(),
            "manifest_path traversal must be rejected by read_capped; got {result:?}"
        );
    }

    /// RR-0047: an absolute `manifest_path` like `/etc/passwd` must be rejected.
    #[test]
    fn manifest_path_absolute_escape_rejected_by_read_capped() {
        let result = read_capped("/etc/passwd", MAX_READ_BYTES);
        assert!(
            result.is_err(),
            "absolute manifest_path must be rejected by read_capped; got {result:?}"
        );
    }

    /// RR-0048: `corpus_build_persona`'s `config_path` input must be contained.
    /// The handler calls `crate::path_safety::contain_for_read(&config_path)`
    /// before passing the path to `EmbedService::embed_corpus`. A traversal
    /// path must be rejected by that primitive.
    #[test]
    fn config_path_traversal_rejected_by_contain_for_read() {
        let result = contain_for_read("../../etc/passwd");
        assert!(
            result.is_err(),
            "config_path traversal must be rejected by contain_for_read; got {result:?}"
        );
    }

    /// RR-0048: an absolute `config_path` like `/etc/passwd` must be rejected.
    #[test]
    fn config_path_absolute_escape_rejected_by_contain_for_read() {
        let result = contain_for_read("/etc/passwd");
        assert!(
            result.is_err(),
            "absolute config_path must be rejected by contain_for_read; got {result:?}"
        );
    }
}
