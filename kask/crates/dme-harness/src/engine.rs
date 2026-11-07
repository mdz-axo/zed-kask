//! The inference engine wrapper (llama.cpp via llama-cpp-4 v0.11.0).
//!
//! Spike-1 facts this module bakes in:
//! - the GGUF's embedded chat template renders BOTH modes via
//!   `enable_thinking` (config A = `assistant\n<think>\n`,
//!   config B = `assistant\n<think>\n\n</think>\n\n`) — the DME-0 mechanism;
//! - `TensorSelector::layer_output` captures `l_out-N` residual streams on
//!   the GDN hybrid over Vulkan; layer 23 is computed in the 1-row logits
//!   split and is NOT batch-mappable — capture 0..=22 only;
//! - two live contexts on one model abort — one context at a time;
//! - the native penalties sampler implements the card's presence penalty:
//!   we pin `penalty_last_n = n_ctx` (the whole window) as the closest
//!   match to the API semantics the card's recommendation assumes. This is
//!   a recorded deviation from the retired transformers-side processor and
//!   is reported in every receipt (`penalty_last_n` key).

use crate::config::RunConfig;
use crate::receipt::RunRow;
use anyhow::{Context, Result, anyhow};
use std::num::NonZeroU32;
use std::path::Path;
use std::time::Instant;

use llama_cpp_4::prelude::*;

// The shared GGUF component (P3): parse + sha256 live in the
// private-inference stack; this crate's hand parser is deleted.
use hkask_private_inference::local_leg::gguf as pi_gguf;
use hkask_private_inference::local_leg::gguf_writer;
pub use hkask_private_inference::local_leg::gguf_writer::sha256_file;

pub const HARNESS: &str = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));

pub struct Engine {
    pub backend: LlamaBackend,
    pub model: LlamaModel,
    templates: ChatTemplates,
    pub gguf_path: String,
    pub gguf_sha256: String,
    pub model_name: String,
    pub dtype: String,
    pub param_count: u64,
    pub backend_name: String,
}

