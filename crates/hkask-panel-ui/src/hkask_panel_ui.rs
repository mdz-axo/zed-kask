#![forbid(unsafe_code)]
//! zed-kask: shared UI chrome and workspace-action registration for the kask
//! GPUI panels (`kanban_panel`, `media_panel`, `nostr_panel`):
//! - `PanelToggleButton` — status-bar button configured per-panel.
//! - `register_center_panel_actions` — shared `Toggle` / `ToggleFocus`
//!   workspace action registration for center-pane panel items.
//!
//! The former marketplace-catalog chrome (card container, bordered search
//! bar, empty-state row) was deleted at the 2026-11-02 RA-01 remediation:
//! its only consumer was the retired swarm panel (D33, 2026-10-28), and the
//! symbol census found zero remaining callers — the shared half that
//! survives is the panel toggle button and center-pane action wiring.

mod panel_button;
pub use panel_button::PanelToggleButton;

use gpui::{Action, App, Context, Entity, Focusable, Window};
use workspace::{Workspace, item::Item};

/// Register `Toggle` (activate an existing center-pane item, or deploy and
/// focus a new one) and `ToggleFocus` (activate an existing center-pane item;
/// no-op when absent) on every new `Workspace`.
///
/// Callers keep their own `register_serializable_item::<P>(cx)` call in `init`
/// so panel-level serialization pins remain explicit.
pub fn register_center_panel_actions<P, Toggle, ToggleFocus>(
    cx: &mut App,
    build: fn(&Workspace, &mut Window, &mut Context<Workspace>) -> Entity<P>,
) where
    P: Item + Focusable,
    Toggle: Action,
    ToggleFocus: Action,
{
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // View menu entries use `Toggle` (deploys a new item if none exists),
        // not `ToggleFocus` (silent no-op when absent).
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<P>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let panel = build(workspace, window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    panel.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<P>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}
