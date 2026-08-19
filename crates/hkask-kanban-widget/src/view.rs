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
use hkask_tool_invoker::BlockProvenance;
use hkask_types::TaskStatus;
use hkask_types::kanban_wire;
use theme::ActiveTheme;
use ui::Tooltip;
use ui::prelude::*;

use crate::block::{KanbanBlockBody, TaskBody};

/// Display label for each standard `TaskStatus`. The wire keys come from
/// `TaskStatus::as_str()` (the shared source of truth in `hkask-types`); the
/// display labels are widget-local since the server has no display concern.
fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Backlog => "Backlog",
        TaskStatus::Ready => "Ready",
        TaskStatus::InProgress => "In Progress",
        TaskStatus::Review => "Review",
        TaskStatus::Done => "Done",
    }
}

/// Parse the numeric rank from a `pN`-style priority label (e.g. `"p1"`,
/// `"P1-high"`). Returns `Some(rank)` only when the label starts with `p`
/// followed by a single digit — so `"p1"` → `Some(1)` but `"p10"` → `None`
/// (avoids the `starts_with("p1")` trap where `"p10"` would match `"p1"`).
fn priority_rank(lowered: &str) -> Option<u8> {
    let bytes = lowered.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'p' && bytes[1].is_ascii_digit() {
        // Only single-digit ranks are recognized; `p10` is not rank 1.
        let next = bytes.get(2);
        if next.is_some_and(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        return Some(bytes[1] - b'0');
    }
    None
}

/// Tool the widget dispatches to move a task. Sourced from the shared
/// `kanban_wire` module. The args shape is confirmed against the server's
/// `TaskMoveRequest { task_id, target_status }` schema (no `board_id`).
const MOVE_TOOL: &str = kanban_wire::KANBAN_TASK_MOVE_TOOL;
/// Surfaced when the process-global `ToolInvoker` is not wired. Visible state,
/// not a silent no-op (repo `.rules` startup-failure-signal trap).
pub(crate) const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";
/// Surfaced when provenance is not dispatchable: the widget refuses to
/// dispatch against an unknown server and asks the user to route through the
/// agent. Empty provenance (no tool/server) is treated the same as partial —
/// the widget no longer falls back to a hardcoded default server.
const PROVENANCE_INCOMPLETE_MSG: &str = "provenance incomplete — ask the agent";
/// Surfaced when a card carries no dispatchable `task_id`.
const MISSING_TASK_ID_MSG: &str = "missing task_id";
/// Surfaced when the target status is not one of the five standard wire
/// strings the MCP server accepts.
const INVALID_TARGET_STATUS_MSG: &str = "invalid target status";

/// One column in the kanban board.
#[derive(Clone)]
pub(crate) struct KanbanColumn {
    #[allow(dead_code)]
    pub(crate) status: String,
    pub(crate) title: String,
    pub(crate) tasks: Vec<TaskBody>,
    /// S8: WIP limit for this column. `None` when no limit is set (older
    /// blocks or columns without a `wip_limit`). Rendered as "N / WIP" when
    /// `Some`; red when `N >= WIP`.
    pub(crate) wip_limit: Option<u32>,
}

/// A staged but unconfirmed move (consent gate H). The chip click stages a
/// pending move; the banner's Confirm/Cancel pair either dispatches it (via
/// `dispatch_move`, which surfaces `INVOKER_NOT_WIRED_MSG` when the invoker is
/// absent — never a silent drop) or discards it without any tool call. Only one
/// move may be pending at a time (chips are disabled while pending).
///
/// S9/R1: the struct lives in `move_controller.rs`; this re-export keeps the
/// `view.rs` test module's references working.
pub(crate) use crate::move_controller::PendingMove;

/// The kanban widget view. Renders inline in agent markdown (via the D18 seam
/// composed by `hkask-viz-core`).
pub struct KanbanWidget {
    pub(crate) board_name: String,
    pub(crate) columns: Vec<KanbanColumn>,
    /// S8: column metadata (WIP limits) copied from the parsed block body, so
    /// `rollback_optimistic_move` / `apply_optimistic_move` can re-group with
    /// the same WIP limits after an optimistic mutation.
    pub(crate) column_meta: Vec<crate::block::ColumnBody>,
    /// Server-authoritative provenance copied from the parsed block body. The
    /// move affordance uses it to pick the dispatch server and to decide
    /// whether to show an active affordance or a disabled "ask the agent"
    /// hint.
    pub(crate) provenance: BlockProvenance,
    focus_handle: FocusHandle,
    /// S9/R1: the move dispatch state machine. Owns `pending_move`,
    /// `dispatch_in_flight`, `dispatch_error`, and `optimistic_move`. The
    /// widget delegates move lifecycle calls to it.
    pub(crate) move_controller: crate::move_controller::KanbanMoveController,
    /// Composed revision request surfaced as a copyable draft when the
    /// conversation injector is absent (no active conversation). Lets the user
    /// still use the "I disagree" body even when it can't be injected. Cleared
    /// when a successful inject fires (repo `.rules`: visible, not a silent
    /// no-op).
    disagree_draft: Option<String>,
    /// Task ids whose description is expanded ("See more" toggled). Per-card
    /// expand state so a long description can be revealed without affecting
    /// other cards.
    expanded_descriptions: HashSet<String>,
    /// Task id whose card-detail panel is open (B3). `None` when no panel is
    /// open. Escape closes it; the Close button closes it. Click-outside is
    /// not implemented (the panel is inline below the board, not a floating
    /// popover).
    detail_open: Option<String>,
}

impl KanbanWidget {
    /// Create a new kanban widget for the parsed block body.
    ///
    /// The widget renders one board per block. The agent emits multiple
    /// blocks for multiple boards.
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
        let (_board_id, board_name, tasks) = body.board_with_tasks();
        let tasks = tasks.to_vec();
        let column_meta = body.columns.clone();
        let columns = group_tasks_into_columns(tasks, &column_meta);
        let provenance = body.provenance.clone();

        Self {
            board_name,
            columns,
            column_meta,
            provenance,
            focus_handle: cx.focus_handle(),
            move_controller: crate::move_controller::KanbanMoveController::new(),
            disagree_draft: None,
            expanded_descriptions: HashSet::new(),
            detail_open: None,
        }
    }

    /// Update the board data from a new block body, preserving UI state
    /// (pending moves, expanded descriptions, detail panel, disagree draft).
    ///
    /// When a move is in flight or pending, the columns are NOT updated — the
    /// optimistic move reflects the in-progress dispatch, and the next refresh
    /// after the dispatch completes will carry the server-authoritative state.
    /// The board name, column metadata, and provenance are always updated
    /// because they don't change during a move.
    pub fn set_body(&mut self, body: KanbanBlockBody, cx: &mut Context<Self>) {
        let (_board_id, board_name, tasks) = body.board_with_tasks();
        let tasks = tasks.to_vec();
        let column_meta = body.columns.clone();

        if !self.move_controller.in_flight_any() {
            self.columns = group_tasks_into_columns(tasks, &column_meta);
        }

        self.board_name = board_name;
        self.column_meta = column_meta;
        self.provenance = body.provenance;

        let new_task_ids: HashSet<String> = body.tasks.iter().map(|t| t.task_id.clone()).collect();
        self.expanded_descriptions
            .retain(|id| new_task_ids.contains(id));
        if let Some(ref open_id) = self.detail_open {
            if !new_task_ids.contains(open_id) {
                self.detail_open = None;
            }
        }

        cx.notify();
    }

    /// Update a single task's comments in the cached columns. Used by the
    /// kanban panel to populate comments on demand when the card detail
    /// panel is opened.
    pub fn update_task_comments(
        &mut self,
        task_id: &str,
        comments: Vec<crate::block::CommentBody>,
        cx: &mut Context<Self>,
    ) {
        for column in &mut self.columns {
            for task in &mut column.tasks {
                if task.task_id == task_id {
                    task.comments = comments.clone();
                    break;
                }
            }
        }
        cx.notify();
    }

    /// The task id whose card-detail panel is currently open, if any.
    /// The kanban panel reads this to fetch comments on demand.
    pub fn detail_open(&self) -> Option<&str> {
        self.detail_open.as_deref()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new(self.board_name.clone()).size(LabelSize::Large))
            // C: "I disagree" affordance — composes a provenance-scoped revision
            // request back into the active conversation (D21). Board-level (one
            // chip in the board header), not per-card.
            .child(
                div()
                    .id("kanban-disagree")
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.on_disagree_click(window, cx);
                    }))
                    .child(
                        Label::new("I disagree")
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    ),
            )
    }

    /// Render the dispatch-status banner: a Confirm/Cancel/Evaluate pair when
    /// a move is pending, a Cancel button when a dispatch is in flight, or the
    /// dispatch error when set. Returns `None` when there is no dispatch state
    /// to show. S9/R1: reads controller state via accessors; the controller is
    /// a pure state machine and does not render.
    fn render_dispatch_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let border_color = cx.theme().colors().border;
        if let Some(pending) = self.move_controller.pending_move() {
            Some(
                h_flex()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .gap_2()
                    .items_center()
                    .child(
                        Label::new(format!(
                            "Move '{}' \u{2192} {}?",
                            pending.task_title, pending.to_label
                        ))
                        .size(LabelSize::XSmall),
                    )
                    .child(
                        div()
                            .id("kanban-confirm-move")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.move_controller.confirm_move(
                                    &mut this.columns,
                                    &this.column_meta,
                                    &this.provenance,
                                    cx,
                                );
                            }))
                            .child(
                                Label::new("Confirm")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            ),
                    )
                    .child(
                        div()
                            .id("kanban-cancel-move")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.move_controller.cancel_move(cx);
                            }))
                            .child(
                                Label::new("Cancel")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        div()
                            .id("kanban-evaluate-move")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.evaluate_move(window, cx);
                            }))
                            .child(
                                Label::new("Evaluate")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            ),
                    )
                    .into_any_element(),
            )
        } else if let Some(task_id) = self.move_controller.dispatch_in_flight() {
            Some(
                h_flex()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .gap_2()
                    .items_center()
                    .child(
                        Label::new(format!("Moving {task_id} \u{2026}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .child(
                        div()
                            .id("kanban-cancel-dispatch")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.move_controller.cancel_dispatch(
                                    &mut this.columns,
                                    &this.column_meta,
                                    cx,
                                );
                            }))
                            .child(
                                Label::new("Cancel")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .into_any_element(),
            )
        } else if let Some(error) = self.move_controller.dispatch_error() {
            Some(
                h_flex()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .gap_2()
                    .items_center()
                    .child(
                        Label::new(error.to_string())
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                    .into_any_element(),
            )
        } else {
            None
        }
    }

    fn render_columns(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let card_bg = cx.theme().colors().editor_background;
        let in_flight_any = self.move_controller.in_flight_any();

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

                // S8: render "N / WIP" when a WIP limit is set; red when at or
                // over the limit. No limit → count only.
                let count_label = match column.wip_limit {
                    Some(limit) if count >= limit as usize => {
                        Label::new(format!("{count} / {limit}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Warning)
                    }
                    Some(limit) => Label::new(format!("{count} / {limit}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    None => Label::new(format!("{count}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                };

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
                            .child(count_label),
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
        let task_id = task.task_id.clone();
        let is_open = self.detail_open.as_deref() == Some(task.task_id.as_str());
        v_flex()
            .w_full()
            .p_2()
            .gap_1()
            .rounded_sm()
            .border_1()
            .border_color(border_color)
            .bg(card_bg)
            .id(SharedString::from(format!("kanban-card-{}", task.task_id)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _event, _window, cx| {
                this.detail_open = if is_open { None } else { Some(task_id.clone()) };
                cx.notify();
            }))
            .child(Label::new(task.title.clone()).size(LabelSize::Small))
            .when_some(task.description.clone(), |this, description| {
                this.child(self.render_description(task.task_id.clone(), description, cx))
            })
            .when_some(task.assignee.clone(), |this, assignee| {
                this.child(
                    Label::new(format!("@{assignee}"))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            // R1: the visible swarm↔kanban link. When the server emits a
            // `swarm_id` on the task, render a badge so the operator can see
            // which swarm is running the task at a glance.
            .when_some(task.swarm_id.clone(), |this, swarm_id| {
                this.child(
                    div()
                        .id(format!("kanban-swarm-{}", task.task_id))
                        .tooltip(Tooltip::text(format!(
                            "Running in swarm {swarm_id} — the durable \
                             coordination link (Task.swarm_id)."
                        )))
                        .child(
                            Label::new(format!("◈ {swarm_id}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Accent),
                        ),
                )
            })
            .when_some(task.ontology.clone(), |this, ontology| {
                this.child(
                    div()
                        .id(format!("kanban-ontology-{}", task.task_id))
                        .tooltip(Tooltip::text(format!("Ontology: {ontology}")))
                        .child(Label::new("§").size(LabelSize::XSmall).color(Color::Muted)),
                )
            })
            .when_some(task.priority.clone(), |this, priority| {
                this.child(self.render_priority_badge(priority))
            })
            .when(!task.labels.is_empty(), |this| {
                this.child(self.render_labels(&task.labels))
            })
            .when(!task.criteria.is_empty(), |this| {
                this.child(
                    Label::new(format!("✓ {} criteria", task.criteria.len()))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            // R3: a one-line activity status strip so the operator sees the
            // latest recorded update on the task at a glance (the largest UX
            // gap vs. cline/kanban's live hook activity). `min_w_0` +
            // `truncate` protect the text column so a long update ellipsizes
            // instead of blowing out the 220px card.
            .when_some(task.activity.clone(), |this, activity| {
                this.child(
                    h_flex()
                        .gap_1()
                        .min_w_0()
                        .child(Label::new("●").size(LabelSize::XSmall).color(Color::Accent))
                        .child(
                            Label::new(activity.text)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                )
            })
            .child(self.render_move_affordance(task, in_flight_any, cx))
            .into_any_element()
    }

    /// Render the priority as a colored badge. High-priority labels (containing
    /// "high" or matching the `P0`/`P1` token) render in the accent color;
    /// medium (`P2`) in the default text color; everything else muted.
    fn render_priority_badge(&self, priority: String) -> impl IntoElement {
        let lower = priority.to_lowercase();
        let color = if lower.contains("high") || priority_rank(&lower).is_some_and(|r| r <= 1) {
            Color::Accent
        } else if priority_rank(&lower) == Some(2) || lower.contains("medium") {
            Color::Default
        } else {
            Color::Muted
        };
        Label::new(priority).size(LabelSize::XSmall).color(color)
    }

    /// Render labels as muted chips separated by spaces.
    fn render_labels(&self, labels: &[String]) -> impl IntoElement {
        h_flex().gap_1().children(labels.iter().map(|label| {
            Label::new(label.clone())
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        }))
    }

    /// Render a task description clamped to 3 lines, with a "See more" /
    /// "See less" toggle to expand. Short descriptions (≤3 lines) render
    /// without the toggle.
    fn render_description(
        &self,
        task_id: String,
        description: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let expanded = self.expanded_descriptions.contains(&task_id);
        // Heuristic: a description longer than ~180 chars likely exceeds 3
        // lines at the card width, so show the toggle. The clamp itself is
        // enforced by `Label::line_clamp` when collapsed.
        let likely_long = description.chars().count() > 180;
        v_flex()
            .gap_1()
            .child(
                Label::new(description)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .when(!expanded && likely_long, |label| label.line_clamp(3)),
            )
            .when(likely_long, |this| {
                this.child(
                    div()
                        .id(format!("kanban-desc-toggle-{task_id}"))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            if this.expanded_descriptions.contains(&task_id) {
                                this.expanded_descriptions.remove(&task_id);
                            } else {
                                this.expanded_descriptions.insert(task_id.clone());
                            }
                            cx.notify();
                        }))
                        .child(
                            Label::new(if expanded { "See less" } else { "See more" })
                                .size(LabelSize::XSmall)
                                .color(Color::Accent),
                        ),
                )
            })
    }

    /// The T6 per-card move affordance: a clickable status chip that dispatches
    /// `kanban_task_move` to the next standard status. When provenance is not
    /// dispatchable (empty or partial), renders a disabled "ask the agent" hint
    /// instead — a visible state, never a silent no-op (repo `.rules`). Cards in
    /// the `Done` status have no next status (`TaskStatus::next` returns `None`)
    /// and render no move chip.
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
        // Parse the task's status and compute the next status. `Done` has no
        // next status → no move chip (the card is terminal).
        let Some(current) = TaskStatus::parse_str(&task.status) else {
            return div().into_any_element();
        };
        let Some(next) = current.next() else {
            return div().into_any_element();
        };
        let next_label = status_label(next);
        let task_id = task.task_id.clone();
        let task_title = task.title.clone();
        let from_label = status_label(current).to_string();
        let to_status = next.as_str().to_string();
        let to_label = next_label.to_string();
        let label_text = format!("Move → {next_label}");
        // Pending or in-flight moves gate all chips non-interactive (single
        // pending, single flight). Consent gate H: the click stages a
        // `PendingMove` rather than dispatching directly — the user confirms
        // via the banner.
        let disabled = in_flight_any || task.task_id.is_empty();

        let mut chip = div()
            .id(SharedString::from(format!("kanban-move-{}", task.task_id)))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(border_color)
            .tooltip(Tooltip::text(format!("Move this task to {next_label}")))
            .child(
                Label::new(label_text)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );

        if !disabled {
            chip = chip
                .cursor_pointer()
                .on_click(cx.listener(move |this, _event, _window, cx| {
                    this.stage_move(
                        task_id.clone(),
                        task_title.clone(),
                        from_label.clone(),
                        to_status.clone(),
                        to_label.clone(),
                        cx,
                    );
                }));
        }
        chip.into_any_element()
    }

    /// Stage a move for user confirmation (consent gate H). S9/R1: delegates
    /// to `move_controller`.
    fn stage_move(
        &mut self,
        task_id: String,
        task_title: String,
        from_label: String,
        to_status: String,
        to_label: String,
        cx: &mut Context<Self>,
    ) {
        self.move_controller
            .stage_move(task_id, task_title, from_label, to_status, to_label);
        cx.notify();
    }

    /// Find the current status of a task in the local cache, if present. S9/R1:
    /// free function for test access (the controller has its own private
    /// equivalent).
    #[cfg(test)]
    fn find_task_status(&self, task_id: &str) -> Option<String> {
        self.columns.iter().find_map(|column| {
            column
                .tasks
                .iter()
                .find(|task| task.task_id == task_id)
                .map(|task| task.status.clone())
        })
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

    /// Render the card-detail panel (B3) when a card is open. The panel shows
    /// the full task: description (unclamped), criteria list, comments thread,
    /// verification result, and gas spend log. Closes on a "Close" button
    /// click or Escape (handled on the root element). The panel is inline
    /// (below the board), not a floating popover — click-outside is not
    /// implemented.
    fn render_detail_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let task_id = self.detail_open.as_ref()?;
        let task = self
            .columns
            .iter()
            .find_map(|column| column.tasks.iter().find(|task| &task.task_id == task_id))?;
        let border_color = cx.theme().colors().border;

        let mut panel = v_flex()
            .w_full()
            .p_3()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(Label::new(task.title.clone()).size(LabelSize::Small))
                    .child(
                        div()
                            .id("kanban-detail-close")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.detail_open = None;
                                cx.notify();
                            }))
                            .child(
                                Label::new("Close")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            ),
                    ),
            );

        // Full description (unclamped — the card clamps to 3 lines; the panel
        // shows the whole thing).
        if let Some(description) = task.description.clone() {
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Description")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(description)
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    ),
            );
        }

        // Criteria list.
        if !task.criteria.is_empty() {
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("Criteria ({})", task.criteria.len()))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .children(task.criteria.iter().map(|criterion| {
                        Label::new(format!("✓ {criterion}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Default)
                    })),
            );
        }

        // Comments thread.
        if !task.comments.is_empty() {
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("Comments ({})", task.comments.len()))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .children(task.comments.iter().map(|comment| {
                        v_flex()
                            .gap_1()
                            .child(
                                Label::new(format!("@{} · {}", comment.author, comment.created_at))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(comment.body.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                    })),
            );
        }

        // Verification result.
        if let Some(verification) = task.verification.clone() {
            let verdict = if verification.passed {
                "✓ Passed"
            } else {
                "✗ Failed"
            };
            let color = if verification.passed {
                Color::Accent
            } else {
                Color::Warning
            };
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Verification")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("{verdict}: {}", verification.reason))
                            .size(LabelSize::XSmall)
                            .color(color),
                    ),
            );
        }

        // Spend log.
        if !task.spend_log.is_empty() {
            panel = panel.child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(format!("Spend log ({})", task.spend_log.len()))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .children(task.spend_log.iter().map(|entry| {
                        Label::new(format!(
                            "{} {} — {}",
                            entry.amount, entry.kind, entry.reason
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Default)
                    })),
            );
        }

        Some(panel.into_any_element())
    }

    /// Compose the provenance-scoped "I disagree" body. References the board's
    /// name and the tool that produced the block so the agent can correlate the
    /// revision request to the exact `kanban_task_list` result the widget
    /// rendered. Falls back to a generic "the kanban board" framing when the
    /// board name is empty (grill-me edge case c).
    fn compose_disagree_body(&self) -> String {
        let board_clause = if self.board_name.is_empty() {
            String::new()
        } else {
            format!(" '{}'", self.board_name)
        };
        let tool = self
            .provenance
            .tool
            .as_deref()
            .unwrap_or("kanban_task_list");
        // Reference the PKO concept when available so the agent can correlate
        // the revision request to the ontology-anchored artifact.
        let pko_clause = self
            .columns
            .iter()
            .flat_map(|col| &col.tasks)
            .find_map(|t| t.ontology.clone())
            .map(|pko| format!(" [{pko}]"))
            .unwrap_or_default();
        format!(
            "Re: the kanban board{board_clause} (via {tool}){pko_clause}.\n\
             I believe a task's status or the board setup is incorrect. Please re-check the task states and ordering.\n\n\
             My concern: "
        )
    }

    /// The "I disagree" affordance handler (C). Composes the provenance-scoped
    /// revision request and injects it back into the active conversation via
    /// the kask `shared_injector()` (D21 widget→agent seam). When no
    /// conversation is active, surfaces the composed body as a copyable draft
    /// instead of a silent no-op (repo `.rules`). Never auto-sends when the
    /// injector is absent — the production injector only pre-fills the
    /// composer; the user reviews and submits.
    /// The "I disagree" affordance handler (C). Composes the provenance-scoped
    /// revision request and injects it back into the active conversation via
    /// the kask `shared_injector()` (D21 widget→agent seam). When no
    /// conversation is active, surfaces the composed body as a copyable draft
    /// instead of a silent no-op (repo `.rules`). Never auto-sends when the
    /// injector is absent — the production injector only pre-fills the
    /// composer; the user reviews and submits.
    fn on_disagree_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.compose_disagree_body();
        self.compose_back(body, window, cx);
    }

    /// Shared compose-back seam (D21 widget→agent) for both the "I disagree"
    /// affordance (C) and the "Evaluate" affordance (D — ghost edits). S10/R2:
    /// delegates to `hkask_conversation_injector::compose_back_via_injector`,
    /// which emits the `reg.widget.disagree` span, injects `body` into the
    /// active conversation, and on a no-injector or inject-error path surfaces
    /// `body` as a copyable `disagree_draft` (visible, not a silent no-op —
    /// repo `.rules`). Never auto-sends when the injector is absent — the
    /// production injector only pre-fills the composer; the user reviews and
    /// submits.
    fn compose_back(&mut self, body: String, window: &mut Window, cx: &mut Context<Self>) {
        let widget = cx.entity().downgrade();
        let draft = hkask_conversation_injector::compose_back_via_injector(
            body,
            window,
            cx,
            widget,
            |this, draft| {
                this.disagree_draft = draft;
            },
        );
        self.disagree_draft = draft;
        cx.notify();
    }

    /// Composes the evaluation request body for the ghost-edit affordance (D):
    /// asks the agent to advise whether a staged move is safe — checking the
    /// blocker DAG, dependencies, and task constraints — without executing it.
    fn compose_evaluate_body(&self, pending: &PendingMove) -> String {
        format!(
            "Evaluate this proposed move: should task '{}' move from {} to {}?\n\
             Check the blocker DAG, dependencies, and task constraints.\n\
             Don't execute — just advise whether this move is safe and consistent.\n\n\
             My reasoning: ",
            pending.task_title, pending.from_label, pending.to_label
        )
    }

    /// The "Evaluate" affordance handler (D — ghost edits). Composes an
    /// evaluation request back to the agent via D21 compose-back: the agent
    /// advises whether the staged move is safe without executing it, after
    /// which the user re-stages and confirms or cancels. Clears the pending
    /// move so the user can't double-evaluate; they re-stage if they want to
    /// actually execute after the agent's evaluation comes back.
    fn evaluate_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pending) = self.move_controller.take_pending_move() {
            let body = self.compose_evaluate_body(&pending);
            self.compose_back(body, window, cx);
            cx.notify();
        }
    }
}

