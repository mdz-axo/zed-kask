//! CurationContext — Runtime composition of Curator capability handles

use crate::a2a::A2ARuntime;
use crate::consent::ConsentManager;
use crate::ports::EscalationPort;
use hkask_regulation::RegulationLedger;
use hkask_regulation::meta_span::emit_meta_directive;
use hkask_regulation::types::loops::CommunicationEvent;
use hkask_templates::ManifestExecutor;
use hkask_types::LedgerStoragePort;
use hkask_types::curator::{CuratorDirective, CuratorHandle};
use hkask_types::event::RegulationSink;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, mpsc};

/// In-process counters for the Curator's own decision quality.
///
/// These are NOT persisted as `reg.*` algedonic events (so CurationLoop never
/// reads them back — circularity guard). They feed the metacognition
/// self-calibration loop, and corresponding `reg.meta.*` spans are emitted for
/// external observability via the `RegulationSink`.
#[derive(Debug, Default)]
pub struct SelfQuality {
    directives_issued: AtomicU64,
    escalations_dropped: AtomicU64,
    circuit_breaker_trips: AtomicU64,
}

/// An immutable snapshot of the Curator's self-quality counters.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelfQualitySnapshot {
    pub directives_issued: u64,
    pub escalations_dropped: u64,
    pub circuit_breaker_trips: u64,
}

