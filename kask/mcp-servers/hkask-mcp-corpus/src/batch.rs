//! Shared batch-processing infrastructure for concurrent LLM operations.
//!
//! Eliminates the duplicated semaphore-gated-concurrency + retry-with-backoff +
//! degraded-outcome-classification skeleton that was hand-rolled across
//! `corpus_generate_qa_batch`, `corpus_extract_assertions`, `embed_batch_from_jsonl`,
//! and `corpus_tag_chunks`.

use std::future::Future;

use hkask_types::InferenceError;

/// Failure-rate threshold (percent) above which a batch run reports `degraded`
/// outcome. A run exceeding this rate indicates systemic issues (model
/// unavailable, rate limiting, adversarial input) and must not be reported as
/// `success`.
pub(crate) const DEGRADED_FAILURE_THRESHOLD: usize = 10;

/// Maximum LLM retry attempts for batch operations. Matches the 3-attempt
/// pattern used across all batch tool methods.
pub(crate) const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchStatus {
    Success,
    Degraded,
}

impl BatchStatus {}

/// Outcome of a batch run, classifying the failure rate against the degraded threshold.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BatchOutcome {
    pub failed: usize,
    pub total: usize,
    pub status: BatchStatus,
}

impl BatchOutcome {
    pub fn from_counts(failed: usize, total: usize) -> Self {
        let status = if Self::is_degraded(failed, total) {
            BatchStatus::Degraded
        } else {
            BatchStatus::Success
        };
        Self {
            failed,
            total,
            status,
        }
    }

    pub fn failure_pct(&self) -> usize {
        (self.failed * 100).saturating_div(self.total.max(1))
    }

    pub fn is_degraded(failed: usize, total: usize) -> bool {
        (failed * 100).saturating_div(total.max(1)) >= DEGRADED_FAILURE_THRESHOLD
    }

    pub fn log_if_degraded(&self, target: &str, operation: &str) {
        if self.status == BatchStatus::Degraded {
            tracing::warn!(
                target = target,
                failed = self.failed,
                total = self.total,
                failure_pct = self.failure_pct(),
                threshold_pct = DEGRADED_FAILURE_THRESHOLD,
                "{operation} run degraded — failure rate exceeds threshold",
            );
        }
    }
}

/// Only transport/capacity failures warrant another inference attempt. Do not
/// guess retryability from provider prose (which can contain arbitrary text).
pub(crate) fn inference_error_is_transient(error: &InferenceError) -> bool {
    matches!(
        error,
        InferenceError::Connection(_) | InferenceError::Overloaded(_) | InferenceError::Timeout(_)
    )
}

/// The two concrete error types already used by this helper retain their
/// identity; authorization/configuration failures must not become retryable
/// merely because callers share a scheduling loop.
pub(crate) trait BatchRetryError: std::fmt::Display {
    fn is_transient(&self) -> bool;
}

impl BatchRetryError for InferenceError {
    fn is_transient(&self) -> bool {
        inference_error_is_transient(self)
    }
}

impl BatchRetryError for hkask_types::EmbeddingGenerationError {
    fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Connection(_) | Self::Api(408 | 429 | 500..=599, _)
        )
    }
}

/// The retry loop's outcome: the value plus how many retries it took, so
/// tool summaries can surface retry telemetry (the 2026-09-29 inference
/// saturation wedge was visible only in WARN logs — the embed summary
/// reported embedded/failed with no retry signal).
pub(crate) struct RetryOutcome<T> {
    pub value: T,
    /// Retries consumed after the first attempt (0 = succeeded first try).
    pub retries: u32,
}

