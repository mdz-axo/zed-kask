//! Kask-specific per-thread state, grouped so upstream `Thread`'s struct
//! definition stays clean and upstream rebases touch one field instead of
//! nine.
//!
//! Methods on this struct encapsulate the kask-specific turn-loop behaviors
//! that were previously inline in `Thread`'s methods. See
//! git history (`plans/thread-hooks-refactor-2026-08-26.md`, deleted after
//! implementation) for the full plan.
//!
//! ## D-seam mapping
//!
//! | Field | D-seam | Purpose |
//! |-------|--------|---------|
//! | `agent_id` | D6 | Memory ingestion routing (Curator vs user) |
//! | `agent_static_context` | D2 | Curator overlay / Steer mode system prompt |
//! | `system_prompt_override` | D2 | System prompt override (Curator persona) |
//! | `mcp_server_scope` | D2 | Per-tab MCP server scoping |
//! | `tool_retry_tracker` | .rules | Tool retry death spiral prevention |
//! | `deferred_tool_results` | — | Deferred tool result delivery across turn boundaries |
//! | `last_completion_truncated` | D25 | Distinguish MaxTokens truncation from user cancel |
//! | `cached_system_prompt` | — | System prompt digest caching |
//! | `cached_filtered_context` | — | Filtered context caching |

use std::cell::RefCell;
use std::rc::Rc;

use gpui::SharedString;
use project::AgentId;

use crate::thread::{CachedFilteredContext, CachedSystemPrompt, DeferredToolResult};
use crate::tool_retry_tracker::ToolRetryTracker;

/// All kask-specific per-thread state. Created with `new()` (all defaults)
/// for both upstream Zed and kask threads. Kask-specific setters
/// (`set_agent_id`, `set_static_context`, etc.) are called by
/// `NativeAgent::new_session` after construction.
pub(crate) struct KaskThreadState {
    // Identity (D6)
    agent_id: Option<AgentId>,

    // System prompt overlays (D2)
    agent_static_context: Option<SharedString>,
    system_prompt_override: Option<SharedString>,
    mcp_server_scope: Option<SharedString>,

    // Tool retry cap (.rules)
    tool_retry_tracker: Rc<RefCell<ToolRetryTracker>>,

    // Deferred tool results
    deferred_tool_results: Vec<DeferredToolResult>,

    // Truncation detection (D25)
    last_completion_truncated: bool,

    // Caching
    cached_system_prompt: Option<CachedSystemPrompt>,
    cached_filtered_context: Option<CachedFilteredContext>,
}

impl KaskThreadState {
    pub fn new() -> Self {
        Self {
            agent_id: None,
            agent_static_context: None,
            system_prompt_override: None,
            mcp_server_scope: None,
            tool_retry_tracker: Rc::new(RefCell::new(ToolRetryTracker::default())),
            deferred_tool_results: Vec::new(),
            last_completion_truncated: false,
            cached_system_prompt: None,
            cached_filtered_context: None,
        }
    }

    // ── Truncation detection (D25) ────────────────────────────────────

    /// Called when a completion stops with `StopReason::MaxTokens`,
    /// indicating the model's output was truncated before it finished.
    pub fn on_max_tokens(&mut self) {
        self.last_completion_truncated = true;
    }

    /// Whether the last completion was truncated. Read by
    /// `flush_pending_message` to distinguish stream-truncated tool calls
    /// from genuine user cancellations.
    pub fn last_completion_truncated(&self) -> bool {
        self.last_completion_truncated
    }

    /// Reset the truncation flag. Called at the start of each completion
    /// request so the flag reflects only the most recent completion.
    pub fn reset_truncation_flag(&mut self) {
        self.last_completion_truncated = false;
    }

