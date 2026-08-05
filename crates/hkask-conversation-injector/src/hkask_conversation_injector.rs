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
//! The trait + the process-global accessor live here so the kask GPUI widget
//! crates can compose back into the conversation without depending on the
//! heavy `agent_ui` crate (which would invert sane layering: leaf widgets →
//! heavy panel/agent). The production impl (`ThreadConversationInjector`) lives
//! in `crates/agent_ui/src/conversation_view.rs` (the D-seam) and holds the
//! active `Entity<ThreadView>`; the composition root / `ConversationView`
//! activation publishes it via [`set_active_injector`].

use std::sync::Arc;

use gpui::{App, Task, Window};

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
    /// returns `Task::ready`.
    fn inject(&self, body: String, window: &mut Window, cx: &mut App) -> Task<Result<(), String>>;
}

static INJECTOR: std::sync::Mutex<Option<Arc<dyn ConversationInjector>>> =
    std::sync::Mutex::new(None);

/// Composition root + `ConversationView` activation call this to publish the
/// active-conversation injector. Re-settable (Mutex, not OnceLock) so each
/// activation of a different `ThreadView` replaces the prior one. Pass `None`
/// to clear (e.g. when the active conversation is closed).
pub fn set_active_injector(injector: Option<Arc<dyn ConversationInjector>>) {
    *INJECTOR.lock().expect("INJECTOR poisoned") = injector;
}

/// Widgets read this for the "I disagree" affordance. Returns `None` when no
/// conversation is active — callers MUST surface this as a visible hint (e.g.
/// show the composed body as a copyable draft), not a silent no-op (repo
/// `.rules` "Process-global hooks set at runtime need a startup-failure
/// signal").
pub fn shared_injector() -> Option<Arc<dyn ConversationInjector>> {
    INJECTOR.lock().expect("INJECTOR poisoned").clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The static `INJECTOR` is process-global; tests that mutate it must
    // serialize so parallel test threads never observe each other's injector.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that restores the global injector to `None` on drop so a
    /// test failure cannot leak a mock into sibling tests.
    struct InjectorGuard;
    impl Drop for InjectorGuard {
        fn drop(&mut self) {
            set_active_injector(None);
        }
    }

    /// Trivial injector used only as a placeholder `Arc<dyn
    /// ConversationInjector>` for the global. `inject` is never called here
    /// (it needs `Window` + `App`, which this leaf crate does not enable the
    /// `test-support` feature for); the tests below exercise only the global
    /// accessor contract.
    #[derive(Default)]
    struct NoopInjector;

    impl ConversationInjector for NoopInjector {
        fn inject(
            &self,
            _body: String,
            _window: &mut Window,
            _cx: &mut App,
        ) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
    }

    #[test]
    fn shared_injector_returns_none_by_default() {
        let _guard = TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InjectorGuard;
        set_active_injector(None);
        assert!(shared_injector().is_none(), "no injector wired by default");
    }

    #[test]
    fn set_then_shared_returns_some() {
        let _guard = TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InjectorGuard;
        set_active_injector(None);
        let injector = Arc::new(NoopInjector);
        set_active_injector(Some(injector));
        assert!(
            shared_injector().is_some(),
            "shared_injector must return the wired injector"
        );
    }

    #[test]
    fn set_none_clears_a_prior_injector() {
        let _guard = TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InjectorGuard;
        set_active_injector(None);
        let injector = Arc::new(NoopInjector);
        set_active_injector(Some(injector));
        assert!(shared_injector().is_some());
        set_active_injector(None);
        assert!(
            shared_injector().is_none(),
            "set_active_injector(None) must clear the global"
        );
    }
}
