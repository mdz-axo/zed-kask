//! LoRA/QLoRA training-config validation — math-contract gates.
//!
//! Implements the static subset of the `lora-training` skill's quality gates
//! as Rust assertions. Called by `training_submit` before pod creation to
//! catch config errors that would silently degrade model quality or waste
//! GPU time.
//!
//! Gates implemented (13 of 17):
//! - G-M1: No-op-at-init invariant (init_lora_weights produces ΔW=0 at step 0)
//! - G-M2: Merge equivalence (bias='none' required for must-merge inference)
//! - G-M3: Scaling form (α/r or α/√r, never raw α or 1)
//! - G-M4: Rank budget (r < min(d_in, d_out), warn if r ≥ 0.5×min)
//! - G-M5: Trainable param count (post-training, in preflight check)
//! - G-Q1: Frozen base quantized (QLoRA mode: load_in_4bit + nf4)
//! - G-Q2: Adapter dtype (compute dtype is bf16/fp16, not fp32)
//! - G-Q4: No silent upcast (QLoRA mode: bf16=true, not fp16-only)
//! - G-Q5: Paged optimizer (conditional — warns for large models with QLoRA)
//! - G-D0: Dataset format compatibility (detects format, checks against
//!   expected trainer/method, emits copy-paste mapping code on mismatch)
//! - G-D1: Dataset size vs quality (warns <1000 or >100000 samples)
//! - G-D2: Eval protocol (advisory in preflight — Vicuna/MMLU not trustworthy)
//! - G-D3: Lemon-pick analysis (advisory in preflight — report failure cases)
//! - G-F1: Intruder dimension check (advisory in preflight — requires Python PEFT)
//! - G-H1: Harness-method compatibility (axolotl=SFT/DPO/KTO/ORPO/GRPO/RM/FullFT via rl:; trl=SFT/DPO/KTO/ORPO/Reward; ludwig=SFT/DPO/KTO/ORPO/GRPO)
//!
//! Gates NOT enforced (require runtime instrumentation in Python/training loop):
//! - G-Q3: Gradient flow (needs backward pass — A.grad and B.grad must be non-None)
//! - G-Q6: NF4 optimality (needs weight distribution analysis — NF4 assumes normal)
//! - G-F2: Knowledge preservation (needs CorDA mode + world-knowledge eval)
//!
//! Anchored to: LoRA (arXiv:2106.09685), QLoRA (arXiv:2305.14314),
//! rsLoRA (arXiv:2312.03732), DoRA (arXiv:2402.09353), PiSSA (arXiv:2404.02948),
//! Razin et al. (arXiv:2410.21228), PEFT v0.19.0, TRL v1.8.0.

// ── LoRA-param validation gates — extracted to `lora_validation/param_gates.rs`
mod param_gates;
pub(crate) use param_gates::{
    ValidationFinding, ValidationSeverity, has_refusals, validate_dataset_size,
    validate_paged_optimizer, validate_training_params,
};

// ── Dataset-format validation (G-D0) — extracted to `lora_validation/dataset_format.rs`
mod dataset_format;
pub(crate) use dataset_format::{
    DatasetFormatResult, DatasetFormatVerdict, validate_dataset_format,
};

/// G-R1: Runtime alert gate — validates runtime metrics for training instability.
///
/// Mirrors the HuggingFace trackio alert pattern: loss spikes, NaN gradients,
/// vanishing loss, and training stalls. This gate is `runtime`-phase only; it
/// is `not_applicable` in preflight. When runtime metrics are supplied via the
/// completion manifest, each alert becomes a normalized finding with
/// `evidence_kind: runtime_measurement`.
///
/// Anchored to: trackio alert API (huggingface-trackio skill §Alerts),
/// QLoRA paper §3 (training stability), Razin et al. arXiv:2410.21228
/// (intruder dimensions and structured forgetting).
// ── Runtime-metrics validation (G-R1) — extracted to `lora_validation/runtime_metrics.rs`
mod runtime_metrics;
pub(crate) use runtime_metrics::validate_runtime_metrics;
