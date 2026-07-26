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

use crate::dataset::DatasetFormat;
use crate::providers::types::{
    LoraParams, QuantizationParams, TrainingHarnessId, TrainingParams, TrlTrainer,
};

/// Severity of a validation finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Hard refusal — do not submit the job.
    Refuse,
    /// Soft warning — submit but flag in telemetry.
    Warn,
    /// Informational — no action needed.
    Info,
}

/// A single validation finding from a math-contract gate.
#[derive(Debug, Clone)]
pub struct ValidationFinding {
    /// Gate ID (e.g., "G-M1", "G-Q1").
    pub gate_id: &'static str,
    /// Severity: refuse, warn, or info.
    pub severity: ValidationSeverity,
    /// Human-readable message with the specific violation.
    pub message: String,
    /// Source citation (arXiv paper section or PEFT docs section).
    pub source: &'static str,
    /// Concrete remediation recommendation.
    pub remediation: String,
}

/// Validate training params against the LoRA/QLoRA math-contract gates.
///
/// Returns a list of findings. If any finding has `Refuse` severity, the
/// caller must not submit the job. `Warn` findings should be logged but
/// do not block submission.
pub fn validate_training_params(params: &TrainingParams) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    // G-M1: No-op-at-init invariant.
    validate_noop_at_init(&params.lora, &mut findings);

    // G-M2: Merge equivalence.
    validate_merge_equivalence(&params.lora, &mut findings);

    // G-M3: Scaling form.
    validate_scaling_form(&params.lora, &mut findings);

    // G-M4: Rank budget.
    validate_rank_budget(&params.lora, &mut findings);

    // G-Q1: Frozen base quantized (QLoRA mode only).
    validate_qlora_quantization(&params.quantization, &mut findings);

    // G-Q2: Adapter dtype (compute dtype).
    validate_compute_dtype(&params.quantization, &mut findings);

    // G-Q4: No silent upcast.
    validate_no_silent_upcast(params, &mut findings);

    // G-H1: Harness-method compatibility.
    validate_harness_compatibility(params, &mut findings);

    findings
}

/// G-D1: Dataset size vs quality gate.
///
/// QLoRA paper §5: small high-quality datasets beat large noisy ones.
/// - n_samples < 1000: warn (require explicit justification)
/// - n_samples > 100000: warn (require quality audit — dedup, contamination)
///
/// This gate is called from `training_submit` after dataset normalization,
/// not from `validate_training_params` (which doesn't have the dataset path).
pub fn validate_dataset_size(dataset_path: &std::path::Path) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    let content = match std::fs::read_to_string(dataset_path) {
        Ok(c) => c,
        Err(_) => return findings, // File read error is handled elsewhere
    };

    // Count non-empty lines (each line is one training example in ChatML JSONL).
    let n_samples = content.lines().filter(|l| !l.trim().is_empty()).count();

    if n_samples < 1000 {
        findings.push(ValidationFinding {
            gate_id: "G-D1",
            severity: ValidationSeverity::Warn,
            message: format!(
                "Dataset has only {} examples — QLoRA paper §5 recommends small high-quality datasets, but <1000 may be insufficient for stable convergence",
                n_samples
            ),
            source: "QLoRA paper §5 (small high-quality > large noisy)",
            remediation: format!(
                "Add more examples (current: {}) or document explicit justification for the small dataset",
                n_samples
            ),
        });
    }

    if n_samples > 100_000 {
        findings.push(ValidationFinding {
            gate_id: "G-D1",
            severity: ValidationSeverity::Warn,
            message: format!(
                "Dataset has {} examples — large datasets require quality audit (dedup, contamination check) per QLoRA paper §5",
                n_samples
            ),
            source: "QLoRA paper §5 (small high-quality > large noisy)",
            remediation: "Run dedup and contamination checks before training. Consider subsampling to a high-quality subset.".to_string(),
        });
    }

    findings
}

/// G-Q5: Paged optimizer gate (conditional).
///
/// QLoRA paper §3: paged optimizers manage memory spikes. Required when
/// peak memory is likely to exceed available VRAM. We can't measure peak
/// memory pre-submission, but we can warn when the config suggests high
/// memory pressure (large model + 4-bit + high batch size).
pub fn validate_paged_optimizer(
    params: &TrainingParams,
    base_model: &str,
) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    if params.quantization.load_in_4bit {
        // Heuristic: large models (13B+) with QLoRA should use paged optimizer.
        let lower = base_model.to_lowercase();
        let is_large = ["13b", "14b", "20b", "30b", "70b", "72b", "120b", "405b"]
            .iter()
            .any(|p| lower.contains(p));

        let uses_paged = params
            .optimization
            .optimizer
            .as_deref()
            .map(|o| o.contains("paged"))
            .unwrap_or(false);

        if is_large && !uses_paged {
            findings.push(ValidationFinding {
                gate_id: "G-Q5",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "QLoRA on large model ({}) without paged optimizer — may OOM on attention spikes",
                    base_model
                ),
                source: "QLoRA paper §3 (paged optimizers)",
                remediation: "Set optimizer=\"paged_adamw_8bit\" to handle memory spikes".to_string(),
            });
        }
    }

    findings
}

/// G-M1: No-op-at-init invariant.
///
/// PEFT default init and EVA both produce ΔW=0 at step 0 because B=0.
/// Initializers that modify base weights (PiSSA, LoftQ, OLoRA, CorDA) require
/// preprocessing calls (e.g., `preprocess_loraga`, `replace_lora_weights_loftq`).
fn validate_noop_at_init(lora: &LoraParams, findings: &mut Vec<ValidationFinding>) {
    if let Some(ref init) = lora.init_lora_weights {
        if !init.is_noop_at_init() {
            findings.push(ValidationFinding {
                gate_id: "G-M1",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "init_lora_weights={:?} — adapter is NOT a no-op at step 0 (ΔW≠0)",
                    init
                ),
                source: "LoRA paper §4.1; PEFT v0.19.0 LoraConfig.init_lora_weights docstring",
                remediation:
                    "Default init (true) is safe. Non-default inits require explicit justification."
                        .to_string(),
            });
        }
        if init.modifies_base_weights() {
            findings.push(ValidationFinding {
                gate_id: "G-M1",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "init_lora_weights={:?} modifies base weights — requires preprocessing call and explicit save handling",
                    init
                ),
                source: "PiSSA arXiv:2404.02948; LoRA-GA arXiv:2407.05000; PEFT v0.19.0 docs",
                remediation: match init {
                    crate::providers::types::LoraInit::Pissa
                    | crate::providers::types::LoraInit::PissaNiter(_) => {
                        "Call subtract_mutated_init() before merge, or use save_mutated_as_lora pattern".to_string()
                    }
                    crate::providers::types::LoraInit::Loftq => {
                        "Call replace_lora_weights_loftq() after model load".to_string()
                    }
                    _ => "Ensure training script calls the corresponding preprocessing function".to_string(),
                },
            });
        }
    }
}

/// G-M2: Merge equivalence.
///
/// bias='none' is the only safe setting for must-merge inference.
/// bias='all' and bias='lora_only' break merge equivalence — the model
/// will not produce the same output as the base model when adapters are disabled.
fn validate_merge_equivalence(lora: &LoraParams, findings: &mut Vec<ValidationFinding>) {
    if lora.bias.breaks_merge() {
        findings.push(ValidationFinding {
            gate_id: "G-M2",
            severity: ValidationSeverity::Warn,
            message: format!(
                "bias={:?} breaks merge equivalence — model will not match base model when adapter disabled",
                lora.bias
            ),
            source: "LoRA paper §4.2; PEFT v0.19.0 LoraConfig.bias docstring",
            remediation: "Set bias=none for must-merge inference. Use lora_only/all only when extracting from full fine-tune.".to_string(),
        });
    }
}

