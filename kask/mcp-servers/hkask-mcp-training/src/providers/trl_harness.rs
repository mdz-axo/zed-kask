//! TRL harness — renders HuggingFace TRL Python training scripts from canonical
//! `TrainingParams`.
//!
//! Mirrors `AxolotlHarness` but emits a Python script (using TRL's
//! `SFTTrainer` + `SFTConfig`) instead of axolotl YAML. The script is injected
//! into the RunPod pod as the `HKASK_TRL_SCRIPT` env var (parallel to
//! `HKASK_AXOLOTL_CONFIG`), and the pod's entrypoint writes it to
//! `/workspace/train.py` before running `python /workspace/train.py`.
//!
//! All trainers (SFT, DPO, KTO, ORPO, Reward) are implemented via two templates:
//! `trl-sft.j2` (SFT) and `trl-preference.j2` (DPO/KTO/ORPO/Reward).
//!
//! TRL version pinning: the pod template must install a pinned TRL version
//! (see `docs/how-to/runpod-lora-training-guide.md` Lesson 12). Version
//! mismatches between training and inference cause garbage output (Lesson 6
//! precedent: PiSSA portability). The rendered script includes a version
//! assertion that fails fast if the installed TRL version is incompatible.
//!
//! References:
//! - TRL SFTTrainer: https://huggingface.co/docs/trl/main/en/sft_trainer
//! - TRL SFTConfig:  https://huggingface.co/docs/trl/main/en/sft_trainer#sftconfig
//! - PEFT LoraConfig: https://huggingface.co/docs/peft/v0.19.0/package_reference/lora

use crate::providers::harness::HarnessAdapter;
use crate::providers::types::*;
use std::path::PathBuf;

/// Renders TRL Python training scripts from canonical `TrainingParams`.
///
/// All trainers are implemented. The trainer is selected from
/// `job.params.trl_trainer` (defaults to `Sft` when `None`).
pub struct TrlHarness;

impl HarnessAdapter for TrlHarness {
    fn render_config(&self, job: &TrainingJob) -> Result<String, ProviderError> {
        let trainer = job.params.trl_trainer.unwrap_or_default();
        match trainer {
            TrlTrainer::Sft => self.render_sft_script(job),
            TrlTrainer::Dpo => self.render_preference_script(job, "dpo"),
            TrlTrainer::Kto => self.render_preference_script(job, "kto"),
            TrlTrainer::Orpo => self.render_preference_script(job, "orpo"),
            TrlTrainer::Reward => self.render_preference_script(job, "reward"),
        }
    }

    fn output_dir(&self, job_id: &str) -> PathBuf {
        // Same canonical output dir as AxolotlHarness — the RunPod pod contract
        // is harness-agnostic (/workspace/outputs/{job_id}).
        PathBuf::from(format!("/workspace/outputs/{}", job_id))
    }

    fn completion_marker(&self, job_id: &str) -> PathBuf {
        // TRL + PEFT saves adapter_model.safetensors (same as axolotl).
        self.output_dir(job_id).join("adapter_model.safetensors")
    }

    fn harness_id(&self) -> TrainingHarnessId {
        TrainingHarnessId::Trl
    }
}

