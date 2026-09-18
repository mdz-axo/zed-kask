//! LoRA hyperparameter validation gates — each `validate_*` enforces a
//! training-config invariant (init method, merge equivalence, scaling form,
//! rank budget, QLoRA quantization, compute dtype, silent upcast, harness
//! compatibility) and appends `ValidationFinding`s. `has_refusals` reports
//! whether any finding is `Refuse`.
//!
//! Anchored to: LoRA (arXiv:2106.09685), QLoRA (arXiv:2305.14314), rsLoRA
//! (arXiv:2312.03732), DoRA (arXiv:2402.09353), PiSSA (arXiv:2404.02948),
//! Razin et al. (arXiv:2410.21228), and PEFT v0.19.0.
//!
//! Extracted from `lora_validation.rs` (deep-module split: the LoRA-param
//! gates are independent of dataset-format compatibility and runtime metrics).

use crate::providers::types::{
    LoraParams, QuantizationParams, TrainingHarnessId, TrainingMethod, TrainingParams,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidationSeverity {
    /// Hard refusal — do not submit the job.
    Refuse,
    /// Soft warning — submit but flag in telemetry.
    Warn,
    /// Informational — no action needed.
    Info,
}

impl ValidationSeverity {
    /// String representation for tracing spans and JSON serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }
}

/// A single validation finding from a math-contract gate.
#[derive(Debug, Clone)]
pub(crate) struct ValidationFinding {
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

impl ValidationFinding {
    /// Serialize to a JSON object for MCP tool responses.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "gate_id": self.gate_id,
            "severity": self.severity.as_str(),
            "message": self.message,
            "source": self.source,
            "remediation": self.remediation,
        })
    }
}

