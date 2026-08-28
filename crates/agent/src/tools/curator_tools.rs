//! Curator tools — regulatory surface tools for the Curator agent.
//!
//! These tools are registered on Curator threads alongside the standard Zed
//! Agent tools. They expose the Curator's regulatory surface:
//! - `curator_status`: read system health (variety, regulation effectiveness, alerts)
//! - `curator_directive`: issue directives to the cybernetics regulation loop
//!
//! The tools use process-global hooks (`MetacognitionProvider`,
//! `CuratorDirectiveSink`) that the composition root injects at startup.
//! When a hook is not set, the tool returns "not available".

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

pub(crate) fn metacognition_provider() -> Option<Arc<dyn MetacognitionProvider>> {
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
    /// `true` when the curator's own memory stores (episodic/semantic in
    /// `agents/curator/curator.db`) are down or partially down — the curator is
    /// running without durable memory until the self-healing re-open
    /// succeeds. `None` when the memory probe isn't wired (pre-login or
    /// upstream Zed).
    pub memory_degraded: Option<bool>,
    /// Number of alerts currently in the in-memory algedonic log.
    /// `None` when the metacognition provider isn't wired.
    pub alert_log_count: Option<usize>,
    /// The configured cap for the in-memory algedonic log.
    /// `None` when the metacognition provider isn't wired.
    pub alert_log_cap: Option<usize>,
    /// `true` when the alert log is ≥ 80% of its cap. The operator (or the
    /// `algedonic-review` skill) should review and clear reviewed entries
    /// before they are evicted unread. `None` when the metacognition provider
    /// isn't wired.
    pub alert_log_approaching_cap: Option<bool>,
    /// Trust/absence assembly verdict (Fermi `LoopView.reading`). Distinguishes
    /// wiring-closed (wired but never ticked) from turning (running and
    /// verified) from broken (running but failing) from unobserved (can't
    /// tell). `None` when the metacognition provider isn't wired.
    pub loop_reading: Option<String>,
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
                memory_degraded: None,
                alert_log_count: None,
                alert_log_cap: None,
                alert_log_approaching_cap: None,
                loop_reading: None,
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
                    memory_degraded: None,
                    alert_log_count: None,
                    alert_log_cap: None,
                    alert_log_approaching_cap: None,
                loop_reading: None,
                });
            };
            let Some(snapshot) = provider.health_snapshot_json().await else {
                return Ok(CuratorStatusOutput {
                    status: "snapshot unavailable".to_string(),
                    regulation_effectiveness: None,
                    escalation_count: None,
                    critical_alerts: None,
                    variety_deficit: None,
                    memory_degraded: None,
                    alert_log_count: None,
                    alert_log_cap: None,
                    alert_log_approaching_cap: None,
                loop_reading: None,
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
            let memory_degraded = snapshot
                .get("memory")
                .and_then(|m| m.get("degraded"))
                .and_then(|d| d.as_bool());
            // Algedonic alert log cap status — surfaced so the operator (or
            // the `algedonic-review` skill) can tell when the in-memory log is
            // approaching its cap and review/clear is needed.
            let alert_log_count = snapshot
                .get("alert_log_count")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let alert_log_cap = snapshot
                .get("alert_log_cap")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);
            let alert_log_approaching_cap = snapshot
                .get("alert_log_approaching_cap")
                .and_then(|d| d.as_bool());
            // Trust/absence assembly verdict — the Fermi LoopView reading.
            // Distinguishes wiring-closed from turning from working.
            let loop_reading = snapshot
                .get("loop_reading")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // A degraded curator memory store is a health signal in its own
            // right — surface it in `status` so a caller reading only the
            // status line (not the structured fields) still sees it.
            let status = if memory_degraded == Some(true) {
                "ok (memory degraded — curator episodic/semantic store down, \
                 self-healing re-open in progress)"
                    .to_string()
            } else if alert_log_approaching_cap == Some(true) {
                "ok (algedonic log approaching cap — run algedonic-review to clear reviewed entries)"
                    .to_string()
            } else {
                "ok".to_string()
            };
            Ok(CuratorStatusOutput {
                status,
                regulation_effectiveness: effectiveness,
                escalation_count,
                critical_alerts: critical,
                variety_deficit: if input.include_variety {
                    deficit.map(|d| vec![("overall".to_string(), d)])
                } else {
                    None
                },
                memory_degraded,
                alert_log_count,
                alert_log_cap,
                alert_log_approaching_cap,
                loop_reading,
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
             Variety Deficit: {}\n\
             Memory: {}\n\
             Algedonic Log: {}\n\
             Loop Reading: {}",
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
            match output.memory_degraded {
                Some(true) => "DEGRADED — curator memory store down".to_string(),
                Some(false) => "ok".to_string(),
                None => "not monitored".to_string(),
            },
            match (output.alert_log_count, output.alert_log_cap) {
                (Some(count), Some(cap)) => {
                    let suffix = if output.alert_log_approaching_cap == Some(true) {
                        " — APPROACHING CAP, run algedonic-review"
                    } else {
                        ""
                    };
                    format!("{}/{}{}", count, cap, suffix)
                }
                _ => "not available".to_string(),
            },
            output
                .loop_reading
                .as_deref()
                .unwrap_or("not available"),
        );
        language_model::LanguageModelToolResultContent::Text(text.into())
    }
}

// ── CuratorDirectiveSink trait ──────────────────────────────────────────────

/// Sink for curator directives — the composition root implements this over
/// the tokio channel that feeds `CyberneticsLoop::process_inbox`.
///
/// The trait lives here (not in `hkask-types`) so the `agent` crate can use it
/// without depending on `hkask-types`. The composition root converts the
/// tool-local `CuratorDirectiveRequest` into `hkask_types::CuratorDirective`
/// before sending.
pub trait CuratorDirectiveSink: Send + Sync {
    /// Send a directive to the cybernetics regulation loop.
    ///
    /// Returns `Ok(accepted)` where `accepted` is `true` if the directive was
    /// sent, `false` if it was dampened (duplicate within cooldown). Returns
    /// `Err` if the channel is closed or the sink is unwired.
    fn send_directive(&self, directive: CuratorDirectiveRequest) -> Result<bool, String>;
}

/// Global hook for the curator directive sink.
static CURATOR_DIRECTIVE_SINK: std::sync::Mutex<Option<Arc<dyn CuratorDirectiveSink>>> =
    std::sync::Mutex::new(None);

/// Set the global curator directive sink (composition root).
///
/// Re-settable — later calls replace the earlier sink.
pub fn set_curator_directive_sink(sink: Option<Arc<dyn CuratorDirectiveSink>>) {
    *CURATOR_DIRECTIVE_SINK
        .lock()
        .expect("CURATOR_DIRECTIVE_SINK poisoned") = sink;
}

fn curator_directive_sink() -> Option<Arc<dyn CuratorDirectiveSink>> {
    CURATOR_DIRECTIVE_SINK
        .lock()
        .expect("CURATOR_DIRECTIVE_SINK poisoned")
        .clone()
}

// ── AlgedonicLogSink trait ────────────────────────────────────────────────────

/// Sink for clearing the in-memory algedonic alert log — the composition root
/// implements this over the `RegulationLedger` (which holds the
/// `AlgedonicManager`). The trait lives here (not in `hkask-types`) so the
/// agent crate can use it without depending on `hkask-regulation`.
///
/// The `algedonic-review` skill calls this after the operator has reviewed
/// the log and confirmed that escalated alerts have been persisted to the
/// `EscalationQueue`. Clearing reviewed entries frees the in-memory log
/// before it reaches its cap and evicts entries unread.
///
/// Methods are async because the `RegulationLedger` is behind a
/// `tokio::sync::RwLock` — the sink spawns the lock acquisition on the
/// tokio runtime. The tool that calls this sink already runs in an async
/// context (`background_executor().spawn`).
#[async_trait::async_trait]
pub trait AlgedonicLogSink: Send + Sync {
    /// Clear reviewed alerts from the in-memory log. Retains unresolved
    /// Critical alerts (those that have not been escalated yet) so the live
    /// signal is not lost.
    ///
    /// Returns `Ok(cleared_count)` where `cleared_count` is the number of
    /// alerts removed. Returns `Err` if the sink is unwired or the clear
    /// failed.
    async fn clear_reviewed_alerts(&self) -> Result<usize, String>;

    /// Clear ALL alerts from the in-memory log, including unresolved Critical.
    /// Use with caution — this loses live signals. The operator should only
    /// call this when the log is being reset for a fresh session or when all
    /// alerts have been reviewed and persisted to the `EscalationQueue`.
    ///
    /// Returns `Ok(cleared_count)` where `cleared_count` is the number of
    /// alerts removed.
    async fn clear_all_alerts(&self) -> Result<usize, String>;

    /// Number of alerts currently in the in-memory log.
    /// Returns `None` if the sink is unwired.
    async fn alert_log_count(&self) -> Option<usize>;

    /// Whether the in-memory log is approaching its cap (≥ 80%).
    /// Returns `None` if the sink is unwired.
    async fn alert_log_approaching_cap(&self) -> Option<bool>;
}

/// Global hook for the algedonic log sink.
static ALGEDONIC_LOG_SINK: std::sync::Mutex<Option<Arc<dyn AlgedonicLogSink>>> =
    std::sync::Mutex::new(None);

/// Set the global algedonic log sink (composition root).
///
/// Re-settable — later calls replace the earlier sink.
pub fn set_algedonic_log_sink(sink: Option<Arc<dyn AlgedonicLogSink>>) {
    *ALGEDONIC_LOG_SINK
        .lock()
        .expect("ALGEDONIC_LOG_SINK poisoned") = sink;
}

fn algedonic_log_sink() -> Option<Arc<dyn AlgedonicLogSink>> {
    ALGEDONIC_LOG_SINK
        .lock()
        .expect("ALGEDONIC_LOG_SINK poisoned")
        .clone()
}

// ── Curator Clear Algedonic Log Tool ────────────────────────────────────────

/// Clear reviewed alerts from the in-memory algedonic alert log.
///
/// Called by the `algedonic-review` skill after the operator has reviewed
/// the log and confirmed that escalated alerts have been persisted to the
/// `EscalationQueue`. Retains unresolved Critical alerts (those that have
/// not been escalated yet) so the live signal is not lost.
pub struct CuratorClearAlgedonicLogTool;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CuratorClearAlgedonicLogInput {
    /// When true, clear ALL alerts including unresolved Critical (use with
    /// caution — loses live signals). Default false (retain unresolved).
    #[serde(default)]
    pub clear_all: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CuratorClearAlgedonicLogOutput {
    pub cleared: usize,
    pub remaining: usize,
    pub message: String,
}

impl AgentTool for CuratorClearAlgedonicLogTool {
    type Input = CuratorClearAlgedonicLogInput;
    type Output = CuratorClearAlgedonicLogOutput;

    const NAME: &'static str = "curator_clear_algedonic_log";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Execute
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        "Clear Algedonic Log".into()
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let sink = algedonic_log_sink();
        cx.background_executor().spawn(async move {
            let input = input
                .recv()
                .await
                .map_err(|_| CuratorClearAlgedonicLogOutput {
                    cleared: 0,
                    remaining: 0,
                    message: "error: invalid input".to_string(),
                })?;

            let Some(sink) = sink else {
                return Ok(CuratorClearAlgedonicLogOutput {
                    cleared: 0,
                    remaining: 0,
                    message: "algedonic log sink not wired".to_string(),
                });
            };

            // Route `clear_all`: when true, clear ALL alerts (including
            // unresolved Critical) — the hard-reset path. When false (default),
            // clear only reviewed alerts, retaining unresolved Critical.
            let cleared = if input.clear_all {
                sink.clear_all_alerts().await
            } else {
                sink.clear_reviewed_alerts().await
            }
            .map_err(|e| CuratorClearAlgedonicLogOutput {
                cleared: 0,
                remaining: 0,
                message: format!("clear failed: {e}"),
            })?;
            let remaining = sink.alert_log_count().await.unwrap_or(0);
            let message = if cleared == 0 {
                if input.clear_all {
                    "No alerts to clear (log is empty)".to_string()
                } else {
                    "No reviewed alerts to clear (log is empty or all alerts are unresolved Critical)"
                        .to_string()
                }
            } else if input.clear_all {
                format!("Cleared ALL {cleared} alerts (hard reset), {remaining} remaining")
            } else {
                format!("Cleared {cleared} reviewed alerts, {remaining} remaining")
            };
            Ok(CuratorClearAlgedonicLogOutput {
                cleared,
                remaining,
                message,
            })
        })
    }
}

impl From<CuratorClearAlgedonicLogOutput> for language_model::LanguageModelToolResultContent {
    fn from(output: CuratorClearAlgedonicLogOutput) -> Self {
        let text = format!(
            "Algedonic Log Clear: {}\nCleared: {}\nRemaining: {}",
            output.message, output.cleared, output.remaining,
        );
        language_model::LanguageModelToolResultContent::Text(text.into())
    }
}

// ── Curator Directive Tool ───────────────────────────────────────────────────

/// A directive the Curator model issues to the cybernetics regulation loop.
///
/// This is a tool-local representation — the composition root converts it to
/// `hkask_types::CuratorDirective` before sending. Agent fields use names
/// (e.g. `"curator"`, `"swarm-panel"`), not raw WebIDs, because the model
/// knows agents by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CuratorDirectiveRequest {
    /// Adjust a regulation threshold for a domain.
    CalibrateThreshold {
        /// Domain identifier (e.g., "inference", "storage", "variety").
        domain: String,
        /// New threshold value.
        new_threshold: u64,
    },
    /// Add or remove capabilities for an agent.
    UpdateCapabilities {
        /// Agent name (e.g., "curator", "swarm-panel").
        agent: String,
        /// Capabilities to add.
        #[serde(default)]
        additions: Vec<String>,
        /// Capabilities to remove.
        #[serde(default)]
        removals: Vec<String>,
    },
    /// Override an agent's energy budget beyond cybernetics set-points.
    OverrideEnergyBudget {
        /// Agent name.
        agent: String,
        /// New energy budget (call cap per tick).
        new_budget: u64,
    },
    /// Request more evidence for a pending decision (confidence-gated).
    SeekMoreEvidence {
        /// The decision context requiring more evidence.
        context: String,
        /// Which evidence channel to verify (e.g., "llm_confidence", "validation_result").
        channel: String,
        /// Current confidence level (e.g., "0.45").
        confidence: String,
    },
    /// Replenish an agent's energy budget by a specific amount.
    ReplenishBudget {
        /// Agent name.
        agent: String,
        /// Amount to replenish.
        amount: u64,
        /// Priority weight for replenishment scaling (0.0–1.0). Defaults to 1.0.
        #[serde(default)]
        priority: Option<f64>,
    },
    /// Clear a curation override on an agent's energy budget.
    ClearOverride {
        /// Agent name.
        agent: String,
    },
    /// Escalate a domain-level concern to the user for human review.
    EscalateDomain {
        /// Domain identifier (e.g., "inference", "storage").
        domain: String,
        /// Severity level: "info", "warning", or "critical".
        severity: String,
        /// Human-readable evidence summary.
        evidence: String,
    },
    /// Evolve an MCP tool's input schema based on skill-use feedback.
    ///
    /// This is the Phase 3 co-evolution directive. The Curator analyzes
    /// skill-use reports and issues a directive to evolve a tool's schema.
    /// The directive is recorded in the regulation ledger for a developer
    /// or automated migration agent to act on — it does not directly modify
    /// the compiled Rust struct.
    EvolveMcpToolSchema {
        /// MCP server name (e.g., "hkask-mcp-companies").
        server_name: String,
        /// Tool name on that server (e.g., "dcf_valuation").
        tool_name: String,
        /// Type of schema evolution requested.
        evolution_type: SchemaEvolutionType,
        /// Field name to add/remove/rename/change.
        field_name: String,
        /// New field type (for add_field/change_type), or new field name (for
        /// rename_field). Omitted for remove_field.
        #[serde(default)]
        new_type: Option<String>,
        /// Why this evolution is needed — grounded in skill-use reports.
        rationale: String,
        /// Evidence summary (which skill, which step, what failure).
        evidence: String,
    },
}