/// G-M3: Scaling form validation.
///
/// scaling = α/r (default) or α/√r (if use_rslora).
/// Refuse if r=0 or alpha=0 (division by zero).
/// Warn if r > 64 and use_rslora is false (should use rsLoRA for high rank).
fn validate_scaling_form(lora: &LoraParams, findings: &mut Vec<ValidationFinding>) {
    if lora.r == 0 {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity: ValidationSeverity::Refuse,
            message: "LoRA rank r=0 — division by zero in scaling α/r".to_string(),
            source: "LoRA paper §4.1 (α/r scaling); rsLoRA arXiv:2312.03732",
            remediation: "Set r to a positive integer (typical: 8–64)".to_string(),
        });
    }
    if lora.alpha == 0 {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity: ValidationSeverity::Refuse,
            message: "LoRA alpha=0 — scaling factor is zero, adapter has no effect".to_string(),
            source: "LoRA paper §4.1 (α/r scaling)",
            remediation: "Set alpha to a positive integer (typical: 2×r)".to_string(),
        });
    }
    // rsLoRA recommendation for high rank.
    if lora.r > 64 && !lora.use_rslora {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity: ValidationSeverity::Warn,
            message: format!(
                "LoRA rank r={} > 64 without use_rslora — scaling α/r underperforms α/√r at high rank",
                lora.r
            ),
            source: "rsLoRA paper arXiv:2312.03732 (Rank-Stabilized LoRA)",
            remediation: format!(
                "Set use_rslora=true, or reduce r to ≤64 (current scaling: {}/{})",
                lora.alpha, lora.r
            ),
        });
    }
}

/// G-M4: Rank budget validation.
///
/// r should be < min(d_in, d_out). Without the model loaded we can't check
/// the exact bound, but we warn on absurdly high r that defeats the low-rank
/// premise.
fn validate_rank_budget(lora: &LoraParams, findings: &mut Vec<ValidationFinding>) {
    if lora.r > 128 {
        findings.push(ValidationFinding {
            gate_id: "G-M4",
            severity: ValidationSeverity::Warn,
            message: format!(
                "LoRA rank r={} > 128 — defeats low-rank premise; consider full fine-tuning",
                lora.r
            ),
            source: "LoRA paper §4.3 (rank sufficiency experiments)",
            remediation: "Reduce r to ≤128, or use full fine-tuning if the task requires high rank"
                .to_string(),
        });
    }
    if lora.r > 256 {
        findings.push(ValidationFinding {
            gate_id: "G-M4",
            severity: ValidationSeverity::Refuse,
            message: format!(
                "LoRA rank r={} > 256 — not low-rank; LoRA provides no benefit at this rank",
                lora.r
            ),
            source: "LoRA paper §4.3 (rank sufficiency experiments)",
            remediation: "Use full fine-tuning, or reduce r significantly".to_string(),
        });
    }
}

/// G-Q1: QLoRA quantization validation.
///
/// If load_in_4bit is true, bnb_4bit_quant_type must be 'nf4' (not 'fp4').
/// NF4 is information-theoretically optimal for normally-distributed weights.
fn validate_qlora_quantization(quant: &QuantizationParams, findings: &mut Vec<ValidationFinding>) {
    if quant.load_in_4bit {
        match &quant.bnb_4bit_quant_type {
            None => {
                findings.push(ValidationFinding {
                    gate_id: "G-Q1",
                    severity: ValidationSeverity::Warn,
                    message: "QLoRA mode (load_in_4bit=true) without bnb_4bit_quant_type — defaults to fp4, but nf4 is optimal".to_string(),
                    source: "QLoRA paper §3 (NF4 — 4-bit NormalFloat)",
                    remediation: "Set bnb_4bit_quant_type=\"nf4\"".to_string(),
                });
            }
            Some(t) if t != "nf4" => {
                findings.push(ValidationFinding {
                    gate_id: "G-Q1",
                    severity: ValidationSeverity::Warn,
                    message: format!(
                        "QLoRA mode with bnb_4bit_quant_type=\"{}\" — nf4 is information-theoretically optimal for normally-distributed weights",
                        t
                    ),
                    source: "QLoRA paper §3 (NF4 derivation)",
                    remediation: "Set bnb_4bit_quant_type=\"nf4\"".to_string(),
                });
            }
            _ => {} // nf4 — pass
        }
        if !quant.bnb_4bit_use_double_quant {
            findings.push(ValidationFinding {
                gate_id: "G-Q1",
                severity: ValidationSeverity::Info,
                message: "QLoRA mode without bnb_4bit_use_double_quant — double quantization saves ~0.37 bits/param".to_string(),
                source: "QLoRA paper §3 (double quantization)",
                remediation: "Set bnb_4bit_use_double_quant=true for additional memory savings".to_string(),
            });
        }
    }
}

/// G-Q2: Compute dtype validation.
///
/// If QLoRA mode, bnb_4bit_compute_dtype should be bf16 or fp16, not fp32.
/// fp32 compute through a 4-bit base wastes the memory savings.
fn validate_compute_dtype(quant: &QuantizationParams, findings: &mut Vec<ValidationFinding>) {
    if quant.load_in_4bit {
        match &quant.bnb_4bit_compute_dtype {
            None => {
                // Default is fp16 in bitsandbytes — acceptable.
            }
            Some(dt) if dt == "fp32" => {
                findings.push(ValidationFinding {
                    gate_id: "G-Q2",
                    severity: ValidationSeverity::Refuse,
                    message: "QLoRA mode with bnb_4bit_compute_dtype=\"fp32\" — fp32 compute through 4-bit base wastes memory (silent 2× upcast)".to_string(),
                    source: "QLoRA paper §3 (compute in bf16 through frozen base)",
                    remediation: "Set bnb_4bit_compute_dtype=\"bf16\" or \"fp16\"".to_string(),
                });
            }
            Some(dt) if dt != "bf16" && dt != "fp16" => {
                findings.push(ValidationFinding {
                    gate_id: "G-Q2",
                    severity: ValidationSeverity::Warn,
                    message: format!(
                        "QLoRA mode with bnb_4bit_compute_dtype=\"{}\" — expected \"bf16\" or \"fp16\"",
                        dt
                    ),
                    source: "QLoRA paper §3 (compute dtype)",
                    remediation: "Set bnb_4bit_compute_dtype=\"bf16\" (preferred) or \"fp16\"".to_string(),
                });
            }
            _ => {} // bf16 or fp16 — pass
        }
    }
}

/// G-Q4: No silent upcast.
///
/// If QLoRA mode, bf16 should be true (not fp16-only). fp16 can cause
/// silent upcast to fp32 in some operations, doubling memory.
fn validate_no_silent_upcast(params: &TrainingParams, findings: &mut Vec<ValidationFinding>) {
    if params.quantization.load_in_4bit && !params.advanced.bf16 && params.advanced.fp16 {
        findings.push(ValidationFinding {
                gate_id: "G-Q4",
                severity: ValidationSeverity::Warn,
                message: "QLoRA mode with fp16=true and bf16=false — fp16 can cause silent upcast to fp32 in some operations".to_string(),
                source: "QLoRA paper §3 (bf16 compute); PEFT prepare_model_for_kbit_training docstring",
                remediation: "Set bf16=true (preferred over fp16 for QLoRA)".to_string(),
            });
    }
}

