//! The kanban data pipeline: board/task fetchers, the auto-refresh loop,
//! per-task comment fetching, and the detail-open detection. Extracted
//! from `kanban_panel.rs` — the fetchers stay methods on `KanbanPanel`
//! (they mutate panel state via `cx.spawn` + `this.update`); this module
//! owns the tool invocations and response parsing. See the swarm panel's
//! `fetch.rs` for the same extraction pattern.

use gpui::Context;
use gpui_util::ResultExt;
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde_json::json;

use crate::KanbanPanel;
use crate::{BOARD_LIST_TOOL, KANBAN_SERVER, TASK_LIST_TOOL, classify_kanban_fetch_error, refresh_target};
use crate::REFRESH_INTERVAL;
use crate::RefreshTarget;
use crate::{BoardListResponse, CommentsResponse, TaskListResponse};
use hkask_tool_invoker::shared_tool_invoker;

impl KanbanPanel {
    /// Fetch the list of boards from the kanban MCP server. Auto-selects the
    /// first board if none is selected, which triggers a task fetch.
    pub(crate) fn fetch_boards(&mut self, cx: &mut Context<Self>) {
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
                            // If the selected board was deleted externally
                            // (via the MCP tool, not through the panel UI),
                            // the selected_board_id still points to the dead
                            // id. Clear the selection so the stale widget and
                            // tasks are dropped, and the empty state or the
                            // next available board renders instead. Without
                            // this, the panel shows the deleted board's stale
                            // widget forever while fetch_tasks loops on
                            // "board not found" errors.
                            let selected_still_exists = this
                                .selected_board_id
                                .as_ref()
                                .is_some_and(|id| this.boards.iter().any(|b| &b.board_id == id));
                            if !selected_still_exists {
                                this.clear_board_selection();
                            }
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
    pub(crate) fn fetch_tasks(&mut self, cx: &mut Context<Self>) {
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
                        // If the board was deleted externally, the task
                        // fetch returns NotFound. Clear the selection so
                        // the stale widget is dropped and the board list
                        // refresh can re-select or show the empty state.
                        // Without this, the panel loops on "board not
                        // found" while the dead board's widget stays.
                        if matches!(err.kind, Some(hkask_types::McpErrorKind::NotFound)) {
                            this.clear_board_selection();
                            this.fetch_boards(cx);
                            return;
                        }
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
    pub(crate) fn start_refresh_task(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn fetch_task_comments(&mut self, task_id: String, cx: &mut Context<Self>) {
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
    pub(crate) fn check_detail_opened(&mut self, cx: &mut Context<Self>) {
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
}
