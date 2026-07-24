//! Dual-layer skill audit harness.
//!
//! Scans the Zed agent layer (`.agents/skills/*/SKILL.md`) and the registry layer
//! (`registry/templates/*/manifest.yaml` + `*.j2`) and produces a health report.
//!
//! REQ: P5-svc-skills-095 — Implement dual-layer skill audit as a reusable service.
//! expect: "The service layer exposes minimal, essential interfaces shared by all surfaces"

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use hkask_templates::SkillLoader;
use hkask_types::template_type::TemplateType;
use hkask_types::visibility::Visibility;
use hkask_types::{RegistryIndex, SkillRegistryIndex};
use serde::{Deserialize, Serialize};

// ── Public API ───────────────────────────────────────────────────────────

/// Auditor for the dual-layer skill corpus.
///
/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
/// pre:  registry and skill_index are valid; project_root points to hKask root
/// post: returns an auditor configured for both layers
pub struct SkillAuditor<'a> {
    registry: &'a dyn RegistryIndex,
    skill_index: &'a dyn SkillRegistryIndex,
    project_root: PathBuf,
}

impl<'a> SkillAuditor<'a> {
    /// Create a new auditor.
    pub fn new(
        registry: &'a dyn RegistryIndex,
        skill_index: &'a dyn SkillRegistryIndex,
        project_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            registry,
            skill_index,
            project_root: project_root.into(),
        }
    }

    /// Audit every skill name found in either layer.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// post: returns a report with a health score and defects per skill
    pub fn audit_all(&self) -> Result<SkillAuditReport, SkillAuditError> {
        let names = self.collect_skill_names()?;
        let mut entries = Vec::with_capacity(names.len());
        for name in names {
            entries.push(self.audit_skill_internal(&name)?);
        }
        Ok(SkillAuditReport {
            workspace_version: WORKSPACE_VERSION.to_string(),
            entries,
        })
    }

    /// Audit a single skill by name.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  name is non-empty
    /// post: returns the skill's health score or an error if audit fails
    pub fn audit_skill(&self, name: &str) -> Result<SkillHealthScore, SkillAuditError> {
        self.audit_skill_internal(name)
    }
}

/// Full audit report for the skill corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAuditReport {
    pub workspace_version: String,
    pub entries: Vec<SkillHealthScore>,
}

impl SkillAuditReport {
    /// Serialize the report to JSON.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// post: returns a JSON string representation of the report
    pub fn to_json(&self) -> Result<String, SkillAuditError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| SkillAuditError::Serialize(format!("JSON serialize: {e}")))
    }

    /// Count of active agent skills (category `skill`, health_score >= 0.8).
    /// Non-skill template crates (pipelines, daemon-processes, etc.) are
    /// audited for template health but not counted here.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// post: returns number of `skill`-category entries with health_score >= 0.8
    pub fn active_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.is_active() && e.category == "skill")
            .count()
    }

    /// Count of non-skill template crates audited (infrastructure sharing the
    /// FlowDef form: pipelines, daemon-processes, qa-scripts, runtime-config).
    pub fn non_skill_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.category != "skill")
            .count()
    }

    /// Count of .j2 files that incorrectly declare template_type FlowDef.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// post: returns number of defects matching "FlowDef declared on .j2"
    pub fn flowdef_on_j2_count(&self) -> usize {
        self.entries
            .iter()
            .map(|e| {
                e.defects
                    .iter()
                    .filter(|d| d.contains("FlowDef declared on .j2"))
                    .count()
            })
            .sum()
    }
}

/// Health score and defect list for one skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHealthScore {
    pub skill_name: String,
    pub zed_layer_present: bool,
    pub registry_layer_present: bool,
    pub health_score: f64,
    pub status: SkillStatus,
    pub defects: Vec<String>,
    pub template_summary: TemplateSummary,
    /// Manifest category (`skill` | `qa-script` | `runtime-config` | `daemon-process` |
    /// `pipeline`), read from `registry/manifests/<name>.yaml`. Defaults to `skill`
    /// when the FlowDef manifest is absent or carries no `category`. Non-`skill`
    /// entries are template crates for infrastructure that shares the FlowDef
    /// form — audited for template health but not counted as agent skills.
    #[serde(default = "default_category")]
    pub category: String,
}

fn default_category() -> String {
    "skill".to_string()
}

impl SkillHealthScore {
    /// True iff the skill is active (health_score >= 0.8).
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// post: returns true iff health_score >= 0.8
    pub fn is_active(&self) -> bool {
        self.health_score >= ACTIVE_THRESHOLD
    }
}

/// Audit status derived from health score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillStatus {
    Active,
    StaleWarning,
    Critical,
    RecommendDeprecation,
}

/// Health-score threshold for an `Active` skill. Shared by [`SkillHealthScore::is_active`]
/// and [`status_from_score`] (adversarial review F17 — was a magic number duplicated).
const ACTIVE_THRESHOLD: f64 = 0.8;

fn status_from_score(score: f64) -> SkillStatus {
    if score >= ACTIVE_THRESHOLD {
        SkillStatus::Active
    } else if score >= 0.5 {
        SkillStatus::StaleWarning
    } else if score >= 0.2 {
        SkillStatus::Critical
    } else {
        SkillStatus::RecommendDeprecation
    }
}

/// Errors emitted by the skill audit harness.
#[derive(Debug, thiserror::Error)]
pub enum SkillAuditError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// String-wrapped because YAML errors arrive from two distinct sources:
    /// `anyhow::Error` (via `SkillLoader::parse_front_matter`) and
    /// `serde_yaml_neo::Error` (via direct `serde_yaml_neo::from_str`). A single
    /// `#[from]` cannot cover both, so the message is preserved as a string
    /// (adversarial review F16 — io provenance restored; yaml deferred).
    #[error("YAML error: {0}")]
    Yaml(String),
    #[error("JSON error: {0}")]
    Serialize(String),
}

