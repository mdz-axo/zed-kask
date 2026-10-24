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

use hkask_types::event::{RegulationRecord, RegulationSink};
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
/// `StoredSkillSpan` is the bounded in-process working view used by drift
/// sensing. Production operator-feedback records are restored into this view
/// from the durable RegulationArchive after restart.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OperatorFeedbackObservation {
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl OperatorFeedbackObservation {
    #[must_use]
    pub fn new(accepted: bool, note: Option<String>) -> Self {
        Self { accepted, note }
    }

    #[must_use]
    pub fn into_payload(self) -> serde_json::Value {
        serde_json::json!({
            "accepted": self.accepted,
            "note": self.note,
        })
    }

    #[must_use]
    pub fn from_payload(payload: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(payload.clone()).ok()
    }
}

#[derive(Debug, Clone)]
pub struct StoredSkillSpan {
    /// Skill ID extracted from the invocation (e.g., "code-review").
    pub skill_id: String,
    /// Phase: `outcome` or `operator_feedback`.
    pub phase: String,
    /// Structured field payload (e.g., {"success": true, "error": "..."}).
    pub payload: serde_json::Value,
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

/// Current observation window for variety, outcomes, and advice evidence (1 minute).
pub const OBSERVATION_WINDOW_SECS: u64 = 60;

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
            window_duration: Duration::from_secs(OBSERVATION_WINDOW_SECS),
            ema: 0.0,
        }
    }

    pub(crate) fn increment(&mut self, key: &str) {
        self.check_window();
        *self.counts.entry(key.to_string()).or_insert(0) += 1;
    }

    pub(crate) fn variety(&self) -> u64 {
        if self.window_start.elapsed() >= self.window_duration {
            0
        } else {
            self.counts.len() as u64
        }
    }

    /// Session-level exponential moving average of variety.
    /// Survives window resets — decays slowly (α = 0.1 per reset).
    pub(crate) fn variety_ema(&self) -> f64 {
        self.ema
    }

    pub(crate) fn deficit(&self, expected_variety: u64) -> u64 {
        // An empty window means the domain is idle — no states observed
        // since the last reset. Idle is not deficit: counting a resting
        // domain at full deficit made quiet periods read as maximal
        // variety loss (the OutcomeTracker's not-tool-fault exclusion
        // (`is_not_tool_fault_kind`) excludes environment gaps from
        // success-rate math for the same reason — a missing observation
        // is not a negative one).
        if self.variety() == 0 {
            return 0;
        }
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
            window_duration: Duration::from_secs(OBSERVATION_WINDOW_SECS),
        }
    }

    pub(crate) fn record_success(&mut self) {
        self.check_window();
        self.total += 1;
        self.successes += 1;
    }

    pub(crate) fn record_failure(&mut self, error_kind: &str) {
        self.check_window();
        // Not-tool-fault kinds don't count toward the success rate — see
        // `hkask_types::tool_response::is_not_tool_fault_kind`: environment
        // gaps (a binary not installed, a credential not provisioned) are
        // operator-actionable signals, and `invalid_argument` rejections are
        // caller-caused — the tool behaved as designed by rejecting malformed
        // arguments (the model-caused classes: truncated tool-call JSON,
        // dropped parameters). Counting either made healthy servers read as
        // failing (a media server without yt-dlp degraded the whole media
        // domain) and fired false `ToolReliabilityDegraded` alerts. They still
        // land in the per-kind breakdown so diagnosis can see them.
        if !hkask_types::tool_response::is_not_tool_fault_kind(error_kind) {
            self.total += 1;
            self.failures += 1;
        }
        *self.error_kinds.entry(error_kind.to_string()).or_insert(0) += 1;
    }

    /// No current sample is not a success or a failure.
    pub(crate) fn success_rate(&self) -> Option<f64> {
        let total = self.total_operations();
        (total > 0).then(|| self.successes as f64 / total as f64)
    }

    pub(crate) fn total_operations(&self) -> u64 {
        if self.window_start.elapsed() >= self.window_duration {
            0
        } else {
            self.total
        }
    }

    /// Raw success count. Callers reporting a domain snapshot must gate on
    /// `total_operations() > 0` — these counters go stale between window
    /// expiry and the next write.
    pub(crate) fn successes(&self) -> u64 {
        self.successes
    }

    /// Raw failure count of tool-fault failures only (not-tool-fault kinds
    /// never increment it). Same window-expiry caveat as `successes`.
    pub(crate) fn failures(&self) -> u64 {
        self.failures
    }

    /// Per-error-kind counts for diagnosis, most frequent first, then by
    /// kind name for deterministic output. Includes not-tool-fault kinds —
    /// the breakdown is the visibility surface for kinds excluded from the
    /// success-rate math.
    pub(crate) fn error_kind_counts(&self) -> Vec<(String, u64)> {
        let mut counts: Vec<(String, u64)> = self
            .error_kinds
            .iter()
            .map(|(kind, count)| (kind.clone(), *count))
            .collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
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

    /// Whether the observation window is still live (has not expired
    /// without a write). Expired trackers report no data — their counters
    /// are stale until the next write resets them. Note a LIVE tracker can
    /// still have `total_operations() == 0`: a domain whose only in-window
    /// calls failed with not-tool-fault kinds (e.g. `invalid_argument`)
    /// counts nothing toward the rate but its per-kind breakdown is real
    /// diagnosis data.
    pub(crate) fn window_live(&self) -> bool {
        self.window_start.elapsed() < self.window_duration
    }
}

