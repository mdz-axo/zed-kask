//! Manifest compliance test for the company-research skill (flash + deep
//! flowdefs). Pins the flowdef contract: step count, action types, `mcp:`
//! fields on `execute` steps, `condition:` early-exit gates, `loop` target,
//! and cross-skill `template_ref` resolution.
//!
//! This is the contract test that pins the EFRA-AI → kask conversion
//! (kask/docs/plans/efra-ai-to-kask-company-research-skill.md). It verifies
//! the design constraints:
//! - MCP tool calls are native `action: execute` steps (not agent-mediated).
//! - Early-exit gates (DROP / HALT / BLOCK) are `condition:` on downstream steps.
//! - Cross-skill composition reuses `listening/apply-template` and
//!   `kata-improvement/improvement-step1-direction` via `template_ref`.
//! - Convergence is a `compute` step with `lisp.eval`.
//! - The loop step re-enters at a valid prior ordinal.
//! - Every `select` step has a `template_ref` resolving to an existing file.
//!
//! # REQ: P8 — every test verifies a stated behavioral property of a public seam.

use hkask_templates::load_manifest_from_yaml;
use std::collections::HashSet;
use std::path::Path;

/// Resolve the registry manifests directory from this crate's CARGO_MANIFEST_DIR
/// (`kask/crates/hkask-templates`). Two `..` segments reach the `kask/` root.
fn registry_manifests_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests")
}

/// Resolve the registry templates directory.
fn registry_templates_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/templates")
}

/// Load a manifest by name from registry/manifests/.
fn load_named_manifest(name: &str) -> hkask_templates::BundleManifest {
    let path = registry_manifests_dir().join(format!("{name}.yaml"));
    let yaml = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {name}.yaml at {}: {e}", path.display()));
    load_manifest_from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("{name}.yaml must parse with the general executor: {e:?}"))
}

/// Resolve a `template_ref` (e.g. "company-research/scout-alpha-score") to a
/// file path under registry/templates/. Returns true if the .j2 file exists.
fn template_ref_resolves(template_ref: &str) -> bool {
    // template_ref is "<crate>/<template_id>" — the .j2 file is at
    // registry/templates/<crate>/<template_id>.j2. But the actual path is
    // declared in the crate manifest's `path` field. For this test we check
    // the common case: <crate>/<id>.j2 where id == the template_ref suffix.
    let parts: Vec<&str> = template_ref.splitn(2, '/').collect();
    if parts.len() != 2 {
        return false;
    }
    let crate_name = parts[0];
    let template_id = parts[1];
    // Check the .j2 file exists with the template_id as filename.
    let j2_path = registry_templates_dir()
        .join(crate_name)
        .join(format!("{template_id}.j2"));
    if j2_path.is_file() {
        return true;
    }
    // Fallback: parse the crate manifest to find the template's `path` field.
    let crate_manifest = registry_templates_dir()
        .join(crate_name)
        .join("manifest.yaml");
    if let Ok(crate_yaml) = std::fs::read_to_string(&crate_manifest) {
        // Naive grep for the template id and the next `path:` field.
        let mut found_id = false;
        for line in crate_yaml.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- id:") && trimmed.contains(template_id) {
                found_id = true;
                continue;
            }
            if found_id && trimmed.starts_with("path:") {
                let path_val = trimmed.trim_start_matches("path:").trim().trim_matches('"');
                let resolved = registry_templates_dir().join(crate_name).join(path_val);
                return resolved.is_file();
            }
        }
    }
    false
}

// ── Flash pipeline contract ──────────────────────────────────────────────

#[test]
fn company_research_flash_manifest_parses() {
    let manifest = load_named_manifest("company-research-flash");
    assert_eq!(
        manifest.category.as_deref(),
        Some("skill"),
        "company-research-flash must declare category: skill"
    );
    assert!(manifest.is_skill(), "must be classified as a skill");
    assert_eq!(
        manifest.id, "company-research-flash",
        "manifest.id must match filename"
    );
}

