#![forbid(unsafe_code)]
//! Kanban Panel — a center-pane `Item` showing a persistent, auto-refreshing
//! kanban board. Mirrors the `swarm_panel` crate's center-pane `Item` pattern.
//!
//! The panel fetches boards and tasks from the `hkask-mcp-kata-kanban` MCP
//! server through the global `ToolInvoker` hook (the governed, OCAP-gated MCP
//! runtime path), then renders them via the `KanbanWidget` — the same widget
//! that renders ```` ```kanban ```` fenced blocks inline in agent markdown.
//! The panel is the persistent, always-visible counterpart to the agent's
//! on-demand block rendering: the board state is live without asking the
//! agent to re-emit a block.
//!
//! Data flow:
//! 1. On open, `fetch_boards` calls `kanban_board_list` and auto-selects the
//!    first board.
//! 2. `fetch_tasks` calls `kanban_task_list` for the selected board and
//!    constructs/updates a `KanbanWidget`.
//! 3. A background `refresh_task` re-fetches the task list every 10 seconds
//!    so the board stays current without manual refresh.

use std::time::Duration;

use std::collections::HashSet;

use anyhow::Result;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString, Task,
    WeakEntity, Window, actions,
};
use gpui_util::ResultExt;
use hkask_kanban_widget::block::{KanbanBlockBody, TaskActivityBody, TaskBody};
use hkask_kanban_widget::view::KanbanWidget;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use hkask_types::kanban_wire::KANBAN_SERVER_NAME;
use hkask_types::tool_response::parse_tool_response;
use serde::Deserialize;
use serde_json::json;
use ui::{CommonAnimationExt, IconName, prelude::*};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

pub mod panel_button;
pub use panel_button::KanbanPanelButton;

/// The MCP server id (matches `KANBAN_SERVER_NAME` in `hkask_types::kanban_wire`).
const KANBAN_SERVER: &str = KANBAN_SERVER_NAME;

/// The tool name for listing boards.
const BOARD_LIST_TOOL: &str = "kanban_board_list";
/// The tool name for listing tasks on a board.
const TASK_LIST_TOOL: &str = "kanban_task_list";

/// Auto-refresh interval for the task list.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

actions!(
    kanban_panel,
    [
        /// Deploys a new Kanban Panel if none is open, else focuses the
        /// existing one. Used by the View menu entry.
        Toggle,
        /// Focuses an existing Kanban Panel (no-op if none is open).
        ToggleFocus,
    ]
);

/// Register the panel's actions on every new `Workspace`.
pub fn init(cx: &mut App) {
    register_serializable_item::<KanbanPanel>(cx);
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // Per the `.rules` trap "Center-pane Item Toggle vs ToggleFocus", the
        // View menu entry uses `Toggle` (deploys a new item if none exists),
        // not `ToggleFocus` (silent no-op when absent).
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<KanbanPanel>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let panel = KanbanPanel::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    // Per the `.rules` deploy-and-focus trap, explicitly focus
                    // the newly created panel even though its `FocusHandle` is
                    // stable (created in the constructor, not delegated to a
                    // child entity).
                    panel.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<KanbanPanel>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}

// ── Response models (mirror the MCP server's response shapes) ───────────────

/// One board from `kanban_board_list`. Mirrors the server's `BoardInfo`.
#[derive(Debug, Clone, Deserialize)]
struct BoardInfo {
    #[serde(default)]
    board_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    column_count: usize,
    /// Column definitions including WIP limits. Populated by the server's
    /// `kanban_board_list` response so the panel can render WIP limits.
    #[serde(default)]
    columns: Vec<ColumnDef>,
}

/// One column definition from the server. Mirrors the server's `ColumnInfo`.
#[derive(Debug, Clone, Deserialize)]
struct ColumnDef {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    wip_limit: Option<u32>,
}

/// The `kanban_board_list` response payload (after unwrapping the `content`
/// envelope).
#[derive(Debug, Clone, Deserialize)]
struct BoardListResponse {
    #[serde(default)]
    boards: Vec<BoardInfo>,
}

/// One task from `kanban_task_list`. Mirrors the server's `TaskInfo`.
#[derive(Debug, Clone, Deserialize)]
struct TaskInfo {
    #[serde(default)]
    task_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    gas_remaining: Option<u64>,
    #[serde(default)]
    swarm_id: Option<String>,
    #[serde(default)]
    activity: Option<TaskActivityInfo>,
}

/// The activity strip on a task. Mirrors the server's `TaskActivity`.
#[derive(Debug, Clone, Deserialize)]
struct TaskActivityInfo {
    #[serde(default)]
    text: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    at: String,
}

/// The `kanban_task_list` response payload (after unwrapping the `content`
/// envelope).
#[derive(Debug, Clone, Deserialize)]
struct TaskListResponse {
    #[serde(default)]
    tasks: Vec<TaskInfo>,
}