/// Counts of templates per type for a skill.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateSummary {
    pub total: usize,
    pub word_act: usize,
    pub know_act: usize,
    pub flow_def: usize,
    pub render_act: usize,
}

/// A typed cross-step reference defect found by `validate_flowdef_refs`.
/// Replaces fragile string-matching for severity (SMELL 4 fix).
#[derive(Debug, Clone)]
pub enum FlowDefDefect {
    ConvergenceFieldNonExistent {
        step: u32,
    },
    ConvergenceFieldNonOutput {
        step: u32,
        action: String,
    },
    LoopTargetInvalid {
        target: u32,
    },
    InputMappingBadRef {
        step: u32,
    },
    TemplatesNotBuilt {
        count: usize,
    },
    /// A `branching:` dispatch target (success/failure/classifier routes) references
    /// a step ordinal that doesn't exist — the skill silently dead-ends at runtime.
    BranchingTargetInvalid {
        target: u32,
    },
    /// An `input_mapping` key doesn't match any input declared in the referenced
    /// template's contract — the template won't receive that variable by name.
    InputMappingContractMismatch {
        step: u32,
        key: String,
    },
    /// A `fusion.skills` composition reference doesn't resolve to a known skill —
    /// the composition will fail at runtime.
    CompositionRefInvalid {
        skill_ref: String,
    },
}

impl FlowDefDefect {
    /// The health-score penalty for this defect.
    pub fn penalty(&self) -> f64 {
        match self {
            Self::TemplatesNotBuilt { .. } => 0.15,
            _ => 0.10,
        }
    }

    /// The human-readable defect message.
    pub fn message(&self) -> String {
        match self {
            Self::ConvergenceFieldNonExistent { step } => format!(
                "convergence_field references step {step}_result but no step {step} exists (silently never converges)"
            ),
            Self::ConvergenceFieldNonOutput { step, action } => format!(
                "convergence_field references step {step} which is a '{action}' step (produces no result; must reference an output-producing step)"
            ),
            Self::LoopTargetInvalid { target } => {
                format!("loop_target {target} references no existing step")
            }
            Self::InputMappingBadRef { step } => format!(
                "input_mapping references step {step}_result but no step {step} exists (resolves to empty at runtime)"
            ),
            Self::TemplatesNotBuilt { count } => format!(
                "FlowDef references {count} template_ref(s) but the crate manifest declares 0 templates (templates not built — skill non-executable)"
            ),
            Self::BranchingTargetInvalid { target } => format!(
                "branching target {target} references no existing step (silently dead-ends the skill)"
            ),
            Self::InputMappingContractMismatch { step, key } => format!(
                "step {step} input_mapping key '{key}' does not match any input declared in template contract (template won't receive this variable)"
            ),
            Self::CompositionRefInvalid { skill_ref } => format!(
                "fusion.skills references '{skill_ref}' which is not a known skill (composition will fail at runtime)"
            ),
        }
    }
}

// ── Internal implementation ──────────────────────────────────────────────

/// Extract every `step_<N>_result` ordinal referenced in `s`. These occur in
/// FlowDef `convergence_field` values and `input_mapping` Jinja `{{ }}` values.
fn extract_step_refs(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("step_") {
        rest = &rest[i + 5..];
        let mut digits = String::new();
        for c in rest.chars() {
            if c.is_ascii_digit() {
                digits.push(c);
            } else {
                break;
            }
        }
        if !digits.is_empty() && rest[digits.len()..].starts_with("_result") {
            if let Ok(n) = digits.parse::<u32>() {
                out.push(n);
            }
            rest = &rest[digits.len() + 7..];
        } else {
            rest = &rest[digits.len().max(1)..];
        }
    }
    out
}

// ── Internal implementation continues ─────────────────────────────────────