/// Validate training params against the LoRA/QLoRA math-contract gates.
///
/// Returns a list of findings. If any finding has `Refuse` severity, the
/// caller must not submit the job. `Warn` findings should be logged but
/// do not block submission.
pub(crate) fn validate_training_params(params: &TrainingParams) -> Vec<ValidationFinding> {
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
/// This gate is called from `training_validate_config`. The `training_submit`
/// tool does not run G-D1 — run `training_validate_config` first to check dataset size.
pub(crate) fn validate_dataset_size(dataset_path: &std::path::Path) -> Vec<ValidationFinding> {
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
pub(crate) fn validate_paged_optimizer(
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

/// G-H1: harness-method compatibility.
///
/// Axolotl's retained renderer produces SFT configuration. Ludwig renders SFT,
/// DPO, KTO, ORPO, and GRPO through `trainer.type`. Unsupported combinations
/// are refused rather than silently dropping the selected method.
fn validate_harness_compatibility(params: &TrainingParams, findings: &mut Vec<ValidationFinding>) {
    let harness = params.harness.unwrap_or(TrainingHarnessId::Axolotl);
    let method = params.training_method.unwrap_or_default();

    if harness == TrainingHarnessId::Axolotl && method != TrainingMethod::Sft {
        findings.push(ValidationFinding {
            gate_id: "G-H1",
            severity: ValidationSeverity::Refuse,
            message: format!(
                "harness=axolotl does not render training_method={}",
                method.as_dataset_preference()
            ),
            source: "Axolotl renderer contract: registry/templates/training/axolotl-lora.j2",
            remediation:
                "Select harness=ludwig for DPO, KTO, ORPO, or GRPO, or select training_method=sft"
                    .to_string(),
        });
    }
}

/// Returns true if any finding has `Refuse` severity — the job must not be submitted.
pub(crate) fn has_refusals(findings: &[ValidationFinding]) -> bool {
    findings
        .iter()
        .any(|f| f.severity == ValidationSeverity::Refuse)
}

/// Kani proof harnesses for the math-contract gates (goedel-gap-closure
/// plan slice S4, A-R2 pilot). Anchored to the gate catalog
/// (`kask/docs/reference/lora-training-catalog.md`, G-M1..G-M4) and the
/// `.agents/skills/lora-training/` skill's `audit-config` phase.
///
/// The `kani` library is provided by the Kani toolchain at `cargo kani`
/// time; the crates.io `kani` crate is a 3-line placeholder and must NOT
/// be added as a dependency. This module is `#[cfg(kani)]`-gated: regular
/// builds never compile it, so there is no Cargo.toml or lockfile churn
/// on the shared tree. Syntax is still checked by regular builds; the
/// semantic proofs run only under `cargo kani -p hkask-mcp-training`
/// (https://model-checking.github.io/kani/).
#[cfg(kani)]
mod proofs {
    use super::*;
    use crate::providers::types::LoraInit;

    fn lora_with(r: u32, alpha: u32) -> LoraParams {
        LoraParams {
            r,
            alpha,
            ..LoraParams::default()
        }
    }

    fn refuse_count(findings: &[ValidationFinding]) -> usize {
        findings
            .iter()
            .filter(|f| f.severity == ValidationSeverity::Refuse)
            .count()
    }

    /// G-M3: refuse fires exactly for degenerate scaling (r=0 or alpha=0).
    #[kani::proof]
    fn gm3_refuse_iff_degenerate_scaling() {
        let r: u32 = kani::any();
        let alpha: u32 = kani::any();
        let lora = lora_with(r, alpha);
        let mut findings = Vec::new();
        validate_scaling_form(&lora, &mut findings);
        assert_eq!(
            refuse_count(&findings),
            (r == 0) as usize + (alpha == 0) as usize,
            "G-M3 must refuse exactly the degenerate-scaling configs"
        );
    }

    /// G-M4: warn fires for r>128, refuse for r>256; both fire above 256.
    #[kani::proof]
    fn gm4_findings_follow_rank_thresholds() {
        let r: u32 = kani::any();
        let lora = lora_with(r, 32);
        let mut findings = Vec::new();
        validate_rank_budget(&lora, &mut findings);
        let expected = if r > 256 {
            2
        } else if r > 128 {
            1
        } else {
            0
        };
        assert_eq!(
            findings.len(),
            expected,
            "G-M4 findings must track the 128/256 rank thresholds exactly"
        );
    }

    /// G-M1: no findings iff the initializer is unset or a step-0 no-op
    /// (None, Default, or EVA — the catalog's no-op set).
    #[kani::proof]
    fn gm1_clean_iff_noop_init() {
        let init: Option<LoraInit> = kani::any();
        let is_clean = matches!(init, None | Some(LoraInit::Default) | Some(LoraInit::Eva));
        let lora = LoraParams {
            init_lora_weights: init,
            ..LoraParams::default()
        };
        let mut findings = Vec::new();
        validate_noop_at_init(&lora, &mut findings);
        assert_eq!(
            findings.is_empty(),
            is_clean,
            "G-M1 must flag exactly the non-noop initializers"
        );
    }

    /// Safe region: the documented standard config region
    /// (r in 1..=128, alpha >= 1, default bias/init) produces zero
    /// refusals across G-M1..G-M4.
    #[kani::proof]
    fn safe_region_has_no_refusals() {
        let r: u32 = kani::any();
        let alpha: u32 = kani::any();
        kani::assume(r >= 1 && r <= 128);
        kani::assume(alpha >= 1);
        let lora = lora_with(r, alpha);
        let mut findings = Vec::new();
        validate_noop_at_init(&lora, &mut findings);
        validate_merge_equivalence(&lora, &mut findings);
        validate_scaling_form(&lora, &mut findings);
        validate_rank_budget(&lora, &mut findings);
        assert!(
            !has_refusals(&findings),
            "the safe config region must never produce a refuse finding"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axolotl_refuses_non_sft_training_method() {
        let params = TrainingParams {
            harness: Some(TrainingHarnessId::Axolotl),
            training_method: Some(TrainingMethod::Dpo),
            ..TrainingParams::default()
        };
        let mut findings = Vec::new();

        validate_harness_compatibility(&params, &mut findings);

        assert!(findings.iter().any(|finding| {
            finding.gate_id == "G-H1" && finding.severity == ValidationSeverity::Refuse
        }));
    }

    #[test]
    fn ludwig_accepts_grpo_training_method() {
        let params = TrainingParams {
            harness: Some(TrainingHarnessId::Ludwig),
            training_method: Some(TrainingMethod::Grpo),
            ..TrainingParams::default()
        };
        let mut findings = Vec::new();

        validate_harness_compatibility(&params, &mut findings);

        assert!(findings.is_empty());
    }
}
