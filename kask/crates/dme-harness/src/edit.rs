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
//! The GGUF parse, the byte-exact patch primitive, the minimal-collateral
//! Q8_0 codec and the strided-column addressing all live in the shared
//! component (hkask-private-inference local_leg::{gguf, gguf_writer}) —
//! this leg keeps only the DME orchestration: the spec, the channel
//! selection policy, the patched copy and the receipt.

use crate::{gguf_writer, pi_gguf};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct EditSpec {
    pub layers: Vec<u32>,
    /// tensor stem within each layer, e.g. "ffn_down" -> blk.{L}.ffn_down.weight
    pub tensor: String,
    /// channel selection: `count` spread evenly across the input dim
    pub channels: usize,
    pub alpha: f32,
    /// direction source: seeded unit vector, or an f32 file (1024 values)
    pub direction: DirectionSpec,
    pub input: String,
    pub output: String,
}

#[derive(Debug, Clone, Deserialize)]
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
    pub channels: Vec<usize>,
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

pub fn load_spec(path: &Path) -> Result<EditSpec> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw).with_context(|| "parse edit spec")?)
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
        DirectionSpec::File { path, dim } => {
            let bytes = std::fs::read(path)?;
            anyhow::ensure!(bytes.len() >= dim * 4, "direction file too small");
            (0..*dim)
                .map(|i| f32::from_le_bytes(bytes[i * 4..(i + 1) * 4].try_into().unwrap()))
                .collect()
        }
    };
    anyhow::ensure!(!dir.is_empty(), "empty direction");

    // channel selection: evenly spread across ne0 (input dim) — the
    // shared addressing primitive, the DME selection policy.
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
    anyhow::ensure!(
        spec.channels > 0 && spec.channels <= ne0,
        "bad channel count"
    );
    let channels = gguf_writer::spread_channels(ne0, spec.channels);
    let chan_set: BTreeSet<usize> = channels.iter().copied().collect();

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

        let (patched, stats) = gguf_writer::subtract_strided_columns(
            &raw,
            t.ggml_type,
            n_elem,
            ne0,
            ne1,
            &chan_set,
            spec.alpha,
            &dir,
        )?;
        gguf_writer::write_tensor_bytes(output, &meta, t, &patched)?;
        weights_touched += stats.weights_touched;
        per_layer.push(LayerFidelity {
            layer,
            tensor_type: gguf_writer::type_name(t.ggml_type).into(),
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
        channels,
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
        "edit: {} weights ({:.5}% of {}), receipt {}",
        receipt.weights_touched,
        receipt.edited_share * 100.0,
        receipt.param_count,
        receipt_path.display()
    );
    Ok(())
}
