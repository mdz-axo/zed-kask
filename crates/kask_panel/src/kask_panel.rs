//! Kask Panel — native GPUI panel for per-MCP-server interaction (D10).
//!
//! A dockable panel that provides per-server access to the 10 built-in kask
//! MCP servers. The interaction model is a chat-like interface:
//! - **Regular text** → scoped inference (LLM acts as intermediary, calling
//!   only the selected server's tools)
//! - **`/tool_name args`** → direct tool invocation (bypasses LLM, calls the
//!   MCP tool directly via the OCAP-gated path)
//!
//! This mirrors the original hKask `McpTabbedWindow`'s two input paths (Chat
//! tab + Data tab) unified into a single zed-idiomatic chat interface with
//! slash commands — the same pattern zed's agent panel uses.
//!
//! The panel uses global hooks (`set_tool_invoker` / `set_scoped_inference`)
//! so it doesn't depend on `kask_bridge`. The composition root injects the
//! bridge adapters.

use std::sync::{Arc, OnceLock};

use editor::{Editor, EditorMode, MultiBuffer};
use gpui::{
    Action, AnyElement, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle,
    Focusable, Pixels, Render, Task, WeakEntity, Window, prelude::*,
};
use language::Buffer;
use serde_json::Value;
use ui::{IconName, prelude::*};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use zed_actions::kask_panel::{Toggle, ToggleFocus};

const KASK_PANEL_KEY: &str = "KaskPanel";
const MIN_PANEL_WIDTH: Pixels = px(300.);

/// The 10 built-in kask MCP servers (matches `kask_page.rs`).
const BUILT_IN_MCP_SERVERS: &[&str] = &[
    "codegraph",
    "companies",
    "condenser",
    "corpus",
    "curator",
    "kata-kanban",
    "media",
    "research",
    "scenarios",
    "training",
];

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

/// Trait for direct tool invocation (mirrors `hkask_capability::ToolPort`).
/// The bridge provides the implementation.
pub trait ToolInvoker: Send + Sync {
    /// Invoke a tool on a specific server. Returns the result as JSON text.
    fn invoke_tool(&self, server: &str, tool: &str, args: Value) -> Task<Result<String, String>>;
}

/// Trait for scoped inference (mirrors `hkask_types::InferencePort`).
/// The bridge provides the implementation.
pub trait ScopedInference: Send + Sync {
    /// Run scoped inference with only the selected server's tools in scope.
    fn infer(&self, server: &str, prompt: &str) -> Task<Result<String, String>>;
}

static TOOL_INVOKER: OnceLock<Option<Arc<dyn ToolInvoker>>> = OnceLock::new();
static SCOPED_INFERENCE: OnceLock<Option<Arc<dyn ScopedInference>>> = OnceLock::new();

/// Inject the global tool invoker (composition root).
pub fn set_tool_invoker(invoker: Option<Arc<dyn ToolInvoker>>) {
    let _ = TOOL_INVOKER.set(invoker);
}

/// Inject the global scoped inference port (composition root).
pub fn set_scoped_inference(inference: Option<Arc<dyn ScopedInference>>) {
    let _ = SCOPED_INFERENCE.set(inference);
}

fn tool_invoker() -> Option<&'static Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.get().and_then(|opt| opt.as_ref())
}

fn scoped_inference() -> Option<&'static Arc<dyn ScopedInference>> {
    SCOPED_INFERENCE.get().and_then(|opt| opt.as_ref())
}

// ── Panel ────────────────────────────────────────────────────────────────

/// The kask panel — a dockable panel for per-MCP-server chat + tool invocation.
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// Currently selected server (index into `BUILT_IN_MCP_SERVERS`).
    selected_server: usize,
    /// Conversation messages (user, assistant, tool results).
    messages: Vec<KaskMessage>,
    /// The message input editor.
    input_editor: Entity<Editor>,
    /// Whether a request is in progress.
    busy: bool,
}

