//! Harness definitions — the tooling that runs on top of a host.
//!
//! A harness renders training configuration (YAML, Python script) from canonical
//! `TrainingParams`. Hosts bind a harness and use it to generate the config/script
//! they dispatch to their compute backend.
//!
//! Harness → Host mapping:
//!   Axolotl → Runpod
//!   TRL     → Runpod
//!   Ludwig  → Runpod

use crate::providers::types::*;
use std::path::PathBuf;

// ── HarnessAdapter trait ───────────────────────────────────────────────────

/// Renders training configuration in a harness-specific format.
///
/// The *harness* is the tooling that orchestrates training (axolotl CLI,
/// unsloth Python, TRL SFTTrainer). The *host* is where compute runs.
/// Each host binds exactly one harness; the harness generates the config
/// or script that the host dispatches.
///
/// pre:  job.params carries full expanded TrainingParams
/// post: returns harness-native config string (YAML, Python script, etc.)
///
/// MDS: Composition — CAN render_config ON TrainingJob VIA HarnessAdapter
pub trait HarnessAdapter: Send + Sync {
    /// Render the training configuration in the harness's native format.
    fn render_config(&self, job: &TrainingJob) -> Result<String, ProviderError>;

    /// Directory where the harness outputs adapter weights.
    fn output_dir(&self, job_id: &str) -> PathBuf;

    /// File whose existence signals training completion.
    fn completion_marker(&self, job_id: &str) -> PathBuf;

    /// The harness identifier for Regulation spans.
    fn harness_id(&self) -> TrainingHarnessId;
}

// ── Axolotl harness ────────────────────────────────────────────────────────

/// Renders axolotl YAML configuration from canonical TrainingParams.
pub struct AxolotlHarness;

