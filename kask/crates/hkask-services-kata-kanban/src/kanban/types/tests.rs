#[cfg(test)]
use super::*;

// ── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn task_status_transitions() {
    // Forward transitions
    assert!(TaskStatus::Backlog.can_transition_to(TaskStatus::Ready));
    assert!(TaskStatus::Ready.can_transition_to(TaskStatus::InProgress));
    assert!(TaskStatus::InProgress.can_transition_to(TaskStatus::Review));
    assert!(TaskStatus::Review.can_transition_to(TaskStatus::Done));

    // Backward transitions (one step only)
    assert!(TaskStatus::Ready.can_transition_to(TaskStatus::Backlog));
    assert!(TaskStatus::InProgress.can_transition_to(TaskStatus::Ready));
    assert!(TaskStatus::Review.can_transition_to(TaskStatus::InProgress));

    // Done cannot transition anywhere
    assert!(!TaskStatus::Done.can_transition_to(TaskStatus::Review));
    assert!(!TaskStatus::Done.can_transition_to(TaskStatus::Backlog));

    // Skipping columns is prohibited
    assert!(!TaskStatus::Backlog.can_transition_to(TaskStatus::InProgress));
    assert!(!TaskStatus::Backlog.can_transition_to(TaskStatus::Review));
    assert!(!TaskStatus::Backlog.can_transition_to(TaskStatus::Done));
    assert!(!TaskStatus::Ready.can_transition_to(TaskStatus::Review));
    assert!(!TaskStatus::Ready.can_transition_to(TaskStatus::Done));
    assert!(!TaskStatus::InProgress.can_transition_to(TaskStatus::Done));
    assert!(!TaskStatus::InProgress.can_transition_to(TaskStatus::Backlog));
    assert!(!TaskStatus::Review.can_transition_to(TaskStatus::Backlog));
}

#[test]
fn task_status_next() {
    assert_eq!(TaskStatus::Backlog.next(), Some(TaskStatus::Ready));
    assert_eq!(TaskStatus::Ready.next(), Some(TaskStatus::InProgress));
    assert_eq!(TaskStatus::InProgress.next(), Some(TaskStatus::Review));
    assert_eq!(TaskStatus::Review.next(), Some(TaskStatus::Done));
    assert_eq!(TaskStatus::Done.next(), None);
}

#[test]
fn task_status_string_roundtrip() {
    for status in &[
        TaskStatus::Backlog,
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
        TaskStatus::Done,
    ] {
        let s = status.as_str();
        let parsed = TaskStatus::parse_str(s).unwrap();
        assert_eq!(*status, parsed);

        // Also test via FromStr
        let from_str: TaskStatus = s.parse().unwrap();
        assert_eq!(*status, from_str);
    }
}

#[test]
fn task_status_parse_aliases() {
    assert_eq!(
        TaskStatus::parse_str("inprogress"),
        Some(TaskStatus::InProgress)
    );
    assert_eq!(
        TaskStatus::parse_str("in-progress"),
        Some(TaskStatus::InProgress)
    );
    assert_eq!(
        TaskStatus::parse_str("IN_PROGRESS"),
        Some(TaskStatus::InProgress)
    );
    assert_eq!(TaskStatus::parse_str("Done"), Some(TaskStatus::Done));
    assert_eq!(TaskStatus::parse_str("invalid"), None);
}

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
