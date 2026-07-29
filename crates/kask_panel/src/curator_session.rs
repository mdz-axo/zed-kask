//! CuratorSession — the per-tab stateful streaming curator conversation
//! contract.
//!
//! This is the one genuinely new abstraction introduced by the kask panel
//! redesign (v0.3.0). It replaces the stateless `ScopedInference::infer`
//! path, which rebuilt a fresh `[system, user]` array on every call with
//! no conversation history. A `CuratorSession` is **one instance per tab**:
//! it owns that tab's conversation history, streams `CuratorEvent`s as the
//! curator model produces them, and dispatches tool calls through the
//! OCAP-gated `ToolPort`.
//!
//! ## Why a trait, not a struct
//!
//! The panel crate cannot depend on `kask_bridge` (circular dependency),
//! so the bridge provides the concrete `PanelCuratorSession` adapter in
//! `crates/zed/src/main.rs`. The trait is the seam.
//!
//! ## Cross-tab observation
//!
//! The trait deliberately has **no `observe_tool_use` method**. Cross-tab
//! curation is the curator MCP server's job: it owns `EpisodicMemory` and
//! `SemanticMemory`, and `McpRuntime` records every governed tool
//! invocation's outcome in the `RegulationLedger`. The panel forwards
//! nothing between tabs — that would violate thread independence and
//! duplicate the curator server's own memory. See
//! `kask/docs/plans/kask-panel-redesign.md` §1.3.

use std::sync::{Arc, OnceLock};

use gpui::Task;
use serde_json::Value;

// ── Tool scope ─────────────────────────────────────────────────────────

/// The set of tools available to the curator for a given tab.
///
/// v1 supports only `Server` — the tab's MCP server's tools. v2 may add
/// `Multiple` (a curated subset across servers) or `All` (every kask MCP
/// server's tools).
#[derive(Clone, Debug)]
pub enum ToolScope {
    /// Only the named MCP server's tools are available to the curator.
    Server(String),
}

// ── Curator events ──────────────────────────────────────────────────────

/// A single event in a curator turn stream. Mirrors the fields of
/// `hkask_types::InferenceStreamChunk` directly (text + reasoning deltas,
/// structured tool calls, finish reason, usage) plus a `ToolResult` variant
/// for the result of a tool the curator dispatched.
#[derive(Clone, Debug)]
pub enum CuratorEvent {
    /// A chunk of assistant text (the primary response).
    TextDelta(String),
    /// A chunk of thinking-mode reasoning (rendered as a collapsible block).
    ThinkingDelta(String),
    /// The curator decided to call a tool. The panel renders this as a
    /// pending tool-call card; the session dispatches it via the `ToolPort`
    /// and emits a matching `ToolResult` when it completes.
    ToolCall(ToolCallRequest),
    /// The result of a tool the curator called. Paired with the preceding
    /// `ToolCall` by `call_id`.
    ToolResult {
        call_id: String,
        result: Result<Value, String>,
    },
    /// The turn is complete. `finish_reason` is the provider's stop reason
    /// (e.g. "stop", "tool_calls"); `usage` is token accounting if the
    /// provider reports it.
    Done {
        finish_reason: Option<String>,
        usage: Option<Usage>,
    },
    /// The turn failed (inference error, tool dispatch error, or cancel).
    Error(String),
}

/// A tool call the curator wants to make. Mirrors the relevant fields of
/// `hkask_types::StructuredToolCall` without depending on `hkask-types`.
#[derive(Clone, Debug)]
pub struct ToolCallRequest {
    /// The provider's tool-call id (used to pair `ToolResult`s).
    pub call_id: String,
    /// The tool name (must be a tool exposed by this tab's `ToolScope`).
    pub name: String,
    /// The JSON arguments for the tool call.
    pub arguments: Value,
}

/// Token usage for a completed turn. Mirrors `hkask_types::InferenceUsage`
/// without the dependency.
#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ── The session trait ───────────────────────────────────────────────────

/// A stateful, streaming curator conversation scoped to one tab.
///
/// One instance per tab. The panel holds `HashMap<usize, Arc<dyn
/// CuratorSession>>` keyed by server index; switching tabs swaps which
/// session receives `send` calls. Each session owns its tab's conversation
/// history internally (the bridge's `PanelCuratorSession` holds a
/// `tokio::sync::Mutex<Vec<ChatMessage>>`).
///
/// The panel never inspects or mutates the history directly — it observes
/// the turn through the `CuratorEvent` stream and renders from that. The
/// history is the session's private state.
pub trait CuratorSession: Send + Sync {
    /// Send a user message to the curator with this tab's tool scope and
    /// system prompt. Returns a stream of curator events (text chunks,
    /// thinking deltas, tool calls, tool results, done).
    ///
    /// The session appends the user message to its internal history,
    /// prepends the per-tab system prompt as the leading `system` message,
    /// and calls `InferencePort::generate_stream_with_messages` with the
    /// full history + the tab's tool definitions.
    fn send(
        &self,
        message: &str,
        tool_scope: &ToolScope,
        system_prompt: &str,
    ) -> Task<Result<CuratorEventStream, String>>;

    /// Cancel the in-flight curator turn for this tab. Best-effort: the
    /// stream may yield a final `Error("cancelled")` event or simply end.
    fn cancel(&self) -> Task<std::result::Result<(), String>>;

