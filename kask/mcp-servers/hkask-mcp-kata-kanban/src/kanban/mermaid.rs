//! Mermaid kanban markdown export and import for kanban boards.
//!
//! Renders a [`Board`] and its tasks as a mermaid kanban markdown block using
//! the `section`-based syntax, and parses that markdown back into a
//! [`ParsedBoard`] suitable for re-creating the board through the service layer.
//!
//! ## Format
//!
//! ```text
//! ```mermaid
//! kanban
//! %% kanban board: <optional board name>
//!   section Backlog
//!     Task Title 1
//!     Task Title 2
//!   section In Progress
//!     Task Title 3
//! ```
//! ```
//!
//! The format is intentionally minimal: columns (sections) and task titles
//! only. Rich metadata (description, criteria, labels, assignees) is not
//! preserved — the round-trip is structural, not semantic. Task titles with
//! special characters (quotes, brackets, unicode, backslashes) are escaped on
//! export and unescaped on parse so they survive the round-trip unchanged.
//!
//! ## Why not the official mermaid kanban syntax?
//!
//! The official syntax (`columnId[Title]` / `taskId[Description]`) requires
//! unique identifiers and square-bracket-quoted descriptions. The `section`
//! syntax is simpler, human-editable, and round-trips task titles verbatim
//! without identifier management. It renders correctly in any mermaid renderer
//! that supports the `kanban` directive.

use crate::kanban::{Board, ColumnDef, Task, TaskStatus};

/// A task reduced to the fields the mermaid format can carry: a slugified id
/// (for renderers that benefit from unique node ids) and the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    /// Slugified task id — safe to use as a mermaid node identifier.
    pub id: String,
    /// Task title — escaped on export, unescaped on parse.
    pub title: String,
}

/// A parsed column from mermaid kanban markdown: a name and the task titles
/// in the order they appeared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedColumn {
    pub name: String,
    pub tasks: Vec<String>,
}

/// A parsed board from mermaid kanban markdown: an optional board name (from
/// the `%% kanban board: <name>` comment) and the columns in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBoard {
    pub name: Option<String>,
    pub columns: Vec<ParsedColumn>,
}

/// Slugify a task id for use as a mermaid node identifier. Replaces any
/// character that is not alphanumeric or underscore with `_` and prefixes
/// `t_` so the result is a valid mermaid identifier (which must start with
/// a letter).
pub fn slugify_task_id(id: &str) -> String {
    let slug: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('_');
    if trimmed.is_empty() {
        "t_task".to_string()
    } else if trimmed
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        format!("t_{trimmed}")
    } else {
        format!("t_{trimmed}")
    }
}

/// Escape a task title for mermaid kanban markdown. The title appears on its
/// own indented line under a `section`, so the only characters that need
/// escaping are those that would break line parsing: newlines (which would
/// start a new line and be misread as a task or section) and leading
/// whitespace (which would change indentation). We also escape backslash
/// so the unescape step is unambiguous.
fn escape_title(title: &str) -> String {
    title
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .trim()
        .to_string()
}

