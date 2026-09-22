use super::service::KanbanService;
use crate::VerificationCriterion;
use crate::kanban::mermaid::{columns_from_parsed, export_board_to_mermaid, parse_mermaid_kanban};
use crate::kanban::{
    Board, ColumnDef, CriterionCitation, SpawnSpec, TaskFilter, TaskSpec, TaskStatus,
};
use hkask_storage::HMemStore;
use hkask_types::WebID;
use hkask_types::id::BoardId;
use hkask_types::kanban_wire::KANBAN_BOARD_NAME_MAX_CHARS;

use super::types::KanbanError;

fn make_store() -> HMemStore {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    HMemStore::from_driver(driver).expect("hmem store init")
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

/// expect: "A board schema represents every new task in exactly one initial column."
/// [P3] Motivating: Generative Space — every created task is visible and movable on its board.
/// [P4] Constraining: Clear Boundaries — column statuses identify one unambiguous workflow position.
/// pre: a custom schema either omits Backlog or assigns Backlog to two columns
/// post: board creation rejects both schemas without writing a board
#[test]
fn board_create_rejects_unrepresentable_column_sets() {
    let svc = KanbanService::new(make_store());
    let missing_backlog = vec![ColumnDef::new("Ready".into(), TaskStatus::Ready, 0)];
    assert!(
        svc.board_create(WebID::new(), "No backlog", &missing_backlog)
            .is_err()
    );

    let duplicate_status = vec![
        ColumnDef::new("Inbox".into(), TaskStatus::Backlog, 0),
        ColumnDef::new("Also inbox".into(), TaskStatus::Backlog, 1),
    ];
    assert!(
        svc.board_create(WebID::new(), "Duplicate", &duplicate_status)
            .is_err()
    );
}

// ── Board name validation (reference model R1) ──────────────────────────
//
// The service boundary is the single enforcement point for every caller
// (panel, MCP agents, import). Before the trim landed here, an MCP caller
// could create a `"   "` board and the panel could store `"  My Board  "`
// — while the mermaid import path already trimmed. Falsifier for the
// pre-fix code: `board_create("   ")` succeeded.

/// T1: a whitespace-only name is rejected after trimming.
#[test]
fn board_create_rejects_whitespace_only_name() {
    let svc = KanbanService::new(make_store());
    assert!(
        svc.board_create(WebID::new(), "   \t ", &make_default_columns())
            .is_err()
    );
}

/// T1: a padded name is stored trimmed — the stored name is the identity
/// every surface (board_list, export, steer prompt) addresses the board by.
#[test]
fn board_create_stores_trimmed_name() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc
        .board_create(owner, "  Padded Board  ", &make_default_columns())
        .unwrap();
    assert_eq!(board.name, "Padded Board");
    assert_eq!(svc.board_list(&owner).unwrap()[0].name, "Padded Board");
}

/// The 128-character cap (operator decision 2026-09-18; Planka
/// `maxLength={128}` — §6.6). A name over the cap is rejected at the
/// single enforcement point, and the error names the cap.
#[test]
fn board_create_rejects_name_over_the_cap() {
    let svc = KanbanService::new(make_store());
    let over_cap = "x".repeat(KANBAN_BOARD_NAME_MAX_CHARS + 1);
    let err = svc
        .board_create(WebID::new(), &over_cap, &make_default_columns())
        .unwrap_err();
    assert!(
        matches!(&err, KanbanError::InvalidInput(msg) if msg.contains("longer than 128")),
        "the error must name the cap, got: {err:?}"
    );
}

/// The cap boundary: exactly 128 characters is valid. The cap bounds, it
/// never truncates — a silent truncation would address the board by a
/// name the caller never typed.
#[test]
fn board_create_accepts_name_at_the_cap() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let at_cap = "x".repeat(KANBAN_BOARD_NAME_MAX_CHARS);
    let board = svc
        .board_create(owner, &at_cap, &make_default_columns())
        .unwrap();
    assert_eq!(board.name.chars().count(), KANBAN_BOARD_NAME_MAX_CHARS);
    assert_eq!(svc.board_list(&owner).unwrap()[0].name, at_cap);
}

