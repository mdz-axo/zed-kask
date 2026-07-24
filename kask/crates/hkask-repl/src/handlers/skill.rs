//! `/skill` REPL commands — skill discovery, status, publishing, auditing.
//!
//! Calls `hkask-services-skill` directly (same service layer the deleted CLI
//! commands used). Skill *execution* is via `/invoke skill/skill_execute`
//! (the `hkask-mcp-skill` MCP server).

use crate::ReplState;
use hkask_services_skill::{SkillAuditor, discover_skills};
use hkask_types::SkillZone;
use hkask_types::visibility::Visibility;
use std::path::PathBuf;

/// Handle `/skill` REPL commands.
pub fn handle_skill(subcommand: &str, rest: &str, _state: &mut ReplState) {
    let root = project_root();

    match subcommand {
        "" | "help" => {
            println!("  \x1b[1mSkill Commands\x1b[0m");
            println!("    \x1b[36m/skill list [public|private]\x1b[0m   List skills");
            println!("    \x1b[36m/skill status <name>\x1b[0m          Show skill status");
            println!("    \x1b[36m/skill publish <name>\x1b[0m         Publish a private skill");
            println!("    \x1b[36m/skill audit [--json]\x1b[0m          Audit all skills");
            println!(
                "    \x1b[36m/skill derive <name>\x1b[0m          Derive SKILL.md from registry"
            );
            println!();
            println!("  \x1b[2mSkill execution: /invoke skill/skill_execute\x1b[0m");
            println!();
        }

        "list" => {
            let vis_filter = rest
                .split_whitespace()
                .next()
                .and_then(Visibility::parse_str);
            list_skills(&root, vis_filter);
        }

        "status" => {
            let name = rest.trim();
            if name.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Skill name required");
                println!("  Usage: \x1b[36m/skill status <name>\x1b[0m");
                println!();
                return;
            }
            skill_status(&root, name);
        }

        "publish" => {
            let name = rest.trim();
            if name.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Skill name required");
                println!("  Usage: \x1b[36m/skill publish <name>\x1b[0m");
                println!();
                return;
            }
            skill_publish(&root, name);
        }

        "audit" => {
            let json = rest.trim().contains("--json");
            skill_audit(&root, json);
        }

        "derive" => {
            let name = rest.trim();
            if name.is_empty() {
                println!("  \x1b[31mError:\x1b[0m Skill name required");
                println!("  Usage: \x1b[36m/skill derive <name>\x1b[0m");
                println!();
                return;
            }
            println!("  \x1b[2mSkill derivation is a build-time operation.\x1b[0m");
            println!("  \x1b[2mUse: cargo xtask skill derive {}\x1b[0m", name);
            println!();
        }

        _ => {
            println!("  Unknown skill subcommand: \x1b[31m{}\x1b[0m", subcommand);
            println!("  Type \x1b[36m/skill help\x1b[0m for available commands.");
            println!();
        }
    }
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn list_skills(root: &std::path::Path, vis_filter: Option<Visibility>) {
    for zone in [SkillZone::Private, SkillZone::Public] {
        let zone_dir = root.join(zone.directory());
        if !zone_dir.exists() {
            continue;
        }

        let skill_infos = match discover_skills(&zone_dir) {
            Ok(dirs) => dirs,
            Err(e) => {
                eprintln!("  Error scanning {}: {}", zone_dir.display(), e);
                continue;
            }
        };

        if skill_infos.is_empty() {
            continue;
        }

        println!("  {} zone ({}):", zone.as_str(), zone_dir.display());

        for info in &skill_infos {
            if let Some(filter) = vis_filter
                && info.visibility != filter
            {
                continue;
            }

            let hash_display = info
                .content_hash
                .as_deref()
                .map(|h| &h[..h.len().min(12)])
                .unwrap_or("-");
            let ns_display = info.namespace.as_deref().unwrap_or("-");

            println!(
                "    {:30} visibility={:8} namespace={:12} hash={}",
                info.name,
                info.visibility.as_str(),
                ns_display,
                hash_display
            );
        }
    }
    println!();
}

fn skill_status(root: &std::path::Path, name: &str) {
    let private_dir = root.join(SkillZone::Private.directory()).join(name);
    let public_dir = root
        .join(SkillZone::Public.directory())
        .join(format!("--{}", name));

    if !private_dir.exists() && !public_dir.exists() {
        eprintln!("  Skill '{}' not found in private or public zone", name);
        return;
    }

    println!("  Skill: {}", name);

    if private_dir.exists() {
        let skill_md = private_dir.join("SKILL.md");
        if skill_md.exists() {
            let hash = hkask_services_skill::compute_file_hash(&skill_md);
            println!(
                "    Private: {} (hash: {})",
                private_dir.display(),
                hash.as_deref().unwrap_or("-")
            );
        } else {
            println!("    Private: {} (no SKILL.md)", private_dir.display());
        }
    } else {
        println!("    Private: (not found)");
    }

    if public_dir.exists() {
        let skill_md = public_dir.join("SKILL.md");
        if skill_md.exists() {
            let hash = hkask_services_skill::compute_file_hash(&skill_md);
            println!(
                "    Public:  {} (hash: {})",
                public_dir.display(),
                hash.as_deref().unwrap_or("-")
            );
        } else {
            println!("    Public:  {} (no SKILL.md)", public_dir.display());
        }
    } else {
        println!("    Public:  (not published)");
    }
    println!();
}

fn skill_publish(root: &std::path::Path, name: &str) {
    match hkask_services_skill::publish_skill(root, name) {
        Ok(result) => {
            println!("  \x1b[32m✓\x1b[0m Published skill '{}'", result.name);
            println!("    Namespaced: {}", result.namespaced_name);
            println!("    Namespace:  {}", result.namespace);
            println!("    Public dir: {}", result.public_dir.display());
            println!();
        }
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Publish failed: {}", e);
            println!();
        }
    }
}

fn skill_audit(root: &std::path::Path, json: bool) {
    // The auditor needs a RegistryIndex and SkillRegistryIndex. The REPL's
    // service_context has a SqliteRegistry, but it's behind an Arc<Mutex>.
    // For the audit, we construct a fresh in-memory registry (same as the
    // old CLI command did).
    let registry = match hkask_templates::SqliteRegistry::new(None) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Failed to initialize registry: {}", e);
            return;
        }
    };

    use hkask_types::{RegistryIndex, SkillRegistryIndex};
    let registry_ref: &dyn RegistryIndex = &registry;
    let skill_index_ref: &dyn SkillRegistryIndex = &registry;

    let auditor = SkillAuditor::new(registry_ref, skill_index_ref, root);

    if json {
        match auditor.audit_all() {
            Ok(report) => match report.to_json() {
                Ok(json_str) => println!("{}", json_str),
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m JSON serialize failed: {}", e),
            },
            Err(e) => eprintln!("  \x1b[31m✗\x1b[0m Audit failed: {}", e),
        }
        return;
    }

    match auditor.audit_all() {
        Ok(report) => {
            println!("  \x1b[1mSkill Audit Report\x1b[0m");
            println!("  Active skills: {}", report.active_count());
            println!("  Non-skill crates: {}", report.non_skill_count());
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
            }

            if report.entries.is_empty() {
                println!("  (no skills found)");
            }
            println!();
        }
        Err(e) => {
            eprintln!("  \x1b[31m✗\x1b[0m Audit failed: {}", e);
            println!();
        }
    }
}
