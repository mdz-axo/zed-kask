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
//! - Effector: emit_critical_depletion() — broadcasts DepletionSignal to observers

use crate::algedonic::{
    AlgedonicManager, DEFAULT_EXPECTED_VARIETY, RuntimeAlert, reg_health_check,
};
use crate::energy::{AgentGasStatus, GasBudget, GasCost};
use crate::set_points::DEFAULT_VARIETY_MAX_DEFICIT;
use crate::tool_stats::ToolStats;

use hkask_types::WebID;
use hkask_types::event::{RegulationRecord, RegulationSink, SpanNamespace};
use hkask_types::regulation::{LedgerHealth, RegulationHealth};
use hkask_types::{BackpressureSignal, DepletionSignal, LedgerObserver};
use parking_lot::RwLock as ParkingRwLock;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;

/// Maximum number of regulation cycles retained for history queries.
const MAX_REGULATION_HISTORY: usize = 100;
/// Maximum number of skill feedback spans retained per skill+phase.
const MAX_SKILL_SPAN_HISTORY: usize = 50;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing;

/// Error healing callback: (error_string, operation_name).
type HealCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

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
/// Retains the last `MAX_SKILL_SPAN_HISTORY` spans per key.
#[derive(Debug, Default)]
pub(crate) struct SkillSpanStore {
    spans: HashMap<String, VecDeque<StoredSkillSpan>>,
}

impl SkillSpanStore {
    fn key(skill_id: &str, phase: &str) -> String {
        format!("{skill_id}:{phase}")
    }