/// G-H1: Harness-method compatibility.
///
/// Asserts that the selected harness supports the selected method/trainer.
/// This is the runtime enforcement point for the `lora-training` skill's
/// G-H1 audit gate (see `registry/templates/lora-training/audit-config.j2`).
///
/// - harness=axolotl → SFT, DPO, KTO, ORPO, GRPO, GDPO, RM, Full FT via `rl:`
///   parameter. If a TRL trainer is selected, warn — trl_trainer is TRL-specific
///   and Axolotl ignores it (Axolotl uses its own `rl:` config parameter).
/// - harness=trl → All trainers (SFT, DPO, KTO, ORPO, Reward) are supported.
/// - harness=None → not_evaluated (runtime defaults to axolotl).
///
/// Citation: TRL trainer taxonomy — https://huggingface.co/docs/trl/index
fn validate_harness_compatibility(params: &TrainingParams, findings: &mut Vec<ValidationFinding>) {
    match params.harness {
        None => {
            // No harness selected — runtime defaults to axolotl. If a TRL
            // trainer was specified without selecting harness=trl, warn: the
            // trainer will be ignored.
            if params.trl_trainer.is_some() {
                findings.push(ValidationFinding {
                    gate_id: "G-H1",
                    severity: ValidationSeverity::Warn,
                    message: "trl_trainer specified but harness is not set to trl — the trainer will be ignored (runtime defaults to axolotl)".to_string(),
                    source: "TRL trainer taxonomy — https://huggingface.co/docs/trl/index",
                    remediation: "Set harness=trl to use the specified TRL trainer, or remove trl_trainer to use axolotl SFT".to_string(),
                });
            }
        }
        Some(TrainingHarnessId::Axolotl) => {
            // Axolotl supports the full training spectrum (SFT, DPO, KTO, ORPO,
            // GRPO, GDPO, RM, Full FT) via its `rl:` config parameter. However,
            // trl_trainer is a TRL-specific concept — Axolotl ignores it and uses
            // its own method selection. Warn (not refuse) so the operator knows
            // the trl_trainer will be dropped.
            if params.trl_trainer.is_some() {
                findings.push(ValidationFinding {
                    gate_id: "G-H1",
                    severity: ValidationSeverity::Warn,
                    message: "harness=axolotl with trl_trainer set — trl_trainer is TRL-specific and Axolotl ignores it (Axolotl uses rl: in its YAML for method selection)".to_string(),
                    source: "Axolotl docs — https://docs.axolotl.ai/docs/rlhf.html",
                    remediation: "Remove trl_trainer when using harness=axolotl; Axolotl selects training method via rl: in the rendered config".to_string(),
                });
            }
        }
        Some(TrainingHarnessId::Trl) => {
            // TRL harness: all trainers are supported.
            // G-H1 only checks harness-method compatibility, not dataset format.
            // Dataset format validation is handled by the dataset pipeline's
            // format detection (DatasetFormat::detect) and the trainer's
            // expected_dataset_format() method.
            match params.trl_trainer.unwrap_or_default() {
                TrlTrainer::Sft
                | TrlTrainer::Dpo
                | TrlTrainer::Kto
                | TrlTrainer::Orpo
                | TrlTrainer::Reward => {
                    // All trainers are supported — no finding.
                }
            }
        }
        Some(TrainingHarnessId::Ludwig) => {
            // Ludwig harness: supports SFT, DPO, KTO, ORPO, GRPO via trainer.type.
            // If a TRL-specific trl_trainer field is set, warn: Ludwig uses its
            // own trainer.type taxonomy, not TRL's.
            if params.trl_trainer.is_some() {
                findings.push(ValidationFinding {
                    gate_id: "G-H1",
                    severity: ValidationSeverity::Warn,
                    message: "harness=ludwig with trl_trainer set — trl_trainer is TRL-specific and Ludwig ignores it (Ludwig uses trainer.type in its own YAML)".to_string(),
                    source: "Ludwig trainer taxonomy — https://ludwig.ai/latest/configuration/#trainer",
                    remediation: "Remove trl_trainer when using harness=ludwig; Ludwig's trainer is selected via trainer.type in the rendered config".to_string(),
                });
            }
        }
    }
}

/// G-P1: Persistence preflight — verify that training results will be
/// persisted before allowing submit on ephemeral cloud hosts.
///
/// Mirrors the HuggingFace "Critical: Saving Results to Hub" checklist.
/// The persistence contract in hKask is job-level (env vars + artifacts),
/// not config-level (`TrainingParams`): the install script's
/// `huggingface-cli upload` is the actual push-to-Hub, driven by
/// `HKASK_HF_MODEL_REPO` / `HF_TOKEN` env vars and `job.artifacts`.
///
/// # Arguments
/// * `host_id` - The training host (Runpod, DeepInfra, Nebius).
/// * `hf_training_result` - `Ok(training)` if env vars are set, `Err` if not.
///
/// # Returns
/// Findings for G-P1. On Runpod, missing env vars → `Refuse` (adapter
/// upload is configured via env vars; without them, results are lost on the
/// ephemeral pod). On DeepInfra/Nebius, `Warn` (these hosts don't set
/// `job.artifacts` today — no upload happens — but manual retrieval may be
/// possible). On unknown hosts, `not_applicable` (no finding emitted).
pub fn validate_persistence(
    host_id: &crate::providers::TrainingHostId,
    hf_training_result: &Result<(), String>,
) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();
    match host_id {
        crate::providers::TrainingHostId::Runpod => {
            if let Err(reason) = hf_training_result {
                findings.push(ValidationFinding {
                    gate_id: "G-P1",
                    severity: ValidationSeverity::Refuse,
                    message: format!(
                        "Runpod host requires HuggingFace persistence env vars to be configured \
                         (HKASK_HF_ARTIFACT_OWNER, HKASK_HF_MODEL_REPO, HF_TOKEN) — \
                         without them, the adapter and completion manifest are lost when the \
                         ephemeral pod terminates. Error: {reason}"
                    ),
                    source: "HF huggingface-llm-trainer skill §Critical: Saving Results to Hub — \
                             ephemeral environment, must push to Hub",
                    remediation: "Set HKASK_HF_ARTIFACT_OWNER, HKASK_HF_MODEL_REPO, \
                                  HKASK_HF_DATASET_REPO, and HF_TOKEN environment variables \
                                  before submitting a Runpod training job"
                        .to_string(),
                });
            }
        }
        crate::providers::TrainingHostId::DeepInfra | crate::providers::TrainingHostId::Nebius => {
            // These hosts do not currently set job.artifacts, so the install
            // script has no model_repo to upload to. The operator may retrieve
            // results manually from the pod, but this is not guaranteed on
            // ephemeral infrastructure. Warn — do not refuse, since the host
            // may support manual retrieval or the operator may have a custom
            // persistence path.
            findings.push(ValidationFinding {
                gate_id: "G-P1",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "{host:?} host does not configure HuggingFace artifact persistence — \
                     adapter weights and completion manifest are not automatically uploaded. \
                     Results may be lost when the ephemeral pod terminates.",
                    host = host_id
                ),
                source: "HF huggingface-llm-trainer skill §Critical: Saving Results to Hub",
                remediation: "Configure HuggingFace persistence env vars \
                              (HKASK_HF_ARTIFACT_OWNER, HKASK_HF_MODEL_REPO, HF_TOKEN) \
                              and ensure the host sets job.artifacts, or retrieve the adapter \
                              manually before the pod terminates"
                    .to_string(),
            });
        }
    }
    findings
}

/// Returns true if any finding has `Refuse` severity — the job must not be submitted.
pub fn has_refusals(findings: &[ValidationFinding]) -> bool {
    findings
        .iter()
        .any(|f| f.severity == ValidationSeverity::Refuse)
}

/// Verdict from G-D0 dataset format compatibility check.
///
/// Mirrors the HuggingFace `dataset_inspector.py` three-state pattern:
/// `Ready` (use directly), `NeedsMapping` (compatible but needs preprocessing —
/// mapping code is provided), `Incompatible` (cannot be used for this method).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetFormatVerdict {
    /// Dataset format matches the expected format for the selected method.
    Ready,
    /// Dataset is compatible but needs column-name mapping. `mapping_code` is
    /// copy-paste Python that transforms the dataset into the expected schema.
    NeedsMapping,
    /// Dataset cannot be used for the selected method (e.g., SFT data for DPO).
    Incompatible,
}

