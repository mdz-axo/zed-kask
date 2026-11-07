//! Model brief — the per-model data layer (M9's deterministic core).
//!
//! A model brief is a TOML file in `kask/registry/model-briefs/` carrying
//! the model-level policy that the GGUF parse cannot infer: the PLE pin
//! pattern (which tensor family stays on CPU during GPU offload), the
//! expert count (for the ExpertSlotFloor guard), and the recommended
//! KV type and context length. The placement compiler composes the
//! brief with the live hardware probe and the engine manifest.
//!
//! The brief is the **data** counterpart to the engine manifest: the
//! manifest carries engine drift (flag spellings, constraints), the
//! brief carries model drift (tensor patterns, expert counts). Both
//! are versioned data; the compiler is the stable Rust decision core.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Model-brief decode and registry-load failures — per-variant, never a
/// blanket String (the string-errors gate; the `KeystoreError` pattern:
/// every failure state a caller can distinguish maps to exactly one
/// variant).
#[derive(Debug, Error)]
pub enum ModelBriefError {
    /// The TOML does not match the brief shape (missing tables, wrong
    /// types, unknown fields).
    #[error("model brief decode failed: {source}")]
    Decode { source: toml::de::Error },
}

/// A per-model brief. Every field is a declared, sourced number with
/// provenance (the source GGUF repo and verification date).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBrief {
    /// Stable model identifier (matches the GGUF's `general.name`).
    pub model_id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Model-level metadata (the [model] table).
    pub model: ModelMetadata,
    /// Placement metadata (the [placement] table).
    pub placement: PlacementMetadata,
    /// Recommended settings (the [recommendations] table).
    pub recommendations: Recommendations,
    /// Speculative-draft spec (the optional `[draft]` table) — None on
    /// models that ship no draft module.
    pub draft: Option<DraftSpec>,
}

/// Model metadata (the [model] table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadata {
    /// Total parameters (the GGUF's total parameter count).
    pub total_params: u64,
    /// Active parameters per token (MoE: the active slice; dense: same
    /// as total_params).
    pub active_params: u64,
    /// Routed experts (MoE: the expert pool size; dense: 0).
    pub expert_count: u32,
    /// Experts used per token (MoE: the per-token routing count; dense: 0).
    pub expert_used_count: u32,
}

/// Placement metadata (the [placement] table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementMetadata {
    /// The tensor-name prefix that matches the model's PLE/n-gram
    /// table family (e.g. "per_layer_token_embd"). When non-empty,
    /// the compiler emits `--override-tensor <pattern>=CPU` to keep
    /// the PLE table on the host while layers offload to the GPU.
    /// Dense models have no PLE: the pattern is empty.
    pub ple_pin_pattern: String,
}

/// Recommended settings (the [recommendations] table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommendations {
    /// The recommended KV cache type (e.g. "q8_0", "f16").
    pub kv_cache_type: String,
    /// Whether flash-attention is recommended (D1: required for
    /// quantized KV).
    pub flash_attn: bool,
    /// The recommended context length.
    pub context_length: u32,
}

/// Speculative-decoding draft spec (the optional `[draft]` table) —
/// per-model data: which draft module ships with the model, which
/// speculative mode it serves, the recommended draft depth, and the
/// module's byte size (the guard and placement arithmetic consume it).
/// Absent on models that ship no draft module — the policy's draft ask
/// against a brief without this table is a typed refusal, never an
/// invented draft.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftSpec {
    /// The draft module's path RELATIVE to the model GGUF's directory
    /// (the same-dir convention: `MTP/<file>` inside the shard dir).
    pub module_file: String,
    /// The speculative mode the draft serves (e.g. "draft-mtp") — the
    /// engine's `--spec-type` value, spelled here because the mode is a
    /// property of the draft module, not the machine.
    pub spec_type: String,
    /// The recommended `--spec-draft-n-max` (unsloth's guidance: start
    /// at 2, tune 1-6 per hardware).
    pub draft_n_max: u32,
    /// The draft module's file size in bytes — the guard and placement
    /// arithmetic consume it.
    pub draft_bytes: u64,
    /// zed-kask: MTP record §7 item 1 (2026-11-06) — the spec-decode
    /// VERIFY workspace in bytes: GPU-side whenever a draft is ASKED,
    /// regardless of the draft module's own placement (a CPU-routed
    /// draft still runs speculative verification in GPU memory — the D-B
    /// finding: the verify path OOM'd ctx 65536 on the 8 GB card with
    /// 0.9-1.2 GB free, under-reserved by exactly this term). Unsloth's
    /// guidance: prepare 1-2 GB extra headroom; the brief declares the
    /// conservative upper bound. `0` (the serde default, also the value
    /// for briefs predating the field) = no reserve — the pre-§7
    /// arithmetic.
    #[serde(default)]
    pub verify_workspace_bytes: u64,
}

impl ModelBrief {
    /// Decode a model brief from TOML. One decode home — every
    /// consumer routes through here.
    pub fn decode(toml_text: &str) -> Result<Self, ModelBriefError> {
        toml::from_str(toml_text).map_err(|source| ModelBriefError::Decode { source })
    }

    /// Whether this model is sparse (MoE with routed experts).
    pub fn is_moe(&self) -> bool {
        self.model.expert_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QWEN_BRIEF_TOML: &str =
        include_str!("../../../../registry/model-briefs/qwen3-8-flash-next.toml");

    const GEMMA_BRIEF_TOML: &str =
        include_str!("../../../../registry/model-briefs/gemma-4-31b.toml");

    #[test]
    fn decodes_the_qwen_brief() {
        let brief = ModelBrief::decode(QWEN_BRIEF_TOML).expect("Qwen brief decodes");
        assert_eq!(brief.model_id, "qwen3-8-flash-next");
        assert!(brief.is_moe());
        assert_eq!(brief.model.expert_count, 512);
        assert_eq!(brief.model.expert_used_count, 10);
        assert_eq!(brief.placement.ple_pin_pattern, "per_layer_token_embd");
        assert_eq!(brief.recommendations.kv_cache_type, "q8_0");
        assert!(brief.recommendations.flash_attn);
        let draft = brief
            .draft
            .as_ref()
            .expect("qwen brief carries a draft spec");
        assert_eq!(
            draft.module_file,
            "MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf"
        );
        assert_eq!(draft.spec_type, "draft-mtp");
        assert_eq!(draft.draft_n_max, 2);
        assert_eq!(draft.draft_bytes, 2_786_568_256);
        assert_eq!(
            draft.verify_workspace_bytes, 2_147_483_648,
            "the §7 item-1 verify workspace is declared at unsloth's conservative upper bound"
        );
    }

    #[test]
    fn decodes_the_gemma_brief() {
        let brief = ModelBrief::decode(GEMMA_BRIEF_TOML).expect("Gemma brief decodes");
        assert!(!brief.is_moe());
        assert_eq!(brief.placement.ple_pin_pattern, "");
        assert!(brief.draft.is_none());
    }
}