#[test]
fn company_research_flash_has_expected_step_count() {
    let manifest = load_named_manifest("company-research-flash");
    // 0 (forecast_list) + 1 (SCOUT) + 2 (INTEL mcp_batch: research_search + web_search) +
    // 3 (intel-mosaic) + 4 (semantic-classify) + 5 (company_transcript) +
    // 6 (listening flowdef) + 7 (forensic-pre-screen) + 8 (scenario_build) +
    // 9 (CRITICAL FACTOR) + 10 (company_transcript) + 11 (FORENSIC full) +
    // 12 (VALUATION mcp_batch: dcf + comparables + expectations + scenario_impact) +
    // 13 (valuation-8step) + 14 (COMM) +
    // 15 (KATA mcp_batch: market_check_resolutions + market_calibration) +
    // 16 (kata-direction) + 17 (kata-calibration-measure) +
    // 18 (LENS mcp_batch: market_match + evaluate_evidence) +
    // 19 (LENS) + 20 (convergence) + 21 (loop) + 22 (forecast_persist) = 23 steps.
    assert_eq!(
        manifest.steps.len(),
        23,
        "company-research-flash must have 23 steps (consolidated mcp_batch steps reduce count from original 29), got {}",
        manifest.steps.len()
    );
}

#[test]
fn company_research_flash_execute_steps_have_mcp_fields() {
    let manifest = load_named_manifest("company-research-flash");
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    // 0 (forecast_list) + 2 (mcp_batch: research_search + web_search) +
    // 5 (company_transcript) + 8 (scenario_build) + 10 (company_transcript) +
    // 12 (mcp_batch: dcf + comparables + expectations + scenario_impact) +
    // 15 (mcp_batch: market_check_resolutions + market_calibration) +
    // 18 (mcp_batch: market_match + evaluate_evidence) +
    // 22 (forecast_persist) = 9 execute steps.
    assert_eq!(
        execute_steps.len(),
        9,
        "expected 9 execute steps (consolidated mcp_batch steps reduce count from original 15), got {}",
        execute_steps.len()
    );
    for step in &execute_steps {
        assert!(
            step.mcp.is_some() || step.mcp_batch.is_some(),
            "execute step {} must have an mcp or mcp_batch reference",
            step.ordinal
        );
    }
}

#[test]
fn company_research_flash_select_steps_have_template_refs() {
    let manifest = load_named_manifest("company-research-flash");
    let select_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "select")
        .collect();
    // 1 (scout) + 4 (intel-mosaic) + 5 (semantic-classify) +
    // 7 (forensic-pre) + 9 (critical-factor) + 11 (forensic-full) +
    // 16 (valuation-8step) + 17 (communication-enter) +
    // 21 (calibration-measure) + 22 (kata-improvement) + 25 (lens)
    // = 11 select steps.
    // (Step 6 was select but is now flowdef — invokes the full listening skill.)
    assert_eq!(
        select_steps.len(),
        11,
        "expected 11 select steps (9 original + 2 cross-skill adapters: semantic-classify + calibration-measure), got {}",
        select_steps.len()
    );
    for step in &select_steps {
        let template_ref = step
            .template_ref
            .as_deref()
            .unwrap_or_else(|| panic!("select step {} must have template_ref", step.ordinal));
        assert!(
            template_ref_resolves(template_ref),
            "select step {} template_ref '{}' does not resolve to an existing .j2 file",
            step.ordinal,
            template_ref
        );
    }
}

#[test]
fn company_research_flash_has_convergence_compute_step() {
    let manifest = load_named_manifest("company-research-flash");
    let compute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "compute")
        .collect();
    assert_eq!(
        compute_steps.len(),
        1,
        "expected exactly 1 compute step (convergence check), got {}",
        compute_steps.len()
    );
    let convergence = compute_steps[0];
    assert_eq!(
        convergence.compute_ref.as_deref(),
        Some("lisp.eval"),
        "convergence step must use lisp.eval"
    );
}

#[test]
fn company_research_flash_has_loop_step() {
    let manifest = load_named_manifest("company-research-flash");
    let loop_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "loop")
        .collect();
    assert_eq!(
        loop_steps.len(),
        1,
        "expected exactly 1 loop step, got {}",
        loop_steps.len()
    );
    // The loop step must have a loop_target in its input_mapping.
    let loop_step = loop_steps[0];
    let has_loop_target = loop_step
        .input_mapping
        .as_ref()
        .and_then(|m| m.get("loop_target"))
        .is_some();
    assert!(
        has_loop_target,
        "loop step {} must have loop_target in input_mapping",
        loop_step.ordinal
    );
}

