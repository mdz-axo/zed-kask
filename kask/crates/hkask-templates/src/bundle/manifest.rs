//! BundleManifest type system — skill bundling for hKask
//!
//! A BundleManifest composes multiple skills into a coherent agent capability,
//! declaring conflicts, complementarities, and cascade steps that govern
//! how the bundled skills execute together.
//!
//! The config sub-structs (ConvergenceConfig, etc.) mirror the
//! fields found in existing process manifests under `registry/manifests/`.

use serde::{Deserialize, Serialize};

use super::composition::{BundleComplementarity, BundleConflict};
use super::config::{
    BundleAuditConfig, BundleLedgerConfig, ConvergenceConfig, ErrorHandlingConfig, RjouleConfig,
};
use hkask_types::SkillPolarity;

/// Cascade phase — where a step sits in the Pre/Core/Post pipeline.
///
/// Inlined from `bundle/cascade.rs` (F2 pass-through sweep — the module was
/// a 23-line file for a 3-variant enum with one consumer, this file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum CascadePhase {
    Pre,
    #[default]
    Core,
    Post,
}

/// Manifest category — the closed taxonomy distinguishing agent skills from
/// infrastructure that shares the FlowDef `.yaml` form. Parsing is strict:
/// an unknown value is a load error naming the value (see
/// `deserialize_manifest_category`), so a typo cannot silently reclassify a
/// skill as infrastructure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ManifestCategory {
    /// Agent PDCA loop, bindable as an agent `process_manifest`.
    Skill,
    /// QA script run by `kask qa` (no live consumer — retained for the
    /// documented taxonomy).
    QaScript,
    /// System bootstrap config (no live consumer — retained for the
    /// documented taxonomy).
    RuntimeConfig,
    /// Regulation/Curator daemon, run directly — not agent-bound (no live
    /// consumer — retained for the documented taxonomy).
    DaemonProcess,
    /// MCP-server/pipeline process, executed via `execute_pipeline`.
    Pipeline,
    /// Company-source manifest (`registry/company-sources/*.yaml`) — the
    /// operator's source policy, not a public skill.
    CompanySourceManifest,
}

impl<'de> Deserialize<'de> for ManifestCategory {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "skill" => Ok(ManifestCategory::Skill),
            "qa-script" => Ok(ManifestCategory::QaScript),
            "runtime-config" => Ok(ManifestCategory::RuntimeConfig),
            "daemon-process" => Ok(ManifestCategory::DaemonProcess),
            "pipeline" => Ok(ManifestCategory::Pipeline),
            "company-source-manifest" => Ok(ManifestCategory::CompanySourceManifest),
            other => Err(serde::de::Error::custom(format!(
                "unknown manifest category '{other}' — must be one of: skill, qa-script, \
                 runtime-config, daemon-process, pipeline, company-source-manifest"
            ))),
        }
    }
}

impl std::fmt::Display for ManifestCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ManifestCategory::Skill => "skill",
            ManifestCategory::QaScript => "qa-script",
            ManifestCategory::RuntimeConfig => "runtime-config",
            ManifestCategory::DaemonProcess => "daemon-process",
            ManifestCategory::Pipeline => "pipeline",
            ManifestCategory::CompanySourceManifest => "company-source-manifest",
        };
        write!(f, "{s}")
    }
}

// as_str pre:  self is a valid CascadePhase variant
// as_str post: returns PascalCase string ("Pre", "Core", "Post")
// parse_str pre:  s is PascalCase or lowercase ("Pre"/"pre", "Core"/"core", "Post"/"post")
// parse_str post: returns Some(CascadePhase) if s matches; None otherwise
hkask_types::enum_str_ops!(CascadePhase, {
    Pre => ("Pre", "pre"),
    Core => ("Core", "core"),
    Post => ("Post", "post"),
});

/// Default concurrency for step execution within a PDCA iteration.
const DEFAULT_CONCURRENCY: u32 = 32;

/// A golden-output fixture for maintenance-time validation of skills with
/// deterministic-ish output contracts. The skill is run with the provided
/// input context and the output is compared exactly against
/// `expected_output`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenOutputFixture {
    /// A JSON object string parsed into the skill's context map. Example:
    /// `{"task": "generate a flowchart of a login flow"}`.
    pub input: String,
    /// The exact expected output string from the skill cascade.
    pub expected_output: String,
}

/// Default per-step timeout in seconds. Used when a manifest step omits
/// `timeout_seconds` — serde's `#[serde(default = "default_timeout_seconds")]`
/// calls this function instead of `u32::default()` (0). A zero timeout
/// causes `tokio::time::timeout` to fire immediately without polling the
/// future, silently breaking every `select` (inference) and `execute` (tool)
/// step that doesn't explicitly set a timeout.
const DEFAULT_STEP_TIMEOUT_SECONDS: u32 = 120;

