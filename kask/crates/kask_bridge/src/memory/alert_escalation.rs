//! Alert escalation sink + queue opener — extracted from `memory.rs`
//! (deep-module split: the algedonic alert path implements a *different*
//! port — `hkask_regulation::AlertEscalationSink` — with zero coupling to the
//! memory port). `open_curator_escalation_queue` borrows `curator_db_path`
//! from the parent's re-export of `curator_stores`.

use hkask_storage::open_or_repair;
use std::sync::Arc;

use super::curator_db_path;

/// Open an `EscalationQueue` (reviewable alert backlog) on the curator's
/// sovereign `curator.db` — the same DB the curator MCP server's
/// `curator_escalations` / `curator_escalation_resolve` /
/// `curator_escalation_dismiss` tools read. Returns `None` on any failure;
/// the caller degrades to no escalation-queue persistence with a warn.
///
/// Mirrors the regulation archive opener (`open_regulation_archive`) —
/// same DB, same passphrase, same resolution path. The queue is the
/// primary durable path for alert review: `CyberneticsLoop` writes
/// escalated alerts here unconditionally so the Curator/user can review and
/// resolve them.
pub fn open_curator_escalation_queue(
    passphrase: &str,
) -> Option<Arc<hkask_storage::EscalationQueue>> {
    let db_path = curator_db_path();
    let db = match open_or_repair(&db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                db_path = %db_path,
                "Failed to open curator DB for escalation queue"
            );
            return None;
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, "Failed to get SQLite pool for escalation queue");
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path.as_str()),
    );
    match hkask_storage::EscalationQueue::from_driver(driver) {
        Ok(queue) => Some(Arc::new(queue)),
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, "Failed to init EscalationQueue schema");
            None
        }
    }
}
/// Adapter implementing `hkask_regulation::AlertEscalationSink` by forwarding
/// algedonic alerts to the `EscalationQueue` (the reviewable backlog on the
/// curator's `curator.db`).
///
/// This closes the Store seam: `CyberneticsLoop` calls
/// `persist_alert_to_queue` → this adapter → `EscalationQueue::add` → the
/// `escalations` table → `curator_escalations` MCP tool reads it. The queue
/// write is best-effort; a failing or missing queue never breaks the
/// regulation loop.
pub struct BridgeAlertEscalationSink {
    queue: Arc<hkask_storage::EscalationQueue>,
}

impl BridgeAlertEscalationSink {
    pub fn new(queue: Arc<hkask_storage::EscalationQueue>) -> Self {
        Self { queue }
    }

    fn reconcile_conditions_at(
        &self,
        observations: &[hkask_regulation::Signal],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<hkask_regulation::AdviceReviewReconciliation, hkask_regulation::AlertPersistError>
    {
        let entries = self
            .queue
            .list_advice_observations()
            .map_err(|error| hkask_regulation::AlertPersistError::QueueRead(error.to_string()))?;
        let mut reconciliation = hkask_regulation::AdviceReviewReconciliation::default();
        for entry in entries {
            let mut context: serde_json::Value = match serde_json::from_str(&entry.error_context) {
                Ok(context) => context,
                Err(error) => {
                    tracing::warn!(target: "reg.alert", %error, "Invalid escalation context");
                    continue;
                }
            };
            if context
                .get("applied_at")
                .is_some_and(|value| !value.is_null())
            {
                reconciliation.interventions_confirmed += 1;
            }
            let Some(value) = context
                .get("recovery_signal")
                .filter(|value| !value.is_null())
            else {
                continue;
            };
            let trigger: hkask_regulation::Signal = match serde_json::from_value(value.clone()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::warn!(target: "reg.alert", %error, "Invalid recovery signal");
                    continue;
                }
            };
            if !trigger.is_recovery_trigger() {
                tracing::warn!(target: "reg.alert", "Unmeasurable recovery trigger; condition retained");
                continue;
            }
            let current = observations
                .iter()
                .find(|current| current.metric == trigger.metric && current.is_fresh_at(now));
            let mut observed_context = entry.error_context.clone();
            if context
                .pointer("/advice_review/finalized")
                .and_then(|value| value.as_bool())
                != Some(true)
            {
                let applied_at = context
                    .get("applied_at")
                    .filter(|value| !value.is_null())
                    .map(|value| {
                        serde_json::from_value::<chrono::DateTime<chrono::Utc>>(value.clone())
                    })
                    .transpose();
                let review_due_at = context
                    .get("review_due_at")
                    .filter(|value| !value.is_null())
                    .map(|value| {
                        serde_json::from_value::<chrono::DateTime<chrono::Utc>>(value.clone())
                    })
                    .transpose();
                let baseline = context
                    .get("applied_baseline")
                    .filter(|value| !value.is_null())
                    .map(|value| serde_json::from_value::<hkask_regulation::Signal>(value.clone()))
                    .transpose();
                let (Ok(applied_at), Ok(review_due_at), Ok(baseline)) =
                    (applied_at, review_due_at, baseline)
                else {
                    tracing::warn!(target: "reg.alert", "Invalid advice application metadata; review not performed");
                    continue;
                };
                let status = trigger.advice_review(
                    baseline.as_ref(),
                    current,
                    applied_at,
                    review_due_at,
                    now,
                );
                let finalized = !matches!(status, "awaiting_action" | "observation_window");
                let receipt_id = finalized.then(hkask_types::EventID::new);
                context["latest_observation"] = serde_json::json!(current);
                context["advice_review"] = serde_json::json!({
                    "status": status,
                    "observed_at": now,
                    "causal_attribution": "unverified",
                    "finalized": finalized,
                    "receipt_id": receipt_id,
                    "telemetry_published_at": null,
                });
                match self.queue.update_advice_context(
                    &entry.id.to_string(),
                    &entry.error_context,
                    &context.to_string(),
                ) {
                    Ok(true) => {
                        observed_context = context.to_string();
                    }
                    Ok(false) => {
                        tracing::debug!(target: "reg.alert", "Advice changed concurrently; retry on next tick");
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(target: "reg.alert", %error, "Advice observation could not be saved");
                        continue;
                    }
                }
            }
            if current.is_some_and(|current| trigger.recovered_by(current)) {
                match self.queue.resolve_observed_condition(
                    &entry.id.to_string(),
                    &observed_context,
                    "cybernetics_loop:auto_resolve",
                ) {
                    Ok(true) => {
                        tracing::info!(target: "reg.alert", "Resolved observed condition at its original threshold")
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(target: "reg.alert", %error, "Recovery could not be persisted; condition retained")
                    }
                }
            }
            if let Some(receipt) = Self::pending_receipt(&entry.id.to_string(), &context)? {
                reconciliation.pending_receipts.push(receipt);
            }
        }
        Ok(reconciliation)
    }

