//! Governed widget→MCP dispatch, in a leaf crate.
//!
//! This crate holds two things:
//!
//! 1. The [`ToolInvoker`] trait + its process-global accessor
//!    ([`set_tool_invoker`] / [`shared_tool_invoker`]), relocated verbatim from
//!    `crates/swarm_panel/src/tool_invoker.rs`. The production impl
//!    (`PanelToolInvoker`) lives in `crates/zed/src/main.rs` and delegates to
//!    `McpRuntime` (which implements `ToolPort`), so every call flows through
//!    the same metered, call-capped path as agent-initiated tool
//!    calls. Relocating the trait to a leaf crate lets the kask GPUI widget
//!    crates dispatch MCP tools without depending on the heavy `swarm_panel`
//!    crate (which would invert sane layering: leaf widgets → heavy panel).
//!
//!    `swarm_panel` re-exports these symbols so its existing call sites compile
//!    unchanged. The `McpRuntime`/`ToolPort`/token minting stays in `main.rs`
//!    (it needs `hkask-tool-port` + `hkask-mcp`); only the trait + the global
//!    accessor live here.
//!
//! 2. [`BlockProvenance`] — the payload a rendered widget block carries so the
//!    widget can re-issue the *originating* MCP tool with modified args. Without
//!    it, a widget can display an artifact but cannot iterate on it; the user
//!    must re-ask the agent. Provenance is `#[serde(default)]` and additive on
//!    each block body, so existing tolerant parsers are unaffected.
//!
//! ## Why a leaf crate
//!
//! `hkask-viz-core` depends one-way on the widget crates
//! (`hkask-viz-core/Cargo.toml`). Widget crates do not depend on `viz-core`.
//! A widget that wants to dispatch therefore cannot reach `ToolInvoker` via
//! `viz-core`, and depending on `swarm_panel` would pull `agent`, `editor`,
//! `project`, … into a leaf widget. This crate is the minimal shared seam: it
//! depends only on `gpui` (`Task`), `serde`, and `serde_json`.

use std::sync::Arc;
use std::time::Instant;

use gpui::Task;
use serde::Deserialize;
use serde_json::Value;

/// Why a UI-initiated tool call failed, at the granularity a panel needs to
/// decide what to do next.
///
/// The seam previously carried a bare `String`, which forced every panel to
/// either treat all failures as terminal or substring-match the message. Both
/// were wrong in the same direction: a transient MCP transport loss (server
/// restarting after a settings change, child process replaced) was rendered as a
/// permanent error, and the panel never re-fetched. Classifying here lets a panel
/// retry exactly the failures a retry can fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// No invoker is wired yet (pre-login, or MCP servers disabled). Not a
    /// failure of the request — the dispatch path does not exist yet.
    NotWired,
    /// The server could not be reached and the request provably never left:
    /// no live connection, or it could not be re-started. Retrying is safe.
    Unavailable(String),
    /// The request was delivered and the connection dropped before a response
    /// arrived. **The operation may or may not have taken effect.**
    ///
    /// Never retried automatically — doing so could duplicate a side effect
    /// (two tasks created, a hire charged twice). A panel should refresh its
    /// view so the operator can see the true state and decide.
    Interrupted(String),
    /// The call reached the tool and failed there, or was refused before
    /// dispatch (call cap, unknown tool). Retrying repeats the same outcome.
    Failed(String),
}

impl InvokeError {
    /// Whether re-issuing the identical call is both plausibly useful and free
    /// of duplicate-side-effect risk.
    ///
    /// [`InvokeError::NotWired`] is retryable because wiring happens
    /// asynchronously at startup: a panel constructed before the deferred
    /// post-login task runs will find an invoker moments later.
    /// [`InvokeError::Interrupted`] is excluded — its outcome is unknown.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, InvokeError::NotWired | InvokeError::Unavailable(_))
    }

    /// Whether the operation's outcome is unknown, so the caller must re-read
    /// state rather than assume success or failure.
    #[must_use]
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, InvokeError::Interrupted(_))
    }

    /// The operator-facing message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            InvokeError::NotWired => NOT_WIRED_MESSAGE.to_string(),
            InvokeError::Unavailable(detail)
            | InvokeError::Interrupted(detail)
            | InvokeError::Failed(detail) => detail.clone(),
        }
    }
}

