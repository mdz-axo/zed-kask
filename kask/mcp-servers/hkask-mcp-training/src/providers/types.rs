//! Training provider types — enums, params, traits, and error types.
//!
//! This module contains the foundational types for the training provider system:
//! - Host/harness identifiers
//! - Training job representation
//! - Hyperparameter types
//! - The `TrainingHost` trait
//! - Error and metadata types
//! - Cost estimation

use crate::huggingface::TrainingArtifacts;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

// ── Training harness identifiers ─────────────────────────────────────────────

/// Training harnesses — the tooling that runs on top of a host.
///
/// This is the *harness* layer (Axolotl tooling), distinct from the
/// *host* layer (where compute runs: Runpod) and the *base model* layer
/// (what model is fine-tuned: Qwen/Gemma/Mistral).
///
/// Each variant represents a training framework that produces LoRA adapters.
/// All training runs on cloud hosts — there is no local training path.
///
/// Harness selection is driven by the `lora-training` skill's G6 gate
/// (harness capability), which recommends a harness from declared evidence.
/// The operator accepts, overrides, or rejects; the runtime enforces
/// harness-method compatibility (G-H1) via `lora_validation.rs`.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrainingHarnessId {
    /// axolotl — YAML-based training framework, dispatched to Runpod.
    /// Supports SFT only (no preference optimization). Mature, single-file config.
    Axolotl,
    /// trl — HuggingFace TRL Python library, dispatched to Runpod.
    /// Supports SFT (SFTTrainer) and preference optimization (DPO/KTO/ORPO/Reward).
    /// All trainers are implemented: SFT, DPO, KTO, ORPO, Reward.
    Trl,
    /// ludwig — declarative YAML deep-learning framework (Linux Foundation AI & Data),
    /// dispatched to Runpod. Supports SFT, DPO/KTO/ORPO, and GRPO (reward-model-free
    /// RLHF) — the only harness in the candidate set covering GRPO. Also covers
    /// advanced PEFT initializers (PiSSA, EVA, CorDA, LoftQ) that hKask's `LoraInit`
    /// enum declares but Axolotl cannot render. Added v0.31.0.
    /// Source: https://ludwig.ai/latest/ · https://github.com/ludwig-ai/ludwig
    Ludwig,
}

impl TrainingHarnessId {
    /// Parse from a config string (case-insensitive).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "axolotl" => Some(Self::Axolotl),
            "trl" => Some(Self::Trl),
            "ludwig" => Some(Self::Ludwig),
            _ => None,
        }
    }
}

// ── TRL trainer identifiers ───────────────────────────────────────────────

/// TRL trainer selection — only meaningful when `harness = Trl`.
///
/// Mirrors the TRL trainer taxonomy (https://huggingface.co/docs/trl/index).
/// Each variant maps to a TRL trainer class + config class pair.
/// Add trainers as concrete needs emerge (P7 — evolutionary architecture).
///
/// All trainers are implemented: `Sft`, `Dpo`, `Kto`, `Orpo`, `Reward`.
/// Deferred: online RL trainers (GRPO, RLOO, PPO, OnlineDPO, NashMD, XPO) —
///   require vLLM co-location and sandboxed environments; add when a concrete
///   RL use case emerges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrlTrainer {
    /// TRL `SFTTrainer` + `SFTConfig` — supervised fine-tuning.
    /// The canonical SFT path; parallel to Axolotl's SFT support.
    /// Supports: packing, assistant_only_loss, completion_only_loss, VLMs.
    /// Data format: ChatML `{"messages": [...]}` or prompt-completion.
    #[default]
    Sft,
    /// TRL `DPOTrainer` + `DPOConfig` — Direct Preference Optimization.
    /// Offline preference optimization from paired chosen/rejected data.
    /// Data format: `{"prompt": ..., "chosen": ..., "rejected": ...}`.
    /// Source: arXiv:2305.18290.
    Dpo,
    /// TRL `KTOTrainer` + `KTOConfig` — Kahneman-Tversky Optimization.
    /// Unpaired binary preference data (good/bad labels, no pairing needed).
    /// Data format: `{"prompt": ..., "completion": ..., "label": bool}`.
    /// Source: arXiv:2402.01306.
    Kto,
    /// TRL `ORPOTrainer` + `ORPOConfig` — Odds Ratio Preference Optimization.
    /// Single-stage SFT + preference alignment in one pass.
    /// Data format: `{"chosen": ..., "rejected": ...}` (prompt implicit).
    /// Source: arXiv:2403.07691.
    Orpo,
    /// TRL `RewardTrainer` + `RewardConfig` — reward model training.
    /// Trains a reward model for RLHF pipelines (needed before PPO/GRPO).
    /// Data format: `{"chosen": ..., "rejected": ...}` (prompt implicit).
    /// Source: https://huggingface.co/docs/trl/main/en/reward_trainer.
    Reward,
    // Deferred: online RL trainers (GRPO, RLOO, PPO, OnlineDPO, NashMD, XPO)
    // — require vLLM co-location and sandboxed environments. Add when a
    // concrete RL use case emerges (P7 — evolutionary architecture).
}