impl Engine {
    pub fn load(gguf_path: &Path, n_gpu_layers: u32) -> Result<Self> {
        let meta = pi_gguf::parse_file(gguf_path)?;
        let backend = LlamaBackend::init()?;
        let model = LlamaModel::load_from_file(
            &backend,
            gguf_path
                .to_str()
                .ok_or_else(|| anyhow!("non-utf8 gguf path"))?,
            &LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers),
        )
        .with_context(|| "llama_model_load")?;
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
        Ok(Self {
            backend,
            model,
            templates,
            gguf_path: gguf_path.display().to_string(),
            gguf_sha256: sha256_file(gguf_path)?,
            model_name,
            dtype: gguf_writer::type_name(meta.dominant_type()).to_string(),
            param_count: model.n_params(),
            backend_name: {
                let mut b = String::new();
                let dev = supports_gpu_offload();
                b.push_str(if dev {
                    "llama.cpp(gpu)"
                } else {
                    "llama.cpp(cpu)"
                });
                b
            },
        })
    }

    /// Render the chat prompt for a problem under a config's mode.
    pub fn render_prompt(&self, problem_question: &str, cfg: &RunConfig) -> Result<String> {
        // The card's math-prompt standard (Best Practices item 3):
        // "Please reason step by step, and put your final answer within \boxed{}."
        let content = format!(
            "{problem_question} Please reason step by step, and put your final answer within \\boxed{{}}."
        );
        let messages = serde_json::json!([{ "role": "user", "content": content }]);
        let applied = self
            .templates
            .apply(
                &ChatApplyParams::new(messages.to_string())
                    .with_enable_thinking(cfg.enable_thinking),
            )
            .with_context(|| "chat template apply")?;
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

pub struct GenOutcome {
    pub new_tokens: u32,
    pub wall_secs: f64,
    pub text: String,
    pub token_ids: Vec<LlamaToken>,
    pub hit_limit: bool,
}

/// Generate up to `cap` new tokens from `prompt_tokens` under `chain`.
pub fn generate(
    ctx: &mut LlamaContext,
    model: &LlamaModel,
    chain: &mut LlamaSampler,
    prompt_tokens: &[LlamaToken],
    cap: u32,
) -> Result<GenOutcome> {
    let n_ctx = ctx.n_ctx() as usize;
    anyhow::ensure!(
        (prompt_tokens.len() + cap as usize) <= n_ctx,
        "n_ctx ({n_ctx}) too small for prompt + cap"
    );
    let eos = model.token_eos();
    let mut batch = LlamaBatch::new(1024, 1);
    for (i, &tok) in prompt_tokens.iter().enumerate() {
        batch.add(tok, i as i32, &[0], i == prompt_tokens.len() - 1)?;
    }
    ctx.decode(&mut batch)?;

    let mut detok = StreamDetokenizer::new(model, Special::Plaintext);
    let mut text = String::new();
    let mut token_ids: Vec<LlamaToken> = Vec::new();
    let mut pos = prompt_tokens.len() as i32;
    let t0 = Instant::now();
    let mut hit_limit = false;
    loop {
        if token_ids.len() >= cap as usize {
            hit_limit = true;
            break;
        }
        let tok = chain.try_sample(ctx, -1)?;
        if tok == eos {
            break;
        }
        token_ids.push(tok);
        text.push_str(&detok.push(tok)?);
        batch.clear();
        batch.add(tok, pos, &[0], true)?;
        ctx.decode(&mut batch)?;
        pos += 1;
    }
    text.push_str(&detok.finish()?);
    Ok(GenOutcome {
        new_tokens: token_ids.len() as u32,
        wall_secs: t0.elapsed().as_secs_f64(),
        text,
        token_ids,
        hit_limit,
    })
}

/// One probe row: render, generate, classify, receipt.
#[allow(clippy::too_many_arguments)]
pub fn probe_row(
    engine: &Engine,
    cfg: &RunConfig,
    problem_id: &str,
    question: &str,
    gold: &str,
    cap: u32,
    seed: u64,
    n_ctx: u32,
    threads: u32,
    n_gpu_layers: u32,
    config_file: serde_json::Value,
) -> Result<RunRow> {
    let prompt = engine.render_prompt(question, cfg)?;
    let prompt_tokens = engine
        .model
        .str_to_token(&prompt, AddBos::Never)
        .with_context(|| "tokenize prompt")?;

    let mut ctx = engine.model.new_context(
        &engine.backend,
        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(1024)
            .with_n_threads(threads as i32)
            .with_n_threads_batch(threads as i32)
            .with_offload_kqv(n_gpu_layers > 0),
    )?;
    let mut chain = engine.sampler_chain(cfg, n_ctx, seed);
    let out = generate(&mut ctx, &engine.model, &mut chain, &prompt_tokens, cap)?;

    let extracted = crate::receipt::extract_boxed(&out.text);
    let correct = extracted
        .as_deref()
        .map(|e| crate::receipt::normalize_numeric(e) == crate::receipt::normalize_numeric(gold))
        .unwrap_or(false);
    let label = if out.hit_limit {
        "hit_limit"
    } else if correct {
        "correct_complete"
    } else {
        "wrong_complete"
    };
    let tail = out.token_ids.len().min(256);
    let mut distinct: std::collections::HashSet<i32> = Default::default();
    for t in &out.token_ids[out.token_ids.len() - tail..] {
        distinct.insert(t.0);
    }
    let tail_ratio = if tail == 0 {
        0.0
    } else {
        distinct.len() as f64 / tail as f64
    };

    Ok(RunRow {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        config: cfg.name.clone(),
        problem_id: problem_id.to_string(),
        source: "gsm8k-train".into(),
        gold: gold.to_string(),
        kind: "numeric".into(),
        prompt_tokens: prompt_tokens.len() as u32,
        new_tokens: out.new_tokens,
        tok_per_sec: if out.wall_secs > 0.0 {
            out.new_tokens as f64 / out.wall_secs
        } else {
            0.0
        },
        wall_secs: out.wall_secs,
        hit_limit: out.hit_limit,
        extracted: extracted.clone(),
        extraction_via: if extracted.is_some() {
            "boxed".into()
        } else {
            "none".into()
        },
        correct,
        label: label.into(),
        tail_distinct_ratio: tail_ratio,
        cap,
        seed,
        enable_thinking_applied: cfg.enable_thinking,
        dropped_gen_kwargs: vec![],
        presence_penalty_applied: cfg.presence_penalty as f64,
        model: engine.model_name.clone(),
        dtype: engine.dtype.clone(),
        threads,
        param_count: engine.param_count,
        config_file,
        harness: HARNESS.into(),
        backend: engine.backend_name.clone(),
        gguf_sha256: engine.gguf_sha256.clone(),
        penalty_last_n: n_ctx as i32,
        n_ctx,
    })
}
