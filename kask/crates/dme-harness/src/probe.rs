//! The measurement leg: run N problems under a config, emit archived-schema
//! JSONL rows plus a stage summary.

use crate::config::RunConfig;
use crate::engine::{Engine, HARNESS};
use crate::problems::ProblemSet;
use crate::receipt::{Counts, RunRow, StageSummary};
use anyhow::Result;
use std::io::Write;
use std::path::Path;

pub struct ProbeArgs {
    pub gguf: std::path::PathBuf,
    pub config: std::path::PathBuf,
    pub problems: std::path::PathBuf,
    pub out_jsonl: std::path::PathBuf,
    pub out_summary: Option<std::path::PathBuf>,
    pub start: usize,
    pub count: usize,
    pub cap: u32,
    pub n_ctx: u32,
    pub threads: u32,
    pub n_gpu_layers: u32,
    pub seed_base: u64,
    pub order: String,
}

pub fn run(args: ProbeArgs) -> Result<()> {
    let cfg = RunConfig::load(&args.config)?;
    let set = ProblemSet::load(&args.problems, args.seed_base, &args.order)?;
    let engine = Engine::load(&args.gguf, args.n_gpu_layers)?;
    let config_file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args.config)?)?;

    eprintln!(
        "probe: cfg={} model={} dtype={} params={} start={} count={} cap={} n_ctx={}",
        cfg.name,
        engine.gguf_path,
        engine.dtype,
        engine.param_count,
        args.start,
        args.count,
        args.cap,
        args.n_ctx
    );

    let t_start = std::time::Instant::now();
    let mut counts = Counts::default();
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args.out_jsonl)?);
    for i in 0..args.count {
        let row_idx = args.start + i;
        let problem = set.at_row(row_idx);
        let problem_id = set.problem_id(row_idx);
        let seed = args.seed_base + args.start as u64 + i as u64;
        let row = crate::engine::probe_row(
            &engine,
            &cfg,
            &problem_id,
            &problem.question,
            &problem.gold(),
            args.cap,
            seed,
            args.n_ctx,
            args.threads,
            args.n_gpu_layers,
            config_file.clone(),
        )?;
        eprintln!(
            "[{}/{}] {} seed={seed} label={} new_tokens={} tok/s={:.1}",
            i + 1,
            args.count,
            row.problem_id,
            row.label,
            row.new_tokens,
            row.tok_per_sec
        );
        match row.label.as_str() {
            "correct_complete" => counts.correct_complete += 1,
            "wrong_complete" => counts.wrong_complete += 1,
            "hit_limit" => counts.hit_limit += 1,
            _ => {}
        }
        serde_json::to_writer(&mut out, &row)?;
        out.write_all(b"\n")?;
        out.flush()?;
    }

    let n = args.count as u32;
    let mut mean_wall = 0.0;
    let mut mean_tps = 0.0;
    // recompute means from the rows we wrote (the honest way: reread)
    for line in std::io::BufReader::new(std::fs::File::open(&args.out_jsonl)?).lines() {
        let line = line?;
        let v: serde_json::Value = serde_json::from_str(&line)?;
        mean_wall += v["wall_secs"].as_f64().unwrap_or(0.0);
        mean_tps += v["tok_per_sec"].as_f64().unwrap_or(0.0);
    }
    if n > 0 {
        mean_wall /= f64::from(n);
        mean_tps /= f64::from(n);
    }
    let summary = StageSummary {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        config: cfg.name.clone(),
        n,
        counts,
        cap: args.cap,
        mean_wall_secs: mean_wall,
        mean_tok_per_sec: mean_tps,
        gguf: engine.gguf_path.clone(),
        gguf_sha256: engine.gguf_sha256.clone(),
        dtype: engine.dtype.clone(),
        elapsed_secs: t_start.elapsed().as_secs_f64(),
        order: args.order.clone(),
        harness: HARNESS.into(),
    };
    let json = serde_json::to_string_pretty(&summary)?;
    if let Some(path) = args.out_summary {
        std::fs::write(path, &json)?;
    }
    eprintln!("summary: {json}");
    Ok(())
}

/// Summarize an existing JSONL (for `summary` subcommand).
pub fn summarize(rows: &Path, out: Option<&Path>) -> Result<()> {
    let mut counts = Counts::default();
    let mut n = 0u32;
    let mut mean_wall = 0.0;
    let mut mean_tps = 0.0;
    let mut cap = 0;
    let mut config = String::new();
    let mut first: Option<serde_json::Value> = None;
    for line in std::io::BufReader::new(std::fs::File::open(rows)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line)?;
        if first.is_none() {
            first = Some(v.clone());
            cap = v["cap"].as_u64().unwrap_or(0) as u32;
            config = v["config"].as_str().unwrap_or("").to_string();
        }
        n += 1;
        mean_wall += v["wall_secs"].as_f64().unwrap_or(0.0);
        mean_tps += v["tok_per_sec"].as_f64().unwrap_or(0.0);
        match v["label"].as_str().unwrap_or("") {
            "correct_complete" => counts.correct_complete += 1,
            "wrong_complete" => counts.wrong_complete += 1,
            "hit_limit" => counts.hit_limit += 1,
            _ => {}
        }
    }
    if n > 0 {
        mean_wall /= f64::from(n);
        mean_tps /= f64::from(n);
    }
    let f = first.unwrap_or_default();
    let summary = StageSummary {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        config,
        n,
        counts,
        cap,
        mean_wall_secs: mean_wall,
        mean_tok_per_sec: mean_tps,
        gguf: f["model"].as_str().unwrap_or("").to_string(),
        gguf_sha256: f["gguf_sha256"].as_str().unwrap_or("").to_string(),
        dtype: f["dtype"].as_str().unwrap_or("").to_string(),
        elapsed_secs: 0.0,
        order: f
            .get("order")
            .and_then(|o| o.as_str())
            .unwrap_or("")
            .to_string(),
        harness: HARNESS.into(),
    };
    let json = serde_json::to_string_pretty(&summary)?;
    if let Some(path) = out {
        std::fs::write(path, &json)?;
    }
    println!("{json}");
    Ok(())
}

// Re-export for probe.rs users
pub type Row = RunRow;
