//! Algedonic alerts on the operator's existing kanban board.
//! Dispatch stays on the governed panel tool channel; the kanban service owns
//! storage, transitions and verification.

use std::sync::Arc;

use hkask_regulation::{AlertDeliveryOutcome, AlertEscalationSink, AlertPersistError, Signal};
use hkask_tool_invoker::ToolInvoker;
use hkask_types::TaskStatus;
pub use hkask_types::kanban_wire::ALGEDONIC_BOARD_NAME;
use hkask_types::kanban_wire::{KANBAN_SERVER_NAME, KANBAN_TASK_MOVE_TOOL};
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde_json::{Value, json};

const CONDITION_MARKER: &str = "algedonic-condition: ";
const CONTEXT_MARKER: &str = "algedonic-context: ";

struct OpenCard {
    task_id: String,
    status: TaskStatus,
    condition: String,
    recovery_signal: Option<Signal>,
}

pub struct BoardAlertEscalationSink {
    invoker: Arc<dyn ToolInvoker>,
}

impl BoardAlertEscalationSink {
    pub fn new(invoker: Arc<dyn ToolInvoker>) -> Self {
        Self { invoker }
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let output = self
            .invoker
            .invoke_tool(KANBAN_SERVER_NAME, tool, args)
            .await
            .map_err(|error| format!("{tool}: not delivered: {error}"))?;
        if let Some(error) = parse_tool_error(&output) {
            return Err(format!("{tool}: {}", error.message));
        }
        parse_tool_response(&output).ok_or_else(|| format!("{tool}: unparsable response"))
    }

