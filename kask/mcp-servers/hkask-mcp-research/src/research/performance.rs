//! In-process rolling performance aggregator for web search providers.
//!
//! Closes the cybernetic feedback loop for provider selection: each
//! `reg.web.provider` span (emitted by `search_compound` and
//! `search_single_provider`) is recorded here, and `score_providers` reads
//! the rolling aggregates to apply live success-rate and p50-latency
//! penalties on top of the static `ProviderProfile` table.
//!
//! This is the in-process path — the spans still flow to the durable
//! `RegulationArchive` on the curator's `curator.db` for cross-session
//! observability and curator `reg_query`. The aggregator here is the
//! fast-path read for real-time selection; the archive is the slow-path
//! read for long-term trend analysis.
//!
//! Design:
//! - Bounded ring buffer per provider (default 64 samples). Old samples
//!   drop off as new ones arrive — a rolling window, not an unbounded
//!   accumulator.
//! - `Mutex<...>` (not `RwLock`) — write path is on every search call,
//!   read path is on `score_providers`/`web_recommend_provider`. Both
//!   are short critical sections; a lock is simpler and correct.
//! - `Send + Sync` so it can sit behind `ProviderPool`'s `Arc<dyn WebSearchPort>`.

use std::collections::HashMap;
use std::sync::Mutex;

/// Maximum samples retained per provider. A rolling window — once full,
/// the oldest sample is dropped when a new one arrives. 64 is enough for
/// a stable p50 without unbounded memory growth.
const MAX_SAMPLES_PER_PROVIDER: usize = 64;

/// Minimum samples before live penalties apply. Below this, the static
/// profile alone drives selection — the live data is too thin to trust.
const MIN_SAMPLES_FOR_LIVE: usize = 3;

/// A single provider outcome observation, recorded per `reg.web.provider` span.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderOutcome {
    /// Latency of the provider call in milliseconds.
    pub latency_ms: u64,
    /// Whether the call succeeded (Ok) or failed (any Err variant).
    pub success: bool,
}

/// Rolling performance stats for a single provider, computed from the
/// recent `ProviderOutcome` samples.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProviderStats {
    /// Success rate over the rolling window: `successes / total`.
    /// `0.0` when no samples.
    pub success_rate: f64,
    /// p50 (median) latency in milliseconds over the rolling window.
    /// `0` when no samples.
    pub p50_latency_ms: u64,
    /// Number of samples in the window.
    pub sample_count: usize,
}

/// In-process rolling aggregator. One instance lives on `ProviderPool`
/// behind a `Mutex`. Updated by `record_outcome` (called from the span
/// emission sites), read by `stats_for` / `all_stats` (called from
/// `score_providers`).
#[derive(Debug, Default)]
pub(crate) struct ProviderPerformanceAggregator {
    /// Per-provider ring buffer of recent outcomes.
    samples: HashMap<String, Vec<ProviderOutcome>>,
}

impl ProviderPerformanceAggregator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a provider outcome. Called inline at the `reg.web.provider`
    /// span emission site — right after the provider call completes.
    pub(crate) fn record_outcome(&mut self, kind: &str, outcome: ProviderOutcome) {
        let samples = self.samples.entry(kind.to_string()).or_default();
        if samples.len() >= MAX_SAMPLES_PER_PROVIDER {
            samples.remove(0);
        }
        samples.push(outcome);
    }

    /// Compute rolling stats for a single provider. Returns empty stats
    /// (0 success rate, 0 p50, 0 samples) when no observations exist.
    pub(crate) fn stats_for(&self, kind: &str) -> ProviderStats {
        let Some(samples) = self.samples.get(kind) else {
            return ProviderStats::default();
        };
        if samples.is_empty() {
            return ProviderStats::default();
        }
        let total = samples.len();
        let successes = samples.iter().filter(|o| o.success).count();
        let success_rate = successes as f64 / total as f64;

        // p50: sort a copy of latencies, take the middle.
        let mut latencies: Vec<u64> = samples.iter().map(|o| o.latency_ms).collect();
        latencies.sort_unstable();
        let p50 = latencies[total / 2];

        ProviderStats {
            success_rate,
            p50_latency_ms: p50,
            sample_count: total,
        }
    }

    /// Whether a provider has enough samples for live penalties to apply.
    /// Below this, the static profile alone drives selection.
    pub(crate) fn has_enough_samples(&self, kind: &str) -> bool {
        self.samples
            .get(kind)
            .is_some_and(|s| s.len() >= MIN_SAMPLES_FOR_LIVE)
    }
}