impl<'a> SkillAuditor<'a> {
    fn collect_skill_names(&self) -> Result<Vec<String>, SkillAuditError> {
        let mut names = HashSet::new();

        let zed_dir = self.project_root.join(".agents").join("skills");
        if zed_dir.exists() {
            for entry in fs::read_dir(&zed_dir)? {
                let entry = entry?;
                if entry.path().is_dir() {
                    names.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }

        let reg_dir = self.project_root.join("registry").join("templates");
        if reg_dir.exists() {
            for entry in fs::read_dir(&reg_dir)? {
                let entry = entry?;
                if entry.path().is_dir() {
                    names.insert(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }

        // Cross-check with the loaded runtime indexes.
        for skill in self.skill_index.list_skills() {
            names.insert(skill.id);
        }
        for entry in self.registry.list(None) {
            if let Some(skill_name) = entry.id.split('/').next() {
                names.insert(skill_name.to_string());
            }
        }

        let mut names: Vec<String> = names.into_iter().collect();
        names.sort();
        Ok(names)
    }

    fn audit_skill_internal(&self, name: &str) -> Result<SkillHealthScore, SkillAuditError> {
        let zed = self.audit_zed_layer(name)?;
        let reg = self.audit_registry_layer(name)?;

        let mut score = 1.0_f64;
        let mut defects = Vec::new();

        if !zed.present {
            score -= 0.05;
            defects.push("missing SKILL.md companion (info — registry is canonical)".to_string());
        } else {
            if !zed.has_frontmatter {
                score -= 0.10;
                defects.push("SKILL.md missing frontmatter".to_string());
            }
            if !zed.name_matches_dir {
                score -= 0.10;
                defects.push(format!(
                    "SKILL.md name '{}' does not match directory '{}'",
                    zed.name, name
                ));
            }
            if zed.description_len < 20 {
                score -= 0.05;
                defects.push("SKILL.md description too short".to_string());
            }
        }

        if !reg.present {
            score -= 0.50;
            defects.push("missing registry crate (CRITICAL — not executable)".to_string());
        } else {
            if !reg.manifest_present {
                score -= 0.15;
                defects.push("missing manifest.yaml".to_string());
            }
            for j2 in &reg.j2_files {
                if j2.frontmatter_missing {
                    score -= 0.10;
                    defects.push(format!("{}: missing [inference] frontmatter", j2.filename));
                    continue;
                }
                if j2.ddmvss_alias {
                    score -= 0.15;
                    defects.push(format!(
                        "{}: DDMVSS alias template_type {:?} (must be WordAct/KnowAct/FlowDef)",
                        j2.filename, j2.template_type_raw
                    ));
                } else if j2.template_type == Some(TemplateType::FlowDef) {
                    score -= 0.15;
                    defects.push(format!(
                        "{}: FlowDef declared on .j2 file (runtime says FlowDef = YAML .yaml)",
                        j2.filename
                    ));
                } else if j2.template_type.is_none() {
                    score -= 0.10;
                    defects.push(format!("{}: missing or invalid template_type", j2.filename));
                }
                if !j2.visibility_valid {
                    score -= 0.10;
                    defects.push(format!(
                        "{}: invalid visibility {:?}",
                        j2.filename, j2.visibility
                    ));
                }
                if !j2.contract_valid {
                    score -= 0.10;
                    defects.push(format!("{}: missing/empty contract", j2.filename));
                }
                if !j2.energy_cap_valid {
                    score -= 0.05;
                    defects.push(format!(
                        "{}: energy_cap {:?} out of range [2048, 8192]",
                        j2.filename, j2.energy_cap
                    ));
                }
            }
        }

        for d in self.validate_flowdef_refs(name) {
            score -= d.penalty();
            defects.push(d.message());
        }

        if zed.present && reg.present && zed.name != reg.crate_name && !reg.crate_name.is_empty() {
            score -= 0.10;
            defects.push(format!(
                "name mismatch: SKILL.md '{}' vs manifest '{}'",
                zed.name, reg.crate_name
            ));
        }

        score = score.clamp(0.0, 1.0);
        let status = status_from_score(score);

        // Read the manifest category from registry/manifests/<name>.yaml (if
        // present). A template crate with no FlowDef manifest is infrastructure
        // (not a skill) — read_manifest_category returns "infrastructure" in
        // that case. Only `skill`-category entries are counted as agent skills.
        let category = self.read_manifest_category(name);

        Ok(SkillHealthScore {
            skill_name: name.to_string(),
            zed_layer_present: zed.present,
            registry_layer_present: reg.present,
            health_score: score,
            status,
            template_summary: reg.template_summary,
            defects,
            category,
        })
    }

    /// Determine the category of a template crate.
    ///
    /// The **authoritative skill discriminator** is the `.agents/skills/<name>/`
    /// directory — the curated set of agent-facing skills. If that directory
    /// exists, the crate is a `"skill"`, regardless of what the FlowDef manifest
    /// declares (the directory is the ground truth, not a heuristic). If it does
    /// not exist, the crate is infrastructure: the declared `manifest.category`
    /// is returned (e.g. `qa-script`, `runtime-config`, `daemon-process`,
    /// `pipeline`, `infrastructure`), defaulting to `"infrastructure"` when no
    /// FlowDef manifest is present.
    fn read_manifest_category(&self, name: &str) -> String {
        // Ground truth: the curated .agents/skills/<name>/ directory.
        let skill_dir = self.project_root.join(".agents").join("skills").join(name);
        if skill_dir.is_dir() {
            return "skill".to_string();
        }
        // Not in the curated skill set — read the declared category from the
        // FlowDef manifest, or default to infrastructure.
        let path = self
            .project_root
            .join("registry")
            .join("manifests")
            .join(format!("{name}.yaml"));
        let Ok(content) = fs::read_to_string(&path) else {
            return "infrastructure".to_string();
        };
        // Light parse: extract the `category:` field under the `manifest:` block
        // without deserializing the full (schema-varied) manifest.
        let mut in_manifest = false;
        for line in content.lines() {
            if line.starts_with("manifest:") {
                in_manifest = true;
                continue;
            }
            if in_manifest {
                // A top-level key (no leading indent) ends the manifest block.
                if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                    break;
                }
                let trimmed = line.trim_start();
                if let Some(rest) = trimmed.strip_prefix("category:") {
                    return rest.trim().trim_matches('"').to_string();
                }
            }
        }
        "skill".to_string()
    }

    /// Validate cross-step references in the FlowDef manifest
    /// (`registry/manifests/<name>.yaml`):
    /// - `convergence_field` must NOT reference a loop/abort/escalate/choice step
    ///   (which produce no step_N_result), else the skill never converges.
    /// - every `step_N_result` reference in `convergence_field` and `input_mapping`,
    ///   plus `loop_target`, must point to a declared step ordinal.
    /// - a FlowDef that references templates but whose own crate manifest declares
    ///   none (`templates: []`) was never built — flag it as non-executable.
    ///
    /// Surfaces silent self-harming gaps as CI-visible defects.
    fn validate_flowdef_refs(&self, name: &str) -> Vec<FlowDefDefect> {
        let path = self
            .project_root
            .join("registry")
            .join("manifests")
            .join(format!("{name}.yaml"));
        let Ok(content) = fs::read_to_string(&path) else {
            return Vec::new();
        };

        // One pass over the `steps:` block: collect ordinals, per-step action, and
        // template_refs. (ordinal/action/template_ref only occur inside steps.)
        let mut ordinals: HashSet<u32> = HashSet::new();
        let mut step_actions: std::collections::HashMap<u32, String> =
            std::collections::HashMap::new();
        let mut template_refs: Vec<(u32, String)> = Vec::new();
        let mut in_steps = false;
        let mut current_ordinal: Option<u32> = None;
        for line in content.lines() {
            if line.starts_with("steps:") {
                in_steps = true;
                continue;
            }
            if in_steps && !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                in_steps = false;
                current_ordinal = None;
            }
            if !in_steps {
                continue;
            }
            let t = line.trim_start();
            if let Some(r) = t
                .strip_prefix("- ordinal:")
                .or_else(|| t.strip_prefix("ordinal:"))
            {
                if let Ok(n) = r.trim().parse::<u32>() {
                    ordinals.insert(n);
                    current_ordinal = Some(n);
                }
                continue;
            }
            if let Some(r) = t.strip_prefix("action:") {
                if let Some(ord) = current_ordinal {
                    step_actions.insert(ord, r.trim().trim_matches('"').to_string());
                }
                continue;
            }
            if let Some(r) = t.strip_prefix("template_ref:")
                && let Some(ord) = current_ordinal
            {
                template_refs.push((ord, r.trim().trim_matches('"').to_string()));
            }
        }
        if ordinals.is_empty() {
            return Vec::new(); // not a FlowDef manifest (infrastructure config, no steps)
        }

        let mut defects = Vec::new();

        // convergence_field (control-flow: determines convergence) — must NOT reference
        // a loop/abort/escalate/choice step (which produce no step_N_result), else
        // check_convergence reads None and the skill never converges. Custom action
        // vocabularies (render/evaluate/flowdef/...) are assumed to produce output.
        const NON_OUTPUT_ACTIONS: &[&str] = &["loop", "abort", "escalate", "choice"];
        let mut in_convergence = false;
        for line in content.lines() {
            if line.starts_with("convergence:") {
                in_convergence = true;
                continue;
            }
            if in_convergence {
                if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                    in_convergence = false;
                } else if let Some(rest) = line.trim_start().strip_prefix("convergence_field:") {
                    for n in extract_step_refs(rest) {
                        if !ordinals.contains(&n) {
                            defects.push(FlowDefDefect::ConvergenceFieldNonExistent { step: n });
                        } else if let Some(action) = step_actions.get(&n)
                            && NON_OUTPUT_ACTIONS.contains(&action.as_str())
                        {
                            defects.push(FlowDefDefect::ConvergenceFieldNonOutput {
                                step: n,
                                action: action.clone(),
                            });
                        }
                    }
                }
            }
        }

        // loop_target (control-flow: re-entry point) — must be a declared ordinal.
        for line in content.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("loop_target:")
                && let Ok(n) = rest.trim().parse::<u32>()
                && !ordinals.contains(&n)
            {
                defects.push(FlowDefDefect::LoopTargetInvalid { target: n });
            }
        }

        // branching dispatch targets (control-flow: success/failure/classifier
        // routes) — each must be a declared ordinal. A typo here silently
        // dead-ends the skill at runtime.
        let mut bad_branch: HashSet<u32> = HashSet::new();
        let mut in_branching = false;
        let mut branching_indent: usize = 0;
        for line in content.lines() {
            let t = line.trim_start();
            if t.starts_with("branching:") {
                in_branching = true;
                branching_indent = line.len() - t.len();
                continue;
            }
            if in_branching {
                if line.trim().is_empty() {
                    continue;
                }
                let indent = line.len() - line.trim_start().len();
                if indent <= branching_indent {
                    in_branching = false;
                    continue;
                }
                // Branching values are ordinals: "key: <integer>".
                if let Some((_, val)) = t.split_once(':')
                    && let Ok(n) = val.trim().parse::<u32>()
                    && !ordinals.contains(&n)
                {
                    bad_branch.insert(n);
                }
            }
        }
        let mut bad_branch: Vec<u32> = bad_branch.into_iter().collect();
        bad_branch.sort_unstable();
        for n in bad_branch {
            defects.push(FlowDefDefect::BranchingTargetInvalid { target: n });
        }

        // input_mapping data refs (lines containing Jinja {{ }}) — every step_N_result
        // referenced must exist, else resolve_mapping_value yields empty at runtime.
        let mut bad_input: HashSet<u32> = HashSet::new();
        for line in content.lines() {
            if line.contains("{{") {
                for n in extract_step_refs(line) {
                    if !ordinals.contains(&n) {
                        bad_input.insert(n);
                    }
                }
            }
        }
        let mut bad_input: Vec<u32> = bad_input.into_iter().collect();
        bad_input.sort_unstable();
        for n in bad_input {
            defects.push(FlowDefDefect::InputMappingBadRef { step: n });
        }

        // template_ref build-completeness. A FlowDef that references templates but
        // whose own crate manifest declares none (`templates: []`) was never built —
        // the .j2 templates don't exist and the skill is non-executable. This targets
        // the "templates never authored" class precisely (the only such skill today is
        // semantic-graph-audit). Per-ref .j2 path mismatches in FlowDefs whose crates DO
        // declare templates are a separate, lower-severity cohort (often latent because
        // those skills are invoked as KnowAct, not via the FlowDef) — left to a follow-up.
        let crate_manifest_path = self
            .project_root
            .join("registry")
            .join("templates")
            .join(name)
            .join("manifest.yaml");
        let declared_count = fs::read_to_string(&crate_manifest_path)
            .ok()
            .and_then(|c| {
                let m = serde_yaml_neo::from_str::<serde_yaml_neo::Value>(&c).ok()?;
                m.get("templates")
                    .and_then(|v| v.as_sequence())
                    .map(|s| s.len())
            })
            .unwrap_or(0);
        if declared_count == 0 && !template_refs.is_empty() {
            defects.push(FlowDefDefect::TemplatesNotBuilt {
                count: template_refs.len(),
            });
        }

        // input_mapping ↔ template contract validation. Each step's input_mapping
        // keys must match the referenced template's contract input fields, else the
        // template silently won't receive the variable. Uses the shared
        // parse_template_contract_inputs helper so the audit and the cross-validation
        // test stay in sync.
        if let Ok(manifest_val) = serde_yaml_neo::from_str::<serde_yaml_neo::Value>(&content) {
            if let Some(steps) = manifest_val.get("steps").and_then(|v| v.as_sequence()) {
                let templates_root = self.project_root.join("registry").join("templates");
                for step in steps {
                    let ordinal = step.get("ordinal").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let template_ref = step.get("template_ref").and_then(|v| v.as_str());
                    let input_mapping = step.get("input_mapping").and_then(|v| v.as_mapping());
                    let (Some(template_ref), Some(input_mapping)) = (template_ref, input_mapping)
                    else {
                        continue;
                    };
                    // Skip non-standard refs resolved by different engines.
                    if template_ref.contains("${")
                        || template_ref.contains('#')
                        || template_ref.contains("{{")
                        || template_ref.starts_with("process/")
                        || template_ref.starts_with("composition/")
                        || template_ref.starts_with("inference/")
                    {
                        continue;
                    }
                    // Resolve template path matching the executor's direct-path resolution.
                    let exact = templates_root.join(template_ref);
                    let with_j2 = templates_root.join(format!("{template_ref}.j2"));
                    let with_yaml = templates_root.join(format!("{template_ref}.yaml"));
                    let template_path = if exact.exists() {
                        exact
                    } else if !template_ref.ends_with(".j2") && with_j2.exists() {
                        with_j2
                    } else if with_yaml.exists() {
                        continue; // Media workflow .yaml — different schema, skip
                    } else {
                        continue; // Missing — caught by build-completeness / cross-validation test
                    };
                    let Ok(template_content) = fs::read_to_string(&template_path) else {
                        continue;
                    };
                    let contract_inputs =
                        hkask_types::flowdef_validation::parse_template_contract_inputs(
                            &template_content,
                        );
                    if contract_inputs.is_empty() {
                        continue;
                    }
                    let contract_keys: HashSet<&str> =
                        contract_inputs.iter().map(|s| s.as_str()).collect();
                    for (key, _) in input_mapping {
                        let Some(key_str) = key.as_str() else {
                            continue;
                        };
                        // Skip known infrastructure keys that aren't template contract inputs.
                        if key_str == "convergence_description"
                            || key_str == "include_blockers"
                            || key_str == "loop_target"
                        {
                            continue;
                        }
                        if !contract_keys.contains(key_str) {
                            defects.push(FlowDefDefect::InputMappingContractMismatch {
                                step: ordinal,
                                key: key_str.to_string(),
                            });
                        }
                    }
                }
            }

            // fusion.skills composition references — each must resolve to a known
            // skill, else the composition will fail at runtime.
            if let Some(skills) = manifest_val
                .get("fusion")
                .and_then(|v| v.get("skills"))
                .and_then(|v| v.as_sequence())
                && let Ok(known) = self.collect_skill_names()
            {
                for skill_ref in skills {
                    let Some(ref_str) = skill_ref.as_str() else {
                        continue;
                    };
                    if !known.iter().any(|s| s == ref_str) {
                        defects.push(FlowDefDefect::CompositionRefInvalid {
                            skill_ref: ref_str.to_string(),
                        });
                    }
                }
            }
        }

        defects
    }

    fn audit_zed_layer(&self, name: &str) -> Result<ZedLayerInfo, SkillAuditError> {
        let path = self
            .project_root
            .join(".agents")
            .join("skills")
            .join(name)
            .join("SKILL.md");
        if !path.exists() {
            return Ok(ZedLayerInfo::default());
        }
        let content = fs::read_to_string(&path)?;
        let front = SkillLoader::parse_front_matter(&content)
            .map_err(|e| SkillAuditError::Yaml(e.to_string()))?;
        let dir_name = name.to_string();
        let name_matches_dir = front.name == dir_name;
        Ok(ZedLayerInfo {
            present: true,
            has_frontmatter: content.trim_start().starts_with("---"),
            name: front.name,
            description_len: front.description.as_ref().map(|s| s.len()).unwrap_or(0),
            name_matches_dir,
        })
    }

    fn audit_registry_layer(&self, name: &str) -> Result<RegistryLayerInfo, SkillAuditError> {
        let dir = self
            .project_root
            .join("registry")
            .join("templates")
            .join(name);
        if !dir.exists() {
            return Ok(RegistryLayerInfo::default());
        }

        let manifest_path = dir.join("manifest.yaml");
        let manifest_present = manifest_path.exists();
        let mut crate_name = String::new();
        let mut manifest_templates: std::collections::HashMap<String, serde_yaml_neo::Value> =
            std::collections::HashMap::new();
        let mut j2_files = Vec::new();
        let mut summary = TemplateSummary::default();

        if manifest_present {
            let content = fs::read_to_string(&manifest_path)?;
            let manifest: serde_yaml_neo::Value = serde_yaml_neo::from_str(&content)
                .map_err(|e| SkillAuditError::Yaml(e.to_string()))?;
            if let Some(c) = manifest.get("crate").and_then(|v| v.get("name")) {
                crate_name = c.as_str().unwrap_or("").to_string();
            }
            // Collect template metadata from manifest for fallback when [inference] is missing
            if let Some(templates) = manifest.get("templates").and_then(|v| v.as_sequence()) {
                for tmpl in templates {
                    if let Some(path) = tmpl.get("path").and_then(|v| v.as_str()) {
                        manifest_templates.insert(path.to_string(), tmpl.clone());
                    }
                }
            }
        }

        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("j2") {
                let filename = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let info =
                    self.audit_j2_file_with_fallback(&path, &filename, &manifest_templates)?;
                match info.template_type {
                    Some(TemplateType::WordAct) => summary.word_act += 1,
                    Some(TemplateType::KnowAct) => summary.know_act += 1,
                    Some(TemplateType::FlowDef) => summary.flow_def += 1,
                    Some(TemplateType::RenderAct) => summary.render_act += 1,
                    None => {}
                }
                summary.total += 1;
                j2_files.push(info);
            }
        }

        Ok(RegistryLayerInfo {
            present: true,
            manifest_present,
            crate_name,
            j2_files,
            template_summary: summary,
        })
    }

