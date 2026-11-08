//! The capture path's in-process engine (llama-cpp-4 over the in-repo
//! llama-cpp-sys-4 fork — P5: LLAMA_CPP_SRC_DIR names the pinned
//! canonical checkout and the fork copies + patches + cmakes THAT into
//! its own OUT_DIR, pin-exact; the published crate's prebuilt route fails
//! closed while its decode-hooks patch is staged, and the bundled vendored
//! copy is never built again).
//!
//! This module serves ONLY the capture leg (localization): the probe
//! leg's generation rides the canonical serving route (`serving.rs` —
//! supervisor + endpoint) since P4; capture needs the in-process model
//! for the tensor-transaction hooks.
//!
//! Spike-1 facts this module bakes in:
//! - the GGUF's embedded chat template renders BOTH modes via
//!   `enable_thinking` (config A = `assistant\n<|im_start|` thinking,
//!   config B = the non-thinking form) — the DME-0 mechanism;
//! - `TensorSelector::layer_output` captures `l_out-N` residual streams on
//!   the GDN hybrid; layer 23 is computed in the 1-row logits split and
//!   is NOT batch-mappable — capture 0..=22 only;
//! - two live contexts on one model abort — one context at a time;
//! - the native penalties sampler implements the card's presence penalty:
//!   we pin `penalty_last_n = n_ctx` (the whole window) as the closest
//!   match to the API semantics the card's recommendation assumes. This is
//!   a recorded deviation from the retired transformers-side processor.

use crate::config::RunConfig;
use anyhow::{Result, anyhow};
use std::path::Path;

use llama_cpp_4::model::params::LlamaLoadMode;
use llama_cpp_4::prelude::*;

pub const HARNESS: &str = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));

use hkask_private_inference::local_leg::gguf as pi_gguf;
pub use hkask_private_inference::local_leg::gguf_writer::sha256_file;

pub struct Engine {
    pub backend: LlamaBackend,
    pub model: LlamaModel,
    templates: ChatTemplates,
    pub gguf_path: String,
    pub model_name: String,
    pub dtype: String,
    pub param_count: u64,
}

impl Engine {
    pub fn load(gguf_path: &Path, n_gpu_layers: u32, no_mmap: bool) -> Result<Self> {
        let meta = pi_gguf::parse_file(gguf_path)?;
        let backend = LlamaBackend::init()?;
        let mut params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
        if no_mmap {
            // The 2026-11-07 link-loss investigation: mmap'd weights under
            // the HIP backend drive the KFD userptr/SVM churn that preceded
            // every dGPU PCIe link loss — load without mmap instead (the
            // same intent the serving route spells as `--load-mode none`).
            params = params.with_load_mode(LlamaLoadMode::None);
        }
        let model = LlamaModel::load_from_file(
            &backend,
            gguf_path
                .to_str()
                .ok_or_else(|| anyhow!("non-utf8 gguf path"))?,
            &params,
        )
        .map_err(|e| anyhow!("llama_model_load: {e}"))?;
        let templates = ChatTemplates::from_model(&model, None)?;
        anyhow::ensure!(
            templates.supports_enable_thinking(),
            "model template does not support enable_thinking"
        );
        let model_name = format!(
            "gguf:{}/{}",
            gguf_path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            gguf_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        );
        let param_count = model.n_params();
        Ok(Self {
            backend,
            model,
            templates,
            gguf_path: gguf_path.display().to_string(),
            model_name,
            dtype: pi_gguf::type_name(meta.dominant_type()).to_string(),
            param_count,
        })
    }

    /// Render the chat prompt for a problem under a config's mode (the
    /// capture leg's in-process path — the probe leg renders through
    /// the serving route instead). The prompt standard is the ONE
    /// instrument constant (`probe::PROMPT_SUFFIX`).
    pub fn render_prompt(&self, problem_question: &str, cfg: &RunConfig) -> Result<String> {
        let content = format!("{problem_question}{}", crate::probe::PROMPT_SUFFIX);
        let messages = serde_json::json!([{ "role": "user", "content": content }]);
        let applied = self
            .templates
            .apply(
                &ChatApplyParams::new(messages.to_string())
                    .with_enable_thinking(cfg.enable_thinking),
            )
            .map_err(|e| anyhow!("chat template apply: {e}"))?;
        Ok(applied.prompt)
    }

    /// Per-config sampler chain. `penalty_last_n = n_ctx` pins the
    /// presence-penalty window to the whole context (card semantics).
    pub fn sampler_chain(&self, cfg: &RunConfig, n_ctx: u32, seed: u64) -> LlamaSampler {
        let mut samplers = vec![
            LlamaSampler::penalties(
                0,
                n_ctx as i32,
                cfg.repetition_penalty,
                0.0,
                cfg.presence_penalty,
            ),
            LlamaSampler::temp(cfg.temperature),
            LlamaSampler::top_k(cfg.top_k as i32),
            LlamaSampler::top_p(cfg.top_p, 1),
        ];
        if cfg.min_p > 0.0 {
            samplers.push(LlamaSampler::min_p(cfg.min_p, 1));
        }
        samplers.push(LlamaSampler::dist(seed as u32));
        LlamaSampler::chain(samplers, true)
    }
}
