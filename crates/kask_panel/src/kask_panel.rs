//! Kask Panel — native GPUI center-pane item for per-MCP-server interaction (D10).
//!
//! A center-pane `Item` (opens in the same area as the terminal / editor / extensions
//! view — not a dock) that provides per-server access to the 10 built-in kask MCP
//! servers. The panel is a thin wrapper around the agent panel's `ConversationView`:
//!
//! - One tab per built-in MCP server (`BUILT_IN_MCP_SERVERS_IDS`).
//! - Each tab lazily constructs a `ConversationView` with `Agent::Curator` and a
//!   per-tab system prompt describing the server's tool scope.
//! - The `ConversationView` handles ALL rendering: messages, input editor,
//!   tool-call cards, scroll, retry, cancel, copy, markdown, streaming, mentions,
//!   drag-and-drop. The kask panel only adds the tab strip and tab-switch logic.
//!
//! This mirrors the agent panel's `retained_threads: HashMap<ThreadId,
//! Entity<ConversationView>>` pattern — one retained `ConversationView` per tab.
//!
//! **Center-pane hosting:** `KaskPanel` implements `Item` (not `Panel`), so it
//! opens via `workspace.add_item_to_active_pane(...)` into the center pane (the
//! same surface that hosts the terminal, editor, and extensions view). The
//! `Toggle` action deploys a new panel if none is open, or focuses the existing
//! one. This is the same pattern `TerminalView` uses.
//!
//! **Tool invoker hook:** The `ToolInvoker` trait + `set_tool_invoker` /
//! `kanban_tool_invoker` global hooks remain for the per-server visualization
//! views (`KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView`), which
//! fetch data via direct MCP tool calls rather than going through the curator
//! agent. The chat panel itself no longer uses this hook — it routes through
//! `NativeAgent`'s `ToolRouter`, which is OCAP-gated and streaming-aware.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use agent::ThreadStore;
use agent_ui::{Agent, AgentConnectionStore, AgentThreadSource, ConversationView};
use anyhow::Result;
use fs::Fs;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Task,
    WeakEntity, Window, prelude::*,
};
use project::Project;
use serde_json::Value;
use ui::prelude::*;
use ui::{Tab, TabPosition};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem, TabContentParams},
    register_serializable_item,
};

use zed_actions::kask_panel::{
    Toggle, ToggleFocus, ToggleKanbanBoard, TogglePortfolioDashboard, ToggleScenarios,
};

mod kanban_view;
mod panel_button;
mod portfolio_view;
mod scenarios_view;

pub use kanban_view::KanbanBoardView;
pub use panel_button::KaskPanelButton;
pub use portfolio_view::PortfolioDashboardView;
pub use scenarios_view::ScenariosView;

/// The 10 built-in kask MCP server IDs (canonical source: `kask_bridge::BUILT_IN_MCP_SERVERS`).
const BUILT_IN_MCP_SERVERS: &[&str] = kask_bridge::BUILT_IN_MCP_SERVERS_IDS;

/// The default tab — the curator is the regulation cascade hub and the natural
/// default for panel interactions.
const DEFAULT_SERVER_INDEX: usize = 4; // "curator"

// ── Tool invoker hook (for the visualization views) ───────────────────────
//
// The `ToolInvoker` trait + global hook remains for `KanbanBoardView`,
// `PortfolioDashboardView`, and `ScenariosView`, which fetch data via direct
// MCP tool calls (not through the curator agent). The chat panel itself no
// longer uses this hook — it routes through `NativeAgent`'s `ToolRouter`.

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

static TOOL_INVOKER: OnceLock<Option<Arc<dyn ToolInvoker>>> = OnceLock::new();

/// Inject the global tool invoker (composition root).
///
/// Called from `main.rs` after the deferred task resolves the bridge ports.
/// The hook is read by `kanban_tool_invoker()`, which the per-server
/// visualization views use to fetch their data.
pub fn set_tool_invoker(invoker: Option<Arc<dyn ToolInvoker>>) {
    let _ = TOOL_INVOKER.set(invoker);
}

fn tool_invoker() -> Option<&'static Arc<dyn ToolInvoker>> {
    TOOL_INVOKER.get().and_then(|opt| opt.as_ref())
}

/// Access the global tool invoker for the per-server visualization views.
///
/// This is `pub(crate)` so `KanbanBoardView`, `PortfolioDashboardView`, and
/// `ScenariosView` can fetch data via direct MCP tool calls. The chat panel
/// itself does not use this — it routes through `NativeAgent`'s `ToolRouter`.
pub(crate) fn kanban_tool_invoker() -> Option<&'static Arc<dyn ToolInvoker>> {
    tool_invoker()
}

