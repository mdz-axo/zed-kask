//! Widget→agent compose-back seam, in a leaf crate.
//!
//! Lets a kask widget compose a structured message back into the active agent
//! conversation (a revision request / "I disagree"), mirroring the widget→MCP
//! [`hkask_tool_invoker::ToolInvoker`] seam but for widget→agent. The widget
//! calls [`ConversationInjector::inject`] from a foreground `on_click` (which
//! has `window` + `cx`), so the production impl (in `agent_ui`) updates the
//! active `ThreadView`'s message editor directly — no cross-thread channel is
//! needed (the call is foreground; `AsyncApp` is not `Send`, but it is not
//! captured across threads here).
//!
//! The trait + the per-app global accessor live here so the kask GPUI widget
//! crates can compose back into the conversation without depending on the
//! heavy `agent_ui` crate (which would invert sane layering: leaf widgets →
//! heavy panel/agent). The production impl (`ThreadConversationInjector`) lives
//! in `crates/agent_ui/src/conversation_view.rs` (the D-seam) and holds a weak
//! ref to the active `Entity<ThreadView>`; the composition root /
//! `ConversationView` activation publishes it via [`set_active_injector`].
//!
//! The injector is stored as a per-app [`gpui::Global`], not a process-global.
//! A process-global would outlive the per-app entity map and keep the active
//! `ThreadView` alive after its `App`/`TestAppContext` drops — a leaked handle
//! that broke ~47 `agent_ui` tests. The per-app global drops with the app, so
//! the weak ref can never retain a dead thread across app/test lifetimes.

use std::sync::Arc;

use gpui::{App, Global, Task, Window};

/// Inject a user-authored message into the active conversation. The production
/// impl (in `agent_ui`) pre-fills the active `ThreadView`'s message editor with
/// `body`; the user reviews and submits via the existing Send button so the
/// turn-loop's checkpoints/telemetry are preserved (auto-send would bypass
/// them).
///
/// Called from a widget `on_click` (foreground), so `window` + `cx` are passed
/// in — the impl updates the active thread entity directly, no cross-thread
/// channel needed.
pub trait ConversationInjector: Send + Sync {
    /// Compose `body` back into the active conversation. Returns a `Task` so the
    /// caller can await completion; the production impl is synchronous and
    /// returns `Task::ready`. Returns `Err` when the active conversation no
    /// longer exists (the active `ThreadView` was dropped) — callers must
    /// surface the composed body as a visible draft, not a silent no-op.
    fn inject(&self, body: String, window: &mut Window, cx: &mut App) -> Task<Result<(), String>>;
}

/// Per-app holder for the active-conversation injector. Stored as a GPUI
/// global so it drops with the `App`/`TestAppContext`; a process-global would
/// outlive the per-app entity map and leak the active `ThreadView` across
/// app/test lifetimes.
struct ActiveInjector(Option<Arc<dyn ConversationInjector>>);

impl Global for ActiveInjector {}

/// Composition root + `ConversationView` activation call this to publish the
/// active-conversation injector. Re-settable so each activation of a different
/// `ThreadView` replaces the prior one. Pass `None` to clear (e.g. when the
/// active conversation is closed).
pub fn set_active_injector(cx: &mut App, injector: Option<Arc<dyn ConversationInjector>>) {
    match injector {
        Some(injector) => cx.set_global(ActiveInjector(Some(injector))),
        None => {
            if cx.has_global::<ActiveInjector>() {
                cx.remove_global::<ActiveInjector>();
            }
        }
    }
}

/// Widgets read this for the "I disagree" affordance. Returns `None` when no
/// conversation is active — callers MUST surface this as a visible hint (e.g.
/// show the composed body as a copyable draft), not a silent no-op (repo
/// `.rules` "Process-global hooks set at runtime need a startup-failure
/// signal").
pub fn shared_injector(cx: &App) -> Option<Arc<dyn ConversationInjector>> {
    cx.try_global::<ActiveInjector>().and_then(|g| g.0.clone())
}
