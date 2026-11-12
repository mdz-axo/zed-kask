//! The edit leg (DME steps 4-5): apply a direction-subtraction edit to a
//! GGUF in place on a copy, with a receipt carrying the exact surface and
//! the arithmetic fidelity.
//!
//! The anchor's surface: 131,072 weights = 2 layers x 64 channels x 1024
//! weights. A "channel" is a torch-COLUMN of the down projection
//! (`blk.{L}.ffn_down.weight`, ggml [ne0=input_dim, ne1=1024]): the
//! 1024-dim direction (residual space) is subtracted from the channel's
//! 1024 strided elements — the direction's write path through that
//! channel is removed. Spike 2 measured this exact shape.
//!
//! The dense trigger generalizes the mask: the edit is the rank-one action
//! W <- W - alpha * direction * k^T (`rank_one_edit_action`), where the
//! channel mask is the indicator special case of the per-column weight
//! vector k (the convert module emits k and the calibration covariance
//! diagonal; this leg whitens when covdiag_path is set).
//!
//! The explicit column-list trigger (corrective phase S2) is the
//! paper-faithful form of the same indicator: a sha-bound columns file
//! names the selected input columns and this leg builds the exact
//! k = indicator(S) — the anchor's mechanism (64 output columns selected
//! by failure involvement; the S1 provenance run 3d2c6f7b39ec213c pins
//! the citation), and the sparse-support shape the Lean contract's
//! sparse-support preservation corollary names (k^T x = sum_{i in S} x_i).
//!
//! DME-2 is mechanical from the corrective phase: the projected edit
//! surface (every trigger-support column receives the direction in
//! every edited layer) must sit within 0.1% of the model's total
//! weights — refused here against the parsed GGUF's real param_count,
//! belt and braces with the pre-emit lisp gate over the convert
//! manifest (the instrument measures, the gates decide; this refusal
//! is the instrument's own hard stop).
//!
//! The GGUF parse, the byte-exact patch primitive, the minimal-collateral
//! Q8_0 codec and the weighted-column kernel live in `gguf` and
//! `gguf_writer` —
//! this leg keeps only the DME orchestration: the spec (the trigger — the
//! ONE required spelling), the patched copy and the receipt.

use crate::{gguf, gguf_writer};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EditSpec {
    pub layers: Vec<u32>,
    /// tensor stem within each layer, e.g. "ffn_down" -> blk.{L}.ffn_down.weight
    pub tensor: String,
    pub alpha: f32,
    /// direction source: seeded unit vector, or an f32 file (1024 values)
    pub direction: DirectionSpec,
    /// the rank-one trigger — the ONE spelling (operator ruling
    /// 2026-11-06: no backward-compatibility surface exists; the legacy
    /// parallel `channels` field is deleted — no archived edit spec ever
    /// existed to be compatible with). The channel mask is the indicator
    /// special case (the anchor's measured surface); `Dense` carries a
    /// per-column weight file (the convert module's k) plus an optional
    /// calibration covariance diagonal, whitened at edit time; `Columns`
    /// (corrective phase S2) is the paper-faithful explicit column list —
    /// the sha-bound file names the selected columns and this leg builds
    /// the exact indicator.
    pub trigger: TriggerSpec,
    pub input: String,
    pub output: String,
}

/// Trigger selection over the tensor's input dim (ne0): `count` channels
/// spread evenly (the anchor's indicator special case of the pinned
/// rank-one family, cso:low-rank_adaptation), a dense per-column weight
/// vector — the general case — or the explicit column list (corrective
/// phase S2): the sha-bound columns file names the selected input
/// columns and the edit builds the exact indicator k = 1{S}, the
/// paper-faithful mechanism (the anchor's 64 output columns selected by
/// failure involvement).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSpec {
    Channels {
        count: usize,
    },
    Dense {
        path: String,
        dim: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        covdiag_path: Option<String>,
    },
    Columns {
        /// the columns file: one selected column index per line, in the
        /// localization ranking's order (the set is what the edit uses)
        path: String,
        /// the sha256 binding, verified at load time — the explicit list
        /// is load-bearing (a truncated or tampered list silently changes
        /// the edit surface), so a mismatch is a typed refusal
        sha256: String,
        /// the tensor input dim the columns live on (must equal ne0)
        dim: usize,
        /// the number of selected columns
        count: usize,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "source")]
