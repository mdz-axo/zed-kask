//! Status bar button that toggles the Media panel.
//!
//! A `marketplace_ui_common::PanelToggleButton` configured with the Media
//! panel's id, icon, labels, and `Toggle` action. Mirrors
//! `swarm_panel::panel_button` and `kanban_panel::panel_button`.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

/// The status bar button type that opens/focuses the Media panel.
pub type MediaPanelButton = PanelToggleButton<Toggle>;

/// Construct the Media status bar button with its panel-specific
/// parameters (id, icon, labels, and `Toggle` action).
pub fn new() -> MediaPanelButton {
    PanelToggleButton::new(
        "media-panel-button",
        IconName::Image,
        "Media",
        "Toggle Media",
        Toggle,
    )
}
