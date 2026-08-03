//! Cross-artifact consistency between `.agents/skills/<name>/SKILL.md` companions
//! and `registry/manifests/<name>.yaml` process manifests.
//!
//! This test compile-enforces the X4 check ("bidirectional name match") and the
//! X2 check ("no orphan SKILL.md") that `skill-maintenance-validate.j2` encodes
//! as LLM-validated KnowAct checks (Z8, X2, X3, X4). The structural floor — name
//! matching, non-empty descriptions, every catalogued skill has a process
//! manifest — is deterministic and belongs in the test suite, not behind an LLM
//! round-trip. Semantic drift (SKILL.md claims a behaviour the registry does not
//! support) remains LLM-validated via X3/Z8; this test does not attempt it.
//!
//! Canonical-source rule: the process manifest's `manifest.description` is the
//! ground truth (it is what the ManifestExecutor runs). The SKILL.md
//! `description` is a derived companion. When they disagree, the registry wins
//! (already stated in every SKILL.md's Constraints section).

use hkask_templates::load_manifest_from_yaml;
use std::collections::HashSet;
use std::path::Path;

/// Parsed SKILL.md frontmatter (the YAML block between `---` delimiters).
#[derive(Debug, serde::Deserialize)]
struct SkillFrontMatter {
    name: String,
    #[serde(default)]
    description: String,
}

/// Extract and parse the YAML frontmatter from a SKILL.md file.
/// Returns `Err` if the file doesn't have valid `---`-delimited frontmatter.
fn parse_skill_frontmatter(content: &str) -> Result<SkillFrontMatter, String> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("missing opening --- delimiter".to_string());
    }
    let after_open = &trimmed[3..];
    let end = after_open
        .find("\n---")
        .ok_or("missing closing --- delimiter")?;
    let yaml_block = &after_open[..end];
    serde_yaml_neo::from_str::<SkillFrontMatter>(yaml_block)
        .map_err(|e| format!("YAML parse error: {e}"))
}

/// Resolve the repo-root `.agents/skills/` directory from this crate's
/// CARGO_MANIFEST_DIR (`kask/crates/hkask-templates`). Three `..` segments
/// reach the zed-kask repo root.
fn global_skills_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(".agents/skills")
}

/// Resolve the `kask/registry/manifests/` directory. Two `..` segments
/// from CARGO_MANIFEST_DIR reach the `kask/` root.
fn registry_manifests_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("registry/manifests")
}

