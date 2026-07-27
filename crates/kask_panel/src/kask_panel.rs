//! Kask Panel — native GPUI center-pane item for per-MCP-server interaction (D10).
//!
//! A center-pane `Item` (opens in the same area as the terminal / editor / extensions
//! view — not a dock) that provides per-server access to the 10 built-in kask MCP
//! servers. The interaction model is a chat-like interface:
//! - **Regular text** → scoped inference (LLM acts as intermediary, calling
//!   only the selected server's tools)
//! - **`/tool_name args`** → direct tool invocation (bypasses LLM, calls the
//!   MCP tool directly via the OCAP-gated path)
//!
//! This mirrors the original hKask `McpScopedWindow`'s two input paths (Chat
//! tab + Data tab) unified into a single zed-idiomatic chat interface with
//! slash commands — the same pattern zed's agent panel uses.
//!
//! The panel uses global hooks (`set_tool_invoker` / `set_scoped_inference`)
//! so it doesn't depend on `kask_bridge`. The composition root injects the
//! bridge adapters.
//!
//! **Center-pane hosting:** `KaskPanel` implements `Item` (not `Panel`), so it
//! opens via `workspace.add_item_to_active_pane(...)` into the center pane
//! (the same surface that hosts the terminal, editor, and extensions view).
//! The `Toggle` action deploys a new panel if none is open, or focuses the
//! existing one. This is the same pattern `TerminalView` uses.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use editor::{CompletionProvider, Editor, EditorMode, MultiBuffer};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Task,
    WeakEntity, Window, prelude::*,
};
use language::Buffer;
use language_core::CodeLabel;
use project::lsp_store::CompletionDocumentation;
use project::{Completion, CompletionResponse, CompletionSource};
use serde_json::Value;
use text::ToOffset;
use ui::prelude::*;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem, TabContentParams},
    register_serializable_item,
};

use zed_actions::kask_panel::{
    Toggle, ToggleFocus, ToggleKanbanBoard, TogglePortfolioDashboard, ToggleScenarios,
};

mod kanban_view;
mod portfolio_view;
mod scenarios_view;

pub use kanban_view::KanbanBoardView;
pub use portfolio_view::PortfolioDashboardView;
pub use scenarios_view::ScenariosView;

/// The 10 built-in kask MCP server IDs (canonical source: `kask_bridge::BUILT_IN_MCP_SERVERS`).
const BUILT_IN_MCP_SERVERS: &[&str] = kask_bridge::BUILT_IN_MCP_SERVERS_IDS;

// ── Global hooks (same OnceLock pattern as D1/D5/D6) ──────────────────────

/// A chat message in the kask panel conversation.
#[derive(Clone, Debug)]
pub struct KaskMessage {
    pub role: KaskMessageRole,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KaskMessageRole {
    User,
    Assistant,
    Tool,
    System,
}

/// A tool descriptor for the completion provider (name + description).
#[derive(Clone, Debug)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
}

/// Trait for direct tool invocation (mirrors `hkask_capability::ToolPort`).
/// The bridge provides the implementation.
pub trait ToolInvoker: Send + Sync {
    /// Invoke a tool on a specific server. Returns the result as JSON text.
    fn invoke_tool(&self, server: &str, tool: &str, args: Value) -> Task<Result<String, String>>;

    /// List the tools exposed by a specific server (for completion / `/help`).
    /// Returns an empty vec if the server is not connected or introspection
    /// is unavailable.
    fn list_tools(&self, server: &str) -> Task<Result<Vec<ToolDescriptor>, String>>;
}

/// Trait for scoped inference (mirrors `hkask_types::InferencePort`).
/// The bridge provides the implementation.
pub trait ScopedInference: Send + Sync {
    /// Run scoped inference with only the selected server's tools in scope.
    fn infer(&self, server: &str, prompt: &str) -> Task<Result<String, String>>;
}

/// A snapshot of regulation/gas status for the status bar.
///
/// Mirrors the deleted hKask TUI's `StatusBar` — gas remaining, regulation
/// health, and alert counts. Fetched on a background task by the bridge's
/// `RegulationStatus` implementation and rendered in the panel's status bar.
#[derive(Clone, Debug, Default)]
pub struct RegulationSnapshot {
    /// Remaining gas for the panel's WebID (0 if no budget registered).
    pub gas_remaining: u64,
    /// Gas cap for the panel's WebID (0 if no budget registered).
    pub gas_cap: u64,
    /// Non-critical alert count (warnings).
    pub alert_count: usize,
    /// Critical alert count.
    pub critical_count: usize,
    /// Overall regulation health flag.
    pub healthy: bool,
}

/// Trait for fetching regulation status (mirrors the deleted hKask TUI's
/// `StatusBar`). The bridge provides the implementation.
pub trait RegulationStatus: Send + Sync {
    /// Fetch a status snapshot. Called on a background task; the result
    /// is rendered in the status bar.
    fn snapshot(&self) -> Task<RegulationSnapshot>;
}