impl SelfQuality {
    /// Record that a CuratorDirective was issued.
    pub fn record_directive(&self) {
        self.directives_issued.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that an escalation could not be persisted (dropped).
    pub fn record_escalation_dropped(&self) {
        self.escalations_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record that the template circuit breaker tripped.
    pub fn record_circuit_breaker(&self) {
        self.circuit_breaker_trips.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot all counters (relaxed ordering — approximate is sufficient
    /// for self-calibration decisions).
    #[must_use]
    pub fn snapshot(&self) -> SelfQualitySnapshot {
        SelfQualitySnapshot {
            directives_issued: self.directives_issued.load(Ordering::Relaxed),
            escalations_dropped: self.escalations_dropped.load(Ordering::Relaxed),
            circuit_breaker_trips: self.circuit_breaker_trips.load(Ordering::Relaxed),
        }
    }
}

/// CuratorContext — aggregates the runtime references the Curator needs.
pub struct CuratorContext {
    handle: CuratorHandle,
    ledger: Arc<RegulationLedger>,
    /// Direct channel for issuing CuratorDirectives to Cybernetics.
    /// None when running standalone (e.g., CLI metacognition) where no
    /// CyberneticsLoop receiver exists.
    curator_directive_tx: Option<mpsc::UnboundedSender<CuratorDirective>>,
    escalation_port: Arc<dyn EscalationPort>,
    /// RegulationRecord store for algedonic review queries.
    /// Curation reads from the persistent log, not live Regulation state.
    regulation_store: Option<Arc<dyn LedgerStoragePort>>,
    /// Sink for emitting `reg.meta.*` self-observation spans. Optional because
    /// standalone CLI metacognition has no persistent Regulation archive.
    /// Distinct from `regulation_store` (read trait) — this is the write trait
    /// (`RegulationSink`). Both are satisfied by the same `RegulationArchive`.
    regulation_sink: Option<Arc<dyn RegulationSink>>,
    /// In-process self-quality counters — feed metacognition self-calibration.
    /// Shared (Arc) so both CurationLoop and MetacognitionLoop can record/observe.
    self_quality: Arc<SelfQuality>,
    /// A2A port for A2A messaging (e.g. directing bots).
    a2a_port: Option<Arc<A2ARuntime>>,
    /// Manifest executor for invoking KnowAct templates.
    /// Per P3 (Generative Space), selection intelligence lives in the
    /// template system. Uses `RwLock<Option<>>` for late binding — the
    /// executor is constructed after MCP pods, but CuratorContext must
    /// exist before them. Set via `set_manifest_executor()`.
    manifest_executor: RwLock<Option<Arc<ManifestExecutor>>>,
    /// Skill catalog snapshot for epistemic routing. Late binding —
    /// set via `set_skill_catalog()` after the registry is bootstrapped.
    skill_catalog: RwLock<Option<Vec<hkask_types::RegistryEntry>>>,
    /// Consent manager for P2 sovereignty checks. Optional because some
    /// CuratorContext users (standalone CLI metacognition) do not have
    /// a consent store. When present, the Curation Loop uses it to gate
    /// auto-consolidation behind affirmative consent.
    consent_manager: Option<Arc<ConsentManager>>,
    /// Pending communication events from Matrix, drained by metacognition.
    /// Events arrive here from CurationLoop.sense() on each loop tick (~10s).
    /// Metacognition drains them on CLI-triggered cycles — there may be a
    /// one-cycle delay between push and drain. This is eventual consistency;
    /// no events are lost.
    pub(crate) pending_communication: Arc<RwLock<Vec<CommunicationEvent>>>,
}

impl CuratorContext {
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — CuratorContext bundles regulatory dependencies
    /// pre:  `handle` is a valid `CuratorHandle`; `ledger` is a valid
    ///       `Arc<RegulationLedger>`; `curator_directive_tx` is `Some` or `None`;
    ///       `escalation_port` is a valid `Arc<dyn EscalationPort>`.
    /// post: Returns a `CuratorContext` with no RegulationRecord store, no A2A
    ///       port, and no manifest executor.
    pub fn new(
        handle: CuratorHandle,
        ledger: Arc<RegulationLedger>,
        curator_directive_tx: Option<mpsc::UnboundedSender<CuratorDirective>>,
        escalation_port: Arc<dyn EscalationPort>,
    ) -> Self {
        Self {
            handle,
            ledger,
            curator_directive_tx,
            escalation_port,
            regulation_store: None,
            regulation_sink: None,
            self_quality: Arc::new(SelfQuality::default()),
            a2a_port: None,
            manifest_executor: RwLock::new(None),
            skill_catalog: RwLock::new(None),
            pending_communication: Arc::new(RwLock::new(Vec::new())),
            consent_manager: None,
        }
    }

    /// Create CuratorContext with a RegulationRecord store for algedonic review.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — RegulationRecord store enables algedonic review
    /// pre:  All arguments are valid (same as `new`); `regulation_store` is
    ///       a valid `Arc<dyn LedgerStoragePort>`.
    /// post: Returns a `CuratorContext` with `regulation_store` set, no
    ///       A2A port, and no manifest executor.
    pub fn with_regulation_store(
        handle: CuratorHandle,
        ledger: Arc<RegulationLedger>,
        curator_directive_tx: Option<mpsc::UnboundedSender<CuratorDirective>>,
        escalation_port: Arc<dyn EscalationPort>,
        regulation_store: Arc<dyn LedgerStoragePort>,
    ) -> Self {
        Self {
            handle,
            ledger,
            curator_directive_tx,
            escalation_port,
            regulation_store: Some(regulation_store),
            regulation_sink: None,
            self_quality: Arc::new(SelfQuality::default()),
            a2a_port: None,
            manifest_executor: RwLock::new(None),
            skill_catalog: RwLock::new(None),
            pending_communication: Arc::new(RwLock::new(Vec::new())),
            consent_manager: None,
        }
    }

    /// Builder: attach an A2A port for A2A bot-directed messaging.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P4\] Motivating: Clear Boundaries — A2A port lets Curator direct bots
    /// pre:  `a2a_runtime` is a valid `Arc<A2ARuntime>`.
    /// post: Returns `self` with `a2a_port` set to `Some(a2a_runtime)`.
    pub fn with_a2a(mut self, a2a_runtime: Arc<A2ARuntime>) -> Self {
        self.a2a_port = Some(a2a_runtime);
        self
    }

    /// Builder: attach a ConsentManager so the Curation Loop can enforce P2
    /// affirmative consent before autonomous actions such as auto-consolidation.
    ///
    /// expect: "Agent consent is explicitly granted, scoped, and revocable"
    /// \[P2\] Motivating: Affirmative Consent — CuratorContext carries the consent manager
    /// pre:  `consent_manager` is a valid `Arc<ConsentManager>`.
    /// post: Returns `self` with the consent manager set.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_consent_manager(mut self, consent_manager: Arc<ConsentManager>) -> Self {
        self.consent_manager = Some(consent_manager);
        self
    }

    /// Builder: attach a `RegulationSink` so the Curator can emit `reg.meta.*`
    /// self-observation spans. Optional — standalone CLI metacognition has no
    /// persistent archive. When absent, self-observation spans are skipped
    /// (in-process `SelfQuality` counters still work).
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_regulation_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.regulation_sink = Some(sink);
        self
    }

    /// Late-binding setter: attach a ManifestExecutor after construction.
    ///
    /// The ManifestExecutor depends on the governed McpRuntime, which is built
    /// after CuratorContext. This setter allows the executor to be wired in later
    /// without changing the construction order.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P3\] Motivating: Generative Space — late-binding template executor
    /// pre:  `executor` is a valid `Arc<ManifestExecutor>`.
    /// post: `manifest_executor` is set to `Some(executor)`.
    pub async fn set_manifest_executor(&self, executor: Arc<ManifestExecutor>) {
        *self.manifest_executor.write().await = Some(executor);
    }

    /// Access the CuratorHandle (capability handle).
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — accessor for the Curator capability handle
    /// pre:  (none — accessor).
    /// post: Returns a reference to the inner `CuratorHandle`.
    pub fn handle(&self) -> &CuratorHandle {
        &self.handle
    }

    /// Access the Regulation runtime for health checks and variety queries.
    pub(crate) fn ledger(&self) -> &Arc<RegulationLedger> {
        &self.ledger
    }

    /// Drain pending communication events for processing.
    pub async fn drain_communication_events(&self) -> Vec<CommunicationEvent> {
        let mut events = self.pending_communication.write().await;
        std::mem::take(&mut *events)
    }

    /// Access the RegulationRecord store for algedonic review queries.
    ///
    /// Curation reads from the persistent event log, not live Regulation state.
    /// Returns None if no RegulationRecord store is configured (graceful degradation).
    pub(crate) fn regulation_store(&self) -> Option<&Arc<dyn LedgerStoragePort>> {
        self.regulation_store.as_ref()
    }

    /// Access the escalation port for posting human review items.
    pub(crate) fn escalation_port(&self) -> &Arc<dyn EscalationPort> {
        &self.escalation_port
    }

    /// Access the A2A port for A2A messaging.
    ///
    /// Returns None if no A2A port is configured (graceful degradation).
    pub(crate) fn a2a(&self) -> Option<&Arc<A2ARuntime>> {
        self.a2a_port.as_ref()
    }

    /// Access the ConsentManager, if one has been wired.
    ///
    /// Returns None in contexts where sovereignty checks are not applicable
    /// (e.g., standalone CLI metacognition).
    pub(crate) fn consent_manager(&self) -> Option<&Arc<ConsentManager>> {
        self.consent_manager.as_ref()
    }

    /// Access the `RegulationSink` for emitting `reg.meta.*` self-observation spans.
    ///
    /// Returns None when no persistent archive is configured (standalone CLI).
    pub(crate) fn regulation_sink(&self) -> Option<&Arc<dyn RegulationSink>> {
        self.regulation_sink.as_ref()
    }

    /// Access the in-process self-quality counters (for metacognition
    /// self-calibration). Always present.
    pub(crate) fn self_quality(&self) -> &Arc<SelfQuality> {
        &self.self_quality
    }

    /// Access the ManifestExecutor for template invocations.
    ///
    /// Returns None if no executor has been set yet (late binding —
    /// set via `set_manifest_executor()` after MCP pods are built).
    pub(crate) async fn manifest_executor(&self) -> Option<Arc<ManifestExecutor>> {
        self.manifest_executor.read().await.clone()
    }

    /// Check whether a ManifestExecutor has been set (late binding).
    ///
    /// Returns `true` if `set_manifest_executor()` has been called.
    /// This is a lighter check than `manifest_executor()` — it avoids
    /// cloning the `Arc`.
    pub async fn has_manifest_executor(&self) -> bool {
        self.manifest_executor.read().await.is_some()
    }

    /// Late-binding setter: attach a skill catalog snapshot after construction.
    ///
    /// Used by the epistemic routing consumer to invoke skill-router when
    /// low-confidence escalations are detected.
    pub async fn set_skill_catalog(&self, catalog: Vec<hkask_types::RegistryEntry>) {
        *self.skill_catalog.write().await = Some(catalog);
    }

    /// Access the skill catalog snapshot for epistemic routing.
    pub(crate) async fn skill_catalog(&self) -> Option<Vec<hkask_types::RegistryEntry>> {
        self.skill_catalog.read().await.clone()
    }

    /// Issue a CuratorDirective through the direct channel to Cybernetics.
    ///
    /// Curation (Loop 5) governs Cybernetics (Loop 6) per the authority DAG,
    /// so Curator directives MUST NOT be dampened by a Cybernetics dampener.
    /// Dampening is applied at the Cybernetics receipt boundary instead.
    ///
    /// The CuratorHandle is a structural singleton — only `CuratorHandle::system()`
    /// can construct it, so every `CuratorContext` holds the one authorized handle.
    /// There is no runtime OCAP gate here: authority is enforced by construction
    /// (the singleton invariant), not by an always-pass check.
    ///
    /// When no channel is configured (e.g., standalone CLI), this is a no-op.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — direct directive channel
    /// \[P4\] Constraining: Clear Boundaries — authority enforced by singleton construction
    /// pre:  `directive` is a valid `CuratorDirective`; `self.handle` is the
    ///       singleton `CuratorHandle`.
    /// post: If `curator_directive_tx` is `Some`, the directive is sent; logs
    ///       a warning if the send fails. If `None`, this is a no-op.
    pub async fn issue_directive(&self, directive: CuratorDirective) {
        // Record self-quality (in-process) and emit a reg.meta.directive span
        // (persistent, for external observability). The variant name is captured
        // before the directive is moved into the channel.
        let variant = directive.variant_name();
        let target = directive.agent_target();
        self.self_quality.record_directive();
        if let Some(sink) = self.regulation_sink() {
            emit_meta_directive(
                sink.as_ref(),
                self.handle.curator_id(),
                variant,
                target.as_ref(),
            );
        }

        if let Some(ref tx) = self.curator_directive_tx
            && let Err(e) = tx.send(directive)
        {
            tracing::warn!(
                target: "curator.context",
                error = %e,
                "Failed to send CuratorDirective on direct channel"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_quality_counters_accumulate() {
        let sq = SelfQuality::default();
        assert_eq!(sq.snapshot().directives_issued, 0);
        assert_eq!(sq.snapshot().escalations_dropped, 0);

        sq.record_directive();
        sq.record_directive();
        sq.record_escalation_dropped();
        sq.record_circuit_breaker();
        sq.record_escalation_dropped();

        let snap = sq.snapshot();
        assert_eq!(snap.directives_issued, 2);
        assert_eq!(snap.escalations_dropped, 2);
        assert_eq!(snap.circuit_breaker_trips, 1);
    }
}
