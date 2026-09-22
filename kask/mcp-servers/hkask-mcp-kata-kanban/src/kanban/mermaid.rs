//! Mermaid kanban markdown export and import for kanban boards.
//!
//! Renders a [`Board`] and its tasks as a mermaid kanban markdown block using
//! the `section`-based syntax, and parses that markdown back into a
//! [`ParsedBoard`] suitable for re-creating the board through the service layer.
//!
//! ## Format
//!
//! ````text
//! ```mermaid
//! kanban
//! %% kanban board: <optional board name>
//! %% kanban column status: backlog
//!   section Backlog
//!     Task Title 1
//!     Task Title 2
//! %% kanban column status: in_progress
//!   section In Progress
//!     Task Title 3
//! ```
//! ````
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

/// Errors returned by [`parse_mermaid_kanban`].
#[derive(Debug, thiserror::Error)]
pub(crate) enum MermaidParseError {
    /// The markdown did not contain a `kanban` directive on its own line.
    #[error("not a mermaid kanban block: missing `kanban` directive")]
    MissingKanbanDirective,
    #[error("kanban has {count} columns but at most {max} distinct task statuses are supported")]
    TooManyColumns { count: usize, max: usize },
    #[error("kanban columns map more than once to status {status}")]
    DuplicateStatus { status: TaskStatus },
    #[error("kanban section {column:?} has no preceding `%% kanban column status:` comment")]
    MissingColumnStatus { column: String },
    #[error("invalid kanban column status {status:?}")]
    InvalidColumnStatus { status: String },
}

/// A parsed column from mermaid kanban markdown: a name and the task titles
/// in the order they appeared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedColumn {
    pub name: String,
    pub status: TaskStatus,
    pub tasks: Vec<String>,
}

/// A parsed board from mermaid kanban markdown: an optional board name (from
/// the `%% kanban board: <name>` comment) and the columns in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedBoard {
    pub name: Option<String>,
    pub columns: Vec<ParsedColumn>,
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
pub(crate) fn export_board_to_mermaid(board: &Board, tasks: &[Task]) -> String {
    let mut out = String::from("```mermaid\nkanban\n");
    out.push_str(&format!("%% kanban board: {}\n", board.name));
    for column in &board.columns {
        out.push_str(&format!(
            "%% kanban column status: {}\n",
            column.status.as_str()
        ));
        out.push_str(&format!("  section {}\n", column.name));
        let mut column_tasks = tasks
            .iter()
            .filter(|task| task.status == column.status)
            .collect::<Vec<_>>();
        column_tasks.reverse();
        for task in column_tasks {
            out.push_str(&format!("    {}\n", escape_title(&task.title)));
        }
    }
    out.push_str("```");
    out
}