/// The message shown when no invoker is wired. Shared so every panel explains
/// the same condition the same way.
pub const NOT_WIRED_MESSAGE: &str = "The kask MCP servers are not connected yet. If this persists, ensure they are enabled \
     (kask.mcp.load_default).";

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// Invoke MCP tools directly from UI surfaces (panels, widgets), bypassing the
/// agent conversation loop. The implementation (in `main.rs` as
/// `PanelToolInvoker`) delegates to `McpRuntime` (which implements `ToolPort`),
/// so every call flows through the same governed, gas-budgeted path as
/// agent-initiated tool calls.
pub trait ToolInvoker: Send + Sync {
    /// Invoke a tool on a specific MCP server. Returns the result as JSON text.
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Task<Result<String, InvokeError>>;
}

static TOOL_INVOKER: std::sync::Mutex<Option<Arc<dyn ToolInvoker>>> = std::sync::Mutex::new(None);

/// Inject the global tool invoker (composition root).
///
/// Called from `main.rs` after the deferred task resolves the bridge ports.
/// Re-settable — later calls replace the earlier invoker.
pub fn set_tool_invoker(invoker: Option<Arc<dyn ToolInvoker>>) {
    *TOOL_INVOKER.lock().expect("TOOL_INVOKER poisoned") = invoker;
}

/// Access the global tool invoker.
///
/// Returns `None` when the invoker has not been wired (e.g. before the deferred
/// post-login task runs, or when the MCP runtime is unavailable). Callers MUST
/// surface this as a visible error rather than silently no-op'ing — see the
/// `.rules` "Process-global hooks set at runtime need a startup-failure signal"
/// trap.
pub fn shared_tool_invoker() -> Option<Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.lock().expect("TOOL_INVOKER poisoned").clone()
}

/// Provenance for a rendered widget block: which MCP tool produced this
/// artifact, with which args, and under which regulation span.
///
/// A widget carries this so it can re-issue the originating tool with modified
/// args — letting the user iterate on the displayed artifact (e.g. scrub a
/// portfolio date range, override a scenario probability, move a kanban task)
/// without re-explaining the request to the agent. The block body is the agent's
/// output, so provenance is only as honest as the emitter; MCP servers bake it
/// into their `display_hint` blocks (authoritative) rather than relying on the
/// agent to copy it faithfully.
///
/// Every field is `#[serde(default)]` so adding provenance to a block body is
/// non-breaking: bodies emitted before provenance lands parse with all fields
/// empty, and the widget falls back to a read-only display.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockProvenance {
    /// The MCP tool name that produced this block (e.g. `"portfolio_returns"`).
    #[serde(default)]
    pub tool: Option<String>,
    /// The MCP server name that hosts the tool (e.g. `"hkask-mcp-companies"`).
    #[serde(default)]
    pub server: Option<String>,
    /// The args the tool was invoked with, as a JSON object. A widget re-issues
    /// the tool by merging its modification into this object.
    #[serde(default)]
    pub args: Value,
    /// The `reg.*` span id under which the producing tool call was traced, for
    /// observability and re-ask detection.
    #[serde(default)]
    pub span_id: Option<String>,
}

impl BlockProvenance {
    /// Whether this provenance is sufficient to re-issue the tool: it needs
    /// both a tool name and a server name. Widgets use this to decide whether
    /// to show an active affordance or a disabled "ask the agent" hint.
    pub fn is_dispatchable(&self) -> bool {
        self.tool.is_some() && self.server.is_some()
    }

    /// Whether provenance carries no dispatchable signal (no tool, no server,
    /// null/absent args) — the shape a block body emitted before provenance
    /// landed has. Widgets use this to decide between a provenance-driven
    /// dispatch and the hardcoded fallback; any other non-dispatchable shape is
    /// treated as a partial/incomplete provenance and disabled.
    pub fn is_empty(&self) -> bool {
        self.tool.is_none()
            && self.server.is_none()
            && (self.args.is_null()
                || self
                    .args
                    .as_object()
                    .map(serde_json::Map::is_empty)
                    .unwrap_or(false))
    }
}

// ── reask correlator (T7b) ──────────────────────────────────────────────────
//
// A coarse measurement proxy for "the user re-asked after a widget rendered".
// Provenance-carrying widgets (scenarios, portfolio, kanban) call `record_render`
// on construction; the memory port calls `correlate_reask` at turn completion.
// When a user-message turn follows a turn that rendered at least one widget,
// the correlator emits a `reg.widget.reask` Regulation span. This is an
// upper-bound proxy — it counts any user message after a render turn as a
// re-ask, regardless of intent matching (open question #3 in the plan). The
// flag is global, so multi-conversation interleaving adds noise — acceptable
// for the aggregate measurement tap.