    pub(crate) fn record(&mut self, span: StoredSkillSpan) {
        let key = Self::key(&span.skill_id, &span.phase);
        let deque = self.spans.entry(key).or_default();
        if deque.len() >= MAX_SKILL_SPAN_HISTORY {
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
    pub fn variety_for_domain(&self, domain: &str) -> u64 {
        self.counters.get(domain).map(|c| c.variety()).unwrap_or(0)
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
    gas_budgets: Arc<tokio::sync::RwLock<HashMap<WebID, GasBudget>>>,
    regulation_health: RegulationHealth,
    regulation_history: VecDeque<RegulationCycleEntry>,
    tool_stats: Arc<ToolStats>,
    skill_spans: SkillSpanStore,
}

impl RegState {
    fn new(threshold: u64) -> Self {
        let algedonic = Arc::new(ParkingRwLock::new(AlgedonicManager::new(
            threshold,
            DEFAULT_EXPECTED_VARIETY,
        )));
        let tracker = VarietyMonitor::new();
        let outcome = HashMap::new();
        let gas_budgets = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let regulation_health = RegulationHealth::default();
        let regulation_history = VecDeque::with_capacity(MAX_REGULATION_HISTORY);
        let tool_stats = Arc::new(ToolStats::new());
        let skill_spans = SkillSpanStore::default();
        Self {
            algedonic,
            tracker,
            outcome,
            gas_budgets,
            regulation_health,
            regulation_history,
            tool_stats,
            skill_spans,
        }
    }
}

/// Regulation runtime — single entry point for observability and regulation
///
/// Cheaply clonable: both fields are `Arc`-wrapped, so cloning only bumps
/// reference counts. All clones share the same inner state (variety tracker,
/// algedonic manager, subscribers).
#[derive(Clone)]
pub struct RegulationLedger {
    state: Arc<RwLock<RegState>>,
    subscribers: Arc<RwLock<Vec<Arc<dyn LedgerObserver>>>>,
    /// Optional heal callback: (error_string, operation_name).
    heal_error_cb: Option<HealCallback>,
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
            subscribers: Arc::new(RwLock::new(Vec::new())),
            heal_error_cb: None,
        }
    }

    /// Attach a self-healing callback for automatic error recovery on depletion.
    ///
    /// expect: "The system provides configurable error recovery for homeostatic self-regulation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — heal callback closes the recovery loop
    /// \[P4\] Constraining: Clear Boundaries — callback is user-owned, Regulation does not self-modify
    /// pre:  cb is valid
    /// post: RegulationLedger with heal callback configured
    pub fn with_heal_cb(mut self, cb: HealCallback) -> Self {
        self.heal_error_cb = Some(cb);
        self
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
        if state.regulation_history.len() > MAX_REGULATION_HISTORY {
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
    /// post: returns up to n entries, newest first; never exceeds MAX_REGULATION_HISTORY
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
    /// post: span stored in SkillSpanStore, bounded to MAX_SKILL_SPAN_HISTORY per key
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

        let alert = {
            let state = self.state.write().await;
            let mut mgr = state.algedonic.write();
            mgr.check_outcome(domain, success_rate, total_ops).cloned()
        };

        if let Some(ref a) = alert
            && a.severity == crate::algedonic::AlertSeverity::Critical
        {
            emit_critical_depletion(self, a).await;
        }

        alert
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

    /// Increment variety and check thresholds — the loop closes here.
    /// After persisting variety, notifies subscribers whose interest mask
    /// includes the relevant span namespace.
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
        let alert = self.check_variety(domain).await;

        // Notify subscribers interested in this domain's span namespace.
        // Uses SpanNamespace::parse directly (not RegulationSpan::from_str) so that
        // regulatory domains (reg.algedonic, reg.cybernetics, etc.) are included.
        if let Some(span_ns) = hkask_types::event::SpanNamespace::parse(domain) {
            let event = hkask_types::event::RegulationRecord::new(
                WebID::default(),
                hkask_types::event::Span::new(span_ns.clone(), "variety_incremented"),
                hkask_types::event::CyclePhase::Act,
                serde_json::json!({"domain": domain, "state": state_name}),
                0,
            );
            let subscribers = self.subscribers.read().await;
            for observer in subscribers.iter() {
                if observer.interest_mask().iter().any(|ns| ns == &span_ns) {
                    observer.on_event(&event).await;
                }
            }
            drop(subscribers);

            if let Some(ref a) = alert
                && a.severity == crate::algedonic::AlertSeverity::Critical
            {
                emit_critical_depletion(self, a).await;
            }
        }
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

        let alert = {
            let state = self.state.write().await;
            let mut mgr = state.algedonic.write();
            mgr.check(&counter, domain).cloned()
        };

        // Depletion signals are now emitted from increment_variety after
        // it receives the alert from check_variety. Kept here for direct
        // callers that don't go through increment_variety.
        if let Some(ref alert) = alert
            && alert.severity == crate::algedonic::AlertSeverity::Critical
        {
            emit_critical_depletion(self, alert).await;
        }

        alert
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

    // ── Bot Observation (Regulation Observer) ──

    /// Register a LedgerObserver to receive events matching its interest mask.
    ///
    /// Observers are notified asynchronously when:
    /// - A variety increment matches their interest mask (on_event)
    /// - A depletion signal fires for their agent (on_depletion)
    /// - A backpressure signal fires (on_backpressure)
    ///
    /// Use `subscribe_async` when calling from an async context.
    /// Subscribe an observer to Regulation events.
    ///
    /// expect: "I can explicitly subscribe an observer to receive Regulation events"
    /// \[P12\] Motivating: Affirmative Consent — observer registration requires explicit subscription
    /// \[P2\] Constraining: User Sovereignty — subscriber identity is user-owned (WebID-tagged)
    /// pre:  observer is valid
    /// post: observer added to subscribers
    pub fn subscribe(&self, observer: Arc<dyn LedgerObserver>) {
        let mut subscribers = self.subscribers.blocking_write();
        subscribers.push(observer);
    }

    /// Register a LedgerObserver to receive events matching its interest mask.
    ///
    /// This is the async version of subscribe, preferred when called from
    /// an async context (e.g., during bootstrap or from the API).
    /// Subscribe an observer (async).
    ///
    /// expect: "I can explicitly subscribe an async observer to receive Regulation events"
    /// \[P12\] Motivating: Affirmative Consent — observer registration requires explicit subscription
    /// \[P2\] Constraining: User Sovereignty — subscriber identity is user-owned (WebID-tagged)
    /// pre:  observer is valid
    /// post: observer added to subscribers
    pub async fn subscribe_async(&self, observer: Arc<dyn LedgerObserver>) {
        let mut subscribers = self.subscribers.write().await;
        subscribers.push(observer);
    }

    /// Broadcast a Regulation event to all subscribers whose interest mask
    /// covers the event's span namespace.
    ///
    /// This is the entry point that turns a `RegulationSink::persist` call
    /// into live observability: external emitters (MCP runtime, cybernetics
    /// loop, memory ports) forward their spans here, and registered
    /// `LedgerObserver`s (e.g. the Curator status surface) receive them.
    /// Fire-and-forget — callers must not rely on broadcast completion for
    /// correctness, matching the best-effort semantics of every existing
    /// emitter.
    ///
    /// pre:  event has a valid span namespace
    /// post: every subscriber whose `interest_mask` contains `event.span.namespace`
    ///       has had `on_event` scheduled; unmatched subscribers are skipped
    pub async fn publish_event(&self, event: RegulationRecord) {
        let span_ns = event.span.namespace.clone();
        let subscribers = self.subscribers.read().await;
        for observer in subscribers.iter() {
            if observer.interest_mask().iter().any(|ns| ns == &span_ns) {
                observer.on_event(&event).await;
            }
        }
    }

    /// Emit a backpressure signal to all subscribers.
    ///
    /// Called by the Cybernetics Loop when energy budget depletion
    /// reaches critical levels, signaling downstream loops to throttle.
    /// Emit a backpressure signal.
    ///
    /// expect: "The system emits backpressure signals to close the regulation loop"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — backpressure signal closes the regulation loop
    /// \[P4\] Constraining: Clear Boundaries — signal emission gates downstream throttling
    /// pre:  signal is valid
    /// post: backpressure signal emitted to subscribers
    pub async fn emit_backpressure(&self, signal: BackpressureSignal) {
        let subscribers = self.subscribers.read().await;
        for observer in subscribers.iter() {
            observer.on_backpressure(&signal).await;
        }
    }

    /// Register a energy budget for an agent.
    ///
    /// Called during agent pod creation so the Regulation can track and replenish budgets.
    /// Register an energy budget for an agent.
    ///
    /// expect: "I can register an energy budget for an agent to enable tracking"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — budget registration enables energy tracking
    /// \[P4\] Constraining: Clear Boundaries — budget cap enforces resource boundary
    /// pre:  agent is valid, budget is valid
    /// post: budget registered for agent
    pub async fn register_gas_budget(&self, agent: WebID, budget: GasBudget) {
        let state = self.state.read().await;
        let mut budgets = state.gas_budgets.write().await;
        budgets.insert(agent, budget);
    }

    /// Replenish a specific agent's energy budget by a specific amount.
    ///
    /// Returns the new remaining gas after replenishment, or 0 if the agent
    /// has no registered budget.
    /// Replenish an agent's energy budget.
    ///
    /// expect: "The system replenishes agent budgets on the regulation cycle"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — budget replenishment drives energy loop
    /// \[P4\] Constraining: Clear Boundaries — cap enforcement prevents over-replenishment
    /// pre:  agent is registered, amount > 0
    /// post: budget replenished, returns actual amount added
    pub async fn replenish_agent_budget(&self, agent: &WebID, amount: GasCost) -> GasCost {
        let state = self.state.read().await;
        let mut budgets = state.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(agent) {
            budget.replenish_by(amount);
            let remaining = budget.remaining();
            tracing::info!(
                target: "hkask.runtime",
                agent = %agent,
                amount = amount.0,
                remaining = remaining.0,
                "Replenished agent energy budget via Regulation runtime"
            );
            remaining
        } else {
            GasCost::ZERO
        }
    }

    /// Get a read-only snapshot of an agent's energy budget status.
    ///
    /// Returns `None` if the agent has no registered budget.
    /// Used by the Regulation service.
    /// Get agent energy status.
    ///
    /// expect: "I can query an agent's gas status for energy loop feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — gas status query drives energy loop decisions
    /// \[P8\] Constraining: Semantic Grounding — pure observation, no transformation
    /// pre:  agent is valid
    /// post: returns Some(status) if budget exists, None otherwise
    pub async fn agent_gas_status(&self, agent: &WebID) -> Option<AgentGasStatus> {
        let state = self.state.read().await;
        let budgets = state.gas_budgets.read().await;
        budgets.get(agent).map(AgentGasStatus::from)
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

/// `RegulationSink` that forwards events into a `RegulationLedger`'s
/// subscriber bus via `publish_event`.
///
/// Use this at composition roots that already hold a `RegulationLedger` and
/// want emitted spans to reach registered `LedgerObserver`s (e.g. the Curator
/// status surface) without standing up a durable `RegulationArchive`. The
/// broadcast is spawned onto the captured tokio handle so the sync
/// `RegulationSink::persist` contract is honoured without blocking the
/// caller's async context.
///
/// Failures to spawn (e.g. runtime shut down) are logged and swallowed —
/// observability is best-effort, never a correctness path.
pub struct LedgerSink {
    ledger: Arc<RwLock<RegulationLedger>>,
    handle: tokio::runtime::Handle,
}

impl LedgerSink {
    /// Capture the ledger and an explicit tokio handle to spawn broadcasts on.
    ///
    /// The handle must outlive the sink (typically the app-global tokio
    /// runtime). Passing the handle explicitly — rather than calling
    /// `Handle::current()` — lets callers on threads without a tokio
    /// reactor context (e.g. the GPUI foreground thread) construct the
    /// sink without panicking.
    ///
    /// pre:  `handle` belongs to a running tokio runtime
    /// post: returns a sink that spawns `publish_event` on `handle` for each persist
    pub fn new(ledger: Arc<RwLock<RegulationLedger>>, handle: tokio::runtime::Handle) -> Self {
        Self { ledger, handle }
    }
}

impl RegulationSink for LedgerSink {
    fn persist(&self, event: &RegulationRecord) -> Result<(), hkask_types::InfrastructureError> {
        let ledger = self.ledger.clone();
        let event = event.clone();
        self.handle.spawn(async move {
            ledger.read().await.publish_event(event).await;
        });
        Ok(())
    }
}

/// Build and broadcast a `DepletionSignal` for a critical algedonic alert.
async fn emit_critical_depletion(
    runtime: &RegulationLedger,
    alert: &crate::algedonic::RuntimeAlert,
) {
    let signal = DepletionSignal {
        agent: WebID::default(),
        remaining: alert.threshold.saturating_sub(alert.deficit),
        cap: alert.threshold,
        usage_ratio: if alert.threshold > 0 {
            alert.deficit as f64 / alert.threshold as f64
        } else {
            1.0
        },
    };

    // Attempt self-healing before broadcasting to observers
    if let Some(ref cb) = runtime.heal_error_cb {
        let msg = format!(
            "Regulation variety depletion: deficit={} threshold={} usage_ratio={:.2}",
            alert.deficit, alert.threshold, signal.usage_ratio
        );
        cb(&msg, "reg.depletion");
    }

    let subscribers = runtime.subscribers.read().await;
    for observer in subscribers.iter() {
        observer.on_depletion(&signal).await;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    //
    // TASK 1 cybernetic property: the VarietyMonitor sensor must count
    // distinct tool states per domain for Ashby's Law compliance.
    // A domain with 5 distinct tool invocations must report variety=5.
    #[test]
    fn variety_monitor_tracks_distinct_states() {
        let mut monitor = VarietyMonitor::new();

        // Simulate 5 distinct tool invocations in domain "inference"
        for tool in &["chat", "embed", "generate", "classify", "tokenize"] {
            monitor.counter("inference").increment(tool);
        }

        assert_eq!(monitor.variety_for_domain("inference"), 5);
    }

    //
    // When 3 distinct states exist but 10 are expected, deficit must be 7.
    #[test]
    fn variety_tracker_deficit_calculation() {
        let mut tracker = VarietyTracker::new();
        for i in 0..3 {
            tracker.increment(&format!("state_{}", i));
        }
        assert_eq!(tracker.deficit(10), 7);
        assert_eq!(tracker.variety(), 3);
    }

    //
    // Two domains must track variety independently.
    #[test]
    fn variety_monitor_multi_domain_isolation() {
        let mut monitor = VarietyMonitor::new();

        monitor.counter("tools").increment("chat");
        monitor.counter("tools").increment("embed");
        monitor.counter("models").increment("llama3");
        monitor.counter("models").increment("qwen3");
        monitor.counter("models").increment("deepseek");

        assert_eq!(monitor.variety_for_domain("tools"), 2);
        assert_eq!(monitor.variety_for_domain("models"), 3);
        assert_eq!(monitor.variety_for_domain("nonexistent"), 0);
    }

    //
    // OutcomeTracker must correctly compute success rate from recorded
    // successes and failures.
    #[test]
    fn outcome_tracker_success_rate_calculation() {
        let mut tracker = OutcomeTracker::new();

        // Empty tracker: 1.0 (no data = healthy)
        assert!((tracker.success_rate() - 1.0).abs() < 0.001);

        tracker.record_success();
        tracker.record_success();
        tracker.record_failure("timeout");
        // 2 successes, 1 failure → 0.666...
        assert!((tracker.success_rate() - 2.0 / 3.0).abs() < 0.001);
        assert_eq!(tracker.total_operations(), 3);
    }

    //
    // OutcomeTracker must track per-error-kind counts for diagnosis.
    #[test]
    fn outcome_tracker_error_kind_breakdown() {
        let mut tracker = OutcomeTracker::new();

        tracker.record_failure("timeout");
        tracker.record_failure("timeout");
        tracker.record_failure("permission_denied");
        tracker.record_success();

        assert_eq!(tracker.total_operations(), 4);
        // 1 success, 3 failures → 0.25
        assert!((tracker.success_rate() - 0.25).abs() < 0.001);
    }

    //
    // OutcomeTracker must reset its window after the configured duration.
    #[test]
    fn outcome_tracker_window_reset() {
        let mut tracker = OutcomeTracker::new();

        tracker.record_success();
        tracker.record_failure("error");
        assert_eq!(tracker.total_operations(), 2);

        // Force window expiry by setting window_start far in the past
        tracker.window_start = Instant::now() - Duration::from_secs(120);
        tracker.record_success();

        // After reset, only the new record should count
        assert_eq!(tracker.total_operations(), 1);
        assert!((tracker.success_rate() - 1.0).abs() < 0.001);
    }

    // ── SkillSpanStore tests ──

    #[test]
    fn skill_span_store_records_and_queries() {
        let mut store = SkillSpanStore::default();

        store.record(StoredSkillSpan {
            namespace: "reg.skill.lora-training.outcome".into(),
            skill_id: "lora-training".into(),
            phase: "outcome".into(),
            payload: serde_json::json!({"training_completed": true, "loss": 0.3}),
            recorded_at: chrono::Utc::now(),
        });

        store.record(StoredSkillSpan {
            namespace: "reg.skill.lora-training.outcome".into(),
            skill_id: "lora-training".into(),
            phase: "outcome".into(),
            payload: serde_json::json!({"training_completed": false, "oom_occurred": true}),
            recorded_at: chrono::Utc::now(),
        });

        let results = store.query("lora-training", "outcome");
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].payload["training_completed"],
            serde_json::json!(true)
        );
        assert_eq!(results[1].payload["oom_occurred"], serde_json::json!(true));
    }

    #[test]
    fn skill_span_store_returns_empty_for_unknown_skill() {
        let store = SkillSpanStore::default();
        let results = store.query("nonexistent", "outcome");
        assert!(results.is_empty());
    }

    #[test]
    fn skill_span_store_bounded_to_max_history() {
        let mut store = SkillSpanStore::default();

        // Record MAX_SKILL_SPAN_HISTORY + 10 spans
        for i in 0..(MAX_SKILL_SPAN_HISTORY + 10) {
            store.record(StoredSkillSpan {
                namespace: "reg.skill.lora-training.outcome".into(),
                skill_id: "lora-training".into(),
                phase: "outcome".into(),
                payload: serde_json::json!({"iteration": i}),
                recorded_at: chrono::Utc::now(),
            });
        }

        let results = store.query("lora-training", "outcome");
        assert_eq!(results.len(), MAX_SKILL_SPAN_HISTORY);
        // Oldest spans evicted — first remaining should be iteration 10
        assert_eq!(results[0].payload["iteration"], serde_json::json!(10));
    }

    #[test]
    fn skill_span_store_isolates_phases() {
        let mut store = SkillSpanStore::default();

        store.record(StoredSkillSpan {
            namespace: "reg.skill.lora-training.outcome".into(),
            skill_id: "lora-training".into(),
            phase: "outcome".into(),
            payload: serde_json::json!({"training_completed": true}),
            recorded_at: chrono::Utc::now(),
        });

        store.record(StoredSkillSpan {
            namespace: "reg.skill.lora-training.operator_feedback".into(),
            skill_id: "lora-training".into(),
            phase: "operator_feedback".into(),
            payload: serde_json::json!({"disposition": "accepted"}),
            recorded_at: chrono::Utc::now(),
        });

        // Querying outcome should not return operator_feedback spans
        let outcome = store.query("lora-training", "outcome");
        assert_eq!(outcome.len(), 1);
        assert_eq!(
            outcome[0].payload["training_completed"],
            serde_json::json!(true)
        );

        // Querying operator_feedback should not return outcome spans
        let feedback = store.query("lora-training", "operator_feedback");
        assert_eq!(feedback.len(), 1);
        assert_eq!(
            feedback[0].payload["disposition"],
            serde_json::json!("accepted")
        );
    }

    // ── LedgerSink / publish_event ──────────────────────────────────────
    //
    // The composition root wires emitters (MCP runtime, CyberneticsLoop) to a
    // RegulationSink. Replacing NoopEventSink with LedgerSink is what makes
    // emitted spans observable. These tests pin the contract: a persisted
    // record reaches subscribers whose interest mask matches, and does not
    // reach subscribers whose mask does not.

    /// A minimal `LedgerObserver` that records every event it receives, for
    /// asserting on the broadcast behaviour of `publish_event` / `LedgerSink`.
    struct CapturingObserver {
        interest: Vec<SpanNamespace>,
        seen: std::sync::Mutex<Vec<RegulationRecord>>,
    }

    impl CapturingObserver {
        fn new(interest: Vec<SpanNamespace>) -> Self {
            Self {
                interest,
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn seen_count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    use hkask_types::event::{CyclePhase, Span};

    #[async_trait::async_trait]
    impl LedgerObserver for CapturingObserver {
        fn interest_mask(&self) -> Vec<SpanNamespace> {
            self.interest.clone()
        }

        async fn on_event(&self, event: &RegulationRecord) {
            self.seen.lock().unwrap().push(event.clone());
        }

        async fn on_depletion(&self, _signal: &DepletionSignal) {}
        async fn on_backpressure(&self, _signal: &BackpressureSignal) {}
    }

    #[tokio::test]
    async fn publish_event_broadcasts_to_matching_subscribers_only() {
        let ledger = RegulationLedger::default();
        let tool_ns = SpanNamespace::new("reg.tool").unwrap();
        let memory_ns = SpanNamespace::new("reg.memory").unwrap();

        let tool_observer = Arc::new(CapturingObserver::new(vec![tool_ns.clone()]));
        let memory_observer = Arc::new(CapturingObserver::new(vec![memory_ns.clone()]));
        ledger
            .subscribe_async(Arc::clone(&tool_observer) as Arc<dyn LedgerObserver>)
            .await;
        ledger
            .subscribe_async(Arc::clone(&memory_observer) as Arc<dyn LedgerObserver>)
            .await;

        // A reg.tool event must reach the tool observer and not the memory observer.
        let tool_event = RegulationRecord::new(
            WebID::from_persona(b"test"),
            Span::new(tool_ns.clone(), "invoked"),
            CyclePhase::Act,
            serde_json::json!({"tool": "search"}),
            0,
        );
        ledger.publish_event(tool_event).await;
        assert_eq!(
            tool_observer.seen_count(),
            1,
            "tool observer should receive its matched event"
        );
        assert_eq!(
            memory_observer.seen_count(),
            0,
            "memory observer should not receive unmatched events"
        );
    }

    #[tokio::test]
    async fn ledger_sink_forwards_persisted_events_to_subscribers() {
        // LedgerSink::persist spawns the broadcast on the current tokio handle,
        // so this test must run inside a multi-thread runtime (the default for
        // #[tokio::test]).
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        let tool_ns = SpanNamespace::new("reg.tool").unwrap();
        let observer = Arc::new(CapturingObserver::new(vec![tool_ns.clone()]));
        ledger
            .read()
            .await
            .subscribe_async(Arc::clone(&observer) as Arc<dyn LedgerObserver>)
            .await;

        let sink = LedgerSink::new(ledger.clone(), tokio::runtime::Handle::current());
        let event = RegulationRecord::new(
            WebID::from_persona(b"test"),
            Span::new(tool_ns, "invoked"),
            CyclePhase::Act,
            serde_json::json!({"tool": "search"}),
            0,
        );
        sink.persist(&event).expect("persist should be infallible");

        // The broadcast is spawned fire-and-forget; yield to let it run.
        for _ in 0..50 {
            if observer.seen_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            observer.seen_count(),
            1,
            "LedgerSink must forward persisted events to subscribers"
        );
    }
}
