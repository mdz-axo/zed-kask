//! Run the dual-layer skill audit against the real hKask project tree.
//!
//! Usage: cargo run --example audit_skills -p hkask-services-skill

use hkask_services_skill::audit::{SkillAuditor, SkillStatus};
use std::path::Path;

fn main() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("examples run two levels below project root");

    let registry: hkask_templates::Registry = hkask_templates::Registry::bootstrap();
    let mut skill_index: hkask_templates::Registry = hkask_templates::Registry::new();
    let loader = hkask_templates::SkillLoader::new(project_root);
    loader.load_into(&mut skill_index);

    let auditor = SkillAuditor::new(&registry, &skill_index, project_root);
    let report = auditor.audit_all().expect("audit all skills");

    let json = report.to_json().expect("serialize report");
    let out_path = project_root.join("tmp").join("skill-audit.json");
    let md_path = project_root.join("tmp").join("skill-audit.md");
    std::fs::write(&out_path, &json).expect("write JSON report");
    println!("Wrote {}", out_path.display());

    let mut md = String::new();
    md.push_str("# Dual-Layer Skill Audit Summary\n\n");
    md.push_str(&format!(
        "Workspace version: {}\n\n",
        report.workspace_version
    ));
    md.push_str("| Skill | Zed | Registry | Score | Status | Defects |\n");
    md.push_str("|-------|-----|----------|-------|--------|---------|\n");
    let mut active = 0;
    let mut stale = 0;
    let mut critical = 0;
    let mut deprecated = 0;
    let mut complete = 0;
    let mut registry_only = 0;
    for score in &report.entries {
        if score.zed_layer_present && score.registry_layer_present {
            complete += 1;
        } else if score.registry_layer_present {
            registry_only += 1;
        }
        match score.status {
            SkillStatus::Active => active += 1,
            SkillStatus::StaleWarning => stale += 1,
            SkillStatus::Critical => critical += 1,
            SkillStatus::RecommendDeprecation => deprecated += 1,
        }
        md.push_str(&format!(
            "| {} | {} | {} | {:.2} | {:?} | {} |\n",
            score.skill_name,
            if score.zed_layer_present {
                "\u{2713}"
            } else {
                "\u{2717}"
            },
            if score.registry_layer_present {
                "\u{2713}"
            } else {
                "\u{2717}"
            },
            score.health_score,
            score.status,
            score.defects.len()
        ));
    }
    md.push_str(&format!("\nTotals: {} skills, {} complete, {} registry-only, {} active, {} stale, {} critical, {} deprecated.\n", report.entries.len(), complete, registry_only, active, stale, critical, deprecated));
    std::fs::write(&md_path, md).expect("write markdown report");
    println!("Wrote {}", md_path.display());

    println!(
        "\nTotals: {} skills, {} complete, {} registry-only, {} active, {} stale, {} critical, {} deprecated.",
        report.entries.len(),
        complete,
        registry_only,
        active,
        stale,
        critical,
        deprecated
    );
}
