//! Shared base for MCP-scoped windows.
//!
//! Each MCP window (Kanban, Companies, Scenarios) provides a dedicated
//! pane where the user can interact with an MCP server's tools. Two
//! input paths are supported:
//!
//! - **Direct tool invocation** (`:tool_name args`) — calls the MCP tool
//!   directly via `ToolInvokeBridge`, bypassing the LLM. Fast, structured
//!   JSON results. Preserves OCAP governance (DelegationToken).
//! - **Scoped inference** (natural language) — runs inference scoped to
//!   the MCP server's tools via `start_scoped_inference`. The LLM acts as
//!   an intermediary that calls the appropriate MCP tools.
//!
//! `McpScopedWindow` is the concrete `Window` impl; `McpScopedState` holds
//! the shared state (input, messages, pending requests).

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::tui::repl_bridge::{
    InferenceRequestId, InferenceState, McpInvokeRequestId, McpInvokeState, ReplBridge,
    ToolInvokeBridge,
};
use crate::tui::text_cursor;
use crate::tui::window::{Window, WindowId, WindowKind, WorkspaceAction};

/// A message in the MCP window's conversation log.
#[derive(Debug, Clone)]
pub(crate) struct McpMessage {
    pub(crate) sender: McpSender,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum McpSender {
    User,
    Agent,
    System,
}

impl McpSender {
    fn prefix(&self) -> &'static str {
        match self {
            McpSender::User => "You",
            McpSender::Agent => "Agent",
            McpSender::System => "System",
        }
    }

    fn color(&self) -> Color {
        match self {
            McpSender::User => Color::Green,
            McpSender::Agent => Color::Cyan,
            McpSender::System => Color::Yellow,
        }
    }
}

/// Shared state for an MCP-scoped window.
pub(crate) struct McpScopedState {
    pub(crate) id: WindowId,
    pub(crate) kind: WindowKind,
    pub(crate) title: String,
    pub(crate) mcp_server: String,
    pub(crate) bridge: Arc<dyn ReplBridge>,
    pub(crate) tool_invoke_bridge: Option<Arc<dyn ToolInvokeBridge>>,
    pub(crate) messages: Vec<McpMessage>,
    pub(crate) input: String,
    pub(crate) cursor_pos: usize,
    pub(crate) scroll_offset: u16,
    pending_request: Option<InferenceRequestId>,
    pending_invoke: Option<(McpInvokeRequestId, String)>,
    spinner_frame: u8,
    pending_actions: Vec<WorkspaceAction>,
}

impl McpScopedState {
    pub(crate) fn new(
        id: WindowId,
        kind: WindowKind,
        title: &str,
        mcp_server: &str,
        bridge: Arc<dyn ReplBridge>,
        welcome: &str,
    ) -> Self {
        Self {
            id,
            kind,
            title: title.to_string(),
            mcp_server: mcp_server.to_string(),
            bridge,
            tool_invoke_bridge: None,
            messages: vec![McpMessage {
                sender: McpSender::System,
                content: welcome.to_string(),
            }],
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            pending_request: None,
            pending_invoke: None,
            spinner_frame: 0,
            pending_actions: Vec::new(),
        }
    }

    /// Set the tool invocation bridge for direct MCP tool calls.
    pub(crate) fn with_tool_invoke_bridge(mut self, bridge: Arc<dyn ToolInvokeBridge>) -> Self {
        self.tool_invoke_bridge = Some(bridge);
        self
    }

    fn add_message(&mut self, sender: McpSender, content: String) {
        self.messages.push(McpMessage { sender, content });
        self.scroll_offset = 0;
    }

    fn send_input(&mut self) {
        if self.pending_request.is_some() {
            return;
        }
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;
        if input.is_empty() {
            return;
        }

        // Check for local slash commands.
        if input.starts_with('/') {
            self.handle_slash(&input);
            return;
        }

        self.add_message(McpSender::User, input.clone());

        // Direct tool calls are opt-in via a ':' sigil prefix. Anything
        // else falls through to scoped inference (LLM round-trip).
        if input.starts_with(':') && self.try_direct_tool_invoke(&input) {
            return;
        }

        let req = self.bridge.start_scoped_inference(input, &self.mcp_server);
        self.pending_request = Some(req);
    }

