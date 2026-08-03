//! Tests for the harness evolution items: Oracle taxonomy + trace filesystem.
//!
//! Validates Slice 1 acceptance criteria from the evolving test harness design:
//! - Oracle constructors (hardcoded, reference, invariant) produce correct verdicts
//! - write_trace persists a JSON file to the trace filesystem

use hkask_test_harness::{Oracle, OracleVerdict, TraceEntry, write_trace};
use serde_json::Value as JsonValue;

// ── Oracle taxonomy tests ──────────────────────────────────────────────────

#[test]
fn hardcoded_oracle_passes_on_match() {
    let expected = serde_json::json!({"status": "ok"});
    let oracle = hkask_test_harness::oracle_hardcoded(expected.clone());
    let input = serde_json::json!({"req": 1});
    let output = expected.clone();
    assert_eq!(oracle.verify(&input, &output), OracleVerdict::Pass);
}

#[test]
fn hardcoded_oracle_fails_on_mismatch() {
    let oracle = hkask_test_harness::oracle_hardcoded(serde_json::json!({"status": "ok"}));
    let input = serde_json::json!({"req": 1});
    let output = serde_json::json!({"status": "error"});
    match oracle.verify(&input, &output) {
        OracleVerdict::Fail(_) => {}
        other => panic!("expected Fail, got {other:?}"),
    }
}

#[test]
fn reference_oracle_compares_against_reference_impl() {
    let oracle = hkask_test_harness::oracle_reference(|input: &JsonValue| {
        serde_json::json!(input.get("x").and_then(|v| v.as_i64()).unwrap_or(0) * 2)
    });
    let input = serde_json::json!({"x": 21});
    let correct_output = serde_json::json!(42);
    let wrong_output = serde_json::json!(41);
    assert_eq!(oracle.verify(&input, &correct_output), OracleVerdict::Pass);
    assert!(matches!(
        oracle.verify(&input, &wrong_output),
        OracleVerdict::Fail(_)
    ));
}

#[test]
fn invariant_oracle_checks_predicate() {
    let oracle = hkask_test_harness::oracle_invariant(|input: &JsonValue, output: &JsonValue| {
        let input_len = input
            .get("data")
            .and_then(|d| d.as_array().map(|a| a.len()))
            .unwrap_or(0);
        let output_len = output
            .get("result")
            .and_then(|v| v.as_array().map(|a| a.len()))
            .unwrap_or(0);
        if output_len > input_len {
            Err(format!(
                "output {} longer than input {}",
                output_len, input_len
            ))
        } else {
            Ok(())
        }
    });
    let input = serde_json::json!({"data": [1, 2, 3]});
    let ok_output = serde_json::json!({"result": [1, 2]});
    let bad_output = serde_json::json!({"result": [1, 2, 3, 4]});
    assert_eq!(oracle.verify(&input, &ok_output), OracleVerdict::Pass);
    assert!(matches!(
        oracle.verify(&input, &bad_output),
        OracleVerdict::Fail(_)
    ));
}

#[test]
fn oracle_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Box<dyn Oracle>>();
}

// ── Trace filesystem tests ─────────────────────────────────────────────────

#[test]
fn write_trace_produces_json_file() {
    let temp_dir = std::env::temp_dir().join("hkask-test-harness-trace-test");
    // SAFETY: set_var is single-threaded in test context; no other thread reads this env var.
    unsafe {
        std::env::set_var("HKASK_TRACE_DIR", &temp_dir);
    }

    let entry = TraceEntry {
        kind: "proptest".to_string(),
        name: "prop_round_trip".to_string(),
        result: "pass".to_string(),
        duration_ms: 42,
        shrunk_counterexample: String::new(),
        oracle_type: "invariant".to_string(),
        metadata: serde_json::json!({"crate": "hkask-templates", "target": "serialize"}),
    };

    let path = write_trace("test-run-1", &entry).expect("write_trace should succeed");
    assert!(path.exists(), "trace file should exist at {:?}", path);
    assert!(
        path.to_string_lossy()
            .contains("proptest-prop_round_trip.json"),
        "filename should contain kind and name"
    );

    let content = std::fs::read_to_string(&path).expect("should read trace file");
    let json: JsonValue = serde_json::from_str(&content).expect("should parse as JSON");
    assert_eq!(json["kind"], "proptest");
    assert_eq!(json["name"], "prop_round_trip");
    assert_eq!(json["result"], "pass");
    assert_eq!(json["duration_ms"], 42);
    assert_eq!(json["oracle_type"], "invariant");

    std::fs::remove_dir_all(&temp_dir).ok();
    // SAFETY: single-threaded test cleanup.
    unsafe {
        std::env::remove_var("HKASK_TRACE_DIR");
    }
}

#[test]
fn write_trace_sanitizes_name_with_path_separators() {
    let temp_dir = std::env::temp_dir().join("hkask-test-harness-sanitize-test");
    // SAFETY: set_var is single-threaded in test context.
    unsafe {
        std::env::set_var("HKASK_TRACE_DIR", &temp_dir);
    }

    let entry = TraceEntry {
        kind: "bug-hunt".to_string(),
        name: "crate/module::function".to_string(),
        result: "fail".to_string(),
        duration_ms: 100,
        shrunk_counterexample: "input=42".to_string(),
        oracle_type: String::new(),
        metadata: serde_json::json!({}),
    };

    let path = write_trace("sanitize-run", &entry).expect("write_trace should succeed");
    assert!(path.exists());
    assert!(
        !path.to_string_lossy().contains("crate/module"),
        "path separators should be sanitized"
    );

    std::fs::remove_dir_all(&temp_dir).ok();
    // SAFETY: single-threaded test cleanup.
    unsafe {
        std::env::remove_var("HKASK_TRACE_DIR");
    }
}
