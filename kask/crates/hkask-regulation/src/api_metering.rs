//! API metering — per-key rate limiting, gas tracking, and Regulation spans.
//!
//! # Design (essentialist G2)
//! - Rate limit state is in-memory (HashMap). Acceptable per handoff:
//!   "What happens to rate limit state on process restart? Acceptable?"
//! - Rate-limited vs allocation-exhausted: separate span fields, not separate span types.
//! - `endpoint_weight` table: hardcoded initially (configurable later).
//!
//! # Span: reg.api.request
//! Every API call with `Authorization: Bearer hk_...` opens a span tracking:
//! `key_id, endpoint, scope_matched, gas_consumed, allocation_remaining, rate_limit_status`

use hkask_types::Encumbrance;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::id::ApiKeyId;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tracing::{info, warn};

// ── Endpoint weight table (hardcoded, per essentialist G2) ────────────────────

/// Per-key rate limit configuration. If present, overrides global defaults.
#[derive(Debug, Clone)]
pub struct KeyRateLimits {
    pub max_rpm: u32,
    pub max_tokens_per_day: u64,
}

/// Configurable rate limit settings with learning parameters.
///
/// Rate limits start from `default_max_rpm` / `default_max_tokens_per_day` and
/// adapt per-key based on observed usage patterns. The learning loop widens
/// limits for keys that consistently hit rate walls and narrows them for keys
/// with sustained low utilization, within bounded ranges.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Default requests-per-minute for any key without a per-key override.
    pub default_max_rpm: u32,
    /// Default daily token quota for any key without a per-key override.
    pub default_max_tokens_per_day: u64,
    /// Per-key overrides (static, from config). Learning adjustments layer on top.
    pub per_key_overrides: HashMap<ApiKeyId, KeyRateLimits>,
    // ── Learning parameters ──
    /// Whether the learning loop is active.
    pub learning_enabled: bool,
    /// How often the learning loop runs.
    pub adaptation_interval: Duration,
    /// Factor by which RPM widens when a key consistently hits RateExceeded (e.g., 1.2 = +20%).
    pub rpm_widen_factor: f64,
    /// Factor by which RPM narrows after sustained low utilization (e.g., 0.9 = -10%).
    pub rpm_narrow_factor: f64,
    /// Floor for adaptive RPM — limits never drop below this.
    pub min_rpm: u32,
    /// Ceiling for adaptive RPM — limits never rise above this.
    pub max_rpm: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            default_max_rpm: 60,
            default_max_tokens_per_day: 500_000,
            per_key_overrides: HashMap::new(),
            learning_enabled: true,
            adaptation_interval: Duration::from_secs(300),
            rpm_widen_factor: 1.2,
            rpm_narrow_factor: 0.9,
            min_rpm: 10,
            max_rpm: 600,
        }
    }
}

impl RateLimitConfig {
    /// Load rate limit configuration from environment variables.
    ///
    /// Variables (all optional, with defaults):
    /// - `HKASK_API_RATE_LIMIT_RPM` — default requests per minute (default: 60)
    /// - `HKASK_API_RATE_LIMIT_TOKENS_PER_DAY` — daily token quota (default: 500000)
    /// - `HKASK_API_RATE_LIMIT_LEARNING` — enable adaptive learning, "true"/"false" (default: true)
    /// - `HKASK_API_RATE_LIMIT_INTERVAL_SECS` — learning loop interval (default: 300)
    /// - `HKASK_API_RATE_LIMIT_MIN_RPM` — floor for adaptive RPM (default: 10)
    /// - `HKASK_API_RATE_LIMIT_MAX_RPM` — ceiling for adaptive RPM (default: 600)
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_RPM")
            && let Ok(n) = v.parse::<u32>()
        {
            config.default_max_rpm = n;
        }
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_TOKENS_PER_DAY")
            && let Ok(n) = v.parse::<u64>()
        {
            config.default_max_tokens_per_day = n;
        }
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_LEARNING") {
            config.learning_enabled = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_INTERVAL_SECS")
            && let Ok(n) = v.parse::<u64>()
        {
            config.adaptation_interval = Duration::from_secs(n);
        }
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_MIN_RPM")
            && let Ok(n) = v.parse::<u32>()
        {
            config.min_rpm = n;
        }
        if let Ok(v) = std::env::var("HKASK_API_RATE_LIMIT_MAX_RPM")
            && let Ok(n) = v.parse::<u32>()
        {
            config.max_rpm = n;
        }
        config
    }
}

