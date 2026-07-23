//! Curation Loop — pure regulatory observer (Loop 5)
//!
//! observe → evaluate → compose → regulate
//!
//! The Curation Loop is the ONLY loop that can override Cybernetics.
//! It observes system state and intervenes when Cybernetics
//! can't self-stabilize (e.g., alert cascade).
//!
//! # Curation / Agent Separation (Task 6)
//!
//! The Curation Loop is pure regulatory code — no persona, no chat behavior,
//! no memory. The Curator Agent (`crate::curator_agent::CuratorAgent`) is the
//! persona layer that holds metacognition, bot orchestration, and human-facing
//! reporting. The Curation Loop reads from the RegulationRecord store and produces
//! `CuratorDirective`s; the Curator Agent *consumes* those directives and
//! formats them for human operators.

use chrono::Utc;
use hkask_memory::ConsolidationBridge;
use hkask_regulation::types::loops::{
    CommunicationEvent, CurationInput, Deviation, GoalLifecycle, LoopId, RegulationLoop,
    RegulatoryAction, Signal, SignalMetric,
};
use hkask_types::ConsolidationRequest;
use hkask_types::DataCategory;
use hkask_types::curator::{CuratorDirective, CuratorHandle};
use hkask_types::{BotID, TemplateID};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, mpsc};

use crate::curation::context::CuratorContext;

const CUR_TARGET: &str = "curation.loop";

/// Default energy-budget override issued for high-confidence escalations when
/// no policy-driven value is available. Exposed as a named constant rather than
/// an inline magic number so it is auditable and overridable in future config.
const DEFAULT_ESCALATION_BUDGET_OVERRIDE: u64 = 5000;

/// Typed reason for a Curation `Escalate` action.
///
/// Replaces magic-string matching in `act()` — the canonical strings live
/// here as `as_str()`, so the producer (`compute()`) and consumer (`act()`)
/// share one source of truth and typos surface at compile time.
enum CurationEscalationReason {
    AlgedonicEventsExceeded,
    PendingEscalationsExist,
    ConsolidationCandidatesExist,
    GoalsStale,
    GoalsExpired,
    Other,
}

impl CurationEscalationReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::AlgedonicEventsExceeded => "algedonic_events_exceeded",
            Self::PendingEscalationsExist => "pending_escalations_exist",
            Self::ConsolidationCandidatesExist => "consolidation_candidates_exist",
            Self::GoalsStale => "goals_stale",
            Self::GoalsExpired => "goals_expired",
            Self::Other => "",
        }
    }

    fn from_reason(s: &str) -> Self {
        match s {
            "algedonic_events_exceeded" => Self::AlgedonicEventsExceeded,
            "pending_escalations_exist" => Self::PendingEscalationsExist,
            "consolidation_candidates_exist" => Self::ConsolidationCandidatesExist,
            "goals_stale" => Self::GoalsStale,
            "goals_expired" => Self::GoalsExpired,
            _ => Self::Other,
        }
    }
}

/// Curation Loop — pure regulatory observer.
///
/// Reads from the RegulationRecord store and produces `CuratorDirective`s through
/// Communication dispatch. All persona concerns (metacognition, bot
/// orchestration, human-facing reporting) belong in `CuratorAgent`.
///
/// **Singleton invariant:** There is exactly one `CurationLoop` per hKask
/// system. It owns the single `CuratorHandle`; all Curator capability access
/// flows through this instance via `CuratorContext::handle()`. The
/// `curator_handle` field makes the singleton relationship explicit at the
/// type level — it is not a runtime enforcement, but a structural guarantee
/// that the loop and its handle are co-located.
pub struct CurationLoop {
    curator_handle: CuratorHandle,
    context: Arc<CuratorContext>,
    consolidation: Option<Arc<ConsolidationBridge>>,
    /// Cursor for incremental algedonic review.
    last_review_ms: AtomicU64,
    /// Inbox for receiving CurationInput messages from Cybernetics, SpecCurator,
    /// and GoalStore. Drained during the sense phase.
    inbox: Option<Arc<RwLock<mpsc::UnboundedReceiver<CurationInput>>>>,
    /// Whether the Curator daemon may auto-consolidate memory when escalations exist.
    /// Default false; controlled by `ServiceConfig::curator_auto_consolidation_enabled`.
    auto_consolidation_enabled: bool,
}

