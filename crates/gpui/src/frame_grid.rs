//! zed-kask: shared redraw-grid facility (D84/D14).
//!
//! Every periodic redraw driver in the process — looping animation timers,
//! streaming-text reveal timers — aligns to one shared interval grid so that
//! N independent actors coalesce their foreground wakes into one redraw per
//! boundary instead of N unsynchronized triggers per interval. Without the
//! shared grid, per-instance timers each fire from their own start time: the
//! streaming-reveal timers of N concurrent agent threads drove up to N×20
//! full-window redraws per second (measured 2026-09-29: main thread 60–92%
//! under 3–4 concurrent turns), and capped animation timers showed the same
//! residue (31 draws/s from sidebar animators alone, 2026-09-30).

use std::sync::OnceLock;
use std::time::{Duration, Instant};

static GRID_EPOCH: OnceLock<Instant> = OnceLock::new();

/// The delay from `now` until the next grid boundary strictly after `now`.
/// Pure over `(now, epoch, interval)` so the alignment math is unit-testable.
pub fn grid_delay(now: Instant, epoch: Instant, interval: Duration) -> Duration {
    let interval_ms = interval.as_millis().max(1);
    let elapsed_ms = now.saturating_duration_since(epoch).as_millis();
    let next_boundary_ms = (elapsed_ms / interval_ms + 1) * interval_ms;
    let next_boundary = u64::try_from(next_boundary_ms).unwrap_or(u64::MAX);
    match epoch.checked_add(Duration::from_millis(next_boundary)) {
        Some(target) => target.saturating_duration_since(now),
        None => interval,
    }
}

/// The delay from `now` until the next boundary of the shared grid for
/// `interval`. Callers with a controllable clock (tests, executors) pass their
/// own `now`; production callers typically pass `background_executor().now()`.
pub fn next_grid_delay_from(now: Instant, interval: Duration) -> Duration {
    let epoch = GRID_EPOCH.get_or_init(Instant::now);
    grid_delay(now, *epoch, interval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// zed-kask pin: the grid delay lands every caller on the same interval
    /// grid strictly after `now` — concurrent actors then coalesce into one
    /// redraw per boundary instead of N independent triggers.
    #[test]
    fn grid_delay_lands_on_shared_boundaries() {
        let epoch = Instant::now();
        let interval = Duration::from_millis(50);

        // Mid-interval: the next boundary is the remainder away.
        let now = epoch + Duration::from_millis(17);
        assert_eq!(grid_delay(now, epoch, interval), Duration::from_millis(33));

        // Exactly on a boundary: strictly after, so a full interval.
        let now = epoch + Duration::from_millis(50);
        assert_eq!(grid_delay(now, epoch, interval), Duration::from_millis(50));

        // Just past a boundary: nearly a full interval.
        let now = epoch + Duration::from_millis(51);
        assert_eq!(grid_delay(now, epoch, interval), Duration::from_millis(49));

        // Two actors at different phases converge on the same boundary: their
        // target instants (now + delay) agree.
        let a = epoch + Duration::from_millis(17);
        let b = epoch + Duration::from_millis(41);
        let target_a = a + grid_delay(a, epoch, interval);
        let target_b = b + grid_delay(b, epoch, interval);
        assert_eq!(target_a, target_b);

        // `now` before the epoch (test clocks) still sleeps to the next real
        // boundary: epoch+50ms is 55ms after `epoch-5ms`.
        let before = epoch - Duration::from_millis(5);
        assert_eq!(
            grid_delay(before, epoch, interval),
            Duration::from_millis(55)
        );
    }
}
