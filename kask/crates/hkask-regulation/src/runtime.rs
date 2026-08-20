//! Regulation Runtime — minimal observability
//!
//! RegulationLedger is the single entry point for all Regulation operations:
//! - Variety counting (Ashby's Law)
//! - Algedonic alerts (deficit > threshold → escalate)
//!
//! # Epistemic grounding (TASK 0)
//! - **crt:certainty** = Declarative (direct sensor readings)
//! - **crt:force** = Evidence (IS statement, measured from runtime state)
//! - **mode** = IS
//!
//! # Cybernetic role (TASK 1)
//! - Sensor: VarietyMonitor.counters() — count distinct agent states
//! - Comparator: AlgedonicManager.check() — compares deficit to threshold
//! - Effector: algedonic alerts are forwarded to the `MetacognitionLoop`
//!   via the alert channel; durable span persistence goes through the
//!   `RegulationSink` wired at the composition root (`RegulationArchive`
//!   on the curator's curator.db in zed-kask).

use crate::algedonic::{
    AlgedonicManager, DEFAULT_EXPECTED_VARIETY, RuntimeAlert, reg_health_check,
};
use crate::set_points::DEFAULT_VARIETY_MAX_DEFICIT;
use crate::tool_stats::ToolStats;

use hkask_types::event::{RegulationRecord, RegulationSink, SpanNamespace};
use hkask_types::regulation::{LedgerHealth, RegulationHealth};
use parking_lot::RwLock as ParkingRwLock;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

// MAX_REGULATION_HISTORY and MAX_SKILL_SPAN_HISTORY moved to
// set_points::DEFAULT_MAX_*_HISTORY. Test-compatibility alias below.
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing;

// ── Skill feedback span storage ───────────────────────────────────────────
// Stores individual reg.skill.<id>.outcome and reg.skill.<id>.operator_feedback
// span payloads so skills can query their own prior feedback (self-improvement
// τ_t / e_t signals). Complements VarietyTracker (counts) and OutcomeTracker
// (success/failure rates) by retaining the actual field payloads that skills
// need to refine their recommendations.

/// A stored skill feedback span with its field payload.
///
/// Unlike tracing events (which fire and vanish), StoredSkillSpan retains
/// the structured payload so `query_skill_feedback` can return it to the
/// next skill invocation.
#[derive(Debug, Clone)]
pub struct StoredSkillSpan {
    /// Full namespace: `reg.skill.<skill-id>.<phase>`
    pub namespace: String,
    /// Skill ID extracted from the namespace (e.g., "lora-training").
    pub skill_id: String,
    /// Phase: `outcome` or `operator_feedback`.
    pub phase: String,
    /// Structured field payload (e.g., {"training_completed": true, "loss": 0.3}).
    pub payload: serde_json::Value,
    /// When the span was recorded.
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Bounded storage for skill feedback spans, keyed by (skill_id, phase).
/// Retains the last `max_history` spans per key.
#[derive(Debug)]
pub(crate) struct SkillSpanStore {
    spans: HashMap<String, VecDeque<StoredSkillSpan>>,
    max_history: usize,
}

impl Default for SkillSpanStore {
    fn default() -> Self {
        Self {
            spans: HashMap::new(),
            max_history: crate::set_points::DEFAULT_MAX_SKILL_SPAN_HISTORY,
        }
    }
}

impl SkillSpanStore {
    fn key(skill_id: &str, phase: &str) -> String {
        format!("{skill_id}:{phase}")
    }

    pub(crate) fn with_capacity(max_history: usize) -> Self {
        Self {
            spans: HashMap::new(),
            max_history,
        }
    }

    pub(crate) fn record(&mut self, span: StoredSkillSpan) {
        let key = Self::key(&span.skill_id, &span.phase);
        let deque = self.spans.entry(key).or_default();
        if deque.len() >= self.max_history {
            deque.pop_front();
        }
        deque.push_back(span);
    }