/// Result of G-D0 dataset format validation.
#[derive(Debug, Clone)]
pub struct DatasetFormatResult {
    /// The verdict: ready, needs mapping, or incompatible.
    pub verdict: DatasetFormatVerdict,
    /// The format detected from the dataset file (None if detection failed).
    pub detected_format: Option<DatasetFormat>,
    /// The format expected by the selected trainer/method (None if undetermined).
    pub expected_format: Option<DatasetFormat>,
    /// Copy-paste Python mapping code when verdict is `NeedsMapping`.
    /// Empty string otherwise.
    pub mapping_code: String,
    /// Validation findings (G-D0 warnings/refusals).
    pub findings: Vec<ValidationFinding>,
}

/// G-D0: Dataset format compatibility check.
///
/// Detects the dataset format from the file and checks it against the format
/// expected by the selected trainer/method. When the detected format is
/// structurally compatible but uses non-standard column names, emits
/// copy-paste Python mapping code (mirrors HF's `dataset_inspector.py`).
///
/// This is the runtime enforcement point for the lora-training skill's G-D0
/// gate. Called from `training_validate_config` and `training_submit`.
///
/// # Arguments
/// * `dataset_path` - Path to the dataset file.
/// * `trainer_preference` - The operator-selected trainer (sft/dpo/kto/orpo/etc.).
/// * `adapter_purpose` - The adapter purpose (instruction/preference/reward_model/etc.).
///
/// # Returns
/// A `DatasetFormatResult` with the verdict, detected/expected formats,
/// mapping code (if applicable), and any findings.
pub fn validate_dataset_format(
    dataset_path: &std::path::Path,
    trainer_preference: Option<&str>,
    adapter_purpose: Option<&str>,
) -> DatasetFormatResult {
    let mut findings = Vec::new();

    let detected_format = DatasetFormat::detect(dataset_path);
    let expected_format = derive_expected_format(trainer_preference, adapter_purpose);

    match (&detected_format, &expected_format) {
        (None, _) => {
            findings.push(ValidationFinding {
                gate_id: "G-D0",
                severity: ValidationSeverity::Warn,
                message: format!(
                    "Could not detect dataset format from file: {} — format detection requires .jsonl/.json/.txt extension",
                    dataset_path.display()
                ),
                source: "hKask dataset pipeline — DatasetFormat::detect",
                remediation: "Ensure the dataset file has a .jsonl, .json, or .txt extension".to_string(),
            });
            DatasetFormatResult {
                verdict: DatasetFormatVerdict::Ready,
                detected_format,
                expected_format,
                mapping_code: String::new(),
                findings,
            }
        }
        (Some(_detected), None) => {
            // No expected format derivable — cannot check compatibility.
            // This is not a failure; the operator may not have declared a trainer.
            DatasetFormatResult {
                verdict: DatasetFormatVerdict::Ready,
                detected_format,
                expected_format,
                mapping_code: String::new(),
                findings,
            }
        }
        (Some(detected), Some(expected)) => {
            if detected == expected {
                // Exact match — ready.
                DatasetFormatResult {
                    verdict: DatasetFormatVerdict::Ready,
                    detected_format,
                    expected_format,
                    mapping_code: String::new(),
                    findings,
                }
            } else if is_format_compatible(detected, expected) {
                // Structurally compatible. SFT formats (ChatML, ShareGPT, Alpaca,
                // RawText) are auto-normalized by the dataset pipeline — no manual
                // mapping needed, so verdict is Ready. Preference formats need
                // manual column mapping, so verdict is NeedsMapping.
                let needs_manual_mapping = detected.is_preference() && expected.is_preference();
                if needs_manual_mapping {
                    let mapping_code = generate_mapping_code(detected, expected);
                    findings.push(ValidationFinding {
                        gate_id: "G-D0",
                        severity: ValidationSeverity::Warn,
                        message: format!(
                            "Dataset format {:?} needs mapping to {:?} for the selected method — mapping code provided",
                            detected, expected
                        ),
                        source: "HF dataset_inspector.py pattern — huggingface.co/datasets/mcp-tools/skills",
                        remediation: "Apply the mapping code below before training to avoid format-mismatch failures".to_string(),
                    });
                    DatasetFormatResult {
                        verdict: DatasetFormatVerdict::NeedsMapping,
                        detected_format,
                        expected_format,
                        mapping_code,
                        findings,
                    }
                } else {
                    // SFT format conversion — auto-normalized by the pipeline.
                    DatasetFormatResult {
                        verdict: DatasetFormatVerdict::Ready,
                        detected_format,
                        expected_format,
                        mapping_code: String::new(),
                        findings,
                    }
                }
            } else {
                // Incompatible — e.g., SFT data (ChatML) for DPO training.
                findings.push(ValidationFinding {
                    gate_id: "G-D0",
                    severity: ValidationSeverity::Refuse,
                    message: format!(
                        "Dataset format {:?} is incompatible with expected format {:?} — cannot use this dataset for the selected method",
                        detected, expected
                    ),
                    source: "TRL dataset formats — huggingface.co/docs/trl/main/en/dataset_formats",
                    remediation: format!(
                        "Use a {:?} dataset for the selected method, or change the trainer to match the {:?} dataset",
                        expected, detected
                    ),
                });
                DatasetFormatResult {
                    verdict: DatasetFormatVerdict::Incompatible,
                    detected_format,
                    expected_format,
                    mapping_code: String::new(),
                    findings,
                }
            }
        }
    }
}

/// Derive the expected dataset format from the trainer preference and adapter purpose.
///
/// Returns `None` when neither input is sufficient to determine the expected format.
fn derive_expected_format(
    trainer_preference: Option<&str>,
    adapter_purpose: Option<&str>,
) -> Option<DatasetFormat> {
    // Trainer preference takes precedence.
    if let Some(trainer) = trainer_preference {
        match trainer {
            "sft" | "undetermined" => return Some(DatasetFormat::ChatML),
            "dpo" => return Some(DatasetFormat::PreferenceDpo),
            "kto" => return Some(DatasetFormat::PreferenceKto),
            "orpo" => return Some(DatasetFormat::PreferenceOrpo),
            "reward" | "grpo" => return Some(DatasetFormat::ChatML),
            _ => {}
        }
    }
    // Fall back to adapter purpose.
    if let Some(purpose) = adapter_purpose {
        match purpose {
            "instruction" | "reasoning" | "vision" | "reward_model" | "undetermined" => {
                return Some(DatasetFormat::ChatML);
            }
            "preference" => return Some(DatasetFormat::PreferenceDpo),
            _ => {}
        }
    }
    None
}

/// Check whether a detected format is structurally compatible with the expected
/// format (i.e., can be mapped via column renaming, not a fundamental mismatch).
fn is_format_compatible(detected: &DatasetFormat, expected: &DatasetFormat) -> bool {
    use DatasetFormat::*;
    // SFT formats are interchangeable via normalization (ChatML, ShareGPT, Alpaca, RawText).
    let sft_formats = [ChatML, ShareGPT, Alpaca, RawText];
    let detected_is_sft = sft_formats.contains(detected);
    let expected_is_sft = sft_formats.contains(expected);
    if detected_is_sft && expected_is_sft {
        return true;
    }
    // Preference formats are interchangeable via column mapping (DPO, KTO, ORPO).
    let preference_formats = [PreferenceDpo, PreferenceKto, PreferenceOrpo];
    let detected_is_preference = preference_formats.contains(detected);
    let expected_is_preference = preference_formats.contains(expected);
    if detected_is_preference && expected_is_preference {
        return true;
    }
    // SFT data cannot be used for preference training and vice versa.
    false
}

