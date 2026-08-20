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
pub use param_gates::{
    ValidationFinding, ValidationSeverity, has_refusals, validate_dataset_size,
    validate_paged_optimizer, validate_training_params,
};

// ── Dataset-format validation (G-D0) — extracted to `lora_validation/dataset_format.rs`
mod dataset_format;
pub use dataset_format::{DatasetFormatResult, DatasetFormatVerdict, validate_dataset_format};

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
pub use runtime_metrics::validate_runtime_metrics;

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
    fn gd0_no_trainer_no_purpose_returns_ready_with_info_finding() {
        // No trainer/purpose declared → format compatibility not checked.
        // Verdict stays Ready (the dataset may be fine) but an Info finding
        // makes the "not checked" state visible, distinguishing it from
        // "validated and compatible."
        let file = write_temp_dataset(
            r#"{"messages":[{"role":"user","content":"hi"}]}
"#,
            "jsonl",
        );
        let result = validate_dataset_format(file.path(), None, None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Ready);
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.gate_id == "G-D0" && f.severity == ValidationSeverity::Info),
            "expected an Info finding when no trainer/purpose is declared"
        );
    }

    #[test]
    fn gd0_unrecognized_extension_warns() {
        // .csv with an expected format derivable (sft → ChatML) but detection
        // failed → Undetermined (not Ready), so a caller branching on verdict
        // does not treat an undetectable dataset as ready-to-train.
        let file = write_temp_dataset("some content", "csv");
        let result = validate_dataset_format(file.path(), Some("sft"), None);
        assert_eq!(result.verdict, DatasetFormatVerdict::Undetermined);
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
    //
    // These tests construct CompletionManifest instances (not RuntimeMetrics,
    // which was deleted) and call validate_runtime_metrics(&manifest).

    use crate::huggingface::{CompletionManifest, TrainingArtifact};

    fn test_manifest() -> CompletionManifest {
        CompletionManifest {
            job_id: "test".to_string(),
            status: "success".to_string(),
            dataset_sha256: String::new(),
            adapter: TrainingArtifact {
                repository: String::new(),
                revision: String::new(),
                path: String::new(),
                sha256: String::new(),
            },
            finished_at: String::new(),
            base_model: None,
            harness: None,
            training_duration_secs: None,
            loss: None,
            grad_norm: None,
            current_step: None,
            total_steps: None,
            alerts: Vec::new(),
            output_dir: None,
        }
    }

    #[test]
    fn gr1_no_metrics_no_findings() {
        let manifest = test_manifest();
        let findings = validate_runtime_metrics(&manifest);
        assert!(findings.is_empty());
    }

    #[test]
    fn gr1_loss_spike_after_step_100_refuses() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(150);
        manifest.loss = Some(6.0);
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_loss_spike_before_step_100_no_finding() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(50);
        manifest.loss = Some(6.0);
        let findings = validate_runtime_metrics(&manifest);
        assert!(findings.is_empty());
    }

    #[test]
    fn gr1_vanishing_loss_warns() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(10);
        manifest.loss = Some(1e-10);
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Warn })
        );
    }

    #[test]
    fn gr1_nan_gradient_refuses() {
        let mut manifest = test_manifest();
        manifest.grad_norm = Some(f64::NAN);
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_infinite_gradient_refuses() {
        let mut manifest = test_manifest();
        manifest.grad_norm = Some(f64::INFINITY);
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_error_alert_refuses() {
        let mut manifest = test_manifest();
        manifest.alerts = vec![crate::huggingface::TrainingAlert {
            title: "Loss divergence".to_string(),
            level: "error".to_string(),
            text: "Loss 8.0 still high after 200 steps".to_string(),
            step: Some(200),
        }];
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_warn_alert_warns() {
        let mut manifest = test_manifest();
        manifest.alerts = vec![crate::huggingface::TrainingAlert {
            title: "Slow convergence".to_string(),
            level: "warn".to_string(),
            text: "Loss decreased <1% over 50 steps".to_string(),
            step: None,
        }];
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Warn })
        );
    }

    #[test]
    fn gr1_nan_loss_refuses() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(150);
        manifest.loss = Some(f64::NAN);
        let findings = validate_runtime_metrics(&manifest);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Refuse
                && f.message.contains("NaN loss")
        }));
    }

    #[test]
    fn gr1_infinite_loss_refuses() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(150);
        manifest.loss = Some(f64::INFINITY);
        let findings = validate_runtime_metrics(&manifest);
        assert!(findings.iter().any(|f| {
            f.gate_id == "G-R1"
                && f.severity == ValidationSeverity::Refuse
                && f.message.contains("Infinite loss")
        }));
    }

    #[test]
    fn gr1_nan_loss_does_not_trigger_divergence_check() {
        let mut manifest = test_manifest();
        manifest.current_step = Some(200);
        manifest.loss = Some(f64::NAN);
        let findings = validate_runtime_metrics(&manifest);
        let nan_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("NaN loss"))
            .collect();
        let divergence_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.message.contains("Loss divergence"))
            .collect();
        assert_eq!(nan_findings.len(), 1);
        assert!(divergence_findings.is_empty());
    }

    #[test]
    fn gr1_critical_alert_level_refuses() {
        let mut manifest = test_manifest();
        manifest.alerts = vec![crate::huggingface::TrainingAlert {
            title: "Critical training failure".to_string(),
            level: "critical".to_string(),
            text: "Gradient explosion detected".to_string(),
            step: Some(300),
        }];
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Refuse })
        );
    }

    #[test]
    fn gr1_unknown_alert_level_warns_not_info() {
        let mut manifest = test_manifest();
        manifest.alerts = vec![crate::huggingface::TrainingAlert {
            title: "Unknown severity".to_string(),
            level: "severe".to_string(),
            text: "Some unknown alert".to_string(),
            step: None,
        }];
        let findings = validate_runtime_metrics(&manifest);
        assert!(
            findings
                .iter()
                .any(|f| { f.gate_id == "G-R1" && f.severity == ValidationSeverity::Warn }),
            "Unknown alert level should default to Warn, not Info"
        );
    }
}
