//! The converter (the embedding-to-edit bridge): pure math from an
//! embedder-vectors file plus the capture leg's manifest to the edit leg's
//! inputs — engine-free, GGUF-parse-free, like `edit` (dimensions come from
//! the config, sourced via `gguf-info`, and are cross-checked against the
//! captures; the edit leg re-checks them against the tensor).
//!
//! Math contract (kask/lean/embed_to_adapter.lean — implemented, not
//! re-derived): the ridge bridge phi: R^d -> R^h is the pinned linear
//! family (`bridge_linear`), fitted by the normal equations plus a
//! hand-rolled Cholesky on `Vec<f64>` — no new dependencies. The edit is
//! the rank-one action W <- W - alpha * phi(v) * k^T
//! (`rank_one_edit_action`): `u.f32` carries phi(v_target) and the edit
//! applies alpha at edit time, so the net direction is alpha * phi(v)
//! with r = 1 and s = alpha/r = alpha (`lora_runtime_semantics`).
//! Constant steering is not a linear edit (`no_linear_edit_is_constant`):
//! a constant fitted direction refuses loudly, pointing at the bias-edit
//! limitation (out of scope, recorded).
//!
//! Join keys: the capture manifest's `problem_id` (the manifest has no
//! label column — the trajectory labels land in per-problem
//! `{problem_id}.label.txt` sidecars because they are knowable only
//! post-generation; the manifest rows are appended mid-flight), so the
//! trigger's labeled-mean keys on the anchor ids the config names.

use crate::edit::{DirectionSpec, EditSpec, TriggerSpec, dme2_share};
use crate::gguf_writer;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct ConvertArgs {
    pub config: PathBuf,
    pub vectors: PathBuf,
    pub capture_manifest: PathBuf,
    pub capture_dir: PathBuf,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Train,
    HeldOut,
}

#[derive(Debug, Deserialize)]
pub struct ConvertConfig {
    /// embedder vector width d (one vector per anchor, in file row order)
    pub vectors_dim: usize,
    /// ordered like the vectors rows; `id` is the capture `problem_id`
    pub anchors: Vec<Anchor>,
    /// bridge fit site + layer (e.g. "l_out", 12)
    pub site: String,
    pub layer: u32,
    /// the edit layers and tensor stem carried into the EditSpec
    pub layers: Vec<u32>,
    pub tensor: String,
    pub alpha: f32,
    /// ridge lambda (>= 0; raise it when the anchor gram is singular)
    pub ridge_lambda: f64,
    pub target_anchor_id: String,
    pub trigger: TriggerConfig,
    /// dims sourced via `gguf-info`: hidden = the direction space (the
    /// tensor's ne1), trigger = the tensor's input dim (ne0)
    pub dims: DimExpect,
    /// the model's total weights (`gguf-info` params=), declared for the
    /// DME-2 projection (corrective phase S2): convert emits
    /// projected_weights_touched and projected_edited_share in the
    /// manifest — the instrument measures, the pre-emit gate decides —
    /// and the edit leg re-checks the share against the parsed GGUF's
    /// real param_count (belt and braces). Required: a config that
    /// cannot state the model's size cannot state its edit's share.
    pub total_weights: u64,
    /// the seeded random-direction control (E1)
    pub control: Option<ControlCfg>,
    pub input_gguf: String,
    pub output_gguf: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Anchor {
    pub id: String,
    pub split: Split,
}

#[derive(Debug, Deserialize)]
pub struct TriggerConfig {
    pub site: String,
    pub layer: u32,
    /// the trigger anchors — the labeled-mean set
    pub problem_ids: Vec<String>,
    /// the calibration set for the covariance diagonal;
    /// None = every other problem at the site
    pub calibration_problem_ids: Option<Vec<String>>,
    /// the explicit column-list trigger (corrective phase S2): when
    /// set, k = indicator(S) with S = the top `count` columns of the
    /// ranked list file — the paper-faithful mechanism (the anchor's 64
    /// output columns selected by failure involvement; provenance run
    /// 3d2c6f7b39ec213c pins the citation) — and the dense
    /// labeled-mean/covdiag construction is skipped for this emit.
    /// Unset: the dense path, unchanged.
    #[serde(default)]
    pub columns: Option<ColumnListCfg>,
}

/// The ranked-column-list source: one column index per line, most
/// involved first (the localization leg's ranking over the edit
/// tensor's input dim), sliced to `count` by the converter and written
/// out as the sha-bound columns file the edit leg loads.
#[derive(Debug, Deserialize)]
pub struct ColumnListCfg {
    pub path: String,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct DimExpect {
    pub hidden: usize,
    pub trigger: usize,
}

#[derive(Debug, Deserialize)]
pub struct ControlCfg {
    pub seed: u64,
}

/// One capture-manifest row (the capture leg's schema, mirrored for
/// reading — the P5-frozen capture writer stays untouched).
#[derive(Debug, Deserialize)]
struct CaptureRow {
    problem_id: String,
    site: String,
    layer: u32,
    rows: usize,
    row_elements: usize,
    file: String,
}

// ---------- the math (first principles: normal equations + Cholesky) ----------

/// Lower-triangular Cholesky factor of an SPD matrix, or None (not SPD).
pub(crate) fn cholesky(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut l = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = a[i][j];
            s -= l[i]
                .iter()
                .zip(&l[j])
                .take(j)
                .map(|(&x, &y)| x * y)
                .sum::<f64>();
            if i == j {
                if s <= 0.0 {
                    return None;
                }
                l[i][j] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

/// Solve L L^T x = b (forward then back substitution).
pub(crate) fn chol_solve(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let n = l.len();
    let mut y = vec![0.0f64; n];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i][k] * y[k];
        }
        y[i] = s / l[i][i];
    }
    let mut x = vec![0.0f64; n];
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in i + 1..n {
            s -= l[k][i] * x[k];
        }
        x[i] = s / l[i][i];
    }
    x
}

