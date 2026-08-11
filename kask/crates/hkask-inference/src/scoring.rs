//! Scored provider selection — 7-dimension scoring engine for media
//! provider selection (OpenMontage pattern).
//!
//! Each provider is scored across 7 dimensions: `task_fit`, `quality`,
//! `control`, `reliability`, `cost`, `latency`, `continuity`. The provider
//! with the highest weighted score is selected. Default scores encode the
//! current DeepInfra-first / AtlasCloud-fallback policy so behavior is preserved
//! until operators tune weights.

use crate::provider::{MediaOp, MediaProvider, ProviderRegistry};
use hkask_types::MediaGenerateParams;
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
/// Default scoring encodes the current dispatch policy:
/// - `deepinfra`: highest for `RemoveBackground`/`GenerateSpeech`/`Transcribe`
///   (cheapest for these ops, registered first as fallback target)
/// - `atlascloud`: highest for image/video (quality, broad support)
/// - `atlascloud`: lower (fallback only, task-based API adds latency)
fn score_provider(id: &str, op: MediaOp) -> ProviderScore {
    match (id, op) {
        (
            "deepinfra",
            MediaOp::RemoveBackground | MediaOp::GenerateSpeech | MediaOp::Transcribe,
        ) => ProviderScore {
            task_fit: 0.95,
            quality: 0.80,
            control: 0.70,
            reliability: 0.90,
            cost: 0.95,
            latency: 0.85,
            continuity: 0.80,
        },
        ("atlascloud", _) => ProviderScore {
            task_fit: 0.90,
            quality: 0.90,
            control: 0.85,
            reliability: 0.85,
            cost: 0.70,
            latency: 0.75,
            continuity: 0.90,
        },
        _ => ProviderScore::default(),
    }
}

/// Select the best provider for `op` using 7-dimension scored selection.
///
/// Returns the chosen provider + all candidate scores (including the
/// chosen one). The decision is logged via `tracing::info!` at
/// `reg.media.select` with all candidate scores.
///
/// Default weights reproduce the current DeepInfra-first / AtlasCloud-fallback
/// policy: DeepInfra wins for `RemoveBackground`/`GenerateSpeech`/`Transcribe`,
/// AtlasCloud wins for everything else.
pub fn select_scored(
    registry: &ProviderRegistry,
    op: MediaOp,
    _params: &MediaGenerateParams,
) -> Result<(Arc<dyn MediaProvider>, Vec<ScoredProvider>), hkask_types::InferenceError> {
    let weights = ScoreWeights::default();
    let candidates: Vec<&Arc<dyn MediaProvider>> = registry
        .providers()
        .iter()
        .filter(|p| p.supports(op))
        .collect();

    if candidates.is_empty() {
        return Err(hkask_types::InferenceError::Connection(format!(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InferenceConfig;
    use crate::media_router::MediaRouter;

    /// Build a router with both DeepInfra and AtlasCloud keys so both backends
    /// register — the multi-provider case where `select_scored` is exercised.
    fn router_with_both() -> MediaRouter {
        let config = InferenceConfig {
            deepinfra_api_key: "di-key".into(),
            atlascloud_api_key: "ac-key".into(),
            ..Default::default()
        };
        MediaRouter::new(config)
    }

    #[test]
    fn select_scored_prefers_deepinfra_for_shared_ops() {
        let router = router_with_both();
        let params = MediaGenerateParams::default();
        // GenerateSpeech and Transcribe are served by both DeepInfra and AtlasCloud.
        // The score table ranks DeepInfra higher for these shared ops.
        for op in [MediaOp::GenerateSpeech, MediaOp::Transcribe] {
            let (chosen, scores) =
                select_scored(&router.registry, op, &params).expect("candidates exist");
            assert_eq!(chosen.id(), "deepinfra", "DeepInfra must win for {op:?}");
            assert!(
                scores.iter().any(|s| s.id == "deepinfra"),
                "DeepInfra scored"
            );
            assert!(
                scores.iter().any(|s| s.id == "atlascloud"),
                "atlascloud scored"
            );
        }
    }

    #[test]
    fn select_scored_errors_when_no_provider_supports_op() {
        let router = MediaRouter::new(InferenceConfig::default());
        let result = select_scored(
            &router.registry,
            MediaOp::GenerateImage,
            &MediaGenerateParams::default(),
        );
        assert!(result.is_err(), "empty registry must error, not panic");
    }

    #[test]
    fn select_scored_returns_all_candidate_scores() {
        let router = router_with_both();
        let (chosen, scores) = select_scored(
            &router.registry,
            MediaOp::GenerateSpeech,
            &MediaGenerateParams::default(),
        )
        .expect("candidates exist");
        // GenerateSpeech is served by both DeepInfra and AtlasCloud → both appear in scores.
        assert_eq!(scores.len(), 2, "both candidates scored");
        // The chosen provider's score must be the maximum.
        let max = scores
            .iter()
            .map(|s| s.weighted)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            scores
                .iter()
                .any(|s| s.id == chosen.id() && (s.weighted - max).abs() < f64::EPSILON)
        );
    }
}
