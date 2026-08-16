//! QA contract tests for hkask-mcp-kata-kanban.
//!
//! Instantiates the 7-category contract from
//! kask/docs/qa/per-tool-contracts.md for every tool on the server.
//! Driven by kask/scripts/qa-mcp-servers.sh.
//!
//! Category 7 (adversarial) is N/A for all kata-kanban tools — none are
//! LLM I/O boundaries (the server is a pure state machine over sqlite).
//! Category 3 (dependency-denial) applies to all tools: the server declares
//! an optional HKASK_DB_PASSPHRASE credential (read via `ctx.credentials.get`)
//! and an optional HKASK_KANBAN_DB config path (read via `std::env::var`);
//! calling without them yields in-memory operation, not denial. The contract
//! therefore asserts the no-credential path returns structured errors
//! (not panics) and does NOT assert reg.outcome (Gap B — not wired).
//!
//! Each test constructs a fresh in-memory KanbanServer so tests are
//! independent and idempotent.

#![cfg(test)]

use hkask_mcp_kata_kanban::KanbanServer;
use hkask_mcp_kata_kanban::KanbanService;
use hkask_mcp_kata_kanban::TaskSpec;
use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_swarm::{LazyLocalSwarmRuntime, LocalAgentRegistry};
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::WebID;
use std::sync::Arc;

// ── Test harness ────────────────────────────────────────────────────────────

/// Build an in-memory KanbanServer. HMemStore::from_driver creates the
/// `hmems` table with the canonical schema, so no manual DDL is needed here.
fn make_server() -> KanbanServer {
    let driver = SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver).expect("hmem store init");
    let service = KanbanService::new(store);
    // Local swarm delegation surface. The ledger path is process-unique so
    // parallel test processes don't contend on the same SQLite file; only the
    // kanban_task_spawn happy path opens it (via get_or_init). The registry
    // points at a nonexistent dir so spawns build task-specific agents.
    let ledger_path = std::env::temp_dir()
        .join(format!("kata-kanban-spawn-{}.db", std::process::id()))
        .to_string_lossy()
        .to_string();
    let local_runtime = Arc::new(LazyLocalSwarmRuntime::lazy(ledger_path, None));
    let local_registry = Arc::new(LocalAgentRegistry::new("/nonexistent"));
    let worktree_spawn_port: Arc<dyn hkask_types::WorktreeSpawnPort> =
        Arc::new(hkask_inference::UnavailableWorktreeSpawn);
    KanbanServer::new(
        WebID::new(),
        service,
        local_runtime,
        local_registry,
        worktree_spawn_port,
        Arc::new(hkask_mcp_kata_kanban::idempotency::IdempotencyStore::default()),
        Arc::new(hkask_verification::VerificationStore::in_memory()),
    )
}

/// Parse a tool's JSON string response into a serde_json::Value.
///
/// Delegates to the canonical `hkask_types::tool_response::parse_tool_response`
/// helper so the `content` envelope is unwrapped the same way every consumer
/// does (`.rules`: do not re-implement the envelope unwrap locally).
fn parse(out: &str) -> serde_json::Value {
    hkask_types::tool_response::parse_tool_response(out).expect("tool output must be valid JSON")
}

/// Assert the response is a structured McpToolError with the given kind.
fn assert_error_kind(out: &str, expected_kind: &str) {
    let v = parse(out);
    let err = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("expected 'error' field, got: {out}"));
    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or_else(|| panic!("expected 'kind' field, got: {out}"));
    assert!(
        !err.is_empty(),
        "error message must not be empty, got: {out}"
    );
    assert_eq!(
        kind, expected_kind,
        "expected kind '{expected_kind}', got '{kind}' in: {out}"
    );
}

/// Create a board and return its id (string form) for use by task tools.
async fn make_board(server: &KanbanServer, name: &str) -> String {
    let req = BoardCreateRequest {
        name: name.to_string(),
        columns: None,
        idempotency_key: None,
    };
    let out = server
        .kanban_board_create(rmcp::handler::server::wrapper::Parameters(req))
        .await;
    let v = parse(&out);
    v.get("board_id")
        .and_then(|b| b.as_str())
        .unwrap_or_else(|| panic!("board_create did not return board_id: {out}"))
        .to_string()
}

