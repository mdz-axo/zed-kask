//! Replay protection end-to-end, through the real tool surface.
//!
//! `src/idempotency.rs` unit-tests the store. These tests exercise the thing that
//! actually matters: calling `kanban_task_create` twice with the same
//! `idempotency_key` must leave **one** task on the board, not two.
//!
//! # Why this layer needs its own tests
//!
//! The store can be perfectly correct while the wiring is wrong — a tool that
//! forgets to thread its key, or reserves without recording, would pass every
//! store test and still duplicate work in production. These tests go through
//! `kanban_*` tool calls and then *count rows*, so they fail if either the store
//! or its wiring regresses.
//!
//! # What a replayed call looks like
//!
//! The original response, plus `replayed: true`. Callers can therefore tell "your
//! retry was absorbed" from "this just ran", which the panel uses to avoid
//! reporting a spurious second create.

#![cfg(test)]

use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_kata_kanban::{KanbanServer, KanbanService};
use hkask_mcp_server::server::McpToolError;
use hkask_mcp_swarm::{LazyLocalSwarmRuntime, LocalAgentRegistry};
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::{InferenceError, McpErrorKind, WebID, WorktreeSpawnPort};
use rmcp::handler::server::wrapper::Parameters;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Worktree-spawn stub for tests: returns an error so `kanban_task_spawn` falls
/// back to `LazyLocalSwarmRuntime`. Mirrors `hkask_inference::UnavailableWorktreeSpawn`,
/// which is `pub(crate)` and so not nameable from an external test crate.
struct UnavailableWorktreeSpawn;

impl WorktreeSpawnPort for UnavailableWorktreeSpawn {
    fn create_worktree_thread<'a>(
        &'a self,
        _prompt: &'a str,
        _title: &'a str,
        _worktree_name: Option<&'a str>,
        _base_ref: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, InferenceError>> + Send + 'a>> {
        Box::pin(async {
            Err(InferenceError::Connection(
                "worktree spawn unavailable (test stub)".to_string(),
            ))
        })
    }
}

/// Build a server whose replay-protection store shares the kanban driver, as
/// production does. Returns the server plus the shared driver so a test can
/// simulate a second process over the same database.
fn make_server_with_shared_driver() -> (
    KanbanServer,
    Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
) {
    let driver = SqliteDriver::in_memory_driver();
    let idempotency_driver = driver.clone();
    let store = HMemStore::from_driver(driver.clone()).expect("hmem store init");
    let service = KanbanService::new(store);
    let ledger_path = std::env::temp_dir()
        .join(format!("kanban-idem-{}.db", std::process::id()))
        .to_string_lossy()
        .to_string();
    let idempotency =
        hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(idempotency_driver)
            .expect("idempotency schema");
    let server = KanbanServer::new(
        WebID::new(),
        service,
        Arc::new(LazyLocalSwarmRuntime::lazy(ledger_path)),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(UnavailableWorktreeSpawn),
        Arc::new(idempotency),
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
    );
    (server, driver)
}

fn make_server() -> KanbanServer {
    make_server_with_shared_driver().0
}

fn parse(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output must be valid JSON")
}

async fn create_board(server: &KanbanServer, name: &str, key: Option<&str>) -> serde_json::Value {
    let out = server
        .kanban_board_create(Parameters(BoardCreateRequest {
            name: name.to_string(),
            columns: None,
            idempotency_key: key.map(str::to_string),
        }))
        .await
        .expect("tool ok");
    parse(&out)
}

async fn create_task(
    server: &KanbanServer,
    board_id: &str,
    title: &str,
    key: Option<&str>,
) -> Result<serde_json::Value, McpToolError> {
    let out = server
        .kanban_task_create(Parameters(TaskCreateRequest {
            board_id: board_id.to_string(),
            title: title.to_string(),
            description: None,
            criteria: None,
            rjoule_budget: None,
            idempotency_key: key.map(str::to_string),
        }))
        .await?;
    Ok(parse(&out))
}