static TOOL_INVOKER: OnceLock<Option<Arc<dyn ToolInvoker>>> = OnceLock::new();
static SCOPED_INFERENCE: OnceLock<Option<Arc<dyn ScopedInference>>> = OnceLock::new();
static REGULATION_STATUS: OnceLock<Option<Arc<dyn RegulationStatus>>> = OnceLock::new();

/// Inject the global tool invoker (composition root).
pub fn set_tool_invoker(invoker: Option<Arc<dyn ToolInvoker>>) {
    let _ = TOOL_INVOKER.set(invoker);
}

/// Inject the global scoped inference port (composition root).
pub fn set_scoped_inference(inference: Option<Arc<dyn ScopedInference>>) {
    let _ = SCOPED_INFERENCE.set(inference);
}

/// Inject the global regulation status provider (composition root).
pub fn set_regulation_status(status: Option<Arc<dyn RegulationStatus>>) {
    let _ = REGULATION_STATUS.set(status);
}

fn tool_invoker() -> Option<&'static Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.get().and_then(|opt| opt.as_ref())
}

pub(crate) fn kanban_tool_invoker() -> Option<&'static Arc<dyn ToolInvoker>> {
    tool_invoker()
}

fn scoped_inference() -> Option<&'static Arc<dyn ScopedInference>> {
    SCOPED_INFERENCE.get().and_then(|opt| opt.as_ref())
}

fn regulation_status() -> Option<&'static Arc<dyn RegulationStatus>> {
    REGULATION_STATUS.get().and_then(|opt| opt.as_ref())
}

// ── Center-pane Item ────────────────────────────────────────────────────

/// A per-server welcome message explaining what the server does and how to
/// interact with it. Mirrors the deleted hKask TUI's per-window welcome text.
fn server_welcome(server: &str) -> String {
    let description = match server {
        "codegraph" => "code structure query and traversal",
        "companies" => "company research and filings",
        "condenser" => "context condensation and summarization",
        "corpus" => "document corpus and QA generation",
        "curator" => "regulation cascade and algedonic signals",
        "kata-kanban" => "improvement kata board and task coordination",
        "media" => "image generation and media workflows",
        "research" => "web research and paper search",
        "scenarios" => "scenario planning and Wardley mapping",
        "training" => "LoRA training configuration and audit",
        _ => "MCP server",
    };
    format!(
        "{server} — {description}.\nType /tool_name args for direct invocation, or a natural language message for scoped inference.\nType /help for commands, /tools to list this server's tools."
    )
}

/// The kask panel — a center-pane `Item` for per-MCP-server chat + tool invocation.
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// Currently selected server (index into `BUILT_IN_MCP_SERVERS`).
    selected_server: usize,
    /// Per-server conversation history (preserved when switching servers).
    conversations: std::collections::HashMap<usize, Vec<KaskMessage>>,
    /// The message input editor.
    input_editor: Entity<Editor>,
    /// Whether a request is in progress.
    busy: bool,
    /// Spinner frame counter (animated while `busy`).
    spinner_frame: u8,
    /// Cached tool list for the selected server (for `/tools` and completion).
    cached_tools: Option<(usize, Vec<ToolDescriptor>)>,
    /// Latest regulation/gas snapshot for the status bar.
    regulation_snapshot: RegulationSnapshot,
    /// Whether a regulation status fetch is in progress (guards against
    /// overlapping fetches on every render).
    status_fetching: bool,
}

