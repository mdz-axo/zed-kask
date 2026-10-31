use super::service::KanbanService;
use crate::VerificationCriterion;
use crate::kanban::mermaid::{export_board_to_mermaid, parse_mermaid_kanban};
use crate::kanban::types::goal::{GoalTrustEvent, GoalTrustSeverity, GoalTrustStatus};
use crate::kanban::{
    Board, ColumnDef, CriterionCitation, KataExperimentRecord, SpawnSpec, Task, TaskFilter,
    TaskSpec, TaskStatus,
};
use hkask_storage::HMemStore;
use hkask_types::WebID;
use hkask_types::id::BoardId;
use hkask_types::kanban_wire::{ALGEDONIC_BOARD_NAME, KANBAN_BOARD_NAME_MAX_CHARS};

use super::kata::KataStateUpdate;
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

/// A service over a three-column board (Backlog, In Progress, Done) — the
/// custom-column arrangement the column-order and round-trip tests share.
/// The board name is the caller's own (each test's fixture name); the
/// columns are the shared intent.
fn make_service_with_three_column_board(name: &str) -> (KanbanService, Board, WebID) {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = vec![
        ColumnDef::new("Backlog".into(), TaskStatus::Backlog, 0),
        ColumnDef::new("In Progress".into(), TaskStatus::InProgress, 1),
        ColumnDef::new("Done".into(), TaskStatus::Done, 2),
    ];
    let board = svc
        .board_create(owner, name, &columns)
        .expect("board create");
    (svc, board, owner)
}

/// The owner-isolation body shared by `board_list_by_owner` and
/// `board_isolation` — two named intents (whose list, and isolation of
/// owners) carried by the test names, one body: two owners' boards, and
/// each owner's list shows only their own.
fn assert_boards_isolated_by_owner() {
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

/// The fault-injection test environment: the in-memory driver (the
/// trigger-injection site), its store (the row-count/query oracle), a
/// service over a store clone, an owner, and a board — the arrangement the
/// four atomicity tests share. The task each test creates and the injected
/// SQL stay per-test: they are the pre-condition each test varies.
struct FaultEnv {
    driver: std::sync::Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
    store: HMemStore,
    service: KanbanService,
    owner: WebID,
    board: Board,
}

/// The shared post-condition of the board-delete failure tests: the failed
/// delete leaves the board, its task, and the board index all readable, and
/// no rows were removed. The two board-delete fault tests assert this tail
/// identically (their pre-conditions differ only in the injected SQL);
/// `task_delete_atomic` keeps its own tail (its arrangement differs).
fn assert_board_and_task_retained(env: &FaultEnv, task: &Task) -> anyhow::Result<()> {
    assert!(env.service.board_get(env.board.id)?.is_some());
    assert!(env.service.task_get(task.id)?.is_some());
    assert_eq!(
        env.service
            .task_list(env.board.id, TaskFilter::all())?
            .tasks
            .len(),
        1
    );
    assert_eq!(env.store.count()?, 3);
    Ok(())
}

fn fault_env() -> anyhow::Result<FaultEnv> {
    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver.clone())?;
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service.board_create(owner, "Board", &make_default_columns())?;
    Ok(FaultEnv {
        driver,
        store,
        service,
        owner,
        board,
    })
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

/// A whitespace-only name is rejected after trimming.
#[test]
fn board_create_rejects_whitespace_only_name() {
    let svc = KanbanService::new(make_store());
    assert!(
        svc.board_create(WebID::new(), "   \t ", &make_default_columns())
            .is_err()
    );
}

/// A padded name is stored trimmed — the stored name is the identity
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

/// Rename round-trips — the new name reaches `board_list`, and the
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

/// A name that is empty after trimming is rejected and the original
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

/// Renaming an unknown board is NotFound.
#[test]
fn board_rename_unknown_board_is_not_found() {
    let svc = KanbanService::new(make_store());
    assert!(svc.board_rename(BoardId::new(), "New Name").is_err());
}

/// Rename is convergent — renaming to the name the board already has
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
    assert_boards_isolated_by_owner();
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

/// Stable row identity prevents the earlier deleted-row write failure;
/// content preservation is a separate obligation tested below.
#[test]
fn task_comment_preserves_the_task_row_h_mem_id() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Stable".into()), owner)
        .unwrap();
    let before = svc
        .store
        .query_by_entity_attribute("kanban:task", &task.id.to_string())
        .unwrap()
        .into_iter()
        .next()
        .expect("the task row exists")
        .id;
    svc.task_comment(task.id, owner, "a write")
        .expect("comment lands");
    let after = svc
        .store
        .query_by_entity_attribute("kanban:task", &task.id.to_string())
        .unwrap()
        .into_iter()
        .next()
        .expect("the task row exists after the update")
        .id;
    assert_eq!(
        before, after,
        "the task row's h_mem ID must be stable across updates — a re-keying write leaves concurrent read-then-writers targeting a deleted row ('Query returned no rows')"
    );
    let stored = svc.task_get(task.id).unwrap().expect("task reads back");
    assert_eq!(
        stored.comments.last().map(|comment| comment.body.as_str()),
        Some("a write"),
        "the written value lands"
    );
}