/// Live penalty applied on top of the static `score_static` baseline.
///
/// Returns `(penalty, rationale_parts)`:
/// - `penalty`: added to the static score (lower is better, so a positive
///   penalty worsens the ranking).
/// - `rationale_parts`: human-readable factors to append to the recommendation
///   rationale so the model sees *why* a provider's live score shifted.
///
/// Penalties:
/// - Success rate < 0.5: +2.0 (severe — provider is failing half the time)
/// - Success rate 0.5-0.8: +1.0 × (1 - success_rate) (scaled)
/// - p50 latency > 3000ms: +0.5 (slow)
/// - p50 latency 1500-3000ms: +0.2 (medium)
///
/// Only applies when `has_enough_samples` is true (≥3 samples). Below that,
/// returns (0.0, []) — the static profile alone drives selection.
pub(crate) fn live_performance_penalty(
    aggregator: &Mutex<ProviderPerformanceAggregator>,
    kind: &str,
) -> (f64, Vec<&'static str>) {
    let agg = match aggregator.lock() {
        Ok(a) => a,
        Err(_) => {
            // Poisoned lock — skip live data, fall back to static. Don't
            // panic: provider selection is not a correctness path.
            return (0.0, Vec::new());
        }
    };
    if !agg.has_enough_samples(kind) {
        return (0.0, Vec::new());
    }
    let stats = agg.stats_for(kind);
    drop(agg); // release lock before building rationale

    let mut penalty = 0.0;
    let mut rationale: Vec<&'static str> = Vec::new();

    if stats.success_rate < 0.5 {
        penalty += 2.0;
        rationale.push("low success rate");
    } else if stats.success_rate < 0.8 {
        penalty += (1.0 - stats.success_rate) * 1.0;
        rationale.push("degraded success rate");
    }

    if stats.p50_latency_ms > 3000 {
        penalty += 0.5;
        rationale.push("slow p50 latency");
    } else if stats.p50_latency_ms > 1500 {
        penalty += 0.2;
        rationale.push("medium p50 latency");
    }

    (penalty, rationale)
}

