//! `kask skill` — Skill corpus audit (CI gate).
//!
//! Runtime skill operations (list, status, publish, derive) are REPL-only
//! (`/skill`). The CLI exposes only the structural audit, which CI runs
//! as a gate to enforce skill health-score thresholds.

use crate::cli::SkillAction;
use hkask_services_skill::SkillAuditor;
use std::path::PathBuf;

/// Run a skill audit command.
pub fn run(action: SkillAction) {
    match action {
        SkillAction::Audit { fail_below, json } => {
            skill_audit(fail_below, json);
        }
    }
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn skill_audit(fail_below: f64, json: bool) {
    let root = project_root();
    let registry = match hkask_templates::SqliteRegistry::new(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Audit failed: {e}");
            std::process::exit(1);
        }
    };

    use hkask_types::{RegistryIndex, SkillRegistryIndex};
    let registry_ref: &dyn RegistryIndex = &registry;
    let skill_index_ref: &dyn SkillRegistryIndex = &registry;
    let auditor = SkillAuditor::new(registry_ref, skill_index_ref, &root);

    let report = match auditor.audit_all() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Audit failed: {e}");
            std::process::exit(1);
        }
    };

    let threshold = fail_below.clamp(0.0, 1.0);
    let mut failed = false;

    if json {
        match report.to_json() {
            Ok(output) => println!("{output}"),
            Err(e) => {
                eprintln!("Failed to serialize audit report: {e}");
                std::process::exit(1);
            }
        }
    } else {
        println!("Skill audit report (fail_below={threshold:.2})");
        println!();
        println!(
            "Active skills: {} / {} total entries",
            report.active_count(),
            report.entries.len()
        );
        println!();

        for entry in &report.entries {
            let icon = if entry.is_active() { "✓" } else { "△" };
            println!(
                "  {} {} — score: {:.2}",
                icon, entry.skill_name, entry.health_score
            );
            for defect in &entry.defects {
                println!("    → {}", defect);
            }
            if entry.health_score < threshold {
                failed = true;
            }
        }
    }

    if failed {
        eprintln!("\nAudit FAILED: one or more skills scored below {threshold:.2}");
        std::process::exit(1);
    }
}