/// How many tasks the board actually holds. The row count is the oracle: a
/// response-shape assertion alone would not catch a duplicate write.
async fn task_count(server: &KanbanServer, board_id: &str) -> usize {
    let out = server
        .kanban_task_list(Parameters(TaskListRequest {
            board_id: board_id.to_string(),
            status: None,
        }))
        .await
        .expect("tool ok");
    parse(&out)
        .get("tasks")
        .and_then(|t| t.as_array())
        .map(Vec::len)
        .unwrap_or_else(|| panic!("task_list did not return a tasks array: {out}"))
}

async fn board_count(server: &KanbanServer) -> usize {
    let out = server
        .kanban_board_list(Parameters(BoardListRequest {}))
        .await
        .expect("tool ok");
    parse(&out)
        .get("boards")
        .and_then(|b| b.as_array())
        .map(Vec::len)
        .unwrap_or_else(|| panic!("board_list did not return a boards array: {out}"))
}

// ── The headline guarantee ──────────────────────────────────────────────────

/// Two `kanban_task_create` calls with the same key create ONE task.
///
/// This is the regression for the duplicate-create hazard: an interrupted call
/// has an unknown outcome, so the client must be able to retry it. Before replay
/// protection, that retry produced a second task.
#[tokio::test]
async fn replayed_task_create_yields_one_task() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let first = create_task(&server, &board_id, "Write tests", Some("gesture-1"))
        .await
        .expect("tool ok");
    let first_id = first["task_id"].as_str().expect("task_id").to_string();

    // The retry an interrupted call would issue: same key, same args.
    let replay = create_task(&server, &board_id, "Write tests", Some("gesture-1"))
        .await
        .expect("tool ok");

    assert_eq!(
        replay["task_id"].as_str(),
        Some(first_id.as_str()),
        "a replay must return the ORIGINAL task id, not mint a new one"
    );
    assert_eq!(
        replay["replayed"].as_bool(),
        Some(true),
        "a replayed response must be marked so the caller can tell it was absorbed"
    );
    assert_eq!(
        task_count(&server, &board_id).await,
        1,
        "the board must hold exactly one task - two would be the duplicate-create bug"
    );
}

/// Same guarantee for boards.
#[tokio::test]
async fn replayed_board_create_yields_one_board() {
    let server = make_server();
    let first = create_board(&server, "Only one", Some("board-gesture")).await;
    let replay = create_board(&server, "Only one", Some("board-gesture")).await;

    assert_eq!(
        replay["board_id"].as_str(),
        first["board_id"].as_str(),
        "a replay must return the original board id"
    );
    assert_eq!(
        board_count(&server).await,
        1,
        "exactly one board must exist after a replayed create"
    );
}

/// Hammering the same key never produces a second row.
#[tokio::test]
async fn repeated_replays_never_duplicate() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    for _ in 0..5 {
        create_task(&server, &board_id, "Same gesture", Some("k"))
            .await
            .expect("tool ok");
    }
    assert_eq!(
        task_count(&server, &board_id).await,
        1,
        "five replays of one gesture must still yield one task"
    );
}

// ── Keys must not over-suppress ─────────────────────────────────────────────

/// Distinct keys are distinct work.
///
/// The inverse failure of the duplicate bug: an over-eager store that collapsed
/// different gestures would silently drop real work the operator asked for.
#[tokio::test]
async fn different_keys_create_different_tasks() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let a = create_task(&server, &board_id, "First", Some("k-a"))
        .await
        .expect("tool ok");
    let b = create_task(&server, &board_id, "Second", Some("k-b"))
        .await
        .expect("tool ok");

    assert_ne!(
        a["task_id"].as_str(),
        b["task_id"].as_str(),
        "distinct keys are distinct gestures and must both create"
    );
    assert_eq!(task_count(&server, &board_id).await, 2);
}

/// Omitting the key keeps the old behavior: every call is new work.
///
/// Protection is opt-in, so existing callers (and the agent, which does not send
/// keys) are unaffected.
#[tokio::test]
async fn omitted_key_preserves_unprotected_behavior() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    create_task(&server, &board_id, "One", None)
        .await
        .expect("tool ok");
    create_task(&server, &board_id, "One", None)
        .await
        .expect("tool ok");

    assert_eq!(
        task_count(&server, &board_id).await,
        2,
        "without a key there is no replay protection - two calls are two tasks"
    );
}

