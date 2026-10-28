//! Shared status bar toggle button for center-pane catalog pages.
//!
//! `swarm_panel::SwarmPanelButton` and `kanban_panel::KanbanPanelButton`
//! implement the same `StatusItemView` pattern — track the active pane item's focus
//! handle, render an `IconButton` that dispatches a `Toggle` action on click, and
//! expose a tooltip whose keybinding reflects the active item.
//! `PanelToggleButton` captures that pattern once, generic over the `Toggle`
//! action the registering panel defines.

use gpui::{App, Context, FocusHandle, SharedString, Window, prelude::*};
use ui::{ButtonCommon, Clickable, IconButton, IconName, IconSize, Tooltip};
use workspace::{HideStatusItem, ItemHandle, StatusItemView};

/// A status bar button that toggles a center-pane `Item` page.
///
/// Generic over the `Toggle` action the panel registers with the workspace.
/// On click, dispatches a clone of `action` via `window.dispatch_action`,
/// mirroring the per-panel implementations this replaces. The active pane
/// item's focus handle is tracked so the tooltip can show the item-scoped
/// keybinding when the page is open.
pub struct PanelToggleButton<A: gpui::Action + Clone> {
    button_id: SharedString,
    icon: IconName,
    aria_label: SharedString,
    tooltip_label: SharedString,
    action: A,
    pane_item_focus_handle: Option<FocusHandle>,
}

impl<A: gpui::Action + Clone> PanelToggleButton<A> {
    /// Construct a new toggle button.
    ///
    /// `button_id` must be unique among `IconButton`s in the status bar.
    /// `aria_label` and `tooltip_label` are shown to assistive tech and on
    /// hover respectively; `action` is the `Toggle` action dispatched on click.
    pub fn new(
        button_id: impl Into<SharedString>,
        icon: IconName,
        aria_label: impl Into<SharedString>,
        tooltip_label: impl Into<SharedString>,
        action: A,
    ) -> Self {
        Self {
            button_id: button_id.into(),
            icon,
            aria_label: aria_label.into(),
            tooltip_label: tooltip_label.into(),
            action,
            pane_item_focus_handle: None,
        }
    }
}

impl<A: gpui::Action + Clone> Render for PanelToggleButton<A> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.pane_item_focus_handle.clone();
        let tooltip_label = self.tooltip_label.clone();
        let tooltip_action = self.action.clone();
        let click_action = self.action.clone();
        IconButton::new(self.button_id.clone(), self.icon)
            .icon_size(IconSize::Small)
            .tab_index(0isize)
            .aria_label(self.aria_label.clone())
            .tooltip(move |_window, cx| {
                if let Some(focus_handle) = &focus_handle {
                    Tooltip::for_action_in(tooltip_label.clone(), &tooltip_action, focus_handle, cx)
                } else {
                    Tooltip::for_action(tooltip_label.clone(), &tooltip_action, cx)
                }
            })
            .on_click(move |_, window, cx| {
                window.dispatch_action(Box::new(click_action.clone()), cx);
            })
    }
}

impl<A: gpui::Action + Clone> StatusItemView for PanelToggleButton<A> {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_item_focus_handle = active_pane_item.map(|item| item.item_focus_handle(cx));
        cx.notify();
    }

    fn hide_setting(&self, _: &App) -> Option<HideStatusItem> {
        None
    }
}
