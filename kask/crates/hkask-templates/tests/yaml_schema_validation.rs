//! YAML schema validation tests — Wave 6 Task 6.1
//!
//! Validates that all registry manifest YAML files are well-formed
//! and contain required fields. Catches malformed manifests at test time
//! rather than at runtime.
//!
//! # Principle grounding
//! - P8 (Semantic Grounding): config errors should be caught before runtime
//! - P11 (Digital Public/Private Sphere): visibility must be canonical

use serde::Deserialize;
use std::path::Path;

/// Minimal manifest structure for validation.
#[derive(Debug, Deserialize)]
struct ManifestFile {
    manifest: ManifestHeader,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ManifestHeader {
    id: String,
    name: String,
    description: String,
    version: String,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    functional_role: Option<String>,
}

// [P3] Motivating: Generative Space — validates registry manifests are well-formed
//Constraining: Semantic Grounding — required fields present and correctly typed
// All registry manifests are well-formed YAML with required fields.

#[test]
fn all_skill_manifests_are_well_formed() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_dir = workspace_root.join("registry/manifests");
    if !manifest_dir.exists() {
        eprintln!("{} not found — skipping test", manifest_dir.display());
        return;
    }

    let mut errors = Vec::new();
    let mut count = 0;

    for entry in walkdir::WalkDir::new(manifest_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
    {
        count += 1;
        let path = entry.path();
        match std::fs::read_to_string(path) {
            Ok(content) => {
                // Skip pipeline configs (no `manifest:` key at top level).
                if !content.contains("\nmanifest:") && !content.starts_with("manifest:") {
                    eprintln!("Skipping non-manifest YAML: {}", path.display());
                    continue;
                }
                match serde_yaml_neo::from_str::<ManifestFile>(&content) {
                    Ok(mf) => {
                        assert!(
                            !mf.manifest.id.is_empty(),
                            "{}: manifest.id is empty",
                            path.display()
                        );
                        assert!(
                            !mf.manifest.name.is_empty(),
                            "{}: manifest.name is empty",
                            path.display()
                        );
                        // P3: description must be present (Generative Space requires discoverability)
                        assert!(
                            !mf.manifest.description.is_empty(),
                            "{}: manifest.description is empty",
                            path.display()
                        );
                        // P7: version must be present (Evolutionary Architecture requires versioning)
                        assert!(
                            !mf.manifest.version.is_empty(),
                            "{}: manifest.version is empty",
                            path.display()
                        );
                        // P11: visibility must be present and canonical (Public or Private only)
                        let vis = mf.manifest.visibility.as_deref().unwrap_or("");
                        assert!(
                            !vis.is_empty(),
                            "{}: manifest.visibility is missing",
                            path.display()
                        );
                        assert!(
                            vis == "Public" || vis == "Private",
                            "{}: manifest.visibility is '{vis}' — must be Public or Private (P11)",
                            path.display()
                        );
                        // functional_role should be present if the manifest uses it
                        // (Note: some manifests like kata and improv use alternative structural schemas)
                    }
                    Err(e) => {
                        errors.push(format!("{}: YAML parse error: {}", path.display(), e));
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: IO error: {}", path.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        panic!(
            "{} of {} manifests failed validation:\n{}",
            errors.len(),
            count,
            errors.join("\n")
        );
    }

    eprintln!("Validated {} manifests — all well-formed", count);
}

// [P3] Motivating: Generative Space — validates registry manifests are well-formed
//Constraining: Semantic Grounding — required fields present and correctly typed
#[test]
fn invalid_yaml_is_rejected() {
    let invalid = "id: 123\nname: []\n"; // name should be string, not array
    let result = serde_yaml_neo::from_str::<ManifestFile>(invalid);
    assert!(result.is_err(), "invalid YAML should be rejected");
}

/// Verify the superforecasting manifest loads via the full loader and that
/// its `compute` step (the connected-layers bridge to hkask_forecast) parses
/// correctly with `action: "compute"` and a valid `compute_ref`.
///
/// Co-evolution Phase 1: three native MCP `execute` steps were added to close
/// the calibration loop:
///   - Step 4: market_match (outside-view anchor from prediction markets)
///   - Step 16: scenario_score (persist forecast for later Brier scoring)
///   - Step 18: scenario_calibration (fetch prior Brier/overconfidence curve)
/// The manifest grew from 18 to 21 steps; all downstream ordinals shifted.
#[test]
fn superforecasting_manifest_loads_with_compute_step() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/superforecasting.yaml");
    if !manifest_path.exists() {
        eprintln!("superforecasting.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load superforecasting manifest: {e}"));

    // 11 select steps + 6 compute steps + 3 execute steps + 1 loop step = 21 total.
    // The three execute steps (market_match, scenario_score, scenario_calibration)
    // were added in Co-evolution Phase 1 to close the calibration loop.
    assert_eq!(
        manifest.steps.len(),
        21,
        "expected 21 steps after Co-evolution Phase 1 (3 execute steps added)"
    );

    // Six compute steps: Fermi (3), outside-view (6), tree-combine (10),
    // Bayesian (12), lisp.eval signal (19), calibration (20). The former
    // convergence-check compute was removed — convergence is gated by the
    // ConvergenceTracker.
    let compute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "compute")
        .collect();
    assert_eq!(compute_steps.len(), 6, "manifest must have 6 compute steps");
    assert_eq!(compute_steps[0].ordinal, 3, "Fermi compute at ordinal 3");
    assert_eq!(
        compute_steps[0].compute_ref.as_deref(),
        Some("calibrate_from_fermi")
    );
    assert_eq!(
        compute_steps[1].ordinal, 6,
        "outside-view compute at ordinal 6"
    );
    assert_eq!(
        compute_steps[1].compute_ref.as_deref(),
        Some("outside_view_adjustment")
    );
    assert_eq!(
        compute_steps[2].ordinal, 10,
        "tree-combine compute at ordinal 10"
    );
    assert_eq!(
        compute_steps[2].compute_ref.as_deref(),
        Some("combine_tree_probabilities")
    );
    assert_eq!(
        compute_steps[3].ordinal, 12,
        "Bayesian compute at ordinal 12"
    );
    assert_eq!(
        compute_steps[3].compute_ref.as_deref(),
        Some("bayesian_update")
    );
    assert_eq!(
        compute_steps[4].ordinal, 19,
        "lisp.eval signal compute at ordinal 19"
    );
    assert_eq!(compute_steps[4].compute_ref.as_deref(), Some("lisp.eval"));
    assert_eq!(
        compute_steps[5].ordinal, 20,
        "calibration feedback compute at ordinal 20"
    );
    assert_eq!(
        compute_steps[5].compute_ref.as_deref(),
        Some("apply_calibration_adjustment")
    );

    // Three execute steps (Co-evolution Phase 1): market_match (4),
    // scenario_score (16), scenario_calibration (18). Each must have an
    // mcp field and an on_failure config (no silent collapse to defaults).
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    assert_eq!(
        execute_steps.len(),
        3,
        "manifest must have 3 execute steps (Co-evolution Phase 1)"
    );
    assert_eq!(
        execute_steps[0].ordinal, 4,
        "market_match execute at ordinal 4"
    );
    assert_eq!(
        execute_steps[0].mcp.as_deref(),
        Some("market_match"),
        "step 4 must call market_match"
    );
    assert_eq!(
        execute_steps[1].ordinal, 16,
        "scenario_score execute at ordinal 16"
    );
    assert_eq!(
        execute_steps[1].mcp.as_deref(),
        Some("scenario_score"),
        "step 16 must call scenario_score"
    );
    assert_eq!(
        execute_steps[2].ordinal, 18,
        "scenario_calibration execute at ordinal 18"
    );
    assert_eq!(
        execute_steps[2].mcp.as_deref(),
        Some("scenario_calibration"),
        "step 18 must call scenario_calibration"
    );
    // Every execute step must have on_failure (no silent collapse to defaults).
    for step in &execute_steps {
        assert!(
            step.on_failure.is_some(),
            "execute step {} must have on_failure config (no silent collapse)",
            step.ordinal
        );
    }

    // The loop step (ordinal 21) must carry the calibration-adjusted prior.
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 21, "loop step should be ordinal 21");
}

/// Verify the stage_1 and stage_3 templates declare the tree-structure
/// contract fields in their front-matter `output` blocks. This pins the
/// advertised invariant (`.rules`: advertised invariants need enforcement
/// points) so a future template edit cannot silently drop the tree fields
/// and revert to the heuristic combine.
#[test]
fn superforecasting_tree_contract_fields_present() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let templates_dir = workspace_root.join("registry/templates/superforecasting");

