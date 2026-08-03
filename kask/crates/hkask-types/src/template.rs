//! Template types — Loop 1 (Inference): template rendering and invocation
//
//! Templates are the primary interface for the Inference loop. The registry
//! stores them; Inference renders them; Curation evaluates their output.

use serde::{Deserialize, Serialize};

/// LLMParameters — Full parameter set for LLM invocation
/// Loop: Inference
///
/// Temperature is primary. Other parameters support.
/// Temperature breaks the pattern. Other parameters vary the break.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMParameters {
    /// Temperature: primary control for randomness (0.0-1.0)
    /// - Low (0.1-0.3): deterministic, optimal, normative
    /// - High (0.7-0.9): random, suboptimal, creative
    pub temperature: f32,

    /// Top-p (nucleus sampling): cumulative probability threshold (0.0-1.0)
    /// - Lower: more focused
    /// - Higher: more diverse
    pub top_p: f32,

    /// Top-k: sample from top k tokens (1-100)
    /// - Lower: safer
    /// - Higher: more surprising
    pub top_k: u32,

    /// Frequency penalty: penalize repetition (-2.0 to 2.0)
    /// - Higher: more varied vocabulary
    pub frequency_penalty: f32,

    /// Presence penalty: penalize familiar tokens (-2.0 to 2.0)
    /// - Higher: more novel concepts
    pub presence_penalty: f32,

    /// Min-p: minimum probability threshold for token sampling (0.0-1.0)
    /// - Tokens below `min_p * max_prob` are filtered out
    /// - 0.0 disables (no minimum). Typical: 0.02-0.1
    pub min_p: f32,

    /// Typical-p (locally typical sampling): entropy-centered threshold (0.0-1.0)
    /// - Selects tokens whose negative log-prob is close to the distribution's entropy
    /// - Filters both high-prob (too obvious) and low-prob (too surprising) extremes
    /// - 0.0 disables. Typical: 0.9-0.95
    pub typical_p: f32,

    /// Maximum tokens to generate
    pub max_tokens: u32,

    /// Random seed (None for random, Some for reproducibility)
    pub seed: Option<u64>,

    /// Disable thinking/reasoning mode for models that support it (e.g., qwen3).
    /// When true, the model is instructed to skip internal reasoning and produce
    /// output directly. Essential for summarization/condensation tasks where
    /// output tokens are needed, not reasoning tokens.
    /// Default: false (thinking enabled). Set to true for condenser tasks.
    #[serde(default)]
    pub disable_thinking: bool,

    /// LoRA adapter to apply at inference time (for multi-LoRA serving).
    /// When set, this COMPLETELY OVERRIDES the model — it is the full model
    /// identifier including the base model. The adapter was trained on a specific
    /// base model and cannot be applied to a different one.
    ///
    /// Format: `"Qwen3.5-9B#pragmatic-semantics-v1"` (multi-LoRA)
    ///         `"accounts/together/models/my-model"` (Together AI fine-tuned)
    ///
    /// The caller is responsible for resolving which base model the adapter
    /// was trained on (via AdapterStore lookup by skill_name).
    /// Default: None (use default model without adapter).
    #[serde(default)]
    pub adapter: Option<String>,

    /// System prompt for the chat request. When present, sent as a
    /// `{"role": "system"}` message before the user message. Used by
    /// inference paths that need few-shot examples as a proper system
    /// message rather than prepending to user content.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

impl LLMParameters {
    /// Edge work preset: moderate anti-normative parameters
    /// Temperature: 0.6, top_p: 0.85, top_k: 35, freq: 0.4, presence: 0.4
    pub(crate) fn edge_work() -> Self {
        Self {
            temperature: 0.6,
            top_p: 0.85,
            top_k: 35,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.4,
            presence_penalty: 0.4,
            max_tokens: 2048,
            seed: None,
            disable_thinking: false,
            adapter: None,
            system_prompt: None,
        }
    }
}

impl Default for LLMParameters {
    fn default() -> Self {
        Self::edge_work()
    }
}