/// Create a task and return its id (string form).
async fn make_task(server: &KanbanServer, board_id: &str, title: &str) -> String {
    let req = TaskCreateRequest {
        board_id: board_id.to_string(),
        title: title.to_string(),
        description: None,
        criteria: None,
        gas_budget: None,
        rjoule_budget: None,
        idempotency_key: None,
    };
    let out = server
        .kanban_task_create(rmcp::handler::server::wrapper::Parameters(req))
        .await;
    let v = parse(&out);
    v.get("task_id")
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("task_create did not return task_id: {out}"))
        .to_string()
}

// ── kanban_board_create ──────────────────────────────────────────────────────

mod board_create {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let req = BoardCreateRequest {
            name: "QA Board".to_string(),
            columns: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_board_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("board_id").is_some(), "missing board_id: {out}");
        assert_eq!(v.get("name").and_then(|n| n.as_str()), Some("QA Board"));
        let cols = v
            .get("columns")
            .and_then(|c| c.as_array())
            .expect("missing columns array");
        assert_eq!(cols.len(), 5, "default board should have 5 columns");
    }

    #[tokio::test]
    async fn schema_violation_missing_name() {
        // REQ: schema-violation (a) missing required field
        // rmcp Parameters<T> deserialization: a missing 'name' field fails
        // before the tool body runs. We simulate by sending JSON without it.
        // Construct raw JSON missing 'name' — serde will reject on the Parameters boundary.
        let raw = serde_json::json!({"columns": null});
        let result: Result<BoardCreateRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "missing 'name' must fail deserialization");
    }

    #[tokio::test]
    async fn schema_violation_wrong_type() {
        // REQ: schema-violation (b) wrong type
        let raw = serde_json::json!({"name": 123, "columns": null});
        let result: Result<BoardCreateRequest, _> = serde_json::from_value(raw);
        assert!(result.is_err(), "numeric name must fail deserialization");
    }

    #[tokio::test]
    async fn schema_violation_extra_unknown_field() {
        // REQ: schema-violation (c) extra unknown field — serde ignores by default
        let raw = serde_json::json!({"name": "X", "columns": null, "unknown_extra": 42});
        let result: Result<BoardCreateRequest, _> = serde_json::from_value(raw);
        assert!(result.is_ok(), "unknown fields should be ignored by serde");
    }

    #[tokio::test]
    async fn empty_result() {
        // REQ: empty-result — board_list on a fresh server returns empty
        let server = make_server();
        let out = server
            .kanban_board_list(rmcp::handler::server::wrapper::Parameters(
                BoardListRequest {},
            ))
            .await;
        let v = parse(&out);
        let boards = v
            .get("boards")
            .and_then(|b| b.as_array())
            .expect("missing boards array");
        assert!(boards.is_empty(), "fresh server should have no boards");
    }

    #[tokio::test]
    async fn error_propagation_invalid_status() {
        // REQ: error-propagation — invalid column status yields invalid_argument
        let server = make_server();
        let req = BoardCreateRequest {
            name: "Bad".to_string(),
            columns: Some(vec![ColumnDefInput {
                name: "X".to_string(),
                status: "not_a_status".to_string(),
                wip_limit: None,
            }]),
            idempotency_key: None,
        };
        let out = server
            .kanban_board_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }

    #[tokio::test]
    async fn resource_bounds_long_name() {
        // REQ: resource-bounds — a long board name is accepted (no panic)
        let server = make_server();
        let long_name = "x".repeat(10_000);
        let req = BoardCreateRequest {
            name: long_name.clone(),
            columns: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_board_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(
            v.get("name").and_then(|n| n.as_str()),
            Some(long_name.as_str())
        );
    }
}

// ── kanban_board_list ───────────────────────────────────────────────────────

