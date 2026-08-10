//! Status bar button that toggles the Kask Extensions page.
//!
//! A `marketplace_ui_common::PanelToggleButton` configured with the Kask
//! Extensions button's id, icon, labels, and `Toggle` action. This button now
//! carries the zk application mark because the former kask panel, which was
//! the only status-bar consumer of `IconName::Kask`, no longer exists. The
//! render, tooltip, click-dispatch, and active-pane-item tracking logic lives
//! once in `PanelToggleButton`; this module only supplies the panel-specific
//! parameters and a no-argument constructor.

use marketplace_ui_common::PanelToggleButton;
use ui::IconName;

use crate::Toggle;

const KASK_STATUS_ICON: IconName = IconName::Kask;

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
        KASK_STATUS_ICON,
        "Kask Extensions",
        "Toggle Kask Extensions",
        Toggle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kask_status_button_uses_zk_icon() {
        assert_eq!(KASK_STATUS_ICON, IconName::Kask);
    }
}