/// Exercise real WAL contention as well as the deterministic schedules below:
/// successful comment appends must all survive, not just keep a valid row.
#[test]
fn concurrent_comments_preserve_all_successful_appends() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("concurrent-task-writes.sqlite");
    let pool = hkask_storage::SqliteDriver::file_pool_with_schema(
        path.to_str().expect("utf-8 temp path"),
    )?;
    let store =
        HMemStore::from_driver(std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool)))?;
    let svc = KanbanService::new(store);
    let owner = WebID::new();
    let board = svc.board_create(owner, "Race Board", &make_default_columns())?;
    let task = svc.task_create(board.id, TaskSpec::new("Race".into()), owner)?;

    const WRITERS: usize = 8;
    const ROUNDS: usize = 25;
    let failures: Vec<String> = std::thread::scope(|scope| {
        let svc = &svc;
        let task = &task;
        let mut handles = Vec::new();
        for writer in 0..WRITERS {
            handles.push(scope.spawn(move || {
                let mut failures = Vec::new();
                for round in 0..ROUNDS {
                    if let Err(error) =
                        svc.task_comment(task.id, owner, &format!("writer {writer} round {round}"))
                    {
                        failures.push(format!("update {writer}/{round}: {error}"));
                    }
                }
                failures
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("writer thread panicked"))
            .collect()
    });
    assert!(
        failures.is_empty(),
        "concurrent task writes must not fail under WAL contention: {failures:#?}"
    );
    // Every acknowledged append survives with its own identity.
    let stored = svc.task_get(task.id)?.expect("the task row survives");
    assert_eq!(stored.id, task.id);
    assert_eq!(
        stored.comments.len(),
        WRITERS * ROUNDS,
        "every successful comment must survive concurrent writes"
    );
    for writer in 0..WRITERS {
        for round in 0..ROUNDS {
            let body = format!("writer {writer} round {round}");
            assert_eq!(
                stored
                    .comments
                    .iter()
                    .filter(|comment| comment.body == body)
                    .count(),
                1,
                "exactly one acknowledged append: {body}"
            );
        }
    }
    Ok(())
}

/// A WIP-limited column admits a move below its limit and refuses the move
/// that would exceed it — the first server-side pin of the enforcement.
#[test]
fn wip_limit_blocks_move_into_full_column() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let mut columns = KanbanService::standard_columns();
    columns[1] = ColumnDef::new("Ready".into(), TaskStatus::Ready, 1).with_wip_limit(1);
    let board = svc
        .board_create(owner, "WIP Board", &columns)
        .expect("board create");
    let first = svc
        .task_create(board.id, TaskSpec::new("First".into()), owner)
        .expect("first task");
    let second = svc
        .task_create(board.id, TaskSpec::new("Second".into()), owner)
        .expect("second task");

    svc.task_move(first.id, TaskStatus::Ready, owner)
        .expect("count 0 admits below a limit of 1");
    let refused = svc.task_move(second.id, TaskStatus::Ready, owner);
    assert!(
        matches!(refused, Err(KanbanError::WipLimitExceeded { ref column, limit: 1, current: 1 }) if column == "Ready"),
        "the move that would exceed the limit is refused with the column named: {refused:?}"
    );
}

/// WIP admission is atomic under concurrent moves: two tasks race into a
/// column limited to one, over the file-backed WAL pool (real
/// multi-connection semantics — the in-memory max_size(1) driver serializes
/// at the pool and would hide the window). The count and the commit must
/// share one transaction: counting before the write transaction lets both
/// movers read the same count and both be admitted into a full column.
#[test]
fn concurrent_moves_into_a_wip_one_column_admit_exactly_one() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("wip-race.sqlite");
    let pool = hkask_storage::SqliteDriver::file_pool_with_schema(
        path.to_str().expect("utf-8 temp path"),
    )?;
    let store =
        HMemStore::from_driver(std::sync::Arc::new(hkask_storage::SqliteDriver::new(pool)))?;
    let svc = KanbanService::new(store);
    let owner = WebID::new();
    let mut columns = KanbanService::standard_columns();
    columns[1] = ColumnDef::new("Ready".into(), TaskStatus::Ready, 1).with_wip_limit(1);
    let board = svc.board_create(owner, "WIP Race", &columns)?;
    let first = svc.task_create(board.id, TaskSpec::new("First".into()), owner)?;
    let second = svc.task_create(board.id, TaskSpec::new("Second".into()), owner)?;

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_mover = {
        let svc = svc.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            svc.task_move(first.id, TaskStatus::Ready, owner)
        })
    };
    let second_mover = {
        std::thread::spawn(move || {
            barrier.wait();
            svc.task_move(second.id, TaskStatus::Ready, owner)
        })
    };
    let first_result = first_mover.join().expect("first mover thread");
    let second_result = second_mover.join().expect("second mover thread");

    let admitted = [&first_result, &second_result]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(
        admitted, 1,
        "exactly one mover may enter a WIP-1 column: first={first_result:?}, second={second_result:?}"
    );
    let refused = if first_result.is_ok() {
        &second_result
    } else {
        &first_result
    };
    assert!(
        matches!(refused, Err(KanbanError::WipLimitExceeded { column, limit: 1, current: 1 }) if column == "Ready"),
        "the losing mover is refused by the WIP limit: {refused:?}"
    );
    Ok(())
}

/// An indexed task row that fails to decode is surfaced by the listing —
/// never silently skipped. The row is named and the reason stated, so a
/// corrupt row is visible instead of shrinking the board's task set.
#[test]
fn task_list_surfaces_undecodable_task_rows() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Doomed".into()), owner)?;
    // Corrupt the task row directly: a value the current decode cannot
    // parse (schema drift, a torn write) — the index entry stays intact.
    svc.store
        .update_value_atomic::<_, hkask_storage::HMemError>(
            "kanban:task",
            &task.id.to_string(),
            |_| Ok((Some(serde_json::json!({"corrupted": true})), ())),
        )?
        .expect("the corrupted row still exists");

    let report = svc.task_list(board.id, TaskFilter::all())?;
    assert!(
        report.tasks.is_empty(),
        "the corrupted row is not listed as a readable task"
    );
    assert_eq!(report.undecodable.len(), 1);
    assert_eq!(report.undecodable[0].task_id, task.id.to_string());
    assert!(
        report.undecodable[0].reason.contains("failed to decode"),
        "the fault names the decode failure: {}",
        report.undecodable[0].reason
    );
    Ok(())
}

/// The WIP admission fails closed on an unreadable sibling row: the count
/// would be a lower bound (an unreadable row can only undercount), and
/// admitting against a lower bound over-admits. The refusal names the row.
#[test]
fn wip_count_fails_closed_on_undecodable_sibling() -> anyhow::Result<()> {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let mut columns = KanbanService::standard_columns();
    columns[1] = ColumnDef::new("Ready".into(), TaskStatus::Ready, 1).with_wip_limit(1);
    let board = svc.board_create(owner, "WIP Closed Board", &columns)?;
    let corrupted = svc.task_create(board.id, TaskSpec::new("Corrupted".into()), owner)?;
    let mover = svc.task_create(board.id, TaskSpec::new("Mover".into()), owner)?;
    svc.store
        .update_value_atomic::<_, hkask_storage::HMemError>(
            "kanban:task",
            &corrupted.id.to_string(),
            |_| Ok((Some(serde_json::json!({"corrupted": true})), ())),
        )?
        .expect("the corrupted row still exists");

    let refused = svc.task_move(mover.id, TaskStatus::Ready, owner);
    assert!(
        matches!(&refused, Err(KanbanError::Internal(message)) if message.contains("WIP count unreliable")
            && message.contains(&corrupted.id.to_string())
            && message.contains("failed to decode")),
        "the move is refused naming the unreadable row: {refused:?}"
    );
    assert_eq!(
        svc.task_get(mover.id)?.expect("mover survives").status,
        TaskStatus::Backlog,
        "the refused move leaves the task untouched"
    );
    Ok(())
}

