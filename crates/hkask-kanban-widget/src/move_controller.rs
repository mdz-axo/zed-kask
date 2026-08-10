//! S9/R1: the kanban move dispatch state machine, extracted from
//! `KanbanWidget` into a standalone controller.
//!
//! The controller owns the dispatch state (`pending_move`,
//! `dispatch_in_flight`, `dispatch_error`, `optimistic_move`) and exposes the
//! move lifecycle: `stage_move` → `confirm_move`/`cancel_move` →
//! `dispatch_move` → `cancel_dispatch`. The optimistic-move mutation
//! (`apply_optimistic_move` / `rollback_optimistic_move`) operates on the
//! widget's `columns` (passed in by reference) so the board view and the
//! dispatch state stay in sync.
//!
//! The controller accesses the governed `shared_tool_invoker()` directly (OCAP
//! / gas-budgeted in production via `McpRuntime`), so the widget does not need
//! to wire the invoker through. The pure dispatch-planning logic
//! (`build_move_dispatch_args`) stays in `view.rs` so it remains unit-testable
//! without the controller's GPUI context.

use gpui::Context;
use gpui_util::ResultExt as _;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};

use crate::block::TaskBody;
use crate::view::{
    INVOKER_NOT_WIRED_MSG, KanbanColumn, apply_move_to_tasks, build_move_dispatch_args,
    group_tasks_into_columns,
};

/// A staged but unconfirmed move (consent gate H). The chip click stages a
/// pending move; the banner's Confirm/Cancel pair either dispatches it (via
/// `dispatch_move`, which surfaces `INVOKER_NOT_WIRED_MSG` when the invoker is
/// absent — never a silent drop) or discards it without any tool call. Only one
/// move may be pending at a time (chips are disabled while pending).
#[derive(Clone, Debug)]
pub(crate) struct PendingMove {
    pub task_id: String,
    pub task_title: String,
    pub from_label: String,
    /// Wire-format target status (e.g. `"review"`).
    pub to_status: String,
    /// Display label for the target status (e.g. `"Review"`).
    pub to_label: String,
}

/// An optimistic move applied to the local cache at dispatch time, tracked so
/// it can be rolled back if the user cancels mid-dispatch or the dispatch fails.
/// The next agent-emitted block is authoritative; this only drives the local
/// cache mutation.
struct OptimisticMove {
    task_id: String,
    /// The task's status before the optimistic move, to restore on rollback.
    original_status: String,
}

/// The kanban move dispatch state machine (S9/R1). Owns the dispatch state;
/// the widget delegates move lifecycle calls to it and passes its own
/// `columns` / `column_meta` / `provenance` by reference for the
/// optimistic-move mutation and dispatch planning.
pub(crate) struct KanbanMoveController {
    /// `task_id` currently being moved, if a dispatch is in flight. Single
    /// flight: while set, all move affordances are non-interactive.
    dispatch_in_flight: Option<String>,
    /// The optimistic move applied to the local cache at dispatch time, tracked
    /// so it can be rolled back if the user cancels mid-dispatch or the dispatch
    /// fails. Cleared on successful dispatch (the move sticks).
    optimistic_move: Option<OptimisticMove>,
    /// Visible error/hint when dispatch cannot proceed (missing invoker,
    /// provenance incomplete, missing task_id, tool error). Never silently
    /// dropped (repo `.rules`).
    dispatch_error: Option<String>,
    /// A staged move awaiting user confirmation (consent gate H). While set,
    /// all move chips are non-interactive and the dispatch-status banner
    /// shows a Confirm/Cancel pair instead of the in-flight/error state.
    pending_move: Option<PendingMove>,
}

impl Default for KanbanMoveController {
    fn default() -> Self {
        Self::new()
    }
}

impl KanbanMoveController {
    /// Create a fresh controller with no dispatch state.
    pub(crate) fn new() -> Self {
        Self {
            dispatch_in_flight: None,
            optimistic_move: None,
            dispatch_error: None,
            pending_move: None,
        }
    }

    /// Whether any move is pending or in flight (chips should be
    /// non-interactive while true).
    pub(crate) fn in_flight_any(&self) -> bool {
        self.dispatch_in_flight.is_some() || self.pending_move.is_some()
    }

    /// The current dispatch error, if any (rendered as a visible hint by the
    /// widget).
    pub(crate) fn dispatch_error(&self) -> Option<&str> {
        self.dispatch_error.as_deref()
    }

    /// The pending move, if any (rendered as a Confirm/Cancel banner by the
    /// widget).
    pub(crate) fn pending_move(&self) -> Option<&PendingMove> {
        self.pending_move.as_ref()
    }

    /// Take and clear the pending move without dispatching. Used by the
    /// widget's `evaluate_move` path, which composes an evaluation request
    /// from the pending move and then clears it so the user can't
    /// double-evaluate (they re-stage to actually execute).
    pub(crate) fn take_pending_move(&mut self) -> Option<PendingMove> {
        self.pending_move.take()
    }

    /// The task_id currently being moved, if a dispatch is in flight.
    pub(crate) fn dispatch_in_flight(&self) -> Option<&str> {
        self.dispatch_in_flight.as_deref()
    }

