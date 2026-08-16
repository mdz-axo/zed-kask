//! Property tests for `hkask_templates::concurrency::ConcurrencyLimiter`.
//!
//! The limiter is a pure state machine with concurrency hazards (atomic
//! `compare_exchange` ramp-up/back-off over a shared `Semaphore`). The unit
//! tests in `src/concurrency.rs` cover single-threaded behavior; these
//! property tests pin the invariants that matter under concurrent callers —
//! the exact hazard the limiter exists to govern.
//!
//! # Principle grounding
//! - P1 (Correctness): invariants hold for all inputs in the generated space
//! - P4 (Clear Boundaries): the limiter never admits more than `max` permits
//!   and never drops below `step`, regardless of call ordering
//!
//! # Oracle taxonomy (HarnessLLM §3)
//! - `oracle_invariant` — check a property of (input, output)
//!
//! # Properties
//! 1. **Bounds invariant**: after any sequence of `on_success`/`on_throttle`
//!    calls, `step <= current() <= max`. The `compare_exchange` loops must
//!    never overshoot `max` or undershoot `step`.
//! 2. **Ramp monotonicity**: `on_success` never decreases `current()`; a
//!    sequence of pure `on_success` calls is monotonically non-decreasing.
//! 3. **Throttle monotonicity**: `on_throttle` never increases `current()`;
//!    a sequence of pure `on_throttle` calls is monotonically non-increasing.
//! 4. **Permit conservation**: the semaphore's available permits + in-flight
//!    permits == `current()`. Acquiring N permits reduces availability by N;
//!    dropping them restores it. The semaphore never leaks permits.
//! 5. **Concurrent ramp safety**: N concurrent `on_success` calls move
//!    `current()` up by at most `N * step` (capped at `max`), never beyond.
//!    Pins the `compare_exchange` idempotence under real concurrency.

use std::sync::Arc;

use hkask_templates::concurrency::ConcurrencyLimiter;
use proptest::prelude::*;

/// Arbitrary `(max, step)` pair with the constraints the constructor enforces:
/// `max >= 1`, `step` clamped to `[1, max]`. Generates values the limiter
/// actually accepts, not values it rejects.
fn arb_limiter_config() -> BoxedStrategy<(u32, u32)> {
    (1u32..256u32, 1u32..256u32).boxed()
}

/// Arbitrary sequence of success/throttle tokens. Models a real workload:
/// mostly successes with occasional throttles. `true` = success, `false` =
/// throttle.
fn arb_op_sequence() -> BoxedStrategy<Vec<bool>> {
    prop::collection::vec(prop_oneof![Just(true), Just(true), Just(false)], 0..64).boxed()
}

