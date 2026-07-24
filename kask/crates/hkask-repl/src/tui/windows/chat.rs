//! Chat window — the primary AI interaction surface.
//!
//! Renders the conversation history (messages, tool calls, Regulation alerts)
//! and a mode-aware prompt input line. This is the TUI equivalent of
//! the rustyline REPL, with additional visual structure.
//!
//! # TuiMode State Machine
//!
//! The default mode is `Chat` — the user's input goes directly to inference.
//! The user can switch to `Curator` mode via `/curator` to talk to the
//! Curator daemon, or enter `Command` mode via `/` prefix for slash commands.
//!
//! ```text
//! Chat (default) ──/curator──▶ Curator ──/repl──▶ Chat
//! Chat ──/──▶ Command ──Esc/Enter──▶ Chat
//! ```
//!
//! Mode transitions emit `reg.tui.mode_switch { from, to }` spans.
//!
//! # Prompt Mode Prefixes (P12 Authenticated Host Mandate)
//!
//! - `REPL ▸` in cyan — normal chat mode (userpod agent) — default
//! - `CMD  ▸` in yellow — slash-command entry
//! - `CRTR ▸` in magenta — direct curator address
//!
//! # Window Management Slash Commands
//!
//! The Chat window can request workspace actions via `drain_actions()`.
//! Supported commands: `/open <kind>`, `/close`, `/split h|v`, `/focus`,
//! `/tab new|next|prev`, `/quit`. Global keybindings: Ctrl-W (window ops),
//! Ctrl-T (new tab), Ctrl-Tab/Ctrl-Shift-Tab (tab cycling).

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::repl_bridge::{
    InferenceRequestId, InferenceState, ReplBridge, SessionBridge, SettingsBridge,
};
use crate::tui::text_cursor;
use crate::tui::window::{Window, WindowId, WindowKind, WorkspaceAction};

/// The interaction mode for the chat window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiMode {
    /// Normal chat — user input goes to the agent
    Chat,
    /// Slash-command entry — input starts with '/'
    Command,
    /// Direct curator chat (P12.1 dual-presence)
    Curator,
}

impl TuiMode {
    /// Returns the prompt prefix for this mode.
    pub fn prompt_prefix(&self) -> &'static str {
        match self {
            TuiMode::Chat => "REPL ▸ ",
            TuiMode::Command => "CMD  ▸ ",
            TuiMode::Curator => "CRTR ▸ ",
        }
    }

    /// Returns the prompt color for this mode.
    pub fn prompt_color(&self) -> Color {
        match self {
            TuiMode::Chat => Color::Cyan,
            TuiMode::Command => Color::Yellow,
            TuiMode::Curator => Color::Magenta,
        }
    }

    /// Determine the next mode based on input.
    /// Returns (new_mode, input_consumed).
    pub fn transition(self, input: &str) -> (Self, bool) {
        match self {
            TuiMode::Chat => {
                if input == "/curator" || input == "/curator chat" {
                    (TuiMode::Curator, true)
                } else if input.starts_with('/') {
                    (TuiMode::Command, false)
                } else {
                    (TuiMode::Chat, false)
                }
            }
            TuiMode::Command => {
                (TuiMode::Chat, true) // Command dispatched, return to chat
            }
            TuiMode::Curator => {
                if input == "/repl" || input == "/chat" {
                    (TuiMode::Chat, true)
                } else {
                    (TuiMode::Curator, false)
                }
            }
        }
    }
}