impl TrlTrainer {
    /// Parse from a config string (case-insensitive).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sft" => Some(Self::Sft),
            "dpo" => Some(Self::Dpo),
            "kto" => Some(Self::Kto),
            "orpo" => Some(Self::Orpo),
            "reward" => Some(Self::Reward),
            _ => None,
        }
    }

    /// The TRL trainer class name (e.g., `"SFTTrainer"`).
    pub fn trainer_class(&self) -> &'static str {
        match self {
            Self::Sft => "SFTTrainer",
            Self::Dpo => "DPOTrainer",
            Self::Kto => "KTOTrainer",
            Self::Orpo => "ORPOTrainer",
            Self::Reward => "RewardTrainer",
        }
    }

    /// The TRL config class name (e.g., `"SFTConfig"`).
    pub fn config_class(&self) -> &'static str {
        match self {
            Self::Sft => "SFTConfig",
            Self::Dpo => "DPOConfig",
            Self::Kto => "KTOConfig",
            Self::Orpo => "ORPOConfig",
            Self::Reward => "RewardConfig",
        }
    }

    /// The expected dataset format for this trainer (for G-H1 validation).
    pub fn expected_dataset_format(&self) -> &'static str {
        match self {
            Self::Sft => "chatml or prompt-completion",
            Self::Dpo => "preference (prompt + chosen + rejected)",
            Self::Kto => "unpaired preference (prompt + completion + label)",
            Self::Orpo => "preference (chosen + rejected)",
            Self::Reward => "preference (chosen + rejected)",
        }
    }
}

// ── Host identifiers ─────────────────────────────────────────────────────────

/// Training hosts — where the GPU compute runs.
///
/// This is the *host* layer (cloud only — no local training), distinct from the
/// *harness* layer (Axolotl tooling) and the *base model* layer
/// (Qwen/Gemma/Mistral/etc.). Each variant represents a cloud backend that
/// executes training jobs.
///
/// Host → Harness mapping:
///   Runpod → Axolotl
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingHostId {
    /// runpod — Runpod GPU cloud training, pod-based axolotl dispatch
    Runpod,
    /// deepinfra — DeepInfra dedicated GPU containers with SSH access
    DeepInfra,
    /// nebius — Nebius AI Cloud VMs with H100/H200/B200 GPUs
    Nebius,
}

impl TrainingHostId {
    /// Parse from a config string (case-insensitive).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "runpod" => Some(Self::Runpod),
            "deepinfra" => Some(Self::DeepInfra),
            "nebius" => Some(Self::Nebius),
            _ => None,
        }
    }
}

// ── Job/Adapter types ──────────────────────────────────────────────────────

