//! The `KanbanWidget` GPUI view — renders a horizontal kanban column layout
//! (Backlog, Ready, In Progress, Review, Done) from parsed block data.
//!
//! Data comes from the parsed `KanbanBlockBody` (JSON already in the chat
//! stream), not from `ToolInvoker` MCP fetches — the widget is a passive
//! renderer of the board. T6 (widget sovereignty) adds a per-card move
//! affordance: a clickable status chip that dispatches `kanban_task_move`
//! via the governed `shared_tool_invoker()` (OCAP/gas-budgeted in production
//! via `McpRuntime`). A missing invoker and non-dispatchable provenance are
//! surfaced as visible states, never silent no-ops (repo `.rules`). The
//! authoritative board state arrives with the next agent-emitted block; a
//! successful move only mutates the local cached view for immediate feedback.
//!
//! Rendering mirrors the deleted `KanbanBoardView`: an `h_flex` of columns,
//! each column has a header label (status name + task count) and a vertical
//! list of task cards. Non-standard statuses are appended after the five
//! standard columns, sorted alphabetically.

use std::collections::{HashMap, HashSet};

use gpui::{FocusHandle, Focusable, Hsla};
use gpui_util::ResultExt as _;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
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

/// MCP server that hosts the kanban tools. Fallback dispatch target when a
/// block carries no dispatchable provenance.
const DEFAULT_SERVER: &str = "hkask-mcp-kata-kanban";
/// Tool the widget dispatches to move a task. Confirmed against the server's
/// `TaskMoveRequest { task_id, target_status }` schema (no `board_id`).
const DEFAULT_TOOL: &str = "kanban_task_move";
/// Surfaced when the process-global `ToolInvoker` is not wired. Visible state,
/// not a silent no-op (repo `.rules` startup-failure-signal trap).
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";
/// Surfaced when provenance is partial (non-dispatchable but not empty): the
/// widget refuses to dispatch against the wrong server and asks the user to
/// route through the agent.
const PROVENANCE_INCOMPLETE_MSG: &str = "provenance incomplete — ask the agent";
/// Surfaced when a card carries no dispatchable `task_id`.
const MISSING_TASK_ID_MSG: &str = "missing task_id";
/// Surfaced when the target status is not one of the five standard wire
/// strings the MCP server accepts.
const INVALID_TARGET_STATUS_MSG: &str = "invalid target status";

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
    /// Server-authoritative provenance copied from the parsed block body. The
    /// move affordance uses it to pick the dispatch server and to decide
    /// whether to show an active affordance or a disabled "ask the agent"
    /// hint.
    provenance: BlockProvenance,
    focus_handle: FocusHandle,
    /// `task_id` currently being moved, if a dispatch is in flight. Single
    /// flight: while set, all move affordances are non-interactive.
    dispatch_in_flight: Option<String>,
    /// Visible error/hint when dispatch cannot proceed (missing invoker,
    /// provenance incomplete, missing task_id, tool error). Never silently
    /// dropped (repo `.rules`).
    dispatch_error: Option<String>,
}

impl KanbanWidget {
    /// Create a new kanban widget for the parsed block body.
    ///
    /// The widget renders one board at a time. If the block has multiple
    /// boards, the first one is rendered (the agent can emit multiple blocks
    /// for multiple boards).
    pub fn new(body: KanbanBlockBody, cx: &mut Context<Self>) -> Self {
        hkask_tool_invoker::record_render(
            body.provenance.tool.clone(),
            body.provenance.span_id.clone(),
        );
        tracing::info!(
            target: "reg.widget.render",
            tool = body.provenance.tool.as_deref().unwrap_or(""),
            span_id = body.provenance.span_id.as_deref().unwrap_or(""),
            "REG",
        );
        let boards = body.boards_with_tasks();
        let (board_name, tasks) = if let Some((_, name, tasks)) = boards.first() {
            (name.clone(), tasks.to_vec())
        } else {
            ("Kanban Board".to_string(), Vec::new())
        };
        let columns = group_tasks_into_columns(tasks);
        let provenance = body.provenance.clone();

        Self {
            board_name,
            columns,
            provenance,
            focus_handle: cx.focus_handle(),
            dispatch_in_flight: None,
            dispatch_error: None,
        }
    }