/// Unescape a task title parsed from mermaid kanban markdown. Reverses
/// [`escape_title`].
fn unescape_title(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    let mut chars = title.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Render a board and its tasks as mermaid kanban markdown.
///
/// Tasks are grouped by their column's status, preserving the board's column
/// order. Within each column, tasks are reversed from the input order so that
/// the markdown lists them in creation order (oldest first). This matches the
/// convention that `task_list` returns newest-first, and the parsed markdown's
/// source order is creation order (see the round-trip tests).
pub fn export_board_to_mermaid(board: &Board, tasks: &[Task]) -> String {
    let columns: Vec<(String, Vec<TaskSummary>)> = board
        .columns
        .iter()
        .map(|column| {
            // `task_list` returns newest-first; reverse to get creation order
            // so the markdown's source order is creation order.
            let mut column_tasks: Vec<TaskSummary> = tasks
                .iter()
                .filter(|task| task.status == column.status)
                .map(|task| TaskSummary {
                    id: slugify_task_id(&task.id.to_string()),
                    title: escape_title(&task.title),
                })
                .collect();
            column_tasks.reverse();
            (column.name.clone(), column_tasks)
        })
        .collect();
    export_board_to_mermaid_from_parts(&board.name, &columns)
}

/// Render mermaid kanban markdown from pre-grouped columns.
///
/// This is the lower-level entry point used by the MCP tool layer, which
/// builds the `(column_name, tasks)` pairs itself (e.g., to apply a different
/// task ordering or filtering before export).
pub fn export_board_to_mermaid_from_parts(
    board_name: &str,
    columns: &[(String, Vec<TaskSummary>)],
) -> String {
    let mut out = String::from("```mermaid\nkanban\n");
    // Board name comment — parsed back by `parse_mermaid_kanban`.
    out.push_str(&format!("%% kanban board: {board_name}\n"));
    for (column_name, column_tasks) in columns {
        out.push_str(&format!("  section {column_name}\n"));
        for task in column_tasks {
            out.push_str(&format!("    {}\n", task.title));
        }
    }
    out.push_str("```");
    out
}

/// Parse mermaid kanban markdown into a [`ParsedBoard`].
///
/// Returns an error (as a `String`) if the markdown does not contain the
/// `kanban` directive on its own line. The error message references `kanban`
/// so callers can distinguish "not a kanban block" from other parse failures.
pub fn parse_mermaid_kanban(markdown: &str) -> Result<ParsedBoard, String> {
    // Strip the ```mermaid ... ``` fence if present. We tolerate markdown
    // with or without the fence, and with or without leading/trailing
    // whitespace, but we require the `kanban` directive.
    let body = strip_code_fence(markdown);

    let mut name: Option<String> = None;
    let mut columns: Vec<ParsedColumn> = Vec::new();
    let mut current_column: Option<usize> = None;

    let mut saw_kanban_directive = false;

    for raw_line in body.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() {
            continue;
        }
        let trimmed = line.trim_start();

        // Board name comment: `%% kanban board: <name>`
        if let Some(rest) = trimmed.strip_prefix("%%") {
            let rest = rest.trim();
            if let Some(board_name) = rest.strip_prefix("kanban board:") {
                let board_name = board_name.trim();
                if !board_name.is_empty() {
                    name = Some(board_name.to_string());
                }
            }
            // Other %% comments are ignored.
            continue;
        }

        // `kanban` directive — must appear on its own line (after trimming).
        if trimmed == "kanban" {
            saw_kanban_directive = true;
            continue;
        }

        // If we haven't seen the `kanban` directive yet, we can't parse
        // sections. Keep scanning — the directive might appear later in an
        // oddly-ordered file. In practice it's always first.
        if !saw_kanban_directive {
            continue;
        }

        // `section <name>` — starts a new column.
        if let Some(column_name) = trimmed.strip_prefix("section ") {
            let column_name = column_name.trim();
            columns.push(ParsedColumn {
                name: column_name.to_string(),
                tasks: Vec::new(),
            });
            current_column = Some(columns.len() - 1);
            continue;
        }

        // Any other non-empty, indented line under a section is a task title.
        // Lines before the first section are ignored.
        if let Some(idx) = current_column {
            // The line must be indented relative to the section (i.e., it
            // has more leading whitespace than `  section`). We accept any
            // line that is indented at all and isn't a section/directive.
            let leading_ws = line.len() - trimmed.len();
            if leading_ws >= 4 {
                let title = unescape_title(trimmed);
                columns[idx].tasks.push(title);
            }
        }
    }

    if !saw_kanban_directive {
        return Err(
            "not a mermaid kanban block: missing `kanban` directive".to_string(),
        );
    }

    Ok(ParsedBoard { name, columns })
}