/// Recording kata state round-trips durably, and the kata prompts read it
/// back — a new session's prompt reflects the recorded target condition,
/// focus obstacle, and last experiment instead of restarting from zero
/// (the pre-fix tree had no durable kata state: prompts showed only status,
/// assignee, estimates, deliverables, and recent comments).
#[test]
fn kata_record_round_trips_and_prompts_reflect_it() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Kata task".into()), owner)?;
    svc.task_kata_record(
        task.id,
        owner,
        KataStateUpdate {
            target_condition: Some("95% pass rate within 3 experiments".into()),
            focus_obstacle: Some(Some("flaky fixture ordering".into())),
            last_experiment: Some(KataExperimentRecord {
                next_step: "Run the suite with --test-threads=1".into(),
                prediction: "the flake disappears (test-order coupling)".into(),
                outcome: Some("prediction held: green 3x in a row".into()),
            }),
        },
    )?;

    let stored = svc.task_get(task.id)?.expect("task survives");
    let state = stored.kata_state.as_ref().expect("kata state persisted");
    assert_eq!(state.target_condition, "95% pass rate within 3 experiments");
    assert_eq!(
        state.focus_obstacle.as_deref(),
        Some("flaky fixture ordering")
    );
    let experiment = state
        .last_experiment
        .as_ref()
        .expect("experiment persisted");
    assert_eq!(experiment.next_step, "Run the suite with --test-threads=1");

    // The prompt reflects the recorded state — the continuity contract a
    // fresh session relies on.
    let prompt = svc.task_coaching_prompt(task.id)?;
    assert!(
        prompt.contains("95% pass rate within 3 experiments"),
        "the prompt carries the target condition: {prompt}"
    );
    assert!(
        prompt.contains("flaky fixture ordering"),
        "the prompt carries the focus obstacle: {prompt}"
    );
    assert!(
        prompt.contains("--test-threads=1"),
        "the prompt carries the last experiment: {prompt}"
    );
    assert!(
        prompt.contains("prediction held"),
        "the prompt carries the observed outcome: {prompt}"
    );
    Ok(())
}

/// The first kata record on a task requires the target condition — the
/// kata's step-3 artifact cannot be skipped.
#[test]
fn kata_record_requires_target_condition_to_start() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Kata task".into()), owner)?;
    let refused = svc.task_kata_record(
        task.id,
        owner,
        KataStateUpdate {
            target_condition: None,
            focus_obstacle: Some(Some("premature".into())),
            last_experiment: None,
        },
    );
    assert!(
        matches!(&refused, Err(KanbanError::InvalidInput(message)) if message.contains("target_condition")),
        "an obstacle cannot be recorded before any target exists: {refused:?}"
    );
    Ok(())
}

/// Kata recording is the learner's write: owner or assignee. A stranger is
/// refused.
#[test]
fn kata_record_requires_owner_or_assignee() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Kata task".into()), owner)?;
    let refused = svc.task_kata_record(
        task.id,
        WebID::new(),
        KataStateUpdate {
            target_condition: Some("target".into()),
            focus_obstacle: None,
            last_experiment: None,
        },
    );
    assert!(
        matches!(&refused, Err(KanbanError::PermissionDenied(_))),
        "a stranger cannot record kata state: {refused:?}"
    );
    Ok(())
}

/// The focus obstacle can be cleared without touching the rest of the kata
/// state (Some(None) clears; None keeps — the task_update precedent).
#[test]
fn kata_record_clears_the_focus_obstacle() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Kata task".into()), owner)?;
    svc.task_kata_record(
        task.id,
        owner,
        KataStateUpdate {
            target_condition: Some("target".into()),
            focus_obstacle: Some(Some("first obstacle".into())),
            last_experiment: None,
        },
    )?;
    svc.task_kata_record(
        task.id,
        owner,
        KataStateUpdate {
            target_condition: None,
            focus_obstacle: Some(None),
            last_experiment: None,
        },
    )?;
    let state = svc
        .task_get(task.id)?
        .expect("task")
        .kata_state
        .expect("state kept");
    assert_eq!(state.target_condition, "target");
    assert!(
        state.focus_obstacle.is_none(),
        "the obstacle is cleared, the target kept"
    );
    Ok(())
}

/// The Done-gate policy is structural: a board created with the flag
/// refuses ordinary moves to Done, while `task_verify` still completes.
#[test]
fn done_gate_flag_refuses_direct_moves_to_done() -> anyhow::Result<()> {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board =
        svc.board_create_with_policy(owner, "Gated Board", &make_default_columns(), Some(true))?;
    let task = svc.task_create(board.id, TaskSpec::new("Gate".into()), owner)?;
    for status in [
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
    ] {
        svc.task_move(task.id, status, owner)?;
    }
    let refused = svc.task_move(task.id, TaskStatus::Done, owner);
    assert!(
        matches!(&refused, Err(KanbanError::InvalidTransition { to, .. }) if *to == TaskStatus::Done),
        "an ordinary move to Done is refused on a gated board: {refused:?}"
    );
    let verified = svc.task_verify(task.id, "user confirmed", owner)?;
    assert!(
        verified
            .0
            .verification
            .as_ref()
            .expect("verification recorded")
            .passed
    );
    assert_eq!(verified.0.status, TaskStatus::Done);
    Ok(())
}

/// A plain board without the flag allows the ordinary path to Done — the
/// gate is opt-in (or reserved-name), never global.
#[test]
fn plain_board_allows_ordinary_done_moves() -> anyhow::Result<()> {
    let (svc, board, owner) = make_service_with_board();
    let task = svc.task_create(board.id, TaskSpec::new("Plain".into()), owner)?;
    for status in [
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
        TaskStatus::Done,
    ] {
        svc.task_move(task.id, status, owner)?;
    }
    assert_eq!(
        svc.task_get(task.id)?.expect("task").status,
        TaskStatus::Done
    );
    Ok(())
}

/// The reserved name stamps the flag at creation — structural, so renaming
/// the board keeps the gate (the pre-flag name-keyed check moved with the
/// name instead).
#[test]
fn renamed_algedonic_board_keeps_the_done_gate() -> anyhow::Result<()> {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc.board_create(owner, ALGEDONIC_BOARD_NAME, &make_default_columns())?;
    assert!(
        board.done_requires_verification,
        "the reserved name stamps the structural flag at creation"
    );
    svc.board_rename(board.id, "Renamed Review")?;
    let task = svc.task_create(board.id, TaskSpec::new("Gate".into()), owner)?;
    for status in [
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
    ] {
        svc.task_move(task.id, status, owner)?;
    }
    let refused = svc.task_move(task.id, TaskStatus::Done, owner);
    assert!(
        matches!(&refused, Err(KanbanError::InvalidTransition { to, .. }) if *to == TaskStatus::Done),
        "the gate survives the rename because it lives on the row: {refused:?}"
    );
    Ok(())
}

/// Legacy migration: a board created before the flag existed carries the
/// reserved name without the flag. The first move evaluation stamps the
/// flag persistently — the name check retires, the gate stays.
#[test]
fn legacy_board_gets_the_done_gate_stamped_at_first_move() -> anyhow::Result<()> {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc.board_create(owner, "Legacy", &make_default_columns())?;
    // Simulate the pre-flag row: the reserved name without the persisted
    // policy — the state the operator's existing board carries.
    let mut legacy = serde_json::to_value(&board).expect("board serializes");
    legacy["name"] = serde_json::json!(ALGEDONIC_BOARD_NAME);
    legacy["done_requires_verification"] = serde_json::json!(false);
    svc.store
        .update_value_atomic::<_, hkask_storage::HMemError>(
            "kanban:board",
            &board.id.to_string(),
            |_| Ok((Some(legacy), ())),
        )?
        .expect("board row exists");

    let task = svc.task_create(board.id, TaskSpec::new("Migrate".into()), owner)?;
    for status in [
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::Review,
    ] {
        svc.task_move(task.id, status, owner)?;
    }
    let reloaded_after_moves = svc.board_get(board.id)?.expect("board reloaded");
    assert!(
        reloaded_after_moves.done_requires_verification,
        "the first move migrated the flag onto the row"
    );
    let refused = svc.task_move(task.id, TaskStatus::Done, owner);
    assert!(
        matches!(&refused, Err(KanbanError::InvalidTransition { to, .. }) if *to == TaskStatus::Done),
        "the migrated gate refuses the ordinary Done move: {refused:?}"
    );
    Ok(())
}