/// Generate copy-paste Python mapping code for a compatible format mismatch.
///
/// Mirrors the HF `dataset_inspector.py` "MAPPING CODE" section pattern.
fn generate_mapping_code(detected: &DatasetFormat, expected: &DatasetFormat) -> String {
    use DatasetFormat::*;
    match (detected, expected) {
        // SFT format conversions — the pipeline normalizes these, so no manual mapping needed.
        (ShareGPT, ChatML) | (Alpaca, ChatML) | (RawText, ChatML) => {
            "# The hKask dataset pipeline normalizes this format to ChatML automatically.\n# No manual mapping code needed — proceed with training.".to_string()
        }
        (ChatML, ShareGPT) | (Alpaca, ShareGPT) | (RawText, ShareGPT) => {
            "# The hKask dataset pipeline normalizes this format to ChatML automatically.\n# No manual mapping code needed — proceed with training.".to_string()
        }
        // Preference format conversions — column-name mapping.
        (PreferenceKto, PreferenceDpo) => {
            "# Map KTO format (prompt/completion/label) to DPO format (prompt/chosen/rejected):\n\ndef format_for_dpo(example):\n    return {\n        'prompt': example['prompt'],\n        'chosen': example['completion'] if example.get('label', True) else '',\n        'rejected': example['completion'] if not example.get('label', True) else '',\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_dpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceOrpo, PreferenceDpo) => {
            "# Map ORPO format (chosen/rejected, no prompt) to DPO format (prompt/chosen/rejected):\n# ORPO's chosen/rejected typically contain the prompt implicitly.\n\ndef format_for_dpo(example):\n    # ORPO chosen/rejected are conversational; extract the prompt from the first turn.\n    chosen = example['chosen']\n    rejected = example['rejected']\n    # If chosen is a list of messages, extract prompt from the first user turn.\n    prompt = ''\n    if isinstance(chosen, list) and len(chosen) > 0:\n        prompt = chosen[0].get('content', '') if isinstance(chosen[0], dict) else str(chosen[0])\n    return {'prompt': prompt, 'chosen': chosen, 'rejected': rejected}\n\n# Apply before training:\n# dataset = dataset.map(format_for_dpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceDpo, PreferenceKto) => {
            "# Map DPO format (prompt/chosen/rejected) to KTO format (prompt/completion/label):\n\ndef format_for_kto(example):\n    return {\n        'prompt': example['prompt'],\n        'completion': example['chosen'],\n        'label': True,\n    }\n\n# Note: DPO data only provides positive examples. For KTO, you also need\n# negative examples. Consider adding rejected completions as label=False rows.\n# Apply before training:\n# dataset = dataset.map(format_for_kto, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceDpo, PreferenceOrpo) => {
            "# Map DPO format (prompt/chosen/rejected) to ORPO format (chosen/rejected):\n\ndef format_for_orpo(example):\n    return {\n        'chosen': example['chosen'],\n        'rejected': example['rejected'],\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_orpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceKto, PreferenceOrpo) => {
            "# Map KTO format (prompt/completion/label) to ORPO format (chosen/rejected):\n# KTO is unpaired; ORPO is paired. This mapping loses information.\n# Consider collecting paired preference data instead.\n\ndef format_for_orpo(example):\n    completion = example['completion']\n    label = example.get('label', True)\n    return {\n        'chosen': completion if label else '',\n        'rejected': completion if not label else '',\n    }\n\n# Apply before training:\n# dataset = dataset.map(format_for_orpo, remove_columns=dataset.column_names)".to_string()
        }
        (PreferenceOrpo, PreferenceKto) => {
            "# Map ORPO format (chosen/rejected) to KTO format (prompt/completion/label):\n\ndef format_for_kto(example):\n    chosen = example['chosen']\n    rejected = example['rejected']\n    # Extract prompt from chosen (first user turn if conversational).\n    prompt = ''\n    if isinstance(chosen, list) and len(chosen) > 0:\n        prompt = chosen[0].get('content', '') if isinstance(chosen[0], dict) else str(chosen[0])\n    return [\n        {'prompt': prompt, 'completion': chosen, 'label': True},\n        {'prompt': prompt, 'completion': rejected, 'label': False},\n    ]\n\n# Apply before training (flatten the resulting list):\n# dataset = dataset.map(format_for_kto, remove_columns=dataset.column_names)\n# dataset = datasets.concatenate_datasets([dataset])  # flatten if needed".to_string()
        }
        _ => {
            format!(
                "# No automatic mapping code available for {:?} → {:?}.\n# Inspect the dataset columns and write a custom mapping function.",
                detected, expected
            )
        }
    }
}

/// Runtime metrics from a training job — consumed by G-R1 (runtime alert gate).
///
/// Sourced from the completion manifest or live metric polling. When supplied
/// to `validate_runtime_metrics`, produces findings for loss spikes, NaN
/// gradients, vanishing loss, and training stalls.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RuntimeMetrics {
    /// Current training step.
    #[serde(default)]
    pub current_step: Option<u32>,
    /// Total training steps.
    #[serde(default)]
    pub total_steps: Option<u32>,
    /// Latest loss value.
    #[serde(default)]
    pub loss: Option<f64>,
    /// Latest gradient norm.
    #[serde(default)]
    pub grad_norm: Option<f64>,
    /// Runtime alerts (e.g., from trackio.alert() or equivalent).
    #[serde(default)]
    pub alerts: Vec<TrainingAlert>,
}

/// A single runtime alert from the training loop.
///
/// Renamed from `RuntimeAlert` to avoid collision with
/// `hkask_regulation::RuntimeAlert` (which has different fields).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrainingAlert {
    /// Alert title (e.g., "Loss divergence", "Vanishing loss").
    #[serde(default)]
    pub title: String,
    /// Alert severity: "info", "warn", "error", "critical".
    /// Unknown levels default to "warn" in `validate_runtime_metrics`.
    #[serde(default = "default_alert_level")]
    pub level: String,
    /// Alert text/body.
    #[serde(default)]
    pub text: String,
    /// Step at which the alert fired.
    #[serde(default)]
    pub step: Option<u32>,
}

fn default_alert_level() -> String {
    "warn".to_string()
}

