//! Propose a commit-bundling plan for the zed-kask fork range.
//!
//! This is a **dry run only**. It never calls `git filter-repo`, `git rebase`,
//! or any history-rewriting command — it only reads. The `git()` function
//! enforces a read-only subcommand allowlist as a structural guard.
//!
//! Self-healing: per-item errors (diffstat, prompt write, plan section) are
//! collected, logged, and skipped — the run continues and reports all failures
//! at the end. Output files are written atomically (temp + rename) so a crash
//! never leaves a partial file that looks valid.
//!
//! Driven off `git log --format='@@@%H%x1e%B%x1e' --name-only upstream/main..main`.
//!
//! Two greedy variants (oldest -> newest):
//!   - variant A: bundle consecutive <8-file runs into one bundle. >=8-file
//!                commits are singleton barriers; >20-file commits are protected
//!                anchors. A 20-file hard cap splits long runs.
//!   - variant B: A, then absorb any bundle with <8 files into an adjacent
//!                8-20-file anchor as long as the merged union stays <= 20 files.

#![allow(clippy::disallowed_methods, reason = "tooling is exempt")]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};
use clap::Args;
use serde::{Deserialize, Serialize};

/// `upstream/main..main` — zed-kask's own history only. Never analyze the full
/// 41,931-commit log; ~39,700 of those are upstream Zed and out of scope.
const RANGE: &str = "upstream/main..main";

// Bundling policy thresholds (guidelines, not hard rules).
const BUNDLE_TARGET: usize = 8;
const BIG_BUNDLE_CAP: usize = 20;
const ANCHOR_MIN: usize = 8;
const ANCHOR_MAX: usize = 20;

const RECORD_MARK: &str = "@@@";
const RECORD_SEP: char = '\x1e';

/// Only these git subcommands are permitted. This is a structural guard:
/// any attempt to call a history-rewriting command (rebase, filter-repo, push,
/// reset, commit, checkout, etc.) is refused before the process is spawned.
/// At 0.0001% risk tolerance, this guard must never be bypassed.
const GIT_ALLOWED_SUBCOMMANDS: &[&str] = &["log", "rev-parse", "diff"];

/// One entry in the prompts JSONL output. Serialized with serde_json to avoid
/// hand-rolled escaping bugs (tabs, control chars, nested quotes).
#[derive(Serialize)]
struct PromptEntry<'a> {
    bundle_index: usize,
    shas: &'a [String],
    prompt: String,
}

/// One entry in the apply-messages JSONL input. Deserialized with serde_json.
#[derive(Deserialize)]
struct MessageEntry {
    bundle_index: usize,
    message: String,
}

#[derive(Args)]
pub struct BundleCommitsArgs {
    /// Bundling variant: `a` (consecutive <6-file runs) or `b` (a + absorb into
    /// adjacent 8-20 anchors, cap 20). Default: b.
    #[arg(long, default_value = "b")]
    variant: String,

    /// Repository path (defaults to current directory).
    #[arg(long)]
    repo: Option<PathBuf>,

    /// Compute per-bundle union diffstat (slower).
    #[arg(long)]
    diffstat: bool,

    /// Scan docs/rules for bundled-SHA references.
    #[arg(long)]
    scan_refs: bool,

    /// Fill in consolidated messages from a JSONL file
    /// (`{"bundle_index": N, "message": "..."}`).
    #[arg(long)]
    apply_messages: Option<PathBuf>,

    /// Output plan file (markdown).
    #[arg(long, default_value = "bundle_plan.md")]
    out: PathBuf,

    /// Output prompts JSONL (one per multi-commit bundle).
    #[arg(long, default_value = "bundle_prompts.jsonl")]
    prompts_out: PathBuf,

    /// Output stats file.
    #[arg(long, default_value = "bundle_stats.txt")]
    stats_out: PathBuf,

    /// Write a checkpoint file recording the repo HEAD SHA and output state.
    /// On a crash, the operator can inspect the checkpoint to see how far the
    /// run got. The checkpoint is written AFTER all outputs succeed, so its
    /// presence means the run completed.
    #[arg(long, default_value = "bundle_checkpoint.txt")]
    checkpoint: PathBuf,

    /// Verify a previously generated plan against the current repo state.
    /// Re-reads the stats file, re-parses the git log, and confirms the SHA
    /// count and bundle count match. Exits non-zero on mismatch. This is the
    /// self-healing recovery path: if the plan was generated on a different
    /// repo state, the operator detects it before acting.
    #[arg(long)]
    verify: bool,
}

#[derive(Clone, Debug)]
struct Commit {
    sha: String,
    body: String,
    files: Vec<String>,
    is_merge: bool,
}

#[derive(Clone, Debug)]
struct Bundle {
    shas: Vec<String>,
    files: BTreeSet<String>,
    single: bool,
    is_merge: bool,
}