    /// Stage a move for user confirmation (consent gate H). Replaces any
    /// already-pending move (only one pending at a time) and clears any prior
    /// dispatch error so the banner shows the fresh confirmation prompt.
    pub(crate) fn stage_move(
        &mut self,
        task_id: String,
        task_title: String,
        from_label: String,
        to_status: String,
        to_label: String,
    ) {
        self.pending_move = Some(PendingMove {
            task_id,
            task_title,
            from_label,
            to_status,
            to_label,
        });
        self.dispatch_error = None;
    }

    /// Confirm the staged move: take the pending move (clearing it) and
    /// dispatch it. If the invoker is unwired, `dispatch_move` surfaces
    /// `INVOKER_NOT_WIRED_MSG` as usual — the pending move is already taken, so
    /// no stale pending survives a failed dispatch. A no-op when no move is
    /// pending.
    pub(crate) fn confirm_move(
        &mut self,
        columns: &mut Vec<KanbanColumn>,
        column_meta: &[crate::block::ColumnBody],
        provenance: &BlockProvenance,
        cx: &mut Context<crate::view::KanbanWidget>,
    ) {
        if let Some(pending) = self.pending_move.take() {
            let task_id = pending.task_id;
            let to_status = pending.to_status;
            self.dispatch_move(columns, column_meta, provenance, task_id, to_status, cx);
        }
    }

    /// Cancel the staged move: drop it without any tool call.
    pub(crate) fn cancel_move(&mut self, cx: &mut Context<crate::view::KanbanWidget>) {
        self.pending_move = None;
        cx.notify();
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
    pub(crate) fn dispatch_move(
        &mut self,
        columns: &mut Vec<KanbanColumn>,
        column_meta: &[crate::block::ColumnBody],
        provenance: &BlockProvenance,
        task_id: String,
        target_status: String,
        cx: &mut Context<crate::view::KanbanWidget>,
    ) {
        let plan = build_move_dispatch_args(provenance, &task_id, &target_status);
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
        // Apply the optimistic move to the local cache immediately so the UI
        // reflects the move while the dispatch is in flight. Track the original
        // status so a cancel or a dispatch failure can roll it back.
        let original_status = find_task_status(columns, &task_id);
        apply_optimistic_move(columns, column_meta, &task_id, &target_status);
        self.optimistic_move = Some(OptimisticMove {
            task_id: task_id.clone(),
            original_status: original_status.unwrap_or_default(),
        });
        let task = invoker.invoke_tool(&server, &tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                this.move_controller.dispatch_in_flight = None;
                match outcome {
                    Ok(_) => {
                        this.move_controller.dispatch_error = None;
                        // The optimistic move already reflected the new status;
                        // drop the rollback record (the move sticks).
                        this.move_controller.optimistic_move = None;
                    }
                    Err(error) => {
                        this.move_controller.dispatch_error = Some(error);
                        this.move_controller
                            .rollback_optimistic_move(&mut this.columns, &this.column_meta);
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Cancel a dispatch that is in flight: clear the in-flight marker and
    /// roll back the optimistic local move. The visible feedback is the
    /// rolled-back card position. The underlying tool call is not cancelled
    /// (it may already be queued on the server); the rollback only restores
    /// the local cache so the user sees the pre-move state. When the deferred
    /// result lands, it is applied on top of the rolled-back state.
    pub(crate) fn cancel_dispatch(
        &mut self,
        columns: &mut Vec<KanbanColumn>,
        column_meta: &[crate::block::ColumnBody],
        cx: &mut Context<crate::view::KanbanWidget>,
    ) {
        if self.dispatch_in_flight.is_none() {
            return;
        }
        self.dispatch_in_flight = None;
        self.rollback_optimistic_move(columns, column_meta);
        cx.notify();
    }

    /// Roll back the optimistic move (if any) by restoring the task's original
    /// status in the local cache. No-op when there is no recorded optimistic
    /// move.
    fn rollback_optimistic_move(
        &mut self,
        columns: &mut Vec<KanbanColumn>,
        column_meta: &[crate::block::ColumnBody],
    ) {
        if let Some(optimistic) = self.optimistic_move.take() {
            let all_tasks: Vec<TaskBody> = std::mem::take(columns)
                .into_iter()
                .flat_map(|column| column.tasks)
                .collect();
            let restored =
                apply_move_to_tasks(all_tasks, &optimistic.task_id, &optimistic.original_status);
            *columns = group_tasks_into_columns(restored, column_meta);
        }
    }
}

/// Find the current status of a task in the local cache, if present.
fn find_task_status(columns: &[KanbanColumn], task_id: &str) -> Option<String> {
    columns.iter().find_map(|column| {
        column
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .map(|task| task.status.clone())
    })
}

/// Reflect a move in the local cached view: re-group all tasks with the
/// moved task's status updated. Applied optimistically at dispatch time so
/// the UI reflects the move while the dispatch is in flight; rolled back
/// on cancel or dispatch failure. The next agent-emitted block is
/// authoritative.
fn apply_optimistic_move(
    columns: &mut Vec<KanbanColumn>,
    column_meta: &[crate::block::ColumnBody],
    task_id: &str,
    target_status: &str,
) {
    let all_tasks: Vec<TaskBody> = std::mem::take(columns)
        .into_iter()
        .flat_map(|column| column.tasks)
        .collect();
    let moved = apply_move_to_tasks(all_tasks, task_id, target_status);
    *columns = group_tasks_into_columns(moved, column_meta);
}
