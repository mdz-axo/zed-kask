#![forbid(unsafe_code)]
//! hKask MCP Condenser — Context condensation for tool outputs
//!
//! Loop: Episodic (Loop 2) — Confirmed. Context condensation operates on the active
//! conversation window, which is episodic in nature. The condenser compresses and persists
//! tool outputs within the episodic memory boundary.
//!
//! Provides compression algorithms (rtk_style, word_rank, flashrank) for reducing
//! tool output size while preserving essential information. `word_rank` uses
//! TF-IDF bag-of-words compression with ontology anchoring.
//! CPU-only algorithms with no LLM dependency. Phase 2 adds LLM-assisted
//! thread summarization via the centralized hKask inference router.
//!
//! When `HKASK_DB_PATH` + `HKASK_DB_PASSPHRASE` environment variables are set,
//! the condenser can persist compressed outputs to episodic memory via the
//! `condenser:persist` tool. Without them, the server operates in memory-only
//! mode (the default — no persistence backend required).
//!
//! The `condenser_thread_summary` tool uses the centralized `InferencePort`
//! (hkask-inference router) for LLM-powered summarization. No standalone
//! HTTP client or inference URL configuration is needed — the router handles
//! provider dispatch (DeepInfra, Together AI) automatically.
//!
//! STUB (T5.7 deletion candidate): The original implementation depended on the
//! `hkask-condenser`, `hkask-inference`, and `hkask-storage` crates, which are
//! deletion candidates under the zed-kask merge. Until those domains are
//! re-homed (see `kask/docs/specs/seam-specs.md` T0.6), this server is
//! non-functional and `run()` returns an error. The tool implementations,
//! `CondenserServer` struct, and persistence wiring have been removed.

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

/// Run the condenser MCP server (used by binary target).
///
/// Currently a stub: returns an error because the upstream domain crates
/// (`hkask-condenser`, `hkask-inference`, `hkask-storage`) have not yet been
/// ported under the zed-kask merge.
pub async fn run(
    _userpod: String,
    _daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    Err(hkask_mcp_server::McpError::UnexpectedResponse {
        context: "hkask-mcp-condenser".into(),
        detail:
            "not yet ported — depends on deleted hkask-condenser/hkask-inference/hkask-storage \
             (see kask/docs/specs/seam-specs.md T0.6)"
                .into(),
    })
}
