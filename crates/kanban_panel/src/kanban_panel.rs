#![forbid(unsafe_code)]
//! Kanban Panel — a center-pane `Item` showing a persistent, auto-refreshing
//! kanban board. Mirrors the `swarm_panel` crate's center-pane `Item` pattern.
//!
//! The panel fetches boards and tasks from the `hkask-mcp-kata-kanban` MCP
//! server through the global `ToolInvoker` hook (the metered MCP
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
use editor::Editor;
use gpui::{
    App, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, Render,
    SharedString, Task, WeakEntity, Window, actions,
};
use gpui_util::ResultExt;
use hkask_kanban_widget::block::{KanbanBlockBody, TaskActivityBody, TaskBody};
use hkask_kanban_widget::view::KanbanWidget;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use hkask_types::kanban_wire::KANBAN_SERVER_NAME;
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde::Deserialize;
use serde_json::json;
use ui::{
    CommonAnimationExt, IconName, IconSize, ToggleButtonGroup, ToggleButtonGroupSize,
    ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

// Steer mode: a ConversationView scoped to the kanban MCP server, so the
// kanban panel hosts all swarm-agent kanban coordination. The curator can
// create tasks, spawn subagents, move tasks, and decompose work via the
// kanban MCP tools.
use agent::ThreadStore;
use agent_ui::{Agent, AgentConnectionStore, AgentThreadSource, ConversationView};

pub mod panel_button;
pub mod task_actions;
pub use panel_button::KanbanPanelButton;

use task_actions::{
    CreateTaskForm, EditTaskForm, SpawnTaskForm, render_create_board_form, render_create_task_form,
    render_edit_task_form, render_spawn_task_form,
};

/// The MCP server id (matches `KANBAN_SERVER_NAME` in `hkask_types::kanban_wire`).
const KANBAN_SERVER: &str = KANBAN_SERVER_NAME;

/// The tool name for listing boards.
const BOARD_LIST_TOOL: &str = "kanban_board_list";
/// The tool name for listing tasks on a board.
const TASK_LIST_TOOL: &str = "kanban_task_list";
/// The tool name for creating a board.
const BOARD_CREATE_TOOL: &str = "kanban_board_create";
/// The tool name for deleting a board.
const BOARD_DELETE_TOOL: &str = "kanban_board_delete";
/// The tool name for exporting a board as mermaid markdown.
const BOARD_EXPORT_TOOL: &str = "kanban_board_export";
/// The tool name for importing a board from mermaid markdown.
const BOARD_IMPORT_TOOL: &str = "kanban_board_import";
/// The tool name for creating a task.
const TASK_CREATE_TOOL: &str = "kanban_task_create";
/// The tool name for deleting a task.
const TASK_DELETE_TOOL: &str = "kanban_task_delete";
/// The tool name for updating a task.
const TASK_UPDATE_TOOL: &str = "kanban_task_update";
/// The tool name for spawning a subagent on a task.
const TASK_SPAWN_TOOL: &str = "kanban_task_spawn";
/// The tool name for assigning a task.
const TASK_ASSIGN_TOOL: &str = "kanban_task_assign";
/// The tool name for unassigning a task.
const TASK_UNASSIGN_TOOL: &str = "kanban_task_unassign";

/// Auto-refresh interval for the task list.
const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/// What a refresh tick should re-fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshTarget {
    Boards,
    Tasks,
}

/// Tools that accept an `idempotency_key` and therefore absorb a replay
/// server-side.
///
/// These are exactly the kanban tools that mint a fresh server-side identity, so
/// a duplicate call would create a second row or burn a second spawn. Every other
/// mutation is already idempotent by construction (`task_update` converges;
/// `task_delete` of a deleted task is a no-op), so it needs no key.
///
/// Keep in sync with the `with_idempotency` wiring in
/// `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`. A tool listed here that
/// the server does not protect would make an interrupted call *look* replay-safe
/// while actually duplicating — `idempotent_tools_are_creates_or_spawns` pins the
/// list shape against that drift.
const IDEMPOTENT_TOOLS: &[&str] = &[
    TASK_CREATE_TOOL,
    BOARD_CREATE_TOOL,
    BOARD_IMPORT_TOOL,
    TASK_SPAWN_TOOL,
];

/// Whether `tool` absorbs a replayed call server-side.
fn is_idempotent_tool(tool: &str) -> bool {
    IDEMPOTENT_TOOLS.contains(&tool)
}

/// Attach a fresh `idempotency_key` to `args` for a replay-safe tool.
///
/// Called once per operator gesture, *before* the retry loop, so all attempts
/// share one key. A non-object `args` is a programming error rather than an
/// operator-visible condition, but it is reported instead of silently dropping
/// the key — a silently missing key would leave the retry path believing it was
/// protected when it was not.
fn attach_idempotency_key(
    tool: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !is_idempotent_tool(tool) {
        return Ok(args);
    }
    let mut args = args;
    let Some(object) = args.as_object_mut() else {
        return Err(format!(
            "internal error: {tool} arguments must be a JSON object to carry an \
             idempotency key"
        ));
    };
    object.insert(
        "idempotency_key".to_string(),
        serde_json::Value::String(uuid::Uuid::new_v4().to_string()),
    );
    Ok(args)
}

/// First retry delay for a state-changing tool call whose request never left.
/// Doubles per attempt (250ms, 500ms, 1s).
const MUTATION_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);

/// Maximum retries for a state-changing tool call.
///
/// Deliberately smaller and faster than the read-path budget: a mutation is
/// driven by a direct operator gesture (a form submit, a delete confirmation), so
/// it must resolve or report quickly rather than retrying quietly for half a
/// minute behind a dismissed form.
const MAX_MUTATION_RETRIES: u32 = 3;

/// The backoff delay for the next mutation retry, or `None` once the attempt
/// budget is spent.
///
/// Pure so the policy is unit-testable without a `Workspace`.
fn mutation_retry_delay(attempts_so_far: u32) -> Option<Duration> {
    if attempts_so_far >= MAX_MUTATION_RETRIES {
        return None;
    }
    Some(MUTATION_RETRY_BASE_DELAY * 2u32.pow(attempts_so_far))
}

/// Which fetch a refresh tick should issue.
///
/// Kept as a pure function so the recovery behavior is unit-testable without a
/// `Workspace` (mirrors `hkask-kanban-widget`'s split of its dispatch decision
/// out of the handler). The board-list branch is the load-bearing one: the loop
/// previously skipped the tick entirely when no board was selected, so a
/// `kanban_board_list` that failed at construction — MCP server still starting,
/// or restarting — was never retried and the panel stayed empty for the whole
/// session.
fn refresh_target(has_board: bool) -> RefreshTarget {
    if has_board {
        RefreshTarget::Tasks
    } else {
        RefreshTarget::Boards
    }
}

/// Classify a kanban fetch failure (board list or task list) into an
/// operator-facing message.
///
/// Shared by the `Err(_)` branch of `invoke_tool` (transport-level failure) and
/// the `Ok(output)` branch when the server returned a tool error envelope
/// `{"error": ..., "kind": ...}` (e.g. `failed_precondition` when the DB is
/// not initialized, `unavailable` when the server is down). Before this helper
/// existed, the envelope case fell through to `BoardListResponse`/
/// `TaskListResponse` parsing and surfaced as the misleading
/// "Failed to parse … response: {…}".
///
/// Kept as a pure function so the classification is unit-testable without a
/// `Workspace`, mirroring `refresh_target` and `mutation_retry_delay`.
fn classify_kanban_fetch_error(retryable: bool, message: &str) -> SharedString {
    if retryable {
        format!("Reconnecting to the kanban server… ({message})").into()
    } else {
        message.into()
    }
}