    pub(crate) fn query(&self, skill_id: &str, phase: &str) -> Vec<StoredSkillSpan> {
        let key = Self::key(skill_id, phase);
        self.spans
            .get(&key)
            .map(|deque| deque.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Return the set of skill IDs that have at least one span for the given
    /// phase. Used by the metacognition loop to enumerate skills for drift
    /// detection without scanning all possible skill names.
    pub(crate) fn skill_ids_with_phase(&self, phase: &str) -> Vec<String> {
        self.spans
            .keys()
            .filter_map(|key| {
                key.split_once(':')
                    .filter(|(_, p)| *p == phase)
                    .map(|(id, _)| id.to_string())
            })
            .collect()
    }
}

// ── Variety counter infrastructure ────────────────────────────────────────
// Relocated from variety.rs (TASK 2 deletion test — VarietyMonitor only used
// by RegulationLedger, so depth increases when co-located).

/// Default variety counter window duration (1 minute).
const DEFAULT_VARIETY_WINDOW_SECS: u64 = 60;

/// Variety counter for tracking state diversity in a domain.
///
/// # Epistemic grounding
/// - **crt:certainty** = Subjunctive (sampling, not complete observation)
/// - **crt:force** = Hypothesis (counter is an estimate, not a ground truth)
/// - **mode** = IS
#[derive(Debug, Clone)]
pub(crate) struct VarietyTracker {
    counts: HashMap<String, u64>,
    window_start: Instant,
    window_duration: Duration,
    /// Exponential moving average of variety over the session.
    /// Decay factor α = 0.1 per window-reset. Survives the 60s hard-reset
    /// so the health check can distinguish "spiked and died" from sustained low variety.
    ema: f64,
}

impl VarietyTracker {
    pub(crate) fn new() -> Self {
        Self {
            counts: HashMap::new(),
            window_start: Instant::now(),
            window_duration: Duration::from_secs(DEFAULT_VARIETY_WINDOW_SECS),
            ema: 0.0,
        }
    }

    pub(crate) fn increment(&mut self, key: &str) {
        self.check_window();
        *self.counts.entry(key.to_string()).or_insert(0) += 1;
    }

    pub(crate) fn variety(&self) -> u64 {
        self.counts.len() as u64
    }

    /// Session-level exponential moving average of variety.
    /// Survives window resets — decays slowly (α = 0.1 per reset).
    pub(crate) fn variety_ema(&self) -> f64 {
        self.ema
    }

    pub(crate) fn deficit(&self, expected_variety: u64) -> u64 {
        expected_variety.saturating_sub(self.variety())
    }

    fn check_window(&mut self) {
        if self.window_start.elapsed() > self.window_duration {
            self.reset();
        }
    }

    pub(crate) fn reset(&mut self) {
        // Blend current raw variety into the EMA before clearing.
        // α = 0.1: new EMA = 0.9 × old EMA + 0.1 × current variety.
        let current = self.counts.len() as f64;
        const ALPHA: f64 = 0.1;
        self.ema = (1.0 - ALPHA) * self.ema + ALPHA * current;
        self.counts.clear();
        self.window_start = Instant::now();
    }

    /// Hard reset — clears both counts and EMA for a fresh session.
    /// Unlike `reset()`, which blends into the EMA, this discards all history.
    pub(crate) fn session_reset(&mut self) {
        self.counts.clear();
        self.ema = 0.0;
        self.window_start = Instant::now();
    }
}

impl Default for VarietyTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome quality tracker — success/failure distribution per domain.
///
/// Complements `VarietyTracker` by tracking not just *what* was done
/// (distinct tool names) but *how well* (success rate). A system calling
/// 47 distinct tools that all fail would show variety=47 ("healthy") while
/// being completely broken. Outcome tracking closes this blind spot.
///
/// # Epistemic grounding
/// - **crt:certainty** = Declarative (direct measurement of tool outcomes)
/// - **crt:force** = Evidence (IS statement, measured from runtime state)
/// - **mode** = IS
#[derive(Debug, Clone)]
pub(crate) struct OutcomeTracker {
    total: u64,
    successes: u64,
    failures: u64,
    /// Per-error-kind breakdown for diagnosis.
    error_kinds: HashMap<String, u64>,
    window_start: Instant,
    window_duration: Duration,
}

impl OutcomeTracker {
    pub(crate) fn new() -> Self {
        Self {
            total: 0,
            successes: 0,
            failures: 0,
            error_kinds: HashMap::new(),
            window_start: Instant::now(),
            window_duration: Duration::from_secs(DEFAULT_VARIETY_WINDOW_SECS),
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.check_window();
        self.total += 1;
        self.successes += 1;
    }

    pub(crate) fn record_failure(&mut self, error_kind: &str) {
        self.check_window();
        self.total += 1;
        self.failures += 1;
        *self.error_kinds.entry(error_kind.to_string()).or_insert(0) += 1;
    }

    /// Success rate: 1.0 if no operations, successes/total otherwise.
    pub(crate) fn success_rate(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.successes as f64 / self.total as f64
        }
    }

    pub(crate) fn total_operations(&self) -> u64 {
        self.total
    }

    fn check_window(&mut self) {
        if self.window_start.elapsed() > self.window_duration {
            self.total = 0;
            self.successes = 0;
            self.failures = 0;
            self.error_kinds.clear();
            self.window_start = Instant::now();
        }
    }
}

impl Default for OutcomeTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Variety monitor for multiple domains — Ashby's Law tracking at the Regulation level.
///
/// # Epistemic grounding
/// - **crt:certainty** = Subjunctive
/// - **crt:force** = Hypothesis
/// - **mode** = IS
///
/// # Cybernetic role (TASK 1)
/// This is the **sensor** in the variety regulation feedback loop:
/// ```text
/// (MCP tool dispatch) → [VarietyMonitor.counter().increment()]
///     → [AlgedonicManager.check()] → [RuntimeAlert]
///     → [emit_critical_depletion()] → (agent behavior change)
/// ```
#[derive(Debug)]
pub struct VarietyMonitor {
    counters: HashMap<String, VarietyTracker>,
}

impl VarietyMonitor {
    /// Create a new variety monitor.
    ///
    /// expect: "The system creates variety monitors to track state diversity across domains"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — the monitor enables feedback loops
    /// \[P5\] Constraining: Essentialism — minimal defaults, empty counters
    /// post: returns VarietyMonitor with empty counters
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
        }
    }

