//! Status bar button that toggles the kask panel.
//!
//! This is a `StatusItemView` registered on the workspace status bar (bottom
//! bar) so the user can open the kask panel by clicking an icon, in addition
//! to the View dropdown menu entry (`kask_panel::Toggle`).
//!
//! Mirrors `search::search_status_button::SearchButton` — a minimal
//! `StatusItemView` that dispatches an action on click. The icon is
//! `IconName::Kask` (the kask logo), matching the panel's tab icon.

use gpui::{App, Context, Window, prelude::*};
use ui::{ButtonCommon, Clickable, IconButton, IconName, IconSize, Tooltip};
use workspace::{HideStatusItem, ItemHandle, StatusItemView};

use zed_actions::kask_panel::Toggle;

/// Status bar button that opens/focuses the kask panel.
///
/// Rendered as a small icon button on the bottom status bar. Clicking it
/// dispatches `kask_panel::Toggle`, which deploys a new panel if none is open
/// or focuses the existing one.
pub struct KaskPanelButton {
    pane_item_focus_handle: Option<gpui::FocusHandle>,
}

impl KaskPanelButton {
    pub fn new() -> Self {
        Self {
            pane_item_focus_handle: None,
        }
    }
}

impl Default for KaskPanelButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for KaskPanelButton {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.pane_item_focus_handle.clone();
        IconButton::new("kask-panel-button", IconName::Kask)
            .icon_size(IconSize::Small)
            .tab_index(0isize)
            .aria_label("Kask Panel")
            .tooltip(move |_window, cx| {
                if let Some(focus_handle) = &focus_handle {
                    Tooltip::for_action_in("Toggle Kask Panel", &Toggle, focus_handle, cx)
                } else {
                    Tooltip::for_action("Toggle Kask Panel", &Toggle, cx)
                }
            })
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(Toggle), cx);
            })
    }
}

impl StatusItemView for KaskPanelButton {
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
        // The kask panel button is always available — no hide setting.
        None
    }
}