/// Type of MCP tool schema evolution requested by the Curator.
/// Mirrors `hkask_types::curator::SchemaEvolutionType` — the agent crate
/// does not depend on `hkask-types`, so the type is duplicated here and
/// converted in the bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchemaEvolutionType {
    /// Add a new input field to the tool's schema.
    AddField,
    /// Remove an existing input field.
    RemoveField,
    /// Rename an existing input field.
    RenameField,
    /// Change the type of an existing input field.
    ChangeType,
}

/// Issue a CuratorDirective to the cybernetics regulation loop.
///
/// Directives adjust thresholds, capabilities, energy budgets, or escalate
/// domain-level concerns. The cybernetics loop dampens repeated directives to
/// prevent feedback oscillation — a dampened directive returns `accepted: false`.
/// When the directive channel is not wired, returns an error.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Default)]
pub struct CuratorDirectiveTool;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CuratorDirectiveInput {
    /// The directive to issue.
    pub directive: CuratorDirectiveRequest,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CuratorDirectiveOutput {
    /// `true` if the directive was accepted; `false` if dampened (duplicate within cooldown).
    pub accepted: bool,
    /// Human-readable status message.
    pub message: String,
    /// The directive variant name that was issued (for logging).
    pub directive_type: String,
}

impl AgentTool for CuratorDirectiveTool {
    type Input = CuratorDirectiveInput;
    type Output = CuratorDirectiveOutput;