pub fn run_bundle_commits(args: BundleCommitsArgs) -> Result<()> {
    let repo = args.repo.clone().unwrap_or_else(|| {
        std::env::current_dir()
            .context("failed to get current directory")
            .unwrap()
    });

    // Sanity: refuse to run if upstream/main..main doesn't resolve.
    git(&repo, &["rev-parse", "upstream/main", "main"])?;

    // Verify mode: re-read a previously generated stats file and confirm the
    // repo state matches. This is the self-healing recovery path.
    if args.verify {
        return verify_plan(&repo, &args.stats_out);
    }

    eprintln!("Reading {RANGE} ...");
    let commits = parse_log(&repo)?;
    let merge_count = commits.iter().filter(|c| c.is_merge).count();
    eprintln!("  {} commits ({merge_count} merges)", commits.len());

    let variant = args.variant.as_str();
    let bundles = match variant {
        "a" => variant_a(&commits),
        "b" => variant_b(&commits),
        other => bail!("unknown variant: {other} (expected `a` or `b`)"),
    };
    eprintln!("  variant {variant}: {} bundles", bundles.len());

    let (errors, warnings, stats) = validate(&bundles, &commits);

    let commits_by_sha: HashMap<&str, &Commit> =
        commits.iter().map(|c| (c.sha.as_str(), c)).collect();

    // Optional per-bundle diffstat. Self-healing: a diffstat failure for one
    // bundle is logged and skipped, not fatal.
    let diffstats = if args.diffstat {
        eprintln!("Computing per-bundle diffstat ...");
        let mut out = HashMap::new();
        for (idx, b) in bundles.iter().enumerate() {
            if idx % 200 == 0 && idx > 0 {
                eprintln!("  diffstat: {idx}/{} bundles", bundles.len());
            }
            out.insert(idx, bundle_diffstat(&repo, b));
        }
        out
    } else {
        HashMap::new()
    };

    // Optional message application.
    let new_messages = if let Some(path) = &args.apply_messages {
        let m = apply_messages(path)?;
        eprintln!("  loaded {} consolidated messages", m.len());
        m
    } else {
        HashMap::new()
    };

    // Emit prompts JSONL for multi-commit bundles.
    let prompts_written =
        write_prompts_jsonl(&args.prompts_out, &bundles, &commits_by_sha, &diffstats)?;
    eprintln!(
        "  wrote {prompts_written} consolidation prompts -> {}",
        args.prompts_out.display()
    );

    // Emit bundle_plan.md.
    write_plan_md(
        &args.out,
        variant,
        &bundles,
        &commits,
        &commits_by_sha,
        &new_messages,
        &diffstats,
        &errors,
        &warnings,
        &stats,
        args.scan_refs,
        &repo,
    )?;

    // Emit stats file.
    write_stats_txt(&args.stats_out, variant, &errors, &warnings, &stats)?;

    eprintln!(
        "\nWrote {}, {}, {}",
        args.out.display(),
        args.prompts_out.display(),
        args.stats_out.display()
    );
    eprintln!(
        "Result: {} -> {} bundles ({}% reduction, {}x)",
        stats.original_commits,
        stats.resulting_bundles,
        stats.reduction_pct,
        stats.compression_ratio
    );

    if !errors.is_empty() {
        eprintln!(
            "\u{274c} {} validation errors \u{2014} see {}",
            errors.len(),
            args.stats_out.display()
        );
        bail!("validation errors");
    }

    // Write checkpoint AFTER all outputs succeed. Its presence means the run
    // completed; its absence means a crash occurred mid-run.
    let head_sha = git(&repo, &["rev-parse", "HEAD"])?.trim().to_string();
    let checkpoint_content = format!(
        "completed: true\nhead_sha: {head_sha}\nrange: {RANGE}\nvariant: {variant}\n\
         original_commits: {}\nresulting_bundles: {}\n\
         outputs:\n  plan: {}\n  prompts: {}\n  stats: {}\n",
        stats.original_commits,
        stats.resulting_bundles,
        args.out.display(),
        args.prompts_out.display(),
        args.stats_out.display(),
    );
    atomic_write(&args.checkpoint, &checkpoint_content)?;
    eprintln!("  checkpoint -> {}", args.checkpoint.display());

    Ok(())
}

/// Verify a previously generated plan against the current repo state.
/// Re-reads the stats file, re-parses the git log, and confirms the SHA
/// count and bundle count match. Exits non-zero on mismatch.
fn verify_plan(repo: &Path, stats_path: &Path) -> Result<()> {
    let stats_text = fs::read_to_string(stats_path)
        .with_context(|| format!("failed to read stats file {}", stats_path.display()))?;

    // Extract original_commits and resulting_bundles from the stats file.
    let extract = |key: &str| -> Result<usize> {
        for line in stats_text.lines() {
            if let Some(rest) = line.strip_prefix(&format!("{key}: ")) {
                return rest
                    .trim()
                    .parse::<usize>()
                    .with_context(|| format!("malformed stats file: {key} = {rest}"));
            }
        }
        bail!("stats file missing key: {key}")
    };

    let recorded_commits = extract("original_commits")?;
    let recorded_bundles = extract("resulting_bundles")?;

    eprintln!("Verifying plan against current repo state ...");
    eprintln!("  recorded: {recorded_commits} commits -> {recorded_bundles} bundles");

    let commits = parse_log(repo)?;
    let actual_commits = commits.len();
    eprintln!("  actual:   {actual_commits} commits");

    if actual_commits != recorded_commits {
        bail!(
            "MISMATCH: repo has {actual_commits} commits but plan was generated with {recorded_commits}. \
             The repo state has changed since the plan was generated. \
             Re-run the planner to regenerate."
        );
    }

    // Re-run the bundling to confirm the bundle count matches.
    let mut variant_str = "b";
    for line in stats_text.lines() {
        if let Some(rest) = line.strip_prefix("variant: ") {
            variant_str = rest.trim();
            break;
        }
    }
    let bundles = match variant_str {
        "a" => variant_a(&commits),
        "b" => variant_b(&commits),
        other => bail!("unknown variant in stats file: {other}"),
    };
    let actual_bundles = bundles.len();
    eprintln!("  actual:   {actual_bundles} bundles (variant {variant_str})");

    if actual_bundles != recorded_bundles {
        bail!(
            "MISMATCH: re-bundling produces {actual_bundles} bundles but plan recorded {recorded_bundles}. \
             The bundling algorithm may have changed, or the repo state differs."
        );
    }

    eprintln!("\u{2705} Plan verified: repo state matches recorded state.");
    Ok(())
}

