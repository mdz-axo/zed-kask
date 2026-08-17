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

    #[tokio::test]
    async fn on_throttle_removes_permits_from_semaphore() {
        // Regression guard: `on_throttle` must call `forget_permits` so the
        // semaphore's available count actually shrinks. Without it, backoff
        // is a no-op — `current` decrements but the semaphore still admits
        // the old count, and the next `on_success` overshoots `max`.
        let l = limiter(96, 4);
        // Ramp to 12.
        l.on_success();
        l.on_success();
        assert_eq!(l.current(), 12);
        assert_eq!(l.semaphore.available_permits(), 12);
        // Throttle: 12 → 8.
        l.on_throttle();
        assert_eq!(l.current(), 8);
        // The semaphore must reflect the shrink — this is the bug fix.
        assert_eq!(
            l.semaphore.available_permits(),
            8,
            "on_throttle must call forget_permits; without it the semaphore \
             stays at 12 and backoff is a no-op"
        );
    }

    #[tokio::test]
    async fn on_throttle_does_not_overshoot_max_after_recovery() {
        // Regression guard for the overshoot bug: after a throttle, a
        // subsequent `on_success` must not add permits on top of the
        // un-shrunk semaphore past `max`. The throttle removes permits from
        // the available pool; in-flight permits are unaffected (they return
        // on drop and are NOT re-added beyond the forgotten count).
        //
        // Scenario: ramp to max, acquire some, throttle, drop, success.
        // The key invariant: `available_permits` never exceeds `current`.
        let l = limiter(10, 4);
        // Ramp to 10 (max).
        l.on_success(); // 4 → 8
        l.on_success(); // 8 → 10
        assert_eq!(l.current(), 10);
        assert_eq!(l.semaphore.available_permits(), 10);
        // Acquire 4 (6 remain available).
        let p1 = l.acquire().await;
        let p2 = l.acquire().await;
        let p3 = l.acquire().await;
        let p4 = l.acquire().await;
        assert_eq!(l.semaphore.available_permits(), 6);
        // Throttle: 10 → 6. forget_permits(4) removes 4 from available (6 → 2).
        l.on_throttle();
        assert_eq!(l.current(), 6);
        assert_eq!(
            l.semaphore.available_permits(),
            2,
            "forget_permits must remove from available: 6 available - 4 forgotten = 2"
        );
        // Drop the 4 in-flight permits — they return to the semaphore.
        drop(p1);
        drop(p2);
        drop(p3);
        drop(p4);
        // The semaphore now has 6 permits (2 remaining + 4 returned). Not 10.
        assert_eq!(
            l.semaphore.available_permits(),
            6,
            "after throttle + permit drop, available must be 6 (the new pool \
             size), not 10 — otherwise backoff never takes effect"
        );
        // on_success ramps 6 → 10, adds 4 permits → 10. Does not overshoot.
        l.on_success();
        assert_eq!(l.current(), 10);
        assert_eq!(l.semaphore.available_permits(), 10);
    }

    // ── Bug-hunt probes ──────────────────────────────────────────────

    /// Probe: invariant `available_permits + in_flight == current` must hold
    /// after `on_success` while a permit is held. If `on_success` adds permits
    /// to the semaphore while the current call's permit is still held, the
    /// semaphore's total permit count temporarily exceeds `current` by the
    /// number of in-flight permits. This is correct (the in-flight permits
    /// will return on drop), but `available_permits` can momentarily exceed
    /// `current - in_flight`, allowing a brief burst of concurrency above the
    /// intended ceiling.
    #[tokio::test]
    async fn probe_on_success_while_permit_held_invariant() {
        let l = limiter(10, 2);
        // current=2, available=2, in_flight=0
        assert_eq!(l.current(), 2);
        assert_eq!(l.semaphore.available_permits(), 2);
        // Acquire 2 (in_flight=2, available=0)
        let p1 = l.acquire().await;
        let p2 = l.acquire().await;
        assert_eq!(l.semaphore.available_permits(), 0);
        // on_success while holding: current 2→4, add_permits(2) → available=2
        l.on_success();
        assert_eq!(l.current(), 4);
        assert_eq!(l.semaphore.available_permits(), 2);
        // invariant: available + in_flight = current → 2 + 2 = 4. OK.
        drop(p1);
        assert_eq!(l.semaphore.available_permits(), 3);
        // on_success again: current 4→6, add_permits(2) → available=5
        l.on_success();
        assert_eq!(l.current(), 6);
        assert_eq!(l.semaphore.available_permits(), 5);
        // invariant: 5 + 1 = 6 = current. OK.
        drop(p2);
        assert_eq!(l.semaphore.available_permits(), 6);
    }

    /// Probe: concurrent `on_success` + `on_throttle` race. Two threads call
    /// `on_success` and `on_throttle` simultaneously. The `Mutex` ensures
    /// `current` and the semaphore are updated atomically, so after the race
    /// settles, `available_permits == current` (when no permits are in-flight).
    #[tokio::test]
    async fn probe_concurrent_on_success_and_on_throttle_race() {
        let l = Arc::new(limiter(100, 10));
        // Ramp to 50 first (4 on_success from 10: 10→20→30→40→50).
        for _ in 0..4 {
            l.on_success();
        }
        assert_eq!(l.current(), 50);
        assert_eq!(l.semaphore.available_permits(), 50);
        // Now race: 10 on_success + 10 on_throttle concurrently.
        // Net effect should be 0 (10 up, 10 down), so current stays at 50.
        // The invariant: available_permits == current (no permits in-flight).
        let l1 = Arc::clone(&l);
        let l2 = Arc::clone(&l);
        let h1 = tokio::spawn(async move {
            for _ in 0..10 {
                l1.on_success();
            }
        });
        let h2 = tokio::spawn(async move {
            for _ in 0..10 {
                l2.on_throttle();
            }
        });
        h1.await.unwrap();
        h2.await.unwrap();
        // After the race, `current` should be 50 (10 up + 10 down, net 0).
        // But the order is nondeterministic. The invariant we check:
        // `available_permits == current` (no in-flight permits).
        assert_eq!(
            l.semaphore.available_permits(),
            l.current() as usize,
            "after concurrent on_success + on_throttle with no in-flight \
             permits, available_permits must equal current"
        );
    }

    /// Probe: `on_throttle` when `current > available` (permits in-flight).
    /// `forget_permits` saturates at available. After in-flight permits
    /// drop, does `available` exceed `current`?
    #[tokio::test]
    async fn probe_on_throttle_with_inflight_permits() {
        let l = limiter(20, 5);
        // Ramp to 20.
        for _ in 0..3 {
            l.on_success();
        }
        assert_eq!(l.current(), 20);
        // Acquire 15 (5 available, 15 in-flight).
        let mut permits = Vec::new();
        for _ in 0..15 {
            permits.push(l.acquire().await);
        }
        assert_eq!(l.semaphore.available_permits(), 5);
        // Throttle: current 20→15. forget_permits(5) removes 5 from available (5→0).
        l.on_throttle();
        assert_eq!(l.current(), 15);
        assert_eq!(l.semaphore.available_permits(), 0);
        // Drop all 15 in-flight permits. They return to the semaphore.
        // available should be 15 (the new pool size), not 20.
        drop(permits);
        assert_eq!(
            l.semaphore.available_permits(),
            15,
            "after throttle + in-flight drop, available must be 15 (the new \
             pool size), not 20 — the forgotten permits don't come back"
        );
        assert_eq!(l.current(), 15);
    }
}
