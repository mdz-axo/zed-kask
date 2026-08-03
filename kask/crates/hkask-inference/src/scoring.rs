//! Scored provider selection — 7-dimension scoring engine for media
//! provider selection (OpenMontage pattern).
//!
//! Each provider is scored across 7 dimensions: `task_fit`, `quality`,
//! `control`, `reliability`, `cost`, `latency`, `continuity`. The provider
//! with the highest weighted score is selected. Default scores encode the
//! current DeepInfra-first / fal-fallback policy so behavior is preserved
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
/// - `fal.ai`: highest for image/video (quality, broad support)
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
        ("fal.ai", _) => ProviderScore {
            task_fit: 0.90,
            quality: 0.90,
            control: 0.85,
            reliability: 0.85,
            cost: 0.70,
            latency: 0.75,
            continuity: 0.90,
        },
        ("atlascloud", _) => ProviderScore {
            task_fit: 0.75,
            quality: 0.75,
            control: 0.60,
            reliability: 0.70,
            cost: 0.65,
            latency: 0.60,
            continuity: 0.65,
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
/// Default weights reproduce the current DeepInfra-first / fal-fallback
/// policy: DeepInfra wins for `RemoveBackground`/`GenerateSpeech`/`Transcribe`,
/// Fal wins for everything else, AtlasCloud is the lowest-scored fallback.
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
    use std::future::Future;
    use std::pin::Pin;

    struct MockProvider {
        id: &'static str,
        supported: Vec<MediaOp>,
    }

    impl MediaProvider for MockProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn supports(&self, op: MediaOp) -> bool {
            self.supported.contains(&op)
        }
        fn execute<'a>(
            &'a self,
            _op: MediaOp,
            _params: &'a MediaGenerateParams,
        ) -> Pin<
            Box<
                dyn Future<Output = Result<serde_json::Value, hkask_types::InferenceError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(serde_json::json!({"provider": self.id})) })
        }
    }

    fn empty_params() -> MediaGenerateParams {
        MediaGenerateParams::default()
    }

    #[test]
    fn default_weights_reproduce_deepinfra_first_fal_fallback() {
        let registry = ProviderRegistry::new(vec![
            Arc::new(MockProvider {
                id: "deepinfra",
                supported: vec![MediaOp::RemoveBackground],
            }),
            Arc::new(MockProvider {
                id: "fal.ai",
                supported: vec![MediaOp::RemoveBackground, MediaOp::GenerateImage],
            }),
        ]);

        // RemoveBackground: DeepInfra scores higher (cheapest for this op)
        let (chosen, _) =
            select_scored(&registry, MediaOp::RemoveBackground, &empty_params()).unwrap();
        assert_eq!(chosen.id(), "deepinfra");

        // GenerateImage: only Fal supports it, so Fal is chosen
        let (chosen, _) =
            select_scored(&registry, MediaOp::GenerateImage, &empty_params()).unwrap();
        assert_eq!(chosen.id(), "fal.ai");
    }

    #[test]
    fn scoring_logs_all_candidates() {
        let registry = ProviderRegistry::new(vec![
            Arc::new(MockProvider {
                id: "deepinfra",
                supported: vec![MediaOp::RemoveBackground],
            }),
            Arc::new(MockProvider {
                id: "fal.ai",
                supported: vec![MediaOp::RemoveBackground],
            }),
        ]);

        let (_, scores) =
            select_scored(&registry, MediaOp::RemoveBackground, &empty_params()).unwrap();
        assert_eq!(scores.len(), 2, "both providers should be scored");
        assert!(scores.iter().any(|s| s.id == "deepinfra"));
        assert!(scores.iter().any(|s| s.id == "fal.ai"));
    }

    #[test]
    fn no_provider_errors() {
        let registry = ProviderRegistry::new(vec![]);
        let result = select_scored(&registry, MediaOp::GenerateImage, &empty_params());
        assert!(result.is_err());
    }

    #[test]
    fn atlascloud_lower_than_fal() {
        let registry = ProviderRegistry::new(vec![
            Arc::new(MockProvider {
                id: "fal.ai",
                supported: vec![MediaOp::GenerateImage],
            }),
            Arc::new(MockProvider {
                id: "atlascloud",
                supported: vec![MediaOp::GenerateImage],
            }),
        ]);

        let (chosen, scores) =
            select_scored(&registry, MediaOp::GenerateImage, &empty_params()).unwrap();
        assert_eq!(chosen.id(), "fal.ai");
        let fal_score = scores.iter().find(|s| s.id == "fal.ai").unwrap().weighted;
        let ac_score = scores
            .iter()
            .find(|s| s.id == "atlascloud")
            .unwrap()
            .weighted;
        assert!(
            fal_score > ac_score,
            "Fal should score higher than AtlasCloud"
        );
    }
}