    fn render_header(&self) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new(self.board_name.clone()).size(LabelSize::Large))
    }

    fn render_dispatch_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(task_id) = &self.dispatch_in_flight {
            return Some(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Moving {task_id} …"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.dispatch_error {
            let border_color = cx.theme().colors().border;
            return Some(
                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(error.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                    .into_any_element(),
            );
        }
        None
    }

    fn render_columns(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let card_bg = cx.theme().colors().editor_background;
        let in_flight_any = self.dispatch_in_flight.is_some();

        let column_elements: Vec<AnyElement> = self
            .columns
            .iter()
            .map(|column| {
                let count = column.tasks.len();
                let cards: Vec<AnyElement> = column
                    .tasks
                    .iter()
                    .map(|task| self.render_card(task, border_color, card_bg, in_flight_any, cx))
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

    fn render_card(
        &self,
        task: &TaskBody,
        border_color: Hsla,
        card_bg: Hsla,
        in_flight_any: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .child(self.render_move_affordance(task, in_flight_any, cx))
            .into_any_element()
    }

    /// The T6 per-card move affordance: a clickable status chip that dispatches
    /// `kanban_task_move` to the next standard status. When provenance is
    /// partial (non-dispatchable, non-empty), renders a disabled "ask the
    /// agent" hint instead — a visible state, never a silent no-op (repo
    /// `.rules`). Empty provenance falls back to the hardcoded default server.
    fn render_move_affordance(
        &self,
        task: &TaskBody,
        in_flight_any: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !move_enabled(&self.provenance) {
            return div()
                .child(
                    Label::new(PROVENANCE_INCOMPLETE_MSG)
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                )
                .into_any_element();
        }

        let border_color = cx.theme().colors().border;
        let next_status = next_status(&task.status);
        let next_label = status_label(next_status);
        let task_id = task.task_id.clone();
        let target = next_status.to_string();
        let label_text = format!("Move → {next_label}");
        let disabled = in_flight_any || task.task_id.is_empty();

        let mut chip = div()
            .id(SharedString::from(format!("kanban-move-{}", task.task_id)))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(label_text)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );

        if !disabled {
            chip = chip
                .cursor_pointer()
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.dispatch_move(task_id.clone(), target.clone(), cx);
                }));
        }
        chip.into_any_element()
    }

    /// Build the dispatch plan from the card + provenance, then route through
    /// the governed `shared_tool_invoker()` (OCAP/gas-budgeted in production
    /// via `McpRuntime`).
    ///
    /// Surfaced states (never silent per repo `.rules`):
    /// - `MISSING_TASK_ID_MSG` / `INVALID_TARGET_STATUS_MSG` /
    ///   `PROVENANCE_INCOMPLETE_MSG` when the pure planner rejects the request.
    /// - `INVOKER_NOT_WIRED_MSG` when `shared_tool_invoker()` returns `None`.
    /// - The tool's own error string when dispatch fails.
    fn dispatch_move(&mut self, task_id: String, target_status: String, cx: &mut Context<Self>) {
        let plan = build_move_dispatch_args(&self.provenance, &task_id, &target_status);
        let (server, tool, args) = match plan {
            Ok(plan) => plan,
            Err(message) => {
                self.dispatch_error = Some(message.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
        };

        let invoker = match shared_tool_invoker() {
            None => {
                self.dispatch_error = Some(INVOKER_NOT_WIRED_MSG.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
            Some(invoker) => invoker,
        };

        self.dispatch_error = None;
        self.dispatch_in_flight = Some(task_id.clone());
        let task = invoker.invoke_tool(&server, &tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                this.dispatch_in_flight = None;
                match outcome {
                    Ok(_) => {
                        this.dispatch_error = None;
                        // Optimistic local view: reflect the move immediately.
                        // The authoritative state arrives with the next
                        // agent-emitted block; this is a local cache mutation
                        // only.
                        this.apply_optimistic_move(&task_id, &target_status);
                    }
                    Err(error) => this.dispatch_error = Some(error),
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Reflect a successful move in the local cached view: re-group all tasks
    /// with the moved task's status updated. Minimal and clearly a local view
    /// — the next agent-emitted block is authoritative.
    fn apply_optimistic_move(&mut self, task_id: &str, target_status: &str) {
        let all_tasks: Vec<TaskBody> = std::mem::take(&mut self.columns)
            .into_iter()
            .flat_map(|column| column.tasks)
            .collect();
        let moved = apply_move_to_tasks(all_tasks, task_id, target_status);
        self.columns = group_tasks_into_columns(moved);
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
            .children(self.render_dispatch_status(cx))
            .children(self.render_empty_state())
            .child(self.render_columns(cx))
            .into_any_element()
    }
}

// ── Pure dispatch-planning logic (T6) ──────────────────────────────────
//
// Kept free of the GPUI executor / global state so the dispatch decision is
// unit-testable directly (repo `.rules` racy-global trap: never unit-test by
// mutating `set_tool_invoker`). The `dispatch_move` handler composes this
// pure function with `shared_tool_invoker()`.

/// Whether the move affordance is enabled: dispatchable provenance re-issues
/// against the originating server; empty provenance falls back to the
/// hardcoded default. Partial (non-dispatchable, non-empty) provenance is
/// disabled with a hint.
fn move_enabled(provenance: &BlockProvenance) -> bool {
    provenance.is_dispatchable() || provenance.is_empty()
}

/// The next standard status after `current`, wrapping Done → Backlog. A
/// non-standard current status cycles to Backlog. No slice indexing — the
/// cycle is written out explicitly so it cannot panic on an out-of-bounds key.
fn next_status(current: &str) -> &'static str {
    match current.to_lowercase().as_str() {
        "backlog" => "ready",
        "ready" => "in_progress",
        "in_progress" => "review",
        "review" => "done",
        "done" => "backlog",
        _ => "backlog",
    }
}

/// The display label for a standard status wire string, without indexing.
fn status_label(status_key: &str) -> &'static str {
    STANDARD_STATUSES
        .iter()
        .find(|(key, _)| *key == status_key)
        .map(|(_, label)| *label)
        .unwrap_or("Status")
}

/// Whether `status` is one of the five standard wire strings the MCP server's
/// `TaskStatus::parse_str` accepts. The move affordance only offers standard
/// targets; a non-standard value is rejected up front with a visible error.
fn is_valid_target_status(status: &str) -> bool {
    STANDARD_STATUSES.iter().any(|(key, _)| *key == status)
}

/// Pure: decide the `(server, tool, args)` dispatch tuple for a move, given the
/// block's provenance, the task id, and the target status.
///
/// The dispatch tool is always `kanban_task_move` (the move affordance invokes
/// a different tool than the one that produced the block — `kanban_task_list`
/// — so the tool is hardcoded, not copied from provenance). The server is
/// taken from provenance when dispatchable, else the hardcoded default.
///
/// The args shape matches the server's confirmed `TaskMoveRequest` schema:
/// `{ "task_id": <id>, "target_status": <status> }` — no `board_id` (the
/// server resolves the task's board internally).
///
/// - empty `task_id` → `Err(MISSING_TASK_ID_MSG)`.
/// - non-standard `target_status` → `Err(INVALID_TARGET_STATUS_MSG)`.
/// - dispatchable provenance → `(provenance.server, kanban_task_move, args)`.
/// - empty provenance → fall back to `(DEFAULT_SERVER, kanban_task_move, args)`.
/// - partial (non-dispatchable, non-empty) provenance →
///   `Err(PROVENANCE_INCOMPLETE_MSG)`.
fn build_move_dispatch_args(
    provenance: &BlockProvenance,
    task_id: &str,
    target_status: &str,
) -> Result<(String, String, serde_json::Value), &'static str> {
    if task_id.trim().is_empty() {
        return Err(MISSING_TASK_ID_MSG);
    }
    if !is_valid_target_status(target_status) {
        return Err(INVALID_TARGET_STATUS_MSG);
    }
    let move_args = serde_json::json!({
        "task_id": task_id,
        "target_status": target_status,
    });
    if provenance.is_dispatchable() {
        // `is_dispatchable()` guarantees server is Some; the `unwrap_or_default`
        // accessor only returns an empty string if the invariant were violated,
        // keeping this panic-free.
        let server = provenance.server.as_deref().unwrap_or_default().to_string();
        Ok((server, DEFAULT_TOOL.to_string(), move_args))
    } else if provenance.is_empty() {
        Ok((
            DEFAULT_SERVER.to_string(),
            DEFAULT_TOOL.to_string(),
            move_args,
        ))
    } else {
        Err(PROVENANCE_INCOMPLETE_MSG)
    }
}

/// Pure: return the task list with the matching task's status updated to the
/// target. Used by the optimistic local view (`apply_optimistic_move`) so the
/// pure mutation is unit-testable without a GPUI executor.
fn apply_move_to_tasks(
    mut tasks: Vec<TaskBody>,
    task_id: &str,
    target_status: &str,
) -> Vec<TaskBody> {
    let target_key = target_status.to_lowercase();
    for task in &mut tasks {
        if task.task_id == task_id {
            task.status = target_key.clone();
        }
    }
    tasks
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

    // ── Pure dispatch-planning logic (T6) ────────────────────────────────

    fn dispatchable_provenance() -> BlockProvenance {
        BlockProvenance {
            tool: Some("kanban_task_list".into()),
            server: Some("hkask-mcp-kata-kanban".into()),
            args: serde_json::json!({ "board_id": "b1" }),
            span_id: None,
        }
    }

    #[test]
    fn build_args_dispatchable_uses_provenance_server_and_move_tool() {
        let provenance = dispatchable_provenance();
        let (server, tool, args) =
            build_move_dispatch_args(&provenance, "t1", "ready").expect("dispatchable");
        assert_eq!(server, "hkask-mcp-kata-kanban");
        // The tool is the move tool, not the task-list tool that produced the
        // block — the move affordance invokes a different tool.
        assert_eq!(tool, "kanban_task_move");
        assert_eq!(args["task_id"], "t1");
        assert_eq!(args["target_status"], "ready");
        // No board_id: the server's TaskMoveRequest takes only task_id +
        // target_status (confirmed against the schema).
        assert!(
            args.get("board_id").is_none(),
            "kanban_task_move does not take board_id"
        );
    }

    #[test]
    fn build_args_empty_provenance_falls_back_to_default_server_and_tool() {
        let provenance = BlockProvenance::default();
        let (server, tool, args) =
            build_move_dispatch_args(&provenance, "t1", "in_progress").expect("fallback");
        assert_eq!(server, "hkask-mcp-kata-kanban");
        assert_eq!(tool, "kanban_task_move");
        assert_eq!(args["task_id"], "t1");
        assert_eq!(args["target_status"], "in_progress");
    }

    #[test]
    fn build_args_non_dispatchable_partial_provenance_is_disabled() {
        // tool present but server absent → not dispatchable, not empty → disabled.
        let provenance = BlockProvenance {
            tool: Some("kanban_task_list".into()),
            ..Default::default()
        };
        let result = build_move_dispatch_args(&provenance, "t1", "ready");
        assert!(
            matches!(result, Err(PROVENANCE_INCOMPLETE_MSG)),
            "partial provenance is disabled"
        );
    }

    #[test]
    fn build_args_missing_task_id_is_rejected() {
        let provenance = dispatchable_provenance();
        let result = build_move_dispatch_args(&provenance, "", "ready");
        assert!(
            matches!(result, Err(MISSING_TASK_ID_MSG)),
            "empty task_id rejected"
        );
        // Whitespace-only task_id is also missing.
        let result = build_move_dispatch_args(&provenance, "   ", "ready");
        assert!(
            matches!(result, Err(MISSING_TASK_ID_MSG)),
            "whitespace task_id rejected"
        );
    }

    #[test]
    fn build_args_non_standard_target_status_is_rejected() {
        let provenance = dispatchable_provenance();
        let result = build_move_dispatch_args(&provenance, "t1", "blocked");
        assert!(
            matches!(result, Err(INVALID_TARGET_STATUS_MSG)),
            "non-standard target status rejected"
        );
        // The server's wire field is `target_status` with the lowercase wire
        // strings; a Display-form like "Ready" is not a wire string.
        let result = build_move_dispatch_args(&provenance, "t1", "Ready");
        assert!(
            matches!(result, Err(INVALID_TARGET_STATUS_MSG)),
            "display-form status rejected"
        );
    }

    #[test]
    fn build_args_dispatchable_treats_null_args_as_empty() {
        // Provenance with null args is still dispatchable (tool+server present);
        // the move args are built fresh from the card, so null args don't matter.
        let provenance = BlockProvenance {
            tool: Some("kanban_task_list".into()),
            server: Some("hkask-mcp-kata-kanban".into()),
            args: serde_json::Value::Null,
            span_id: None,
        };
        let (server, tool, args) =
            build_move_dispatch_args(&provenance, "t1", "review").expect("dispatchable");
        assert_eq!(server, "hkask-mcp-kata-kanban");
        assert_eq!(tool, "kanban_task_move");
        assert_eq!(args["task_id"], "t1");
        assert_eq!(args["target_status"], "review");
    }

    #[test]
    fn next_status_cycles_through_standard_order_and_wraps() {
        assert_eq!(next_status("backlog"), "ready");
        assert_eq!(next_status("ready"), "in_progress");
        assert_eq!(next_status("in_progress"), "review");
        assert_eq!(next_status("review"), "done");
        assert_eq!(next_status("done"), "backlog");
        // Case-insensitive.
        assert_eq!(next_status("BACKLOG"), "ready");
        // Non-standard cycles to backlog.
        assert_eq!(next_status("blocked"), "backlog");
    }

    #[test]
    fn status_label_returns_display_name_for_standard_statuses() {
        assert_eq!(status_label("backlog"), "Backlog");
        assert_eq!(status_label("in_progress"), "In Progress");
        assert_eq!(status_label("done"), "Done");
        assert_eq!(status_label("nonstandard"), "Status");
    }

    #[test]
    fn move_enabled_accepts_dispatchable_and_empty_rejects_partial() {
        assert!(move_enabled(&dispatchable_provenance()));
        assert!(move_enabled(&BlockProvenance::default()));
        let partial = BlockProvenance {
            tool: Some("kanban_task_list".into()),
            ..Default::default()
        };
        assert!(!move_enabled(&partial));
    }

    #[test]
    fn apply_move_to_tasks_updates_matching_task_status() {
        // Pure optimistic-update logic: the matching task's status changes,
        // others are untouched. The next agent-emitted block is authoritative;
        // this only drives the local cache mutation.
        let tasks = vec![task("t1", "A", "backlog"), task("t2", "B", "backlog")];
        let moved = apply_move_to_tasks(tasks, "t1", "ready");
        assert_eq!(moved[0].task_id, "t1");
        assert_eq!(moved[0].status, "ready");
        assert_eq!(moved[1].task_id, "t2");
        assert_eq!(moved[1].status, "backlog");
        // Re-grouping lands the moved task in the target column.
        let columns = group_tasks_into_columns(moved);
        let ready = columns
            .iter()
            .find(|column| column.status == "ready")
            .expect("ready column exists");
        assert_eq!(ready.tasks.len(), 1);
        assert_eq!(ready.tasks[0].task_id, "t1");
        let backlog = columns
            .iter()
            .find(|column| column.status == "backlog")
            .expect("backlog column exists");
        assert_eq!(backlog.tasks.len(), 1);
        assert_eq!(backlog.tasks[0].task_id, "t2");
    }
}
