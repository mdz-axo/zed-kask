use super::service::KanbanService;
use crate::VerificationCriterion;
use crate::kanban::{Board, ColumnDef, TaskFilter, TaskSpec, TaskStatus};
use hkask_storage::HMemStore;
use hkask_types::WebID;
use hkask_types::id::BoardId;

fn make_store() -> HMemStore {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver);
    store
        .driver()
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS hmems (
                id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                recalled_at TEXT NOT NULL DEFAULT (datetime('now')),
                confidence REAL NOT NULL, perspective TEXT, visibility TEXT NOT NULL,
                owner_webid TEXT NOT NULL,
                dimension TEXT
            )",
        )
        .unwrap();
    store
}

fn make_default_columns() -> Vec<ColumnDef> {
    KanbanService::standard_columns()
}

fn make_service_with_board() -> (KanbanService, Board, WebID) {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc
        .board_create(owner, "Test Board", &make_default_columns())
        .unwrap();
    (svc, board, owner)
}

#[test]
fn board_create_succeeds() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc
        .board_create(owner, "My Board", &make_default_columns())
        .unwrap();
    assert_eq!(board.name, "My Board");
    assert_eq!(board.owner, owner);
    assert_eq!(board.columns.len(), 5);
}

#[test]
fn board_create_rejects_empty_name() {
    let svc = KanbanService::new(make_store());
    let result = svc.board_create(WebID::new(), "", &make_default_columns());
    assert!(result.is_err());
}

#[test]
fn board_create_rejects_empty_columns() {
    let svc = KanbanService::new(make_store());
    let result = svc.board_create(WebID::new(), "Board", &[]);
    assert!(result.is_err());
}

#[test]
fn board_list_by_owner() {
    let svc = KanbanService::new(make_store());
    let alice = WebID::new();
    let bob = WebID::new();

    svc.board_create(alice, "Alice's Board", &make_default_columns())
        .unwrap();
    svc.board_create(bob, "Bob's Board", &make_default_columns())
        .unwrap();

    let alice_boards = svc.board_list(&alice).unwrap();
    assert_eq!(alice_boards.len(), 1);
    assert_eq!(alice_boards[0].name, "Alice's Board");
}

#[test]
fn task_create_defaults_to_backlog() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();
    assert_eq!(task.status, TaskStatus::Backlog);
    assert_eq!(task.board_id, board.id);
}

#[test]
fn task_create_rejects_unknown_board() {
    let svc = KanbanService::new(make_store());
    let result = svc.task_create(BoardId::new(), TaskSpec::new("Test".into()), WebID::new());
    assert!(result.is_err());
}

#[test]
fn task_list_unfiltered() {
    let (svc, board, owner) = make_service_with_board();
    svc.task_create(board.id, TaskSpec::new("T1".into()), owner)
        .unwrap();
    svc.task_create(board.id, TaskSpec::new("T2".into()), owner)
        .unwrap();

    let tasks = svc.task_list(board.id, TaskFilter::all()).unwrap();
    assert_eq!(tasks.len(), 2);
}

#[test]
fn task_list_filter_by_status() {
    let (svc, board, owner) = make_service_with_board();
    let t1 = svc
        .task_create(board.id, TaskSpec::new("T1".into()), owner)
        .unwrap();
    svc.task_move(t1.id, TaskStatus::Ready, owner).unwrap();
    svc.task_move(t1.id, TaskStatus::InProgress, owner).unwrap();

    svc.task_create(board.id, TaskSpec::new("T2".into()), owner)
        .unwrap();

    let backlog = svc
        .task_list(board.id, TaskFilter::by_status(TaskStatus::Backlog))
        .unwrap();
    assert_eq!(backlog.len(), 1);

    let in_progress = svc
        .task_list(board.id, TaskFilter::by_status(TaskStatus::InProgress))
        .unwrap();
    assert_eq!(in_progress.len(), 1);
}