mod board_list {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let _bid = make_board(&server, "B1").await;
        let out = server
            .kanban_board_list(rmcp::handler::server::wrapper::Parameters(
                BoardListRequest {},
            ))
            .await;
        let v = parse(&out);
        let boards = v
            .get("boards")
            .and_then(|b| b.as_array())
            .expect("missing boards array");
        assert_eq!(boards.len(), 1);
    }

    #[tokio::test]
    async fn empty_result() {
        // REQ: empty-result
        let server = make_server();
        let out = server
            .kanban_board_list(rmcp::handler::server::wrapper::Parameters(
                BoardListRequest {},
            ))
            .await;
        let v = parse(&out);
        assert_eq!(
            v.get("boards")
                .and_then(|b| b.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            0
        );
    }
}

// ── kanban_task_create ──────────────────────────────────────────────────────

mod task_create {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let req = TaskCreateRequest {
            board_id: bid,
            title: "T1".to_string(),
            description: None,
            criteria: None,
            gas_budget: None,
            rjoule_budget: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_task_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("task_id").is_some());
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("backlog"));
    }

    #[tokio::test]
    async fn error_propagation_bad_board_id() {
        // REQ: error-propagation — invalid board_id format
        let server = make_server();
        let req = TaskCreateRequest {
            board_id: "not-a-uuid".to_string(),
            title: "T".to_string(),
            description: None,
            criteria: None,
            gas_budget: None,
            rjoule_budget: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_task_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_board() {
        // REQ: error-propagation — valid-format but nonexistent board
        let server = make_server();
        let req = TaskCreateRequest {
            board_id: "00000000-0000-0000-0000-000000000000".to_string(),
            title: "T".to_string(),
            description: None,
            criteria: None,
            gas_budget: None,
            rjoule_budget: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_task_create(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        // board doesn't exist → NotFound
        let v = parse(&out);
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_else(|| panic!("expected kind, got: {out}"));
        assert!(
            kind == "not_found" || kind == "invalid_argument",
            "expected not_found or invalid_argument, got '{kind}' in: {out}"
        );
    }
}

// ── kanban_task_list ────────────────────────────────────────────────────────

mod task_list {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let _t = make_task(&server, &bid, "T1").await;
        let req = TaskListRequest {
            board_id: bid,
            status: None,
        };
        let out = server
            .kanban_task_list(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        let tasks = v
            .get("tasks")
            .and_then(|t| t.as_array())
            .expect("missing tasks array");
        assert_eq!(tasks.len(), 1);
    }

    #[tokio::test]
    async fn carries_activity_after_comment() {
        // R3: kanban_task_list surfaces the latest comment as `activity` on each
        // TaskInfo so the widget can render a one-line status strip on the card.
        // R1: the response also carries a `swarm_id` field (null when the task is
        // not scoped to a swarm) so the widget can render the swarm link.
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T1").await;
        // Add a comment — the server derives `activity` from the latest comment.
        server
            .kanban_task_comment(rmcp::handler::server::wrapper::Parameters(
                TaskCommentRequest {
                    task_id: tid.clone(),
                    body: "Spawn executed: agent=beta, tokens=120".to_string(),
                },
            ))
            .await;
        let req = TaskListRequest {
            board_id: bid,
            status: None,
        };
        let out = server
            .kanban_task_list(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        let task = v
            .get("tasks")
            .and_then(|t| t.as_array())
            .and_then(|a| a.first())
            .expect("missing tasks array entry");
        // R1: `swarm_id` is omitted (skip_serializing_if) when the task is not
        // spawned under a swarm. The positive case (present when set) is pinned
        // by the spawn_task lib tests (C2) + the TaskInfo mapping reads
        // `t.swarm_id.clone()`.
        assert!(
            task.get("swarm_id").is_none(),
            "swarm_id must be absent when the task is not spawned under a swarm"
        );
        // R3: the `activity` field is populated from the latest comment.
        let activity = task
            .get("activity")
            .expect("TaskInfo must carry `activity` after a comment (R3)");
        assert_eq!(
            activity.get("kind").and_then(|k| k.as_str()),
            Some("comment")
        );
        assert_eq!(
            activity.get("text").and_then(|t| t.as_str()),
            Some("Spawn executed: agent=beta, tokens=120")
        );
        assert!(
            activity
                .get("at")
                .and_then(|a| a.as_str())
                .is_some_and(|s| !s.is_empty()),
            "activity.at must be a non-empty ISO timestamp"
        );
    }

    #[tokio::test]
    async fn empty_result() {
        // REQ: empty-result — board with no tasks
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let req = TaskListRequest {
            board_id: bid,
            status: None,
        };
        let out = server
            .kanban_task_list(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(
            v.get("tasks")
                .and_then(|t| t.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            0
        );
    }

    #[tokio::test]
    async fn error_propagation_bad_status_filter() {
        // REQ: error-propagation — invalid status filter
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let req = TaskListRequest {
            board_id: bid,
            status: Some("not_a_status".to_string()),
        };
        let out = server
            .kanban_task_list(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_move ────────────────────────────────────────────────────────

mod task_move {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — Backlog → Ready
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskMoveRequest {
            task_id: tid.clone(),
            target_status: "Ready".to_string(),
        };
        let out = server
            .kanban_task_move(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("new_status").and_then(|s| s.as_str()), Some("ready"));
    }

    #[tokio::test]
    async fn error_propagation_invalid_transition() {
        // REQ: error-propagation — Done → Backlog is not a valid transition
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        // Move to Done first (Backlog → Ready → InProgress → Review → Done)
        for status in ["Ready", "InProgress", "Review", "Done"] {
            let req = TaskMoveRequest {
                task_id: tid.clone(),
                target_status: status.to_string(),
            };
            let _ = server
                .kanban_task_move(rmcp::handler::server::wrapper::Parameters(req))
                .await;
        }
        // Now try Done → Backlog (invalid)
        let req = TaskMoveRequest {
            task_id: tid,
            target_status: "Backlog".to_string(),
        };
        let out = server
            .kanban_task_move(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_else(|| panic!("expected kind, got: {out}"));
        assert!(
            kind == "failed_precondition" || kind == "invalid_argument",
            "expected failed_precondition or invalid_argument, got '{kind}' in: {out}"
        );
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_task() {
        // REQ: error-propagation
        let server = make_server();
        let req = TaskMoveRequest {
            task_id: "00000000-0000-0000-0000-000000000000".to_string(),
            target_status: "Ready".to_string(),
        };
        let out = server
            .kanban_task_move(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "not_found");
    }
}

// ── kanban_task_assign ──────────────────────────────────────────────────────

mod task_assign {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAssignRequest { task_id: tid };
        let out = server
            .kanban_task_assign(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("assignee").is_some());
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_task() {
        // REQ: error-propagation
        let server = make_server();
        let req = TaskAssignRequest {
            task_id: "00000000-0000-0000-0000-000000000000".to_string(),
        };
        let out = server
            .kanban_task_assign(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "not_found");
    }
}

// ── kanban_task_verify ──────────────────────────────────────────────────────

mod task_verify {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — verify moves Review → Done, so move to Review first
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        for status in ["Ready", "InProgress", "Review"] {
            let _ = server
                .kanban_task_move(rmcp::handler::server::wrapper::Parameters(
                    TaskMoveRequest {
                        task_id: tid.clone(),
                        target_status: status.to_string(),
                    },
                ))
                .await;
        }
        let req = TaskVerifyRequest {
            task_id: tid,
            evidence: "tests pass".to_string(),
        };
        let out = server
            .kanban_task_verify(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("passed").is_some(), "missing passed: {out}");
        assert!(v.get("reasoning").is_some());
    }

    #[tokio::test]
    async fn schema_violation_empty_evidence() {
        // REQ: schema-violation — empty evidence is rejected by the tool body
        // (before the state-transition check, so Backlog is fine here)
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskVerifyRequest {
            task_id: tid,
            evidence: "   ".to_string(),
        };
        let out = server
            .kanban_task_verify(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_add_gas ────────────────────────────────────────────────────

mod task_add_gas {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddGasRequest {
            task_id: tid,
            amount: 1000,
        };
        let out = server
            .kanban_task_add_gas(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("new_gas_remaining").is_some());
    }

    #[tokio::test]
    async fn schema_violation_zero_amount() {
        // REQ: schema-violation — amount must be > 0
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddGasRequest {
            task_id: tid,
            amount: 0,
        };
        let out = server
            .kanban_task_add_gas(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_add_rjoules ────────────────────────────────────────────────

mod task_add_rjoules {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddRjoulesRequest {
            task_id: tid,
            amount: 5000,
        };
        let out = server
            .kanban_task_add_rjoules(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("new_rjoule_remaining").is_some());
    }

    #[tokio::test]
    async fn schema_violation_zero_amount() {
        // REQ: schema-violation — amount must be > 0
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddRjoulesRequest {
            task_id: tid,
            amount: 0,
        };
        let out = server
            .kanban_task_add_rjoules(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_comment ────────────────────────────────────────────────────

mod task_comment {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskCommentRequest {
            task_id: tid,
            body: "looks good".to_string(),
        };
        let out = server
            .kanban_task_comment(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("comment_id").is_some());
        assert_eq!(v.get("body").and_then(|b| b.as_str()), Some("looks good"));
    }

    #[tokio::test]
    async fn schema_violation_empty_body() {
        // REQ: schema-violation
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskCommentRequest {
            task_id: tid,
            body: "  ".to_string(),
        };
        let out = server
            .kanban_task_comment(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_comments_since ─────────────────────────────────────────────

mod task_comments_since {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        // Add a comment first
        let _ = server
            .kanban_task_comment(rmcp::handler::server::wrapper::Parameters(
                TaskCommentRequest {
                    task_id: tid.clone(),
                    body: "first".to_string(),
                },
            ))
            .await;
        let req = TaskCommentsSinceRequest {
            task_id: tid,
            since_index: 0,
        };
        let out = server
            .kanban_task_comments_since(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        let comments = v
            .get("comments")
            .and_then(|c| c.as_array())
            .expect("missing comments array");
        assert_eq!(comments.len(), 1);
        assert_eq!(
            v.get("total_count").and_then(|t| t.as_u64()),
            Some(1),
            "total_count must be 1"
        );
    }

    #[tokio::test]
    async fn empty_result() {
        // REQ: empty-result — no comments on the task
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskCommentsSinceRequest {
            task_id: tid,
            since_index: 0,
        };
        let out = server
            .kanban_task_comments_since(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(
            v.get("comments")
                .and_then(|c| c.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            0
        );
    }
}

// ── kanban_task_add_deliverable ────────────────────────────────────────────

mod task_add_deliverable {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddDeliverableRequest {
            task_id: tid,
            path: "/tmp/output.md".to_string(),
        };
        let out = server
            .kanban_task_add_deliverable(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(v.get("deliverable_count").and_then(|d| d.as_u64()), Some(1));
    }

    #[tokio::test]
    async fn schema_violation_empty_path() {
        // REQ: schema-violation
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskAddDeliverableRequest {
            task_id: tid,
            path: "  ".to_string(),
        };
        let out = server
            .kanban_task_add_deliverable(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }
}

// ── kanban_task_reopen ─────────────────────────────────────────────────────

mod task_reopen {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — move to Done then reopen
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        for status in ["Ready", "InProgress", "Review", "Done"] {
            let _ = server
                .kanban_task_move(rmcp::handler::server::wrapper::Parameters(
                    TaskMoveRequest {
                        task_id: tid.clone(),
                        target_status: status.to_string(),
                    },
                ))
                .await;
        }
        let req = TaskReopenRequest {
            task_id: tid,
            gas_budget: Some(500),
            rjoule_budget: None,
        };
        let out = server
            .kanban_task_reopen(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert_eq!(
            v.get("new_status").and_then(|s| s.as_str()),
            Some("in_progress"),
            "reopen should move Done → InProgress"
        );
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_task() {
        // REQ: error-propagation
        let server = make_server();
        let req = TaskReopenRequest {
            task_id: "00000000-0000-0000-0000-000000000000".to_string(),
            gas_budget: None,
            rjoule_budget: None,
        };
        let out = server
            .kanban_task_reopen(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "not_found");
    }
}

// ── kanban_task_kata_coaching ──────────────────────────────────────────────

mod task_kata_coaching {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskKataCoachingRequest { task_id: tid };
        let out = server
            .kanban_task_kata_coaching(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("prompt").is_some(), "missing prompt: {out}");
        assert!(v.get("task_id").is_some());
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_task() {
        // REQ: error-propagation
        let server = make_server();
        let req = TaskKataCoachingRequest {
            task_id: "00000000-0000-0000-0000-000000000000".to_string(),
        };
        let out = server
            .kanban_task_kata_coaching(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "not_found");
    }
}

// ── kanban_task_kata_improvement ───────────────────────────────────────────

mod task_kata_improvement {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskKataImprovementRequest { task_id: tid };
        let out = server
            .kanban_task_kata_improvement(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("prompt").is_some(), "missing prompt: {out}");
    }
}

// ── kanban_task_kata_practice ──────────────────────────────────────────────

mod task_kata_practice {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskKataPracticeRequest {
            task_id: tid,
            sub_problem: "test isolation".to_string(),
        };
        let out = server
            .kanban_task_kata_practice(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        let v = parse(&out);
        assert!(v.get("prompt").is_some(), "missing prompt: {out}");
    }
}

// ── kanban_task_spawn ──────────────────────────────────────────────────────

mod task_spawn {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // The spawn delegates to the local swarm runtime. In the unit-test
        // environment there is no inference socket, so the delegation fails at
        // the inference call with `unavailable` — which proves the spawn reaches
        // the real delegation path, not a static comment. The full happy path
        // (live inference) is an integration test, not a unit test.
        //
        // This previously asserted `permission_denied`, because an unfunded local
        // ledger refused the delegation before it ever attempted inference. That
        // funding gate was removed: local agents run on the operator's own
        // substrate, so there is nothing for the server to withhold (see
        // `LocalSwarmRuntime::delegate`). An unfunded ledger must no longer
        // change the outcome here — `spawn_is_not_blocked_by_an_unfunded_ledger`
        // in `tests/idempotent_creates.rs` pins that directly.
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let tid = make_task(&server, &bid, "T").await;
        let req = TaskSpawnRequest {
            task_id: tid,
            delegation_level: "standard".to_string(),
            delegated_skills: vec!["tdd".to_string()],
            memory_scope: Some("episodic".to_string()),
            gas_budget: Some(1000),
            rjoule_budget: None,
            swarm_id: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_task_spawn(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        // Reaches inference and fails there (no IPC socket in a unit test).
        assert_error_kind(&out, "unavailable");
        // Guard the regression directly: a funding refusal here would mean the
        // local gate came back.
        assert!(
            !out.contains("insufficient local credits") && !out.contains("swarm_fund_local"),
            "local spawn must not be gated on ledger funds; got: {out}"
        );
    }

    #[tokio::test]
    async fn error_propagation_nonexistent_task() {
        // REQ: error-propagation
        let server = make_server();
        let req = TaskSpawnRequest {
            task_id: "00000000-0000-0000-0000-000000000000".to_string(),
            delegation_level: "standard".to_string(),
            delegated_skills: vec![],
            memory_scope: None,
            gas_budget: None,
            rjoule_budget: None,
            swarm_id: None,
            idempotency_key: None,
        };
        let out = server
            .kanban_task_spawn(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "not_found");
    }
}

// ── contract_propose_expect ────────────────────────────────────────────────

mod contract_propose_expect {
    use super::*;

    #[tokio::test]
    async fn happy() {
        // REQ: happy — empty proposals array creates zero tasks (no-op success)
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let req = ContractProposeExpect {
            board_id: bid,
            proposals: serde_json::json!([]).into(),
        };
        let out = server
            .contract_propose_expect(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        // Either success with empty created list, or invalid_argument if the
        // tool rejects empty proposals. Both are non-panic structured responses.
        let v = parse(&out);
        assert!(
            v.get("error").is_some() || v.get("created").is_some() || v.get("task_ids").is_some(),
            "expected structured response (success or error), got: {out}"
        );
    }

    #[tokio::test]
    async fn error_propagation_bad_board_id() {
        // REQ: error-propagation
        let server = make_server();
        let req = ContractProposeExpect {
            board_id: "not-a-uuid".to_string(),
            proposals: serde_json::json!([]).into(),
        };
        let out = server
            .contract_propose_expect(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        assert_error_kind(&out, "invalid_argument");
    }

    #[tokio::test]
    async fn schema_violation_malformed_proposals_json() {
        // REQ: schema-violation — proposals is not a valid ExpectProposal array
        let server = make_server();
        let bid = make_board(&server, "B").await;
        let req = ContractProposeExpect {
            board_id: bid,
            proposals: serde_json::json!("not an ExpectProposal array").into(),
        };
        let out = server
            .contract_propose_expect(rmcp::handler::server::wrapper::Parameters(req))
            .await;
        // A non-array proposals value should produce a structured error, not a panic.
        let v = parse(&out);
        assert!(
            v.get("error").is_some(),
            "malformed proposals must produce a structured error, got: {out}"
        );
    }

    // ── PKO ontology anchoring ─────────────────────────────────────────────
    //
    // Boards and tasks are pure PKO: a board is a `pko:Procedure`, a task is a
    // `pko:Step`. The three `HMem::new` write paths in `KanbanService` must
    // anchor their h_mems so `query_by_pko_procedure(board_id)` reaches them.
    // Without anchoring the h_mems are unreachable via the process-axis query
    // (the `.rules` "Ontology tag field-drop trap" — the ontology blob must be
    // set at write time, not deferred).

    /// A board's h_mem is anchored as a `pko:Procedure` and reachable via
    /// `query_by_pko_procedure(board_id)`.
    #[test]
    fn board_h_mem_anchored_as_pko_procedure() {
        let driver = SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver).expect("hmem store init");
        let query_store = store.clone();
        let service = KanbanService::new(store);
        let owner = WebID::new();

        let board = service
            .board_create(owner, "Test Board", &KanbanService::standard_columns())
            .expect("board_create");

        let anchored = query_store
            .query_by_pko_procedure(&board.id.to_string())
            .expect("query_by_pko_procedure");
        assert_eq!(
            anchored.len(),
            1,
            "board h_mem should be reachable via query_by_pko_procedure"
        );
        let ont = anchored[0]
            .ontology
            .as_ref()
            .expect("board h_mem must carry an ontology blob");
        assert_eq!(ont.dc_type, "pko:Procedure");
        assert_eq!(ont.pko_procedure, Some(board.id.to_string()));
        assert!(ont.pko_step.is_none(), "board is the procedure, not a step");
    }

    /// A task's h_mem and its board→task index h_mem are both anchored as
    /// `pko:Step` under the board's procedure and reachable via
    /// `query_by_pko_procedure(board_id)`.
    #[test]
    fn task_and_index_h_mems_anchored_as_pko_steps() {
        let driver = SqliteDriver::in_memory_driver();
        let store = HMemStore::from_driver(driver).expect("hmem store init");
        let query_store = store.clone();
        let service = KanbanService::new(store);
        let owner = WebID::new();

        let board = service
            .board_create(owner, "Test Board", &KanbanService::standard_columns())
            .expect("board_create");
        let task = service
            .task_create(board.id, TaskSpec::new("Test Task".to_string()), owner)
            .expect("task_create");

        // query_by_pko_procedure reaches the board (Procedure) + task (Step)
        // + index (Step) = 3 h_mems.
        let anchored = query_store
            .query_by_pko_procedure(&board.id.to_string())
            .expect("query_by_pko_procedure");
        assert_eq!(
            anchored.len(),
            3,
            "board + task + index h_mems should all be reachable via query_by_pko_procedure"
        );

        // Every anchored h_mem must carry a PKO procedure matching the board id.
        for h_mem in &anchored {
            let ont = h_mem
                .ontology
                .as_ref()
                .expect("every kanban h_mem must carry an ontology blob");
            assert_eq!(
                ont.pko_procedure,
                Some(board.id.to_string()),
                "h_mem entity {} not anchored to board procedure",
                h_mem.entity
            );
        }

        // The task h_mem itself must be reachable (entity = TASK_ENTITY).
        let task_h_mem = anchored
            .iter()
            .find(|h| h.entity == "kanban:task" && h.attribute == task.id.to_string())
            .expect("task h_mem must be anchored and reachable");
        let task_ont = task_h_mem.ontology.as_ref().expect("task ontology");
        assert_eq!(task_ont.dc_type, "pko:StepExecution");
        assert_eq!(task_ont.pko_step, Some(task.id.to_string()));
    }
}
