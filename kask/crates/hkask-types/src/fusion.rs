//! Fusion configuration types — shared across hkask-templates, hkask-inference, and hkask-types.
//!
//! These types define the multi-model deliberation configuration:
//! - `FusionMode` — how the judge processes panel responses
//! - `FusionSkill` — methodology anchors injected into the judge's system context
//! - `FusionConfig` — the full configuration (judge, panel, mode, skills, max_rounds)
//!
//! Lives in hkask-types (not hkask-inference) so manifests and LLMParameters
//! can carry per-manifest and per-call fusion overrides without a dependency
//! on the inference crate.

use serde::{Deserialize, Serialize};
use std::ops::Deref;

/// Judge deliberation mode for fusion orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FusionMode {
    /// Pick the single best panel response. No synthesis.
    #[serde(rename = "best-of-n")]
    BestOfN,
    /// Compose a unified response incorporating best elements from all panelists.
    #[serde(rename = "synthesis")]
    #[default]
    Synthesis,
    /// 2-round: draft → panel critique → revised final.
    #[serde(rename = "critique")]
    Critique,
    /// Multi-round deliberation with convergence check (up to max_rounds).
    #[serde(rename = "deliberation")]
    Deliberation,
    /// 2-phase Plan-Implement: Phase 1 synthesizes strategy, Phase 2 synthesizes execution plan.
    #[serde(rename = "pi")]
    PlanImplement,
}

crate::enum_snake_str!(FusionMode, {
    BestOfN => "best-of-n",
    Synthesis => "synthesis",
    Critique => "critique",
    Deliberation => "deliberation",
    PlanImplement => "pi",
});

/// Stabilization verdict on whether a `deliberation`-mode round has converged.
///
/// Replaces the former `FOLLOW_UP:` string-prefix self-report: the judge emits
/// a structured verdict (`{"converged": bool, "synthesis"|"follow_up": …}`) parsed
/// structurally, not a prose prefix. Convergence is a stabilization report (has
/// the panel stopped diverging?), not a correctness claim.
///
/// expect: "Deliberation converges when the judge reports stabilization, parsed structurally"
/// [P9] Motivating: Homeostatic Self-Regulation — closed-loop convergence detection
/// pre:  produced by `parse_convergence_verdict` from the judge's structured-verdict response
/// post: `Converged` → judge synthesizes a final answer; `Continue` → judge emits a follow-up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvergenceVerdict {
    /// Panel responses have stabilized; the judge should synthesize a final answer.
    Converged,
    /// Responses still diverge; the judge should produce a follow-up question.
    Continue,
}

impl ConvergenceVerdict {
    /// Canonical lowercase string for span fields and logging.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConvergenceVerdict::Converged => "converged",
            ConvergenceVerdict::Continue => "continue",
        }
    }
}

/// Algorithmic merge strategy for the `algo` / no-judge family.
///
/// Selects which deterministic merge runs when `fusion.judge == "algo"`.
/// Adding a variant requires a matching arm in the orchestrator's algo dispatch
/// and a merge implementation — variants without implementations are
/// prohibited (P5: no stubs).
///
/// expect: "The algo judge family supports multiple deterministic merge strategies"
/// [P5] Motivating: Essentialism & Minimalism — each method earns its variant
/// pre:  only used when `fusion.judge == "algo"`
/// post: selects the merge function applied to panel JSON responses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AlgoMethod {
    /// Recursive JSON union: objects merge by key, arrays dedup, diverging
    /// strings annotated `[A:... B:...]`. Designed for 2-model panels.
    #[serde(rename = "merge")]
    #[default]
    Merge,
    /// Majority vote: scalar fields take the value appearing in a majority of
    /// panelists; array items kept only if a majority of panelists include them.
    /// Scales beyond 2 panelists where `Merge`'s pairwise annotation degrades.
    #[serde(rename = "vote")]
    Vote,
}

crate::enum_snake_str!(AlgoMethod, {
    Merge => "merge",
    Vote => "vote",
});

/// Skill bundle that anchors the judge's reasoning framework.
/// Each skill injects a compact methodology prompt into the judge's system context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FusionSkill {
    #[serde(rename = "pragmatic-semantics")]
    PragmaticSemantics,
    #[serde(rename = "pragmatic-cybernetics")]
    PragmaticCybernetics,
    #[serde(rename = "coding-guidelines")]
    CodingGuidelines,
    #[serde(rename = "deep-module")]
    DeepModule,
    #[serde(rename = "essentialist")]
    Essentialist,
    #[serde(rename = "superforecasting")]
    Superforecasting,
    #[serde(rename = "mcda")]
    Mcda,
    #[serde(rename = "tdd")]
    TestDrivenDevelopment,
    #[serde(rename = "bug-hunt")]
    BugHunt,
    #[serde(rename = "diagnose")]
    Diagnose,
    #[serde(rename = "falsifiability")]
    Falsifiability,
    #[serde(rename = "grill-me")]
    GrillMe,
    #[serde(rename = "idiomatic-rust")]
    IdiomaticRust,
    #[serde(rename = "refactor-architecture")]
    RefactorArchitecture,
    #[serde(rename = "metacognition")]
    Metacognition,
}