    async fn board_id(&self) -> Result<String, String> {
        let boards = self.call("kanban_board_list", json!({})).await?;
        let entries = boards["boards"]
            .as_array()
            .ok_or_else(|| "kanban_board_list: missing boards".to_string())?;
        let mut matching = entries
            .iter()
            .filter(|board| board["name"] == ALGEDONIC_BOARD_NAME);
        if let Some(board) = matching.next() {
            if matching.next().is_some() {
                return Err(
                    "kanban_board_list: multiple Algedonic review boards; cannot choose a worklist"
                        .into(),
                );
            }
            return board["board_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "kanban_board_list: matching board has no board_id".to_string());
        }
        let created = self
            .call(
                "kanban_board_create",
                // First-use status reads and alerts may race. The kanban
                // service atomically reserves this key across both callers.
                json!({ "name": ALGEDONIC_BOARD_NAME, "idempotency_key": "algedonic-review-board" }),
            )
            .await?;
        created["board_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "kanban_board_create: missing board_id".to_string())
    }

    async fn open_cards(&self) -> Result<(String, Vec<OpenCard>), String> {
        let board_id = self.board_id().await?;
        let listed = self
            .call("kanban_task_list", json!({ "board_id": board_id }))
            .await?;
        let tasks = listed["tasks"]
            .as_array()
            .ok_or_else(|| "kanban_task_list: missing tasks".to_string())?;
        let mut cards = Vec::new();
        for task in tasks {
            let status = task["status"]
                .as_str()
                .and_then(TaskStatus::parse_str)
                .ok_or_else(|| "kanban_task_list: missing or invalid status".to_string())?;
            if status == TaskStatus::Done {
                continue;
            }
            let Some(task_id) = task["task_id"].as_str() else {
                return Err("kanban_task_list: missing task_id".to_string());
            };
            let description = task["description"].as_str().unwrap_or_default();
            let Some(condition) = marker_value(description, CONDITION_MARKER) else {
                continue; // Skill proposals and manually added cards are not alert conditions.
            };
            let recovery_signal = marker_value(description, CONTEXT_MARKER)
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .and_then(|context| context.get("recovery_signal").cloned())
                .and_then(|signal| serde_json::from_value::<Signal>(signal).ok())
                .filter(Signal::is_recovery_trigger);
            cards.push(OpenCard {
                task_id: task_id.to_string(),
                status,
                condition: condition.to_string(),
                recovery_signal,
            });
        }
        Ok((board_id, cards))
    }

    /// Count every work item on the board that has not reached verified Done,
    /// including skill proposals and manually added cards.
    pub async fn awaiting_review_count(&self) -> Result<usize, String> {
        let board_id = self.board_id().await?;
        let listed = self
            .call("kanban_task_list", json!({ "board_id": board_id }))
            .await?;
        let tasks = listed["tasks"]
            .as_array()
            .ok_or_else(|| "kanban_task_list: missing tasks".to_string())?;
        tasks.iter().try_fold(0, |count, task| {
            let status = task["status"]
                .as_str()
                .and_then(TaskStatus::parse_str)
                .ok_or_else(|| "kanban_task_list: missing or invalid status".to_string())?;
            Ok(count + usize::from(status != TaskStatus::Done))
        })
    }

    async fn comment(&self, task_id: &str, body: String) -> Result<(), String> {
        self.call(
            "kanban_task_comment",
            json!({ "task_id": task_id, "body": body }),
        )
        .await
        .map(|_| ())
    }

    async fn move_to_review(&self, card: &OpenCard) -> Result<(), String> {
        let mut status = card.status;
        while status != TaskStatus::Review {
            let next = status
                .next()
                .ok_or_else(|| "cannot advance Done card".to_string())?;
            self.call(
                KANBAN_TASK_MOVE_TOOL,
                json!({ "task_id": card.task_id, "target_status": next.as_str() }),
            )
            .await?;
            status = next;
        }
        Ok(())
    }

    async fn reconcile_at(
        &self,
        observations: &[Signal],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AlertPersistError> {
        let (_, cards) = self
            .open_cards()
            .await
            .map_err(AlertPersistError::BoardRead)?;
        for card in cards {
            let Some(trigger) = card.recovery_signal.as_ref() else {
                continue;
            };
            let Some(current) = observations.iter().find(|signal| {
                signal.metric == trigger.metric
                    && signal.is_fresh_at(now)
                    && trigger.recovered_by(signal)
            }) else {
                continue;
            };
            if card.status == TaskStatus::Review {
                continue;
            }
            let evidence = format!(
                "Self-recovered: {} is {} against set point {} (observed {}). Awaiting verification.",
                current.metric.as_str(),
                current.value,
                current.set_point,
                current.timestamp.to_rfc3339(),
            );
            // A failed comment leaves the card in place to retry on the next tick.
            let recorded = match self.comment(&card.task_id, evidence).await {
                Ok(()) => self.move_to_review(&card).await,
                Err(error) => Err(error),
            };
            if let Err(error) = recorded {
                tracing::warn!(target: "reg.alert", %error, task_id = %card.task_id,
                    "Recovery not delivered to board Review; retrying next tick");
            }
        }
        Ok(())
    }
}

fn marker_value<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(marker))
        .map(str::trim)
}

#[async_trait::async_trait]
impl AlertEscalationSink for BoardAlertEscalationSink {
    async fn reconcile_conditions(&self, observations: &[Signal]) -> Result<(), AlertPersistError> {
        self.reconcile_at(observations, chrono::Utc::now()).await
    }