    pub(crate) fn counter(&mut self, domain: &str) -> &mut VarietyTracker {
        self.counters.entry(domain.to_string()).or_default()
    }

    /// Get variety count for a domain.
    ///
    /// expect: "I can query variety counts for any tracked domain"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — variety measurement drives loop closure
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// pre:  domain is non-empty
    /// post: returns variety count, 0 if domain not tracked
    ///
    /// Degradation: an untracked domain is indistinguishable from a tracked
    /// domain with zero variety to the caller (both yield 0). To keep the two
    /// cases distinguishable to an operator reading logs, an untracked domain
    /// emits a `tracing::warn!` naming the stale signal before returning 0.
    /// A regulation loop that reads 0 here cannot tell whether the domain
    /// genuinely has no variety or was never registered; the warn is the only
    /// effector that surfaces the difference.
    pub fn variety_for_domain(&self, domain: &str) -> u64 {
        match self.counters.get(domain) {
            Some(counter) => counter.variety(),
            None => {
                tracing::warn!(
                    target: "hkask.regulation",
                    domain = %domain,
                    "variety_for_domain: domain is not tracked; returning 0 (stale signal — \
                     indistinguishable from a tracked domain with zero variety)",
                );
                0
            }
        }
    }

    /// List all tracked domains.
    ///
    /// expect: "I can enumerate all tracked domains for loop feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — domain enumeration enables loop feedback
    /// \[P8\] Constraining: Semantic Grounding — pure enumeration, no side effects
    /// post: returns Vec of domain name strings
    pub fn domains(&self) -> Vec<&str> {
        self.counters.keys().map(|s| s.as_str()).collect()
    }

    pub(crate) fn counters(&self) -> &HashMap<String, VarietyTracker> {
        &self.counters
    }

    /// Hard reset all trackers for a fresh session.
    /// Preserves domain entries but clears counts and EMAs.
    pub fn session_reset(&mut self) {
        for tracker in self.counters.values_mut() {
            tracker.session_reset();
        }
    }
}

impl Default for VarietyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// A single regulation cycle's full pipeline record for historical querying.
///
/// Captures the input/output of every phase: signals sensed, deviations
/// detected, actions produced, impact verified, decisions classified.
/// Enables post-hoc analysis of regulation effectiveness.
#[derive(Debug, Clone)]
pub struct RegulationCycleEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Count of afferent signals from sense phase.
    pub signals: u64,
    /// Count of deviations from compare phase.
    pub deviations: u64,
    /// Count of actions produced by compute phase.
    pub actions: u64,
    /// Count of actions verified by verify_impact.
    pub verified: u64,
    /// Decision counts from impact verification.
    pub accepted: u64,
    pub staged: u64,
    pub blocked: u64,
    /// Accumulated regulation health at this point.
    pub cumulative_effectiveness: f64,
}