/// One domain's outcome-tracker state, serialized into the tool-reliability
/// diagnosis surfaces: the `reg.outcome.tool_domains` span and the
/// board card's alert context. This names the
/// failing domain — the aggregate success rate the sensor reports cannot.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct DomainOutcomeSnapshot {
    pub domain: String,
    pub successes: u64,
    /// Tool-fault failures only — not-tool-fault kinds never increment it.
    pub failures: u64,
    pub total_operations: u64,
    pub success_rate: Option<f64>,
    /// (kind, count) pairs, most frequent first. Includes not-tool-fault
    /// kinds — the breakdown is the visibility surface for kinds excluded
    /// from the success-rate math.
    pub error_kinds: Vec<(String, u64)>,
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
pub(crate) struct VarietyMonitor {
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

    /// All tracked domains with their live trackers — the feed's landing
    /// surface. Read by `RegulationLedger::health` (current deficit level)
    /// and `variety` (query surface).
    pub(crate) fn counters(&self) -> &HashMap<String, VarietyTracker> {
        &self.counters
    }
}

impl Default for VarietyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Regulation state shared between threads
struct RegState {
    algedonic: Arc<ParkingRwLock<AlgedonicManager>>,
    tracker: VarietyMonitor,
    outcome: HashMap<String, OutcomeTracker>,
    regulation_health: RegulationHealth,
    skill_spans: SkillSpanStore,
    native_first: NativeFirstWindow,
}

/// Rolling record of substantive agent turns for the native-first rate
/// sensor: one boolean per completed substantive turn (true = native-first,
/// zero kask instruments used). Non-substantive turns (short prompts) are
/// not recorded — the sensor measures drift on real work, not
/// acknowledgments. Capped to a rolling window so the ledger's memory stays
/// bounded.
const NATIVE_FIRST_WINDOW_CAP: usize = 200;

#[derive(Default)]
struct NativeFirstWindow {
    records: std::collections::VecDeque<bool>,
}

impl NativeFirstWindow {
    fn record(&mut self, native_first: bool) {
        if self.records.len() >= NATIVE_FIRST_WINDOW_CAP {
            self.records.pop_front();
        }
        self.records.push_back(native_first);
    }

    /// The rolling rate: (native-first fraction, samples). `None` when the
    /// window is empty — the no-data state, never a fabricated 0.0 (the
    /// `.rules` `unwrap_or(0)` trap: an empty window would read as perfect
    /// instrument compliance while nothing has been measured).
    fn rate(&self) -> Option<(f64, u64)> {
        let total = self.records.len();
        if total == 0 {
            return None;
        }
        let native = self.records.iter().filter(|&&native| native).count() as f64;
        Some((native / total as f64, total as u64))
    }
}

impl RegState {
    fn new(threshold: u64) -> Self {
        Self::with_history_caps(
            threshold,
            crate::set_points::DEFAULT_MAX_SKILL_SPAN_HISTORY,
            crate::set_points::DEFAULT_MAX_ALERTS,
        )
    }

