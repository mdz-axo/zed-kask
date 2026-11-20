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
//!
//! zed-kask: hang-audit Pr5 (2026-11-12) — session markers. Every record
//! carries a `kind` discriminant (`session_start` / `incidents` /
//! `session_end`); pre-marker lines have no `kind` and read as incident
//! batches. `start_hang_detection` writes `session_start` (with the build
//! identity) at monitor start and `session_end` at app quit, so stall
//! batches attribute to builds through their enclosing markers, and a
//! `session_start` with no `session_end` before the next start is the
//! durable quit-flush failure signal (an unclean end whose final batch
//! may be lost).

use std::io::Write;
use std::path::Path;

use gpui::profiler::hang::SerializedHangIncident;

/// Append one JSON line to `hang-incidents.jsonl` under `dir` — the shared
/// body beneath every record kind. The `.jsonl` extension keeps the file
/// outside the task-trace cleanup's `json`/`miniprof` filter.
fn append_line(dir: &Path, line: serde_json::Value) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("hang-incidents.jsonl"))?;
    file.write_all(line.to_string().as_bytes())?;
    file.write_all(b"\n")
}

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
        "kind": "incidents",
        "recorded_at": chrono::Local::now().to_rfc3339(),
        "total_incidents": threshold_incidents + budget_incidents,
        "threshold_incidents": threshold_incidents,
        "budget_incidents": budget_incidents,
        "incidents": incidents,
    });
    append_line(dir, line)
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

/// Append the session's `session_start` marker, carrying the build identity
/// so stall batches attribute to builds through their enclosing markers.
/// Split from [`record_session_start`] for temp-dir testing.
fn append_session_start(
    dir: &Path,
    version: &str,
    commit_sha: &str,
    build_id: &str,
) -> std::io::Result<()> {
    let line = serde_json::json!({
        "kind": "session_start",
        "recorded_at": chrono::Local::now().to_rfc3339(),
        "build": {
            "version": version,
            "commit_sha": commit_sha,
            "build_id": build_id,
        },
    });
    append_line(dir, line)
}

/// Append the session's `session_end` marker. Split from
/// [`record_session_end`] for temp-dir testing.
fn append_session_end(dir: &Path) -> std::io::Result<()> {
    let line = serde_json::json!({
        "kind": "session_end",
        "recorded_at": chrono::Local::now().to_rfc3339(),
    });
    append_line(dir, line)
}

/// zed-kask: hang-audit Pr5 — mark the session's start in the durable
/// journal, carrying the build identity (the same compile-time sources the
/// startup log line prints). Best-effort like [`record`]: a failure warns
/// and never blocks startup.
pub(crate) fn record_session_start(version: &str, commit_sha: &str, build_id: &str) {
    if let Err(error) =
        append_session_start(paths::hang_traces_dir(), version, commit_sha, build_id)
    {
        log::warn!("failed to append the session-start marker: {error}");
    }
}

/// zed-kask: hang-audit Pr5 — mark the session's clean end. The durable
/// quit-flush failure signal is this marker's ABSENCE: a session_start
/// with no session_end before the next session_start is an unclean end
/// (crash or kill), and its final Flush-poll batch may be lost. Best-effort,
/// never blocks quit.
pub(crate) fn record_session_end() {
    if let Err(error) = append_session_end(paths::hang_traces_dir()) {
        log::warn!("failed to append the session-end marker: {error}");
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
        trigger: gpui::profiler::hang::HangTrigger::LateFrame,
        during_active_use: true,
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

/// zed-kask: hang-audit Pr5 — session markers delimit the durable journal.
/// The `session_start` marker carries the build identity (so stall batches
/// attribute to builds through their enclosing markers), the batch records
/// carry their `incidents` discriminant, and the `session_end` marker closes
/// a clean quit — its absence before the next session_start is the durable
/// quit-flush failure signal.
#[test]
fn session_markers_delimit_and_batches_carry_kind() {
    let dir = std::env::temp_dir().join(format!(
        "zed-hang-session-marker-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);

    append_session_start(&dir, "0.40.2-test", "abc123", "test-build-id").expect("session start");
    append_batch(&dir, &[], 0, 0).expect("batch");
    append_session_end(&dir).expect("session end");

    let file =
        std::fs::read_to_string(dir.join("hang-incidents.jsonl")).expect("incident log readable");
    let lines: Vec<&str> = file.lines().collect();
    assert_eq!(lines.len(), 3, "markers and batch append in order");
    let start: serde_json::Value = serde_json::from_str(lines[0]).expect("start line is JSON");
    assert_eq!(start["kind"], "session_start");
    assert_eq!(start["build"]["version"], "0.40.2-test");
    assert_eq!(start["build"]["commit_sha"], "abc123");
    assert_eq!(start["build"]["build_id"], "test-build-id");
    let batch: serde_json::Value = serde_json::from_str(lines[1]).expect("batch line is JSON");
    assert_eq!(
        batch["kind"], "incidents",
        "batches carry their discriminant"
    );
    let end: serde_json::Value = serde_json::from_str(lines[2]).expect("end line is JSON");
    assert_eq!(end["kind"], "session_end");

    std::fs::remove_dir_all(&dir).expect("test cleanup");
}