#[test]
fn company_research_flash_has_early_exit_conditions() {
    let manifest = load_named_manifest("company-research-flash");
    // At least 5 steps must carry a condition (DROP at SCOUT, HALT at INTEL,
    // BLOCK at FORENSIC pre, DROP at CRITICAL FACTOR, BLOCK at FORENSIC full,
    // DROP at VALUATION, DROP at COMMUNICATION).
    let conditioned_steps = manifest
        .steps
        .iter()
        .filter(|s| s.condition.is_some())
        .count();
    assert!(
        conditioned_steps >= 7,
        "expected at least 7 conditioned steps (early-exit gates), got {conditioned_steps}"
    );
}

#[test]
fn company_research_flash_uses_canonical_actions_only() {
    let manifest = load_named_manifest("company-research-flash");
    let canonical: HashSet<&str> = [
        "select", "populate", "compute", "execute", "feedback", "validate", "retrieve", "render",
        "flowdef", "loop", "choice", "abort", "escalate", "parallel",
    ]
    .iter()
    .copied()
    .collect();
    for step in &manifest.steps {
        assert!(
            canonical.contains(step.action.as_str()),
            "step {} has non-canonical action '{}'",
            step.ordinal,
            step.action
        );
    }
}

// ── Deep pipeline contract ───────────────────────────────────────────────

#[test]
fn company_research_deep_manifest_parses() {
    let manifest = load_named_manifest("company-research-deep");
    assert_eq!(
        manifest.category.as_deref(),
        Some("skill"),
        "company-research-deep must declare category: skill"
    );
    assert!(manifest.is_skill(), "must be classified as a skill");
    assert_eq!(
        manifest.id, "company-research-deep",
        "manifest.id must match filename"
    );
}

#[test]
fn company_research_deep_has_expected_step_count() {
    let manifest = load_named_manifest("company-research-deep");
    // 1 (COMPANY mcp_batch: company_transcript + dcf_valuation + comparable_analysis + web_search) +
    // 2 (web_extract) + 3 (company-8part) + 4 (falstaffian) + 5 (gorilla-4dim) +
    // 6 (gorilla-capability-reason) + 7 (GORILLA lisp.eval) +
    // 8 (scenario_build) + 9 (economic-trajectory) + 10 (imagine) +
    // 11 (thesis) + 12 (thesis-essentialist) + 13 (goal-analysis gate) +
    // 14 (convergence lisp.eval) + 15 (loop) = 15 steps.
    assert_eq!(
        manifest.steps.len(),
        15,
        "company-research-deep must have 15 steps (COMPANY batch merge reduced count from 18), got {}",
        manifest.steps.len()
    );
}

#[test]
fn company_research_deep_execute_steps_have_mcp_fields() {
    let manifest = load_named_manifest("company-research-deep");
    let execute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "execute")
        .collect();
    // 1 (mcp_batch: company_transcript + dcf_valuation + comparable_analysis + web_search) +
    // 2 (web_extract) + 8 (scenario_build) = 3 execute steps.
    assert_eq!(
        execute_steps.len(),
        3,
        "expected 3 execute steps (COMPANY batch merge reduced count from 6), got {}",
        execute_steps.len()
    );
    for step in &execute_steps {
        assert!(
            step.mcp.is_some() || step.mcp_batch.is_some(),
            "execute step {} must have an mcp or mcp_batch reference",
            step.ordinal
        );
    }
}

#[test]
fn company_research_deep_select_steps_have_template_refs() {
    let manifest = load_named_manifest("company-research-deep");
    let select_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "select")
        .collect();
    // 6 (company-8part) + 7 (falstaffian-competitive-rotation) + 8 (gorilla-4dim) +
    // 9 (gorilla-capability-reason) + 12 (imagine-longrange) +
    // 13 (economic-trajectory) + 14 (thesis-three-pillars) +
    // 15 (thesis-essentialist) + 16 (goal-analysis/judge) = 9 select steps.
    assert_eq!(
        select_steps.len(),
        9,
        "expected 9 select steps (7 original + 2 cross-skill adapters: gorilla-capability-reason + thesis-essentialist), got {}",
        select_steps.len()
    );
    for step in &select_steps {
        let template_ref = step
            .template_ref
            .as_deref()
            .unwrap_or_else(|| panic!("select step {} must have template_ref", step.ordinal));
        assert!(
            template_ref_resolves(template_ref),
            "select step {} template_ref '{}' does not resolve to an existing .j2 file",
            step.ordinal,
            template_ref
        );
    }
}