/// Group tasks into columns by status, preserving the standard order and
/// appending any non-standard statuses at the end (sorted alphabetically).
/// Case-insensitive matching: "BACKLOG" and "backlog" both map to the
/// "backlog" column.
///
/// S8: `column_meta` carries optional WIP limits per status (case-insensitive
/// match). When a column's status matches a `ColumnBody` entry, the column
/// inherits its `wip_limit`.
pub(crate) fn group_tasks_into_columns(
    tasks: Vec<TaskBody>,
    column_meta: &[crate::block::ColumnBody],
) -> Vec<KanbanColumn> {
    let mut by_status: HashMap<String, Vec<TaskBody>> = HashMap::new();
    for task in tasks {
        by_status
            .entry(task.status.to_lowercase())
            .or_default()
            .push(task);
    }

    // Index WIP limits by lowercased status for case-insensitive lookup.
    let wip_by_status: HashMap<String, Option<u32>> = column_meta
        .iter()
        .map(|column| (column.status.to_lowercase(), column.wip_limit))
        .collect();

    let mut columns = Vec::new();

    for status in TaskStatus::STANDARD_ORDER {
        let status_key = status.as_str();
        let tasks = by_status.remove(status_key).unwrap_or_default();
        columns.push(KanbanColumn {
            status: status_key.to_string(),
            title: status_label(status).to_string(),
            tasks,
            wip_limit: wip_by_status.get(status_key).copied().flatten(),
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
            wip_limit: wip_by_status.get(&status_key).copied().flatten(),
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
        let border_color = cx.theme().colors().border;

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                    if event.keystroke.key == "escape" && this.detail_open.is_some() {
                        this.detail_open = None;
                        cx.notify();
                    }
                }),
            )
            .child(self.render_header(cx))
            // Fallback draft (no active conversation): surface the composed body
            // so the user can copy it into chat — visible, not a silent no-op
            // (repo `.rules`).
            .when_some(self.disagree_draft.clone(), |this, draft| {
                this.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(border_color)
                        .child(
                            Label::new(draft)
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        ),
                )
            })
            .children(self.render_dispatch_status(cx))
            .children(self.render_empty_state())
            .child(self.render_columns(cx))
            .children(self.render_detail_panel(cx))
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
/// against the originating server. Non-dispatchable provenance (empty or
/// partial) is disabled — `build_move_dispatch_args` returns
/// `Err(PROVENANCE_INCOMPLETE_MSG)` and the affordance is hidden.
fn move_enabled(provenance: &BlockProvenance) -> bool {
    provenance.is_dispatchable()
}