pub(crate) fn default_concurrency() -> u32 {
    DEFAULT_CONCURRENCY
}

pub(crate) fn default_timeout_seconds() -> u32 {
    DEFAULT_STEP_TIMEOUT_SECONDS
}

/// Maximum allowed concurrency (safety cap).
pub const MAX_CONCURRENCY: u32 = 128;

/// A skill reference within a bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BundleSkill {
    pub id: String,
    pub polarity: SkillPolarity,
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
    /// Optional string identifier for the step. Used by pipeline manifests
    /// that reference steps by name (e.g. `resume_from: "extract_text"`).
    /// Skill manifests use `ordinal` exclusively; `id` is `None` for them.
    /// When present, it supplements `ordinal` as a human-readable alias —
    /// the executor still indexes by `StepId` (vector position).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Canonical computation function to invoke for `action: compute` steps.
    /// Names a `hkask_forecast::*` primitive (e.g. "calibrate_from_fermi",
    /// "outside_view_adjustment", "bayesian_update", "apply_calibration_adjustment",
    /// "brier_score", "brier_interpretation"). The step's `input_mapping` binds
    /// the function's arguments from prior step results; the result is stored
    /// as `step_{ordinal}_result`. This connects the skill pipeline to the
    /// deterministic math layer without an LLM round-trip.
    #[serde(default)]
    pub compute_ref: Option<String>,
    /// Per-step timeout in seconds (hard — enforced via tokio::time::timeout).
    /// Defaults to 120s when omitted — a zero timeout fires immediately without
    /// polling the future, silently breaking inference and tool calls.
    #[serde(default = "default_timeout_seconds")]
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
    /// Branching map: maps a routing key (read from the step result's
    /// `branching_field`, default "routing") to a target step ordinal. When
    /// present, the executor reads the routing key from the step result after
    /// execution and jumps to the target step instead of continuing to the
    /// next ordinal. Enables `select` and `execute` steps to route based on
    /// their own output (e.g., a proptest fail → re-enter the tracer, a
    /// bug-hunt gap → re-enter the plan). If the routing field is absent or
    /// does not match any key, execution continues to the next ordinal
    /// (safe default — no branching).
    #[serde(default)]
    pub branching: Option<std::collections::HashMap<String, u32>>,
    /// The field name in the step result to read for `branching` lookup.
    /// Defaults to "routing". The step result's field value (a string) must
    /// match a key in `branching`.
    #[serde(default)]
    pub branching_field: Option<String>,
    /// Agent profile required for this step. When present, the executor verifies
    /// that the `terminal` tool is NOT available — enforcing proposer/evaluator
    /// separation (a proposer with `terminal` can evaluate its own tests, a
    /// self-confirming loop anti-pattern). The check uses a `terminal_check`
    /// callback (wired by the bridge with `AgentProfileSettings::is_tool_enabled`)
    /// when available; falls back to `ToolPort::discover_tools()` (MCP tools
    /// only — won't find built-in `terminal` in production) when the callback
    /// is absent. Production enforcement requires the bridge to wire the
    /// callback via `ManifestExecutor::with_terminal_check`.
    /// Example: `profile: ask` (the built-in `ask` profile omits `terminal`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Shell command for `action: gate` steps. The executor runs this via
    /// `std::process::Command::new("sh").arg("-c").arg(command)`, captures
    /// stdout/stderr, and checks the last non-empty line for `GATE_PASS` or
    /// `GATE_FAIL`. A non-zero exit code is also a failure. The full stdout
    /// is stored as the step result for downstream inspection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Per-step failure handling. When present and the step fails (gate
    /// failure, tool error, or timeout exhaustion), the executor applies
    /// this instead of the manifest-level `error_handling` policy.
    /// `action: halt` produces `Effect::Exit(ExitKind::Escalated)` with the
    /// `resume` text; `action: escalate` is an alias.
    /// `action: report` calls `curator_report_skill_use_issue` (skill name,
    /// tool name, step ordinal, error) before escalating — wires the
    /// skill-use reporting loop (Co-evolution Phase 2, Loop 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<OnFailureConfig>,
    /// Batch of MCP tool invocations to run concurrently. When present, the
    /// executor invokes each tool in the batch in parallel (gated by the
    /// global concurrency limiter), collects results into a `Value::Object`
    /// keyed by `entry.key` (defaulting to the tool name), and stores the
    /// object as the step result. Mutually exclusive with `mcp` — a step
    /// declares either a single `mcp` call or an `mcp_batch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_batch: Option<Vec<McpBatchEntry>>,
}

