---
title: "hKask Kanban Widget — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-04
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
tool responses), not from live MCP fetches. Read-only — task moves are done by
the agent calling `kanban_task_move` directly.

```mermaid
classDiagram
    class KanbanBlockBody {
        +viz: Option~String~
        +board_id: Option~String~
        +board_name: Option~String~
        +tasks: Vec~TaskBody~
        +provenance: BlockProvenance
        +board_with_tasks() board tuple
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
    }
    class KanbanColumn {
        +status: String
        +title: String
        +tasks: Vec~TaskBody~
    }
    class KanbanWidget {
        +board_name: String
        +columns: Vec~KanbanColumn~
        +provenance: BlockProvenance
        +focus_handle: FocusHandle
        +dispatch_in_flight: Option~String~
        +optimistic_move: Option~OptimisticMove~
        +dispatch_error: Option~String~
        +pending_move: Option~PendingMove~
        +disagree_draft: Option~String~
        +expanded_descriptions: HashSet~String~
        +new(body, cx) KanbanWidget
    }
    class create_kanban_widget {
        +create_kanban_widget(body, cx) Option~Entity~KanbanWidget~~
    }

    KanbanBlockBody "1" o-- "many" TaskBody : tasks
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
lowercased `status`, emits the five standard columns in order, then appends any
non-standard statuses sorted alphabetically (title-cased).

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-KANBAN
verified_against: crates/hkask-kanban-widget/src/block.rs; crates/hkask-kanban-widget/src/view.rs
status: STALE
note: Fields synced to S5/S7 (TaskBody: description/priority/labels/criteria) and S4/S5 (KanbanWidget: dispatch_in_flight/optimistic_move/dispatch_error/pending_move/disagree_draft/expanded_descriptions). Method bodies and render-tree relationships not re-verified.
-->