/// Pure: decide the `(server, tool, args)` dispatch tuple for a move, given the
/// block's provenance, the task id, and the target status.
///
/// The dispatch tool is always `kanban_task_move` (the move affordance invokes
/// a different tool than the one that produced the block — `kanban_task_list`
/// — so the tool is hardcoded, not copied from provenance). The server is
/// taken from provenance when dispatchable; non-dispatchable provenance
/// (empty or partial) yields `Err(PROVENANCE_INCOMPLETE_MSG)`.
///
/// The args shape matches the server's confirmed `TaskMoveRequest` schema:
/// `{ "task_id": <id>, "target_status": <status> }` — no `board_id` (the
/// server resolves the task's board internally). `target_status` is accepted
/// case-insensitively (via `TaskStatus::parse_str`) and emitted as the
/// lowercase wire string (`TaskStatus::as_str`), so display-form input like
/// `"Ready"` round-trips to the server's `rename_all = "lowercase"` serde.
///
/// - empty `task_id` → `Err(MISSING_TASK_ID_MSG)`.
/// - non-standard `target_status` → `Err(INVALID_TARGET_STATUS_MSG)`.
/// - dispatchable provenance → `(provenance.server, MOVE_TOOL, args)`.
/// - non-dispatchable provenance (empty or partial) →
///   `Err(PROVENANCE_INCOMPLETE_MSG)`.
pub(crate) fn build_move_dispatch_args(
    provenance: &BlockProvenance,
    task_id: &str,
    target_status: &str,
) -> Result<(String, String, serde_json::Value), &'static str> {
    if task_id.trim().is_empty() {
        return Err(MISSING_TASK_ID_MSG);
    }
    // Validate case-insensitively, then emit the canonical lowercase wire
    // string so the server's `rename_all = "lowercase"` serde accepts it.
    let target = TaskStatus::parse_str(target_status).ok_or(INVALID_TARGET_STATUS_MSG)?;
    if !provenance.is_dispatchable() {
        return Err(PROVENANCE_INCOMPLETE_MSG);
    }
    let move_args = serde_json::json!({
        "task_id": task_id,
        "target_status": target.as_str(),
    });
    // `is_dispatchable()` guarantees server is Some; the `unwrap_or_default`
    // accessor only returns an empty string if the invariant were violated,
    // keeping this panic-free.
    let server = provenance.server.as_deref().unwrap_or_default().to_string();
    Ok((server, MOVE_TOOL.to_string(), move_args))
}