pub enum DirectionSpec {
    #[serde(rename = "seeded")]
    Seeded { seed: u64, dim: usize },
    #[serde(rename = "file")]
    File { path: String, dim: usize },
}

#[derive(Debug, Serialize)]
pub struct EditReceipt {
    pub ts: f64,
    pub input: String,
    pub output: String,
    pub input_sha256: String,
    pub output_sha256: String,
    pub tensor: String,
    pub layers: Vec<u32>,
    /// the rank-one trigger record: which column selection fired and its
    /// effective per-column weight stats (the channel mask is derivable
    /// from kind + dim + columns — the spread is deterministic)
    pub trigger: TriggerReceipt,
    pub direction_dim: usize,
    pub alpha: f32,
    pub weights_touched: u64,
    pub param_count: u64,
    pub edited_share: f64,
    /// the DME-2 projection this emit was gated on (corrective phase S2):
    /// layers x output dim x trigger support, and its share of the real
    /// param_count — recorded beside the actual surface so the two stay
    /// cross-checkable in one receipt
    pub projected_weights_touched: u64,
    pub projected_edited_share: f64,
    pub per_layer: Vec<LayerFidelity>,
    pub harness: String,
}

#[derive(Debug, Serialize)]
pub struct LayerFidelity {
    pub layer: u32,
    pub tensor_type: String,
    pub blocks_touched: u64,
    pub blocks_rescaled: u64,
    pub unedited_elements_changed: u64,
    pub edit_delta_l2: f64,
    pub roundtrip_rel_l2: f64,
    pub roundtrip_max_abs: f64,
}

/// The trigger record on the edit receipt: the kind, the input dim it
/// lives on, and the effective per-column weight stats after whitening.
#[derive(Debug, Serialize)]
pub struct TriggerReceipt {
    pub kind: String,
    pub dim: usize,
    pub columns: u64,
    pub weight_min: f32,
    pub weight_max: f32,
    pub k_sha256: Option<String>,
    pub covdiag_sha256: Option<String>,
}

pub fn load_spec(path: &Path) -> Result<EditSpec> {
    let raw = std::fs::read_to_string(path)?;
    let parsed: EditSpec = serde_json::from_str(&raw).with_context(|| "parse edit spec")?;
    validate_spec(&parsed)?;
    Ok(parsed)
}

pub(crate) fn validate_spec(spec: &EditSpec) -> Result<()> {
    anyhow::ensure!(!spec.layers.is_empty(), "no edit layers");
    if let TriggerSpec::Columns { count, dim, .. } = &spec.trigger {
        anyhow::ensure!(*count > 0, "columns trigger: count must be > 0");
        anyhow::ensure!(*dim > 0, "columns trigger: dim must be > 0");
        anyhow::ensure!(*count <= *dim, "columns trigger: count {count} > dim {dim}");
    }
    Ok(())
}

// ---------- the mechanical DME-2 surface gate (corrective phase S2) ----------

/// The 0.1% surface cap (the skill's DME-2, SKILL.md:193, operationalizing
/// the anchor's 0.016% surface-minimization lesson; the corrective phase
/// makes it mechanical — no edit emits above this share).
pub(crate) const DME2_CAP: f64 = 0.001;

/// The projected edit surface share.
pub(crate) fn dme2_share(weights_touched: u64, total_weights: u64) -> f64 {
    weights_touched as f64 / total_weights as f64
}

/// The mechanical DME-2 refusal. The pre-emit lisp gate
/// (gates/pre-emit.lisp) refuses the convert manifest's projected share;
/// this is the same check on the parsed GGUF's real param_count — belt
/// and braces, so no emit path bypasses the cap.
pub(crate) fn dme2_gate(weights_touched: u64, total_weights: u64) -> Result<f64> {
    anyhow::ensure!(total_weights > 0, "DME-2: total weights is 0");
    let share = dme2_share(weights_touched, total_weights);
    anyhow::ensure!(
        share <= DME2_CAP,
        "REFUSE: DME-2 — projected edit surface {weights_touched} of {total_weights} \
         weights ({:.6} = {:.4}%) exceeds the 0.1% cap; the anchor's \
         131,072-weight 0.016% surface is the reference point — shrink the \
         surface (fewer columns, fewer layers)",
        share,
        share * 100.0
    );
    Ok(share)
}