/// One comment from `kanban_task_comments_since`.
#[derive(Debug, Clone, Deserialize)]
struct CommentInfo {
    #[serde(default)]
    author: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    created_at: String,
}

/// The `kanban_task_comments_since` response payload.
#[derive(Debug, Clone, Deserialize)]
struct CommentsResponse {
    #[serde(default)]
    comments: Vec<CommentInfo>,
}

// ── Panel ───────────────────────────────────────────────────────────────────

/// A persistent, auto-refreshing kanban board panel.
pub struct KanbanPanel {
    #[allow(dead_code)]
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// The currently selected board ID. None until the operator picks one
    /// (auto-selected to the first board after the initial fetch).
    selected_board_id: Option<String>,
    /// Board name for display.
    board_name: Option<SharedString>,
    /// Available boards (from `kanban_board_list`).
    boards: Vec<BoardInfo>,
    /// Tasks for the selected board (from `kanban_task_list`).
    tasks: Vec<TaskInfo>,
    /// Column definitions including WIP limits. Populated from the
    /// selected board's `columns` field in the `kanban_board_list` response.
    columns: Vec<ColumnDef>,
    /// The rendered `KanbanWidget`, cached and reused across refreshes.
    kanban_widget: Option<Entity<KanbanWidget>>,
    /// True while a fetch (board list or task list) is in flight.
    fetching: bool,
    /// Error message if the last fetch failed.
    error: Option<SharedString>,
    /// Auto-refresh task — periodically re-fetches the task list. Cancelled
    /// when the panel is dropped.
    refresh_task: Option<Task<()>>,
    /// The task id whose card-detail panel was open on the last render. Used
    /// to detect when the operator opens a new card detail so the panel can
    /// fetch comments for that task on demand.
    last_detail_open: Option<String>,
    /// Tasks for which comments have already been fetched. Avoids re-fetching
    /// comments on every refresh for tasks whose detail panel was previously
    /// opened.
    comments_fetched: HashSet<String>,
    /// Subscriptions.
    _subscriptions: Vec<gpui::Subscription>,
}

