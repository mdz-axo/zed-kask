//! Cross-seam board test over the real kanban tool methods and an in-memory
//! SqliteDriver. The adapter forwards tool requests; it models no kanban state.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::BoardAlertEscalationSink;
use gpui::BackgroundExecutor;
use hkask_mcp_server::server::McpToolError;
use hkask_mcp_swarm::LocalAgentRegistry;
use hkask_storage::HMemStore;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_tool_invoker::{InvokeError, ToolInvoker};
use hkask_types::kanban_wire::{ALGEDONIC_BOARD_NAME, KANBAN_SERVER_NAME};
use hkask_types::tool_response::parse_tool_response;
use hkask_types::{InferenceError, WebID, WorktreeSpawnPort};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{Value, json};

use hkask_mcp_kata_kanban::types::*;
use hkask_mcp_kata_kanban::{KanbanServer, KanbanService};

struct NoWorktree;
impl WorktreeSpawnPort for NoWorktree {
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
                "no worktree in board test".into(),
            ))
        })
    }
}

struct RealKanbanInvoker {
    server: Arc<KanbanServer>,
    executor: BackgroundExecutor,
}

impl ToolInvoker for RealKanbanInvoker {
    fn invoke_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> gpui::Task<Result<String, InvokeError>> {
        if server != KANBAN_SERVER_NAME {
            return gpui::Task::ready(Err(InvokeError::Failed(format!("wrong server: {server}"))));
        }
        let kanban = self.server.clone();
        let tool = tool.to_owned();
        self.executor.spawn(async move {
            // Forward the caller's JSON to the actual MCP tool, not a test
            // implementation of board creation, listing or transitions.
            macro_rules! forward {
                ($request:ty, $method:ident) => {{
                    let request: $request = serde_json::from_value(args)
                        .map_err(|error| InvokeError::Failed(error.to_string()))?;
                    kanban.$method(Parameters(request)).await
                }};
            }

            let result: Result<String, McpToolError> = match tool.as_str() {
                "kanban_board_list" => forward!(BoardListRequest, kanban_board_list),
                "kanban_board_create" => forward!(BoardCreateRequest, kanban_board_create),
                "kanban_task_list" => forward!(TaskListRequest, kanban_task_list),
                "kanban_task_create" => forward!(TaskCreateRequest, kanban_task_create),
                "kanban_task_comment" => forward!(TaskCommentRequest, kanban_task_comment),
                "kanban_task_move" => forward!(TaskMoveRequest, kanban_task_move),
                other => return Err(InvokeError::Failed(format!("unexpected tool: {other}"))),
            };
            result.map_err(|error| InvokeError::Failed(error.to_string()))
        })
    }
}

fn tool_content(result: Result<String, McpToolError>) -> Value {
    parse_tool_response(&result.expect("real kanban tool call")).expect("tool content envelope")
}

