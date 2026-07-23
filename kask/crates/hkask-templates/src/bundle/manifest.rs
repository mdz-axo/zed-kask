//! BundleManifest type system — skill bundling for hKask
//!
//! A BundleManifest composes multiple skills into a coherent agent capability,
//! declaring conflicts, complementarities, and cascade steps that govern
//! how the bundled skills execute together.
//!
//! The config sub-structs (ConvergenceConfig, BundleGasConfig, etc.) mirror the
//! fields found in existing process manifests under `registry/manifests/`.

use serde::{Deserialize, Serialize};

use super::cascade::CascadePhase;
use super::composition::{BundleComplementarity, BundleConflict};
use super::config::{
    BundleAuditConfig, BundleGasConfig, BundleLedgerConfig, ConvergenceConfig, ErrorHandlingConfig,
    OcapConfig, RjouleConfig,
};
use hkask_types::SkillPolarity;
use hkask_types::Visibility;

/// A skill reference within a bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BundleSkill {
    pub id: String,
    pub polarity: SkillPolarity,
    pub lexicon_terms: Vec<String>,
    pub manifest_ref: String,
    pub content_hash: String,
}

/// A single step in a bundle's cascade
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BundleManifestStep {
    pub ordinal: u32,
    pub action: String,
    pub description: String,
    pub renderer: Option<String>,
    pub template_ref: Option<String>,
    pub mcp: Option<String>,
    /// Canonical computation function to invoke for `action: compute` steps.
    /// Names a `hkask_forecast::*` primitive (e.g. "calibrate_from_fermi",
    /// "outside_view_adjustment", "bayesian_update", "apply_calibration_adjustment",
    /// "brier_score", "brier_interpretation"). The step's `input_mapping` binds
    /// the function's arguments from prior step results; the result is stored
    /// as `step_{ordinal}_result`. This connects the skill pipeline to the
    /// deterministic math layer without an LLM round-trip.
    #[serde(default)]
    pub compute_ref: Option<String>,
    /// Per-step gas budget estimate (informational — total gas.cap is the hard boundary).
    #[serde(default)]
    pub gas_cap: u32,
    /// Per-step timeout in seconds (hard — enforced via tokio::time::timeout).
    #[serde(default)]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub input_mapping: Option<serde_json::Value>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub phase: CascadePhase,
    /// Optional condition expression. If present, the step is only executed when
    /// the condition evaluates to true against the current context.
    /// Supported: "var_name" (truthy), "NOT var_name" (falsy),
    /// "a AND b" (both truthy), "a OR b" (either truthy).
    #[serde(default)]
    pub condition: Option<String>,
    /// Per-step fusion override. When `Some(true)`, this step routes through
    /// the fusion multi-model panel even if the manifest-level fusion is false.
    /// When `Some(false)`, this step bypasses fusion even if the manifest-level
    /// fusion is true. When `None`, inherits the manifest-level fusion setting.
    /// Convergence checks and gates typically set `fusion: false` to avoid
    /// multi-model deliberation on deterministic rubric evaluation.
    #[serde(default)]
    pub fusion: Option<bool>,
}

impl BundleManifestStep {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.phase is a valid CascadePhase variant
    /// post: returns the PascalCase string representation of the cascade phase
    pub fn phase_str(&self) -> &'static str {
        self.phase.as_str()
    }
}

/// Composed bundle of skills with declared conflicts, complementarities, and cascade steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BundleManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub editor: String,
    pub visibility: Visibility,
    pub skills: Vec<BundleSkill>,
    pub conflicts: Vec<BundleConflict>,
    pub complementarities: Vec<BundleComplementarity>,
    pub steps: Vec<BundleManifestStep>,
    pub convergence: ConvergenceConfig,
    pub gas: BundleGasConfig,
    pub rjoule: RjouleConfig,
    pub error_handling: ErrorHandlingConfig,
    pub ocap: OcapConfig,
    pub ledger: BundleLedgerConfig,
    pub audit: BundleAuditConfig,
    #[serde(default)]
    pub functional_role: Option<String>,
    /// Manifest category: `skill` | `qa-script` | `runtime-config` | `daemon-process` | `pipeline`.
    /// Skills are agent PDCA loops; the rest are infrastructure sharing the FlowDef form.
    /// `None` is treated as `skill` for back-compat.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub inputs: Option<serde_json::Value>,
    #[serde(default)]
    pub principles: Option<serde_json::Value>,
    /// Manifest-level fusion configuration. When `Some`, all steps in this
    /// manifest route through this fusion config (panel models in parallel,
    /// judge synthesizes). When `None`, follows the global default. Per-step
    /// `fusion: Some(false)` bypasses fusion for that step; `fusion: Some(true)`
    /// forces the manifest config even if the manifest-level config is None.
    #[serde(default)]
    pub fusion: Option<hkask_types::fusion::FusionConfig>,
}