/// Regulation state shared between threads
struct RegState {
    algedonic: Arc<ParkingRwLock<AlgedonicManager>>,
    tracker: VarietyMonitor,
    outcome: HashMap<String, OutcomeTracker>,
    regulation_health: RegulationHealth,
    regulation_history: VecDeque<RegulationCycleEntry>,
    max_regulation_history: usize,
    tool_stats: Arc<ToolStats>,
    skill_spans: SkillSpanStore,
}

impl RegState {
    fn new(threshold: u64) -> Self {
        Self::with_history_caps(
            threshold,
            crate::set_points::DEFAULT_MAX_REGULATION_HISTORY,
            crate::set_points::DEFAULT_MAX_SKILL_SPAN_HISTORY,
            crate::set_points::DEFAULT_MAX_ALERTS,
        )
    }

    fn with_history_caps(
        threshold: u64,
        max_regulation_history: usize,
        max_skill_span_history: usize,
        max_alerts: usize,
    ) -> Self {
        let algedonic = Arc::new(ParkingRwLock::new(AlgedonicManager::with_max_alerts(
            threshold,
            DEFAULT_EXPECTED_VARIETY,
            max_alerts,
        )));
        let tracker = VarietyMonitor::new();
        let outcome = HashMap::new();
        let regulation_health = RegulationHealth::default();
        let regulation_history = VecDeque::with_capacity(max_regulation_history);
        let tool_stats = Arc::new(ToolStats::new());
        let skill_spans = SkillSpanStore::with_capacity(max_skill_span_history);

        Self {
            algedonic,
            tracker,
            outcome,
            regulation_health,
            regulation_history,
            max_regulation_history,
            tool_stats,
            skill_spans,
        }
    }
}

/// Regulation runtime — single entry point for observability and regulation
///
/// Cheaply clonable: the state is `Arc`-wrapped, so cloning only bumps the
/// reference count. All clones share the same inner state (variety tracker,
/// algedonic manager).
#[derive(Clone)]
pub struct RegulationLedger {
    state: Arc<RwLock<RegState>>,
}

impl RegulationLedger {
    /// Create a Regulation runtime with a custom threshold.
    ///
    /// expect: "I can create a Regulation runtime with a configurable variety threshold"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — runtime creation enables regulation
    /// \[P7\] Constraining: Evolutionary Architecture — threshold config emerged from real usage
    /// pre:  threshold > 0
    /// post: returns RegulationLedger with configured threshold
    pub fn with_threshold(threshold: u64) -> Self {
        Self {
            state: Arc::new(RwLock::new(RegState::new(threshold))),
        }
    }

    /// Create a Regulation runtime with history caps from `SetPoints`.
    ///
    /// Allows operators to tune how many regulation cycles and skill spans
    /// are retained for history queries, via YAML config or env vars.
    pub fn with_set_points(threshold: u64, set_points: &crate::set_points::SetPoints) -> Self {
        Self {
            state: Arc::new(RwLock::new(RegState::with_history_caps(
                threshold,
                set_points.max_regulation_history,
                set_points.max_skill_span_history,
                set_points.max_alerts,
            ))),
        }
    }

    /// Override the outcome quality thresholds from YAML configurable SetPoints.
    ///
    /// Called by the CyberneticsLoop when SetPointsConfig is loaded.
    ///
    /// expect: "The system provides configurable outcome quality thresholds for homeostatic regulation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — threshold configuration enables loop tuning
    /// \[P7\] Constraining: Evolutionary Architecture — thresholds emerged from real usage data
    /// pre:  warning >= 0.0, critical >= 0.0, warning > critical
    /// post: outcome thresholds updated for all domains
    pub async fn set_outcome_thresholds(&self, warning: f64, critical: f64) {
        let state = self.state.write().await;
        let mut mgr = state.algedonic.write();
        mgr.set_outcome_thresholds(warning, critical);
    }

    // ── Health & Alerts ──