/// The ridge bridge: fit M (h x d) minimizing
/// sum_i ||M v_i - t_i||^2 + lambda * ||M||_F^2 — per output row the
/// normal equations (V^T V + lambda I) m_j = V^T t_j, one Cholesky factor
/// of the gram, h back-substitutions.
pub(crate) fn ridge_bridge(v: &[Vec<f64>], t: &[Vec<f64>], lambda: f64) -> Result<Vec<Vec<f64>>> {
    anyhow::ensure!(!v.is_empty(), "ridge: empty train set");
    anyhow::ensure!(lambda >= 0.0, "ridge: negative lambda {lambda}");
    let d = v[0].len();
    let h = t[0].len();
    let mut g = vec![vec![0.0f64; d]; d];
    for i in 0..d {
        for j in 0..d {
            g[i][j] = v.iter().map(|row| row[i] * row[j]).sum::<f64>();
        }
        g[i][i] += lambda;
    }
    let l = cholesky(&g).ok_or_else(|| {
        anyhow::anyhow!("ridge: the anchor gram is not positive definite — raise ridge_lambda")
    })?;
    let mut m = vec![vec![0.0f64; d]; h];
    for (j, mrow) in m.iter_mut().enumerate() {
        let mut b = vec![0.0f64; d];
        for i in 0..d {
            b[i] = v.iter().zip(t).map(|(vr, tr)| vr[i] * tr[j]).sum::<f64>();
        }
        *mrow = chol_solve(&l, &b);
    }
    Ok(m)
}

/// phi(v) = M v — the pinned linear bridge (`bridge_linear`).
pub(crate) fn apply_bridge(m: &[Vec<f64>], v: &[f64]) -> Vec<f64> {
    m.iter()
        .map(|row| row.iter().zip(v).map(|(&a, &b)| a * b).sum())
        .collect()
}

/// The seeded random control direction — xorshift64* (the documented
/// fallback generator family; problems.rs `declared_order` uses the same
/// shift triple), one unit vector per held-out anchor index.
pub(crate) fn xorshift_unit(seed: u64, i: usize, dim: usize) -> Vec<f64> {
    let mut s = seed.wrapping_add((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut v = vec![0.0f64; dim];
    for x in &mut v {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        let bits = s.wrapping_mul(0x2545_F491_4F6C_DD1D);
        *x = (bits >> 11) as f64 / ((1u64 << 52) as f64) - 1.0; // [-1, 1)
    }
    let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if n == 0.0 {
        v[0] = 1.0;
        return v;
    }
    v.iter_mut().for_each(|x| *x /= n);
    v
}

fn cosine(a: &[f64], b: &[f64]) -> Result<f64> {
    anyhow::ensure!(a.len() == b.len(), "cosine: length mismatch");
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    anyhow::ensure!(na > 0.0 && nb > 0.0, "cosine: zero-norm vector");
    Ok(dot / (na * nb).sqrt())
}

// ---------- capture pooling ----------

fn read_manifest(path: &Path) -> Result<Vec<CaptureRow>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read capture manifest {}", path.display()))?;
    let mut rows = Vec::new();
    for (n, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).with_context(|| format!("manifest line {}", n + 1))?);
    }
    Ok(rows)
}

fn read_f32s(path: &Path, expect: usize) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("read capture {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() % 4 == 0,
        "capture {} is not a whole f32 file",
        path.display()
    );
    let vals: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    anyhow::ensure!(
        vals.len() >= expect,
        "capture {} holds {} f32s, expected at least {expect}",
        path.display(),
        vals.len()
    );
    Ok(vals)
}

/// Pool the given manifest rows' capture files into one row list; every
/// row must share `row_elements` (typed refusal on a ragged set).
fn pool(rows: &[&CaptureRow], dir: &Path) -> Result<(Vec<Vec<f32>>, usize)> {
    anyhow::ensure!(!rows.is_empty(), "pool: no capture rows");
    let re = rows[0].row_elements;
    let mut pooled = Vec::new();
    for r in rows {
        anyhow::ensure!(
            r.row_elements == re,
            "ragged capture set: {} is {}-wide, expected {re}",
            r.file,
            r.row_elements
        );
        let vals = read_f32s(&dir.join(&r.file), r.rows * re)?;
        for chunk in vals.chunks_exact(re) {
            pooled.push(chunk.to_vec());
        }
    }
    Ok((pooled, re))
}

fn mean_rows(rows: &[Vec<f32>]) -> Vec<f64> {
    let re = rows[0].len();
    (0..re)
        .map(|j| rows.iter().map(|r| f64::from(r[j])).sum::<f64>() / rows.len() as f64)
        .collect()
}