    // stage_1 must declare sub_question_tree, topological_order, outcome_node_id
    // in its output contract.
    let stage_1 = std::fs::read_to_string(templates_dir.join("stage_1_fermi_decompose.j2"))
        .expect("stage_1_fermi_decompose.j2 must exist");
    assert!(
        stage_1.contains("sub_question_tree:"),
        "stage_1 contract must declare sub_question_tree output"
    );
    assert!(
        stage_1.contains("topological_order:"),
        "stage_1 contract must declare topological_order output"
    );
    assert!(
        stage_1.contains("outcome_node_id:"),
        "stage_1 contract must declare outcome_node_id output"
    );

    // stage_3 must declare tree_nodes in its output contract, and must NOT
    // declare combined_probability (the compute step owns that now).
    let stage_3 = std::fs::read_to_string(templates_dir.join("stage_3_probability_estimate.j2"))
        .expect("stage_3_probability_estimate.j2 must exist");
    assert!(
        stage_3.contains("tree_nodes:"),
        "stage_3 contract must declare tree_nodes output"
    );
    // The contract output block must not list combined_probability as an
    // LLM-estimated field. (The word may still appear in explanatory prose,
    // but not as a contract output field declaration.)
    let stage_3_output_block = stage_3
        .split("contract:")
        .nth(1)
        .and_then(|s| s.split("---").next())
        .unwrap_or("");
    assert!(
        !stage_3_output_block.contains("combined_probability:"),
        "stage_3 contract output must not declare combined_probability (the compute step owns it)"
    );
}

