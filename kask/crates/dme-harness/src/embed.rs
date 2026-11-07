//! The embeddings leg: one L2-normalized embedding vector per anchor id
//! through the canonical in-process engine — E1's vector acquisition
//! route (Qwen3-Embedding-0.6B, d=1024, dimension-matched to the proxy
//! target's hidden 1024). Raw prompt text, no chat template — the
//! alignment is text-mediated (the E1 geometry declaration).
//!
//! Output: ONE f32 file (N x d, ProblemSet row order — the same order and
//! ids the capture leg's manifest carries, so the two files join by row
//! index) plus a manifest row per anchor (the pre-normalization L2
//! recorded for provenance).

use crate::problems::ProblemSet;
use anyhow::{Context, Result};
use std::io::Write as _;
use std::num::NonZeroU32;
use std::path::PathBuf;

use llama_cpp_4::prelude::*;

pub struct EmbedArgs {
    pub gguf: PathBuf,
    pub problems: PathBuf,
    pub out: PathBuf,
    pub start: usize,
    pub count: usize,
    pub n_ctx: u32,
    pub threads: u32,
    pub n_gpu_layers: u32,
    pub free_corpus: bool,
}

#[derive(serde::Serialize)]
struct EmbedManifestRow {
    ts: f64,
    problem_id: String,
    dim: usize,
    pre_norm: f64,
    harness: String,
}

pub fn run(args: EmbedArgs) -> Result<()> {
    let harness = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));
    let set = if args.free_corpus {
        ProblemSet::load_free(&args.problems, 20261106)?
    } else {
        ProblemSet::load(&args.problems, 20261106)?
    };
    anyhow::ensure!(args.count > 0, "no anchors to embed");
    let backend = LlamaBackend::init()?;
    let model = LlamaModel::load_from_file(
        &backend,
        args.gguf
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-utf8 gguf path"))?,
        &LlamaModelParams::default().with_n_gpu_layers(args.n_gpu_layers),
    )
    .map_err(|e| anyhow::anyhow!("llama_model_load: {e}"))?;

    let mut manifest_path = args.out.clone();
    manifest_path.set_extension("embed-manifest.jsonl");
    let mut manifest = std::io::BufWriter::new(std::fs::File::create(&manifest_path)?);

    let mut all: Vec<f32> = Vec::new();
    let mut dim_out: Option<usize> = None;
    for i in 0..args.count {
        let row_idx = args.start + i;
        let problem = set.at_row(row_idx);
        let problem_id = set.problem_id(row_idx);
        // one context at a time (the spike's rule); embeddings ride the
        // model's own pooling metadata (last-token for Qwen3-Embedding)
        let mut ctx = model.new_context(
            &backend,
            LlamaContextParams::default()
                .with_embeddings(true)
                .with_n_ctx(NonZeroU32::new(args.n_ctx))
                .with_n_batch(1024)
                .with_n_threads(args.threads as i32)
                .with_offload_kqv(args.n_gpu_layers > 0),
        )?;
        let tokens = model
            .str_to_token(&problem.question, AddBos::Never)
            .with_context(|| "tokenize")?;
        anyhow::ensure!(!tokens.is_empty(), "empty tokenization for {problem_id}");
        let mut batch = LlamaBatch::new(1024, 1);
        for (k, &tok) in tokens.iter().enumerate() {
            batch.add(tok, k as i32, &[0], k == tokens.len() - 1)?;
        }
        ctx.decode(&mut batch)?;
        let emb = ctx
            .embeddings_seq_ith(0)
            .context("no embedding produced — is this an embedding model (pooling metadata)?")?;
        let emb: &[f32] = emb.as_ref();
        let d = emb.len();
        match dim_out {
            None => dim_out = Some(d),
            Some(prev) => anyhow::ensure!(prev == d, "ragged embeddings: {d} != {prev}"),
        }
        let pre_norm: f64 = emb.iter().map(|x| f64::from(x * x)).sum::<f64>().sqrt();
        anyhow::ensure!(pre_norm > 0.0, "zero-norm embedding for {problem_id}");
        let scale = pre_norm as f32;
        for x in emb {
            all.push(x / scale);
        }
        serde_json::to_writer(
            &mut manifest,
            &EmbedManifestRow {
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|t| t.as_secs_f64())
                    .unwrap_or(0.0),
                problem_id,
                dim: d,
                pre_norm,
                harness: harness.into(),
            },
        )?;
        manifest.write_all(b"\n")?;
        drop(ctx);
        eprintln!(
            "[{}/{}] {} dim={d} pre_norm={pre_norm:.4}",
            i + 1,
            args.count,
            set.at_row(row_idx).question
        );
    }
    manifest.flush()?;
    let mut bytes = Vec::with_capacity(all.len() * 4);
    for x in &all {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    std::fs::write(&args.out, &bytes)?;
    let d = dim_out.unwrap_or(0);
    eprintln!(
        "embed: {} vectors x dim {} -> {} ({} bytes)",
        args.count,
        d,
        args.out.display(),
        bytes.len()
    );
    Ok(())
}