/// Rename enforces the same cap at the same boundary; a failed rename
/// leaves the original name intact (the T4 discipline).
#[test]
fn board_rename_rejects_name_over_the_cap() {
    let (svc, board, _owner) = make_service_with_board();
    let over_cap = "x".repeat(KANBAN_BOARD_NAME_MAX_CHARS + 1);
    assert!(svc.board_rename(board.id, &over_cap).is_err());
    assert_eq!(
        svc.board_list(&board.owner).unwrap()[0].name,
        "Test Board",
        "a failed rename must leave the original name intact"
    );
}

// ── Board rename (reference model R6) ──────────────────────────────────
//
// Rename is first-class because the name is the addressing key; it is
// convergent by construction (replaying the same rename re-applies the same
// name — the `task_update` class), so it carries no idempotency key.

/// T4: rename round-trips — the new name reaches `board_list`, and the
/// board's identity (id, columns, creation time) survives.
#[test]
fn board_rename_round_trips() {
    let (svc, board, _owner) = make_service_with_board();
    let renamed = svc.board_rename(board.id, "  Renamed Board  ").unwrap();
    assert_eq!(
        renamed.name, "Renamed Board",
        "rename must store the trimmed name"
    );
    assert_eq!(renamed.id, board.id);
    assert_eq!(renamed.columns.len(), board.columns.len());
    assert_eq!(renamed.created_at, board.created_at);

    let boards = svc.board_list(&renamed.owner).unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].name, "Renamed Board");
    // The task links survive — the board's PKO procedure root is the same
    // row it was before the rename.
    let spec = TaskSpec::new("Survivor".into());
    let task = svc.task_create(board.id, spec, renamed.owner).unwrap();
    assert_eq!(task.board_id, board.id);
}

/// T4: a name that is empty after trimming is rejected and the original
/// name survives the failed rename.
#[test]
fn board_rename_rejects_empty_after_trim() {
    let (svc, board, _owner) = make_service_with_board();
    assert!(svc.board_rename(board.id, "   ").is_err());
    assert_eq!(
        svc.board_list(&board.owner).unwrap()[0].name,
        "Test Board",
        "a failed rename must leave the original name intact"
    );
}

/// T4: renaming an unknown board is NotFound.
#[test]
fn board_rename_unknown_board_is_not_found() {
    let svc = KanbanService::new(make_store());
    assert!(svc.board_rename(BoardId::new(), "New Name").is_err());
}

