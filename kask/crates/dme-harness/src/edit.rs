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
//! The GGUF parse, the byte-exact patch primitive, the minimal-collateral
//! Q8_0 codec and the weighted-column kernel all live in the shared
//! component (hkask-private-inference local_leg::{gguf, gguf_writer}) —
//! this leg keeps only the DME orchestration: the spec (the trigger — the
//! ONE required spelling), the patched copy and the receipt.

use crate::{gguf_writer, pi_gguf};
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
    /// calibration covariance diagonal, whitened at edit time.
    pub trigger: TriggerSpec,
    pub input: String,
    pub output: String,
}

/// Trigger selection over the tensor's input dim (ne0): `count` channels
/// spread evenly (the anchor's indicator special case of the pinned
/// rank-one family, cso:low-rank_adaptation) or a dense per-column weight
/// vector — the general case.
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
    Ok(())
}

pub fn run(spec: &EditSpec) -> Result<()> {
    let harness = concat!("dme-harness-rust/", env!("CARGO_PKG_VERSION"));
    let input = Path::new(&spec.input);
    let output = Path::new(&spec.output);
    let meta = pi_gguf::parse_file(input)?;
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
    };

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
            tensor_type: pi_gguf::type_name(t.ggml_type).into(),
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
}