/// One widget render event, for the reask correlator. Recorded by
/// provenance-carrying widgets on construction; drained by the memory port
/// at turn completion.
#[derive(Debug, Clone)]
pub struct RenderRecord {
    pub tool: Option<String>,
    pub span_id: Option<String>,
    pub at: Instant,
}

static RENDERS: std::sync::Mutex<Vec<RenderRecord>> = std::sync::Mutex::new(Vec::new());

/// Whether the turn preceding the current `correlate_reask` call rendered any
/// widget. Global (not per-thread) — multi-conversation interleaving adds noise
/// to this measurement proxy; acceptable for the aggregate measurement tap
/// (see plan).
static PREV_HAD_RENDER: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

const MAX_RENDER_HISTORY: usize = 64;

/// Record that a provenance-carrying widget rendered. Called from widget
/// `new`/`new_widget`. Bounded to `MAX_RENDER_HISTORY` (oldest dropped).
pub fn record_render(tool: Option<String>, span_id: Option<String>) {
    if let Ok(mut renders) = RENDERS.lock() {
        renders.push(RenderRecord {
            tool,
            span_id,
            at: Instant::now(),
        });
        if renders.len() > MAX_RENDER_HISTORY {
            renders.remove(0);
        }
    } else {
        tracing::warn!(
            target: "reg.widget",
            "RENDERS mutex poisoned — widget render telemetry degraded"
        );
    }
}