impl KaskPanel {
    /// Create a new kask panel.
    pub fn new(
        workspace: &Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let panel = cx.new(|cx| {
            let input_editor = cx.new(|cx| {
                let buffer = cx.new(|cx| Buffer::local("", cx));
                let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));
                let mut editor = Editor::new(
                    EditorMode::AutoHeight {
                        min_lines: 1,
                        max_lines: Some(5),
                    },
                    buffer,
                    None,
                    window,
                    cx,
                );
                editor.set_placeholder_text(
                    "Type a message, or /tool_name args for direct invocation",
                    window,
                    cx,
                );
                editor
            });

            Self {
                _workspace: workspace.weak_handle(),
                focus_handle: cx.focus_handle(),
                selected_server: 0,
                conversations: std::collections::HashMap::new(),
                input_editor,
                busy: false,
                spinner_frame: 0,
                cached_tools: None,
                regulation_snapshot: RegulationSnapshot::default(),
                status_fetching: false,
            }
        });
        // Wire the completion provider now that we have a weak handle.
        let weak = panel.downgrade();
        panel.update(cx, |panel, cx| {
            panel.input_editor.update(cx, |editor, cx| {
                editor
                    .set_completion_provider(Some(Rc::new(KaskToolCompletionProvider::new(weak))));
                cx.notify();
            });
        });
        panel
    }

    fn selected_server_name(&self) -> &'static str {
        BUILT_IN_MCP_SERVERS
            .get(self.selected_server)
            .copied()
            .unwrap_or("none")
    }

    /// Get the conversation for the currently selected server, initializing
    /// it with the welcome message on first access.
    fn current_messages(&mut self) -> &mut Vec<KaskMessage> {
        let index = self.selected_server;
        self.conversations.entry(index).or_insert_with(|| {
            vec![KaskMessage {
                role: KaskMessageRole::System,
                content: server_welcome(BUILT_IN_MCP_SERVERS.get(index).copied().unwrap_or("none")),
            }]
        })
    }

    /// Switch to a different server (called by the selector buttons).
    fn select_server(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_server = index;
        // Invalidate the tool cache — it's per-server.
        self.cached_tools = None;
        cx.notify();
    }

    fn submit_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }

        let text = self.input_editor.read(cx).text(cx).trim().to_string();
        if text.is_empty() {
            return;
        }

        // Clear the input.
        self.input_editor
            .update(cx, |editor, cx| editor.clear(window, cx));

        // Handle slash commands first (/help, /clear, /tools).
        if text.starts_with('/') {
            let parts: Vec<&str> = text.split_whitespace().collect();
            let command = parts
                .first()
                .map(|s| s.trim_start_matches('/'))
                .unwrap_or("");
            if matches!(command, "help" | "clear" | "tools") {
                self.handle_slash_command(command, cx);
                return;
            }
        }

        // Check if it's a direct tool invocation (/tool_name args).
        if let Some((tool, args)) = parse_tool_invocation(&text) {
            self.invoke_tool(tool, args, cx);
        } else {
            self.run_scoped_inference(&text, cx);
        }
    }

    /// Handle `/help`, `/clear`, `/tools` slash commands.
    fn handle_slash_command(&mut self, command: &str, cx: &mut Context<Self>) {
        match command {
            "help" => {
                self.current_messages().push(KaskMessage {
                    role: KaskMessageRole::System,
                    content: "Commands:\n  /help              — show this help\n  /clear             — clear the conversation\n  /tools             — list this server's tools\n  /tool_name args    — direct tool invocation (bypasses LLM)\n  <natural language> — scoped inference (LLM calls the server's tools)".to_string(),
                });
                cx.notify();
            }
            "clear" => {
                let server = self.selected_server_name().to_string();
                self.conversations.insert(
                    self.selected_server,
                    vec![KaskMessage {
                        role: KaskMessageRole::System,
                        content: format!("Cleared. {server} conversation reset."),
                    }],
                );
                cx.notify();
            }
            "tools" => {
                self.list_tools(cx);
            }
            _ => {}
        }
    }

    /// Fetch and display the selected server's tool list.
    fn list_tools(&mut self, cx: &mut Context<Self>) {
        let server = self.selected_server_name().to_string();
        let index = self.selected_server;

        // Return cached if available.
        if let Some((cached_index, tools)) = &self.cached_tools
            && *cached_index == index
        {
            let content = if tools.is_empty() {
                format!("{server}: no tools discovered (server may not be connected).")
            } else {
                let mut lines = format!("{server} tools ({}):", tools.len());
                for tool in tools {
                    lines.push_str(&format!("\n  /{} — {}", tool.name, tool.description));
                }
                lines
            };
            self.current_messages().push(KaskMessage {
                role: KaskMessageRole::System,
                content,
            });
            cx.notify();
            return;
        }

        if let Some(invoker) = tool_invoker() {
            self.current_messages().push(KaskMessage {
                role: KaskMessageRole::System,
                content: format!("Fetching tools from {server}…"),
            });
            cx.notify();

            let task = invoker.list_tools(&server);
            cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(tools) => {
                            let content = if tools.is_empty() {
                                format!(
                                    "{server}: no tools discovered (server may not be connected)."
                                )
                            } else {
                                let mut lines = format!("{server} tools ({}):", tools.len());
                                for tool in &tools {
                                    lines.push_str(&format!(
                                        "\n  /{} — {}",
                                        tool.name, tool.description
                                    ));
                                }
                                lines
                            };
                            this.current_messages().push(KaskMessage {
                                role: KaskMessageRole::System,
                                content,
                            });
                            this.cached_tools = Some((index, tools));
                        }
                        Err(error) => {
                            this.current_messages().push(KaskMessage {
                                role: KaskMessageRole::System,
                                content: format!("Error listing tools: {error}"),
                            });
                        }
                    }
                    cx.notify();
                })
            })
            .detach();
        } else {
            self.current_messages().push(KaskMessage {
                role: KaskMessageRole::System,
                content: "Tool invoker not wired — set_tool_invoker() not called.".to_string(),
            });
            cx.notify();
        }
    }

    fn invoke_tool(&mut self, tool: String, args: String, cx: &mut Context<Self>) {
        let server = self.selected_server_name().to_string();

        self.current_messages().push(KaskMessage {
            role: KaskMessageRole::User,
            content: format!("/{tool} {args}"),
        });
        self.busy = true;
        self.spinner_frame = 0;
        cx.notify();

        let args_value = parse_args(&args);

        if let Some(invoker) = tool_invoker() {
            let task = invoker.invoke_tool(&server, &tool, args_value);
            cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => {
                            // Try to pretty-print if the output is JSON;
                            // otherwise show the raw string.
                            let formatted = serde_json::from_str::<Value>(&output)
                                .map(|v| format_json_result(&v))
                                .unwrap_or(output);
                            this.current_messages().push(KaskMessage {
                                role: KaskMessageRole::Tool,
                                content: format!("{tool}\n{formatted}"),
                            });
                        }
                        Err(error) => this.current_messages().push(KaskMessage {
                            role: KaskMessageRole::System,
                            content: format!("Error: {error}"),
                        }),
                    }
                    this.busy = false;
                    cx.notify();
                })
            })
            .detach();
        } else {
            self.current_messages().push(KaskMessage {
                role: KaskMessageRole::System,
                content: "Tool invoker not wired — set_tool_invoker() not called.".to_string(),
            });
            self.busy = false;
            cx.notify();
        }
    }

    fn run_scoped_inference(&mut self, prompt: &str, cx: &mut Context<Self>) {
        let server = self.selected_server_name().to_string();

        self.current_messages().push(KaskMessage {
            role: KaskMessageRole::User,
            content: prompt.to_string(),
        });
        self.busy = true;
        self.spinner_frame = 0;
        cx.notify();

        if let Some(inference) = scoped_inference() {
            let task = inference.infer(&server, prompt);
            cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => this.current_messages().push(KaskMessage {
                            role: KaskMessageRole::Assistant,
                            content: output,
                        }),
                        Err(error) => this.current_messages().push(KaskMessage {
                            role: KaskMessageRole::System,
                            content: format!("Inference error: {error}"),
                        }),
                    }
                    this.busy = false;
                    cx.notify();
                })
            })
            .detach();
        } else {
            self.current_messages().push(KaskMessage {
                role: KaskMessageRole::System,
                content: "Scoped inference not wired — set_scoped_inference() not called."
                    .to_string(),
            });
            self.busy = false;
            cx.notify();
        }
    }

    fn render_server_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.selected_server_name();
        let buttons: Vec<AnyElement> = BUILT_IN_MCP_SERVERS
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let is_selected = index == self.selected_server;
                Button::new(("server-btn", index), *name)
                    .style(if is_selected {
                        ButtonStyle::Tinted(ui::TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_server(index, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .child(
                Label::new("MCP Server")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(h_flex().gap_1().flex_wrap().children(buttons))
            .child(
                Label::new(format!("Selected: {current}"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    fn render_messages(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let bg_color = cx.theme().colors().editor_background;

        // Borrow the conversation for the selected server (read-only).
        // If it hasn't been initialized yet, show the welcome message inline.
        let messages: &[KaskMessage] = self
            .conversations
            .get(&self.selected_server)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        let message_elements: Vec<AnyElement> = messages
            .iter()
            .map(|msg| {
                let (color, prefix) = match msg.role {
                    KaskMessageRole::User => (Color::Default, ""),
                    KaskMessageRole::Assistant => (Color::Accent, ""),
                    KaskMessageRole::Tool => (Color::Muted, "[tool] "),
                    KaskMessageRole::System => (Color::Warning, "[system] "),
                };
                v_flex()
                    .gap_0p5()
                    .child(
                        Label::new(format!("{prefix}{}", msg.content))
                            .size(LabelSize::Small)
                            .color(color),
                    )
                    .into_any_element()
            })
            .collect();

        // Spinner line while busy (mirrors the deleted hKask TUI's `⠋ thinking…`).
        let spinner_element: Option<AnyElement> = if self.busy {
            let spinner = match self.spinner_frame % 4 {
                0 => "⠋",
                1 => "⠙",
                2 => "⠹",
                _ => "⠸",
            };
            Some(
                v_flex()
                    .gap_0p5()
                    .child(
                        Label::new(format!("{spinner} working…"))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        div()
            .id("kask-messages")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .gap_2()
            .rounded_sm()
            .border_1()
            .border_color(border_color)
            .bg(bg_color)
            .children(message_elements)
            .children(spinner_element)
    }

    fn render_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        v_flex()
            .gap_1()
            .child(
                div()
                    .border_1()
                    .border_color(border_color)
                    .rounded_sm()
                    .child(self.input_editor.clone())
                    .when(self.busy, |this| this.opacity(0.5)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .child(
                        Label::new("Enter to send · /tool args · /help · /tools")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Button::new("send-btn", "Send")
                            .style(ButtonStyle::Filled)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_input(window, cx);
                            })),
                    ),
            )
    }

    /// Fetch a fresh regulation/gas snapshot on a background task and update
    /// `regulation_snapshot`. Guarded by `status_fetching` so concurrent
    /// renders don't spawn overlapping fetches.
    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        if self.status_fetching {
            return;
        }
        let Some(status) = regulation_status() else {
            return;
        };
        self.status_fetching = true;
        let task = status.snapshot();
        cx.spawn(async move |this, cx| {
            let snapshot = task.await;
            this.update(cx, |this, cx| {
                this.regulation_snapshot = snapshot;
                this.status_fetching = false;
                cx.notify();
            })
        })
        .detach();
    }

    /// Render the regulation status bar — gas gauge + regulation health.
    ///
    /// Mirrors the deleted hKask TUI's `StatusBar`: `Gas: ████░░░░ 50%`
    /// plus a `Reg: ✓ / ⚠ N / ✗ N` indicator. Placed just above the input.
    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;

        // ── Gas gauge ──
        let gas_label = if self.regulation_snapshot.gas_cap == 0 {
            Label::new("Gas: —")
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        } else {
            let cap = self.regulation_snapshot.gas_cap;
            let remaining = self.regulation_snapshot.gas_remaining;
            let pct = (remaining * 100) / cap;
            // 8-cell gauge, filled proportionally.
            let filled = ((remaining * 8) / cap).min(8) as usize;
            let gauge: String = "█".repeat(filled) + &"░".repeat(8 - filled);
            let gas_color = if pct > 50 {
                Color::Created
            } else if pct > 20 {
                Color::Warning
            } else {
                Color::Error
            };
            Label::new(format!("Gas: {gauge} {pct}%"))
                .size(LabelSize::XSmall)
                .color(gas_color)
        };

        // ── Regulation health ──
        let (reg_text, reg_color) = if self.regulation_snapshot.critical_count > 0 {
            (
                format!("Reg: ✗ {}", self.regulation_snapshot.critical_count),
                Color::Error,
            )
        } else if self.regulation_snapshot.alert_count > 0 {
            (
                format!("Reg: ⚠ {}", self.regulation_snapshot.alert_count),
                Color::Warning,
            )
        } else {
            ("Reg: ✓".to_string(), Color::Created)
        };
        let reg_label = Label::new(reg_text)
            .size(LabelSize::XSmall)
            .color(reg_color);

        h_flex()
            .gap_3()
            .border_1()
            .border_color(border_color)
            .rounded_sm()
            .px_2()
            .py_1()
            .child(gas_label)
            .child(reg_label)
    }
}

/// Parse a `/tool_name args` invocation from user input.
/// Returns `Some((tool_name, args_string))` if the input starts with `/` and
/// the first token is not a recognized slash command (`/help`, `/clear`).
/// Slash commands are handled separately in `handle_slash_command`.
fn parse_tool_invocation(text: &str) -> Option<(String, String)> {
    let text = text.strip_prefix('/')?;
    let mut parts = text.splitn(2, char::is_whitespace);
    let tool = parts.next()?.to_string();
    let args = parts.next().unwrap_or("").trim().to_string();
    if tool.is_empty() {
        return None;
    }
    // Don't treat slash commands as tool invocations.
    if matches!(tool.as_str(), "help" | "clear" | "tools") {
        return None;
    }
    Some((tool, args))
}

/// Parse an args string into a JSON `Value`.
///
/// Tries JSON first (for `{...}` inputs), then `key=value` pairs with type
/// coercion (int / float / bool / string), mirroring the deleted hKask TUI's
/// `try_direct_tool_invoke` arg parser. Falls back to wrapping the raw string
/// in `Value::String` if neither applies.
fn parse_args(args: &str) -> Value {
    if args.is_empty() {
        return Value::Object(serde_json::Map::new());
    }
    if args.starts_with('{') {
        return serde_json::from_str(args).unwrap_or_else(|_| Value::String(args.to_string()));
    }
    // key=value pairs with type coercion.
    let mut map = serde_json::Map::new();
    for pair in args.split_whitespace() {
        if let Some((key, value)) = pair.split_once('=') {
            let coerced = if let Ok(n) = value.parse::<i64>() {
                Value::from(n)
            } else if let Ok(f) = value.parse::<f64>() {
                Value::from(f)
            } else if value == "true" {
                Value::from(true)
            } else if value == "false" {
                Value::from(false)
            } else {
                Value::from(value)
            };
            map.insert(key.to_string(), coerced);
        }
    }
    if map.is_empty() {
        // No `key=value` pairs found — wrap the whole string.
        Value::String(args.to_string())
    } else {
        Value::Object(map)
    }
}

/// Format a JSON tool result for display. Pretty-prints objects/arrays,
/// parses stringified-JSON strings, caps recursion at depth 5, and truncates
/// output at 5000 chars on a UTF-8-safe boundary.
///
/// Ported from the deleted hKask TUI `mcp_scoped.rs::format_json_result`.
fn format_json_result(value: &Value) -> String {
    format_json_result_depth(value, 0)
}

fn format_json_result_depth(value: &Value, depth: u8) -> String {
    if depth > 5 {
        return "[...]".to_string();
    }
    let result = match value {
        Value::String(s) => {
            if depth < 5 {
                if let Ok(inner) = serde_json::from_str::<Value>(s) {
                    return format_json_result_depth(&inner, depth + 1);
                }
            }
            s.clone()
        }
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    };
    const MAX_LEN: usize = 5000;
    if result.len() > MAX_LEN {
        let mut end = MAX_LEN;
        while !result.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &result[..end])
    } else {
        result
    }
}

impl Focusable for KaskPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<ItemEvent> for KaskPanel {}

impl Item for KaskPanel {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> gpui::SharedString {
        format!("Kask — {}", self.selected_server_name()).into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        h_flex()
            .gap_1()
            .child(
                self.tab_icon(_window, _cx)
                    .unwrap_or_else(|| Icon::new(IconName::Kask)),
            )
            .child(
                Label::new(self.tab_content_text(params.detail.unwrap_or_default(), _cx))
                    .color(params.text_color()),
            )
            .into_any_element()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Kask).color(Color::Muted))
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<gpui::SharedString> {
        Some("Kask Panel — per-MCP-server chat + tool invocation".into())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Kask Panel Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(_event: &Self::Event, _f: &mut dyn FnMut(ItemEvent)) {}
}

impl SerializableItem for KaskPanel {
    fn serialized_item_kind() -> &'static str {
        "KaskPanel"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        _cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                KaskPanel::new(workspace, window, cx)
            })
        })
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        // Stateless item — nothing to persist beyond the fact that it's open.
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl Render for KaskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Ensure the conversation for the selected server is initialized
        // (lazy welcome message on first render).
        self.current_messages();

        // Animate the spinner while busy.
        if self.busy {
            self.spinner_frame = self.spinner_frame.wrapping_add(1);
            // Schedule a re-render to keep the spinner animating.
            cx.notify();
        }

        // Refresh the regulation/gas snapshot on each render when not busy
        // and no fetch is in flight. The fetch is on a background task; the
        // result triggers a re-render via `cx.notify()`.
        if !self.busy {
            self.refresh_status(cx);
        }

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Kask Panel").size(LabelSize::Large)),
            )
            .child(self.render_server_selector(cx))
            .child(self.render_messages(cx))
            .child(self.render_status_bar(cx))
            .child(self.render_input(cx))
            .into_any_element()
    }
}

