//! Kanban board view — a center-pane `Item` that visualizes the
//! `kata-kanban` MCP server's boards as a horizontal column layout.
//!
//! Fetches board + task data via the global `ToolInvoker` hook (the same
//! hook the chat-based `KaskPanel` uses). Read-only visualization for now;
//! task moves can be added later by calling `kanban_task_move`.

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Task,
    WeakEntity, Window, prelude::*,
};
use serde::Deserialize;
use serde_json::json;
use ui::prelude::*;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem, TabContentParams},
};

use crate::kanban_tool_invoker;

/// The MCP server name (matches `BUILT_IN_MCP_SERVERS`).
const KATA_KANBAN_SERVER: &str = "kata-kanban";

/// Standard kanban statuses in display order (matches `TaskStatus` in the
/// kanban service). The string values are the wire-format status strings
/// used by the MCP server.
const STANDARD_STATUSES: &[(&str, &str)] = &[
    ("backlog", "Backlog"),
    ("ready", "Ready"),
    ("in_progress", "In Progress"),
    ("review", "Review"),
    ("done", "Done"),
];

// ── MCP response structs (minimal, mirror the server's types) ────────────

#[derive(Debug, Deserialize)]
struct BoardListResponse {
    boards: Vec<BoardInfo>,
}

#[derive(Debug, Deserialize)]
struct BoardInfo {
    board_id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct TaskListResponse {
    tasks: Vec<TaskInfo>,
}

#[derive(Debug, Deserialize)]
struct TaskInfo {
    task_id: String,
    title: String,
    status: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    gas_remaining: Option<u64>,
}

// ── View model ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct BoardSummary {
    id: String,
    name: String,
}

struct KanbanColumn {
    #[allow(dead_code)]
    status: String,
    title: String,
    tasks: Vec<TaskCard>,
}

struct TaskCard {
    #[allow(dead_code)]
    id: String,
    title: String,
    assignee: Option<String>,
    gas_remaining: Option<u64>,
}

/// A center-pane kanban board visualization backed by the `kata-kanban`
/// MCP server.
pub struct KanbanBoardView {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    boards: Vec<BoardSummary>,
    selected_board: usize,
    columns: Vec<KanbanColumn>,
    loading: bool,
    error: Option<String>,
}

