use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v1 as acp;
use gpui::{App, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The operator's direct skill-feedback control (T15, channel b): explicitly
/// rate a skill's output — acceptance or rejection with an optional reason.
///
/// The rating lands in the shared `RegulationLedger` as a
/// `reg.skill.<id>.operator_feedback` span, which the metacognition loop's
/// drift sensing trends ("declining operator acceptance" — outputs that are
/// technically successful but increasingly useless; the outcome channel alone
/// cannot catch that). This is the operator's channel for reacting to a
/// skill's output directly: overridden tasks, rejection reasons, and
/// correction directions (task-breakdown's `corrected_fields`).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecordSkillFeedbackInput {
    /// The skill whose output is being rated (e.g., "lora-training").
    pub skill_id: String,
    /// The operator's disposition: true = accepted, false = rejected.
    pub accepted: bool,
    /// Optional reason — the rejection reason or correction direction.
    /// This is the payload the skill reads as `prior_operator_feedback`
    /// on its next invocation (e_t intrinsic evaluative feedback).
    #[serde(default)]
    pub note: Option<String>,
}

pub struct RecordSkillFeedbackTool;

impl RecordSkillFeedbackTool {
    pub fn new() -> Self {
        Self
    }
}

impl AgentTool for RecordSkillFeedbackTool {
    type Input = RecordSkillFeedbackInput;
    type Output = String;

    const NAME: &'static str = "record_skill_feedback";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Fetch
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> ui::SharedString {
        match input {
            Ok(input) => {
                let disposition = if input.accepted {
                    "Accepted"
                } else {
                    "Rejected"
                };
                format!("{disposition} output of {}", input.skill_id).into()
            }
            Err(_) => "Record skill feedback".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |_| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            crate::record_operator_feedback(&input.skill_id, input.accepted, input.note.as_deref());
            let disposition = if input.accepted {
                "accepted"
            } else {
                "rejected"
            };
            Ok(format!(
                "Recorded operator feedback: {} output of {} ({disposition}).",
                disposition, input.skill_id
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// T15 (channel b): the direct tool fires the operator-feedback hook
    /// with the operator's disposition and reason — the explicit rating
    /// reaches the ledger wiring as an operator_feedback span.
    #[gpui::test]
    async fn records_the_operators_disposition(cx: &mut gpui::TestAppContext) {
        let recorded: Arc<Mutex<Vec<(String, bool, Option<String>)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&recorded);
        crate::set_operator_feedback_recorder(Arc::new(move |skill_id, accepted, note| {
            sink.lock().unwrap_or_else(|e| e.into_inner()).push((
                skill_id.to_string(),
                accepted,
                note.map(str::to_string),
            ));
        }));

        let (event_stream, _event_rx) = ToolCallEventStream::test();
        let tool = Arc::new(RecordSkillFeedbackTool::new());
        let task = cx.update(|cx: &mut App| {
            tool.run(
                ToolInput::resolved(RecordSkillFeedbackInput {
                    skill_id: "lora-training".to_string(),
                    accepted: false,
                    note: Some("rank 16 too small for the dataset".to_string()),
                }),
                event_stream,
                cx,
            )
        });
        let output = task.await.expect("tool runs");

        assert!(
            output.contains("lora-training"),
            "the tool confirms: {output}"
        );
        let recorded = recorded.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            *recorded,
            vec![(
                "lora-training".to_string(),
                false,
                Some("rank 16 too small for the dataset".to_string())
            )],
            "the hook must receive the skill, disposition, and reason"
        );

        // Restore the unwired state for other tests.
        crate::set_operator_feedback_recorder(Arc::new(|_, _, _| {}));
    }
}
