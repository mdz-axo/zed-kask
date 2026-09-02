//! Task action forms and their handlers — create, edit, spawn, and delete
//! task UI, plus the board lifecycle handlers (create/delete/export/import
//! board). Extracted from `kanban_panel.rs` — the handlers stay methods on
//! `KanbanPanel` (they mutate panel state and dispatch through the panel's
//! mutation pipeline); this module owns the form structs, the form
//! renderers, and the action handlers.
//!
//! Each form is a lightweight inline panel rendered below the board header.
//! Forms use `Editor::single_line` for text input (matching the swarm panel's
//! compose form pattern). Form state is owned by the `KanbanPanel` so it
//! persists across re-renders.

use editor::Editor;
use gpui::{ClipboardItem, Context, Entity, Window};
use gpui_util::ResultExt;
use hkask_tool_invoker::shared_tool_invoker;
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde_json::json;
use ui::prelude::*;

use crate::KanbanPanel;
use crate::{
    BOARD_CREATE_TOOL, BOARD_DELETE_TOOL, BOARD_EXPORT_TOOL, BOARD_IMPORT_TOOL, KANBAN_SERVER,
    RefreshTarget, TASK_ASSIGN_TOOL, TASK_CREATE_TOOL, TASK_DELETE_TOOL, TASK_SPAWN_TOOL,
    TASK_UNASSIGN_TOOL, TASK_UPDATE_TOOL, TaskActionKind,
};

/// The form state for creating a new task.
pub(crate) struct CreateTaskForm {
    pub title: Entity<Editor>,
    pub description: Entity<Editor>,
    pub criteria: Entity<Editor>,
}

impl CreateTaskForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<KanbanPanel>) -> Self {
        Self {
            title: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("Task title", window, cx);
                editor
            }),
            description: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("Description (optional)", window, cx);
                editor
            }),
            criteria: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text(
                    "Acceptance criteria, semicolon-separated (optional)",
                    window,
                    cx,
                );
                editor
            }),
        }
    }

    /// Collect the form values into a `kanban_task_create` args JSON.
    pub(crate) fn collect_args(&self, board_id: &str, cx: &gpui::App) -> serde_json::Value {
        let title = self.title.read(cx).text(cx);
        let description = self.description.read(cx).text(cx);
        let criteria_text = self.criteria.read(cx).text(cx);

        let mut args = json!({
            "board_id": board_id,
            "title": title,
        });

        if !description.trim().is_empty() {
            args["description"] = json!(description);
        }
        if !criteria_text.trim().is_empty() {
            let criteria: Vec<String> = criteria_text
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !criteria.is_empty() {
                args["criteria"] = json!(criteria);
            }
        }
        args
    }
}

/// The form state for editing an existing task.
pub(crate) struct EditTaskForm {
    pub title: Entity<Editor>,
    pub description: Entity<Editor>,
    pub priority: Entity<Editor>,
    pub labels: Entity<Editor>,
    /// The task id being edited.
    pub task_id: String,
}

impl EditTaskForm {
    pub(crate) fn for_task(
        task_id: &str,
        current_title: &str,
        current_description: Option<&str>,
        current_priority: Option<&str>,
        current_labels: &[String],
        window: &mut Window,
        cx: &mut Context<KanbanPanel>,
    ) -> Self {
        let title = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Task title", window, cx);
            editor.set_text(current_title.to_string(), window, cx);
            editor
        });
        let description = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Description (empty to clear)", window, cx);
            if let Some(desc) = current_description {
                editor.set_text(desc.to_string(), window, cx);
            }
            editor
        });
        let priority = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Priority: low, medium, high, critical", window, cx);
            if let Some(p) = current_priority {
                editor.set_text(p.to_string(), window, cx);
            }
            editor
        });
        let labels = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Labels, comma-separated", window, cx);
            if !current_labels.is_empty() {
                editor.set_text(current_labels.join(", "), window, cx);
            }
            editor
        });
        Self {
            title,
            description,
            priority,
            labels,
            task_id: task_id.to_string(),
        }
    }

    /// Collect the form values into a `kanban_task_update` args JSON.
    pub(crate) fn collect_args(&self, cx: &gpui::App) -> serde_json::Value {
        let title = self.title.read(cx).text(cx);
        let description = self.description.read(cx).text(cx);
        let priority = self.priority.read(cx).text(cx);
        let labels_text = self.labels.read(cx).text(cx);

        let mut args = json!({ "task_id": self.task_id });

        if !title.trim().is_empty() {
            args["title"] = json!(title);
        }
        // Empty description clears the field; non-empty sets it.
        args["description"] = json!(description);
        if !priority.trim().is_empty() {
            args["priority"] = json!(priority);
        }
        if !labels_text.trim().is_empty() {
            let labels: Vec<String> = labels_text
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            args["labels"] = json!(labels);
        }
        args
    }
}