/// One entry in an `mcp_batch` step — a single MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpBatchEntry {
    /// The MCP tool reference (same format as `BundleManifestStep::mcp`).
    pub mcp: String,
    /// Input mapping for this tool call. Resolved against the step context
    /// the same way as a single-call step's `input_mapping`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<serde_json::Value>,
    /// Key in the result object. Defaults to the tool name (the last segment
    /// of the `mcp` reference after any `/` or `.`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Per-step failure handling configuration for pipeline manifests.
/// Allows each step to declare its own failure behavior instead of relying
/// solely on the manifest-level `error_handling` policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnFailureConfig {
    /// What to do when the step fails: `halt` (stop the cascade, escalate),
    /// `escalate` (alias for `halt`), or `report` (call
    /// `curator_report_skill_use_issue` with the failure details, then
    /// escalate — wires the Co-evolution Phase 2 skill-use reporting loop).
    pub action: String,
    /// Human-readable instruction for how to resume from this failure.
    /// Stored in the step result and surfaced to the operator.
    #[serde(default)]
    pub resume: String,
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
    pub skills: Vec<BundleSkill>,
    pub conflicts: Vec<BundleConflict>,
    pub complementarities: Vec<BundleComplementarity>,
    pub steps: Vec<BundleManifestStep>,
    pub convergence: ConvergenceConfig,
    pub rjoule: RjouleConfig,
    pub error_handling: ErrorHandlingConfig,
    pub ledger: BundleLedgerConfig,
    pub audit: BundleAuditConfig,
    #[serde(default)]
    pub functional_role: Option<String>,
    /// Manifest category: agent skill vs infrastructure sharing the FlowDef
    /// form. Parsed as [`ManifestCategory`] — an unknown value in YAML is a
    /// load error naming the value, not a silent "not a skill" (a typo'd
    /// `catgory: skill` previously classified the manifest as infrastructure
    /// with no signal). `None` is treated as `skill` for back-compat.
    #[serde(default)]
    pub category: Option<ManifestCategory>,
    #[serde(default)]
    pub inputs: Option<serde_json::Value>,
    /// Opt-in to runtime validation of caller-supplied context against the
    /// manifest's declared `inputs` (see `inputs::validate_inputs`). When
    /// `Some(true)`, the skill executor rejects invocations that omit a
    /// `required` input or supply a value whose JSON type does not match the
    /// declared `type`. Unknown keys are warned, not rejected (manifests may
    /// declare inputs sparsely). Default `None` = no validation, preserving
    /// back-compat for skills whose required inputs are supplied programmatically
    /// rather than via the interactive `skill` tool's `context` map.
    #[serde(default)]
    pub enforce_inputs: Option<bool>,
    #[serde(default)]
    pub principles: Option<serde_json::Value>,
    /// Declared maximum number of steps to execute concurrently within a single
    /// PDCA iteration. Default 32, max 128 (`MAX_CONCURRENCY`); set to 1 for
    /// strictly serial execution.
    ///
    /// **Not yet enforced at the manifest level.** The kernel's `run_pass` is
    /// strictly sequential; this field is parsed and round-tripped but has no
    /// scheduling effect on the top-level iteration loop. `concurrency: 1` and
    /// `concurrency: 32` produce identical output — pinned by
    /// `executor_baseline_contract::concurrency_field_has_no_effect_today`.
    ///
    /// Concurrency is wired at the `parallel` step action level (slice K2) via
    /// `input_mapping.concurrency_cap`, which bounds in-flight branch futures
    /// (`futures::stream::buffer_unordered`). That is a per-step cap, not a
    /// manifest-wide scheduler — the manifest-level `concurrency` field remains
    /// advisory.
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    /// Optional golden-output fixtures for maintenance-time validation of
    /// skills with deterministic-ish output contracts. Each fixture is a
    /// `(input_context_json, expected_output)` pair. When present,
    /// `BridgeManifestExecutor::validate_golden_outputs` runs the skill
    /// against the input and compares the output string exactly.
    ///
    /// Not a runtime gate — this is a maintenance-time check used by
    /// `skill-maintenance` and the gemba walk briefing. Not applicable to
    /// methodology-driven synthesis skills (which have no golden output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub golden_outputs: Option<Vec<GoldenOutputFixture>>,
}