impl KanbanBoardView {
    /// Create a new kanban board view and kick off the initial board fetch.
    pub fn new(
        workspace: &Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| {
            let mut view = Self {
                _workspace: workspace.weak_handle(),
                focus_handle: cx.focus_handle(),
                boards: Vec::new(),
                selected_board: 0,
                columns: Vec::new(),
                loading: true,
                error: None,
            };
            // Defer the initial fetch so the entity is fully constructed first.
            view.fetch_boards(cx);
            let _ = window;
            view
        })
    }

    /// Fetch the list of boards from the kanban MCP server.
    fn fetch_boards(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kanban_tool_invoker() else {
            self.set_error(
                "Tool invoker not wired — set_tool_invoker() not called.".to_string(),
                cx,
            );
            return;
        };

        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(KATA_KANBAN_SERVER, "kanban_board_list", json!({}))
                .await;
            this.update(cx, |this, cx| match result {
                Ok(output) => match serde_json::from_str::<BoardListResponse>(&output) {
                    Ok(response) => {
                        this.boards = response
                            .boards
                            .into_iter()
                            .map(|b| BoardSummary {
                                id: b.board_id,
                                name: b.name,
                            })
                            .collect();
                        this.error = None;
                        this.loading = false;
                        if this.boards.is_empty() {
                            this.columns.clear();
                        } else {
                            // Fetch tasks for the first board.
                            let board_id = this.boards[0].id.clone();
                            this.fetch_tasks(&board_id, cx);
                        }
                        cx.notify();
                    }
                    Err(err) => {
                        this.set_error(
                            format!("Failed to parse board list: {err}\nRaw: {output}"),
                            cx,
                        );
                    }
                },
                Err(err) => {
                    this.set_error(format!("Failed to list boards: {err}"), cx);
                }
            })
        })
        .detach();
    }

    /// Fetch tasks for a specific board and group them into columns.
    fn fetch_tasks(&mut self, board_id: &str, cx: &mut Context<Self>) {
        let Some(invoker) = kanban_tool_invoker() else {
            self.set_error(
                "Tool invoker not wired — set_tool_invoker() not called.".to_string(),
                cx,
            );
            return;
        };

        self.set_loading(cx);
        let board_id = board_id.to_string();

        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    KATA_KANBAN_SERVER,
                    "kanban_task_list",
                    json!({ "board_id": board_id }),
                )
                .await;
            this.update(cx, |this, cx| match result {
                Ok(output) => match serde_json::from_str::<TaskListResponse>(&output) {
                    Ok(response) => {
                        this.columns = group_tasks_into_columns(response.tasks);
                        this.error = None;
                        this.loading = false;
                        cx.notify();
                    }
                    Err(err) => {
                        this.set_error(
                            format!("Failed to parse task list: {err}\nRaw: {output}"),
                            cx,
                        );
                    }
                },
                Err(err) => {
                    this.set_error(format!("Failed to list tasks: {err}"), cx);
                }
            })
        })
        .detach();
    }

    fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        self.loading = false;
        cx.notify();
    }

    fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        cx.notify();
    }

    fn select_board(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.selected_board || index >= self.boards.len() {
            return;
        }
        self.selected_board = index;
        let board_id = self.boards[index].id.clone();
        self.fetch_tasks(&board_id, cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(board) = self.boards.get(self.selected_board) {
            let board_id = board.id.clone();
            self.fetch_tasks(&board_id, cx);
        } else {
            self.fetch_boards(cx);
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let board_buttons: Vec<AnyElement> = self
            .boards
            .iter()
            .enumerate()
            .map(|(index, board)| {
                let is_selected = index == self.selected_board;
                Button::new(("board-btn", index), board.name.clone())
                    .style(if is_selected {
                        ButtonStyle::Tinted(ui::TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_board(index, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new("Kanban Board").size(LabelSize::Large))
            .child(div().flex_1())
            .child(h_flex().gap_1().flex_wrap().children(board_buttons))
            .child(
                Button::new("refresh-btn", "Refresh")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh(cx);
                    })),
            )
    }

    fn render_columns(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let card_bg = cx.theme().colors().editor_background;

        let column_elements: Vec<AnyElement> = self
            .columns
            .iter()
            .map(|column| {
                let count = column.tasks.len();
                let cards: Vec<AnyElement> = column
                    .tasks
                    .iter()
                    .map(|task| {
                        v_flex()
                            .w_full()
                            .p_2()
                            .gap_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(border_color)
                            .bg(card_bg)
                            .child(Label::new(task.title.clone()).size(LabelSize::Small))
                            .when_some(task.assignee.clone(), |this, assignee| {
                                this.child(
                                    Label::new(format!("@{assignee}"))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                            })
                            .when_some(task.gas_remaining, |this, gas| {
                                this.child(
                                    Label::new(format!("⛽ {gas}"))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                            })
                            .into_any_element()
                    })
                    .collect();

                v_flex()
                    .w(px(220.))
                    .min_w(px(220.))
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .justify_between()
                            .child(
                                Label::new(column.title.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(format!("{count}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .children(cards)
                    .into_any_element()
            })
            .collect();

        h_flex()
            .id("kanban-columns")
            .gap_2()
            .overflow_x_scroll()
            .flex_1()
            .min_h_0()
            .children(column_elements)
    }

    fn render_status(&self) -> Option<AnyElement> {
        if self.loading {
            Some(
                Label::new("Loading…")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element(),
            )
        } else if let Some(error) = &self.error {
            Some(
                Label::new(error.clone())
                    .size(LabelSize::Small)
                    .color(Color::Warning)
                    .into_any_element(),
            )
        } else if self.boards.is_empty() {
            Some(
                Label::new("No boards found. Create one via the kata-kanban server.")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element(),
            )
        } else {
            None
        }
    }
}

/// Group tasks into columns by status, preserving the standard order and
/// appending any non-standard statuses at the end.
fn group_tasks_into_columns(tasks: Vec<TaskInfo>) -> Vec<KanbanColumn> {
    use std::collections::HashMap;

    let mut by_status: HashMap<String, Vec<TaskCard>> = HashMap::new();
    for task in tasks {
        let card = TaskCard {
            id: task.task_id,
            title: task.title,
            assignee: task.assignee,
            gas_remaining: task.gas_remaining,
        };
        by_status
            .entry(task.status.to_lowercase())
            .or_default()
            .push(card);
    }

    let mut columns = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (status_key, title) in STANDARD_STATUSES {
        seen.insert(status_key);
        if let Some(cards) = by_status.remove(*status_key) {
            columns.push(KanbanColumn {
                status: status_key.to_string(),
                title: title.to_string(),
                tasks: cards,
            });
        } else {
            columns.push(KanbanColumn {
                status: status_key.to_string(),
                title: title.to_string(),
                tasks: Vec::new(),
            });
        }
    }

    // Any remaining non-standard statuses, sorted for stable display.
    let mut extra: Vec<(String, Vec<TaskCard>)> = by_status.into_iter().collect();
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    for (status_key, cards) in extra {
        // Title-case the status for display.
        let title = status_key
            .chars()
            .enumerate()
            .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
            .collect::<String>();
        columns.push(KanbanColumn {
            status: status_key,
            title,
            tasks: cards,
        });
    }

    columns
}

impl Focusable for KanbanBoardView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<ItemEvent> for KanbanBoardView {}

impl Item for KanbanBoardView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> gpui::SharedString {
        "Kanban Board".into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), _cx))
            .color(params.text_color())
            .into_any_element()
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<gpui::SharedString> {
        Some("Kanban Board — kata-kanban visualization".into())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Kanban Board Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(_event: &Self::Event, _f: &mut dyn FnMut(ItemEvent)) {}
}

impl SerializableItem for KanbanBoardView {
    fn serialized_item_kind() -> &'static str {
        "KanbanBoardView"
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
                KanbanBoardView::new(workspace, window, cx)
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
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl Render for KanbanBoardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(self.render_header(cx))
            .children(self.render_status())
            .child(self.render_columns(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_tasks_into_columns_preserves_standard_order() {
        let tasks = vec![
            TaskInfo {
                task_id: "t1".into(),
                title: "Backlog task".into(),
                status: "backlog".into(),
                assignee: None,
                gas_remaining: None,
            },
            TaskInfo {
                task_id: "t2".into(),
                title: "In progress".into(),
                status: "in_progress".into(),
                assignee: Some("alice".into()),
                gas_remaining: Some(100),
            },
            TaskInfo {
                task_id: "t3".into(),
                title: "Done".into(),
                status: "done".into(),
                assignee: None,
                gas_remaining: None,
            },
        ];

        let columns = group_tasks_into_columns(tasks);
        assert_eq!(columns.len(), 5);
        assert_eq!(columns[0].status, "backlog");
        assert_eq!(columns[0].tasks.len(), 1);
        assert_eq!(columns[2].status, "in_progress");
        assert_eq!(columns[2].tasks.len(), 1);
        assert_eq!(columns[2].tasks[0].assignee.as_deref(), Some("alice"));
        assert_eq!(columns[4].status, "done");
        assert_eq!(columns[4].tasks.len(), 1);
    }

    #[test]
    fn group_tasks_into_columns_appends_extra_statuses() {
        let tasks = vec![TaskInfo {
            task_id: "t1".into(),
            title: "Blocked".into(),
            status: "blocked".into(),
            assignee: None,
            gas_remaining: None,
        }];

        let columns = group_tasks_into_columns(tasks);
        // 5 standard (all empty) + 1 extra.
        assert_eq!(columns.len(), 6);
        assert_eq!(columns[5].status, "blocked");
        assert_eq!(columns[5].tasks.len(), 1);
    }

    #[test]
    fn group_tasks_into_columns_normalizes_case() {
        let tasks = vec![
            TaskInfo {
                task_id: "t1".into(),
                title: "Upper".into(),
                status: "BACKLOG".into(),
                assignee: None,
                gas_remaining: None,
            },
            TaskInfo {
                task_id: "t2".into(),
                title: "Lower".into(),
                status: "backlog".into(),
                assignee: None,
                gas_remaining: None,
            },
        ];

        let columns = group_tasks_into_columns(tasks);
        assert_eq!(columns[0].status, "backlog");
        assert_eq!(columns[0].tasks.len(), 2);
    }
}
