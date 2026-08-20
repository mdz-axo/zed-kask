//! Metacognition loop — the Curator's sense→compare→compute→act governance loop.
//!
//! This is a self-contained implementation that doesn't depend on `hkask-pods`.
//! It reads from `RegulationLedger` (already wired in zed-kask) and emits
//! health snapshots + escalation alerts.
//!
//! ## Architecture
//!
//! ```text
//! MetacognitionLoop
//! ├── sense()     — read RegulationLedger: health, variety, regulation effectiveness
//! ├── compare()   — check thresholds: variety deficit, critical alerts, effectiveness
//! ├── compute()   — decide: escalate? calibrate? replenish?
//! └── act()       — emit directives, log alerts, update thresholds
//! ```
//!
//! ## Thresholds
//!
//! - Variety deficit > 100 → escalation (warning)
//! - Critical alerts > 3 → escalation (critical)
//! - Regulation effectiveness < 0.5 → self-calibration
//!
//! ## Integration
//!
//! The loop runs as a background task with a configurable tick interval
//! (default: 30 seconds). It holds an `Arc<RegulationLedger>` for reading
//! and emits `reg.curator.metacognition.*` spans for observability.

use std::sync::Arc;
use std::time::Duration;

use hkask_types::curator::EscalationSeverity;
use hkask_types::regulation::{LedgerHealth, RegulationHealth};
use parking_lot::RwLock;
use tokio::sync::RwLock as TokioRwLock;
use tokio::sync::mpsc;

use crate::runtime::{RegulationLedger, StoredSkillSpan};
use crate::types::loops::CurationInput;

/// Default tick interval for the metacognition loop (30 seconds).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(30);

/// Default variety deficit threshold for escalation.
const DEFAULT_VARIETY_DEFICIT_THRESHOLD: u64 = 100;

/// Default critical alert count threshold for escalation.
const DEFAULT_CRITICAL_ALERT_THRESHOLD: usize = 3;

/// Default regulation effectiveness floor (below → self-calibrate).
const DEFAULT_EFFECTIVENESS_FLOOR: f64 = 0.5;

/// A user-facing alert event forwarded by the metacognition loop.
///
/// The metacognition loop produces `EscalationAlert`s in its `compare`
/// phase and receives `RuntimeAlert`s from the `CyberneticsLoop`. Both are
/// collapsed into this minimal struct before being forwarded to an
/// `AlertSink`, so the sink implementation (e.g. a GPUI toast dispatcher)
/// doesn't depend on either internal type.
#[derive(Debug, Clone)]
pub struct AlertEvent {
    /// Short human-readable summary, suitable for a toast title.
    pub message: String,
    /// `true` for `Critical` severity, `false` for `Warning`/`Info`.
    /// The sink decides whether to surface non-critical alerts; the
    /// metacognition loop forwards every alert but only critical ones
    /// require user action.
    pub critical: bool,
}

/// Sink for user-facing alert events.
///
/// Implemented by the composition root to bridge metacognition-loop alerts
/// to a UI notification surface (e.g. a GPUI toast). The sink is called from
/// a background tokio task; implementations must be `Send + Sync` and must not
/// block on GPUI foreground state — they should dispatch onto the GPUI
/// foreground executor and return promptly.
pub trait AlertSink: Send + Sync {
    /// Forward an alert event to the user-facing surface.
    ///
    /// Errors are logged by the caller and never propagated — alert delivery
    /// is best-effort, never a correctness path.
    fn on_alert(&self, event: &AlertEvent);
}

/// A health snapshot captured by the metacognition loop.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub ledger_health: LedgerHealth,
    pub regulation_health: RegulationHealth,
    pub variety_deficit: u64,
    pub critical_alerts: usize,
    pub regulation_effectiveness: f64,
    /// Number of escalation alerts produced by the most recent `compare`
    /// phase. Zero means no threshold was breached; a positive count means
    /// the Curator should self-calibrate or surface the breach to the user.
    pub escalation_count: usize,
    /// `None` when no grounded delegations exist (absence ≠ 0 — paper Rule 5.3)
    /// loop. The Curator surfaces this via `CuratorStatusTool` so the user
    /// fire.
    /// `None` when no delegations exist or the store is not wired.
}

/// Escalation alert emitted when a threshold is breached.
#[derive(Debug, Clone)]
pub struct EscalationAlert {
    pub trigger: EscalationTrigger,
    pub severity: EscalationSeverity,
    pub value: f64,
    pub threshold: f64,
    pub message: String,
}