/// A key used for one tool must not suppress a different tool.
#[tokio::test]
async fn keys_do_not_collide_across_tools() {
    let server = make_server();
    let board = create_board(&server, "Board", Some("shared-key")).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let task = create_task(&server, &board_id, "Task", Some("shared-key"))
        .await
        .expect("tool ok");
    assert!(
        task.get("task_id").is_some(),
        "the same key on a different tool must still do its work, got: {task}"
    );
    assert_eq!(task_count(&server, &board_id).await, 1);
}

// ── Failure and degradation paths ───────────────────────────────────────────

/// A failed call releases its key, so a corrected retry succeeds.
///
/// Without the release, a rejected call would poison its key: the retry would be
/// told "outcome unknown" for work that demonstrably never happened.
#[tokio::test]
async fn failed_call_releases_the_key_for_a_clean_retry() {
    let server = make_server();
    // A bad board id fails validation before any write.
    let failed = create_task(&server, "not-a-board-id", "Task", Some("retry-me"))
        .await
        .expect_err("a malformed board id must fail before any write");
    assert!(
        matches!(failed.kind, McpErrorKind::InvalidArgument),
        "expected a structured invalid-argument error for a malformed board id, got: {:?}",
        failed
    );

    // Same key, now with a valid board: must run, not report "outcome unknown".
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();
    let retried = create_task(&server, &board_id, "Task", Some("retry-me"))
        .await
        .expect("tool ok");

    assert!(
        retried.get("task_id").is_some(),
        "a key from a cleanly-failed call must be reusable, got: {retried}"
    );
    assert_eq!(task_count(&server, &board_id).await, 1);
}

/// An empty key is rejected rather than silently treated as "no protection".
///
/// Silently ignoring it would give the caller the opposite of what it asked for.
#[tokio::test]
async fn empty_key_is_rejected() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let error = create_task(&server, &board_id, "Task", Some("   "))
        .await
        .expect_err("a whitespace-only key must be refused, not silently ignored");
    assert!(
        matches!(error.kind, McpErrorKind::InvalidArgument),
        "a whitespace-only key must be refused as invalid_argument, got: {:?}",
        error
    );
    assert_eq!(
        task_count(&server, &board_id).await,
        0,
        "a refused call must not have created anything"
    );
}