impl CurationLoop {
    /// Create a new Curation Loop with a CuratorContext.
    ///
    /// The `curator_handle` is the single Curator capability handle for
    /// the system. Use `CuratorHandle::system()` to construct it.
    /// The `context` provides capability-disciplined access to Regulation, dispatch,
    /// and escalation — the Curation Loop's only runtime dependencies.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — Curation Loop is the regulatory sense-act loop
    /// \[P4\] Constraining: Clear Boundaries — single CuratorHandle capability
    /// pre:  `curator_handle` is a valid `CuratorHandle` (singleton);
    ///       `context` is a valid `Arc<CuratorContext>`.
    /// post: Returns a `CurationLoop` with no consolidation, no inbox,
    ///       and `last_review_ms` initialized to 0.
    pub fn new(curator_handle: CuratorHandle, context: Arc<CuratorContext>) -> Self {
        Self {
            curator_handle,
            context,
            consolidation: None,
            last_review_ms: AtomicU64::new(0),
            inbox: None,
            auto_consolidation_enabled: false,
        }
    }

    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — consolidation tunes the loop
    /// \[P7\] Constraining: Evolutionary Architecture — consolidation config emerged from usage
    /// pre:  `curator_handle` is a valid `CuratorHandle`; `context` is a
    ///       valid `Arc<CuratorContext>`; `consolidation` is a valid
    ///       `Arc<ConsolidationBridge>`.
    /// post: Returns a `CurationLoop` with consolidation set, no inbox,
    ///       and `last_review_ms` initialized to 0.
    pub fn with_consolidation(
        curator_handle: CuratorHandle,
        context: Arc<CuratorContext>,
        consolidation: Arc<ConsolidationBridge>,
    ) -> Self {
        Self {
            curator_handle,
            context,
            consolidation: Some(consolidation),
            last_review_ms: AtomicU64::new(0),
            inbox: None,
            auto_consolidation_enabled: false,
        }
    }