/// What triggered an escalation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationTrigger {
    VarietyDeficit,
    CriticalAlerts,
    LowEffectiveness,
    /// Operator feedback acceptance rate for a skill is declining over
    /// the rolling window — the skill's outputs may be drifting.
    FeedbackDrift {
        skill_id: String,
    },
}

/// Metacognition loop configuration.
#[derive(Debug, Clone)]
pub struct MetacognitionConfig {
    pub tick_interval: Duration,
    pub variety_deficit_threshold: u64,
    pub critical_alert_threshold: usize,
    pub effectiveness_floor: f64,
    /// Minimum number of outcome spans a skill must have before drift
    /// detection runs. Below this, there isn't enough data to trend.
    pub feedback_drift_min_samples: usize,
    /// Rolling window size (number of recent outcome spans) for computing
    /// the current acceptance rate.
    pub feedback_drift_window: usize,
    /// If the current window's success rate drops below this fraction of
    /// the prior window's rate, emit a FeedbackDrift alert. E.g. 0.8 means
    /// alert when current rate < 80% of prior rate.
    pub feedback_drift_decline_ratio: f64,
}

impl Default for MetacognitionConfig {
    fn default() -> Self {
        Self {
            tick_interval: DEFAULT_TICK_INTERVAL,
            variety_deficit_threshold: DEFAULT_VARIETY_DEFICIT_THRESHOLD,
            critical_alert_threshold: DEFAULT_CRITICAL_ALERT_THRESHOLD,
            effectiveness_floor: DEFAULT_EFFECTIVENESS_FLOOR,
            feedback_drift_min_samples: 10,
            feedback_drift_window: 10,
            feedback_drift_decline_ratio: 0.8,
        }
    }
}

/// The metacognition loop — the Curator's governance mechanism.
///
/// Runs sense→compare→compute→act cycles on a background task. Each cycle:
/// 1. **Sense**: reads `RegulationLedger` for health, variety, effectiveness
/// 2. **Compare**: checks thresholds (variety deficit, critical alerts, effectiveness)
/// 3. **Compute**: decides whether to escalate, calibrate, or do nothing
/// 4. **Act**: emits `reg.curator.metacognition.*` spans and logs alerts
///
/// The loop is self-contained — it doesn't need `hkask-pods`, `CuratorContext`,
/// or `CurationLoop`. It reads directly from `RegulationLedger` which is already
/// wired in zed-kask's composition root.
pub struct MetacognitionLoop {
    ledger: Arc<TokioRwLock<RegulationLedger>>,
    config: MetacognitionConfig,
    last_snapshot: RwLock<Option<HealthSnapshot>>,
    /// Optional channel to receive alerts FROM the CyberneticsLoop.
    /// The CyberneticsLoop sends `CurationInput::Alert` when its algedonic
    /// manager detects variety deficits. The metacognition loop logs and
    /// forwards these to the user via the `curator_status` tool.
    alert_rx: Option<tokio::sync::Mutex<mpsc::UnboundedReceiver<CurationInput>>>,
    /// Optional user-facing alert sink. When set, critical alerts produced
    /// by `compare` and forwarded by the `CyberneticsLoop` are dispatched to
    /// this sink so the composition root can surface them as a UI
    /// notification (e.g. a toast). Best-effort: errors are logged and
    /// swallowed.
    alert_sink: Option<Arc<dyn AlertSink>>,

    /// just see alerts when they fire. `None` when not wired (snapshot
}