    /// Warning for a turn that ends with `StopReason::MaxTokens` (D43).
    /// Upstream sends the stop reason with no operator-visible signal, so
    /// threads appear to "just stop" mid-generation. The message
    /// distinguishes the zero-content signature (prompt → silence →
    /// nothing — the operator sees a dead thread) from partial-content
    /// truncation, and names the model so the log line is actionable
    /// without cross-referencing the thread.
    pub(crate) fn max_tokens_turn_end_warning(model_name: &str, produced_content: bool) -> String {
        if produced_content {
            format!(
                "Turn ended at the model's token limit (StopReason::MaxTokens) after \
                 producing partial content (model: {model_name}). The turn stopped \
                 mid-generation with no error shown to the operator."
            )
        } else {
            format!(
                "Turn ended at the model's token limit (StopReason::MaxTokens) with NO \
                 content (model: {model_name}) — the silent-stop signature: prompt → \
                 silence → nothing. Check the provider's reasoning/output token budget."
            )
        }
    }

    // ── System prompt caching ─────────────────────────────────────────

    /// Get the cached system prompt if the digest matches.
    pub fn cached_system_prompt(&self, digest: &[u8; 32]) -> Option<SharedString> {
        self.cached_system_prompt
            .as_ref()
            .filter(|c| c.digest == *digest)
            .map(|c| c.prompt.clone())
    }

    /// Store a rendered system prompt with its digest.
    pub fn cache_system_prompt(&mut self, digest: [u8; 32], prompt: SharedString) {
        self.cached_system_prompt = Some(CachedSystemPrompt { digest, prompt });
    }

    /// Bust the system prompt cache. Called when `static_context` or
    /// `system_prompt_override` changes.
    pub fn bust_system_prompt_cache(&mut self) {
        self.cached_system_prompt = None;
    }

    // ── Filtered context caching ─────────────────────────────────────

    /// Get the cached filtered context if the digest matches.
    pub fn cached_filtered_context(
        &self,
        digest: &[u8; 32],
    ) -> Option<&prompt_store::ProjectContext> {
        self.cached_filtered_context
            .as_ref()
            .filter(|c| c.filter_digest == *digest)
            .map(|c| &c.context)
    }

    /// Store a filtered context with its digest.
    pub fn cache_filtered_context(
        &mut self,
        digest: [u8; 32],
        context: prompt_store::ProjectContext,
    ) {
        self.cached_filtered_context = Some(CachedFilteredContext {
            filter_digest: digest,
            context,
        });
    }

    /// Whether a filtered context cache entry exists. For test assertions.
    #[cfg(test)]
    pub fn has_cached_filtered_context(&self) -> bool {
        self.cached_filtered_context.is_some()
    }

    // ── System prompt override (D2) ──────────────────────────────────

    /// System prompt override — when set, returned directly instead of
    /// rendering the template. Used by the Curator agent to inject its
    /// own persona.
    pub fn system_prompt_override(&self) -> Option<&SharedString> {
        self.system_prompt_override.as_ref()
    }

    /// Set the system prompt override. Busts the system prompt cache.
    pub fn set_system_prompt_override(&mut self, prompt: SharedString) {
        self.system_prompt_override = Some(prompt);
        self.bust_system_prompt_cache();
    }

    // ── Static context (D2) ──────────────────────────────────────────

    /// Static context rendered in the system prompt's Session Context
    /// section (e.g., Curator overlay, Steer panel overlay).
    pub fn static_context(&self) -> Option<&SharedString> {
        self.agent_static_context.as_ref()
    }

    /// Set the static context. Busts the system prompt cache.
    pub fn set_static_context(&mut self, context: SharedString) {
        self.agent_static_context = Some(context);
        self.bust_system_prompt_cache();
    }

    // ── MCP server scoping (D2) ──────────────────────────────────────

    /// When set, `enabled_tools` filters MCP tools to only this server.
    pub fn mcp_server_scope(&self) -> Option<&SharedString> {
        self.mcp_server_scope.as_ref()
    }

    /// Set the MCP server scope.
    pub fn set_mcp_server_scope(&mut self, scope: Option<SharedString>) {
        self.mcp_server_scope = scope;
    }