/// expect: "One real board receives new/repeated alerts and self-recovery;
/// only verification moves its card to Done" [P9]
#[gpui::test]
async fn alert_to_real_board_repeats_recovers_and_requires_verification(
    cx: &mut gpui::TestAppContext,
) {
    use hkask_regulation::AlertEscalationSink;

    let driver = SqliteDriver::in_memory_driver();
    let store = HMemStore::from_driver(driver.clone()).expect("kanban store");
    let idempotency = Arc::new(
        hkask_mcp_kata_kanban::idempotency::IdempotencyStore::with_driver(driver)
            .expect("idempotency store"),
    );
    let server = Arc::new(KanbanServer::new(
        WebID::new(),
        KanbanService::new(store),
        Arc::new(LocalAgentRegistry::new("/nonexistent")),
        Arc::new(NoWorktree),
        idempotency.clone(),
        idempotency,
    ));
    let invoker: Arc<dyn ToolInvoker> = Arc::new(RealKanbanInvoker {
        server: server.clone(),
        executor: cx.background_executor.clone(),
    });
    let sink = BoardAlertEscalationSink::new(invoker);
    let now = chrono::Utc::now();
    let trigger: hkask_regulation::Signal = serde_json::from_value(json!({
        "source": "cybernetics", "metric": "tool_reliability", "value": 0.4,
        "set_point": 0.8, "timestamp": now,
    }))
    .expect("trigger");
    let recovered: hkask_regulation::Signal = serde_json::from_value(json!({
        "source": "cybernetics", "metric": "tool_reliability", "value": 0.95,
        "set_point": 0.8, "timestamp": now,
    }))
    .expect("recovery");
    let first = sink
        .try_persist_alert(
            "degraded — value 40",
            0.5,
            &json!({"recovery_signal": trigger}).to_string(),
        )
        .await
        .expect("new alert");
    let task_id = match first {
        hkask_regulation::AlertDeliveryOutcome::Confirmed(Some(id)) => id,
        other => panic!("new alert must create a card: {other:?}"),
    };
    assert_eq!(
        sink.try_persist_alert("degraded — value 30", 0.5, "{}")
            .await
            .expect("repeat"),
        hkask_regulation::AlertDeliveryOutcome::Confirmed(None)
    );
    let boards = tool_content(
        server
            .kanban_board_list(Parameters(BoardListRequest {}))
            .await,
    );
    let matches: Vec<&Value> = boards["boards"]
        .as_array()
        .expect("boards")
        .iter()
        .filter(|board| board["name"] == ALGEDONIC_BOARD_NAME)
        .collect();
    assert_eq!(matches.len(), 1, "one review board, not one per alert");
    let board_id = matches[0]["board_id"].as_str().expect("board id");
    let tasks = tool_content(
        server
            .kanban_task_list(Parameters(TaskListRequest {
                board_id: board_id.to_string(),
                status: None,
            }))
            .await,
    );
    assert_eq!(tasks["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(sink.awaiting_review_count().await.expect("status count"), 1);
    let comments = tool_content(
        server
            .kanban_task_comments_since(Parameters(
                serde_json::from_value(json!({ "task_id": task_id, "since_index": 0 }))
                    .expect("comments request"),
            ))
            .await,
    );
    assert!(
        comments["comments"]
            .as_array()
            .expect("comments")
            .iter()
            .any(|comment| comment["body"]
                .as_str()
                .is_some_and(|body| body.contains("value 30")))
    );

    sink.reconcile_conditions(&[recovered.clone()])
        .await
        .expect("self-recovery");
    let review = tool_content(
        server
            .kanban_task_list(Parameters(TaskListRequest {
                board_id: board_id.to_string(),
                status: Some("review".into()),
            }))
            .await,
    );
    assert_eq!(review["tasks"].as_array().expect("review cards").len(), 1);
    let comments = tool_content(
        server
            .kanban_task_comments_since(Parameters(
                serde_json::from_value(json!({ "task_id": task_id, "since_index": 0 }))
                    .expect("comments request"),
            ))
            .await,
    );
    assert!(
        comments["comments"]
            .as_array()
            .expect("comments")
            .iter()
            .any(|comment| comment["body"]
                .as_str()
                .is_some_and(|body| body.contains("Self-recovered")))
    );
    // A fresh bad observation invalidates an unverified recovery. The
    // existing card must leave Review, not invite a stale Done verdict.
    assert_eq!(
        sink.try_persist_alert("degraded — recurrence after recovery", 0.5, "{}")
            .await
            .expect("recurrence"),
        hkask_regulation::AlertDeliveryOutcome::Confirmed(None)
    );
    let in_progress = tool_content(
        server
            .kanban_task_list(Parameters(TaskListRequest {
                board_id: board_id.to_string(),
                status: Some("in_progress".into()),
            }))
            .await,
    );
    assert_eq!(
        in_progress["tasks"]
            .as_array()
            .expect("reopened cards")
            .len(),
        1,
        "a fresh degradation must remove the card from Review"
    );
    sink.reconcile_conditions(&[recovered])
        .await
        .expect("recovered again");
    assert!(
        server
            .kanban_task_move(Parameters(TaskMoveRequest {
                task_id: task_id.clone(),
                target_status: "done".into(),
            }))
            .await
            .is_err(),
        "ordinary move cannot bypass verification"
    );
    let verified = tool_content(
        server
            .kanban_task_verify(Parameters(
                serde_json::from_value(json!({
                    "task_id": task_id, "evidence": "Reviewer checked the recovered observation",
                }))
                .expect("verify request"),
            ))
            .await,
    );
    assert_eq!(verified["new_status"], "done");
    assert_eq!(
        sink.awaiting_review_count()
            .await
            .expect("after verification"),
        0
    );
}