    /// Get Regulation health status.
    ///
    /// expect: "I can query the cybernetic health status of the entire Regulation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — health query drives loop decisions
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// post: returns LedgerHealth with current state
    pub async fn health(&self) -> LedgerHealth {
        let state = self.state.read().await;
        // Compute sum of EMA variety across all tracked domains.
        let ema_sum: f64 = state
            .tracker
            .counters()
            .values()
            .map(|t| t.variety_ema())
            .sum();
        {
            let mgr = state.algedonic.read();
            reg_health_check(&mgr, ema_sum)
        }
    }

    /// Record a regulation cycle's impact decisions for metacognition observability.
    ///
    /// Called by `CyberneticsLoop::tick()` after `verify_impact()`.
    /// Aggregates Accept/Stage/Block counts so the Curator can assess whether
    /// regulatory actions are actually improving system state.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — cycle recording enables metacognitive feedback
    /// \[P8\] Constraining: Semantic Grounding — Accept/Stage/Block counts are measured, not guessed
    /// pre:  entry signals and actions are non-empty
    /// post: regulation health counters updated, history appended
    pub async fn record_regulation_cycle(&self, entry: RegulationCycleEntry) {
        let mut state = self.state.write().await;
        state.regulation_health.total_cycles += 1;
        state.regulation_health.accepted += entry.accepted;
        state.regulation_health.staged += entry.staged;
        state.regulation_health.blocked += entry.blocked;
        state.regulation_history.push_back(entry);
        if state.regulation_history.len() > state.max_regulation_history {
            state.regulation_history.pop_front();
        }
    }

    /// Get regulation health summary for metacognition.
    ///
    /// Returns the accumulated Accept/Stage/Block counts and effectiveness ratio
    /// across all recorded regulation cycles.
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — health query drives loop decisions
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// post: returns RegulationHealth with current Accept/Stage/Block counts
    pub async fn regulation_health(&self) -> RegulationHealth {
        let state = self.state.read().await;
        state.regulation_health.clone()
    }

    /// Get the last N regulation cycle entries for detailed analysis.
    ///
    /// Returns up to `n` most recent cycles, newest first.
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — history enables trend analysis for loop tuning
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// post: returns up to n entries, newest first; never exceeds the configured history cap
    pub async fn regulation_history(&self, n: usize) -> Vec<RegulationCycleEntry> {
        let state = self.state.read().await;
        state
            .regulation_history
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect()
    }

    /// Access the tool stats learner for recording and querying tool distributions.
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — tool stats inform energy and reliability decisions
    /// \[P8\] Constraining: Semantic Grounding — LogNormal distributions are computed from measured data
    /// post: returns `Arc<ToolStats>` shared reference
    pub async fn tool_stats(&self) -> Arc<ToolStats> {
        let state = self.state.read().await;
        Arc::clone(&state.tool_stats)
    }

    /// Get all alerts.
    ///
    /// expect: "I can retrieve all active runtime alerts for loop response"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — alert retrieval enables loop response
    /// \[P8\] Constraining: Semantic Grounding — pure observation, no transformation
    /// post: returns Vec of RuntimeAlert
    pub async fn alerts(&self) -> Vec<RuntimeAlert> {
        let state = self.state.read().await;
        state.algedonic.read().alerts().to_vec()
    }

    /// Get the configured default threshold from the algedonic manager.
    /// Get the configured default threshold.
    ///
    /// expect: "I can query the default variety threshold for loop tuning"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — threshold config enables loop tuning
    /// \[P7\] Constraining: Evolutionary Architecture — threshold emerged from real usage
    /// post: returns threshold value from algedonic manager
    pub async fn default_threshold(&self) -> u64 {
        let state = self.state.read().await;
        state.algedonic.read().default_threshold()
    }

    /// Get critical alerts only.
    ///
    /// expect: "I can filter alerts to only critical severity for prioritized response"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — critical alert filtering enables prioritised response
    /// \[P8\] Constraining: Semantic Grounding — pure observation, no transformation
    /// post: returns Vec of critical RuntimeAlert
    pub async fn critical_alerts(&self) -> Vec<RuntimeAlert> {
        let state = self.state.read().await;
        {
            state
                .algedonic
                .read()
                .critical_alerts()
                .into_iter()
                .cloned()
                .collect()
        }
    }