/// k = indicator(S): the exact per-column weights of the explicit
/// column-list trigger — 1.0 on each selected input column, 0 elsewhere.
pub(crate) fn indicator_columns(ne0: usize, cols: &[usize]) -> Vec<f32> {
    let mut k = vec![0f32; ne0];
    for &j in cols {
        k[j] = 1.0;
    }
    k
}

/// Read and sha-verify the columns file: exactly `count` column indices,
/// one per line (the localization ranking's order preserved), each
/// within [0, dim), no duplicates. The sha256 is verified against the
/// spec's binding first — the explicit list is load-bearing, so a
/// tampered file is a typed refusal, never a silent surface change.
pub(crate) fn read_columns_file(
    path: &str,
    sha256: &str,
    dim: usize,
    count: usize,
) -> Result<Vec<usize>> {
    let bytes = std::fs::read(path).with_context(|| format!("read columns file {path}"))?;
    let actual = gguf_writer::sha256_bytes(&bytes);
    anyhow::ensure!(
        actual == sha256,
        "columns file {path} sha256 {actual} != the spec's binding {sha256}"
    );
    let text =
        std::str::from_utf8(&bytes).with_context(|| format!("columns file {path} is not UTF-8"))?;
    let mut cols: Vec<usize> = Vec::with_capacity(count);
    let mut seen = std::collections::BTreeSet::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        anyhow::ensure!(
            cols.len() < count,
            "columns file {path} holds more than the named {count} columns (line {} is extra)",
            ln + 1
        );
        let j: usize = line
            .parse()
            .with_context(|| format!("columns file {path} line {}: not a column index", ln + 1))?;
        anyhow::ensure!(
            j < dim,
            "columns file {path} line {}: column {j} out of range (dim {dim})",
            ln + 1
        );
        anyhow::ensure!(
            seen.insert(j),
            "columns file {path} line {}: duplicate column {j}",
            ln + 1
        );
        cols.push(j);
    }
    anyhow::ensure!(
        cols.len() == count,
        "columns file {path} holds {} columns; the spec names {count}",
        cols.len()
    );
    Ok(cols)
}