/// A single message in the chat history.
#[derive(Debug, Clone)]
pub struct TuiChatMessage {
    /// Who sent this message
    pub sender: MessageSender,
    /// The message content (may contain markdown)
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageSender {
    /// The human user
    User,
    /// The AI agent/userpod
    Agent(String),
    /// The Curator daemon (dual-presence)
    Curator,
    /// A Regulation alert (interleaved into the stream)
    RegAlert,
    /// Tool execution result
    Tool(String),
}

/// The chat window — conversation history + input.
pub struct ChatWindow {
    id: WindowId,
    userpod_name: String,
    model: String,
    /// Current interaction mode
    mode: TuiMode,
    /// Conversation message history
    messages: Vec<TuiChatMessage>,
    /// Current input buffer
    input: String,
    /// Cursor position in input
    cursor_pos: usize,
    /// Scroll position in message history
    scroll_offset: u16,
    /// Bridge to the inference engine
    bridge: Arc<dyn ReplBridge>,
    /// Optional settings/model mutation surface. When unset, `/model` and
    /// `/repl` fall back to stub messages (tests / minimal hosts).
    settings_bridge: Option<Arc<dyn SettingsBridge>>,
    session_bridge: Option<Arc<dyn SessionBridge>>,
    /// Request owned by this window, if inference is active.
    pending_request: Option<InferenceRequestId>,
    /// Current inference state for async polling
    inference_state: InferenceState,
    /// Spinner frame counter for "Thinking..." animation
    spinner_frame: u8,
    /// Partial streaming text shown during inference
    streaming_partial: String,
    /// Pending workspace actions (drained by Workspace::tick).
    /// A Vec so multi-command lines like `/open kanban /split v` work.
    pending_actions: Vec<WorkspaceAction>,
}

impl ChatWindow {
    pub fn new(id: WindowId, userpod_name: &str, model: &str, bridge: Arc<dyn ReplBridge>) -> Self {
        let mut messages = Vec::new();
        messages.push(TuiChatMessage {
            sender: MessageSender::Agent(userpod_name.to_string()),
            content: format!(
                "hKask v{} — Agent: {} | Model: {} | Type /help for commands, /curator for Curator",
                env!("CARGO_PKG_VERSION"),
                userpod_name,
                model
            ),
        });

        Self {
            id,
            userpod_name: userpod_name.to_string(),
            model: model.to_string(),
            // Default mode is Chat — user input goes directly to inference.
            // Curator is available via /curator slash command.
            mode: TuiMode::Chat,
            messages,
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            bridge,
            settings_bridge: None,
            session_bridge: None,
            pending_request: None,
            inference_state: InferenceState::Idle,
            spinner_frame: 0,
            streaming_partial: String::new(),
            pending_actions: Vec::new(),
        }
    }

    /// Attach the settings/model mutation surface. Required for `/model`
    /// and `/repl` to actually mutate state; without it they emit a stub.
    pub fn with_settings_bridge(mut self, bridge: Arc<dyn SettingsBridge>) -> Self {
        self.settings_bridge = Some(bridge);
        self
    }

    pub fn with_session_bridge(mut self, bridge: Arc<dyn SessionBridge>) -> Self {
        self.session_bridge = Some(bridge);
        self
    }

    /// Add a message to the history and auto-scroll to bottom.
    /// Drops oldest messages when exceeding MAX_MESSAGES.
    fn add_message(&mut self, sender: MessageSender, content: String) {
        const MAX_MESSAGES: usize = 500;
        if self.messages.len() >= MAX_MESSAGES {
            let excess = self.messages.len() - MAX_MESSAGES + 1;
            self.messages.drain(0..excess);
            // Adjust scroll offset so the user doesn't jump
            self.scroll_offset = self.scroll_offset.saturating_sub(excess as u16);
        }
        self.messages.push(TuiChatMessage { sender, content });
        // Only auto-scroll to bottom if user was already at the bottom
        if self.scroll_offset == 0 {
            // already at bottom, stay there
        }
    }

    /// Export chat history to a markdown file.
    fn export_to_markdown(&mut self) {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let filename = format!("kask-chat-{}.md", ts);
        let mut md = String::new();
        md.push_str(&format!("# hKask Chat Export — {}\n\n", ts));
        md.push_str(&format!(
            "**Agent:** {} | **Model:** {}\n\n---\n\n",
            self.userpod_name, self.model
        ));
        for msg in &self.messages {
            match &msg.sender {
                MessageSender::User => md.push_str(&format!("**You:** {}\n\n", msg.content)),
                MessageSender::Agent(name) => {
                    md.push_str(&format!("**{}:** {}\n\n", name, msg.content))
                }
                MessageSender::Curator => md.push_str(&format!("**Curator:** {}\n\n", msg.content)),
                MessageSender::RegAlert => md.push_str(&format!("> ⚠ *{}*\n\n", msg.content)),
                MessageSender::Tool(name) => {
                    md.push_str(&format!("```\n[{}]\n{}\n```\n\n", name, msg.content))
                }
            }
        }
        match std::fs::write(&filename, &md) {
            Ok(_) => {
                self.add_message(
                    MessageSender::RegAlert,
                    format!("Chat exported to {}", filename),
                );
            }
            Err(e) => {
                self.add_message(MessageSender::RegAlert, format!("Export failed: {}", e));
            }
        }
    }

