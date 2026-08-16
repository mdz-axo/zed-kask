//! Global inference concurrency limiter — process-wide, shared across all
//! consumers (skill cascades, corpus OCR, MCP tool calls, any future caller).
//!
//! Stepped ramp-up: starts at `concurrency_step` permits, adds `concurrency_step`
//! per ramp tick on success, until `max_concurrency` or a throttle. Backs off
//! one `step` on 429/503 only (not on deterministic errors — a 400/404 must not
//! shrink the pool for unrelated callers).
//!
//! The limiter is an `Arc` held in a process-global `OnceLock` (see
//! `set_global_concurrency_limiter` / `global_concurrency_limiter`). Consumers
//! acquire a permit before issuing a cloud inference or tool call, then call
//! `on_success` or `on_throttle` after the call completes. The permit is
//! released when the returned guard drops.
//!
//! Why stepped, not AIMD or linear-by-1: the operator configures both the
//! starting point and the increment via `concurrency_step`. With the default
//! `concurrency_step: 4`, the ramp is 4 → 8 → 12 → … → 96 (24 ramp ticks).
//! Each tick fires on a successful call. The ramp reaches `max_concurrency`
//! quickly under steady success and backs off one step on throttle.

use std::sync::{Arc, OnceLock};

use tokio::sync::Semaphore;

pub struct ConcurrencyLimiter {
    semaphore: Arc<Semaphore>,
    max: u32,
    step: u32,
    current: std::sync::atomic::AtomicU32,
}

/// A permit acquired from the limiter. Releases on drop.
pub type ConcurrencyPermit = tokio::sync::OwnedSemaphorePermit;

impl ConcurrencyLimiter {
    /// Construct a new limiter. `concurrency_step` is clamped to
    /// `[1, max_concurrency]` so the starting pool is always ≥ 1 and never
    /// exceeds the ceiling. The semaphore starts with `concurrency_step`
    /// permits; `on_success` adds `step` per tick up to `max`.
    pub fn new(max_concurrency: u32, concurrency_step: u32) -> Self {
        let max = max_concurrency.max(1);
        let step = concurrency_step.clamp(1, max);
        Self {
            semaphore: Arc::new(Semaphore::new(step as usize)),
            max,
            step,
            current: std::sync::atomic::AtomicU32::new(step),
        }
    }

    /// Acquire a permit, blocking until one is available. The returned guard
    /// releases the permit on drop.
    ///
    /// `Semaphore::acquire_owned` returns `Err(AcquireError)` only if the
    /// semaphore is closed, which never happens here (no `close()` call on
    /// the limiter's semaphore). A closed semaphore is a programmer error,
    /// not a runtime condition — panicking surfaces it loudly rather than
    /// silently dropping the call.
    pub async fn acquire(&self) -> ConcurrencyPermit {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("concurrency limiter semaphore closed — should never happen")
    }

