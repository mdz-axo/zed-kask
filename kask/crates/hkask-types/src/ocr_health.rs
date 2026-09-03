//! OCR health snapshot — the cross-process file contract between the corpus
//! MCP server (writer) and the cybernetics loop's OCR health source (reader).
//!
//! The corpus server is a subprocess: its `reg.pipeline.ocr.silent_failure`
//! tracing events never reach the zed main process's in-memory algedonic log,
//! so the cybernetics loop reported `signal_count=0` during an OCR silent
//! failure storm (a dead-but-responsive OCR endpoint returning HTTP 200
//! with empty content on every Complex page). This file is the IPC channel:
//! the corpus server's `OcrHealthRecorder` appends events atomically
//! (tmp+rename), and the bridge's `BridgeOcrHealthSource` reads the snapshot
//! each regulation tick. The schema lives here — in the shared types crate —
//! so the writer and the reader cannot drift apart.

use serde::{Deserialize, Serialize};

/// Bounded history of silent-failure timestamps kept in the file.
///
/// The cap bounds the file size; the reader only counts entries inside its
/// recent window, so entries older than the window are dead weight and the
/// oldest are dropped first.
pub const OCR_SILENT_FAILURE_HISTORY_CAP: usize = 128;

/// A point-in-time record of OCR pipeline degradation events.
///
/// Written by the corpus server (`OcrHealthRecorder`), read by the zed-side
/// health source each regulation tick. All timestamps are Unix seconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OcrHealthSnapshot {
    /// Unix seconds of each OCR silent failure (empty LLM output on a page),
    /// oldest first, capped at [`OCR_SILENT_FAILURE_HISTORY_CAP`].
    pub silent_failure_timestamps: Vec<i64>,
    /// Whether the LLM OCR circuit breaker is currently open (the endpoint
    /// is quarantined and Complex pages are degrading to Tesseract).
    pub circuit_breaker_open: bool,
    /// Unix seconds of the last file write.
    pub updated_unix: i64,
}

impl OcrHealthSnapshot {
    /// Record a silent failure at `unix`, dropping the oldest entry when the
    /// history cap is reached.
    pub fn record_silent_failure(&mut self, unix: i64) {
        self.silent_failure_timestamps.push(unix);
        let overflow = self
            .silent_failure_timestamps
            .len()
            .saturating_sub(OCR_SILENT_FAILURE_HISTORY_CAP);
        if overflow > 0 {
            self.silent_failure_timestamps.drain(..overflow);
        }
    }

    /// Count silent failures within `window_secs` of `now_unix`.
    ///
    /// The window policy belongs to the reader (the regulation side), not the
    /// writer — this is a pure filter over the recorded history.
    pub fn silent_failures_within(&self, window_secs: i64, now_unix: i64) -> u64 {
        self.silent_failure_timestamps
            .iter()
            .filter(|&&ts| now_unix - ts < window_secs)
            .count() as u64
    }
}

/// Canonical health-file path: `{kask_data_dir}/mcp/corpus/ocr-health.json`.
///
/// Both processes resolve the path through this single helper (backed by
/// `agent_paths::resolve_under_data_dir` + the D28 `mcp/{server_id}` layout)
/// so the writer and the reader cannot land on different files.
#[must_use]
pub fn ocr_health_path() -> std::path::PathBuf {
    crate::agent_paths::resolve_under_data_dir(&crate::agent_paths::mcp_server_subdir(
        "corpus",
        "ocr-health.json",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_count_filters_by_age() {
        let mut snapshot = OcrHealthSnapshot::default();
        snapshot.record_silent_failure(100);
        snapshot.record_silent_failure(200);
        snapshot.record_silent_failure(900);
        // Window of 300s at now=1000: entries at 900 (age 100) and 200
        // (age 800) — only the 900 entry is inside.
        assert_eq!(snapshot.silent_failures_within(300, 1000), 1);
        // Boundary: an entry exactly `window_secs` old is outside.
        assert_eq!(snapshot.silent_failures_within(800, 1000), 1);
        assert_eq!(snapshot.silent_failures_within(801, 1000), 2);
        assert_eq!(snapshot.silent_failures_within(799, 1000), 1);
        // Empty history counts zero.
        assert_eq!(
            OcrHealthSnapshot::default().silent_failures_within(300, 1000),
            0
        );
    }

    #[test]
    fn history_cap_drops_oldest_first() {
        let mut snapshot = OcrHealthSnapshot::default();
        for i in 0..(OCR_SILENT_FAILURE_HISTORY_CAP + 10) {
            snapshot.record_silent_failure(i as i64);
        }
        assert_eq!(
            snapshot.silent_failure_timestamps.len(),
            OCR_SILENT_FAILURE_HISTORY_CAP
        );
        // The oldest 10 entries were dropped; the first remaining is 10.
        assert_eq!(snapshot.silent_failure_timestamps[0], 10);
        assert_eq!(
            snapshot.silent_failure_timestamps.last().copied(),
            Some((OCR_SILENT_FAILURE_HISTORY_CAP + 9) as i64)
        );
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        // The file contract: writer and reader share this schema, so the
        // serde round-trip is the drift pin.
        let mut snapshot = OcrHealthSnapshot::default();
        snapshot.record_silent_failure(42);
        snapshot.circuit_breaker_open = true;
        snapshot.updated_unix = 43;
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let parsed: OcrHealthSnapshot = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.silent_failure_timestamps, vec![42]);
        assert!(parsed.circuit_breaker_open);
        assert_eq!(parsed.updated_unix, 43);
    }
}
