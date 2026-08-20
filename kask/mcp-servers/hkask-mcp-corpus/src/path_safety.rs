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