    /// Called after a successful call. Ramps up by `step` (capped at `max`)
    /// if below the ceiling. Idempotent under concurrent success — uses
    /// `compare_exchange` so concurrent `on_success` calls never overshoot.
    pub fn on_success(&self) {
        use std::sync::atomic::Ordering;
        let mut current = self.current.load(Ordering::Acquire);
        loop {
            if current >= self.max {
                return;
            }
            let next = (current + self.step).min(self.max);
            match self.current.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let added = next - current;
                    self.semaphore.add_permits(added as usize);
                    return;
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Called after a throttle (429/503 only). Backs off one `step` (down
    /// to `step`, never below). The permit is released by the guard's Drop;
    /// this only shrinks the pool for future calls. Idempotent under
    /// concurrent throttle — `compare_exchange` prevents undershoot.
    pub fn on_throttle(&self) {
        use std::sync::atomic::Ordering;
        let floor = self.step;
        let mut current = self.current.load(Ordering::Acquire);
        loop {
            if current <= floor {
                return;
            }
            let next = (current - self.step).max(floor);
            match self.current.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    /// Current permit count (for observability and tests).
    pub fn current(&self) -> u32 {
        self.current.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The configured maximum.
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Access the underlying semaphore `Arc` for consumers that need to
    /// `acquire()` directly (e.g. the OCR pipeline's `JoinSet` spawn pattern,
    /// which holds the permit across a `tokio::spawn` boundary). Prefer
    /// `acquire()` + `on_success` / `on_throttle` for the standard pattern;
    /// this is for consumers that integrate with their own concurrency
    /// primitives and can't hold a `ConcurrencyPermit` across their spawn.
    pub fn semaphore_ref(&self) -> &Arc<Semaphore> {
        &self.semaphore
    }
}

static GLOBAL_LIMITER: OnceLock<Arc<ConcurrencyLimiter>> = OnceLock::new();

/// Wire the global concurrency limiter. Called once at startup (after
/// settings load). A second call logs a warning and drops the new limiter —
/// the previously-wired limiter remains active. Runtime changes to the
/// settings do not take effect until restart.
pub fn set_global_concurrency_limiter(max_concurrency: u32, concurrency_step: u32) {
    let limiter = Arc::new(ConcurrencyLimiter::new(max_concurrency, concurrency_step));
    if GLOBAL_LIMITER.set(limiter).is_err() {
        tracing::warn!(
            target: "hkask.concurrency",
            "set_global_concurrency_limiter: hook already set — second wiring attempt \
             dropped. The previously-wired limiter remains active. Restart the app \
             to re-wire from a clean process."
        );
    }
}

/// Access the process-global concurrency limiter. Returns `None` before
/// `set_global_concurrency_limiter` has run (tests, pre-startup). Callers
/// must skip gating when `None`.
pub fn global_concurrency_limiter() -> Option<&'static Arc<ConcurrencyLimiter>> {
    GLOBAL_LIMITER.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter(max: u32, step: u32) -> ConcurrencyLimiter {
        ConcurrencyLimiter::new(max, step)
    }

    #[test]
    fn starts_at_step_not_max() {
        let l = limiter(96, 4);
        assert_eq!(l.current(), 4);
        assert_eq!(l.max(), 96);
    }

    #[test]
    fn on_success_ramps_by_step() {
        let l = limiter(96, 4);
        assert_eq!(l.current(), 4);
        l.on_success();
        assert_eq!(l.current(), 8);
        l.on_success();
        assert_eq!(l.current(), 12);
    }

    #[test]
    fn on_success_caps_at_max() {
        let l = limiter(10, 4);
        l.on_success(); // 4 → 8
        l.on_success(); // 8 → 10 (capped)
        assert_eq!(l.current(), 10);
        l.on_success(); // already at max, no-op
        assert_eq!(l.current(), 10);
    }

    #[test]
    fn on_throttle_backs_off_one_step() {
        let l = limiter(96, 4);
        // Ramp up to 12 first.
        l.on_success();
        l.on_success();
        assert_eq!(l.current(), 12);
        l.on_throttle();
        assert_eq!(l.current(), 8);
    }

    #[test]
    fn on_throttle_floors_at_step() {
        let l = limiter(96, 4);
        assert_eq!(l.current(), 4);
        l.on_throttle(); // already at floor, no-op
        assert_eq!(l.current(), 4);
    }

    #[test]
    fn step_clamped_to_max_when_step_exceeds_max() {
        let l = limiter(3, 10);
        assert_eq!(l.current(), 3); // step clamped to max
        assert_eq!(l.max(), 3);
    }

    #[test]
    fn step_clamped_to_one_when_zero() {
        let l = limiter(96, 0);
        assert_eq!(l.current(), 1); // step clamped to 1
    }

    #[test]
    fn max_floored_at_one() {
        let l = limiter(0, 0);
        assert_eq!(l.max(), 1);
        assert_eq!(l.current(), 1);
    }

    #[tokio::test]
    async fn acquire_releases_on_drop() {
        let l = limiter(2, 2);
        let p1 = l.acquire().await;
        let p2 = l.acquire().await;
        // Pool exhausted — third acquire would block. Drop one permit.
        drop(p1);
        // Now one permit available again.
        let p3 = l.acquire().await;
        drop(p2);
        drop(p3);
    }

    #[tokio::test]
    async fn on_success_adds_permits_so_blocked_acquire_proceeds() {
        let l = limiter(96, 4);
        let _p1 = l.acquire().await;
        let _p2 = l.acquire().await;
        let _p3 = l.acquire().await;
        let _p4 = l.acquire().await;
        // Pool exhausted at 4. Ramp up adds 4 permits.
        l.on_success();
        // Now 8 permits total, 4 in use → 4 available.
        let _p5 = l.acquire().await;
    }
}