/// Weight multiplier per endpoint category. Heavier endpoints cost more gas.
#[derive(Debug, Clone, Copy)]
pub struct EndpointWeight(pub f64);

impl Default for EndpointWeight {
    fn default() -> Self {
        EndpointWeight(1.0)
    }
}

/// Look up the weight for an endpoint path.
/// Hardcoded table — configurable in future release.
/// Get endpoint weight for rate limiting.
///
/// expect: "The system assigns weight multipliers to API endpoints for rate limiting"
/// \[P9\] Motivating: Homeostatic Self-Regulation — per-request rate limiting for API stability
/// \[P7\] Constraining: Evolutionary Architecture — hardcoded table to be configurable later
/// pre:  path is non-empty
/// post: returns EndpointWeight based on path pattern
pub fn endpoint_weight(path: &str) -> EndpointWeight {
    if path.contains("embed-corpus") || path.contains("compose") {
        EndpointWeight(5.0)
    } else if path.contains("chat") || path.contains("invoke") {
        EndpointWeight(2.0)
    } else {
        EndpointWeight(1.0)
    }
}

// ── Rate limit state (in-memory) ──────────────────────────────────────────────

/// Per-key rate limit tracking.
#[derive(Debug, Clone)]
struct RateLimitBucket {
    /// Timestamps of requests in the current minute window.
    request_timestamps: Vec<Instant>,
    /// Tokens consumed today (UTC day boundary).
    tokens_today: u64,
    /// Day identifier (UTC date string) for token reset.
    day_key: String,
}

impl RateLimitBucket {
    fn new() -> Self {
        Self {
            request_timestamps: Vec::new(),
            tokens_today: 0,
            day_key: String::new(),
        }
    }

    /// Prune timestamps older than 60 seconds.
    fn prune(&mut self, now: Instant) {
        let cutoff = now - std::time::Duration::from_secs(60);
        self.request_timestamps.retain(|t| *t >= cutoff);
    }

    /// Check if a new request would exceed the per-minute limit.
    fn check_rpm(&mut self, now: Instant, max_rpm: u32) -> bool {
        self.prune(now);
        (self.request_timestamps.len() as u32) < max_rpm
    }

    /// Record a request.
    fn record_request(&mut self, now: Instant) {
        self.prune(now);
        self.request_timestamps.push(now);
    }

    /// Check and record token consumption for today.
    fn check_tokens(&mut self, tokens: u64, max_tokens_per_day: u64, today: &str) -> bool {
        if self.day_key != today {
            self.tokens_today = 0;
            self.day_key = today.to_string();
        }
        if self.tokens_today + tokens > max_tokens_per_day {
            return false;
        }
        self.tokens_today += tokens;
        true
    }
}

/// Rate limit status returned after a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitStatus {
    /// Request is within all limits.
    Ok,
    /// Per-minute request rate exceeded.
    RateExceeded,
    /// Daily token quota exceeded.
    TokensExceeded,
}

impl RateLimitStatus {
    /// Get string representation of alert type.
    ///
    /// expect: "I can query the rate limit status as a stable string for Regulation feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — rate limit status feedback for Regulation
    /// \[P8\] Constraining: Semantic Grounding — string representation must be stable across versions
    /// post: returns lowercase alert type string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::RateExceeded => "rate_exceeded",
            Self::TokensExceeded => "tokens_exceeded",
        }
    }
}

// ── API Meter ────────────────────────────────────────────────────────────────

