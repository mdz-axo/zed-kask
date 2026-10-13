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