#[test]
fn task_move_forward() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();

    let t = svc.task_move(task.id, TaskStatus::Ready, owner).unwrap();
    assert_eq!(t.status, TaskStatus::Ready);

    let t = svc
        .task_move(task.id, TaskStatus::InProgress, owner)
        .unwrap();
    assert_eq!(t.status, TaskStatus::InProgress);
}

#[test]
fn task_move_rejects_skip() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();

    let result = svc.task_move(task.id, TaskStatus::InProgress, owner);
    assert!(result.is_err());
}

#[test]
fn task_claim_records_authenticated_actor() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();
    let agent = WebID::new();

    let assigned = svc.task_claim(task.id, agent).unwrap();
    assert_eq!(assigned.assignee, Some(agent));
    assert!(matches!(
        svc.task_claim(task.id, WebID::new()),
        Err(super::KanbanError::PermissionDenied(_))
    ));
}

#[test]
fn task_claim_rejects_in_progress_task() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();
    // Move to InProgress (not claimable)
    svc.task_move(task.id, TaskStatus::Ready, owner).unwrap();
    svc.task_move(task.id, TaskStatus::InProgress, owner)
        .unwrap();

    let err = svc.task_claim(task.id, WebID::new()).unwrap_err();
    assert!(
        matches!(err, super::KanbanError::InvalidTransition { .. }),
        "claiming an InProgress task should fail with InvalidTransition, got: {err}"
    );
}

#[test]
fn task_claim_rejects_done_task() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();
    // Move all the way to Done (not claimable)
    svc.task_move(task.id, TaskStatus::Ready, owner).unwrap();
    svc.task_move(task.id, TaskStatus::InProgress, owner)
        .unwrap();
    svc.task_move(task.id, TaskStatus::Review, owner).unwrap();
    svc.task_verify(task.id, "done", owner).unwrap();

    let err = svc.task_claim(task.id, WebID::new()).unwrap_err();
    assert!(
        matches!(err, super::KanbanError::InvalidTransition { .. }),
        "claiming a Done task should fail with InvalidTransition, got: {err}"
    );
}

#[test]
fn task_claim_accepts_ready_task() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();
    // Move to Ready (claimable)
    svc.task_move(task.id, TaskStatus::Ready, owner).unwrap();

    let agent = WebID::new();
    let assigned = svc.task_claim(task.id, agent).unwrap();
    assert_eq!(assigned.assignee, Some(agent));
    assert_eq!(assigned.status, TaskStatus::Ready);
}

#[test]
fn task_verify_pass() {
    let (svc, board, owner) = make_service_with_board();
    let spec = TaskSpec::new("Test".into())
        .with_criteria(vec![VerificationCriterion::new("compile".into())]);
    let task = svc.task_create(board.id, spec, owner).unwrap();

    svc.task_move(task.id, TaskStatus::Ready, owner).unwrap();
    svc.task_move(task.id, TaskStatus::InProgress, owner)
        .unwrap();
    svc.task_move(task.id, TaskStatus::Review, owner).unwrap();

    let (verified, _verif) = svc
        .task_verify(task.id, "The code compiles successfully", owner)
        .unwrap();
    assert_eq!(verified.status, TaskStatus::Done);
    assert!(verified.verification.as_ref().unwrap().passed);
}

#[test]
fn task_verify_rejects_non_review() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();

    let result = svc.task_verify(task.id, "evidence", owner);
    assert!(result.is_err());
}

#[test]
fn board_get_succeeds() {
    let (svc, board, _owner) = make_service_with_board();
    let retrieved = svc.board_get(board.id).unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "Test Board");
}

#[test]
fn board_isolation() {
    let svc = KanbanService::new(make_store());
    let alice = WebID::new();
    let bob = WebID::new();

    svc.board_create(alice, "Alice's Board", &make_default_columns())
        .unwrap();
    svc.board_create(bob, "Bob's Board", &make_default_columns())
        .unwrap();

    let alice_boards = svc.board_list(&alice).unwrap();
    assert_eq!(alice_boards.len(), 1);
    assert_eq!(alice_boards[0].name, "Alice's Board");
}
