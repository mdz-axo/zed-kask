use super::service::KanbanService;
use crate::VerificationCriterion;
use crate::kanban::mermaid::{columns_from_parsed, export_board_to_mermaid, parse_mermaid_kanban};
use crate::kanban::{Board, ColumnDef, SpawnSpec, TaskFilter, TaskSpec, TaskStatus};
use hkask_storage::HMemStore;
use hkask_types::WebID;
use hkask_types::id::BoardId;

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
    // durable link is set before the worktree/fallback branch in
    // `kanban_task_spawn`. Both execution paths then expose the link via
    // `kanban_task_delegate_result` without each having to set it.
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
fn task_record_delegation_writes_structured_fields() {
    // Slice 2: task_record_delegation writes the LocalDelegateResult and
    // TaskSuccessVerdict to the task's persisted fields.
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Spawn task".into()), owner)
        .unwrap();

    let delegate_result = hkask_mcp_swarm::LocalDelegateResult {
        agent_id: "test-agent".to_string(),
        response: "test response".to_string(),
        model: "test-model".to_string(),
        tokens_used: 100,
        cost: 5,
        balance: Some(95),
        cost_uncapped: 5,
        latency_ms: 200,
        tool_calls: vec![],
        task_success: None,
        bind_matched: None,
        raw_response: None,
        envelope: None,
    };
    let verdict = hkask_mcp_swarm::TaskSuccessVerdict {
        pass: true,
        score: Some(0.9),
        detail: Some("all checks passed".to_string()),
        provenance: hkask_mcp_swarm::VerdictSource::DeterministicEvaluator,
    };

    let updated = svc
        .task_record_delegation(
            task.id,
            Some("swarm-1".to_string()),
            delegate_result,
            Some(verdict),
            owner,
        )
        .unwrap();

    assert_eq!(updated.swarm_id.as_deref(), Some("swarm-1"));
    let dr = updated
        .delegate_result
        .expect("delegate_result should be set");
    assert_eq!(dr.agent_id, "test-agent");
    assert_eq!(dr.response, "test response");
    assert_eq!(dr.tokens_used, 100);
    let dv = updated
        .deterministic_verdict
        .expect("deterministic_verdict should be set");
    assert!(dv.pass);
    assert_eq!(dv.score, Some(0.9));
    assert_eq!(
        dv.provenance,
        hkask_mcp_swarm::VerdictSource::DeterministicEvaluator
    );

    // Verify persistence: re-read the task from the store.
    let reloaded = svc.task_get(task.id).unwrap().expect("task should persist");
    assert_eq!(reloaded.swarm_id.as_deref(), Some("swarm-1"));
    assert!(reloaded.delegate_result.is_some());
    assert!(reloaded.deterministic_verdict.is_some());
}

#[test]
fn task_record_delegation_rejects_non_owner() {
    // Slice 2: only the task owner can record a delegation result.
    let (svc, board, owner) = make_service_with_board();
    let task = svc
        .task_create(board.id, TaskSpec::new("Owner task".into()), owner)
        .unwrap();
    let other = WebID::new();
    let delegate_result = hkask_mcp_swarm::LocalDelegateResult {
        agent_id: "test-agent".to_string(),
        response: "test".to_string(),
        model: "m".to_string(),
        tokens_used: 0,
        cost: 0,
        balance: Some(0),
        cost_uncapped: 0,
        latency_ms: 0,
        tool_calls: vec![],
        task_success: None,
        bind_matched: None,
        raw_response: None,
        envelope: None,
    };
    let result = svc.task_record_delegation(task.id, None, delegate_result, None, other);
    assert!(
        result.is_err(),
        "non-owner should not be able to record delegation"
    );
}

#[test]
fn board_delete_removes_board_and_tasks() {
    // Slice 4: board_delete removes the board and all its tasks.
    let (svc, board, owner) = make_service_with_board();
    svc.task_create(board.id, TaskSpec::new("T1".into()), owner)
        .unwrap();
    svc.task_create(board.id, TaskSpec::new("T2".into()), owner)
        .unwrap();

    let tasks_deleted = svc.board_delete(board.id).unwrap();
    assert_eq!(tasks_deleted, 2, "should delete both tasks");

    // Board is gone.
    assert!(svc.board_get(board.id).unwrap().is_none());
    // Board list no longer includes it.
    let boards = svc.board_list(&owner).unwrap();
    assert!(boards.iter().all(|b| b.id != board.id));
}

