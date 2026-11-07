//! The measurement leg (P4): run N problems under a config through the
//! CANONICAL serving route — the L4 supervisor launches the fleet model
//! per the L1 manifest, the completions ride the M3 endpoint semantics
//! with per-row sampling-parity verification, and the rows follow the
//! archived JSONL schema (comparability by contract).
//!
//! The DME gates (DME-0 config gate, DME-1 route, DME-2 budget, DME-3
//! attribution, DME-4 convergence) stay OUTSIDE this crate: they run in
//! `lisp_eval` at decision time over these receipts — the instrument
//! measures, the gates decide.

use crate::config::RunConfig;
use crate::problems::ProblemSet;
use crate::receipt::{self, Counts, RunRow, StageSummary};
use crate::serving::{CompletionRequest, Serving};
use anyhow::{Context, Result};
use hkask_private_inference::local_leg::fleet;
use hkask_private_inference::local_leg::gguf as pi_gguf;
use hkask_private_inference::local_leg::gguf_writer;
use hkask_private_inference::local_leg::placement::PlacementPolicy;
use hkask_types::agent_paths::resolve_under_artifacts_dir;
use std::io::{BufRead as _, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const HARNESS: &str = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));

/// The archived instrument's math-prompt standard (vendor card, Best
/// Practices item 3).
pub const PROMPT_SUFFIX: &str =
    "\n\nPlease reason step by step, and put your final answer within \\boxed{}.";

pub struct ProbeArgs {
    /// The fleet model to serve (the supervisor resolves + launches it).
    pub model_id: String,
    pub config: PathBuf,
    pub problems: PathBuf,
    pub out_jsonl: PathBuf,
    pub out_summary: Option<PathBuf>,
    pub start: usize,
    pub count: usize,
    pub cap: u32,
    pub threads: u32,
    pub n_gpu_layers: u32,
    pub partial_gpu_offload: bool,
    pub kv_cache_type: String,
    pub seed_base: u64,
    pub order: String,
    pub budget_secs: u64,
    pub port: u16,
}