/// Column WIP limits survive the export → parse → import round-trip — the
/// one flow-control mechanism a board carries (the pre-fix format dropped
/// it silently: `columns_from_parsed` rebuilt every column without a
/// limit).
#[test]
fn export_import_round_trip_preserves_wip_limits() -> anyhow::Result<()> {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let mut columns = KanbanService::standard_columns();
    columns[1] = ColumnDef::new("Ready".into(), TaskStatus::Ready, 1).with_wip_limit(2);
    let board = svc.board_create(owner, "WIP Round Trip", &columns)?;
    let report = svc.task_list(board.id, TaskFilter::all())?;
    let markdown = export_board_to_mermaid(&board, &report.tasks);
    let parsed = parse_mermaid_kanban(&markdown)?;
    let (imported, _) = svc.board_import(owner, "WIP Round Trip 2", &parsed)?;
    let ready = imported
        .columns
        .iter()
        .find(|column| column.status == TaskStatus::Ready)
        .expect("Ready column imported");
    assert_eq!(
        ready.wip_limit,
        Some(2),
        "the WIP limit survives the round-trip"
    );
    Ok(())
}

/// Pause exactly one operation before its write, commit a competing operation,
/// then release it. On the old path the paused operation has read a stale Task;
/// on the repaired path its transformation receives the competing commit.
fn interleave_task_writes<T: Send>(
    svc: &KanbanService,
    paused: impl FnOnce() -> T + Send,
    competing: impl FnOnce(),
) -> T {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let timeout = std::time::Duration::from_secs(10);
    *svc.before_task_write.lock().expect("scheduling hook lock") = Some(Box::new(move || {
        entered_tx.send(()).expect("announce write boundary");
        release_rx
            .recv_timeout(timeout)
            .expect("release paused writer");
    }));
    std::thread::scope(|scope| {
        let writer = scope.spawn(paused);
        entered_rx
            .recv_timeout(timeout)
            .expect("writer reached boundary");
        competing();
        release_tx.send(()).expect("release writer");
        writer.join().expect("writer thread completed")
    })
}

#[test]
fn interleaved_comments_moves_and_deliverables_preserve_each_other() {
    // Both write orders for comment/move and comment/deliverable, plus two
    // independent appends of each kind. Each case uses the real service API.
    for pair in [
        (0, 1),
        (1, 0),
        (0, 2),
        (2, 0),
        (0, 0),
        (2, 2),
        (1, 2),
        (2, 1),
    ] {
        let (svc, board, owner) = make_service_with_board();
        let task = svc
            .task_create(board.id, TaskSpec::new("Unchanged title".into()), owner)
            .expect("task create");
        let apply = |operation, body: &str| match operation {
            0 => svc.task_comment(task.id, owner, body).map(|_| ()),
            1 => svc.task_move(task.id, TaskStatus::Ready, owner).map(|_| ()),
            2 => svc.task_add_deliverable(task.id, body, owner).map(|_| ()),
            _ => unreachable!("fixture operation"),
        };
        interleave_task_writes(
            &svc,
            || apply(pair.0, "paused"),
            || {
                apply(pair.1, "competing").expect("competing operation");
            },
        )
        .expect("paused operation");
        let stored = svc
            .task_get(task.id)
            .expect("task read")
            .expect("task survives");
        let bodies: Vec<_> = stored.comments.iter().map(|c| c.body.as_str()).collect();
        let expected_comments: Vec<_> = [(pair.1, "competing"), (pair.0, "paused")]
            .into_iter()
            .filter_map(|(op, body)| (op == 0).then_some(body))
            .collect();
        let expected_deliverables: Vec<_> = [(pair.1, "competing"), (pair.0, "paused")]
            .into_iter()
            .filter_map(|(op, body)| (op == 2).then_some(body))
            .collect();
        assert_eq!(bodies, expected_comments, "comment preservation: {pair:?}");
        assert_eq!(
            stored.deliverables, expected_deliverables,
            "deliverables: {pair:?}"
        );
        assert_eq!(
            stored.status,
            if pair.0 == 1 || pair.1 == 1 {
                TaskStatus::Ready
            } else {
                TaskStatus::Backlog
            },
            "status: {pair:?}"
        );

        let mut expected = task.clone();
        expected.status = stored.status;
        expected.comments = stored.comments.clone();
        expected.deliverables = stored.deliverables.clone();
        expected.updated_at = stored.updated_at;
        assert_eq!(stored, expected, "all unrelated fields: {pair:?}");
        assert_eq!(
            svc.task_list(board.id, TaskFilter::default())
                .expect("indexed read")
                .tasks
                .len(),
            1
        );
    }
}