impl HarnessAdapter for AxolotlHarness {
    fn render_config(&self, job: &TrainingJob) -> Result<String, ProviderError> {
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
        for (key, value) in [
            (
                "peft_init_lora_weights",
                lo.init_lora_weights
                    .as_ref()
                    .map(|init| init.as_config_value()),
            ),
            ("optim", opt.optimizer.clone()),
            ("lr_scheduler", opt.lr_scheduler.clone()),
            (
                "sequence_len",
                p.sequence.sequence_len.map(|value| value.to_string()),
            ),
            (
                "warmup_steps",
                opt.warmup_steps.map(|value| value.to_string()),
            ),
            (
                "max_grad_norm",
                opt.max_grad_norm.map(|value| value.to_string()),
            ),
            (
                "val_set_size",
                p.advanced.eval_split_ratio.map(|value| value.to_string()),
            ),
            (
                "gradient_checkpointing",
                p.advanced.gradient_checkpointing.clone(),
            ),
            (
                "flash_attention",
                p.advanced
                    .attn_implementation
                    .as_ref()
                    .map(|attn| (attn == "flash_attention_2").to_string()),
            ),
        ] {
            if let Some(value) = value {
                context.insert(key.to_string(), serde_json::json!(value));
            }
        }
        if opt.weight_decay > 0.0 {
            context.insert(
                "weight_decay".to_string(),
                serde_json::json!(opt.weight_decay),
            );
        }

        // Wire optimization fields from TrainingParams to the template context.
        // These fields exist in the struct but were not previously passed to the
        // template, causing the template to use hardcoded defaults.
        context.insert("bf16".to_string(), serde_json::json!(p.advanced.bf16));
        context.insert(
            "sample_packing".to_string(),
            serde_json::json!(p.sequence.sample_packing),
        );

        let template_root = std::env::var_os("HKASK_TEMPLATE_ROOT")
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
            });

        // Chat template injection for text-only base mirrors that omit it.
        // Some text-only decoders extracted from multimodal checkpoints (e.g.
        // the Gemma4 text decoder) ship without the canonical chat_template
        // that axolotl's `chat_template` dataset type requires. Load a
        // family-specific template from a registry asset and inject it as a
        // pre-indented YAML block scalar. minijinja emits context strings raw
        // (no re-interpretation of the embedded Jinja, no autoescaping), so the
        // chat template's own {{ }}/{% %} reach the tokenizer/axolotl untouched.
        // Always insert (empty for unsupported families) so Strict-undefined
        // minijinja stays happy on the `{% if chat_template_block %}` guard.
        let base_lower = job.base_model.to_lowercase();
        let chat_asset = if base_lower.contains("gemma-4") || base_lower.contains("gemma4") {
            Some("gemma4.jinja")
        } else if base_lower.contains("qwen3") {
            // Qwen3.5/3.6 text decoder — uses training-optimized template with enable_thinking=false
            Some("qwen3.jinja")
        } else {
            None
        };
        let mut chat_block = String::new();
        if let Some(asset) = chat_asset {
            let asset_path = template_root
                .join("templates/training/chat-templates")
                .join(asset);
            match std::fs::read_to_string(&asset_path) {
                Ok(raw) => {
                    let indented = raw
                        .lines()
                        .map(|line| format!("  {line}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    chat_block = format!("chat_template: |\n{indented}");
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.training.axolotl",
                        asset = %asset_path.display(),
                        error = %error,
                        "Chat template asset not found; base may lack a chat_template"
                    );
                }
            }
        }
        context.insert(
            "chat_template_block".to_string(),
            serde_json::json!(chat_block),
        );

        let template_path = template_root.join("templates/training/axolotl-lora.j2");
        let template = std::fs::read_to_string(&template_path).map_err(|error| {
            ProviderError::InvalidConfig(format!(
                "Read Axolotl template {}: {error}",
                template_path.display()
            ))
        })?;
        let mut environment = minijinja::Environment::new();
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        environment
            .render_str(&template, serde_json::Value::Object(context))
            .map(|yaml| yaml.trim().to_string() + "\n")
            .map_err(|error| {
                ProviderError::InvalidConfig(format!("Render Axolotl template: {error}"))
            })
    }

    fn output_dir(&self, job_id: &str) -> PathBuf {
        // /workspace/outputs is the canonical output dir on the RunPod pod
        // (matches the entrypoint's OUTPUT_DIR default and the
        // HKASK_COMPLETION_MANIFEST_PATH contract).
        PathBuf::from(format!("/workspace/outputs/{}", job_id))
    }

    fn completion_marker(&self, job_id: &str) -> PathBuf {
        self.output_dir(job_id).join("adapter_model.safetensors")
    }

    fn harness_id(&self) -> TrainingHarnessId {
        TrainingHarnessId::Axolotl
    }
}

// ── Ludwig harness ──────────────────────────────────────────────────────────

/// Renders Ludwig YAML configuration from canonical `TrainingParams`.
///
/// Ludwig is a declarative deep-learning framework (Linux Foundation AI & Data,
/// Apache-2.0) that, like Axolotl, takes a single YAML config. Unlike Axolotl,
/// Ludwig covers the full alignment spectrum — SFT, DPO, KTO, ORPO, and GRPO
/// (reward-model-free RLHF) — plus advanced PEFT initializers (PiSSA, EVA,
/// CorDA, LoftQ) that hKask's `LoraInit` enum declares but Axolotl cannot render.
///
/// The rendered YAML is dispatched to the RunPod pod as `HKASK_LUDWIG_CONFIG`
/// (parallel to `HKASK_AXOLOTL_CONFIG`). The pod's entrypoint writes it to
/// `/workspace/model.yaml` and runs `ludwig train --config /workspace/model.yaml`.
///
/// All Ludwig trainer types are supported via `trainer.type` in the rendered
/// YAML: finetune (SFT), dpo, kto, orpo, grpo. The trainer is selected from
/// `job.params.trl_trainer` (reused for Ludwig since the trainer taxonomy
/// maps 1:1) or defaults to SFT.
///
/// Ludwig is the only harness covering GRPO (reward-model-free RLHF) and
/// the full advanced-PEFT initializer set (PiSSA, EVA, CorDA, LoftQ).
///
/// References:
/// - Ludwig docs: https://ludwig.ai/latest/
/// - Ludwig LLM fine-tuning: https://ludwig.ai/latest/getting_started/llm_fine_tuning/
/// - Ludwig config schema: https://ludwig.ai/latest/configuration/
/// - Ludwig GitHub: https://github.com/ludwig-ai/ludwig
pub struct LudwigHarness;