crate::enum_snake_str!(FusionSkill, {
    PragmaticSemantics => "pragmatic-semantics",
    PragmaticCybernetics => "pragmatic-cybernetics",
    CodingGuidelines => "coding-guidelines",
    DeepModule => "deep-module",
    Essentialist => "essentialist",
    Superforecasting => "superforecasting",
    Mcda => "mcda",
    TestDrivenDevelopment => "tdd",
    BugHunt => "bug-hunt",
    Diagnose => "diagnose",
    Falsifiability => "falsifiability",
    GrillMe => "grill-me",
    IdiomaticRust => "idiomatic-rust",
    RefactorArchitecture => "refactor-architecture",
    Metacognition => "metacognition",
});

/// Configuration for fusion multi-model deliberation.
///
/// Provider-agnostic: hKask orchestrates the fusion itself by sending
/// the prompt to all panel models in parallel, collecting responses,
/// then having the judge operate in the configured mode.
///
/// When carried in `LLMParameters.fusion_config`, overrides the global
/// fusion config for that specific inference call.
/// When carried in `BundleManifest.fusion`, provides a per-manifest
/// fusion config for all steps in that skill's pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionConfig {
    /// The judge/fuser model that orchestrates and synthesizes the fusion.
    /// Supports provider prefix routing (e.g., "KC/z-ai/glm-5.2").
    pub judge: String,
    /// The panel of analysis models that answer in parallel.
    /// Each model supports provider prefix routing.
    pub panel: NonEmptyVec<String>,
    /// Judge deliberation mode. Default: Synthesis.
    #[serde(default)]
    pub mode: FusionMode,
    /// Skills that anchor the judge's reasoning framework.
    /// Default: empty (no skill anchoring).
    #[serde(default)]
    pub skills: Vec<FusionSkill>,
    /// Max rounds for deliberation mode. Default: 5.
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,
    /// Algorithmic merge strategy when `judge == "algo"`. Default: `merge`.
    /// Ignored when the judge is an LLM model name.
    #[serde(default)]
    pub algo_method: AlgoMethod,
}

fn default_max_rounds() -> u32 {
    5
}

impl FusionConfig {
    /// Return the kask default fusion configuration.
    ///
    /// Reads judge model from `HKASK_FUSION_JUDGE_MODEL` (default: `KC/z-ai/glm-5.2`)
    /// and panel models from `HKASK_FUSION_PANEL_MODELS` (comma-separated,
    /// default: `"Kimi2.7,Qwen3.7 Max,GLM5.2,Minimax3"`).
    pub fn kask_default() -> Self {
        let judge = std::env::var("HKASK_FUSION_JUDGE_MODEL")
            .unwrap_or_else(|_| "KC/z-ai/glm-5.2".to_string());
        let panel: Vec<String> = std::env::var("HKASK_FUSION_PANEL_MODELS")
            .unwrap_or_else(|_| "Kimi2.7,Qwen3.7 Max,GLM5.2,Minimax3".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            judge,
            panel: NonEmptyVec::from_vec(panel).expect("default panel models must not be empty"),
            mode: FusionMode::Synthesis,
            skills: Vec::new(),
            max_rounds: 5,
            algo_method: AlgoMethod::default(),
        }
    }

    /// The model ID to use when fusion is active (judge model).
    #[must_use]
    pub fn model_id(&self) -> String {
        self.judge.clone()
    }

    /// Human-readable description of the fusion setup.
    #[must_use]
    pub fn description(&self) -> String {
        let skills_str = if self.skills.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = self.skills.iter().map(FusionSkill::as_str).collect();
            format!(" [{}]", names.join(", "))
        };
        format!(
            "{} panel models judged by {} (mode: {}){}",
            self.panel.len(),
            self.judge,
            self.mode.as_str(),
            skills_str
        )
    }
}

// ── NonEmptyVec ──────────────────────────────────────────────────────────────

/// A vector that is guaranteed to contain at least one element.
///
/// Makes the "non-empty" invariant unrepresentable by the type system,
/// preventing construction of `FusionConfig` with an empty panel.
///
/// Serializes transparently as `Vec<T>`; deserializes by validating non-emptiness
/// and returns a serde error if the collection is empty.
#[derive(Debug, Clone)]
pub struct NonEmptyVec<T>(Vec<T>);

impl<T> NonEmptyVec<T> {
    /// Construct from a first element and optional rest.
    #[must_use]
    pub fn new(first: T, rest: Vec<T>) -> Self {
        let mut v = Vec::with_capacity(rest.len() + 1);
        v.push(first);
        v.extend(rest);
        NonEmptyVec(v)
    }

    /// Construct a single-element `NonEmptyVec`.
    #[must_use]
    pub fn one(t: T) -> Self {
        NonEmptyVec(vec![t])
    }

    /// Convert from a `Vec`, returning `None` if empty.
    #[must_use]
    pub fn from_vec(v: Vec<T>) -> Option<Self> {
        if v.is_empty() {
            None
        } else {
            Some(NonEmptyVec(v))
        }
    }
}

impl<T> Deref for NonEmptyVec<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Serialize> Serialize for NonEmptyVec<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for NonEmptyVec<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<T>::deserialize(d)?;
        NonEmptyVec::from_vec(v)
            .ok_or_else(|| serde::de::Error::custom("collection must not be empty"))
    }
}