impl BundleManifest {
    /// Returns true if this manifest is an agent-facing skill (a PDCA loop
    /// used by agents via `process_manifest`). Infrastructure manifests
    /// (`qa-script`, `runtime-config`, `daemon-process`, `pipeline`) share the
    /// FlowDef form but are not agent skills and must not bind as process
    /// manifests. `None` category is treated as `skill` for back-compat.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// pre:  self is a loaded BundleManifest
    /// post: returns true iff `category` is `skill` or unset
    pub fn is_skill(&self) -> bool {
        matches!(self.category.as_deref(), None | Some("skill"))
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a fully constructed BundleManifest
    /// post: returns ValidationResult with errors for hard violations (skill count, cascade depth, P1 polarity, etc.) and warnings for soft recommendations
    pub fn validate(&self) -> ValidationResult {
        let mut errors: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        if self.skills.len() < 2 {
            errors.push(format!(
                "Bundle must have at least 2 skills, found {}",
                self.skills.len()
            ));
        }
        if self.steps.len() > 7 {
            errors.push(format!(
                "Cascade depth exceeds matroshka limit ({} steps, max 7)",
                self.steps.len()
            ));
        }
        for skill in &self.skills {
            if skill.lexicon_terms.len() > 10 {
                errors.push(format!(
                    "Skill '{}' has {} lexicon terms (max 10)",
                    skill.id,
                    skill.lexicon_terms.len()
                ));
            }
        }
        let all_terms: std::collections::HashSet<&str> = self
            .skills
            .iter()
            .flat_map(|s| s.lexicon_terms.iter().map(|t| t.as_str()))
            .collect();
        if all_terms.len() > 30 {
            warnings.push(format!(
                "Bundle has {} unique lexicon terms (recommended max 30)",
                all_terms.len()
            ));
        }
        // P1: No divergent + convergent in the same phase
        let polarities_in = |phase: CascadePhase| -> Vec<&SkillPolarity> {
            self.steps
                .iter()
                .filter(|s| s.phase == phase)
                .filter_map(|s| {
                    self.skills
                        .iter()
                        .find(|sk| sk.id == s.description)
                        .map(|sk| &sk.polarity)
                })
                .collect()
        };
        for (phase, name) in [(CascadePhase::Pre, "Pre"), (CascadePhase::Core, "Core")] {
            let ps = polarities_in(phase);
            if ps.iter().any(|p| p.is_divergent()) && ps.iter().any(|p| p.is_convergent()) {
                errors.push(format!(
                    "P1 violation: divergent and convergent skills in same {name} phase"
                ));
            }
        }
        let skill_ids: std::collections::HashSet<&str> =
            self.skills.iter().map(|s| s.id.as_str()).collect();
        for conflict in &self.conflicts {
            for skill_id in &conflict.skills {
                if !skill_ids.contains(skill_id.as_str()) {
                    errors.push(format!(
                        "Conflict references skill '{}' not found in bundle",
                        skill_id
                    ));
                }
            }
            if conflict.skills.len() != 2 {
                errors.push(format!(
                    "Conflict must reference exactly 2 skills, found {}",
                    conflict.skills.len()
                ));
            }
        }
        for comp in &self.complementarities {
            for skill_id in &comp.skills {
                if !skill_ids.contains(skill_id.as_str()) {
                    errors.push(format!(
                        "Complementarity references skill '{}' not found in bundle",
                        skill_id
                    ));
                }
            }
            if comp.skills.len() != 2 {
                warnings.push(format!(
                    "Complementarity typically references 2 skills, found {}",
                    comp.skills.len()
                ));
            }
        }
        let mut ordinals: Vec<u32> = self.steps.iter().map(|s| s.ordinal).collect();
        ordinals.sort();
        for (i, expected) in ordinals.iter().enumerate() {
            if *expected != (i as u32) + 1 {
                errors.push(format!(
                    "Step ordinals not sequential: expected {}, found {}",
                    (i as u32) + 1,
                    expected
                ));
                break;
            }
        }
        if !self.version.contains('.') {
            warnings.push(format!(
                "Version '{}' does not follow semantic versioning",
                self.version
            ));
        }
        for skill in &self.skills {
            if skill.content_hash.is_empty() {
                warnings.push(format!("Skill '{}' has empty content_hash", skill.id));
            }
        }

        // Skill validity: iterative manifests must have loop + threshold + gas + exit
        if self.convergence.max_iterations > 1 {
            let has_loop = self.steps.iter().any(|s| s.action == "loop");
            if !has_loop {
                errors.push(
                    "Iterative manifest (max_iterations > 1) must contain a loop action".into(),
                );
            }
            if self.convergence.threshold <= 0.0 {
                errors.push("Iterative manifest must declare convergence.threshold > 0".into());
            }
        }
        if self.gas.cap == 0 {
            errors.push("Manifest must declare gas.cap > 0 (energy budget)".into());
        }
        let has_exit = self
            .steps
            .iter()
            .any(|s| s.action == "abort" || s.action == "escalate");
        if !has_exit {
            warnings.push("Manifest has no abort or escalate action".into());
        }

        ValidationResult { errors, warnings }
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self.steps is populated with valid BundleManifestStep entries
    /// post: returns the sum of all step gas_cap values
    pub fn total_step_gas(&self) -> u32 {
        self.steps.iter().map(|s| s.gas_cap).sum()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  phase is a valid CascadePhase variant
    /// post: returns Vec of &BundleSkill references for skills whose step description contains their id and whose phase matches
    pub fn skills_in_phase(&self, phase: CascadePhase) -> Vec<&BundleSkill> {
        self.steps
            .iter()
            .filter(|s| s.phase == phase)
            .filter_map(|step| {
                self.skills
                    .iter()
                    .find(|sk| step.description.contains(&sk.id))
            })
            .collect()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns `Vec<String>` of all skill ids in the bundle
    pub fn skill_ids(&self) -> Vec<String> {
        self.skills.iter().map(|s| s.id.clone()).collect()
    }
}

/// Result of validating a BundleManifest.
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if errors is empty (no hard violations); false otherwise
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// post: returns true if warnings is non-empty; false otherwise
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}
