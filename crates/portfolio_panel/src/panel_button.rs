//! Status bar button that toggles the Portfolio panel.
//!
//! Mirrors `kanban_panel::panel_button` — a `marketplace_ui_common::PanelToggleButton`
//! configured with the id, icon, labels, and `Toggle` action.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

/// The status bar button type that opens/focuses the Portfolio panel.
pub type PortfolioPanelButton = PanelToggleButton<Toggle>;

/// Construct the Portfolio status bar button with its parameters.
pub fn new() -> PortfolioPanelButton {
    PanelToggleButton::new(
        "portfolio-panel-button",
        IconName::ChartBar,
        "Portfolio",
        "Toggle Portfolio",
        Toggle,
    )
}
