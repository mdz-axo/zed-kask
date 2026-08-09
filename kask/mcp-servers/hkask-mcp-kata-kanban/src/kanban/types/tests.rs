#[cfg(test)]
use super::*;

// ── Tests ──────────────────────────────────────────────────────────────────
// `TaskStatus` itself (transitions, next, parse, display) is tested in
// `hkask_types::kanban_status`. The tests below cover the server's own types
// (Board, ColumnDef, Task, TaskSpec, VerificationCriterion, TaskFilter).

#[test]
fn board_column_for_status() {
    let columns = vec![
        ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
        ColumnDef::new("Ready".into(), TaskStatus::Ready, 1),
        ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 2),
        ColumnDef::new("Review".into(), TaskStatus::Review, 3),
        ColumnDef::new("Done".into(), TaskStatus::Done, 4),
    ];
    let board = Board::new("Test Board".into(), WebID::new(), columns);

    assert_eq!(
        board.column_for_status(TaskStatus::Backlog).unwrap().status,
        TaskStatus::Backlog
    );
    assert_eq!(
        board.column_for_status(TaskStatus::Done).unwrap().status,
        TaskStatus::Done
    );
}

#[test]
fn task_created_in_backlog() {
    let spec = TaskSpec::new("Test task".into());
    let task = Task::new(BoardId::new(), spec, WebID::new());
    assert_eq!(task.status, TaskStatus::Backlog);
    assert!(task.verification.is_none());
    assert!(task.assignee.is_none());
}

#[test]
fn task_spec_builder() {
    let spec = TaskSpec::new("Build CI".into())
        .with_description("Set up CI pipeline".into())
        .with_criteria(vec![VerificationCriterion::new("All tests pass".into())]);

    assert_eq!(spec.title, "Build CI");
    assert_eq!(spec.description, Some("Set up CI pipeline".into()));
    assert_eq!(spec.criteria.len(), 1);
}

#[test]
fn verification_criterion_with_llm() {
    let vc = VerificationCriterion::new("Task must compile".into())
        .with_llm_prompt("Check if the code compiles without errors".into());

    assert_eq!(vc.description, "Task must compile");
    assert!(vc.llm_prompt.is_some());
}

#[test]
fn task_filter_by_status() {
    let filter = TaskFilter::by_status(TaskStatus::InProgress);
    assert_eq!(filter.status, Some(TaskStatus::InProgress));
    assert!(filter.assignee.is_none());
}