    /// Clear reviewed alerts from the in-memory algedonic log.
    ///
    /// Called by the `algedonic-review` skill (via the `AlgedonicLogSink`
    /// bridge) after the operator has reviewed the log and confirmed that
    /// escalated alerts have been persisted to the `EscalationQueue`. Retains
    /// unresolved Critical alerts (those that have not been escalated yet)
    /// so the live signal is not lost.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — clearing reviewed alerts closes the review loop
    /// post: reviewed alerts cleared from the in-memory log; unresolved Critical alerts retained
    pub async fn clear_reviewed_alerts(&self) {
        let state = self.state.write().await;
        let mut mgr = state.algedonic.write();
        mgr.clear_reviewed(true);
    }

    /// Clear ALL alerts from the in-memory log, including unresolved Critical.
    /// Use with caution — this loses live signals. The operator should only
    /// call this when the log is being reset for a fresh session or when all
    /// alerts have been reviewed and persisted to the `EscalationQueue`.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — hard reset clears the log for a fresh session
    /// post: all alerts cleared from the in-memory log
    pub async fn clear_all_alerts(&self) {
        let state = self.state.write().await;
        let mut mgr = state.algedonic.write();
        mgr.clear_reviewed(false);
    }

    /// Whether the in-memory algedonic log is approaching its cap.
    ///
    /// When true, the cybernetics loop emits an `AlgedonicLogApproachingCap`
    /// signal so the operator (or the `algedonic-review` skill) can review
    /// and clear reviewed entries before they are evicted unread.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// post: returns true iff the log is ≥ 80% of the cap
    pub async fn alert_log_approaching_cap(&self) -> bool {
        let state = self.state.read().await;
        state.algedonic.read().log_approaching_cap()
    }

    /// Number of alerts currently in the in-memory algedonic log.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// post: returns the current alert count
    pub async fn alert_log_count(&self) -> usize {
        let state = self.state.read().await;
        state.algedonic.read().alert_count()
    }

    // ── Variety ──

    /// Get variety counts across all domains.
    ///
    /// expect: "I can query variety measurements across all span namespaces"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — variety measurement drives loop closure
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// post: returns HashMap of namespace → variety count
    pub async fn variety(&self) -> HashMap<SpanNamespace, u64> {
        let state = self.state.read().await;
        let domains: Vec<String> = state
            .tracker
            .domains()
            .iter()
            .map(|s| s.to_string())
            .collect();
        drop(state);

        let mut results = HashMap::new();
        for domain in &domains {
            // Filter against CANONICAL_NAMESPACES — the single registry for all
            // Regulation namespace strings (core + domain). Replaces the old RegulationSpan::from_str
            // gate which previously only accepted core variants.
            if let Some(ns) = SpanNamespace::parse(domain) {
                let state = self.state.read().await;
                let count = state.tracker.variety_for_domain(domain);
                drop(state);
                results.insert(ns, count);
            }
        }
        results
    }

    /// Get variety for a specific domain.
    ///
    /// expect: "I can query domain-specific variety counts"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — domain-specific variety measurement
    /// \[P8\] Constraining: Semantic Grounding — pure observation, no transformation
    /// pre:  domain is non-empty
    /// post: returns variety count for domain
    pub async fn variety_for_domain(&self, domain: &str) -> u64 {
        let state = self.state.read().await;
        state.tracker.variety_for_domain(domain)
    }

    /// Reset all variety counters for a new session.
    ///
    /// Clears accumulated counts and EMAs while preserving domain entries
    /// (i.e., domains registered by the seam watcher remain). Call this
    /// at session start to prevent stale variety deficits from persisting
    /// across agent rebuilds.
    ///
    /// expect: "Variety counters reset cleanly across sessions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — clean session state
    /// post: all VarietyTracker counts and EMAs are zeroed
    pub async fn reset_variety(&self) {
        let mut state = self.state.write().await;
        state.tracker.session_reset();
    }