/// Parse mermaid kanban markdown into a [`ParsedBoard`].
///
/// Returns [`MermaidParseError::MissingKanbanDirective`] if the markdown does
/// not contain the `kanban` directive on its own line. The error message
/// references `kanban` so callers can distinguish "not a kanban block" from
/// other parse failures.
pub(crate) fn parse_mermaid_kanban(markdown: &str) -> Result<ParsedBoard, MermaidParseError> {
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
        return Err(MermaidParseError::MissingKanbanDirective);
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

/// Build [`ColumnDef`]s from a parsed board with a one-to-one status mapping.
///
/// Standard status names claim their matching status. Other names receive the
/// next unused status in [`TaskStatus::STANDARD_ORDER`]. More than five columns
/// or two columns naming the same standard status are rejected explicitly.
pub(crate) fn columns_from_parsed(
    parsed: &ParsedBoard,
) -> Result<Vec<ColumnDef>, MermaidParseError> {
    if parsed.columns.len() > TaskStatus::STANDARD_ORDER.len() {
        return Err(MermaidParseError::TooManyColumns {
            count: parsed.columns.len(),
            max: TaskStatus::STANDARD_ORDER.len(),
        });
    }

    let mut used = std::collections::HashSet::new();
    let mut assigned = Vec::with_capacity(parsed.columns.len());
    for column in &parsed.columns {
        let status = match_column_name_to_status(&column.name);
        if let Some(status) = status
            && !used.insert(status)
        {
            return Err(MermaidParseError::DuplicateStatus { status });
        }
        assigned.push(status);
    }
    for status in &mut assigned {
        if status.is_some() {
            continue;
        }
        let generated = TaskStatus::STANDARD_ORDER
            .iter()
            .copied()
            .find(|candidate| !used.contains(candidate))
            .ok_or(MermaidParseError::TooManyColumns {
                count: parsed.columns.len(),
                max: TaskStatus::STANDARD_ORDER.len(),
            })?;
        used.insert(generated);
        *status = Some(generated);
    }

    parsed
        .columns
        .iter()
        .zip(assigned)
        .enumerate()
        .map(|(position, (column, status))| {
            status
                .map(|status| ColumnDef::new(column.name.clone(), status, position as u32))
                .ok_or(MermaidParseError::TooManyColumns {
                    count: parsed.columns.len(),
                    max: TaskStatus::STANDARD_ORDER.len(),
                })
        })
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
    use super::{TaskStatus, columns_from_parsed, parse_mermaid_kanban, slugify_task_id};

    #[test]
    fn slugify_alphanumeric_id_keeps_content_under_t_prefix() {
        assert_eq!(slugify_task_id("task-42"), "t_task_42");
    }

    #[test]
    fn slugify_digit_leading_id_still_gets_letter_prefix() {
        // Mermaid node ids must start with a letter. A raw id beginning with a
        // digit must still be prefixed with `t_`; the prefix is load-bearing,
        // not cosmetic.
        assert_eq!(slugify_task_id("9start"), "t_9start");
        assert!(slugify_task_id("9start").starts_with('t'));
    }

    #[test]
    fn slugify_empty_or_all_symbols_id_falls_back_to_t_task() {
        assert_eq!(slugify_task_id(""), "t_task");
        assert_eq!(slugify_task_id("!!!"), "t_task");
        assert_eq!(slugify_task_id("___"), "t_task");
    }

    #[test]
    fn slugify_trims_leading_and_trailing_underscores() {
        assert_eq!(slugify_task_id("_foo_"), "t_foo");
    }

    #[test]
    fn slugify_is_not_involutive_output_must_not_be_refed() {
        // Pinning a known property, not a bug: applying slugify twice
        // double-prefixes. Render and parse each apply it once, so this is
        // safe today; a future consumer that re-feeds a rendered node id
        // would break. This test exists so a "fix" that makes it involutive
        // (e.g. stripping an existing `t_` prefix) trips here before it
        // silently introduces id collisions (`"foo"` and `"t_foo"` would
        // both slug to `"t_foo"`).
        let once = slugify_task_id("foo");
        let twice = slugify_task_id(&once);
        assert_ne!(once, twice, "slugify must not be involutive; see doc note");
        assert_eq!(once, "t_foo");
        assert_eq!(twice, "t_t_foo");
    }

    /// expect: "Explicit status names keep their status even when an unnamed column appears first."
    /// [P3] Motivating: Generative Space — imported column identity does not depend on source order.
    /// [P4] Constraining: Clear Boundaries — generated assignments cannot steal an explicit status.
    /// pre: an unknown column appears before an explicitly named Backlog column
    /// post: conversion succeeds with two distinct statuses and Backlog belongs to the named column
    #[test]
    fn explicit_status_is_reserved_before_assigning_unknown_columns() {
        let markdown = "kanban\n  section Queue\n  section Backlog\n";
        let parsed = parse_mermaid_kanban(markdown).expect("valid mermaid parses");
        let columns = columns_from_parsed(&parsed).expect("columns map uniquely");
        assert_eq!(columns.len(), 2);
        assert_ne!(columns[0].status, TaskStatus::Backlog);
        assert_eq!(columns[1].status, TaskStatus::Backlog);
    }

    /// expect: "Mermaid import rejects workflows that cannot map one-to-one onto TaskStatus."
    /// [P3] Motivating: Generative Space — imported columns remain unambiguous workflow states.
    /// [P4] Constraining: Clear Boundaries — no sixth column silently aliases Backlog.
    /// pre: a mermaid board contains six sections but TaskStatus has five variants
    /// post: column conversion returns an explicit error
    #[test]
    fn six_columns_are_rejected_instead_of_falling_back_to_backlog() {
        let markdown = "kanban\n  section One\n  section Two\n  section Three\n  section Four\n  section Five\n  section Six\n";
        let parsed = parse_mermaid_kanban(markdown).expect("valid mermaid parses");
        assert!(columns_from_parsed(&parsed).is_err());
    }
}