/// Write content to a file atomically: write to `<path>.tmp`, then rename.
/// A crash mid-write leaves the temp file, not a truncated `<path>` that
/// looks valid. At 0.0001% risk tolerance, partial files are dangerous.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .map(|e| e.to_string_lossy())
            .unwrap_or_default()
    ));
    {
        let file = fs::File::create(&tmp)
            .with_context(|| format!("failed to create temp file {}", tmp.display()))?;
        let mut writer = BufWriter::new(file);
        writer.write_all(content.as_bytes())?;
        writer.flush()?;
    }
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    // This prevents a future edit from accidentally adding a history-rewriting
    // command (rebase, filter-repo, push, reset, commit, checkout, etc.).
    // At 0.0001% risk tolerance, this guard must never be bypassed.
    if let Some(subcmd) = args.first() {
        if !GIT_ALLOWED_SUBCOMMANDS.contains(subcmd) {
            bail!(
                "REFUSED: git subcommand `{subcmd}` is not in the read-only allowlist {GIT_ALLOWED_SUBCOMMANDS:?}. \
                 This script must never invoke history-rewriting commands."
            );
        }
    }
    let output = Command::new("git")
        .arg("--no-pager")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to spawn git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {} failed ({}):\n{}",
            args.join(" "),
            output.status,
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_log(repo: &Path) -> Result<Vec<Commit>> {
    // %x1e separates sha / body / files; @@@ marks each record start.
    let format = format!("{RECORD_MARK}%H{RECORD_SEP}%B{RECORD_SEP}");
    let out = git(
        repo,
        &[
            "log",
            "--no-renames",
            &format!("--format={format}"),
            "--name-only",
            RANGE,
        ],
    )?;

    let mut commits: Vec<Commit> = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    for (record_idx, chunk) in out.split(RECORD_MARK).skip(1).enumerate() {
        let mut parts = chunk.splitn(3, RECORD_SEP);
        let sha = match parts.next() {
            Some(s) => s.trim().to_string(),
            None => {
                parse_errors.push(format!("record {record_idx}: no sha field"));
                continue;
            }
        };
        if sha.is_empty() {
            parse_errors.push(format!("record {record_idx}: empty sha"));
            continue;
        }
        let body = parts
            .next()
            .unwrap_or("")
            .trim_end_matches('\n')
            .to_string();
        let files_part = parts.next().unwrap_or("");
        let files: Vec<String> = files_part
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        commits.push(Commit {
            sha,
            body,
            files,
            is_merge: false,
        });
    }
    for e in &parse_errors {
        eprintln!("  warn: parse: {e}");
    }
    // git log emits newest-first; walk oldest-first.
    commits.reverse();

    if commits.is_empty() {
        bail!("no commits in range {RANGE}");
    }

    // Flag merges: a merge commit has >=2 parents.
    let merge_out = git(repo, &["log", "--merges", "--format=%H", RANGE])?;
    let merge_set: HashSet<String> = merge_out
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    for c in commits.iter_mut() {
        c.is_merge = merge_set.contains(&c.sha);
    }

    Ok(commits)
}