    fn audit_j2_file_with_fallback(
        &self,
        path: &Path,
        filename: &str,
        manifest_templates: &std::collections::HashMap<String, serde_yaml_neo::Value>,
    ) -> Result<J2FileInfo, SkillAuditError> {
        let content = fs::read_to_string(path)?;
        let front = parse_j2_frontmatter(&content);

        // If [inference] frontmatter is missing, try manifest.yaml fallback.
        // Templates that declare their metadata in manifest.yaml (not [inference])
        // should not be penalized for missing [inference] frontmatter.
        let frontmatter_missing = front.is_none();
        let has_manifest_fallback = manifest_templates.contains_key(filename);

        let mut info = J2FileInfo {
            filename: filename.to_string(),
            frontmatter_missing: frontmatter_missing && !has_manifest_fallback,
            ..Default::default()
        };

        if let Some(front) = front {
            info.template_type = front.template_type;
            info.template_type_raw = front.template_type_raw.clone();
            info.visibility = front.visibility.clone();
            info.energy_cap = front.energy_cap;
            info.contract_valid = front.contract_input.is_some() && front.contract_output.is_some();

            const DDMVSS_ALIASES: [&str; 3] = ["Cognition", "Prompt", "Process"];
            if let Some(ref raw) = front.template_type_raw {
                if DDMVSS_ALIASES.contains(&raw.as_str()) {
                    info.ddmvss_alias = true;
                }
                if TemplateType::parse_str(raw).is_some() {
                    info.template_type_valid = true;
                }
            }

            if let Some(ref vis) = front.visibility {
                info.visibility_valid = Visibility::parse_str(vis).is_some();
            }

            if let Some(ec) = front.energy_cap {
                info.energy_cap_valid = (2048..=8192).contains(&ec);
            }
        } else if let Some(tmpl_meta) = manifest_templates.get(filename) {
            // Fallback: use manifest.yaml metadata.
            // Manifest templates declare type/lexicon in manifest.yaml, not [inference].
            // They don't have visibility/energy_cap/contract fields — skip those checks
            // by marking them valid (the manifest format doesn't use those fields).
            //
            // RenderAct templates (reference content, macro libraries, error views)
            // are declared here with `type: RenderAct`; as a first-class TemplateType
            // they parse cleanly and need no special-casing — the manifest fallback's
            // valid-marks below handle their lack of inference-only fields.
            if let Some(ttype) = tmpl_meta.get("type").and_then(|v| v.as_str()) {
                info.template_type = TemplateType::parse_str(ttype);
                info.template_type_raw = Some(ttype.to_string());
                info.template_type_valid = info.template_type.is_some();
            }
            // Mark these valid to avoid penalizing manifest-format templates
            info.visibility_valid = true;
            info.energy_cap_valid = true;
            info.contract_valid = true;
        }

        Ok(info)
    }
}