#[test]
fn remaining_task_mutations_preserve_interleaved_comments_in_both_orders() {
    for operation in ["claim", "unassign", "update", "verify", "reopen", "spawn"] {
        for comment_first in [false, true] {
            let (svc, board, owner) = make_service_with_board();
            let spec = TaskSpec::new("Original".into())
                .with_description("Keep description".into())
                .with_criteria(vec![VerificationCriterion::new("Keep criterion".into())]);
            let task = svc.task_create(board.id, spec, owner).expect("task create");
            if operation == "unassign" {
                svc.task_claim(task.id, owner).expect("claim fixture");
            }
            if matches!(operation, "verify" | "reopen") {
                for status in [
                    TaskStatus::Ready,
                    TaskStatus::InProgress,
                    TaskStatus::Review,
                ] {
                    svc.task_move(task.id, status, owner)
                        .expect("review fixture");
                }
            }
            if operation == "reopen" {
                svc.task_verify(task.id, "Confirmed", owner)
                    .expect("done fixture");
            }
            let before = svc.require_task(task.id).expect("fixture read");
            let mutate = || match operation {
                "claim" => svc.task_claim(task.id, owner).map(|_| ()),
                "unassign" => svc.task_unassign(task.id, owner).map(|_| ()),
                "update" => svc
                    .task_update(
                        task.id,
                        owner,
                        Some("Edited".into()),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .map(|_| ()),
                "verify" => svc.task_verify(task.id, "Confirmed", owner).map(|_| ()),
                "reopen" => svc.task_reopen(task.id, owner).map(|_| ()),
                "spawn" => svc
                    .spawn_task(task.id, SpawnSpec::new(task.id), owner)
                    .map(|_| ()),
                _ => unreachable!("fixture operation"),
            };
            let comment = || svc.task_comment(task.id, owner, "Keep comment").map(|_| ());
            if comment_first {
                interleave_task_writes(&svc, mutate, || {
                    comment().expect("competing comment");
                })
                .expect("paused mutation");
            } else {
                interleave_task_writes(&svc, comment, || {
                    mutate().expect("competing mutation");
                })
                .expect("paused comment");
            }
            let stored = svc.require_task(task.id).expect("read committed task");
            let mut expected = before;
            match operation {
                "claim" => expected.assignee = Some(owner),
                "unassign" => expected.assignee = None,
                "update" => expected.title = "Edited".into(),
                "verify" => {
                    expected.status = TaskStatus::Done;
                    let verification = stored.verification.clone().expect("verification survives");
                    assert!(verification.passed);
                    assert_eq!(verification.verifier, owner);
                    assert!(verification.reasoning.contains("Keep criterion"));
                    expected.verification = Some(verification);
                }
                "reopen" => {
                    expected.status = TaskStatus::InProgress;
                    expected.verification = None;
                }
                "spawn" => {}
                _ => unreachable!("fixture operation"),
            }
            let comments = svc
                .task_comments_since(task.id, 0)
                .expect("comment readback");
            assert_eq!(
                comments.iter().filter(|c| c.body == "Keep comment").count(),
                1
            );
            assert_eq!(comments.len(), if operation == "spawn" { 2 } else { 1 });
            assert!(
                comments
                    .iter()
                    .all(|c| c.task_id == task.id && c.author == owner)
            );
            expected.comments = comments;
            expected.updated_at = stored.updated_at;
            assert_eq!(
                stored, expected,
                "{operation}, comment_first={comment_first}"
            );
        }
    }
}

#[test]
fn interleaved_moves_validate_current_status_and_leave_failed_changes_unwritten() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Move".into()), owner)
        .expect("create");
    let result = interleave_task_writes(
        &svc,
        || svc.task_move(task.id, TaskStatus::Ready, owner),
        || {
            svc.task_move(task.id, TaskStatus::Ready, owner)
                .expect("competing move");
        },
    );
    assert!(matches!(
        result,
        Err(KanbanError::InvalidTransition {
            from: TaskStatus::Ready,
            to: TaskStatus::Ready,
            ..
        })
    ));
    let committed = svc.require_task(task.id).expect("task survives");
    // A legitimate forward move still works; a skipped column still fails.
    assert!(matches!(
        svc.task_move(task.id, TaskStatus::Done, owner),
        Err(KanbanError::InvalidTransition {
            from: TaskStatus::Ready,
            to: TaskStatus::Done,
            ..
        })
    ));
    assert_eq!(
        svc.require_task(task.id).expect("unchanged on failure"),
        committed
    );
    svc.task_move(task.id, TaskStatus::InProgress, owner)
        .expect("legitimate next move");
}

#[test]
fn interleaved_mutations_recheck_revoked_assignee_authority() {
    for operation in ["comment", "move", "deliverable"] {
        let (svc, board, owner) = make_service_with_board();
        let task = svc
            .task_create(board.id, TaskSpec::new("Authority".into()), owner)
            .expect("create");
        let assignee = WebID::new();
        svc.task_claim(task.id, assignee).expect("claim");
        let result = interleave_task_writes(
            &svc,
            || match operation {
                "comment" => svc
                    .task_comment(task.id, assignee, "Not authorized")
                    .map(|_| ()),
                "move" => svc
                    .task_move(task.id, TaskStatus::Ready, assignee)
                    .map(|_| ()),
                "deliverable" => svc
                    .task_add_deliverable(task.id, "Not authorized", assignee)
                    .map(|_| ()),
                _ => unreachable!("fixture operation"),
            },
            || {
                svc.task_unassign(task.id, owner).expect("revoke authority");
            },
        );
        assert!(
            matches!(result, Err(KanbanError::PermissionDenied(_))),
            "{operation}"
        );
        let stored = svc.require_task(task.id).expect("task survives");
        assert_eq!(stored.assignee, None);
        assert_eq!(stored.status, TaskStatus::Backlog);
        assert!(stored.comments.is_empty() && stored.deliverables.is_empty());
        svc.task_comment(task.id, owner, "Owner still authorized")
            .expect("owner remains authorized");
    }
}

#[test]
fn interleaved_lifecycle_changes_validate_the_current_status() {
    for operation in ["claim", "verify", "reopen"] {
        let (svc, board, owner) = make_service_with_board();
        let task = svc
            .task_create(board.id, TaskSpec::new("Lifecycle".into()), owner)
            .expect("create");
        let initial_status = match operation {
            "claim" => TaskStatus::Ready,
            "verify" => TaskStatus::Review,
            "reopen" => TaskStatus::Done,
            _ => unreachable!("fixture operation"),
        };
        for status in [
            TaskStatus::Ready,
            TaskStatus::InProgress,
            TaskStatus::Review,
            TaskStatus::Done,
        ] {
            svc.task_move(task.id, status, owner)
                .expect("initial lifecycle");
            if status == initial_status {
                break;
            }
        }
        let result = interleave_task_writes(
            &svc,
            || match operation {
                "claim" => svc.task_claim(task.id, owner),
                "verify" => svc
                    .task_verify(task.id, "Confirmed", owner)
                    .map(|(task, _)| task),
                "reopen" => svc.task_reopen(task.id, owner),
                _ => unreachable!("fixture operation"),
            },
            || {
                let target = if operation == "reopen" {
                    TaskStatus::Review
                } else {
                    TaskStatus::InProgress
                };
                svc.task_move(task.id, target, owner)
                    .expect("competing lifecycle change");
            },
        );
        assert!(
            matches!(result, Err(KanbanError::InvalidTransition { .. })),
            "{operation}"
        );
        let stored = svc.require_task(task.id).expect("task survives");
        assert_eq!(
            stored.status,
            if operation == "reopen" {
                TaskStatus::Review
            } else {
                TaskStatus::InProgress
            }
        );
        assert_eq!(stored.assignee, None);
        assert_eq!(stored.verification, None);
    }
}

#[test]
fn interleaved_disjoint_edits_preserve_current_fields_and_explicit_clears() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(
            board.id,
            TaskSpec::new("Original".into()).with_description("Clear me".into()),
            owner,
        )
        .expect("create");
    interleave_task_writes(
        &svc,
        || {
            svc.task_update(
                task.id,
                owner,
                Some("Edited".into()),
                None,
                None,
                None,
                None,
                None,
            )
        },
        || {
            svc.task_update(
                task.id,
                owner,
                None,
                Some(None),
                None,
                None,
                Some(vec!["Kept label".into()]),
                None,
            )
            .expect("competing edit");
        },
    )
    .expect("paused edit");
    let stored = svc.require_task(task.id).expect("task survives");
    assert_eq!(stored.title, "Edited");
    assert_eq!(stored.description, None);
    assert_eq!(stored.labels, ["Kept label"]);
}