pub fn run(args: ProbeArgs) -> Result<()> {
    let cfg = RunConfig::load(&args.config)?;
    let config_file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args.config)?)?;
    let set = ProblemSet::load(&args.problems, args.seed_base, &args.order)?;

    // The artifact receipts, resolved the fleet's own way (the same
    // path the supervisor launches) — parse for dtype/params, hash for
    // provenance.
    let models_root = resolve_under_artifacts_dir(Path::new("private-inference/models"));
    let files = fleet::resolve_model_files(&models_root, &args.model_id)
        .with_context(|| format!("resolve the fleet artifact for `{}`", args.model_id))?;
    let artifact = files
        .first()
        .context("the fleet artifact list is non-empty")?;
    let meta = pi_gguf::parse_file(artifact)?;
    let gguf_sha256 = gguf_writer::sha256_file(artifact)?;
    let model_name = artifact
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| artifact.display().to_string());
    let dtype = gguf_writer::type_name(meta.dominant_type()).to_string();

    // The route: one supervisor launch per leg, one policy from the CLI.
    let n_ctx = args.cap + 4096;
    let policy = PlacementPolicy {
        context_length: n_ctx,
        threads: args.threads,
        n_gpu_layers: args.n_gpu_layers,
        partial_gpu_offload: args.partial_gpu_offload,
        kv_cache_type: args.kv_cache_type.clone(),
        flash_attn: true,
        cpu_moe: true,
        draft: false,
        host: "127.0.0.1".into(),
        port: args.port,
        ..PlacementPolicy::default()
    };
    let serving =
        Serving::launch(&args.model_id, policy).with_context(|| "the canonical serving route")?;
    eprintln!(
        "probe: cfg={} model={} ({} params, {}) engine=[{}] endpoint={} plan=[{}]",
        cfg.name,
        serving.model_id,
        meta.param_count,
        dtype,
        serving.engine_version,
        serving.endpoint,
        serving.launch_plan
    );

    let t_start = Instant::now();
    let mut counts = Counts::default();
    let mut out = std::io::BufWriter::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&args.out_jsonl)
            .with_context(|| format!("open rows file {}", args.out_jsonl.display()))?,
    );
    let mut first_ids: Vec<String> = Vec::new();
    let mut n_ok = 0u32;
    let mut budget_stopped = false;
    for i in 0..args.count {
        if t_start.elapsed().as_secs() > args.budget_secs {
            eprintln!(
                "[budget] stop before problem {} (elapsed {}s > budget {}s)",
                i,
                t_start.elapsed().as_secs(),
                args.budget_secs
            );
            budget_stopped = true;
            break;
        }
        let row_idx = args.start + i;
        let problem = set.at_row(row_idx);
        let problem_id = set.problem_id(row_idx);
        if first_ids.len() < 5 {
            first_ids.push(problem_id.clone());
        }
        let seed = args.seed_base + args.start as u64 + i as u64;
        let row = probe_row(
            &serving,
            &cfg,
            &problem_id,
            &problem.question,
            &problem.gold(),
            &RunFacts {
                cap: args.cap,
                seed,
                n_ctx,
                threads: args.threads,
                n_gpu_layers: args.n_gpu_layers,
                model: model_name.clone(),
                dtype: dtype.clone(),
                gguf_sha256: gguf_sha256.clone(),
                param_count: meta.param_count,
                engine_version: serving.engine_version.clone(),
                config_file: config_file.clone(),
            },
        )
        .with_context(|| format!("probe {problem_id}"))?;
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
        n_ok += 1;
    }

    let n = n_ok;
    let mut mean_wall = 0.0;
    let mut mean_tps = 0.0;
    // recompute means from the rows we wrote (the honest way: reread)
    for line in std::io::BufReader::new(std::fs::File::open(&args.out_jsonl)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
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
        mean_wall_secs: receipt::r1(mean_wall),
        mean_tok_per_sec: receipt::r2(mean_tps),
        gguf: model_name.clone(),
        gguf_sha256: gguf_sha256.clone(),
        dtype: dtype.clone(),
        elapsed_secs: receipt::r1(t_start.elapsed().as_secs_f64()),
        order: args.order.clone(),
        harness: HARNESS.into(),
        model_id: serving.model_id.clone(),
        engine_version: serving.engine_version.clone(),
        endpoint: serving.endpoint.clone(),
        launch_plan: serving.launch_plan.clone(),
        budget_secs: args.budget_secs,
        first_problem_ids: first_ids,
    };
    let json = serde_json::to_string_pretty(&summary)?;
    if let Some(path) = &args.out_summary {
        std::fs::write(path, &json)?;
    }
    eprintln!("summary: {json}");
    if budget_stopped {
        eprintln!(
            "[budget] the leg stopped at {n}/{} rows — a transitional record, never resumed as-is",
            args.count
        );
    }
    serving.stop()?;
    Ok(())
}

/// Everything a row needs from the launch + artifact, computed once.
struct RunFacts {
    cap: u32,
    seed: u64,
    n_ctx: u32,
    threads: u32,
    n_gpu_layers: u32,
    model: String,
    dtype: String,
    gguf_sha256: String,
    param_count: u64,
    engine_version: String,
    config_file: serde_json::Value,
}