    const NAME: &'static str = "curator_directive";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        let variant = input
            .ok()
            .map(|i| i.directive.variant_name())
            .unwrap_or("directive");
        format!("Curator Directive: {variant}").into()
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let sink = curator_directive_sink();
        cx.background_executor().spawn(async move {
            let input = input.recv().await.map_err(|_| CuratorDirectiveOutput {
                accepted: false,
                message: "error: invalid input".to_string(),
                directive_type: "unknown".to_string(),
            })?;

            let directive_type = input.directive.variant_name().to_string();

            let Some(sink) = sink else {
                return Ok(CuratorDirectiveOutput {
                    accepted: false,
                    message: "directive sink not wired — the regulation loop is not running"
                        .to_string(),
                    directive_type,
                });
            };

            match sink.send_directive(input.directive) {
                Ok(true) => Ok(CuratorDirectiveOutput {
                    accepted: true,
                    message: format!("directive '{directive_type}' accepted by the regulation loop"),
                    directive_type,
                }),
                Ok(false) => Ok(CuratorDirectiveOutput {
                    accepted: false,
                    message: format!(
                        "directive '{directive_type}' dampened — a similar directive was issued recently (cooldown active)"
                    ),
                    directive_type,
                }),
                Err(channel_err) => Ok(CuratorDirectiveOutput {
                    accepted: false,
                    message: format!("directive '{directive_type}' failed: {channel_err}"),
                    directive_type,
                }),
            }
        })
    }
}

impl CuratorDirectiveRequest {
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::CalibrateThreshold { .. } => "calibrate_threshold",
            Self::UpdateCapabilities { .. } => "update_capabilities",
            Self::OverrideEnergyBudget { .. } => "override_energy_budget",
            Self::SeekMoreEvidence { .. } => "seek_more_evidence",
            Self::ReplenishBudget { .. } => "replenish_budget",
            Self::ClearOverride { .. } => "clear_override",
            Self::EscalateDomain { .. } => "escalate_domain",
            Self::EvolveMcpToolSchema { .. } => "evolve_mcp_tool_schema",
        }
    }
}

impl From<CuratorDirectiveOutput> for language_model::LanguageModelToolResultContent {
    fn from(output: CuratorDirectiveOutput) -> Self {
        let status = if output.accepted {
            "accepted"
        } else {
            "not accepted"
        };
        let text = format!(
            "Curator Directive ({}): {}\n  {}",
            output.directive_type, status, output.message,
        );
        language_model::LanguageModelToolResultContent::Text(text.into())
    }
}