#[test]
fn interleaved_claims_do_not_replace_the_successful_assignee() {
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Claim".into()), owner)
        .expect("create");
    let winner = WebID::new();
    let result = interleave_task_writes(
        &svc,
        || svc.task_claim(task.id, owner),
        || {
            svc.task_claim(task.id, winner).expect("competing claim");
        },
    );
    assert!(matches!(result, Err(KanbanError::PermissionDenied(_))));
    assert_eq!(
        svc.require_task(task.id).expect("task survives").assignee,
        Some(winner)
    );
}

#[test]
fn task_mutations_preserve_typed_missing_task_and_authorization_errors() {
    for operation in [
        "move",
        "claim",
        "verify",
        "unassign",
        "update",
        "reopen",
        "comment",
        "deliverable",
        "spawn",
        "delete",
    ] {
        let (svc, board, owner) = make_service_with_board();
        let task = svc
            .task_create(board.id, TaskSpec::new("Protected".into()), owner)
            .expect("create");
        let stranger = WebID::new();
        let apply = |id, actor| match operation {
            "move" => svc.task_move(id, TaskStatus::Ready, actor).map(|_| ()),
            "claim" => svc.task_claim(id, actor).map(|_| ()),
            "verify" => svc.task_verify(id, "Confirmed", actor).map(|_| ()),
            "unassign" => svc.task_unassign(id, actor).map(|_| ()),
            "update" => svc
                .task_update(
                    id,
                    actor,
                    Some("Unauthorized".into()),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .map(|_| ()),
            "reopen" => svc.task_reopen(id, actor).map(|_| ()),
            "comment" => svc.task_comment(id, actor, "Unauthorized").map(|_| ()),
            "deliverable" => svc
                .task_add_deliverable(id, "Unauthorized", actor)
                .map(|_| ()),
            "spawn" => svc.spawn_task(id, SpawnSpec::new(id), actor).map(|_| ()),
            "delete" => svc.task_delete(id, actor).map(|_| ()),
            _ => unreachable!("fixture operation"),
        };
        let missing = hkask_types::id::TaskId::new();
        assert!(
            matches!(apply(missing, owner), Err(KanbanError::NotFound(ref error))
            if error.id == missing.to_string() && error.entity_type == "task"),
            "{operation}"
        );
        if operation == "claim" {
            // Self-claim is deliberately open to a new actor, but cannot
            // replace an existing assignment.
            svc.task_claim(task.id, owner).expect("claim fixture");
        }
        let before = svc
            .require_task(task.id)
            .expect("before unauthorized mutation");
        assert!(
            matches!(
                apply(task.id, stranger),
                Err(KanbanError::PermissionDenied(_))
            ),
            "{operation}"
        );
        assert_eq!(svc.require_task(task.id).expect("after refusal"), before);
    }
}

#[test]
fn interleaved_deletion_returns_not_found_without_resurrecting_task_or_index() {
    for delete_board in [false, true] {
        let (svc, board, owner) = make_service_with_board();
        let task = svc
            .task_create(board.id, TaskSpec::new("Delete".into()), owner)
            .expect("create");
        let result = interleave_task_writes(
            &svc,
            || svc.task_comment(task.id, owner, "Too late"),
            || {
                if delete_board {
                    svc.board_delete(board.id).expect("delete board");
                } else {
                    svc.task_delete(task.id, owner).expect("delete task");
                }
            },
        );
        assert!(
            matches!(result, Err(KanbanError::NotFound(ref missing)) if missing.id == task.id.to_string())
        );
        assert!(svc.task_get(task.id).expect("read missing task").is_none());
        if !delete_board {
            assert!(
                svc.task_list(board.id, TaskFilter::default())
                    .expect("empty index")
                    .tasks
                    .is_empty()
            );
        }
    }
}

/// Card 84c7e7b8 class pin: `board_rename`'s contract promises the board
/// h_mem is updated in place — same h_mem id, PKO procedure anchoring
/// preserved. The prior resolve-then-`HMemStore::update` form re-keyed the
/// row (delete + reinsert, new ID) on every rename, breaking that contract
/// the same way the task path's writes did.
#[test]
fn board_rename_preserves_the_board_row_h_mem_id() {
    let (svc, board, _owner) = make_service_with_board();
    let before = svc
        .store
        .query_by_entity_attribute("kanban:board", &board.id.to_string())
        .unwrap()
        .into_iter()
        .next()
        .expect("the board row exists")
        .id;
    svc.board_rename(board.id, "Renamed").unwrap();
    let after = svc
        .store
        .query_by_entity_attribute("kanban:board", &board.id.to_string())
        .unwrap()
        .into_iter()
        .next()
        .expect("the board row exists after the rename")
        .id;
    assert_eq!(
        before, after,
        "the board row's h_mem ID must be stable across a rename — the contract promises in-place update (PKO anchoring preserved)"
    );
    assert_eq!(
        svc.board_get(board.id)
            .unwrap()
            .expect("board reads back")
            .name,
        "Renamed",
        "the rename lands"
    );
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
    let env = fault_env()?;
    env.driver.execute_batch(
        "CREATE TRIGGER reject_task_index_insert BEFORE INSERT ON hmems
         WHEN NEW.entity LIKE 'kanban:board_tasks:%'
         BEGIN SELECT RAISE(FAIL, 'forced task index failure'); END;",
    )?;

    assert!(
        env.service
            .task_create(env.board.id, TaskSpec::new("Task".into()), env.owner)
            .is_err()
    );
    assert!(
        env.service
            .task_list(env.board.id, TaskFilter::all())?
            .tasks
            .is_empty()
    );
    assert!(env.store.query_by_entity("kanban:task")?.is_empty());
    assert!(
        env.store
            .query_by_entity(&format!("kanban:board_tasks:{}", env.board.id))?
            .is_empty()
    );
    Ok(())
}

#[test]
fn task_create_validates_goal_citations() {
    // The functional–technical join: a task may cite the goal criteria it
    // advances. Citations are validated at creation — the goal must exist,
    // the index must be in range, and the text must match verbatim — and
    // captured on the task as documentation of the immutable
    // goal.
    let (svc, board, owner) = make_service_with_board();
    let goal = svc
        .goal_create(
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![
                    VerificationCriterion::new("criterion 0 is observable".into()),
                    VerificationCriterion::new("criterion 1 is observable".into()),
                ],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: None,
            },
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
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![
                    VerificationCriterion::new("criterion 0 is observable".into()),
                    VerificationCriterion::new("criterion 1 is observable".into()),
                ],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: None,
            },
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
    let spec = TaskSpec::new("T2".into())
        .with_criteria(vec![VerificationCriterion::new("compile".into())]);
    svc.task_create(board.id, spec, owner).unwrap();

    let tasks = svc.task_list(board.id, TaskFilter::all()).unwrap().tasks;
    assert_eq!(tasks.len(), 2);
    // The criteria text round-trips through the list — a reader sees what
    // "done" means, not just how many criteria exist. The TaskInfo wire
    // projection maps this text 1:1.
    let t2 = tasks.iter().find(|t| t.title == "T2").unwrap();
    assert_eq!(t2.criteria.len(), 1);
    assert_eq!(t2.criteria[0].description, "compile");
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
        .unwrap()
        .tasks;
    assert_eq!(backlog.len(), 1);

    let in_progress = svc
        .task_list(board.id, TaskFilter::by_status(TaskStatus::InProgress))
        .unwrap()
        .tasks;
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

/// expect: "An Algedonic review card reaches Done only through verification" [P9]
#[test]
fn algedonic_review_done_requires_task_verify() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let board = svc
        .board_create(owner, ALGEDONIC_BOARD_NAME, &make_default_columns())
        .expect("board");
    let task = svc
        .task_create(board.id, TaskSpec::new("Observed alert".into()), owner)
        .expect("card");
    svc.task_move(task.id, TaskStatus::Ready, owner)
        .expect("triage");
    svc.task_move(task.id, TaskStatus::InProgress, owner)
        .expect("action");
    svc.task_move(task.id, TaskStatus::Review, owner)
        .expect("review");
    assert!(
        svc.task_move(task.id, TaskStatus::Done, owner).is_err(),
        "moving a card directly to Done must not bypass verification"
    );
    assert_eq!(
        svc.task_get(task.id).expect("read").expect("card").status,
        TaskStatus::Review
    );
    let (verified, _) = svc
        .task_verify(task.id, "Observation and reviewer decision", owner)
        .expect("verify");
    assert_eq!(verified.status, TaskStatus::Done);
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
    let (svc, board, owner) = make_service_with_three_column_board("Three columns");
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
    assert_boards_isolated_by_owner();
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

    service.task_delete(task.id, owner).expect("delete task");
    assert!(service.task_get(task.id).expect("query task").is_none());
    assert!(
        service
            .task_list(board.id, TaskFilter::all())
            .expect("task list")
            .tasks
            .is_empty()
    );
    assert!(service.board_get(board.id).expect("query board").is_some());
    assert_eq!(
        store.count().expect("row count"),
        1,
        "only the board row may remain"
    );
}

