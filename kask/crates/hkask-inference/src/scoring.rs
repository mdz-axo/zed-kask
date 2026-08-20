//! Scored provider selection — 7-dimension scoring engine for media
//! provider selection (OpenMontage pattern).
//!
//! Each provider is scored across 7 dimensions: `task_fit`, `quality`,
//! `control`, `reliability`, `cost`, `latency`, `continuity`. The provider
//! with the highest weighted score is selected. Default scores are a
//! starting point for operators to tune once multiple media providers are
//! registered again.

use crate::provider::{MediaOp, MediaProvider, ProviderRegistry};
use std::sync::Arc;

/// 7-dimension score for a provider (each 0.0–1.0).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderScore {
    pub task_fit: f64,
    pub quality: f64,
    pub control: f64,
    pub reliability: f64,
    pub cost: f64,
    pub latency: f64,
    pub continuity: f64,
}

/// Weights for the 7 dimensions (defaults from OpenMontage).
#[derive(Debug, Clone)]
pub struct ScoreWeights {
    pub task_fit: f64,
    pub quality: f64,
    pub control: f64,
    pub reliability: f64,
    pub cost: f64,
    pub latency: f64,
    pub continuity: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            task_fit: 0.30,
            quality: 0.20,
            control: 0.15,
            reliability: 0.15,
            cost: 0.10,
            latency: 0.05,
            continuity: 0.05,
        }
    }
}

/// A provider scored for a specific op.
#[derive(Debug, Clone)]
pub struct ScoredProvider {
    pub id: String,
    pub score: ProviderScore,
    pub weighted: f64,
}

impl ProviderScore {
    /// Compute the weighted sum of all 7 dimensions.
    #[must_use]
    pub fn weighted(&self, w: &ScoreWeights) -> f64 {
        self.task_fit * w.task_fit
            + self.quality * w.quality
            + self.control * w.control
            + self.reliability * w.reliability
            + self.cost * w.cost
            + self.latency * w.latency
            + self.continuity * w.continuity
    }
}

/// Score a single provider for a given op.
///
/// Default scoring is a neutral baseline; provider-specific arms are added
/// here as media providers are (re-)registered.
fn score_provider(id: &str, op: MediaOp) -> ProviderScore {
    match (id, op) {
        _ => ProviderScore::default(),
    }
}

/// Select the best provider for `op` using 7-dimension scored selection.
///
/// Returns the chosen provider + all candidate scores (including the
/// chosen one). The decision is logged via `tracing::info!` at
/// `reg.media.select` with all candidate scores.
pub fn select_scored(
    registry: &ProviderRegistry,
    op: MediaOp,
) -> Result<(Arc<dyn MediaProvider>, Vec<ScoredProvider>), hkask_types::InferenceError> {
    let weights = ScoreWeights::default();
    let candidates: Vec<&Arc<dyn MediaProvider>> = registry
        .providers()
        .iter()
        .filter(|p| p.supports(op))
        .collect();

    if candidates.is_empty() {
        return Err(hkask_types::InferenceError::NotConfigured(format!(
            "no provider configured for media op: {}",
            op.as_str()
        )));
    }

    let mut scored: Vec<ScoredProvider> = Vec::new();
    for provider in &candidates {
        let score = score_provider(provider.id(), op);
        let weighted = score.weighted(&weights);
        scored.push(ScoredProvider {
            id: provider.id().to_string(),
            score,
            weighted,
        });
    }

    let mut best_idx = 0;
    let mut best_weighted = f64::NEG_INFINITY;
    for (i, s) in scored.iter().enumerate() {
        if s.weighted > best_weighted {
            best_weighted = s.weighted;
            best_idx = i;
        }
    }

    let chosen = Arc::clone(candidates[best_idx]);

    tracing::info!(
        target: "reg.media.select",
        op = op.as_str(),
        chosen = chosen.id(),
        chosen_score = scored[best_idx].weighted,
        candidates = ?scored.iter().map(|s| (s.id.as_str(), s.weighted)).collect::<Vec<_>>(),
        "Scored provider selection"
    );

    Ok((chosen, scored))
}
