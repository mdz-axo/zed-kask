//! Task deletion through the real tool surface.
//!
//! Mirrors tests/board_rename.rs: the service-level tests pin the
//! stranger-refusal semantics (the `delete` arm of the parametrized
//! task-mutation test in `src/kanban/service_impl/tests.rs`); these tests pin
//! the *tool wiring* — `kanban_task_delete` must thread the caller and
//! authorize before destroying, because a correct service check behind a
//! tool that forgets the caller would pass every service test and let any
//! caller delete any task over a shared database (P12; found by the
//! security-audit skill's trial, card 8f69c30d).

#![cfg(test)]

use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_kata_kanban::{KanbanServer, KanbanService};
use hkask_mcp_server::server::McpToolError;
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::WebID;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

use hkask_mcp_kata_kanban::test_support::{UnavailableWorktreeSpawn, parse};

/// Build a server whose caller identity is `caller`, over a store that may
/// already hold another owner's data (shared driver) — so a test can pin the
/// ownership gate: a task created by caller A must not be deletable by
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
        Arc::new(UnavailableWorktreeSpawn),
        idempotency,
        goal_idempotency,
    )
}

async fn create_task(server: &KanbanServer, board_id: &str, title: &str) -> serde_json::Value {
    let out = server
        .kanban_task_create(Parameters(TaskCreateRequest {
            board_id: board_id.to_string(),
            title: title.to_string(),
            description: None,
            criteria: None,
            advances: Vec::new(),
            idempotency_key: None,
        }))
        .await
        .expect("task ok");
    parse(&out)
}

#[tokio::test]
async fn delete_by_non_owner_is_permission_denied_and_preserves_the_task() {
    let driver = SqliteDriver::in_memory_driver();
    let owner = WebID::new();
    let owner_server = server_over_shared_driver(driver.clone(), owner);
    let board = owner_server
        .kanban_board_create(Parameters(BoardCreateRequest {
            name: "Owner's Board".to_string(),
            columns: None,
            idempotency_key: None,
            done_requires_verification: None,
        }))
        .await
        .expect("board create");
    let board = parse(&board);
    let board_id = board["board_id"].as_str().expect("board_id").to_string();
    let created = create_task(&owner_server, &board_id, "Owner's Task").await;
    let task_id = created["task_id"].as_str().expect("task_id").to_string();

    // A second caller over the same database must not be able to delete the
    // owner's task.
    let other_server = server_over_shared_driver(driver, WebID::new());
    let err: McpToolError = other_server
        .kanban_task_delete(Parameters(TaskDeleteRequest {
            task_id: task_id.clone(),
        }))
        .await
        .expect_err("non-owner delete must fail");
    assert!(
        format!("{err:?}").to_lowercase().contains("permission"),
        "authorization failure, got: {err:?}"
    );

    // The owner still deletes it cleanly — and the successful owner delete
    // also proves the refused delete preserved the task: had the stranger's
    // call destroyed it, this call would surface NotFound instead.
    let deleted = owner_server
        .kanban_task_delete(Parameters(TaskDeleteRequest { task_id }))
        .await
        .expect("owner delete — also proves the refused delete preserved the task");
    parse(&deleted);
}
