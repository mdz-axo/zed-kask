//! Window catalog and factory — canonical list of window kinds and constructors.

use std::sync::Arc;

use crate::tui::repl_bridge::{
    ReplBridge, SessionBridge, SettingsBridge, SystemBridge, ToolInvokeBridge,
};
use crate::tui::window::{Window, WindowId, WindowKind};
use crate::tui::windows::McpScopedWindow;
use crate::tui::windows::chat::ChatWindow;

pub fn window_kind_from_title(title: &str) -> Option<WindowKind> {
    WindowKind::parse_kind(title)
}

/// All bridge dependencies for window construction.
pub(crate) struct WindowBridges {
    pub system_bridge: Arc<dyn SystemBridge>,
    pub repl_bridge: Arc<dyn ReplBridge>,
    pub settings_bridge: Option<Arc<dyn SettingsBridge>>,
    pub session_bridge: Option<Arc<dyn SessionBridge>>,
    pub tool_invoke_bridge: Option<Arc<dyn ToolInvokeBridge>>,
}

pub(crate) fn create_window(
    kind: WindowKind,
    id: WindowId,
    ctx: &WindowBridges,
) -> Box<dyn Window> {
    let bridge = ctx.repl_bridge.clone();

    match kind {
        WindowKind::Chat => {
            let mut w = ChatWindow::new(
                id,
                ctx.system_bridge.userpod_name(),
                ctx.system_bridge.model_name(),
                bridge,
            );
            if let Some(b) = ctx.settings_bridge.clone() {
                w = w.with_settings_bridge(b);
            }
            if let Some(b) = ctx.session_bridge.clone() {
                w = w.with_session_bridge(b);
            }
            Box::new(w)
        }
        WindowKind::Kanban => {
            let mut w = McpScopedWindow::new(
                id,
                WindowKind::Kanban,
                "Kanban",
                "kanban",
                bridge,
                "Kanban — task coordination. Type ':tool_name args' for direct tool calls, or a natural language query. Type /help for commands.",
            );
            if let Some(b) = ctx.tool_invoke_bridge.clone() {
                w = w.with_tool_invoke_bridge(b);
            }
            Box::new(w)
        }
        WindowKind::Companies => {
            let mut w = McpScopedWindow::new(
                id,
                WindowKind::Companies,
                "Companies",
                "companies",
                bridge,
                "Companies — financial data. Type ':tool_name args' for direct tool calls, or a natural language query. Type /help for commands.",
            );
            if let Some(b) = ctx.tool_invoke_bridge.clone() {
                w = w.with_tool_invoke_bridge(b);
            }
            Box::new(w)
        }
        WindowKind::Scenarios => {
            let mut w = McpScopedWindow::new(
                id,
                WindowKind::Scenarios,
                "Scenarios",
                "scenarios",
                bridge,
                "Scenarios — planning and forecasting. Type ':tool_name args' for direct tool calls, or a natural language query. Type /help for commands.",
            );
            if let Some(b) = ctx.tool_invoke_bridge.clone() {
                w = w.with_tool_invoke_bridge(b);
            }
            Box::new(w)
        }
    }
}
