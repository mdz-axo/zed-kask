//! The contrast-u instrument leg (the replication's one required harness
//! addition — amendment slot 7's designated small leg, the frozen form of
//! REPLICATION-DESIGN.md §3; NOT convert's direction-input mode).
//!
//! Reads the two contrast capture sets — the failing arm (config-A
//! hit_limit trajectories) and the progressing arm (config-A complete
//! trajectories), each a `capture` out-dir holding manifest.jsonl — and
//! per edit layer L computes:
//!
//!   * u_L = mean(ffn_out | looping, L) − mean(ffn_out | progressing, L),
//!     pooled over the arm's step-phase capture rows, unit-norm, written
//!     `u-{L}.f32` (LE f32 — the edit spec's DirectionSpec::File format).
//!     [Provenance: the paper's progressing-vs-looping contrast
//!     (S1-pinned) in the CAA/ITI/RepE contrastive mean-difference family
//!     (Rimsky et al., doi:10.18653/v1/2024.acl-long.828;
//!     arXiv:2306.03341; arXiv:2310.01405); unit-norm + the swept dose =
//!     program construct, labeled.]
//!   * the ffn_swiglu per-column contrast separation
//!     sep_L(j) = |mean_looping(j) − mean_progressing(j)| ranked descending
//!     (ties by column index — deterministic), written `ranked-{L}.txt`
//!     (one column index per line, most separated first — convert's
//!     columns path slices its top `count` into the sha-bound
//!     columns.txt). [Provenance: the paper's 64-column rule (S1-pinned);
//!     the ranking statistic is this program's instrument proxy,
//!     labeled.]
//!   * the S-mass margin: sumWhere(S, x) — the signed S-restricted sum
//!     (the T6 construct, kask/lean/embed_to_adapter.lean, commit
//!     52a93ab02d) per pooled step-phase vector of the PROGRESSING arm's
//!     swiglu captures at the selected S = the top `top` columns —
//!     measured and recorded in the receipt BEFORE any edit: the
//!     orthogonality margin is measured, not hoped.
//!
//! The step-phase pooling follows the E2-established dual-site per-step
//! capture pattern (the design doc §2: "per-step captures through each
//! trajectory") — the prompt phase is excluded from every pooled arm.
//! Engine-free: pure f32/f64 math over the capture files, like
//! convert's math section.

use crate::gguf_writer;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The contrast leg's CLI surface (main.rs constructs it; the defaults are
/// the frozen form's: layers 15, 19; top 64).
pub struct ContrastArgs {
    /// the failing arm's capture out-dir (its manifest.jsonl)
    pub failing_dir: PathBuf,
    /// the progressing arm's capture out-dir
    pub progressing_dir: PathBuf,
    /// the receipt/artifact out-dir
    pub out_dir: PathBuf,
    /// the edit layers (u/ranking/margin computed per layer)
    pub layers: Vec<u32>,
    /// |S| — the top columns of the ranking (the paper's 64)
    pub top: usize,
}

/// One capture-manifest row — the schema of the capture sets this leg
/// reads (produced outside this crate).
#[derive(Debug, Deserialize)]
struct CaptureRow {
    problem_id: String,
    site: String,
    layer: u32,
    rows: usize,
    row_elements: usize,
    file: String,
}

/// The direction site (the MLP's direct write — amendment slot 5's default)
/// and the ranking site (ffn_down's input space).
const DIRECTION_SITE: &str = "ffn_out";
const RANKING_SITE: &str = "ffn_swiglu";
/// The pooled capture phase: the per-step captures through each trajectory
/// (the E2-established pattern); the prompt phase is excluded.
const POOL_PHASE: &str = "step";

fn read_manifest(dir: &Path) -> Result<Vec<CaptureRow>> {
    let path = dir.join("manifest.jsonl");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read capture manifest {}", path.display()))?;
    let mut rows = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(
            serde_json::from_str(line)
                .with_context(|| format!("parse capture manifest {} row", path.display()))?,
        );
    }
    anyhow::ensure!(
        !rows.is_empty(),
        "empty capture manifest {}",
        path.display()
    );
    Ok(rows)
}

