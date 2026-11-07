//! dme-harness — the Rust direct-model-edit live-replication instrument
//! (operator ruling 2026-11-06: no agent-authored Python anywhere).
//!
//! Modes:
//!   probe      measurement leg (P4): N problems x config -> JSONL
//!              receipts, through the CANONICAL serving route (the L4
//!              supervisor launches the fleet model per the L1 manifest;
//!              completions ride the M3 endpoint semantics with per-row
//!              sampling-parity verification)
//!   capture    localization leg (the engine feature): per-layer
//!              activation captures through llama-cpp-4's prebuilt link
//!              against the canonical libs (P5)
//!   convert    embedding-to-edit bridge: embedder vectors + capture
//!              manifests -> u/k/covdiag + a hash-bound manifest + the
//!              edit spec (pure math, engine-free)
//!   edit       direction-subtraction edit -> patched GGUF + receipt
//!              (the shared GGUF writer component)
//!   gguf-info  dump a GGUF's metadata and tensor directory
//!   summary    recompute a stage summary from a JSONL
//!
//! The DME gates (DME-0 config gate, DME-1 route, DME-2 budget, DME-3
//! attribution, DME-4 convergence) stay OUTSIDE this crate: they run in
//! `lisp_eval` at decision time over these receipts — the instrument
//! measures, the gates decide.

use anyhow::{Result, bail};
#[cfg(feature = "engine")]
use dme_harness::capture;
use dme_harness::{convert, edit, gguf_writer, pi_gguf, probe};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
    }
    None
}

fn flag_or(args: &[String], name: &str, default: &str) -> String {
    flag(args, name).unwrap_or_else(|| default.to_string())
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).ok_or_else(|| {
        anyhow::anyhow!("usage: dme-harness <probe|capture|convert|edit|gguf-info|summary> ...")
    })?;
    let rest: &[String] = if args.is_empty() { &[] } else { &args[1..] };

    match cmd {
        "gguf-info" => {
            let path = flag(rest, "--gguf").ok_or_else(|| anyhow::anyhow!("--gguf required"))?;
            let meta = pi_gguf::parse_file(std::path::Path::new(&path))?;
            println!(
                "gguf {} arch={} alignment={} block_count={} tensors={} params={} dominant={} data_start={}",
                meta.gguf_version,
                meta.architecture(),
                meta.alignment,
                meta.block_count().unwrap_or(0),
                meta.tensors.len(),
                meta.param_count,
                pi_gguf::type_name(meta.dominant_type()),
                meta.data_start
            );
            println!(
                "sha256={}",
                gguf_writer::sha256_file(std::path::Path::new(&path))?
            );
            let filter = flag(rest, "--layer");
            for t in &meta.tensors {
                if let Some(l) = &filter {
                    let want = format!("blk.{l}.");
                    if !t.name.starts_with(&want) {
                        continue;
                    }
                }
                println!(
                    "  {:<40} {:>6} dims={:?} offset={}",
                    t.name,
                    pi_gguf::type_name(t.ggml_type),
                    t.dims,
                    t.offset
                );
            }
            Ok(())
        }
        "summary" => {
            let rows = flag(rest, "--rows").ok_or_else(|| anyhow::anyhow!("--rows required"))?;
            let out = flag(rest, "--out");
            probe::summarize(
                std::path::Path::new(&rows),
                out.as_deref().map(std::path::Path::new),
            )
        }
        "probe" => {
            let config =
                flag(rest, "--config").ok_or_else(|| anyhow::anyhow!("--config required"))?;
            let problems =
                flag(rest, "--problems").ok_or_else(|| anyhow::anyhow!("--problems required"))?;
            let out = flag(rest, "--out").ok_or_else(|| anyhow::anyhow!("--out required"))?;
            let args = probe::ProbeArgs {
                model_id: flag_or(rest, "--model", "qwen35-0.8b"),
                config: config.into(),
                problems: problems.into(),
                out_jsonl: out.into(),
                out_summary: flag(rest, "--summary").map(Into::into),
                start: flag_or(rest, "--start", "0").parse()?,
                count: flag_or(rest, "--count", "8").parse()?,
                cap: flag_or(rest, "--cap", "4096").parse()?,
                threads: flag_or(rest, "--threads", "4").parse()?,
                n_gpu_layers: flag_or(rest, "--ngl", "99").parse()?,
                partial_gpu_offload: has_flag(rest, "--partial"),
                kv_cache_type: flag_or(rest, "--kv", "f16"),
                seed_base: flag_or(rest, "--seed", "20261106").parse()?,

                budget_secs: flag_or(rest, "--budget-secs", "1500").parse()?,
                port: flag_or(rest, "--port", "8011").parse()?,
            };
            probe::run(args)
        }
        "capture" => {
            #[cfg(feature = "engine")]
            {
                let gguf =
                    flag(rest, "--gguf").ok_or_else(|| anyhow::anyhow!("--gguf required"))?;
                let cfg =
                    flag(rest, "--config").ok_or_else(|| anyhow::anyhow!("--config required"))?;
                let problems = flag(rest, "--problems")
                    .ok_or_else(|| anyhow::anyhow!("--problems required"))?;
                let out_dir =
                    flag(rest, "--out").ok_or_else(|| anyhow::anyhow!("--out required"))?;
                let layers: Vec<u32> = flag_or(rest, "--layers", "15,19")
                    .split(',')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let sites: Vec<String> = flag_or(rest, "--sites", "l_out,ffn_out")
                    .split(',')
                    .map(String::from)
                    .collect();
                let args = capture::CaptureArgs {
                    gguf: gguf.into(),
                    config: cfg.into(),
                    problems: problems.into(),
                    out_dir: out_dir.into(),
                    start: flag_or(rest, "--start", "0").parse()?,
                    count: flag_or(rest, "--count", "2").parse()?,
                    cap: flag_or(rest, "--cap", "512").parse()?,
                    n_ctx: flag_or(rest, "--n-ctx", "4096").parse()?,
                    threads: flag_or(rest, "--threads", "4").parse()?,
                    n_gpu_layers: flag_or(rest, "--n-gpu-layers", "99").parse()?,
                    seed_base: flag_or(rest, "--seed", "20261106").parse()?,

                    sites,
                    layers,
                    max_new: flag_or(rest, "--max-new", "256").parse()?,
                };
                capture::run(args)
            }
            #[cfg(not(feature = "engine"))]
            {
                let _ = rest;
                bail!("capture requires the engine feature; build with --features engine")
            }
        }
        "convert" => {
            let config =
                flag(rest, "--config").ok_or_else(|| anyhow::anyhow!("--config required"))?;
            let vectors =
                flag(rest, "--vectors").ok_or_else(|| anyhow::anyhow!("--vectors required"))?;
            let captures = flag(rest, "--capture-manifest")
                .ok_or_else(|| anyhow::anyhow!("--capture-manifest required"))?;
            let capture_dir = flag(rest, "--capture-dir")
                .ok_or_else(|| anyhow::anyhow!("--capture-dir required"))?;
            let out = flag(rest, "--out").ok_or_else(|| anyhow::anyhow!("--out required"))?;
            convert::run(convert::ConvertArgs {
                config: config.into(),
                vectors: vectors.into(),
                capture_manifest: captures.into(),
                capture_dir: capture_dir.into(),
                out_dir: out.into(),
            })
        }
        "edit" => {
            let spec = flag(rest, "--spec").ok_or_else(|| anyhow::anyhow!("--spec required"))?;
            let parsed = edit::load_spec(std::path::Path::new(&spec))?;
            edit::run(&parsed)
        }
        other => bail!("unknown mode {other:?}"),
    }
}