/// Verify the kali-audit FlowDef manifest loads correctly with the expected
/// PDCA structure after the convergence-check removal: 4 select
/// steps + 1 loop step, with the Cauchy convergence block. The former
/// kata.convergence_check compute step was removed — the ConvergenceTracker
/// is the single convergence gate.
#[test]
fn kali_audit_manifest_loads_with_correct_structure() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/kali-audit.yaml");
    if !manifest_path.exists() {
        eprintln!("kali-audit.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load kali-audit manifest: {e}"));

    // 4 select steps + 1 lisp.eval compute step (signal computation) +
    // 1 loop step = 6 total. The former kata.convergence_check compute step
    // was removed — the ConvergenceTracker is the single convergence gate.
    // The lisp.eval step remains: it computes the convergence signal (count
    // of open critical/high findings) that the loop step pushes via
    // `convergence_signal:`.
    assert_eq!(
        manifest.steps.len(),
        6,
        "expected 6 steps: select-surface → audit → report → taxonomy-map → lisp.eval (signal) → loop"
    );

    // Verify step ordinals are sequential starting at 1.
    for (i, step) in manifest.steps.iter().enumerate() {
        assert_eq!(
            step.ordinal,
            (i + 1) as u32,
            "step ordinals must be sequential starting at 1"
        );
    }

    // Verify step 1 is select-surface.
    assert_eq!(manifest.steps[0].action, "select");
    assert_eq!(
        manifest.steps[0].template_ref.as_deref(),
        Some("kali-audit/select-surface")
    );

    // Verify step 2 is audit.
    assert_eq!(manifest.steps[1].action, "select");
    assert_eq!(
        manifest.steps[1].template_ref.as_deref(),
        Some("kali-audit/audit")
    );

    // Verify step 3 is report.
    assert_eq!(manifest.steps[2].action, "select");
    assert_eq!(
        manifest.steps[2].template_ref.as_deref(),
        Some("kali-audit/report")
    );

    // Verify step 4 is taxonomy-map (folded from attack-taxonomy-mapper).
    assert_eq!(manifest.steps[3].action, "select");
    assert_eq!(
        manifest.steps[3].template_ref.as_deref(),
        Some("kali-audit/taxonomy-map")
    );

    // Verify step 5 is the lisp.eval signal-compute step (computes the
    // convergence signal — count of open critical/high findings — that the
    // loop step pushes via convergence_signal:). The former
    // kata.convergence_check compute step was removed.
    assert_eq!(manifest.steps[4].action, "compute");
    assert_eq!(manifest.steps[4].compute_ref.as_deref(), Some("lisp.eval"));

    // Verify step 6 is loop.
    assert_eq!(manifest.steps[5].action, "loop");

    // Verify the convergence block uses the Cauchy-only model.
    assert_eq!(
        manifest.convergence.convergence_mode, "cauchy",
        "kali-audit should use the Cauchy-only convergence mode after migration"
    );
    assert_eq!(
        manifest.convergence.cauchy_epsilon, 0.03,
        "kali-audit cauchy_epsilon should be 0.03"
    );
    assert_eq!(
        manifest.convergence.cauchy_window, 3,
        "kali-audit cauchy_window should be 3"
    );

    // Verify max_iterations is 10 (Cauchy model default).
    assert_eq!(
        manifest.convergence.max_iterations, 10,
        "max_iterations should be 10 after Cauchy migration"
    );

    // Verify gas cap is positive.
    assert!(manifest.gas.cap > 0, "gas cap must be positive");

    // Verify steps are present.
}

