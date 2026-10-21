//! Native goal decisions. Model input is evidence, never a human outcome.
use crate::{AgentTool, ToolCallEventStream, ToolInput};
use agent_client_protocol::schema::v2 as acp;
use anyhow::Result;
use gpui::{App, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Worker-supplied manifest metadata, not trusted read receipts. After the
/// native human approval only, the host may read the bounded local artifact.
pub struct NativeGoalEvidence {
    pub id: String,
    pub source: String,
    /// Absolute local file path or local file URI. No network or execution.
    pub locator: String,
    /// Expected SHA-256 of exact bytes: 64 hex digits, optionally `sha256:`.
    pub digest: String,
    pub reliability: String,
    pub oracle: String,
    pub limitation: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
/// Ask for an actual human closure choice. The native prompt discloses local
/// reads and sharing before approval; file contents are never worker-authored
/// observations, and absent/unfetchable evidence is assessed as unknown.
pub struct GoalCloseInput {
    pub goal_id: String,
    /// At most 16 references. The human reviews exact paths/digests and the
    /// read/sharing limits before any access; absent evidence remains unknown.
    #[serde(default)]
    pub evidence_manifest: Vec<NativeGoalEvidence>,
}
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalCorrectInput {
    pub goal_id: String,
}

/// Values created from the ActionChoice response, never deserialized from input.
#[derive(Debug, Clone, Copy)]
pub enum NativeGoalDecision {
    AccomplishedAchieved,
    AccomplishedNotAchieved,
    AccomplishedUnknown,
    StopAchieved,
    StopNotAchieved,
    StopUnknown,
    CorrectAchieved,
    CorrectNotAchieved,
    CorrectUnknown,
}

pub struct NativeGoalPrompt {
    pub spec_digest: String,
    pub message: String,
    pub supersedes_event_id: Option<String>,
}
pub struct NativeGoalSubmission {
    pub goal_id: String,
    pub actor: hkask_types::WebID,
    pub interaction_id: String,
    pub session_id: String,
    pub prompt: NativeGoalPrompt,
    pub decision: NativeGoalDecision,
    pub evidence_manifest: Vec<NativeGoalEvidence>,
}
/// Host-only port; not exposed by the MCP/IPC tool dispatcher.
pub trait NativeGoalHost: Send + Sync {
    fn prepare(
        &self,
        goal_id: &str,
        actor: hkask_types::WebID,
        evidence: &[NativeGoalEvidence],
        correction: bool,
    ) -> Result<NativeGoalPrompt, String>;
    fn submit(&self, submission: NativeGoalSubmission) -> Result<serde_json::Value, String>;
}
static GOAL_HOST: std::sync::RwLock<Option<Arc<dyn NativeGoalHost>>> = std::sync::RwLock::new(None);
pub fn set_native_goal_host(host: Option<Arc<dyn NativeGoalHost>>) {
    *GOAL_HOST.write().unwrap_or_else(|p| p.into_inner()) = host;
}
fn host() -> Result<Arc<dyn NativeGoalHost>, String> {
    GOAL_HOST
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
        .ok_or_else(|| "Native goal host is not configured".into())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NativeGoalOutput {
    Recorded { goal: serde_json::Value },
    Error { error: String },
}
impl From<NativeGoalOutput> for LanguageModelToolResultContent {
    fn from(output: NativeGoalOutput) -> Self {
        match output {
            NativeGoalOutput::Recorded { goal } => goal.to_string().into(),
            NativeGoalOutput::Error { error } => error.into(),
        }
    }
}

pub struct GoalCloseTool;
pub struct GoalCorrectTool;
impl AgentTool for GoalCloseTool {
    type Input = GoalCloseInput;
    type Output = NativeGoalOutput;
    const NAME: &'static str = "kanban_goal_close";
    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }
    fn initial_title(
        &self,
        _: Result<Self::Input, serde_json::Value>,
        _: &mut App,
    ) -> SharedString {
        "Ask the human to close a goal".into()
    }
    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(error)?;
            decide(
                input.goal_id,
                input.evidence_manifest,
                false,
                stream,
                host().map_err(error)?,
                cx,
            )
            .await
        })
    }
}
impl AgentTool for GoalCorrectTool {
    type Input = GoalCorrectInput;
    type Output = NativeGoalOutput;
    const NAME: &'static str = "kanban_goal_correct_outcome";
    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }
    fn initial_title(
        &self,
        _: Result<Self::Input, serde_json::Value>,
        _: &mut App,
    ) -> SharedString {
        "Ask the human to correct a goal outcome".into()
    }
    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(error)?;
            decide(
                input.goal_id,
                Vec::new(),
                true,
                stream,
                host().map_err(error)?,
                cx,
            )
            .await
        })
    }
}
fn error(error: impl std::fmt::Display) -> NativeGoalOutput {
    NativeGoalOutput::Error {
        error: error.to_string(),
    }
}