/// Initialize the kask panel — registers the center-pane item and actions.
///
/// `Toggle` opens a new kask panel in the active center pane (or focuses an
/// existing one). `ToggleFocus` always focuses an existing panel (no-op if
/// none is open). This mirrors how `TerminalView::deploy` works.
pub fn init(cx: &mut App) {
    register_serializable_item::<KaskPanel>(cx);
    register_serializable_item::<KanbanBoardView>(cx);
    register_serializable_item::<PortfolioDashboardView>(cx);
    register_serializable_item::<ScenariosView>(cx);

    cx.observe_new(
        |workspace: &mut Workspace, _window, _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &Toggle, window, cx| {
                // If a KaskPanel is already open in the active pane, focus it;
                // otherwise add a new one to the active center pane.
                let active_pane = workspace.active_pane().clone();
                let existing_focus = active_pane
                    .read(cx)
                    .items_of_type::<KaskPanel>()
                    .next()
                    .map(|panel| panel.focus_handle(cx));
                if let Some(focus) = existing_focus {
                    focus.focus(window, cx);
                } else {
                    let panel = KaskPanel::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(Box::new(panel), None, true, window, cx);
                }
            });
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                let active_pane = workspace.active_pane().clone();
                let existing_focus = active_pane
                    .read(cx)
                    .items_of_type::<KaskPanel>()
                    .next()
                    .map(|panel| panel.focus_handle(cx));
                if let Some(focus) = existing_focus {
                    focus.focus(window, cx);
                }
            });
            workspace.register_action(|workspace, _: &ToggleKanbanBoard, window, cx| {
                // If a KanbanBoardView is already open in the active pane, focus it;
                // otherwise add a new one to the active center pane.
                let active_pane = workspace.active_pane().clone();
                let existing_focus = active_pane
                    .read(cx)
                    .items_of_type::<KanbanBoardView>()
                    .next()
                    .map(|view| view.focus_handle(cx));
                if let Some(focus) = existing_focus {
                    focus.focus(window, cx);
                } else {
                    let view = KanbanBoardView::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
                }
            });
            workspace.register_action(|workspace, _: &TogglePortfolioDashboard, window, cx| {
                // If a PortfolioDashboardView is already open in the active pane, focus it;
                // otherwise add a new one to the active center pane.
                let active_pane = workspace.active_pane().clone();
                let existing_focus = active_pane
                    .read(cx)
                    .items_of_type::<PortfolioDashboardView>()
                    .next()
                    .map(|view| view.focus_handle(cx));
                if let Some(focus) = existing_focus {
                    focus.focus(window, cx);
                } else {
                    let view = PortfolioDashboardView::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
                }
            });
            workspace.register_action(|workspace, _: &ToggleScenarios, window, cx| {
                // If a ScenariosView is already open in the active pane, focus it;
                // otherwise add a new one to the active center pane.
                let active_pane = workspace.active_pane().clone();
                let existing_focus = active_pane
                    .read(cx)
                    .items_of_type::<ScenariosView>()
                    .next()
                    .map(|view| view.focus_handle(cx));
                if let Some(focus) = existing_focus {
                    focus.focus(window, cx);
                } else {
                    let view = ScenariosView::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
                }
            });
        },
    )
    .detach();
}

