//! Tool-seam pins for the kanzo-benchmark proposals' new wire surface
//! (P1/P2/P3): the change counter on the board list, the compact listing
//! view, and the claim-next pull — driven through the real `Parameters<T>`
//! seam per the testing standard
//! (`kask/docs/reference/mcp-servers/README.md` §Testing standard). The
//! service pins in `service_impl/tests.rs` cover the guards; these prove
//! the wire the panel and agents actually see.

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

fn make_server() -> KanbanServer {
    let driver = SqliteDriver::in_memory_driver();
    let idempotency = Arc::new(
        hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(Arc::clone(&driver))
            .expect("idempotency schema"),
    );
    KanbanServer::new(
        WebID::new(),
        KanbanService::new(HMemStore::from_driver(driver).expect("hmem store")),
        Arc::new(UnavailableWorktreeSpawn),
        Arc::clone(&idempotency),
        idempotency,
    )
}

async fn create_board(server: &KanbanServer, name: &str, two_column: bool) -> String {
    let columns = two_column.then(|| {
        vec![
            ColumnDefInput {
                name: "Backlog".into(),
                status: "backlog".into(),
                wip_limit: None,
            },
            ColumnDefInput {
                name: "Done".into(),
                status: "done".into(),
                wip_limit: None,
            },
        ]
    });
    let out = server
        .kanban_board_create(Parameters(BoardCreateRequest {
            name: name.to_string(),
            columns,
            idempotency_key: None,
            done_requires_verification: None,
        }))
        .await
        .expect("board create ok");
    parse(&out)["board_id"]
        .as_str()
        .expect("board_id in response")
        .to_string()
}

async fn create_task(server: &KanbanServer, board_id: &str, title: &str) -> String {
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
        .expect("task create ok");
    parse(&out)["task_id"]
        .as_str()
        .expect("task_id in response")
        .to_string()
}

async fn move_task(server: &KanbanServer, task_id: &str, target: &str) {
    let out = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.to_string(),
            target_status: target.to_string(),
        }))
        .await
        .expect("move ok");
    parse(&out);
}

async fn list_compact(server: &KanbanServer, board_id: &str) -> serde_json::Value {
    let out = server
        .kanban_task_list(Parameters(TaskListRequest {
            board_id: board_id.to_string(),
            status: None,
            view: Some("compact".into()),
        }))
        .await
        .expect("compact list ok");
    parse(&out)
}

async fn claim_next(
    server: &KanbanServer,
    board_id: &str,
    key: Option<&str>,
) -> Result<serde_json::Value, McpToolError> {
    let out = server
        .kanban_task_claim_next(Parameters(ClaimNextRequest {
            board_id: board_id.to_string(),
            idempotency_key: key.map(str::to_string),
        }))
        .await?;
    Ok(parse(&out))
}

/// P1: the board list carries the change counter on the wire — the
/// panel's cheap-poll signal — and it moves on a write.
#[tokio::test]
async fn board_list_carries_the_change_counter() {
    let server = make_server();
    let board_id = create_board(&server, "Counter Board", false).await;

    let out = server
        .kanban_board_list(Parameters(BoardListRequest {}))
        .await
        .expect("board list ok");
    let before = parse(&out)["changes"]
        .as_u64()
        .expect("changes on the wire");

    create_task(&server, &board_id, "Moves the counter").await;

    let out = server
        .kanban_board_list(Parameters(BoardListRequest {}))
        .await
        .expect("board list ok");
    let after = parse(&out)["changes"]
        .as_u64()
        .expect("changes on the wire");
    assert!(
        after > before,
        "a write moves the wire counter: {before} -> {after}"
    );
}

