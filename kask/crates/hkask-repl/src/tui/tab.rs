//! Tab — a named collection of window splits.
//!
//! Each tab has its own SplitNode root tree and display name.
//! Tabs isolate workspaces within a single TUI session.

/// A tab contains a named split tree.
#[derive(Debug)]
pub struct Tab {
    /// Display name shown in the tab bar
    pub name: String,
    /// Root of the split tree for this tab
    pub root: crate::tui::workspace::SplitNode,
}

impl Tab {
    /// Create a new tab with the given name and root split node.
    pub fn new(name: String, root: crate::tui::workspace::SplitNode) -> Self {
        Self { name, root }
    }
}
