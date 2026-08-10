---
title: "hKask Kanban Widget — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-09
version: "1.0.0"
status: "Active"
domain: "Composition"
mds_categories: [composition]
---

# hKask Kanban Widget — Class Diagram

`hkask-kanban-widget` renders ` ```kanban ` fenced blocks as a horizontal
column layout (Backlog → Ready → In Progress → Review → Done). It is a passive
renderer: the data comes from the parsed `KanbanBlockBody` (JSON already in the
chat stream, mirroring the combined `kanban_board_list` + `kanban_task_list`
tool responses), not from live MCP fetches. Task moves are interactive (S4/Wave
1): the move affordance stages a pending move, the user confirms/cancels, and
the controller dispatches `kanban_task_move` via the governed
`shared_tool_invoker()` (OCAP/gas-budgeted). See the [Move Controller State
Diagram](state-kanban-move-controller.md) and the [Task Status State
Diagram](state-task-status.md).

```mermaid
classDiagram
    class KanbanBlockBody {
        +viz: Option~String~
        +board_id: Option~String~
        +board_name: Option~String~
        +tasks: Vec~TaskBody~
        +columns: Vec~ColumnBody~
        +provenance: BlockProvenance
        +board_with_tasks() board tuple
    }
    class ColumnBody {
        +status: String
        +wip_limit: Option~u32~
    }
    class TaskBody {
        +task_id: String
        +title: String
        +status: String
        +description: Option~String~
        +assignee: Option~String~
        +gas_remaining: Option~u64~
        +ontology: Option~String~
        +priority: Option~String~
        +labels: Vec~String~
        +criteria: Vec~String~
        +comments: Vec~CommentBody~
        +verification: Option~VerificationBody~
        +gas_spend: Vec~GasEntryBody~
    }
    class CommentBody {
        +author: String
        +body: String
        +created_at: String
    }
    class VerificationBody {
        +passed: bool
        +reason: String
    }
    class GasEntryBody {
        +amount: u64
        +reason: String
        +kind: String
    }
    class KanbanColumn {
        +status: String
        +title: String
        +tasks: Vec~TaskBody~
        +wip_limit: Option~u32~
    }
    class KanbanMoveController {
        +dispatch_in_flight: Option~String~
        +optimistic_move: Option~OptimisticMove~
        +dispatch_error: Option~String~
        +pending_move: Option~PendingMove~
        +new() KanbanMoveController
        +stage_move(...)
        +confirm_move(...)
        +cancel_move(...)
        +dispatch_move(...)
        +cancel_dispatch(...)
    }
    class KanbanWidget {
        +board_name: String
        +columns: Vec~KanbanColumn~
        +column_meta: Vec~ColumnBody~
        +provenance: BlockProvenance
        +focus_handle: FocusHandle
        +move_controller: KanbanMoveController
        +disagree_draft: Option~String~
        +expanded_descriptions: HashSet~String~
        +detail_open: Option~String~
        +new(body, cx) KanbanWidget
        +render_dispatch_status(cx)
        +evaluate_move(window, cx)
    }
    class create_kanban_widget {
        +create_kanban_widget(body, cx) Option~Entity~KanbanWidget~~
    }

    KanbanBlockBody "1" o-- "many" TaskBody : tasks
    KanbanBlockBody "1" o-- "many" ColumnBody : columns
    TaskBody "1" o-- "many" CommentBody : comments
    TaskBody "1" o-- "0..1" VerificationBody : verification
    TaskBody "1" o-- "many" GasEntryBody : gas_spend
    KanbanWidget "1" *-- "1" KanbanMoveController : move_controller
    KanbanWidget "1" o-- "many" KanbanColumn : columns
    KanbanColumn "1" o-- "many" TaskBody : tasks
    KanbanWidget ..|> gpui_Focusable : Focusable
    KanbanWidget ..|> gpui_Render : Render
    create_kanban_widget ..> KanbanWidget : viz is kanban
```

**Block shape:** a JSON body with `viz: "kanban"` and a single board
(`board_id` + `board_name` + `tasks`). The agent emits one block per board
when multiple boards are needed. `board_with_tasks()` returns the
`(board_id, board_name, tasks)` tuple, defaulting the name to the id or
`"Kanban Board"` when both are absent.

**Column grouping:** `group_tasks_into_columns` buckets tasks by
lowercased `status`, emits the five standard columns in order (attaching WIP
limits from `column_meta`), then appends any non-standard statuses sorted
alphabetically (title-cased).

**Move lifecycle (S9):** `KanbanMoveController` owns the dispatch state
machine (`pending_move`, `dispatch_in_flight`, `dispatch_error`,
`optimistic_move`). `KanbanWidget` delegates move lifecycle calls to it and
renders the Confirm/Cancel/Evaluate banner via `render_dispatch_status`
(reading controller state via accessors). The controller is a pure state
machine — it does not render.

**Card detail (B3):** `detail_open` holds the task id whose detail panel is
open. The panel renders the full task (description, criteria, comments,
verification, gas spend log) passively from the block body. Escape and the
Close button close it; click-outside is not implemented (inline panel).

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-KANBAN
verified_against: crates/hkask-kanban-widget/src/block.rs; crates/hkask-kanban-widget/src/view.rs; crates/hkask-kanban-widget/src/move_controller.rs
status: VERIFIED
note: Fields and class structure verified against block.rs, view.rs, and move_controller.rs after S9 refactor (render_dispatch_status moved back to widget; controller is pure state machine). Method bodies not re-verified.
-->