/// X4 (forward): every `.agents/skills/<name>/SKILL.md` has a matching
/// `registry/manifests/<name>.yaml`. Closes the "orphan SKILL.md" gap — a
/// skill advertised in the agent catalog must have a process manifest the
/// executor can run, or it silently falls back to body injection.
#[test]
fn every_skill_md_has_a_process_manifest() {
    let skills_dir = global_skills_dir();
    if !skills_dir.is_dir() {
        eprintln!(
            "{} not found — skipping (not a source-tree build)",
            skills_dir.display()
        );
        return;
    }
    let manifests_dir = registry_manifests_dir();

    let mut errors = Vec::new();
    let mut checked = 0;

    for entry in std::fs::read_dir(&skills_dir).expect("read .agents/skills") {
        let entry = entry.expect("dir entry");
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
        let frontmatter = match parse_skill_frontmatter(&content) {
            Ok(fm) => fm,
            Err(e) => {
                errors.push(format!("{dir_name}: SKILL.md frontmatter invalid: {e}"));
                continue;
            }
        };
        checked += 1;

        // Z3: name field matches directory name
        if frontmatter.name != dir_name {
            errors.push(format!(
                "{dir_name}: SKILL.md name='{}' does not match directory name",
                frontmatter.name
            ));
        }

        // Z4: description non-empty
        if frontmatter.description.trim().is_empty() {
            errors.push(format!("{dir_name}: SKILL.md description is empty"));
        }

        // X4: matching process manifest exists
        let manifest_path = manifests_dir.join(format!("{dir_name}.yaml"));
        if !manifest_path.is_file() {
            errors.push(format!(
                "{dir_name}: no process manifest at registry/manifests/{dir_name}.yaml (orphan SKILL.md — skill is advertised but has no FlowDef cascade)"
            ));
            continue;
        }

        // Load the manifest and verify id matches
        let yaml = std::fs::read_to_string(&manifest_path).unwrap();
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("{dir_name}: manifest failed to load: {e}"));
                continue;
            }
        };
        if manifest.id != dir_name {
            errors.push(format!(
                "{dir_name}: manifest.id='{}' does not match directory name",
                manifest.id
            ));
        }
        if manifest.id != frontmatter.name {
            errors.push(format!(
                "{dir_name}: manifest.id='{}' != SKILL.md name='{}'",
                manifest.id, frontmatter.name
            ));
        }
    }

    eprintln!(
        "Checked {checked} SKILL.md companions — {} errors",
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} cross-artifact consistency errors found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// X4 (reverse): every `category: skill` process manifest has a matching
/// `.agents/skills/<name>/SKILL.md`. Catches manifests that are registered
/// in the executor but invisible to the agent catalog (the model can't invoke
/// a skill it can't discover).
#[test]
fn every_skill_manifest_has_a_skill_md() {
    let manifests_dir = registry_manifests_dir();
    if !manifests_dir.is_dir() {
        eprintln!(
            "{} not found — skipping (not a source-tree build)",
            manifests_dir.display()
        );
        return;
    }
    let skills_dir = global_skills_dir();

    let mut errors = Vec::new();
    let mut skill_manifests: Vec<String> = Vec::new();

    for entry in walkdir::WalkDir::new(&manifests_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue, // load failures caught by other tests
        };

        // Only check skill-category manifests — infra manifests (pipeline,
        // qa-script, runtime-config, daemon-process) intentionally have no
        // SKILL.md companion.
        if !manifest.is_skill() {
            continue;
        }
        skill_manifests.push(manifest.id.clone());

        let skill_md = skills_dir.join(&manifest.id).join("SKILL.md");
        if !skill_md.is_file() {
            errors.push(format!(
                "{}: category:skill manifest has no .agents/skills/{}/SKILL.md (invisible to agent catalog)",
                manifest.id,
                manifest.id
            ));
        }
    }

    eprintln!(
        "Checked {} skill-category manifests — {} errors",
        skill_manifests.len(),
        errors.len()
    );
    for err in &errors {
        eprintln!("  ERR: {err}");
    }
    assert!(
        errors.is_empty(),
        "{} orphan skill manifests found:\n{}",
        errors.len(),
        errors.join("\n")
    );
}

/// No aspirational documentation blocks. Catches the F3 friction pattern: manifests
/// carrying commented-out blocks labelled "removed — documentation-only field not
/// parsed by Rust; deny_unknown_fields enforces schema" that document fields the
/// Rust schema cannot parse. After the C excise, no manifest should contain this
/// pattern. If a new one appears, it means someone added aspirational documentation
/// for a field the executor doesn't support — either implement the field or delete
/// the block.
#[test]
fn no_aspirational_documentation_blocks() {
    let manifests_dir = registry_manifests_dir();
    if !manifests_dir.is_dir() {
        return;
    }

    let mut offenders = Vec::new();

    for entry in walkdir::WalkDir::new(&manifests_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let content = std::fs::read_to_string(path).unwrap_or_default();
        if content.contains("removed — documentation-only field not parsed by Rust") {
            offenders.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "manifests with aspirational documentation blocks (commented-out fields \
         the Rust schema cannot parse): {}",
        offenders.join(", ")
    );
}

/// No duplicate skill names across the catalog. A name collision means the
/// precedence resolution in `SkillSource::precedence` silently shadows one
/// skill with another, which is a debugging dead-end.
#[test]
fn no_duplicate_skill_ids() {
    let manifests_dir = registry_manifests_dir();
    if !manifests_dir.is_dir() {
        return;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates = Vec::new();

    for entry in walkdir::WalkDir::new(&manifests_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(path).unwrap();
        if !yaml.contains("\nmanifest:") && !yaml.starts_with("manifest:") {
            continue;
        }
        let manifest = match load_manifest_from_yaml(&yaml) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !manifest.is_skill() {
            continue;
        }
        if !seen.insert(manifest.id.clone()) {
            duplicates.push(manifest.id);
        }
    }

    assert!(
        duplicates.is_empty(),
        "duplicate skill manifest ids: {}",
        duplicates.join(", ")
    );
}