    async fn try_persist_alert(
        &self,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<AlertDeliveryOutcome, AlertPersistError> {
        let condition = hkask_regulation::alert_condition(output);
        let (board_id, cards) = self
            .open_cards()
            .await
            .map_err(AlertPersistError::BoardRead)?;
        if let Some(card) = cards.iter().find(|card| card.condition == condition) {
            self.comment(&card.task_id, format!("Repeated: {output}"))
                .await
                .map_err(AlertPersistError::BoardWrite)?;
            return Ok(AlertDeliveryOutcome::Confirmed(None));
        }
        let context = serde_json::from_str::<Value>(error_context)
            .unwrap_or_else(|_| json!({ "raw": error_context }));
        let description = format!(
            "{output}\n\nConfidence: {confidence}\n\n{CONDITION_MARKER}{condition}\n{CONTEXT_MARKER}{context}"
        );
        let created = self
            .call(
                "kanban_task_create",
                json!({
                    "board_id": board_id, "title": output, "description": description,
                }),
            )
            .await
            .map_err(AlertPersistError::BoardWrite)?;
        let task_id = created["task_id"].as_str().ok_or_else(|| {
            AlertPersistError::BoardWrite("kanban_task_create: missing task_id".into())
        })?;
        Ok(AlertDeliveryOutcome::Confirmed(Some(task_id.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_tool_invoker::InvokeError;
    use std::sync::Mutex;

    // A call recorder, not a kanban implementation: fixed responses expose the
    // shape of the real server's board/task list to the connector.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<(String, Value)>>,
        replies: Mutex<Vec<Value>>,
    }

    impl Recorder {
        fn with_replies(replies: Vec<Value>) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                replies: Mutex::new(replies),
            })
        }
        fn names(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("calls")
                .iter()
                .map(|(name, _)| name.clone())
                .collect()
        }
    }

    impl ToolInvoker for Recorder {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: Value,
        ) -> gpui::Task<Result<String, InvokeError>> {
            assert_eq!(server, KANBAN_SERVER_NAME);
            self.calls
                .lock()
                .expect("calls")
                .push((tool.to_string(), args));
            let reply = self.replies.lock().expect("replies").remove(0);
            gpui::Task::ready(Ok(json!({ "content": reply }).to_string()))
        }
    }

    fn board() -> Value {
        json!({ "boards": [{ "board_id": "b", "name": ALGEDONIC_BOARD_NAME }] })
    }
    fn tasks(status: &str) -> Value {
        json!({ "tasks": [{
        "task_id": "t", "status": status,
        "description": "alert\nalgedonic-condition: degraded\nalgedonic-context: {}",
    }] })
    }

    #[tokio::test]
    async fn repeated_alert_comments_without_creating_a_card() {
        let recorder = Recorder::with_replies(vec![
            board(),
            tasks("backlog"),
            json!({ "comment_id": "c" }),
        ]);
        let sink = BoardAlertEscalationSink::new(recorder.clone());
        assert_eq!(
            sink.try_persist_alert("degraded — new reading", 0.5, "{}")
                .await
                .expect("repeat"),
            AlertDeliveryOutcome::Confirmed(None)
        );
        assert_eq!(
            recorder.names(),
            [
                "kanban_board_list",
                "kanban_task_list",
                "kanban_task_comment"
            ]
        );
        assert!(
            recorder.calls.lock().expect("calls")[2].1["body"]
                .as_str()
                .is_some_and(|s| s.contains("new reading"))
        );
    }

    #[tokio::test]
    async fn new_alert_creates_a_card_on_the_existing_board() {
        let recorder = Recorder::with_replies(vec![
            board(),
            json!({ "tasks": [] }),
            json!({ "task_id": "t" }),
        ]);
        let sink = BoardAlertEscalationSink::new(recorder.clone());
        assert_eq!(
            sink.try_persist_alert("degraded — reading", 0.5, "{}")
                .await
                .expect("create"),
            AlertDeliveryOutcome::Confirmed(Some("t".into()))
        );
        assert_eq!(
            recorder.names(),
            [
                "kanban_board_list",
                "kanban_task_list",
                "kanban_task_create"
            ]
        );
        assert_eq!(recorder.calls.lock().expect("calls")[2].1["board_id"], "b");
    }

    #[tokio::test]
    async fn duplicate_review_board_names_fail_instead_of_splitting_the_worklist() {
        let recorder = Recorder::with_replies(vec![json!({ "boards": [
            { "board_id": "b1", "name": ALGEDONIC_BOARD_NAME },
            { "board_id": "b2", "name": ALGEDONIC_BOARD_NAME },
        ] })]);
        let sink = BoardAlertEscalationSink::new(recorder.clone());
        assert!(sink.awaiting_review_count().await.is_err());
        assert_eq!(recorder.names(), ["kanban_board_list"]);
    }