/// The canonical representation of a training job, provider-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingJob {
    /// Unique job identifier (UUIDv4).
    pub id: String,
    /// Path to the preprocessed dataset file.
    pub dataset_path: PathBuf,
    /// Base model identifier (provider-prefixed, e.g., "OM/qwen3:8b").
    pub base_model: String,
    /// Training hyperparameters.
    pub params: TrainingParams,
    /// Current job status.
    pub status: TrainingJobStatus,
    /// Job creation timestamp.
    #[serde(with = "chrono::serde::ts_seconds")]
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Host executing this job.
    pub host: TrainingHostId,
    /// Harness (training framework) to use.
    pub harness: TrainingHarnessId,
    /// User/owner WebID for provenance.
    #[serde(default)]
    pub owner: Option<String>,
    /// Skill name for retraining — when set, enables A/B comparison on completion.
    /// Semantic/generic fine-tuning leaves this `None`.
    #[serde(default)]
    pub skill_name: Option<String>,
    /// Estimated cost of this training job in micro-rJoules (µrJ).
    /// Computed from host provider pricing + training epochs.
    /// Not optional — always computed at job creation time.
    #[serde(default)]
    pub estimated_cost_urj: u64,
    /// Immutable input/output artifacts for RunPod training.
    #[serde(default)]
    pub artifacts: Option<TrainingArtifacts>,
}

impl TrainingJob {
    pub fn new(
        dataset_path: PathBuf,
        base_model: String,
        params: TrainingParams,
        host: TrainingHostId,
        harness: TrainingHarnessId,
    ) -> Self {
        let cost = estimate_training_cost_urj(&host, params.num_epochs, &base_model);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            dataset_path,
            base_model,
            params,
            status: TrainingJobStatus::Queued,
            created_at: chrono::Utc::now(),
            host,
            harness,
            owner: None,
            skill_name: None,
            estimated_cost_urj: cost,
            artifacts: None,
        }
    }
}

/// Estimate training cost in micro-rJoules (µrJ) from host provider pricing.
///
/// Uses host-specific base rates multiplied by epoch count and model size.
pub(crate) fn estimate_training_cost_urj(
    host: &TrainingHostId,
    num_epochs: u32,
    base_model: &str,
) -> u64 {
    let base_per_epoch: u64 = match host {
        TrainingHostId::Runpod => 500_000, // ~$0.50/epoch (H100 @ $2.39/hr)
        TrainingHostId::DeepInfra => 740_000, // ~$0.74/epoch (B200 @ $3.69/hr)
        TrainingHostId::Nebius => 650_000, // ~$0.65/epoch (H100 @ $3.85/hr)
    };
    let size_mult = extract_model_size_multiplier(base_model);
    base_per_epoch * (num_epochs as u64) * size_mult
}

/// Extract size multiplier from model identifier string.
pub(crate) fn extract_model_size_multiplier(base_model: &str) -> u64 {
    let lower = base_model.to_lowercase();
    for (pattern, mult) in &[
        ("8b", 1),
        ("9b", 1),
        ("7b", 1),
        ("3b", 1),
        ("1b", 1),
        ("13b", 2),
        ("14b", 2),
        ("20b", 2),
        ("26b", 2),
        ("30b", 2),
        ("70b", 4),
        ("72b", 4),
        ("120b", 4),
        ("405b", 4),
        ("8x7b", 2),
        ("8x22b", 4),
    ] {
        if lower.contains(pattern) {
            return *mult;
        }
    }
    2 // Default: assume medium size
}