/// In-memory API meter for per-key rate limiting and gas tracking.
///
/// # Design (essentialist G2)
/// - Single `HashMap<ApiKeyId, RateLimitBucket>` — no separate store abstraction.
/// - `check_and_record` is the single entry point for rate limit enforcement.
/// - Adaptive limits layer on top of config defaults via `key_limits`.
pub struct ApiMeter {
    buckets: HashMap<ApiKeyId, RateLimitBucket>,
    config: RateLimitConfig,
    /// Per-key effective (adaptive) limits. Start from config defaults, evolve via `learn()`.
    key_limits: HashMap<ApiKeyId, KeyRateLimits>,
    /// Per-key rate-exceeded count since last learning tick.
    rate_exceeded_count: HashMap<ApiKeyId, u32>,
    /// Whether the learning loop is running.
    learning_alive: AtomicBool,
}

impl ApiMeter {
    /// Create a new empty meter with default config.
    pub fn new() -> Self {
        Self::with_config(RateLimitConfig::default())
    }

    /// Create a new meter with the given rate limit configuration.
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            buckets: HashMap::new(),
            key_limits: config.per_key_overrides.clone(),
            rate_exceeded_count: HashMap::new(),
            config,
            learning_alive: AtomicBool::new(false),
        }
    }

    /// Check rate limits and record the request if within limits.
    ///
    /// Uses per-key effective limits (adaptive if learning is enabled, otherwise
    /// config defaults). Returns `RateLimitStatus::Ok` if the request can proceed.
    #[must_use]
    pub fn check_and_record(
        &mut self,
        key_id: ApiKeyId,
        tokens_this_request: u64,
    ) -> RateLimitStatus {
        let limits = self.effective_limits(&key_id);
        let now = Instant::now();
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        let bucket = self
            .buckets
            .entry(key_id)
            .or_insert_with(RateLimitBucket::new);

        if !bucket.check_rpm(now, limits.max_rpm) {
            *self.rate_exceeded_count.entry(key_id).or_insert(0) += 1;
            return RateLimitStatus::RateExceeded;
        }

        if !bucket.check_tokens(tokens_this_request, limits.max_tokens_per_day, &today) {
            return RateLimitStatus::TokensExceeded;
        }

        bucket.record_request(now);
        RateLimitStatus::Ok
    }

    /// Get the effective limits for a key — adaptive if present, otherwise config defaults.
    fn effective_limits(&self, key_id: &ApiKeyId) -> KeyRateLimits {
        self.key_limits
            .get(key_id)
            .cloned()
            .unwrap_or(KeyRateLimits {
                max_rpm: self.config.default_max_rpm,
                max_tokens_per_day: self.config.default_max_tokens_per_day,
            })
    }

    /// Run one learning pass — adapts per-key RPM limits based on observed patterns.
    ///
    /// Rules:
    /// - Keys that hit RateExceeded >= 3 times since last pass → widen RPM by `rpm_widen_factor`.
    /// - Keys with 0 RateExceeded and < 20% RPM utilization → narrow RPM by `rpm_narrow_factor`.
    /// - All adjustments clamped to `[min_rpm, max_rpm]`.
    ///
    /// Returns a description of adjustments made (for observability/logging).
    pub fn learn(&mut self) -> Vec<String> {
        let mut adjustments = Vec::new();
        let exceeded_counts: Vec<(ApiKeyId, u32)> = self.rate_exceeded_count.drain().collect();

        for (key_id, exceeded_count) in exceeded_counts {
            let current = self.effective_limits(&key_id);
            let current_rpm = self.current_rpm(key_id);

            let new_rpm = if exceeded_count >= 3 {
                // Consistently hitting rate wall — widen
                let widened =
                    (current.max_rpm as f64 * self.config.rpm_widen_factor).round() as u32;
                widened.clamp(self.config.min_rpm, self.config.max_rpm)
            } else if exceeded_count == 0 && current_rpm < (current.max_rpm as f64 * 0.2) as u32 {
                // Sustained low utilization — narrow (but don't be punitive)
                let narrowed =
                    (current.max_rpm as f64 * self.config.rpm_narrow_factor).round() as u32;
                narrowed.clamp(self.config.min_rpm, self.config.max_rpm)
            } else {
                current.max_rpm
            };

            if new_rpm != current.max_rpm {
                let direction = if new_rpm > current.max_rpm {
                    "widened"
                } else {
                    "narrowed"
                };
                info!(
                    target: "hkask.api.metering.learn",
                    key_id = %key_id,
                    old_rpm = current.max_rpm,
                    new_rpm,
                    direction,
                    exceeded_count,
                    "Adaptive rate limit adjusted",
                );
                adjustments.push(format!(
                    "key {} RPM {} {} to {}",
                    key_id, direction, current.max_rpm, new_rpm
                ));
                self.key_limits.insert(
                    key_id,
                    KeyRateLimits {
                        max_rpm: new_rpm,
                        max_tokens_per_day: current.max_tokens_per_day,
                    },
                );
            }
        }

        adjustments
    }

    /// Spawn a background learning loop that periodically adapts rate limits.
    pub fn spawn_learning_loop(meter: Arc<std::sync::RwLock<Self>>) {
        let (enabled, interval) = meter
            .read()
            .map(|m| (m.config.learning_enabled, m.config.adaptation_interval))
            .unwrap_or((false, Duration::from_secs(300)));
        if !enabled {
            return;
        }
        meter
            .write()
            .map(|m| m.learning_alive.store(true, Ordering::Release))
            .ok();

        tokio::spawn(async move {
            info!(
                target: "hkask.api.metering.learn",
                interval_secs = interval.as_secs(),
                "API metering learning loop started",
            );
            loop {
                tokio::time::sleep(interval).await;
                if let Ok(mut m) = meter.write() {
                    let adjustments = m.learn();
                    if !adjustments.is_empty() {
                        info!(
                            target: "hkask.api.metering.learn",
                            adjustment_count = adjustments.len(),
                            "Learning pass complete",
                        );
                    }
                } else {
                    warn!(target: "hkask.api.metering.learn", "RwLock poisoned — learning loop exiting");
                    break;
                }
            }
        });
    }

    /// Get the current request count in the last minute for a key.
    /// Get current RPM for a key.
    ///
    /// expect: "I can query the current requests-per-minute rate for any API key"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — current rate is the cybernetic state
    /// \[P8\] Constraining: Semantic Grounding — RPM count must be stable and accurate
    /// pre:  key_id is valid
    /// post: returns current requests per minute
    #[must_use]
    pub fn current_rpm(&self, key_id: ApiKeyId) -> u32 {
        let now = Instant::now();
        self.buckets
            .get(&key_id)
            .map(|b| {
                let cutoff = now - std::time::Duration::from_secs(60);
                b.request_timestamps
                    .iter()
                    .filter(|t| **t >= cutoff)
                    .count() as u32
            })
            .unwrap_or(0)
    }
}

