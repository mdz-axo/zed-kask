//! zed-kask: D89 — durable hang-incident record.
//!
//! Upstream serializes `SerializedHangIncident`s into `telemetry.log`, but
//! that file is an upload spool, not an archive: `Client::start` recreates
//! it with `File::create` at every app launch
//! (crates/client/src/telemetry.rs), truncating the previous session's
//! events — a bad session's hang evidence is destroyed by the restart that
//! follows it. This module appends every non-empty incident batch, stamped
//! with a wall-clock `recorded_at`, to `hang_traces/hang-incidents.jsonl`:
//! append-only, surviving restarts. The `.jsonl` extension sits outside the
//! task-trace cleanup's `json`/`miniprof` filter, so the 3-file cap never
//! sweeps it.

use std::io::Write;
use std::path::Path;

use gpui::profiler::hang::SerializedHangIncident;

/// Append one batch record to `hang-incidents.jsonl` under `dir`.
///
/// Split from [`record`] so the append-only contract is testable against a
/// temporary directory instead of the operator's real hang-traces dir.
fn append_batch(
    dir: &Path,
    incidents: &[SerializedHangIncident],
    threshold_incidents: u64,
    budget_incidents: u64,
) -> std::io::Result<()> {
    let line = serde_json::json!({
        "recorded_at": chrono::Local::now().to_rfc3339(),
        "total_incidents": threshold_incidents + budget_incidents,
        "threshold_incidents": threshold_incidents,
        "budget_incidents": budget_incidents,
        "incidents": incidents,
    });
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("hang-incidents.jsonl"))?;
    file.write_all(line.to_string().as_bytes())?;
    file.write_all(b"\n")
}

/// Record a non-empty incident batch to the durable hang-incident log.
/// Failures are logged, never fatal: the in-session telemetry channel
/// carries the same payload, and a broken sink must not take the detection
/// loop down with it.
pub(crate) fn record(
    incidents: &[SerializedHangIncident],
    threshold_incidents: u64,
    budget_incidents: u64,
) {
    if let Err(error) = append_batch(
        paths::hang_traces_dir(),
        incidents,
        threshold_incidents,
        budget_incidents,
    ) {
        log::warn!("failed to append the durable hang-incident record: {error}");
    }
}

#[test]
fn batches_append_without_truncating() {
    let dir =
        std::env::temp_dir().join(format!("zed-hang-incident-log-test-{}", std::process::id()));
    // A previously failed run may have left the directory behind.
    let _ = std::fs::remove_dir_all(&dir);
    let incident = SerializedHangIncident {
        measurement_version: 2,
        phase: "steady",
        trigger: gpui::profiler::hang::HangTrigger::Budget,
        start_ms: 0.0,
        active_ms: 100.0,
        stall_ms: 94.0,
        dirty_to_present_ms: Some(94.0),
        sealed_by: "present",
        busy_fraction: 0.75,
        event_count: 2,
        small_poll_count: 0,
        small_poll_total_ms: 0.0,
        dropped_events: 0,
        journal_discontinuous: false,
        contributors: vec![],
        contributors_elided: 0,
    };
    append_batch(&dir, std::slice::from_ref(&incident), 0, 1).expect("first append");
    append_batch(&dir, &[incident], 0, 1).expect("second append");

    let file =
        std::fs::read_to_string(dir.join("hang-incidents.jsonl")).expect("incident log readable");
    let lines: Vec<&str> = file.lines().collect();
    assert_eq!(lines.len(), 2, "appends must accumulate, never truncate");
    for line in &lines {
        let record: serde_json::Value = serde_json::from_str(line).expect("each line is JSON");
        assert_eq!(record["total_incidents"], 1);
        assert_eq!(record["incidents"].as_array().map(Vec::len), Some(1));
        assert!(
            record["recorded_at"].as_str().is_some(),
            "wall-clock recorded_at is the timestamp the truncated telemetry log lacks"
        );
    }
    std::fs::remove_dir_all(&dir).expect("test cleanup");
}
