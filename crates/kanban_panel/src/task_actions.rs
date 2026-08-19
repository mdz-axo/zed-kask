//! Task action forms — create, edit, spawn, and delete task UI.
//!
//! Each form is a lightweight inline panel rendered below the board header.
//! Forms use `Editor::single_line` for text input (matching the swarm panel's
//! compose form pattern). Form state is owned by the `KanbanPanel` so it
//! persists across re-renders.

use editor::Editor;
use gpui::{Context, Entity, Window};
use serde_json::json;
use ui::prelude::*;

use crate::KanbanPanel;

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
                let mut editor = Editor::single_line(window, cx);
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
        if let Ok(budget) = gas_text.trim().parse::<u64>() {
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
                let mut editor = Editor::single_line(window, cx);
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
        if let Ok(budget) = gas_text.trim().parse::<u64>() {
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