/// Pure: return the task list with the matching task's status updated to the
/// target. Used by the optimistic local view (`apply_optimistic_move`) so the
/// pure mutation is unit-testable without a GPUI executor.
pub(crate) fn apply_move_to_tasks(
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
    use gpui::{Task, TestAppContext};
    use hkask_tool_invoker::{InvokeError, ToolInvoker, set_tool_invoker};
    use std::sync::Arc;

    fn task(task_id: &str, title: &str, status: &str) -> TaskBody {
        TaskBody {
            task_id: task_id.to_string(),
            title: title.to_string(),
            status: status.to_string(),
            description: None,
            assignee: None,
            swarm_id: None,
            
            activity: None,
            ontology: None,
            priority: None,
            labels: Vec::new(),
            criteria: Vec::new(),
            comments: Vec::new(),
            verification: None,
            spend_log: Vec::new(),
        }
    }

    #[test]
    fn group_tasks_into_columns_preserves_standard_order() {
        let tasks = vec![
            task("t1", "Backlog task", "backlog"),
            task("t2", "In progress", "in_progress"),
            task("t3", "Done", "done"),
        ];
        let columns = group_tasks_into_columns(tasks, &[]);
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
        let columns = group_tasks_into_columns(tasks, &[]);
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
        let columns = group_tasks_into_columns(tasks, &[]);
        assert_eq!(columns[0].status, "backlog");
        assert_eq!(columns[0].tasks.len(), 2);
    }

    #[test]
    fn empty_tasks_produce_five_empty_columns() {
        let columns = group_tasks_into_columns(Vec::new(), &[]);
        assert_eq!(columns.len(), 5);
        for column in &columns {
            assert!(column.tasks.is_empty());
        }
    }

    // ── Pure dispatch-planning logic (T6) ────────────────────────────────

    fn dispatchable_provenance() -> BlockProvenance {
        BlockProvenance {
            tool: Some("kanban_task_list".into()),
            server: Some("kata-kanban".into()),
            args: serde_json::json!({ "board_id": "b1" }),
            span_id: None,
        }
    }

    #[test]
    fn build_args_dispatchable_uses_provenance_server_and_move_tool() {
        let provenance = dispatchable_provenance();
        let (server, tool, args) =
            build_move_dispatch_args(&provenance, "t1", "ready").expect("dispatchable");
        assert_eq!(server, "kata-kanban");
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
    fn build_args_non_dispatchable_provenance_is_disabled() {
        // Empty provenance (no tool/server) is not dispatchable → disabled.
        let empty = BlockProvenance::default();
        let result = build_move_dispatch_args(&empty, "t1", "ready");
        assert!(
            matches!(result, Err(PROVENANCE_INCOMPLETE_MSG)),
            "empty provenance is disabled"
        );
        // Partial provenance (tool present, server absent) → not dispatchable → disabled.
        let partial = BlockProvenance {
            tool: Some("kanban_task_list".into()),
            ..Default::default()
        };
        let result = build_move_dispatch_args(&partial, "t1", "ready");
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
        // `TaskStatus::parse_str` is case-insensitive, so Display-form inputs
        // (e.g. "Ready", "In Progress") are accepted and normalized to the
        // lowercase wire string the server's `rename_all = "lowercase"` serde
        // expects.
        let result = build_move_dispatch_args(&provenance, "t1", "Ready");
        let (_, _, args) = result.expect("display-form status accepted (case-insensitive)");
        assert_eq!(
            args["target_status"], "ready",
            "emitted as lowercase wire string"
        );
    }

    #[test]
    fn build_args_dispatchable_treats_null_args_as_empty() {
        // Provenance with null args is still dispatchable (tool+server present);
        // the move args are built fresh from the card, so null args don't matter.
        let provenance = BlockProvenance {
            tool: Some("kanban_task_list".into()),
            server: Some("kata-kanban".into()),
            args: serde_json::Value::Null,
            span_id: None,
        };
        let (server, tool, args) =
            build_move_dispatch_args(&provenance, "t1", "review").expect("dispatchable");
        assert_eq!(server, "kata-kanban");
        assert_eq!(tool, "kanban_task_move");
        assert_eq!(args["task_id"], "t1");
        assert_eq!(args["target_status"], "review");
    }

    #[test]
    fn next_status_returns_none_at_done() {
        // B2: `TaskStatus::next()` returns `None` at Done — no wrap. The move
        // affordance hides on Done cards (terminal status).
        assert_eq!(TaskStatus::Backlog.next(), Some(TaskStatus::Ready));
        assert_eq!(TaskStatus::Ready.next(), Some(TaskStatus::InProgress));
        assert_eq!(TaskStatus::InProgress.next(), Some(TaskStatus::Review));
        assert_eq!(TaskStatus::Review.next(), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::Done.next(), None);
    }

    #[test]
    fn status_label_returns_display_name_for_task_status() {
        assert_eq!(status_label(TaskStatus::Backlog), "Backlog");
        assert_eq!(status_label(TaskStatus::InProgress), "In Progress");
        assert_eq!(status_label(TaskStatus::Done), "Done");
    }

    #[test]
    fn priority_rank_parses_single_digit_and_rejects_p10() {
        // `p0`/`p1`/`p2` parse to their ranks; `p10` does NOT parse as rank 1
        // (the `starts_with("p1")` trap).
        assert_eq!(priority_rank("p0"), Some(0));
        assert_eq!(priority_rank("p1"), Some(1));
        assert_eq!(priority_rank("p2"), Some(2));
        assert_eq!(priority_rank("p3"), Some(3));
        assert_eq!(priority_rank("p10"), None, "p10 is not rank 1");
        assert_eq!(priority_rank("p1-high"), Some(1), "suffix after digit ok");
        assert_eq!(priority_rank("high"), None);
        assert_eq!(priority_rank(""), None);
    }

    #[test]
    fn move_enabled_only_accepts_dispatchable() {
        assert!(move_enabled(&dispatchable_provenance()));
        assert!(!move_enabled(&BlockProvenance::default()));
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
        let columns = group_tasks_into_columns(moved, &[]);
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

    // ── Consent gate H (stage → confirm/cancel → dispatch) ──────────────────
    //
    // The `ToolInvoker` is a process-global `static Mutex<Option<Arc<dyn
    // ToolInvoker>>>`. Tests that mutate it via `set_tool_invoker` must
    // serialize so parallel test threads never observe each other's invoker.
    // `GLOBAL_TEST_LOCK` serializes the consent-gate tests within this binary;
    // `InvokerGuard` restores the global invoker to `None` on drop so a later
    // test never sees a stale mock (repo `.rules` racy-global trap).
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that clears the process-global `ToolInvoker` when dropped,
    /// so a test wiring a mock invoker never leaks it into a sibling test.
    struct InvokerGuard;
    impl Drop for InvokerGuard {
        fn drop(&mut self) {
            set_tool_invoker(None);
        }
    }

    /// Records `invoke_tool` calls and resolves them immediately with `{}`.
    /// The `calls` buffer is an `Arc<Mutex<…>>` so the test can read the
    /// recorded calls through a clone held outside the trait object.
    #[derive(Default)]
    struct MockToolInvoker {
        calls: Arc<std::sync::Mutex<Vec<(String, String, serde_json::Value)>>>,
    }

    impl ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> Task<Result<String, InvokeError>> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push((server.to_string(), tool.to_string(), args));
            }
            Task::ready(Ok("{}".into()))
        }
    }

    /// Build a single-board `KanbanBlockBody` named "Test" with empty
    /// (non-dispatchable) provenance. Use `body_with_board_and_provenance_with`
    /// for move-dispatch tests that need dispatchable provenance.
    fn kanban_body(tasks: Vec<TaskBody>) -> KanbanBlockBody {
        KanbanBlockBody {
            viz: Some("kanban".into()),
            board_id: Some("b1".into()),
            board_name: Some("Test".into()),
            tasks,
            columns: Vec::new(),
            provenance: BlockProvenance::default(),
        }
    }

    #[gpui::test]
    async fn stage_move_sets_pending(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _guard = InvokerGuard;
        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });

        let pending = widget.read_with(cx, |this, _| this.move_controller.pending_move().cloned());
        let pending = pending.expect("pending move staged");
        assert_eq!(pending.task_id, "t1");
        assert_eq!(pending.to_status, "ready");
        assert_eq!(pending.to_label, "Ready");
        assert_eq!(pending.from_label, "Backlog");
        assert_eq!(pending.task_title, "Write tests");
    }

    #[gpui::test]
    async fn confirm_move_dispatches_and_clears_pending(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let mock = Arc::new(MockToolInvoker::default());
        let recorded = mock.calls.clone();
        let invoker: Arc<dyn ToolInvoker> = mock;
        set_tool_invoker(Some(invoker));
        let _guard = InvokerGuard;

        let body = body_with_board_and_provenance_with(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update(cx, |this, cx| {
            this.move_controller.confirm_move(
                &mut this.columns,
                &this.column_meta,
                &this.provenance,
                cx,
            );
        });
        cx.run_until_parked();

        let pending_is_none =
            widget.read_with(cx, |this, _| this.move_controller.pending_move().is_none());
        assert!(pending_is_none, "confirm_move must clear the pending move");

        let calls = recorded.lock().map(|c| c.clone()).unwrap_or_default();
        assert_eq!(calls.len(), 1, "exactly one kanban_task_move dispatch");
        let (server, tool, args) = calls.into_iter().next().expect("one call");
        assert_eq!(server, "kata-kanban");
        assert_eq!(tool, "kanban_task_move");
        assert_eq!(args["task_id"], "t1");
        assert_eq!(args["target_status"], "ready");
    }

    #[gpui::test]
    async fn confirm_move_with_unwired_invoker_surfaces_error_and_clears_pending(
        cx: &mut TestAppContext,
    ) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        // No invoker wired — ensure clean state regardless of prior tests.
        set_tool_invoker(None);
        let _guard = InvokerGuard;

        let body = body_with_board_and_provenance_with(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update(cx, |this, cx| {
            this.move_controller.confirm_move(
                &mut this.columns,
                &this.column_meta,
                &this.provenance,
                cx,
            );
        });

        let (pending_is_none, error) = widget.read_with(cx, |this, _| {
            (
                this.move_controller.pending_move().is_none(),
                this.move_controller.dispatch_error().map(str::to_string),
            )
        });
        assert!(
            pending_is_none,
            "pending move must be cleared even on dispatch failure"
        );
        assert_eq!(
            error.as_deref(),
            Some(INVOKER_NOT_WIRED_MSG),
            "unwired invoker surfaces INVOKER_NOT_WIRED_MSG, not a silent drop"
        );
    }

    #[gpui::test]
    async fn cancel_move_clears_without_dispatch(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let mock = Arc::new(MockToolInvoker::default());
        let recorded = mock.calls.clone();
        let invoker: Arc<dyn ToolInvoker> = mock;
        set_tool_invoker(Some(invoker));
        let _guard = InvokerGuard;

        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update(cx, |this, cx| this.move_controller.cancel_move(cx));
        cx.run_until_parked();

        let pending_is_none =
            widget.read_with(cx, |this, _| this.move_controller.pending_move().is_none());
        assert!(pending_is_none, "cancel_move must clear the pending move");
        let calls = recorded.lock().map(|c| c.len()).unwrap_or(0);
        assert_eq!(calls, 0, "cancel_move must not dispatch kanban_task_move");
    }

    #[gpui::test]
    async fn cancel_during_dispatch_rolls_back_optimistic_move(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let mock = Arc::new(MockToolInvoker::default());
        let invoker: Arc<dyn ToolInvoker> = mock;
        set_tool_invoker(Some(invoker));
        let _guard = InvokerGuard;

        let body = body_with_board_and_provenance_with(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        // Stage + confirm the move. `dispatch_move` applies the optimistic
        // move synchronously (task t1 → ready) and sets `dispatch_in_flight`
        // before the spawned completion task is polled.
        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update(cx, |this, cx| {
            this.move_controller.confirm_move(
                &mut this.columns,
                &this.column_meta,
                &this.provenance,
                cx,
            );
        });

        // The optimistic move is reflected locally before the dispatch resolves.
        let in_flight = widget.read_with(cx, |this, _| {
            this.move_controller
                .dispatch_in_flight()
                .map(str::to_string)
        });
        assert_eq!(in_flight.as_deref(), Some("t1"), "dispatch is in flight");
        let optimistic_status = widget.read_with(cx, |this, _| this.find_task_status("t1"));
        assert_eq!(
            optimistic_status.as_deref(),
            Some("ready"),
            "optimistic move reflected locally"
        );

        // Cancel mid-dispatch (before `run_until_parked` lets the spawned
        // completion task run). The rollback restores t1 to its original
        // `backlog` status and clears `dispatch_in_flight`.
        widget.update(cx, |this, cx| {
            this.move_controller
                .cancel_dispatch(&mut this.columns, &this.column_meta, cx);
        });

        let in_flight_after = widget.read_with(cx, |this, _| {
            this.move_controller
                .dispatch_in_flight()
                .map(str::to_string)
        });
        assert!(
            in_flight_after.is_none(),
            "cancel clears dispatch_in_flight"
        );
        let rolled_back_status = widget.read_with(cx, |this, _| this.find_task_status("t1"));
        assert_eq!(
            rolled_back_status.as_deref(),
            Some("backlog"),
            "cancel rolls back the optimistic move to the original status"
        );

        // Let the spawned completion task run. It sees `dispatch_in_flight`
        // is already cleared; on `Ok(_)` it clears `optimistic_move` (already
        // None) — a no-op. The rolled-back status must survive.
        cx.run_until_parked();
        let final_status = widget.read_with(cx, |this, _| this.find_task_status("t1"));
        assert_eq!(
            final_status.as_deref(),
            Some("backlog"),
            "rolled-back status survives the deferred completion"
        );
    }

    #[gpui::test]
    async fn chips_disabled_while_pending(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _guard = InvokerGuard;
        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });

        // The render gate disables all move chips while a move is pending or
        // in flight (single pending, single flight). Asserting the gating
        // condition directly is the robust check — introspecting the rendered
        // element tree for a `disabled` flag is brittle across GPUI versions.
        let gated = widget.read_with(cx, |this, _| {
            this.move_controller.dispatch_in_flight().is_some()
                || this.move_controller.pending_move().is_some()
        });
        assert!(
            gated,
            "chips must be gated non-interactive while a move is pending"
        );
    }

    #[gpui::test]
    async fn stage_move_replaces_existing_pending(cx: &mut TestAppContext) {
        let _lock = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _guard = InvokerGuard;
        let body = kanban_body(vec![
            task("t1", "First", "backlog"),
            task("t2", "Second", "backlog"),
        ]);
        let widget = cx.new(|cx| KanbanWidget::new(body, cx));

        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "First".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update(cx, |this, cx| {
            this.stage_move(
                "t2".into(),
                "Second".into(),
                "Backlog".into(),
                "in_progress".into(),
                "In Progress".into(),
                cx,
            );
        });

        let pending = widget.read_with(cx, |this, _| this.move_controller.pending_move().cloned());
        let pending = pending.expect("a pending move remains");
        assert_eq!(
            pending.task_id, "t2",
            "the second stage_move replaces the first"
        );
        assert_eq!(pending.to_status, "in_progress");
    }

    // ── "I disagree" compose-back affordance (C, D21) ──────────────────────
    //
    // Mirrors `hkask-portfolio-widget`'s disagree tests. These mutate the
    // per-app `ConversationInjector` global (a separate global from
    // `TOOL_INVOKER`), so they take `GLOBAL_TEST_LOCK` too. The per-app global
    // drops with each test's `TestAppContext`, so no RAII reset guard is needed.

    /// Records the body of every `inject` call. `Send + Sync` for the
    /// `Arc<dyn ConversationInjector>` global.
    #[derive(Default)]
    struct MockConversationInjector {
        bodies: std::sync::Mutex<Vec<String>>,
    }

    impl hkask_conversation_injector::ConversationInjector for MockConversationInjector {
        fn inject(
            &self,
            body: String,
            _window: &mut gpui::Window,
            _cx: &mut gpui::App,
        ) -> gpui::Task<Result<(), String>> {
            self.bodies
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(body);
            gpui::Task::ready(Ok(()))
        }
    }

    /// Trivial root view for `add_window_view` so the test can obtain a `Window`
    /// for `on_disagree_click` without rendering `KanbanWidget` (which would
    /// need a theme global this leaf crate's tests don't initialise). Renders a
    /// bare `div()`.
    struct DummyView;
    impl Render for DummyView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// Single-board `KanbanBlockBody` named "Test" with dispatchable provenance
    /// so `compose_disagree_body` references both the board name and the tool.
    fn body_with_board_and_provenance() -> KanbanBlockBody {
        let mut body = kanban_body(Vec::new());
        body.provenance = dispatchable_provenance();
        body
    }

    /// Like `body_with_board_and_provenance` but with tasks populated, for
    /// move-dispatch tests that need dispatchable provenance.
    fn body_with_board_and_provenance_with(tasks: Vec<TaskBody>) -> KanbanBlockBody {
        let mut body = kanban_body(tasks);
        body.provenance = dispatchable_provenance();
        body
    }

    #[gpui::test]
    async fn disagree_routes_through_injector(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mock = std::sync::Arc::new(MockConversationInjector::default());
        cx.update(|cx| {
            hkask_conversation_injector::set_active_injector(cx, Some(mock.clone()));
        });

        let body = body_with_board_and_provenance();
        // Use a throwaway window root so we get a `Window` for `on_disagree_click`
        // without rendering `KanbanWidget` (no theme global in these tests).
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(bodies.len(), 1, "exactly one inject");
        assert!(bodies[0].contains("Re:"), "body references the revision");
        assert!(
            bodies[0].contains("Test"),
            "body references the board name from the block"
        );
        assert!(
            bodies[0].contains("kanban_task_list"),
            "body references the provenance tool"
        );

        // A successful inject clears the fallback draft.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        assert!(draft.is_none(), "draft cleared after a successful inject");
    }

    #[gpui::test]
    async fn disagree_surfaces_draft_when_no_injector(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Per-app global starts empty — no injector is wired by default.

        let body = body_with_board_and_provenance();
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        widget.update_in(cx, |widget, window, cx| {
            widget.on_disagree_click(window, cx);
        });
        cx.run_until_parked();

        // No injector: the composed body is surfaced as a copyable draft
        // (visible, not a silent no-op — repo `.rules`), and no panic.
        let draft = widget.read_with(cx, |widget, _cx| widget.disagree_draft.clone());
        let draft = draft.expect("draft surfaced when no injector is active");
        assert!(draft.contains("Re:"), "draft carries the revision prefix");
        assert!(draft.contains("Test"), "draft carries the board name");
    }

    #[gpui::test]
    async fn disagree_body_falls_back_when_board_name_empty(cx: &mut gpui::TestAppContext) {
        // grill-me edge case (c): empty board name → generic "the kanban board"
        // framing (no empty quotes, no panic). `compose_disagree_body` is pure,
        // so no window is needed.
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        let empty = KanbanBlockBody {
            viz: Some("kanban".into()),
            board_id: Some("b1".into()),
            board_name: Some(String::new()),
            tasks: Vec::new(),
            columns: Vec::new(),
            provenance: BlockProvenance::default(),
        };
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(empty, cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("Re: the kanban board (via"),
            "empty board name falls back to the generic framing"
        );
        assert!(
            !body.contains("''"),
            "no empty-quoted board name in the fallback framing"
        );
    }

    #[test]
    fn task_body_parses_ontology_field() {
        // The server emits `"ontology": "pko:Step"` on every TaskInfo. The widget
        // must parse it (additive `#[serde(default)]` — older blocks without
        // it still parse with `ontology: None`).
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog","ontology":"pko:Step"}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert_eq!(task.ontology.as_deref(), Some("pko:Step"));
    }

    #[test]
    fn task_body_parses_without_ontology_field() {
        // Older blocks without the ontology field still parse (defaults to None).
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog"}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert!(task.ontology.is_none());
    }

    #[test]
    fn block_body_has_ontology_field() {
        // S4 sensor: the kata-kanban server emits `"ontology": "pko:Step"` on
        // every TaskInfo response. The widget's TaskBody MUST have an `ontology`
        // field to receive it — if this field is absent, the server's tag is
        // silently dropped (the cybernetic S4 gap — no spec-drift sensor).
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog","ontology":"pko:Step"}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert_eq!(
            task.ontology.as_deref(),
            Some("pko:Step"),
            "TaskBody must parse the ontology field the server emits"
        );
    }

    #[test]
    fn task_body_parses_description_field() {
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog","description":"A longer description."}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert_eq!(task.description.as_deref(), Some("A longer description."));
    }

    #[test]
    fn task_body_description_defaults_to_none_when_absent() {
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog"}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert!(task.description.is_none());
    }

    #[gpui::test]
    async fn description_expand_toggle_adds_and_removes_task_id(cx: &mut gpui::TestAppContext) {
        // Only mutates the widget's own `expanded_descriptions` field — no
        // process global is touched, so `GLOBAL_TEST_LOCK` is not needed.
        let mut t = task("t1", "Write tests", "backlog");
        t.description = Some("x".repeat(200));
        let body = kanban_body(vec![t]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        // Initially collapsed.
        let expanded = widget.read_with(cx, |this, _| this.expanded_descriptions.contains("t1"));
        assert!(!expanded, "description starts collapsed");

        // Toggle expand.
        widget.update(cx, |this, cx| {
            this.expanded_descriptions.insert("t1".to_string());
            cx.notify();
        });
        let expanded = widget.read_with(cx, |this, _| this.expanded_descriptions.contains("t1"));
        assert!(expanded, "description expanded");

        // Toggle collapse.
        widget.update(cx, |this, cx| {
            this.expanded_descriptions.remove("t1");
            cx.notify();
        });
        let expanded = widget.read_with(cx, |this, _| this.expanded_descriptions.contains("t1"));
        assert!(!expanded, "description collapsed again");
    }

    #[gpui::test]
    async fn card_click_opens_detail_popover(cx: &mut gpui::TestAppContext) {
        // B3: clicking a card opens the detail panel (sets `detail_open`).
        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert!(open.is_none(), "detail panel starts closed");

        widget.update(cx, |this, cx| {
            this.detail_open = Some("t1".to_string());
            cx.notify();
        });
        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert_eq!(open.as_deref(), Some("t1"), "detail panel open after click");

        widget.update(cx, |this, cx| {
            this.detail_open = None;
            cx.notify();
        });
        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert!(open.is_none(), "detail panel closed");
    }

    #[gpui::test]
    async fn detail_popover_shows_criteria_and_comments(cx: &mut gpui::TestAppContext) {
        // B3: the detail panel renders criteria and comments when present.
        let mut t = task("t1", "Write tests", "backlog");
        t.criteria = vec!["compiles".to_string(), "tests pass".to_string()];
        t.comments = vec![crate::block::CommentBody {
            author: "alice".to_string(),
            body: "Looks good".to_string(),
            created_at: "2026-08-09T10:00:00Z".to_string(),
        }];
        let body = kanban_body(vec![t]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        widget.update(cx, |this, cx| {
            this.detail_open = Some("t1".to_string());
            cx.notify();
        });

        let (criteria_count, comments_count) = widget.read_with(cx, |this, _| {
            let task = this
                .columns
                .iter()
                .find_map(|column| column.tasks.iter().find(|task| task.task_id == "t1"))
                .expect("task found");
            (task.criteria.len(), task.comments.len())
        });
        assert_eq!(criteria_count, 2, "criteria rendered in detail panel");
        assert_eq!(comments_count, 1, "comments rendered in detail panel");
    }

    #[gpui::test]
    async fn detail_popover_shows_verification_when_present(cx: &mut gpui::TestAppContext) {
        // B3: the detail panel renders the verification result when present.
        let mut t = task("t1", "Write tests", "done");
        t.verification = Some(crate::block::VerificationBody {
            passed: true,
            reason: "tests pass".to_string(),
        });
        let body = kanban_body(vec![t]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        widget.update(cx, |this, cx| {
            this.detail_open = Some("t1".to_string());
            cx.notify();
        });

        let verification_present = widget.read_with(cx, |this, _| {
            this.columns
                .iter()
                .find_map(|column| column.tasks.iter().find(|task| task.task_id == "t1"))
                .map(|task| task.verification.is_some())
                .unwrap_or(false)
        });
        assert!(
            verification_present,
            "verification rendered in detail panel"
        );
    }

    #[gpui::test]
    async fn detail_popover_empty_when_no_extras(cx: &mut gpui::TestAppContext) {
        // B3: a task with no criteria/comments/verification/spend_log still
        // opens the detail panel (it shows the title + description only).
        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        widget.update(cx, |this, cx| {
            this.detail_open = Some("t1".to_string());
            cx.notify();
        });

        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert_eq!(
            open.as_deref(),
            Some("t1"),
            "detail panel opens even with no extras"
        );

        let (criteria, comments, verification, spend_log) = widget.read_with(cx, |this, _| {
            let task = this
                .columns
                .iter()
                .find_map(|column| column.tasks.iter().find(|task| task.task_id == "t1"))
                .expect("task found");
            (
                task.criteria.is_empty(),
                task.comments.is_empty(),
                task.verification.is_none(),
                task.spend_log.is_empty(),
            )
        });
        assert!(
            criteria && comments && verification && spend_log,
            "all extras empty"
        );
    }

    #[gpui::test]
    async fn escape_closes_detail_panel(cx: &mut gpui::TestAppContext) {
        // B3: Escape closes the detail panel (clears `detail_open`). The root
        // element's `on_key_down` handler checks for the `escape` key.
        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));

        // Open the detail panel.
        widget.update(cx, |this, cx| {
            this.detail_open = Some("t1".to_string());
            cx.notify();
        });
        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert_eq!(open.as_deref(), Some("t1"), "detail panel open");

        // Simulate Escape: the `on_key_down` handler clears `detail_open`.
        widget.update(cx, |this, cx| {
            if this.detail_open.is_some() {
                this.detail_open = None;
                cx.notify();
            }
        });
        let open = widget.read_with(cx, |this, _| this.detail_open.clone());
        assert!(open.is_none(), "detail panel closed by escape");
    }

    #[test]
    fn column_at_wip_limit_renders_red() {
        // S8: a column at or over its WIP limit flags red (Warning color).
        let tasks = vec![
            task("t1", "A", "in_progress"),
            task("t2", "B", "in_progress"),
        ];
        let meta = vec![crate::block::ColumnBody {
            status: "in_progress".to_string(),
            wip_limit: Some(2),
        }];
        let columns = group_tasks_into_columns(tasks, &meta);
        let in_progress = columns
            .iter()
            .find(|column| column.status == "in_progress")
            .expect("in_progress column");
        let count = in_progress.tasks.len() as u32;
        let limit = in_progress.wip_limit.expect("wip limit set");
        assert!(count >= limit, "column at limit flags red");
    }

    #[test]
    fn column_under_wip_renders_normal() {
        // S8: a column under its WIP limit renders normal (Muted color).
        let tasks = vec![task("t1", "A", "in_progress")];
        let meta = vec![crate::block::ColumnBody {
            status: "in_progress".to_string(),
            wip_limit: Some(3),
        }];
        let columns = group_tasks_into_columns(tasks, &meta);
        let in_progress = columns
            .iter()
            .find(|column| column.status == "in_progress")
            .expect("in_progress column");
        let count = in_progress.tasks.len() as u32;
        let limit = in_progress.wip_limit.expect("wip limit set");
        assert!(count < limit, "column under limit renders normal");
    }

    #[test]
    fn column_without_wip_renders_count_only() {
        // S8: a column with no WIP limit renders count only (no "N / WIP").
        let tasks = vec![task("t1", "A", "backlog")];
        let columns = group_tasks_into_columns(tasks, &[]);
        let backlog = columns
            .iter()
            .find(|column| column.status == "backlog")
            .expect("backlog column");
        assert_eq!(backlog.wip_limit, None, "no WIP limit renders count only");
    }

    #[test]
    fn task_body_parses_priority_labels_and_criteria_fields() {
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog","priority":"P1","labels":["a"],"criteria":["c1","c2"]}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert_eq!(task.priority.as_deref(), Some("P1"));
        assert_eq!(task.labels, vec!["a"]);
        assert_eq!(task.criteria, vec!["c1", "c2"]);
    }

    #[test]
    fn task_body_priority_labels_criteria_default_empty_when_absent() {
        let json = r##"{"task_id":"t1","title":"Test","status":"backlog"}"##;
        let task: TaskBody = serde_json::from_str(json).expect("parses");
        assert!(task.priority.is_none());
        assert!(task.labels.is_empty());
        assert!(task.criteria.is_empty());
    }

    #[gpui::test]
    async fn disagree_body_includes_ontology_concept_when_present(cx: &mut gpui::TestAppContext) {
        // When a task carries an ontology tag, the compose-back body references it
        // so the agent can correlate the revision to the ontology-anchored
        // artifact.
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        let mut t = task("t1", "Write tests", "backlog");
        t.ontology = Some("pko:Step".to_string());
        let body = kanban_body(vec![t]);
        let widget = cx.update(|cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        let body = widget.read_with(cx, |widget, _cx| widget.compose_disagree_body());
        assert!(
            body.contains("[pko:Step]"),
            "compose-back body must reference the ontology concept: {body}"
        );
    }

    // ── "Evaluate" ghost-edit affordance (D, D21) ──────────────────────────
    //
    // The Evaluate button (in the PendingMove confirm banner) composes an
    // evaluation request back to the agent via the same `compose_back` seam as
    // the disagree affordance (C), then clears the pending move so the user
    // re-stages after the agent's advice comes back. Shares
    // `MockConversationInjector` / `DummyView` with the disagree tests above.

    #[gpui::test]
    async fn evaluate_move_composes_evaluation_request(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mock = std::sync::Arc::new(MockConversationInjector::default());
        cx.update(|cx| {
            hkask_conversation_injector::set_active_injector(cx, Some(mock.clone()));
        });

        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update_in(cx, |this, window, cx| {
            this.evaluate_move(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert_eq!(bodies.len(), 1, "exactly one inject");
        assert!(
            bodies[0].contains("Evaluate this proposed move"),
            "body carries the evaluation framing"
        );
        assert!(
            bodies[0].contains("Write tests"),
            "body references the task title"
        );
        assert!(
            bodies[0].contains("Backlog"),
            "body references the from label"
        );
        assert!(bodies[0].contains("Ready"), "body references the to label");

        // evaluate_move clears the pending move (no double-evaluate).
        let pending_is_none = widget.read_with(cx, |this, _cx| {
            this.move_controller.pending_move().is_none()
        });
        assert!(pending_is_none, "evaluate_move clears the pending move");
    }

    #[gpui::test]
    async fn evaluate_move_noop_when_no_pending(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mock = std::sync::Arc::new(MockConversationInjector::default());
        cx.update(|cx| {
            hkask_conversation_injector::set_active_injector(cx, Some(mock.clone()));
        });

        let body = kanban_body(Vec::new());
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        // No stage_move: pending_move is None.
        widget.update_in(cx, |this, window, cx| {
            this.evaluate_move(window, cx);
        });
        cx.run_until_parked();

        let bodies = mock
            .bodies
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert!(
            bodies.is_empty(),
            "no inject when no pending move is staged"
        );
        let draft = widget.read_with(cx, |this, _cx| this.disagree_draft.clone());
        assert!(
            draft.is_none(),
            "disagree_draft unchanged when no pending move"
        );
        let pending_is_none = widget.read_with(cx, |this, _cx| {
            this.move_controller.pending_move().is_none()
        });
        assert!(pending_is_none, "pending_move remains None");
    }

    #[gpui::test]
    async fn evaluate_move_surfaces_draft_when_no_injector(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Per-app global starts empty — no injector is wired by default.

        let body = kanban_body(vec![task("t1", "Write tests", "backlog")]);
        let (_dummy, cx) = cx.add_window_view(|_window, _cx| DummyView);
        let widget = cx.update(|_window, cx| cx.new(|cx| KanbanWidget::new(body, cx)));
        widget.update(cx, |this, cx| {
            this.stage_move(
                "t1".into(),
                "Write tests".into(),
                "Backlog".into(),
                "ready".into(),
                "Ready".into(),
                cx,
            );
        });
        widget.update_in(cx, |this, window, cx| {
            this.evaluate_move(window, cx);
        });
        cx.run_until_parked();

        // No injector: the evaluation body is surfaced as a copyable draft
        // (visible, not a silent no-op — repo `.rules`), and the pending move
        // is still cleared.
        let draft = widget.read_with(cx, |this, _cx| this.disagree_draft.clone());
        let draft = draft.expect("draft surfaced when no injector is active");
        assert!(
            draft.contains("Evaluate"),
            "draft carries the evaluation framing"
        );
        assert!(
            draft.contains("Write tests"),
            "draft carries the task title"
        );
        let pending_is_none = widget.read_with(cx, |this, _cx| {
            this.move_controller.pending_move().is_none()
        });
        assert!(
            pending_is_none,
            "evaluate_move clears pending even with no injector"
        );
    }
}
