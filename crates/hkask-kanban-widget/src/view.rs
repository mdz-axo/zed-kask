//! The `KanbanWidget` GPUI view — renders a horizontal kanban column layout
//! (Backlog, Ready, In Progress, Review, Done) from parsed block data.
//!
//! This is a passive renderer: the data comes from the parsed `KanbanBlockBody`
//! (JSON already in the chat stream), not from `ToolInvoker` MCP fetches. The
//! widget is read-only — task moves are not supported (the agent calls
//! `kanban_task_move` directly).
//!
//! Rendering mirrors the deleted `KanbanBoardView`: an `h_flex` of columns,
//! each column has a header label (status name + task count) and a vertical
//! list of task cards. Non-standard statuses are appended after the five
//! standard columns, sorted alphabetically.

use std::collections::{HashMap, HashSet};

use gpui::{FocusHandle, Focusable};
use theme::ActiveTheme;
use ui::prelude::*;

use crate::block::{KanbanBlockBody, TaskBody};

/// Standard kanban statuses in display order (matches `TaskStatus` in the
/// kata-kanban service). The string values are the wire-format status strings
/// used by the MCP server.
const STANDARD_STATUSES: &[(&str, &str)] = &[
    ("backlog", "Backlog"),
    ("ready", "Ready"),
    ("in_progress", "In Progress"),
    ("review", "Review"),
    ("done", "Done"),
];

/// One column in the kanban board.
struct KanbanColumn {
    #[allow(dead_code)]
    status: String,
    title: String,
    tasks: Vec<TaskBody>,
}

/// The kanban widget view. Renders inline in agent markdown (via the D18 seam
/// composed by `hkask-viz-core`).
pub struct KanbanWidget {
    board_name: String,
    columns: Vec<KanbanColumn>,
    focus_handle: FocusHandle,
}

impl KanbanWidget {
    /// Create a new kanban widget for the parsed block body.
    ///
    /// The widget renders one board at a time. If the block has multiple
    /// boards, the first one is rendered (the agent can emit multiple blocks
    /// for multiple boards).
    pub fn new(body: KanbanBlockBody, cx: &mut Context<Self>) -> Self {
        let boards = body.boards_with_tasks();
        let (board_name, tasks) = if let Some((_, name, tasks)) = boards.first() {
            (name.clone(), tasks.to_vec())
        } else {
            ("Kanban Board".to_string(), Vec::new())
        };
        let columns = group_tasks_into_columns(tasks);

        Self {
            board_name,
            columns,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_header(&self) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new(self.board_name.clone()).size(LabelSize::Large))
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

    fn render_empty_state(&self) -> Option<AnyElement> {
        if self.columns.iter().all(|column| column.tasks.is_empty()) {
            Some(
                Label::new("No tasks on this board.")
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
/// appending any non-standard statuses at the end (sorted alphabetically).
/// Case-insensitive matching: "BACKLOG" and "backlog" both map to the
/// "backlog" column.
fn group_tasks_into_columns(tasks: Vec<TaskBody>) -> Vec<KanbanColumn> {
    let mut by_status: HashMap<String, Vec<TaskBody>> = HashMap::new();
    for task in tasks {
        by_status
            .entry(task.status.to_lowercase())
            .or_default()
            .push(task);
    }

    let mut columns = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();

    for (status_key, title) in STANDARD_STATUSES {
        seen.insert(status_key);
        let tasks = by_status.remove(*status_key).unwrap_or_default();
        columns.push(KanbanColumn {
            status: status_key.to_string(),
            title: title.to_string(),
            tasks,
        });
    }

    // Any remaining non-standard statuses, sorted for stable display.
    let mut extra: Vec<(String, Vec<TaskBody>)> = by_status.into_iter().collect();
    extra.sort_by(|a, b| a.0.cmp(&b.0));
    for (status_key, tasks) in extra {
        // Title-case the status for display.
        let title = status_key
            .chars()
            .enumerate()
            .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
            .collect::<String>();
        columns.push(KanbanColumn {
            status: status_key,
            title,
            tasks,
        });
    }

    columns
}

impl Focusable for KanbanWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for KanbanWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(self.render_header())
            .children(self.render_empty_state())
            .child(self.render_columns(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(task_id: &str, title: &str, status: &str) -> TaskBody {
        TaskBody {
            task_id: task_id.to_string(),
            title: title.to_string(),
            status: status.to_string(),
            assignee: None,
            gas_remaining: None,
        }
    }

    #[test]
    fn group_tasks_into_columns_preserves_standard_order() {
        let tasks = vec![
            task("t1", "Backlog task", "backlog"),
            task("t2", "In progress", "in_progress"),
            task("t3", "Done", "done"),
        ];
        let columns = group_tasks_into_columns(tasks);
        assert_eq!(columns.len(), 5);
        assert_eq!(columns[0].status, "backlog");
        assert_eq!(columns[0].tasks.len(), 1);
        assert_eq!(columns[2].status, "in_progress");
        assert_eq!(columns[2].tasks.len(), 1);
        assert_eq!(columns[4].status, "done");
        assert_eq!(columns[4].tasks.len(), 1);
    }

    #[test]
    fn group_tasks_into_columns_appends_extra_statuses() {
        let tasks = vec![task("t1", "Blocked", "blocked")];
        let columns = group_tasks_into_columns(tasks);
        // 5 standard (all empty) + 1 extra.
        assert_eq!(columns.len(), 6);
        assert_eq!(columns[5].status, "blocked");
        assert_eq!(columns[5].tasks.len(), 1);
    }

    #[test]
    fn group_tasks_into_columns_normalizes_case() {
        let tasks = vec![
            task("t1", "Upper", "BACKLOG"),
            task("t2", "Lower", "backlog"),
        ];
        let columns = group_tasks_into_columns(tasks);
        assert_eq!(columns[0].status, "backlog");
        assert_eq!(columns[0].tasks.len(), 2);
    }

    #[test]
    fn empty_tasks_produce_five_empty_columns() {
        let columns = group_tasks_into_columns(Vec::new());
        assert_eq!(columns.len(), 5);
        for column in &columns {
            assert!(column.tasks.is_empty());
        }
    }
}