/// Verify the scenario-builder manifest loads correctly after Co-evolution
/// Phase 1 + Phase 2 migration. Three native MCP `execute` steps:
///   - Step 1: scenario_calibration (Phase 2 — read prior calibration curve)
///   - Step 3: market_match (Phase 1 — fetch prediction-market records)
///   - Step 9: scenario_build (Phase 1 — persist generated scenarios)
/// The manifest grew from 8 to 11 steps.
#[test]
fn scenario_builder_manifest_loads_with_execute_steps() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/scenario-builder.yaml");
    if !manifest_path.exists() {
        eprintln!("scenario-builder.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load scenario-builder manifest: {e}"));

    // 6 select steps + 1 compute step + 3 execute steps + 1 loop step = 11 total.
    assert_eq!(
        manifest.steps.len(),
        11,
        "expected 11 steps after Co-evolution Phase 1 + Phase 2 (3 execute steps)"
    );

    // Three execute steps: scenario_calibration (1), market_match (3),
    // scenario_build (9).
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    assert_eq!(
        execute_steps.len(),
        3,
        "manifest must have 3 execute steps (Co-evolution Phase 1 + Phase 2)"
    );
    assert_eq!(
        execute_steps[0].ordinal, 1,
        "scenario_calibration execute at ordinal 1"
    );
    assert_eq!(
        execute_steps[0].mcp.as_deref(),
        Some("scenario_calibration"),
        "step 1 must call scenario_calibration"
    );
    assert_eq!(
        execute_steps[1].ordinal, 3,
        "market_match execute at ordinal 3"
    );
    assert_eq!(
        execute_steps[1].mcp.as_deref(),
        Some("market_match"),
        "step 3 must call market_match"
    );
    assert_eq!(
        execute_steps[2].ordinal, 9,
        "scenario_build execute at ordinal 9"
    );
    assert_eq!(
        execute_steps[2].mcp.as_deref(),
        Some("scenario_build"),
        "step 9 must call scenario_build"
    );
    // Every execute step must have on_failure (no silent collapse to defaults).
    for step in &execute_steps {
        assert!(
            step.on_failure.is_some(),
            "execute step {} must have on_failure config (no silent collapse)",
            step.ordinal
        );
    }

    // The loop step (ordinal 11) must reference step_10_result for convergence.
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 11, "loop step should be ordinal 11");
}