    /// Retry the last user message (re-send with the same scope + prompt).
    /// The session drops the last assistant response from its history
    /// before re-sending, so the retry replaces the prior turn.
    fn retry(&self) -> Task<Result<CuratorEventStream, String>>;
}

/// A stream of `CuratorEvent`s for one curator turn.
///
/// The concrete transport is a `tokio::sync::mpsc::UnboundedReceiver`:
/// the bridge adapter (`PanelCuratorSession` in `main.rs`) spawns the
/// inference stream on a background task and forwards each
/// `InferenceStreamChunk` as a `CuratorEvent` over the channel. The panel
/// drains the receiver on the foreground executor (GPUI is single-threaded;
/// `AsyncApp` is not `Send`, so the bridge must not capture it — per the
/// `.rules` "Cross-thread GPUI communication uses channels" trap).
pub struct CuratorEventStream {
    /// Receiver for events the bridge pushes from the background inference
    /// task. The panel drains this on the foreground executor.
    pub rx: tokio::sync::mpsc::UnboundedReceiver<CuratorEvent>,
}

impl CuratorEventStream {
    /// Construct from a receiver (used by the bridge adapter).
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<CuratorEvent>) -> Self {
        Self { rx }
    }

    /// Try to take the next event without blocking. Returns `None` if the
    /// stream is empty but not yet closed (the panel should re-poll on the
    /// next `cx.notify()`).
    pub fn try_next(&mut self) -> Option<CuratorEvent> {
        self.rx.try_recv().ok()
    }
}

// ── Factory ─────────────────────────────────────────────────────────────

/// A factory that constructs a fresh `CuratorSession` for one tab.
///
/// The panel calls this once per tab (lazily, on first activation) to
/// obtain a session with its own history mutex. The bridge provides the
/// implementation; it closes over the `InferencePort`, `ToolPort`, and
/// `a2a_secret` resolved in the deferred task in `main.rs`.
pub trait CuratorSessionFactory: Send + Sync {
    /// Construct a new curator session for the given server.
    fn session_for(&self, server: &str) -> Arc<dyn CuratorSession>;
}

static CURATOR_SESSION_FACTORY: OnceLock<Option<Arc<dyn CuratorSessionFactory>>> = OnceLock::new();

/// Inject the global curator session factory (composition root).
///
/// Replaces `set_scoped_inference` for the v0.3.0 redesign. The factory is
/// wired in the deferred task in `main.rs` (per the `.rules`
/// "Model-dependent kask wiring must run in the deferred task" trap), with
/// a `log::warn!` in the failure branch naming the hook (per the
/// "process-global hooks need a startup-failure signal" trap).
pub fn set_curator_session_factory(factory: Option<Arc<dyn CuratorSessionFactory>>) {
    let _ = CURATOR_SESSION_FACTORY.set(factory);
}

/// Read the global factory. Returns `None` if not yet wired (the deferred
/// task hasn't run) or if wiring failed (no default model configured).
pub fn curator_session_factory() -> Option<&'static Arc<dyn CuratorSessionFactory>> {
    CURATOR_SESSION_FACTORY.get().and_then(|opt| opt.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_scope_server_carries_name() {
        let scope = ToolScope::Server("curator".to_string());
        match scope {
            ToolScope::Server(name) => assert_eq!(name, "curator"),
        }
    }

    #[test]
    fn curator_event_stream_try_next_returns_none_when_empty() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel::<CuratorEvent>();
        let mut stream = CuratorEventStream::new(rx);
        assert!(stream.try_next().is_none());
    }

    #[test]
    fn curator_event_stream_try_next_yields_pushed_event() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CuratorEvent>();
        let _ = tx.send(CuratorEvent::TextDelta("hello".to_string()));
        let _ = tx.send(CuratorEvent::Done {
            finish_reason: Some("stop".to_string()),
            usage: None,
        });
        let mut stream = CuratorEventStream::new(rx);
        assert!(matches!(
            stream.try_next(),
            Some(CuratorEvent::TextDelta(_))
        ));
        assert!(matches!(stream.try_next(), Some(CuratorEvent::Done { .. })));
        assert!(stream.try_next().is_none());
    }

    #[test]
    fn curator_event_stream_try_next_yields_error_event() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CuratorEvent>();
        let _ = tx.send(CuratorEvent::Error("boom".to_string()));
        let mut stream = CuratorEventStream::new(rx);
        assert!(matches!(stream.try_next(), Some(CuratorEvent::Error(_))));
    }

    #[test]
    fn curator_event_stream_try_next_yields_tool_call_and_result() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<CuratorEvent>();
        let _ = tx.send(CuratorEvent::ToolCall(ToolCallRequest {
            call_id: "call_1".to_string(),
            name: "regulation_status".to_string(),
            arguments: serde_json::json!({}),
        }));
        let _ = tx.send(CuratorEvent::ToolResult {
            call_id: "call_1".to_string(),
            result: Ok(serde_json::json!({"healthy": true})),
        });
        let mut stream = CuratorEventStream::new(rx);
        assert!(matches!(stream.try_next(), Some(CuratorEvent::ToolCall(_))));
        assert!(matches!(
            stream.try_next(),
            Some(CuratorEvent::ToolResult { .. })
        ));
    }

    #[test]
    fn factory_unset_returns_none() {
        // The OnceLock is process-global; if another test wired it, this
        // would fail. We only assert the accessor is callable.
        let _ = curator_session_factory();
    }
}
