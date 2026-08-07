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
    let output = expected;
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

#[test]
fn inconclusive_oracle_returns_inconclusive_when_reference_errors() {
    let oracle = hkask_test_harness::oracle_inconclusive(|input: &JsonValue| {
        input
            .get("x")
            .and_then(|v| v.as_i64())
            .map(|x| serde_json::json!(x * 2))
    });
    let valid_input = serde_json::json!({"x": 21});
    let valid_output = serde_json::json!(42);
    assert_eq!(
        oracle.verify(&valid_input, &valid_output),
        OracleVerdict::Pass
    );

    // When the reference cannot evaluate the input, the oracle cannot judge.
    let unhandled_input = serde_json::json!({"y": 1});
    let any_output = serde_json::json!(99);
    assert_eq!(
        oracle.verify(&unhandled_input, &any_output),
        OracleVerdict::Inconclusive
    );
}

// ── Trace filesystem tests ─────────────────────────────────────────────────
/// Per-test temp trace dir — avoids mutating the process-global `HKASK_TRACE_DIR`
/// env var, which is unsafe under parallel test execution.
struct TempTraceDir(std::path::PathBuf);
impl TempTraceDir {
    fn new(label: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("hkask-test-harness-{label}-{}", std::process::id()));
        // Start from a clean slate in case a prior run left artifacts.
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempTraceDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn write_trace_produces_json_file() {
    let temp = TempTraceDir::new("trace");

    let entry = TraceEntry {
        kind: "proptest".to_string(),
        name: "prop_round_trip".to_string(),
        result: "pass".to_string(),
        duration_ms: 42,
        shrunk_counterexample: String::new(),
        oracle_type: "invariant".to_string(),
        metadata: serde_json::json!({"crate": "hkask-templates", "target": "serialize"}),
    };

    let path = write_trace(temp.path(), "test-run-1", &entry).expect("write_trace should succeed");
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
}

#[test]
fn write_trace_sanitizes_name_with_path_separators() {
    let temp = TempTraceDir::new("sanitize");

    let entry = TraceEntry {
        kind: "bug-hunt".to_string(),
        name: "crate/module::function".to_string(),
        result: "fail".to_string(),
        duration_ms: 100,
        shrunk_counterexample: "input=42".to_string(),
        oracle_type: String::new(),
        metadata: serde_json::json!({}),
    };

    let path =
        write_trace(temp.path(), "sanitize-run", &entry).expect("write_trace should succeed");
    assert!(path.exists());
    assert!(
        !path.to_string_lossy().contains("crate/module"),
        "path separators should be sanitized"
    );
}

#[test]
fn write_trace_does_not_clobber_duplicate_kind_name() {
    let temp = TempTraceDir::new("collision");

    let entry = TraceEntry {
        kind: "proptest".to_string(),
        name: "prop_round_trip".to_string(),
        result: "pass".to_string(),
        duration_ms: 1,
        shrunk_counterexample: String::new(),
        oracle_type: String::new(),
        metadata: serde_json::json!({"run": "first"}),
    };
    let entry2 = TraceEntry {
        metadata: serde_json::json!({"run": "second"}),
        ..entry.clone()
    };

    let path1 = write_trace(temp.path(), "dup-run", &entry).expect("first write");
    let path2 = write_trace(temp.path(), "dup-run", &entry2).expect("second write");

    assert!(path1 != path2, "duplicate (kind, name) must not overwrite");
    assert!(path1.exists(), "first trace must survive the second write");
    assert!(path2.exists(), "second trace must be written");
    assert!(
        path2.to_string_lossy().contains("prop_round_trip-2.json"),
        "second trace should get a -2 suffix: {:?}",
        path2
    );

    // Both contents must be distinct (no silent data loss).
    let c1 = std::fs::read_to_string(&path1).unwrap();
    let c2 = std::fs::read_to_string(&path2).unwrap();
    assert_ne!(c1, c2, "the two traces must retain their own metadata");
}