/// LoRA-specific training parameters.
///
/// Field set mirrors the subset of PEFT `LoraConfig` fields that axolotl
/// can render to YAML (see `AxolotlHarness::render_config`). Fields not
/// renderable by axolotl (e.g., `lora_ga_config`, `corda_config`,
/// `eva_config`, `alora_invocation_tokens`) are intentionally absent —
/// adding them would create a phantom config surface that the harness
/// can't consume.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoraParams {
    /// LoRA rank (r value). Typical range: 4–64.
    pub r: u32,
    /// LoRA alpha scaling factor.
    pub alpha: u32,
    /// LoRA dropout rate. 0.0 is the standard for QLoRA training.
    #[serde(default)]
    pub dropout: f32,
    /// Target modules for LoRA adaptation.
    pub target_modules: Vec<String>,
    /// Additional modules to save fully (e.g., ["embed_tokens", "lm_head"]).
    #[serde(default)]
    pub modules_to_save: Vec<String>,
    /// Use Rank-Stabilized LoRA (scales by alpha/sqrt(r)).
    /// Axolotl YAML: `peft_use_rslora: true`.
    #[serde(default)]
    pub use_rslora: bool,
    /// Use Weight-Decomposed Low-Rank Adaptation (DoRA).
    /// Axolotl YAML: `peft_use_dora: true`.
    /// Requires PEFT >= 0.10 for correct magnitude folding during merge.
    #[serde(default)]
    pub use_dora: bool,
    /// How to initialize LoRA weights. PEFT default is `true` (B=0, A~Gaussian
    /// — adapter is a no-op at step 0). Other values: `"gaussian"`,
    /// `"pissa"`, `"pissa_niter_N"`, `"loftq"`, `"olora"`, `"corda"`,
    /// `"eva"`, `"orthogonal"`, `false`.
    /// Axolotl YAML: `peft_init_lora_weights: <value>`.
    /// Non-default values may require preprocessing (e.g., `preprocess_loraga`).
    #[serde(default)]
    pub init_lora_weights: Option<LoraInit>,
    /// Bias type for LoRA. `"none"` (default) is the only safe setting for
    /// must-merge inference. `"all"` and `"lora_only"` break merge equivalence.
    /// Axolotl YAML: emitted via `peft:` config block.
    #[serde(default)]
    pub bias: LoraBias,
}

/// LoRA initialization strategy (mirrors PEFT `init_lora_weights`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoraInit {
    /// PEFT default: B=0, A~Gaussian Kaiming-uniform. Adapter is a no-op at step 0.
    #[default]
    Default,
    /// Gaussian initialization scaled by rank.
    Gaussian,
    /// PiSSA: SVD of base weight. Requires `subtract_mutated_init` for merge.
    Pissa,
    /// PiSSA with Fast-SVD: `pissa_niter_N` where N is iterations (e.g., 16).
    PissaNiter(u32),
    /// LoftQ: quantization-error-minimizing init. Requires `replace_lora_weights_loftq`.
    Loftq,
    /// OLoRA: orthogonal init.
    Olora,
    /// CorDA: context-oriented init. May be Knowledge-Preserved or Instruction-Previewed.
    Corda,
    /// Orthogonal init (like OLoRA but base weights untouched). Requires even r.
    Orthogonal,
    /// EVA: activation-vector SVD init (Paischer et al. 2024, arXiv:2410.07170).
    /// Initializes A with principal activation directions, B=0. Produces standard
    /// portable LoRA adapters (unlike PiSSA which is weight-SVD based and
    /// non-portable across transformers versions). No-op at init (B=0 → ΔW=0).
    /// Uses the training data for activation-SVD initialization.
    Eva,
    /// Random init (debugging only — not a no-op at step 0).
    Random,
}

impl LoraInit {
    /// Render to the PEFT/axolotl YAML value.
    pub fn as_config_value(&self) -> String {
        match self {
            Self::Default => "true".to_string(),
            Self::Gaussian => "gaussian".to_string(),
            Self::Pissa => "pissa".to_string(),
            Self::PissaNiter(n) => format!("pissa_niter_{}", n),
            Self::Loftq => "loftq".to_string(),
            Self::Olora => "olora".to_string(),
            Self::Corda => "corda".to_string(),
            Self::Orthogonal => "orthogonal".to_string(),
            Self::Eva => "eva".to_string(),
            Self::Random => "false".to_string(),
        }
    }