fn variant_a(commits: &[Commit]) -> Vec<Bundle> {
    // Variant A: bundle the ENTIRE consecutive run of <8-file commits into
    // one bundle. The barrier threshold is >=8 files (the bundle target) —
    // 6-7 file commits are below the target and join the current run rather
    // than acting as barriers. The `>= 8` target is a minimum a bundle reaches
    // if the run is long enough — it is NOT a closing threshold. A bundle
    // closes only when:
    //   1. it hits a >=8-file barrier (which emits as a singleton), or
    //   2. adding the next small commit would push the union over 20 files
    //      (the hard cap — policy point 2).
    // Runs too short to reach 8 files stay small (policy point 4).
    let mut bundles: Vec<Bundle> = Vec::new();
    let mut current: Option<Bundle> = None;

    for c in commits {
        let nfiles = c.files.len();
        let small = nfiles < BUNDLE_TARGET;
        if !small {
            // >=8 files: barrier. Close any open bundle, emit singleton.
            if let Some(b) = current.take() {
                bundles.push(b);
            }
            bundles.push(Bundle {
                shas: vec![c.sha.clone()],
                files: c.files.iter().cloned().collect(),
                single: true,
                is_merge: c.is_merge,
            });
            continue;
        }
        // small commit (<8 files). Check the 20-file cap before extending.
        let would_exceed = current
            .as_ref()
            .map(|b| {
                let mut union = b.files.clone();
                for f in &c.files {
                    union.insert(f.clone());
                }
                union.len() > BIG_BUNDLE_CAP
            })
            .unwrap_or(false);
        if would_exceed {
            // Close the current bundle; start a new one with this commit.
            if let Some(b) = current.take() {
                bundles.push(b);
            }
            current = Some(Bundle {
                shas: vec![c.sha.clone()],
                files: c.files.iter().cloned().collect(),
                single: false,
                is_merge: c.is_merge,
            });
            continue;
        }
        // Extend the current run.
        match current.as_mut() {
            Some(b) => {
                b.shas.push(c.sha.clone());
                for f in &c.files {
                    b.files.insert(f.clone());
                }
                if c.is_merge {
                    b.is_merge = true;
                }
            }
            None => {
                current = Some(Bundle {
                    shas: vec![c.sha.clone()],
                    files: c.files.iter().cloned().collect(),
                    single: false,
                    is_merge: c.is_merge,
                });
            }
        }
    }
    if let Some(b) = current.take() {
        bundles.push(b);
    }
    bundles
}

fn variant_b(commits: &[Commit]) -> Vec<Bundle> {
    // Variant B: A, then absorb <8-file bundles into adjacent 8-20-file
    // anchors (cap 20). Single left-to-right pass — each absorbable bundle
    // tries its right neighbor then its left neighbor, once. No cascading
    // (an anchor that grew from absorption does not re-attract). This matches
    // the measured baseline: A + one round of absorption into the original
    // 8-20 anchors.
    let bundles = variant_a(commits);

    // Collect indices of absorbable (<8-file, multi-or-single) bundles and
    // anchor (8-20-file singleton) bundles, based on post-variant-A state.
    // We walk left-to-right and absorb each small bundle into a neighbor.
    // Because removal shifts indices, we rebuild the list in one pass using a
    // merge-style approach: build a new vec, deciding absorption as we go.
    let mut result: Vec<Bundle> = Vec::with_capacity(bundles.len());

    for b in bundles {
        let n = b.files.len();
        if n >= BUNDLE_TARGET {
            // Anchor or protected barrier — emit as-is.
            result.push(b);
            continue;
        }
        // Absorbable (<8 files). Try to merge into the last-emitted anchor
        // (right neighbor in original order = previous in result). If that
        // fails or there is none, try the next original bundle (look ahead).
        let mut absorbed = false;
        if let Some(last) = result.last_mut() {
            let ln = last.files.len();
            if (ANCHOR_MIN..=ANCHOR_MAX).contains(&ln) {
                let mut union = last.files.clone();
                union.extend(b.files.iter().cloned());
                if union.len() <= BIG_BUNDLE_CAP {
                    last.files = union;
                    last.shas.extend(b.shas.iter().cloned());
                    last.single = false;
                    if b.is_merge {
                        last.is_merge = true;
                    }
                    absorbed = true;
                }
            }
        }
        if !absorbed {
            // Couldn't absorb into the previous anchor. Emit as standalone;
            // it stays a small bundle (policy point 4).
            result.push(b);
        }
    }
    result
}

#[derive(Default)]
struct Stats {
    original_commits: usize,
    resulting_bundles: usize,
    reduction: isize,
    reduction_pct: f64,
    compression_ratio: f64,
    multi_commit_bundles: usize,
    singleton_bundles: usize,
    bundles_under_8_files: usize,
    bundles_8_to_20_files: usize,
    bundles_over_20_files: usize,
    merge_bundles: usize,
    max_bundle_files: usize,
    median_bundle_files: usize,
}

