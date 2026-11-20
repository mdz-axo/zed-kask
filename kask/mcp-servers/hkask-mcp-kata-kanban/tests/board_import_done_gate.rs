//! S049-F02 regression suite: `kanban_board_export` and `kanban_board_import`
//! preserve structural `done_requires_verification` policy across round-trips
//! and refuse importing unverified pre-Done tasks onto verification-gated boards.

#![cfg(test)]

use hkask_mcp_kata_kanban::test_support::{UnavailableWorktreeSpawn, parse};
use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_kata_kanban::{KanbanServer, KanbanService};
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::{McpErrorKind, WebID};
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

fn make_server() -> KanbanServer {
    let driver = SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver).expect("hmem store init");
    let idempotency = Arc::new(
        hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(
            SqliteDriver::in_memory_driver(),
        )
        .expect("idempotency schema"),
    );
    let goal_idempotency = Arc::clone(&idempotency);
    KanbanServer::new(
        WebID::new(),
        KanbanService::new(store),
        Arc::new(UnavailableWorktreeSpawn),
        idempotency,
        goal_idempotency,
    )
}

#[tokio::test]
async fn export_import_round_trips_done_requires_verification_and_enforces_done_gate() {
    let server = make_server();

    let created = parse(
        &server
            .kanban_board_create(Parameters(BoardCreateRequest {
                name: "Release Gate".into(),
                columns: None,
                idempotency_key: None,
                done_requires_verification: Some(true),
            }))
            .await
            .expect("create gated board"),
    );
    let board_id = created["board_id"].as_str().expect("board_id").to_string();

    // Add a task and advance it to Review (Backlog -> Ready -> InProgress -> Review).
    let task = parse(
        &server
            .kanban_task_create(Parameters(TaskCreateRequest {
                board_id: board_id.clone(),
                title: "Audit release checklist".into(),
                description: None,
                criteria: None,
                advances: Vec::new(),
                idempotency_key: None,
            }))
            .await
            .expect("create task"),
    );
    let task_id = task["task_id"].as_str().expect("task_id").to_string();
    for status in ["ready", "in_progress", "review"] {
        server
            .kanban_task_move(Parameters(TaskMoveRequest {
                task_id: task_id.clone(),
                target_status: status.into(),
            }))
            .await
            .expect("move to review");
    }

    let exported = parse(
        &server
            .kanban_board_export(Parameters(BoardExportRequest { board_id }))
            .await
            .expect("export"),
    );
    let markdown = exported["markdown"].as_str().expect("markdown").to_string();
    assert!(
        markdown.contains("%% kanban board done_requires_verification: true"),
        "exported mermaid must carry done_requires_verification header, got:\n{markdown}"
    );

    // Round-trip via kanban_board_import (deserialize from JSON so it works with or without optional field).
    let import_req: BoardImportRequest = serde_json::from_value(serde_json::json!({
        "markdown": markdown,
        "board_name": "Release Gate Copy",
    }))
    .expect("deserialize import request");
    let imported = parse(
        &server
            .kanban_board_import(Parameters(import_req))
            .await
            .expect("import gated board"),
    );
    let imported_board_id = imported["board_id"].as_str().expect("board_id").to_string();

    // Board list must show done_requires_verification = true on the imported board.
    let boards = parse(
        &server
            .kanban_board_list(Parameters(BoardListRequest {}))
            .await
            .expect("board_list"),
    );
    let imported_entry = boards["boards"]
        .as_array()
        .expect("boards")
        .iter()
        .find(|b| b["board_id"].as_str() == Some(imported_board_id.as_str()))
        .expect("imported board in list");
    assert_eq!(
        imported_entry["done_requires_verification"],
        serde_json::json!(true),
        "imported board must retain done_requires_verification: {imported_entry}"
    );

    // Moving the imported Review task directly to Done must be refused!
    let tasks = parse(
        &server
            .kanban_task_list(Parameters(TaskListRequest {
                board_id: imported_board_id,
                status: None,
                view: None,
            }))
            .await
            .expect("task_list"),
    );
    let imported_task_id = tasks["tasks"][0]["task_id"]
        .as_str()
        .expect("imported task id")
        .to_string();
    let move_err = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: imported_task_id,
            target_status: "done".into(),
        }))
        .await
        .expect_err("direct move to Done on imported gated board must be refused");
    assert_eq!(move_err.kind, McpErrorKind::FailedPrecondition);
}

#[tokio::test]
async fn import_refuses_pre_done_tasks_on_algedonic_or_verification_gated_board() {
    let server = make_server();

    let markdown_with_done_task = "\
```mermaid
kanban
%% kanban column status: backlog
  section Backlog
%% kanban column status: review
  section Review
%% kanban column status: done
  section Done
    Unverified smuggled task
```";

    // 1) Reserved "Algedonic review" name refuses pre-Done tasks.
    let req_algedonic: BoardImportRequest = serde_json::from_value(serde_json::json!({
        "markdown": markdown_with_done_task,
        "board_name": "Algedonic review",
    }))
    .expect("deserialize");
    let err = server
        .kanban_board_import(Parameters(req_algedonic))
        .await
        .expect_err("Algedonic review import must refuse pre-Done tasks");
    assert_eq!(err.kind, McpErrorKind::InvalidArgument);

    // 2) Mermaid carrying done_requires_verification: true also refuses pre-Done tasks.
    let gated_markdown = "\
```mermaid
kanban
%% kanban board: Custom Gate
%% kanban board done_requires_verification: true
%% kanban column status: backlog
  section Backlog
%% kanban column status: done
  section Done
    Unverified smuggled task
```";
    let req_header: BoardImportRequest = serde_json::from_value(serde_json::json!({
        "markdown": gated_markdown,
    }))
    .expect("deserialize");
    let err2 = server
        .kanban_board_import(Parameters(req_header))
        .await
        .expect_err("header-gated import must refuse pre-Done tasks");
    assert_eq!(err2.kind, McpErrorKind::InvalidArgument);

    // Nothing was created in the store.
    let boards = parse(
        &server
            .kanban_board_list(Parameters(BoardListRequest {}))
            .await
            .expect("board_list"),
    );
    assert_eq!(boards["boards"].as_array().expect("boards").len(), 0);
}