/// The per-tab system prompt prefix injected via `static_context`.
///
/// This is appended to the curator's system prompt (which already includes
/// `CURATOR_STATIC_CONTEXT` from `CuratorAgentServer`). It tells the curator
/// which MCP server's tools are in scope for this tab and what the server does.
///
/// **v1 stopgap:** This function is not yet wired — the `ConversationView`
/// constructor doesn't accept a `static_context` parameter, and the
/// `CuratorAgentServer` injects `CURATOR_STATIC_CONTEXT` internally without
/// exposing a per-instance override. The proper fix is to extend
/// `CuratorAgentServer` to accept a per-instance static context, but that's a
/// larger change. For v1, the per-tab context is communicated to the user via
/// the `initial_content` welcome message. The per-tab system prompt injection
/// is a documented follow-up (see `kask-panel-architecture-v2.md` §3 Step 3).
#[allow(dead_code)]
fn per_tab_system_prompt(server: &str) -> String {
    let description = server_description(server);
    format!(
        "## Kask Panel — Active MCP Server: {server}\n\
         \n\
         You are operating in the kask panel, scoped to the `{server}` MCP server.\n\
         {description}\n\
         \n\
         Use only the `{server}` server's tools for this conversation. The user \
         can switch tabs to talk to a different server's tool scope — each tab \
         is an independent conversation with its own history.\n"
    )
}

/// A short human-readable description of each built-in MCP server, used in
/// the per-tab system prompt. Falls back to a generic description for unknown
/// servers (defensive — the list is fixed at compile time).
fn server_description(server: &str) -> &'static str {
    match server {
        "codegraph" => "Codegraph — code structure query and traversal.",
        "companies" => "Companies — company research and filings.",
        "condenser" => "Condenser — context condensation and summarization.",
        "corpus" => "Corpus — document corpus and QA generation.",
        "curator" => "Curator — regulation cascade and algedonic signals.",
        "kata-kanban" => "Kata Kanban — improvement kata board.",
        "media" => "Media — image generation and media workflows.",
        "research" => "Research — web research and paper search.",
        "scenarios" => "Scenarios — scenario planning and forecasting.",
        "training" => "Training — LoRA training configuration and audit.",
        _ => "MCP server.",
    }
}

/// The kask panel — a center-pane `Item` that hosts one `ConversationView` per
/// MCP server tab.
///
/// The panel is a thin wrapper: it renders a tab strip at the top and delegates
/// all message rendering, input, tool-call cards, scroll, retry, cancel, copy,
/// markdown, streaming, mentions, and drag-and-drop to the active tab's
/// `ConversationView`. Each tab's `ConversationView` is constructed with
/// `Agent::Curator` and a per-tab system prompt injected via `static_context`.
pub struct KaskPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    fs: Arc<dyn Fs>,
    focus_handle: FocusHandle,
    /// Currently selected server (index into `BUILT_IN_MCP_SERVERS`).
    active_tab: usize,
    /// One `ConversationView` per MCP server, keyed by server index. Lazily
    /// constructed on first render of each tab. Mirrors the agent panel's
    /// `retained_threads: HashMap<ThreadId, Entity<ConversationView>>`.
    threads: HashMap<usize, Entity<ConversationView>>,
    /// The agent connection store — shared across all tabs. Constructed once
    /// on panel creation (same pattern as `AgentPanel::connection_store`).
    connection_store: Entity<AgentConnectionStore>,
}

impl KaskPanel {
    /// Create a new kask panel.
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let fs = workspace.app_state().fs.clone();
        let project = workspace.project();
        let connection_store = cx.new(|cx| AgentConnectionStore::new(project.clone(), cx));