impl BundleManifest {
    /// Returns true if this manifest is an agent-facing skill (a PDCA loop
    /// used by agents via `process_manifest`). Infrastructure manifests
    /// (`qa-script`, `runtime-config`, `daemon-process`, `pipeline`,
    /// `company-source-manifest`) share the FlowDef form but are not agent
    /// skills and must not bind as process manifests. `None` category is
    /// treated as `skill` for back-compat.
    ///
    /// expect: "The system resolves and executes template manifest cascades"
    /// pre:  self is a loaded BundleManifest
    /// post: returns true iff `category` is `skill` or unset
    pub fn is_skill(&self) -> bool {
        matches!(self.category, None | Some(ManifestCategory::Skill))
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is a fully constructed BundleManifest
    /// post: returns ValidationResult with errors for hard violations (skill count, cascade depth, P1 polarity, etc.) and warnings for soft recommendations
    pub fn validate(&self) -> ValidationResult {
        let mut errors: Vec<String> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        if self.steps.is_empty() {
            errors.push("Manifest must have at least one step".to_string());
        }
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
        // Ordinals must be sequential. Two valid starting points:
        //   - 0 (pre-processing step pattern, e.g., forecast_list, codegraph_stats)
        //   - 1 (standard sequential pattern)
        // Once the starting ordinal is determined, all subsequent ordinals must
        // be consecutive (no gaps, no duplicates).
        let start = ordinals.first().copied().unwrap_or(1);
        let expected_start = if start == 0 { 0 } else { 1 };
        for (i, expected) in ordinals.iter().enumerate() {
            let want = expected_start + (i as u32);
            if *expected != want {
                errors.push(format!(
                    "Step ordinals not sequential: expected {}, found {}",
                    want, expected
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

        // Skill validity: iterative manifests must have loop + threshold + exit
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `timeout_seconds` defaults to 120 when omitted from a manifest step.
    /// A zero timeout causes `tokio::time::timeout` to fire immediately without
    /// polling the future, silently breaking every `select` (inference) and
    /// `execute` (tool) step that doesn't explicitly set a timeout. This was
    /// the root cause of skill cascades failing — the first `select` step would
    /// time out at 0s before the inference call was even dispatched.
    #[test]
    fn timeout_seconds_defaults_to_nonzero_when_omitted() {
        let yaml = r#"
ordinal: 1
action: select
description: test
renderer: default
template_ref: test.j2
"#;
        let step: BundleManifestStep = serde_yaml_neo::from_str(yaml).expect("parses");
        assert_eq!(
            step.timeout_seconds, DEFAULT_STEP_TIMEOUT_SECONDS,
            "timeout_seconds must default to {DEFAULT_STEP_TIMEOUT_SECONDS}, not 0 — \
             a zero timeout breaks inference calls in the skill cascade"
        );
    }

    /// Ordinals starting at 0 (pre-processing step pattern) must pass
    /// validation. Four production manifests use this pattern:
    /// company-research-flash, metacognition, bug-hunt, diagnose.
    #[test]
    fn validate_accepts_ordinal_zero_start() {
        let yaml = r#"
manifest:
  id: test-ordinal-zero
  category: skill
  name: Test
  description: test
  functional_role: flowdef
  version: 1.0.0
  editor: test
steps:
  - ordinal: 0
    action: execute
    description: pre-processing
    mcp: test_tool
  - ordinal: 1
    action: select
    description: main
    renderer: minijinja
    template_ref: test/template
convergence:
  convergence_mode: ""
  max_iterations: 1
  min_iterations: 1
  on_not_reached: abort
rjoule:
  cap: 1
  alert_threshold: 0.8
  hard_limit: true
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(yaml).expect("manifest should parse");
        let result = manifest.validate();
        assert!(
            result.errors.iter().all(|e| !e.contains("ordinal")),
            "ordinal-0 start should not produce ordinal errors, got: {:?}",
            result.errors
        );
    }

    /// Ordinals with gaps must still fail validation.
    #[test]
    fn validate_rejects_ordinal_gaps() {
        let yaml = r#"
manifest:
  id: test-ordinal-gap
  category: skill
  name: Test
  description: test
  functional_role: flowdef
  version: 1.0.0
  editor: test
steps:
  - ordinal: 1
    action: select
    description: first
    renderer: minijinja
    template_ref: test/template
  - ordinal: 3
    action: select
    description: gap
    renderer: minijinja
    template_ref: test/template
convergence:
  convergence_mode: ""
  max_iterations: 1
  min_iterations: 1
  on_not_reached: abort
rjoule:
  cap: 1
  alert_threshold: 0.8
  hard_limit: true
"#;
        let manifest =
            crate::manifest_loader::load_manifest_from_yaml(yaml).expect("manifest should parse");
        let result = manifest.validate();
        assert!(
            result.errors.iter().any(|e| e.contains("ordinal")),
            "ordinal gap (1→3) should produce an error, got: {:?}",
            result.errors
        );
    }
}