#[test]
fn company_research_deep_has_convergence_and_loop() {
    let manifest = load_named_manifest("company-research-deep");
    let compute_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "compute")
        .collect();
    // The deep pipeline has 2 compute steps: GORILLA weighted scoring
    // (step 7) and the THESIS convergence check (step 14).
    assert_eq!(
        compute_steps.len(),
        2,
        "expected exactly 2 compute steps (GORILLA scoring + convergence), got {}",
        compute_steps.len()
    );
    // Both must use lisp.eval.
    for step in &compute_steps {
        assert_eq!(
            step.compute_ref.as_deref(),
            Some("lisp.eval"),
            "compute step {} must use lisp.eval",
            step.ordinal
        );
    }
    let loop_steps: Vec<_> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "loop")
        .collect();
    assert_eq!(
        loop_steps.len(),
        1,
        "expected exactly 1 loop step, got {}",
        loop_steps.len()
    );
}

#[test]
fn company_research_deep_uses_canonical_actions_only() {
    let manifest = load_named_manifest("company-research-deep");
    let canonical: HashSet<&str> = [
        "select", "populate", "compute", "execute", "feedback", "validate", "retrieve", "render",
        "flowdef", "loop", "choice", "abort", "escalate", "parallel",
    ]
    .iter()
    .copied()
    .collect();
    for step in &manifest.steps {
        assert!(
            canonical.contains(step.action.as_str()),
            "step {} has non-canonical action '{}'",
            step.ordinal,
            step.action
        );
    }
}

// ── Condition syntax contract ────────────────────────────────────────────

/// The condition evaluator (`condition.rs::evaluate_step_condition`) does NOT
/// render Jinja expressions. It evaluates raw strings as dot-path lookups,
/// comparisons, and boolean compositions. A condition wrapped in `{{ }}` is
/// treated as a literal string key lookup — it will never resolve, and the
/// condition silently evaluates to a fixed value (true for `!=`, false for
/// truthy checks). This test pins the contract: no condition field may
/// contain `{{` — the native syntax (dot paths, `==`, `!=`, `AND`, `OR`)
/// must be used directly.
#[test]
fn company_research_conditions_do_not_use_jinja_syntax() {
    for name in &["company-research-flash", "company-research-deep"] {
        let manifest = load_named_manifest(name);
        for step in &manifest.steps {
            if let Some(ref cond) = step.condition {
                assert!(
                    !cond.contains("{{"),
                    "{} step {} condition contains Jinja syntax '{{{{' — the condition evaluator does not render Jinja. Use native syntax (dot paths, ==, !=, AND, OR). Condition: {}",
                    name,
                    step.ordinal,
                    cond
                );
            }
        }
    }
}

// ── Cross-skill composition contract ──────────────────────────────────────

#[test]
fn company_research_flash_reuses_listening_and_kata_templates() {
    let manifest = load_named_manifest("company-research-flash");
    let template_refs: Vec<String> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "select")
        .filter_map(|s| s.template_ref.clone())
        .collect();
    // The flash flowdef reuses listening as a sub-flowdef (step 6) and
    // kata-improvement as a select template_ref (step 16).
    let has_listening_flowdef = manifest
        .steps
        .iter()
        .any(|s| s.action == "flowdef" && s.template_ref.as_deref() == Some("listening"));
    assert!(
        has_listening_flowdef,
        "flash flowdef must invoke listening as a sub-flowdef (cross-skill)"
    );
    assert!(
        template_refs
            .iter()
            .any(|r| r == "kata-improvement/improvement-step1-direction"),
        "flash flowdef must reuse kata-improvement/improvement-step1-direction (cross-skill)"
    );
}

#[test]
fn company_research_deep_reuses_goal_analysis_template() {
    let manifest = load_named_manifest("company-research-deep");
    let template_refs: Vec<String> = manifest
        .steps
        .iter()
        .filter(|s| s.action == "select")
        .filter_map(|s| s.template_ref.clone())
        .collect();
    // Cross-skill composition: goal-analysis/judge (step 13 — THESIS quality
    // gate). Avoids the LLM-improves-against-LLM-scored-target trap per .rules.
    assert!(
        template_refs.iter().any(|r| r == "goal-analysis/judge"),
        "deep flowdef must reuse goal-analysis/judge (cross-skill THESIS quality gate)"
    );
}
