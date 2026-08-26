//! Context-server health source bridge.
//!
//! Implements `hkask_regulation::ContextServerHealthSource` over a snapshot
//! of the per-project `ContextServerStore` state. The cybernetics loop reads
//! this snapshot each tick to sense MCP servers stuck in `Starting` or
//! `Error` — the signature of the foreground-executor starvation bug where
//! `initialize` is never sent (or never completes) within the 600s timeout.
//!
//! ## Why a snapshot, not a live entity read
//!
//! `ContextServerStore` is a per-project GPUI entity and `AsyncApp` is not
//! `Send` (`.rules` trap). The cybernetics loop runs on a tokio background
//! thread and cannot hold a GPUI entity lock. Instead, the composition root
//! (`main.rs`) subscribes to `ServerStatusChangedEvent` on the foreground
//! thread and calls `update` with the latest counts. The sensor reads the
//! snapshot asynchronously — no GPUI access from the background.
//!
//! ## The blind-feedback-loop gap this closes
//!
//! Without this source, the cybernetics loop reports `signal_count=0` while
//! every MCP context server is hung on `initialize`. The loop's existing
//! sensors read ledger/DB state, not context-server process state. This is
//! the same trap as `InferenceHealthSource` but for the MCP stdio children.

use std::sync::{Arc, Mutex};

/// A `ContextServerHealthSource` over a shared snapshot of context-server
/// fleet health.
///
/// The composition root creates one `Arc<BridgeContextServerHealthSource>`,
/// passes a clone to the cybernetics loop, and calls `update` from a
/// foreground `ServerStatusChangedEvent` observer. The sensor reads the
/// shared counts asynchronously on its tick.
pub struct BridgeContextServerHealthSource {
    healthy: Arc<Mutex<usize>>,
    total: Arc<Mutex<usize>>,
}

impl BridgeContextServerHealthSource {
    pub fn new() -> Self {
        Self {
            healthy: Arc::new(Mutex::new(0)),
            total: Arc::new(Mutex::new(0)),
        }
    }

    /// Replace the snapshot with fresh counts computed from the
    /// `ContextServerStore`'s current server states. Called from the
    /// foreground thread (a `ServerStatusChangedEvent` observer or a
    /// `maintain_servers` hook).
    pub fn update(&self, healthy: usize, total: usize) {
        *self.healthy.lock().expect("health snapshot mutex poisoned") = healthy;
        *self.total.lock().expect("health snapshot mutex poisoned") = total;
    }
}

impl Default for BridgeContextServerHealthSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl hkask_regulation::ContextServerHealthSource for BridgeContextServerHealthSource {
    async fn healthy_count(&self) -> usize {
        *self.healthy.lock().expect("health snapshot mutex poisoned")
    }

    async fn total_count(&self) -> usize {
        *self.total.lock().expect("health snapshot mutex poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_regulation::ContextServerHealthSource;

    #[tokio::test]
    async fn snapshot_updates_are_visible_to_source() {
        let source = BridgeContextServerHealthSource::new();
        // Initially empty — no servers registered.
        assert_eq!(source.total_count().await, 0);
        assert_eq!(source.healthy_count().await, 0);

        // Simulate 10 servers, all stuck in Starting (the storm).
        source.update(0, 10);
        assert_eq!(source.total_count().await, 10);
        assert_eq!(source.healthy_count().await, 0);

        // All healthy.
        source.update(10, 10);
        assert_eq!(source.healthy_count().await, 10);
    }
}
