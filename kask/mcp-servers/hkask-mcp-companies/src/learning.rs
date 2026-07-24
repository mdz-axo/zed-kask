//! Provider-learning regulator — Beta(α+1, β+1) reliability tracking and
//! temporal-staleness detection for the dual-provider (FMP / EODHD) routing.
//!
//! Extracted from the crate root so the server composition (`lib.rs`) holds
//! dispatch and forecast-store responsibilities only. The regulator models
//! each (symbol, provider) pair as a Beta posterior; a provider is bypassed
//! when its success probability falls below the flaky threshold with
//! sufficient observations, or when its latest filing is older than the
//! chronic-staleness window.

use crate::Provider;
use crate::data_quality::TemporalSnapshot;
use hkask_types::time::now_rfc3339;

/// Flaky provider threshold: P(success) below this with sufficient observations → flaky.
const FLAKY_PROBABILITY_THRESHOLD: f64 = 0.70;
/// Minimum observations before the flaky classification is trusted.
const FLAKY_MIN_OBSERVATIONS: u64 = 5;
/// Chronic staleness: data older than this many days → chronically stale.
/// Default threshold; overridable per-instance via `LearningState::with_staleness_days`
/// and at launch via the `HKASK_CHRONIC_STALENESS_DAYS` environment variable.
pub const CHRONIC_STALENESS_DAYS: u32 = 90;

/// Learning state — tracks user feedback per (tool, symbol, provider) to adapt
/// provider routing. Uses Beta(α+1, β+1) conjugate prior (same statistical
/// foundation as Regulation ToolStats Layer 2) for Bayesian reliability tracking.
///
/// Temporal coherence (FinGPT RLSP-inspired): also tracks price snapshots
/// at fetch time so we can detect when a provider returned stale data.
///
/// Statistical foundation: each (symbol, provider) maintains α = successes+1,
/// β = failures+1. The posterior success probability is α/(α+β). A provider
/// is flaky when P(success) < FLAKY_PROBABILITY_THRESHOLD with at least FLAKY_MIN_OBSERVATIONS observations.
#[derive(Debug, Clone)]
pub struct LearningState {
    /// (symbol, provider) → (α_successes, β_failures, n_total)
    /// Beta posterior: P(success) = α / (α+β)
    provider_scores: std::collections::HashMap<(String, Provider), (u64, u64, u64)>,
    /// Temporal snapshots: (symbol, provider) → most recent snapshot.
    temporal_snapshots: std::collections::HashMap<(String, Provider), TemporalSnapshot>,
    /// Chronic-staleness threshold in days. A provider whose latest temporal
    /// snapshot is older than this is treated as chronically stale and
    /// bypassed by `preferred_provider`. Defaults to `CHRONIC_STALENESS_DAYS`
    /// (90); configurable via `with_staleness_days` or the
    /// `HKASK_CHRONIC_STALENESS_DAYS` launch variable.
    staleness_days: u32,
}

impl Default for LearningState {
    fn default() -> Self {
        Self {
            provider_scores: Default::default(),
            temporal_snapshots: Default::default(),
            staleness_days: CHRONIC_STALENESS_DAYS,
        }
    }
}

impl LearningState {
    /// Record a user rating for a tool result. Updates Beta conjugate prior.
    ///
    /// Scores 4–5 count as successes (the data was useful/accurate).
    /// Scores 1–3 count as failures (the data missed or misled).
    /// None (comments only) counts as an observation without success/failure.
    pub fn record(&mut self, symbol: &str, provider: Provider, score: Option<u8>) {
        let key = (symbol.to_string(), provider);
        let entry = self.provider_scores.entry(key).or_insert((1, 1, 0));
        if let Some(s) = score {
            entry.2 += 1; // only count scored observations toward threshold
            if s >= 4 {
                entry.0 += 1; // α: success
            } else {
                entry.1 += 1; // β: failure
            }
        }
        // Comments-only ratings don't affect probability or threshold
    }

    /// Beta posterior success probability: α / (α + β).
    pub fn success_probability(&self, symbol: &str, provider: Provider) -> Option<f64> {
        let key = (symbol.to_string(), provider);
        let (alpha, beta, n) = self.provider_scores.get(&key)?;
        if *n == 0 {
            return None;
        }
        Some(*alpha as f64 / (*alpha + *beta) as f64)
    }

    /// Number of observations for this (symbol, provider).
    pub fn observation_count(&self, symbol: &str, provider: Provider) -> u64 {
        self.provider_scores
            .get(&(symbol.to_string(), provider))
            .map(|(_, _, n)| *n)
            .unwrap_or(0)
    }

