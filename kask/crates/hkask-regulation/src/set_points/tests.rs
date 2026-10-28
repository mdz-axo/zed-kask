use super::*;

/// expect: "Invalid outcome sensitivity is rejected instead of silently disabling alerts" [P9]
#[test]
fn validate_outcome_threshold_bounds_and_order() {
    for (warning, critical) in [
        (1.1, 0.25),
        (0.5, -0.1),
        (0.5, 0.5),
        (0.25, 0.5),
        (f64::NAN, 0.25),
        (0.5, f64::NAN),
        (f64::INFINITY, 0.25),
        (0.5, f64::NEG_INFINITY),
    ] {
        let points = SetPoints {
            outcome_warning_threshold: warning,
            outcome_critical_threshold: critical,
            ..SetPoints::default()
        };
        assert!(points.validate().is_err(), "accepted {warning}/{critical}");
    }
    let points = SetPoints {
        outcome_warning_threshold: 1.0,
        outcome_critical_threshold: 0.0,
        ..SetPoints::default()
    };
    assert!(points.validate().is_ok());
}

/// Pins the ceiling for native_first_rate_threshold: 1.0 silently disables
/// the sensor (no observed rate can exceed it), so it must be rejected —
/// the mirror image of the tool_reliability 0.0 floor. 0.0 is strict but
/// meaningful (any native-first substantive turn deviates).
#[test]
fn validate_bounds_native_first_rate_threshold() {
    let mut points = SetPoints::default();
    assert!(points.validate().is_ok(), "defaults must validate");

    points.native_first_rate_threshold = 1.0;
    assert!(
        points.validate().is_err(),
        "1.0 silently disables the native-first sensor — must be rejected"
    );

    points.native_first_rate_threshold = 0.0;
    assert!(
        points.validate().is_ok(),
        "0.0 is strict but meaningful — any native-first turn deviates"
    );

    points.native_first_rate_threshold = -0.1;
    assert!(points.validate().is_err(), "negative rates are invalid");

    points.native_first_rate_threshold = 0.5;
    assert!(points.validate().is_ok());
}

/// Pins the non-zero floor for tool_reliability_threshold: 0.0
/// configures a silently-vacuous sensor (no outcome can ever breach a
/// 0.0 floor), and > 1.0 configures an always-firing one. Both must be
/// rejected so `load_set_points` falls back to the 0.80 default.
#[test]
fn validate_bounds_tool_reliability_threshold() {
    let mut points = SetPoints::default();
    assert!(points.validate().is_ok(), "defaults must validate");

    points.tool_reliability_threshold = 0.0;
    assert!(
        points.validate().is_err(),
        "0.0 silently disables the reliability check — must be rejected"
    );

    points.tool_reliability_threshold = 1.0;
    assert!(
        points.validate().is_ok(),
        "1.0 (perfect-reliability floor) is a valid, if strict, setting"
    );

    points.tool_reliability_threshold = 1.5;
    assert!(
        points.validate().is_err(),
        "> 1.0 is unsatisfiable — every aggregate would deviate every tick"
    );
}

/// `algedonic_events_max` round-trips from YAML config and defaults to 0
/// — the knob exists (option (B), algedonic card f69162f4, operator ruling
/// 2026-10-19), but the policy is unchanged until the operator sets it.
#[test]
fn algedonic_events_max_round_trips_from_config() {
    let config = SetPointsConfig::from_yaml("algedonic_events_max: 3\n").expect("valid yaml");
    let points = SetPoints::from_config(&config);
    assert_eq!(points.algedonic_events_max, 3);
    assert_eq!(
        SetPoints::default().algedonic_events_max,
        0,
        "the default set point is unchanged — option (B) alters no behavior"
    );
}

/// Pins the bounds check for `algedonic_events_max`: a value at or above
/// `max_alerts` configures a vacuous sensor — the actionable count can
/// never exceed the log cap, so the sensor could never fire and the
/// operator cannot distinguish "no advisories" from "check disabled"
/// (the same class the `tool_reliability_threshold` bounds check rejects).
#[test]
fn validate_bounds_algedonic_events_max() {
    let mut points = SetPoints::default();
    assert!(points.validate().is_ok(), "defaults must validate");

    points.algedonic_events_max = points.max_alerts - 1;
    assert!(
        points.validate().is_ok(),
        "a tolerance below the log cap is satisfiable and valid"
    );

    points.algedonic_events_max = points.max_alerts;
    assert!(
        points.validate().is_err(),
        "a tolerance at the log cap can never be exceeded — the sensor is silently disabled"
    );
}