    /// expect: "Concurrent first-use requests cannot create multiple review boards" [P9]
    #[tokio::test]
    async fn first_alert_creates_the_one_review_board_with_replay_protection() {
        let recorder = Recorder::with_replies(vec![
            json!({ "boards": [] }),
            json!({ "board_id": "b" }),
            json!({ "tasks": [] }),
            json!({ "task_id": "t" }),
        ]);
        let sink = BoardAlertEscalationSink::new(recorder.clone());
        assert_eq!(
            sink.try_persist_alert("degraded — first", 0.5, "{}")
                .await
                .expect("create"),
            AlertDeliveryOutcome::Confirmed(Some("t".into()))
        );
        assert_eq!(
            recorder.names(),
            [
                "kanban_board_list",
                "kanban_board_create",
                "kanban_task_list",
                "kanban_task_create"
            ]
        );
        assert_eq!(
            recorder.calls.lock().expect("calls")[1].1["idempotency_key"],
            "algedonic-review-board"
        );
    }

    /// expect: "Self-recovery reaches Review with evidence, never Done" [P9]
    #[tokio::test]
    async fn recovery_advances_through_standard_columns_to_review_only() {
        let now = chrono::Utc::now();
        let trigger: Signal = serde_json::from_value(json!({
            "source": "cybernetics", "metric": "tool_reliability", "value": 0.4,
            "set_point": 0.8, "timestamp": now,
        }))
        .expect("trigger");
        let current: Signal = serde_json::from_value(json!({
            "source": "cybernetics", "metric": "tool_reliability", "value": 0.95,
            "set_point": 0.8, "timestamp": now,
        }))
        .expect("observation");
        let description = format!(
            "algedonic-condition: degraded\nalgedonic-context: {}",
            json!({ "recovery_signal": trigger })
        );
        let recorder = Recorder::with_replies(vec![
            board(),
            json!({ "tasks": [{ "task_id": "t", "status": "backlog", "description": description }] }),
            json!({ "comment_id": "c" }),
            json!({ "new_status": "ready" }),
            json!({ "new_status": "in_progress" }),
            json!({ "new_status": "review" }),
        ]);
        BoardAlertEscalationSink::new(recorder.clone())
            .reconcile_at(&[current], now)
            .await
            .expect("reconcile");
        assert_eq!(
            recorder.names(),
            [
                "kanban_board_list",
                "kanban_task_list",
                "kanban_task_comment",
                KANBAN_TASK_MOVE_TOOL,
                KANBAN_TASK_MOVE_TOOL,
                KANBAN_TASK_MOVE_TOOL
            ]
        );
        let calls = recorder.calls.lock().expect("calls");
        assert!(
            calls[2].1["body"]
                .as_str()
                .is_some_and(|text| text.contains("Self-recovered"))
        );
        assert_eq!(calls[3].1["target_status"], "ready");
        assert_eq!(calls[4].1["target_status"], "in_progress");
        assert_eq!(calls[5].1["target_status"], "review");
    }

    #[tokio::test]
    async fn status_counts_all_non_done_cards_including_proposals() {
        let recorder = Recorder::with_replies(vec![
            board(),
            json!({ "tasks": [
            { "status": "backlog" }, { "status": "review" }, { "status": "done" }
        ] }),
        ]);
        assert_eq!(
            BoardAlertEscalationSink::new(recorder)
                .awaiting_review_count()
                .await
                .expect("count"),
            2
        );
    }

    #[tokio::test]
    async fn missing_board_delivery_is_not_silently_empty() {
        struct Unwired;
        impl ToolInvoker for Unwired {
            fn invoke_tool(
                &self,
                _: &str,
                _: &str,
                _: Value,
            ) -> gpui::Task<Result<String, InvokeError>> {
                gpui::Task::ready(Err(InvokeError::NotWired))
            }
        }
        let sink = BoardAlertEscalationSink::new(Arc::new(Unwired));
        assert!(sink.try_persist_alert("degraded", 0.5, "{}").await.is_err());
        assert!(sink.reconcile_conditions(&[]).await.is_err());
    }
}
