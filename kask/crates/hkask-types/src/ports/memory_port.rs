//! MemoryPort — ingestion boundary for thread-to-memory wiring (D6).
//!
//! When a zed-kask agent thread completes a turn, the conversation is offered
//! to the memory system for episodic + semantic ingestion. This port is the
//! hexagonal boundary: the `agent` crate calls it (via a global hook, same
//! pattern as `set_manifest_executor`), and the bridge provides the
//! implementation.
//!
//! The initial bridge implementation is a logging no-op — the full hKask
//! memory stack (SQLCipher, episodic/semantic storage, consolidation) is
//! deferred until the storage layer and WebID mapping are available in-process.

use std::future::Future;
use std::pin::Pin;

/// A completed turn offered to the memory system for ingestion.
#[derive(Debug, Clone)]
pub struct TurnRecord {
    /// The thread/session identifier (zed's `SessionId` as a string).
    pub thread_id: String,
    /// The user's prompt text for this turn.
    pub user_prompt: String,
    /// The agent's response text for this turn.
    pub agent_response: String,
    /// The model that produced the response (e.g., "claude-sonnet-4-20250514").
    pub model: String,
    /// Optional thread title (if available).
    pub thread_title: Option<String>,
}

/// Error type for memory ingestion failures.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory ingestion failed: {0}")]
    Ingestion(String),
}

/// Pinned boxed future for dyn-compatibility.
pub type MemoryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Port for ingesting completed thread turns into hKask memory (D6).
///
/// The bridge provides the implementation. When no implementation is injected
/// (standalone or first-run), ingestion is a no-op.
pub trait MemoryPort: Send + Sync {
    /// Ingest a completed turn into episodic (and optionally semantic) memory.
    ///
    /// This is fire-and-forget from the caller's perspective — the memory system
    /// handles classification, confidence scoring, and consolidation asynchronously.
    fn ingest_turn<'a>(&'a self, record: TurnRecord) -> MemoryFuture<'a, Result<(), MemoryError>>;
}