    fn pending_receipt(
        escalation_id: &str,
        context: &serde_json::Value,
    ) -> Result<Option<hkask_regulation::AdviceReviewReceipt>, hkask_regulation::AlertPersistError>
    {
        let review = &context["advice_review"];
        if review["finalized"].as_bool() != Some(true)
            || !review["telemetry_published_at"].is_null()
        {
            return Ok(None);
        }
        let Some(receipt_id) = review["receipt_id"].as_str() else {
            return Ok(None);
        };
        let event_id = receipt_id.parse().map_err(|error| {
            hkask_regulation::AlertPersistError::InvalidAdviceReview(format!(
                "invalid receipt id for escalation {escalation_id}: {error}"
            ))
        })?;
        let outcome = match review["status"].as_str() {
            Some("recovered") => hkask_regulation::AdviceReviewOutcome::Recovered,
            Some("improved") => hkask_regulation::AdviceReviewOutcome::Improved,
            Some("no_improvement") => hkask_regulation::AdviceReviewOutcome::NoImprovement,
            Some("insufficient_evidence") => {
                hkask_regulation::AdviceReviewOutcome::InsufficientEvidence
            }
            status => {
                return Err(hkask_regulation::AlertPersistError::InvalidAdviceReview(
                    format!("invalid finalized status for escalation {escalation_id}: {status:?}"),
                ));
            }
        };
        if review["causal_attribution"].as_str() != Some("unverified") {
            return Err(hkask_regulation::AlertPersistError::InvalidAdviceReview(
                format!("invalid causal attribution for escalation {escalation_id}"),
            ));
        }
        Ok(Some(hkask_regulation::AdviceReviewReceipt {
            event_id,
            escalation_id: escalation_id.to_string(),
            outcome,
            causal_attribution: hkask_regulation::AdviceReviewCausalAttribution::Unverified,
        }))
    }