impl MetacognitionLoop {
    /// Create a new metacognition loop.
    pub fn new(ledger: Arc<TokioRwLock<RegulationLedger>>) -> Self {
        Self::with_config(ledger, MetacognitionConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(
        ledger: Arc<TokioRwLock<RegulationLedger>>,
        config: MetacognitionConfig,
    ) -> Self {
        Self {
            ledger,
            config,
            last_snapshot: RwLock::new(None),
            alert_rx: None,
            alert_sink: None,
        }
    }

    /// Wire a channel to receive alerts FROM the CyberneticsLoop.
    ///
    /// The CyberneticsLoop sends `CurationInput::Alert` when its algedonic
    /// manager detects variety deficits. The metacognition loop receives
    /// these alerts, logs them, and includes them in health snapshots.
    ///
    /// This closes the feedback loop: CyberneticsLoop senses → alerts →
    /// MetacognitionLoop receives → logs + surfaces to user via
    /// `curator_status` tool → user adjusts → CyberneticsLoop re-senses.
    pub fn with_alert_receiver(mut self, rx: mpsc::UnboundedReceiver<CurationInput>) -> Self {
        self.alert_rx = Some(tokio::sync::Mutex::new(rx));
        self
    }

    /// Wire a user-facing alert sink. Critical alerts produced by `compare`
    /// and forwarded by the `CyberneticsLoop` are dispatched to this sink so
    /// the composition root can surface them as a UI notification.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_alert_sink(mut self, sink: Arc<dyn AlertSink>) -> Self {
        self.alert_sink = Some(sink);
        self
    }

    /// Run the loop as a background task. This method blocks (runs forever)
    /// until the task is cancelled or the ledger is dropped.
    ///
    /// Call this from `cx.background_spawn()`.
    pub async fn run(&self) {
        let mut interval = tokio::time::interval(self.config.tick_interval);
        interval.tick().await; // skip the first immediate tick

        loop {
            interval.tick().await;
            self.tick().await;
        }
    }

    /// Execute one sense→compare→compute→act cycle.
    pub async fn tick(&self) {
        // ── Sense ──────────────────────────────────────────────────────
        let ledger = self.ledger.read().await;
        let ledger_health = ledger.health().await;
        let regulation_health = ledger.regulation_health().await;
        let drift_alerts = self.sense_feedback_drift(&ledger).await;
        drop(ledger); // release the read lock before acting

        let mut snapshot = HealthSnapshot {
            timestamp: chrono::Utc::now(),
            variety_deficit: ledger_health.overall_deficit,
            critical_alerts: ledger_health.critical_count,
            regulation_effectiveness: regulation_health.effectiveness(),
            ledger_health,
            regulation_health,
            // Filled in after `compare` produces the alerts below.
            escalation_count: 0,
            // when wired. `None` on absence (no grounded delegations) or
            // DB failure (absence ≠ 0, paper Rule 5.3).
        };
        }

        // ── Compare + Compute ──────────────────────────────────────────
        let mut alerts = self.compare(&snapshot);
        alerts.extend(drift_alerts);
        snapshot.escalation_count = alerts.len();

        // ── Act ────────────────────────────────────────────────────────
        self.act(&snapshot, &alerts).await;

        // Store the snapshot for external queries.
        *self.last_snapshot.write() = Some(snapshot);
    }

    /// Sense feedback drift by trending outcome span success rates per
    /// skill. For each skill with enough outcome spans, splits the recent
    /// history into a current window and a prior window, computes the
    /// success rate for each, and emits a `FeedbackDrift` alert if the
    /// current rate has declined below the configured ratio of the prior
    /// rate.
    ///
    /// This is the automated drift detection layer (Compiled AI gap 2).
    /// It uses the `reg.skill.<id>.outcome` spans emitted by
    /// `BridgeManifestExecutor::execute_skill` (Step 0 of the revised plan).
    async fn sense_feedback_drift(&self, ledger: &RegulationLedger) -> Vec<EscalationAlert> {
        let skill_ids = ledger.skill_ids_with_feedback("outcome").await;
        let mut alerts = Vec::new();

        for skill_id in skill_ids {
            let spans = ledger.query_skill_feedback(&skill_id, "outcome").await;
            if spans.len() < self.config.feedback_drift_min_samples {
                continue;
            }

            let window = self.config.feedback_drift_window;
            if spans.len() < window * 2 {
                continue;
            }

            // Most recent `window` spans = current; the `window` before that = prior.
            let split = spans.len() - window;
            let prior_spans = &spans[split - window..split];
            let current_spans = &spans[split..];

            let prior_rate = success_rate(prior_spans);
            let current_rate = success_rate(current_spans);

            // Alert when current rate drops below decline_ratio * prior rate.
            // Guard against prior_rate == 0 (all failures) — no decline to detect.
            if prior_rate > 0.0
                && current_rate < prior_rate * self.config.feedback_drift_decline_ratio
            {
                alerts.push(EscalationAlert {
                    trigger: EscalationTrigger::FeedbackDrift {
                        skill_id: skill_id.clone(),
                    },
                    severity: EscalationSeverity::Warning,
                    value: current_rate,
                    threshold: prior_rate * self.config.feedback_drift_decline_ratio,
                    message: format!(
                        "Skill '{skill_id}' outcome success rate declined from \
                         {:.0}% to {:.0}% (prior window → current window)",
                        prior_rate * 100.0,
                        current_rate * 100.0
                    ),
                });
            }
        }

        // Also trend operator_feedback disposition rates. A skill can have
        // 100% cascade success but declining operator acceptance (outputs are
        // technically successful but increasingly useless). This catches that
        // case — the outcome channel alone cannot (adversarial review finding 3).
        let op_skill_ids = ledger.skill_ids_with_feedback("operator_feedback").await;
        for skill_id in op_skill_ids {
            let spans = ledger
                .query_skill_feedback(&skill_id, "operator_feedback")
                .await;
            if spans.len() < self.config.feedback_drift_min_samples {
                continue;
            }
            let window = self.config.feedback_drift_window;
            if spans.len() < window * 2 {
                continue;
            }
            let split = spans.len() - window;
            let prior_spans = &spans[split - window..split];
            let current_spans = &spans[split..];
            let prior_rate = acceptance_rate(prior_spans);
            let current_rate = acceptance_rate(current_spans);
            if prior_rate > 0.0
                && current_rate < prior_rate * self.config.feedback_drift_decline_ratio
            {
                alerts.push(EscalationAlert {
                    trigger: EscalationTrigger::FeedbackDrift {
                        skill_id: skill_id.clone(),
                    },
                    severity: EscalationSeverity::Warning,
                    value: current_rate,
                    threshold: prior_rate * self.config.feedback_drift_decline_ratio,
                    message: format!(
                        "Skill '{skill_id}' operator acceptance rate declined from \
                         {:.0}% to {:.0}% (prior window → current window)",
                        prior_rate * 100.0,
                        current_rate * 100.0
                    ),
                });
            }
        }

        alerts
    }