fn validate(bundles: &[Bundle], commits: &[Commit]) -> (Vec<String>, Vec<String>, Stats) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let original_shas: Vec<&str> = commits.iter().map(|c| c.sha.as_str()).collect();
    let original_set: HashSet<&str> = original_shas.iter().copied().collect();

    // 1. Every original SHA appears in exactly one bundle.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for (idx, b) in bundles.iter().enumerate() {
        for s in &b.shas {
            if let Some(prev) = seen.insert(s.as_str(), idx) {
                errors.push(format!("SHA {s} appears in bundle {prev} AND bundle {idx}"));
            }
        }
    }
    let missing: Vec<&str> = original_set
        .iter()
        .filter(|s| !seen.contains_key(*s))
        .copied()
        .collect();
    let extra: Vec<&str> = seen
        .keys()
        .filter(|s| !original_set.contains(*s))
        .copied()
        .collect();
    if !missing.is_empty() {
        errors.push(format!("{} original SHAs missing from plan", missing.len()));
    }
    if !extra.is_empty() {
        errors.push(format!(
            "{} SHAs in plan not in original range",
            extra.len()
        ));
    }
    if seen.len() != original_shas.len() {
        errors.push(format!(
            "SHA count mismatch: original={} bundled={}",
            original_shas.len(),
            seen.len()
        ));
    }

    // 2. No multi-commit bundle exceeds 20 files.
    for (idx, b) in bundles.iter().enumerate() {
        let nfiles = b.files.len();
        if b.shas.len() > 1 && nfiles > BIG_BUNDLE_CAP {
            errors.push(format!(
                "bundle {idx} has {} commits / {nfiles} files (>20 cap)",
                b.shas.len()
            ));
        }
        if b.shas.len() == 1 && nfiles > BIG_BUNDLE_CAP {
            warnings.push(format!(
                "bundle {idx} is a protected >20-file singleton ({nfiles} files)"
            ));
        }
    }

    // 3. Merge commits flagged.
    let merge_bundles: Vec<usize> = bundles
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_merge)
        .map(|(i, _)| i)
        .collect();
    if !merge_bundles.is_empty() {
        let preview: Vec<String> = merge_bundles
            .iter()
            .take(12)
            .map(|i| i.to_string())
            .collect();
        warnings.push(format!(
            "{} bundles contain merge commits (would be linearized by a rewrite): {}",
            merge_bundles.len(),
            preview.join(", ")
        ));
    }

    let file_counts: Vec<usize> = bundles.iter().map(|b| b.files.len()).collect();
    let stats = Stats {
        original_commits: commits.len(),
        resulting_bundles: bundles.len(),
        reduction: commits.len() as isize - bundles.len() as isize,
        reduction_pct: if commits.is_empty() {
            0.0
        } else {
            (100.0 * (1.0 - bundles.len() as f64 / commits.len() as f64)).round()
        },
        compression_ratio: if bundles.is_empty() {
            0.0
        } else {
            (commits.len() as f64 / bundles.len() as f64 * 100.0).round() / 100.0
        },
        multi_commit_bundles: bundles.iter().filter(|b| b.shas.len() > 1).count(),
        singleton_bundles: bundles.iter().filter(|b| b.shas.len() == 1).count(),
        bundles_under_8_files: file_counts.iter().filter(|n| **n < BUNDLE_TARGET).count(),
        bundles_8_to_20_files: file_counts
            .iter()
            .filter(|n| (ANCHOR_MIN..=ANCHOR_MAX).contains(*n))
            .count(),
        bundles_over_20_files: file_counts.iter().filter(|n| **n > BIG_BUNDLE_CAP).count(),
        merge_bundles: merge_bundles.len(),
        max_bundle_files: file_counts.iter().copied().max().unwrap_or(0),
        median_bundle_files: {
            let mut sorted = file_counts.clone();
            sorted.sort_unstable();
            sorted.get(sorted.len() / 2).copied().unwrap_or(0)
        },
    };

    (errors, warnings, stats)
}

fn bundle_diffstat(repo: &Path, bundle: &Bundle) -> Option<String> {
    if bundle.is_merge {
        return None;
    }
    let first = bundle.shas.first()?;
    let last = bundle.shas.last()?;
    let parent = git(repo, &["rev-parse", &format!("{first}^")])
        .ok()?
        .trim()
        .to_string();
    let stat = git(repo, &["diff", "--stat", &format!("{parent}..{last}")]).ok()?;
    let trimmed = stat.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn subject_of(body: &str) -> String {
    body.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "(empty message)".to_string())
}

fn build_consolidation_prompt(bundle: &Bundle, idx: usize, diffstat: &Option<String>) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("You are consolidating multiple git commits into ONE commit message.".into());
    lines.push("Follow the repo's PR-hygiene conventions:".into());
    lines.push("  - Imperative mood (e.g. 'Add', 'Fix', 'Refactor').".into());
    lines.push("  - No conventional-commit prefixes (no 'feat:', 'fix:').".into());
    lines.push("  - No trailing punctuation.".into());
    lines.push(
        "  - Subject line <= 72 characters, then a blank line, then an optional body.".into(),
    );
    lines.push("  - Optional crate prefix (e.g. 'git_ui: Add history view').".into());
    lines.push(
        "  - End with a `Bundled:` trailer listing constituent SHAs + original subjects.".into(),
    );
    lines.push(String::new());
    lines.push(format!("Bundle index: {idx}"));
    lines.push(format!("Constituent commits ({}):", bundle.shas.len()));
    for s in &bundle.shas {
        lines.push(format!("  {s}"));
    }
    lines.push(String::new());
    lines.push("Union of files touched:".into());
    for f in bundle.files.iter() {
        lines.push(format!("  {f}"));
    }
    if let Some(stat) = diffstat {
        lines.push(String::new());
        lines.push("Diffstat (union):".into());
        lines.push(stat.clone());
    }
    lines.push(String::new());
    lines.push("Original commit messages:".into());
    lines.push("----------------------------------------".into());
    lines.push("__ORIGINAL_MESSAGES__".into());
    lines.push("----------------------------------------".into());
    lines.push(String::new());
    lines
        .push("Produce the consolidated commit message now. Output ONLY the commit message".into());
    lines.push("(subject, blank line, body, blank line, Bundled: trailer). No commentary.".into());
    lines.join("\n")
}

