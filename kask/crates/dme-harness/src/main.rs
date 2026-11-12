//! dme-harness — the Rust direct-model-edit live-replication instrument
//! (operator ruling 2026-11-06: no agent-authored Python anywhere).
//!
//! Modes:
//!   probe      measurement leg: N problems x config -> JSONL receipts,
//!              against a llama.cpp server the operator runs (bound to
//!              the GGUF by path, per-row sampling-parity verification)
//!   convert    embedding-to-edit bridge: embedder vectors + capture
//!              manifests -> u/k/covdiag + a hash-bound manifest + the
//!              edit spec (pure math, engine-free)
//!   contrast   the contrast-u instrument leg (the replication's slot-7
//!              addition): the failing/progressing capture pair ->
//!              per-layer u-{L}.f32 (unit-norm mean difference), the
//!              swiglu column ranking ranked-{L}.txt, and the S-mass
//!              margin receipt (pure math, engine-free)
//!   edit       direction-subtraction edit -> patched GGUF + receipt
//!   gguf-info  dump a GGUF's metadata and tensor directory
//!   summary    recompute a stage summary from a JSONL
//!
//! The DME gates (DME-0 config gate, DME-1 route, DME-2 budget, DME-3
//! attribution, DME-4 convergence) stay OUTSIDE this crate: they run in
//! `lisp_eval` at decision time over these receipts — the instrument
//! measures, the gates decide.
//!
//! The harness links no inference engine: the activation captures that
//! `contrast` and `convert` read are produced outside this crate.

use anyhow::{Result, bail};
use dme_harness::{contrast, convert, edit, gguf, gguf_writer, probe};

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

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).ok_or_else(|| {
        anyhow::anyhow!("usage: dme-harness <probe|contrast|convert|edit|gguf-info|summary> ...")
    })?;
    let rest: &[String] = if args.is_empty() { &[] } else { &args[1..] };

    match cmd {
        "gguf-info" => {
            let path = flag(rest, "--gguf").ok_or_else(|| anyhow::anyhow!("--gguf required"))?;
            let meta = gguf::parse_file(std::path::Path::new(&path))?;
            println!(
                "gguf {} arch={} alignment={} block_count={} tensors={} params={} dominant={} data_start={}",
                meta.gguf_version,
                meta.architecture(),
                meta.alignment,
                meta.block_count().unwrap_or(0),
                meta.tensors.len(),
                meta.param_count,
                gguf::type_name(meta.dominant_type()),
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
                    gguf::type_name(t.ggml_type),
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
            let gguf = flag(rest, "--gguf").ok_or_else(|| anyhow::anyhow!("--gguf required"))?;
            let args = probe::ProbeArgs {
                gguf: gguf.into(),
                endpoint: flag_or(rest, "--endpoint", "http://127.0.0.1:8080"),
                config: config.into(),
                problems: problems.into(),
                out_jsonl: out.into(),
                out_summary: flag(rest, "--summary").map(Into::into),
                start: flag_or(rest, "--start", "0").parse()?,
                count: flag_or(rest, "--count", "8").parse()?,
                cap: flag_or(rest, "--cap", "4096").parse()?,
                seed_base: flag_or(rest, "--seed", "20261106").parse()?,
                budget_secs: flag_or(rest, "--budget-secs", "1500").parse()?,
            };
            probe::run(args)
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
        "contrast" => {
            let failing =
                flag(rest, "--failing").ok_or_else(|| anyhow::anyhow!("--failing required"))?;
            let progressing = flag(rest, "--progressing")
                .ok_or_else(|| anyhow::anyhow!("--progressing required"))?;
            let out = flag(rest, "--out").ok_or_else(|| anyhow::anyhow!("--out required"))?;
            let layers: Vec<u32> = flag_or(rest, "--layers", "15,19")
                .split(',')
                .filter_map(|s| s.parse().ok())
                .collect();
            contrast::run(contrast::ContrastArgs {
                failing_dir: failing.into(),
                progressing_dir: progressing.into(),
                out_dir: out.into(),
                layers,
                top: flag_or(rest, "--top", "64").parse()?,
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
