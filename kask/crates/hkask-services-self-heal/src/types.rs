//! Self-healing core types — outcomes, contexts, strategies, actions.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum HealOutcome {
    Healed {
        action_taken: String,
        modifications: Vec<String>,
    },
    Degraded {
        reason: String,
        fallback_description: String,
    },
    Unhealable {
        reason: String,
        suggestion: String,
        requires_code_change: bool,
        debug_log: MiniDebugLog,
    },
}

#[derive(Debug, Clone, Default)]
pub struct HealContext {
    pub operation: String,
    pub error_message: String,
    pub env_vars: HashMap<String, String>,
    pub config_search_paths: Vec<PathBuf>,
    pub can_retry: bool,
}

#[derive(Debug, Clone)]
pub struct HealStrategy {
    pub name: String,
    pub error_pattern: String,
    pub description: String,
    pub action: HealAction,
}

#[derive(Debug, Clone)]
pub enum HealAction {
    RunCommand {
        command: String,
        capture_output: bool,
    },
    SetEnv {
        key: String,
        value_source: EnvValueSource,
    },
    LoadDotEnv {
        search_paths: Vec<PathBuf>,
    },
    CreateDefaultFile {
        path: PathBuf,
        content: String,
    },
    RetryWithBackoff {
        max_attempts: u32,
        delay_ms: u64,
    },
    ProposeCodeChange {
        file: PathBuf,
        description: String,
        diff_suggestion: String,
    },
    Sequence(Vec<HealAction>),
    LlmAssisted {
        template_path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub enum EnvValueSource {
    Literal(String),
    FromFile(PathBuf),
    FromCommand(String),
    FirstOf(Vec<EnvValueSource>),
}

pub type HealInferenceFn =
    Box<dyn Fn(&str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

/// Errors produced by the self-healing engine.
#[derive(Debug, thiserror::Error)]
pub enum HealError {
    #[error("{0}")]
    TemplateRender(String),
    #[error("{0}")]
    ParseResponse(String),
    #[error("{0}")]
    Inference(String),
    #[error("{0}")]
    Command(String),
    #[error("{0}")]
    EnvResolve(String),
    #[error("{0}")]
    Io(String),
    #[error("No inference wired")]
    NoInference,
    #[error("Template not found")]
    TemplateNotFound,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct MiniDebugLog {
    pub attempt_count: u32,
    pub reg_spans: Vec<String>,
    pub modifications: Vec<String>,
    pub actions_taken: Vec<DebugLogAction>,
    pub suggestion: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct DebugLogAction {
    pub name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionResult {
    pub success: bool,
    pub output: String,
    pub modifications: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct HealInstruction {
    pub action: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
}