/// P2: the compact view caps Done at 20 with the hidden count disclosed,
/// skips operator-reserved tasks with their count disclosed, omits the
/// heavy work context (descriptions, criteria), and never reports a
/// truncated listing as complete. The full view stays byte-compatible
/// for the panel: every task, none of the compact fields.
#[tokio::test]
async fn compact_view_caps_done_skips_reserved_and_discloses() {
    let server = make_server();
    // A two-column board (Backlog, Done): every task reaches Done in one
    // column-ordered move, so 22 done tasks cost 22 moves, not 88.
    let board_id = create_board(&server, "Compact Board", true).await;

    for i in 0..22 {
        let task_id = create_task(&server, &board_id, &format!("Done {i}")).await;
        move_task(&server, &task_id, "done").await;
    }
    let _open_id = create_task(&server, &board_id, "Open task").await;

    // An operator-reserved task the compact view must skip and disclose.
    let reserved_id = create_task(&server, &board_id, "Operator's own").await;
    let out = server
        .kanban_task_update(Parameters(TaskUpdateRequest {
            task_id: reserved_id,
            title: None,
            description: None,
            criteria: None,
            priority: None,
            labels: None,
            advances: None,
            reserved: Some(true),
        }))
        .await
        .expect("reserve ok");
    parse(&out);

    let compact = list_compact(&server, &board_id).await;
    let entries = compact["compact_tasks"]
        .as_array()
        .expect("compact entries");
    assert_eq!(
        entries.len(),
        21,
        "1 open + 20 capped done tasks (newest kept): {compact}"
    );
    assert_eq!(
        compact["done_not_shown"].as_u64(),
        Some(2),
        "the hidden done count is disclosed: {compact}"
    );
    assert_eq!(
        compact["reserved_not_shown"].as_u64(),
        Some(1),
        "the reserved skip is disclosed: {compact}"
    );
    assert!(
        compact["tasks"]
            .as_array()
            .is_none_or(|tasks| tasks.is_empty()),
        "the compact view carries no full TaskInfo objects: {compact}"
    );
    for entry in entries {
        assert!(
            entry.get("description").is_none(),
            "compact entries carry no descriptions: {entry}"
        );
        assert!(
            entry.get("criteria").is_none(),
            "compact entries carry no criteria: {entry}"
        );
    }

    // The full view is unchanged for the panel: all 24 tasks, none of the
    // compact fields.
    let out = server
        .kanban_task_list(Parameters(TaskListRequest {
            board_id: board_id.clone(),
            status: None,
            view: None,
        }))
        .await
        .expect("full list ok");
    let full = parse(&out);
    assert_eq!(
        full["tasks"].as_array().expect("full tasks").len(),
        24,
        "every task, reserved included, in the full view: {full}"
    );
    assert!(
        full.get("compact_tasks").is_none(),
        "the full view carries no compact entries: {full}"
    );
    assert!(
        full.get("done_not_shown").is_none() && full.get("reserved_not_shown").is_none(),
        "the full view carries no disclosure fields: {full}"
    );

    // P2 acceptance: the compact wire is materially smaller than the full
    // wire for the same board — a measured floor evaluated on the real
    // serialized bytes, not a vibes claim. The absolute saving grows with
    // description/criteria content (exactly the fields this view omits).
    let compact_len = serde_json::to_string(&compact).unwrap_or_default().len();
    let full_len = serde_json::to_string(&full).unwrap_or_default().len();
    assert!(
        compact_len * 2 < full_len,
        "compact wire ({compact_len} bytes) must be under half the full wire ({full_len} bytes)"
    );
}