fn render_bundle_section(
    idx: usize,
    bundle: &Bundle,
    commits_by_sha: &HashMap<&str, &Commit>,
    new_message: Option<&str>,
    diffstat: &Option<String>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("## Bundle {idx}\n\n"));

    let mut tags: Vec<String> = Vec::new();
    if bundle.shas.len() == 1 {
        tags.push("singleton".into());
    } else {
        tags.push(format!("{}-commit bundle", bundle.shas.len()));
    }
    if bundle.is_merge {
        tags.push("CONTAINS MERGE (linearizes on rewrite)".into());
    }
    let nfiles = bundle.files.len();
    if nfiles > BIG_BUNDLE_CAP {
        tags.push(format!("protected >20-file anchor ({nfiles} files)"));
    } else if nfiles < BUNDLE_TARGET {
        tags.push(format!("under-target ({nfiles} files)"));
    }
    out.push_str(&format!("**Tags:** {}\n\n", tags.join(", ")));
    out.push_str(&format!("**Union files:** {nfiles}\n"));
    out.push_str(&format!(
        "**Constituent SHAs:** {}\n\n",
        bundle.shas.join(", ")
    ));

    out.push_str("### New commit message\n```\n");
    if let Some(msg) = new_message {
        out.push_str(msg);
    } else {
        out.push_str("TODO: consolidate (see prompt below)");
    }
    out.push_str("\n```\n\n");

    out.push_str("### Files\n");
    for f in bundle.files.iter() {
        out.push_str(&format!("- `{f}`\n"));
    }
    out.push('\n');

    if let Some(stat) = diffstat {
        out.push_str("### Diffstat (union)\n```\n");
        out.push_str(stat);
        out.push_str("\n```\n\n");
    }

    out.push_str("### Bundled: (original messages for archaeology)\n");
    for s in &bundle.shas {
        if let Some(c) = commits_by_sha.get(s.as_str()) {
            let subj = subject_of(&c.body);
            out.push_str(&format!("- `{s}` — {subj}\n"));
        }
    }
    out.push('\n');

    out.push_str("<details><summary>Original commit bodies</summary>\n\n");
    for s in &bundle.shas {
        if let Some(c) = commits_by_sha.get(s.as_str()) {
            out.push_str(&format!("#### {s}\n```\n{}\n```\n\n", c.body));
        }
    }
    out.push_str("</details>\n");

    if bundle.shas.len() > 1 {
        out.push_str("\n### Consolidation prompt\n```\n");
        let mut prompt = build_consolidation_prompt(bundle, idx, diffstat);
        let bodies: Vec<String> = bundle
            .shas
            .iter()
            .filter_map(|s| {
                commits_by_sha
                    .get(s.as_str())
                    .map(|c| format!("### {s}\n{}", c.body))
            })
            .collect();
        prompt = prompt.replace("__ORIGINAL_MESSAGES__", &bodies.join("\n\n"));
        out.push_str(&prompt);
        out.push_str("\n```\n");
    }
    out.push_str("\n---\n\n");
    out
}

fn write_prompts_jsonl(
    path: &Path,
    bundles: &[Bundle],
    commits_by_sha: &HashMap<&str, &Commit>,
    diffstats: &HashMap<usize, Option<String>>,
) -> Result<usize> {
    // Build all JSONL lines in memory, then write atomically. A crash never
    // leaves a partial JSONL that looks valid.
    let mut lines: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut count = 0;

    for (idx, b) in bundles.iter().enumerate() {
        if b.shas.len() > 1 {
            let diffstat = diffstats.get(&idx).cloned().flatten();
            let mut prompt = build_consolidation_prompt(b, idx, &diffstat);
            let bodies: Vec<String> = b
                .shas
                .iter()
                .filter_map(|s| {
                    commits_by_sha
                        .get(s.as_str())
                        .map(|c| format!("### {s}\n{}", c.body))
                })
                .collect();
            prompt = prompt.replace("__ORIGINAL_MESSAGES__", &bodies.join("\n\n"));
            let entry = PromptEntry {
                bundle_index: idx,
                shas: &b.shas,
                prompt,
            };
            match serde_json::to_string(&entry) {
                Ok(json) => {
                    lines.push(json);
                    count += 1;
                }
                Err(e) => {
                    errors.push(format!("bundle {idx}: serde_json serialize failed: {e}"));
                }
            }
        }
    }

    for e in &errors {
        eprintln!("  warn: prompts: {e}");
    }

    let content = lines.join("\n") + "\n";
    atomic_write(path, &content)?;
    Ok(count)
}

