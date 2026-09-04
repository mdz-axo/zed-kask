//! Shared batch-processing infrastructure for concurrent LLM operations.
//!
//! Eliminates the duplicated semaphore-gated-concurrency + retry-with-backoff +
//! degraded-outcome-classification skeleton that was hand-rolled across
//! `corpus_generate_qa_batch`, `corpus_extract_assertions`, `embed_batch_from_jsonl`,
//! and `corpus_tag_chunks`.

use std::future::Future;

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

/// Retry an async operation with exponential backoff.
///
/// Backoff: `2^attempts` seconds (2s, 4s for attempts 1, 2). The previous
/// `2^attempts * 5` schedule (10s, 20s) caused multi-hour wall times for
/// large corpus embedding runs where each batch retried independently —
/// 170 batches × 30s of sleep = 85 minutes of pure backoff. The 2s/4s
/// schedule is sufficient for transient throttles without making large
/// runs impractical.
///
/// Returns `Ok(result)` on success, `Err(last_error)` after `max_retries`
/// failures. Each retry logs a warning with the attempt number and backoff.
pub(crate) async fn retry_with_backoff<T, E, F, Fut>(
    max_retries: u32,
    target: &str,
    context: &str,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempts = 0u32;
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempts += 1;
                if attempts >= max_retries {
                    tracing::warn!(
                        target = target,
                        context = %context,
                        attempts = attempts,
                        error = %e,
                        "Operation failed after {max_retries} retries",
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

/// AIMD adaptive concurrency limiter for remote LLM work.
///
/// Starts at `floor`, grows additively (+1 per success) toward `ceiling`, and
/// backs off multiplicatively (halve per failure, floor-bounded). A service
/// with lower capacity than the ceiling is discovered by probing, not by
/// stampede. Local work (Tesseract, file IO) is NOT gated here — a static
/// bound is correct for a local resource; adaptation is for remote services.
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
    /// current allowance. Cancellation-safe: `in_flight` is only incremented
    /// when a slot is granted, so a dropped acquire future leaks nothing.
    pub(crate) async fn acquire(&self) -> AdaptiveSlot {
        loop {
            let notified = self.inner.slot_open.notified();
            {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .expect("adaptive limiter state mutex poisoned");
                if state.in_flight < state.current {
                    state.in_flight += 1;
                    return AdaptiveSlot {
                        limiter: self.clone(),
                    };
                }
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
}

/// One acquired execution slot. The gated call reports its outcome
/// (`report_success` / `report_failure`); `Drop` releases the in-flight
/// count and wakes a waiter.
pub(crate) struct AdaptiveSlot {
    limiter: AdaptiveLimiter,
}

impl AdaptiveSlot {
    /// The gated call succeeded — grow the allowance (additive, +1).
    pub(crate) fn report_success(&self) {
        self.limiter.report_success();
    }

    /// The gated call failed — back off (multiplicative, halve, floor-bounded).
    pub(crate) fn report_failure(&self) {
        self.limiter.report_failure();
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
}
