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

use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::Semaphore;

pub struct ConcurrencyLimiter {
    semaphore: Arc<Semaphore>,
    max: u32,
    step: u32,
    /// The logical pool size. Held in a `Mutex` (not an `AtomicU32`) because
    /// `on_success`/`on_throttle` must update `current` AND modify the
    /// semaphore (`add_permits`/`forget_permits`) atomically. With an atomic,
    /// the CAS on `current` and the semaphore modification are separate steps,
    /// and a concurrent call can interleave between them — causing the
    /// semaphore to drift above `current` (see bug-hunt BUG-1). The lock is
    /// held only in sync methods, never across an `await`, so `std::sync::Mutex`
    /// is correct. Contention is negligible: `on_success`/`on_throttle` fire
    /// once per inference call (which takes seconds), not per token.
    current: Mutex<u32>,
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
            current: Mutex::new(step),
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
    /// if below the ceiling. The lock ensures `current` and the semaphore
    /// are updated atomically — no concurrent `on_success` or `on_throttle`
    /// can interleave between the `current` update and `add_permits`.
    pub fn on_success(&self) {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if *current >= self.max {
            return;
        }
        let next = (*current + self.step).min(self.max);
        let added = next - *current;
        *current = next;
        self.semaphore.add_permits(added as usize);
    }

    /// Called after a throttle (429/503 only). Backs off one `step` (down
    /// to `step`, never below). The permit is released by the guard's Drop;
    /// this shrinks the pool for future calls by removing `step` permits
    /// from the semaphore via `forget_permits`. The lock ensures `current`
    /// and the semaphore are updated atomically — no concurrent `on_success`
    /// can interleave between the `current` update and `forget_permits`,
    /// which would cause the semaphore to drift above `current`.
    ///
    /// `forget_permits` saturates at the available count so it never goes
    /// negative (in-flight permits are unaffected and return on drop).
    pub fn on_throttle(&self) {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let floor = self.step;
        if *current <= floor {
            return;
        }
        let next = (*current - self.step).max(floor);
        let removed = *current - next;
        *current = next;
        // Remove permits from the semaphore so backoff actually reduces
        // concurrency. `forget_permits` saturates at the available count —
        // if fewer than `removed` permits are available (some are in-flight),
        // it removes what it can; the in-flight ones return on drop and
        // don't get re-added.
        let _actually_removed = self.semaphore.forget_permits(removed as usize);
    }

    /// Current permit count (for observability and tests).
    pub fn current(&self) -> u32 {
        *self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The configured maximum.
    pub fn max(&self) -> u32 {
        self.max
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