/// Verify the kanban-task-management manifest loads correctly after Co-evolution
/// Phase 1 migration. Four native MCP `execute` steps plus one `mcp_batch`
/// step (kanban_board_list + kanban_task_list run concurrently) replace the
/// "post-cascade instructions for the agent" pattern for deterministic
/// single-call tool invocations:
///   - Step 6: kanban_board_create (decompose phase — create the board)
///   - Step 8: kanban_task_spawn (delegate phase — spawn the subagent)
///   - Step 10: kanban_task_comment (delegate phase — post status comment)
///   - Step 11: mcp_batch { kanban_board_list, kanban_task_list }
///             (operate phase — fetch board state + task list concurrently)
/// Multi-task creation and LLM-judgment tool calls remain agent-mediated.
/// The manifest has 19 steps (was 20 before the step 11/12 batch merge).
#[test]
fn kanban_task_management_manifest_loads_with_execute_steps() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/kanban-task-management.yaml");
    if !manifest_path.exists() {
        eprintln!("kanban-task-management.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load kanban-task-management manifest: {e}"));

    // 13 select steps + 4 execute steps (3 single-mcp + 1 mcp_batch) +
    // 1 compute step + 1 loop step = 19 total. The mcp_batch step replaces
    // two execute steps (board_list + task_list) with one concurrent batch.
    // The compute step (lisp.eval) extracts the convergence signal
    // deterministically from the last completed phase's result.
    assert_eq!(
        manifest.steps.len(),
        19,
        "expected 19 steps after Co-evolution Phase 1 + step 11/12 batch merge (3 execute + 1 mcp_batch + 1 lisp.eval convergence-signal step)"
    );

    // Four execute steps: three single-mcp (board_create, task_spawn,
    // task_comment) plus one mcp_batch (board_list + task_list). Each is
    // condition-gated on a triage phase.
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    assert_eq!(
        execute_steps.len(),
        4,
        "manifest must have 4 execute steps after step 11/12 batch merge (3 single-mcp + 1 mcp_batch)"
    );
    // Single-mcp execute steps (exclude the mcp_batch step).
    let single_mcp_execute: Vec<_> = execute_steps.iter().filter(|s| s.mcp.is_some()).collect();
    assert_eq!(single_mcp_execute.len(), 3, "3 single-mcp execute steps");
    assert_eq!(
        single_mcp_execute[0].ordinal, 6,
        "kanban_board_create execute at ordinal 6"
    );
    assert_eq!(
        single_mcp_execute[0].mcp.as_deref(),
        Some("kanban_board_create"),
        "step 6 must call kanban_board_create"
    );
    assert_eq!(
        single_mcp_execute[1].ordinal, 8,
        "kanban_task_spawn execute at ordinal 8"
    );
    assert_eq!(
        single_mcp_execute[1].mcp.as_deref(),
        Some("kanban_task_spawn"),
        "step 8 must call kanban_task_spawn"
    );
    assert_eq!(
        single_mcp_execute[2].ordinal, 10,
        "kanban_task_comment execute at ordinal 10"
    );
    assert_eq!(
        single_mcp_execute[2].mcp.as_deref(),
        Some("kanban_task_comment"),
        "step 10 must call kanban_task_comment"
    );

    // The mcp_batch step at ordinal 11 runs kanban_board_list and
    // kanban_task_list concurrently.
    let batch_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "execute" && s.mcp_batch.is_some())
        .expect("manifest must have an mcp_batch step");
    assert_eq!(
        batch_step.ordinal, 11,
        "mcp_batch step should be ordinal 11"
    );
    assert_eq!(
        batch_step.mcp_batch.as_ref().unwrap().len(),
        2,
        "mcp_batch at step 11 must have 2 sub-calls (board_list + task_list)"
    );
    let batch_mcps: Vec<_> = batch_step
        .mcp_batch
        .as_ref()
        .unwrap()
        .iter()
        .map(|c| c.mcp.as_str())
        .collect();
    assert!(
        batch_mcps.contains(&"kanban_board_list"),
        "mcp_batch must include kanban_board_list"
    );
    assert!(
        batch_mcps.contains(&"kanban_task_list"),
        "mcp_batch must include kanban_task_list"
    );

    // Every execute step (including the mcp_batch step) must have on_failure
    // and a condition gate.
    for step in manifest.steps.iter().filter(|s| s.action == "execute") {
        assert!(
            step.on_failure.is_some(),
            "execute step {} must have on_failure config (no silent collapse)",
            step.ordinal
        );
        assert!(
            step.condition.is_some(),
            "execute step {} must have a condition gate (triage phase)",
            step.ordinal
        );
    }

    // The loop step (ordinal 19) must reference the final phase outputs.
    // Ordinal shifted from 20 to 19 when the step 11/12 batch merge removed
    // one step.
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 19, "loop step should be ordinal 19");
}

