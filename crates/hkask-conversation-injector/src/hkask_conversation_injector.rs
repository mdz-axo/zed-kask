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

/// S10/R2: shared compose-back seam for all kask widgets. Emits the
/// `reg.widget.disagree` span, injects `body` into the active conversation via
/// `shared_injector()`, and returns:
/// - `None` on a successful inject (the caller clears its draft field).
/// - `Some(draft)` on a no-injector or inject-error path (the caller surfaces
///   `draft` as a copyable draft — visible, not a silent no-op — repo
///   `.rules`).
///
/// The injection's `Task` is awaited in a detached `cx.spawn` so the `Err`
/// path surfaces the draft asynchronously (the production injector pre-fills
/// the composer synchronously, so `Ok` is immediate; `Err` means the active
/// conversation was dropped). The caller passes a `WeakEntity<W>` and a
/// `set_draft` closure so this helper can update the widget's draft field on
/// the inject-error path without the caller plumbing its own `cx.spawn`.
///
/// Mirrors the per-widget `compose_back` methods that previously lived in
/// `hkask-kanban-widget`, `hkask-graph-widget`, and `hkask-portfolio-widget`
/// (identical bodies modulo the widget type). Extracted here so the three
/// widgets share one implementation.
pub fn compose_back_via_injector<W, F>(
    body: String,
    window: &mut Window,
    cx: &mut App,
    widget: gpui::WeakEntity<W>,
    set_draft: F,
) -> Option<String>
where
    W: 'static,
    F: Fn(&mut W, Option<String>) + 'static + Send + Sync,
{
    tracing::info!(target: "reg.widget.disagree", "REG");
    if let Some(injector) = shared_injector(cx) {
        let draft = body.clone();
        let task = injector.inject(body, window, cx);
        cx.spawn(async move |cx| {
            if let Err(error) = task.await {
                tracing::warn!(
                    target: "reg.widget.disagree",
                    error = %error,
                    "conversation inject failed; surfacing draft"
                );
                if widget
                    .update(cx, |widget, cx| {
                        set_draft(widget, Some(draft));
                        cx.notify();
                    })
                    .is_err()
                {
                    tracing::warn!(
                        target: "reg.widget.disagree",
                        "widget dropped before inject-error draft could be surfaced"
                    );
                }
            }
        })
        .detach();
        None
    } else {
        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext, Context, IntoElement, TestAppContext};
    use std::sync::{Arc, Mutex};

    struct OkInjector {
        injected: Arc<Mutex<Vec<String>>>,
    }

    impl ConversationInjector for OkInjector {
        fn inject(
            &self,
            body: String,
            _window: &mut Window,
            _cx: &mut App,
        ) -> Task<Result<(), String>> {
            self.injected.lock().expect("lock").push(body);
            Task::ready(Ok(()))
        }
    }

    struct ErrInjector;

    impl ConversationInjector for ErrInjector {
        fn inject(
            &self,
            _body: String,
            _window: &mut Window,
            _cx: &mut App,
        ) -> Task<Result<(), String>> {
            Task::ready(Err("conversation dropped".to_string()))
        }
    }

    #[derive(Default)]
    struct DummyWidget {
        draft: Option<String>,
    }

    struct DummyView;

    impl gpui::Render for DummyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            gpui::div()
        }
    }

    #[gpui::test]
    async fn compose_back_via_injector_returns_some_on_no_injector(
        cx: &mut TestAppContext,
    ) {
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let draft = cx.update(|window, cx| {
            let widget = cx.new(|_cx| DummyWidget::default()).downgrade();
            compose_back_via_injector(
                "disagree body".to_string(),
                window,
                cx,
                widget,
                |this, draft| {
                    this.draft = draft;
                },
            )
        });
        assert_eq!(draft.as_deref(), Some("disagree body"));
    }

    #[gpui::test]
    async fn compose_back_via_injector_returns_none_on_successful_inject(
        cx: &mut TestAppContext,
    ) {
        let injected = Arc::new(Mutex::new(Vec::new()));
        let injector: Arc<dyn ConversationInjector> = Arc::new(OkInjector {
            injected: injected.clone(),
        });
        cx.update(|cx| set_active_injector(cx, Some(injector)));

        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let draft = cx.update(|window, cx| {
            let widget = cx.new(|_cx| DummyWidget::default()).downgrade();
            compose_back_via_injector(
                "disagree body".to_string(),
                window,
                cx,
                widget,
                |this, draft| {
                    this.draft = draft;
                },
            )
        });
        assert!(draft.is_none(), "successful inject returns None");
        let injected = injected.lock().expect("lock");
        assert_eq!(injected.len(), 1);
        assert_eq!(injected[0], "disagree body");
    }

    #[gpui::test]
    async fn compose_back_via_injector_surfaces_draft_on_inject_error(
        cx: &mut TestAppContext,
    ) {
        let injector: Arc<dyn ConversationInjector> = Arc::new(ErrInjector);
        cx.update(|cx| set_active_injector(cx, Some(injector)));

        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|_cx| DummyWidget::default()));
        let weak = widget.downgrade();
        cx.update(|window, cx| {
            compose_back_via_injector(
                "disagree body".to_string(),
                window,
                cx,
                weak,
                |this, draft| {
                    this.draft = draft;
                },
            )
        });
        cx.run_until_parked();
        let draft = widget.read_with(cx, |this, _| this.draft.clone());
        assert_eq!(draft.as_deref(), Some("disagree body"));
    }
}