    /// Synchronous version of variety_for_domain — uses blocking_read() on the
    /// internal tokio RwLock. This enables sync contexts (e.g., metric collectors,
    /// CLI closures) to query Regulation variety counters without requiring async.
    /// Get variety for a domain (blocking).
    ///
    /// expect: "I can access Regulation observability synchronously — preserving generative capability"
    /// \[P3\] Motivating: Generative Space — sync access preserves generative capability
    /// \[P7\] Constraining: Evolutionary Architecture — blocking variant emerged from real usage
    /// \[P4\] Constraining: Clear Boundaries — must not be called from async context
    /// pre:  domain is non-empty
    /// post: returns variety count
    pub fn blocking_variety_for_domain(&self, domain: &str) -> u64 {
        let state = self.state.blocking_read();
        state.tracker.variety_for_domain(domain)
    }

    // ── Outcome Quality Tracking ──

    // ── Skill Feedback Span Storage ──

    /// Record a skill feedback span for later query by the same skill.
    ///
    /// Stores `reg.skill.<skill_id>.<phase>` span payloads so skills can
    /// query their own prior feedback (self-improvement τ_t / e_t signals).
    /// Unlike tracing events (which fire and vanish), this method persists
    /// the structured payload in the ledger.
    ///
    /// pre:  skill_id is non-empty; phase is "outcome" or "operator_feedback"
    /// post: span stored in SkillSpanStore, bounded to the configured max_skill_span_history per key
    pub async fn record_skill_span(&self, skill_id: &str, phase: &str, payload: serde_json::Value) {
        let span = StoredSkillSpan {
            namespace: format!("reg.skill.{skill_id}.{phase}"),
            skill_id: skill_id.to_string(),
            phase: phase.to_string(),
            payload,
            recorded_at: chrono::Utc::now(),
        };
        let mut state = self.state.write().await;
        state.skill_spans.record(span);
    }

    /// Query prior skill feedback spans for a given skill and phase.
    ///
    /// Returns the stored spans (most recent last) for `reg.skill.<skill_id>.<phase>`.
    /// Returns an empty Vec if no spans have been recorded.
    ///
    /// pre:  skill_id is non-empty; phase is "outcome" or "operator_feedback"
    /// post: returns Vec<StoredSkillSpan> (may be empty)
    pub async fn query_skill_feedback(&self, skill_id: &str, phase: &str) -> Vec<StoredSkillSpan> {
        let state = self.state.read().await;
        state.skill_spans.query(skill_id, phase)
    }

    /// Return skill IDs that have at least one recorded span for the given
    /// phase. Used by the metacognition loop to enumerate skills for drift
    /// detection.
    pub async fn skill_ids_with_feedback(&self, phase: &str) -> Vec<String> {
        let state = self.state.read().await;
        state.skill_spans.skill_ids_with_phase(phase)
    }

    /// Record a tool outcome (success or failure) for outcome quality tracking.
    ///
    /// Complements variety tracking by measuring not just *what* was done
    /// but *how well*. After recording, checks outcome thresholds and emits
    /// alerts if success rate drops below warning/critical levels.
    /// Record an outcome (success/failure) for a domain.
    ///
    /// expect: "The system records tool outcomes for quality-based regulation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — outcome tracking enables quality-based regulation
    /// \[P4\] Constraining: Clear Boundaries — domain isolation enforces OCAP boundary
    /// pre:  domain is non-empty
    /// post: outcome tracked for domain
    pub async fn record_outcome(&self, domain: &str, success: bool, error_kind: Option<&str>) {
        {
            let mut state = self.state.write().await;
            let tracker = state.outcome.entry(domain.to_string()).or_default();
            if success {
                tracker.record_success();
            } else {
                tracker.record_failure(error_kind.unwrap_or("unknown"));
            }
        }
        self.check_outcome(domain).await;
    }

    /// Check outcome quality thresholds and emit alerts if degraded.
    ///
    /// Thresholds: success_rate < 0.50 → Warning, < 0.25 → Critical.
    /// Only checks when at least 5 operations have been recorded (avoids
    /// alert storms from small sample sizes).
    /// Check outcome health for a domain.
    ///
    /// expect: "I can check outcome quality to drive loop decisions"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — outcome check drives loop decisions
    /// \[P4\] Constraining: Clear Boundaries — threshold gating enforces boundary
    /// pre:  domain is non-empty
    /// post: returns Some(alert) if success rate below threshold, None if healthy
    pub async fn check_outcome(&self, domain: &str) -> Option<RuntimeAlert> {
        let (success_rate, total_ops) = {
            let state = self.state.read().await;
            let tracker = state.outcome.get(domain).cloned().unwrap_or_default();
            (tracker.success_rate(), tracker.total_operations())
        };

        // Only alert when we have enough data to be meaningful
        if total_ops < 5 {
            return None;
        }

        let state = self.state.write().await;
        let mut mgr = state.algedonic.write();
        mgr.check_outcome(domain, success_rate, total_ops).cloned()
    }