/// P3: the claim-next tool claims through the wire and is replay-safe —
/// a retried call with the same key returns the originally claimed task
/// (marked replayed), never a second claim; a fresh key claims the next
/// task; an exhausted pull reports nothing claimable in plain text, not
/// as an error.
#[tokio::test]
async fn claim_next_tool_claims_and_replays() {
    let server = make_server();
    let board_id = create_board(&server, "Claim Board", false).await;
    let first = create_task(&server, &board_id, "First").await;
    let second = create_task(&server, &board_id, "Second").await;
    move_task(&server, &first, "ready").await;
    move_task(&server, &second, "ready").await;

    let first_claim = claim_next(&server, &board_id, Some("claim-1"))
        .await
        .expect("first claim ok");
    let claimed_id = first_claim["task"]["task_id"]
        .as_str()
        .expect("task in response")
        .to_string();
    assert_eq!(
        claimed_id, first,
        "the oldest ready task is claimed first: {first_claim}"
    );
    assert!(
        first_claim["message"]
            .as_str()
            .expect("message")
            .contains("Claimed"),
        "the plain-text outcome names the claim: {first_claim}"
    );

    // A retried call with the same key replays the original claim —
    // never a second task.
    let replayed = claim_next(&server, &board_id, Some("claim-1"))
        .await
        .expect("replay ok");
    assert_eq!(
        replayed["replayed"].as_bool(),
        Some(true),
        "the replay is marked: {replayed}"
    );
    assert_eq!(
        replayed["task"]["task_id"].as_str(),
        Some(claimed_id.as_str()),
        "the replay returns the originally claimed task: {replayed}"
    );

    // A fresh key claims the next task.
    let second_claim = claim_next(&server, &board_id, Some("claim-2"))
        .await
        .expect("second claim ok");
    assert_eq!(
        second_claim["task"]["task_id"].as_str(),
        Some(second.as_str()),
        "the next ready task is claimed: {second_claim}"
    );

    // Exhausted: nothing claimable — a plain-text outcome, not an error.
    let exhausted = claim_next(&server, &board_id, None)
        .await
        .expect("an exhausted pull is Ok, not an error");
    assert!(
        exhausted["task"].is_null(),
        "nothing is claimed when the board is drained: {exhausted}"
    );
    let message = exhausted["message"].as_str().expect("message");
    assert!(
        message.contains("No claimable"),
        "the exhausted pull says so in plain text: {message}"
    );
}

// ── mcp-tool-review 2026-11-11 wire pins ────────────────────────────────────

