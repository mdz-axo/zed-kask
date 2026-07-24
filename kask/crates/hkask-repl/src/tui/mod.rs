//! hKask TUI — Terminal UI workspace for the hKask tool platform.
//!
//! Provides a multi-window, split-pane terminal interface modelled on
//! Zed's workspace architecture: a binary tree of splits hosts stateful
//! Window implementations, with keyboard-driven focus, split, close, and
//! tab management. Windows can be opened via slash commands (/open kanban)
//! or keybindings (Ctrl-W prefix sub-mode). Each MCP-backed window (Kanban,
//! Companies, Scenarios) runs inference scoped to its MCP server's tools.
//!
//! # Architecture
//!
//! ```text
//! TuiSession
//!   �u2514── Workspace
//!         ├── Tab bar
//!         ├── SplitNode tree (binary splits)
//!         │     ├── Leaf: Window
//!         │     ├── Horizontal { left, right, ratio }
//!         │     └── Vertical   { top, bottom, ratio }
//!         └── StatusBar (global)
//! ```
//!
//! # Entry Point
//!
//! ```ignore
//! let session = TuiSession::new(state, repl_settings)?;
//! session.run()?;
//! ```

#![allow(clippy::too_many_arguments)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::new_without_default)]
#![allow(clippy::useless_format)]
#![allow(clippy::needless_borrow)]

mod repl_bridge;
mod splash;
mod status_bar;
mod tab;
mod text_cursor;
mod window;
mod window_catalog;
mod workspace;

#[cfg(test)]
mod render_guards_tests;
#[cfg(test)]
mod test_util;

pub mod bridges;
pub mod layout;
pub mod widgets;
pub mod windows;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;
use std::io::Stdout;
use std::time::Duration;

use bridges::{tui_bridge_setter, with_bridges};
pub use repl_bridge::{
    CommandResult, InferenceRequestId, InferenceState, McpInvokeError, McpInvokeRequestId,
    McpInvokeState, ModelSwitchResult, ReplBridge, SessionBridge, SettingsBridge, SystemBridge,
    ToolInvokeBridge, TuiModelInfo, TuiTurnResult,
};
pub use splash::SplashScreen;
pub use window::{SplitDirection, Window, WindowId, WindowKind, WorkspaceAction};
pub use workspace::Workspace;

/// Top-level TUI session — owns the terminal, workspace, and event loop.
///
/// Constructed with the shared service context from `hkask-services`
/// and user-configurable settings. The session takes over the terminal
/// (raw mode + alternate screen) and restores it on drop.
pub struct TuiSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    workspace: Workspace,
    layout_path: Option<std::path::PathBuf>,
    /// Tick rate for event polling (ms)
    tick_rate: Duration,
}

impl TuiSession {
    /// Create a new TUI session with the given service context.
    ///
    /// Initializes the terminal in raw mode + alternate screen,
    /// builds the workspace with a default layout (chat window).
    pub fn new(
        system: std::sync::Arc<dyn SystemBridge>,
        repl: std::sync::Arc<dyn ReplBridge>,
    ) -> anyhow::Result<Self> {
        let mut terminal = ratatui::init();
        terminal.clear()?;

        let workspace = Workspace::new(system, repl);

        Ok(Self {
            terminal,
            workspace,
            layout_path: None,
            tick_rate: Duration::from_millis(16),
        })
    }

    with_bridges!(tui_bridge_setter;
        settings_bridge, SettingsBridge, with_settings_bridge;
        session_bridge, SessionBridge, with_session_bridge;
        tool_invoke_bridge, ToolInvokeBridge, with_tool_invoke_bridge
    );

    /// Run the main event loop. Blocks until the user quits.
    /// Set the layout path for per-agent workspace persistence.
    pub fn with_layout_path(mut self, path: std::path::PathBuf) -> Self {
        self.layout_path = Some(path);
        self
    }

    fn restore_layout(&mut self) {
        if let Some(ref path) = self.layout_path {
            if let Some(layout) = crate::tui::layout::load(path) {
                self.workspace.restore_layout(&layout);
            }
        }
    }

    fn save_layout(&self) {
        if let Some(ref path) = self.layout_path {
            let layout = self.workspace.extract_layout();
            let _ = crate::tui::layout::save(path, &layout);
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        // Restore saved layout if available
        self.restore_layout();

        while !self.workspace.should_quit {
            // Render current frame
            self.terminal.draw(|f| self.workspace.render(f))?;

            // Poll for events
            if event::poll(self.tick_rate)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key);
                    }
                    Event::Resize(_, _) => {
                        // ratatui handles resize on next draw automatically
                    }
                    _ => {}
                }
            }

            // Tick workspace for background updates (Regulation polling, etc.)
            self.workspace.tick();
        }

        // Save layout on quit
        self.save_layout();
        Ok(())
    }

    /// Route a key event: global bindings, then focused window.
    fn handle_key(&mut self, key: KeyEvent) {
        // Global keybindings take precedence
        if self.workspace.handle_global_key(key) {
            return;
        }

        // Route to focused window
        self.workspace.handle_key(key);
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