/// T4: rename is convergent — renaming to the name the board already has
/// succeeds without duplicating anything.
#[test]
fn board_rename_to_same_name_converges() {
    let (svc, board, _owner) = make_service_with_board();
    let once = svc.board_rename(board.id, "Test Board").unwrap();
    assert_eq!(once.name, "Test Board");
    let twice = svc.board_rename(board.id, "Test Board").unwrap();
    assert_eq!(twice.name, "Test Board");
    assert_eq!(
        svc.board_list(&board.owner).unwrap().len(),
        1,
        "a converged rename must not mint a second board"
    );
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

/// expect: "Creating a task never publishes its payload without its board-membership index."
/// [P3] Motivating: Generative Space — a task is usable only as part of its board.
/// [P2] Constraining: Transparent Imperfection — a failed compound write leaves no hidden task.
/// pre: a board exists and insertion of its task-index row is forced to fail
/// post: task creation fails and neither task payload nor board index row exists
#[test]
fn task_create_atomic_when_index_insert_fails() -> anyhow::Result<()> {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver.clone())?;
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service.board_create(owner, "Board", &make_default_columns())?;
    driver.execute_batch(
        "CREATE TRIGGER reject_task_index_insert BEFORE INSERT ON hmems
         WHEN NEW.entity LIKE 'kanban:board_tasks:%'
         BEGIN SELECT RAISE(FAIL, 'forced task index failure'); END;",
    )?;

    assert!(
        service
            .task_create(board.id, TaskSpec::new("Task".into()), owner)
            .is_err()
    );
    assert!(service.task_list(board.id, TaskFilter::all())?.is_empty());
    assert!(store.query_by_entity("kanban:task")?.is_empty());
    assert!(
        store
            .query_by_entity(&format!("kanban:board_tasks:{}", board.id))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn task_create_validates_goal_citations() {
    // The functional–technical join: a task may cite the goal criteria it
    // advances. Citations are validated at creation — the goal must exist,
    // the index must be in range, and the text must match verbatim — and
    // captured on the task as documentation that outlives the ephemeral
    // goal.
    let (svc, board, owner) = make_service_with_board();
    let goal = svc
        .goal_create(
            "The user can see which work serves which goal".into(),
            vec![
                VerificationCriterion::new("criterion 0 is observable".into()),
                VerificationCriterion::new("criterion 1 is observable".into()),
            ],
            None,
            None,
            owner,
        )
        .unwrap();

    let valid_citation = CriterionCitation {
        goal_id: goal.id,
        criterion_index: 1,
        criterion_text: "criterion 1 is observable".into(),
    };
    let mut spec = TaskSpec::new("Cited task".into());
    spec.advances = vec![valid_citation];
    let task = svc.task_create(board.id, spec, owner).unwrap();
    assert_eq!(task.advances.len(), 1);
    assert_eq!(task.advances[0].criterion_index, 1);
    // The citation persists with the task.
    let reloaded = svc.task_get(task.id).unwrap().expect("task persists");
    assert_eq!(reloaded.advances.len(), 1);

    // Unknown goal → NotFound.
    let mut spec = TaskSpec::new("Ghost goal".into());
    spec.advances = vec![CriterionCitation {
        goal_id: hkask_types::id::GoalID::new(),
        criterion_index: 0,
        criterion_text: "criterion 0 is observable".into(),
    }];
    assert!(svc.task_create(board.id, spec, owner).is_err());

    // Out-of-range index → rejected.
    let mut spec = TaskSpec::new("Bad index".into());
    spec.advances = vec![CriterionCitation {
        goal_id: goal.id,
        criterion_index: 5,
        criterion_text: "criterion 5 is observable".into(),
    }];
    assert!(svc.task_create(board.id, spec, owner).is_err());

    // Text mismatch → rejected: the citation must quote the goal's
    // criterion verbatim, so the documentation cannot silently diverge
    // from the goal.
    let mut spec = TaskSpec::new("Drifted text".into());
    spec.advances = vec![CriterionCitation {
        goal_id: goal.id,
        criterion_index: 0,
        criterion_text: "a paraphrase, not the criterion".into(),
    }];
    assert!(svc.task_create(board.id, spec, owner).is_err());
}

#[test]
fn task_update_replaces_and_validates_goal_citations() {
    // `advances` on task_update replaces the citation list and re-anchors
    // each citation against the live goal store — an invalid replacement
    // is rejected and leaves the task's existing citations untouched.
    let (svc, board, owner) = make_service_with_board();
    let goal = svc
        .goal_create(
            "The user can see which work serves which goal".into(),
            vec![
                VerificationCriterion::new("criterion 0 is observable".into()),
                VerificationCriterion::new("criterion 1 is observable".into()),
            ],
            None,
            None,
            owner,
        )
        .unwrap();
    let task = svc
        .task_create(board.id, TaskSpec::new("Cited task".into()), owner)
        .unwrap();
    assert!(task.advances.is_empty());

    let updated = svc
        .task_update(
            task.id,
            owner,
            None,
            None,
            None,
            None,
            None,
            Some(vec![CriterionCitation {
                goal_id: goal.id,
                criterion_index: 0,
                criterion_text: "criterion 0 is observable".into(),
            }]),
        )
        .unwrap();
    assert_eq!(updated.advances.len(), 1);

    // An out-of-range replacement is rejected; the prior citation stands.
    let bad = svc.task_update(
        task.id,
        owner,
        None,
        None,
        None,
        None,
        None,
        Some(vec![CriterionCitation {
            goal_id: goal.id,
            criterion_index: 9,
            criterion_text: "criterion 9 is observable".into(),
        }]),
    );
    assert!(bad.is_err());
    let reloaded = svc.task_get(task.id).unwrap().expect("task persists");
    assert_eq!(reloaded.advances.len(), 1);
    assert_eq!(reloaded.advances[0].criterion_index, 0);
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
fn task_move_uses_custom_board_column_order() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = vec![
        ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
        ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 1),
        ColumnDef::new("Done".into(), TaskStatus::Done, 2),
    ];
    let board = svc.board_create(owner, "Three columns", &columns).unwrap();
    let task = svc
        .task_create(board.id, TaskSpec::new("Test".into()), owner)
        .unwrap();

    let task = svc
        .task_move(task.id, TaskStatus::InProgress, owner)
        .unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);

    let task = svc.task_move(task.id, TaskStatus::Done, owner).unwrap();
    assert_eq!(task.status, TaskStatus::Done);
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

// ── Kanban-as-swarm-coordination tests (Slices 1-4) ──────────────────────

#[test]
fn task_swarm_fields_default_to_none() {
    // Slice 1: new tasks have swarm_id, delegate_result, deterministic_verdict = None.
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Swarm task".into()), owner)
        .unwrap();
    assert!(task.swarm_id.is_none(), "swarm_id should default to None");
    assert!(
        task.delegate_result.is_none(),
        "delegate_result should default to None"
    );
    assert!(
        task.deterministic_verdict.is_none(),
        "deterministic_verdict should default to None"
    );
}

#[test]
fn spawn_task_writes_swarm_id_when_spec_carries_it() {
    // C2: `spawn_task` writes `SpawnSpec.swarm_id` to `Task.swarm_id` so the
    // durable link is set before worktree admission in `kanban_task_spawn`
    // and exposed through `kanban_task_delegate_result`.
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Spawn task".into()), owner)
        .unwrap();
    let spec = SpawnSpec::new(task.id).with_swarm(Some("sw-42".to_string()));
    svc.spawn_task(task.id, spec, owner).unwrap();
    let reloaded = svc.task_get(task.id).unwrap().expect("task should persist");
    assert_eq!(
        reloaded.swarm_id.as_deref(),
        Some("sw-42"),
        "spawn_task must persist the spec's swarm_id"
    );
}

#[test]
fn spawn_task_leaves_swarm_id_none_when_spec_omits_it() {
    // C2: a spec with `swarm_id: None` (the default) must not set a swarm link.
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Spawn task".into()), owner)
        .unwrap();
    let spec = SpawnSpec::new(task.id);
    svc.spawn_task(task.id, spec, owner).unwrap();
    let reloaded = svc.task_get(task.id).unwrap().expect("task should persist");
    assert!(
        reloaded.swarm_id.is_none(),
        "spawn_task with no swarm_id must leave Task.swarm_id as None"
    );
}

#[test]
fn board_delete_removes_board_and_tasks() {
    let store = make_store();
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service
        .board_create(owner, "Board", &make_default_columns())
        .expect("board");
    service
        .task_create(board.id, TaskSpec::new("T1".into()), owner)
        .expect("first task");
    service
        .task_create(board.id, TaskSpec::new("T2".into()), owner)
        .expect("second task");
    assert_eq!(store.count().expect("row count"), 5);

    let tasks_deleted = service.board_delete(board.id).expect("delete board");
    assert_eq!(tasks_deleted, 2);
    assert!(service.board_get(board.id).expect("query board").is_none());
    assert!(service.board_list(&owner).expect("board list").is_empty());
    assert_eq!(
        store.count().expect("row count"),
        0,
        "board, task and index rows must be deleted"
    );
}

#[test]
fn task_delete_removes_task_and_index_rows_but_preserves_board() {
    let store = make_store();
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service
        .board_create(owner, "Board", &make_default_columns())
        .expect("board");
    let task = service
        .task_create(board.id, TaskSpec::new("Task".into()), owner)
        .expect("task");
    assert_eq!(store.count().expect("row count"), 3);

    service.task_delete(task.id).expect("delete task");
    assert!(service.task_get(task.id).expect("query task").is_none());
    assert!(
        service
            .task_list(board.id, TaskFilter::all())
            .expect("task list")
            .is_empty()
    );
    assert!(service.board_get(board.id).expect("query board").is_some());
    assert_eq!(
        store.count().expect("row count"),
        1,
        "only the board row may remain"
    );
}

/// expect: "Deleting a task removes its payload and board index together or removes neither."
/// [P3] Motivating: Generative Space — board membership never points at missing work.
/// [P2] Constraining: Transparent Imperfection — a failed delete preserves visible prior state.
/// pre: one task exists and deletion of its index row is forced to fail
/// post: task deletion fails while task payload, board index, and board all remain
#[test]
fn task_delete_atomic_when_index_delete_fails() -> anyhow::Result<()> {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver.clone())?;
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service.board_create(owner, "Board", &make_default_columns())?;
    let task = service.task_create(board.id, TaskSpec::new("Task".into()), owner)?;
    driver.execute_batch(
        "CREATE TRIGGER reject_task_index_delete BEFORE DELETE ON hmems
         WHEN OLD.entity LIKE 'kanban:board_tasks:%'
         BEGIN SELECT RAISE(FAIL, 'forced task index delete failure'); END;",
    )?;

    assert!(service.task_delete(task.id).is_err());
    assert!(service.task_get(task.id)?.is_some());
    assert_eq!(service.task_list(board.id, TaskFilter::all())?.len(), 1);
    assert!(service.board_get(board.id)?.is_some());
    assert_eq!(store.count()?, 3);
    Ok(())
}