    /// Execute a slash command (REPL-compatible subset).
    fn execute_slash_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let primary = parts
            .first()
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or("");

        match primary {
            "help" | "?" => {
                self.add_message(
                    MessageSender::RegAlert,
                    "Commands: /help /quit /clear /model /status /repl /mcp /agent /curator /export /ask /fusion /thread\n             /open <kind> /close /split h|v /focus /tab new|next|prev".into(),
                );
            }
            "quit" | "exit" => {
                self.pending_actions.push(WorkspaceAction::Quit);
                self.add_message(MessageSender::RegAlert, "Quitting TUI...".into());
            }
            "open" => {
                if let Some(kind_str) = parts.get(1) {
                    if let Some(kind) = WindowKind::parse_kind(kind_str) {
                        self.pending_actions.push(WorkspaceAction::OpenWindow(kind));
                        self.add_message(
                            MessageSender::RegAlert,
                            format!("Opening {} window...", kind.default_title()),
                        );
                    } else {
                        self.add_message(
                            MessageSender::RegAlert,
                            format!(
                                "Unknown window kind: {}. Available: chat kanban companies scenarios",
                                kind_str
                            ),
                        );
                    }
                } else {
                    self.add_message(
                        MessageSender::RegAlert,
                        "Available windows: /open chat  /open kanban  /open companies  /open scenarios".into(),
                    );
                }
            }
            "close" => {
                self.pending_actions.push(WorkspaceAction::CloseFocused);
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
            "tab" => match parts.get(1).copied().unwrap_or("") {
                "new" => {
                    let name = parts.get(2).map(|s| s.to_string());
                    self.pending_actions.push(WorkspaceAction::NewTab(name));
                }
                "next" => {
                    self.pending_actions.push(WorkspaceAction::NextTab);
                }
                "prev" => {
                    self.pending_actions.push(WorkspaceAction::PrevTab);
                }
                _ => {
                    self.add_message(
                        MessageSender::RegAlert,
                        "Usage: /tab new [name] | /tab next | /tab prev".into(),
                    );
                }
            },
            "clear" => {
                self.messages.clear();
                self.scroll_offset = 0;
            }
            "model" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match sub {
                    "list" => match self.settings_bridge.as_ref() {
                                    Some(b) => match b.list_models() {
                                        Ok(models) if models.is_empty() => self.add_message(
                                            MessageSender::RegAlert,
                                            "No models found — no providers reachable.".into(),
                                        ),
                                        Ok(models) => {
                                            let mut lines = String::new();
                                            lines.push_str(&format!("Available models ({}):\n", models.len()));
                                            for m in &models {
                                                lines.push_str(&format!(
                                                    "  {}  {}  {}  {}\n",
                                                    m.name,
                                                    m.family.as_deref().unwrap_or("-"),
                                                    m.parameter_size.as_deref().unwrap_or("-"),
                                                    m.size_bytes
                                                        .map(|s| format!("{:.1} GB", s as f64 / 1_073_741_824.0))
                                                        .unwrap_or_else(|| "-".to_string()),
                                                ));
                                            }
                                            lines.push_str("Use /model <name> to switch.");
                                            self.add_message(MessageSender::RegAlert, lines);
                                        }
                                        Err(e) => self.add_message(
                                            MessageSender::RegAlert,
                                            format!("No models found — error listing models: {}", e),
                                        ),
                                    },
                                    None => self.add_message(
                                        MessageSender::RegAlert,
                                        "Model listing unavailable in this host. Use `kask model list`.".into(),
                                    ),
                                },
                                "" => {
                                    // Show current model + real context pressure (not a fake token count).
                                    let pressure = self.bridge.context_pressure();
                                    self.add_message(
                                        MessageSender::RegAlert,
                                        format!(
                                            "Current model: {} | Context: {:.0}% used",
                                            self.model,
                                            pressure * 100.0,
                                        ),
                                    );
                                    self.add_message(
                                        MessageSender::RegAlert,
                                        "Use /model <name> to switch, /model list to browse.".into(),
                                    );
                                }
                                _ => match self.settings_bridge.as_ref() {
                                    Some(b) => {
                                        let result = b.set_model(sub);
                                        self.model = result.resolved_name.clone();
                                        let mut text = format!("Model set to: {}", result.resolved_name);
                                        if !result.detail.is_empty() {
                                            text.push('\n');
                                            text.push_str(&result.detail);
                                        }
                                        self.add_message(MessageSender::RegAlert, text);
                                    }
                                    None => self.add_message(
                                        MessageSender::RegAlert,
                                        format!(
                                            "Model switch to '{}' requested. Use `kask chat --model {}` to change models.",
                                            sub,
                                            sub,
                                        ),
                                    ),
                                },
                            }
            }
            "status" => {
                let gas = self.bridge.gas_remaining();
                let cap = self.bridge.gas_cap();
                let alerts = self.bridge.reg_alert_count();
                let ctx = self.bridge.context_pressure();
                let (mcp_loaded, mcp_total) = self.bridge.mcp_status();
                self.add_message(
                    MessageSender::RegAlert,
                    format!(
                        "Agent: {} | Model: {} | Gas: {}/{} ({:.0}%) | Regulation alerts: {} | Context: {:.0}% | MCP: {}/{}",
                        self.userpod_name, self.bridge.model_name(),
                        gas, cap, if cap > 0 { (gas as f64 / cap as f64) * 100.0 } else { 100.0 },
                        alerts, ctx * 100.0, mcp_loaded, mcp_total,
                    ),
                );
            }
            "mcp" => {
                let (loaded, total) = self.bridge.mcp_status();
                self.add_message(
                    MessageSender::RegAlert,
                    format!(
                        "MCP Servers: {}/{} loaded. Use `kask mcp` CLI for full management.",
                        loaded, total
                    ),
                );
            }
            "repl" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match self.settings_bridge.as_ref() {
                    Some(b) => match sub {
                        "" | "show" | "status" => {
                            self.add_message(MessageSender::RegAlert, b.settings_display());
                        }
                        "set" => {
                            // parts: ["/repl", "set", <key>, <value...>]
                            let key = parts.get(2).copied().unwrap_or("");
                            let value = parts[3..].join(" ");
                            if key.is_empty() {
                                self.add_message(
                                    MessageSender::RegAlert,
                                    "Usage: /repl set <key> <value>".into(),
                                );
                            } else {
                                match b.set_setting(key, &value) {
                                    Ok(msg) => self.add_message(MessageSender::RegAlert, msg),
                                    Err(e) => self.add_message(
                                        MessageSender::RegAlert,
                                        format!("Error: {}", e),
                                    ),
                                }
                            }
                        }
                        _ => self.add_message(
                            MessageSender::RegAlert,
                            "REPL settings: /repl show | /repl set <key> <value>".into(),
                        ),
                    },
                    None => self.add_message(
                        MessageSender::RegAlert,
                        "REPL settings unavailable in this host. Use `kask repl` to manage settings.".into(),
                    ),
                }
            }
            "agent" => {
                let sub = parts.get(1).copied().unwrap_or("");
                match self.session_bridge.as_ref() {
                    Some(b) => {
                        if sub.is_empty() {
                            self.add_message(
                                MessageSender::RegAlert,
                                format!("Current agent: {}", b.current_agent()),
                            );
                        } else {
                            self.add_message(
                                MessageSender::RegAlert,
                                "No switching — one userpod per user.".to_string(),
                            );
                        }
                    }
                    None => self.add_message(
                        MessageSender::RegAlert,
                        format!(
                            "Current agent: {} (agent info unavailable in this host)",
                            self.userpod_name
                        ),
                    ),
                }
            }
            "agents" | "ls" => match self.session_bridge.as_ref() {
                Some(b) => self.add_message(MessageSender::RegAlert, b.list_agents_display()),
                None => self.add_message(
                    MessageSender::RegAlert,
                    "Agent listing unavailable in this host. Use `kask agents`.".into(),
                ),
            },
            "history" | "hist" => match self.session_bridge.as_ref() {
                Some(b) => self.add_message(MessageSender::RegAlert, b.history_display()),
                None => self.add_message(
                    MessageSender::RegAlert,
                    "History unavailable in this host. Use `kask history`.".into(),
                ),
            },
            "curator" => {
                self.mode = TuiMode::Curator;
                self.add_message(
                    MessageSender::RegAlert,
                    "Curator mode active. Type /repl to switch to userpod chat.".into(),
                );
            }
            "export" => {
                self.export_to_markdown();
            }
            _ => {
                // Delegate to the bridge — thin dispatch (Cline pattern).
                // The bridge handles what it can and returns guidance for the rest.
                let result = self.bridge.handle_command(primary);
                self.add_message(MessageSender::RegAlert, result.text);
                if result.should_quit {
                    self.pending_actions.push(WorkspaceAction::Quit);
                }
            }
        }
    }

    /// Send the current input as a user message.
    fn send_input(&mut self) {
        if self.pending_request.is_some() {
            return;
        }
        let input = std::mem::take(&mut self.input);
        self.cursor_pos = 0;

        if input.is_empty() {
            return;
        }

        // Check for mode transitions
        let (new_mode, consumed) = self.mode.transition(&input);
        let old_mode = self.mode;
        self.mode = new_mode;

        if old_mode != new_mode && consumed {
            tracing::info!(
                target: "hkask.tui.mode_switch",
                from = ?old_mode,
                to = ?new_mode,
                "REG"
            );
            self.add_message(
                MessageSender::RegAlert,
                format!("Mode: {:?} → {:?}", old_mode, new_mode),
            );
            return;
        }

        // Handle slash commands
        if input.starts_with('/') {
            self.execute_slash_command(&input);
            // Return to the mode that was active before the command.
            // If we were in Curator mode, stay in Curator mode (not Chat).
            if self.mode == TuiMode::Command {
                self.mode = TuiMode::Chat;
            }
            return;
        }

        // Non-slash input: route based on mode.
        match self.mode {
            TuiMode::Curator => {
                // Send to Curator daemon via blocking call.
                self.add_message(MessageSender::User, input.clone());
                let reply = self.bridge.send_curator_message(&input);
                self.add_message(MessageSender::Curator, reply);
            }
            TuiMode::Chat => {
                // Normal chat message — use async inference
                self.add_message(MessageSender::User, input.clone());
                self.pending_request = Some(self.bridge.start_inference(input));
                self.inference_state = InferenceState::Thinking;
            }
            TuiMode::Command => {
                // Should not reach here — commands are handled above.
                self.mode = TuiMode::Chat;
            }
        }
    }
}

