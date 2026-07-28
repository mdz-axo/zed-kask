//! Status bar button that toggles the Kask Extensions page.
//!
//! Mirrors `kask_panel::panel_button::KaskPanelButton` — a minimal
//! `StatusItemView` that dispatches `Toggle` on click. The icon is
//! `IconName::Share` (visual language for sharing/trading skills in the
//! marketplace), distinct from `IconName::Kask` used by the kask panel.

use gpui::{App, Context, Window, prelude::*};
use ui::{ButtonCommon, Clickable, IconButton, IconName, IconSize, Tooltip};
use workspace::{HideStatusItem, ItemHandle, StatusItemView};

use crate::Toggle;

/// Status bar button that opens/focuses the Kask Extensions page.
pub struct KaskExtensionsButton {
    pane_item_focus_handle: Option<gpui::FocusHandle>,
}

impl KaskExtensionsButton {
    pub fn new() -> Self {
        Self {
            pane_item_focus_handle: None,
        }
    }
}

impl Default for KaskExtensionsButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for KaskExtensionsButton {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.pane_item_focus_handle.clone();
        IconButton::new("kask-extensions-button", IconName::Share)
            .icon_size(IconSize::Small)
            .tab_index(0isize)
            .aria_label("Kask Extensions")
            .tooltip(move |_window, cx| {
                if let Some(focus_handle) = &focus_handle {
                    Tooltip::for_action_in("Toggle Kask Extensions", &Toggle, focus_handle, cx)
                } else {
                    Tooltip::for_action("Toggle Kask Extensions", &Toggle, cx)
                }
            })
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(Toggle), cx);
            })
    }
}

impl StatusItemView for KaskExtensionsButton {
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