/// Strip a ```mermaid ... ``` code fence from `markdown`, returning the inner
/// body. If no fence is present, returns `markdown` unchanged.
fn strip_code_fence(markdown: &str) -> String {
    let trimmed = markdown.trim();
    if let Some(after_open) = trimmed.strip_prefix("```mermaid") {
        // Remove the opening fence line and any trailing ``` fence.
        let after_open = after_open.trim_start_matches('\n');
        if let Some(before_close) = after_open.strip_suffix("```") {
            before_close.trim_end_matches('\n').to_string()
        } else {
            // Opening fence but no closing — take everything after the opening.
            after_open.to_string()
        }
    } else if let Some(after_open) = trimmed.strip_prefix("```") {
        // Generic ``` fence (not ```mermaid). Tolerate it.
        let after_open = after_open.trim_start_matches('\n');
        if let Some(before_close) = after_open.strip_suffix("```") {
            before_close.trim_end_matches('\n').to_string()
        } else {
            after_open.to_string()
        }
    } else {
        markdown.to_string()
    }
}

/// Build [`ColumnDef`]s from a parsed board, mapping each parsed column to a
/// distinct [`TaskStatus`].
///
/// The mapping prefers standard status names (case-insensitive): "backlog",
/// "ready", "in progress" / "in_progress", "review", "done". Columns whose
/// names don't match a standard status are assigned statuses in
/// [`TaskStatus::STANDARD_ORDER`] by position, skipping any already claimed
/// by a name match. If all five standard statuses are claimed, non-matching
/// columns fall back to [`TaskStatus::Backlog`].
pub fn columns_from_parsed(parsed: &ParsedBoard) -> Vec<ColumnDef> {
    let mut claimed: Vec<Option<TaskStatus>> = vec![None; parsed.columns.len()];
    // First pass: match by name.
    for (i, column) in parsed.columns.iter().enumerate() {
        if let Some(status) = match_column_name_to_status(&column.name) {
            // Only claim if no earlier column claimed the same status.
            if !claimed.iter().any(|c| c == &Some(status)) {
                claimed[i] = Some(status);
            }
        }
    }
    // Second pass: assign remaining columns to the next unclaimed standard
    // status, in order. Collect into a fresh Vec to avoid borrowing `claimed`
    // mutably and immutably in the same loop.
    let mut next_standard = 0;
    let mut assigned: Vec<TaskStatus> = Vec::with_capacity(claimed.len());
    for claimed_status in &claimed {
        match claimed_status {
            Some(status) => assigned.push(*status),
            None => {
                let mut found = TaskStatus::Backlog;
                while next_standard < TaskStatus::STANDARD_ORDER.len() {
                    let candidate = TaskStatus::STANDARD_ORDER[next_standard];
                    next_standard += 1;
                    if !claimed.iter().any(|c| c == &Some(candidate)) {
                        found = candidate;
                        break;
                    }
                }
                assigned.push(found);
            }
        }
    }

    parsed
        .columns
        .iter()
        .enumerate()
        .map(|(i, column)| ColumnDef::new(column.name.clone(), assigned[i], i as u32))
        .collect()
}

