//! Adaptive Provider Monitor — background daemon for provider cost surveillance.
//!
//! Monitors configured providers at dynamically-adjusted intervals:
//!   usage < 50%  → daily
//!   50-70%       → every 6 hours
//!   70-90%       → hourly
//!   usage ≥ 90%  → every 10 minutes
//!
//! Emits Regulation spans when a provider crosses from pre-paid/subscription
//! into marginal/overage pricing (`reg.provider.marginal_activated`).

use crate::runtime::provider_intel::ProviderIntelligence;
#[cfg(test)]
use crate::runtime::provider_intel::UsageStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::Instant;

/// A single provider under surveillance.
struct WatchedProvider {
    provider: Box<dyn ProviderIntelligence>,
    api_key: String,
    /// Last known marginal state — used to detect transitions.
    was_marginal: bool,
    /// True until the first check completes — suppresses false alerts
    /// for always-marginal providers.
    first_check: bool,
    /// When to next check this provider.
    next_check: Instant,
    /// Current check interval (adjusted by usage fraction).
    interval: Duration,
}

impl WatchedProvider {
    fn new(provider: Box<dyn ProviderIntelligence>, api_key: String) -> Self {
        Self {
            provider,
            api_key,
            was_marginal: false,
            first_check: true,
            next_check: Instant::now(),
            interval: Duration::from_secs(24 * 3600),
        }
    }

    /// Determine check interval from usage fraction.
    fn interval_for_fraction(fraction: f64) -> Duration {
        if fraction >= 0.90 {
            Duration::from_secs(10 * 60)
        } else if fraction >= 0.70 {
            Duration::from_secs(3600)
        } else if fraction >= 0.50 {
            Duration::from_secs(6 * 3600)
        } else {
            Duration::from_secs(24 * 3600)
        }
    }

    /// Run one check cycle for this provider.
    async fn check(&mut self) {
        let provider_id = self.provider.provider_id();

        // Query usage
        let usage = match self.provider.usage(&self.api_key).await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.provider",
                    provider = %provider_id,
                    error = %e,
                    "Failed to query provider usage"
                );
                return;
            }
        };

        // Query actual cost (use empty model name for base/provider-default rate)
        let cost = match self.provider.actual_cost(&self.api_key, "").await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.provider",
                    provider = %provider_id,
                    error = %e,
                    "Failed to query actual cost"
                );
                return;
            }
        };

        // Detect marginal activation (false → true transition).
        // Suppress on first check — always-marginal providers would false-positive.
        if cost.is_marginal && !self.was_marginal && !self.first_check {
            tracing::warn!(
                target: "hkask.provider.marginal_activated",
                provider = %provider_id,
                consumed = usage.consumed,
                limit = usage.limit,
                fraction = %format!("{:.1}%", usage.fraction * 100.0),
                "Provider crossed into marginal pricing — overage rates now apply"
            );
        }
        self.was_marginal = cost.is_marginal;
        self.first_check = false;

        // Adjust check interval based on usage fraction
        let new_interval = Self::interval_for_fraction(usage.fraction);
        if new_interval != self.interval {
            tracing::info!(
                target: "hkask.provider",
                provider = %provider_id,
                old_interval_secs = self.interval.as_secs(),
                new_interval_secs = new_interval.as_secs(),
                fraction = %format!("{:.1}%", usage.fraction * 100.0),
                "Adjusted monitoring interval"
            );
            self.interval = new_interval;
        }

        self.next_check = Instant::now() + self.interval;

        tracing::debug!(
            target: "hkask.provider",
            provider = %provider_id,
            consumed = usage.consumed,
            limit = usage.limit,
            fraction = %format!("{:.1}%", usage.fraction * 100.0),
            is_marginal = cost.is_marginal,
            next_check_secs = self.interval.as_secs(),
            "Provider check complete"
        );
    }
}

/// Adaptive monitoring daemon — watches multiple providers,
/// accelerating check frequency as usage approaches limits.
pub struct AdaptiveMonitor {
    providers: Vec<WatchedProvider>,
    /// Set to true to trigger graceful shutdown.
    shutdown: Arc<AtomicBool>,
}

impl AdaptiveMonitor {
    /// REQ: P9-daemon-create
    /// expect: "I can create an adaptive monitor to watch provider costs" \[P9\]
    /// pre:  none
    /// post: returns empty monitor ready for provider registration
    /// \[P9\] Constraining: Observability — provider costs are surveilled
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Signal the monitor to shut down gracefully at the next check cycle.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// REQ: P9-daemon-add-provider
    /// expect: "I can register a provider for adaptive cost monitoring" \[P9\]
    /// pre:  provider is a valid ProviderIntelligence implementation
    /// post: provider is added to the monitoring schedule
    /// \[P9\] Constraining: Observability — all registered providers are watched
    pub fn add_provider(&mut self, provider: Box<dyn ProviderIntelligence>, api_key: String) {
        tracing::info!(
            target: "hkask.provider",
            provider = %provider.provider_id(),
            "Provider registered for adaptive monitoring"
        );
        self.providers.push(WatchedProvider::new(provider, api_key));
    }

    /// REQ: P9-daemon-run
    /// expect: "The daemon watches providers and accelerates checks as limits approach" \[P9\]
    /// pre:  at least one provider registered (or daemon parks idle)
    /// post: runs indefinitely, checking each provider at its adaptive interval
    /// inv:  returns on shutdown signal
    /// \[P9\] Constraining: Observability — continuous provider surveillance
    pub async fn run(&mut self) {
        if self.providers.is_empty() {
            tracing::warn!(
                target: "hkask.provider",
                "Adaptive monitor started with no providers — idle"
            );
            // Park forever — caller can add providers externally
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }

        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                tracing::info!(target: "hkask.provider", "Adaptive monitor shutting down");
                return;
            }

            // Find the provider with the earliest next_check
            let now = Instant::now();
            let mut next_deadline = now + Duration::from_secs(3600); // default: 1 hour

            for p in &mut self.providers {
                if p.next_check <= now {
                    p.check().await;
                }
                if p.next_check < next_deadline {
                    next_deadline = p.next_check;
                }
            }

            // Sleep until the next provider needs checking
            let sleep_dur = next_deadline.saturating_duration_since(Instant::now());
            if sleep_dur > Duration::ZERO {
                tokio::time::sleep(sleep_dur).await;
            }
        }
    }
}

impl Default for AdaptiveMonitor {
    fn default() -> Self {
        Self::new()
    }
}