async fn record_kata(
    server: &KanbanServer,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, McpToolError> {
    let request: TaskKataRecordRequest =
        serde_json::from_value(arguments).expect("arguments deserialize like a tool call");
    let out = server.kanban_task_kata_record(Parameters(request)).await?;
    Ok(parse(&out))
}

/// mcp-tool-review KK-01 pin: `"focus_obstacle": null` clears the obstacle.
/// The description promised "null to clear", but serde reads a null into
/// `Option<Option<T>>` as the outer `None`, the same as an absent field.
/// Probed 2026-11-11 against the installed server: null alone was refused
/// as an empty update, and null beside another field left the obstacle set.
#[tokio::test]
async fn kata_record_null_clears_the_focus_obstacle_and_omission_keeps_it() {
    let server = make_server();
    let board_id = create_board(&server, "kata-wire", false).await;
    let task_id = create_task(&server, &board_id, "kata task").await;

    let first = record_kata(
        &server,
        serde_json::json!({"task_id": task_id, "target_condition": "t1", "focus_obstacle": "o1"}),
    )
    .await
    .expect("first record");
    assert_eq!(first["focus_obstacle"], "o1");

    let kept = record_kata(
        &server,
        serde_json::json!({"task_id": task_id, "target_condition": "t2"}),
    )
    .await
    .expect("an omitted obstacle is kept");
    assert_eq!(kept["focus_obstacle"], "o1", "{kept}");

    let cleared = record_kata(
        &server,
        serde_json::json!({"task_id": task_id, "focus_obstacle": null}),
    )
    .await
    .expect("null alone is a real update: it clears the obstacle");
    assert!(cleared.get("focus_obstacle").is_none(), "{cleared}");
    assert_eq!(cleared["target_condition"], "t2");

    let empty = record_kata(&server, serde_json::json!({"task_id": task_id})).await;
    assert!(empty.is_err(), "no field at all is still refused");
}

/// mcp-tool-review KK-06 pin at the wire: `total_count` is the cursor.
#[tokio::test]
async fn comments_since_total_count_is_the_cursor() {
    let server = make_server();
    let board_id = create_board(&server, "comments-wire", false).await;
    let task_id = create_task(&server, &board_id, "thread").await;
    for body in ["first", "second"] {
        server
            .kanban_task_comment(Parameters(TaskCommentRequest {
                task_id: task_id.clone(),
                body: body.into(),
            }))
            .await
            .expect("comment ok");
    }
    for (since, returned) in [(0usize, 2usize), (2, 0), (5, 0)] {
        let out = server
            .kanban_task_comments_since(Parameters(TaskCommentsSinceRequest {
                task_id: task_id.clone(),
                since_index: since,
            }))
            .await
            .expect("comments ok");
        let value = parse(&out);
        assert_eq!(value["comments"].as_array().map(Vec::len), Some(returned));
        assert_eq!(value["total_count"], 2, "since_index {since}: {value}");
    }
}

/// mcp-tool-review KK-08/KK-09 pins: a refused closed-set value names the
/// valid set; padding and case are tolerated on every closed set; and the
/// full listing reads back the priority and labels `kanban_task_update`
/// wrote (the deliverables read path is pinned on `task_info` in the lib,
/// where the deliverable request type is visible).
#[tokio::test]
async fn closed_sets_name_their_values_and_task_fields_read_back() {
    let server = make_server();
    let board_id = create_board(&server, "vocab-wire", false).await;
    let task_id = create_task(&server, &board_id, "fields").await;

    let update = |priority: &str| TaskUpdateRequest {
        task_id: task_id.clone(),
        title: None,
        description: None,
        criteria: None,
        priority: Some(priority.to_string()),
        labels: Some(vec!["alpha".into()]),
        advances: None,
        reserved: None,
    };
    let refused = server
        .kanban_task_update(Parameters(update("urgent")))
        .await
        .expect_err("urgent is not a priority");
    assert!(
        refused.message.contains("low, medium, high, critical"),
        "{}",
        refused.message
    );
    server
        .kanban_task_update(Parameters(update(" High ")))
        .await
        .expect("a padded, capitalized priority resolves");

    let refused = server
        .kanban_task_move(Parameters(TaskMoveRequest {
            task_id: task_id.clone(),
            target_status: "bogus".into(),
        }))
        .await
        .expect_err("bogus is not a status");
    assert!(
        refused
            .message
            .contains("backlog, ready, in_progress, review, done"),
        "{}",
        refused.message
    );

    let list = |view: Option<&str>| TaskListRequest {
        board_id: board_id.clone(),
        status: None,
        view: view.map(str::to_string),
    };
    let compact = parse(
        &server
            .kanban_task_list(Parameters(list(Some(" Compact "))))
            .await
            .expect("a padded, capitalized view resolves"),
    );
    assert_eq!(compact["compact_tasks"].as_array().map(Vec::len), Some(1));

    let full = parse(
        &server
            .kanban_task_list(Parameters(list(None)))
            .await
            .expect("full list ok"),
    );
    let task = &full["tasks"][0];
    assert_eq!(task["priority"], "high", "{task}");
    assert_eq!(task["labels"], serde_json::json!(["alpha"]), "{task}");
}

/// mcp-tool-review KK-10 pin: the judge response carries the canonical
/// verdict, not the caller's spelling. Probed 2026-11-11: `" Done "` was
/// accepted and echoed back as `" Done "`.
#[tokio::test]
async fn goal_judge_returns_the_canonical_verdict() {
    let server = make_server();
    let created = parse(
        &server
            .kanban_goal_create(Parameters(
                serde_json::from_value(serde_json::json!({
                    "goal_text": "wire goal",
                    "criteria": [{"description": "observable"}],
                    "source": {"interaction_id": "t", "session_id": "t", "source": "test"}
                }))
                .expect("goal request"),
            ))
            .await
            .expect("goal create ok"),
    );
    let judged = parse(
        &server
            .kanban_goal_judge(Parameters(GoalJudgeRequest {
                goal_id: created["id"].as_str().expect("goal id").to_string(),
                verdict: " Done ".into(),
                confidence: 0.9,
                criterion_results: vec![CriterionJudgmentInput {
                    index: 0,
                    passed: true,
                    note: "evidence".into(),
                }],
                reasoning: "reason".into(),
            }))
            .await
            .expect("judge ok"),
    );
    assert_eq!(judged["verdict"], "done", "{judged}");
}