    /// Configure whether the Curator daemon may auto-consolidate memory when
    /// escalations exist. Default is `false`.
    ///
    /// Even when enabled, auto-consolidation still requires:
    /// - a `ConsentManager` wired into `CuratorContext`, and
    /// - affirmative consent for both `EpisodicMemory` and `SemanticMemory` on the
    ///   Curator WebID.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_auto_consolidation_enabled(mut self, enabled: bool) -> Self {
        self.auto_consolidation_enabled = enabled;
        self
    }

    /// Wire the unified inbox for CurationInput messages.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — unified inbox receives CurationInput
    /// pre:  `rx` is a valid `UnboundedReceiver<CurationInput>`.
    /// post: Returns `self` with `inbox` set to ``Some(Arc<RwLock<rx>>)``.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_inbox(mut self, rx: mpsc::UnboundedReceiver<CurationInput>) -> Self {
        self.inbox = Some(Arc::new(RwLock::new(rx)));
        self
    }

    /// Access the CuratorContext (capability-disciplined runtime references).
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — context exposes Regulation and escalation
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `Arc<CuratorContext>`.
    pub fn context(&self) -> &Arc<CuratorContext> {
        &self.context
    }

    /// Access the CuratorHandle owned by this loop.
    ///
    /// Per the singleton invariant, this is the single CuratorHandle
    /// for the entire system.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — handle is the capability to curate
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `CuratorHandle`.
    pub fn curator_handle(&self) -> &CuratorHandle {
        &self.curator_handle
    }

    /// Restore the last_review_ms cursor from persistent storage.
    ///
    /// Call this after construction and before the first tick to avoid
    /// re-processing all historical algedonic events on restart.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — cursor restore avoids re-processing history
    /// pre:  `self.context.regulation_store()` may be `Some` or `None`.
    /// post: If a persisted cursor exists, `last_review_ms` is updated;
    ///       otherwise it remains at 0. Logs the outcome at info/warn level.
    ///       Does not panic on storage errors.
    pub fn restore_cursor(&self) {
        if let Some(store) = self.context.regulation_store() {
            match store.load_cursor("curation_last_review_ms") {
                Ok(Some(cursor_ms)) => {
                    self.last_review_ms
                        .store(cursor_ms as u64, Ordering::Relaxed);
                    tracing::info!(target: CUR_TARGET, cursor_ms = cursor_ms, "Restored curation review cursor from persistence");
                }
                Ok(None) => {
                    tracing::info!(target: CUR_TARGET, "No persisted cursor found — starting from epoch")
                }
                Err(e) => {
                    tracing::warn!(target: CUR_TARGET, error = %e, "Failed to load persisted curation cursor — starting from epoch")
                }
            }
        }
    }

    /// Auto-consolidation: fire the consolidation bridge if auto-consolidation is
    /// enabled and all consent requirements are satisfied.
    ///
    /// Called from [`act()`](RegulationLoop::act) for both `consolidation_candidates_exist`
    /// and `pending_escalations_exist` actions. Requires a `ConsentManager` wired
    /// into `CuratorContext` and affirmative consent for both `EpisodicMemory`
    /// and `SemanticMemory` on the Curator WebID.
    ///
    /// Idempotent: called from both handlers in [`act()`]. Multiple calls within
    /// the same tick are safe — consolidation processes the same candidates and
    /// the second pass is effectively a no-op.
    async fn try_auto_consolidate(&self) {
        let consolidation = match &self.consolidation {
            Some(c) => c,
            None => return,
        };
        let curator_id = *self.context.handle().curator_id();
        if !self.auto_consolidation_enabled {
            tracing::info!(
                target: CUR_TARGET,
                "Curator auto-consolidation disabled by configuration"
            );
        } else {
            // P2: require explicit consent for both memory categories.
            let mut can_run = true;
            let mut missing = Vec::new();
            if let Some(cm) = self.context.consent_manager() {
                for cat in [DataCategory::EpisodicMemory, DataCategory::SemanticMemory] {
                    match cm.has_consent(&curator_id.to_string(), &cat) {
                        Ok(true) => {}
                        _ => {
                            can_run = false;
                            missing.push(cat.to_string());
                        }
                    }
                }
            } else {
                can_run = false;
                missing.push("no consent manager wired".to_string());
            }

            if !can_run {
                let curator_id_str = curator_id.to_string();
                tracing::warn!(
                    target: CUR_TARGET,
                    missing_categories = ?missing,
                    curator_webid = %curator_id_str,
                    "Curator auto-consolidation skipped — missing consent"
                );
                tracing::info!(
                    target: "reg",
                    reg_domain = %hkask_regulation::infra_span::InfraSpan::CuratorConsolidation.as_str(),
                    operation = "skipped",
                    reason = "missing_consent",
                    missing_categories = ?missing,
                    curator_webid = %curator_id_str,
                    "REG"
                );
                let _ = self.context.escalation_port().add(
                    TemplateID::new(),
                    BotID::from_uuid(curator_id.as_uuid()),
                    "Curator auto-consolidation skipped: missing consent"
                        .to_string(),
                    0.9,
                    0,
                    format!(
                        "Missing: {}. Grant consent with: kask sovereignty grant --category episodic_memory --agent curator && kask sovereignty grant --category semantic_memory --agent curator",
                        missing.join(", ")
                    ),
                );
            } else {
                match consolidation.consolidate(
                    curator_id,
                    ConsolidationRequest {
                        limit: 100,
                        ..Default::default()
                    },
                ) {
                    Ok(outcome) => {
                        tracing::info!(
                            target: CUR_TARGET,
                            consolidated = outcome.consolidated_count,
                            deleted = outcome.deleted_count,
                            failed = outcome.failed_count,
                            "Curator auto-consolidation completed"
                        );
                        tracing::info!(
                            target: "reg",
                            reg_domain = %hkask_regulation::infra_span::InfraSpan::CuratorConsolidation.as_str(),
                                operation = "completed",
                                consolidated = outcome.consolidated_count,
                            deleted = outcome.deleted_count,
                            failed = outcome.failed_count,
                            "REG"
                        );
                        if outcome.consolidated_count > 0 {
                            let _ = self.context.escalation_port().add(
                                TemplateID::new(),
                                BotID::from_uuid(curator_id.as_uuid()),
                                format!(
                                    "Curator auto-consolidated {} episodic h_mem(s) into semantic memory",
                                    outcome.consolidated_count
                                ),
                                0.5,
                                0,
                                format!(
                                    "Deleted {} semantic h_mem(s); failed {}",
                                    outcome.deleted_count,
                                    outcome.failed_count
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: CUR_TARGET,
                            error = %e,
                            "Curator auto-consolidation failed"
                        );
                        tracing::info!(
                            target: "reg",
                            reg_domain = %hkask_regulation::infra_span::InfraSpan::CuratorConsolidation.as_str(),
                                operation = "failed",
                                error = %e,
                            "REG"
                        );
                        let _ = self.context.escalation_port().add(
                            TemplateID::new(),
                            BotID::from_uuid(curator_id.as_uuid()),
                            "Curator auto-consolidation failed".to_string(),
                            0.7,
                            0,
                            e.to_string(),
                        );
                    }
                }
            }
        }
    }

    // Explicit 4-stage cycle: sense → compare → compute → act
    // Delegation methods removed — RegulationLoop trait impl provides tick().
}
#[async_trait::async_trait]
impl RegulationLoop for CurationLoop {
    fn id(&self) -> LoopId {
        LoopId::Curation
    }

    /// Sense: read algedonic-significant RegulationRecords from the persistent store.
    /// Falls back to live Regulation reads if no RegulationRecord store is configured.
    async fn sense(&self) -> Vec<Signal> {
        // Primary: Read from RegulationRecord store using cursor-based algedonic review
        let since_ms = self.last_review_ms.load(Ordering::Relaxed);
        let since = chrono::DateTime::<Utc>::from_timestamp_millis(since_ms as i64)
            .unwrap_or(chrono::DateTime::<Utc>::UNIX_EPOCH);

        let algedonic_count = if let Some(store) = self.context.regulation_store() {
            // Read algedonic-significant events since last review cursor
            match store.query_algedonic(since, 1000) {
                Ok(events) => {
                    let count = events.len() as u64;

                    // Push communication events to shared context for metacognition processing.
                    // This replaces the CommunicationWatcher — no duplicate polling, no data-loss bug.
                    {
                        let mut comm_events = self.context.pending_communication.write().await;
                        for event in &events {
                            let cat = event.span.namespace.short_name();
                            if cat.starts_with("communication.") {
                                let ce = CommunicationEvent {
                                    span_category: cat.to_string(),
                                    span_path: event.span.as_str().to_string(),
                                    observation: event.observation.clone(),
                                    observed_at: event.timestamp.to_rfc3339(),
                                };
                                comm_events.push(ce);
                            }
                        }
                    }

                    if let Some(latest) = events.last() {
                        let new_cursor = latest.timestamp.timestamp_millis() as u64;
                        self.last_review_ms.store(new_cursor, Ordering::Relaxed);
                        if let Err(e) =
                            store.persist_cursor("curation_last_review_ms", new_cursor as i64)
                        {
                            tracing::warn!(target: CUR_TARGET, error = %e, "Failed to persist curation review cursor");
                        }
                    }
                    tracing::info!(target: CUR_TARGET, since = %since.to_rfc3339(), event_count = count, "Curation read algedonic events from RegulationRecord store");
                    count
                }
                Err(e) => {
                    tracing::warn!(target: CUR_TARGET, error = %e, "Failed to query RegulationRecord store, falling back to live Regulation reads");
                    self.context.ledger().critical_alerts().await.len() as u64
                }
            }
        } else {
            // No RegulationRecord store configured: fall back to live Regulation reads
            self.context.ledger().critical_alerts().await.len() as u64
        };

        let pending_escalations = self
            .context
            .escalation_port()
            .list_pending()
            .map(|v| v.len())
            .unwrap_or(0);

        let consolidation_candidates = self
            .consolidation
            .as_ref()
            .map(|port| port.consolidation_candidate_count(self.context.handle().curator_id()))
            .unwrap_or(0);

        // Drain unified inbox for CurationInput messages.
        let mut goal_stale_count: u64 = 0;
        let mut goal_expired_count: u64 = 0;
        let mut direct_alerts: u64 = 0;
        if let Some(inbox) = &self.inbox {
            let mut rx = inbox.write().await;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    CurationInput::Alert(alert) => {
                        direct_alerts += 1;
                        tracing::info!(
                            target: CUR_TARGET,
                            deficit = alert.deficit,
                            threshold = alert.threshold,
                            "RuntimeAlert received from Cybernetics"
                        );
                    }
                    CurationInput::GoalTransition(event) => {
                        match GoalLifecycle::from_state_str(&event.to_state) {
                            GoalLifecycle::Stale => {
                                goal_stale_count += 1;
                                tracing::debug!(target: CUR_TARGET, goal_id = %event.goal_id, "Goal stale");
                            }
                            GoalLifecycle::Expired => {
                                goal_expired_count += 1;
                                tracing::debug!(target: CUR_TARGET, goal_id = %event.goal_id, "Goal expired");
                            }
                            GoalLifecycle::Other => {}
                        }
                    }
                    CurationInput::Communication(event) => {
                        self.context.pending_communication.write().await.push(event);
                    }
                }
            }
        }

        let s = |metric, value: u64| Signal::new(LoopId::Curation, metric, value as f64, 0.0);
        vec![
            s(
                SignalMetric::AlgedonicEvents,
                algedonic_count + direct_alerts,
            ),
            s(SignalMetric::PendingEscalations, pending_escalations as u64),
            s(
                SignalMetric::ConsolidationCandidates,
                consolidation_candidates as u64,
            ),
            s(SignalMetric::GoalStaleCount, goal_stale_count),
            s(SignalMetric::GoalExpiredCount, goal_expired_count),
        ]
    }

    /// Compute: produce CuratorDirectives as RegulatoryActions.
    ///
    /// Produces escalation actions for all six deviation types. Consolidation is
    /// handled independently via `try_auto_consolidate()`, called from `act()` for
    /// both `consolidation_candidates_exist` and `pending_escalations_exist` actions.
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        for dev in deviations {
            match dev.signal.metric {
                SignalMetric::AlgedonicEvents if dev.signal.value > 0.0 => {
                    actions.push(RegulatoryAction::new(
                        LoopId::Cybernetics,
                        hkask_regulation::types::loops::ActionType::Escalate,
                        hkask_regulation::types::loops::RegulatoryActionParams::reason(
                            CurationEscalationReason::AlgedonicEventsExceeded.as_str(),
                        ),
                    ));
                }
                SignalMetric::PendingEscalations if dev.signal.value > 0.0 => {
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        hkask_regulation::types::loops::ActionType::Escalate,
                        hkask_regulation::types::loops::RegulatoryActionParams::reason(
                            CurationEscalationReason::PendingEscalationsExist.as_str(),
                        ),
                    ));
                }
                SignalMetric::ConsolidationCandidates if dev.signal.value > 0.0 => {
                    // Episodic budget pressure — fire consolidation bridge in act()
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        hkask_regulation::types::loops::ActionType::Escalate,
                        hkask_regulation::types::loops::RegulatoryActionParams::reason(
                            CurationEscalationReason::ConsolidationCandidatesExist.as_str(),
                        ),
                    ));
                }
                SignalMetric::GoalStaleCount if dev.signal.value > 0.0 => {
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        hkask_regulation::types::loops::ActionType::Escalate,
                        hkask_regulation::types::loops::RegulatoryActionParams::reason(
                            CurationEscalationReason::GoalsStale.as_str(),
                        ),
                    ));
                }
                SignalMetric::GoalExpiredCount if dev.signal.value > 0.0 => {
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        hkask_regulation::types::loops::ActionType::Escalate,
                        hkask_regulation::types::loops::RegulatoryActionParams::reason(
                            CurationEscalationReason::GoalsExpired.as_str(),
                        ),
                    ));
                }
                _ => {}
            }
        }

        actions
    }

    /// Act: issue directives through CuratorContext with DAMPEN filtering.
    async fn act(&self, actions: &[RegulatoryAction]) {
        for action in actions {
            tracing::info!(target: CUR_TARGET, action_type = ?action.action_type, target_loop = %action.target, "Curation Loop regulatory action");
            let reason = CurationEscalationReason::from_reason(&action.parameters.reason);
            let directive = match (action.action_type, reason) {
                (
                    hkask_regulation::types::loops::ActionType::Escalate,
                    CurationEscalationReason::AlgedonicEventsExceeded,
                ) => {
                    tracing::warn!(target: CUR_TARGET, "Algedonic events exceeded threshold — Curation reviewing");
                    None
                }
                (
                    hkask_regulation::types::loops::ActionType::Escalate,
                    CurationEscalationReason::ConsolidationCandidatesExist,
                ) => {
                    tracing::info!(
                        target: CUR_TARGET,
                        "Consolidation candidates exist — attempting auto-consolidation"
                    );
                    self.try_auto_consolidate().await;
                    continue;
                }
                (
                    hkask_regulation::types::loops::ActionType::Escalate,
                    CurationEscalationReason::PendingEscalationsExist,
                ) => {
                    // Process pending escalations from the queue
                    match self.context.escalation_port().list_pending() {
                        Ok(entries) if !entries.is_empty() => {
                            tracing::warn!(
                                target: CUR_TARGET,
                                count = entries.len(),
                                "Processing pending escalations"
                            );
                            for entry in &entries {
                                tracing::info!(
                                    target: CUR_TARGET,
                                    escalation_id = %entry.id,
                                    confidence = entry.confidence,
                                    "Reviewing escalation entry"
                                );
                            }
                            // Issue directives for high-confidence escalations
                            // (adjust energy budgets for the associated bot)
                            for entry in entries.iter().filter(|e| e.confidence > 0.5) {
                                let directive = CuratorDirective::OverrideEnergyBudget {
                                    agent: entry.bot_id.into(),
                                    new_budget: DEFAULT_ESCALATION_BUDGET_OVERRIDE,
                                };
                                self.context.issue_directive(directive).await;
                                tracing::info!(target: CUR_TARGET, escalation_id = %entry.id, "Issued OverrideEnergyBudget directive for escalated bot");
                            }
                            // Consolidation is driven by the dedicated
                            // `consolidation_candidates_exist` action, not here —
                            // avoids a double `try_auto_consolidate()` when both
                            // signals fire in one act() pass.
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(
                                target: CUR_TARGET,
                                error = %e,
                                "Failed to list pending escalations"
                            );
                        }
                    }
                    continue;
                }
                (hkask_regulation::types::loops::ActionType::Escalate, _) => {
                    // Other escalations (goals_stale, goals_expired, unknown)
                    // go through the escalation queue (handled by CuratorContext internally)
                    None
                }
                _ => None,
            };

            if let Some(directive) = directive {
                self.context.issue_directive(directive).await;
                tracing::info!(
                    target: CUR_TARGET,
                    "Directive issued through dispatch"
                );
            }
            // None means directive was dampened or issuance failed
        }
    }
}