/// Retry a typed inference operation with exponential backoff.
///
/// Backoff: `2^attempts` seconds (2s, 4s for attempts 1, 2). The previous
/// `2^attempts * 5` schedule (10s, 20s) caused multi-hour wall times for
/// large corpus embedding runs where each batch retried independently —
/// 170 batches × 30s of sleep = 85 minutes of pure backoff. The 2s/4s
/// schedule is sufficient for transient throttles without making large
/// runs impractical.
///
/// Returns the original typed error immediately for permanent failures, or
/// after at most `max_retries` attempts for transient failures. Each retry logs
/// its actual attempt number and backoff; cancellation drops the active future.
pub(crate) async fn retry_with_backoff<T, E, F, Fut>(
    max_retries: u32,
    target: &str,
    context: &str,
    mut f: F,
) -> Result<RetryOutcome<T>, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: BatchRetryError,
{
    let mut attempts = 0u32;
    loop {
        match f().await {
            Ok(result) => {
                return Ok(RetryOutcome {
                    value: result,
                    retries: attempts,
                });
            }
            Err(e) => {
                attempts += 1;
                if !e.is_transient() || attempts >= max_retries.min(MAX_RETRIES) {
                    tracing::warn!(
                        target = target,
                        context = %context,
                        attempts = attempts,
                        error = %e,
                        "Inference stopped after {attempts} attempts",
                    );
                    return Err(e);
                }
                let backoff = std::time::Duration::from_secs(2u64.pow(attempts));
                tracing::warn!(
                    target = target,
                    context = %context,
                    attempt = attempts,
                    backoff_secs = backoff.as_secs(),
                    error = %e,
                    "Retry — backing off",
                );
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

// ─── Adaptive concurrency (AIMD ramp-up) ──────────────────────────────────

/// Floor for adaptive remote-LLM concurrency: the ramp starts here, never at
/// the ceiling. Remote LLM work probes a service's real capacity instead of
/// launching at max — the 2026-09-03 RunPod incident (96 concurrent page
/// requests at a 32-worker endpoint, instant rejections collapsed to
/// "empty output") was the motivating failure.
pub(crate) const ADAPTIVE_CONCURRENCY_FLOOR: usize = 2;

/// Fallback pause horizon when a breaker-open signal carries no parseable
/// retry-after (the shared bridge formats its cooldown into the
/// `CircuitOpen` message; an unparseable message must still pause, never
/// busy-spin the floor against a just-expired breaker).
pub(crate) const DEFAULT_BREAKER_RETRY_AFTER: std::time::Duration =
    std::time::Duration::from_secs(30);

/// Margin added to a PARSED retry-after horizon at the outcome classifier,
/// so the first post-pause probes do not land exactly on the shared
/// breaker's expiry boundary. Exact horizons from the executor's own
/// breaker (which already escalates its cooldown) are honored as-is.
const BREAKER_PAUSE_MARGIN: std::time::Duration = std::time::Duration::from_secs(2);

/// Breaker-open feedback carried by a gated call's error. The shared
/// inference bridge's circuit breaker rejects dispatch while open and names
/// its retry-after horizon in the `CircuitOpen` message; the limiter pauses
/// for that horizon instead of halving into the open breaker (the 2026-10-02
/// spiral: the limiter halved per instant failure but never paused, so
/// floor-concurrency pages kept re-tripping the shared breaker — 94
/// consecutive openings on one book run).
pub(crate) trait BreakerFeedback {
    /// `Some(retry_after)` when this error is a circuit-breaker-open verdict
    /// from the shared inference bridge.
    fn breaker_retry_after(&self) -> Option<std::time::Duration>;
}

impl BreakerFeedback for InferenceError {
    fn breaker_retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::CircuitOpen(message) => Some(
                parse_retry_after(message).unwrap_or(DEFAULT_BREAKER_RETRY_AFTER)
                    + BREAKER_PAUSE_MARGIN,
            ),
            _ => None,
        }
    }
}

/// The embedding port has no breaker variant today — the shared bridge's
/// `admit()` circuit gates the chat/vision path, not the embedding port.
/// Honest `None`: an embedding failure is a capacity failure for AIMD
/// purposes, never a fabricated pause signal.
impl BreakerFeedback for hkask_types::EmbeddingGenerationError {
    fn breaker_retry_after(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Parse the retry-after horizon out of the shared bridge's `CircuitOpen`
/// message. The bridge formats it as
/// `"...; retry after {retry_after:?}"` (a Debug-formatted `Duration`:
/// `30s`, `1m 30s`, `1.5s`, `500ms`, `1h`). Any drift from that format
/// returns `None` and the caller falls back to
/// [`DEFAULT_BREAKER_RETRY_AFTER`] — safe degradation, never a busy-spin.
fn parse_retry_after(message: &str) -> Option<std::time::Duration> {
    let rest = message.split("retry after").nth(1)?.trim_start();
    let mut total = 0f64;
    let mut parsed_any = false;
    for token in rest.split_whitespace() {
        // `split_once` would CONSUME the delimiter char ("30s" -> ("30", ""),
        // "500ms" -> ("500", "s") — silently reading 500ms as 500s); `find`
        // + `split_at` splits at the boundary without consuming it.
        let split_at = token
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(token.len());
        let (value, unit_part) = token.split_at(split_at);
        let Ok(value) = value.parse::<f64>() else {
            break;
        };
        let unit: String = unit_part
            .chars()
            .take_while(|c| c.is_alphabetic())
            .collect();
        let multiplier = match unit.as_str() {
            "h" => 3600.0,
            "m" => 60.0,
            "s" => 1.0,
            "ms" => 0.001,
            "us" | "\u{b5}s" => 0.000_001,
            "ns" => 0.000_000_001,
            _ => break,
        };
        total += value * multiplier;
        parsed_any = true;
    }
    if parsed_any && total.is_finite() && total >= 0.0 {
        // A hostile/garbage horizon must not overflow `from_secs_f64`; an
        // hour is the maximum sane breaker cooldown.
        Some(std::time::Duration::from_secs_f64(total.min(3600.0)))
    } else {
        None
    }
}

/// AIMD adaptive concurrency limiter for remote LLM work.
///
/// Starts at `floor`, grows additively (+1 per success) toward `ceiling`, and
/// backs off multiplicatively (halve per failure, floor-bounded). A service
/// with lower capacity than the ceiling is discovered by probing, not by
/// stampede. Local work (file IO) is NOT gated here — a static
/// bound is correct for a local resource; adaptation is for remote services.
///
/// A circuit-breaker-open verdict PAUSES all acquisitions until the
/// breaker's retry-after horizon: `current` snaps to the floor and
/// `acquire` sleeps until the pause expires. Halving alone cannot express
/// "do not dispatch until T" — at the floor it keeps firing doomed calls
/// that re-trip the open breaker (the 2026-10-02 spiral).
///
/// Backoff needs no permit recall: the acquire check (`in_flight < current`)
/// is the authority, so shrinking `current` simply makes the next acquires
/// wait until in-flight work drains naturally.
pub(crate) struct AdaptiveLimiter {
    inner: std::sync::Arc<AdaptiveLimiterInner>,
}

impl Clone for AdaptiveLimiter {
    fn clone(&self) -> Self {
        Self {
            inner: std::sync::Arc::clone(&self.inner),
        }
    }
}

struct AdaptiveLimiterInner {
    ceiling: usize,
    floor: usize,
    state: std::sync::Mutex<AdaptiveLimiterState>,
    /// Wakes waiters when a slot may have opened (growth or in-flight
    /// release). `notify_one` stores a permit when no waiter is registered,
    /// so a notify between a waiter's check and its await is never lost;
    /// spurious wakeups are absorbed by the acquire loop's re-check.
    slot_open: tokio::sync::Notify,
}

struct AdaptiveLimiterState {
    current: usize,
    in_flight: usize,
    /// While `Some`, no slot is granted until this instant passes — the
    /// limiter's model of an open circuit breaker's cooldown horizon.
    paused_until: Option<std::time::Instant>,
}

impl AdaptiveLimiter {
    /// `ceiling` is the never-exceeded bound (`HKASK_MAX_CONCURRENCY`);
    /// `floor` is the ramp's starting allowance. Both are normalized to ≥ 1
    /// with `floor ≤ ceiling` — a zero ceiling would otherwise deadlock every
    /// acquire (the `Semaphore::new(0)` trap this replaces).
    pub(crate) fn new(ceiling: usize, floor: usize) -> Self {
        let ceiling = ceiling.max(1);
        let floor = floor.clamp(1, ceiling);
        Self {
            inner: std::sync::Arc::new(AdaptiveLimiterInner {
                ceiling,
                floor,
                state: std::sync::Mutex::new(AdaptiveLimiterState {
                    current: floor,
                    in_flight: 0,
                    paused_until: None,
                }),
                slot_open: tokio::sync::Notify::new(),
            }),
        }
    }

    /// Current concurrency allowance — observability for logs and tests.
    pub(crate) fn current(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("adaptive limiter state mutex poisoned")
            .current
    }

    /// Acquire an execution slot, waiting while in-flight work is at the
    /// current allowance or a breaker pause is active. Cancellation-safe:
    /// `in_flight` is only incremented when a slot is granted, so a dropped
    /// acquire future (mid-pause-sleep or mid-park) leaks nothing.
    pub(crate) async fn acquire(&self) -> AdaptiveSlot {
        loop {
            let notified = self.inner.slot_open.notified();
            // Decide under the lock; the guard never crosses an await —
            // a `MutexGuard` is not `Send`, and every consumer spawns this
            // future onto tokio workers.
            let pause_sleep = {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .expect("adaptive limiter state mutex poisoned");
                let pause_sleep = if let Some(paused_until) = state.paused_until {
                    let now = std::time::Instant::now();
                    if now < paused_until {
                        Some(paused_until - now)
                    } else {
                        // Lazily clear an expired pause.
                        state.paused_until = None;
                        None
                    }
                } else {
                    None
                };
                if pause_sleep.is_none() && state.in_flight < state.current {
                    state.in_flight += 1;
                    return AdaptiveSlot {
                        limiter: self.clone(),
                    };
                }
                pause_sleep
            };
            if let Some(remaining) = pause_sleep {
                // Sleep out the pause, then re-check under the lock: the
                // pause may have been extended while we slept. Nothing is
                // reserved while waiting, so a dropped acquire leaks nothing.
                tokio::time::sleep(remaining).await;
                continue;
            }
            notified.await;
        }
    }

    fn report_success(&self) {
        let grew = {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("adaptive limiter state mutex poisoned");
            if state.current < self.inner.ceiling {
                state.current += 1;
                true
            } else {
                false
            }
        };
        if grew {
            tracing::debug!(
                target: "reg.batch.concurrency",
                current = self.current(),
                ceiling = self.inner.ceiling,
                "Adaptive limiter grew on success"
            );
            self.inner.slot_open.notify_one();
        }
    }

    fn report_failure(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("adaptive limiter state mutex poisoned");
        let backed_off = (state.current / 2).max(self.inner.floor);
        if backed_off != state.current {
            state.current = backed_off;
            tracing::warn!(
                target: "reg.batch.concurrency",
                current = backed_off,
                floor = self.inner.floor,
                "Adaptive limiter backed off on failure"
            );
        }
    }

    /// A circuit breaker opened on the remote inference path: snap the
    /// allowance to the floor and pause every acquisition until
    /// `retry_after` has passed. Queued work waits; nothing fires into the
    /// open breaker. A later `report_breaker_open` extends an active
    /// pause (max), never shortens it.
    pub(crate) fn report_breaker_open(&self, retry_after: std::time::Duration) {
        let paused_until = std::time::Instant::now() + retry_after;
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("adaptive limiter state mutex poisoned");
            state.current = self.inner.floor;
            state.paused_until = Some(match state.paused_until {
                Some(existing) => existing.max(paused_until),
                None => paused_until,
            });
        }
        tracing::warn!(
            target: "reg.batch.concurrency",
            current = self.inner.floor,
            retry_after_secs = retry_after.as_secs(),
            "Adaptive limiter paused on breaker-open — acquisitions wait out the cooldown"
        );
    }
}

/// One acquired execution slot. The gated call reports its outcome through
/// [`AdaptiveSlot::report_inference_outcome`] — the one classifier; `Drop`
/// releases the in-flight count and wakes a waiter.
pub(crate) struct AdaptiveSlot {
    limiter: AdaptiveLimiter,
}

impl AdaptiveSlot {
    /// The gated call succeeded — grow the allowance (additive, +1).
    fn report_success(&self) {
        self.limiter.report_success();
    }

    /// The gated call failed — back off (multiplicative, halve, floor-bounded).
    fn report_failure(&self) {
        self.limiter.report_failure();
    }

    /// THE outcome classifier for a gated remote-inference call. Every
    /// consumer reports through this one method — per-callsite match arms
    /// are how the 2026-10-02 spiral hid (CircuitOpen stringified into a
    /// generic failure arm: halve-only, never pause).
    ///
    /// - `Ok` → success: grow the allowance additively.
    /// - `Err` carrying a breaker-open verdict → pause: snap to the floor
    ///   and hold acquisitions for the breaker's retry-after horizon.
    /// - any other `Err` → failure: halve, floor-bounded.
    pub(crate) fn report_inference_outcome<T, E: BreakerFeedback>(&self, result: &Result<T, E>) {
        match result {
            Ok(_) => self.report_success(),
            Err(error) => match error.breaker_retry_after() {
                Some(retry_after) => self.limiter.report_breaker_open(retry_after),
                None => self.report_failure(),
            },
        }
    }
}

impl Drop for AdaptiveSlot {
    fn drop(&mut self) {
        let mut state = self
            .limiter
            .inner
            .state
            .lock()
            .expect("adaptive limiter state mutex poisoned");
        state.in_flight = state.in_flight.saturating_sub(1);
        drop(state);
        self.limiter.inner.slot_open.notify_one();
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use hkask_types::EmbeddingGenerationError;

    /// expect: Slice5 preserves both existing caller error types without treating credentials or invalid payloads as transient.
    #[tokio::test]
    async fn slice5_shared_retry_preserves_error_types() {
        let mut calls = 0;
        let inference: Result<RetryOutcome<()>, InferenceError> =
            retry_with_backoff(MAX_RETRIES, "test", "auth", || {
                calls += 1;
                std::future::ready(Err(InferenceError::Auth("same credential error".into())))
            })
            .await;
        assert_eq!(calls, 1);
        assert!(
            matches!(inference, Err(InferenceError::Auth(message)) if message == "same credential error")
        );

        let mut calls = 0;
        let embedding: Result<RetryOutcome<()>, EmbeddingGenerationError> =
            retry_with_backoff(MAX_RETRIES, "test", "embedding auth", || {
                calls += 1;
                std::future::ready(Err(EmbeddingGenerationError::Api(
                    401,
                    "same API error".into(),
                )))
            })
            .await;
        assert_eq!(calls, 1);
        assert!(
            matches!(embedding, Err(EmbeddingGenerationError::Api(401, message)) if message == "same API error")
        );

        for error in [
            InferenceError::Generation("not a transport classification".into()),
            InferenceError::Json("bad response".into()),
            InferenceError::CircuitOpen("quarantined".into()),
        ] {
            assert!(!error.is_transient());
        }
        for error in [
            EmbeddingGenerationError::InvalidRequest("bad request".into()),
            EmbeddingGenerationError::Api(400, "bad payload".into()),
            EmbeddingGenerationError::Api(403, "forbidden".into()),
            EmbeddingGenerationError::Json("bad response".into()),
            EmbeddingGenerationError::EmptyResponse,
            EmbeddingGenerationError::DimensionMismatch {
                expected: 4,
                actual: 2,
            },
        ] {
            assert!(!error.is_transient());
        }
        for error in [
            EmbeddingGenerationError::Connection("offline".into()),
            EmbeddingGenerationError::Api(408, "timeout".into()),
            EmbeddingGenerationError::Api(429, "busy".into()),
            EmbeddingGenerationError::Api(503, "unavailable".into()),
        ] {
            assert!(error.is_transient());
        }
    }

    /// expect: Slice5 keeps the existing three-attempt ceiling even if a caller requests more.
    #[tokio::test]
    async fn slice5_retry_ceiling_and_transient_type() {
        let mut calls = 0;
        let result: Result<RetryOutcome<()>, InferenceError> =
            retry_with_backoff(MAX_RETRIES + 1, "test", "bounded transient", || {
                calls += 1;
                std::future::ready(Err(InferenceError::Timeout("same timeout".into())))
            })
            .await;
        assert_eq!(calls, MAX_RETRIES);
        assert!(
            matches!(result, Err(InferenceError::Timeout(message)) if message == "same timeout")
        );
    }

    /// expect: the counted outcome reports retries consumed — 0 on a
    /// first-try success, N after N transient failures then success — so
    /// tool summaries can surface the retry signal.
    #[tokio::test]
    async fn slice5_counted_outcome_reports_retries() {
        let mut calls = 0;
        let first_try: RetryOutcome<()> =
            retry_with_backoff(MAX_RETRIES, "test", "first try", || {
                calls += 1;
                std::future::ready(Ok::<_, InferenceError>(()))
            })
            .await
            .expect("first try succeeds");
        assert_eq!((first_try.retries, calls), (0, 1));

        let mut calls = 0;
        let after_retries: RetryOutcome<()> =
            retry_with_backoff(MAX_RETRIES, "test", "retry then succeed", || {
                calls += 1;
                if calls < 3 {
                    std::future::ready(Err(InferenceError::Timeout("transient".into())))
                } else {
                    std::future::ready(Ok(()))
                }
            })
            .await
            .expect("succeeds after transient retries");
        assert_eq!((after_retries.retries, calls), (2, 3));
    }
}

#[cfg(test)]
mod adaptive_limiter_tests {
    use super::*;

    #[test]
    fn limiter_starts_at_floor_not_ceiling() {
        let limiter = AdaptiveLimiter::new(96, 2);
        assert_eq!(limiter.current(), 2, "the ramp must start at the floor");
    }

    #[test]
    fn zero_ceiling_is_normalized_not_a_deadlock() {
        let limiter = AdaptiveLimiter::new(0, 0);
        assert_eq!(limiter.current(), 1);
    }

    #[test]
    fn success_grows_additively_bounded_by_ceiling() {
        let limiter = AdaptiveLimiter::new(4, 2);
        limiter.report_success();
        assert_eq!(limiter.current(), 3);
        limiter.report_success();
        assert_eq!(limiter.current(), 4);
        limiter.report_success();
        assert_eq!(limiter.current(), 4, "growth must stop at the ceiling");
    }

    #[test]
    fn failure_halves_with_floor_bound() {
        let limiter = AdaptiveLimiter::new(96, 2);
        for _ in 0..30 {
            limiter.report_success();
        }
        assert_eq!(limiter.current(), 32);
        limiter.report_failure();
        assert_eq!(limiter.current(), 16);
        limiter.report_failure();
        assert_eq!(limiter.current(), 8);
        let low = AdaptiveLimiter::new(96, 2);
        low.report_failure();
        assert_eq!(low.current(), 2, "backoff must stop at the floor");
    }

    #[tokio::test]
    async fn acquire_blocks_at_current_and_unblocks_on_release() {
        let limiter = AdaptiveLimiter::new(4, 2);
        let first = limiter.acquire().await;
        let second = limiter.acquire().await;

        // current=2, both slots held: a third acquire must not be granted.
        let blocked =
            tokio::time::timeout(std::time::Duration::from_millis(50), limiter.acquire()).await;
        assert!(
            blocked.is_err(),
            "acquire must block at the current allowance"
        );

        // Releasing a slot must unblock the next acquire.
        drop(second);
        let third = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            limiter.clone().acquire().await
        })
        .await;
        assert!(
            third.is_ok(),
            "a released slot must unblock a waiting acquire"
        );
        drop(first);
    }

    #[tokio::test]
    async fn success_growth_unblocks_a_waiting_acquire() {
        let limiter = AdaptiveLimiter::new(4, 2);
        let first = limiter.acquire().await;
        let second = limiter.acquire().await;

        let waiter = tokio::spawn({
            let limiter = limiter.clone();
            async move { limiter.acquire().await }
        });

        // Give the waiter a chance to park, then grow the allowance.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        first.report_success();

        let granted = tokio::time::timeout(std::time::Duration::from_secs(1), waiter).await;
        assert!(
            granted.is_ok(),
            "growth must unblock a waiting acquire without any slot release"
        );
        drop(second);
    }

    /// A breaker-open verdict snaps the allowance to the floor AND pauses
    /// acquisitions — halving alone keeps firing doomed calls at the floor
    /// (the 2026-10-02 spiral: 94 consecutive breaker openings on one book).
    #[test]
    fn breaker_open_snaps_to_floor() {
        let limiter = AdaptiveLimiter::new(96, 2);
        for _ in 0..30 {
            limiter.report_success();
        }
        assert_eq!(limiter.current(), 32);
        limiter.report_breaker_open(std::time::Duration::from_millis(200));
        assert_eq!(limiter.current(), 2, "breaker-open must snap to the floor");
    }

    #[tokio::test]
    async fn pause_grants_nothing_until_expiry() {
        let limiter = AdaptiveLimiter::new(4, 2);
        limiter.report_breaker_open(std::time::Duration::from_millis(150));
        let blocked =
            tokio::time::timeout(std::time::Duration::from_millis(60), limiter.acquire()).await;
        assert!(blocked.is_err(), "a paused limiter must grant nothing");
        let granted =
            tokio::time::timeout(std::time::Duration::from_secs(2), limiter.acquire()).await;
        assert!(
            granted.is_ok(),
            "acquire must grant after the pause expires"
        );
    }

    #[tokio::test]
    async fn growth_resumes_from_floor_after_pause() {
        let limiter = AdaptiveLimiter::new(4, 2);
        for _ in 0..2 {
            limiter.report_success();
        }
        assert_eq!(limiter.current(), 4);
        limiter.report_breaker_open(std::time::Duration::from_millis(50));
        assert_eq!(limiter.current(), 2);
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let _slot = limiter.acquire().await;
        limiter.report_success();
        assert_eq!(
            limiter.current(),
            3,
            "growth resumes from the floor post-pause"
        );
    }

    #[tokio::test]
    async fn circuit_open_routes_to_pause_not_halve() {
        let limiter = AdaptiveLimiter::new(96, 2);
        for _ in 0..30 {
            limiter.report_success();
        }
        assert_eq!(limiter.current(), 32);
        let slot = limiter.acquire().await;
        let breaker_verdict: Result<(), InferenceError> = Err(InferenceError::CircuitOpen(
            "transient inference failure threshold reached; retry after 30s".into(),
        ));
        slot.report_inference_outcome(&breaker_verdict);
        assert_eq!(
            limiter.current(),
            2,
            "CircuitOpen must snap to the floor (pause), not halve"
        );
        // The pause is active: the next acquire grants nothing promptly.
        let blocked =
            tokio::time::timeout(std::time::Duration::from_millis(60), limiter.acquire()).await;
        assert!(blocked.is_err(), "CircuitOpen must pause acquisitions");
    }

    #[tokio::test]
    async fn plain_failure_halves_without_pausing() {
        let limiter = AdaptiveLimiter::new(96, 2);
        for _ in 0..6 {
            limiter.report_success();
        }
        assert_eq!(limiter.current(), 8);
        let slot = limiter.acquire().await;
        let capacity_failure: Result<(), InferenceError> =
            Err(InferenceError::Overloaded("endpoint busy".into()));
        slot.report_inference_outcome(&capacity_failure);
        assert_eq!(limiter.current(), 4, "a non-breaker failure halves");
        let granted =
            tokio::time::timeout(std::time::Duration::from_millis(100), limiter.acquire()).await;
        assert!(
            granted.is_ok(),
            "a plain failure must not pause acquisitions"
        );
    }

    /// The parser is pinned to the shared bridge's exact `CircuitOpen`
    /// message format (kask_bridge/src/inference_chat.rs `admit`):
    /// `"...; retry after {retry_after:?}"`. If the bridge drifts, parsing
    /// returns `None` and the classifier falls back to the 30s default —
    /// safe, but this test is the tripwire that says the pin broke.
    #[test]
    fn retry_after_parses_the_bridge_message_format() {
        let bridge_message = |retry_after: std::time::Duration| {
            format!("transient inference failure threshold reached; retry after {retry_after:?}")
        };
        assert_eq!(
            parse_retry_after(&bridge_message(std::time::Duration::from_secs(30))),
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(
            parse_retry_after(&bridge_message(std::time::Duration::from_secs(90))),
            Some(std::time::Duration::from_secs(90))
        );
        assert_eq!(
            parse_retry_after(&bridge_message(std::time::Duration::from_millis(500))),
            Some(std::time::Duration::from_millis(500))
        );
        assert_eq!(
            parse_retry_after(&bridge_message(std::time::Duration::from_secs(3600))),
            Some(std::time::Duration::from_secs(3600))
        );
        assert_eq!(parse_retry_after("no horizon in here"), None);
        // The classifier adds the pause margin to a parsed horizon...
        let verdict =
            InferenceError::CircuitOpen(bridge_message(std::time::Duration::from_secs(30)));
        assert_eq!(
            verdict.breaker_retry_after(),
            Some(std::time::Duration::from_secs(32))
        );
        // ...and falls back to the default when the horizon is unparseable.
        let verdict = InferenceError::CircuitOpen("circuit open (no horizon)".into());
        assert_eq!(
            verdict.breaker_retry_after(),
            Some(DEFAULT_BREAKER_RETRY_AFTER + BREAKER_PAUSE_MARGIN)
        );
    }
}
