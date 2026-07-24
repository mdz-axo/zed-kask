//! Window trait — the interface every sub-window implements.
//!
//! Modelled on Zed's Item/View architecture: each window has its own
//! state, render function, and event handler. The workspace manages
//! layout, focus, and event routing.

use crossterm::event::KeyEvent;
use ratatui::Frame;
use ratatui::layout::Rect;
use uuid::Uuid;

/// Unique identifier for a window instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub Uuid);

/// Actions a window can request from the workspace.
///
/// Drained via `Window::drain_action()` after each tick. Windows emit
/// these to request structural changes (open/close/split) that they
/// cannot perform themselves because they don't own the split tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAction {
    /// Open a window of the given kind as a split from the focused window.
    OpenWindow(WindowKind),
    /// Close the focused window.
    CloseFocused,
    /// Split the focused window in the given direction.
    Split(SplitDirection),
    /// Cycle focus to the next window.
    FocusNext,
    /// Cycle focus to the previous window.
    FocusPrev,
    /// Create a new tab with an optional name.
    NewTab(Option<String>),
    /// Switch to the next tab.
    NextTab,
    /// Switch to the previous tab.
    PrevTab,
    /// Quit the TUI.
    Quit,
}

/// Direction for splitting a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Side-by-side (left | right).
    Horizontal,
    /// Stacked (top / bottom).
    Vertical,
}

/// The kind of window — determines title, description, and creation behaviour.
///
/// Each variant maps to an MCP server or a local UI surface. The workspace
/// factory (`window_catalog::create_window`) constructs the concrete `Window`
/// impl for each kind. Layout persistence serializes the kind by
/// `default_title()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// AI chat interface — the primary interaction window.
    /// Curator chat is a mode within ChatWindow.
    Chat,
    /// Kanban board — task coordination via `hkask-mcp-kata-kanban`.
    Kanban,
    /// Companies — financial data via `hkask-mcp-companies`.
    Companies,
    /// Scenarios — scenario planning and forecasting via `hkask-mcp-scenarios`.
    Scenarios,
}

impl WindowKind {
    pub fn default_title(&self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Kanban => "Kanban",
            Self::Companies => "Companies",
            Self::Scenarios => "Scenarios",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Chat => "AI chat with your userpod agent (default: Chat mode)",
            Self::Kanban => "Kanban board and task coordination",
            Self::Companies => "Company financial data and profiles",
            Self::Scenarios => "Scenario planning and forecast tracking",
        }
    }

    /// Whether multiple instances of this window kind can exist.
    pub fn allows_multiple(&self) -> bool {
        match self {
            Self::Chat => true,
            Self::Kanban | Self::Companies | Self::Scenarios => false,
        }
    }

    /// Whether this window handles Tab key internally.
    pub fn uses_internal_tab(&self) -> bool {
        // No MCP window currently handles Tab internally.
        false
    }

    /// Parse a kind from a slash-command argument (case-insensitive).
    ///
    /// Accepts both the title ("Chat") and the variant name ("chat").
    pub fn parse_kind(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "chat" => Some(Self::Chat),
            "kanban" => Some(Self::Kanban),
            "companies" => Some(Self::Companies),
            "scenarios" => Some(Self::Scenarios),
            _ => None,
        }
    }
}

/// Every sub-window implements this trait.
///
/// This is the single interface the workspace uses to render and
/// interact with windows. The trait is object-safe so windows can
/// be stored heterogeneously.
pub trait Window {
    /// Unique identifier for this window instance.
    fn id(&self) -> WindowId;

    /// Display title shown in the window border and tab bar.
    fn title(&self) -> &str;

    /// The kind of window.
    fn kind(&self) -> WindowKind;

    /// Render this window into the given rectangle.
    fn render(&self, f: &mut Frame, area: Rect, is_focused: bool);

    /// Handle a key event. Return `true` if the event was consumed.
    fn handle_key(&mut self, key: KeyEvent) -> bool;

    /// Whether this window can be closed by the user.
    fn can_close(&self) -> bool {
        true
    }

    /// Called when this window gains keyboard focus.
    fn on_focus(&mut self) {}

    /// Called when this window loses keyboard focus.
    fn on_blur(&mut self) {}

    /// Called periodically (every tick) for background updates.
    fn tick(&mut self) {}

    /// Drain all pending workspace actions (e.g., from slash commands).
    ///
    /// Returns a `Vec` so a window can emit multiple actions per tick
    /// (e.g., `/open kanban /split v` in one input line).
    fn drain_actions(&mut self) -> Vec<WorkspaceAction> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_multiple_is_correct() {
        assert!(WindowKind::Chat.allows_multiple());
        assert!(!WindowKind::Kanban.allows_multiple());
        assert!(!WindowKind::Companies.allows_multiple());
        assert!(!WindowKind::Scenarios.allows_multiple());
    }

    #[test]
    fn default_titles_are_non_empty() {
        for kind in [
            WindowKind::Chat,
            WindowKind::Kanban,
            WindowKind::Companies,
            WindowKind::Scenarios,
        ] {
            assert!(
                !kind.default_title().is_empty(),
                "{:?} has empty title",
                kind
            );
            assert!(
                !kind.description().is_empty(),
                "{:?} has empty description",
                kind
            );
        }
    }

    #[test]
    fn from_str_parses_case_insensitive() {
        assert_eq!(WindowKind::parse_kind("chat"), Some(WindowKind::Chat));
        assert_eq!(WindowKind::parse_kind("Kanban"), Some(WindowKind::Kanban));
        assert_eq!(
            WindowKind::parse_kind("COMPANIES"),
            Some(WindowKind::Companies)
        );
        assert_eq!(
            WindowKind::parse_kind("scenarios"),
            Some(WindowKind::Scenarios)
        );
        assert_eq!(WindowKind::parse_kind("nonexistent"), None);
    }
}