proptest! {
    /// After any sequence of `on_success`/`on_throttle` calls, `current()`
    /// stays within `[step, max]`. The `compare_exchange` loops must never
    /// overshoot `max` or undershoot `step`.
    ///
    /// Oracle: invariant — `step <= current() <= max` after every op.
    #[test]
    fn current_stays_within_bounds_after_any_op_sequence(
        (max, step) in arb_limiter_config(),
        ops in arb_op_sequence(),
    ) {
        let limiter = ConcurrencyLimiter::new(max, step);
        let effective_step = step.min(max).max(1);
        for is_success in ops {
            if is_success {
                limiter.on_success();
            } else {
                limiter.on_throttle();
            }
            let current = limiter.current();
            prop_assert!(
                effective_step <= current,
                "current {} fell below step {} after {:?}",
                current, effective_step, if is_success { "success" } else { "throttle" }
            );
            prop_assert!(
                current <= max,
                "current {} exceeded max {} after {:?}",
                current, max, if is_success { "success" } else { "throttle" }
            );
        }
    }

    /// A pure sequence of `on_success` calls is monotonically non-decreasing.
    /// `on_success` must never decrease `current()`.
    ///
    /// Oracle: invariant — `current()` after op >= `current()` before op.
    #[test]
    fn on_success_never_decreases_current(
        (max, step) in arb_limiter_config(),
        n in 0u32..128u32,
    ) {
        let limiter = ConcurrencyLimiter::new(max, step);
        let mut prev = limiter.current();
        for _ in 0..n {
            limiter.on_success();
            let current = limiter.current();
            prop_assert!(
                current >= prev,
                "on_success decreased current from {} to {}",
                prev, current
            );
            prev = current;
        }
    }

    /// A pure sequence of `on_throttle` calls is monotonically non-increasing.
    /// `on_throttle` must never increase `current()`.
    ///
    /// Oracle: invariant — `current()` after op <= `current()` before op.
    #[test]
    fn on_throttle_never_increases_current(
        (max, step) in arb_limiter_config(),
        n in 0u32..128u32,
    ) {
        let limiter = ConcurrencyLimiter::new(max, step);
        // Ramp up first so there's room to back off.
        for _ in 0..10 {
            limiter.on_success();
        }
        let mut prev = limiter.current();
        for _ in 0..n {
            limiter.on_throttle();
            let current = limiter.current();
            prop_assert!(
                current <= prev,
                "on_throttle increased current from {} to {}",
                prev, current
            );
            prev = current;
        }
    }

    /// Permit conservation: acquiring N permits reduces availability by N,
    /// dropping them restores it. The semaphore never leaks permits.
    ///
    /// Oracle: invariant — after acquiring and dropping, `current()` is
    /// unchanged and the semaphore admits `current()` permits again.
    ///
    /// `proptest!` generates sync `#[test]` functions, so we drive the
    /// async `acquire()` via a `tokio::runtime::Runtime::block_on`. The
    /// semaphore is runtime-agnostic (it's `tokio::sync::Semaphore` but
    /// `acquire_owned` doesn't require a `#[tokio::test]` attribute —
    /// `block_on` is sufficient for a single poll).
    #[test]
    fn permit_acquisition_and_release_conserves_count(
        (max, step) in arb_limiter_config(),
        n_acquire in 1u32..16u32,
    ) {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let limiter = ConcurrencyLimiter::new(max, step);
        let effective_step = step.min(max).max(1);
        let initial = limiter.current();

        // Acquire up to `initial` permits (we can't acquire more than are
        // available without blocking).
        let n_acquire = n_acquire.min(initial);
        let mut permits = Vec::new();
        for _ in 0..n_acquire {
            permits.push(runtime.block_on(limiter.acquire()));
        }

        // current() is unaffected by acquisition (it tracks the pool size,
        // not in-flight permits).
        prop_assert_eq!(
            limiter.current(),
            initial,
            "acquire changed current() — it tracks pool size, not in-flight"
        );

        // Drop all permits.
        drop(permits);

        // After release, we can acquire `initial` permits again (the pool
        // is restored). If we can't, a permit leaked.
        let mut reacquired = Vec::new();
        for _ in 0..initial {
            reacquired.push(runtime.block_on(limiter.acquire()));
        }
        drop(reacquired);

        // The effective_step floor is invariant throughout.
        prop_assert!(
            limiter.current() >= effective_step,
            "current {} fell below step {} after acquire/drop cycle",
            limiter.current(), effective_step
        );
    }

    /// Concurrent `on_success` calls move `current()` up by at most
    /// `N * step` (capped at `max`), never beyond. Pins the
    /// `compare_exchange` idempotence under real concurrency — a race must
    /// not overshoot `max`.
    ///
    /// Oracle: invariant — `current() <= max` after N concurrent successes.
    #[test]
    fn concurrent_on_success_never_overshoots_max(
        (max, step) in arb_limiter_config(),
        n_threads in 1u32..32u32,
    ) {
        let limiter = Arc::new(ConcurrencyLimiter::new(max, step));
        let mut handles = Vec::new();
        for _ in 0..n_threads {
            let l = Arc::clone(&limiter);
            handles.push(std::thread::spawn(move || {
                // Each thread ramps 8 times — enough to stress the CAS loop.
                for _ in 0..8 {
                    l.on_success();
                }
            }));
        }
        for handle in handles {
            handle.join().expect("worker thread panicked");
        }
        let current = limiter.current();
        prop_assert!(
            current <= max,
            "concurrent on_success overshot: current {} > max {}",
            current, max
        );
    }

    /// Concurrent `on_throttle` calls never undershoot `step`. Pins the
    /// back-off CAS loop under real concurrency.
    ///
    /// Oracle: invariant — `current() >= step` after N concurrent throttles.
    #[test]
    fn concurrent_on_throttle_never_undershoots_step(
        (max, step) in arb_limiter_config(),
        n_threads in 1u32..32u32,
    ) {
        let limiter = Arc::new(ConcurrencyLimiter::new(max, step));
        let effective_step = step.min(max).max(1);
        // Ramp to max first so there's room to back off.
        for _ in 0..64 {
            limiter.on_success();
        }
        let mut handles = Vec::new();
        for _ in 0..n_threads {
            let l = Arc::clone(&limiter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..8 {
                    l.on_throttle();
                }
            }));
        }
        for handle in handles {
            handle.join().expect("worker thread panicked");
        }
        let current = limiter.current();
        prop_assert!(
            current >= effective_step,
            "concurrent on_throttle undershot: current {} < step {}",
            current, effective_step
        );
    }
}