/// A replay from a *second process* over the same database is also absorbed.
///
/// This is the real deployment shape: the governed `McpRuntime` instance and the
/// per-project `ContextServerStore` instance both open the same kanban DB. An
/// in-memory-only store would pass every single-process test and still duplicate
/// here.
#[tokio::test]
async fn replay_is_absorbed_across_processes() {
    let (process_a, shared_driver) = make_server_with_shared_driver();
    let board = create_board(&process_a, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let first = create_task(&process_a, &board_id, "Cross-process", Some("shared"))
        .await
        .expect("tool ok");
    let first_id = first["task_id"].as_str().expect("task_id").to_string();

    // A second server over the same database — the two-instance production shape.
    let store = HMemStore::from_driver(shared_driver.clone()).expect("hmem store");
    let process_b = KanbanServer::new(
        WebID::new(),
        KanbanService::new(store),
        Arc::new(LazyLocalSwarmRuntime::lazy(
            std::env::temp_dir()
                .join(format!("kanban-idem-b-{}.db", std::process::id()))
                .to_string_lossy()
                .to_string(),
        )),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(UnavailableWorktreeSpawn),
        Arc::new(
            hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(shared_driver)
                .expect("idempotency schema"),
        ),
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
    );

    let replay = create_task(&process_b, &board_id, "Cross-process", Some("shared"))
        .await
        .expect("tool ok");
    assert_eq!(
        replay["task_id"].as_str(),
        Some(first_id.as_str()),
        "a replay in another process must return the first process's task id"
    );
    assert_eq!(
        task_count(&process_a, &board_id).await,
        1,
        "cross-process replay must not duplicate the task"
    );
}

/// When protection is only process-local, the response says so.
///
/// Per the repo's advertised-invariant rule: a guarantee that is weaker than it
/// looks must be labelled, not assumed. The in-memory store cannot dedupe across
/// a restart, so a caller relying on that must be able to tell.
#[tokio::test]
async fn non_durable_protection_is_labelled_in_the_response() {
    let ledger_path = std::env::temp_dir()
        .join(format!("kanban-idem-mem-{}.db", std::process::id()))
        .to_string_lossy()
        .to_string();
    let store = HMemStore::from_driver(SqliteDriver::in_memory_driver()).expect("hmem store");
    // The in-memory (non-durable) replay-protection backend.
    let server = KanbanServer::new(
        WebID::new(),
        KanbanService::new(store),
        Arc::new(LazyLocalSwarmRuntime::lazy(ledger_path)),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(UnavailableWorktreeSpawn),
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
    );

    let board = create_board(&server, "Board", Some("labelled")).await;
    assert_eq!(
        board["idempotency_durable"].as_bool(),
        Some(false),
        "a process-local guarantee must be labelled so an operator is not told a call \
         was replay-protected across restarts when it was not, got: {board}"
    );

    // The guarantee still holds within the process.
    let replay = create_board(&server, "Board", Some("labelled")).await;
    assert_eq!(replay["board_id"].as_str(), board["board_id"].as_str());
    assert_eq!(board_count(&server).await, 1);
}

/// A durable store does NOT add the label, so its absence is meaningful.
#[tokio::test]
async fn durable_protection_carries_no_degradation_label() {
    let server = make_server();
    let board = create_board(&server, "Board", Some("durable")).await;
    assert!(
        board.get("idempotency_durable").is_none(),
        "a durable store must not emit the degradation marker, or the marker would \
         carry no information, got: {board}"
    );
}

// ── Goal replay protection: process-local by design ──────────────────────

async fn create_goal(
    server: &KanbanServer,
    goal_text: &str,
    key: Option<&str>,
) -> serde_json::Value {
    let out = server
        .kanban_goal_create(Parameters(GoalCreateRequest {
            goal_text: goal_text.to_string(),
            criteria: vec![GoalCriterionInput {
                description: "observable criterion".to_string(),
            }],
            prediction: None,
            task_id: None,
            idempotency_key: key.map(str::to_string),
        }))
        .await
        .expect("tool ok");
    parse(&out)
}

/// Goal replay protection must NOT survive a restart, even when the kanban DB
/// (and the durable idempotency store) does.
///
/// Goals are ephemeral (operator ruling 2026-08-29): the goal store dies with
/// the process. A durable replay cache for `kanban_goal_create` would return
/// the first call's response — `replayed: true`, the dead goal's id — for a
/// goal that no longer exists, handing the agent a ghost pointer whose next
/// `kanban_goal_judge` fails NotFound. The re-create must mint a fresh goal.
#[tokio::test]
async fn goal_replay_protection_does_not_survive_a_restart() {
    let (process_a, shared_driver) = make_server_with_shared_driver();
    let first = create_goal(&process_a, "Goal", Some("goal-key")).await;
    let first_id = first["goal_id"].as_str().expect("goal_id").to_string();

    // A second server over the same durable kanban DB — the restart shape.
    // The durable idempotency store is intact; the goal store is fresh.
    let store = HMemStore::from_driver(shared_driver.clone()).expect("hmem store");
    let process_b = KanbanServer::new(
        WebID::new(),
        KanbanService::new(store),
        Arc::new(LazyLocalSwarmRuntime::lazy(
            std::env::temp_dir()
                .join(format!("kanban-idem-goal-b-{}.db", std::process::id()))
                .to_string_lossy()
                .to_string(),
        )),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(UnavailableWorktreeSpawn),
        Arc::new(
            hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(shared_driver)
                .expect("idempotency schema"),
        ),
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
    );

    let second = create_goal(&process_b, "Goal", Some("goal-key")).await;
    let second_id = second["goal_id"].as_str().expect("goal_id").to_string();
    assert_ne!(
        second_id, first_id,
        "a re-create after restart must mint a fresh goal, not replay the dead one"
    );
    assert!(
        second.get("replayed").is_none(),
        "the response must not claim a replay of a goal that died with the process, got: {second}"
    );

    // The fresh goal is real: it is listable on the new process; the dead one
    // is not.
    let out = process_b
        .kanban_goal_list(Parameters(GoalListRequest {}))
        .await
        .expect("tool ok");
    let listed = parse(&out);
    let ids: Vec<&str> = listed["goals"]
        .as_array()
        .expect("goals array")
        .iter()
        .filter_map(|g| g["goal_id"].as_str())
        .collect();
    assert!(
        ids.contains(&second_id.as_str()),
        "the fresh goal must exist in the live store, got: {listed}"
    );
    assert!(
        !ids.contains(&first_id.as_str()),
        "the dead goal must not appear in the new process's store"
    );
}

/// Within one process, a keyed goal create is still replay-protected — the
/// process-local store absorbs the retry, and the label says so.
#[tokio::test]
async fn goal_replay_is_absorbed_within_the_process() {
    let server = make_server();
    let first = create_goal(&server, "Goal", Some("goal-key")).await;
    assert_eq!(
        first["idempotency_durable"].as_bool(),
        Some(false),
        "goal replay protection is process-local and must be labelled as such, got: {first}"
    );

    let replay = create_goal(&server, "Goal", Some("goal-key")).await;
    assert_eq!(
        replay["goal_id"].as_str(),
        first["goal_id"].as_str(),
        "an in-process retry must return the original goal"
    );
    assert_eq!(
        replay["replayed"].as_bool(),
        Some(true),
        "an in-process retry must be marked as a replay, got: {replay}"
    );
}

// ── Wire contract ──────────────────────────────────────────────────────────

/// The field name the panel sends must be the field name the server reads.
///
/// The panel injects `idempotency_key` into a raw `serde_json` object
/// (`kanban_panel::attach_idempotency_key`) rather than building a typed request,
/// so nothing at compile time ties the two sides together. A rename on either
/// side would silently disable replay protection: `#[serde(default)]` makes the
/// server accept the request and quietly treat every retry as new work.
///
/// This drives the real deserialization path to pin the contract.
#[test]
fn wire_field_name_matches_what_clients_send() {
    // Exactly the shape `attach_idempotency_key` produces.
    let from_panel = serde_json::json!({
        "board_id": "b-1",
        "title": "Write tests",
        "idempotency_key": "gesture-123",
    });
    let parsed: TaskCreateRequest =
        serde_json::from_value(from_panel).expect("the panel's payload must deserialize");
    assert_eq!(
        parsed.idempotency_key.as_deref(),
        Some("gesture-123"),
        "the server must read the same field name the panel writes; a rename on \
         either side silently disables replay protection"
    );

    let board: BoardCreateRequest =
        serde_json::from_value(serde_json::json!({ "name": "B", "idempotency_key": "k" }))
            .expect("board payload deserializes");
    assert_eq!(board.idempotency_key.as_deref(), Some("k"));

    let spawn: TaskSpawnRequest = serde_json::from_value(serde_json::json!({
        "task_id": "t-1",
        "delegation_level": "standard",
        "idempotency_key": "k",
    }))
    .expect("spawn payload deserializes");
    assert_eq!(spawn.idempotency_key.as_deref(), Some("k"));
}

/// Omitting the field still deserializes, so unprotected callers keep working.
#[test]
fn wire_contract_is_backward_compatible() {
    let legacy: TaskCreateRequest =
        serde_json::from_value(serde_json::json!({ "board_id": "b", "title": "t" }))
            .expect("a request without the field must still deserialize");
    assert!(
        legacy.idempotency_key.is_none(),
        "adding the field must not break callers that never send it (the agent \
         does not)"
    );
}

// ── Spawn on an unfunded ledger ─────────────────────────────────────────────

/// `kanban_task_spawn` is not blocked by an unfunded ledger.
///
/// A kanban board does not need funding, and neither do local swarm agents: they
/// run on the operator's own substrate, so there is nothing for the server to
/// withhold. Before the local funding gate was removed, this call returned
/// `permission_denied: "insufficient local credits: have 0, need 10 — fund via
/// swarm_fund_local"` and never reached the work at all.
///
/// The spawn still fails here — no inference port is wired in a test binary — but
/// it must fail *past* the funding check. The assertion is therefore about which
/// failure occurs, which is the only honest thing to assert without a live model.
#[tokio::test]
async fn spawn_is_not_blocked_by_an_unfunded_ledger() {
    let server = make_server();
    let board = create_board(&server, "Board", None).await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();
    let task = create_task(&server, &board_id, "Spawn me", None)
        .await
        .expect("tool ok");
    let task_id = task["task_id"].as_str().expect("task_id").to_string();

    let error = server
        .kanban_task_spawn(Parameters(TaskSpawnRequest {
            task_id,
            idempotency_key: Some("spawn-gesture".to_string()),
            delegation_level: "standard".to_string(),
            delegated_skills: vec![],
            memory_scope: None,
            rjoule_budget: None,
            swarm_id: None,
        }))
        .await
        .expect_err("spawn fails without a wired inference port");

    assert!(
        !error.message.contains("insufficient local credits"),
        "an unfunded ledger must NOT block a local spawn - the kanban board and local \
         agents run on the operator's own substrate, so there is nothing to fund. \
         Got: {}",
        error.message
    );
    assert!(
        !error.message.contains("swarm_fund_local"),
        "the spawn path must not tell the operator to fund a ledger that is not a \
         gate. Got: {}",
        error.message
    );
}

// ── kanban_task_move PKO execution-status annotation ────────────────────────

/// A status transition must carry the execution-axis annotation: the new
/// status mapped to its PKO execution-status individual via the shared
/// vocabulary bridge (`hkask_bridge_ontology::pko::kanban_status_to_pko_execution`),
/// not a local re-implementation. Only statuses PKO v2.0.0 publishes
/// individuals for are mapped (InProgress, Completed, Paused); the
/// pre-execution and review statuses omit the field rather than force a
/// nonexistent individual.
#[tokio::test]
async fn task_move_carries_pko_execution_status_for_every_standard_transition() {
    let server = make_server();
    let board = create_board(&server, "pko-board", None).await;
    let board_id = board["board_id"].as_str().expect("board id").to_string();
    let task = create_task(&server, &board_id, "pko task", None)
        .await
        .expect("task ok");
    let task_id = task["task_id"].as_str().expect("task id").to_string();

    // The state machine requires backlog → ready → in_progress → review → done.
    // `ready` has no published PKO individual — the field is omitted.
    let out = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.clone(),
            target_status: "ready".to_string(),
        }))
        .await
        .expect("move ok");
    let parsed = parse(&out);
    assert!(
        parsed["pko_execution_status"].is_null(),
        "moving to ready must omit the PKO execution status (no published individual), got: {parsed}"
    );
    // Statuses PKO v2.0.0 publishes individuals for carry the annotation.
    let out = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.clone(),
            target_status: "in_progress".to_string(),
        }))
        .await
        .expect("move ok");
    let parsed = parse(&out);
    assert_eq!(
        parsed["pko_execution_status"].as_str(),
        Some("pko:InProgress"),
        "moving to in_progress must carry the PKO execution status, got: {parsed}"
    );
    // Statuses PKO publishes no individual for (review) omit the field —
    // never a forced nonexistent status.
    let out = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.clone(),
            target_status: "review".to_string(),
        }))
        .await
        .expect("move ok");
    let parsed = parse(&out);
    assert!(
        parsed["pko_execution_status"].is_null(),
        "moving to review must omit the PKO execution status (no published individual), got: {parsed}"
    );
    let out = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.clone(),
            target_status: "done".to_string(),
        }))
        .await
        .expect("move ok");
    let parsed = parse(&out);
    assert_eq!(
        parsed["pko_execution_status"].as_str(),
        Some("pko:Completed"),
        "moving to done must carry the PKO execution status, got: {parsed}"
    );
}