// ── Tool completion provider ─────────────────────────────────────────────
//
// When the user types `/` in the input editor, this provider suggests the
// selected server's tools (from the cached tool list). The user can run
// `/tools` first to populate the cache; if the cache is empty, no
// completions are offered.

/// Completion provider for `/tool_name` invocations.
///
/// Reads the panel's `cached_tools` to suggest tool names. Stateless beyond
/// the weak panel handle — the cache is populated by `/tools` or the
// `list_tools` call.
pub(crate) struct KaskToolCompletionProvider {
    panel: WeakEntity<KaskPanel>,
}

impl KaskToolCompletionProvider {
    pub(crate) fn new(panel: WeakEntity<KaskPanel>) -> Self {
        Self { panel }
    }
}

impl CompletionProvider for KaskToolCompletionProvider {
    fn completions(
        &self,
        buffer: &Entity<Buffer>,
        buffer_position: text::Anchor,
        _trigger: editor::CompletionContext,
        _window: &mut Window,
        cx: &mut Context<Editor>,
    ) -> Task<anyhow::Result<Vec<CompletionResponse>>> {
        let panel = self.panel.clone();
        let buffer = buffer.clone();
        cx.spawn(async move |_, cx| {
            // Read the cached tools from the panel.
            let tools = panel
                .read_with(cx, |panel, _| {
                    let (server_index, tools) = panel.cached_tools.as_ref()?;
                    // Only offer completions if the cache is for the currently
                    // selected server.
                    if *server_index != panel.selected_server {
                        return None;
                    }
                    Some(tools.clone())
                })
                .ok()
                .flatten();
            let tools = tools.unwrap_or_default();
            if tools.is_empty() {
                return Ok(Vec::new());
            }

            // Find the `/` prefix range and build the replace_range anchor.
            let replace_range = buffer.read_with(cx, |buffer, _| {
                let snapshot = buffer.text_snapshot();
                let cursor_offset = buffer_position.to_offset(&snapshot);
                let text = snapshot.text();
                let before = &text[..cursor_offset.min(text.len())];
                let slash_offset = before.rfind('/')?;
                let between = &before[slash_offset + 1..];
                if between.chars().any(char::is_whitespace) {
                    return None;
                }
                if slash_offset > 0 {
                    let prev_char = before.as_bytes()[slash_offset - 1];
                    if !prev_char.is_ascii_whitespace() {
                        return None;
                    }
                }
                Some(buffer.anchor_before(slash_offset)..buffer_position)
            });
            let Some(replace_range) = replace_range else {
                return Ok(Vec::new());
            };

            // Build completions for each tool.
            let completions: Vec<Completion> = tools
                .iter()
                .map(|tool| {
                    let new_text = format!("/{} ", tool.name);
                    let label = CodeLabel::plain(format!("/{}", tool.name), Some(&tool.name));
                    let documentation = if tool.description.is_empty() {
                        None
                    } else {
                        Some(CompletionDocumentation::SingleLine(
                            tool.description.clone().into(),
                        ))
                    };
                    Completion {
                        replace_range: replace_range.clone(),
                        new_text,
                        label,
                        documentation,
                        source: CompletionSource::Custom,
                        icon_path: None,
                        icon_color: None,
                        match_start: None,
                        snippet_deduplication_key: None,
                        insert_text_mode: None,
                        confirm: None,
                        group: None,
                    }
                })
                .collect();

            Ok(vec![CompletionResponse {
                completions,
                display_options: Default::default(),
                is_incomplete: false,
            }])
        })
    }