/// expect: "Deleting an edited task removes the current payload as well as its board index."
/// [P3] Motivating: Generative Space — an edit cannot leave invisible work after deletion.
/// [P2] Constraining: Transparent Imperfection — row-version identity changes do not weaken cleanup.
/// pre: task_update replaces the task's durable h_mem row
/// post: task_delete leaves only the board row, with no task payload or index
#[test]
fn task_delete_after_update_removes_replacement_payload() -> anyhow::Result<()> {
    let store = make_store();
    let service = KanbanService::new(store.clone());
    let owner = WebID::new();
    let board = service.board_create(owner, "Board", &make_default_columns())?;
    let task = service.task_create(board.id, TaskSpec::new("Original".into()), owner)?;
    service.task_update(
        task.id,
        owner,
        Some("Edited".into()),
        None,
        None,
        None,
        None,
        None,
    )?;
    service.task_delete(task.id, owner)?;
    assert!(service.task_get(task.id)?.is_none());
    assert!(
        service
            .task_list(board.id, TaskFilter::all())?
            .tasks
            .is_empty()
    );
    assert!(store.query_by_entity("kanban:task")?.is_empty());
    assert!(
        store
            .query_by_entity(&format!("kanban:board_tasks:{}", board.id))?
            .is_empty()
    );
    assert_eq!(store.count()?, 1);
    Ok(())
}

/// expect: "Deleting a task removes its payload and board index together or removes neither."
/// [P3] Motivating: Generative Space — board membership never points at missing work.
/// [P2] Constraining: Transparent Imperfection — a failed delete preserves visible prior state.
/// pre: one task exists and deletion of its index row is forced to fail
/// post: task deletion fails while task payload, board index, and board all remain
#[test]
fn task_delete_atomic_when_index_delete_fails() -> anyhow::Result<()> {
    let env = fault_env()?;
    let task = env
        .service
        .task_create(env.board.id, TaskSpec::new("Task".into()), env.owner)?;
    env.driver.execute_batch(
        "CREATE TRIGGER reject_task_index_delete BEFORE DELETE ON hmems
         WHEN OLD.entity LIKE 'kanban:board_tasks:%'
         BEGIN SELECT RAISE(FAIL, 'forced task index delete failure'); END;",
    )?;

    assert!(env.service.task_delete(task.id, env.owner).is_err());
    assert!(env.service.task_get(task.id)?.is_some());
    assert_eq!(
        env.service
            .task_list(env.board.id, TaskFilter::all())?
            .tasks
            .len(),
        1
    );
    assert!(env.service.board_get(env.board.id)?.is_some());
    assert_eq!(env.store.count()?, 3);
    Ok(())
}

/// expect: "Deleting a board removes the board and every child row together or removes nothing."
/// [P3] Motivating: Generative Space — board lifecycle owns its complete task aggregate.
/// [P2] Constraining: Transparent Imperfection — a child failure cannot orphan or hide work.
/// pre: one board with one task exists and deletion of the task payload is forced to fail
/// post: board deletion fails while board, task payload, and index remain readable
#[test]
fn board_delete_atomic_when_child_delete_fails() -> anyhow::Result<()> {
    let env = fault_env()?;
    let task = env
        .service
        .task_create(env.board.id, TaskSpec::new("Task".into()), env.owner)?;
    env.driver.execute_batch(
        "CREATE TRIGGER reject_board_child_delete BEFORE DELETE ON hmems
         WHEN OLD.entity = 'kanban:task'
         BEGIN SELECT RAISE(FAIL, 'forced board child delete failure'); END;",
    )?;

    assert!(env.service.board_delete(env.board.id).is_err());
    assert_board_and_task_retained(&env, &task)?;
    Ok(())
}

/// expect: "A board with an invalid procedure root cannot report successful deletion of its tasks."
/// [P2] Motivating: Transparent Imperfection — malformed roots fail without erasing work.
/// pre: board and task exist, but the root's procedure anchor is absent or wrong
/// post: deletion reports an error and board, task, and index remain
#[test]
fn board_delete_rejects_invalid_root_and_retains_all_rows() -> anyhow::Result<()> {
    for replacement in ["NULL", "'{\"pko_procedure\":\"other-board\"}'"] {
        let env = fault_env()?;
        let task =
            env.service
                .task_create(env.board.id, TaskSpec::new("Task".into()), env.owner)?;
        env.driver.execute_batch(&format!(
            "UPDATE hmems SET ontology = {replacement} WHERE entity = 'kanban:board'"
        ))?;

        assert!(env.service.board_delete(env.board.id).is_err());
        assert_board_and_task_retained(&env, &task)?;
    }
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
    let (svc, board, owner) = make_service_with_three_column_board("Round Trip Board");

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
        .expect("task list")
        .tasks;
    let markdown = export_board_to_mermaid(&board, &tasks);

    // Parse the exported markdown.
    let parsed = parse_mermaid_kanban(&markdown).expect("parse exported markdown");

    let (new_board, imported_count) = svc
        .board_import(owner, "Imported Board", &parsed)
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
        .expect("new board task list")
        .tasks;
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
        .expect("task list")
        .tasks;
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
        .expect("task list")
        .tasks;
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
    let (new_board, imported_count) = svc
        .board_import(owner, "Re-imported Ordered Board", &parsed)
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
    let md = "```mermaid\nkanban\n%% kanban column status: backlog\n  section Backlog\n```";
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
    let (board, imported_count) = svc
        .board_import(owner, "Empty Imported Board", &parsed)
        .expect("empty board import");
    assert_eq!(imported_count, 0);
    assert_eq!(board.columns.len(), 1);
    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list")
        .tasks;
    assert!(
        tasks.is_empty(),
        "imported empty board should have no tasks"
    );
}