impl Default for ApiMeter {
    fn default() -> Self {
        Self::new()
    }
}

// ── Regulation span: reg.api.request ────────────────────────────────────────────────

/// Observation data for a `reg.api.request` span.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiRequestSpan {
    pub key_id: String,
    pub endpoint: String,
    pub scope_matched: bool,
    pub gas_consumed: u64,
    pub allocation_remaining: u64,
    pub rate_limit_status: String,
}

impl ApiRequestSpan {
    /// Build a span observation from metering data.
    /// Create a new API request span.
    ///
    /// expect: "The system creates Regulation observation spans for every metered API request"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — span creation is the Regulation observation layer
    /// \[P8\] Constraining: Semantic Grounding — span fields must be traceable to source
    /// pre:  path and method are non-empty
    /// post: returns ApiRequestSpan
    pub fn new(
        key_id: &str,
        endpoint: &str,
        scope_matched: bool,
        gas_consumed: u64,
        encumbrance: Option<&Encumbrance>,
        rate_limit_status: RateLimitStatus,
    ) -> Self {
        Self {
            key_id: key_id.to_string(),
            endpoint: endpoint.to_string(),
            scope_matched,
            gas_consumed,
            allocation_remaining: encumbrance.map(|e| e.remaining_rj()).unwrap_or(0),
            rate_limit_status: rate_limit_status.as_str().to_string(),
        }
    }

