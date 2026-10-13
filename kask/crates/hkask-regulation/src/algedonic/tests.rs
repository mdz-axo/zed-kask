use super::*;
use crate::runtime::VarietyTracker;
use std::collections::HashMap;

fn manager() -> AlgedonicManager {
    AlgedonicManager::with_max_alerts(20, DEFAULT_EXPECTED_VARIETY, 200)
}

/// An active domain meeting its expected variety produces no alert and
/// no log entry — a healthy check is silent. Pushing deficit-0 Info
/// alerts on every check populated the log during normal tool use,
/// which the AlgedonicEvents sensor read as a standing review demand.
#[test]
fn check_returns_none_at_zero_deficit() {
    let mut mgr = manager();
    let mut tracker = VarietyTracker::new();
    tracker.increment("tool_a");
    tracker.increment("tool_b");
    tracker.increment("tool_c");
    // expected 3 (DEFAULT_EXPECTED_VARIETY), observed 3 → deficit 0
    assert!(mgr.check(&tracker, "domain").is_none());
    assert_eq!(
        mgr.alert_count(),
        0,
        "a healthy check must not push an alert"
    );
}

/// Idle domains produce no alert and no log growth — a domain at rest
/// is not variety-deficient. Pins the idle gate in `check`.
#[test]
fn check_returns_none_for_idle_domain() {
    let mut mgr = manager();
    let idle = VarietyTracker::new();
    assert!(mgr.check(&idle, "media").is_none());
    assert_eq!(mgr.alert_count(), 0, "idle check must not push an alert");
}

/// `VarietyTracker::deficit` reads zero for an idle window even against
/// a non-zero expected variety.
#[test]
fn deficit_is_zero_for_idle_window() {
    let tracker = VarietyTracker::new();
    assert_eq!(tracker.deficit(DEFAULT_EXPECTED_VARIETY), 0);
}

/// An active domain with fewer distinct states than expected reports
/// the gap — the genuine Ashby signal the sensor exists to carry.
#[test]
fn deficit_reports_gap_for_active_domain() {
    let mut tracker = VarietyTracker::new();
    tracker.increment("state_a");
    tracker.increment("state_b");
    assert_eq!(tracker.deficit(10), 8);
}

/// `current_total_deficit` is a level: it reports the live per-domain
/// gaps and does not grow as alerts accumulate in the log. This pins
/// the fix for the monotonic log-sum that made every threshold trip
/// forever. Expected variety is explicit (10) — this test pins the
/// level semantics, not the default calibration.
#[test]
fn current_total_deficit_is_a_level_not_a_log_sum() {
    let mut mgr = AlgedonicManager::with_max_alerts(20, 10, 200);
    let mut active = VarietyTracker::new();
    active.increment("state_a");
    active.increment("state_b");
    let counters = HashMap::from([
        ("active".to_string(), active.clone()),
        ("idle".to_string(), VarietyTracker::new()),
    ]);

    // Re-check the active domain many times — the checks coalesce into
    // one surviving alert (occurrence count grows) — but the current
    // deficit must not move.
    for _ in 0..10 {
        mgr.check(&active, "active");
    }
    assert!(mgr.alert_count() > 0, "sanity: the log grew");
    assert_eq!(
        mgr.current_total_deficit(&counters),
        8,
        "deficit is the live gap (10 expected − 2 observed), not the log sum"
    );
}

/// The default calibration: 3 distinct tools per domain per window. A
/// healthy active minute uses 2–5 distinct tools per MCP server; the
/// previous default of 10 made every active domain read as deficient.
/// One distinct tool → gap 2.
#[test]
fn default_expected_variety_is_calibrated_for_tool_ruts() {
    let mgr = manager();
    let mut tracker = VarietyTracker::new();
    tracker.increment("only_tool");
    let counters = HashMap::from([("domain".to_string(), tracker)]);
    assert_eq!(mgr.current_total_deficit(&counters), 2);
}

/// `reg_health_check` reports the caller-computed level, not the
/// manager's alert history. Expected variety is explicit (10) — this
/// test pins the pass-through semantics, not the default calibration.
#[test]
fn reg_health_check_reports_passed_deficit() {
    let mut mgr = AlgedonicManager::with_max_alerts(20, 10, 200);
    let mut tracker = VarietyTracker::new();
    tracker.increment("state_a");
    let counters = HashMap::from([("domain".to_string(), tracker.clone())]);
    mgr.check(&tracker, "domain");

    let health = reg_health_check(&mgr, 0.0, mgr.current_total_deficit(&counters));
    assert_eq!(health.overall_deficit, 9);
}

/// Recurrences of the same condition (domain + severity) coalesce into
/// one surviving alert: the occurrence count grows, the latest
/// observation's deficit/threshold/message/timestamp refresh in place,
/// and the log does not fill with per-cycle duplicates. (operator
/// ruling 2026-09-28: consolidate the alert paths — the chronic
/// conditions re-alerted every cycle and kept the 200-cap log
/// permanently full, which `log_approaching_cap` then read as a
/// permanent level deviation.)
#[test]
fn push_alert_coalesces_recurrences_by_condition_identity() {
    let mut mgr = manager();
    mgr.push_alert(RuntimeAlert::new("chronic", 3, 4).expect("warning alert"));
    for _ in 0..9 {
        mgr.push_alert(RuntimeAlert::new("chronic", 3, 4).expect("warning recurrence"));
    }
    assert_eq!(
        mgr.alert_count(),
        1,
        "recurrences must coalesce into the surviving alert, not duplicate"
    );
    let surviving = mgr.alerts().first().expect("surviving alert");
    assert_eq!(surviving.occurrence_count, 10);
    assert_eq!(surviving.domain, "chronic");
}

/// A severity change is a transition, not a recurrence: the Critical
/// alert is a new entry alongside the surviving Warning entry, so the
/// transition history stays visible to review.
#[test]
fn push_alert_treats_severity_change_as_transition() {
    let mut mgr = manager();
    mgr.push_alert(RuntimeAlert::new("chronic", 3, 4).expect("warning"));
    mgr.push_alert(RuntimeAlert::new("chronic", 5, 4).expect("critical"));
    assert_eq!(
        mgr.alert_count(),
        2,
        "a severity change must push a new alert, not coalesce over the old severity"
    );
}

/// At the cap the oldest entry is evicted; a condition whose alert was
/// evicted re-alerts as a fresh occurrence — recurrence after recovery
/// (or eviction) emits again.
#[test]
fn push_alert_evicts_oldest_at_cap_and_recurrence_reemits() {
    let mut mgr = AlgedonicManager::with_max_alerts(200, DEFAULT_EXPECTED_VARIETY, 3);
    for domain in ["a", "b", "c"] {
        mgr.push_alert(RuntimeAlert::new(domain, 3, 4).expect("alert"));
    }
    assert_eq!(mgr.alert_count(), 3);
    mgr.push_alert(RuntimeAlert::new("d", 3, 4).expect("alert"));
    assert_eq!(mgr.alert_count(), 3, "the cap evicts the oldest entry");
    assert!(
        mgr.alerts().iter().all(|alert| alert.domain != "a"),
        "the oldest entry is evicted"
    );
    mgr.push_alert(RuntimeAlert::new("a", 3, 4).expect("re-emitted alert"));
    let re_emitted = mgr
        .alerts()
        .iter()
        .find(|alert| alert.domain == "a")
        .expect("recurrence after eviction re-emits");
    assert_eq!(
        re_emitted.occurrence_count, 1,
        "recurrence after eviction starts a fresh count"
    );
}