/// Which inline action form is currently active (if any).
#[derive(Clone, Debug)]
pub enum TaskActionKind {
    CreateTask,
    EditTask(String),
    SpawnTask(String),
    CreateBoard,
    /// Confirmation dialog for deleting a task.
    ConfirmDeleteTask(String),
    /// Confirmation dialog for deleting a board.
    ConfirmDeleteBoard,
}

/// The panel's active mode: Browse (board view) or Steer (conversation with
/// the curator scoped to the kanban MCP server for swarm-agent kanban
/// coordination).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelMode {
    Browse,
    Steer,
}

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

/// The system prompt injected into the Steer mode ConversationView. Tells
/// the curator it is scoped to the kanban MCP server and can use all kanban
/// tools for board and task management, including spawning subagents and
/// coordinating with swarms.
/// The `kanban_*` tool names the Steer prompt is allowed to advertise, mirrored
/// from the `#[tool]` fns in `hkask-mcp-kata-kanban`. The swarm panel keeps the
/// same contract for its own prompt (`swarm_panel::parse::KANBAN_TOOLS` plus a
/// `debug_assert!`); this crate does not depend on `swarm_panel`, so it carries
/// its own list rather than inverting the dependency direction.
///
/// Advertising a tool the server does not expose is worse than omitting it: the
/// model calls a name that cannot resolve and the turn fails at dispatch. The
/// `steer_prompt_advertises_only_known_tools` test is the enforcement point.
const ADVERTISED_KANBAN_TOOLS: &[&str] = &[
    "kanban_board_create",
    "kanban_board_list",
    "kanban_board_delete",
    "kanban_board_export",
    "kanban_board_import",
    "kanban_task_create",
    "kanban_task_list",
    "kanban_task_move",
    "kanban_task_assign",
    "kanban_task_unassign",
    "kanban_task_update",
    "kanban_task_delete",
    "kanban_task_verify",
    "kanban_task_reopen",
    "kanban_task_add_rjoules",
    "kanban_task_comment",
    "kanban_task_comments_since",
    "kanban_task_add_deliverable",
    "kanban_task_spawn",
    "kanban_task_delegate_result",
    "kanban_task_kata_coaching",
    "kanban_task_kata_improvement",
    "kanban_task_kata_practice",
    "contract_propose_expect",
];