impl Window for ChatWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn title(&self) -> &str {
        match self.mode {
            TuiMode::Chat => "Chat",
            TuiMode::Command => "Chat [CMD]",
            TuiMode::Curator => "Curator",
        }
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Chat
    }

    fn render(&self, f: &mut Frame, area: Rect, is_focused: bool) {
        // Guard: skip rendering on degenerate areas from deep splits.
        if area.height < 5 {
            return;
        }
        // Split area: message history (fill) | input line (3)
        let vert = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(1),
                ratatui::layout::Constraint::Length(3),
            ])
            .split(area);

        self.render_messages(f, vert[0]);
        self.render_input(f, vert[1], is_focused);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.send_input();
                true
            }
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return false;
                }
                text_cursor::insert(&mut self.input, &mut self.cursor_pos, c);
                true
            }
            KeyCode::Backspace => {
                text_cursor::backspace(&mut self.input, &mut self.cursor_pos);
                true
            }
            KeyCode::Delete => {
                text_cursor::delete(&mut self.input, self.cursor_pos);
                true
            }
            KeyCode::Left => {
                text_cursor::move_left(&self.input, &mut self.cursor_pos);
                true
            }
            KeyCode::Right => {
                text_cursor::move_right(&self.input, &mut self.cursor_pos);
                true
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
                true
            }
            KeyCode::End => {
                self.cursor_pos = self.input.len();
                true
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(5);
                true
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
                true
            }
            KeyCode::Esc => {
                if self.mode == TuiMode::Command {
                    self.mode = TuiMode::Chat;
                }
                // Clear input on Esc in any mode
                self.input.clear();
                self.cursor_pos = 0;
                true
            }
            _ => false,
        }
    }

    fn can_close(&self) -> bool {
        // At least one chat window should remain open
        true
    }

    fn on_focus(&mut self) {
        // Nothing needed on focus gain for now
    }

    fn on_blur(&mut self) {
        // Nothing needed on focus loss for now
    }

    fn tick(&mut self) {
        let Some(request) = self.pending_request else {
            return;
        };
        let state = self.bridge.poll_inference(request);
        match state {
            InferenceState::Done(result) => {
                if result.budget_exhausted {
                    self.add_message(
                        MessageSender::RegAlert,
                        "Gas budget exhausted — turn blocked by cybernetic regulator.".into(),
                    );
                } else {
                    let sender = match self.mode {
                        TuiMode::Curator => MessageSender::Curator,
                        _ => MessageSender::Agent(self.userpod_name.clone()),
                    };
                    self.add_message(sender, result.text.clone());
                    if result.iterations > 1 {
                        self.add_message(
                            MessageSender::Tool("usage".into()),
                            format!(
                                "{} tokens ({} prompt + {} completion) across {} iterations — gas: {}",
                                result.total_tokens, result.prompt_tokens,
                                result.completion_tokens, result.iterations, result.gas_cost,
                            ),
                        );
                    }
                }
                self.pending_request = None;
                self.inference_state = InferenceState::Idle;
            }
            InferenceState::Thinking => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
                self.streaming_partial = self.bridge.streaming_text(request);
                self.inference_state = InferenceState::Thinking;
            }
            InferenceState::Idle => {
                self.pending_request = None;
                self.inference_state = InferenceState::Idle;
            }
        }
    }

    fn drain_actions(&mut self) -> Vec<WorkspaceAction> {
        std::mem::take(&mut self.pending_actions)
    }
}

