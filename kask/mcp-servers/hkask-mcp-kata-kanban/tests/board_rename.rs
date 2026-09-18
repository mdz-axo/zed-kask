//! Board rename through the real tool surface.
//!
//! The service-level tests in `src/kanban/service_impl/tests.rs` pin the
//! trim/reject/round-trip semantics; these tests pin the *tool wiring* —
//! ownership enforcement and the response shape — because a perfectly
//! correct `board_rename` behind a tool that forgets the ownership check
//! would pass every service test and still let any caller rename any board
//! (reference model R6; `kask/docs/research/kanban-board-reference-models.md`
//! §6.3).

#![cfg(test)]

use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_kata_kanban::{KanbanServer, KanbanService};
use hkask_mcp_server::server::McpToolError;
use hkask_mcp_swarm::LocalAgentRegistry;
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::kanban_wire::KANBAN_BOARD_NAME_MAX_CHARS;
use hkask_types::{InferenceError, WebID, WorktreeSpawnPort};
use rmcp::handler::server::wrapper::Parameters;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Worktree-spawn stub for tests: returns an error so no test here can
/// accidentally exercise a spawn. Mirrors the stub in
/// `tests/idempotent_creates.rs`.
struct UnavailableWorktreeSpawn;

impl WorktreeSpawnPort for UnavailableWorktreeSpawn {
    fn create_worktree_thread<'a>(
        &'a self,
        _prompt: &'a str,
        _title: &'a str,
        _worktree_name: Option<&'a str>,
        _base_ref: Option<&'a str>,
        _allowed_tools: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<String, InferenceError>> + Send + 'a>> {
        Box::pin(async {
            Err(InferenceError::Connection(
                "worktree spawn unavailable (test stub)".into(),
            ))
        })
    }
}

/// Build a server whose caller identity is `caller`, over a store that may
/// already hold another owner's boards (shared driver) — so a test can pin
/// the ownership gate: a board created by caller A must not be renamable by
/// caller B.
fn server_over_shared_driver(
    driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
    caller: WebID,
) -> KanbanServer {
    let store = HMemStore::from_driver(driver).expect("hmem store init");
    let idempotency_driver = SqliteDriver::in_memory_driver();
    let idempotency = Arc::new(
        hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(idempotency_driver)
            .expect("idempotency schema"),
    );
    let goal_idempotency = Arc::clone(&idempotency);
    KanbanServer::new(
        caller,
        KanbanService::new(store),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(UnavailableWorktreeSpawn),
        idempotency,
        goal_idempotency,
    )
}

fn make_server() -> KanbanServer {
    let driver = SqliteDriver::in_memory_driver();
    server_over_shared_driver(driver, WebID::new())
}

fn parse(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output must be valid JSON")
}

async fn create_board(server: &KanbanServer, name: &str) -> serde_json::Value {
    let out = server
        .kanban_board_create(Parameters(BoardCreateRequest {
            name: name.to_string(),
            columns: None,
            idempotency_key: None,
        }))
        .await
        .expect("tool ok");
    parse(&out)
}

async fn board_names(server: &KanbanServer) -> Vec<String> {
    let out = server
        .kanban_board_list(Parameters(BoardListRequest {}))
        .await
        .expect("tool ok");
    parse(&out)
        .get("boards")
        .and_then(|b| b.as_array())
        .map(|boards| {
            boards
                .iter()
                .filter_map(|b| b["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| panic!("board_list did not return a boards array"))
}

#[tokio::test]
async fn rename_by_owner_round_trips_through_the_tool() {
    let server = make_server();
    let board = create_board(&server, "Original Name").await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let out = server
        .kanban_board_update(Parameters(BoardUpdateRequest {
            board_id: board_id.clone(),
            name: "  Renamed Board  ".into(),
        }))
        .await
        .expect("owner rename must succeed");
    let parsed = parse(&out);
    assert_eq!(
        parsed["name"].as_str(),
        Some("Renamed Board"),
        "the response must carry the trimmed new name"
    );
    assert_eq!(parsed["board_id"].as_str(), Some(board_id.as_str()));

    // The board list — what the panel and every agent read — shows the new
    // name, not the old one.
    assert_eq!(board_names(&server).await, ["Renamed Board"]);
}

#[tokio::test]
async fn rename_by_non_owner_is_permission_denied() {
    let driver = SqliteDriver::in_memory_driver();
    let owner = WebID::new();
    let owner_server = server_over_shared_driver(driver.clone(), owner);
    let board = create_board(&owner_server, "Owner's Board").await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    // A second caller over the same database: can see nothing of the owner's
    // boards, and must not be able to rename one.
    let other_server = server_over_shared_driver(driver, WebID::new());
    let err: McpToolError = other_server
        .kanban_board_update(Parameters(BoardUpdateRequest {
            board_id: board_id.clone(),
            name: "Hijacked Name".into(),
        }))
        .await
        .expect_err("a non-owner rename must fail");
    assert!(
        format!("{err:?}").contains("not owned by caller"),
        "the failure must name the ownership gate, got: {err:?}"
    );

    // The owner's board is untouched.
    assert_eq!(board_names(&owner_server).await, ["Owner's Board"]);
}

#[tokio::test]
async fn rename_to_whitespace_is_rejected_at_the_tool_seam() {
    let server = make_server();
    let board = create_board(&server, "Board").await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    let err: McpToolError = server
        .kanban_board_update(Parameters(BoardUpdateRequest {
            board_id,
            name: "   ".into(),
        }))
        .await
        .expect_err("whitespace-only names must be rejected (reference model R1)");
    assert!(
        format!("{err:?}").contains("board name is empty"),
        "the error must name the empty-name problem, got: {err:?}"
    );
}

#[tokio::test]
async fn rename_to_an_over_cap_name_is_rejected_at_the_tool_seam() {
    let server = make_server();
    let board = create_board(&server, "Board").await;
    let board_id = board["board_id"].as_str().expect("board_id").to_string();

    // The 128-character cap (reference model R1; §6.6) is enforced at the
    // service boundary the tool delegates to — the typed error surfaces
    // through the MCP seam instead of a silent truncation.
    let err: McpToolError = server
        .kanban_board_update(Parameters(BoardUpdateRequest {
            board_id,
            name: "x".repeat(KANBAN_BOARD_NAME_MAX_CHARS + 1),
        }))
        .await
        .expect_err("over-cap names must be rejected (reference model R1)");
    assert!(
        format!("{err:?}").contains("longer than 128"),
        "the error must name the cap, got: {err:?}"
    );

    // The board is untouched.
    assert_eq!(board_names(&server).await, ["Board"]);
}

#[tokio::test]
async fn rename_of_unknown_board_is_not_found() {
    let server = make_server();
    let err: McpToolError = server
        .kanban_board_update(Parameters(BoardUpdateRequest {
            board_id: "00000000-0000-0000-0000-000000000000".into(),
            name: "Ghost Board".into(),
        }))
        .await
        .expect_err("renaming a board that does not exist must fail");
    assert!(
        format!("{err:?}").to_lowercase().contains("not found"),
        "the error must identify the missing board, got: {err:?}"
    );
}