    /// Get outcome success rate for a domain.
    /// Get outcome success rate for a domain.
    ///
    /// expect: "I can query the success rate for a domain as a feedback metric"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — success rate is a feedback metric
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// pre:  domain is non-empty
    /// post: returns Some(rate) if domain tracked, None otherwise
    pub async fn outcome_success_rate(&self, domain: &str) -> Option<f64> {
        let state = self.state.read().await;
        state.outcome.get(domain).map(|t| t.success_rate())
    }

    /// Increment variety counter for a domain.
    ///
    /// expect: "The system increments variety counters to drive loop closure"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — variety counter drives loop closure
    /// \[P4\] Constraining: Clear Boundaries — domain isolation enforces OCAP boundary
    /// pre:  domain and state_name are non-empty
    /// post: variety counter incremented
    pub async fn increment_variety(&self, domain: &str, state_name: &str) {
        {
            let mut state = self.state.write().await;
            state.tracker.counter(domain).increment(state_name);
        }
        self.check_variety(domain).await;
    }

    /// Check variety health for a domain.
    ///
    /// expect: "I can check variety levels to determine if an alert is needed"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — variety check drives loop closure
    /// \[P4\] Constraining: Clear Boundaries — threshold gating enforces boundary
    /// pre:  domain is non-empty
    /// post: returns Some(alert) if variety below threshold, None if healthy
    pub async fn check_variety(&self, domain: &str) -> Option<RuntimeAlert> {
        let counter = {
            let state = self.state.read().await;
            state
                .tracker
                .counters()
                .get(domain)
                .cloned()
                .unwrap_or_else(VarietyTracker::new)
        };

        let state = self.state.write().await;
        let mut mgr = state.algedonic.write();
        mgr.check(&counter, domain).cloned()
    }

    /// Calibrate the variety threshold for a domain.
    ///
    /// expect: "I can calibrate variety thresholds from real usage patterns"
    /// \[P7\] Motivating: Evolutionary Architecture — threshold parameter emerged from real usage
    /// \[P4\] Constraining: Clear Boundaries — threshold gating enforces boundary
    /// pre:  domain is non-empty, new_threshold > 0
    /// post: threshold updated for domain
    pub async fn calibrate_threshold(&self, domain: &str, new_threshold: u64) {
        let state = self.state.write().await;
        {
            state
                .algedonic
                .write()
                .set_expected_variety(domain, new_threshold);
        }
        drop(state);
    }

    /// Synchronous variant of `calibrate_threshold` for startup/bootstrap contexts.
    ///
    /// Uses `blocking_write()` on the internal `ParkingRwLock` — safe because
    /// this is called during bootstrap before the async runtime is fully active.
    /// Calibrate threshold (blocking).
    ///
    /// expect: "I can access Regulation observability synchronously — preserving generative capability"
    /// \[P3\] Motivating: Generative Space — sync access preserves generative capability
    /// \[P7\] Constraining: Evolutionary Architecture — blocking variant emerged from real usage
    /// \[P4\] Constraining: Clear Boundaries — must not be called from async context
    /// pre:  domain is non-empty, new_threshold > 0
    /// post: threshold updated
    pub fn calibrate_threshold_blocking(&self, domain: &str, new_threshold: u64) {
        let state = self.state.blocking_write();
        state
            .algedonic
            .write()
            .set_expected_variety(domain, new_threshold);
    }
}

impl Default for RegulationLedger {
    fn default() -> Self {
        Self::with_threshold(DEFAULT_VARIETY_MAX_DEFICIT as u64)
    }
}

/// No-op event sink for tests and contexts where Regulation event persistence
/// is not needed (e.g., seam watcher unit tests).
pub struct NoopEventSink;

impl RegulationSink for NoopEventSink {
    fn persist(&self, _event: &RegulationRecord) -> Result<(), hkask_types::InfrastructureError> {
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────────────