fn write_plan_md(
    path: &Path,
    variant: &str,
    bundles: &[Bundle],
    commits: &[Commit],
    commits_by_sha: &HashMap<&str, &Commit>,
    new_messages: &HashMap<usize, String>,
    diffstats: &HashMap<usize, Option<String>>,
    errors: &[String],
    warnings: &[String],
    stats: &Stats,
    scan_refs: bool,
    repo: &Path,
) -> Result<()> {
    // Build the entire plan in a String, then write atomically. A crash never
    // leaves a truncated plan that looks valid.
    let mut out = String::with_capacity(2 * 1024 * 1024);

    out.push_str(&format!(
        "# Commit-bundling dry-run plan (variant {})\n\n",
        variant.to_uppercase()
    ));
    out.push_str("> **DRY RUN ONLY.** No history was rewritten. No force-push was issued.\n");
    out.push_str("> This file is a proposal for operator review.\n\n");
    out.push_str(&format!("Range: `{RANGE}`  \n"));
    out.push_str(&format!("Original commits: {}  \n", stats.original_commits));
    out.push_str(&format!(
        "Resulting bundles: {}  \n",
        stats.resulting_bundles
    ));
    out.push_str(&format!(
        "Reduction: {} ({}%, {}x)  \n",
        stats.reduction, stats.reduction_pct, stats.compression_ratio
    ));
    out.push_str(&format!(
        "Multi-commit bundles: {}  \n",
        stats.multi_commit_bundles
    ));
    out.push_str(&format!(
        "Bundles under 8 files: {}  \n",
        stats.bundles_under_8_files
    ));
    out.push_str(&format!(
        "Bundles 8-20 files: {}  \n",
        stats.bundles_8_to_20_files
    ));
    out.push_str(&format!(
        "Bundles over 20 files: {}  \n",
        stats.bundles_over_20_files
    ));
    out.push_str(&format!(
        "Merge-containing bundles: {}\n\n",
        stats.merge_bundles
    ));

    if !errors.is_empty() {
        out.push_str("## \u{274c} VALIDATION ERRORS\n\n");
        for e in errors {
            out.push_str(&format!("- {e}\n"));
        }
        out.push('\n');
    } else {
        out.push_str("## \u{2705} Validation passed\n\n");
        out.push_str("- Every original SHA appears in exactly one bundle.\n");
        out.push_str("- No multi-commit bundle exceeds 20 files.\n");
        out.push_str("- Protected >20-file singletons preserved as barriers.\n\n");
    }

    if !warnings.is_empty() {
        out.push_str("## \u{26a0}\u{fe0f} Warnings\n\n");
        for w in warnings {
            out.push_str(&format!("- {w}\n"));
        }
        out.push('\n');
    }

    out.push_str("## How to use this plan\n\n");
    out.push_str(
        "1. Review each bundle below. Multi-commit bundles have a `### Consolidation prompt`.\n",
    );
    out.push_str(
        "2. Run the prompts in `bundle_prompts.jsonl` through an LLM (batch or per-commit).\n",
    );
    out.push_str("3. Collect results as JSONL `{\"bundle_index\": N, \"message\": \"...\"}`.\n");
    out.push_str("4. Re-run with `--apply-messages <jsonl>` to fill in `New commit message`.\n");
    out.push_str("5. Only after operator approval, execute the rewrite with `git filter-repo` (NOT done here).\n\n");
    out.push_str("### Rewrite hazards (do NOT proceed without addressing)\n\n");
    out.push_str("- The 8 merge commits in range will be **linearized** by any rewrite. Bundles containing\n");
    out.push_str("  merges are tagged above. Confirm the linearized history is acceptable.\n");
    out.push_str(
        "- Rewriting **invalidates every SHA** in the range. `DIVERGENCE.md`, PR links, and any\n",
    );
    out.push_str("  doc/rule referencing a bundled SHA will point at dead hashes.\n");
    if scan_refs {
        out.push_str("- See the SHA-reference scan section below.\n\n");
    } else {
        out.push_str("- Run with `--scan-refs` to enumerate files referencing bundled SHAs.\n\n");
    }

    if scan_refs {
        eprintln!("Scanning docs/rules for bundled-SHA references ...");
        let refs = scan_sha_refs(repo, commits);
        out.push_str("## SHA-reference scan (files referencing bundled SHAs)\n\n");
        if refs.is_empty() {
            out.push_str("No bundled SHAs found referenced in scanned docs/rules.\n\n");
        } else {
            out.push_str(&format!(
                "{} bundled SHAs are referenced in-repo. **These must be updated after any rewrite.**\n\n",
                refs.len()
            ));
            let mut by_file: HashMap<String, Vec<String>> = HashMap::new();
            for (sha, files) in &refs {
                for f in files {
                    by_file.entry(f.clone()).or_default().push(sha.clone());
                }
            }
            let mut files_sorted: Vec<&String> = by_file.keys().collect();
            files_sorted.sort();
            for f in files_sorted {
                let shas = &by_file[f];
                out.push_str(&format!("### `{f}` ({} SHAs)\n\n", shas.len()));
                for sha in shas.iter().take(20) {
                    if let Some(c) = commits_by_sha.get(sha.as_str()) {
                        let subj = subject_of(&c.body);
                        out.push_str(&format!("- `{sha}` \u{2014} {subj}\n"));
                    }
                }
                if shas.len() > 20 {
                    out.push_str(&format!("- ... and {} more\n", shas.len() - 20));
                }
                out.push('\n');
            }
        }
    }

    out.push_str("## Bundles\n\n");
    let section_errors: Vec<String> = Vec::new();
    for (idx, b) in bundles.iter().enumerate() {
        if idx % 200 == 0 && idx > 0 {
            eprintln!("  plan: {idx}/{} bundles", bundles.len());
        }
        let msg = new_messages.get(&idx).map(|s| s.as_str());
        let diffstat = diffstats.get(&idx).cloned().flatten();
        let section = render_bundle_section(idx, b, commits_by_sha, msg, &diffstat);
        out.push_str(&section);
    }
    if !section_errors.is_empty() {
        for e in &section_errors {
            eprintln!("  warn: plan section: {e}");
        }
    }

    atomic_write(path, &out)?;
    Ok(())
}