    fn acknowledge_advice_review_at(
        &self,
        receipt: &hkask_regulation::AdviceReviewReceipt,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, hkask_regulation::AlertPersistError> {
        let Some(entry) = self
            .queue
            .get(&receipt.escalation_id)
            .map_err(|error| hkask_regulation::AlertPersistError::QueueRead(error.to_string()))?
        else {
            return Ok(false);
        };
        let mut context: serde_json::Value =
            serde_json::from_str(&entry.error_context).map_err(|error| {
                hkask_regulation::AlertPersistError::InvalidAdviceReview(error.to_string())
            })?;
        if context["advice_review"]["receipt_id"].as_str()
            != Some(receipt.event_id.to_string().as_str())
        {
            return Ok(false);
        }
        if !context["advice_review"]["telemetry_published_at"].is_null() {
            return Ok(true);
        }
        context["advice_review"]["telemetry_published_at"] = serde_json::json!(now);
        self.queue
            .update_advice_context(
                &receipt.escalation_id,
                &entry.error_context,
                &context.to_string(),
            )
            .map_err(|error| hkask_regulation::AlertPersistError::QueueWrite(error.to_string()))
    }
}

impl BridgeAlertEscalationSink {
    /// The reporting core both `persist_alert` and `try_persist_alert`
    /// delegate to. Supersede at the source: a pending escalation for the
    /// same condition is updated in place (latest output/context,
    /// retry_count+1) instead of appending a duplicate row per re-sensed
    /// cycle. The condition key strips the per-cycle value
    /// (`alert_condition`), so a persistent deficit — whose embedded
    /// value changes every tick — updates ONE reviewable row rather than
    /// flooding the queue. The operator reviews that row; when they
    /// resolve or dismiss it, the next cycle inserts a fresh one.
    fn persist_alert_reporting(
        &self,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<hkask_regulation::AlertQueueOutcome, hkask_regulation::AlertPersistError> {
        let condition = hkask_regulation::alert_condition(output);
        match self.queue.supersede_pending_by_condition(
            condition,
            output,
            confidence,
            error_context,
        ) {
            Ok(true) => {
                tracing::debug!(
                    target: "reg.alert",
                    "Superseded pending escalation — condition re-fired while pending"
                );
                // The row is confirmed in the queue; a supersede updates the
                // existing entry, so no new id exists to report.
                return Ok(hkask_regulation::AlertQueueOutcome::Confirmed(None));
            }
            Ok(false) => {} // no existing pending — insert below
            Err(e) => {
                // Supersede failed — don't block the insert. Best-effort:
                // a failing dedup query is preferable to losing the alert.
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Supersede check failed — proceeding to insert without dedup"
                );
            }
        }

        // `EscalationQueue::add` requires `template_id` and `bot_id` args that
        // don't map from a `RuntimeAlert` — use auto-generated defaults (the
        // same defaults `EscalationEntry::pending` uses). The structured alert
        // fields are preserved in `error_context` (JSON).
        let template_id = hkask_types::TemplateID::new();
        let bot_id = hkask_types::BotID::new();
        match self
            .queue
            .add(
                template_id,
                bot_id,
                output.to_string(),
                confidence,
                0,
                error_context.to_string(),
            )
            .map(|id| hkask_regulation::AlertQueueOutcome::Confirmed(Some(id.to_string())))
            .map_err(|e| hkask_regulation::AlertPersistError::QueueWrite(e.to_string()))
        {
            Ok(outcome) => {
                if let hkask_regulation::AlertQueueOutcome::Confirmed(Some(ref id)) = outcome {
                    tracing::debug!(
                        target: "reg.alert",
                        escalation_id = %id,
                        "Algedonic alert persisted to escalation queue"
                    );
                }
                Ok(outcome)
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Failed to persist algedonic alert to escalation queue"
                );
                Err(e)
            }
        }
    }
}

impl hkask_regulation::AlertEscalationSink for BridgeAlertEscalationSink {
    fn reconcile_conditions(
        &self,
        observations: &[hkask_regulation::Signal],
    ) -> Result<hkask_regulation::AdviceReviewReconciliation, hkask_regulation::AlertPersistError>
    {
        self.reconcile_conditions_at(observations, chrono::Utc::now())
    }

    fn acknowledge_advice_review(
        &self,
        receipt: &hkask_regulation::AdviceReviewReceipt,
    ) -> Result<bool, hkask_regulation::AlertPersistError> {
        self.acknowledge_advice_review_at(receipt, chrono::Utc::now())
    }

    fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
        // Best-effort: the outcome is logged inside the reporting core and
        // discarded here — the legacy contract.
        let _ = self.persist_alert_reporting(output, confidence, error_context);
    }

    fn try_persist_alert(
        &self,
        output: &str,
        confidence: f64,
        error_context: &str,
    ) -> Result<hkask_regulation::AlertQueueOutcome, hkask_regulation::AlertPersistError> {
        self.persist_alert_reporting(output, confidence, error_context)
    }

    fn has_pending_alert(&self, output: &str) -> bool {
        // Condition match, not exact output: the pending escalation's
        // embedded value differs from this cycle's, so exact matching
        // never suppresses a re-sensed condition.
        let condition = hkask_regulation::alert_condition(output);
        match self.queue.has_pending_with_condition(condition) {
            Ok(true) => true,
            Ok(false) => false,
            Err(e) => {
                tracing::debug!(
                    target: "reg.alert",
                    error = %e,
                    "Dedup query failed — assuming no pending alert"
                );
                false
            }
        }
    }

    fn auto_resolve_cleared(&self, output: &str, resolution_note: &str) {
        // Condition match, not exact output: the persisted escalation and
        // the clearing cycle's reconstruction embed different values (they
        // were sensed in different cycles), so exact matching would leave
        // the stale escalation pending forever.
        let condition = hkask_regulation::alert_condition(output);
        match self
            .queue
            .resolve_pending_by_condition(condition, "cybernetics_loop:auto_resolve")
        {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        target: "reg.alert",
                        count = count,
                        note = %resolution_note,
                        "Auto-resolved pending escalation — triggering condition cleared"
                    );
                } else {
                    // No pending escalation with this condition — either it
                    // was already resolved/dismissed by the operator, or the
                    // condition never escalated. Not an error; the condition
                    // is clear.
                    tracing::debug!(
                        target: "reg.alert",
                        "Auto-resolve found no pending escalation with this condition — already cleared or not found"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Auto-resolve query failed — escalation remains pending"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_regulation::AlertEscalationSink as _;
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

    fn in_memory_queue() -> Arc<hkask_storage::EscalationQueue> {
        Arc::new(
            hkask_storage::EscalationQueue::from_driver(
                hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            )
            .expect("queue"),
        )
    }

    struct ScriptedAdviceDriver {
        inner: Arc<dyn hkask_storage::DatabaseDriver>,
        next_advice_update: Arc<AtomicU8>,
    }

    impl hkask_storage::DatabaseDriver for ScriptedAdviceDriver {
        fn execute(
            &self,
            sql: &str,
            params: &[hkask_storage::database::value::DbValue],
        ) -> Result<usize, hkask_types::DbError> {
            if sql.starts_with("UPDATE escalations SET error_context") {
                match self.next_advice_update.swap(0, Ordering::SeqCst) {
                    1 => {
                        return Err(hkask_types::DbError::Database(
                            "scripted advice persistence failure".to_string(),
                        ));
                    }
                    2 => return Ok(0),
                    _ => {}
                }
            }
            self.inner.execute(sql, params)
        }

        fn execute_batch(&self, sql: &str) -> Result<(), hkask_types::DbError> {
            self.inner.execute_batch(sql)
        }

        fn query(
            &self,
            sql: &str,
            params: &[hkask_storage::database::value::DbValue],
        ) -> Result<Vec<hkask_storage::database::value::DbRow>, hkask_types::DbError> {
            self.inner.query(sql, params)
        }

        fn query_optional(
            &self,
            sql: &str,
            params: &[hkask_storage::database::value::DbValue],
        ) -> Result<Option<hkask_storage::database::value::DbRow>, hkask_types::DbError> {
            self.inner.query_optional(sql, params)
        }

        fn commit_tx(&self) -> Result<(), hkask_types::DbError> {
            self.inner.commit_tx()
        }

        fn rollback_tx(&self) -> Result<(), hkask_types::DbError> {
            self.inner.rollback_tx()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn is_durable(&self) -> bool {
            self.inner.is_durable()
        }
    }

    fn scripted_queue() -> (Arc<hkask_storage::EscalationQueue>, Arc<AtomicU8>) {
        let next_advice_update = Arc::new(AtomicU8::new(0));
        let driver = ScriptedAdviceDriver {
            inner: hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            next_advice_update: next_advice_update.clone(),
        };
        let queue = hkask_storage::EscalationQueue::from_driver(Arc::new(driver)).expect("queue");
        (Arc::new(queue), next_advice_update)
    }

    /// T08: `try_persist_alert` reports the durable-write truth against a
    /// real queue — a new insert is Confirmed with the row's id and the row
    /// is readable with the exact payload; a re-fired condition supersedes
    /// the pending row (Confirmed, no new id) instead of duplicating it.
    #[test]
    fn try_persist_alert_reports_confirmed_insert_and_supersede() {
        let queue = in_memory_queue();
        let sink = BridgeAlertEscalationSink::new(queue.clone());
        let context =
            serde_json::json!({"explicit": true, "domain": "storage", "severity": "warning", "evidence": "variety deficit"})
                .to_string();

        // First delivery: confirmed insert with a readable id.
        let first = sink
            .try_persist_alert(
                "Explicit escalation (storage, warning) — first",
                0.5,
                &context,
            )
            .expect("insert");
        let hkask_regulation::AlertQueueOutcome::Confirmed(Some(id)) = first else {
            panic!("a fresh insert must report Confirmed with the row id: {first:?}")
        };
        let entry = queue.get(&id).expect("get").expect("entry");
        assert_eq!(
            entry.output,
            "Explicit escalation (storage, warning) — first"
        );
        assert_eq!(entry.confidence, 0.5);

        // Same condition re-fired: the pending row is superseded in place —
        // confirmed in queue, no new id, still exactly one pending row.
        let second = sink
            .try_persist_alert(
                "Explicit escalation (storage, warning) — updated",
                0.5,
                &context,
            )
            .expect("supersede");
        assert_eq!(
            second,
            hkask_regulation::AlertQueueOutcome::Confirmed(None),
            "a supersede updates the existing row — confirmed, no new id"
        );
        let pending = queue.list_pending().expect("pending");
        assert_eq!(pending.len(), 1, "supersede must not duplicate the row");
        assert_eq!(
            pending[0].output,
            "Explicit escalation (storage, warning) — updated"
        );
    }

    /// T08 control: the legacy best-effort `persist_alert` still writes
    /// through the same core (its outcome is logged inside, not returned).
    #[test]
    fn persist_alert_still_writes_through_the_reporting_core() {
        let queue = in_memory_queue();
        let sink = BridgeAlertEscalationSink::new(queue.clone());
        sink.persist_alert(
            "Explicit escalation (storage, critical) — legacy",
            1.0,
            "{}",
        );
        let pending = queue.list_pending().expect("pending");
        assert_eq!(pending.len(), 1, "the legacy path must still write the row");
        assert_eq!(pending[0].confidence, 1.0);
    }

    /// Capturing `RegulationSink` — records every persisted span's path and
    /// observation so the directive acknowledgment can be asserted.
    struct CapturingAckSink(std::sync::Mutex<Vec<(String, serde_json::Value)>>);

    impl hkask_types::RegulationSink for CapturingAckSink {
        fn persist(
            &self,
            event: &hkask_types::RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((event.span.path.clone(), event.observation.clone()));
            Ok(())
        }
    }

    /// T08 end-to-end: an `EscalateDomain` directive through the inbox, with
    /// the real `BridgeAlertEscalationSink` and a real in-memory escalation
    /// store — the full bridge→inbox→queue chain. The queue row retains the
    /// concern's identity, and the acknowledgment reports "queued" with the
    /// escalation id matching the real queue row.
    #[tokio::test]
    async fn escalate_domain_directive_lands_in_real_queue_end_to_end() {
        let queue = in_memory_queue();
        let acks = Arc::new(CapturingAckSink(std::sync::Mutex::new(Vec::new())));
        let ledger = Arc::new(tokio::sync::RwLock::new(
            hkask_regulation::RegulationLedger::default(),
        ));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut regulation_loop = hkask_regulation::CyberneticsLoop::new(ledger)
            .with_event_sink(Arc::clone(&acks) as Arc<dyn hkask_types::RegulationSink>)
            .with_curator_directive_channel(rx);
        regulation_loop.set_alert_escalation_sink(Some(Arc::new(BridgeAlertEscalationSink::new(
            queue.clone(),
        ))));

        tx.send(hkask_types::CuratorDirective::EscalateDomain {
            domain: "storage".to_string(),
            severity: hkask_types::curator::EscalationSeverity::Warning,
            evidence: "variety deficit".to_string(),
        })
        .expect("send escalation");
        regulation_loop.process_inbox().await;

        // The real queue holds the concern with its identity — no fabricated
        // deficit/threshold fields.
        let pending = queue.list_pending().expect("pending");
        assert_eq!(pending.len(), 1, "exactly one queue row");
        let entry = &pending[0];
        assert!(
            entry.output.contains("storage") && entry.output.contains("warning"),
            "the queue output must name the domain and severity: {}",
            entry.output
        );
        let context: serde_json::Value =
            serde_json::from_str(&entry.error_context).expect("context");
        assert_eq!(context["domain"], "storage");
        assert_eq!(context["severity"], "warning");
        assert_eq!(context["evidence"], "variety deficit");
        assert_eq!(context["explicit"], true);
        assert!(context.get("deficit").is_none() && context.get("threshold").is_none());

        // The acknowledgment reports the confirmed queue outcome with the
        // real row's id and the concern's identity.
        let records = acks
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(path, _)| path.contains("directive_acknowledged"))
            .map(|(_, observation)| observation.clone())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1, "one acknowledgment: {records:#?}");
        let ack = &records[0];
        assert_eq!(ack["outcome"], "queued");
        assert_eq!(
            ack["escalation_id"],
            serde_json::Value::String(entry.id.to_string()),
            "the ack's escalation id must match the real queue row"
        );
        assert_eq!(ack["domain"], "storage");
        assert_eq!(ack["severity"], "warning");
        assert_eq!(ack["evidence"], "variety deficit");
    }

    fn reliability(
        value: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> hkask_regulation::Signal {
        serde_json::from_value(serde_json::json!({"source":"cybernetics", "metric":"tool_reliability", "value":value, "set_point":0.8, "timestamp":timestamp})).expect("signal")
    }

    /// expect: "Persisted advice is assessed after seven days, including after early resolution, without causal claims" [P9]
    #[test]
    fn persisted_advice_review_keeps_observing_after_recovery() {
        let queue = Arc::new(
            hkask_storage::EscalationQueue::from_driver(
                hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            )
            .expect("queue"),
        );
        let sink = BridgeAlertEscalationSink::new(queue.clone());
        let applied = chrono::Utc::now();
        let trigger = reliability(0.2, applied);
        let id = queue.add(hkask_types::TemplateID::new(), hkask_types::BotID::new(), "reliability — first".into(), 1.0, 0,
            serde_json::json!({"recovery_signal":trigger, "applied_at":applied, "review_due_at":applied + chrono::Duration::days(7), "applied_baseline":trigger, "action_note":"fixed"}).to_string()).expect("add").to_string();
        let context = || -> serde_json::Value {
            serde_json::from_str(&queue.get(&id).expect("get").expect("entry").error_context)
                .expect("context")
        };
        let early = applied + chrono::Duration::days(1);
        sink.reconcile_conditions_at(&[reliability(1.0, early)], early)
            .expect("early reconciliation");
        let resolved_at = queue
            .get(&id)
            .expect("get")
            .expect("entry")
            .resolved_at
            .expect("early recovery");
        assert_eq!(context()["advice_review"]["status"], "observation_window");
        assert_eq!(context()["advice_review"]["finalized"], false);
        // A recurrence must not be resolved using the old alert's lower threshold.
        let mut recurrence = reliability(0.85, early);
        recurrence.set_point = 0.95;
        let next = queue
            .add(
                hkask_types::TemplateID::new(),
                hkask_types::BotID::new(),
                "reliability — next".into(),
                1.0,
                0,
                serde_json::json!({"recovery_signal":recurrence}).to_string(),
            )
            .expect("recurrence");
        let due = applied + chrono::Duration::days(7);
        sink.reconcile_conditions_at(&[reliability(0.9, due)], due)
            .expect("due reconciliation");
        assert_eq!(context()["advice_review"]["status"], "recovered");
        assert_eq!(
            context()["advice_review"]["causal_attribution"],
            "unverified"
        );
        assert_eq!(
            queue
                .get(&next.to_string())
                .expect("get")
                .expect("entry")
                .status,
            hkask_storage::EscalationStatus::Pending
        );
        assert_eq!(
            queue.get(&id).expect("get").expect("entry").resolved_at,
            Some(resolved_at)
        );
        sink.reconcile_conditions_at(&[], due + chrono::Duration::days(1))
            .expect("repeated reconciliation");
        assert_eq!(
            context()["advice_review"]["status"],
            "recovered",
            "completed assessment is retained"
        );
    }

    /// expect: "Absent or stale advice evidence stays unknown and cannot resolve a pending condition" [P9]
    #[test]
    fn persisted_advice_review_evidence_matrix() {
        let applied = chrono::Utc::now();
        let due = applied + chrono::Duration::days(7);
        for (baseline, current, status) in [
            (
                Some(reliability(0.2, applied)),
                Some(reliability(0.3, due)),
                "improved",
            ),
            (
                Some(reliability(0.2, applied)),
                Some(reliability(0.2, due)),
                "no_improvement",
            ),
            (None, Some(reliability(0.3, due)), "insufficient_evidence"),
            (
                Some(reliability(0.2, applied - chrono::Duration::seconds(61))),
                Some(reliability(0.3, due)),
                "insufficient_evidence",
            ),
            (
                Some(reliability(0.2, applied)),
                None,
                "insufficient_evidence",
            ),
            (
                Some(reliability(0.2, applied)),
                Some(reliability(1.0, due - chrono::Duration::seconds(61))),
                "insufficient_evidence",
            ),
            (
                Some(reliability(0.2, applied)),
                Some(reliability(1.0, due + chrono::Duration::seconds(1))),
                "insufficient_evidence",
            ),
        ] {
            let queue = Arc::new(
                hkask_storage::EscalationQueue::from_driver(
                    hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
                )
                .expect("queue"),
            );
            let id = queue.add(hkask_types::TemplateID::new(), hkask_types::BotID::new(), "reliability".into(), 1.0, 0,
                serde_json::json!({"recovery_signal":reliability(0.2, applied), "applied_at":applied, "review_due_at":due, "applied_baseline":baseline}).to_string()).expect("add");
            let sink = BridgeAlertEscalationSink::new(queue.clone());
            sink.reconcile_conditions_at(&current.into_iter().collect::<Vec<_>>(), due)
                .expect("evidence reconciliation");
            let entry = queue.get(&id.to_string()).expect("get").expect("entry");
            let context: serde_json::Value =
                serde_json::from_str(&entry.error_context).expect("context");
            assert_eq!(context["advice_review"]["status"], status);
            assert_eq!(context["advice_review"]["causal_attribution"], "unverified");
            assert_eq!(entry.status, hkask_storage::EscalationStatus::Pending);
        }
    }

    /// expect: "A failed or conflicting final-review write emits no durable final state and remains retryable" [P9]
    #[test]
    fn advice_review_finalization_retries_after_persistence_failure_or_conflict() {
        let applied = chrono::Utc::now();
        let due = applied + chrono::Duration::days(7);
        for scripted_outcome in [1, 2] {
            let (queue, next_advice_update) = scripted_queue();
            let trigger = reliability(0.2, applied);
            let id = queue
                .add(
                    hkask_types::TemplateID::new(),
                    hkask_types::BotID::new(),
                    "tool reliability advice".into(),
                    1.0,
                    0,
                    serde_json::json!({
                        "recovery_signal": trigger,
                        "applied_at": applied,
                        "review_due_at": due,
                        "applied_baseline": trigger,
                        "advice_review": {
                            "status": "observation_window",
                            "finalized": false,
                            "causal_attribution": "unverified"
                        }
                    })
                    .to_string(),
                )
                .expect("add escalation")
                .to_string();
            let sink = BridgeAlertEscalationSink::new(queue.clone());

            next_advice_update.store(scripted_outcome, Ordering::SeqCst);
            sink.reconcile_conditions_at(&[reliability(1.0, due)], due)
                .expect("scripted failed reconciliation");
            let unchanged = queue.get(&id).expect("get").expect("entry");
            let unchanged: serde_json::Value =
                serde_json::from_str(&unchanged.error_context).expect("context");
            assert_eq!(unchanged["advice_review"]["finalized"], false);
            assert_eq!(unchanged["advice_review"]["status"], "observation_window");

            sink.reconcile_conditions_at(&[reliability(1.0, due)], due)
                .expect("scripted retry reconciliation");
            let retried = queue.get(&id).expect("get").expect("entry");
            let retried: serde_json::Value =
                serde_json::from_str(&retried.error_context).expect("context");
            assert_eq!(retried["advice_review"]["finalized"], true);
            assert_eq!(retried["advice_review"]["status"], "recovered");
            assert_eq!(retried["advice_review"]["causal_attribution"], "unverified");
        }
    }

    struct FailingInferencePort;

    impl hkask_types::InferencePort for FailingInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "test inference disabled".to_string(),
                ))
            })
        }
    }

    /// expect: "The persisted due time governs a restart-durable advice review from confirmed application through visible final outcome" [P9]
    #[tokio::test]
    async fn advice_review_crosses_tool_queue_sink_and_read_tool_at_persisted_due_time() {
        use hkask_mcp_curator::types::{AdviceAppliedRequest, PingRequest};
        use hkask_mcp_curator::{CuratorDb, CuratorServer, CuratorStores};
        use hkask_types::WebID;
        use rmcp::handler::server::wrapper::Parameters;

        let queue = in_memory_queue();
        let build_server = || {
            CuratorServer::new(
                WebID::new(),
                Arc::new(CuratorDb::from_stores(CuratorStores {
                    escalation_queue: Some(queue.clone()),
                    regulation_store: None,
                    memory: None,
                })),
                Arc::new(FailingInferencePort),
            )
        };
        let trigger = reliability(0.2, chrono::Utc::now());
        let id = queue
            .add(
                hkask_types::TemplateID::new(),
                hkask_types::BotID::new(),
                "tool reliability advice".into(),
                1.0,
                0,
                serde_json::json!({"recovery_signal":trigger}).to_string(),
            )
            .expect("add escalation")
            .to_string();

        let server = build_server();
        let applied = server
            .curator_advice_mark_applied(Parameters(AdviceAppliedRequest {
                id: id.clone(),
                operator_confirmed: true,
                action_note: "operator repaired tool service".into(),
                skill_id: None,
            }))
            .await
            .expect("confirmed application");
        let applied = hkask_types::tool_response::parse_tool_response(&applied)
            .expect("application response");
        let applied_at: chrono::DateTime<chrono::Utc> =
            serde_json::from_value(applied["applied_at"].clone()).expect("applied_at");
        let original_due: chrono::DateTime<chrono::Utc> =
            serde_json::from_value(applied["review_due_at"].clone()).expect("review_due_at");
        let postponed_due = original_due + chrono::Duration::days(1);
        let entry = queue.get(&id).expect("get").expect("entry");
        let mut context: serde_json::Value =
            serde_json::from_str(&entry.error_context).expect("context");
        assert!(
            !context["applied_baseline"].is_null(),
            "fresh application must persist a baseline"
        );
        context["review_due_at"] = serde_json::json!(postponed_due);
        assert!(
            queue
                .update_advice_context(&id, &entry.error_context, &context.to_string())
                .expect("postpone review"),
            "controlled due-time update must win its compare-and-set"
        );

        let sink = BridgeAlertEscalationSink::new(queue.clone());
        sink.reconcile_conditions_at(&[reliability(1.0, original_due)], original_due)
            .expect("original due reconciliation");
        let before_due = queue.get(&id).expect("get").expect("entry");
        let before_due_context: serde_json::Value =
            serde_json::from_str(&before_due.error_context).expect("context");
        assert_eq!(
            before_due_context["advice_review"]["status"], "observation_window",
            "persisted review_due_at, not applied_at arithmetic, governs finalization"
        );

        queue.resolve(&id, "operator").expect("early resolution");
        drop(sink);
        drop(server);

        let restarted_sink = BridgeAlertEscalationSink::new(queue.clone());
        let restarted_server = build_server();
        restarted_sink
            .reconcile_conditions_at(&[reliability(1.0, postponed_due)], postponed_due)
            .expect("postponed due reconciliation");
        let finalized = queue.get(&id).expect("get").expect("entry").error_context;
        restarted_sink
            .reconcile_conditions_at(&[reliability(1.0, postponed_due)], postponed_due)
            .expect("repeated postponed due reconciliation");
        assert_eq!(
            queue.get(&id).expect("get").expect("entry").error_context,
            finalized,
            "repeated reconciliation must not rewrite a finalized review"
        );

        let reviews = restarted_server
            .curator_advice_reviews(Parameters(PingRequest {}))
            .await
            .expect("review query");
        let reviews =
            hkask_types::tool_response::parse_tool_response(&reviews).expect("review response");
        let review = &reviews["reviews"][0];
        assert_eq!(review["status"], "resolved");
        assert_eq!(
            review["application"]["advice_review"]["status"],
            "recovered"
        );
        assert_eq!(
            review["application"]["advice_review"]["causal_attribution"],
            "unverified"
        );
        assert_eq!(
            serde_json::from_value::<chrono::DateTime<chrono::Utc>>(
                review["application"]["applied_at"].clone()
            )
            .expect("persisted applied_at"),
            applied_at
        );
    }

    /// expect: "A finalized queue review reaches Regulation exactly once through telemetry separate from rollout impact" [P9]
    #[tokio::test]
    async fn finalized_advice_review_reaches_regulation_once_as_observational_progress() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let queue =
            Arc::new(hkask_storage::EscalationQueue::from_driver(driver.clone()).expect("queue"));
        let archive = Arc::new(
            hkask_storage::RegulationArchive::from_driver(driver).expect("regulation archive"),
        );
        let applied = chrono::Utc::now();
        let due = applied + chrono::Duration::days(7);
        let trigger = reliability(0.2, applied);
        let id = queue
            .add(
                hkask_types::TemplateID::new(),
                hkask_types::BotID::new(),
                "tool reliability advice".into(),
                1.0,
                0,
                serde_json::json!({
                    "recovery_signal": trigger,
                    "applied_at": applied,
                    "review_due_at": due,
                    "applied_baseline": trigger,
                    "advice_review": {
                        "status": "observation_window",
                        "finalized": false,
                        "causal_attribution": "unverified"
                    }
                })
                .to_string(),
            )
            .expect("add escalation")
            .to_string();
        let sink = Arc::new(BridgeAlertEscalationSink::new(queue.clone()));
        sink.reconcile_conditions_at(&[reliability(1.0, due)], due)
            .expect("final reconciliation");
        let finalized: serde_json::Value =
            serde_json::from_str(&queue.get(&id).expect("get").expect("entry").error_context)
                .expect("context");
        assert_eq!(finalized["advice_review"]["status"], "recovered");

        let mut regulation = hkask_regulation::CyberneticsLoop::new(Arc::new(
            tokio::sync::RwLock::new(hkask_regulation::RegulationLedger::default()),
        ))
        .with_event_sink(archive.clone() as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_alert_escalation_sink(Some(sink));
        regulation.tick().await;
        regulation.tick().await;

        let records = archive
            .query_records(applied - chrono::Duration::seconds(1), None, 100)
            .expect("records");
        let review_records = records
            .iter()
            .filter(|record| record.span.path == "reg.outcome.advice_review_observed")
            .collect::<Vec<_>>();
        assert_eq!(
            review_records.len(),
            1,
            "one finalized transition must have one durable Regulation identity"
        );
        assert_eq!(review_records[0].observation["outcome"], "recovered");
        assert_eq!(
            review_records[0].observation["causal_attribution"],
            "unverified"
        );

        let loop_metrics = records
            .iter()
            .find(|record| {
                record.span.path == "reg.outcome.loop_quality"
                    && record.observation["advice_reviews_finalized"] == 1
            })
            .expect("loop telemetry carrying the finalized review");
        assert_eq!(loop_metrics.observation["advisories_computed"], 0);
        assert_eq!(loop_metrics.observation["interventions_confirmed"], 1);
        assert_eq!(loop_metrics.observation["rollout_impact_reports"], 0);
        assert_eq!(loop_metrics.observation["advice_reviews_recovered"], 1);
        assert_eq!(loop_metrics.observation["advice_reviews_improved"], 0);
        assert_eq!(loop_metrics.observation["advice_reviews_no_improvement"], 0);
        assert_eq!(
            loop_metrics.observation["advice_reviews_insufficient_evidence"],
            0
        );
        assert_eq!(
            loop_metrics.observation["advice_review_progress_score"],
            1.0
        );
        assert_eq!(
            loop_metrics.observation["advice_review_causal_attribution"],
            "unverified"
        );
        assert!(
            loop_metrics
                .observation
                .get("rollout_progress_score")
                .is_some(),
            "rollout progress must remain a separate telemetry channel"
        );
    }

    struct Fleet {
        healthy: AtomicUsize,
        total: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl hkask_regulation::ContextServerHealthSource for Fleet {
        async fn healthy_count(&self) -> usize {
            self.healthy.load(Ordering::SeqCst)
        }
        async fn total_count(&self) -> usize {
            self.total.load(Ordering::SeqCst)
        }
    }

    /// expect: "Recovery on a later tick resolves exactly the original condition, not partial or absent data" [P9]
    #[tokio::test]
    async fn later_tick_reconciles_durable_conditions() {
        let queue = Arc::new(
            hkask_storage::EscalationQueue::from_driver(
                hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            )
            .expect("queue"),
        );
        let sink = Arc::new(BridgeAlertEscalationSink::new(queue.clone()));
        let fleet = Arc::new(Fleet {
            healthy: AtomicUsize::new(2),
            total: AtomicUsize::new(10),
        });
        let build = || {
            let mut regulation = hkask_regulation::CyberneticsLoop::new(Arc::new(
                tokio::sync::RwLock::new(hkask_regulation::RegulationLedger::default()),
            ))
            .with_context_server_health_source(fleet.clone());
            regulation.set_alert_escalation_sink(Some(sink.clone()));
            regulation
        };
        let regulation = build();
        regulation.tick().await;
        let original = queue
            .list_pending()
            .expect("pending")
            .into_iter()
            .find(|entry| entry.error_context.contains("context_server_health"))
            .expect("fleet escalation");
        fleet.healthy.store(3, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        fleet.total.store(0, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        // A rebuilt loop must reconcile the persisted condition as well.
        drop(regulation);
        let regulation = build();
        fleet.total.store(10, Ordering::SeqCst);
        fleet.healthy.store(10, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            !queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        let resolved_at = queue
            .get(&original.id.to_string())
            .expect("entry")
            .expect("retained")
            .resolved_at;
        regulation.tick().await;
        assert_eq!(
            queue
                .get(&original.id.to_string())
                .expect("entry")
                .expect("retained")
                .resolved_at,
            resolved_at
        );
    }
}