/// expect: "Deleting a board removes the board and every child row together or removes nothing."
/// [P3] Motivating: Generative Space — board lifecycle owns its complete task aggregate.
/// [P2] Constraining: Transparent Imperfection — a child failure cannot orphan or hide work.
/// pre: one board with one task exists and deletion of the task payload is forced to fail
/// post: board deletion fails while board, task payload, and index remain readable
#[test]
fn board_delete_atomic_when_child_delete_fails() -> anyhow::Result<()> {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver.clone())?;
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service.board_create(owner, "Board", &make_default_columns())?;
    let task = service.task_create(board.id, TaskSpec::new("Task".into()), owner)?;
    driver.execute_batch(
        "CREATE TRIGGER reject_board_child_delete BEFORE DELETE ON hmems
         WHEN OLD.entity = 'kanban:task'
         BEGIN SELECT RAISE(FAIL, 'forced board child delete failure'); END;",
    )?;

    assert!(service.board_delete(board.id).is_err());
    assert!(service.board_get(board.id)?.is_some());
    assert!(service.task_get(task.id)?.is_some());
    assert_eq!(service.task_list(board.id, TaskFilter::all())?.len(), 1);
    assert_eq!(store.count()?, 3);
    Ok(())
}

// ── Mermaid export/import round-trip integration tests ───────────────────
//
// These tests exercise the full round-trip through the kanban service layer:
// create a board → add tasks → export to mermaid markdown → parse the
// markdown → import as a new board → verify the new board matches the
// original's structure (columns, task titles, task order). They complement
// the unit tests in `mermaid.rs`, which cover the export/parse functions in
// isolation; these verify the service-layer wiring end-to-end.

