//! CLI Experience Recorder — shared bridge from CLI commands to the daemon's
//! dual-encoding memory pipeline (episodic + semantic).
//!
//! Every CLI command that produces meaningful output (embed-corpus, compose,
//! settings changes, etc.) records an experience through this bridge. The
//! daemon handles dual encoding, narrative generation, and consolidation.
//!
//! Graceful degradation: if the daemon socket is unavailable, recording is
//! silently skipped with a warning log. CLI commands never fail because of
//! memory unavailability.

use hkask_mcp_server::DaemonClient;
use hkask_types::time::now_rfc3339;
use serde_json::json;

/// Shared recorder for CLI command experiences.
///
/// Usage:
/// ```ignore
/// let recorder = CliExperienceRecorder::new();
/// recorder.record(
///     "Jacques rZuck",
///     "embed_corpus",
///     "hemingway",
///     "success",
///     json!({"passages": 1827, "h_mems": 28592}),
/// ).await;
/// ```
pub struct CliExperienceRecorder {
    daemon: Option<DaemonClient>,
}

impl CliExperienceRecorder {
    /// Create a recorder that connects to the default daemon socket.
    /// Returns a recorder even if the daemon is unreachable — recording
    /// will silently skip in that case.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  none (always succeeds)
    /// post: returns CliExperienceRecorder; daemon is Some if socket exists, None otherwise
    pub fn new() -> Self {
        let daemon = DaemonClient::new();
        // Test connectivity — if the socket doesn't exist, mark daemon as None
        let socket_path = hkask_mcp_server::daemon::daemon_socket_path();
        if socket_path.exists() {
            Self {
                daemon: Some(daemon),
            }
        } else {
            tracing::warn!(
                path = %socket_path.display(),
                "Daemon socket not found — CLI experiences will not be recorded"
            );
            Self { daemon: None }
        }
    }

    /// Record a CLI command experience in episodic and semantic memory.
    ///
    /// Parameters follow the MCP server `record_experience` pattern:
    /// - `userpod`: the authenticated userpod name
    /// - `tool`: the CLI command name (e.g., "embed_corpus", "compose")
    /// - `input_summary`: short description of what was done
    /// - `outcome`: "success" or "failure"
    /// - `detail`: structured JSON with command-specific statistics
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  userpod, tool, input_summary, outcome must be non-empty; detail must be valid JSON
    /// post: experience is sent to daemon for dual encoding; silently skipped if daemon unavailable
    pub async fn record(
        &self,
        userpod: &str,
        tool: &str,
        input_summary: &str,
        outcome: &str,
        detail: serde_json::Value,
    ) {
        let Some(ref daemon) = self.daemon else {
            tracing::debug!(
                tool,
                userpod,
                "Daemon unavailable — skipping experience recording"
            );
            return;
        };

        let value = json!({
            "tool": tool,
            "input": input_summary,
            "outcome": outcome,
            "detail": detail,
            "timestamp": now_rfc3339(),
        });

        let entity = format!("cli:{}", tool);

        match daemon
            .store_experience(userpod, &entity, "executed", &value, Some(0.9))
            .await
        {
            Ok(hkask_mcp_server::DaemonResponse::StoreResponse { stored: true, .. }) => {
                tracing::info!(tool, userpod, "CLI experience recorded via daemon");
            }
            Ok(other) => {
                tracing::warn!(
                    tool,
                    userpod,
                    response = ?other,
                    "Unexpected daemon response for CLI experience"
                );
            }
            Err(e) => {
                tracing::warn!(
                    tool,
                    userpod,
                    error = %e,
                    "Failed to record CLI experience via daemon"
                );
            }
        }
    }
}

impl Default for CliExperienceRecorder {
    fn default() -> Self {
        Self::new()
    }
}