/// Correlate a completed turn against recent widget renders and emit a
/// `reg.widget.reask` Regulation span when a user message followed a turn
/// that rendered a widget. Coarse upper-bound proxy: it counts ANY user
/// message after a render turn as a re-ask, regardless of intent matching
/// (the intent-matching heuristic is open question #3 in the plan). The flag
/// is global, so multi-conversation interleaving adds noise — acceptable for
/// the aggregate measurement tap.
///
/// Returns `true` when a reask was emitted this turn. Called from
/// `BridgeMemoryPort::ingest_turn` with `!user_input.trim().is_empty()`.
pub fn correlate_reask(user_message: bool) -> bool {
    // Drain this turn's renders (renders happen during the turn, before
    // ingest_turn fires; prior turns were already drained).
    let this_turn_count = if let Ok(mut renders) = RENDERS.lock() {
        let count = renders.len();
        renders.clear();
        count
    } else {
        tracing::warn!(
            target: "reg.widget",
            "RENDERS mutex poisoned — widget render telemetry degraded"
        );
        0
    };
    let prev_had_render = if let Ok(mut flag) = PREV_HAD_RENDER.lock() {
        let old = *flag;
        *flag = this_turn_count > 0;
        old
    } else {
        tracing::warn!(
            target: "reg.widget",
            "PREV_HAD_RENDER mutex poisoned — widget render telemetry degraded"
        );
        false
    };
    if user_message && prev_had_render {
        tracing::info!(
            target: "reg.widget.reask",
            this_turn_renders = this_turn_count,
            "REG",
        );
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_defaults_empty_and_not_dispatchable() {
        let p = BlockProvenance::default();
        assert!(p.tool.is_none());
        assert!(p.server.is_none());
        assert!(p.args.is_null());
        assert!(p.span_id.is_none());
        assert!(!p.is_dispatchable());
    }

    #[test]
    fn provenance_parses_partial_body() {
        let p: BlockProvenance = serde_json::from_str(r#"{"tool":"portfolio_returns"}"#).unwrap();
        assert_eq!(p.tool.as_deref(), Some("portfolio_returns"));
        assert!(p.server.is_none());
        assert!(!p.is_dispatchable());
    }

    #[test]
    fn provenance_dispatchable_when_tool_and_server_present() {
        let p: BlockProvenance = serde_json::from_str(
            r#"{"tool":"scenario_quantify","server":"hkask-mcp-scenarios","args":{"event_id":"e1"}}"#,
        )
        .unwrap();
        assert!(p.is_dispatchable());
    }

    #[test]
    fn provenance_absent_field_parses_as_empty() {
        // A block body emitted before provenance lands has no `provenance` key.
        // The widget parses the body; provenance defaults empty. This pins that
        // adding the field is non-breaking.
        let body: serde_json::Value =
            serde_json::from_str(r#"{"viz":"scenarios","pipeline":{}}"#).unwrap();
        let p: BlockProvenance = body
            .get("provenance")
            .cloned()
            .map(|v| serde_json::from_value(v).unwrap_or_default())
            .unwrap_or_default();
        assert!(!p.is_dispatchable());
    }

    // ── reask correlator tests (T7b) ────────────────────────────────────────
    //
    // `record_render` and `correlate_reask` share process-global statics
    // (`RENDERS`, `PREV_HAD_RENDER`). Tests that mutate them must serialize so
    // parallel test threads never observe each other's state. `TEST_LOCK`
    // serializes the three correlator tests within this binary; each test
    // resets the global state at start (drain renders, clear the flag).
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Reset the correlator global state. Called at the start of each test
    /// under `TEST_LOCK` so prior test state never leaks in.
    fn reset_correlator_state() {
        if let Ok(mut renders) = RENDERS.lock() {
            renders.clear();
        }
        if let Ok(mut flag) = PREV_HAD_RENDER.lock() {
            *flag = false;
        }
    }

    #[test]
    fn record_render_then_drain() {
        let _guard = TEST_LOCK.lock().expect("correlator test lock poisoned");
        reset_correlator_state();
        record_render(Some("portfolio_returns".into()), None);
        record_render(Some("scenario_quantify".into()), Some("span-1".into()));
        record_render(None, None);
        // Three renders buffered before the drain.
        let len_before = RENDERS.lock().map(|renders| renders.len()).unwrap_or(0);
        assert_eq!(len_before, 3, "three renders should be buffered");
        // A user-message turn with no prior render: drains the 3 records, but
        // prev_had_render was false so no reask. The drain clears the buffer.
        let emitted = correlate_reask(true);
        assert!(!emitted, "first turn after reset has no prior render");
        let len_after = RENDERS.lock().map(|renders| renders.len()).unwrap_or(0);
        assert_eq!(len_after, 0, "correlate_reask must drain the buffer");
        // Second drain (next turn) returns 0 — the prior drain cleared the buffer.
        let emitted_again = correlate_reask(false);
        assert!(!emitted_again, "no renders this turn");
    }

    #[test]
    fn record_render_bounds_history() {
        let _guard = TEST_LOCK.lock().expect("correlator test lock poisoned");
        reset_correlator_state();
        for _ in 0..(MAX_RENDER_HISTORY + 5) {
            record_render(Some("kanban_task_move".into()), None);
        }
        // The buffer is bounded to MAX_RENDER_HISTORY — the oldest 5 were
        // dropped. Inspect the static directly (in-crate test) since
        // `correlate_reask` returns a bool, not the count.
        let len = RENDERS.lock().map(|renders| renders.len()).unwrap_or(0);
        assert_eq!(
            len, MAX_RENDER_HISTORY,
            "render history must be bounded to MAX_RENDER_HISTORY"
        );
        // Drain via correlate_reask to leave clean state.
        let emitted = correlate_reask(false);
        assert!(!emitted, "non-user-message turn never emits reask");
    }

    #[test]
    fn correlate_reask_emits_only_when_user_message_follows_render() {
        let _guard = TEST_LOCK.lock().expect("correlator test lock poisoned");
        reset_correlator_state();

        // Turn A: render turn. record_render pushes 2 renders; ingest_turn
        // fires correlate_reask(user_message=false) — drains the 2, sets
        // prev_had_render=true, no reask (not a user message).
        record_render(Some("portfolio_returns".into()), None);
        record_render(Some("scenario_quantify".into()), None);
        let emitted_a = correlate_reask(false);
        assert!(
            !emitted_a,
            "render turn (no user message) must not emit reask"
        );

        // Turn B: user-message turn, no new renders. Drains 0, prev_had_render
        // is true (from turn A), user_message=true → reask emitted.
        let emitted_b = correlate_reask(true);
        assert!(
            emitted_b,
            "user message following a render turn must emit reask"
        );

        // Turn C: another user-message turn with no prior render. Drains 0,
        // prev_had_render is now false (turn B had 0 renders) → no reask.
        let emitted_c = correlate_reask(true);
        assert!(
            !emitted_c,
            "user message after a non-render turn must not emit reask"
        );
    }
}
