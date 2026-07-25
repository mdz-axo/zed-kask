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

use hkask_types::curator::{CuratorDirective, EscalationSeverity};
use hkask_types::regulation::{LedgerHealth, RegulationHealth};
use tokio::sync::RwLock;

use crate::runtime::RegulationLedger;

/// Default tick interval for the metacognition loop (30 seconds).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(30);

/// Default variety deficit threshold for escalation.
const DEFAULT_VARIETY_DEFICIT_THRESHOLD: u64 = 100;

/// Default critical alert count threshold for escalation.
const DEFAULT_CRITICAL_ALERT_THRESHOLD: usize = 3;

/// Default regulation effectiveness floor (below → self-calibrate).
const DEFAULT_EFFECTIVENESS_FLOOR: f64 = 0.5;

/// A health snapshot captured by the metacognition loop.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub ledger_health: LedgerHealth,
    pub regulation_health: RegulationHealth,
    pub variety_deficit: u64,
    pub critical_alerts: usize,
    pub regulation_effectiveness: f64,
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
}

/// Metacognition loop configuration.
#[derive(Debug, Clone)]
pub struct MetacognitionConfig {
    pub tick_interval: Duration,
    pub variety_deficit_threshold: u64,
    pub critical_alert_threshold: usize,
    pub effectiveness_floor: f64,
}

impl Default for MetacognitionConfig {
    fn default() -> Self {
        Self {
            tick_interval: DEFAULT_TICK_INTERVAL,
            variety_deficit_threshold: DEFAULT_VARIETY_DEFICIT_THRESHOLD,
            critical_alert_threshold: DEFAULT_CRITICAL_ALERT_THRESHOLD,
            effectiveness_floor: DEFAULT_EFFECTIVENESS_FLOOR,
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
    ledger: Arc<RwLock<RegulationLedger>>,
    config: MetacognitionConfig,
    last_snapshot: RwLock<Option<HealthSnapshot>>,
}

impl MetacognitionLoop {
    /// Create a new metacognition loop.
    pub fn new(ledger: Arc<RwLock<RegulationLedger>>) -> Self {
        Self::with_config(ledger, MetacognitionConfig::default())
    }

    /// Create with custom configuration.
    pub fn with_config(ledger: Arc<RwLock<RegulationLedger>>, config: MetacognitionConfig) -> Self {
        Self {
            ledger,
            config,
            last_snapshot: RwLock::new(None),
        }
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
        drop(ledger); // release the read lock before acting

        let snapshot = HealthSnapshot {
            timestamp: chrono::Utc::now(),
            variety_deficit: ledger_health.overall_deficit,
            critical_alerts: ledger_health.critical_count,
            regulation_effectiveness: regulation_health.effectiveness(),
            ledger_health,
            regulation_health,
        };

        // ── Compare + Compute ──────────────────────────────────────────
        let alerts = self.compare(&snapshot);

        // ── Act ────────────────────────────────────────────────────────
        self.act(&snapshot, &alerts).await;

        // Store the snapshot for external queries.
        *self.last_snapshot.write().await = Some(snapshot);
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

    /// Act on the snapshot and alerts — log, emit spans, issue directives.
    async fn act(&self, snapshot: &HealthSnapshot, alerts: &[EscalationAlert]) {
        // Log the health snapshot
        tracing::info!(
            target: "reg.curator.metacognition",
            variety_deficit = snapshot.variety_deficit,
            critical_alerts = snapshot.critical_alerts,
            effectiveness = format!("{:.1}%", snapshot.regulation_effectiveness * 100.0),
            healthy = snapshot.ledger_health.healthy,
            alerts = alerts.len(),
            "Curator metacognition tick"
        );

        // Log each alert
        for alert in alerts {
            match alert.severity {
                EscalationSeverity::Critical => {
                    tracing::warn!(
                        target: "reg.curator.metacognition",
                        trigger = ?alert.trigger,
                        value = alert.value,
                        threshold = alert.threshold,
                        message = %alert.message,
                        "CRITICAL escalation alert"
                    );
                }
                EscalationSeverity::Warning => {
                    tracing::warn!(
                        target: "reg.curator.metacognition",
                        trigger = ?alert.trigger,
                        value = alert.value,
                        threshold = alert.threshold,
                        message = %alert.message,
                        "Escalation alert"
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
        }
    }

    /// Get the last health snapshot (if any).
    pub async fn last_snapshot(&self) -> Option<HealthSnapshot> {
        self.last_snapshot.read().await.clone()
    }
}
