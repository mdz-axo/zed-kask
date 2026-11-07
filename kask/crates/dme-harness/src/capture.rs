//! The localization leg (DME step 3): capture per-layer activations across
//! contrasting trajectories (progressing vs looping answers) at the same
//! config, and write raw f32 captures plus a manifest.
//!
//! Spike-1 facts baked in: capture sites are the graph node names
//! `l_out-{N}` (block output) and `ffn_out-{N}` (per-layer MLP output,
//! [1024, T]); layers 0..=22 only (layer 23 lives in the 1-row logits
//! split); one context at a time; `maximum_rows` must cover the decode
//! batch (the API refuses a shape mismatch, loudly).

use crate::config::RunConfig;
use crate::engine::{Engine, HARNESS, sha256_file};
use crate::problems::ProblemSet;
use anyhow::{Context, Result};
use std::io::Write as _;
use std::num::NonZeroU32;
use std::path::Path;

use llama_cpp_4::prelude::*;

pub struct CaptureArgs {
    pub gguf: std::path::PathBuf,
    pub config: std::path::PathBuf,
    pub problems: std::path::PathBuf,
    pub out_dir: std::path::PathBuf,
    pub start: usize,
    pub count: usize,
    pub cap: u32,
    pub n_ctx: u32,
    pub threads: u32,
    pub n_gpu_layers: u32,
    pub seed_base: u64,
    pub sites: Vec<String>, // "l_out" and/or "ffn_out"
    pub layers: Vec<u32>,   // e.g. 15,19 (plus any contrast layers)
    pub max_new: u32,       // how many generation steps to capture
}

#[derive(serde::Serialize)]
struct CaptureManifestRow {
    ts: f64,
    problem_id: String,
    label: String,
    site: String,
    layer: u32,
    rows: usize,
    row_elements: usize,
    file: String,
    harness: String,
}

pub fn run(args: CaptureArgs) -> Result<()> {
    std::fs::create_dir_all(&args.out_dir)?;
    let cfg = RunConfig::load(&args.config)?;
    let set = ProblemSet::load(&args.problems, args.seed_base)?;
    let engine = Engine::load(&args.gguf, args.n_gpu_layers)?;

    anyhow::ensure!(!args.layers.is_empty(), "no layers to capture");
    anyhow::ensure!(!args.sites.is_empty(), "no capture sites");
    for &l in &args.layers {
        anyhow::ensure!(l <= 22, "layer {l} not batch-mappable (0..=22)");
    }

    let manifest_path = args.out_dir.join("manifest.jsonl");
    // Start the manifest fresh each run; the per-capture rows append to
    // it inside drain_captures.
    drop(std::fs::File::create(&manifest_path)?);
    let gguf_hash = sha256_file(&args.gguf)?;

    for i in 0..args.count {
        let row_idx = args.start + i;
        let problem = set.at_row(row_idx);
        let problem_id = set.problem_id(row_idx);
        let seed = args.seed_base + args.start as u64 + i as u64;
        let label = capture_one(&engine, &cfg, problem, &problem_id, seed, &args, &gguf_hash)
            .with_context(|| format!("capture {problem_id}"))?;
        eprintln!("[{}/{}] {problem_id} -> {label}", i + 1, args.count);
    }
    Ok(())
}