/// Shared by the actual native tools and the interaction integration harness.
pub async fn decide(
    goal_id: String,
    evidence_manifest: Vec<NativeGoalEvidence>,
    correction: bool,
    stream: ToolCallEventStream,
    host: Arc<dyn NativeGoalHost>,
    cx: &mut gpui::AsyncApp,
) -> Result<NativeGoalOutput, NativeGoalOutput> {
    let actor = stream
        .calling_actor()
        .ok_or_else(|| error("Native goal decision requires a host-authenticated thread"))?;
    let interaction_id = format!("{actor}:{}", stream.tool_call_id());
    // The ACP-scoped call id includes the owning session; no worker-supplied source is used.
    let session_id = stream
        .calling_session_id()
        .ok_or_else(|| error("Native goal decision requires a captured session"))?
        .to_string();
    let prompt = host
        .prepare(&goal_id, actor, &evidence_manifest, correction)
        .map_err(error)?;
    let choices: &[(&str, &str, NativeGoalDecision)] = if correction {
        &[
            (
                "correct_achieved",
                "Correct outcome: achieved",
                NativeGoalDecision::CorrectAchieved,
            ),
            (
                "correct_not_achieved",
                "Correct outcome: not achieved",
                NativeGoalDecision::CorrectNotAchieved,
            ),
            (
                "correct_unknown",
                "Correct outcome: unknown",
                NativeGoalDecision::CorrectUnknown,
            ),
        ]
    } else {
        &[
            (
                "accomplished_achieved",
                "Accomplished · achieved",
                NativeGoalDecision::AccomplishedAchieved,
            ),
            (
                "accomplished_not_achieved",
                "Accomplished · not achieved",
                NativeGoalDecision::AccomplishedNotAchieved,
            ),
            (
                "accomplished_unknown",
                "Accomplished · unknown",
                NativeGoalDecision::AccomplishedUnknown,
            ),
            (
                "stop_achieved",
                "Stop · achieved",
                NativeGoalDecision::StopAchieved,
            ),
            (
                "stop_not_achieved",
                "Stop · not achieved",
                NativeGoalDecision::StopNotAchieved,
            ),
            (
                "stop_unknown",
                "Stop · unknown",
                NativeGoalDecision::StopUnknown,
            ),
        ]
    };
    let options = choices
        .iter()
        .map(|(id, label, _)| {
            acp::PermissionOption::new(*id, *label, acp::PermissionOptionKind::AllowOnce)
        })
        .collect();
    let selected = cx
        .update(|cx| {
            stream.prompt_for_decision(
                Some("Your goal decision".into()),
                Some(prompt.message.clone()),
                options,
                cx,
            )
        })
        .await
        .map_err(error)?;
    let decision = choices
        .iter()
        .find(|(id, _, _)| *id == selected.0.as_ref())
        .map(|(_, _, decision)| *decision)
        .ok_or_else(|| error("Goal decision dismissed or invalid; nothing recorded"))?;
    let goal = host
        .submit(NativeGoalSubmission {
            goal_id,
            actor,
            interaction_id,
            session_id,
            prompt,
            decision,
            evidence_manifest,
        })
        .map_err(error)?;
    Ok(NativeGoalOutput::Recorded { goal })
}
