//! Status bar button that toggles the Kask Extensions page.
//!
//! A `marketplace_ui_common::PanelToggleButton` configured with the Kask
//! Extensions button's id, icon, labels, and `Toggle` action. The icon is
//! `IconName::Share` (visual language for sharing/trading skills in the
//! marketplace), distinct from `IconName::Kask` used by the kask panel. The
//! render, tooltip, click-dispatch, and active-pane-item tracking logic lives
//! once in `PanelToggleButton`; this module only supplies the panel-specific
//! parameters and a no-argument constructor.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

/// The status bar button type that opens/focuses the Kask Extensions page.
///
/// A type alias for `PanelToggleButton<Toggle>` so callers (and the status
/// bar) refer to the panel by name while the implementation is shared.
pub type KaskExtensionsButton = PanelToggleButton<Toggle>;

/// Construct the Kask Extensions status bar button with its panel-specific
/// parameters (id, icon, labels, and `Toggle` action).
pub fn new() -> KaskExtensionsButton {
    PanelToggleButton::new(
        "kask-extensions-button",
        IconName::Share,
        "Kask Extensions",
        "Toggle Kask Extensions",
        Toggle,
    )
}
