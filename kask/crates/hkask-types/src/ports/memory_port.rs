//! MemoryPort — ingestion boundary for thread-to-memory wiring (D6).
//!
//! When a zed-kask agent thread completes a turn, the conversation is offered
//! to the memory system for ingestion. This port is the hexagonal boundary:
//! the `agent` crate calls it (via a global hook, same pattern as
//! the memory port hook), and the bridge provides the implementation.
//!
//! `TurnRecord` is the ingest-side contract: the completed turn's raw fields.
//! The bridge's write path (2026-09-04 design) cleans and chunks the turn
//! into word-bounded passages, so the h_mem `value` a turn produces is chunk
//! text (with `user:` / `assistant:` role prefixes), not the whole-turn JSON
//! envelope this module's earlier revisions described. The read side (recall)
//! reads `h_mem.value` as a raw JSON value — there is no typed projection
//! struct on the read side.

use std::future::Future;
use std::pin::Pin;

/// A completed turn offered to the memory system for ingestion.
///
/// The bridge's write path cleans this record into role-prefixed text and
/// chunks it into word-bounded passages — one h_mem per chunk under the
/// thread entity (2026-09-04 design; see `kask_bridge/src/memory/ingest.rs`).
#[derive(Debug, Clone)]
pub struct TurnRecord {
    /// The thread/session identifier (zed's `SessionId` as a string).
    /// Maps to the h_mem `entity` field.
    pub thread_id: String,
    /// The user's input text for this turn.
    pub user_input: String,
    /// The agent's response text for this turn.
    pub agent_response: String,
    /// The model that produced the response (e.g., "claude-sonnet-4-20250514").
    pub model: String,
    /// Optional thread title (if available).
    pub thread_title: Option<String>,
    /// The agent ID that produced this turn (e.g., "Curator", "zed"),
    /// when the host runtime tags threads with their owning agent. `None`
    /// for upstream-zed threads that have no agent identity, or when the
    /// caller has no agent-awareness. The memory port uses this to route
    /// ingestion to the correct perspective-scoped store — e.g., Curator
    /// turns are written to the curator's sovereign DB with the curator's
    /// WebID, not the user's.
    pub agent_id: Option<String>,
    /// Goal-tool events observed in this turn (every `kanban_goal_*` tool
    /// result from the last agent message). The goal store is ephemeral
    /// (operator ruling 2026-08-29: zed-agent goals are ephemeral; curator
    /// memory is the durable vehicle) — these events are what the memory
    /// write path turns into first-class goal h_mems, so therapy and
    /// algedonic reviews find goal entities, not prose archaeology.
    pub goal_events: Vec<GoalEvent>,
}

/// A goal-tool event observed in a turn — the durable record of goal
/// activity, extracted from the turn's `kanban_goal_*` tool results.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GoalEvent {
    /// The goal tool that produced this event (e.g. `kanban_goal_create`).
    pub tool_name: String,
    /// The tool result's JSON output (goal text, criteria, verdicts, Brier
    /// scores — the structured record the curator's memory stores).
    pub output: serde_json::Value,
}

/// A recalled memory snippet for context injection.
///
/// Lightweight representation of a stored memory — just enough to format
/// into a prompt.
#[derive(Debug, Clone)]
pub struct MemorySnippet {
    /// The text content of the memory (e.g., a chat turn, a fact, a summary).
    pub text: String,
    /// The entity key this memory was stored under (e.g. `chat:thread:{id}`).
    /// Used by the context injector to record co-occurrence links between
    /// entities recalled in the same context — the `connectedness` signal.
    pub entity: String,
    /// The memory's confidence score (0.0–1.0), decayed by time since recall.
    pub confidence: f64,
    /// Relevance score to the query (0.0–1.0), computed by the recall method.
    pub relevance_score: f64,
}

/// Error type for memory operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory ingestion failed: {0}")]
    Ingestion(String),
    #[error("memory recall failed: {0}")]
    Recall(String),
}

/// Pinned boxed future for dyn-compatibility.
pub(crate) type MemoryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port for ingesting completed thread turns into hKask memory (D6).
///
/// The bridge provides the implementation. When no implementation is injected
/// (standalone or first-run), ingestion is a no-op.
///
/// The ingestion pattern mirrors hKask's `DaemonHandler::store_experience`:
/// - The write path cleans and chunks the turn into word-bounded passages,
///   stored as shared h_mems under `curator:thread:{thread_id}` — one copy
///   per turn (2026-09-04 single-copy ruling), each chunk embedded with its
///   passage text and ontologically tagged
/// - Confidence: every write enters at the 0.5 floor
pub trait MemoryPort: Send + Sync {
    /// Ingest a completed turn into memory.
    ///
    /// This is fire-and-forget from the caller's perspective — the memory system
    /// handles classification, confidence scoring, and consolidation asynchronously.
    fn ingest_turn<'a>(&'a self, record: TurnRecord) -> MemoryFuture<'a, Result<(), MemoryError>>;

    /// Recall memory snippets relevant to a query for context injection.
    ///
    /// The implementation should:
    /// 1. Embed the query and search by embedding similarity (KNN)
    /// 2. Query by entity/keyword overlap
    /// 3. Merge, dedup, and score results by relevance × confidence
    /// 4. Return up to `limit` snippets, sorted by score descending
    ///
    /// The default implementation returns an empty vec — graceful degradation
    /// when no memory store is configured.
    fn recall_context<'a>(
        &'a self,
        _query: &'a str,
        _limit: usize,
    ) -> MemoryFuture<'a, Result<Vec<MemorySnippet>, MemoryError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    /// Recall all memory snippets associated with a specific thread.
    ///
    /// Unlike `recall_context` (which recalls by content similarity / keyword
    /// overlap), this recalls by exact entity match — returning every h_mem
    /// stored under the thread's entity. Used by the context injector's
    /// `inject_context` to load a thread's prior turns per turn (fresh, not
    /// session-cached).
    ///
    /// The default implementation returns an empty vec — graceful degradation
    /// when no memory store is configured.
    fn recall_thread<'a>(
        &'a self,
        _thread_id: &'a str,
        _limit: usize,
    ) -> MemoryFuture<'a, Result<Vec<MemorySnippet>, MemoryError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
