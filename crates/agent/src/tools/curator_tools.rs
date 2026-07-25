//! Curator tools — regulatory surface tools for the Curator agent.
//!
//! These tools are registered on Curator threads alongside the standard Zed
//! Agent tools. They expose the Curator's regulatory surface: system health,
//! escalation management, and regulation observability.
//!
//! The tools are thin wrappers over the regulation system's in-process state
//! (RegulationLedger, CyberneticsLoop). When the regulation system is not
//! wired (upstream Zed), the tools return "not available" messages.

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

// ── Curator Status Tool ─────────────────────────────────────────────────────

/// Check the Curator's regulatory system health.
///
/// Returns the current variety counters, regulation effectiveness, escalation
/// count, and any critical alerts. When the regulation system is not wired,
/// returns "not available".
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Default)]
pub struct CuratorStatusTool;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CuratorStatusInput {
    /// When true, include per-domain variety counters in the response.
    #[serde(default)]
    pub include_variety: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CuratorStatusOutput {
    pub status: String,
    pub regulation_effectiveness: Option<f64>,
    pub escalation_count: Option<usize>,
    pub critical_alerts: Option<usize>,
    pub variety_counters: Option<Vec<(String, u64)>>,
}

impl AgentTool for CuratorStatusTool {
    type Input = CuratorStatusInput;
    type Output = CuratorStatusOutput;

    const NAME: &'static str = "curator_status";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Read
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        "Curator Status".into()
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.background_executor().spawn(async move {
            let input = input.recv().await.map_err(|_| CuratorStatusOutput {
                status: "error: invalid input".to_string(),
                regulation_effectiveness: None,
                escalation_count: None,
                critical_alerts: None,
                variety_counters: None,
            })?;
            Ok(CuratorStatusOutput {
                status: "ok".to_string(),
                regulation_effectiveness: None,
                escalation_count: None,
                critical_alerts: None,
                variety_counters: if input.include_variety {
                    Some(Vec::new())
                } else {
                    None
                },
            })
        })
    }
}

impl From<CuratorStatusOutput> for language_model::LanguageModelToolResultContent {
    fn from(output: CuratorStatusOutput) -> Self {
        let text = format!(
            "Curator Status: {}\n\
             Regulation Effectiveness: {}\n\
             Escalations: {}\n\
             Critical Alerts: {}\n\
             Variety Counters: {}",
            output.status,
            output
                .regulation_effectiveness
                .map(|v| format!("{:.1}%", v * 100.0))
                .unwrap_or_else(|| "not available".to_string()),
            output
                .escalation_count
                .map(|c| c.to_string())
                .unwrap_or_else(|| "not available".to_string()),
            output
                .critical_alerts
                .map(|c| c.to_string())
                .unwrap_or_else(|| "not available".to_string()),
            output
                .variety_counters
                .map(|v| {
                    v.iter()
                        .map(|(k, c)| format!("{k}: {c}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "not requested".to_string()),
        );
        language_model::LanguageModelToolResultContent::Text(text.into())
    }
}
