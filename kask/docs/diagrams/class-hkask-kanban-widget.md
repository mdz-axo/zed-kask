# hKask Kanban Widget — Class Diagram

`hkask-kanban-widget` renders ```` ```kanban ```` fenced blocks as a horizontal
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
        +boards: Vec~BoardBody~
        +tasks_by_board: Vec~BoardTasksBody~
        +boards_with_tasks() Vec~(String, String, &[TaskBody])~
    }
    class BoardBody {
        +board_id: String
        +name: String
    }
    class BoardTasksBody {
        +board_id: String
        +tasks: Vec~TaskBody~
    }
    class TaskBody {
        +task_id: String
        +title: String
        +status: String
        +assignee: Option~String~
        +gas_remaining: Option~u64~
    }
    class KanbanColumn {
        +status: String
        +title: String
        +tasks: Vec~TaskBody~
    }
    class KanbanWidget {
        +board_name: String
        +columns: Vec~KanbanColumn~
        +focus_handle: FocusHandle
        +new(body, cx) KanbanWidget
    }
    class create_kanban_widget {
        +create_kanban_widget(body, cx) Option~Entity~KanbanWidget~~
    }

    KanbanBlockBody "1" o-- "many" TaskBody : tasks (single-board)
    KanbanBlockBody "1" o-- "many" BoardBody : boards (multi-board)
    KanbanBlockBody "1" o-- "many" BoardTasksBody : tasks_by_board
    BoardTasksBody "1" o-- "many" TaskBody : tasks
    KanbanWidget "1" o-- "many" KanbanColumn : columns
    KanbanColumn "1" o-- "many" TaskBody : tasks
    KanbanWidget ..|> gpui_Focusable [Focusable]
    KanbanWidget ..|> gpui_Render [Render]
    create_kanban_widget ..> KanbanWidget : viz == "kanban"
```

**Block shape:** a JSON body with `viz: "kanban"`. Two shapes are supported —
single-board (`board_id` + `board_name` + `tasks`) or multi-board (`boards` +
`tasks_by_board`). `boards_with_tasks()` reconciles the two; when both are
present and `tasks` is non-empty, the single-board shape wins. The widget
renders one board at a time (the first).

**Column grouping:** `group_tasks_into_columns` buckets tasks by
lowercased `status`, emits the five standard columns in order, then appends any
non-standard statuses sorted alphabetically (title-cased).

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-KANBAN
verified_date: 2026-08-03
verified_against: crates/hkask-kanban-widget/src/block.rs; crates/hkask-kanban-widget/src/view.rs
status: VERIFIED 2026-08-03
-->