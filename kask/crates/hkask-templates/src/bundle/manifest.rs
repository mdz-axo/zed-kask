//! BundleManifest type system — skill manifests for hKask
//!
//! A BundleManifest defines a PDCA cascade of steps that govern how a skill
//! executes. The config sub-structs (ConvergenceConfig, etc.) mirror the
//! fields found in registry/manifests/.

use serde::{Deserialize, Serialize};

use super::config::{
    BundleAuditConfig, BundleLedgerConfig, ConvergenceConfig, ErrorHandlingConfig,
};

/// Manifest category — the closed taxonomy distinguishing agent skills from
/// infrastructure that shares the FlowDef `.yaml` form. Parsing is strict:
/// an unknown value is a load error naming the value, so a typo cannot
/// silently reclassify a skill as infrastructure.
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

/// Default concurrency for step execution within a PDCA iteration.
const DEFAULT_CONCURRENCY: u32 = 32;

/// Default per-step timeout in seconds. Used when a manifest step omits
/// `timeout_seconds` — serde's `#[serde(default = "default_timeout_seconds")]`
/// calls this function instead of `u32::default()` (0). A zero timeout fires
/// immediately without polling the future, silently breaking inference and
/// tool calls.
const DEFAULT_STEP_TIMEOUT_SECONDS: u32 = 120;

pub(crate) fn default_concurrency() -> u32 {
    DEFAULT_CONCURRENCY
}

pub(crate) fn default_timeout_seconds() -> u32 {
    DEFAULT_STEP_TIMEOUT_SECONDS
}

/// Maximum allowed concurrency (safety cap).
pub const MAX_CONCURRENCY: u32 = 128;

/// A single step in a manifest's cascade
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
    #[serde(default)]
    pub compute_ref: Option<String>,
    /// Per-step timeout in seconds (hard — enforced via tokio::time::timeout).
    /// Defaults to 120s when omitted.
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub input_mapping: Option<serde_json::Value>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
    /// Optional condition expression. If present, the step is only executed when
    /// the condition evaluates to true against the current context.
    /// Supported: "var_name" (truthy), "NOT var_name" (falsy),
    /// "a AND b" (both truthy), "a OR b" (either truthy).
    #[serde(default)]
    pub condition: Option<String>,
    /// Branching map: maps a routing key (read from the step result's
    /// `branching_field`, default "routing") to a target step ordinal.
    #[serde(default)]
    pub branching: Option<std::collections::HashMap<String, u32>>,
    /// The field name in the step result to read for `branching` lookup.
    /// Defaults to "routing".
    #[serde(default)]
    pub branching_field: Option<String>,
    /// Agent profile required for this step. When present, the executor verifies
    /// that the `terminal` tool is NOT available — enforcing proposer/evaluator
    /// separation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Shell command for `action: gate` steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Per-step failure handling. When present and the step fails, the executor
    /// applies this instead of the manifest-level `error_handling` policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_failure: Option<OnFailureConfig>,
    /// Batch of MCP tool invocations to run concurrently. When present, the
    /// executor invokes each tool in the batch in parallel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_batch: Option<Vec<McpBatchEntry>>,
}

/// One entry in an `mcp_batch` step — a single MCP tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpBatchEntry {
    /// The MCP tool reference (same format as `BundleManifestStep::mcp`).
    pub mcp: String,
    /// Input mapping for this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<serde_json::Value>,
    /// Key in the result object. Defaults to the tool name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// Per-step failure handling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnFailureConfig {
    /// What to do when the step fails: `halt`, `escalate` (alias for `halt`),
    /// or `report` (call `curator_report_skill_use_issue` before escalating).
    pub action: String,
    /// Human-readable instruction for how to resume from this failure.
    #[serde(default)]
    pub resume: String,
}

/// A skill manifest with cascade steps and convergence config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BundleManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub editor: String,
    pub steps: Vec<BundleManifestStep>,
    pub convergence: ConvergenceConfig,
    pub error_handling: ErrorHandlingConfig,
    pub ledger: BundleLedgerConfig,
    pub audit: BundleAuditConfig,
    #[serde(default)]
    pub functional_role: Option<String>,
    /// Manifest category: agent skill vs infrastructure sharing the FlowDef
    /// form. `None` is treated as `skill` for back-compat.
    #[serde(default)]
    pub category: Option<ManifestCategory>,
    #[serde(default)]
    pub inputs: Option<serde_json::Value>,
    /// Opt-in to runtime validation of caller-supplied context against the
    /// manifest's declared `inputs`.
    #[serde(default)]
    pub enforce_inputs: Option<bool>,
    /// Declared maximum number of steps to execute concurrently within a single
    /// PDCA iteration. Default 32, max 128 (`MAX_CONCURRENCY`).
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

impl BundleManifest {
    /// Returns true if this manifest is an agent-facing skill.
    pub fn is_skill(&self) -> bool {
        matches!(self.category, None | Some(ManifestCategory::Skill))
    }
}