/// Build a board with three custom columns (Backlog, In Progress, Done) and
/// populate it with tasks in each column, returning the service, board, and
/// owner for the test to drive.
fn make_board_with_tasks_for_round_trip() -> (KanbanService, Board, WebID) {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = vec![
        ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
        ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 1),
        ColumnDef::new("Done".into(), TaskStatus::Done, 2),
    ];
    let board = svc
        .board_create(owner, "Round Trip Board", &columns)
        .expect("board create");

    // Backlog tasks (created in Backlog by default).
    svc.task_create(board.id, TaskSpec::new("Backlog A".into()), owner)
        .expect("task Backlog A");
    svc.task_create(board.id, TaskSpec::new("Backlog B".into()), owner)
        .expect("task Backlog B");

    // In Progress task — move to the next configured column.
    let in_prog = svc
        .task_create(board.id, TaskSpec::new("In Progress Task".into()), owner)
        .expect("task In Progress");
    svc.task_move(in_prog.id, TaskStatus::InProgress, owner)
        .expect("move to InProgress");

    // Done task — walk through the board's configured columns.
    let done = svc
        .task_create(board.id, TaskSpec::new("Done Task".into()), owner)
        .expect("task Done");
    svc.task_move(done.id, TaskStatus::InProgress, owner)
        .expect("move to InProgress");
    svc.task_move(done.id, TaskStatus::Done, owner)
        .expect("move to Done");

    (svc, board, owner)
}

