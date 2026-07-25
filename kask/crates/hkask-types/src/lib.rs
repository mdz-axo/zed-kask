#![forbid(unsafe_code)]
//! hKask Types — Foundation types for the hKask tool platform
//!

pub mod agent_paths;
pub mod corpus;
pub mod crypto;
pub mod curation;
pub mod curator;
pub mod document;
pub mod error;
pub mod event;
pub mod fusion;
pub mod goal;
pub mod id;
pub mod inference_ipc;

pub mod keychain_keys;
pub mod loops;
pub mod macros;
pub mod observable_span;
pub mod regulation;
pub mod secret;
pub mod server_config;
pub mod skill;
pub mod template;
pub mod template_type;

pub mod ports;
pub mod time;
pub mod tool_taint;
pub mod transcript;
pub mod visibility;
pub mod wallet_types;

#[cfg(feature = "sql")]
pub mod sql_impls;

// ── Essential re-exports (used by ≥3 downstream crates) ─────────────────

pub use crypto::Ed25519PublicKey;
pub use curation::{
    BoundaryClassification, DataCategory, DataSovereigntyBoundary, UserSovereigntyState,
};
pub use curator::{CurationThresholdConfig, CuratorDirective, CuratorHandle, EscalationSeverity};
pub use document::{Block, DocStructure, Page};
pub use error::{
    CapabilityDenied, DatabaseErrorKind, DbError, DbProvider, InfrastructureError, McpErrorKind,
    NotFound,
};
pub use event::{RegulationRecord, RegulationSink};
pub use goal::GoalState;
pub use id::{
    ApiKeyId, BoardId, BotID, ColumnId, CommentId, EmbeddingID, EscalationID, EventID, GoalID,
    HMemId, Id, IdKind, PhaseId, PodID, TaskId, TemplateID, UserID, WalletId, WebID,
};
pub use regulation::CircuitState;

pub use loops::{
    ActionDecision, ActionType, BudgetOption, Deviation, DeviationDirection,
    ExperienceClassification, ImpactReport, LoopId, LoopMetrics, RegulationData, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric, TriggerOrigin,
};
pub use observable_span::ObservableSpan;
pub use skill::SkillPolarity;
pub use template::LLMParameters;
pub use template_type::TemplateType;
pub use tool_taint::ToolTaint;
pub use transcript::{TimedWord, TranscriptBundle, TranscriptSegment};
pub use visibility::{Confidence, Dimension, Visibility};

pub use ports::*;
pub use wallet_types::{
    ApiKeyCapability, ChainId, DepositAddress, DepositReference, Encumbrance, EncumbranceStatus,
    GAS_PER_RJOULE, PrivacyMode, RJoule, RateLimitConfig, TransactionType, WalletBalance,
    WalletConfig, WalletError, WalletTransaction,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HMemEntry {
    pub id: String,
    pub entity: String,
    pub attribute: String,
    pub value: serde_json::Value,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub confidence: f64,
    pub perspective: String,
    pub visibility: String,
    pub dimension: Option<String>,
}

/// A proposal template for a contract missing its user-facing `expect:` annotation.
/// UserPods use this to compose and submit contract grounding proposals.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpectProposal {
    pub crate_name: String,
    pub contract_id: String,
    pub function: String,
    pub file: String,
    pub line: usize,
    pub pre: String,
    pub post: String,
    pub expect_template: String,
    pub suggested_goal_principle: String,
    pub existing_constraining_principles: Vec<String>,
}

pub mod voice;
pub use voice::VoiceDesign;