/// The form state for spawning a subagent on a task.
pub(crate) struct SpawnTaskForm {
    pub skills: Entity<Editor>,
    pub delegation_level: Entity<Editor>,
    pub swarm_id: Entity<Editor>,
    /// The task id to spawn.
    pub task_id: String,
}

impl SpawnTaskForm {
    pub(crate) fn for_task(
        task_id: &str,
        window: &mut Window,
        cx: &mut Context<KanbanPanel>,
    ) -> Self {
        Self {
            skills: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text(
                    "Skills, comma-separated (e.g. tdd, bug-hunt)",
                    window,
                    cx,
                );
                editor
            }),
            delegation_level: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text(
                    "Delegation level: minimal, standard, maximal",
                    window,
                    cx,
                );
                editor.set_text("standard".to_string(), window, cx);
                editor
            }),
            swarm_id: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("Swarm id (optional)", window, cx);
                editor
            }),
            task_id: task_id.to_string(),
        }
    }

    /// Collect the form values into a `kanban_task_spawn` args JSON.
    pub(crate) fn collect_args(&self, cx: &gpui::App) -> serde_json::Value {
        let skills_text = self.skills.read(cx).text(cx);
        let level = self.delegation_level.read(cx).text(cx);
        let swarm_id = self.swarm_id.read(cx).text(cx);

        let mut args = json!({
            "task_id": self.task_id,
            "delegation_level": if level.trim().is_empty() { "standard" } else { level.trim() },
        });

        if !skills_text.trim().is_empty() {
            let skills: Vec<String> = skills_text
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            args["delegated_skills"] = json!(skills);
        }
        if !swarm_id.trim().is_empty() {
            args["swarm_id"] = json!(swarm_id);
        }
        args
    }
}

/// Render the create-task form.
pub(crate) fn render_create_task_form(
    form: &CreateTaskForm,
    cx: &mut Context<KanbanPanel>,
) -> impl IntoElement {
    let border_color = cx.theme().colors().border;
    let bg = cx.theme().colors().editor_background;

    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Label::new("New Task").size(LabelSize::Small))
                .child(div().flex_1().child(form.title.clone())),
        )
        .child(div().child(form.description.clone()))
        .child(div().child(form.criteria.clone()))
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id("kanban-create-task-submit")
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(Color::Accent.color(cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit_create_task(cx);
                        }))
                        .child(
                            Label::new("Create Task")
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        ),
                )
                .child(
                    div()
                        .id("kanban-create-task-cancel")
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
}

/// Render the edit-task form.
pub(crate) fn render_edit_task_form(
    form: &EditTaskForm,
    cx: &mut Context<KanbanPanel>,
) -> impl IntoElement {
    let border_color = cx.theme().colors().border;
    let bg = cx.theme().colors().editor_background;

    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Label::new("Edit Task").size(LabelSize::Small))
                .child(div().flex_1().child(form.title.clone())),
        )
        .child(div().child(form.description.clone()))
        .child(
            h_flex()
                .gap_2()
                .child(div().w_48().child(form.priority.clone()))
                .child(div().flex_1().child(form.labels.clone())),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id("kanban-edit-task-submit")
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(Color::Accent.color(cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit_edit_task(cx);
                        }))
                        .child(
                            Label::new("Save")
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        ),
                )
                .child(
                    div()
                        .id("kanban-edit-task-cancel")
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
}