impl KanbanPanel {
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let workspace_handle = workspace.weak_handle();
        cx.new(|cx| {
            let mut this = Self {
                workspace: workspace_handle,
                focus_handle: cx.focus_handle(),
                selected_board_id: None,
                board_name: None,
                boards: Vec::new(),
                tasks: Vec::new(),
                columns: Vec::new(),
                kanban_widget: None,
                fetching: false,
                error: None,
                refresh_task: None,
                last_detail_open: None,
                comments_fetched: HashSet::new(),
                _subscriptions: Vec::new(),
            };
            this.fetch_boards(cx);
            this.start_refresh_task(cx);
            this
        })
    }

    /// Fetch the list of boards from the kanban MCP server. Auto-selects the
    /// first board if none is selected, which triggers a task fetch.
    fn fetch_boards(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = shared_tool_invoker() else {
            self.error = Some(
                "Tool invoker not wired — the kanban MCP server is unavailable. \
                 Ensure kask MCP servers are enabled (kask.mcp.load_default)."
                    .into(),
            );
            cx.notify();
            return;
        };

        self.fetching = true;
        self.error = None;
        cx.notify();

        let task = invoker.invoke_tool(KANBAN_SERVER, BOARD_LIST_TOOL, json!({}));
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let parsed = parse_tool_response(&output)
                    .and_then(|content| serde_json::from_value::<BoardListResponse>(content).ok());
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    match parsed {
                        Some(response) => {
                            this.boards = response.boards;
                            if this.selected_board_id.is_none() && !this.boards.is_empty() {
                                let first = this.boards[0].clone();
                                this.selected_board_id = Some(first.board_id.clone());
                                this.board_name = Some(first.name.into());
                                this.columns = first.columns;
                                this.fetch_tasks(cx);
                            }
                        }
                        None => {
                            this.error = Some(
                                format!("Failed to parse board list response: {output}").into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    this.error = Some(error.into());
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Fetch tasks for the selected board from the kanban MCP server. Builds
    /// or updates the `KanbanWidget` from the response.
    fn fetch_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(board_id) = self.selected_board_id.clone() else {
            return;
        };

        let Some(invoker) = shared_tool_invoker() else {
            self.error = Some(
                "Tool invoker not wired — the kanban MCP server is unavailable. \
                 Ensure kask MCP servers are enabled (kask.mcp.load_default)."
                    .into(),
            );
            cx.notify();
            return;
        };

        self.fetching = true;
        self.error = None;
        cx.notify();

        let args = json!({ "board_id": board_id });
        let task = invoker.invoke_tool(KANBAN_SERVER, TASK_LIST_TOOL, args);
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let parsed = parse_tool_response(&output)
                    .and_then(|content| serde_json::from_value::<TaskListResponse>(content).ok());
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    match parsed {
                        Some(response) => {
                            this.tasks = response.tasks;
                            this.build_or_update_widget(cx);
                        }
                        None => {
                            this.error = Some(
                                format!("Failed to parse task list response: {output}").into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    this.error = Some(error.into());
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Build a `KanbanBlockBody` from the fetched tasks and create or update
    /// the `KanbanWidget`.
    fn build_or_update_widget(&mut self, cx: &mut Context<Self>) {
        let board_id = self.selected_board_id.clone().unwrap_or_default();
        let board_name = self.board_name.as_ref().map(|s| s.to_string());

        let tasks: Vec<TaskBody> = self
            .tasks
            .iter()
            .map(|task| TaskBody {
                task_id: task.task_id.clone(),
                title: task.title.clone(),
                status: task.status.clone(),
                description: None,
                assignee: task.assignee.clone(),
                swarm_id: task.swarm_id.clone(),
                gas_remaining: task.gas_remaining,
                activity: task.activity.as_ref().map(|activity| TaskActivityBody {
                    text: activity.text.clone(),
                    kind: activity.kind.clone(),
                    at: activity.at.clone(),
                }),
                ontology: None,
                priority: None,
                labels: Vec::new(),
                criteria: Vec::new(),
                comments: Vec::new(),
                verification: None,
                gas_spend: Vec::new(),
            })
            .collect();

        let columns: Vec<hkask_kanban_widget::block::ColumnBody> = self
            .columns
            .iter()
            .map(|column| hkask_kanban_widget::block::ColumnBody {
                status: column.status.clone(),
                wip_limit: column.wip_limit,
            })
            .collect();

        let body = KanbanBlockBody {
            viz: Some("kanban".to_string()),
            board_id: Some(board_id),
            board_name,
            tasks,
            columns,
            provenance: BlockProvenance {
                tool: Some(TASK_LIST_TOOL.to_string()),
                server: Some(KANBAN_SERVER.to_string()),
                args: serde_json::Value::Null,
                span_id: None,
            },
        };

        if let Some(existing) = &self.kanban_widget {
            // Update the existing widget in-place via `set_body`, which
            // preserves pending moves, expanded descriptions, and the detail
            // panel. This avoids losing UI state on every refresh.
            existing.update(cx, |widget, cx| {
                widget.set_body(body, cx);
            });
        } else {
            // First render — create a fresh widget entity.
            self.kanban_widget = Some(cx.new(|cx| KanbanWidget::new(body, cx)));
        }
        cx.notify();
    }

    /// Start the auto-refresh background task. Re-fetches the task list every
    /// `REFRESH_INTERVAL` seconds. The task is stored in `refresh_task` so it
    /// is cancelled when the panel is dropped.
    fn start_refresh_task(&mut self, cx: &mut Context<Self>) {
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                let has_board = this
                    .read_with(cx, |this, _cx| this.selected_board_id.is_some())
                    .unwrap_or(false);
                if !has_board {
                    continue;
                }
                this.update(cx, |this, cx| {
                    this.fetch_tasks(cx);
                })
                .log_err();
            }
        }));
    }

    /// Fetch comments for a single task via `kanban_task_comments_since` and
    /// update the widget's cached task body. Called on demand when the
    /// operator opens a card's detail panel.
    fn fetch_task_comments(&mut self, task_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = shared_tool_invoker() else {
            return;
        };

        let args = json!({ "task_id": task_id, "since_index": 0 });
        let task = invoker.invoke_tool(KANBAN_SERVER, "kanban_task_comments_since", args);
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let parsed = parse_tool_response(&output)
                    .and_then(|content| serde_json::from_value::<CommentsResponse>(content).ok());
                this.update(cx, |this, cx| {
                    if let Some(response) = parsed {
                        let comments: Vec<hkask_kanban_widget::block::CommentBody> = response
                            .comments
                            .into_iter()
                            .map(|c| hkask_kanban_widget::block::CommentBody {
                                author: c.author,
                                body: c.body,
                                created_at: c.created_at,
                            })
                            .collect();
                        if let Some(widget) = &this.kanban_widget {
                            widget.update(cx, |widget, cx| {
                                widget.update_task_comments(&task_id, comments, cx);
                            });
                        }
                        this.comments_fetched.insert(task_id);
                    }
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                let _ = error; // Non-critical: comments are optional.
            }
        })
        .detach();
    }

    /// Check if the widget's card-detail panel was opened for a new task. If
    /// so, fetch comments for that task on demand.
    fn check_detail_opened(&mut self, cx: &mut Context<Self>) {
        let current_detail = self
            .kanban_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).detail_open().map(String::from));

        if current_detail != self.last_detail_open {
            self.last_detail_open = current_detail.clone();
            if let Some(task_id) = current_detail {
                if !self.comments_fetched.contains(&task_id) {
                    self.fetch_task_comments(task_id, cx);
                }
            }
        }
    }

    /// Select a board by ID and fetch its tasks.
    fn select_board(&mut self, board_id: String, cx: &mut Context<Self>) {
        if self.selected_board_id.as_ref() == Some(&board_id) {
            return;
        }
        let board = self.boards.iter().find(|board| board.board_id == board_id);
        let name = board.map(|b| b.name.clone());
        let columns = board.map(|b| b.columns.clone()).unwrap_or_default();
        self.selected_board_id = Some(board_id);
        self.board_name = name.map(SharedString::from);
        self.columns = columns;
        self.tasks.clear();
        self.kanban_widget = None;
        self.comments_fetched.clear();
        self.last_detail_open = None;
        self.fetch_tasks(cx);
        cx.notify();
    }

    /// Render the board selector as a row of clickable labels (one per
    /// board). Hidden when there are zero or one boards.
    fn render_board_selector(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.boards.len() <= 1 {
            return None;
        }

        let selected_id = self.selected_board_id.clone();
        let buttons: Vec<AnyElement> = self
            .boards
            .iter()
            .map(|board| {
                let board_id = board.board_id.clone();
                let is_selected = selected_id.as_ref() == Some(&board.board_id);
                div()
                    .id(format!("kanban-board-{}", board.board_id))
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(is_selected, |this| {
                        this.border_1().border_color(Color::Accent.color(cx))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_board(board_id.clone(), cx);
                    }))
                    .child(Label::new(board.name.clone()).size(LabelSize::Small).color(
                        if is_selected {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    ))
                    .into_any_element()
            })
            .collect();

        Some(h_flex().gap_1().children(buttons))
    }

    /// Render the refresh button.
    fn render_refresh_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("kanban-refresh")
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                if this.selected_board_id.is_some() {
                    this.fetch_tasks(cx);
                } else {
                    this.fetch_boards(cx);
                }
            }))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Icon::new(IconName::RotateCw)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new("Refresh")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
    }

    /// Render the error strip.
    fn render_error(&self) -> Option<impl IntoElement> {
        self.error.as_ref().map(|error| {
            Label::new(format!("Error: {error}"))
                .size(LabelSize::Small)
                .color(Color::Warning)
        })
    }

    /// Render the loading state.
    fn render_loading(&self) -> impl IntoElement {
        h_flex().flex_1().items_center().justify_center().child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Icon::new(IconName::LoadCircle)
                        .size(IconSize::Small)
                        .color(Color::Muted)
                        .with_rotate_animation(2),
                )
                .child(
                    Label::new("Loading board…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
        )
    }

    /// Render the empty state (no board selected).
    fn render_empty_state(&self) -> impl IntoElement {
        let message = if self.boards.is_empty() {
            "No kanban boards found. Create a board via the agent to get started."
        } else {
            "Select a board to view its tasks."
        };
        h_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(Label::new(message).color(Color::Muted))
    }
}