    /// Whether this init strategy modifies base weights (requires explicit save handling).
    pub fn modifies_base_weights(&self) -> bool {
        matches!(
            self,
            Self::Pissa | Self::PissaNiter(_) | Self::Loftq | Self::Olora | Self::Corda
        )
    }

    /// Whether the adapter is a no-op at step 0 (ΔW = 0).
    pub fn is_noop_at_init(&self) -> bool {
        matches!(self, Self::Default | Self::Eva)
    }
}

/// LoRA bias type (mirrors PEFT `bias`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LoraBias {
    /// No bias trained (default). Safe for merge.
    #[default]
    None,
    /// Train bias on all layers. Breaks merge equivalence.
    All,
    /// Train bias only on LoRA layers. Breaks merge equivalence.
    LoraOnly,
}

impl LoraBias {
    /// Whether this bias setting breaks merge equivalence.
    pub fn breaks_merge(&self) -> bool {
        matches!(self, Self::All | Self::LoraOnly)
    }
}

impl Default for LoraParams {
    fn default() -> Self {
        Self {
            r: 16,
            alpha: 32,
            // dropout=0 is the standard for QLoRA training; non-zero can degrade quality.
            dropout: 0.0,
            target_modules: vec![
                "q_proj".to_string(),
                "v_proj".to_string(),
                "k_proj".to_string(),
                "o_proj".to_string(),
                "gate_proj".to_string(),
                "up_proj".to_string(),
                "down_proj".to_string(),
            ],
            modules_to_save: vec![],
            use_rslora: false,
            use_dora: false,
            init_lora_weights: None, // None = PEFT default (true)
            bias: LoraBias::None,
        }
    }
}

/// Quantization parameters for QLoRA training.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct QuantizationParams {
    /// Load base model in 4-bit (QLoRA).
    #[serde(default)]
    pub load_in_4bit: bool,
    /// Load base model in 8-bit.
    #[serde(default)]
    pub load_in_8bit: bool,
    /// 4-bit compute dtype ("bf16", "fp16", "fp32").
    #[serde(default)]
    pub bnb_4bit_compute_dtype: Option<String>,
    /// 4-bit quantization type ("nf4", "fp4").
    #[serde(default)]
    pub bnb_4bit_quant_type: Option<String>,
    /// Use double quantization for additional memory savings.
    #[serde(default)]
    pub bnb_4bit_use_double_quant: bool,
}

/// Optimization and scheduler parameters.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OptimizationParams {
    /// Optimizer name ("adamw_torch", "adamw_8bit", "adamw_bnb_8bit", "paged_adamw_32bit").
    #[serde(default)]
    pub optimizer: Option<String>,
    /// Weight decay (excludes bias and LayerNorm).
    #[serde(default)]
    pub weight_decay: f32,
    /// Warmup steps (mutually exclusive with warmup_ratio).
    #[serde(default)]
    pub warmup_steps: Option<u32>,
    /// Warmup ratio (fraction of total steps).
    #[serde(default)]
    pub warmup_ratio: Option<f32>,
    /// LR scheduler type ("cosine", "linear", "constant", "constant_with_warmup").
    #[serde(default)]
    pub lr_scheduler: Option<String>,
    /// Gradient accumulation steps (effective batch = batch_size × grad_accum × n_gpu).
    #[serde(default)]
    pub gradient_accumulation_steps: u32,
    /// Cosine minimum LR ratio (e.g., 0.1 for 10% of peak LR).
    #[serde(default)]
    pub cosine_min_lr_ratio: Option<f32>,
    /// Adam beta1.
    #[serde(default)]
    pub adam_beta1: Option<f32>,
    /// Adam beta2.
    #[serde(default)]
    pub adam_beta2: Option<f32>,
    /// Adam epsilon.
    #[serde(default)]
    pub adam_epsilon: Option<f32>,
    /// Max gradient norm for clipping.
    #[serde(default)]
    pub max_grad_norm: Option<f32>,
}