    /// Try to interpret `input` as a direct MCP tool call.
    /// The caller has already verified a ':' prefix is present.
    /// Format (after ':'): `tool_name arg1=value1 arg2=value2` or `tool_name {json}`
    fn try_direct_tool_invoke(&mut self, input: &str) -> bool {
        // Strip the ':' sigil; the caller guarantees it is present.
        let rest = &input[1..];
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let tool_name = parts[0];
        let full_name = format!("{}/{}", self.mcp_server, tool_name);
        let args_str = parts.get(1).copied().unwrap_or("");

        // Parse args: try JSON first, then key=value pairs.
        let args = if args_str.starts_with('{') {
            serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}))
        } else if args_str.is_empty() {
            serde_json::json!({})
        } else {
            // Parse key=value pairs
            let mut map = serde_json::Map::new();
            for pair in args_str.split_whitespace() {
                if let Some((k, v)) = pair.split_once('=') {
                    // Try to parse as number, bool, or keep as string
                    let val = if let Ok(n) = v.parse::<i64>() {
                        serde_json::Value::from(n)
                    } else if let Ok(f) = v.parse::<f64>() {
                        serde_json::Value::from(f)
                    } else if v == "true" || v == "false" {
                        serde_json::Value::from(v == "true")
                    } else {
                        serde_json::Value::from(v)
                    };
                    map.insert(k.to_string(), val);
                }
            }
            serde_json::Value::Object(map)
        };

        let Some(ref bridge) = self.tool_invoke_bridge else {
            // No bridge configured — fall through to scoped inference.
            return false;
        };
        let req = bridge.start_mcp_tool_invoke(&self.mcp_server, tool_name, args);
        self.pending_invoke = Some((req, full_name));
        true
    }

    fn handle_slash(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let primary = parts
            .first()
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or("");

        match primary {
            "help" | "?" => {
                self.add_message(
                    McpSender::System,
                    format!(
                        "Commands: /help /close /clear /open <kind> /split h|v /focus\n\
                         This window is scoped to the {} MCP server.",
                        self.mcp_server
                    ),
                );
            }
            "close" => {
                self.pending_actions.push(WorkspaceAction::CloseFocused);
            }
            "clear" => {
                self.messages.clear();
                self.scroll_offset = 0;
            }
            "open" => {
                if let Some(kind_str) = parts.get(1) {
                    if let Some(kind) = WindowKind::parse_kind(kind_str) {
                        self.pending_actions.push(WorkspaceAction::OpenWindow(kind));
                        self.add_message(
                            McpSender::System,
                            format!("Opening {} window...", kind.default_title()),
                        );
                    } else {
                        self.add_message(
                            McpSender::System,
                            format!(
                                "Unknown window kind: {}. Try: chat kanban companies scenarios",
                                kind_str
                            ),
                        );
                    }
                } else {
                    self.add_message(
                        McpSender::System,
                        "Available: /open chat /open kanban /open companies /open scenarios".into(),
                    );
                }
            }
            "split" => {
                let dir = match parts.get(1).copied().unwrap_or("") {
                    "h" | "horizontal" => crate::tui::window::SplitDirection::Horizontal,
                    _ => crate::tui::window::SplitDirection::Vertical,
                };
                self.pending_actions.push(WorkspaceAction::Split(dir));
            }
            "focus" => {
                self.pending_actions.push(WorkspaceAction::FocusNext);
            }
            _ => {
                self.add_message(
                    McpSender::System,
                    format!(
                        "Unknown command: /{}. Type /help for available commands.",
                        primary
                    ),
                );
            }
        }
    }

    pub(crate) fn tick(&mut self) {
        // Poll scoped inference requests (LLM round-trip path).
        if let Some(req) = self.pending_request {
            let state = self.bridge.poll_inference(req);
            match state {
                InferenceState::Thinking => {
                    self.spinner_frame = self.spinner_frame.wrapping_add(1);
                }
                InferenceState::Done(result) => {
                    self.add_message(McpSender::Agent, result.text);
                    self.pending_request = None;
                }
                InferenceState::Idle => {
                    self.pending_request = None;
                }
            }
        }

        // Poll direct MCP tool invocations.
        if let Some((req, tool_name)) = self.pending_invoke.take() {
            let Some(ref bridge) = self.tool_invoke_bridge else {
                return;
            };
            let state = bridge.poll_mcp_tool_invoke(req);
            match state {
                McpInvokeState::Invoking => {
                    self.spinner_frame = self.spinner_frame.wrapping_add(1);
                    self.pending_invoke = Some((req, tool_name));
                }
                McpInvokeState::Done(value) => {
                    let formatted = format_json_result(&value);
                    self.add_message(McpSender::Agent, format!("{}\n{}", tool_name, formatted));
                }
                McpInvokeState::Error(err) => {
                    self.add_message(
                        McpSender::System,
                        format!("Error calling {}: {}", tool_name, err),
                    );
                }
                McpInvokeState::Idle => {
                    self.add_message(
                        McpSender::System,
                        format!("{} invocation cancelled.", tool_name),
                    );
                }
            }
        }
    }

    pub(crate) fn render(&self, f: &mut Frame, area: Rect, is_focused: bool) {
        if area.height < 3 || area.width < 10 {
            return;
        }

        // Layout: message log (top) + input line (bottom, 1 line)
        let input_h = 1u16;
        let log_h = area.height.saturating_sub(input_h);
        let log_area = Rect::new(area.x, area.y, area.width, log_h);
        let input_area = Rect::new(area.x, area.y + log_h, area.width, input_h);

        // Render message log
        let mut lines: Vec<Line> = Vec::new();
        for msg in &self.messages {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", msg.sender.prefix()),
                    Style::default()
                        .fg(msg.sender.color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(&msg.content),
            ]));
        }

        // Spinner if thinking or invoking
        if self.pending_request.is_some() || self.pending_invoke.is_some() {
            let spinner = match self.spinner_frame % 4 {
                0 => "⠋",
                1 => "⠙",
                2 => "⠹",
                _ => "⠸",
            };
            lines.push(Line::from(vec![
                Span::styled(
                    "Agent: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} thinking...", spinner),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        let log = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        f.render_widget(log, log_area);

        // Render input line
        let prompt_style = Style::default().fg(if is_focused {
            Color::Yellow
        } else {
            Color::DarkGray
        });
        let prompt_span = Span::styled("> ", prompt_style);

        let input_line = if is_focused {
            let (before, ch, after) = text_cursor::parts(&self.input, self.cursor_pos);
            Line::from(vec![
                prompt_span,
                Span::raw(before),
                Span::styled(
                    ch.map(|c| c.to_string()).unwrap_or_else(|| "|".into()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(after),
            ])
        } else {
            Line::from(vec![prompt_span, Span::raw(&self.input)])
        };
        f.render_widget(Paragraph::new(input_line), input_area);
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> bool {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Enter) => {
                self.send_input();
                true
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                if self.cursor_pos > 0 {
                    text_cursor::backspace(&mut self.input, &mut self.cursor_pos);
                }
                true
            }
            (KeyModifiers::NONE, KeyCode::Delete) => {
                text_cursor::delete(&mut self.input, self.cursor_pos);
                true
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                text_cursor::move_left(&self.input, &mut self.cursor_pos);
                true
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                text_cursor::move_right(&self.input, &mut self.cursor_pos);
                true
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                if self.scroll_offset > 0 {
                    self.scroll_offset -= 1;
                }
                true
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                self.scroll_offset += 1;
                true
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.cursor_pos = 0;
                true
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.cursor_pos = self.input.len();
                true
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) => {
                text_cursor::insert(&mut self.input, &mut self.cursor_pos, c);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn drain_actions(&mut self) -> Vec<WorkspaceAction> {
        std::mem::take(&mut self.pending_actions)
    }
}

/// Format a JSON tool result for display in the TUI.
/// Pretty-prints objects/arrays, shows scalars inline.
/// Recursion is capped at depth 5 and output is truncated to 5000 chars
/// to prevent stack overflow and UI flooding from deeply nested or huge results.
fn format_json_result(value: &serde_json::Value) -> String {
    format_json_result_depth(value, 0)
}

fn format_json_result_depth(value: &serde_json::Value, depth: u8) -> String {
    if depth > 5 {
        return "[...]".to_string();
    }
    let result = match value {
        serde_json::Value::String(s) => {
            if depth < 5 {
                if let Ok(inner) = serde_json::from_str::<serde_json::Value>(s) {
                    return format_json_result_depth(&inner, depth + 1);
                }
            }
            s.clone()
        }
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    };
    // Truncate very long output.
    const MAX_LEN: usize = 5000;
    if result.len() > MAX_LEN {
        // Find a safe UTF-8 boundary at or before MAX_LEN.
        let mut end = MAX_LEN;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &result[..end])
    } else {
        result
    }
}

/// A concrete `Window` backed by `McpScopedState`.
///
/// Thin wrapper that delegates every `Window` method to the shared
/// `McpScopedState`. Replaces the former per-server `KanbanWindow`,
/// `CompaniesWindow`, and `ScenariosWindow` newtypes.
pub struct McpScopedWindow {
    state: McpScopedState,
}

impl McpScopedWindow {
    pub fn new(
        id: WindowId,
        kind: WindowKind,
        title: &str,
        mcp_server: &str,
        bridge: Arc<dyn ReplBridge>,
        welcome: &str,
    ) -> Self {
        Self {
            state: McpScopedState::new(id, kind, title, mcp_server, bridge, welcome),
        }
    }

    pub fn with_tool_invoke_bridge(mut self, bridge: Arc<dyn ToolInvokeBridge>) -> Self {
        self.state = self.state.with_tool_invoke_bridge(bridge);
        self
    }
}

impl Window for McpScopedWindow {
    fn id(&self) -> WindowId {
        self.state.id
    }

    fn title(&self) -> &str {
        &self.state.title
    }

    fn kind(&self) -> WindowKind {
        self.state.kind
    }

    fn render(&self, f: &mut Frame, area: Rect, is_focused: bool) {
        self.state.render(f, area, is_focused);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.state.handle_key(key)
    }

    fn tick(&mut self) {
        self.state.tick();
    }

    fn drain_actions(&mut self) -> Vec<WorkspaceAction> {
        self.state.drain_actions()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::test_util::MockReplBridge;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// A mock ToolInvokeBridge that captures the last invocation.
    struct CapturingToolBridge {
        last_server: std::sync::Mutex<String>,
        last_tool: std::sync::Mutex<String>,
        last_args: std::sync::Mutex<serde_json::Value>,
        call_count: AtomicU8,
    }

    impl CapturingToolBridge {
        fn new() -> Self {
            Self {
                last_server: std::sync::Mutex::new(String::new()),
                last_tool: std::sync::Mutex::new(String::new()),
                last_args: std::sync::Mutex::new(serde_json::json!({})),
                call_count: AtomicU8::new(0),
            }
        }

        fn last_tool(&self) -> String {
            self.last_tool.lock().unwrap().clone()
        }

        fn last_args(&self) -> serde_json::Value {
            self.last_args.lock().unwrap().clone()
        }
    }

    impl ToolInvokeBridge for CapturingToolBridge {
        fn start_mcp_tool_invoke(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> McpInvokeRequestId {
            *self.last_server.lock().unwrap() = server.to_string();
            *self.last_tool.lock().unwrap() = tool.to_string();
            *self.last_args.lock().unwrap() = args;
            self.call_count.fetch_add(1, Ordering::Relaxed);
            McpInvokeRequestId::new()
        }
        fn poll_mcp_tool_invoke(&self, _request: McpInvokeRequestId) -> McpInvokeState {
            McpInvokeState::Idle
        }
    }

    fn make_state(server: &str) -> McpScopedState {
        let bridge: Arc<dyn ReplBridge> = Arc::new(MockReplBridge {
            agent_name: "test".into(),
            model_name: "test".into(),
        });
        McpScopedState::new(
            WindowId(uuid::Uuid::new_v4()),
            WindowKind::Kanban,
            "Kanban",
            server,
            bridge,
            "test",
        )
    }

    fn make_state_with_bridge(server: &str) -> (McpScopedState, Arc<CapturingToolBridge>) {
        let bridge: Arc<dyn ReplBridge> = Arc::new(MockReplBridge {
            agent_name: "test".into(),
            model_name: "test".into(),
        });
        let tool_bridge = Arc::new(CapturingToolBridge::new());
        let mut state = McpScopedState::new(
            WindowId(uuid::Uuid::new_v4()),
            WindowKind::Kanban,
            "Kanban",
            server,
            bridge,
            "test",
        );
        state = state.with_tool_invoke_bridge(tool_bridge.clone() as Arc<dyn ToolInvokeBridge>);
        (state, tool_bridge)
    }

    #[test]
    fn sigil_prefix_triggers_tool_invoke() {
        let (mut state, tool_bridge) = make_state_with_bridge("kanban");
        state.input = ":kanban_board_list".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        assert!(
            state.pending_invoke.is_some(),
            "pending_invoke should be set"
        );
        assert!(
            state.pending_request.is_none(),
            "pending_request should NOT be set"
        );
        assert_eq!(tool_bridge.last_tool(), "kanban_board_list");
        assert_eq!(tool_bridge.last_args(), serde_json::json!({}));
    }

    #[test]
    fn non_sigil_falls_through_to_scoped_inference() {
        let (mut state, _) = make_state_with_bridge("kanban");
        state.input = "list my boards".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        assert!(
            state.pending_request.is_some(),
            "pending_request should be set"
        );
        assert!(
            state.pending_invoke.is_none(),
            "pending_invoke should NOT be set"
        );
    }

    #[test]
    fn sigil_with_key_value_args() {
        let (mut state, tool_bridge) = make_state_with_bridge("kanban");
        state.input = ":kanban_task_create board=main title=fix-bug priority=3".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        assert!(state.pending_invoke.is_some());
        assert_eq!(tool_bridge.last_tool(), "kanban_task_create");
        let args = tool_bridge.last_args();
        assert_eq!(args["board"], serde_json::json!("main"));
        assert_eq!(args["title"], serde_json::json!("fix-bug"));
        assert_eq!(args["priority"], serde_json::json!(3));
    }

    #[test]
    fn sigil_with_json_args() {
        let (mut state, tool_bridge) = make_state_with_bridge("kanban");
        state.input = ":kanban_task_create {\"board\": \"main\", \"count\": 5}".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        assert!(state.pending_invoke.is_some());
        assert_eq!(tool_bridge.last_tool(), "kanban_task_create");
        let args = tool_bridge.last_args();
        assert_eq!(args["board"], serde_json::json!("main"));
        assert_eq!(args["count"], serde_json::json!(5));
    }

    #[test]
    fn sigil_with_bool_arg() {
        let (mut state, tool_bridge) = make_state_with_bridge("kanban");
        state.input = ":kanban_board_list active=true archived=false".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        let args = tool_bridge.last_args();
        assert_eq!(args["active"], serde_json::json!(true));
        assert_eq!(args["archived"], serde_json::json!(false));
    }

    #[test]
    fn sigil_with_float_arg() {
        let (mut state, tool_bridge) = make_state_with_bridge("companies");
        state.input = ":stock_quote symbol=AAPL threshold=3.14".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        let args = tool_bridge.last_args();
        assert_eq!(args["symbol"], serde_json::json!("AAPL"));
        assert!(args["threshold"].is_f64());
    }

    #[test]
    fn no_tool_bridge_falls_through_to_inference() {
        let mut state = make_state("kanban");
        state.input = ":kanban_board_list".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        // Without tool_invoke_bridge, the ':' prefix try returns false,
        // and the input falls through to scoped inference.
        assert!(
            state.pending_request.is_some(),
            "should fall through to inference"
        );
        assert!(state.pending_invoke.is_none());
    }

    #[test]
    fn slash_command_intercepted_before_sigil() {
        let (mut state, _) = make_state_with_bridge("kanban");
        state.input = "/help".into();
        state.cursor_pos = state.input.len();
        state.send_input();
        assert!(state.pending_invoke.is_none());
        assert!(state.pending_request.is_none());
        // Should have added a help message
        assert!(state.messages.len() >= 2);
    }

    #[test]
    fn format_json_simple_string() {
        let result = format_json_result(&serde_json::json!("hello"));
        assert_eq!(result, "hello");
    }

    #[test]
    fn format_json_object() {
        let result = format_json_result(&serde_json::json!({"key": "value"}));
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
    }

    #[test]
    fn format_json_nested_string_parses() {
        let inner = serde_json::json!({"nested": true});
        let wrapped = serde_json::Value::String(inner.to_string());
        let result = format_json_result(&wrapped);
        assert!(result.contains("nested"));
        assert!(result.contains("true"));
    }

    #[test]
    fn format_json_recursion_capped() {
        // Create a deeply nested string-within-string: "\"\\\"...\\\"\""
        let mut val = serde_json::json!({"a": 1});
        for _ in 0..10 {
            val = serde_json::Value::String(val.to_string());
        }
        let result = format_json_result(&val);
        // Should hit depth cap and return "[...]" at some point
        assert!(result.contains("[...]") || result.len() < 100);
    }

    #[test]
    fn format_json_truncates_long_output() {
        // Create a very large JSON object
        let mut map = serde_json::Map::new();
        for i in 0..1000 {
            map.insert(
                format!("key_{i}"),
                serde_json::json!(format!("value_{}", i)),
            );
        }
        let val = serde_json::Value::Object(map);
        let result = format_json_result(&val);
        assert!(
            result.len() <= 5003,
            "output should be truncated (got {})",
            result.len()
        ); // ≤5000 safe UTF-8 boundary + "…" (3 bytes)
        assert!(result.ends_with('…'));
    }
}