/// The capture file's phase from the capture writer's own naming
/// (`{problem_id}.{phase}.{step}.{site}-{layer}.f32`, parsed from the
/// right so any id shape survives).
fn file_phase(file: &str) -> Option<&str> {
    file.rsplit('.').nth(3)
}

/// Walk one arm's step-phase capture rows at (site, layer), visiting each
/// pooled vector; returns (width, vectors-visited). The reconciliation
/// refusals fire here so the pooling and the margin share them: a missing
/// capture file, a byte count that disagrees with the manifest's
/// rows × row_elements, or a row_elements disagreement within the arm.
fn walk_step_vectors(
    rows: &[CaptureRow],
    dir: &Path,
    site: &str,
    layer: u32,
    visit: &mut dyn FnMut(&[f32]),
) -> Result<(usize, usize)> {
    let at: Vec<&CaptureRow> = rows
        .iter()
        .filter(|r| r.site == site && r.layer == layer)
        .collect();
    anyhow::ensure!(
        !at.is_empty(),
        "no {site}-{layer} captures in {} — the capture arms do not cover the frozen sites",
        dir.display()
    );
    let width = at[0].row_elements;
    anyhow::ensure!(
        width > 0,
        "{site}-{layer} in {}: zero row_elements",
        dir.display()
    );
    for r in &at {
        anyhow::ensure!(
            r.row_elements == width,
            "{site}-{layer} in {}: row_elements {} disagrees with {} — the capture manifests do not reconcile",
            dir.display(),
            r.row_elements,
            width
        );
    }
    let mut buf: Vec<f32> = Vec::new();
    let mut n = 0usize;
    let mut seen_step = false;
    for r in &at {
        if file_phase(&r.file) != Some(POOL_PHASE) {
            continue;
        }
        let path = dir.join(&r.file);
        let bytes =
            std::fs::read(&path).with_context(|| format!("read capture {}", path.display()))?;
        anyhow::ensure!(
            bytes.len() == r.rows * r.row_elements * 4,
            "capture {} holds {} bytes; the manifest declares {}x{} f32s ({} bytes) — the capture manifests do not reconcile",
            path.display(),
            bytes.len(),
            r.rows,
            r.row_elements,
            r.rows * r.row_elements * 4
        );
        buf.clear();
        for off in (0..bytes.len()).step_by(4) {
            let b: [u8; 4] = bytes[off..off + 4].try_into().map_err(|_| {
                anyhow::anyhow!("capture {}: short f32 at byte {off}", path.display())
            })?;
            buf.push(f32::from_le_bytes(b));
        }
        for v in buf.chunks(width) {
            visit(v);
            n += 1;
        }
        seen_step = true;
    }
    anyhow::ensure!(
        seen_step,
        "no {POOL_PHASE}-phase {site}-{layer} captures in {} — only the prompt phase is present",
        dir.display()
    );
    Ok((width, n))
}

/// One pooled arm at (site, layer): the f64 column sums over every
/// step-phase capture row and the pooled vector count (the mean is
/// sum/vectors — the caller divides).
struct PooledArm {
    sum: Vec<f64>,
    vectors: usize,
    width: usize,
}

fn pool_arm(rows: &[CaptureRow], dir: &Path, site: &str, layer: u32) -> Result<PooledArm> {
    let mut sum: Vec<f64> = Vec::new();
    let (width, vectors) = walk_step_vectors(rows, dir, site, layer, &mut |v| {
        if sum.is_empty() {
            sum.resize(v.len(), 0.0);
        }
        for (j, x) in v.iter().enumerate() {
            sum[j] += f64::from(*x);
        }
    })?;
    Ok(PooledArm {
        sum,
        vectors,
        width,
    })
}

/// The S-mass margin (the T6 construct, measured): per pooled step-phase
/// vector x of the given arm's swiglu captures, sumWhere(S, x) =
/// Σ_{j∈S} x_j (signed — the indicator trigger's injection scalar is
/// c·sumWhere). The context masses (the per-vector total absolute mass
/// and the S-restricted absolute mass) ride along so the margin's scale
/// is readable from the receipt alone.
struct MarginStats {
    n: usize,
    sum_mean: f64,
    sum_std: f64,
    sum_min: f64,
    sum_max: f64,
    abs_sum_mean: f64,
    abs_sum_max: f64,
    mean_abs_total: f64,
    mean_s_abs: f64,
}