/// Expose the rolling stats for a provider for surfacing in tool output
/// (e.g. `web_recommend_provider` could include live stats). Read-only
/// snapshot — does not mutate the aggregator.
pub(crate) fn snapshot_stats(
    aggregator: &Mutex<ProviderPerformanceAggregator>,
    kind: &str,
) -> Option<ProviderStats> {
    let agg = aggregator.lock().ok()?;
    if agg.has_enough_samples(kind) {
        Some(agg.stats_for(kind))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_outcome_builds_up_samples() {
        let mut agg = ProviderPerformanceAggregator::new();
        agg.record_outcome(
            "brave",
            ProviderOutcome {
                latency_ms: 100,
                success: true,
            },
        );
        agg.record_outcome(
            "brave",
            ProviderOutcome {
                latency_ms: 200,
                success: false,
            },
        );
        let stats = agg.stats_for("brave");
        assert_eq!(stats.sample_count, 2);
        assert!((stats.success_rate - 0.5).abs() < 1e-9);
        assert_eq!(stats.p50_latency_ms, 200); // sorted [100,200], middle = 200
    }

    #[test]
    fn ring_buffer_drops_oldest_when_full() {
        let mut agg = ProviderPerformanceAggregator::new();
        for i in 0..(MAX_SAMPLES_PER_PROVIDER + 5) {
            agg.record_outcome(
                "exa",
                ProviderOutcome {
                    latency_ms: i as u64,
                    success: true,
                },
            );
        }
        let stats = agg.stats_for("exa");
        assert_eq!(stats.sample_count, MAX_SAMPLES_PER_PROVIDER);
        // Oldest (0..5) dropped; first remaining is 5.
        assert_eq!(stats.success_rate, 1.0);
    }

    #[test]
    fn has_enough_samples_threshold() {
        let mut agg = ProviderPerformanceAggregator::new();
        assert!(!agg.has_enough_samples("brave"));
        agg.record_outcome(
            "brave",
            ProviderOutcome {
                latency_ms: 100,
                success: true,
            },
        );
        agg.record_outcome(
            "brave",
            ProviderOutcome {
                latency_ms: 100,
                success: true,
            },
        );
        assert!(!agg.has_enough_samples("brave")); // 2 < 3
        agg.record_outcome(
            "brave",
            ProviderOutcome {
                latency_ms: 100,
                success: true,
            },
        );
        assert!(agg.has_enough_samples("brave")); // 3 >= 3
    }

    #[test]
    fn live_penalty_zero_below_threshold() {
        let agg = Mutex::new(ProviderPerformanceAggregator::new());
        // No samples — no penalty.
        let (penalty, rationale) = live_performance_penalty(&agg, "brave");
        assert!((penalty - 0.0).abs() < 1e-9);
        assert!(rationale.is_empty());
    }

    #[test]
    fn live_penalty_applies_for_low_success_rate() {
        let agg = Mutex::new(ProviderPerformanceAggregator::new());
        {
            let mut a = agg.lock().unwrap();
            for _ in 0..3 {
                a.record_outcome(
                    "brave",
                    ProviderOutcome {
                        latency_ms: 100,
                        success: false,
                    },
                );
            }
            for _ in 0..1 {
                a.record_outcome(
                    "brave",
                    ProviderOutcome {
                        latency_ms: 100,
                        success: true,
                    },
                );
            }
        }
        // 1/4 success = 0.25 < 0.5 → +2.0
        let (penalty, rationale) = live_performance_penalty(&agg, "brave");
        assert!((penalty - 2.0).abs() < 1e-9, "penalty was {penalty}");
        assert!(rationale.contains(&"low success rate"));
    }

    #[test]
    fn live_penalty_applies_for_slow_p50() {
        let agg = Mutex::new(ProviderPerformanceAggregator::new());
        {
            let mut a = agg.lock().unwrap();
            for _ in 0..5 {
                a.record_outcome(
                    "exa",
                    ProviderOutcome {
                        latency_ms: 4000,
                        success: true,
                    },
                );
            }
        }
        // 100% success, p50=4000 > 3000 → +0.5
        let (penalty, rationale) = live_performance_penalty(&agg, "exa");
        assert!((penalty - 0.5).abs() < 1e-9, "penalty was {penalty}");
        assert!(rationale.contains(&"slow p50 latency"));
    }

    #[test]
    fn snapshot_stats_returns_none_below_threshold() {
        let agg = Mutex::new(ProviderPerformanceAggregator::new());
        assert!(snapshot_stats(&agg, "brave").is_none());
        {
            let mut a = agg.lock().unwrap();
            a.record_outcome(
                "brave",
                ProviderOutcome {
                    latency_ms: 100,
                    success: true,
                },
            );
        }
        assert!(snapshot_stats(&agg, "brave").is_none()); // 1 < 3
    }

    #[test]
    fn snapshot_stats_returns_some_at_threshold() {
        let agg = Mutex::new(ProviderPerformanceAggregator::new());
        {
            let mut a = agg.lock().unwrap();
            for _ in 0..3 {
                a.record_outcome(
                    "brave",
                    ProviderOutcome {
                        latency_ms: 100,
                        success: true,
                    },
                );
            }
        }
        let stats = snapshot_stats(&agg, "brave").expect("should have stats at 3 samples");
        assert_eq!(stats.sample_count, 3);
        assert!((stats.success_rate - 1.0).abs() < 1e-9);
    }

    /// `Duration` was used in the module doc but not in code — the test
    /// below confirms the module compiles standalone.
    #[test]
    fn module_compiles_standalone() {
        let _agg = ProviderPerformanceAggregator::new();
    }
}
