//! Request types for hkask-mcp-memory MCP tools.
//!
//! Extracted from main.rs — these are the tool input structs that derive
//! Deserialize + JsonSchema for MCP parameter deserialization.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

// ── Shared request types ───────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreRequest {
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    pub confidence: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallRequest {
    pub entity: String,
}

/// Episodic recall with optional context for salience scoring.
/// Mirrors `ChatService::recall_episodic` — takes user input as context
/// and scores recalled episodes by keyword overlap.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallContextRequest {
    /// Entity to recall episodes for (e.g. "chatted" for chat history).
    pub entity: String,
    /// Text to score salience against. Episodes are ranked by keyword overlap
    /// with this context string.
    pub context: Option<String>,
    /// Maximum number of episodes to return (default: 10).
    pub limit: Option<usize>,
}

/// Paired memory recall — returns both semantic (third-person facts) and
/// episodic (first-person experiences) for an entity in a single call.
/// Episodic results are ranked by salience when context is provided.
/// Mirrors the dual-recall pattern in ChatService::prepare_chat.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PairedRecallRequest {
    /// Entity to recall memory for (e.g. "chatted", "skill:X", "project:Y").
    pub entity: String,
    /// Optional context for episodic salience scoring.
    /// Episodes are ranked by keyword overlap with this string.
    pub context: Option<String>,
    /// Maximum results per recall type (default: 10 each).
    pub limit: Option<usize>,
}

// ── Episodic-specific request types ─────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BudgetRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConsolidateStatusRequest {}

// ── Dispatch request types ──────────────────────────────────────────

/// FlowDef dispatch request — carries memory_type for tool routing.
/// The selector outputs "episodic" or "semantic"; the dispatch tool
/// routes to episodic_store / semantic_store accordingly.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MemoryDispatchRequest {
    pub entity: String,
    pub attribute: String,
    pub value: Value,
    pub confidence: Option<f64>,
    pub memory_type: String,
}

/// FlowDef dispatch request for recall — carries memory_type for routing.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallDispatchRequest {
    pub entity: String,
    pub memory_type: String,
}

// ── Semantic-specific request types ─────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmbedRequest {
    pub entity_ref: String,
    pub vector: Vec<f32>,
    pub model: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query_vector: Vec<f32>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CentroidRequest {
    pub prefix: String,
    pub exclude_prefix: String,
    pub exclude_ref: String,
    pub dim: usize,
    pub store_as: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PurgeRequest {
    pub prefix: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChunkTextRequest {
    pub text: String,
    pub entity_ref_prefix: String,
    pub min_words: Option<usize>,
    pub max_words: Option<usize>,
    pub sentence_boundary: Option<String>,
    pub strip_gutenberg: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CountRequest {}

// ── Backup/restore request types ──────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BackupRequest {
    /// File path for the backup. Defaults to "hkask-memory-backup.db"
    /// if not provided.
    pub target_path: Option<String>,
    /// Optional passphrase for the backup file. If not provided,
    /// the backup is unencrypted (plain SQLite).
    pub passphrase: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestoreRequest {
    /// Path to the backup file to restore from.
    pub source_path: String,
    /// Passphrase for the backup file. Required if the backup was encrypted.
    pub passphrase: Option<String>,
}