impl Render for KanbanPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Check if the widget's card-detail panel was opened for a new task.
        // If so, fetch comments for that task on demand.
        self.check_detail_opened(cx);

        v_flex()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                v_flex()
                    .gap_3()
                    .pt_4()
                    .px_4()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .justify_between()
                            .child(Headline::new("Kanban Board").size(HeadlineSize::Large))
                            .child(self.render_refresh_button(cx)),
                    )
                    .when_some(self.render_board_selector(cx), |this, selector| {
                        this.child(selector)
                    })
                    .when_some(self.render_error(), |this, error| this.child(error)),
            )
            .child(v_flex().px_4().size_full().overflow_y_hidden().map(|this| {
                if self.fetching && self.kanban_widget.is_none() {
                    this.child(self.render_loading()).into_any_element()
                } else if let Some(widget) = &self.kanban_widget {
                    this.child(widget.clone()).into_any_element()
                } else {
                    this.child(self.render_empty_state()).into_any_element()
                }
            }))
    }
}

impl EventEmitter<ItemEvent> for KanbanPanel {}

impl Focusable for KanbanPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        // Stable handle created in the constructor — not delegated to a child
        // entity, so the `.rules` deploy-and-focus trap does not apply. The
        // `Toggle` handler focuses explicitly anyway.
        self.focus_handle.clone()
    }
}

impl Item for KanbanPanel {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Kanban Board".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Kanban Panel Opened")
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::ListTodo).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, function: &mut dyn FnMut(ItemEvent)) {
        function(*event)
    }
}

impl SerializableItem for KanbanPanel {
    fn serialized_item_kind() -> &'static str {
        "KanbanPanel"
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
        cx: &mut App,
    ) -> Task<Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                KanbanPanel::new(workspace, window, cx)
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
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}