fn steer_system_prompt(selected_board_id: Option<&str>) -> SharedString {
    let board_clause = match selected_board_id {
        Some(id) => format!(
            "\nThe active board is `{id}`. Use this board id when creating or moving tasks."
        ),
        None => String::new(),
    };
    let prompt = format!(
        "## Kanban Panel — Steer Mode\n\
         You are operating in the Kanban panel's Steer mode, scoped to the \
         `{KANBAN_SERVER}` MCP server. You have access to all kanban tools:\n\
         \n\
         **Board tools**: `kanban_board_create`, `kanban_board_list`, `kanban_board_delete`, \
         `kanban_board_export` (mermaid markdown), `kanban_board_import` (mermaid markdown).\n\
         **Task tools**: `kanban_task_create`, `kanban_task_list`, `kanban_task_move`, \
         `kanban_task_assign`, `kanban_task_unassign`, `kanban_task_update`, `kanban_task_delete`, \
         `kanban_task_verify`, `kanban_task_reopen`, `kanban_task_add_rjoules` (inference/API budget).\n\
         **Communication**: `kanban_task_comment`, `kanban_task_comments_since`, `kanban_task_add_deliverable`.\n\
         **Swarm delegation**: `kanban_task_spawn` (delegates a task to a subagent or swarm agent), \
         `kanban_task_delegate_result` (reads the structured delegation result and verdict).\n\
         **Kata coaching**: `kanban_task_kata_coaching`, `kanban_task_kata_improvement`, `kanban_task_kata_practice`.
         **Contract grounding**: `contract_propose_expect` (creates tasks for contracts missing expect: annotations).
         \n\
         When the operator asks to plan or decompose work, the `kanban-task-management` skill \
         cascade is available. Pass the board id so the cascade writes the durable link on every \
         spawned task.{board_clause}\n\
         \n\
         Pass the swarm id to `kanban_task_spawn` (the `swarm_id` arg) whenever the task is \
         scoped to a swarm — this stamps the durable `Task.swarm_id` link.",
    );
    // Mirrors the swarm panel's `steer_system_prompt` guard: a `kanban_*` name
    // in the prompt that the server does not expose degrades to "tool not
    // found" at dispatch time, so catch it here in dev builds. The
    // `steer_prompt_advertises_only_known_tools` test is the CI enforcement.
    debug_assert!(
        prompt
            .split('`')
            .skip(1)
            .step_by(2)
            .map(|span| span
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>())
            .filter(|name| name.starts_with("kanban_") || name.starts_with("contract_"))
            .all(|name| ADVERTISED_KANBAN_TOOLS.contains(&name.as_str())),
        "steer_system_prompt advertises a kanban_* tool not in ADVERTISED_KANBAN_TOOLS"
    );
    prompt.into()
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
    /// The active inline action form (create task, edit task, spawn task,
    /// create board, delete confirmation). `None` when no action form is open.
    active_action: Option<TaskActionKind>,
    /// The create-task form state. Lazily initialized when the operator
    /// activates the create-task action.
    create_task_form: Option<CreateTaskForm>,
    /// The edit-task form state. Lazily initialized for a specific task.
    edit_task_form: Option<EditTaskForm>,
    /// The spawn-task form state. Lazily initialized for a specific task.
    spawn_task_form: Option<SpawnTaskForm>,
    /// The create-board form state (a single-line editor for the board name).
    create_board_editor: Option<Entity<Editor>>,
    /// The active panel mode (Browse or Steer).
    mode: PanelMode,
    /// Lazily-constructed ConversationView for Steer mode, scoped to the
    /// kanban MCP server. None until the operator first selects Steer.
    steer_conversation: Option<Entity<ConversationView>>,
    /// Connection store for the Steer mode ConversationView.
    steer_connection_store: Option<Entity<AgentConnectionStore>>,
    /// Project entity (needed for ConversationView construction).
    project: Option<Entity<project::Project>>,
    /// Filesystem (needed for CuratorAgentServer).
    fs: Option<std::sync::Arc<dyn fs::Fs>>,
    /// Workspace handle (needed for ConversationView construction).
    workspace_handle: Option<WeakEntity<Workspace>>,
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
        let project = workspace.project().clone();
        let fs = workspace.app_state().fs.clone();
        cx.new(|cx| {
            let mut this = Self {
                workspace: workspace_handle.clone(),
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
                active_action: None,
                create_task_form: None,
                edit_task_form: None,
                spawn_task_form: None,
                create_board_editor: None,
                mode: PanelMode::Browse,
                steer_conversation: None,
                steer_connection_store: None,
                project: Some(project),
                fs: Some(fs),
                workspace_handle: Some(workspace_handle),
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
            // The invoker is wired asynchronously by the deferred post-login task,
            // so a panel opened during startup lands here before the dispatch path
            // exists. The refresh loop retries, so this is a status, not a dead end.
            self.error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
            cx.notify();
            return;
        };

        self.fetching = true;
        self.error = None;
        cx.notify();

        let task = invoker.invoke_tool(KANBAN_SERVER, BOARD_LIST_TOOL, json!({}));
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                // The kanban server returns tool errors as an Ok string carrying
                // the `{"error": ..., "kind": ...}` envelope (see
                // `McpToolError::to_json_string`), not as an `Err` from
                // `invoke_tool`. Without this check, a `failed_precondition`
                // (e.g. DB not initialized) or `unavailable` would fall through
                // to the `BoardListResponse` parse, fail (no `boards` field),
                // and surface as the misleading "Failed to parse board list
                // response: {…}". Route the envelope through the same
                // classification the `Err(_)` branch uses below.
                if let Some(err) = parse_tool_error(&output) {
                    this.update(cx, |this, cx| {
                        this.fetching = false;
                        this.error = Some(classify_kanban_fetch_error(
                            err.is_retryable(),
                            &err.message,
                        ));
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
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
                    // A transport loss is transient: the refresh loop re-attempts
                    // the board list (it no longer skips ticks when no board is
                    // selected), so say so rather than presenting it as terminal.
                    this.error = Some(if error.is_retryable() {
                        format!("Reconnecting to the kanban server… ({error})").into()
                    } else {
                        error.message().into()
                    });
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
            self.error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
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
                // See `fetch_boards`: a server error envelope must be routed
                // through the same classification as the `Err(_)` branch, not
                // fall through to "Failed to parse task list response: {…}".
                if let Some(err) = parse_tool_error(&output) {
                    this.update(cx, |this, cx| {
                        this.fetching = false;
                        this.error = Some(classify_kanban_fetch_error(
                            err.is_retryable(),
                            &err.message,
                        ));
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
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
                    // The refresh loop retries on its own cadence, so a transport
                    // loss reads as reconnecting rather than as a failed board.
                    this.error = Some(if error.is_retryable() {
                        format!("Reconnecting to the kanban server… ({error})").into()
                    } else {
                        error.message().into()
                    });
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Dispatch a state-changing kanban tool, retrying only when the request
    /// provably never reached the server, then refresh the board.
    ///
    /// Every mutation in this panel previously surfaced a single transport
    /// failure as a terminal error, so a routine MCP server restart looked like
    /// "Failed to create task" even though nothing had been attempted. This
    /// centralises the recovery policy instead of repeating it across seven call
    /// sites.
    ///
    /// # Retry safety
    ///
    /// [`InvokeError::Unavailable`] (and `NotWired`) is always retried — the
    /// request provably never left, so nothing can have been applied twice.
    ///
    /// [`InvokeError::Interrupted`] means the request reached the server and the
    /// connection dropped before the response, so the outcome is unknown. Whether
    /// that is retryable depends on the *server*, not the client:
    ///
    /// - Tools listed in [`IDEMPOTENT_TOOLS`] accept an `idempotency_key`. The key
    ///   is generated once per operator gesture and reused across attempts, so a
    ///   replay is absorbed server-side and returns the original result. These are
    ///   retried.
    /// - Everything else is not retried: a blind retry could create a second task
    ///   or charge a second spawn. The panel refreshes and reports that the
    ///   outcome is unknown so the operator can see the true state.
    ///
    /// `label` names the operation in operator-facing messages (e.g. "create
    /// task"). `refresh` selects which list to re-read on completion — board
    /// mutations must refresh the board list, not the task list of a board that
    /// may no longer exist.
    fn dispatch_mutation(
        &mut self,
        tool: &'static str,
        args: serde_json::Value,
        label: &'static str,
        refresh: RefreshTarget,
        cx: &mut Context<Self>,
    ) {
        self.dispatch_mutation_with(tool, args, label, refresh, None, cx);
    }

    /// [`Self::dispatch_mutation`] plus an optional state fixup applied before
    /// the refresh.
    ///
    /// `before_refresh` exists for mutations that invalidate panel state the
    /// refresh depends on — deleting the selected board must clear the selection
    /// before the board list is re-read, or the panel would fetch tasks for a
    /// board that no longer exists. It runs on success and on an unknown outcome
    /// (where the mutation may have landed), never on a clean failure.
    fn dispatch_mutation_with(
        &mut self,
        tool: &'static str,
        args: serde_json::Value,
        label: &'static str,
        refresh: RefreshTarget,
        before_refresh: Option<fn(&mut Self)>,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = shared_tool_invoker() else {
            self.error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
            cx.notify();
            return;
        };
        cx.notify();

        // One key per operator gesture, generated before the retry loop so every
        // attempt carries the SAME key. Generating it per attempt would defeat the
        // purpose entirely — the server would see each retry as new work.
        let args = match attach_idempotency_key(tool, args) {
            Ok(args) => args,
            Err(error) => {
                self.error = Some(format!("Could not {label}: {error}").into());
                cx.notify();
                return;
            }
        };
        let replay_safe = is_idempotent_tool(tool);

        cx.spawn(async move |this, cx| {
            let mut attempt: u32 = 0;
            loop {
                let outcome = invoker.invoke_tool(KANBAN_SERVER, tool, args.clone()).await;
                match outcome {
                    Ok(output) => {
                        // The kanban server returns tool errors as an Ok string
                        // carrying the `{"error": ..., "kind": ...}` envelope
                        // (see `McpToolError::to_json_string`), not as an `Err`
                        // from `invoke_tool`. Without this check, a mutation
                        // that failed server-side (e.g. `invalid_argument`,
                        // `not_found`) would silently look like success — the
                        // `Ok(_)` arm cleared the error and refreshed, so the
                        // operator saw no feedback that their action failed.
                        // Route the envelope to the same failure path as an
                        // `Err(InvokeError::Failed(_))`.
                        if let Some(err) = parse_tool_error(&output) {
                            this.update(cx, |this, cx| {
                                this.error =
                                    Some(format!("Failed to {label}: {}", err.message).into());
                                cx.notify();
                            })
                            .log_err();
                            return;
                        }
                        this.update(cx, |this, cx| {
                            this.error = None;
                            if let Some(fixup) = before_refresh {
                                fixup(this);
                            }
                            match refresh {
                                RefreshTarget::Tasks => this.fetch_tasks(cx),
                                RefreshTarget::Boards => this.fetch_boards(cx),
                            }
                        })
                        .log_err();
                        return;
                    }
                    // An interrupted call on a replay-safe tool is retryable: the
                    // shared idempotency key means the server absorbs the replay
                    // and returns the original result rather than repeating work.
                    Err(error)
                        if error.is_retryable() || (replay_safe && error.is_outcome_unknown()) =>
                    {
                        let Some(delay) = mutation_retry_delay(attempt) else {
                            this.update(cx, |this, cx| {
                                this.error = Some(
                                    format!(
                                        "Could not {label}: the kanban server is unreachable. \
                                         Nothing was changed — try again once it reconnects."
                                    )
                                    .into(),
                                );
                                cx.notify();
                            })
                            .log_err();
                            return;
                        };
                        attempt += 1;
                        // Surface the wait so the operator sees progress rather
                        // than a frozen form.
                        if this
                            .update(cx, |this, cx| {
                                this.error = Some(
                                    format!(
                                        "Reconnecting to the kanban server to {label}… \
                                         (attempt {attempt}/{MAX_MUTATION_RETRIES})"
                                    )
                                    .into(),
                                );
                                cx.notify();
                            })
                            .log_err()
                            .is_none()
                        {
                            return;
                        }
                        cx.background_executor().timer(delay).await;
                    }
                    Err(error) => {
                        // Either the tool ran and failed, or the outcome is
                        // unknown. Both are terminal for this dispatch; the
                        // refresh below lets the operator see the true state.
                        let outcome_unknown = error.is_outcome_unknown();
                        this.update(cx, |this, cx| {
                            this.error = Some(if outcome_unknown {
                                format!(
                                    "The connection dropped while trying to {label}. It may or \
                                     may not have taken effect — check the board below before \
                                     retrying."
                                )
                                .into()
                            } else {
                                format!("Failed to {label}: {error}").into()
                            });
                            // Re-read state after an unknown outcome: the server
                            // is the only source of truth about whether the
                            // mutation landed.
                            if outcome_unknown {
                                if let Some(fixup) = before_refresh {
                                    fixup(this);
                                }
                                match refresh {
                                    RefreshTarget::Tasks => this.fetch_tasks(cx),
                                    RefreshTarget::Boards => this.fetch_boards(cx),
                                }
                            }
                            cx.notify();
                        })
                        .log_err();
                        return;
                    }
                }
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
                spend_log: Vec::new(),
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

    /// Start the auto-refresh background task. Re-fetches every
    /// `REFRESH_INTERVAL` seconds. The task is stored in `refresh_task` so it
    /// is cancelled when the panel is dropped.
    ///
    /// Refreshes the *board list* when no board is selected, and the task list
    /// otherwise. The board-list branch is what makes the panel self-healing: the
    /// loop previously `continue`d whenever `selected_board_id` was `None`, so a
    /// `board_list` that failed at construction (MCP server still starting, or
    /// restarting) was never retried and the panel stayed empty for the rest of
    /// the session.
    fn start_refresh_task(&mut self, cx: &mut Context<Self>) {
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_INTERVAL).await;
                let has_board = this
                    .read_with(cx, |this, _cx| this.selected_board_id.is_some())
                    .unwrap_or(false);
                if this
                    .update(cx, |this, cx| match refresh_target(has_board) {
                        RefreshTarget::Tasks => this.fetch_tasks(cx),
                        RefreshTarget::Boards => this.fetch_boards(cx),
                    })
                    .log_err()
                    .is_none()
                {
                    // The panel is gone; stop the loop rather than spinning on a
                    // dead entity for the lifetime of the process.
                    return;
                }
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

    // ── Task action handlers ───────────────────────────────────────────────

    /// Start the create-task flow: show the inline form.
    fn start_create_task(&mut self, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.create_task_form = None;
        self.active_action = Some(TaskActionKind::CreateTask);
        cx.notify();
    }

    /// Submit the create-task form. Calls `kanban_task_create` and refreshes.
    fn submit_create_task(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.create_task_form else {
            return;
        };
        let Some(board_id) = self.selected_board_id.clone() else {
            return;
        };
        let args = form.collect_args(&board_id, cx);
        self.active_action = None;
        self.create_task_form = None;
        self.dispatch_mutation(
            TASK_CREATE_TOOL,
            args,
            "create task",
            RefreshTarget::Tasks,
            cx,
        );
    }

    /// Start the edit-task flow for a specific task.
    fn start_edit_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.edit_task_form = None;
        self.active_action = Some(TaskActionKind::EditTask(task_id));
        cx.notify();
    }

    /// Submit the edit-task form. Calls `kanban_task_update` and refreshes.
    fn submit_edit_task(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.edit_task_form else {
            return;
        };
        let args = form.collect_args(cx);
        self.active_action = None;
        self.edit_task_form = None;
        self.dispatch_mutation(
            TASK_UPDATE_TOOL,
            args,
            "update task",
            RefreshTarget::Tasks,
            cx,
        );
    }

    /// Start the spawn-task flow for a specific task.
    fn start_spawn_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_task_form = None;
        self.active_action = Some(TaskActionKind::SpawnTask(task_id));
        cx.notify();
    }

    /// Submit the spawn-task form. Calls `kanban_task_spawn`.
    fn submit_spawn_task(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.spawn_task_form else {
            return;
        };
        let args = form.collect_args(cx);
        self.active_action = None;
        self.spawn_task_form = None;
        // A spawn starts a subagent, which burns gas. `dispatch_mutation` only
        // retries requests that provably never left, so a spawn cannot be
        // double-started by the retry path.
        self.dispatch_mutation(
            TASK_SPAWN_TOOL,
            args,
            "spawn subagent",
            RefreshTarget::Tasks,
            cx,
        );
    }

    /// Toggle task assignment. If assigned, unassign; if unassigned, assign.
    fn toggle_task_assignment(
        &mut self,
        task_id: String,
        is_assigned: bool,
        cx: &mut Context<Self>,
    ) {
        let tool = if is_assigned {
            TASK_UNASSIGN_TOOL
        } else {
            TASK_ASSIGN_TOOL
        };
        let args = json!({ "task_id": task_id });
        self.dispatch_mutation(tool, args, "toggle assignment", RefreshTarget::Tasks, cx);
    }

    /// Show the delete-task confirmation dialog.
    fn confirm_delete_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.active_action = Some(TaskActionKind::ConfirmDeleteTask(task_id));
        cx.notify();
    }

    /// Execute the task deletion.
    fn execute_delete_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.active_action = None;
        let args = json!({ "task_id": task_id });
        self.dispatch_mutation(
            TASK_DELETE_TOOL,
            args,
            "delete task",
            RefreshTarget::Tasks,
            cx,
        );
    }

    // ── Board action handlers ──────────────────────────────────────────────

    /// Start the create-board flow.
    fn start_create_board(&mut self, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.create_board_editor = None;
        self.active_action = Some(TaskActionKind::CreateBoard);
        cx.notify();
    }

    /// Submit the create-board form.
    fn submit_create_board(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &self.create_board_editor else {
            return;
        };
        let name = editor.read(cx).text(cx);
        if name.trim().is_empty() {
            return;
        }
        self.active_action = None;
        self.create_board_editor = None;
        let args = json!({ "name": name });
        self.dispatch_mutation(
            BOARD_CREATE_TOOL,
            args,
            "create board",
            RefreshTarget::Boards,
            cx,
        );
    }

    /// Show the delete-board confirmation dialog.
    fn confirm_delete_board(&mut self, cx: &mut Context<Self>) {
        self.active_action = Some(TaskActionKind::ConfirmDeleteBoard);
        cx.notify();
    }

    /// Execute the board deletion.
    fn execute_delete_board(&mut self, cx: &mut Context<Self>) {
        let Some(board_id) = self.selected_board_id.clone() else {
            return;
        };
        self.active_action = None;
        let args = json!({ "board_id": board_id });
        // A deleted board invalidates the selection, so clear it before the
        // refresh. `clear_board_selection` runs on success and on an unknown
        // outcome — in the latter case the board may be gone, and re-reading the
        // board list against a cleared selection is the safe reconciliation.
        self.dispatch_mutation_with(
            BOARD_DELETE_TOOL,
            args,
            "delete board",
            RefreshTarget::Boards,
            Some(Self::clear_board_selection),
            cx,
        );
    }

    /// Drop the selected board and everything derived from it.
    fn clear_board_selection(&mut self) {
        self.selected_board_id = None;
        self.board_name = None;
        self.tasks.clear();
        self.kanban_widget = None;
    }

    /// Export the selected board as mermaid kanban markdown and copy it to
    /// the system clipboard. The markdown round-trips through `import_board`.
    /// Only the board owner can export (the server enforces P12); a
    /// permission error surfaces in the error strip.
    fn export_board(&mut self, cx: &mut Context<Self>) {
        let Some(board_id) = self.selected_board_id.clone() else {
            return;
        };
        let Some(invoker) = shared_tool_invoker() else {
            self.error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
            cx.notify();
            return;
        };
        self.fetching = true;
        self.error = None;
        cx.notify();
        let args = json!({ "board_id": board_id });
        let task = invoker.invoke_tool(KANBAN_SERVER, BOARD_EXPORT_TOOL, args);
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                if let Some(err) = parse_tool_error(&output) {
                    this.update(cx, |this, cx| {
                        this.fetching = false;
                        this.error =
                            Some(format!("Failed to export board: {}", err.message).into());
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
                let markdown = parse_tool_response(&output).and_then(|content| {
                    serde_json::from_value::<serde_json::Value>(content)
                        .ok()
                        .and_then(|v| v.get("markdown")?.as_str().map(|s| s.to_string()))
                });
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    match markdown {
                        Some(md) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(md));
                            this.error = None;
                        }
                        None => {
                            this.error =
                                Some(format!("Failed to parse export response: {output}").into());
                        }
                    }
                    cx.notify();
                })
                .log_err();
            }
            Err(error) => {
                this.update(cx, |this, cx| {
                    this.fetching = false;
                    this.error = Some(if error.is_retryable() {
                        format!("Reconnecting to the kanban server… ({error})").into()
                    } else {
                        error.message().into()
                    });
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    /// Import a board from mermaid kanban markdown read from the system
    /// clipboard. Creates a new board with columns and tasks matching the
    /// parsed markdown, then refreshes the board list. Replay-safe via the
    /// server's idempotency key (generated per gesture).
    fn import_board(&mut self, cx: &mut Context<Self>) {
        let Some(clipboard) = cx.read_from_clipboard() else {
            self.error = Some("Clipboard is empty — copy mermaid kanban markdown first".into());
            cx.notify();
            return;
        };
        let Some(markdown) = clipboard.text() else {
            self.error = Some("Clipboard has no text — copy mermaid kanban markdown first".into());
            cx.notify();
            return;
        };
        if markdown.trim().is_empty() {
            self.error = Some("Clipboard is empty — copy mermaid kanban markdown first".into());
            cx.notify();
            return;
        }
        let args = json!({
            "markdown": markdown,
            // The server falls back to the parsed board name or "Imported Board",
            // so we do not set board_name here — preserve the exported name.
        });
        self.dispatch_mutation(
            BOARD_IMPORT_TOOL,
            args,
            "import board",
            RefreshTarget::Boards,
            cx,
        );
    }

    // ── Steer mode ─────────────────────────────────────────────────────────

    /// Switch the panel mode (Browse or Steer).
    fn set_mode(&mut self, mode: PanelMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        cx.notify();
    }

    /// Lazily construct the ConversationView for Steer mode, scoped to the
    /// kanban MCP server. The curator can create tasks, spawn subagents,
    /// move tasks, and decompose work via the kanban MCP tools.
    fn ensure_steer_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.steer_conversation.is_some() {
            return;
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let Some(fs) = self.fs.clone() else {
            return;
        };
        let Some(workspace) = self.workspace_handle.clone() else {
            return;
        };

        let thread_store = ThreadStore::global(cx);
        let agent_server = std::rc::Rc::new(
            agent::CuratorAgentServer::new(fs, thread_store.clone())
                .with_extra_static_context(steer_system_prompt(self.selected_board_id.as_deref()))
                .with_mcp_server_scope(KANBAN_SERVER.into()),
        );

        let connection_store = cx.new(|cx| AgentConnectionStore::new(project.clone(), cx));
        self.steer_connection_store = Some(connection_store.clone());

        let thread_id = agent_ui::ThreadId::new();
        let conversation_view = cx.new(|cx| {
            ConversationView::new(
                agent_server,
                connection_store,
                Agent::Curator,
                None,
                Some(thread_id),
                None,
                None,
                None,
                workspace,
                project,
                Some(thread_store),
                AgentThreadSource::AgentPanel,
                window,
                cx,
            )
        });
        self.steer_conversation = Some(conversation_view);
    }

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
        // The Steer conversation bakes the active board id into its system
        // prompt at construction (`ensure_steer_conversation` reads
        // `selected_board_id` once). Switching boards while Steer is open would
        // leave the curator creating and moving tasks on the previously selected
        // board. Drop the conversation so the next Steer selection rebuilds with
        // the new board. Mirrors `set_swarm_mode` in the swarm panel, which
        // drops its Steer conversation for the same reason.
        self.steer_conversation = None;
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
            "No kanban boards found. Click + Board to create one."
        } else {
            "Select a board to view its tasks."
        };
        h_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .child(Label::new(message).color(Color::Muted))
    }

    /// Render the toolbar with action buttons (create task, create board,
    /// delete board, refresh).
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_board = self.selected_board_id.is_some();
        let border_color = cx.theme().colors().border;
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .id("kanban-create-task-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(has_board, |this| this.hover(|this| this.bg(border_color)))
                    .when(!has_board, |this| this.opacity(0.5))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected_board_id.is_some() {
                            this.start_create_task(cx);
                        }
                    }))
                    .tooltip(Tooltip::text("Create task"))
                    .child(
                        Icon::new(IconName::Plus)
                            .size(IconSize::Small)
                            .color(if has_board {
                                Color::Accent
                            } else {
                                Color::Muted
                            }),
                    ),
            )
            .child(
                div()
                    .id("kanban-create-board-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|this| this.bg(border_color))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.start_create_board(cx);
                    }))
                    .tooltip(Tooltip::text("Create board"))
                    .child(
                        Icon::new(IconName::Plus)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .id("kanban-delete-board-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(has_board, |this| this.hover(|this| this.bg(border_color)))
                    .when(!has_board, |this| this.opacity(0.5))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected_board_id.is_some() {
                            this.confirm_delete_board(cx);
                        }
                    }))
                    .tooltip(Tooltip::text("Delete board"))
                    .child(
                        Icon::new(IconName::Trash)
                            .size(IconSize::Small)
                            .color(if has_board {
                                Color::Warning
                            } else {
                                Color::Muted
                            }),
                    ),
            )
            .child(
                div()
                    .id("kanban-export-board-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .when(has_board, |this| this.hover(|this| this.bg(border_color)))
                    .when(!has_board, |this| this.opacity(0.5))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected_board_id.is_some() {
                            this.export_board(cx);
                        }
                    }))
                    .tooltip(Tooltip::text(
                        "Export board as mermaid markdown to clipboard",
                    ))
                    .child(Icon::new(IconName::Download).size(IconSize::Small).color(
                        if has_board {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    )),
            )
            .child(
                div()
                    .id("kanban-import-board-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|this| this.bg(border_color))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.import_board(cx);
                    }))
                    .tooltip(Tooltip::text(
                        "Import board from mermaid markdown in clipboard",
                    ))
                    .child(
                        Icon::new(IconName::Share)
                            .size(IconSize::Small)
                            .color(Color::Accent),
                    ),
            )
            .child(self.render_refresh_button(cx))
    }

    /// Render the active action form (if any).
    fn render_action_form(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.active_action {
            None => None,
            Some(TaskActionKind::CreateTask) => self
                .create_task_form
                .as_ref()
                .map(|form| render_create_task_form(form, cx).into_any_element()),
            Some(TaskActionKind::EditTask(_task_id)) => self
                .edit_task_form
                .as_ref()
                .map(|form| render_edit_task_form(form, cx).into_any_element()),
            Some(TaskActionKind::SpawnTask(_task_id)) => self
                .spawn_task_form
                .as_ref()
                .map(|form| render_spawn_task_form(form, cx).into_any_element()),
            Some(TaskActionKind::CreateBoard) => self
                .create_board_editor
                .as_ref()
                .map(|editor| render_create_board_form(editor, cx).into_any_element()),
            Some(TaskActionKind::ConfirmDeleteTask(task_id)) => {
                let task_id_clone = task_id.clone();
                Some(
                    v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .bg(cx.theme().colors().editor_background)
                        .child(
                            Label::new(format!("Delete task '{task_id}'?"))
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .id("kanban-delete-task-confirm")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().colors().border)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.execute_delete_task(task_id_clone.clone(), cx);
                                        }))
                                        .child(
                                            Label::new("Delete")
                                                .size(LabelSize::Small)
                                                .color(Color::Warning),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("kanban-delete-task-cancel")
                                        .cursor_pointer()
                                        .px_2()
                                        .py_1()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_action = None;
                                            cx.notify();
                                        }))
                                        .child(
                                            Label::new("Cancel")
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Some(TaskActionKind::ConfirmDeleteBoard) => {
                let board_name = self
                    .board_name
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                Some(
                    v_flex()
                        .gap_2()
                        .p_3()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .bg(cx.theme().colors().editor_background)
                        .child(
                            Label::new(format!("Delete board '{board_name}' and all its tasks?"))
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .id("kanban-delete-board-confirm")
                                        .cursor_pointer()
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().colors().border)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.execute_delete_board(cx);
                                        }))
                                        .child(
                                            Label::new("Delete Board")
                                                .size(LabelSize::Small)
                                                .color(Color::Warning),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("kanban-delete-board-cancel")
                                        .cursor_pointer()
                                        .px_2()
                                        .py_1()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_action = None;
                                            cx.notify();
                                        }))
                                        .child(
                                            Label::new("Cancel")
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                ),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    /// Render per-task action buttons (Edit, Spawn subagent, Assign/Unassign,
    /// Delete) for the task whose card-detail panel is currently open in the
    /// widget. Returns `None` when no card detail is open, the task is no
    /// longer on the board, or an inline action form/dialog is already active
    /// (the form takes over the action area in that case).
    fn render_task_actions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.active_action.is_some() {
            return None;
        }
        let detail_id = self
            .kanban_widget
            .as_ref()
            .and_then(|widget| widget.read(cx).detail_open().map(String::from))?;
        let task = self.tasks.iter().find(|t| t.task_id == detail_id)?;
        let task_id = task.task_id.clone();
        let is_assigned = task.assignee.is_some();
        let hover_bg = cx.theme().colors().border;

        let edit_id = task_id.clone();
        let spawn_id = task_id.clone();
        let assign_id = task_id.clone();
        let delete_id = task_id;

        Some(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .id("kanban-task-edit")
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .hover(move |this| this.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_edit_task(edit_id.clone(), cx);
                        }))
                        .child(Label::new("Edit").size(LabelSize::Small)),
                )
                .child(
                    div()
                        .id("kanban-task-spawn")
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .hover(move |this| this.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_spawn_task(spawn_id.clone(), cx);
                        }))
                        .child(Label::new("Spawn subagent").size(LabelSize::Small)),
                )
                .child(
                    div()
                        .id("kanban-task-assign")
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .hover(move |this| this.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_task_assignment(assign_id.clone(), is_assigned, cx);
                        }))
                        .child(
                            Label::new(if is_assigned { "Unassign" } else { "Assign" })
                                .size(LabelSize::Small),
                        ),
                )
                .child(
                    div()
                        .id("kanban-task-delete")
                        .cursor_pointer()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .hover(move |this| this.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm_delete_task(delete_id.clone(), cx);
                        }))
                        .child(
                            Label::new("Delete")
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for KanbanPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Check if the widget's card-detail panel was opened for a new task.
        // If so, fetch comments for that task on demand.
        self.check_detail_opened(cx);

        // Lazily initialize edit/spawn/create forms when the action is
        // activated (Editor::single_line needs a Window, which is only
        // available here in render).
        if let Some(TaskActionKind::EditTask(task_id)) = &self.active_action {
            if self.edit_task_form.is_none() {
                let task = self.tasks.iter().find(|t| t.task_id == *task_id);
                if let Some(task) = task {
                    self.edit_task_form = Some(EditTaskForm::for_task(
                        &task.task_id,
                        &task.title,
                        None,
                        None,
                        &[],
                        window,
                        cx,
                    ));
                }
            }
        }
        if let Some(TaskActionKind::SpawnTask(task_id)) = &self.active_action {
            if self.spawn_task_form.is_none() {
                self.spawn_task_form = Some(SpawnTaskForm::for_task(task_id, window, cx));
            }
        }
        if matches!(self.active_action, Some(TaskActionKind::CreateTask))
            && self.create_task_form.is_none()
        {
            self.create_task_form = Some(CreateTaskForm::new(window, cx));
        }
        if matches!(self.active_action, Some(TaskActionKind::CreateBoard))
            && self.create_board_editor.is_none()
        {
            self.create_board_editor = Some(cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("Board name", window, cx);
                editor
            }));
        }

        // Lazily initialize the Steer mode conversation when the operator
        // switches to Steer (ConversationView::new needs a Window).
        if self.mode == PanelMode::Steer && self.steer_conversation.is_none() {
            self.ensure_steer_conversation(window, cx);
        }

        let mode = self.mode;

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
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        // Mode toggle: Browse / Steer — uses
                                        // `ToggleButtonGroup` for consistency with the
                                        // swarm panel (the prior hand-rolled
                                        // `div().cursor_pointer()` toggle had no
                                        // measurement and could collide at narrow
                                        // widths).
                                        div().child(
                                            ToggleButtonGroup::single_row(
                                                "kanban-mode-buttons",
                                                [
                                                    ToggleButtonSimple::new(
                                                        "Browse",
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.set_mode(PanelMode::Browse, cx);
                                                        }),
                                                    ),
                                                    ToggleButtonSimple::new(
                                                        "Steer",
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.set_mode(PanelMode::Steer, cx);
                                                        }),
                                                    ),
                                                ],
                                            )
                                            .style(ToggleButtonGroupStyle::Outlined)
                                            .size(ToggleButtonGroupSize::Custom(rems_from_px(
                                                30.0_f32,
                                            )))
                                            .label_size(LabelSize::Default)
                                            .auto_width()
                                            .selected_index(match mode {
                                                PanelMode::Browse => 0,
                                                PanelMode::Steer => 1,
                                            })
                                            .into_any_element(),
                                        ),
                                    )
                                    .when(mode == PanelMode::Browse, |this| {
                                        this.child(self.render_toolbar(cx))
                                    }),
                            ),
                    )
                    .when(mode == PanelMode::Browse, |this| {
                        this.when_some(self.render_board_selector(cx), |this, selector| {
                            this.child(selector)
                        })
                        .when_some(self.render_error(), |this, error| this.child(error))
                        .when_some(self.render_action_form(cx), |this, form| this.child(form))
                        .when_some(self.render_task_actions(cx), |this, actions| {
                            this.child(actions)
                        })
                    }),
            )
            .child(v_flex().px_4().size_full().overflow_y_hidden().map(|this| {
                if mode == PanelMode::Steer {
                    if let Some(conversation) = &self.steer_conversation {
                        this.child(conversation.clone()).into_any_element()
                    } else {
                        this.child(Label::new("Initializing Steer mode…").color(Color::Muted))
                            .into_any_element()
                    }
                } else if self.fetching && self.kanban_widget.is_none() {
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

#[cfg(test)]
mod tests {
    use super::{
        ADVERTISED_KANBAN_TOOLS, BOARD_CREATE_TOOL, BOARD_DELETE_TOOL, BOARD_IMPORT_TOOL,
        IDEMPOTENT_TOOLS, MAX_MUTATION_RETRIES, RefreshTarget, TASK_CREATE_TOOL, TASK_DELETE_TOOL,
        TASK_SPAWN_TOOL, TASK_UPDATE_TOOL, attach_idempotency_key, classify_kanban_fetch_error,
        is_idempotent_tool, mutation_retry_delay, refresh_target, steer_system_prompt,
    };
    use hkask_tool_invoker::InvokeError;
    use std::time::Duration;

    // ── Idempotency keys ────────────────────────────────────────────────────
    //
    // A key makes an interrupted create safe to retry: the server absorbs the
    // replay and returns the original result. The key must be per *gesture*, not
    // per attempt, or every retry looks like new work and the protection is
    // worthless.

    /// Only the tools that mint a fresh server-side identity carry keys.
    ///
    /// Guards against drift in both directions. Adding a tool here that the
    /// server does not protect would make an interrupted call *look* replay-safe
    /// while actually duplicating — the exact bug the keys exist to prevent.
    #[test]
    fn idempotent_tools_are_exactly_the_identity_minting_creates() {
        assert!(is_idempotent_tool(TASK_CREATE_TOOL));
        assert!(is_idempotent_tool(BOARD_CREATE_TOOL));
        assert!(is_idempotent_tool(BOARD_IMPORT_TOOL));
        assert!(is_idempotent_tool(TASK_SPAWN_TOOL));
        assert_eq!(
            IDEMPOTENT_TOOLS.len(),
            4,
            "the protected set must match the server's `with_idempotency` wiring; \
             adding one here without wiring the server would make an interrupted \
             call look replay-safe while actually duplicating"
        );
    }

    /// Already-idempotent mutations carry no key.
    ///
    /// They converge on the same state (`task_update`) or are no-ops when
    /// repeated (`task_delete`), so a key would be dead weight suggesting a
    /// guarantee the server was never asked for.
    #[test]
    fn convergent_mutations_do_not_carry_keys() {
        for tool in [TASK_UPDATE_TOOL, TASK_DELETE_TOOL, BOARD_DELETE_TOOL] {
            assert!(
                !is_idempotent_tool(tool),
                "{tool} is already idempotent by construction and needs no key"
            );
        }
    }

    /// A protected tool gets a key; an unprotected one is left untouched.
    #[test]
    fn key_is_attached_only_for_protected_tools() {
        let protected =
            attach_idempotency_key(TASK_CREATE_TOOL, serde_json::json!({ "title": "x" }))
                .expect("object args accept a key");
        let key = protected
            .get("idempotency_key")
            .and_then(|k| k.as_str())
            .expect("a protected create must carry a key");
        assert!(!key.trim().is_empty(), "the key must be non-empty");
        // Original arguments survive.
        assert_eq!(protected.get("title").and_then(|t| t.as_str()), Some("x"));

        let unprotected =
            attach_idempotency_key(TASK_UPDATE_TOOL, serde_json::json!({ "task_id": "t" }))
                .expect("pass-through");
        assert!(
            unprotected.get("idempotency_key").is_none(),
            "an unprotected tool must not be sent a key the server ignores"
        );
    }

    /// Each gesture gets a distinct key.
    ///
    /// Two separate operator gestures must not collide, or the second would be
    /// silently absorbed as a replay of the first and the operator's work would
    /// vanish.
    #[test]
    fn separate_gestures_get_distinct_keys() {
        let first = attach_idempotency_key(TASK_CREATE_TOOL, serde_json::json!({})).unwrap();
        let second = attach_idempotency_key(TASK_CREATE_TOOL, serde_json::json!({})).unwrap();
        assert_ne!(
            first.get("idempotency_key"),
            second.get("idempotency_key"),
            "distinct gestures must get distinct keys, or the second would be \
             absorbed as a replay of the first"
        );
    }

    /// Non-object args are reported, not silently unprotected.
    ///
    /// Dropping the key silently would leave the retry path believing a call was
    /// replay-protected when it was not — the worst of both behaviors.
    #[test]
    fn non_object_args_are_reported_rather_than_silently_unprotected() {
        let outcome = attach_idempotency_key(TASK_CREATE_TOOL, serde_json::json!("not-an-object"));
        assert!(
            outcome.is_err(),
            "a key that cannot be attached must surface, not vanish"
        );
    }

    /// An interrupted call is retryable only for the protected tools.
    ///
    /// This is the composition the dispatch loop relies on: `is_retryable()`
    /// stays false for `Interrupted` (it is unsafe in general), and the panel
    /// widens it only where a server-side key makes the replay safe.
    #[test]
    fn interrupted_is_retryable_only_for_replay_safe_tools() {
        // Mirrors the guard in `dispatch_mutation`'s retry arm. Kept as one
        // expression so the test fails if either half of the condition drifts,
        // rather than asserting each half in isolation (where
        // `A && B` / `!A || B` shapes can be trivially true).
        let should_retry = |error: &InvokeError, tool: &str| {
            error.is_retryable() || (is_idempotent_tool(tool) && error.is_outcome_unknown())
        };

        let interrupted = InvokeError::Interrupted("connection reset".into());
        assert!(
            !interrupted.is_retryable(),
            "the transport-level verdict must stay conservative on its own"
        );
        assert!(
            should_retry(&interrupted, TASK_CREATE_TOOL),
            "an interrupted create IS retryable: the shared key makes the server \
             absorb the replay"
        );
        assert!(
            !should_retry(&interrupted, TASK_UPDATE_TOOL),
            "an interrupted call on an unprotected tool must NOT be retried"
        );
        // A clean failure is never retried, protected or not.
        let failed = InvokeError::Failed("rejected".into());
        assert!(!should_retry(&failed, TASK_CREATE_TOOL));
        assert!(!should_retry(&failed, TASK_UPDATE_TOOL));
    }

    // ── Mutation retry policy ─────────────────────────────────────────
    //
    // Every mutation used to surface one transport failure as terminal, so a
    // routine MCP server restart read as "Failed to create task" even though
    // nothing had been attempted. `dispatch_mutation` retries — but only where a
    // retry cannot duplicate a side effect.

    /// Mutation backoff doubles and is bounded.
    #[test]
    fn mutation_retry_backs_off_then_gives_up() {
        assert_eq!(mutation_retry_delay(0), Some(Duration::from_millis(250)));
        assert_eq!(mutation_retry_delay(1), Some(Duration::from_millis(500)));
        assert_eq!(mutation_retry_delay(2), Some(Duration::from_millis(1000)));
        assert_eq!(
            mutation_retry_delay(MAX_MUTATION_RETRIES),
            None,
            "the mutation budget must be finite so a form submit resolves or reports"
        );
    }

    /// The mutation budget resolves faster than the read-path budget.
    ///
    /// A mutation is a direct operator gesture behind a form that has already
    /// closed, so it must not retry quietly for as long as a background list
    /// refresh legitimately can.
    #[test]
    fn mutation_retries_resolve_faster_than_read_retries() {
        let mutation_total: Duration = (0..MAX_MUTATION_RETRIES)
            .filter_map(mutation_retry_delay)
            .sum();
        assert!(
            mutation_total <= Duration::from_secs(2),
            "a state-changing gesture must resolve or report within ~2s, got {mutation_total:?}"
        );
    }

    // The kanban server returns tool errors as an Ok string carrying the
    // `{"error": ..., "kind": ...}` envelope (McpToolError::to_json_string),
    // not as an Err from invoke_tool. Before the error-envelope seam, a
    // `failed_precondition` (DB not initialized) or `unavailable` fell through
    // to the BoardListResponse/TaskListResponse parse and surfaced as the
    // misleading "Failed to parse … response: {…}". These tests pin the
    // classification helper and the seam-level detector so a regression in
    // either surfaces here.
    #[test]
    fn classify_kanban_fetch_error_retryable_reads_as_reconnecting() {
        let msg = classify_kanban_fetch_error(true, "transport closed");
        assert!(
            msg.starts_with("Reconnecting to the kanban server…"),
            "retryable errors read as a transient reconnect, got {msg}"
        );
    }

    #[test]
    fn classify_kanban_fetch_error_non_retryable_passes_message_through() {
        // A non-retryable server error (e.g. failed_precondition) surfaces the
        // server's message verbatim — the operator sees the real cause, not a
        // generic "Failed to parse …".
        let msg = classify_kanban_fetch_error(false, "kanban database not initialized");
        assert_eq!(msg, "kanban database not initialized");
    }

    #[test]
    fn fetch_paths_detect_server_error_envelope_before_typed_parse() {
        // The exact wire format pinned by `error_wire_format_golden_strings`
        // in hkask-mcp-server. A failed_precondition is the canonical kanban
        // case (DB not initialized at first launch).
        let out = r#"{"error":"kanban database not initialized","kind":"failed_precondition"}"#;
        let err = hkask_types::tool_response::parse_tool_error(out)
            .expect("server error envelope must be detected");
        assert_eq!(err.message, "kanban database not initialized");
        assert_eq!(
            err.kind,
            Some(hkask_types::McpErrorKind::FailedPrecondition)
        );
        assert!(!err.is_retryable());
        // A successful payload must NOT be misclassified as an error envelope.
        let ok = r#"{"content":{"boards":[]}}"#;
        assert!(hkask_types::tool_response::parse_tool_error(ok).is_none());
    }

    /// A mutation whose outcome is unknown must NOT be retried.
    ///
    /// This is the duplicate-side-effect guard at the panel layer. `Interrupted`
    /// means the request reached the server and the connection dropped before the
    /// response, so the task/board/spawn may already exist. Retrying would create
    /// a second one; the panel refreshes and asks the operator to look instead.
    #[test]
    fn interrupted_mutation_is_not_retried_and_is_flagged_unknown() {
        let interrupted = InvokeError::Interrupted("connection reset".into());
        assert!(
            !interrupted.is_retryable(),
            "retrying an interrupted mutation could create a duplicate task, board, or spawn"
        );
        assert!(
            interrupted.is_outcome_unknown(),
            "the panel relies on this to force a refresh and warn the operator"
        );
    }

    /// A provably-undelivered mutation is retryable, and is not flagged unknown.
    #[test]
    fn undelivered_mutation_is_retryable_and_not_flagged_unknown() {
        let unavailable = InvokeError::Unavailable("no live connection".into());
        assert!(unavailable.is_retryable());
        assert!(
            !unavailable.is_outcome_unknown(),
            "a request that never left has a known outcome: nothing happened"
        );
    }

    /// Board mutations must refresh the board list, not the task list.
    ///
    /// Deleting the selected board and then fetching *tasks* would query a board
    /// that no longer exists, so the refresh target is part of the contract.
    #[test]
    fn board_and_task_refresh_targets_are_distinct() {
        assert_ne!(RefreshTarget::Boards, RefreshTarget::Tasks);
    }

    /// A refresh tick with no board selected must re-fetch the *board list*.
    ///
    /// Regression for the sticky-empty-panel bug: the loop used to `continue`
    /// whenever `selected_board_id` was `None`, so a `kanban_board_list` that
    /// failed at construction (MCP server still starting, or restarting after a
    /// settings change) was never retried — the panel stayed empty for the rest
    /// of the session even after the server came back.
    #[test]
    fn refresh_retries_the_board_list_when_no_board_is_selected() {
        assert_eq!(
            refresh_target(false),
            RefreshTarget::Boards,
            "without a board, the tick must retry the board list so the panel recovers \
             once the MCP server is reachable again"
        );
    }

    /// With a board selected, the tick refreshes that board's tasks.
    #[test]
    fn refresh_polls_tasks_once_a_board_is_selected() {
        assert_eq!(refresh_target(true), RefreshTarget::Tasks);
    }

    /// Every `kanban_*`/`contract_*` token the Steer prompt names in backticks
    /// must be a tool the server actually exposes. Without this, a rename in
    /// `hkask-mcp-kata-kanban` degrades silently to "tool not found" at dispatch
    /// time instead of failing here. Mirrors the swarm panel's
    /// `steer_prompt_mentions_only_known_tools`.
    #[test]
    fn steer_prompt_advertises_only_known_tools() {
        for prompt in [
            steer_system_prompt(Some("board-1")),
            steer_system_prompt(None),
        ] {
            // Backtick-delimited spans sit at odd indices when splitting on '`'.
            let advertised = prompt
                .split('`')
                .skip(1)
                .step_by(2)
                .map(|span| {
                    span.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .filter(|name| name.starts_with("kanban_") || name.starts_with("contract_"));

            for name in advertised {
                assert!(
                    ADVERTISED_KANBAN_TOOLS.contains(&name.as_str()),
                    "steer prompt advertises `{name}`, which is not in \
                     ADVERTISED_KANBAN_TOOLS — either the tool was renamed in \
                     hkask-mcp-kata-kanban or the prompt names a tool that does \
                     not exist"
                );
            }
        }
    }

    /// The Steer prompt bakes the active board id in at construction, so the
    /// board id must appear in the prompt for the staleness concern to be real —
    /// and `select_board` must drop the conversation (see its `take()` call) so a
    /// board switch rebuilds it. This pins the first half (the prompt is
    /// board-scoped); the `take()` is the fix for the second.
    #[test]
    fn steer_prompt_is_scoped_to_the_selected_board() {
        let with_board = steer_system_prompt(Some("board-alpha"));
        assert!(
            with_board.contains("board-alpha"),
            "prompt must name the active board, otherwise the curator writes to \
             an unspecified board: {with_board}"
        );

        // A different selection must produce a different prompt — if it didn't,
        // dropping the conversation on switch would be pointless.
        let other_board = steer_system_prompt(Some("board-beta"));
        assert_ne!(
            with_board.as_ref(),
            other_board.as_ref(),
            "prompt must differ per board, or the rebuild-on-switch is a no-op"
        );

        // With no board selected the prompt must not invent one.
        let no_board = steer_system_prompt(None);
        assert!(
            !no_board.contains("The active board is"),
            "prompt must not claim an active board when none is selected: {no_board}"
        );
    }

    /// The allowlist is only meaningful if the prompt actually exercises it, and
    /// only correct if it has no duplicates.
    #[test]
    fn advertised_kanban_tools_are_unique_and_referenced() {
        let mut sorted = ADVERTISED_KANBAN_TOOLS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            before,
            "duplicate entries in ADVERTISED_KANBAN_TOOLS"
        );

        let prompt = steer_system_prompt(Some("board-1"));
        for tool in ADVERTISED_KANBAN_TOOLS {
            assert!(
                prompt.contains(tool),
                "ADVERTISED_KANBAN_TOOLS lists `{tool}` but the Steer prompt \
                 never mentions it — drop it from the list or advertise it"
            );
        }
    }
}