impl KaskPanel {
    /// Create a new kask panel.
    pub fn new(
        workspace: &Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
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
                messages: vec![KaskMessage {
                    role: KaskMessageRole::System,
                    content: "Kask Panel — select a server, then type a message or /tool args."
                        .to_string(),
                }],
                input_editor,
                busy: false,
            }
        })
    }

    /// Load the panel asynchronously (matches the `Panel::load` pattern).
    pub fn load(
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                Ok(Self::new(workspace, window, cx))
            })?
        })
    }

    fn selected_server_name(&self) -> &'static str {
        BUILT_IN_MCP_SERVERS
            .get(self.selected_server)
            .copied()
            .unwrap_or("none")
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

        // Check if it's a direct tool invocation (/tool_name args).
        if let Some((tool, args)) = parse_tool_invocation(&text) {
            self.invoke_tool(tool, args, cx);
        } else {
            self.run_scoped_inference(&text, cx);
        }
    }

    fn invoke_tool(&mut self, tool: String, args: String, cx: &mut Context<Self>) {
        let server = self.selected_server_name().to_string();

        self.messages.push(KaskMessage {
            role: KaskMessageRole::User,
            content: format!("/{tool} {args}"),
        });
        self.busy = true;
        cx.notify();

        let args_value = serde_json::from_str(&args).unwrap_or(Value::String(args.clone()));

        if let Some(invoker) = tool_invoker() {
            let task = invoker.invoke_tool(&server, &tool, args_value);
            cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => this.messages.push(KaskMessage {
                            role: KaskMessageRole::Tool,
                            content: output,
                        }),
                        Err(error) => this.messages.push(KaskMessage {
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
            self.messages.push(KaskMessage {
                role: KaskMessageRole::System,
                content: "Tool invoker not wired — set_tool_invoker() not called.".to_string(),
            });
            self.busy = false;
            cx.notify();
        }
    }

    fn run_scoped_inference(&mut self, prompt: &str, cx: &mut Context<Self>) {
        let server = self.selected_server_name().to_string();

        self.messages.push(KaskMessage {
            role: KaskMessageRole::User,
            content: prompt.to_string(),
        });
        self.busy = true;
        cx.notify();

        if let Some(inference) = scoped_inference() {
            let task = inference.infer(&server, prompt);
            cx.spawn(async move |this, cx| {
                let result = task.await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => this.messages.push(KaskMessage {
                            role: KaskMessageRole::Assistant,
                            content: output,
                        }),
                        Err(error) => this.messages.push(KaskMessage {
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
            self.messages.push(KaskMessage {
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
                        this.selected_server = index;
                        cx.notify();
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

        let message_elements: Vec<AnyElement> = self
            .messages
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
                        Label::new("Enter to send · /tool args for direct invocation")
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
}

/// Parse a `/tool_name args` invocation from user input.
/// Returns `Some((tool_name, args_string))` if the input starts with `/`.
fn parse_tool_invocation(text: &str) -> Option<(String, String)> {
    let text = text.strip_prefix('/')?;
    let mut parts = text.splitn(2, char::is_whitespace);
    let tool = parts.next()?.to_string();
    let args = parts.next().unwrap_or("").trim().to_string();
    if tool.is_empty() {
        return None;
    }
    Some((tool, args))
}

impl Focusable for KaskPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for KaskPanel {}

impl Panel for KaskPanel {
    fn persistent_name() -> &'static str {
        "KaskPanel"
    }

    fn panel_key() -> &'static str {
        KASK_PANEL_KEY
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position != DockPosition::Bottom
    }

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(400.)
    }

    fn min_size(&self, _window: &Window, _cx: &App) -> Option<Pixels> {
        Some(MIN_PANEL_WIDTH)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Settings)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Kask Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(Toggle)
    }

    fn activation_priority(&self) -> u32 {
        4
    }
}

impl Render for KaskPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(self.render_input(cx))
            .into_any_element()
    }
}

/// Initialize the kask panel — registers actions on the workspace.
pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window, _cx: &mut Context<Workspace>| {
            workspace.register_action(|workspace, _: &Toggle, window, cx| {
                workspace.toggle_panel_focus::<KaskPanel>(window, cx);
            });
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.focus_panel::<KaskPanel>(window, cx);
            });
        },
    )
    .detach();
}