/// Render the spawn-task form.
pub(crate) fn render_spawn_task_form(
    form: &SpawnTaskForm,
    cx: &mut Context<KanbanPanel>,
) -> impl IntoElement {
    let border_color = cx.theme().colors().border;
    let bg = cx.theme().colors().editor_background;

    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Label::new("Spawn Subagent").size(LabelSize::Small))
                .child(div().flex_1().child(form.skills.clone())),
        )
        .child(
            h_flex()
                .gap_2()
                .child(div().w_48().child(form.delegation_level.clone()))
                .child(div().flex_1().child(form.swarm_id.clone())),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id("kanban-spawn-task-submit")
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(Color::Accent.color(cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit_spawn_task(cx);
                        }))
                        .child(
                            Label::new("Spawn")
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        ),
                )
                .child(
                    div()
                        .id("kanban-spawn-task-cancel")
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
}

/// Render the create-board form.
pub(crate) fn render_create_board_form(
    name_editor: &Entity<Editor>,
    cx: &mut Context<KanbanPanel>,
) -> impl IntoElement {
    let border_color = cx.theme().colors().border;
    let bg = cx.theme().colors().editor_background;

    v_flex()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(border_color)
        .bg(bg)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Label::new("New Board").size(LabelSize::Small))
                .child(div().flex_1().child(name_editor.clone())),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id("kanban-create-board-submit")
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(Color::Accent.color(cx))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit_create_board(cx);
                        }))
                        .child(
                            Label::new("Create Board")
                                .size(LabelSize::Small)
                                .color(Color::Default),
                        ),
                )
                .child(
                    div()
                        .id("kanban-create-board-cancel")
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
}

impl KanbanPanel {
    // ── Task action handlers ───────────────────────────────────────────────

    /// Start the create-task flow: show the inline form.
    pub(crate) fn start_create_task(&mut self, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.create_task_form = None;
        self.active_action = Some(TaskActionKind::CreateTask);
        cx.notify();
    }

    /// Submit the create-task form. Calls `kanban_task_create` and refreshes.
    pub(crate) fn submit_create_task(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn start_edit_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.edit_task_form = None;
        self.active_action = Some(TaskActionKind::EditTask(task_id));
        cx.notify();
    }

    /// Submit the edit-task form. Calls `kanban_task_update` and refreshes.
    pub(crate) fn submit_edit_task(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn start_spawn_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.spawn_task_form = None;
        self.active_action = Some(TaskActionKind::SpawnTask(task_id));
        cx.notify();
    }

    /// Submit the spawn-task form. Calls `kanban_task_spawn`.
    pub(crate) fn submit_spawn_task(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn toggle_task_assignment(
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
    pub(crate) fn confirm_delete_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        self.active_action = Some(TaskActionKind::ConfirmDeleteTask(task_id));
        cx.notify();
    }

    /// Execute the task deletion.
    pub(crate) fn execute_delete_task(&mut self, task_id: String, cx: &mut Context<Self>) {
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
    pub(crate) fn start_create_board(&mut self, cx: &mut Context<Self>) {
        // The form is created lazily in `render` where a Window is available.
        self.create_board_editor = None;
        self.active_action = Some(TaskActionKind::CreateBoard);
        cx.notify();
    }

    /// Submit the create-board form.
    pub(crate) fn submit_create_board(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn confirm_delete_board(&mut self, cx: &mut Context<Self>) {
        self.active_action = Some(TaskActionKind::ConfirmDeleteBoard);
        cx.notify();
    }

    /// Execute the board deletion.
    pub(crate) fn execute_delete_board(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn clear_board_selection(&mut self) {
        self.selected_board_id = None;
        self.board_name = None;
        self.tasks.clear();
        self.kanban_widget = None;
    }

    /// Export the selected board as mermaid kanban markdown and copy it to
    /// the system clipboard. The markdown round-trips through `import_board`.
    /// Only the board owner can export (the server enforces P12); a
    /// permission error surfaces in the error strip.
    pub(crate) fn export_board(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn import_board(&mut self, cx: &mut Context<Self>) {
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
}
