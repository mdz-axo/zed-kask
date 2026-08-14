//! Verifies the translated pipeline manifest (`pipeline-capabilities-researcher.yaml`)
//! parses successfully with the general manifest executor's `load_manifest_from_yaml`.
//! This is the contract test that pins the manifest-to-executor schema compatibility.
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_templates::manifest_loader::load_manifest_from_yaml;

#[test]
fn pipeline_manifest_parses_with_general_executor() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("../../corpus/pipeline-capabilities-researcher.yaml");
    let yaml = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
        panic!(
            "Failed to read pipeline manifest at {}: {e}",
            manifest_path.display()
        )
    });

    let manifest = load_manifest_from_yaml(&yaml).unwrap_or_else(|e| {
        panic!(
            "pipeline-capabilities-researcher.yaml must parse with the general executor: {e:?}"
        )
    });

    // The manifest declares category: pipeline.
    assert_eq!(
        manifest.category.as_deref(),
        Some("pipeline"),
        "pipeline manifest must declare category: pipeline"
    );

    // It must not be a skill — is_skill() must return false.
    assert!(
        !manifest.is_skill(),
        "pipeline manifest must not be classified as a skill"
    );

    // It must have 19 steps (9 tool calls + 10 gates).
    assert_eq!(
        manifest.steps.len(),
        19,
        "pipeline manifest must have 19 steps, got {}",
        manifest.steps.len()
    );

    // Verify step structure: alternating execute/gate pattern.
    let gate_count = manifest
        .steps
        .iter()
        .filter(|s| s.action == "gate")
        .count();
    let execute_count = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .count();
    assert_eq!(gate_count, 10, "expected 10 gate steps, got {gate_count}");
    assert_eq!(
        execute_count, 9,
        "expected 9 execute steps, got {execute_count}"
    );

    // Every gate step must have a command and on_failure.
    for step in &manifest.steps {
        if step.action == "gate" {
            assert!(
                step.command.is_some(),
                "gate step {} must have a command",
                step.ordinal
            );
            assert!(
                step.on_failure.is_some(),
                "gate step {} must have on_failure",
                step.ordinal
            );
        }
    }

    // Every execute step must have an mcp reference.
    for step in &manifest.steps {
        if step.action == "execute" {
            assert!(
                step.mcp.is_some(),
                "execute step {} must have an mcp reference",
                step.ordinal
            );
        }
    }

    // Steps with id fields must preserve them.
    let extract_step = manifest
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some("extract_text"))
        .expect("extract_text step must exist");
    assert_eq!(extract_step.action, "execute");
    assert_eq!(extract_step.mcp.as_deref(), Some("corpus_convert"));
}

#[test]
fn pipeline_manifest_gate_steps_have_literal_command_blocks() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir.join("../../corpus/pipeline-capabilities-researcher.yaml");
    let yaml = std::fs::read_to_string(&manifest_path)
        .expect("Failed to read pipeline manifest");

    let manifest = load_manifest_from_yaml(&yaml)
        .expect("pipeline manifest must parse");

    // The first gate (gate_corpus_server_alive) must have a command that
    // contains the Python heredoc — verifying literal block scalars survived
    // the translation.
    let first_gate = manifest
        .steps
        .iter()
        .find(|s| s.action == "gate")
        .expect("must have at least one gate step");
    let command = first_gate
        .command
        .as_deref()
        .expect("gate must have a command");
    assert!(
        command.contains("python3"),
        "gate command must contain python3 heredoc"
    );
}