/// Population variance per channel (the covariance diagonal).
fn variance_rows(rows: &[Vec<f32>], mean: &[f64]) -> Vec<f64> {
    let re = rows[0].len();
    (0..re)
        .map(|j| {
            rows.iter()
                .map(|r| {
                    let d = f64::from(r[j]) - mean[j];
                    d * d
                })
                .sum::<f64>()
                / rows.len() as f64
        })
        .collect()
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

// ---------- the leg ----------

pub fn run(args: ConvertArgs) -> Result<()> {
    let cfg: ConvertConfig = serde_json::from_str(&std::fs::read_to_string(&args.config)?)
        .with_context(|| "parse convert config")?;
    std::fs::create_dir_all(&args.out_dir)?;
    let out = args.out_dir.canonicalize()?;
    let hash = |p: &Path| gguf_writer::sha256_file(p).ok();
    anyhow::ensure!(!cfg.anchors.is_empty(), "no anchors");
    anyhow::ensure!(!cfg.layers.is_empty(), "no edit layers");

    // vectors: N x d f32, little-endian, one row per anchor
    let vbytes = std::fs::read(&args.vectors)?;
    anyhow::ensure!(
        vbytes.len() == cfg.anchors.len() * cfg.vectors_dim * 4,
        "vectors file holds {} f32s; expected {} anchors x dim {}",
        vbytes.len() / 4,
        cfg.anchors.len(),
        cfg.vectors_dim
    );
    let vectors: Vec<Vec<f64>> = (0..cfg.anchors.len())
        .map(|i| {
            (0..cfg.vectors_dim)
                .map(|j| {
                    let off = (i * cfg.vectors_dim + j) * 4;
                    let b: [u8; 4] = vbytes[off..off + 4]
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("vectors file: short f32 at offset {off}"))?;
                    Ok(f64::from(f32::from_le_bytes(b)))
                })
                .collect::<anyhow::Result<Vec<f64>>>()
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let rows = read_manifest(&args.capture_manifest)?;
    let mut files_used: BTreeSet<String> = BTreeSet::new();

    // bridge targets: each anchor's pooled captures at (site, layer)
    let mut targets: Vec<Vec<f64>> = Vec::with_capacity(cfg.anchors.len());
    for a in &cfg.anchors {
        let at: Vec<&CaptureRow> = rows
            .iter()
            .filter(|r| r.site == cfg.site && r.layer == cfg.layer && r.problem_id == a.id)
            .collect();
        anyhow::ensure!(
            !at.is_empty(),
            "no captures for anchor {} at {}-{}",
            a.id,
            cfg.site,
            cfg.layer
        );
        let (pooled, re) = pool(&at, &args.capture_dir)?;
        anyhow::ensure!(
            re == cfg.dims.hidden,
            "anchor {} captures are {re}-dim but dims.hidden is {} (source dims via gguf-info)",
            a.id,
            cfg.dims.hidden
        );
        files_used.extend(at.iter().map(|r| r.file.clone()));
        targets.push(mean_rows(&pooled));
    }

    // the ridge fit on the train split
    let train: Vec<usize> = cfg
        .anchors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.split == Split::Train)
        .map(|(i, _)| i)
        .collect();
    let held: Vec<usize> = cfg
        .anchors
        .iter()
        .enumerate()
        .filter(|(_, a)| a.split == Split::HeldOut)
        .map(|(i, _)| i)
        .collect();
    anyhow::ensure!(!train.is_empty(), "no train anchors");
    let m = ridge_bridge(
        &train
            .iter()
            .map(|&i| vectors[i].clone())
            .collect::<Vec<_>>(),
        &train
            .iter()
            .map(|&i| targets[i].clone())
            .collect::<Vec<_>>(),
        cfg.ridge_lambda,
    )?;

    // u = phi(v_target); alpha applies at edit time (net direction
    // alpha * phi(v), s = alpha/r with r = 1)
    let target_idx = cfg
        .anchors
        .iter()
        .position(|a| a.id == cfg.target_anchor_id)
        .with_context(|| {
            format!(
                "target anchor {} not in the anchor list",
                cfg.target_anchor_id
            )
        })?;
    let u64 = apply_bridge(&m, &vectors[target_idx]);
    let u: Vec<f32> = u64.iter().map(|&x| x as f32).collect();
    let (umin, umax) = (
        u.iter().copied().fold(f32::INFINITY, f32::min),
        u.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    );
    anyhow::ensure!(
        umax - umin > 0.0,
        "constant direction (u variance 0): constant steering is not a linear \
         weight edit (no_linear_edit_is_constant) — a bias edit (out of scope, \
         recorded) is the only compile path for a constant shift"
    );
    let umean = u.iter().map(|&x| f64::from(x)).sum::<f64>() / u.len() as f64;
    let uvar = u
        .iter()
        .map(|&x| {
            let d = f64::from(x) - umean;
            d * d
        })
        .sum::<f64>()
        / u.len() as f64;

    // trigger — two paths (corrective phase S2 makes the split explicit):
    //  * `columns` (the paper-faithful explicit mechanism): k =
    //    indicator(S), S = the top `count` of the ranked list file; the
    //    dense k/covdiag artifacts are not emitted.
    //  * `dense` (frozen): the labeled mean over the trigger anchors'
    //    captures + the calibration covariance diagonal — unchanged.
    let (trigger_spec, trigger_json, trigger_support, trigger_kind): (
        TriggerSpec,
        serde_json::Value,
        usize,
        &'static str,
    ) = match &cfg.trigger.columns {
        Some(cc) => {
            anyhow::ensure!(cc.count > 0, "columns.count must be > 0");
            anyhow::ensure!(
                cc.count <= cfg.dims.trigger,
                "columns.count {} exceeds the trigger dim {}",
                cc.count,
                cfg.dims.trigger
            );
            let ranked_bytes = std::fs::read(&cc.path)
                .with_context(|| format!("read the ranked column list {}", cc.path))?;
            let ranked_sha = gguf_writer::sha256_bytes(&ranked_bytes);
            let ranked_text = std::str::from_utf8(&ranked_bytes)
                .with_context(|| format!("the ranked column list {} is not UTF-8", cc.path))?;
            let mut seen = BTreeSet::new();
            let mut ranked: Vec<usize> = Vec::new();
            for (ln, line) in ranked_text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let j: usize = line.parse().with_context(|| {
                    format!(
                        "the ranked column list {} line {}: not a column index",
                        cc.path,
                        ln + 1
                    )
                })?;
                anyhow::ensure!(
                    j < cfg.dims.trigger,
                    "the ranked column list {} line {}: column {j} out of range (dim {})",
                    cc.path,
                    ln + 1,
                    cfg.dims.trigger
                );
                anyhow::ensure!(
                    seen.insert(j),
                    "the ranked column list {} line {}: duplicate column {j}",
                    cc.path,
                    ln + 1
                );
                ranked.push(j);
            }
            anyhow::ensure!(
                ranked.len() >= cc.count,
                "the ranked column list {} holds {} columns; the selection needs {}",
                cc.path,
                ranked.len(),
                cc.count
            );
            let selected: Vec<usize> = ranked.iter().copied().take(cc.count).collect();
            let cols_path = out.join("columns.txt");
            let cols_body = selected
                .iter()
                .map(|j| j.to_string())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(&cols_path, cols_body)?;
            let cols_sha = gguf_writer::sha256_file(&cols_path)?;
            (
                TriggerSpec::Columns {
                    path: cols_path.display().to_string(),
                    sha256: cols_sha.clone(),
                    dim: cfg.dims.trigger,
                    count: selected.len(),
                },
                serde_json::json!({
                    "kind": "columns",
                    "site": cfg.trigger.site, "layer": cfg.trigger.layer,
                    "problem_ids": cfg.trigger.problem_ids,
                    "columns": {"path": cols_path.display().to_string(),
                                 "sha256": cols_sha,
                                 "count": selected.len(), "dim": cfg.dims.trigger},
                    "ranked_list": {"path": cc.path, "sha256": ranked_sha,
                                      "rows": ranked.len()},
                    "selected": selected,
                }),
                selected.len(),
                "columns",
            )
        }
        None => {
            let trig_rows: Vec<&CaptureRow> = rows
                .iter()
                .filter(|r| {
                    r.site == cfg.trigger.site
                        && r.layer == cfg.trigger.layer
                        && cfg.trigger.problem_ids.contains(&r.problem_id)
                })
                .collect();
            anyhow::ensure!(
                !trig_rows.is_empty(),
                "no trigger captures at {}-{} for the named anchors",
                cfg.trigger.site,
                cfg.trigger.layer
            );
            let (trig_pooled, tre) = pool(&trig_rows, &args.capture_dir)?;
            anyhow::ensure!(
                tre == cfg.dims.trigger,
                "trigger captures are {tre}-dim but dims.trigger is {} — the dense \
                     trigger must match the edit tensor's input dim (gguf-info)",
                cfg.dims.trigger
            );
            files_used.extend(trig_rows.iter().map(|r| r.file.clone()));
            let k: Vec<f32> = mean_rows(&trig_pooled).iter().map(|&x| x as f32).collect();
            let (kmin, kmax) = (
                k.iter().copied().fold(f32::INFINITY, f32::min),
                k.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            );
            anyhow::ensure!(
                kmax - kmin > 0.0,
                "degenerate trigger (constant): a trigger that fires equally on every \
                     input is not a trigger"
            );

            // calibration: the covariance diagonal over the calibration rows
            let cal_rows: Vec<&CaptureRow> = rows
                .iter()
                .filter(|r| {
                    r.site == cfg.trigger.site
                        && r.layer == cfg.trigger.layer
                        && match &cfg.trigger.calibration_problem_ids {
                            Some(ids) => ids.contains(&r.problem_id),
                            None => !cfg.trigger.problem_ids.contains(&r.problem_id),
                        }
                })
                .collect();
            anyhow::ensure!(
                !cal_rows.is_empty(),
                "no calibration captures at {}-{} — the covariance diagonal needs a \
                     non-trigger baseline",
                cfg.trigger.site,
                cfg.trigger.layer
            );
            let (cal_pooled, cre) = pool(&cal_rows, &args.capture_dir)?;
            anyhow::ensure!(
                cre == tre,
                "calibration rows are {cre}-dim, trigger is {tre}"
            );
            files_used.extend(cal_rows.iter().map(|r| r.file.clone()));
            let covdiag: Vec<f32> = variance_rows(&cal_pooled, &mean_rows(&cal_pooled))
                .iter()
                .map(|&x| x as f32)
                .collect();

            let k_path = out.join("k.f32");
            let cov_path = out.join("covdiag.f32");
            std::fs::write(&k_path, f32_bytes(&k))?;
            std::fs::write(&cov_path, f32_bytes(&covdiag))?;
            (
                TriggerSpec::Dense {
                    path: k_path.display().to_string(),
                    dim: tre,
                    covdiag_path: Some(cov_path.display().to_string()),
                },
                serde_json::json!({
                    "kind": "dense",
                    "site": cfg.trigger.site, "layer": cfg.trigger.layer,
                    "problem_ids": cfg.trigger.problem_ids,
                    "rows_pooled": trig_pooled.len(),
                    "k": {"path": k_path.display().to_string(), "sha256": hash(&k_path),
                           "dim": tre, "min": kmin, "max": kmax},
                    "covdiag": {"path": cov_path.display().to_string(), "sha256": hash(&cov_path),
                                 "dim": tre,
                                 "note": "per-channel variance of the calibration rows; the edit \
                                          leg whitens k_eff = k / sqrt(var + 1e-6) when covdiag_path is set"},
                    "calibration": {"rows": cal_pooled.len(),
                                      "problem_ids": cfg.trigger.calibration_problem_ids
                                          .clone().unwrap_or_else(|| vec!["others".into()])},
                }),
                tre,
                "dense",
            )
        }
    };

    // E1's measurements: per-anchor bridge cosines + the control
    let mut per_anchor = serde_json::Map::new();
    for (i, a) in cfg.anchors.iter().enumerate() {
        per_anchor.insert(
            a.id.clone(),
            serde_json::json!(cosine(&apply_bridge(&m, &vectors[i]), &targets[i])?),
        );
    }
    let mean_of = |idx: &[usize]| -> anyhow::Result<f64> {
        let mut sum = 0.0;
        for &i in idx {
            sum += cosine(&apply_bridge(&m, &vectors[i]), &targets[i])?;
        }
        Ok(sum / idx.len() as f64)
    };
    let train_mean = mean_of(&train)?;
    let held_mean = (!held.is_empty()).then(|| mean_of(&held)).transpose()?;
    let control = match &cfg.control {
        Some(c) => {
            anyhow::ensure!(!held.is_empty(), "control needs held-out anchors");
            let cos: Vec<f64> = held
                .iter()
                .enumerate()
                .map(|(hi, &i)| cosine(&xorshift_unit(c.seed, hi, cfg.dims.hidden), &targets[i]))
                .collect::<Result<_>>()?;
            Some(serde_json::json!({
                "seed": c.seed,
                "held_out_mean": cos.iter().sum::<f64>() / cos.len() as f64,
                "held_out_max": cos.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            }))
        }
        None => None,
    };

    // outputs: u.f32 always (the bridge direction); the trigger's own
    // artifacts were written by the branch above (k.f32 + covdiag.f32 on
    // the dense path, columns.txt on the column-list path)
    let u_path = out.join("u.f32");
    std::fs::write(&u_path, f32_bytes(&u))?;
    let spec = EditSpec {
        layers: cfg.layers.clone(),
        tensor: cfg.tensor.clone(),
        alpha: cfg.alpha,
        direction: DirectionSpec::File {
            path: u_path.display().to_string(),
            dim: cfg.dims.hidden,
        },
        trigger: trigger_spec,
        input: cfg.input_gguf.clone(),
        output: cfg.output_gguf.clone(),
    };
    let spec_path = out.join("edit-spec.json");
    std::fs::write(&spec_path, serde_json::to_string_pretty(&spec)?)?;
    crate::edit::validate_spec(&spec)?;

    // the DME-2 projection (corrective phase S2): every trigger-support
    // column receives the direction in every edited layer — the anchor's
    // own arithmetic is 2 layers x 1024 x 64 = 131,072 weights = 0.016% of
    // 0.8B. The dense trigger's projected support is the full input dim
    // (an upper bound; the edit leg re-derives the real support from the
    // whitened k and gates on it against the parsed GGUF's param_count).
    // The instrument measures and states the share; the pre-emit lisp
    // gate and the edit leg's hard refusal decide on it.
    let projected_weights_touched =
        cfg.layers.len() as u64 * cfg.dims.hidden as u64 * trigger_support as u64;
    let projected_edited_share = dme2_share(projected_weights_touched, cfg.total_weights);

    // the manifest: sha256 of every input and output, the fitted phi's
    // digest, the dims, alpha, and the EditSpec
    let mut phi_bytes = Vec::with_capacity(m.len() * m[0].len() * 8);
    for row in &m {
        for x in row {
            phi_bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    let capture_files: Vec<serde_json::Value> = files_used
        .iter()
        .map(|f| serde_json::json!({"file": f, "sha256": hash(&args.capture_dir.join(f))}))
        .collect();
    let manifest = serde_json::json!({
        "ts": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64()).unwrap_or(0.0),
        "harness": format!("dme-harness-rust/{} convert", env!("CARGO_PKG_VERSION")),
        "inputs": {
            "config": {"path": args.config.display().to_string(), "sha256": hash(&args.config)},
            "vectors": {"path": args.vectors.display().to_string(),
                         "sha256": hash(&args.vectors), "rows": cfg.anchors.len()},
            "capture_manifest": {"path": args.capture_manifest.display().to_string(),
                                   "sha256": hash(&args.capture_manifest)},
            "capture_files": capture_files,
            "input_gguf": {"path": cfg.input_gguf, "sha256": hash(Path::new(&cfg.input_gguf))},
        },
        "dims": {"vectors": cfg.vectors_dim, "hidden": cfg.dims.hidden, "trigger": cfg.dims.trigger},
        "anchors": {
            "count": cfg.anchors.len(), "train": train.len(), "held_out": held.len(),
            "target": cfg.target_anchor_id,
            "ids": cfg.anchors,
        },
        "bridge": {
            "site": cfg.site, "layer": cfg.layer, "ridge_lambda": cfg.ridge_lambda,
            "phi_sha256": gguf_writer::sha256_bytes(&phi_bytes),
            "note": "phi: R^d -> R^h linear (bridge_linear); u.f32 = phi(v_target), \
                     alpha applies at edit time — net direction alpha * phi(v), \
                     s = alpha/r with r = 1 (lora_runtime_semantics)",
        },
        "alpha": cfg.alpha,
        "rank": 1u32,
        "total_weights": cfg.total_weights,
        "projected_weights_touched": projected_weights_touched,
        "projected_edited_share": projected_edited_share,
        "u": {"path": u_path.display().to_string(), "sha256": hash(&u_path),
               "dim": cfg.dims.hidden, "min": umin, "max": umax, "variance": uvar,
               "l2": u.iter().map(|&x| f64::from(x * x)).sum::<f64>().sqrt()},
        "trigger": trigger_json,
        "cosines": {"train_mean": train_mean, "held_out_mean": held_mean,
                     "per_anchor": per_anchor},
        "control": control,
        "edit_spec": spec,
        "limitations": "constant steering is not a linear weight edit \
                        (no_linear_edit_is_constant); bias edits are out of scope",
    });
    let manifest_path = out.join("convert-manifest.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    eprintln!(
        "convert: {} anchors ({} train / {} held-out), train cos {:.4}, held-out cos {}, \
         control {}, u {}-dim, trigger {} (support {} of dim {}), projected {} weights \
         ({:.5}% of {}), manifest {}",
        cfg.anchors.len(),
        train.len(),
        held.len(),
        train_mean,
        held_mean
            .map(|c| format!("{c:.4}"))
            .unwrap_or_else(|| "n/a".into()),
        control
            .as_ref()
            .map(|c| format!("{:.4}", c["held_out_mean"].as_f64().unwrap_or(0.0)))
            .unwrap_or_else(|| "n/a".into()),
        cfg.dims.hidden,
        trigger_kind,
        trigger_support,
        cfg.dims.trigger,
        projected_weights_touched,
        projected_edited_share * 100.0,
        cfg.total_weights,
        manifest_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn write_f32(path: &Path, vals: &[f32]) {
        std::fs::write(path, f32_bytes(vals)).unwrap();
    }

    /// A std-only temp-dir guard: a unique dir under the system temp,
    /// removed on drop — the fixtures carry no external dev-dep.

    fn fixture_captures(dir: &Path, anchors: &[(&str, Vec<f32>)]) -> (String, PathBuf) {
        let dir = dir.join("captures");
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = String::new();
        for (id, vals) in anchors {
            let file = format!("{id}.l_out.12.f32");
            write_f32(&dir.join(&file), vals);
            manifest.push_str(&format!(
                "{{\"ts\":1.0,\"problem_id\":\"{id}\",\"label\":\"\",\"site\":\"l_out\",\
                 \"layer\":12,\"rows\":1,\"row_elements\":{},\"file\":\"{file}\",\
                 \"harness\":\"test\"}}\n",
                vals.len()
            ));
        }
        (manifest, dir)
    }

    /// M2/S2-1: the Cholesky solve is exact on a constructed
    /// perfect-square factorization (A = L L^T with integer L).
    #[test]
    fn cholesky_solves_exact_spd() {
        let a = vec![vec![4.0, 2.0], vec![2.0, 10.0]]; // L = [[2,0],[1,3]]
        let l = cholesky(&a).expect("SPD");
        let x = chol_solve(&l, &[6.0, 12.0]); // A [1,1]^T
        assert!((x[0] - 1.0).abs() < 1e-12 && (x[1] - 1.0).abs() < 1e-12);
    }

    /// M2/S2-2: the ridge fit recovers a constructed exact bridge.
    #[test]
    fn ridge_recovers_the_constructed_bridge() {
        let m_true = [vec![1.0, 0.0], vec![0.0, 1.0], vec![1.0, 1.0]];
        let v = [vec![1.0, 0.0], vec![0.0, 1.0], vec![2.0, 3.0]];
        let t: Vec<Vec<f64>> = v
            .iter()
            .map(|x| {
                m_true
                    .iter()
                    .map(|row| row.iter().zip(x).map(|(&a, &b)| a * b).sum())
                    .collect()
            })
            .collect();
        let m = ridge_bridge(&v, &t, 0.0).expect("lambda=0 on a full-rank gram");
        for (mrow, trow) in m.iter().zip(m_true.iter()) {
            for (got, want) in mrow.iter().zip(trow.iter()) {
                assert!((got - want).abs() < 1e-9, "got {m:?}");
            }
        }
    }

    /// M2/S2-3: lambda > 0 solves a singular (rank-deficient) gram and
    /// satisfies the regularized normal equations.
    #[test]
    fn ridge_regularizes_a_singular_gram() {
        let v = [vec![1.0, 1.0], vec![2.0, 2.0], vec![3.0, 3.0]]; // rank 1
        let t = [vec![1.0], vec![2.0], vec![3.0]];
        let lambda = 1.0;
        let m = ridge_bridge(&v, &t, lambda).expect("regularized fit");
        // residual: (V^T V + lambda I) m^T == V^T t
        for j in 0..1 {
            for i in 0..2 {
                let lhs: f64 = (0..2)
                    .map(|c| {
                        let g: f64 = (0..3).map(|r| v[r][i] * v[r][c]).sum::<f64>()
                            + if i == c { lambda } else { 0.0 };
                        g * m[j][c]
                    })
                    .sum();
                let rhs: f64 = (0..3).map(|r| v[r][i] * t[r][j]).sum();
                assert!((lhs - rhs).abs() < 1e-9);
            }
        }
    }

    /// M2/S2-4: `rank_one_edit_action` — (W - u k^T) x = W x - u (k^T x)
    /// on exact integers, and `rank_one_edit_preserves_orthogonal` for
    /// k^T x = 0.
    #[test]
    fn rank_one_action_matches_the_pinned_lemma() {
        let w = [vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]];
        let u = [2.0, -1.0];
        let k = [1.0, 0.0, 3.0];
        let x = [1.0, 2.0, -1.0];
        let dot: f64 = k.iter().zip(&x).map(|(&a, &b)| a * b).sum();
        for i in 0..2 {
            let wx: f64 = w[i].iter().zip(&x).map(|(&a, &b)| a * b).sum();
            let wpx: f64 = (0..3).map(|j| (w[i][j] - u[i] * k[j]) * x[j]).sum();
            assert_eq!(wpx, wx - dot * u[i], "rank_one_edit_action");
        }
        let x_orth = [3.0, 5.0, -1.0]; // k^T x = 3 - 3 = 0
        for i in 0..2 {
            let wx: f64 = w[i].iter().zip(&x_orth).map(|(&a, &b)| a * b).sum();
            let wpx: f64 = (0..3).map(|j| (w[i][j] - u[i] * k[j]) * x_orth[j]).sum();
            assert_eq!(wpx, wx, "rank_one_edit_preserves_orthogonal");
        }
    }

    /// M2/S2-5: pooling + the labeled mean + the covariance diagonal match
    /// hand-computed values over a fixture manifest.
    #[test]
    fn trigger_mean_and_covdiag_match_hand_values() {
        let dir = TempDir::new("mean-cov");
        let (manifest, cdir) = fixture_captures(
            dir.path(),
            &[
                ("a0", vec![1.0, 0.0, 2.0]),
                ("a1", vec![0.0, 1.0, 3.0]),
                ("a2", vec![1.0, 1.0, 5.0]),
            ],
        );
        let mpath = dir.path().join("manifest.jsonl");
        std::fs::write(&mpath, manifest).unwrap();
        let rows = read_manifest(&mpath).unwrap();
        let trig: Vec<&CaptureRow> = rows.iter().filter(|r| r.problem_id == "a1").collect();
        let (pooled, re) = pool(&trig, &cdir).unwrap();
        assert_eq!(re, 3);
        let mean = mean_rows(&pooled);
        assert_eq!(mean, vec![0.0, 1.0, 3.0]);
        let cal: Vec<&CaptureRow> = rows.iter().filter(|r| r.problem_id != "a1").collect();
        let (cpooled, _) = pool(&cal, &cdir).unwrap();
        let cov = variance_rows(&cpooled, &mean_rows(&cpooled));
        // calibration rows [1,0,2] and [1,1,5]: variances 0, 0.25, 2.25
        assert!((cov[0] - 0.0).abs() < 1e-12, "ch0 {}", cov[0]);
        assert!((cov[1] - 0.25).abs() < 1e-12, "ch1 {}", cov[1]);
        assert!((cov[2] - 2.25).abs() < 1e-12, "ch2 {}", cov[2]);
    }

    /// A convert fixture: the temp dir, the capture fixtures, the manifest
    /// and vectors files, and the config path — the repeated per-test setup
    /// the ratchet's duplication gate caught.
    struct ConvertFixture {
        dir: TempDir,
        manifest_path: PathBuf,
        vectors_path: PathBuf,
        capture_dir: PathBuf,
        config_path: PathBuf,
    }

    impl ConvertFixture {
        fn new(name: &str, captures: &[(&str, Vec<f32>)], vectors: &[f32]) -> Self {
            let dir = TempDir::new(name);
            let (manifest, cdir) = fixture_captures(dir.path(), captures);
            let manifest_path = dir.path().join("manifest.jsonl");
            std::fs::write(&manifest_path, manifest).unwrap();
            let vectors_path = dir.path().join("vectors.f32");
            write_f32(&vectors_path, vectors);
            let config_path = dir.path().join("cfg.json");
            Self {
                dir,
                manifest_path,
                vectors_path,
                capture_dir: cdir,
                config_path,
            }
        }

        fn dir(&self) -> &Path {
            self.dir.path()
        }

        /// Write the test's config and return the args that run the convert.
        fn write_config(&self, cfg: serde_json::Value) -> ConvertArgs {
            std::fs::write(&self.config_path, serde_json::to_string(&cfg).unwrap()).unwrap();
            ConvertArgs {
                config: self.config_path.clone(),
                vectors: self.vectors_path.clone(),
                capture_manifest: self.manifest_path.clone(),
                capture_dir: self.capture_dir.clone(),
                out_dir: self.dir.path().join("out"),
            }
        }
    }

    /// M2/S2-6: the full leg on constructed ground truth — an exact
    /// integer bridge, a held-out anchor, a trigger, a calibration set and
    /// the control — emitting every output plus a manifest whose EditSpec
    /// parses through the edit leg's own loader.
    #[test]
    fn convert_end_to_end_on_constructed_ground_truth() {
        let fixture = ConvertFixture::new(
            "e2e",
            &[
                ("a0", vec![1.0, 0.0, 1.0]),
                ("a1", vec![0.0, 1.0, 1.0]),
                ("a2", vec![2.0, 3.0, 5.0]),
                ("a3", vec![3.0, 1.0, 4.0]),
            ],
            &[1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 1.0],
        );
        let dummy_gguf = fixture.dir().join("target.gguf");
        std::fs::write(&dummy_gguf, b"dummy").unwrap();
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [
                {"id": "a0", "split": "train"},
                {"id": "a1", "split": "train"},
                {"id": "a2", "split": "train"},
                {"id": "a3", "split": "held_out"}
            ],
            "site": "l_out", "layer": 12,
            "layers": [12], "tensor": "ffn_down",
            "alpha": 8.0, "ridge_lambda": 0.0,
            "target_anchor_id": "a3",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a3"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 3, "trigger": 3},
            "total_weights": 1000000,
            "control": {"seed": 20261106},
            "input_gguf": dummy_gguf.display().to_string(),
            "output_gguf": fixture.dir().join("edited.gguf").display().to_string()
        }));
        run(args).expect("convert runs");

        let out = fixture.dir().join("out");
        // u = phi(v_a3) = [3, 1, 4] exactly (the constructed bridge)
        let u = read_f32s(&out.join("u.f32"), 3).unwrap();
        for (got, want) in u.iter().zip([3.0f32, 1.0, 4.0]) {
            assert!((got - want).abs() < 1e-4, "u {u:?}");
        }
        // the trigger is a3's labeled mean; covdiag is the others' variance
        assert_eq!(
            read_f32s(&out.join("k.f32"), 3).unwrap(),
            vec![3.0, 1.0, 4.0]
        );
        let cov = read_f32s(&out.join("covdiag.f32"), 3).unwrap();
        assert!((cov[0] - 2.0 / 3.0).abs() < 1e-6);
        // the emitted EditSpec parses through the edit leg's own loader
        let spec = crate::edit::load_spec(&out.join("edit-spec.json")).expect("spec parses");
        assert!(matches!(
            spec.trigger,
            TriggerSpec::Dense { ref covdiag_path, .. } if covdiag_path.is_some()
        ));
        // the manifest: hashes, dims, the exact-bridge cosine, the control
        let man: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("convert-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(man["dims"]["hidden"], 3);
        assert_eq!(man["anchors"]["train"], 3);
        assert_eq!(man["anchors"]["held_out"], 1);
        // corrective phase S2 — the DME-2 projection on the manifest:
        // 1 layer x hidden 3 x dense support 3 = 9 touched, stated with
        // its denominator
        assert_eq!(man["trigger"]["kind"], "dense");
        assert_eq!(man["total_weights"], 1_000_000);
        assert_eq!(man["projected_weights_touched"], 9);
        let share = man["projected_edited_share"].as_f64().unwrap();
        assert!((share - 9.0 / 1_000_000.0).abs() < 1e-15);
        assert!(
            man["cosines"]["held_out_mean"].as_f64().unwrap() > 0.99999,
            "the constructed bridge is exact on held-out"
        );
        let c1 = man["control"].clone();
        assert!(c1["held_out_mean"].as_f64().unwrap().abs() <= 1.0);
        for key in ["u", "trigger"] {
            let node = &man[key];
            let sha = if key == "u" {
                node["sha256"].as_str()
            } else {
                node["k"]["sha256"].as_str()
            };
            assert!(sha.unwrap().len() == 64, "manifest {key} hash present");
        }
        assert!(man["inputs"]["input_gguf"]["sha256"].as_str().is_some());
        // determinism: the seeded control repeats bit-for-bit
        let a = xorshift_unit(20261106, 0, 3);
        let b = xorshift_unit(20261106, 0, 3);
        assert_eq!(a, b);
        // the demo-good gate fixture: copy the real manifest out for the
        // decision-time lisp gate (gates/demo-good.json is regenerated
        // from this run)
        let demo = std::env::temp_dir().join("dme-convert-e2e-manifest.json");
        std::fs::copy(out.join("convert-manifest.json"), &demo).expect("copy demo manifest");
    }

    /// M2/S2-7: an anchor with no captures refuses loudly.
    #[test]
    fn refuses_an_anchor_without_captures() {
        let fixture = ConvertFixture::new(
            "refuse-missing",
            &[("a0", vec![1.0, 0.0, 1.0])],
            &[1.0, 0.0, 0.0, 1.0],
        );
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [{"id": "a0", "split": "train"}, {"id": "a1", "split": "held_out"}],
            "site": "l_out", "layer": 12, "layers": [12], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0, "target_anchor_id": "a0",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a0"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 3, "trigger": 3},
            "total_weights": 100,
            "input_gguf": "x", "output_gguf": "y"
        }));
        let err = run(args).expect_err("missing anchor captures must refuse");
        assert!(
            err.to_string().contains("no captures for anchor a1"),
            "wrong refusal: {err:#}"
        );
    }

    /// M2/S2-8: a declared dim that disagrees with the captures refuses.
    #[test]
    fn refuses_a_dim_mismatch() {
        let fixture =
            ConvertFixture::new("refuse-dim", &[("a0", vec![1.0, 0.0, 1.0])], &[1.0, 0.0]);
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [{"id": "a0", "split": "train"}],
            "site": "l_out", "layer": 12, "layers": [12], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0, "target_anchor_id": "a0",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a0"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 4, "trigger": 3},
            "total_weights": 100,
            "input_gguf": "x", "output_gguf": "y"
        }));
        let err = run(args).expect_err("dim mismatch must refuse");
        assert!(
            err.to_string().contains("dims.hidden is 4"),
            "wrong refusal: {err:#}"
        );
    }

    /// M2/S2-9: a constant fitted direction refuses, pointing at the
    /// bias-edit limitation (`no_linear_edit_is_constant`).
    #[test]
    fn refuses_constant_steering() {
        let fixture = ConvertFixture::new(
            "refuse-constant",
            &[("a0", vec![1.0, 1.0, 1.0]), ("a1", vec![1.0, 1.0, 1.0])],
            &[1.0, 0.0, 0.0, 1.0],
        );
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [{"id": "a0", "split": "train"}, {"id": "a1", "split": "train"}],
            "site": "l_out", "layer": 12, "layers": [12], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0, "target_anchor_id": "a0",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a0"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 3, "trigger": 3},
            "total_weights": 100,
            "input_gguf": "x", "output_gguf": "y"
        }));
        let err = run(args).expect_err("constant steering must refuse");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("constant steering") && msg.contains("no_linear_edit_is_constant"),
            "wrong refusal: {msg}"
        );
    }

    /// M2/S2-10: the vectors file must hold exactly anchors x dim f32s.
    #[test]
    fn refuses_a_misaligned_vectors_file() {
        let fixture = ConvertFixture::new(
            "refuse-vectors",
            &[("a0", vec![1.0, 0.0, 1.0])],
            &[1.0, 0.0, 9.0], // 3 f32s for 1 anchor x dim 2
        );
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [{"id": "a0", "split": "train"}],
            "site": "l_out", "layer": 12, "layers": [12], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0, "target_anchor_id": "a0",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a0"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 3, "trigger": 3},
            "total_weights": 100,
            "input_gguf": "x", "output_gguf": "y"
        }));
        let err = run(args).expect_err("misaligned vectors file must refuse");
        assert!(
            err.to_string().contains("vectors file holds"),
            "wrong refusal: {err:#}"
        );
    }

    /// Corrective phase S2 — a config that cannot state the model's
    /// total weights refuses at parse time: DME-2 is mechanical, and a
    /// manifest that cannot state its projected share cannot be emitted.
    #[test]
    fn refuses_a_config_without_total_weights() {
        let cfg = serde_json::json!({
            "vectors_dim": 2,
            "anchors": [{"id": "a0", "split": "train"}],
            "site": "l_out", "layer": 12, "layers": [12], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0, "target_anchor_id": "a0",
            "trigger": {"site": "l_out", "layer": 12, "problem_ids": ["a0"],
                         "calibration_problem_ids": null},
            "dims": {"hidden": 3, "trigger": 3},
            "input_gguf": "x", "output_gguf": "y"
        });
        let err = serde_json::from_value::<ConvertConfig>(cfg)
            .expect_err("a config without total_weights must refuse");
        assert!(
            err.to_string().contains("total_weights"),
            "wrong refusal: {err}"
        );
    }

    /// Corrective phase S2 — the explicit column-list trigger path: a
    /// ranked list round-trips into the sha-bound columns file + the
    /// spec, the manifest carries the projection (layers x hidden x
    /// |S|), and no dense k/covdiag artifacts are emitted. The bridge
    /// anchors' captures are still required (u is unchanged); the
    /// trigger captures are not (the columns path does not build k
    /// from captures).
    #[test]
    fn column_list_round_trips_into_the_spec() {
        let fixture = ConvertFixture::new(
            "cols-e2e",
            &[
                ("a0", vec![1.0, 0.0, 1.0]),
                ("a1", vec![0.0, 1.0, 1.0]),
                ("a2", vec![2.0, 3.0, 5.0]),
                ("a3", vec![3.0, 1.0, 4.0]),
            ],
            &[1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 1.0],
        );
        let ranked = fixture.dir().join("ranked.txt");
        std::fs::write(&ranked, "2\n0\n1\n").unwrap(); // ranked: 2, 0, 1 — take 2
        let args = fixture.write_config(serde_json::json!({
            "vectors_dim": 2,
            "anchors": [
                {"id": "a0", "split": "train"},
                {"id": "a1", "split": "train"},
                {"id": "a2", "split": "train"},
                {"id": "a3", "split": "held_out"}
            ],
            "site": "l_out", "layer": 12,
            "layers": [15, 19], "tensor": "ffn_down",
            "alpha": 1.0, "ridge_lambda": 0.0,
            "target_anchor_id": "a3",
            "trigger": {"site": "l_out", "layer": 15, "problem_ids": [],
                         "calibration_problem_ids": null,
                         "columns": {"path": ranked.display().to_string(), "count": 2}},
            "dims": {"hidden": 3, "trigger": 3},
            "total_weights": 1000000,
            "input_gguf": "x", "output_gguf": "y"
        }));
        run(args).expect("the column-list convert runs");

        // the columns file: the top-2 of the ranking, order preserved
        let cols_text =
            std::fs::read_to_string(fixture.dir().join("out").join("columns.txt")).unwrap();
        assert_eq!(cols_text, "2\n0\n");
        // the emitted spec: the sha-bound Columns trigger, parseable by
        // the edit leg's own loader
        let spec = crate::edit::load_spec(&fixture.dir().join("out").join("edit-spec.json"))
            .expect("spec parses");
        match &spec.trigger {
            crate::edit::TriggerSpec::Columns {
                count,
                dim,
                sha256,
                path,
            } => {
                assert_eq!((*count, *dim), (2, 3));
                assert_eq!(
                    sha256.as_str(),
                    gguf_writer::sha256_file(Path::new(path)).unwrap().as_str()
                );
            }
            other => panic!("expected the columns trigger, got {other:?}"),
        }
        assert_eq!(spec.layers, vec![15, 19]);
        // no dense artifacts on this path
        assert!(!fixture.dir().join("out").join("k.f32").exists());
        assert!(!fixture.dir().join("out").join("covdiag.f32").exists());
        // the manifest: the columns trigger record + the projection
        // (2 layers x hidden 3 x |S| 2 = 12)
        let man: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(fixture.dir().join("out").join("convert-manifest.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(man["trigger"]["kind"], "columns");
        assert_eq!(man["trigger"]["selected"], serde_json::json!([2, 0]));
        assert_eq!(man["trigger"]["columns"]["count"], 2);
        assert!(man["trigger"]["columns"]["sha256"].as_str().unwrap().len() == 64);
        assert!(
            man["trigger"]["ranked_list"]["sha256"]
                .as_str()
                .unwrap()
                .len()
                == 64
        );
        assert_eq!(man["projected_weights_touched"], 12);
        let share = man["projected_edited_share"].as_f64().unwrap();
        assert!((share - 12.0 / 1_000_000.0).abs() < 1e-15);
    }

    /// Corrective phase S2 — the ranked list is validated: duplicates,
    /// out-of-range columns, and a short list refuse loudly.
    #[test]
    fn refuses_a_bad_ranked_column_list() {
        for (body, needle) in [
            ("2\n2\n1\n", "duplicate column"),
            ("3\n0\n1\n", "out of range"),
            ("0\n", "the selection needs 2"),
            ("x\n0\n1\n", "not a column index"),
        ] {
            let fixture = ConvertFixture::new(
                "cols-refuse",
                &[
                    ("a0", vec![1.0, 0.0, 1.0]),
                    ("a1", vec![0.0, 1.0, 1.0]),
                    ("a2", vec![2.0, 3.0, 5.0]),
                    ("a3", vec![3.0, 1.0, 4.0]),
                ],
                &[1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 1.0],
            );
            let ranked = fixture.dir().join("ranked.txt");
            std::fs::write(&ranked, body).unwrap();
            let args = fixture.write_config(serde_json::json!({
                "vectors_dim": 2,
                "anchors": [
                    {"id": "a0", "split": "train"},
                    {"id": "a1", "split": "train"},
                    {"id": "a2", "split": "train"},
                    {"id": "a3", "split": "held_out"}
                ],
                "site": "l_out", "layer": 12,
                "layers": [15], "tensor": "ffn_down",
                "alpha": 1.0, "ridge_lambda": 0.0,
                "target_anchor_id": "a3",
                "trigger": {"site": "l_out", "layer": 15, "problem_ids": [],
                             "calibration_problem_ids": null,
                             "columns": {"path": ranked.display().to_string(), "count": 2}},
                "dims": {"hidden": 3, "trigger": 3},
                "total_weights": 1000000,
                "input_gguf": "x", "output_gguf": "y"
            }));
            let err = run(args).expect_err("the bad ranked list must refuse");
            assert!(
                err.to_string().contains(needle),
                "wrong refusal for {body:?}: {err:#}"
            );
        }
    }
}
