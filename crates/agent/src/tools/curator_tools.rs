//! Curator tools — regulatory surface tools for the Curator agent.
//!
//! These tools are registered on Curator threads alongside the standard Zed
//! Agent tools. They expose the Curator's regulatory surface: system health,
//! escalation management, and regulation observability.
//!
//! The tools use a `MetacognitionProvider` trait (defined here) to read
//! system health. The composition root injects the provider; when not set,
//! the tools return "not available".

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, Task};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

// ── MetacognitionProvider trait ─────────────────────────────────────────────

/// Provider for curator metacognition state.
///
/// Implemented by the composition root over `hkask_regulation::MetacognitionLoop`.
/// When not set, curator tools return "not available".
pub trait MetacognitionProvider: Send + Sync {
    /// Get the last health snapshot as a JSON value.
    fn health_snapshot_json(&self) -> Task<Option<serde_json::Value>>;
}

/// Global hook for the metacognition provider.
static METACOGNITION_PROVIDER: std::sync::Mutex<Option<Arc<dyn MetacognitionProvider>>> =
    std::sync::Mutex::new(None);

/// Set the global metacognition provider (composition root).
///
/// Re-settable — later calls replace the earlier provider.
pub fn set_metacognition_provider(provider: Option<Arc<dyn MetacognitionProvider>>) {
    *METACOGNITION_PROVIDER
        .lock()
        .expect("METACOGNITION_PROVIDER poisoned") = provider;
}

fn metacognition_provider() -> Option<Arc<dyn MetacognitionProvider>> {
    METACOGNITION_PROVIDER
        .lock()
        .expect("METACOGNITION_PROVIDER poisoned")
        .clone()
}

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
    /// When true, include the per-domain variety deficit in the response.
    #[serde(default)]
    pub include_variety: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CuratorStatusOutput {
    pub status: String,
    pub regulation_effectiveness: Option<f64>,
    pub escalation_count: Option<usize>,
    pub critical_alerts: Option<usize>,
    /// Per-domain variety deficit (gap from set-point, not a raw counter).
    /// Renamed from `variety_counters` — the value is a deficit, and calling
    /// it a "counter" misled operators into reading it as variety tracked.
    pub variety_deficit: Option<Vec<(String, u64)>>,
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
        let provider = metacognition_provider();
        cx.background_executor().spawn(async move {
            let input = input.recv().await.map_err(|_| CuratorStatusOutput {
                status: "error: invalid input".to_string(),
                regulation_effectiveness: None,
                escalation_count: None,
                critical_alerts: None,
                variety_deficit: None,
            })?;

            // Distinguish "provider not wired" from "provider wired but
            // snapshot missing" — the two cases used to collapse to the same
            // "not available" string, leaving operators unable to tell whether
            // the regulation loop was unwired or just stale. Same class as the
            // `.rules` "Process-global hooks need a startup-failure signal" trap.
            let Some(provider) = provider else {
                return Ok(CuratorStatusOutput {
                    status: "provider not wired".to_string(),
                    regulation_effectiveness: None,
                    escalation_count: None,
                    critical_alerts: None,
                    variety_deficit: None,
                });
            };
            let Some(snapshot) = provider.health_snapshot_json().await else {
                return Ok(CuratorStatusOutput {
                    status: "snapshot unavailable".to_string(),
                    regulation_effectiveness: None,
                    escalation_count: None,
                    critical_alerts: None,
                    variety_deficit: None,
                });
            };
            let effectiveness = snapshot
                .get("regulation_effectiveness")
                .and_then(|v| v.as_f64());
            let critical = snapshot
                .get("critical_alerts")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let deficit = snapshot.get("variety_deficit").and_then(|v| v.as_u64());
            // The metacognition loop's `compare` phase produces
            // `EscalationAlert`s when a threshold is breached; the
            // count is threaded through `HealthSnapshot` ->
            // `BridgeMetacognitionProvider` -> here. Zero means no
            // threshold was breached in the most recent cycle.
            let escalation_count = snapshot
                .get("escalation_count")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            Ok(CuratorStatusOutput {
                status: "ok".to_string(),
                regulation_effectiveness: effectiveness,
                escalation_count,
                critical_alerts: critical,
                variety_deficit: if input.include_variety {
                    deficit.map(|d| vec![("overall".to_string(), d)])
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
             Variety Deficit: {}",
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
                .variety_deficit
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