impl ChatWindow {
    /// Render the scrollable message history.
    fn render_messages(&self, f: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        // Show messages from newest to oldest (or scrolled)
        let visible = self.messages.iter().rev().skip(self.scroll_offset as usize);

        for msg in visible {
            let prefix = match &msg.sender {
                MessageSender::User => Span::styled(
                    "You ▸ ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                MessageSender::Agent(name) => Span::styled(
                    format!("{} ▸ ", name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                MessageSender::Curator => Span::styled(
                    "Curator ▸ ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                MessageSender::RegAlert => Span::styled(
                    "── Regulation: ",
                    Style::default()
                        .fg(Color::Rgb(183, 145, 99)) // Richmond Gold
                        .add_modifier(Modifier::BOLD),
                ),
                MessageSender::Tool(name) => Span::styled(
                    format!("[tool:{}] ", name),
                    Style::default().fg(Color::Yellow),
                ),
            };

            // Split content into lines and prepend prefix to first line
            for (i, content_line) in msg.content.lines().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        prefix.clone(),
                        Span::raw(content_line.to_string()),
                    ]));
                } else {
                    lines.push(Line::from(Span::raw(format!("       {}", content_line))));
                }
            }

            // Add a blank line between messages
            lines.push(Line::from(""));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "Type a message to begin. /help for commands.",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // Show streaming text during inference
        if matches!(self.inference_state, InferenceState::Thinking)
            && !self.streaming_partial.is_empty()
        {
            let prefix = Span::styled(
                format!("{} ▸ ", self.userpod_name),
                Style::default().fg(Color::Cyan).bold(),
            );
            for line in self.streaming_partial.lines() {
                lines.push(Line::from(vec![
                    prefix.clone(),
                    Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
                ]));
            }
        }

        let messages = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(messages, area);
    }

    /// Render the mode-aware prompt input line.
    fn render_input(&self, f: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(border_style);

        let inner = block.inner(area);
        f.render_widget(block, area);

        // Input text with cursor
        let input_style = if is_focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        // Rebuild the line properly
        let mut final_spans = Vec::new();

        // Show thinking spinner during inference
        if matches!(self.inference_state, InferenceState::Thinking) {
            let spinners = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let s = spinners[self.spinner_frame as usize % spinners.len()];
            final_spans.push(Span::styled(
                format!(" {} Thinking... ", s),
                Style::default().fg(Color::Yellow),
            ));
        } else {
            final_spans.push(Span::styled(
                if self.mode == TuiMode::Chat {
                    format!(
                        "{}>> ",
                        crate::display::model_abbrev(self.bridge.model_name())
                    )
                } else {
                    self.mode.prompt_prefix().to_string()
                },
                Style::default()
                    .fg(self.mode.prompt_color())
                    .add_modifier(Modifier::BOLD),
            ));
        }

        if is_focused && !self.input.is_empty() {
            let (before, at, after) = text_cursor::parts(&self.input, self.cursor_pos);
            final_spans.push(Span::styled(before.to_string(), input_style));

            if let Some(at) = at {
                final_spans.push(Span::styled(
                    at.to_string(),
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
                if !after.is_empty() {
                    final_spans.push(Span::styled(after.to_string(), input_style));
                }
            } else {
                final_spans.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
            }
        } else {
            final_spans.push(Span::styled(self.input.clone(), input_style));
            if is_focused {
                // Show cursor at end
                final_spans.push(Span::styled(
                    " ",
                    Style::default().fg(Color::Black).bg(Color::Cyan),
                ));
            }
        }

        let prompt_widget = Paragraph::new(Line::from(final_spans));
        f.render_widget(prompt_widget, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_chat_to_command_on_slash() {
        let (mode, consumed) = TuiMode::Chat.transition("/help");
        assert_eq!(mode, TuiMode::Command);
        assert!(!consumed);
    }

    #[test]
    fn mode_chat_to_curator() {
        let (mode, consumed) = TuiMode::Chat.transition("/curator");
        assert_eq!(mode, TuiMode::Curator);
        assert!(consumed);
    }

    #[test]
    fn mode_chat_stays_on_plain_text() {
        let (mode, _) = TuiMode::Chat.transition("hello world");
        assert_eq!(mode, TuiMode::Chat);
    }

    #[test]
    fn mode_command_returns_to_chat() {
        let (mode, consumed) = TuiMode::Command.transition("anything");
        assert_eq!(mode, TuiMode::Chat);
        assert!(consumed);
    }

    #[test]
    fn mode_curator_to_chat_on_repl() {
        let (mode, consumed) = TuiMode::Curator.transition("/repl");
        assert_eq!(mode, TuiMode::Chat);
        assert!(consumed);
    }

    #[test]
    fn mode_curator_stays_on_plain_text() {
        let (mode, _) = TuiMode::Curator.transition("message to curator");
        assert_eq!(mode, TuiMode::Curator);
    }

    #[test]
    fn prompt_prefixes_are_distinct() {
        assert_ne!(
            TuiMode::Chat.prompt_prefix(),
            TuiMode::Command.prompt_prefix()
        );
        assert_ne!(
            TuiMode::Chat.prompt_prefix(),
            TuiMode::Curator.prompt_prefix()
        );
        assert_ne!(
            TuiMode::Command.prompt_prefix(),
            TuiMode::Curator.prompt_prefix()
        );
    }

    #[test]
    fn prompt_colors_are_visible() {
        let chat_color = TuiMode::Chat.prompt_color();
        let cmd_color = TuiMode::Command.prompt_color();
        let curator_color = TuiMode::Curator.prompt_color();
        assert_ne!(chat_color, cmd_color);
        assert_ne!(chat_color, curator_color);
        assert_ne!(cmd_color, curator_color);
    }

    fn make_window_id() -> WindowId {
        WindowId(uuid::Uuid::new_v4())
    }

    #[test]
    fn model_command_switches_model_via_settings_bridge() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let settings = crate::tui::test_util::mock_settings_bridge();
        let mut chat = ChatWindow::new(make_window_id(), "test-agent", "old-model", repl)
            .with_settings_bridge(settings);
        assert_eq!(chat.model, "old-model");

        chat.execute_slash_command("/model new-model");

        assert_eq!(
            chat.model, "new-model",
            "/model must update ChatWindow.model"
        );
        let last = chat
            .messages
            .last()
            .expect("a confirmation message was added");
        assert!(
            last.content.contains("Model set to: new-model"),
            "got: {}",
            last.content
        );
    }

    #[test]
    fn model_no_arg_shows_real_pressure_not_fake_tokens() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let mut chat = ChatWindow::new(make_window_id(), "test-agent", "m", repl);

        chat.execute_slash_command("/model");

        let status = chat
            .messages
            .iter()
            .find(|m| m.content.contains("Current model: m"))
            .expect("a status message with the current model was added");
        assert!(
            status.content.contains("% used"),
            "/model must show real context pressure %, not a fake token count; got: {}",
            status.content
        );
    }

    #[test]
    fn repl_show_renders_settings_via_settings_bridge() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let settings = crate::tui::test_util::mock_settings_bridge();
        let mut chat = ChatWindow::new(make_window_id(), "test-agent", "m", repl)
            .with_settings_bridge(settings);

        chat.execute_slash_command("/repl show");

        let last = chat.messages.last().expect("a settings display was added");
        assert!(
            last.content.contains("(settings unavailable in test mock)"),
            "/repl show must render the bridge's settings_display; got: {}",
            last.content
        );
    }

    #[test]
    fn repl_set_applies_setting_via_settings_bridge() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let settings = crate::tui::test_util::mock_settings_bridge();
        let mut chat = ChatWindow::new(make_window_id(), "test-agent", "m", repl)
            .with_settings_bridge(settings);

        chat.execute_slash_command("/repl set temperature 0.5");

        let last = chat.messages.last().expect("a confirmation was added");
        assert!(
            last.content.contains("temperature set to 0.5"),
            "/repl set <key> <value> must apply via set_setting and confirm; got: {}",
            last.content
        );
    }

    #[test]
    fn repl_set_without_settings_bridge_emits_stub() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let mut chat = ChatWindow::new(make_window_id(), "test-agent", "m", repl);

        chat.execute_slash_command("/repl set temperature 0.5");

        let last = chat.messages.last().expect("a stub message was added");
        assert!(
            last.content.contains("unavailable in this host"),
            "/repl without a SettingsBridge must emit the stub, not crash; got: {}",
            last.content
        );
    }

    #[test]
    fn agent_with_name_shows_no_switching_message() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let session = crate::tui::test_util::mock_session_bridge();
        let mut chat =
            ChatWindow::new(make_window_id(), "old-agent", "m", repl).with_session_bridge(session);

        chat.execute_slash_command("/agent new-agent");

        let last = chat.messages.last().expect("a message was added");
        assert!(
            last.content.contains("No switching"),
            "/agent <name> must show no-switching message; got: {}",
            last.content
        );
    }

    #[test]
    fn agent_no_arg_shows_live_current_agent() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let session = crate::tui::test_util::mock_session_bridge();
        let mut chat =
            ChatWindow::new(make_window_id(), "stale-name", "m", repl).with_session_bridge(session);

        chat.execute_slash_command("/agent");

        let last = chat.messages.last().expect("a status was added");
        assert!(
            last.content.contains("Current agent: test-agent"),
            "/agent must show the live current_agent from SessionBridge, not the cached field; got: {}",
            last.content
        );
    }

    #[test]
    fn agents_command_renders_list_via_session_bridge() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let session = crate::tui::test_util::mock_session_bridge();
        let mut chat =
            ChatWindow::new(make_window_id(), "a", "m", repl).with_session_bridge(session);

        chat.execute_slash_command("/agents");

        let last = chat.messages.last().expect("an agent list was added");
        assert!(
            last.content.contains("(mock agents)"),
            "/agents must render list_agents_display; got: {}",
            last.content
        );
    }

    #[test]
    fn history_command_renders_via_session_bridge() {
        let (_sys, repl) = crate::tui::test_util::mock_bridges();
        let session = crate::tui::test_util::mock_session_bridge();
        let mut chat =
            ChatWindow::new(make_window_id(), "a", "m", repl).with_session_bridge(session);

        chat.execute_slash_command("/history");

        let last = chat.messages.last().expect("a history display was added");
        assert!(
            last.content.contains("(mock history)"),
            "/history must render history_display; got: {}",
            last.content
        );
    }
}