/// Capture one problem: prompt decode + max_new generation steps, appending
/// capture files + manifest rows; returns the trajectory label.
#[allow(clippy::too_many_arguments)]
fn capture_one(
    engine: &Engine,
    cfg: &RunConfig,
    problem: &crate::problems::Problem,
    problem_id: &str,
    seed: u64,
    args: &CaptureArgs,
    gguf_hash: &str,
) -> Result<String> {
    let prompt = engine.render_prompt(&problem.question, cfg)?;
    let prompt_tokens = engine
        .model
        .str_to_token(&prompt, AddBos::Never)
        .with_context(|| "tokenize")?;

    // selectors: one per (site, layer), sorted by name (API requirement)
    let n_embd = engine.model.n_embd() as usize;
    let mut selectors = Vec::new();
    for site in &args.sites {
        for &l in &args.layers {
            let name = format!("{site}-{l}");
            selectors.push(TensorSelector::new(
                name,
                TensorElementType::F32,
                n_embd,
                (prompt_tokens.len() + args.max_new as usize).max(64),
                TensorAccess::ReadOnly,
                TensorRowMapping::BatchTokens,
                true,
            )?);
        }
    }
    selectors.sort_by(|a, b| a.name().cmp(b.name()));
    let tx = TensorTransactions::capture(selectors)?;

    let mut ctx = engine.model.new_context(
        &engine.backend,
        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(args.n_ctx))
            .with_n_batch(1024)
            .with_n_threads(args.threads as i32)
            .with_offload_kqv(args.n_gpu_layers > 0)
            .with_tensor_transactions(tx),
    )?;

    // prompt decode
    let mut batch = LlamaBatch::new(1024, 1);
    for (k, &tok) in prompt_tokens.iter().enumerate() {
        batch.add(tok, k as i32, &[0], k == prompt_tokens.len() - 1)?;
    }
    ctx.decode(&mut batch)?;
    drain_captures(&mut ctx, problem_id, "prompt", 0, args, gguf_hash)?;

    // generation steps
    let mut chain = engine.sampler_chain(cfg, args.n_ctx, seed);
    let eos = engine.model.token_eos();
    let mut label = "wrong_complete";
    let mut pos = prompt_tokens.len() as i32;
    let mut text = String::new();
    let mut detok = StreamDetokenizer::new(&engine.model, Special::Plaintext);
    let mut token_ids: Vec<LlamaToken> = Vec::new();
    for step in 0..args.max_new {
        if token_ids.len() >= args.cap as usize {
            label = "hit_limit";
            break;
        }
        let tok = chain.try_sample(&ctx, -1)?;
        if tok == eos {
            label = "unknown_complete";
            break;
        }
        token_ids.push(tok);
        text.push_str(&detok.push(tok)?);
        batch.clear();
        batch.add(tok, pos, &[0], true)?;
        ctx.decode(&mut batch)?;
        pos += 1;
        drain_captures(&mut ctx, problem_id, "step", step + 1, args, gguf_hash)?;
    }
    // One extractor, one grader (the archived instrument's — the same
    // semantics the probe leg's rows grade under).
    let (extracted, _) = crate::receipt::extract_answer(&text);
    if label == "unknown_complete" {
        label = if extracted
            .as_deref()
            .is_some_and(|e| crate::receipt::numeric_grade(e, &problem.gold()))
        {
            "correct_complete"
        } else {
            "wrong_complete"
        }
    } else if token_ids.len() >= args.cap as usize {
        label = "hit_limit";
    }
    // write the trajectory for the contrast analysis
    let traj = args.out_dir.join(format!("{problem_id}.tokens.txt"));
    std::fs::write(traj, &text)?;
    Ok(label.to_string())
}

fn drain_captures(
    ctx: &mut LlamaContext,
    problem_id: &str,
    phase: &str,
    step: u32,
    args: &CaptureArgs,
    gguf_hash: &str,
) -> Result<()> {
    let Some(txs) = ctx.tensor_transactions_mut() else {
        return Ok(());
    };
    let captures = txs.take_captures();
    let mut manifest = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(args.out_dir.join("manifest.jsonl"))?,
    );
    for cap in captures {
        let Some(CapturedTensorData::F32(values)) = cap.data else {
            continue;
        };
        let cap_name = cap.name();
        let (site_name, layer) = split_node(cap_name);
        let file = format!("{problem_id}.{phase}.{step}.{cap_name}.f32");
        let path = args.out_dir.join(&file);
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in &values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, &bytes)?;
        let row = CaptureManifestRow {
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
            problem_id: problem_id.to_string(),
            label: String::new(), // filled at the trajectory level
            site: site_name.to_string(),
            layer,
            rows: cap.shape.rows,
            row_elements: cap.shape.row_elements,
            file,
            harness: format!("{HARNESS} gguf_sha256={gguf_hash}"),
        };
        serde_json::to_writer(&mut manifest, &row)?;
        manifest.write_all(b"\n")?;
        manifest.flush()?;
    }
    Ok(())
}

fn split_node(name: &str) -> (&str, u32) {
    let (site, idx) = name.rsplit_once('-').unwrap_or((name, "0"));
    (site, idx.parse().unwrap_or(0))
}