/// Match a parsed mermaid column name to a [`TaskStatus`] by case-insensitive
/// comparison against the standard status display names and wire strings.
fn match_column_name_to_status(name: &str) -> Option<TaskStatus> {
    let lower = name.to_lowercase();
    // Standard display names.
    if lower == "backlog" {
        return Some(TaskStatus::Backlog);
    }
    if lower == "ready" {
        return Some(TaskStatus::Ready);
    }
    if lower == "in progress" || lower == "in_progress" || lower == "inprogress" {
        return Some(TaskStatus::InProgress);
    }
    if lower == "review" {
        return Some(TaskStatus::Review);
    }
    if lower == "done" {
        return Some(TaskStatus::Done);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_replaces_non_alphanumeric() {
        assert_eq!(slugify_task_id("task-123"), "t_task_123");
        assert_eq!(slugify_task_id("abc"), "t_abc");
        assert_eq!(slugify_task_id(""), "t_task");
        assert_eq!(slugify_task_id("---"), "t_task");
    }

    #[test]
    fn escape_and_unescape_round_trip() {
        for title in &[
            "plain",
            "Task with \"quotes\"",
            "Task with [brackets]",
            "Task with unicode: café ☕",
            "Task with backslash \\",
            "multi\nline",
        ] {
            let escaped = escape_title(title);
            assert!(
                !escaped.contains('\n'),
                "escaped title must not contain raw newlines: {escaped:?}"
            );
            let unescaped = unescape_title(&escaped);
            assert_eq!(&unescaped, title, "round-trip failed for {title:?}");
        }
    }

    #[test]
    fn export_then_parse_round_trips_structure() {
        let board_name = "Test Board";
        let columns = vec![
            ("Backlog".to_string(), vec![
                TaskSummary { id: "t_1".into(), title: escape_title("Task A") },
                TaskSummary { id: "t_2".into(), title: escape_title("Task B") },
            ]),
            ("Done".to_string(), vec![
                TaskSummary { id: "t_3".into(), title: escape_title("Task C") },
            ]),
        ];
        let markdown = export_board_to_mermaid_from_parts(board_name, &columns);
        assert!(markdown.starts_with("```mermaid\nkanban"));
        assert!(markdown.contains("%% kanban board: Test Board"));
        assert!(markdown.contains("  section Backlog"));
        assert!(markdown.contains("    Task A"));

        let parsed = parse_mermaid_kanban(&markdown).expect("parse");
        assert_eq!(parsed.name.as_deref(), Some("Test Board"));
        assert_eq!(parsed.columns.len(), 2);
        assert_eq!(parsed.columns[0].name, "Backlog");
        assert_eq!(parsed.columns[0].tasks, vec!["Task A", "Task B"]);
        assert_eq!(parsed.columns[1].name, "Done");
        assert_eq!(parsed.columns[1].tasks, vec!["Task C"]);
    }

    #[test]
    fn parse_rejects_missing_kanban_directive() {
        let md = "```mermaid\n  section Backlog\n    Task\n```";
        let result = parse_mermaid_kanban(md);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("kanban"), "error should mention kanban: {err}");
    }

    #[test]
    fn parse_accepts_empty_board() {
        let md = "```mermaid\nkanban\n  section Backlog\n```";
        let parsed = parse_mermaid_kanban(md).expect("parse");
        assert_eq!(parsed.columns.len(), 1);
        assert_eq!(parsed.columns[0].name, "Backlog");
        assert!(parsed.columns[0].tasks.is_empty());
    }

    #[test]
    fn parse_preserves_column_order() {
        let md = "```mermaid\nkanban\n  section Done\n  section Backlog\n```";
        let parsed = parse_mermaid_kanban(md).expect("parse");
        assert_eq!(parsed.columns.len(), 2);
        assert_eq!(parsed.columns[0].name, "Done");
        assert_eq!(parsed.columns[1].name, "Backlog");
    }

    #[test]
    fn columns_from_parsed_matches_standard_names() {
        let parsed = ParsedBoard {
            name: None,
            columns: vec![
                ParsedColumn { name: "Backlog".into(), tasks: vec![] },
                ParsedColumn { name: "In Progress".into(), tasks: vec![] },
                ParsedColumn { name: "Done".into(), tasks: vec![] },
            ],
        };
        let cols = columns_from_parsed(&parsed);
        assert_eq!(cols[0].status, TaskStatus::Backlog);
        assert_eq!(cols[1].status, TaskStatus::InProgress);
        assert_eq!(cols[2].status, TaskStatus::Done);
        assert_eq!(cols[0].position, 0);
        assert_eq!(cols[1].position, 1);
        assert_eq!(cols[2].position, 2);
    }

    #[test]
    fn columns_from_parsed_assigns_unclaimed_by_position() {
        // Non-standard names — should get Backlog, Ready, InProgress by position.
        let parsed = ParsedBoard {
            name: None,
            columns: vec![
                ParsedColumn { name: "Icebox".into(), tasks: vec![] },
                ParsedColumn { name: "Next".into(), tasks: vec![] },
                ParsedColumn { name: "Now".into(), tasks: vec![] },
            ],
        };
        let cols = columns_from_parsed(&parsed);
        assert_eq!(cols[0].status, TaskStatus::Backlog);
        assert_eq!(cols[1].status, TaskStatus::Ready);
        assert_eq!(cols[2].status, TaskStatus::InProgress);
    }
}
