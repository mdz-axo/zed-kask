//! Status bar button that toggles the Agent Swarm panel.
//!
//! A `marketplace_ui_common::PanelToggleButton` configured with the Agent
//! Swarm button's id, icon, labels, and `Toggle` action. The render, tooltip,
//! click-dispatch, and active-pane-item tracking logic lives once in
//! `PanelToggleButton`; this module only supplies the panel-specific
//! parameters and a no-argument constructor.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

/// The status bar button type that opens/focuses the Agent Swarm panel.
///
/// A type alias for `PanelToggleButton<Toggle>` so callers (and the status
/// bar) refer to the panel by name while the implementation is shared.
pub type SwarmPanelButton = PanelToggleButton<Toggle>;

/// Construct the Agent Swarm status bar button with its panel-specific
/// parameters (id, icon, labels, and `Toggle` action).
pub fn new() -> SwarmPanelButton {
    PanelToggleButton::new(
        "swarm-panel-button",
        IconName::Share,
        "Agent Swarm",
        "Toggle Agent Swarm",
        Toggle,
    )
}