/// One probe row: render, complete (parity-verified), classify, receipt.
fn probe_row(
    serving: &Serving,
    cfg: &RunConfig,
    problem_id: &str,
    question: &str,
    gold: &str,
    facts: &RunFacts,
) -> Result<RunRow> {
    let user_msg = format!("{question}{}", PROMPT_SUFFIX);
    let prompt = serving.render_prompt(&user_msg, cfg.enable_thinking)?;

    let request = CompletionRequest {
        prompt,
        n_predict: facts.cap,
        temperature: f64::from(cfg.temperature),
        top_p: f64::from(cfg.top_p),
        top_k: cfg.top_k,
        min_p: f64::from(cfg.min_p),
        presence_penalty: f64::from(cfg.presence_penalty),
        repeat_penalty: f64::from(cfg.repetition_penalty),
        seed: facts.seed as u32,
        // The card semantics: the presence-penalty window is the whole
        // context — pinned on the request, echoed by the server,
        // verified per row.
        repeat_last_n: facts.n_ctx as i32,
    };
    let t0 = Instant::now();
    let response = serving.complete(&request)?;
    let wall = t0.elapsed().as_secs_f64();

    let (extracted, via) = receipt::extract_answer(&response.content);
    let hit_limit = response.tokens_predicted >= facts.cap as u64;
    let correct = !hit_limit
        && extracted
            .as_deref()
            .is_some_and(|e| receipt::numeric_grade(e, gold));
    let label = if hit_limit {
        "hit_limit"
    } else if correct {
        "correct_complete"
    } else {
        "wrong_complete"
    };

    Ok(RunRow {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        dropped_gen_kwargs: vec![],
        config: cfg.name.clone(),
        problem_id: problem_id.to_string(),
        source: "gsm8k-train".into(),
        gold: gold.to_string(),
        kind: "numeric".into(),
        prompt_tokens: response.tokens_evaluated as u32,
        new_tokens: response.tokens_predicted as u32,
        wall_secs: receipt::r2(wall),
        tok_per_sec: if wall > 0.0 {
            receipt::r2(response.tokens_predicted as f64 / wall)
        } else {
            0.0
        },
        hit_limit,
        extracted: extracted.clone(),
        extraction_via: via,
        correct,
        label: label.into(),
        tail_distinct_ratio: receipt::tail_distinct_ratio(&response.content, 256),
        cap: facts.cap,
        seed: facts.seed,
        enable_thinking_applied: true,
        presence_penalty_applied: f64::from(cfg.presence_penalty),
        model: facts.model.clone(),
        dtype: facts.dtype.clone(),
        device: if facts.n_gpu_layers > 0 { "gpu" } else { "cpu" }.into(),
        threads: facts.threads,
        kernelize: false,
        param_count: facts.param_count,
        config_file: facts.config_file.clone(),
        harness: HARNESS.into(),
        backend: format!("llama.cpp ({})", facts.engine_version),
        gguf_sha256: facts.gguf_sha256.clone(),
        penalty_last_n: facts.n_ctx as i32,
        n_ctx: facts.n_ctx,
        engine_version: facts.engine_version.clone(),
        server_generation_settings: response.generation_settings.clone(),
    })
}

/// Summarize an existing JSONL (the `summary` subcommand) — recompute
/// counts/means from rows; the route receipts carry what the rows
/// carry (engine_version; the launch-level fields re-derive from the
/// first row where present and stay empty otherwise).
pub fn summarize(rows: &Path, out: Option<&Path>) -> Result<()> {
    let mut counts = Counts::default();
    let mut n = 0u32;
    let mut mean_wall = 0.0;
    let mut mean_tps = 0.0;
    let mut cap = 0;
    let mut config = String::new();
    let mut first: Option<serde_json::Value> = None;
    let mut first_ids: Vec<String> = Vec::new();
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
        if first_ids.len() < 5 {
            if let Some(id) = v["problem_id"].as_str() {
                first_ids.push(id.to_string());
            }
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
    let f = first.clone().unwrap_or_default();
    let summary = StageSummary {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        config,
        n,
        counts,
        cap,
        mean_wall_secs: receipt::r1(mean_wall),
        mean_tok_per_sec: receipt::r2(mean_tps),
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
        model_id: f
            .get("model_id")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        engine_version: f["engine_version"].as_str().unwrap_or("").to_string(),
        endpoint: String::new(),
        launch_plan: String::new(),
        budget_secs: 0,
        first_problem_ids: first_ids,
    };
    let json = serde_json::to_string_pretty(&summary)?;
    if let Some(path) = out {
        std::fs::write(path, &json)?;
    }
    println!("{json}");
    Ok(())
}