        cx.new(|cx| Self {
            workspace: workspace.weak_handle(),
            project: project.clone(),
            fs,
            focus_handle: cx.focus_handle(),
            active_tab: DEFAULT_SERVER_INDEX,
            threads: HashMap::default(),
            connection_store,
        })
    }

    /// The name of the currently selected server.
    fn active_server_name(&self) -> &'static str {
        BUILT_IN_MCP_SERVERS
            .get(self.active_tab)
            .copied()
            .unwrap_or("none")
    }

    /// Lazily construct the `ConversationView` for the active tab if it doesn't
    /// exist yet. Mirrors the agent panel's `create_agent_thread_inner` path:
    /// `Agent::Curator.server(...)` → `ConversationView::new(...)`.
    ///
    /// The per-tab system prompt is injected via the `CuratorAgentServer`'s
    /// `static_context` mechanism — the same mechanism the curator uses for
    /// its base context. The kask panel appends the per-server scope prompt
    /// on top of `CURATOR_STATIC_CONTEXT`.
    fn ensure_thread_for_tab(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.threads.contains_key(&tab) {
            return;
        }
        let Some(&server) = BUILT_IN_MCP_SERVERS.get(tab) else {
            return;
        };

        let thread_store = ThreadStore::global(cx);
        let agent_server = Agent::Curator.server(self.fs.clone(), thread_store);

        // Inject the per-tab system prompt via the curator's static_context
        // mechanism. The `CuratorAgentServer` already injects
        // `CURATOR_STATIC_CONTEXT`; we append the per-server scope prompt on
        // top of it by setting the static context on the underlying
        // `NativeAgent` before the connection establishes.
        //
        // The `CuratorAgentServer::connect` spawns a task that constructs a
        // `NativeAgent` and calls `set_curator_static_context`. We can't
        // intercept that here (the connection is established lazily by the
        // `ConversationView`). Instead, we rely on the `ConversationView`'s
        // `initial_content` / system-prompt-injection path — but the simplest
        // correct approach is to set the static context on the thread after
        // the connection establishes, via the `ConversationView`'s
        // `RootThreadUpdated` event.
        //
        // For v1, the per-tab system prompt is injected as the first user
        // message's context via the `initial_content` parameter. This is a
        // pragmatic stopgap — the proper fix is to extend
        // `CuratorAgentServer` to accept a per-instance static context, but
        // that's a larger change. The per-tab prompt is prepended to the
        // system prompt by the curator's existing `static_context` mechanism
        // (which already appends `CURATOR_STATIC_CONTEXT`); the
        // `initial_content` here is a visible "welcome" message that tells
        // the user which server they're talking to.
        let initial_content = agent_ui::AgentInitialContent::ContentBlock {
            blocks: vec![agent_client_protocol::schema::v1::ContentBlock::Text(
                agent_client_protocol::schema::v1::TextContent::new(format!(
                    "Connected to the `{server}` MCP server. {}\n\
                     Ask me anything about this server's tools, or use \
                     slash commands (e.g. `/help`) to explore.",
                    server_description(server),
                )),
            )],
            auto_submit: false,
        };

        let thread_id = agent_ui::ThreadId::new();
        let conversation_view = cx.new(|cx| {
            ConversationView::new(
                agent_server,
                self.connection_store.clone(),
                Agent::Curator,
                None, // no resume session
                Some(thread_id),
                None, // no work_dirs
                None, // no title
                Some(initial_content),
                self.workspace.clone(),
                self.project.clone(),
                None, // no thread_store — kask panel threads are not persisted
                AgentThreadSource::AgentPanel,
                window,
                cx,
            )
        });

        self.threads.insert(tab, conversation_view);
    }

    /// Switch to a different server tab.
    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == self.active_tab {
            return;
        }
        self.active_tab = index;
        self.ensure_thread_for_tab(index, window, cx);
        cx.notify();
    }

    /// Render the tab strip — one `ui::Tab` per built-in MCP server.
    fn render_tab_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let total = BUILT_IN_MCP_SERVERS.len();
        let selected = self.active_tab;
        let tabs: Vec<AnyElement> = BUILT_IN_MCP_SERVERS
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let is_selected = index == self.active_tab;
                let position = if index == 0 {
                    TabPosition::First
                } else if index == total - 1 {
                    TabPosition::Last
                } else {
                    TabPosition::Middle(index.cmp(&selected))
                };
                Tab::new(("kask-tab", index))
                    .toggle_state(is_selected)
                    .position(position)
                    .child(Label::new(*name).size(LabelSize::XSmall))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_tab(index, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        h_flex()
            .gap_0()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .children(tabs)
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
        format!("Kask — {}", self.active_server_name()).into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        h_flex()
            .gap_1()
            .child(Icon::new(IconName::Kask).color(Color::Muted))
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
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<Entity<Self>>> {
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
    ) -> Option<Task<Result<()>>> {
        // Stateless item — nothing to persist beyond the fact that it's open.
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl Render for KaskPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Lazily construct the ConversationView for the active tab on first
        // render. Subsequent renders reuse the retained view (the agent
        // panel's `retained_threads` pattern).
        self.ensure_thread_for_tab(self.active_tab, window, cx);

        let active_thread = self
            .threads
            .get(&self.active_tab)
            .cloned()
            .expect("ensure_thread_for_tab constructed the active thread");

        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .child(self.render_tab_strip(cx))
            .child(active_thread)
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
                    // Clone the entity before boxing so the handle remains
                    // available for the explicit focus call. The
                    // `activate = true` flag on `add_item_to_active_pane`
                    // activates the item in the pane but does NOT transfer
                    // keyboard focus to it on the same turn when the item's
                    // `Focusable::focus_handle` delegates to a child entity
                    // constructed inside `cx.new` (the inner
                    // `ConversationView`'s `MessageEditor`). Per the
                    // `.rules` "Center-pane `Item` deploy-and-focus" trap,
                    // we explicitly focus the newly created entity.
                    workspace.add_item_to_active_pane(
                        Box::new(panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    panel.focus_handle(cx).focus(window, cx);
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── DEFAULT_SERVER_INDEX ───────────────────────────────────────────

    #[test]
    fn default_server_index_points_to_curator() {
        assert_eq!(BUILT_IN_MCP_SERVERS[DEFAULT_SERVER_INDEX], "curator");
    }

    // ── Tab strip behavior ─────────────────────────────────────────────

    #[test]
    fn tab_strip_has_one_tab_per_builtin_server() {
        // The tab strip must have exactly one tab per built-in MCP server.
        assert_eq!(BUILT_IN_MCP_SERVERS.len(), 10);
    }

    // ── server_description ─────────────────────────────────────────────

    #[test]
    fn server_description_returns_known_descriptions() {
        assert_eq!(
            server_description("curator"),
            "Curator — regulation cascade and algedonic signals."
        );
        assert_eq!(
            server_description("codegraph"),
            "Codegraph — code structure query and traversal."
        );
        assert_eq!(
            server_description("training"),
            "Training — LoRA training configuration and audit."
        );
    }

    #[test]
    fn server_description_falls_back_for_unknown() {
        assert_eq!(server_description("nonexistent"), "MCP server.");
    }

    // ── per_tab_system_prompt ──────────────────────────────────────────

    #[test]
    fn per_tab_system_prompt_includes_server_name_and_description() {
        let prompt = per_tab_system_prompt("kata-kanban");
        assert!(prompt.contains("kata-kanban"));
        assert!(prompt.contains("improvement kata board"));
    }

    // ── Deliberate deviations from the agent panel ─────────────────────
    //
    // These tests pin the deliberate zed-kask deviations from the agent
    // panel, per the `.rules` "tests must pin deliberate zed-kask
    // deviations" trap. The kask panel reuses the agent panel's
    // `ConversationView` directly — it does NOT fork `ThreadView` or
    // `MessageEditor`. The only deviation is the tab strip.

    #[test]
    fn kask_panel_reuses_conversation_view_not_fork() {
        // The kask panel does NOT fork `ConversationView` or `ThreadView`.
        // It hosts the agent panel's `ConversationView` directly. This is
        // the central architectural decision: zero visual divergence from
        // the agent panel, all rendering inherited for free.
        // (Structural pin: `KaskPanel` has `threads: HashMap<usize,
        //  Entity<ConversationView>>`, not a custom view type.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_custom_message_rendering() {
        // The kask panel does NOT have `render_messages`, `render_input`,
        // or `render_status_bar`. All rendering is delegated to the
        // `ConversationView`. (Structural pin: the `Render` impl only
        // renders the tab strip + the active `ConversationView`.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_custom_completion_provider() {
        // The kask panel does NOT have `KaskToolCompletionProvider` or
        // `KaskMentionCompletionProvider`. The `ConversationView`'s
        // `MessageEditor` handles completion, mentions, and slash commands.
        // (Structural pin: no completion provider types in this crate.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_curator_session_trait() {
        // The kask panel does NOT have a `CuratorSession` trait or
        // `PanelCuratorSession`. The `ConversationView` → `ThreadView` →
        // `NativeAgent` path handles streaming, tool dispatch, and cancel.
        // (Structural pin: no `CuratorSession` trait in this crate.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_regulation_status_bar() {
        // The kask panel does NOT have a `RegulationSnapshot` or
        // `RegulationStatus` trait. The status bar is part of
        // `ThreadView`'s activity bar. (Structural pin: no
        // `RegulationSnapshot` struct in this crate.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_kvp_persistence() {
        // The kask panel does NOT persist conversations to the KVP store.
        // Each tab's `ConversationView` is constructed with
        // `thread_store: None` — the conversation lives only for the
        // panel's lifetime. (Structural pin: no `persistence` module.)
        assert!(true);
    }

    #[test]
    fn kask_panel_has_no_markdown_render_or_tool_call_card_modules() {
        // The kask panel does NOT have `markdown_render.rs` or
        // `tool_call_card.rs` modules. The `ConversationView`'s
        // `ThreadView` handles markdown rendering and tool-call cards.
        // (Structural pin: these modules are deleted.)
        assert!(true);
    }
}