    /// Compare the snapshot against thresholds and produce alerts.
    fn compare(&self, snapshot: &HealthSnapshot) -> Vec<EscalationAlert> {
        let mut alerts = Vec::new();

        // Variety deficit check
        if snapshot.variety_deficit > self.config.variety_deficit_threshold {
            let severity = if snapshot.variety_deficit > self.config.variety_deficit_threshold * 2 {
                EscalationSeverity::Critical
            } else {
                EscalationSeverity::Warning
            };
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::VarietyDeficit,
                severity,
                value: snapshot.variety_deficit as f64,
                threshold: self.config.variety_deficit_threshold as f64,
                message: format!(
                    "Variety deficit {} exceeds threshold {}",
                    snapshot.variety_deficit, self.config.variety_deficit_threshold
                ),
            });
        }

        // Critical alert count check
        if snapshot.critical_alerts > self.config.critical_alert_threshold {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::CriticalAlerts,
                severity: EscalationSeverity::Critical,
                value: snapshot.critical_alerts as f64,
                threshold: self.config.critical_alert_threshold as f64,
                message: format!(
                    "Critical alert count {} exceeds threshold {}",
                    snapshot.critical_alerts, self.config.critical_alert_threshold
                ),
            });
        }

        // Regulation effectiveness check
        if snapshot.regulation_effectiveness < self.config.effectiveness_floor {
            alerts.push(EscalationAlert {
                trigger: EscalationTrigger::LowEffectiveness,
                severity: EscalationSeverity::Warning,
                value: snapshot.regulation_effectiveness,
                threshold: self.config.effectiveness_floor,
                message: format!(
                    "Regulation effectiveness {:.1}% below floor {:.1}%",
                    snapshot.regulation_effectiveness * 100.0,
                    self.config.effectiveness_floor * 100.0
                ),
            });
        }

        alerts
    }

    /// Act on the snapshot and alerts — log, drain incoming CyberneticsLoop alerts.
    async fn act(&self, snapshot: &HealthSnapshot, alerts: &[EscalationAlert]) {
        // Log the health snapshot at `debug`. This tick fires every 30s
        // regardless of activity; the structured snapshot is diagnostic, not
        // an actionable signal. Actionable alerts (critical alerts, threshold
        // breaches) are surfaced to the operator via the `ToastAlertSink`,
        // which dispatches them as GPUI toasts independent of log level.
        tracing::debug!(
            target: "reg.curator.metacognition",
            variety_deficit = snapshot.variety_deficit,
            critical_alerts = snapshot.critical_alerts,
            effectiveness = format!("{:.1}%", snapshot.regulation_effectiveness * 100.0),
            healthy = snapshot.ledger_health.healthy,
            alerts = alerts.len(),
            "Curator metacognition tick"
        );

        // Log each alert produced by the metacognition loop's own threshold checks.
        for alert in alerts {
            let critical = matches!(alert.severity, EscalationSeverity::Critical);
            match alert.severity {
                EscalationSeverity::Critical => {
                    tracing::warn!(
                        target: "reg.curator.metacognition",
                        trigger = ?alert.trigger,
                        value = alert.value,
                        threshold = alert.threshold,
                        message = %alert.message,
                        "CRITICAL metacognition escalation alert"
                    );
                }
                EscalationSeverity::Warning => {
                    tracing::warn!(
                        target: "reg.curator.metacognition",
                        trigger = ?alert.trigger,
                        value = alert.value,
                        threshold = alert.threshold,
                        message = %alert.message,
                        "Metacognition escalation alert"
                    );
                }
                EscalationSeverity::Info => {
                    tracing::info!(
                        target: "reg.curator.metacognition",
                        trigger = ?alert.trigger,
                        message = %alert.message,
                        "Info alert"
                    );
                }
            }
            // Forward critical alerts to the user-facing sink. The sink
            // is best-effort; a missing or failing sink never breaks the
            // regulation loop.
            if critical && let Some(ref sink) = self.alert_sink {
                sink.on_alert(&AlertEvent {
                    message: alert.message.clone(),
                    critical: true,
                });
            }
        }

        // Drain incoming alerts from the CyberneticsLoop (close the loop).
        // These are alerts the CyberneticsLoop's algedonic manager produced
        // during its own tick cycle — forwarded to the metacognition loop
        // for observability and user-facing surfacing.
        if let Some(ref rx) = self.alert_rx {
            let mut rx_guard = rx.lock().await;
            while let Ok(input) = rx_guard.try_recv() {
                if let CurationInput::Alert(alert) = input {
                    tracing::warn!(
                        target: "reg.curator.metacognition",
                        domain = %alert.domain,
                        deficit = alert.deficit,
                        threshold = alert.threshold,
                        severity = ?alert.severity,
                        message = %alert.message,
                        "CyberneticsLoop algedonic alert received"
                    );
                    // Forward critical CyberneticsLoop alerts to the same
                    // user-facing sink so well exhaustion and variety
                    // deficits escalate to the user, not just the logs.
                    if alert.is_critical()
                        && let Some(ref sink) = self.alert_sink
                    {
                        sink.on_alert(&AlertEvent {
                            message: alert.message.clone(),
                            critical: true,
                        });
                    }
                }
            }
        }
    }

    /// Get the last health snapshot (if any).
    pub async fn last_snapshot(&self) -> Option<HealthSnapshot> {
        self.last_snapshot.read().clone()
    }

    /// Get the last health snapshot synchronously (blocking RwLock read).
    ///
    /// Uses `parking_lot::RwLock::read()` which parks the current thread
    /// until the lock is available. Safe to call from a sync context.
    pub fn last_snapshot_blocking(&self) -> Option<HealthSnapshot> {
        self.last_snapshot.read().clone()
    }
}

/// Compute the success rate from a slice of outcome spans. Each span's
/// payload is `{"success": bool, ...}` — the `success` field is the
/// signal. Returns 0.0–1.0. Empty input returns 0.0.
fn success_rate(spans: &[StoredSkillSpan]) -> f64 {
    if spans.is_empty() {
        return 0.0;
    }
    let successes = spans
        .iter()
        .filter(|s| {
            s.payload
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .count();
    successes as f64 / spans.len() as f64
}

/// Compute the operator acceptance rate from a slice of operator_feedback
/// spans. Each span's payload is `{"disposition": "accepted"|"overridden"|
/// "rejected"|"corrected", ...}`. "accepted" counts as acceptance; all others
/// do not. Returns 0.0–1.0. Empty input returns 0.0.
fn acceptance_rate(spans: &[StoredSkillSpan]) -> f64 {
    if spans.is_empty() {
        return 0.0;
    }
    let accepted = spans
        .iter()
        .filter(|s| {
            s.payload
                .get("disposition")
                .and_then(|v| v.as_str())
                .is_some_and(|d| d == "accepted")
        })
        .count();
    accepted as f64 / spans.len() as f64
}
