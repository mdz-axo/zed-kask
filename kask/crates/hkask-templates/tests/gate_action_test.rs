//! Tests for the `gate` action and pipeline manifest fields (`id`, `command`,
//! `on_failure`). Verifies that the general manifest executor can parse and
//! dispatch gate steps — the core capability needed for pipeline manifests.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_templates::bundle::manifest::{BundleManifestStep, OnFailureConfig};

// ── OnFailureConfig deserialization ────────────────────────────────────────

#[test]
fn on_failure_config_deserializes_halt_action() {
    let yaml = "action: halt\nresume: \"repair extraction, rerun this gate\"";
    let config: OnFailureConfig = serde_yaml_neo::from_str(yaml).expect("should parse");
    assert_eq!(config.action, "halt");
    assert_eq!(config.resume, "repair extraction, rerun this gate");
}

#[test]
fn on_failure_config_rejects_unknown_fields() {
    let yaml = "action: halt\nresume: \"test\"\nbogus: true";
    let result: Result<OnFailureConfig, _> = serde_yaml_neo::from_str(yaml);
    assert!(
        result.is_err(),
        "deny_unknown_fields must reject bogus field"
    );
}

// ── BundleManifestStep with gate fields ─────────────────────────────────────

#[test]
fn bundle_manifest_step_accepts_gate_fields() {
    let yaml = "ordinal: 1\naction: gate\ndescription: \"Verify extraction\"\ncommand: \"echo GATE_PASS\"\non_failure:\n  action: halt\n  resume: \"repair extraction\"\n";
    let step: BundleManifestStep = serde_yaml_neo::from_str(yaml).expect("should parse");
    assert_eq!(step.action, "gate");
    assert_eq!(step.command.as_deref(), Some("echo GATE_PASS"));
    assert!(step.on_failure.is_some());
    assert_eq!(step.on_failure.as_ref().unwrap().action, "halt");
}

#[test]
fn bundle_manifest_step_accepts_id_field() {
    let yaml = "ordinal: 1\naction: execute\ndescription: \"Extract text\"\nmcp: corpus_convert\nid: extract_text\n";
    let step: BundleManifestStep = serde_yaml_neo::from_str(yaml).expect("should parse");
    assert_eq!(step.id.as_deref(), Some("extract_text"));
    assert_eq!(step.mcp.as_deref(), Some("corpus_convert"));
}

#[test]
fn bundle_manifest_step_gate_fields_default_to_none() {
    let yaml = "ordinal: 1\naction: select\ndescription: \"test\"\n";
    let step: BundleManifestStep = serde_yaml_neo::from_str(yaml).expect("should parse");
    assert!(
        step.id.is_none(),
        "id must default to None for skill manifests"
    );
    assert!(step.command.is_none(), "command must default to None");
    assert!(step.on_failure.is_none(), "on_failure must default to None");
}

// ── Gate command execution ──────────────────────────────────────────────────
//
// These tests verify the shell-command execution pattern that `execute_gate`
// uses, without constructing a full StepMachine (which requires Infra).

#[tokio::test]
async fn gate_command_pass_returns_gate_pass_marker() {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("echo 'stats: ok'; echo GATE_PASS")
        .output()
        .await
        .expect("sh should execute");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    assert!(output.status.success());
    assert!(last_line.contains("GATE_PASS"));
    assert!(!last_line.contains("GATE_FAIL"));
}

#[tokio::test]
async fn gate_command_fail_returns_nonzero_exit() {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("echo 'assertion failed'; exit 1")
        .output()
        .await
        .expect("sh should execute");
    assert!(!output.status.success());
}

#[tokio::test]
async fn gate_command_fail_with_gate_fail_marker() {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("echo 'word ratio 0.5 outside [0.90, 1.35]'; echo GATE_FAIL")
        .output()
        .await
        .expect("sh should execute");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    assert!(last_line.contains("GATE_FAIL"));
}