/// Verify the swarm-intelligence manifest loads correctly after Co-evolution
/// Phase 1 migration. Four native MCP `execute` steps replace the
/// agent-mediated state fetch in SENSE and CHECK:
///   - Step 1: swarm_get_swarm (ABW mode — fetch workspace roster)
///   - Step 2: swarm_get_local_swarm (local mode — fetch local swarm roster)
///   - Step 8: swarm_get_swarm (ABW mode — re-fetch post-Act)
///   - Step 9: swarm_get_local_swarm (local mode — re-fetch post-Act)
/// The ACT phase remains agent-mediated (consent-gated).
/// The manifest grew from 9 to 13 steps.
#[test]
fn swarm_intelligence_manifest_loads_with_execute_steps() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/swarm-intelligence.yaml");
    if !manifest_path.exists() {
        eprintln!("swarm-intelligence.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load swarm-intelligence manifest: {e}"));

    // 5 select steps + 3 compute steps + 4 execute steps + 1 loop step + 1 choice = 14 total.
    // The third compute step (lisp.eval) extracts the convergence signal
    // deterministically from the CHECK step's convergence_metric field.
    assert_eq!(
        manifest.steps.len(),
        14,
        "expected 14 steps after Co-evolution Phase 1 (4 execute steps + 1 lisp.eval convergence-signal step)"
    );

    // Four execute steps, each condition-gated on mode.
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    assert_eq!(
        execute_steps.len(),
        4,
        "manifest must have 4 execute steps (Co-evolution Phase 1)"
    );
    // Steps 1 and 8: swarm_get_swarm (ABW mode)
    assert_eq!(
        execute_steps[0].ordinal, 1,
        "swarm_get_swarm execute at ordinal 1"
    );
    assert_eq!(
        execute_steps[0].mcp.as_deref(),
        Some("swarm_get_swarm"),
        "step 1 must call swarm_get_swarm"
    );
    // Steps 2 and 9: swarm_get_local_swarm (local mode)
    assert_eq!(
        execute_steps[1].ordinal, 2,
        "swarm_get_local_swarm execute at ordinal 2"
    );
    assert_eq!(
        execute_steps[1].mcp.as_deref(),
        Some("swarm_get_local_swarm"),
        "step 2 must call swarm_get_local_swarm"
    );
    assert_eq!(
        execute_steps[2].ordinal, 8,
        "swarm_get_swarm re-fetch at ordinal 8"
    );
    assert_eq!(
        execute_steps[2].mcp.as_deref(),
        Some("swarm_get_swarm"),
        "step 8 must call swarm_get_swarm"
    );
    assert_eq!(
        execute_steps[3].ordinal, 9,
        "swarm_get_local_swarm re-fetch at ordinal 9"
    );
    assert_eq!(
        execute_steps[3].mcp.as_deref(),
        Some("swarm_get_local_swarm"),
        "step 9 must call swarm_get_local_swarm"
    );
    // Every execute step must have on_failure and a condition gate.
    for step in &execute_steps {
        assert!(
            step.on_failure.is_some(),
            "execute step {} must have on_failure config (no silent collapse)",
            step.ordinal
        );
        assert!(
            step.condition.is_some(),
            "execute step {} must have a condition gate (mode)",
            step.ordinal
        );
    }

    // The loop step (ordinal 14) must reference the convergence-signal
    // compute step (step 13, which reads step_10_result) and re-enter at
    // step 1 (state-fetch) so execute steps re-run each iteration.
    // Ordinal shifted from 13 to 14 when a lisp.eval convergence-signal
    // compute step was inserted at ordinal 13.
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 14, "loop step should be ordinal 14");
    let loop_mapping = loop_step
        .input_mapping
        .as_ref()
        .and_then(|v| v.as_object())
        .expect("loop step has input_mapping");
    let loop_target = loop_mapping
        .get("loop_target")
        .and_then(|v| v.as_str())
        .expect("loop step has loop_target");
    assert!(
        loop_target.contains("1"),
        "loop_target must re-enter at step 1 (state-fetch) so execute steps re-run, got: {loop_target}"
    );
}