// ── Layer info structs ───────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ZedLayerInfo {
    present: bool,
    has_frontmatter: bool,
    name: String,
    description_len: usize,
    name_matches_dir: bool,
}

#[derive(Debug, Default)]
struct RegistryLayerInfo {
    present: bool,
    manifest_present: bool,
    crate_name: String,
    j2_files: Vec<J2FileInfo>,
    template_summary: TemplateSummary,
}

#[derive(Debug, Default)]
struct J2FileInfo {
    filename: String,
    frontmatter_missing: bool,
    template_type: Option<TemplateType>,
    template_type_valid: bool,
    template_type_raw: Option<String>,
    ddmvss_alias: bool,
    visibility: Option<String>,
    visibility_valid: bool,
    energy_cap: Option<i64>,
    energy_cap_valid: bool,
    contract_valid: bool,
}

#[derive(Debug, Default)]
#[allow(dead_code)] // populated by serde deserialization; some fields consumed in specific code paths
struct J2FrontMatter {
    template_type: Option<TemplateType>,
    template_type_raw: Option<String>,
    lexicon_terms: Vec<String>,
    contract_input: Option<serde_yaml_neo::Value>,
    contract_output: Option<serde_yaml_neo::Value>,
    energy_cap: Option<i64>,
    visibility: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────────────

const WORKSPACE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn parse_j2_frontmatter(content: &str) -> Option<J2FrontMatter> {
    // Skip leading Jinja comments ({# ... #}) and whitespace before [inference].
    // Templates may have documentation comments before the frontmatter block.
    let mut content = content.trim_start();
    while content.starts_with("{#") {
        if let Some(end) = content.find("#}") {
            content = content[end + 2..].trim_start();
        } else {
            break;
        }
    }
    if !content.starts_with("[inference]") {
        return None;
    }
    let after_header = &content["[inference]".len()..].trim_start_matches('\n');
    let sep = after_header.find("\n---")?;
    let mut yaml_text = &after_header[..sep];
    // Strip TOML-style [contract] section if present — it's not valid YAML
    // and is parsed separately by the contract parser.
    if let Some(contract_pos) = yaml_text.find("\n[contract]") {
        yaml_text = &yaml_text[..contract_pos];
    }
    let yaml: serde_yaml_neo::Value = serde_yaml_neo::from_str(yaml_text).ok()?;
    let map = yaml.as_mapping()?;

    let template_type_raw = map
        .get(serde_yaml_neo::Value::String("template_type".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let template_type = template_type_raw
        .as_deref()
        .and_then(TemplateType::parse_str);

    let lexicon_terms = map
        .get(serde_yaml_neo::Value::String("lexicon_terms".to_string()))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let contract = map
        .get(serde_yaml_neo::Value::String("contract".to_string()))
        .and_then(|v| v.as_mapping());
    let (contract_input, contract_output, nested_energy_cap, nested_visibility) =
        if let Some(c) = contract {
            (
                c.get(serde_yaml_neo::Value::String("input".to_string()))
                    .cloned(),
                c.get(serde_yaml_neo::Value::String("output".to_string()))
                    .cloned(),
                c.get(serde_yaml_neo::Value::String("energy_cap".to_string()))
                    .and_then(|v| v.as_i64()),
                c.get(serde_yaml_neo::Value::String("visibility".to_string()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            )
        } else {
            (None, None, None, None)
        };

    // If YAML contract is missing, check for TOML-style [contract] section.
    // The [contract] section uses inline format: input: {field: type, ...}
    // We mark contract_input/output as Some to indicate contract is present.
    let (contract_input, contract_output) = if contract_input.is_some() {
        (contract_input, contract_output)
    } else {
        let full_text = &after_header[..sep];
        if full_text.contains("[contract]")
            && full_text.contains("input:")
            && full_text.contains("output:")
        {
            (
                Some(serde_yaml_neo::Value::Null),
                Some(serde_yaml_neo::Value::Null),
            )
        } else {
            (None, None)
        }
    };

    let top_level_energy_cap = map
        .get(serde_yaml_neo::Value::String("energy_cap".to_string()))
        .and_then(|v| v.as_i64())
        .or(nested_energy_cap);

    let top_level_visibility = map
        .get(serde_yaml_neo::Value::String("visibility".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or(nested_visibility);

    Some(J2FrontMatter {
        template_type,
        template_type_raw,
        lexicon_terms,
        contract_input,
        contract_output,
        energy_cap: top_level_energy_cap,
        visibility: top_level_visibility,
    })
}

// ── Default version helper ────────────────────────────────────────────────

// Workspace version constant used in audit reports until the harness can read
// Cargo.toml workspace metadata at runtime.

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// scores >= 0.8 and is_active() returns true.
    #[test]
    fn complete_skill_is_active() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();

        // Zed layer
        let zed_dir = root.join(".agents").join("skills").join("test-skill");
        fs::create_dir_all(&zed_dir).unwrap();
        fs::write(
            zed_dir.join("SKILL.md"),
            "---\nname: test-skill\nvisibility: public\ndescription: \"A minimal test skill for the audit harness.\"\n---\n\n# Test Skill\n\nInstructions.\n",
        )
        .unwrap();

        // Registry layer
        let reg_dir = root.join("registry").join("templates").join("test-skill");
        fs::create_dir_all(&reg_dir).unwrap();
        fs::write(
            reg_dir.join("manifest.yaml"),
            "crate:\n  name: test-skill\n  version: 0.28.0\n  description: Minimal test skill.\n\ntemplates:\n  - id: test-skill/test\n    path: test.j2\n    type: KnowAct\n    lexicon_terms: [classify]\n    description: Minimal cognition template.\n\nlexicon_terms:\n  - classify\n",
        )
        .unwrap();
        fs::write(
            reg_dir.join("test.j2"),
            "[inference]\ntemplate_type: KnowAct\nlexicon_terms:\n- classify\ncontract:\n  input:\n    x: string\n  output:\n    y: string\n  energy_cap: 4096\n  visibility: Shared\n---\n{# goal: Minimal test template. #}\n{{ x }}\n",
        )
        .unwrap();

        let mut registry = hkask_templates::Registry::new();
        let loader = SkillLoader::new(root);
        let mut skill_index: hkask_templates::Registry = hkask_templates::Registry::new();
        loader.load_into(&mut skill_index);
        // Seed registry with the template entry so list(None) returns it.
        registry
            .register(hkask_types::RegistryEntry {
                id: "test-skill/test".to_string(),
                template_type: TemplateType::KnowAct,
                name: "test".to_string(),
                lexicon_terms: vec!["classify".to_string()],
                description: "Minimal cognition template".to_string(),
                source_path: "registry/templates/test-skill/test.j2".to_string(),
                required_capabilities: vec![],
                cascade_level: 0,
                matroshka_limit: 7,
            })
            .unwrap();

        let auditor = SkillAuditor::new(
            &registry, &registry, // Registry implements both traits
            root,
        );

        let score = auditor.audit_skill("test-skill").expect("audit");
        assert!(
            score.is_active(),
            "complete skill should be active, got score {} with defects {:?}",
            score.health_score,
            score.defects
        );
    }

    /// A FlowDef manifest with non-existent cross-step references must be flagged:
    /// bad convergence_field, bad input_mapping step ref, and bad loop_target each
    /// surface a defect and drop the score below active.
    #[test]
    fn flowdef_broken_cross_step_refs_are_flagged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let name = "test-broken";

        // Zed layer (present so the missing-companion dock doesn't mask the cross-step defects).
        let zed_dir = root.join(".agents").join("skills").join(name);
        fs::create_dir_all(&zed_dir).unwrap();
        fs::write(
            zed_dir.join("SKILL.md"),
            "---\nname: test-broken\nvisibility: public\ndescription: \"Broken cross-step refs for audit validation.\"\n---\n\n# Test Broken\n\nInstructions.\n",
        )
        .unwrap();

        // Registry crate (clean j2 so no frontmatter defects).
        let reg_dir = root.join("registry").join("templates").join(name);
        fs::create_dir_all(&reg_dir).unwrap();
        fs::write(
            reg_dir.join("manifest.yaml"),
            "crate:\n  name: test-broken\n  version: \"0.31.0\"\n  description: Broken refs.\ntemplates:\n  - id: test-broken/s1\n    path: s1.j2\n    type: KnowAct\n    lexicon_terms: [classify]\n    description: Minimal.\n",
        )
        .unwrap();
        fs::write(
            reg_dir.join("s1.j2"),
            "[inference]\ntemplate_type: KnowAct\nlexicon_terms:\n- classify\ncontract:\n  input:\n    x: string\n  output:\n    y: string\n  energy_cap: 4096\n  visibility: Public\n---\n{# goal: Minimal. #}\n{{ x }}\n",
        )
        .unwrap();

        fs::create_dir_all(root.join("registry").join("manifests")).unwrap();
        // FlowDef manifest with THREE broken cross-step refs: convergence_field -> step 9
        // (none), input_mapping -> step 7 (none), loop_target -> 5 (none).
        fs::write(
            root.join("registry").join("manifests").join(format!("{name}.yaml")),
            "manifest:\n  id: test-broken\n  category: skill\n  name: Test Broken\n  description: Broken refs.\n  functional_role: flowdef\n  version: 0.31.0\n  editor: test\n  visibility: Public\nconvergence:\n  threshold: 0.15\n  max_iterations: 3\n  min_iterations: 0\n  convergence_field: step_9_result.convergence_metric\n  on_not_reached: escalate\nsteps:\n  - ordinal: 1\n    action: select\n    description: step1\n    renderer: minijinja\n    template_ref: test-broken/s1\n    gas_cap: 4096\n    timeout_seconds: 30\n    input_mapping:\n      bad: \"{{ step_7_result.x | default(null) }}\"\n  - ordinal: 2\n    action: loop\n    description: loop\n    input_mapping:\n      loop_target: 5\n",
        )
        .unwrap();

        let registry = hkask_templates::Registry::new();
        let auditor = SkillAuditor::new(&registry, &registry, root);
        let score = auditor.audit_skill(name).expect("audit");
        let defects = score.defects.join("; ");
        assert!(
            defects.contains("convergence_field references step 9"),
            "missing convergence_field defect: {defects}"
        );
        assert!(
            defects.contains("input_mapping references step 7"),
            "missing input_mapping defect: {defects}"
        );
        assert!(
            defects.contains("loop_target 5 references no existing step"),
            "missing loop_target defect: {defects}"
        );
        assert!(
            !score.is_active(),
            "3 broken refs (-0.30) should drop below active (0.8); got {} defects {:?}",
            score.health_score,
            score.defects
        );
    }

    /// A FlowDef manifest with an invalid branching target, an input_mapping
    /// ↔ template contract mismatch, and a bad fusion.skills composition ref
    /// must surface all three defects and drop the score below active.
    #[test]
    fn flowdef_branching_contract_composition_defects_are_flagged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let name = "test-comp";

        // Zed layer.
        let zed_dir = root.join(".agents").join("skills").join(name);
        fs::create_dir_all(&zed_dir).unwrap();
        fs::write(
            zed_dir.join("SKILL.md"),
            "---\nname: test-comp\nvisibility: public\ndescription: \"Test branching, contract, composition defects.\"\n---\n\n# Test Comp\n\nInstructions.\n",
        )
        .unwrap();

        // Registry crate with a template whose contract declares input `x`.
        let reg_dir = root.join("registry").join("templates").join(name);
        fs::create_dir_all(&reg_dir).unwrap();
        fs::write(
            reg_dir.join("manifest.yaml"),
            "crate:\n  name: test-comp\n  version: \"0.31.0\"\n  description: Test comp.\ntemplates:\n  - id: test-comp/s1\n    path: s1.j2\n    type: KnowAct\n    lexicon_terms: [classify]\n    description: Minimal.\n",
        )
        .unwrap();
        fs::write(
            reg_dir.join("s1.j2"),
            "[inference]\ntemplate_type: KnowAct\nlexicon_terms:\n- classify\ncontract:\n  input:\n    x: string\n  output:\n    y: string\n  energy_cap: 4096\n  visibility: Public\n---\n{# goal: Minimal. #}\n{{ x }}\n",
        )
        .unwrap();

        fs::create_dir_all(root.join("registry").join("manifests")).unwrap();
        // FlowDef manifest with THREE defects:
        //   1. branching success -> step 9 (does not exist)
        //   2. input_mapping key 'wrong_key' not in template contract (which has 'x')
        //   3. fusion.skills references 'nonexistent-skill' (not a known skill)
        fs::write(
            root.join("registry").join("manifests").join(format!("{name}.yaml")),
            "manifest:\n  id: test-comp\n  category: skill\n  name: Test Comp\n  description: Test comp.\n  functional_role: flowdef\n  version: 0.31.0\n  editor: test\n  visibility: Public\nconvergence:\n  threshold: 0.15\n  max_iterations: 3\n  min_iterations: 0\n  convergence_field: step_1_result.convergence_metric\n  on_not_reached: escalate\nfusion:\n  judge: test-judge\n  skills:\n    - test-comp\n    - nonexistent-skill\nsteps:\n  - ordinal: 1\n    action: select\n    description: step1\n    renderer: minijinja\n    template_ref: test-comp/s1\n    gas_cap: 4096\n    timeout_seconds: 30\n    input_mapping:\n      wrong_key: \"{{ situation }}\"\n    branching:\n      success: 9\n      failure: 2\n  - ordinal: 2\n    action: abort\n    description: abort\n",
        )
        .unwrap();

        let registry = hkask_templates::Registry::new();
        let auditor = SkillAuditor::new(&registry, &registry, root);
        let score = auditor.audit_skill(name).expect("audit");
        let defects = score.defects.join("; ");
        assert!(
            defects.contains("branching target 9 references no existing step"),
            "missing branching defect: {defects}"
        );
        assert!(
            defects.contains("input_mapping key 'wrong_key' does not match"),
            "missing contract mismatch defect: {defects}"
        );
        assert!(
            defects.contains("fusion.skills references 'nonexistent-skill'"),
            "missing composition defect: {defects}"
        );
        assert!(
            !score.is_active(),
            "3 defects (-0.30) should drop below active (0.8); got {} defects {:?}",
            score.health_score,
            score.defects
        );
    }

    /// Property-based skeleton: once proptest is wired, assert that any skill
    /// with both layers present and all .j2 files valid scores >= 0.8.
    #[test]
    #[ignore = "requires proptest fixture for arbitrary complete skills"]
    fn complete_skill_scores_above_threshold() {
        // TODO: implement with proptest once arbitrary skill fixtures exist.
    }
}