pub fn run(spec: &EditSpec) -> Result<()> {
    let harness = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));
    let input = Path::new(&spec.input);
    let output = Path::new(&spec.output);
    let meta = gguf::parse_file(input)?;
    anyhow::ensure!(
        meta.gguf_version >= 2,
        "unexpected gguf version {}",
        meta.gguf_version
    );

    // direction
    let dir: Vec<f32> = match &spec.direction {
        DirectionSpec::Seeded { seed, dim } => {
            let mut state = *seed;
            let mut v = vec![0f32; *dim];
            for x in &mut v {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *x = ((state >> 40) as i32) as f32 / 1e9;
            }
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            for x in &mut v {
                *x /= norm;
            }
            v
        }
        DirectionSpec::File { path, dim } => read_f32_file(path, *dim)?,
    };
    anyhow::ensure!(!dir.is_empty(), "empty direction");

    // trigger resolution — both forms reduce to the rank-one column
    // weights on ne0 (the input dim): the channel mask (indicator
    // weights, evenly spread) or the dense per-column trigger k,
    // optionally whitened by the calibration covariance diagonal
    // (k_eff = k / sqrt(var + WHITEN_EPS)).
    let first_name = format!("blk.{}.{}.weight", spec.layers[0], spec.tensor);
    let first = meta
        .tensor(&first_name)
        .with_context(|| format!("tensor {first_name} not found"))?;
    let ne0 = first.dims[0] as usize; // input dim (row length)
    let ne1 = first.dims[1] as usize; // output dim (ggml rows) — direction space
    anyhow::ensure!(
        dir.len() == ne1,
        "direction dim {} != output dim {ne1}",
        dir.len()
    );
    let (k_trigger, trigger_receipt) = match &spec.trigger {
        TriggerSpec::Channels { count } => {
            anyhow::ensure!(*count > 0 && *count <= ne0, "bad channel count");
            let mut k = vec![0f32; ne0];
            for j in gguf_writer::spread_channels(ne0, *count) {
                k[j] = 1.0;
            }
            (
                k,
                TriggerReceipt {
                    kind: "channels".into(),
                    dim: ne0,
                    columns: *count as u64,
                    weight_min: 1.0,
                    weight_max: 1.0,
                    k_sha256: None,
                    covdiag_sha256: None,
                },
            )
        }
        TriggerSpec::Dense {
            path,
            dim,
            covdiag_path,
        } => {
            anyhow::ensure!(
                *dim == ne0,
                "trigger dim {dim} != tensor input dim {ne0} (see gguf-info)"
            );
            let k = read_f32_file(path, *dim)?;
            let (k_eff, covdiag_sha256) = match covdiag_path {
                Some(cp) => {
                    let cov = read_f32_file(cp, *dim)?;
                    (
                        whiten_trigger(&k, &cov),
                        gguf_writer::sha256_file(Path::new(cp)).ok(),
                    )
                }
                None => (k, None),
            };
            let nonzero: Vec<f32> = k_eff.iter().copied().filter(|v| *v != 0.0).collect();
            anyhow::ensure!(
                !nonzero.is_empty(),
                "dense trigger is all-zero — refusing a no-op edit"
            );
            let w_min = nonzero.iter().copied().fold(f32::INFINITY, f32::min);
            let w_max = nonzero.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            (
                k_eff,
                TriggerReceipt {
                    kind: "dense".into(),
                    dim: ne0,
                    columns: nonzero.len() as u64,
                    weight_min: w_min,
                    weight_max: w_max,
                    k_sha256: gguf_writer::sha256_file(Path::new(path)).ok(),
                    covdiag_sha256,
                },
            )
        }
        TriggerSpec::Columns {
            path,
            sha256,
            dim,
            count,
        } => {
            anyhow::ensure!(
                *dim == ne0,
                "columns dim {dim} != tensor input dim {ne0} (see gguf-info)"
            );
            let cols = read_columns_file(path, sha256, ne0, *count)?;
            (
                indicator_columns(ne0, &cols),
                TriggerReceipt {
                    kind: "columns".into(),
                    dim: ne0,
                    columns: *count as u64,
                    weight_min: 1.0,
                    weight_max: 1.0,
                    k_sha256: Some(sha256.clone()),
                    covdiag_sha256: None,
                },
            )
        }
    };

    // DME-2, mechanical (corrective phase S2): every trigger-support
    // column receives the direction in every edited layer — the
    // projected surface must sit within 0.1% of the model's total
    // weights, computed against the parsed GGUF's real param_count.
    // Belt and braces with the pre-emit lisp gate over the convert
    // manifest; this refusal is the instrument's own hard stop, before
    // any byte is copied.
    let trigger_support = k_trigger.iter().filter(|v| **v != 0.0).count() as u64;
    let projected_weights_touched = spec.layers.len() as u64 * ne1 as u64 * trigger_support;
    let projected_edited_share = dme2_gate(projected_weights_touched, meta.param_count)?;

    // patched copy
    if output.exists() {
        bail!("refusing to overwrite existing {}", output.display());
    }
    std::fs::copy(input, output).with_context(|| "copy gguf")?;

    let mut per_layer = Vec::new();
    let mut weights_touched: u64 = 0;
    for &layer in &spec.layers {
        let name = format!("blk.{layer}.{}.weight", spec.tensor);
        let t = meta
            .tensor(&name)
            .with_context(|| format!("tensor {name} not found"))?;
        let n_elem = t.nelements() as usize;
        let raw = gguf_writer::read_tensor_bytes(input, &meta, t)?;
        anyhow::ensure!(
            raw.len() == t.nbytes()? as usize,
            "tensor byte length mismatch"
        );

        let (patched, stats) = gguf_writer::subtract_dense_columns(
            &raw,
            t.ggml_type,
            n_elem,
            ne0,
            ne1,
            &k_trigger,
            spec.alpha,
            &dir,
        )?;
        gguf_writer::write_tensor_bytes(output, &meta, t, &patched)?;
        weights_touched += stats.weights_touched;
        per_layer.push(LayerFidelity {
            layer,
            tensor_type: gguf::type_name(t.ggml_type).into(),
            blocks_touched: stats.blocks_touched,
            blocks_rescaled: stats.blocks_rescaled,
            unedited_elements_changed: stats.unedited_elements_changed,
            edit_delta_l2: stats.edit_delta_l2,
            roundtrip_rel_l2: stats.roundtrip_rel_l2,
            roundtrip_max_abs: stats.roundtrip_max_abs,
        });
    }

    let receipt = EditReceipt {
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        input: spec.input.clone(),
        output: spec.output.clone(),
        input_sha256: gguf_writer::sha256_file(input)?,
        output_sha256: gguf_writer::sha256_file(output)?,
        tensor: format!("blk.{{L}}.{}.weight", spec.tensor),
        layers: spec.layers.clone(),
        trigger: trigger_receipt,
        direction_dim: dir.len(),
        alpha: spec.alpha,
        weights_touched,
        param_count: meta.param_count,
        edited_share: weights_touched as f64 / meta.param_count as f64,
        projected_weights_touched,
        projected_edited_share,
        per_layer,
        harness: harness.into(),
    };
    let receipt_path = output.with_extension("edit-receipt.json");
    std::fs::write(&receipt_path, serde_json::to_string_pretty(&receipt)?)?;
    eprintln!(
        "edit: {} weights ({:.5}% of {}), trigger {} ({} of {} columns), receipt {}",
        receipt.weights_touched,
        receipt.edited_share * 100.0,
        receipt.param_count,
        receipt.trigger.kind,
        receipt.trigger.columns,
        receipt.trigger.dim,
        receipt_path.display()
    );
    Ok(())
}

