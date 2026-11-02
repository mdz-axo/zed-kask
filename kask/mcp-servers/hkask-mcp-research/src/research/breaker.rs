//! Circuit breaker state for outbound search providers.
//!
//! The 2026-11-01 incident (research server): Semantic Scholar 429'd and
//! arXiv stalled or 429'd on EVERY compound fusion call for two-plus
//! minutes, each call re-burning the same doomed backoff and timeout
//! budget, because nothing in the server remembered between calls that a
//! provider was dead. The per-call retry policies (backoff inside
//! `search`, the compound timeout) treat each fusion call as independent;
//! parallel tool calls make them a storm.
//!
//! This registry is the cross-call memory. `record_provider_span` feeds it
//! alongside the performance aggregator (one choke point, both search
//! paths); `search_compound` consults it before fanning out and skips
//! providers whose breaker is open, surfacing the skip as its own metadata
//! record (`providers_skipped` — a skip is NOT a failure: no request was
//! made, and a skip does not make the response uncacheable the way a
//! provider failure does).
//!
//! Scope: the breaker governs automatic fan-out only. An explicit
//! `provider=` call, the `quick` pick, and the intent-routed pick are the
//! caller's deliberate choices — those paths probe regardless, and their
//! outcomes still feed this registry.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Consecutive failures before the breaker opens. Matches
/// `MIN_SAMPLES_FOR_LIVE` (performance.rs) — the smallest run at which
/// the live-scoring channel already trusts a trend.
pub(crate) const BREAKER_FAILURE_THRESHOLD: u32 = 3;

/// How long an open breaker stays open before the next fusion call probes
/// the provider again. Provider rate-limit windows are seconds; the
/// cooldown instead bounds repeated doomed fan-out across a burst of
/// parallel tool calls, and gives a slow-walking origin (arXiv) time to
/// release the client.
pub(crate) const BREAKER_COOLDOWN_SECS: u64 = 60;

#[derive(Debug, Default, Clone, Copy)]
struct BreakerState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

#[derive(Debug)]
pub(crate) struct ProviderBreakerRegistry {
    states: HashMap<String, BreakerState>,
    cooldown: Duration,
}

impl Default for ProviderBreakerRegistry {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            cooldown: Duration::from_secs(BREAKER_COOLDOWN_SECS),
        }
    }
}

impl ProviderBreakerRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Test seam: the same registry with a short cooldown so expiry paths
    /// are testable without wall-clock waits. Production always uses
    /// `BREAKER_COOLDOWN_SECS`.
    #[cfg(test)]
    pub(crate) fn with_cooldown_for_tests(cooldown: Duration) -> Self {
        Self {
            states: HashMap::new(),
            cooldown,
        }
    }

    /// Record one provider outcome (the same span `record_provider_span`
    /// emits). A success closes and resets the breaker; a failure extends
    /// the consecutive run and opens the breaker at the threshold.
    pub(crate) fn note_outcome(&mut self, kind: &str, success: bool) {
        let state = self.states.entry(kind.to_string()).or_default();
        if success {
            state.consecutive_failures = 0;
            state.open_until = None;
        } else {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if state.consecutive_failures >= BREAKER_FAILURE_THRESHOLD {
                state.open_until = Some(Instant::now() + self.cooldown);
            }
        }
    }

    /// Whether automatic fan-out should skip this provider this call.
    /// `None` = include (breaker closed, never opened, or cooldown expired —
    /// the next call is a probe; a failed probe re-opens the breaker).
    /// `Some(reason)` names the state for the skip metadata record.
    pub(crate) fn check(&self, kind: &str) -> Option<String> {
        let state = self.states.get(kind)?;
        let open_until = state.open_until?;
        let now = Instant::now();
        if now >= open_until {
            return None;
        }
        let remaining_secs = (open_until - now).as_secs();
        Some(format!(
            "circuit breaker open: {} consecutive failures, {remaining_secs}s cooldown remaining",
            state.consecutive_failures
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaker_opens_at_the_failure_threshold() {
        let mut reg = ProviderBreakerRegistry::new();
        for i in 1..BREAKER_FAILURE_THRESHOLD {
            reg.note_outcome("arxiv", false);
            assert!(
                reg.check("arxiv").is_none(),
                "failure {i} below the threshold must not skip"
            );
        }
        reg.note_outcome("arxiv", false); // the threshold failure
        let reason = reg.check("arxiv").expect("open at the threshold");
        assert!(
            reason.contains("3 consecutive failures"),
            "reason names the run: {reason}"
        );
        assert!(
            reason.contains("cooldown remaining"),
            "reason names the cooldown: {reason}"
        );
    }

    #[test]
    fn breaker_resets_on_success() {
        let mut reg = ProviderBreakerRegistry::new();
        for _ in 0..BREAKER_FAILURE_THRESHOLD {
            reg.note_outcome("semantic_scholar", false);
        }
        assert!(reg.check("semantic_scholar").is_some());
        reg.note_outcome("semantic_scholar", true);
        assert!(
            reg.check("semantic_scholar").is_none(),
            "one success closes and resets the breaker"
        );
    }

    #[test]
    fn breaker_ignores_never_failing_kinds() {
        let reg = ProviderBreakerRegistry::new();
        assert!(reg.check("brave").is_none());
    }

    #[tokio::test]
    async fn breaker_probe_after_cooldown_reopens_on_failed_probe() {
        let mut reg = ProviderBreakerRegistry::with_cooldown_for_tests(Duration::from_millis(50));
        for _ in 0..BREAKER_FAILURE_THRESHOLD {
            reg.note_outcome("arxiv", false);
        }
        assert!(reg.check("arxiv").is_some(), "open immediately");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            reg.check("arxiv").is_none(),
            "cooldown expired — the next call is a probe"
        );
        reg.note_outcome("arxiv", false); // the failed probe
        assert!(
            reg.check("arxiv").is_some(),
            "a failed probe re-opens the breaker"
        );
    }
}