impl TrlHarness {
    /// Build the base template context shared by SFT and preference scripts.
    ///
    /// Contains all fields common to both script types: base model, LoRA
    /// params, optimization params, dataset paths, and output dir. Each
    /// render method calls this then adds its specific fields.
    fn build_base_context(&self, job: &TrainingJob) -> serde_json::Map<String, serde_json::Value> {
        let p = &job.params;
        let lo = &p.lora;
        let opt = &p.optimization;
        let (dataset_path, data_files) = job
            .artifacts
            .as_ref()
            .map(|artifacts| {
                (
                    artifacts.dataset.repository.clone(),
                    artifacts.dataset.path.clone(),
                )
            })
            .unwrap_or_else(|| (job.dataset_path.display().to_string(), String::new()));

        let mut context = serde_json::Map::from_iter([
            ("base_model".to_string(), serde_json::json!(job.base_model)),
            (
                "load_in_4bit".to_string(),
                serde_json::json!(p.quantization.load_in_4bit),
            ),
            ("lora_r".to_string(), serde_json::json!(lo.r)),
            ("lora_alpha".to_string(), serde_json::json!(lo.alpha)),
            ("lora_dropout".to_string(), serde_json::json!(lo.dropout)),
            (
                "lora_target_modules".to_string(),
                serde_json::json!(lo.target_modules),
            ),
            ("dataset_path".to_string(), serde_json::json!(dataset_path)),
            ("data_files".to_string(), serde_json::json!(data_files)),
            ("num_epochs".to_string(), serde_json::json!(p.num_epochs)),
            (
                "learning_rate".to_string(),
                serde_json::json!(p.learning_rate),
            ),
            (
                "micro_batch_size".to_string(),
                serde_json::json!(p.batch_size),
            ),
            (
                "gradient_accumulation_steps".to_string(),
                serde_json::json!(opt.gradient_accumulation_steps),
            ),
            (
                "output_dir".to_string(),
                serde_json::json!(self.output_dir(&job.id).display().to_string()),
            ),
        ]);

        // Optional fields — always inserted (with empty defaults) so the
        // template's {% if field %} checks work with minijinja Strict
        // undefined behavior.
        context.insert(
            "peft_init_lora_weights".to_string(),
            serde_json::json!(
                lo.init_lora_weights
                    .as_ref()
                    .map(|init| init.as_config_value())
            ),
        );
        context.insert(
            "optim".to_string(),
            serde_json::json!(opt.optimizer.clone()),
        );
        context.insert(
            "lr_scheduler".to_string(),
            serde_json::json!(opt.lr_scheduler.clone()),
        );
        context.insert(
            "sequence_len".to_string(),
            serde_json::json!(p.sequence.sequence_len.map(|v| v.to_string())),
        );
        context.insert(
            "warmup_steps".to_string(),
            serde_json::json!(opt.warmup_steps.map(|v| v.to_string())),
        );
        context.insert(
            "max_grad_norm".to_string(),
            serde_json::json!(opt.max_grad_norm.map(|v| v.to_string())),
        );
        context.insert(
            "use_rslora".to_string(),
            serde_json::json!(lo.use_rslora.to_string()),
        );
        context.insert(
            "use_dora".to_string(),
            serde_json::json!(lo.use_dora.to_string()),
        );
        context.insert(
            "weight_decay".to_string(),
            serde_json::json!(opt.weight_decay),
        );

        // Quantization params — rendered from QuantizationParams, not hardcoded.
        context.insert(
            "bnb_4bit_quant_type".to_string(),
            serde_json::json!(
                p.quantization
                    .bnb_4bit_quant_type
                    .clone()
                    .unwrap_or_default()
            ),
        );
        context.insert(
            "bnb_4bit_compute_dtype".to_string(),
            serde_json::json!(
                p.quantization
                    .bnb_4bit_compute_dtype
                    .clone()
                    .unwrap_or_default()
            ),
        );
        context.insert(
            "bnb_4bit_use_double_quant".to_string(),
            serde_json::json!(p.quantization.bnb_4bit_use_double_quant),
        );

        // Optimization fields — wired from TrainingParams so operators can
        // control flash attention, gradient checkpointing, bf16, and packing.
        context.insert("bf16".to_string(), serde_json::json!(p.advanced.bf16));
        context.insert(
            "sample_packing".to_string(),
            serde_json::json!(p.sequence.sample_packing),
        );
        if let Some(ref gc) = p.advanced.gradient_checkpointing {
            context.insert("gradient_checkpointing".to_string(), serde_json::json!(gc));
        }
        if let Some(ref attn) = p.advanced.attn_implementation {
            context.insert(
                "flash_attention".to_string(),
                serde_json::json!((attn == "flash_attention_2").to_string()),
            );
        }

        context
    }

    /// Render a TRL SFTTrainer Python script from canonical `TrainingParams`.
    ///
    /// The script is rendered via `registry/templates/training/trl-sft.j2`.
    fn render_sft_script(&self, job: &TrainingJob) -> Result<String, ProviderError> {
        let mut context = self.build_base_context(job);

        // TRL-specific: packing — enables example packing for efficiency.
        context.insert(
            "packing".to_string(),
            serde_json::json!(job.params.sequence.sample_packing),
        );

        let template_root = Self::template_root();
        let template_path = template_root.join("templates/training/trl-sft.j2");
        self.render_template(&template_path, context, "TRL SFT")
    }

    /// Render a TRL preference optimization script (DPO/KTO/ORPO/Reward).
    fn render_preference_script(
        &self,
        job: &TrainingJob,
        trainer_name: &str,
    ) -> Result<String, ProviderError> {
        let trainer = job.params.trl_trainer.unwrap_or_default();
        let mut context = self.build_base_context(job);

        // Preference trainer parameters.
        context.insert("trainer_name".to_string(), serde_json::json!(trainer_name));
        context.insert(
            "trainer_class".to_string(),
            serde_json::json!(trainer.trainer_class()),
        );
        context.insert(
            "config_class".to_string(),
            serde_json::json!(trainer.config_class()),
        );
        context.insert(
            "expected_dataset_format".to_string(),
            serde_json::json!(trainer.expected_dataset_format()),
        );

        let template_root = Self::template_root();
        let template_path = template_root.join("templates/training/trl-preference.j2");
        self.render_template(&template_path, context, "TRL preference")
    }

    /// Resolve the template root directory.
    fn template_root() -> PathBuf {
        std::env::var_os("HKASK_TEMPLATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let working_directory_root = PathBuf::from("registry");
                if working_directory_root.is_dir() {
                    working_directory_root
                } else {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("../..")
                        .join("registry")
                }
            })
    }

    /// Render a Jinja template with the given context.
    fn render_template(
        &self,
        template_path: &std::path::Path,
        context: serde_json::Map<String, serde_json::Value>,
        label: &str,
    ) -> Result<String, ProviderError> {
        let template = std::fs::read_to_string(template_path).map_err(|error| {
            ProviderError::InvalidConfig(format!(
                "Read {label} template {}: {error}",
                template_path.display()
            ))
        })?;
        let mut environment = minijinja::Environment::new();
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        environment
            .render_str(&template, serde_json::Value::Object(context))
            .map(|script| script.trim().to_string() + "\n")
            .map_err(|error| {
                ProviderError::InvalidConfig(format!("Render {label} template: {error}"))
            })
    }
}