#[test]
fn export_import_round_trip_preserves_board_structure() {
    // Create a board with 3 columns and tasks in each column, export to
    // mermaid markdown, parse it, import as a new board, and verify the new
    // board has the same column names and task titles in the same order.
    let (svc, board, owner) = make_board_with_tasks_for_round_trip();

    // Export: pull tasks through the service, render to mermaid markdown.
    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list");
    let markdown = export_board_to_mermaid(&board, &tasks);

    // Parse the exported markdown.
    let parsed = parse_mermaid_kanban(&markdown).expect("parse exported markdown");

    let new_columns = columns_from_parsed(&parsed).expect("parsed columns map uniquely");
    let imported_tasks = parsed
        .columns
        .iter()
        .zip(&new_columns)
        .flat_map(|(column, definition)| {
            column
                .tasks
                .iter()
                .cloned()
                .map(move |title| (TaskSpec::new(title), definition.status))
        })
        .collect();
    let (new_board, imported_count) = svc
        .board_import(owner, "Imported Board", &new_columns, imported_tasks)
        .expect("atomic board import");
    assert_eq!(imported_count, tasks.len());

    // Verify the new board's columns match the original's names and order.
    let original_names: Vec<&str> = board.columns.iter().map(|c| c.name.as_str()).collect();
    let new_names: Vec<&str> = new_board.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        new_names, original_names,
        "imported board columns should match original names and order"
    );

    // Verify task titles per column match, in order. Group the new board's
    // tasks by status and compare the title lists against the parsed markdown.
    let new_tasks = svc
        .task_list(new_board.id, TaskFilter::all())
        .expect("new board task list");
    for column in &parsed.columns {
        let target_status = new_board
            .columns
            .iter()
            .find(|c| c.name == column.name)
            .expect("column exists")
            .status;
        let mut new_titles: Vec<String> = new_tasks
            .iter()
            .filter(|t| t.status == target_status)
            .map(|t| t.title.clone())
            .collect();
        // task_list sorts by created_at descending; reverse to get creation
        // order so the comparison matches the parsed markdown's source order.
        new_titles.reverse();
        assert_eq!(
            new_titles, column.tasks,
            "task titles in column '{}' should match after round-trip",
            column.name
        );
    }
}