/// G-R1: Runtime alert gate — validates runtime metrics for training instability.
///
/// Mirrors the HuggingFace trackio alert pattern: loss spikes, NaN gradients,
/// vanishing loss, and training stalls. This gate is `runtime`-phase only; it
/// is `not_applicable` in preflight. When `runtime_metrics` is supplied, each
/// alert becomes a normalized finding with `evidence_kind: runtime_measurement`.
///
/// Anchored to: trackio alert API (huggingface-trackio skill §Alerts),
/// QLoRA paper §3 (training stability), Razin et al. arXiv:2410.21228
/// (intruder dimensions and structured forgetting).
pub fn validate_runtime_metrics(metrics: &RuntimeMetrics) -> Vec<ValidationFinding> {
    let mut findings = Vec::new();

    // Process explicit alerts first — these are operator/runtime-supplied signals.
    for alert in &metrics.alerts {
        let severity = match alert.level.as_str() {
            "error" | "critical" | "fatal" => ValidationSeverity::Refuse,
            "warn" => ValidationSeverity::Warn,
            // Unknown non-empty levels default to Warn (safer than Info —
            // surfaces the alert rather than silently downgrading it).
            _ => ValidationSeverity::Warn,
        };
        let step_str = alert
            .step
            .map(|s| format!(" at step {s}"))
            .unwrap_or_default();
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity,
            message: format!("{}{}: {}", alert.title, step_str, alert.text),
            source: "Runtime alert (trackio.alert pattern) — huggingface-trackio skill §Alerts",
            remediation:
                "Investigate the alert condition and adjust hyperparameters or stop the run"
                    .to_string(),
        });
    }

    // Detect NaN or infinite loss — training has diverged.
    // NaN comparisons are always false in Rust, so `loss > 5.0` would silently
    // pass. We check is_nan()/is_infinite() explicitly before the divergence
    // and vanishing checks.
    if let Some(loss) = metrics.loss {
        if loss.is_nan() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: "NaN loss detected — training has diverged".to_string(),
                source: "QLoRA paper §3 (training stability); IEEE-754 NaN semantics",
                remediation: "Stop the run, reduce learning rate, enable gradient clipping, or check for numerical instability".to_string(),
            });
        } else if loss.is_infinite() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: format!("Infinite loss ({}) — training has diverged", loss),
                source: "QLoRA paper §3 (training stability)",
                remediation: "Stop the run, reduce learning rate, or enable gradient clipping"
                    .to_string(),
            });
        }
    }

    // Detect loss divergence: loss > 5.0 after step 100.
    // Only reached if loss is finite (NaN/inf handled above).
    if let (Some(step), Some(loss)) = (metrics.current_step, metrics.loss)
        && loss.is_finite()
        && step > 100
        && loss > 5.0
    {
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity: ValidationSeverity::Refuse,
            message: format!(
                "Loss divergence: loss {:.4} still high after {} steps",
                loss, step
            ),
            source: "trackio.alert pattern — huggingface-trackio skill §Autonomous ML Experiment Workflow",
            remediation: "Stop the run, reduce learning rate, or check for dataset/label errors".to_string(),
        });
    }

    // Detect vanishing loss: |loss| < 1e-8 after step 0.
    // Only reached if loss is finite.
    if let (Some(step), Some(loss)) = (metrics.current_step, metrics.loss)
        && loss.is_finite()
        && step > 0
        && loss.abs() < 1e-8
    {
        findings.push(ValidationFinding {
            gate_id: "G-R1",
            severity: ValidationSeverity::Warn,
            message: format!(
                "Vanishing loss: loss {:.2e} near zero at step {} — possible gradient collapse",
                loss, step
            ),
            source: "trackio.alert pattern — huggingface-trackio skill §Alerts",
            remediation: "Check gradient flow, learning rate, and dataset labels".to_string(),
        });
    }

    // Detect NaN gradient norm.
    if let Some(grad_norm) = metrics.grad_norm {
        if grad_norm.is_nan() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: "NaN gradient norm detected — training has diverged".to_string(),
                source: "QLoRA paper §3 (training stability); trackio alert pattern",
                remediation: "Stop the run, reduce learning rate, enable gradient clipping, or check for numerical instability".to_string(),
            });
        } else if grad_norm.is_infinite() {
            findings.push(ValidationFinding {
                gate_id: "G-R1",
                severity: ValidationSeverity::Refuse,
                message: format!(
                    "Infinite gradient norm ({}) — training has diverged",
                    grad_norm
                ),
                source: "QLoRA paper §3 (training stability)",
                remediation: "Stop the run, reduce learning rate, or enable gradient clipping"
                    .to_string(),
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{AdvancedParams, OptimizationParams, SequenceParams};

    fn default_params() -> TrainingParams {
        TrainingParams {
            num_epochs: 3,
            batch_size: 4,
            learning_rate: 2e-4,
            lora: LoraParams::default(),
            quantization: QuantizationParams::default(),
            optimization: OptimizationParams::default(),
            sequence: SequenceParams::default(),
            advanced: AdvancedParams::default(),
            harness: None,
            trl_trainer: None,
        }
    }

    #[test]
    fn default_params_pass_all_gates() {
        let findings = validate_training_params(&default_params());
        let refusals: Vec<_> = findings
            .iter()
            .filter(|f| f.severity == ValidationSeverity::Refuse)
            .collect();
        assert!(
            refusals.is_empty(),
            "Default params should not refuse: {:?}",
            refusals
        );
    }

    #[test]
    fn pissa_init_warns_gm1() {
        let mut params = default_params();
        params.lora.init_lora_weights = Some(crate::providers::types::LoraInit::Pissa);
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M1" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn eva_noop_init_does_not_warn_gm1() {
        let mut params = default_params();
        params.lora.init_lora_weights = Some(crate::providers::types::LoraInit::Eva);
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|finding| finding.gate_id != "G-M1"));
    }

    #[test]
    fn default_init_does_not_warn_gm1() {
        let params = default_params();
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-M1"));
    }

    #[test]
    fn bias_all_warns_gm2() {
        let mut params = default_params();
        params.lora.bias = crate::providers::types::LoraBias::All;
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M2" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn bias_none_does_not_warn_gm2() {
        let params = default_params();
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-M2"));
    }

    #[test]
    fn loftq_init_warns_modifies_base() {
        let mut params = default_params();
        params.lora.init_lora_weights = Some(crate::providers::types::LoraInit::Loftq);
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M1" && f.message.contains("modifies base weights"))
        );
    }

    #[test]
    fn rank_zero_refuses() {
        let mut params = default_params();
        params.lora.r = 0;
        let findings = validate_training_params(&params);
        assert!(has_refusals(&findings));
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M3" && f.severity == ValidationSeverity::Refuse)
        );
    }

    #[test]
    fn alpha_zero_refuses() {
        let mut params = default_params();
        params.lora.alpha = 0;
        let findings = validate_training_params(&params);
        assert!(has_refusals(&findings));
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M3" && f.severity == ValidationSeverity::Refuse)
        );
    }

    #[test]
    fn high_rank_without_rslora_warns() {
        let mut params = default_params();
        params.lora.r = 128;
        params.lora.use_rslora = false;
        let findings = validate_training_params(&params);
        assert!(!has_refusals(&findings));
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M3" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn rank_over_256_refuses() {
        let mut params = default_params();
        params.lora.r = 512;
        let findings = validate_training_params(&params);
        assert!(has_refusals(&findings));
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-M4" && f.severity == ValidationSeverity::Refuse)
        );
    }

    #[test]
    fn qlora_without_nf4_warns() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.quantization.bnb_4bit_quant_type = None;
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-Q1" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn qlora_with_fp4_warns() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.quantization.bnb_4bit_quant_type = Some("fp4".to_string());
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-Q1" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn qlora_with_nf4_passes() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.quantization.bnb_4bit_quant_type = Some("nf4".to_string());
        params.quantization.bnb_4bit_use_double_quant = true;
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-Q1"));
    }

    #[test]
    fn qlora_with_fp32_compute_refuses() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.quantization.bnb_4bit_quant_type = Some("nf4".to_string());
        params.quantization.bnb_4bit_compute_dtype = Some("fp32".to_string());
        let findings = validate_training_params(&params);
        assert!(has_refusals(&findings));
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-Q2" && f.severity == ValidationSeverity::Refuse)
        );
    }

    #[test]
    fn qlora_with_bf16_compute_passes() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.quantization.bnb_4bit_quant_type = Some("nf4".to_string());
        params.quantization.bnb_4bit_compute_dtype = Some("bf16".to_string());
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-Q2"));
    }

    #[test]
    fn qlora_fp16_only_warns_silent_upcast() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.advanced.fp16 = true;
        params.advanced.bf16 = false;
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-Q4" && f.severity == ValidationSeverity::Warn)
        );
    }

    // ── G-D1: Dataset size tests ──

    #[test]
    fn small_dataset_warns_gd1() {
        let temp = std::env::temp_dir().join("test_small_dataset.jsonl");
        // Write 100 examples (below 1000 threshold)
        let content: Vec<String> = (0..100)
            .map(|i| format!("{{\"messages\": [{{\"role\": \"user\", \"content\": \"q{}\"}}, {{\"role\": \"assistant\", \"content\": \"a{}\"}}]}}", i, i))
            .collect();
        std::fs::write(&temp, content.join("\n")).unwrap();
        let findings = validate_dataset_size(&temp);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-D1" && f.severity == ValidationSeverity::Warn)
        );
        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn large_dataset_warns_gd1() {
        let temp = std::env::temp_dir().join("test_large_dataset.jsonl");
        // Write 100001 examples (above 100000 threshold) — use a compact format
        let line = "{\"messages\":[{\"role\":\"user\",\"content\":\"q\"},{\"role\":\"assistant\",\"content\":\"a\"}]}}";
        let content: Vec<&str> = std::iter::repeat_n(line, 100_001).collect();
        std::fs::write(&temp, content.join("\n")).unwrap();
        let findings = validate_dataset_size(&temp);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-D1" && f.severity == ValidationSeverity::Warn)
        );
        assert!(findings[0].message.contains("quality audit"));
        std::fs::remove_file(&temp).ok();
    }

    #[test]
    fn normal_dataset_no_gd1_warning() {
        let temp = std::env::temp_dir().join("test_normal_dataset.jsonl");
        // Write 5000 examples (between 1000 and 100000)
        let content: Vec<String> = (0..5000)
            .map(|i| format!("{{\"messages\": [{{\"role\": \"user\", \"content\": \"q{}\"}}, {{\"role\": \"assistant\", \"content\": \"a{}\"}}]}}", i, i))
            .collect();
        std::fs::write(&temp, content.join("\n")).unwrap();
        let findings = validate_dataset_size(&temp);
        assert!(findings.iter().all(|f| f.gate_id != "G-D1"));
        std::fs::remove_file(&temp).ok();
    }

    // ── G-Q5: Paged optimizer tests ──

    #[test]
    fn large_model_qlora_without_paged_warns_gq5() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.optimization.optimizer = Some("adamw_8bit".to_string());
        let findings = validate_paged_optimizer(&params, "meta-llama/Llama-2-70b");
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-Q5" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn large_model_qlora_with_paged_passes_gq5() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.optimization.optimizer = Some("paged_adamw_8bit".to_string());
        let findings = validate_paged_optimizer(&params, "meta-llama/Llama-2-70b");
        assert!(findings.iter().all(|f| f.gate_id != "G-Q5"));
    }

    #[test]
    fn small_model_qlora_no_gq5_warning() {
        let mut params = default_params();
        params.quantization.load_in_4bit = true;
        params.optimization.optimizer = Some("adamw_8bit".to_string());
        let findings = validate_paged_optimizer(&params, "Qwen/Qwen2.5-7B");
        assert!(findings.iter().all(|f| f.gate_id != "G-Q5"));
    }

    #[test]
    fn non_qlora_no_gq5_warning() {
        let params = default_params();
        // load_in_4bit is false by default
        let findings = validate_paged_optimizer(&params, "meta-llama/Llama-2-70b");
        assert!(findings.iter().all(|f| f.gate_id != "G-Q5"));
    }

    // ── G-H1: Harness-method compatibility tests ──

    #[test]
    fn no_harness_no_gh1_finding() {
        // Default params: harness=None, trl_trainer=None.
        // Runtime defaults to axolotl — no compatibility issue.
        let params = default_params();
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-H1"));
    }

    #[test]
    fn no_harness_with_trl_trainer_warns_gh1() {
        // trl_trainer set but harness not set to trl — trainer will be ignored.
        let mut params = default_params();
        params.trl_trainer = Some(TrlTrainer::Sft);
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-H1" && f.severity == ValidationSeverity::Warn)
        );
    }

    #[test]
    fn axolotl_with_trl_trainer_warns_gh1() {
        // axolotl ignores trl_trainer (uses its own rl: parameter) — warn, not refuse.
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Axolotl);
        params.trl_trainer = Some(TrlTrainer::Dpo);
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-H1" && f.severity == ValidationSeverity::Warn),
            "Axolotl with trl_trainer should warn, not refuse"
        );
        assert!(
            !findings
                .iter()
                .any(|f| f.gate_id == "G-H1" && f.severity == ValidationSeverity::Refuse),
            "Axolotl with trl_trainer should NOT refuse — Axolotl supports the full training spectrum via rl:"
        );
    }

    #[test]
    fn axolotl_without_trl_trainer_passes_gh1() {
        // axolotl with no TRL trainer — SFT only, no compatibility issue.
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Axolotl);
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-H1"));
    }

    #[test]
    fn trl_with_sft_trainer_passes_gh1() {
        // trl + SFT is a supported combination.
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Trl);
        params.trl_trainer = Some(TrlTrainer::Sft);
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-H1"));
    }

    #[test]
    fn trl_without_trainer_defaults_to_sft_passes_gh1() {
        // trl with no trainer specified — defaults to SFT.
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Trl);
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-H1"));
    }

    /// expect: harness=ludwig with no trl_trainer passes G-H1.
    /// Ludwig is the third harness; it uses its own trainer taxonomy
    /// (trainer.type in YAML), so no trl_trainer is the canonical Ludwig path.
    #[test]
    fn ludwig_without_trl_trainer_passes_gh1() {
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Ludwig);
        let findings = validate_training_params(&params);
        assert!(findings.iter().all(|f| f.gate_id != "G-H1"));
    }

    /// expect: harness=ludwig with trl_trainer set warns G-H1 — trl_trainer is
    /// TRL-specific and Ludwig ignores it (Ludwig has its own trainer.type).
    /// This is a warning, not a refusal: Ludwig can still run SFT; the trl_trainer
    /// field is simply dropped.
    #[test]
    fn ludwig_with_trl_trainer_warns_gh1() {
        let mut params = default_params();
        params.harness = Some(TrainingHarnessId::Ludwig);
        params.trl_trainer = Some(TrlTrainer::Sft);
        let findings = validate_training_params(&params);
        assert!(
            findings
                .iter()
                .any(|f| f.gate_id == "G-H1" && f.severity == ValidationSeverity::Warn)
        );
    }

    // ── G-D0: Dataset format compatibility tests ──────────────────────────

    use crate::dataset::DatasetFormat;
    use std::io::Write;

    fn write_temp_dataset(content: &str, ext: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()
            .expect("create temp file");
        file.write_all(content.as_bytes()).expect("write temp file");
        file
    }

    #[test]
    fn gd0_chatml_dataset_for_sft_is_ready() {
        let file = write_temp_dataset(
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
        assert_eq!(result.detected_format, Some(DatasetFormat::ChatML));
        assert_eq!(result.expected_format, Some(DatasetFormat::ChatML));
        assert!(result.findings.is_empty());
    }

    #[test]
    fn gd0_dpo_dataset_for_dpo_is_ready() {
        let file = write_temp_dataset(
            r#"{"prompt":"hi","chosen":"hello","rejected":"bye"}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), Some("dpo"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
        assert_eq!(result.detected_format, Some(DatasetFormat::PreferenceDpo));
    }

    #[test]
    fn gd0_sharegpt_for_sft_is_ready_via_normalization() {
        let file = write_temp_dataset(
            r#"{"conversations":[{"from":"human","value":"hi"},{"from":"gpt","value":"hello"}]}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
    }

    #[test]
    fn gd0_kto_dataset_for_dpo_needs_mapping() {
        let file = write_temp_dataset(
            r#"{"prompt":"hi","completion":"hello","label":true}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), Some("dpo"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::NeedsMapping);
        assert!(!result.mapping_code.is_empty());
        assert!(result.mapping_code.contains("format_for_dpo"));
        assert!(result.findings.iter().any(|f| f.gate_id == "G-D0"));
    }

    #[test]
    fn gd0_chatml_dataset_for_dpo_is_incompatible() {
        let file = write_temp_dataset(
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), Some("dpo"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Incompatible);
        assert!(
            result
                .findings
                .iter()
                .any(|f| { f.gate_id == "G-D0" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gd0_no_trainer_no_purpose_returns_ready_without_findings() {
        let file = write_temp_dataset(
            r#"{"messages":[{"role":"user","content":"hi"}]}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), None, None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn gd0_unrecognized_extension_warns() {
        let file = write_temp_dataset("some content", "csv");
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
        assert!(result.findings.iter().any(|f| f.gate_id == "G-D0"));
    }

    #[test]
    fn gd0_empty_jsonl_returns_none_format() {
        // Empty .jsonl should not default to ChatML — return None so G-D0 warns.
        let file = write_temp_dataset("", "jsonl");
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(
            result.detected_format, None,
            "empty .jsonl should not be detected as ChatML"
        );
        assert!(result.findings.iter().any(|f| f.gate_id == "G-D0"));
    }

    #[test]
    fn gd0_whitespace_only_jsonl_returns_none_format() {
        let file = write_temp_dataset("\n\n  \n", "jsonl");
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.detected_format, None);
    }

    #[test]
    fn gd0_non_json_jsonl_returns_none_format() {
        // A .jsonl with non-JSON content should not default to ChatML.
        let file = write_temp_dataset("hello world\nthis is not json\n", "jsonl");
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.detected_format, None);
    }

    #[test]
    fn gd0_adapter_purpose_preference_expects_dpo() {
        let file = write_temp_dataset(
            r#"{"prompt":"hi","chosen":"hello","rejected":"bye"}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), None, Some("preference"));
        assert_eq!(result.expected_format, Some(DatasetFormat::PreferenceDpo));
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
    }

    // ── G-R1: Runtime metrics validation tests ────────────────────────────

    #[test]
    fn gr1_no_metrics_no_findings() {
        let metrics = RuntimeMetrics::default();
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.is_empty());
    }

    #[test]
    fn gr1_loss_spike_after_step_100_refuses() {
        let metrics = RuntimeMetrics {
            current_step: Some(150),
            loss: Some(6.0),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Refuse
                && f.message.contains("Loss divergence")
        }));
    }

    #[test]
    fn gr1_loss_spike_before_step_100_no_finding() {
        let metrics = RuntimeMetrics {
            current_step: Some(50),
            loss: Some(6.0),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.is_empty());
    }

    #[test]
    fn gr1_vanishing_loss_warns() {
        let metrics = RuntimeMetrics {
            current_step: Some(10),
            loss: Some(1e-10),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Warn
                && f.message.contains("Vanishing loss")
        }));
    }

    #[test]
    fn gr1_nan_gradient_refuses() {
        let metrics = RuntimeMetrics {
            grad_norm: Some(f64::NAN),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_infinite_gradient_refuses() {
        let metrics = RuntimeMetrics {
            grad_norm: Some(f64::INFINITY),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_error_alert_refuses() {
        let metrics = RuntimeMetrics {
            alerts: vec![TrainingAlert {
                title: "Loss divergence".to_string(),
                level: "error".to_string(),
                text: "Loss 8.0 still high after 200 steps".to_string(),
                step: Some(200),
            }],
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_warn_alert_warns() {
        let metrics = RuntimeMetrics {
            alerts: vec![TrainingAlert {
                title: "Slow convergence".to_string(),
                level: "warn".to_string(),
                text: "Loss decreased <1% over 50 steps".to_string(),
                step: None,
            }],
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Warn })
        );
    }

    // ── G-R1: NaN/infinite loss and unknown alert level tests ─────────────

    #[test]
    fn gr1_nan_loss_refuses() {
        let metrics = RuntimeMetrics {
            current_step: Some(150),
            loss: Some(f64::NAN),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Refuse
                && f.message.contains("NaN loss")
        }));
    }

    #[test]
    fn gr1_infinite_loss_refuses() {
        let metrics = RuntimeMetrics {
            current_step: Some(150),
            loss: Some(f64::INFINITY),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Refuse
                && f.message.contains("Infinite loss")
        }));
    }

    #[test]
    fn gr1_nan_loss_does_not_trigger_divergence_check() {
        // NaN loss should be caught by the explicit NaN check, not silently
        // pass the `loss > 5.0` divergence check (NaN > 5.0 is false in Rust).
        let metrics = RuntimeMetrics {
            current_step: Some(200),
            loss: Some(f64::NAN),
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        // Should have exactly one NaN finding, not a divergence finding.
        let nan_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("NaN loss"))
            .collect();
        let divergence_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("Loss divergence"))
            .collect();
        assert_eq!(nan_findings.len(), 1);
        assert!(
            divergence_findings.is_empty(),
            "NaN loss should not trigger divergence check"
        );
    }

    #[test]
    fn gr1_critical_alert_level_refuses() {
        let metrics = RuntimeMetrics {
            alerts: vec![TrainingAlert {
                title: "Critical training failure".to_string(),
                level: "critical".to_string(),
                text: "Gradient explosion detected".to_string(),
                step: Some(300),
            }],
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_unknown_alert_level_warns_not_info() {
        let metrics = RuntimeMetrics {
            alerts: vec![TrainingAlert {
                title: "Unknown severity".to_string(),
                level: "severe".to_string(),
                text: "Some unknown alert".to_string(),
                step: None,
            }],
            ..Default::default()
        };
        let findings = validate_runtime_metrics(&metrics);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Warn }),
            "Unknown alert level should default to Warn, not Info"
        );
    }

    #[test]
    fn training_alert_deserializes_with_missing_level() {
        // A manifest alert without the `level` field should deserialize
        // with the default level ("warn"), not fail parsing.
        let json = r#"{"title":"Test","text":"body"}"#;
        let alert: TrainingAlert =
            serde_json::from_str(json).expect("deserialize with missing level");
        assert_eq!(alert.level, "warn");
        assert_eq!(alert.title, "Test");
    }

    #[test]
    fn training_alert_deserializes_with_all_fields_missing() {
        // All fields have serde defaults — an empty JSON object should deserialize.
        let json = r#"{}"#;
        let alert: TrainingAlert = serde_json::from_str(json).expect("deserialize empty alert");
        assert_eq!(alert.level, "warn");
        assert!(alert.title.is_empty());
        assert!(alert.text.is_empty());
        assert_eq!(alert.step, None);
    }

    // ── G-P1: Persistence preflight tests ─────────────────────────────────

    use crate::providers::TrainingHostId;

    #[test]
    fn gp1_runpod_with_env_vars_configured_passes() {
        let findings = validate_persistence(&TrainingHostId::Runpod, &Ok(()));
        assert!(
            findings.is_empty(),
            "Runpod with env vars configured should not emit a finding"
        );
    }

    #[test]
    fn gp1_runpod_without_env_vars_refuses() {
        let findings = validate_persistence(
            &TrainingHostId::Runpod,
            &Err("HKASK_HF_ARTIFACT_OWNER must be set and non-empty".to_string()),
        );
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-P1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gp1_deepinfra_warns_no_auto_upload() {
        let findings = validate_persistence(&TrainingHostId::DeepInfra, &Ok(()));
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-P1" && f.severity == ValidationSeverity::Warn })
        );
    }

    #[test]
    fn gp1_nebius_warns_no_auto_upload() {
        let findings = validate_persistence(&TrainingHostId::Nebius, &Ok(()));
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-P1" && f.severity == ValidationSeverity::Warn })
        );
    }

    #[test]
    fn gp1_runpod_refusal_message_mentions_ephemeral_pod() {
        let findings = validate_persistence(
            &TrainingHostId::Runpod,
            &Err("HF_TOKEN must be set".to_string()),
        );
        let refusal = findings
            .iter()
            .find(|f| f.severity == ValidationSeverity::Refuse);
        assert!(refusal.is_some());
        assert!(refusal.unwrap().message.contains("ephemeral pod"));
    }
}
