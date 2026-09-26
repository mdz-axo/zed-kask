//! Bridge between `hkask_regulation::MetacognitionLoop` and the agent's
//! `MetacognitionProvider` trait.
//!
//! The composition root constructs a `MetacognitionLoop` and wraps it in a
//! `BridgeMetacognitionProvider` so the `CuratorStatusTool` can read health
//! snapshots from the agent's tool surface.

use std::sync::Arc;

use gpui::Task;
use hkask_regulation::MetacognitionLoop;
use serde_json::json;

/// Adapter that implements `agent::MetacognitionProvider` over a
/// `MetacognitionLoop`.
pub struct BridgeMetacognitionProvider {
    loop_: Arc<MetacognitionLoop>,
    /// Memory-health probe — the curator's self-awareness of its own memory
    /// outage. When set, the snapshot includes a `memory` section so the
    /// curator (and `CuratorStatusTool` callers) can distinguish "regulation
    /// healthy, memory down" from "all healthy". `None` at startup or when
    /// the real memory port failed to construct.
    memory_port: Option<Arc<crate::memory::RealMemoryPort>>,
    /// The durable review queue `curator_escalations` reads. Its pending
    /// count is what "awaiting operator review" means; `None` (not wired)
    /// and a failed read both surface as `pending_escalations: null`.
    escalation_queue: Option<Arc<hkask_storage::EscalationQueue>>,
}

impl BridgeMetacognitionProvider {
    pub fn new(loop_: Arc<MetacognitionLoop>) -> Self {
        Self {
            loop_,
            memory_port: None,
            escalation_queue: None,
        }
    }

    /// Attach the durable escalation queue (composition root, deferred task).
    pub fn with_escalation_queue(mut self, queue: Arc<hkask_storage::EscalationQueue>) -> Self {
        self.escalation_queue = Some(queue);
        self
    }

    /// Attach the memory-health probe (composition root, deferred task).
    pub fn with_memory_port(mut self, port: Arc<crate::memory::RealMemoryPort>) -> Self {
        self.memory_port = Some(port);
        self
    }
}

impl agent::MetacognitionProvider for BridgeMetacognitionProvider {
    fn health_snapshot_json(&self) -> Task<Option<serde_json::Value>> {
        // `last_snapshot_blocking` uses a parking_lot RwLock read — it parks
        // the thread briefly if the metacognition loop is mid-write, then
        // returns. Safe to call synchronously from `Task::ready`.
        let result = self.loop_.last_snapshot_blocking().map(|s| {
            let mut snapshot = json!({
                "timestamp": s.timestamp.to_rfc3339(),
                "variety_deficit": s.variety_deficit,
                "critical_alerts": s.critical_alerts,
                "regulation_acceptance_rate": s.regulation_acceptance_rate,
                "escalation_count": s.escalation_count,
                "healthy": s.ledger_health.healthy,
                "total_cycles": s.regulation_health.total_cycles,
                "accepted": s.regulation_health.accepted,
                "staged": s.regulation_health.staged,
                "blocked": s.regulation_health.blocked,
                // Alert log cap status is an observation for backlog review;
                // the in-memory log self-evicts. This is not authorization to
                // clear it as part of algedonic-review.
                "alert_log_count": s.ledger_health.alert_log_count,
                "alert_log_cap": s.ledger_health.alert_log_cap,
                "alert_log_approaching_cap": s.ledger_health.alert_log_approaching_cap,
                // Trust/absence assembly verdict (Fermi LoopView). The reading
                // distinguishes wiring-closed from turning from working — the
                // dominant failure mode is a loop that reports success while
                // having never run.
                "loop_reading": s.loop_view.reading.to_string(),
                "loop_model": format!("{:?}", s.loop_view.loop_model).to_lowercase(),
                "panel_absence": format!("{:?}", s.loop_view.panel_absence).to_lowercase(),
                "outcome_trust": format!("{:?}", s.loop_view.outcome_trust).to_lowercase(),
                "liveness_trust": format!("{:?}", s.loop_view.liveness_trust).to_lowercase(),
            });
            // Merge the memory-health section — flat keys, so the merge is
            // just inserting the `memory` object. A degraded curator memory
            // store surfaces as `memory.degraded: true`, which the curator's
            // regulation loop can escalate on.
            if let Some(ref port) = self.memory_port {
                snapshot["memory"] = port.memory_health_json();
            }
            snapshot["pending_escalations"] =
                pending_escalations_json(self.escalation_queue.as_deref());
            // Declared human doors for Manual/Prompted stages (Fermi
            // STAGE_ACTIONS). Each entry is (trigger, stage, [tool_names]).
            let doors = self.loop_.stage_actions().all_doors();
            if !doors.is_empty() {
                snapshot["declared_doors"] = json!(
                    doors
                        .iter()
                        .map(|(trigger, stage, tools)| {
                            json!({
                                "trigger": format!("{:?}", trigger).to_lowercase(),
                                "stage": stage,
                                "tools": tools,
                            })
                        })
                        .collect::<Vec<_>>()
                );
            }
            snapshot
        });
        Task::ready(result)
    }
}

/// Pending durable escalations, or `null` when the queue is not wired or
/// cannot be read — never 0 for an unknown count.
fn pending_escalations_json(queue: Option<&hkask_storage::EscalationQueue>) -> serde_json::Value {
    let Some(queue) = queue else {
        return serde_json::Value::Null;
    };
    match queue.list_pending() {
        Ok(pending) => json!(pending.len()),
        Err(error) => {
            tracing::warn!(
                target: "reg.storage",
                error = %error,
                "escalation queue read failed — status reports pending escalations as unknown"
            );
            serde_json::Value::Null
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The status summary's "awaiting review" number is the durable queue's
    /// pending count; an unwired queue is unknown (null), never 0.
    #[test]
    fn pending_escalations_reports_queue_count_or_unknown() {
        assert_eq!(pending_escalations_json(None), serde_json::Value::Null);

        let queue = hkask_storage::EscalationQueue::from_driver(
            hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
        )
        .expect("queue");
        assert_eq!(pending_escalations_json(Some(&queue)), json!(0));
        for output in ["first", "second"] {
            queue
                .add(
                    hkask_types::TemplateID::new(),
                    hkask_types::BotID::new(),
                    output.into(),
                    1.0,
                    0,
                    "{}".into(),
                )
                .expect("add");
        }
        assert_eq!(pending_escalations_json(Some(&queue)), json!(2));
    }
}
