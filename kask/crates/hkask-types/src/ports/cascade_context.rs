//! CascadeContextProvider — participant-based memory + short-term context
//! gathering for skill manifest cascades.
//!
//! When a skill cascade is invoked (via `SkillTool` or slash-command), the
//! `ManifestExecutor` runs Jinja2 templates against an LLM. Without this
//! port, each template step is submitted as an isolated single-prompt call
//! with no conversational context and no long-term memory — the model sees
//! only the rendered template, not the thread it was invoked from.
//!
//! This port closes that gap by gathering two kinds of context:
//!
//! 1. **Short-term**: the last N turns from the invoking thread, role-tagged
//!    as `ChatMessage`s. These are snapshotted at the invocation site and
//!    passed through to `execute_select`, which prepends them to the
//!    inference call as proper role-tagged messages (not a flattened string).
//!
//! 2. **Long-term**: salient memory chunks recalled from the memory stores
//!    of the participants present in the thread. "Participants" is
//!    determined by the thread's `agent_id` and optional `swarm_id`:
//!
//!    | agent_id          | swarm_id | Recall sources          |
//!    |-------------------|----------|-------------------------|
//!    | ZED_AGENT_ID      | absent   | User store              |
//!    | CURATOR_AGENT_ID  | absent   | Curator + User stores   |
//!    | ZED_AGENT_ID      | present  | Swarm store             |
//!    | CURATOR_AGENT_ID  | present  | Curator + Swarm stores  |
//!
//!    Joint recall merges chunks from all sources into a single ranked
//!    list, filtered by a saliency floor and truncated to a max-chunks cap.
//!    The saliency query is the concatenation of `task` + the recent N
//!    turns — the "chat context" that memory chunks should be salient to.
//!
//! Memory is an autonomous feature of processed experiences. It is NOT a
//! consent-gated feature: when participants are present in a thread, their
//! memory stores are read by default. This mirrors the chat path's
//! `ContextInjector`, which recalls memory automatically on every turn.
//!
//! This port is distinct from `prior_outcomes` (intra-cascade step results
//! used for Brier scoring and analytical training). Memory context is fuzzy,
//! unstructured, and connects reasoning graphs; prior outcomes are structured
//! analytical signals. Do not conflate them.

use std::future::Future;
use std::pin::Pin;

use crate::ports::inference_types::ChatMessage;
use crate::ports::memory_port::MemorySnippet;

/// A request for cascade context gathering.
///
/// Built at the skill invocation site (`SkillTool::run` or
/// `send_skill_invocation`) from the thread's `agent_id`, optional
/// `swarm_id` (from `SkillToolInput.context`), the user's `task`, and the
/// snapshot of recent turns. The provider does NOT re-fetch the short-term
/// messages — it only adds long-term memory.
#[derive(Debug, Clone)]
pub struct CascadeContextRequest {
    /// The thread/session identifier (for thread-scoped recall, if needed).
    pub thread_id: String,
    /// The user's task text — the primary query for memory recall.
    pub task: String,
    /// The owning agent of the invoking thread (`ZED_AGENT_ID` or
    /// `CURATOR_AGENT_ID`). Determines which memory stores to recall from.
    /// `None` for upstream-zed threads with no agent identity — treated as
    /// user-only recall.
    pub agent_id: Option<String>,
    /// The swarm ID, when the skill is invoked in a swarm context (from
    /// `SkillToolInput.context["swarm_id"]`). When present, the swarm
    /// memory store is included in recall.
    pub swarm_id: Option<String>,
    /// The short-term message window already snapshot from the thread.
    /// Passed through to the cascade as-is; the provider does not modify it.
    pub short_term_messages: Vec<ChatMessage>,
    /// Saliency floor: a memory chunk is injected only if
    /// `relevance_score * confidence >= saliency_floor`.
    pub saliency_floor: f64,
    /// Maximum number of memory chunks to inject, after merging across all
    /// recall sources.
    pub max_chunks: u32,
}

/// The gathered context for a skill cascade invocation.
///
/// Carries both short-term (thread) and long-term (memory) context. The
/// `ManifestExecutor` injects these into the `StepContext` as template
/// fields (`session_history`, `memory_context`) AND threads them through
/// `Infra` to `execute_select`, which prepends them to the inference call
/// as proper role-tagged messages.
#[derive(Debug, Clone, Default)]
pub struct CascadeContext {
    /// Prior turns from the invoking thread, role-tagged. Empty when
    /// invoked outside a thread (e.g., CLI) or when
    /// `cascade_short_term_turns` is 0.
    pub short_term_messages: Vec<ChatMessage>,
    /// Salient long-term memory snippets, merged and ranked across all
    /// participant stores. Empty when no stores are available or no chunks
    /// exceed the saliency floor.
    pub long_term_snippets: Vec<MemorySnippet>,
}

/// Hexagonal port for gathering cascade context.
///
/// The bridge provides the implementation (`BridgeCascadeContextProvider`),
/// which holds an `Arc<RealMemoryPort>` and applies the participant matrix
/// to select recall sources. The `agent` crate calls this via a global hook
/// (same pattern as `set_context_injector` / `set_manifest_executor`).
pub trait CascadeContextProvider: Send + Sync {
    /// Gather short-term + long-term context for a skill cascade.
    ///
    /// The short-term messages in the request are passed through unchanged.
    /// The long-term snippets are recalled from the participant stores,
    /// merged, deduped, ranked by `relevance_score * confidence`, filtered
    /// by the saliency floor, and truncated to `max_chunks`.
    fn gather_context<'a>(
        &'a self,
        request: &'a CascadeContextRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CascadeContext, CascadeContextError>> + Send + 'a>>;
}

/// Error type for cascade context gathering.
#[derive(Debug, thiserror::Error)]
pub enum CascadeContextError {
    #[error("memory recall failed: {0}")]
    Recall(String),
    #[error("cascade context provider not wired")]
    NotWired,
}
