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
    LoraBias, LoraInit, LoraParams, QuantizationParams, TrainingHarnessId, TrainingMethod,
    TrainingParams,
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

/// Allocation-free G-M1..G-M4 decisions shared by production rendering and Kani.
/// Each slot carries the severity of one existing diagnostic, in emission order.
struct MathDecisions {
    non_noop: Option<ValidationSeverity>,
    base_weights: Option<ValidationSeverity>,
    merge: Option<ValidationSeverity>,
    zero_rank: Option<ValidationSeverity>,
    zero_alpha: Option<ValidationSeverity>,
    rslora: Option<ValidationSeverity>,
    rank_warning: Option<ValidationSeverity>,
    rank_refusal: Option<ValidationSeverity>,
}

fn math_decisions(
    r: u32,
    alpha: u32,
    use_rslora: bool,
    init: Option<&LoraInit>,
    bias: &LoraBias,
) -> MathDecisions {
    MathDecisions {
        non_noop: init
            .is_some_and(|init| !init.is_noop_at_init())
            .then_some(ValidationSeverity::Warn),
        base_weights: init
            .is_some_and(LoraInit::modifies_base_weights)
            .then_some(ValidationSeverity::Warn),
        merge: bias.breaks_merge().then_some(ValidationSeverity::Warn),
        zero_rank: (r == 0).then_some(ValidationSeverity::Refuse),
        zero_alpha: (alpha == 0).then_some(ValidationSeverity::Refuse),
        rslora: (r > 64 && !use_rslora).then_some(ValidationSeverity::Warn),
        rank_warning: (r > 128).then_some(ValidationSeverity::Warn),
        rank_refusal: (r > 256).then_some(ValidationSeverity::Refuse),
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
    let lora = &params.lora;
    let decisions = math_decisions(
        lora.r,
        lora.alpha,
        lora.use_rslora,
        lora.init_lora_weights.as_ref(),
        &lora.bias,
    );

    // G-M1: No-op-at-init invariant.
    validate_noop_at_init(lora, &decisions, &mut findings);

    // G-M2: Merge equivalence.
    validate_merge_equivalence(lora, &decisions, &mut findings);

    // G-M3: Scaling form.
    validate_scaling_form(lora, &decisions, &mut findings);

    // G-M4: Rank budget.
    validate_rank_budget(lora, &decisions, &mut findings);

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
fn validate_noop_at_init(
    lora: &LoraParams,
    decisions: &MathDecisions,
    findings: &mut Vec<ValidationFinding>,
) {
    if let Some(ref init) = lora.init_lora_weights {
        if let Some(severity) = decisions.non_noop {
            findings.push(ValidationFinding {
                gate_id: "G-M1",
                severity,
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
        if let Some(severity) = decisions.base_weights {
            findings.push(ValidationFinding {
                gate_id: "G-M1",
                severity,
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
fn validate_merge_equivalence(
    lora: &LoraParams,
    decisions: &MathDecisions,
    findings: &mut Vec<ValidationFinding>,
) {
    if let Some(severity) = decisions.merge {
        findings.push(ValidationFinding {
            gate_id: "G-M2",
            severity,
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
fn validate_scaling_form(
    lora: &LoraParams,
    decisions: &MathDecisions,
    findings: &mut Vec<ValidationFinding>,
) {
    if let Some(severity) = decisions.zero_rank {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity,
            message: "LoRA rank r=0 — division by zero in scaling α/r".to_string(),
            source: "LoRA paper §4.1 (α/r scaling); rsLoRA arXiv:2312.03732",
            remediation: "Set r to a positive integer (typical: 8–64)".to_string(),
        });
    }
    if let Some(severity) = decisions.zero_alpha {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity,
            message: "LoRA alpha=0 — scaling factor is zero, adapter has no effect".to_string(),
            source: "LoRA paper §4.1 (α/r scaling)",
            remediation: "Set alpha to a positive integer (typical: 2×r)".to_string(),
        });
    }
    // rsLoRA recommendation for high rank.
    if let Some(severity) = decisions.rslora {
        findings.push(ValidationFinding {
            gate_id: "G-M3",
            severity,
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
fn validate_rank_budget(
    lora: &LoraParams,
    decisions: &MathDecisions,
    findings: &mut Vec<ValidationFinding>,
) {
    if let Some(severity) = decisions.rank_warning {
        findings.push(ValidationFinding {
            gate_id: "G-M4",
            severity,
            message: format!(
                "LoRA rank r={} > 128 — defeats low-rank premise; consider full fine-tuning",
                lora.r
            ),
            source: "LoRA paper §4.3 (rank sufficiency experiments)",
            remediation: "Reduce r to ≤128, or use full fine-tuning if the task requires high rank"
                .to_string(),
        });
    }
    if let Some(severity) = decisions.rank_refusal {
        findings.push(ValidationFinding {
            gate_id: "G-M4",
            severity,
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

/// Kani checks the allocation-free production decision core, not diagnostic
/// strings, Vec allocation, MCP serialization, or provider behavior. The public
/// characterization test pins that rendering boundary separately. No alternate
/// cfg(kani) implementation or solver stubs are used.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn gm3_refuse_iff_degenerate_scaling() {
        let r: u32 = kani::any();
        let alpha: u32 = kani::any();
        let use_rslora: bool = kani::any();
        let decisions = math_decisions(r, alpha, use_rslora, None, &LoraBias::None);
        assert_eq!(
            decisions.zero_rank,
            (r == 0).then_some(ValidationSeverity::Refuse)
        );
        assert_eq!(
            decisions.zero_alpha,
            (alpha == 0).then_some(ValidationSeverity::Refuse)
        );
        assert_eq!(
            decisions.rslora,
            (r > 64 && !use_rslora).then_some(ValidationSeverity::Warn)
        );
        kani::cover!(r == 0 && alpha == 0);
        kani::cover!(r > 64 && alpha > 0 && !use_rslora);
    }

    #[kani::proof]
    fn gm4_findings_follow_rank_thresholds() {
        let r: u32 = kani::any();
        let decisions = math_decisions(r, 32, false, None, &LoraBias::None);
        assert_eq!(
            decisions.rank_warning,
            (r > 128).then_some(ValidationSeverity::Warn)
        );
        assert_eq!(
            decisions.rank_refusal,
            (r > 256).then_some(ValidationSeverity::Refuse)
        );
        kani::cover!(r <= 128);
        kani::cover!(r > 128 && r <= 256);
        kani::cover!(r > 256);
    }

    #[kani::proof]
    fn gm1_clean_iff_noop_init() {
        let init: Option<LoraInit> = kani::any();
        let decisions = math_decisions(16, 32, false, init.as_ref(), &LoraBias::None);
        let noop = matches!(init, None | Some(LoraInit::Default) | Some(LoraInit::Eva));
        let modifies_base = matches!(
            init,
            Some(
                LoraInit::Pissa
                    | LoraInit::PissaNiter(_)
                    | LoraInit::Loftq
                    | LoraInit::Olora
                    | LoraInit::Corda
            )
        );
        assert_eq!(
            decisions.non_noop,
            (!noop).then_some(ValidationSeverity::Warn)
        );
        assert_eq!(
            decisions.base_weights,
            modifies_base.then_some(ValidationSeverity::Warn)
        );
        assert_eq!(
            decisions.non_noop.is_none() && decisions.base_weights.is_none(),
            noop
        );
        kani::cover!(noop);
        kani::cover!(modifies_base);
        kani::cover!(matches!(init, Some(LoraInit::PissaNiter(u32::MAX))));
    }

    #[kani::proof]
    fn safe_region_has_no_refusals() {
        let r: u32 = kani::any();
        let alpha: u32 = kani::any();
        kani::assume(r >= 1 && r <= 128);
        kani::assume(alpha >= 1);
        let decisions = math_decisions(r, alpha, false, None, &LoraBias::None);
        for decision in [
            decisions.non_noop,
            decisions.base_weights,
            decisions.merge,
            decisions.zero_rank,
            decisions.zero_alpha,
            decisions.rslora,
            decisions.rank_warning,
            decisions.rank_refusal,
        ] {
            assert_ne!(decision, Some(ValidationSeverity::Refuse));
        }
        kani::cover!(r == 128 && alpha == u32::MAX);
    }

    /// Additional coverage: the merge warning decision ranges over every bias.
    #[kani::proof]
    fn gm2_warns_iff_bias_breaks_merge() {
        let bias: LoraBias = kani::any();
        let decisions = math_decisions(16, 32, false, None, &bias);
        assert_eq!(
            decisions.merge,
            (!matches!(bias, LoraBias::None)).then_some(ValidationSeverity::Warn)
        );
        kani::cover!(matches!(bias, LoraBias::None));
        kani::cover!(matches!(bias, LoraBias::All));
        kani::cover!(matches!(bias, LoraBias::LoraOnly));
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