/// Verify the gemba-walk manifest loads correctly with the expected step
/// structure. The gemba walk implements the Prepare and Present phases of
/// the gemba loop (docs/reports/gemba-loop-specification.md). It is a
/// single-pass briefing generator — not an interactive session.
///
/// Step structure (10 steps):
///   - Step 1: execute (curator_algedonic_log) — SENSE
///   - Step 2: execute (curator_escalations) — GATHER
///   - Step 3: execute (curator_consult) — GATHER
///   - Step 4: execute (curator_grounding_trend) — GATHER
///   - Step 5: execute (curator_grounding_coverage) — GATHER
///   - Step 6: select (synthesize-briefing) — ANALYZE
///   - Step 7: select (present-briefing) — PRESENT
///   - Step 8: select (recommend-actions) — RECOMMEND
///   - Step 9: compute (lisp.eval) — convergence check
///   - Step 10: loop — re-enter if not converged
///
/// Five execute steps call existing curator MCP tools. Three select steps
/// render Jinja2 templates. One compute step extracts the convergence
/// signal. One loop step bounds the cascade.
#[test]
fn gemba_walk_manifest_loads_with_correct_structure() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_dir.join("../..");
    let manifest_path = workspace_root.join("registry/manifests/gemba-walk.yaml");
    if !manifest_path.exists() {
        eprintln!("gemba-walk.yaml not found — skipping");
        return;
    }
    let yaml = std::fs::read_to_string(&manifest_path).unwrap();
    let manifest = hkask_templates::load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("Failed to load gemba-walk manifest: {e}"));

    // 5 execute steps + 3 select steps + 1 compute step + 1 loop step = 10 total.
    // The compute step (lisp.eval) extracts the convergence signal
    // deterministically from the synthesize step's briefing_complete field.
    // Steps 4-5 (grounding trend + coverage) were added in Phase 3 to close
    // the human-in-the-loop feedback loop for grounding health.
    assert_eq!(
        manifest.steps.len(),
        10,
        "expected 10 steps: algedonic_log → escalations → consult → grounding_trend → grounding_coverage → synthesize → present → recommend → compute → loop"
    );

    // Verify step ordinals are sequential starting at 1.
    for (i, step) in manifest.steps.iter().enumerate() {
        assert_eq!(
            step.ordinal,
            (i + 1) as u32,
            "step ordinals must be sequential starting at 1"
        );
    }

    // Verify the five execute steps call the expected curator MCP tools.
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    assert_eq!(
        execute_steps.len(),
        5,
        "manifest must have 5 execute steps (curator MCP tool calls)"
    );
    assert_eq!(
        execute_steps[0].ordinal, 1,
        "curator_algedonic_log at ordinal 1"
    );
    assert_eq!(
        execute_steps[0].mcp.as_deref(),
        Some("curator_algedonic_log"),
        "step 1 must call curator_algedonic_log"
    );
    assert_eq!(
        execute_steps[1].ordinal, 2,
        "curator_escalations at ordinal 2"
    );
    assert_eq!(
        execute_steps[1].mcp.as_deref(),
        Some("curator_escalations"),
        "step 2 must call curator_escalations"
    );
    assert_eq!(execute_steps[2].ordinal, 3, "curator_consult at ordinal 3");
    assert_eq!(
        execute_steps[2].mcp.as_deref(),
        Some("curator_consult"),
        "step 3 must call curator_consult"
    );
    assert_eq!(
        execute_steps[3].ordinal, 4,
        "curator_grounding_trend at ordinal 4"
    );
    assert_eq!(
        execute_steps[3].mcp.as_deref(),
        Some("curator_grounding_trend"),
        "step 4 must call curator_grounding_trend"
    );
    assert_eq!(
        execute_steps[4].ordinal, 5,
        "curator_grounding_coverage at ordinal 5"
    );
    assert_eq!(
        execute_steps[4].mcp.as_deref(),
        Some("curator_grounding_coverage"),
        "step 5 must call curator_grounding_coverage"
    );

    // Every execute step must have on_failure (no silent collapse to defaults).
    for step in &execute_steps {
        assert!(
            step.on_failure.is_some(),
            "execute step {} must have on_failure config (no silent collapse)",
            step.ordinal
        );
    }

    // Verify the three select steps reference the expected templates.
    let select_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "select")
        .collect();
    assert_eq!(
        select_steps.len(),
        3,
        "manifest must have 3 select steps (Jinja2 template rendering)"
    );
    assert_eq!(
        select_steps[0].ordinal, 6,
        "synthesize-briefing at ordinal 6"
    );
    assert_eq!(
        select_steps[0].template_ref.as_deref(),
        Some("gemba-walk/synthesize-briefing"),
        "step 6 must reference gemba-walk/synthesize-briefing"
    );
    assert_eq!(select_steps[1].ordinal, 7, "present-briefing at ordinal 7");
    assert_eq!(
        select_steps[1].template_ref.as_deref(),
        Some("gemba-walk/present-briefing"),
        "step 7 must reference gemba-walk/present-briefing"
    );
    assert_eq!(select_steps[2].ordinal, 8, "recommend-actions at ordinal 8");
    assert_eq!(
        select_steps[2].template_ref.as_deref(),
        Some("gemba-walk/recommend-actions"),
        "step 8 must reference gemba-walk/recommend-actions"
    );

    // Verify the loop step (ordinal 10) re-enters at step 6 (synthesize).
    // Ordinal shifted from 8 to 10 when two grounding execute steps were
    // inserted at ordinals 4-5 (Phase 3: gemba walk grounding integration).
    let loop_step = manifest
        .steps
        .iter()
        .find(|s| s.action == "loop")
        .expect("manifest must have a loop step");
    assert_eq!(loop_step.ordinal, 10, "loop step should be ordinal 10");
    let loop_mapping = loop_step
        .input_mapping
        .as_ref()
        .and_then(|v| v.as_object())
        .expect("loop step has input_mapping");
    let loop_target = loop_mapping
        .get("loop_target")
        .and_then(|v| v.as_str())
        .expect("loop step has loop_target");
    assert!(
        loop_target.contains("6"),
        "loop_target must re-enter at step 6 (synthesize-briefing), got: {loop_target}"
    );

    // Verify the convergence block uses the Cauchy model with a generous
    // iteration budget (the gemba walk synthesizes a potentially large briefing).
    assert_eq!(
        manifest.convergence.convergence_mode, "cauchy",
        "gemba-walk should use the Cauchy convergence mode"
    );
    assert_eq!(
        manifest.convergence.max_iterations, 3,
        "gemba-walk max_iterations should be 3 (single-pass briefing, bounded retries)"
    );
    assert_eq!(
        manifest.convergence.min_iterations, 1,
        "gemba-walk min_iterations should be 1 (single-pass is valid)"
    );

    // Verify gas cap is positive and generous (the briefing queries multiple
    // data sources and synthesizes a large output).
    assert!(
        manifest.gas.cap >= 100000,
        "gas cap must be at least 100000 for a multi-source briefing, got {}",
        manifest.gas.cap
    );

    // Verify the manifest declares the correct span namespace.
    assert_eq!(
        manifest.ledger.span_namespace.as_str(),
        "reg.skill.gemba-walk",
        "ledger span_namespace must be reg.skill.gemba-walk"
    );
}