fn s_mass_margin(
    rows: &[CaptureRow],
    dir: &Path,
    site: &str,
    layer: u32,
    s: &[usize],
) -> Result<MarginStats> {
    anyhow::ensure!(!s.is_empty(), "the margin needs a non-empty S");
    let mut n = 0usize;
    let mut sum_x = 0f64;
    let mut sum_x2 = 0f64;
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut abs_sum = 0f64;
    let mut abs_max = 0f64;
    let mut abs_total_sum = 0f64;
    let mut s_abs_sum = 0f64;
    walk_step_vectors(rows, dir, site, layer, &mut |v| {
        let mut sw = 0f64;
        let mut s_abs = 0f64;
        for &j in s {
            sw += f64::from(v[j]);
            s_abs += f64::from(v[j].abs());
        }
        let abs_total: f64 = v.iter().map(|&x| f64::from(x.abs())).sum();
        sum_x += sw;
        sum_x2 += sw * sw;
        min = min.min(sw);
        max = max.max(sw);
        let a = sw.abs();
        abs_sum += a;
        abs_max = abs_max.max(a);
        abs_total_sum += abs_total;
        s_abs_sum += s_abs;
        n += 1;
    })?;
    anyhow::ensure!(n > 0, "no vectors pooled for the margin at {site}-{layer}");
    let mean = sum_x / n as f64;
    let var = (sum_x2 / n as f64 - mean * mean).max(0.0);
    Ok(MarginStats {
        n,
        sum_mean: mean,
        sum_std: var.sqrt(),
        sum_min: min,
        sum_max: max,
        abs_sum_mean: abs_sum / n as f64,
        abs_sum_max: abs_max,
        mean_abs_total: abs_total_sum / n as f64,
        mean_s_abs: s_abs_sum / n as f64,
    })
}

/// One arm's receipt record: its problems (the manifest's distinct ids,
/// sorted), each capture-window label sidecar when present, and the
/// sites/layers seen — deviations visible, never silent.
fn arm_record(dir: &Path, rows: &[CaptureRow]) -> serde_json::Value {
    let mut problems: Vec<String> = rows.iter().map(|r| r.problem_id.clone()).collect();
    problems.sort();
    problems.dedup();
    let mut labels = serde_json::Map::new();
    for p in &problems {
        if let Ok(text) = std::fs::read_to_string(dir.join(format!("{p}.label.txt"))) {
            labels.insert(
                p.clone(),
                serde_json::Value::String(text.trim().to_string()),
            );
        }
    }
    let mut sites: Vec<&str> = rows.iter().map(|r| r.site.as_str()).collect();
    sites.sort_unstable();
    sites.dedup();
    let mut layers: Vec<u32> = rows.iter().map(|r| r.layer).collect();
    layers.sort_unstable();
    layers.dedup();
    serde_json::json!({
        "problems": problems,
        "labels": labels,
        "sites_seen": sites,
        "layers_seen": layers,
        "capture_rows": rows.len(),
    })
}