    /// Emit this span as a `reg.api.request` regulation record through the sink.
    ///
    /// Degrades gracefully: on namespace miss or persistence failure, logs a
    /// warning and continues (the request is not blocked by observability).
    pub fn emit_to(&self, sink: &dyn RegulationSink, observer: &WebID) {
        let Some(ns) = SpanNamespace::parse("reg.api.request") else {
            tracing::warn!(
                target: "hkask.api_metering",
                "reg.api.request namespace not registered — span not persisted"
            );
            return;
        };
        let span = Span::new(ns, "request");
        let observation = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        let event = RegulationRecord::new(*observer, span, CyclePhase::Sense, observation, 0);
        if let Err(e) = sink.persist(&event) {
            tracing::warn!(
                target: "hkask.api_metering",
                error = %e,
                "Failed to persist reg.api.request event — continuing"
            );
        }
    }
}

// ── Alert types ──────────────────────────────────────────────────────────────

/// Alert types emitted by the API metering system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiMeteringAlert {
    /// Key exceeded its rate limit.
    RateLimitExceeded { key_id: ApiKeyId, endpoint: String },
    /// Key allocation dropped below 20%.
    AllocationLow {
        key_id: ApiKeyId,
        remaining_rj: u64,
        total_rj: u64,
    },
    /// Key allocation exhausted (≤ 0).
    AllocationExhausted { key_id: ApiKeyId },
    /// Potential abuse pattern detected (3+ anomalies).
    AnomalyAbuse { key_id: ApiKeyId, pattern: String },
    /// Key used for endpoint outside declared scope.
    ScopeViolation {
        key_id: ApiKeyId,
        endpoint: String,
        allowed_scope: Vec<String>,
    },
}

