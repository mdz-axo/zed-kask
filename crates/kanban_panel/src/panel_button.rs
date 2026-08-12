//! Status bar button that toggles the Kanban Board panel.
//!
//! A `marketplace_ui_common::PanelToggleButton` configured with the Kanban
//! Board button's id, icon, labels, and `Toggle` action. Mirrors
//! `swarm_panel::panel_button`.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

/// The status bar button type that opens/focuses the Kanban Board panel.
pub type KanbanPanelButton = PanelToggleButton<Toggle>;

/// Construct the Kanban Board status bar button with its panel-specific
/// parameters (id, icon, labels, and `Toggle` action).
pub fn new() -> KanbanPanelButton {
    PanelToggleButton::new(
        "kanban-panel-button",
        IconName::ListTodo,
        "Kanban Board",
        "Toggle Kanban Board",
        Toggle,
    )
}