/// Every goal carries the standing trust-economics clause
/// ([`GoalTrustClause`]) — the ΔT_trust ≥ 0 constraint is structural,
/// not a preference the caller can omit. The clause is populated from
/// the default ([`GoalTrustClause::default_clause`]) when the
/// declaration doesn't provide one, and is included in the goal's
/// spec digest so it cannot be silently stripped.
#[test]
fn goal_create_carries_the_trust_clause_by_default() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let goal = svc
        .goal_create(
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![VerificationCriterion::new(
                    "criterion 0 is observable".into(),
                )],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: None,
            },
            owner,
        )
        .unwrap();

    // The clause is present and populated with the default text.
    let clause = goal
        .trust_clause
        .as_ref()
        .expect("every goal carries the trust clause");
    assert_eq!(
        clause.text,
        crate::kanban::types::goal::GOAL_TRUST_CLAUSE_TEXT,
        "the default trust clause text is present"
    );
    assert!(
        clause.text.contains("T_reliance"),
        "the clause names the trust-economics constraint (Butler–Giuliano–Guiso 2009)"
    );
    assert!(
        clause.text.contains("Akerlof 1970"),
        "the clause cites the discounting model (Akerlof 1970)"
    );
    assert!(
        clause.text.contains("Fuller–Perdue 1936"),
        "the clause prices violations at the expectation interest (Fuller–Perdue 1936)"
    );
    assert!(
        !clause.model_refs.is_empty(),
        "the clause carries its cited reference models"
    );
}

/// A caller-supplied trust clause overrides the default; the clause
/// text is theirs, but the constraint (the clause is present and in
/// the digest) is structural.
#[test]
fn goal_create_accepts_caller_supplied_trust_clause() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let custom_clause = crate::GoalTrustClause {
        text: "Custom trust clause text".into(),
        model_refs: vec!["CustomModel (2024)".into()],
        declared_at: chrono::Utc::now(),
    };
    let goal = svc
        .goal_create(
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![VerificationCriterion::new(
                    "criterion 0 is observable".into(),
                )],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: Some(custom_clause),
            },
            owner,
        )
        .unwrap();

    let clause = goal
        .trust_clause
        .as_ref()
        .expect("the caller-supplied clause is present");
    assert_eq!(clause.text, "Custom trust clause text");
    assert_eq!(clause.model_refs, vec!["CustomModel (2024)".to_string()]);
}

/// The goal's trust ledger records instruction compliance during the
/// goal's lifecycle. Events are appended (never rewritten), the summary
/// is computed from the events, and the regulation signal is the
/// integrity-severity count.
#[test]
fn goal_trust_ledger_records_and_summarizes() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let goal = svc
        .goal_create(
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![VerificationCriterion::new(
                    "criterion 0 is observable".into(),
                )],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: None,
            },
            owner,
        )
        .unwrap();

    // Record a complied event.
    let complied = svc
        .goal_trust_record(
            goal.id,
            GoalTrustEvent {
                instruction: "do not break the user's trust".into(),
                status: GoalTrustStatus::Complied,
                severity: None,
                evidence: "all gates green; no shortcuts taken".into(),
                operator_cost: None,
                recorded_at: chrono::Utc::now(),
            },
            owner,
        )
        .unwrap();
    assert_eq!(complied.trust_events.len(), 1);
    assert_eq!(complied.trust_events[0].status, GoalTrustStatus::Complied);

    // Record a competence-severity violation (disclosed).
    let violated = svc
        .goal_trust_record(
            goal.id,
            GoalTrustEvent {
                instruction: "do not break the user's trust".into(),
                status: GoalTrustStatus::Violated,
                severity: Some(GoalTrustSeverity::Competence),
                evidence: "disclosed at detection; test failed; operator notified".into(),
                operator_cost: Some("operator re-verified the deliverable: 15 minutes".into()),
                recorded_at: chrono::Utc::now(),
            },
            owner,
        )
        .unwrap();
    assert_eq!(violated.trust_events.len(), 2);

    // The summary computes the regulation signal.
    let summary = violated.trust_summary();
    assert_eq!(summary.total_instructions, 2);
    assert_eq!(summary.complied, 1);
    assert_eq!(summary.violated_competence, 1);
    assert_eq!(summary.violated_integrity, 0);
    assert!(!summary.has_integrity_violation);

    // An integrity-severity violation sets the escalation flag.
    let integrity = svc
        .goal_trust_record(
            goal.id,
            GoalTrustEvent {
                instruction: "report defects at their true severity".into(),
                status: GoalTrustStatus::Violated,
                severity: Some(GoalTrustSeverity::Integrity),
                evidence: "defect discovered by the operator after being classified as a nuance"
                    .into(),
                operator_cost: Some("operator re-derived the entire deliverable: 2 hours".into()),
                recorded_at: chrono::Utc::now(),
            },
            owner,
        )
        .unwrap();
    let summary = integrity.trust_summary();
    assert_eq!(summary.violated_integrity, 1);
    assert!(summary.has_integrity_violation);
}

/// A violated trust event without a severity class is rejected —
/// every violation must be classified (competence or integrity)
/// because the regulation loop differentiates on this.
#[test]
fn goal_trust_record_rejects_violation_without_severity() {
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let goal = svc
        .goal_create(
            crate::GoalDeclaration {
                goal_text: "The user can see which work serves which goal".into(),
                criteria: vec![VerificationCriterion::new(
                    "criterion 0 is observable".into(),
                )],
                task_id: None,
                source: crate::GoalSource {
                    interaction_id: "source-turn".into(),
                    session_id: "test-session".into(),
                    source: "explicit-spec".into(),
                },
                forecast: None,
                supersedes: None,
                trust_clause: None,
            },
            owner,
        )
        .unwrap();

    let result = svc.goal_trust_record(
        goal.id,
        GoalTrustEvent {
            instruction: "do not break the user's trust".into(),
            status: GoalTrustStatus::Violated,
            severity: None,
            evidence: "evidence".into(),
            operator_cost: None,
            recorded_at: chrono::Utc::now(),
        },
        owner,
    );
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("violated trust event requires a severity class")
    );
}