#[test]
fn rjoule_exhaust_stamps_rjoule_reason() {
    // rJoule exhaustion marks the task Done with an rJoule-specific
    // verification reason.
    let (svc, board, owner) = make_service_with_board();
    let spec = TaskSpec::new("Inference-heavy task".into()).with_rjoule_budget(0);
    let task = svc.task_create(board.id, spec, owner).unwrap();

    let exhausted = svc.task_rjoule_exhaust(task.id).unwrap();
    assert_eq!(exhausted.status, TaskStatus::Done);
    let verification = exhausted.verification.expect("verification stamped");
    assert!(!verification.passed, "exhaustion is a failed verification");
    assert_eq!(
        verification.verifier, task.owner,
        "verifier is the task owner"
    );
    assert_eq!(
        verification.reasoning,
        "rJoules exhausted — inference budget consumed."
    );
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

    // In Progress task — move through the transition chain.
    let in_prog = svc
        .task_create(board.id, TaskSpec::new("In Progress Task".into()), owner)
        .expect("task In Progress");
    svc.task_move(in_prog.id, TaskStatus::Ready, owner)
        .expect("move to Ready");
    svc.task_move(in_prog.id, TaskStatus::InProgress, owner)
        .expect("move to InProgress");

    // Done task — move all the way through and verify.
    let done = svc
        .task_create(board.id, TaskSpec::new("Done Task".into()), owner)
        .expect("task Done");
    svc.task_move(done.id, TaskStatus::Ready, owner)
        .expect("move to Ready");
    svc.task_move(done.id, TaskStatus::InProgress, owner)
        .expect("move to InProgress");
    svc.task_move(done.id, TaskStatus::Review, owner)
        .expect("move to Review");
    svc.task_verify(done.id, "work complete", owner)
        .expect("verify to Done");

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

    // Import: build columns from the parsed kanban and create a new board.
    let new_columns = columns_from_parsed(&parsed);
    let new_board = svc
        .board_create(owner, "Imported Board", &new_columns)
        .expect("imported board create");

    // Re-create tasks on the new board in the order they appeared in each
    // column of the parsed markdown, placing each in the column's status.
    for column in &parsed.columns {
        let target_status = new_board
            .columns
            .iter()
            .find(|c| c.name == column.name)
            .expect("column exists on new board")
            .status;
        for title in &column.tasks {
            let task = svc
                .task_create(new_board.id, TaskSpec::new(title.clone()), owner)
                .expect("task create on new board");
            // Walk the task forward to the target status. Tasks start in
            // Backlog; advance through the transition chain.
            let mut current = TaskStatus::Backlog;
            while current != target_status {
                let next = current.next().expect("status advances toward target");
                svc.task_move(task.id, next, owner)
                    .expect("task move toward target status");
                current = next;
            }
        }
    }

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
fn export_import_round_trip_handles_special_characters() {
    // Tasks with quotes, brackets, unicode, and backslashes in their titles
    // must survive the export → parse → import round-trip unchanged.
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
    let new_columns = columns_from_parsed(&parsed);
    let new_board = svc
        .board_create(owner, "Re-imported Ordered Board", &new_columns)
        .expect("imported board create");
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

    // Verify the parsed columns can drive a real board creation through the
    // service layer.
    let svc = KanbanService::new(make_store());
    let owner = WebID::new();
    let columns = columns_from_parsed(&parsed);
    let board = svc
        .board_create(owner, "Empty Imported Board", &columns)
        .expect("empty board create");
    assert_eq!(board.columns.len(), 1);
    let tasks = svc
        .task_list(board.id, TaskFilter::all())
        .expect("task list");
    assert!(
        tasks.is_empty(),
        "imported empty board should have no tasks"
    );
}