impl Default for OptimizationParams {
    fn default() -> Self {
        Self {
            optimizer: None,
            weight_decay: 0.0,
            warmup_steps: None,
            warmup_ratio: None,
            lr_scheduler: None,
            gradient_accumulation_steps: 1,
            cosine_min_lr_ratio: None,
            adam_beta1: None,
            adam_beta2: None,
            adam_epsilon: None,
            max_grad_norm: None,
        }
    }
}

/// Sequence and packing parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SequenceParams {
    /// Maximum sequence length for training.
    #[serde(default)]
    pub sequence_len: Option<u32>,
    /// Enable sample packing (block-diagonal attention for short sequences).
    #[serde(default)]
    pub sample_packing: bool,
    /// Pad to sequence length (reduces memory fragmentation).
    #[serde(default)]
    pub pad_to_sequence_len: bool,
    /// Enable NEFTune noise (paper default: 5.0).
    #[serde(default)]
    pub neftune_noise_alpha: Option<f32>,
}

/// Advanced attention and memory parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AdvancedParams {
    /// Attention implementation ("flash_attention_2", "sdpa", "eager").
    #[serde(default)]
    pub attn_implementation: Option<String>,
    /// Gradient checkpointing ("true" for standard, "unsloth" for Unsloth-optimized).
    #[serde(default)]
    pub gradient_checkpointing: Option<String>,
    /// Use bf16 mixed precision.
    #[serde(default)]
    pub bf16: bool,
    /// Use fp16 mixed precision.
    #[serde(default)]
    pub fp16: bool,
    /// Evaluation split ratio (fraction of dataset held out for eval).
    #[serde(default)]
    pub eval_split_ratio: Option<f32>,
}

/// Canonical training hyperparameters — the union of capabilities supported by
/// at least two harnesses or required by a specific training mode.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrainingParams {
    /// Number of training epochs.
    pub num_epochs: u32,
    /// Per-device batch size.
    pub batch_size: u32,
    /// Learning rate.
    pub learning_rate: f32,
    /// LoRA-specific parameters.
    #[serde(default)]
    pub lora: LoraParams,
    /// Quantization parameters (QLoRA).
    #[serde(default)]
    pub quantization: QuantizationParams,
    /// Optimizer, scheduler, and gradient parameters.
    #[serde(default)]
    pub optimization: OptimizationParams,
    /// Sequence length and packing parameters.
    #[serde(default)]
    pub sequence: SequenceParams,
    /// Attention, mixed precision, and eval parameters.
    #[serde(default)]
    pub advanced: AdvancedParams,
    /// Selected training harness (operator-accepted from the `lora-training`
    /// skill's G6 gate recommendation). `None` defers to the runtime default
    /// (Axolotl) — preserves existing behavior when no harness is selected.
    ///
    /// Authority: the skill recommends; the operator accepts, overrides, or
    /// rejects; the runtime enforces harness-method compatibility (G-H1).
    #[serde(default)]
    pub harness: Option<TrainingHarnessId>,
    /// TRL trainer selection — only meaningful when `harness = Trl`.
    /// `None` defaults to `Sft` when `harness = Trl`.
    /// Ignored when `harness = Axolotl` or `None`.
    #[serde(default)]
    pub trl_trainer: Option<TrlTrainer>,
}