    fn is_completion_trigger(
        &self,
        buffer: &Entity<Buffer>,
        position: text::Anchor,
        text: &str,
        _trigger_in_words: bool,
        cx: &mut Context<Editor>,
    ) -> bool {
        // Trigger on `/` or on alphanumeric input following a `/` prefix.
        if text == "/" {
            return true;
        }
        // Check if we're currently inside a `/tool_name` prefix.
        let buffer = buffer.read(cx);
        let snapshot = buffer.text_snapshot();
        let cursor_offset = position.to_offset(&snapshot);
        let buf_text = snapshot.text();
        let before = &buf_text[..cursor_offset.min(buf_text.len())];
        if let Some(slash_offset) = before.rfind('/') {
            let between = &before[slash_offset + 1..];
            if !between.chars().any(char::is_whitespace)
                && (slash_offset == 0 || before.as_bytes()[slash_offset - 1].is_ascii_whitespace())
            {
                return text.chars().all(|c| c.is_alphanumeric() || c == '_');
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_tool_invocation ────────────────────────────────────────────

    #[test]
    fn parse_tool_invocation_strips_slash_and_args() {
        let (tool, args) = parse_tool_invocation("/kanban_board_list").unwrap();
        assert_eq!(tool, "kanban_board_list");
        assert_eq!(args, "");

        let (tool, args) = parse_tool_invocation("/kanban_task_create board=main").unwrap();
        assert_eq!(tool, "kanban_task_create");
        assert_eq!(args, "board=main");
    }

    #[test]
    fn parse_tool_invocation_returns_none_for_slash_commands() {
        assert!(parse_tool_invocation("/help").is_none());
        assert!(parse_tool_invocation("/clear").is_none());
        assert!(parse_tool_invocation("/tools").is_none());
    }

    #[test]
    fn parse_tool_invocation_returns_none_without_slash() {
        assert!(parse_tool_invocation("hello").is_none());
        assert!(parse_tool_invocation("").is_none());
    }

    // ── parse_args ───────────────────────────────────────────────────────

    #[test]
    fn parse_args_empty_is_empty_object() {
        assert_eq!(parse_args(""), Value::Object(serde_json::Map::new()));
    }

    #[test]
    fn parse_args_json_object() {
        let result = parse_args("{\"board\": \"main\", \"count\": 5}");
        assert_eq!(result["board"], Value::from("main"));
        assert_eq!(result["count"], Value::from(5));
    }

    #[test]
    fn parse_args_key_value_int() {
        let result = parse_args("board=main priority=3");
        assert_eq!(result["board"], Value::from("main"));
        assert_eq!(result["priority"], Value::from(3));
    }

    #[test]
    fn parse_args_key_value_float() {
        let result = parse_args("threshold=2.5");
        assert!(result["threshold"].is_f64());
        assert_eq!(result["threshold"], Value::from(2.5));
    }

    #[test]
    fn parse_args_key_value_bool() {
        let result = parse_args("active=true archived=false");
        assert_eq!(result["active"], Value::from(true));
        assert_eq!(result["archived"], Value::from(false));
    }

    #[test]
    fn parse_args_falls_back_to_string_when_no_pairs() {
        let result = parse_args("just some text");
        assert_eq!(result, Value::from("just some text"));
    }

    // ── format_json_result ──────────────────────────────────────────────

    #[test]
    fn format_json_simple_string() {
        assert_eq!(format_json_result(&Value::from("hello")), "hello");
    }

    #[test]
    fn format_json_object_pretty_printed() {
        let result = format_json_result(&serde_json::json!({"key": "value"}));
        assert!(result.contains("\"key\""));
        assert!(result.contains("\"value\""));
        assert!(result.contains('\n')); // pretty-printed
    }

    #[test]
    fn format_json_nested_string_parses() {
        let inner = serde_json::json!({"nested": true});
        let wrapped = Value::String(inner.to_string());
        let result = format_json_result(&wrapped);
        assert!(result.contains("nested"));
        assert!(result.contains("true"));
    }

    #[test]
    fn format_json_recursion_capped_at_depth_5() {
        let mut val = serde_json::json!({"a": 1});
        for _ in 0..10 {
            val = Value::String(val.to_string());
        }
        let result = format_json_result(&val);
        assert!(result.contains("[...]") || result.len() < 100);
    }

    #[test]
    fn format_json_truncates_long_output() {
        let mut map = serde_json::Map::new();
        for i in 0..1000 {
            map.insert(format!("key_{i}"), serde_json::json!(format!("value_{i}")));
        }
        let val = Value::Object(map);
        let result = format_json_result(&val);
        // ≤5000 safe UTF-8 boundary + "…" (3 bytes)
        assert!(
            result.len() <= 5003,
            "output should be truncated (got {})",
            result.len()
        );
        assert!(result.ends_with('…'));
    }

    // ── server_welcome ──────────────────────────────────────────────────

    #[test]
    fn server_welcome_includes_server_name_and_hint() {
        let welcome = server_welcome("kata-kanban");
        assert!(welcome.contains("kata-kanban"));
        assert!(welcome.contains("/help"));
        assert!(welcome.contains("/tools"));
    }

    #[test]
    fn server_welcome_handles_unknown_server() {
        let welcome = server_welcome("nonexistent");
        assert!(welcome.contains("nonexistent"));
        assert!(welcome.contains("MCP server"));
    }
}