impl ApiMeteringAlert {
    /// Regulation alert type string for span emission.
    /// Get alert type string.
    ///
    /// expect: "I can query the Regulation alert type classification for a metering event"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — alert type is the Regulation classification
    /// \[P8\] Constraining: Semantic Grounding — alert type labels must be stable across versions
    /// post: returns alert type label
    pub fn alert_type(&self) -> &'static str {
        match self {
            Self::RateLimitExceeded { .. } => "reg.api.rate_limit_exceeded",
            Self::AllocationLow { .. } => "reg.api.allocation_low",
            Self::AllocationExhausted { .. } => "reg.api.allocation_exhausted",
            Self::AnomalyAbuse { .. } => "reg.api.anomaly_abuse",
            Self::ScopeViolation { .. } => "reg.api.scope_violation",
        }
    }

    /// Severity level for Regulation algedonic signaling.
    /// Get severity string.
    ///
    /// expect: "I can query the algedonic severity level for a metering alert"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — severity is the algedonic signal
    /// \[P8\] Constraining: Semantic Grounding — severity labels must be stable across versions
    /// post: returns severity label
    pub fn severity(&self) -> &'static str {
        match self {
            Self::RateLimitExceeded { .. } => "warning",
            Self::AllocationLow { .. } => "info",
            Self::AllocationExhausted { .. } => "critical",
            Self::AnomalyAbuse { .. } => "critical",
            Self::ScopeViolation { .. } => "warning",
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_weight_embed_corpus_is_heavy() {
        assert!((endpoint_weight("embed-corpus").0 - 5.0).abs() < f64::EPSILON);
        assert!((endpoint_weight("compose").0 - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn endpoint_weight_default_is_one() {
        assert!((endpoint_weight("read-specs").0 - 1.0).abs() < f64::EPSILON);
        assert!((endpoint_weight("unknown").0 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rate_limit_bucket_prunes_old_requests() {
        let mut bucket = RateLimitBucket::new();
        let now = Instant::now();
        let old = now - std::time::Duration::from_secs(61);

        bucket.request_timestamps.push(old);
        bucket.request_timestamps.push(now);
        bucket.prune(now);

        assert_eq!(bucket.request_timestamps.len(), 1);
    }

    #[test]
    fn rate_limit_bucket_enforces_rpm() {
        let mut bucket = RateLimitBucket::new();
        let now = Instant::now();

        // Fill to limit (max_rpm = 3)
        for _ in 0..3 {
            assert!(bucket.check_rpm(now, 3));
            bucket.record_request(now);
        }
        // 4th should be rejected
        assert!(!bucket.check_rpm(now, 3));
    }

    #[test]
    fn token_tracking_resets_on_new_day() {
        let mut bucket = RateLimitBucket::new();
        assert!(bucket.check_tokens(500, 1000, "2026-06-13"));
        assert_eq!(bucket.tokens_today, 500);
        // New day resets
        assert!(bucket.check_tokens(800, 1000, "2026-06-14"));
        assert_eq!(bucket.tokens_today, 800);
    }

    #[test]
    fn api_meter_enforces_limits() {
        let mut meter = ApiMeter::new();
        let key = ApiKeyId::new();

        // First 3 requests within limit (default_max_rpm = 60)
        for _ in 0..3 {
            assert_eq!(meter.check_and_record(key, 100), RateLimitStatus::Ok);
        }

        // RPM should show 3
        assert_eq!(meter.current_rpm(key), 3);
    }

    #[test]
    fn api_request_span_serialization() {
        let span = ApiRequestSpan::new(
            "k_test",
            "/api/specs/123",
            true,
            500,
            None,
            RateLimitStatus::Ok,
        );
        let json = serde_json::to_string(&span).unwrap();
        assert!(json.contains("k_test"));
        assert!(json.contains("/api/specs/123"));
        assert!(json.contains("ok"));
    }

    /// Capture sink for testing regulation record emission.
    struct CaptureSink {
        last_event: std::sync::Mutex<Option<RegulationRecord>>,
    }

    impl RegulationSink for CaptureSink {
        fn persist(
            &self,
            event: &RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            *self.last_event.lock().unwrap_or_else(|e| e.into_inner()) = Some(event.clone());
            Ok(())
        }
    }

    #[test]
    fn api_request_span_emit_to_persists_event() {
        let sink = CaptureSink {
            last_event: std::sync::Mutex::new(None),
        };
        let span = ApiRequestSpan::new(
            "k_test",
            "/api/wallet/balance",
            true,
            0,
            None,
            RateLimitStatus::Ok,
        );
        span.emit_to(&sink, &WebID::default());

        let event = sink
            .last_event
            .lock()
            .unwrap()
            .clone()
            .expect("event was persisted");
        assert_eq!(event.span.namespace.as_str(), "reg.api.request");
        assert_eq!(event.span.path, "reg.api.request.request");
        assert_eq!(event.phase, CyclePhase::Sense);
        // Observation contains the serialized span fields
        let obs = &event.observation;
        assert_eq!(obs["key_id"], "k_test");
        assert_eq!(obs["endpoint"], "/api/wallet/balance");
        assert_eq!(obs["rate_limit_status"], "ok");
    }

    #[test]
    fn alert_severity_levels() {
        assert_eq!(
            ApiMeteringAlert::AllocationExhausted {
                key_id: ApiKeyId::new()
            }
            .severity(),
            "critical"
        );
        assert_eq!(
            ApiMeteringAlert::RateLimitExceeded {
                key_id: ApiKeyId::new(),
                endpoint: "/test".into()
            }
            .severity(),
            "warning"
        );
        assert_eq!(
            ApiMeteringAlert::AllocationLow {
                key_id: ApiKeyId::new(),
                remaining_rj: 100,
                total_rj: 1000
            }
            .severity(),
            "info"
        );
    }
}