    /// Record a temporal snapshot for later coherence checking.
    pub fn record_temporal_snapshot(
        &mut self,
        symbol: &str,
        provider: Provider,
        price: f64,
        latest_filing_date: Option<String>,
    ) {
        let key = (symbol.to_string(), provider);
        self.temporal_snapshots.insert(
            key,
            TemporalSnapshot {
                fetched_at: now_rfc3339(),
                price_at_fetch: price,
                latest_filing_date,
            },
        );
    }

    /// Check temporal coherence for a symbol/provider: how stale was the
    /// data at fetch time? Returns None if no snapshot exists.
    pub fn check_staleness(&self, symbol: &str, provider: Provider) -> Option<u32> {
        let key = (symbol.to_string(), provider);
        let snapshot = self.temporal_snapshots.get(&key)?;
        let now = chrono::Utc::now();
        snapshot.staleness_days(&now)
    }

    /// Check if a provider should be avoided for a given symbol.
    /// Uses Beta posterior: flaky when P(success) < FLAKY_PROBABILITY_THRESHOLD with ≥FLAKY_MIN_OBSERVATIONS observations.
    pub fn is_flaky(&self, symbol: &str, provider: Provider) -> bool {
        if let Some(prob) = self.success_probability(symbol, provider) {
            prob < FLAKY_PROBABILITY_THRESHOLD
                && self.observation_count(symbol, provider) >= FLAKY_MIN_OBSERVATIONS
        } else {
            false
        }
    }

    /// Construct a `LearningState` with a custom chronic-staleness threshold
    /// (days). Use when the default 90-day window does not match the asset
    /// class or data cadence.
    pub fn with_staleness_days(staleness_days: u32) -> Self {
        Self {
            staleness_days,
            ..Default::default()
        }
    }

    /// Chronic-staleness threshold currently in effect (days).
    pub fn staleness_days(&self) -> u32 {
        self.staleness_days
    }

    /// Check if a provider consistently returns stale data for a symbol.
    pub fn is_chronically_stale(&self, symbol: &str, provider: Provider) -> bool {
        self.check_staleness(symbol, provider)
            .map(|days| days > self.staleness_days)
            .unwrap_or(false)
    }

    /// Get the preferred provider for a symbol based on learning.
    /// Emits a Regulation routing span when learning overrides the default provider.
    pub fn preferred_provider(&self, symbol: &str, default_provider: Provider) -> Option<Provider> {
        let fmp_flaky = self.is_flaky(symbol, Provider::Fmp)
            || self.is_chronically_stale(symbol, Provider::Fmp);
        let eodhd_flaky = self.is_flaky(symbol, Provider::Eodhd)
            || self.is_chronically_stale(symbol, Provider::Eodhd);

        let result = if fmp_flaky && !eodhd_flaky {
            Some(Provider::Eodhd)
        } else if eodhd_flaky && !fmp_flaky {
            Some(Provider::Fmp)
        } else {
            None
        };

        // Regulation routing span: when learning overrides the default, emit observability
        if let Some(ref chosen) = result {
            let fmp_prob = self
                .success_probability(symbol, Provider::Fmp)
                .unwrap_or(1.0);
            let eodhd_prob = self
                .success_probability(symbol, Provider::Eodhd)
                .unwrap_or(1.0);
            tracing::debug!(
                target: "hkask.mcp.companies.routing",
                symbol = %symbol,
                default = %default_provider,
                chosen = %chosen,
                fmp_success_prob = %fmp_prob,
                eodhd_success_prob = %eodhd_prob,
                fmp_stale = %self.is_chronically_stale(symbol, Provider::Fmp),
                eodhd_stale = %self.is_chronically_stale(symbol, Provider::Eodhd),
                "Provider routing: learning override active"
            );
        }

        result
    }

    /// Export learning state as JSON for Regulation consumption / persistence.
    pub fn export_state(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for ((symbol, provider), (alpha, beta, n)) in &self.provider_scores {
            let prob = if *n > 0 {
                *alpha as f64 / (*alpha + *beta) as f64
            } else {
                1.0
            };
            let key = format!("{symbol}@{provider}");
            map.insert(
                key,
                serde_json::json!({
                    "symbol": symbol,
                    "provider": provider.to_string(),
                    "alpha": alpha,
                    "beta": beta,
                    "observations": n,
                    "success_probability": prob,
                }),
            );
        }
        serde_json::Value::Object(map)
    }
}