pub fn run(args: ContrastArgs) -> Result<()> {
    anyhow::ensure!(!args.layers.is_empty(), "no layers to contrast");
    anyhow::ensure!(args.top > 0, "top must be > 0");
    std::fs::create_dir_all(&args.out_dir)?;
    let out = args.out_dir.canonicalize()?;
    let hash = |p: &Path| gguf_writer::sha256_file(p).ok();

    let failing_rows = read_manifest(&args.failing_dir)?;
    let progressing_rows = read_manifest(&args.progressing_dir)?;

    let mut per_layer: Vec<serde_json::Value> = Vec::new();
    for &l in &args.layers {
        // u_L — the contrastive mean difference, unit-norm
        let f_dir = pool_arm(&failing_rows, &args.failing_dir, DIRECTION_SITE, l)?;
        let p_dir = pool_arm(&progressing_rows, &args.progressing_dir, DIRECTION_SITE, l)?;
        anyhow::ensure!(
            f_dir.width == p_dir.width,
            "layer {l}: the arms' {DIRECTION_SITE} widths disagree ({} vs {}) — the capture manifests do not reconcile",
            f_dir.width,
            p_dir.width
        );
        let f_mean: Vec<f64> = f_dir
            .sum
            .iter()
            .map(|&x| x / f_dir.vectors as f64)
            .collect();
        let p_mean: Vec<f64> = p_dir
            .sum
            .iter()
            .map(|&x| x / p_dir.vectors as f64)
            .collect();
        let u_raw: Vec<f64> = f_mean.iter().zip(&p_mean).map(|(a, b)| a - b).collect();
        anyhow::ensure!(
            u_raw.iter().all(|x| x.is_finite()),
            "layer {l}: the pooled {DIRECTION_SITE} means contain non-finite values"
        );
        let raw_l2 = u_raw.iter().map(|x| x * x).sum::<f64>().sqrt();
        anyhow::ensure!(
            raw_l2 > 0.0,
            "layer {l}: the contrast direction is zero — the arms' pooled {DIRECTION_SITE} \
             means are identical; the contrast found no direction to subtract"
        );
        let mut u: Vec<f32> = u_raw.iter().map(|&x| x as f32).collect();
        let norm: f64 = u.iter().map(|&x| f64::from(x * x)).sum::<f64>().sqrt();
        anyhow::ensure!(
            norm > 0.0 && norm.is_finite(),
            "layer {l}: the contrast direction underflows to zero in f32"
        );
        for x in &mut u {
            *x /= norm as f32;
        }
        let u_path = out.join(format!("u-{l}.f32"));
        let mut u_bytes = Vec::with_capacity(u.len() * 4);
        for x in &u {
            u_bytes.extend_from_slice(&x.to_le_bytes());
        }
        std::fs::write(&u_path, &u_bytes)?;

        // the swiglu contrast separation ranking
        let f_sw = pool_arm(&failing_rows, &args.failing_dir, RANKING_SITE, l)?;
        let p_sw = pool_arm(&progressing_rows, &args.progressing_dir, RANKING_SITE, l)?;
        anyhow::ensure!(
            f_sw.width == p_sw.width,
            "layer {l}: the arms' {RANKING_SITE} widths disagree ({} vs {}) — the capture manifests do not reconcile",
            f_sw.width,
            p_sw.width
        );
        let f_sw_mean: Vec<f64> = f_sw.sum.iter().map(|&x| x / f_sw.vectors as f64).collect();
        let p_sw_mean: Vec<f64> = p_sw.sum.iter().map(|&x| x / p_sw.vectors as f64).collect();
        let sep: Vec<f64> = f_sw_mean
            .iter()
            .zip(&p_sw_mean)
            .map(|(a, b)| (a - b).abs())
            .collect();
        anyhow::ensure!(
            sep.iter().all(|x| x.is_finite()),
            "layer {l}: the pooled {RANKING_SITE} means contain non-finite values"
        );
        anyhow::ensure!(
            args.top <= sep.len(),
            "layer {l}: top {} exceeds the {RANKING_SITE} dim {}",
            args.top,
            sep.len()
        );
        let mut order: Vec<usize> = (0..sep.len()).collect();
        order.sort_by(|&a, &b| sep[b].total_cmp(&sep[a]).then(a.cmp(&b)));
        anyhow::ensure!(
            sep[order[0]] > 0.0,
            "layer {l}: all {RANKING_SITE} separations are zero — the ranking would be arbitrary"
        );
        let s: Vec<usize> = order.iter().copied().take(args.top).collect();
        let ranked_body = order
            .iter()
            .map(|j| j.to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let ranked_path = out.join(format!("ranked-{l}.txt"));
        std::fs::write(&ranked_path, ranked_body)?;

        // the S-mass margin (T6): measured over the progressing arm's
        // swiglu captures at the selected S, recorded before any edit
        let m = s_mass_margin(
            &progressing_rows,
            &args.progressing_dir,
            RANKING_SITE,
            l,
            &s,
        )?;

        let u_l2: f64 = u.iter().map(|&x| f64::from(x * x)).sum::<f64>().sqrt();
        let (umin, umax) = (
            u.iter().copied().fold(f32::INFINITY, f32::min),
            u.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        );
        let top_scores: Vec<f64> = s.iter().map(|&j| sep[j]).collect();

        per_layer.push(serde_json::json!({
            "layer": l,
            "pooled_vectors": {
                "ffn_out": {"failing": f_dir.vectors, "progressing": p_dir.vectors},
                "ffn_swiglu": {"failing": f_sw.vectors, "progressing": p_sw.vectors},
            },
            "dims": {"hidden": f_dir.width, "swiglu": f_sw.width},
            "u": {
                "path": u_path.display().to_string(),
                "sha256": hash(&u_path),
                "dim": u.len(),
                "l2": u_l2,
                "min": umin,
                "max": umax,
                "raw_l2_before_norm": raw_l2,
            },
            "ranked": {
                "path": ranked_path.display().to_string(),
                "sha256": hash(&ranked_path),
                "rows": order.len(),
            },
            "separation": {"top_scores": top_scores, "count": s.len()},
            "s": {"count": s.len(), "columns": s},
            "s_mass_margin": {
                "n_vectors": m.n,
                "sum_where": {
                    "mean": m.sum_mean, "std": m.sum_std,
                    "min": m.sum_min, "max": m.sum_max,
                },
                "abs_sum_where": {"mean": m.abs_sum_mean, "max": m.abs_sum_max},
                "context": {
                    "mean_abs_total": m.mean_abs_total,
                    "mean_s_abs_mass": m.mean_s_abs,
                },
                "construct": "sumWhere(S, x) = Σ_{j∈S} x_j — the T6 margin (kask/lean/embed_to_adapter.lean, commit 52a93ab02d), measured over the progressing arm's step-phase swiglu captures before any edit",
            },
        }));
        eprintln!(
            "contrast: layer {l} u {}-dim l2 {u_l2:.4}, S = top {} of {} (sep {:.4}..{:.4}), \
             margin sumWhere mean {:+.4} (|mean| {:.4}) of abs-mass {:.4} over {} vectors",
            u.len(),
            s.len(),
            order.len(),
            top_scores[0],
            top_scores[top_scores.len() - 1],
            m.sum_mean,
            m.abs_sum_mean,
            m.mean_abs_total,
            m.n
        );
    }

    let manifest_f = args.failing_dir.join("manifest.jsonl");
    let manifest_p = args.progressing_dir.join("manifest.jsonl");
    let receipt = serde_json::json!({
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        "harness": format!("dme-harness-rust/{} contrast", env!("CARGO_PKG_VERSION")),
        "inputs": {
            "failing_dir": args.failing_dir.display().to_string(),
            "progressing_dir": args.progressing_dir.display().to_string(),
            "failing_manifest": {"path": manifest_f.display().to_string(), "sha256": hash(&manifest_f)},
            "progressing_manifest": {"path": manifest_p.display().to_string(), "sha256": hash(&manifest_p)},
        },
        "config": {
            "layers": args.layers,
            "top": args.top,
            "pool_phase": POOL_PHASE,
            "direction_site": DIRECTION_SITE,
            "ranking_site": RANKING_SITE,
        },
        "arms": {
            "failing": arm_record(&args.failing_dir, &failing_rows),
            "progressing": arm_record(&args.progressing_dir, &progressing_rows),
        },
        "per_layer": per_layer,
        "provenance": {
            "localization": "the paper's progressing-vs-looping contrast (S1-pinned) + the CAA/ITI/RepE contrastive mean-difference family (doi:10.18653/v1/2024.acl-long.828; arXiv:2306.03341; arXiv:2310.01405); unit-norm + the swept dose = program construct (labeled)",
            "column_rule": "the paper's 64-column rule (S1-pinned); the ranking statistic is this program's instrument proxy (labeled)",
            "s_mass_margin": "S3's T6 corollary (kask/lean/embed_to_adapter.lean, commit 52a93ab02d; lean_check passed)",
            "design": "REPLICATION-DESIGN.md §3, the frozen form (corrective-phase digest 0b33f87d…)",
        },
        "limitations": "u and the ranking are instrument proxies for the paper's logs' exact selection; the margin is measured over the capture arms' rows within the loop-regime window, not the full-cap trajectories",
    });
    let receipt_path = out.join("contrast-receipt.json");
    std::fs::write(&receipt_path, serde_json::to_string_pretty(&receipt)?)?;
    eprintln!("contrast: receipt {}", receipt_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// One capture in the writer's own naming, appended to the dir's
    /// manifest.jsonl (single-row captures — the per-step file shape).
    fn write_capture(
        dir: &Path,
        problem: &str,
        phase: &str,
        step: u32,
        site: &str,
        layer: u32,
        vecs: &[f32],
    ) {
        let file = format!("{problem}.{phase}.{step}.{site}-{layer}.f32");
        let mut bytes = Vec::with_capacity(vecs.len() * 4);
        for v in vecs {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(dir.join(&file), &bytes).unwrap();
        let row = serde_json::json!({
            "ts": 0.0, "problem_id": problem, "site": site, "layer": layer,
            "rows": 1, "row_elements": vecs.len(), "file": file, "harness": "fixture",
        });
        use std::io::Write as _;
        let mut m = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("manifest.jsonl"))
            .unwrap();
        writeln!(m, "{row}").unwrap();
    }

    /// Hand-checkable fixture arms. failing: ffn_out [2,0,0,0]/[0,2,0,0] →
    /// mean [1,1,0,0]; swiglu [4,0,…]/[2,0,…] → mean [3,0,0,0,0,0].
    /// progressing: ffn_out [0,0,2,0]/[0,0,0,2] → mean [0,0,1,1]; swiglu
    /// [0,1,0,0,0,0] both → mean [0,1,0,0,0,0]. A prompt capture with
    /// wild values asserts the step-phase pooling.
    fn fixture_arms(tag: &str) -> (TempDir, TempDir) {
        let failing = TempDir::new(&format!("{tag}-fail"));
        let progressing = TempDir::new(&format!("{tag}-prog"));
        for p in ["p1", "p2"] {
            write_capture(
                failing.path(),
                p,
                "step",
                1,
                "ffn_out",
                15,
                &[2.0, 0.0, 0.0, 0.0],
            );
            write_capture(
                failing.path(),
                p,
                "step",
                2,
                "ffn_out",
                15,
                &[0.0, 2.0, 0.0, 0.0],
            );
            write_capture(
                failing.path(),
                p,
                "step",
                1,
                "ffn_swiglu",
                15,
                &[4.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            );
            write_capture(
                failing.path(),
                p,
                "step",
                2,
                "ffn_swiglu",
                15,
                &[2.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            );
        }
        // the prompt phase: wild values that must NOT enter any pooled arm
        write_capture(failing.path(), "p1", "prompt", 0, "ffn_out", 15, &[99.0; 4]);
        write_capture(
            failing.path(),
            "p1",
            "prompt",
            0,
            "ffn_swiglu",
            15,
            &[99.0; 6],
        );
        for p in ["p3", "p4"] {
            write_capture(
                progressing.path(),
                p,
                "step",
                1,
                "ffn_out",
                15,
                &[0.0, 0.0, 2.0, 0.0],
            );
            write_capture(
                progressing.path(),
                p,
                "step",
                2,
                "ffn_out",
                15,
                &[0.0, 0.0, 0.0, 2.0],
            );
            // a mean-preserving third step: unequal arm counts make the u
            // assertion mean-difference-sharp (a sum would NOT cancel)
            write_capture(
                progressing.path(),
                p,
                "step",
                3,
                "ffn_out",
                15,
                &[0.0, 0.0, 1.0, 1.0],
            );
            write_capture(
                progressing.path(),
                p,
                "step",
                1,
                "ffn_swiglu",
                15,
                &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            );
            write_capture(
                progressing.path(),
                p,
                "step",
                2,
                "ffn_swiglu",
                15,
                &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            );
            write_capture(
                progressing.path(),
                p,
                "step",
                3,
                "ffn_swiglu",
                15,
                &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
            );
        }
        // label sidecars (one per problem in a capture set)
        std::fs::write(failing.path().join("p1.label.txt"), "hit_limit").unwrap();
        std::fs::write(progressing.path().join("p3.label.txt"), "correct_complete").unwrap();
        (failing, progressing)
    }

    fn run_leg(
        failing: &Path,
        progressing: &Path,
        out: &Path,
        top: usize,
    ) -> Result<serde_json::Value> {
        super::run(ContrastArgs {
            failing_dir: failing.to_path_buf(),
            progressing_dir: progressing.to_path_buf(),
            out_dir: out.to_path_buf(),
            layers: vec![15],
            top,
        })?;
        let receipt: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("contrast-receipt.json")).unwrap(),
        )
        .unwrap();
        Ok(receipt)
    }

    #[test]
    fn contrast_math_on_constructed_captures() {
        let (failing, progressing) = fixture_arms("math");
        let out = TempDir::new("math-out");
        let receipt =
            run_leg(failing.path(), progressing.path(), out.path(), 2).expect("the leg runs");

        // u: [1,1,0,0] − [0,0,1,1] = [1,1,−1,−1], unit-norm → ±0.5
        let u_bytes = std::fs::read(out.path().join("u-15.f32")).unwrap();
        let u: Vec<f32> = u_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        assert_eq!(u, vec![0.5, 0.5, -0.5, -0.5]);
        // the prompt phase's 99s never entered
        let l2: f64 = u.iter().map(|&x| f64::from(x * x)).sum::<f64>().sqrt();
        assert!((l2 - 1.0).abs() < 1e-6);

        // the ranking: sep = [3,1,0,0,0,0] → indices 0..5 (ties by index)
        let ranked = std::fs::read_to_string(out.path().join("ranked-15.txt")).unwrap();
        assert_eq!(ranked, "0\n1\n2\n3\n4\n5\n");
        let pl = &receipt["per_layer"][0];
        assert_eq!(pl["s"]["columns"], serde_json::json!([0, 1]));
        assert_eq!(
            pl["separation"]["top_scores"],
            serde_json::json!([3.0, 1.0])
        );
        assert_eq!(
            pl["pooled_vectors"]["ffn_out"],
            serde_json::json!({"failing": 4, "progressing": 6})
        );
        assert_eq!(
            pl["pooled_vectors"]["ffn_swiglu"],
            serde_json::json!({"failing": 4, "progressing": 6})
        );
        assert_eq!(pl["dims"], serde_json::json!({"hidden": 4, "swiglu": 6}));

        // the margin: every progressing swiglu vector is [0,1,0,0,0,0] →
        // sumWhere(S={0,1}, x) = 1 per vector
        let m = &pl["s_mass_margin"];
        assert_eq!(m["n_vectors"], 6);
        assert!((m["sum_where"]["mean"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!((m["sum_where"]["min"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!((m["sum_where"]["max"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!(m["sum_where"]["std"].as_f64().unwrap().abs() < 1e-9);
        assert!((m["context"]["mean_abs_total"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert!((m["context"]["mean_s_abs_mass"].as_f64().unwrap() - 1.0).abs() < 1e-9);

        // the arms record: the label sidecars surface
        assert_eq!(receipt["arms"]["failing"]["labels"]["p1"], "hit_limit");
        assert_eq!(
            receipt["arms"]["progressing"]["labels"]["p3"],
            "correct_complete"
        );
        // the u artifact is sha-bound in the receipt
        let u_sha = gguf_writer::sha256_file(&out.path().join("u-15.f32")).unwrap();
        assert_eq!(pl["u"]["sha256"].as_str().unwrap(), u_sha);
    }

    /// The leg's ranked output must feed the S2 columns path: convert
    /// slices the top `count`, and the edit leg's own loader accepts the
    /// sliced, sha-bound file (the real consumer, not a mirror).
    #[test]
    fn ranked_list_round_trips_through_the_columns_path() {
        let (failing, progressing) = fixture_arms("cols");
        let out = TempDir::new("cols-out");
        run_leg(failing.path(), progressing.path(), out.path(), 2).expect("the leg runs");
        let ranked = std::fs::read_to_string(out.path().join("ranked-15.txt")).unwrap();
        let top2: String = ranked.lines().take(2).map(|l| format!("{l}\n")).collect();
        let cols_path = out.path().join("columns.txt");
        std::fs::write(&cols_path, &top2).unwrap();
        let sha = gguf_writer::sha256_file(&cols_path).unwrap();
        let cols = crate::edit::read_columns_file(cols_path.to_str().unwrap(), &sha, 6, 2)
            .expect("the S2 columns loader accepts the leg's ranked output");
        assert_eq!(cols, vec![0, 1]);
    }

    #[test]
    fn refuses_asymmetric_arms() {
        let (failing, progressing) = fixture_arms("asym");
        // drop the progressing arm's swiglu rows from its manifest
        let raw = std::fs::read_to_string(progressing.path().join("manifest.jsonl")).unwrap();
        let kept: Vec<&str> = raw.lines().filter(|l| !l.contains("ffn_swiglu")).collect();
        std::fs::write(
            progressing.path().join("manifest.jsonl"),
            kept.join("\n") + "\n",
        )
        .unwrap();
        let out = TempDir::new("asym-out");
        let err = run_leg(failing.path(), progressing.path(), out.path(), 2).unwrap_err();
        assert!(
            err.to_string().contains("no ffn_swiglu-15 captures"),
            "wrong refusal: {err}"
        );
    }

    #[test]
    fn refuses_a_capture_byte_mismatch() {
        let (failing, progressing) = fixture_arms("bytes");
        let victim = failing.path().join("p1.step.1.ffn_out-15.f32");
        let full = std::fs::read(&victim).unwrap();
        std::fs::write(&victim, &full[..full.len() - 4]).unwrap();
        let out = TempDir::new("bytes-out");
        let err = run_leg(failing.path(), progressing.path(), out.path(), 2).unwrap_err();
        assert!(
            err.to_string().contains("do not reconcile"),
            "wrong refusal: {err}"
        );
    }

    #[test]
    fn refuses_a_zero_contrast_direction() {
        // identical arms → u = 0: the contrast found nothing
        let failing = TempDir::new("zc-fail");
        let progressing = TempDir::new("zc-prog");
        for dir in [failing.path(), progressing.path()] {
            write_capture(dir, "q1", "step", 1, "ffn_out", 15, &[1.0, 0.0, 0.0, 0.0]);
            write_capture(
                dir,
                "q1",
                "step",
                1,
                "ffn_swiglu",
                15,
                &[1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            );
        }
        let out = TempDir::new("zc-out");
        let err = run_leg(failing.path(), progressing.path(), out.path(), 2).unwrap_err();
        assert!(
            err.to_string().contains("contrast direction is zero"),
            "wrong refusal: {err}"
        );
    }

    #[test]
    fn refuses_an_all_zero_separation() {
        // ffn_out contrasts (u nonzero) but the swiglu arms are identical
        // → the ranking would be arbitrary
        let failing = TempDir::new("zs-fail");
        let progressing = TempDir::new("zs-prog");
        write_capture(
            failing.path(),
            "q1",
            "step",
            1,
            "ffn_out",
            15,
            &[1.0, 0.0, 0.0, 0.0],
        );
        write_capture(
            failing.path(),
            "q1",
            "step",
            1,
            "ffn_swiglu",
            15,
            &[1.0, 2.0, 0.0, 0.0, 0.0, 0.0],
        );
        write_capture(
            progressing.path(),
            "q1",
            "step",
            1,
            "ffn_out",
            15,
            &[0.0, 0.0, 1.0, 0.0],
        );
        write_capture(
            progressing.path(),
            "q1",
            "step",
            1,
            "ffn_swiglu",
            15,
            &[1.0, 2.0, 0.0, 0.0, 0.0, 0.0],
        );
        let out = TempDir::new("zs-out");
        let err = run_leg(failing.path(), progressing.path(), out.path(), 2).unwrap_err();
        assert!(
            err.to_string().contains("separations are zero"),
            "wrong refusal: {err}"
        );
    }

    #[test]
    fn refuses_top_exceeding_the_swiglu_dim() {
        let (failing, progressing) = fixture_arms("top7");
        let out = TempDir::new("top7-out");
        let err = run_leg(failing.path(), progressing.path(), out.path(), 7).unwrap_err();
        assert!(
            err.to_string().contains("exceeds the ffn_swiglu dim"),
            "wrong refusal: {err}"
        );
    }
}