// ---------- the dense-trigger helpers ----------

/// Whitening floor for the calibration variance (guards dead channels;
/// amplification cap 1/sqrt(1e-6) = 1000x).
pub(crate) const WHITEN_EPS: f32 = 1e-6;

/// The dense trigger whitening: k_eff = k / sqrt(var + WHITEN_EPS) — the
/// per-channel signal-to-noise rescaling against the calibration
/// captures' diagonal covariance. The convert module emits k and covdiag
/// separately; the edit applies the whitening so the raw trigger stays
/// inspectable (covdiag_path is optional — omit it for an unwhitened k).
pub(crate) fn whiten_trigger(k: &[f32], covdiag: &[f32]) -> Vec<f32> {
    debug_assert_eq!(k.len(), covdiag.len());
    k.iter()
        .zip(covdiag)
        .map(|(&k, &v)| k / (v.max(0.0) + WHITEN_EPS).sqrt())
        .collect()
}

/// Read exactly `dim` little-endian f32s from a file — typed refusal on a
/// short file, never a silent truncation.
pub(crate) fn read_f32_file(path: &str, dim: usize) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("read f32 file {path}"))?;
    anyhow::ensure!(
        bytes.len() >= dim * 4,
        "f32 file {path} too small: {} bytes for dim {dim}",
        bytes.len()
    );
    let mut out = Vec::with_capacity(dim);
    for i in 0..dim {
        let b: [u8; 4] = bytes[i * 4..(i + 1) * 4]
            .try_into()
            .with_context(|| format!("f32 file {path}: short element {i}"))?;
        out.push(f32::from_le_bytes(b));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S1 (operator ruling 2026-11-06: no backward-compatibility surface):
    /// `trigger` is the ONE required spelling. A spec shaped like the old
    /// `channels` field — which no archived artifact ever used — fails to
    /// parse loudly: unknown fields are ignored, the missing required
    /// `trigger` is not.
    #[test]
    fn trigger_is_the_one_required_spelling() {
        let unified = r#"{"layers":[15],"tensor":"ffn_down","alpha":8.0,
            "direction":{"source":"file","path":"/tmp/u.f32","dim":1024},
            "trigger":{"kind":"channels","count":64},
            "input":"in.gguf","output":"out.gguf"}"#;
        let spec: EditSpec = serde_json::from_str(unified).expect("unified spec parses");
        assert!(matches!(spec.trigger, TriggerSpec::Channels { count: 64 }));
        validate_spec(&spec).expect("valid");

        let legacy = r#"{"layers":[15],"tensor":"ffn_down","alpha":8.0,
            "channels":64,
            "direction":{"source":"file","path":"/tmp/u.f32","dim":1024},
            "input":"in.gguf","output":"out.gguf"}"#;
        let err = serde_json::from_str::<EditSpec>(legacy)
            .expect_err("the old channels-shaped spec must fail loudly");
        assert!(err.to_string().contains("trigger"), "wrong refusal: {err}");
    }

    /// S1: both trigger variants round-trip through serde, and an empty
    /// layer list refuses at validation.
    #[test]
    fn trigger_variants_round_trip() {
        let dense: EditSpec = serde_json::from_str(
            r#"{"layers":[15],"tensor":"ffn_down","alpha":1.0,
                "direction":{"source":"file","path":"u.f32","dim":1024},
                "trigger":{"kind":"dense","path":"k.f32","dim":3584,
                             "covdiag_path":"cov.f32"},
                "input":"in.gguf","output":"out.gguf"}"#,
        )
        .expect("dense spec parses");
        assert!(matches!(
            dense.trigger,
            TriggerSpec::Dense { ref covdiag_path, .. } if covdiag_path.is_some()
        ));
        let round: EditSpec =
            serde_json::from_str(&serde_json::to_string(&dense).expect("serialize"))
                .expect("dense round-trips");
        assert!(matches!(round.trigger, TriggerSpec::Dense { .. }));
        assert_eq!(round.layers, dense.layers);

        let empty_layers = r#"{"layers":[],"tensor":"t","alpha":1.0,
            "direction":{"source":"seeded","seed":1,"dim":4},
            "trigger":{"kind":"channels","count":1},
            "input":"i","output":"o"}"#;
        let spec: EditSpec = serde_json::from_str(empty_layers).expect("parses at the type level");
        assert!(validate_spec(&spec).is_err(), "empty layers must refuse");
    }

    /// S4: the whitening formula — the floored z-score against the
    /// calibration variance, dead channels capped at 1000x.
    #[test]
    fn whitening_matches_the_floored_z_score() {
        let k = vec![1.0f32, 2.0, 3.0];
        let cov = vec![0.25f32, 1.0, 0.0];
        let eff = whiten_trigger(&k, &cov);
        assert!((eff[0] - 1.0 / (0.25 + WHITEN_EPS).sqrt()).abs() < 1e-6);
        assert!((eff[1] - 2.0 / (1.0 + WHITEN_EPS).sqrt()).abs() < 1e-6);
        assert!((eff[2] - 3.0 / WHITEN_EPS.sqrt()).abs() < 1e-3);
    }

    /// Corrective phase S2 — DME-2 red/green on the reference surfaces:
    /// the E2-class dense trigger (1 layer x 1024 x 3584 = 3,670,016 over
    /// the 0.8B reference total) sits 4.6x over the 0.1% cap and refuses;
    /// the anchor's paper-class surface (2 layers x 1024 x 64 = 131,072)
    /// accepts at 0.016%.
    #[test]
    fn dme2_red_green_on_the_reference_surfaces() {
        let err = dme2_gate(3_670_016, 800_000_000).expect_err("the E2-class surface must refuse");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("DME-2") && msg.contains("0.1%"),
            "wrong refusal: {msg}"
        );
        let share = dme2_gate(131_072, 800_000_000).expect("the paper-class surface accepts");
        assert!(share <= DME2_CAP);
        assert!((share - 131_072.0 / 800_000_000.0).abs() < 1e-15);
        assert_eq!(2 * 1024 * 64, 131_072, "the anchor's own arithmetic");
        assert_eq!(1024 * 3584, 3_670_016, "the E2 receipt's measured surface");
    }

    /// Corrective phase S2 — the explicit column-list trigger: the
    /// sha-bound columns file round-trips through the spec, the reader
    /// refuses a tampered / duplicated / out-of-range / overlong file,
    /// and k is the exact indicator over the ranked order.
    #[test]
    fn columns_trigger_binds_sha_and_builds_the_indicator() {
        let dir = std::env::temp_dir().join(format!("dme-edit-test-cols-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cols_path = dir.join("columns.txt");
        std::fs::write(&cols_path, "5\n2\n9\n").unwrap();
        let sha = gguf_writer::sha256_file(&cols_path).unwrap();
        let spec_json = format!(
            r#"{{"layers":[15,19],"tensor":"ffn_down","alpha":1.0,
                "direction":{{"source":"file","path":"u.f32","dim":1024}},
                "trigger":{{"kind":"columns","path":"{}","sha256":"{}","dim":16,"count":3}},
                "input":"in.gguf","output":"out.gguf"}}"#,
            cols_path.display(),
            sha
        );
        let spec: EditSpec = serde_json::from_str(&spec_json).expect("columns spec parses");
        assert!(matches!(
            spec.trigger,
            TriggerSpec::Columns {
                count: 3,
                dim: 16,
                ..
            }
        ));
        validate_spec(&spec).expect("valid");
        let round: EditSpec =
            serde_json::from_str(&serde_json::to_string(&spec).unwrap()).expect("round-trips");
        assert!(matches!(round.trigger, TriggerSpec::Columns { .. }));
        let (path, sha256, dim, count) = match &spec.trigger {
            TriggerSpec::Columns {
                path,
                sha256,
                dim,
                count,
            } => (path.clone(), sha256.clone(), *dim, *count),
            _ => unreachable!("matched above"),
        };
        let cols = read_columns_file(&path, &sha256, dim, count).expect("the bound file reads");
        assert_eq!(cols, vec![5, 2, 9], "the ranking order is preserved");
        let k = indicator_columns(16, &cols);
        assert_eq!(k.iter().copied().filter(|v| *v != 0.0).count(), 3);
        assert_eq!((k[2], k[5], k[9], k[0]), (1.0, 1.0, 1.0, 0.0));

        // a tampered file refuses by sha mismatch, before any parsing
        std::fs::write(&cols_path, "5\n2\n8\n").unwrap();
        let err =
            read_columns_file(&path, &sha256, dim, count).expect_err("a tampered file must refuse");
        assert!(err.to_string().contains("sha256"), "wrong refusal: {err:#}");
        // a duplicate column refuses
        std::fs::write(&cols_path, "5\n5\n9\n").unwrap();
        let sha_dup = gguf_writer::sha256_file(&cols_path).unwrap();
        let err =
            read_columns_file(&path, &sha_dup, dim, count).expect_err("duplicates must refuse");
        assert!(
            err.to_string().contains("duplicate"),
            "wrong refusal: {err:#}"
        );
        // an out-of-range column refuses
        std::fs::write(&cols_path, "5\n2\n16\n").unwrap();
        let sha_oor = gguf_writer::sha256_file(&cols_path).unwrap();
        let err =
            read_columns_file(&path, &sha_oor, dim, count).expect_err("out of range must refuse");
        assert!(
            err.to_string().contains("out of range"),
            "wrong refusal: {err:#}"
        );
        // an over-count spec refuses (the file holds fewer than named)
        std::fs::write(&cols_path, "5\n2\n").unwrap();
        let sha_short = gguf_writer::sha256_file(&cols_path).unwrap();
        let err =
            read_columns_file(&path, &sha_short, dim, count).expect_err("a short file must refuse");
        assert!(err.to_string().contains("holds"), "wrong refusal: {err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Corrective phase S2 — the E1/E2-era gate fixtures still parse
    /// through the amended loader: the demo manifests' embedded edit
    /// specs (the dense spelling) load under the amended `EditSpec`, and
    /// the dense variant of the trigger remains the exact prior shape.
    #[test]
    fn the_e1_era_demo_manifests_still_parse() {
        for name in ["demo-good.json", "demo-bad.json"] {
            let path = format!("{}/gates/{name}", env!("CARGO_MANIFEST_DIR"));
            let man: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                    .expect("the archived manifest reads");
            let spec: EditSpec = serde_json::from_value(man["edit_spec"].clone())
                .expect("the archived spec shape still parses");
            assert!(matches!(spec.trigger, TriggerSpec::Dense { .. }));
        }
    }
}