impl Default for TrainingParams {
    fn default() -> Self {
        Self {
            num_epochs: 3,
            batch_size: 4,
            learning_rate: 1e-4,
            lora: LoraParams::default(),
            quantization: QuantizationParams::default(),
            optimization: OptimizationParams::default(),
            sequence: SequenceParams::default(),
            advanced: AdvancedParams::default(),
            harness: None,
            trl_trainer: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingJobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

// ── Provider error ─────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Provider '{0}' is not available (missing CLI or configuration)")]
    Unavailable(String),
    #[error("Training job failed: {0}")]
    JobFailed(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Dataset error: {0}")]
    DatasetError(String),
    #[error("Provider backend error: {0}")]
    Backend(String),
}

// ── TrainingHost trait ────────────────────────────────────────────────────
//
// Pluggable training host — where a training job runs.
//
// ARCHITECTURAL REQUIREMENT: Every pod MUST be debuggable. The `status`
// method returns `PodStatus` with SSH connection info, IP, uptime, and GPU
// type. Pods without public SSH access are useless for debugging and
// must not be deployed.
//
// The old trait returned bare `TrainingJobStatus` — no SSH info, no pod
// details, no way to debug. That design caused repeated money-wasting
// failures where pods ran for 30+ minutes with no way to inspect them.
// This trait fixes that by requiring full visibility.
//
// The `completion_metadata` and `adapter_weight_path` methods have been
// REMOVED. Completion is detected via the HuggingFace manifest
// (`check_completion_manifest` in `TrainingServer`). Adapter paths come
// from the manifest's `adapter.repository` field.
//
// ── PodStatus ─────────────────────────────────────────────────────────────

/// Rich pod status returned by `TrainingHost::status`. Includes everything
/// an operator needs to monitor, debug, and SSH into a running pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodStatus {
    /// High-level job status.
    pub status: TrainingJobStatus,
    /// Provider-specific pod ID (e.g. RunPod pod ID).
    pub pod_id: String,
    /// SSH command to connect to the pod (e.g. "ssh root@1.2.3.4 -p 12345").
    /// Empty if no public SSH is available — this is a red flag.
    pub ssh_command: String,
    /// Pod IP address (public if available, internal otherwise).
    pub ip: String,
    /// Public SSH port (0 if not available).
    pub ssh_port: u64,
    /// Whether the IP is publicly accessible.
    pub is_public_ip: bool,
    /// Pod uptime in seconds.
    pub uptime_seconds: u64,
    /// GPU type (e.g. "NVIDIA H100 80GB HBM3").
    pub gpu_type: String,
    /// Provider-specific failure reason when status is Failed (e.g. "out of capacity").
    /// None when the pod is running or completed normally.
    pub fail_reason: Option<String>,
}

#[async_trait::async_trait]
pub trait TrainingHost: Send + Sync {
    /// Submit a training job for execution.
    /// Returns a provider-specific job ID for status tracking.
    async fn submit(&self, job: &TrainingJob) -> Result<String, ProviderError>;

    /// Query the status of a previously submitted job.
    /// Returns rich pod status including SSH connection info, IP, uptime,
    /// and GPU type. The operator MUST be able to SSH into the pod and
    /// inspect logs. If `ssh_command` is empty, the pod is not debuggable.
    async fn status(&self, job_id: &str) -> Result<PodStatus, ProviderError>;

    /// Cancel a running or queued job. Terminates the pod to stop billing.
    async fn cancel(&self, job_id: &str) -> Result<(), ProviderError>;
}

/// Metadata returned by a provider when a training job completes.
/// DEPRECATED — completion metadata now comes from the HuggingFace
/// completion manifest (CompletionManifest struct in huggingface.rs).
/// This struct is retained for trait conformance but is not used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionMetadata {
    pub base_model: String,
    pub output_name: Option<String>,
    pub loss: Option<f32>,
    pub training_duration_secs: Option<u64>,
    pub tokens_processed: Option<u64>,
}

/// Fetch the tail of a pod's training log via SSH. Used by training_status
/// to surface real-time progress to the operator without requiring manual SSH.
pub async fn fetch_pod_logs(ssh_command: &str, lines: usize) -> Option<String> {
    if ssh_command.is_empty() {
        return None;
    }
    let parts: Vec<&str> = ssh_command.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let user_host = parts[1];
    let port = parts.get(3).and_then(|p| p.parse::<u16>().ok())?;
    let output = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            "-p",
            &port.to_string(),
            user_host,
            &format!(
                "tail -n {lines} /workspace/logs/entrypoint.log 2>/dev/null || echo no-log-file"
            ),
        ])
        .output()
        .await
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}
