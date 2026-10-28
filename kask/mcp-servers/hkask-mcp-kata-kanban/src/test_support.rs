//! Shared fixtures for the kata-kanban integration tests — the lib-crate
//! test seam (single-sourced from the former per-file copies, test-suite
//! cleanup B2; the jscpd cross-file cluster between `board_rename.rs` and
//! `idempotent_creates.rs`). Lives in the lib — not `tests/common/mod.rs` —
//! because the fork surface forbids `mod.rs` (CI: `check-no-mod-rs.sh`);
//! `#[doc(hidden)]` keeps it out of the public docs while the integration
//! test binaries (separate crates) reach it through the lib.

use hkask_types::{InferenceError, WorktreeSpawnPort};
use std::future::Future;
use std::pin::Pin;

/// Worktree-spawn refusal fixture: no alternate executor may be started.
/// Both integration-test binaries pin the same refusal through this one copy.
pub struct UnavailableWorktreeSpawn;

impl WorktreeSpawnPort for UnavailableWorktreeSpawn {
    fn create_worktree_thread<'a>(
        &'a self,
        _prompt: &'a str,
        _title: &'a str,
        _worktree_name: Option<&'a str>,
        _base_ref: Option<&'a str>,
        _allowed_tools: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<String, InferenceError>> + Send + 'a>> {
        Box::pin(async {
            Err(InferenceError::Connection(
                "worktree spawn unavailable (test stub)".to_string(),
            ))
        })
    }
}

/// Parse a tool's JSON output, failing loudly on malformed responses.
pub fn parse(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output must be valid JSON")
}