    /// Whether a context-server id passes the per-tab MCP scope.
    /// `None` (upstream Zed and non-kask threads) passes every server.
    pub fn mcp_server_in_scope(&self, server_id: &str) -> bool {
        self.mcp_server_scope
            .as_ref()
            .is_none_or(|s| s.as_ref() == server_id)
    }

    // ── Agent identity (D6) ──────────────────────────────────────────

    /// The agent ID that owns this thread (D6 routing key).
    pub fn agent_id(&self) -> Option<&AgentId> {
        self.agent_id.as_ref()
    }

    /// Set the agent ID.
    pub fn set_agent_id(&mut self, agent_id: AgentId) {
        self.agent_id = Some(agent_id);
    }

    // ── Tool retry cap (.rules) ──────────────────────────────────────

    /// Get a handle to the retry tracker for cloning into async blocks.
    /// The `Rc<RefCell<>>` is cloned and moved into the spawned task,
    /// which is safe because `run_tool` spawns on the foreground executor.
    pub fn retry_tracker_handle(&self) -> Rc<RefCell<ToolRetryTracker>> {
        self.tool_retry_tracker.clone()
    }

    /// Check the tool retry cap before running a tool.
    pub fn check_tool_retry(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> crate::tool_retry_tracker::RetryVerdict {
        self.tool_retry_tracker.borrow().check(tool_name, input)
    }

    /// Record a tool failure. Test seam — production records via the
    /// `retry_tracker` directly in `Thread::run_turn`.
    #[cfg(test)]
    pub fn record_tool_failure(&self, tool_name: &str, input: &serde_json::Value) {
        self.tool_retry_tracker
            .borrow()
            .record_failure(tool_name, input);
    }

    /// Record a tool success. Test seam — see `record_tool_failure`.
    #[cfg(test)]
    pub fn record_tool_success(&self, tool_name: &str, input: &serde_json::Value) {
        self.tool_retry_tracker
            .borrow()
            .record_success(tool_name, input);
    }

    // ── Deferred tool results ────────────────────────────────────────

    /// Enqueue a deferred tool result.
    pub fn enqueue_deferred_result(&mut self, result: DeferredToolResult) {
        self.deferred_tool_results.push(result);
    }

    /// Drain completed deferred results.
    pub fn drain_completed_deferred_results(
        &mut self,
    ) -> Vec<crate::thread::CompletedDeferredResult> {
        crate::thread::drain_completed_deferred_results(&mut self.deferred_tool_results)
    }

    /// Clear all deferred results (on cancel).
    pub fn clear_deferred_results(&mut self) {
        self.deferred_tool_results.clear();
    }

    /// Number of pending deferred results. For test assertions.
    pub fn deferred_result_count(&self) -> usize {
        self.deferred_tool_results.len()
    }

    // ── Subagent inheritance (D6/D34) ────────────────────────────────

    /// Inherit state from a parent thread's KaskThreadState (for subagents).
    /// Only `agent_id` is inherited — curator-spawned subagents route their
    /// turns to the curator's sovereign DB. All other state starts fresh.
    pub fn inherit_from(parent: &KaskThreadState) -> Self {
        let mut state = Self::new();
        state.agent_id = parent.agent_id.clone();
        state
    }
}

impl Default for KaskThreadState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::KaskThreadState;

    #[test]
    fn max_tokens_warning_names_the_stop_reason_and_content_state() {
        // D43: the warn must carry the stop reason, the model, and the
        // zero-content distinction — the fields that made the 2026-08-30
        // silent-turn-stop incident diagnosable only by inference.
        let zero_content = KaskThreadState::max_tokens_turn_end_warning("GLM 5.3", false);
        assert!(zero_content.contains("StopReason::MaxTokens"));
        assert!(zero_content.contains("GLM 5.3"));
        assert!(zero_content.contains("NO"));
        assert!(zero_content.contains("silent-stop"));

        let partial = KaskThreadState::max_tokens_turn_end_warning("GLM 5.3", true);
        assert!(partial.contains("StopReason::MaxTokens"));
        assert!(partial.contains("partial content"));
        assert!(!partial.contains("silent-stop"));
    }
}