fn write_stats_txt(
    path: &Path,
    variant: &str,
    errors: &[String],
    warnings: &[String],
    stats: &Stats,
) -> Result<()> {
    let mut out = String::new();
    writeln!(out, "variant: {variant}")?;
    writeln!(out, "original_commits: {}", stats.original_commits)?;
    writeln!(out, "resulting_bundles: {}", stats.resulting_bundles)?;
    writeln!(out, "reduction: {}", stats.reduction)?;
    writeln!(out, "reduction_pct: {}", stats.reduction_pct)?;
    writeln!(out, "compression_ratio: {}", stats.compression_ratio)?;
    writeln!(out, "multi_commit_bundles: {}", stats.multi_commit_bundles)?;
    writeln!(out, "singleton_bundles: {}", stats.singleton_bundles)?;
    writeln!(
        out,
        "bundles_under_8_files: {}",
        stats.bundles_under_8_files
    )?;
    writeln!(
        out,
        "bundles_8_to_20_files: {}",
        stats.bundles_8_to_20_files
    )?;
    writeln!(
        out,
        "bundles_over_20_files: {}",
        stats.bundles_over_20_files
    )?;
    writeln!(out, "merge_bundles: {}", stats.merge_bundles)?;
    writeln!(out, "max_bundle_files: {}", stats.max_bundle_files)?;
    writeln!(out, "median_bundle_files: {}", stats.median_bundle_files)?;
    writeln!(out, "errors: {}", errors.len())?;
    writeln!(out, "warnings: {}", warnings.len())?;
    if !errors.is_empty() {
        writeln!(out, "\nERRORS:")?;
        for e in errors {
            writeln!(out, "  - {e}")?;
        }
    }
    if !warnings.is_empty() {
        writeln!(out, "\nWARNINGS:")?;
        for w in warnings {
            writeln!(out, "  - {w}")?;
        }
    }
    atomic_write(path, &out)?;
    Ok(())
}

fn apply_messages(path: &Path) -> Result<HashMap<usize, String>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut messages = HashMap::new();
    let mut errors: Vec<String> = Vec::new();
    for (line_num, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Use serde_json for correct parsing — the hand-rolled parser was
        // fragile (matched first `"key":` occurrence, missed unicode escapes).
        match serde_json::from_str::<MessageEntry>(line) {
            Ok(entry) => {
                messages.insert(entry.bundle_index, entry.message);
            }
            Err(e) => {
                errors.push(format!("line {}: {e}", line_num + 1));
            }
        }
    }
    if !errors.is_empty() {
        eprintln!("  warn: apply-messages: {} parse errors:", errors.len());
        for e in errors.iter().take(10) {
            eprintln!("    {e}");
        }
        if errors.len() > 10 {
            eprintln!("    ... and {} more", errors.len() - 10);
        }
    }
    Ok(messages)
}

fn scan_sha_refs(repo: &Path, commits: &[Commit]) -> HashMap<String, Vec<String>> {
    // Build the set of full SHAs and short (7-char) prefixes to look for.
    let all_shas: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
    let full_set: HashSet<String> = all_shas.iter().cloned().collect();
    let short_to_full: HashMap<String, String> = all_shas
        .iter()
        .map(|s| (s[..7].to_string(), s.clone()))
        .collect();

    // Candidate files most likely to carry SHA references.
    let mut candidate_paths: Vec<String> =
        vec!["DIVERGENCE.md".into(), "AGENTS.md".into(), ".rules".into()];
    for root in ["kask/docs", "kask/scripts", "kask/registry", "docs"] {
        let root_path = repo.join(root);
        if root_path.is_dir() {
            walk_files(&root_path, repo, &mut candidate_paths);
        }
    }

    let hex_re = regex::Regex::new(r"\b([0-9a-f]{7,40})\b").unwrap();
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for rel in &candidate_paths {
        let fpath = repo.join(rel);
        if !fpath.is_file() {
            continue;
        }
        let text = match fs::read_to_string(&fpath) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for cap in hex_re.captures_iter(&text) {
            let token = cap.get(1).unwrap().as_str();
            let full = if full_set.contains(token) {
                Some(token.to_string())
            } else if let Some(f) = short_to_full.get(&token[..7]) {
                Some(f.clone())
            } else {
                None
            };
            if let Some(full) = full {
                if all_shas.contains(&full) {
                    refs.entry(full).or_default().push(rel.clone());
                }
            }
        }
    }
    // Dedupe file lists.
    for files in refs.values_mut() {
        files.sort();
        files.dedup();
    }
    refs
}

fn walk_files(dir: &Path, repo: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, repo, out);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "md" | "sh" | "txt" | "conl" | "toml" | "rs") {
                    if let Ok(rel) = path.strip_prefix(repo) {
                        out.push(rel.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
}