    fn with_history_caps(threshold: u64, max_skill_span_history: usize, max_alerts: usize) -> Self {
        let algedonic = Arc::new(ParkingRwLock::new(AlgedonicManager::with_max_alerts(
            threshold,
            DEFAULT_EXPECTED_VARIETY,
            max_alerts,
        )));
        let tracker = VarietyMonitor::new();
        let outcome = HashMap::new();
        let regulation_health = RegulationHealth::default();
        let skill_spans = SkillSpanStore::with_capacity(max_skill_span_history);

        Self {
            algedonic,
            tracker,
            outcome,
            regulation_health,
            skill_spans,
            native_first: NativeFirstWindow::default(),
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

    /// Create a Regulation runtime with history caps and outcome thresholds from `SetPoints`.
    ///
    /// expect: "My configured outcome thresholds control alerts from startup"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — operator configuration controls sensitivity
    /// pre: set_points passed SetPoints::validate (as enforced by load_set_points)
    /// post: history caps and outcome thresholds are applied before any observations
    pub fn with_set_points(threshold: u64, set_points: &crate::set_points::SetPoints) -> Self {
        let state = RegState::with_history_caps(
            threshold,
            set_points.max_skill_span_history,
            set_points.max_alerts,
        );
        state.algedonic.write().set_outcome_thresholds(
            set_points.outcome_warning_threshold,
            set_points.outcome_critical_threshold,
        );
        Self {
            state: Arc::new(RwLock::new(state)),
        }
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
            // Current deficit from the live trackers — a level. The previous
            // implementation summed the alert log, which accumulates an
            // entry per check cycle and can only grow: any threshold
            // compared against it tripped forever, and outcome-quality
            // alerts (deficit = failure-rate %) leaked into the variety
            // metric through the shared log.
            let overall_deficit = mgr.current_total_deficit(state.tracker.counters());
            reg_health_check(&mgr, ema_sum, overall_deficit)
        }
    }

    /// Record a regulation cycle's impact decisions for metacognition observability.
    ///
    /// Called by `CyberneticsLoop::tick()` after evidence-bearing rollout
    /// checks complete. Aggregates Accept/Stage/Block observations for
    /// metacognition without treating advisory routing as an intervention.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — cycle recording enables metacognitive feedback
    /// \[P8\] Constraining: Semantic Grounding — Accept/Stage/Block counts are measured, not guessed
    /// pre:  counts come from this tick's impact verification
    /// post: regulation health counters updated
    pub(crate) async fn record_cycle_outcome(&self, accepted: u64, staged: u64, blocked: u64) {
        let mut state = self.state.write().await;
        state.regulation_health.total_cycles += 1;
        state.regulation_health.accepted += accepted;
        state.regulation_health.staged += staged;
        state.regulation_health.blocked += blocked;
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

    /// Get the active runtime alerts.
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
    /// Separate log-maintenance operation; the review workflow neither calls
    /// it nor uses it to verify a board card. Retains unresolved Critical
    /// alerts so the live signal is not lost.
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
    /// call this only for a separately authorized log reset.
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
    /// inspect the board and log before entries self-evict.
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

    /// Number of actionable (Warning or Critical) alerts currently in the
    /// in-memory algedonic log. Sensed by the cybernetics loop as
    /// `AlgedonicEvents` — Info diagnostics don't count toward the review
    /// demand.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// post: returns the current actionable alert count
    pub async fn actionable_alert_count(&self) -> usize {
        let state = self.state.read().await;
        state.algedonic.read().actionable_alert_count()
    }

    /// Number of escalated alerts still in the in-memory diagnostic log.
    /// This is not the board's awaiting-review count; that count is read
    /// from cards not yet Done. Sensed as `PendingEscalations`.
    ///
    /// expect: "The system provides homeostatic self-regulation through variety tracking, algedonic alerting, and regulation record observation"
    /// post: returns the current escalated-alert count
    pub async fn escalated_alert_count(&self) -> usize {
        let state = self.state.read().await;
        state.algedonic.read().escalated_alert_count()
    }

    // ── Variety ──

    /// Get variety counts across all tracked domains.
    ///
    /// Domains are MCP server names — the tool-dispatch feed taxonomy shared
    /// with outcome tracking (`record_outcome` / `record_variety`). The
    /// previous `SpanNamespace::parse` filter accepted only canonical span
    /// namespaces, hiding every server-named domain from this query and
    /// making it useless against the live feed.
    ///
    /// expect: "I can query variety measurements across all tracked domains"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — variety measurement drives loop closure
    /// \[P8\] Constraining: Semantic Grounding — pure measurement, no transformation
    /// post: returns HashMap of domain → distinct-state count for the current window
    pub async fn variety(&self) -> HashMap<String, u64> {
        let state = self.state.read().await;
        state
            .tracker
            .counters()
            .iter()
            .map(|(domain, tracker)| (domain.clone(), tracker.variety()))
            .collect()
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

    // ── Outcome Quality Tracking ──

    // ── Skill Feedback Span Storage ──

    /// Record a skill feedback span for later query by the same skill.
    ///
    /// Stores `reg.skill.<skill_id>.<phase>` span payloads in the bounded
    /// working view so skills can query prior feedback (self-improvement τ_t /
    /// e_t signals). Production operator feedback is persisted to the
    /// RegulationArchive before this method is called and rehydrated here
    /// after restart.
    ///
    /// pre:  skill_id is non-empty; phase is "outcome" or "operator_feedback"
    /// post: span stored in SkillSpanStore, bounded to the configured max_skill_span_history per key
    pub async fn record_skill_span(&self, skill_id: &str, phase: &str, payload: serde_json::Value) {
        let span = StoredSkillSpan {
            skill_id: skill_id.to_string(),
            phase: phase.to_string(),
            payload,
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
    pub(crate) async fn query_skill_feedback(
        &self,
        skill_id: &str,
        phase: &str,
    ) -> Vec<StoredSkillSpan> {
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
    /// Uses the configured outcome thresholds (defaults: < 0.50 → Warning,
    /// < 0.25 → Critical).
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
        mgr.check_outcome(domain, success_rate?, total_ops).cloned()
    }

    /// Per-domain outcome snapshots for the tool-reliability diagnosis
    /// surfaces, ordered by domain name for deterministic output. A domain
    /// whose window has expired reports zero operations and no error kinds
    /// — stale counters never leak into a snapshot. A live domain whose
    /// only in-window calls failed with not-tool-fault kinds reports
    /// `total_operations: 0` with its per-kind counts intact: the rate
    /// excludes those failures, the breakdown must not.
    pub(crate) async fn outcome_breakdown(&self) -> Vec<DomainOutcomeSnapshot> {
        let state = self.state.read().await;
        let mut snapshots: Vec<DomainOutcomeSnapshot> = state
            .outcome
            .iter()
            .map(|(domain, tracker)| {
                let live = tracker.window_live();
                DomainOutcomeSnapshot {
                    domain: domain.clone(),
                    successes: if live { tracker.successes() } else { 0 },
                    failures: if live { tracker.failures() } else { 0 },
                    total_operations: tracker.total_operations(),
                    success_rate: tracker.success_rate(),
                    error_kinds: if live {
                        tracker.error_kind_counts()
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect();
        snapshots.sort_by(|a, b| a.domain.cmp(&b.domain));
        snapshots
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

    /// Record a completed agent turn for the native-first rate sensor.
    ///
    /// Non-substantive turns (short prompts, acknowledgments, mode
    /// toggles) are the legitimate skip, not an error — the sensor measures
    /// drift on real work only.
    ///
    /// expect: "Regulation senses instrument absence on agent turns, not only tool failures"
    /// [P9] Motivating: Homeostatic Self-Regulation — closes the drift feedback loop's unsensed leg
    /// pre:  none
    /// post: substantive turns are recorded into the rolling window; non-substantive turns leave the window unchanged
    pub async fn record_agent_turn(&self, substantive: bool, native_first: bool) {
        if !substantive {
            return;
        }
        let mut state = self.state.write().await;
        state.native_first.record(native_first);
    }

    /// The rolling native-first rate over recorded substantive turns:
    /// (rate, samples). Returns `None` when no substantive turns have been
    /// recorded — the no-data state, never a fabricated 0.0 (which would
    /// read as perfect instrument compliance while nothing has been
    /// measured).
    pub async fn native_first_rate(&self) -> Option<(f64, u64)> {
        let state = self.state.read().await;
        state.native_first.rate()
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

#[cfg(test)]
mod tests;