impl HarnessAdapter for LudwigHarness {
    fn render_config(&self, job: &TrainingJob) -> Result<String, ProviderError> {
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

        // Ludwig uses a different config shape from Axolotl — `model_type: llm`,
        // `base_model:`, `adapter:`, `quantization:`, `input_features`,
        // `output_features`, `trainer:`. We assemble the full context the
        // template needs; the template (ludwig-lora.j2) is the single source
        // of truth for the YAML structure.
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
            // Ludwig-specific: use_rslora / use_dora map to adapter config flags.
            (
                "use_rslora".to_string(),
                serde_json::json!(lo.use_rslora.to_string()),
            ),
            (
                "use_dora".to_string(),
                serde_json::json!(lo.use_dora.to_string()),
            ),
            // Ludwig trainer type — derived from trl_trainer (taxonomy maps 1:1).
            // SFT → finetune, DPO → dpo, KTO → kto, ORPO → orpo, Reward → finetune
            // (Ludwig doesn't have a separate reward model trainer; use SFT).
            // GRPO is Ludwig-only — it's set when trl_trainer is None but the
            // operator declared trainer_preference=grpo (handled by G6 gate).
            (
                "trainer_type".to_string(),
                serde_json::json!(match p.trl_trainer.unwrap_or_default() {
                    TrlTrainer::Sft => "finetune",
                    TrlTrainer::Dpo => "dpo",
                    TrlTrainer::Kto => "kto",
                    TrlTrainer::Orpo => "orpo",
                    TrlTrainer::Reward => "finetune", // Ludwig has no reward trainer; use SFT
                }),
            ),
        ]);
        // Optional fields — always inserted (with empty defaults when None) so
        // the template's `{% if field %}` guards work with minijinja Strict
        // undefined behavior. Empty strings are falsy in minijinja, so `{% if %}`
        // correctly skips rendering when the value is absent. This mirrors the
        // Axolotl harness pattern (always insert, let the template decide).
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
            "val_set_size".to_string(),
            serde_json::json!(p.advanced.eval_split_ratio.map(|v| v.to_string())),
        );
        // weight_decay is a float — always inserted so the template's
        // `{% if weight_decay and weight_decay > 0 %}` guard works.
        context.insert(
            "weight_decay".to_string(),
            serde_json::json!(opt.weight_decay),
        );

        // Optimization fields — wired from TrainingParams.
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

        let template_root = std::env::var_os("HKASK_TEMPLATE_ROOT")
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
            });

        let template_path = template_root.join("templates/training/ludwig-lora.j2");
        let template = std::fs::read_to_string(&template_path).map_err(|error| {
            ProviderError::InvalidConfig(format!(
                "Read Ludwig template {}: {error}",
                template_path.display()
            ))
        })?;
        let mut environment = minijinja::Environment::new();
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        environment
            .render_str(&template, serde_json::Value::Object(context))
            .map(|yaml| yaml.trim().to_string() + "\n")
            .map_err(|error| {
                ProviderError::InvalidConfig(format!("Render Ludwig template: {error}"))
            })
    }

    fn output_dir(&self, job_id: &str) -> PathBuf {
        // Same canonical output dir as AxolotlHarness — the RunPod pod contract
        // is harness-agnostic (/workspace/outputs/{job_id}).
        PathBuf::from(format!("/workspace/outputs/{}", job_id))
    }

    fn completion_marker(&self, job_id: &str) -> PathBuf {
        // Ludwig + PEFT saves adapter_model.safetensors (same as axolotl/TRL).
        self.output_dir(job_id).join("adapter_model.safetensors")
    }

    fn harness_id(&self) -> TrainingHarnessId {
        TrainingHarnessId::Ludwig
    }
}
