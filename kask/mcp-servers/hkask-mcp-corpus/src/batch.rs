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
/// Backoff: `2^attempts * 5` seconds (10s, 20s, 40s for attempts 1, 2, 3).
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
                let backoff = std::time::Duration::from_secs(2u64.pow(attempts) * 5);
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
