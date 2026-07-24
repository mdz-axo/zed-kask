//! Kata manifest types — deserialized from `registry/manifests/*.yaml`.
//!
//! Pure data types with zero behavioral methods. Consumed by `KataEngine::load_manifest()`.

use serde::Deserialize;

/// Top-level kata manifest deserialized from YAML.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
pub struct KataManifest {
    pub manifest: ManifestMeta,
    pub gas: KataGasConfig,
    #[serde(default)]
    pub steps: Vec<KataStep>,
    #[serde(default)]
    pub questions: Vec<CoachQuestion>,
    #[serde(default)]
    pub practices: Vec<PracticeRoutine>,
    #[serde(default)]
    pub error_handling: ErrorHandling,
    pub ledger: KataLedgerConfig,
    #[serde(default)]
    pub outcomes: Vec<Outcome>,
    #[serde(default)]
    pub metrics: Vec<MetricDef>,
    #[serde(default)]
    pub starter_outcomes: Vec<StarterOutcome>,
    #[serde(default)]
    pub audit: KataAuditConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kata_type: String,
    pub description: String,
    #[serde(default)]
    pub editor: String,
    #[serde(default)]
    pub visibility: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KataGasConfig {
    #[serde(default = "default_gas_cap")]
    pub cap: u64,
    #[serde(default = "default_alert_threshold")]
    pub alert_threshold: f64,
    #[serde(default = "default_hard_limit")]
    pub hard_limit: bool,
}

fn default_gas_cap() -> u64 {
    15000
}

fn default_alert_threshold() -> f64 {
    0.7
}
fn default_hard_limit() -> bool {
    true
}

/// A single step in an Improvement Kata cycle.
#[derive(Debug, Clone, Deserialize)]
pub struct KataStep {
    pub ordinal: u32,
    pub action: String,
    pub description: String,
    #[serde(default)]
    pub renderer: Option<String>,
    #[serde(default)]
    pub template_ref: Option<String>,
    #[serde(default)]
    pub classifier: bool,
    #[serde(default)]
    pub gas_cap: Option<u64>,
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub mcp: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
}

/// A single question in a Coaching Kata dialogue.
#[derive(Debug, Clone, Deserialize)]
pub struct CoachQuestion {
    pub number: u32,
    pub question: String,
    pub description: String,
    #[serde(default)]
    pub reg_span: Option<String>,
    #[serde(default)]
    pub expected_output: Option<String>,
}

/// A practice routine in a Starter Kata.
#[derive(Debug, Clone, Deserialize)]
pub struct PracticeRoutine {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub frequency: Option<String>,
    #[serde(default)]
    pub duration_minutes: Option<u32>,
    #[serde(default)]
    pub reg_spans: Vec<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub success_criteria: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorHandling {
    #[serde(default)]
    pub on_gas_exceeded: Option<String>,
    #[serde(default)]
    pub on_timeout: Option<String>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub retry_backoff_seconds: Option<u64>,
}

impl Default for ErrorHandling {
    fn default() -> Self {
        Self {
            on_gas_exceeded: Some("abort".into()),
            on_timeout: Some("retry".into()),
            max_retries: Some(2),
            retry_backoff_seconds: Some(1),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct KataLedgerConfig {
    #[serde(default = "default_true")]
    pub emit_spans: bool,
    pub span_namespace: String,
    #[serde(default = "default_true")]
    pub variety_monitoring: bool,
    #[serde(default)]
    pub algedonic_threshold: Option<u64>,
    #[serde(default)]
    pub escalation_target: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Outcome {
    pub name: String,
    pub condition: String,
    pub action: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub span: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StarterOutcome {
    pub name: String,
    pub condition: String,
    pub action: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KataAuditConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default = "default_true")]
    pub include_input: bool,
    #[serde(default = "default_true")]
    pub include_output: bool,
    #[serde(default = "default_true")]
    pub include_gas_cost: bool,
    #[serde(default = "default_true")]
    pub include_reg_events: bool,
}

impl Default for KataAuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_level: Some("info".into()),
            include_input: true,
            include_output: true,
            include_gas_cost: true,
            include_reg_events: true,
        }
    }
}