#[test]
fn export_parse_round_trip_handles_special_characters() {
    // Tasks with quotes, brackets, unicode, and backslashes in their titles
    // must survive the export → parse round-trip unchanged.
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = vec![ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0)];
    let board = svc
        .board_create(owner, "Special Chars Board", &columns)
        .expect("board create");

    let special_titles = vec![
        "Task with \"quotes\"",
        "Task with [brackets]",
        "Task with unicode: café ☕",
        "Task with backslash \\",
    ];
    for title in &special_titles {
        svc.task_create(board.id, TaskSpec::new((*title).to_string()), owner)
            .expect("task create");
    }

    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list");
    let markdown = export_board_to_mermaid(&board, &tasks);
    let parsed = parse_mermaid_kanban(&markdown).expect("parse");

    // The single Backlog column should carry all four titles, in order.
    assert_eq!(parsed.columns.len(), 1);
    assert_eq!(parsed.columns[0].name, "Backlog");
    // task_list sorts by created_at descending; reverse to match creation order.
    let mut expected = special_titles.to_vec();
    expected.reverse();
    let mut actual: Vec<String> = parsed.columns[0].tasks.clone();
    actual.reverse();
    assert_eq!(actual, expected);
}

#[test]
fn export_import_round_trip_preserves_column_order() {
    // A board with columns in a specific, non-standard order must preserve
    // that order through the export → parse → import round-trip.
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    // Deliberately non-standard order: Done first, Backlog last.
    let columns = vec![
        ColumnDef::new("Done".into(), TaskStatus::Done, 0),
        ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 1),
        ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 2),
    ];
    let board = svc
        .board_create(owner, "Ordered Board", &columns)
        .expect("board create");

    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list");
    let markdown = export_board_to_mermaid(&board, &tasks);
    let parsed = parse_mermaid_kanban(&markdown).expect("parse");

    let original_order: Vec<&str> = board.columns.iter().map(|c| c.name.as_str()).collect();
    let parsed_order: Vec<&str> = parsed.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        parsed_order, original_order,
        "parsed column order should match the original board's column order"
    );

    // Import as a new board and verify the new board's columns are in the
    // same order.
    let new_columns = columns_from_parsed(&parsed).expect("parsed columns map uniquely");
    let (new_board, imported_count) = svc
        .board_import(owner, "Re-imported Ordered Board", &new_columns, Vec::new())
        .expect("atomic board import");
    assert_eq!(imported_count, 0);
    let new_order: Vec<&str> = new_board.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        new_order, original_order,
        "imported board column order should match the original"
    );
}

#[test]
fn import_rejects_invalid_markdown() {
    // Markdown without the `kanban` directive must return an error, not panic.
    let invalid = "```mermaid\n  section Backlog\n    Task\n```";
    let result = parse_mermaid_kanban(invalid);
    assert!(
        result.is_err(),
        "markdown missing the `kanban` directive should be rejected"
    );
    let err = result.expect_err("expected error");
    assert!(
        err.to_string().contains("kanban"),
        "error message should reference the missing `kanban` directive, got: {err}"
    );
}

#[test]
fn import_empty_board() {
    // Markdown with the `kanban` directive and one `section` but no tasks
    // must parse successfully, yielding one column with zero tasks. The
    // resulting columns can then be used to create an empty board.
    let md = "```mermaid\nkanban\n  section Backlog\n```";
    let parsed = parse_mermaid_kanban(md).expect("parse empty board");
    assert_eq!(parsed.columns.len(), 1, "should have one column");
    assert_eq!(parsed.columns[0].name, "Backlog");
    assert!(
        parsed.columns[0].tasks.is_empty(),
        "empty board should have no tasks"
    );

    // Verify the parsed columns drive the canonical aggregate import.
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = columns_from_parsed(&parsed).expect("parsed columns map uniquely");
    let (board, imported_count) = svc
        .board_import(owner, "Empty Imported Board", &columns, Vec::new())
        .expect("empty board import");
    assert_eq!(imported_count, 0);
    assert_eq!(board.columns.len(), 1);
    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list");
    assert!(
        tasks.is_empty(),
        "imported empty board should have no tasks"
    );
}
