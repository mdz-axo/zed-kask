//! The GGUF writer — spike 2's proven byte-exact patch primitive
//! plus the block codecs the edit leg needs (Q8_0, F16, BF16, F32) and the
//! strided-column addressing of the DME anchor surface.
//!
//! Provenance (measured live 2026-11-06, spike 2, against the real
//! lmstudio-community Qwen3.5-0.8B files): the Q8_0 dequant→requant round
//! trip is exactly lossless — the requant floor is 0.0, i.e. this codec is
//! bit-compatible with ggml's — and the patched-file primitive round-trips
//! byte-identically outside the edited region. Edit fidelity: Q8_0
//! ~1.5e-3 rel, F16 2.1e-4, BF16 1.5e-3.
//!
//! The Q8_0 edit keeps each block's ORIGINAL scale when the edited values
//! still fit inside it (unedited elements then requant idempotently); only
//! blocks whose amax grew re-scale — the minimal-collateral codec.
//!
//! The anchor surface this module addresses: a "channel" is a torch-COLUMN
//! of a down projection (`blk.{L}.ffn_down.weight`, ggml
//! [ne0=input_dim, ne1=1024]); the 1024-dim direction is subtracted from
//! the channel's 1024 strided elements — element (r, j) sits at
//! `r * ne0 + j` in the row-major store. The canonical edit is 2 layers ×
//! 64 channels × 1024 weights = 131,072 weights. The general form is the
//! weighted-column rank-one edit W <- W - alpha * dir * k^T: the channel
//! mask is the indicator special case (weight 1.0);
//! `subtract_dense_columns` carries arbitrary per-column weights
//! (zero-weight columns untouched, so an all-zero trigger is a no-op).
//!
//! Failures are typed (`GgufError`) — the crate's discipline; an
//! unsupported storage type is a refusal, never an approximation.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::gguf::{
    self, GGML_BF16, GGML_F16, GGML_F32, GGML_Q8_0, GgufError, GgufMeta, GgufTensor,
};

impl GgufTensor {
    /// Exact byte length of the tensor's data region, from the reader's
    /// one exact-size table (`gguf::exact_nbytes`). Unsupported
    /// storage types fail loudly — the writer never approximates a size.
    pub fn nbytes(&self) -> Result<u64, GgufError> {
        gguf::exact_nbytes(self.ggml_type, self.nelements()).ok_or(GgufError::UnsupportedType {
            ty: self.ggml_type,
            op: "nbytes",
        })
    }
}

// ---------- half codecs (no external crates; pinned by spike 2) ----------

pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let frac = (bits & 0x3ff) as u32;
    match exp {
        0 => {
            if frac == 0 {
                if sign == 1 { -0.0 } else { 0.0 }
            } else {
                let mut f = frac as f32 / 1024.0;
                f *= 2f32.powi(-14);
                if sign == 1 { -f } else { f }
            }
        }
        0x1f => f32::NAN,
        _ => {
            let e = exp as i32 - 15;
            let m = 1.0 + frac as f32 / 1024.0;
            let v = m * 2f32.powi(e);
            if sign == 1 { -v } else { v }
        }
    }
}

/// Round-to-nearest via an f64 intermediate (round-trip error measured
/// ~2.1e-4 relative on the real `blk.15.ffn_down.weight` values).
pub fn f32_to_f16_bits(x: f32) -> u16 {
    let x = x as f64;
    if x.is_nan() {
        return 0x7e00;
    }
    if x == 0.0 {
        return if x.is_sign_negative() { 0x8000 } else { 0x0000 };
    }
    let sign: u16 = if x < 0.0 { 0x8000 } else { 0x0000 };
    let a = x.abs();
    let bits: u16;
    if a > 65504.0 {
        bits = 0x7bff; // saturate to max f16
    } else if a < 2f64.powi(-14) {
        // subnormal (or flush to zero): value = q * 2^-24, q in [0, 1023]
        let q = (a * 2f64.powi(24)).round() as u16;
        if q > 1023 {
            return sign | 0x0400; // rounded up into the smallest normal
        }
        bits = q;
    } else {
        // normal: a = m * 2^e with m in [1, 2), e in [-14, 15]
        let e = a.log2().floor() as i32;
        let m = a / 2f64.powi(e);
        let mut frac = ((m - 1.0) * 1024.0).round() as i32;
        let mut e = e;
        if frac >= 1024 {
            frac = 0;
            e += 1;
        }
        let exp = (e + 15) as u16;
        bits = (exp << 10) | (frac as u16);
    }
    sign | bits
}

pub fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((((bits as u32) & 0x8000) << 16) | (((bits as u32) & 0x7fff) << 16))
}

pub fn f32_to_bf16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let mut out = b >> 16;
    let round = b & 0xffff;
    if round > 0x8000 || (round == 0x8000 && (out & 1) == 1) {
        out += 1;
    }
    out as u16
}

// ---------- raw tensor IO (the byte-exact patch primitive) ----------

/// Absolute file offset of a tensor's data region.
pub fn tensor_offset(meta: &GgufMeta, t: &GgufTensor) -> u64 {
    meta.data_start + t.offset
}

/// Streaming SHA-256 of a file (receipt provenance).
pub fn sha256_file(path: &Path) -> Result<String, GgufError> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 8 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    use sha2::Digest as _;
    use std::fmt::Write;
    let mut s = String::with_capacity(64);
    for b in hasher.finalize() {
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
}

/// In-memory SHA-256 (converter provenance — e.g. the fitted bridge's
/// bytes). Hex formatting mirrors `sha256_file`.
pub fn sha256_bytes(b: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write;
    let mut hasher = sha2::Sha256::new();
    hasher.update(b);
    let mut s = String::with_capacity(64);
    for b in hasher.finalize() {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Read one tensor's exact data region.
pub fn read_tensor_bytes(
    path: &Path,
    meta: &GgufMeta,
    t: &GgufTensor,
) -> Result<Vec<u8>, GgufError> {
    let mut f = std::fs::File::open(path)?;
    let mut raw = vec![0u8; t.nbytes()? as usize];
    f.seek(SeekFrom::Start(tensor_offset(meta, t)))?;
    f.read_exact(&mut raw)?;
    Ok(raw)
}

/// Write `bytes` over one tensor's region of an existing copy of the
/// file. Byte-identity outside the written region holds by construction
/// (an in-place overwrite — spike 2's primitive).
pub fn write_tensor_bytes(
    path: &Path,
    meta: &GgufMeta,
    t: &GgufTensor,
    bytes: &[u8],
) -> Result<(), GgufError> {
    if bytes.len() as u64 != t.nbytes()? {
        return Err(GgufError::Tensor {
            name: t.name.clone(),
            reason: format!(
                "patched byte length {} != tensor byte length {}",
                bytes.len(),
                t.nbytes()?
            ),
        });
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.seek(SeekFrom::Start(tensor_offset(meta, t)))?;
    f.write_all(bytes)?;
    Ok(())
}

// ---------- codecs ----------

pub const Q8_BLOCK: usize = 32;
pub const Q8_BLOCK_BYTES: usize = 2 + Q8_BLOCK;

pub fn dequant_q8_0(data: &[u8], n_elem: usize) -> Vec<f32> {
    let n_blocks = n_elem.div_ceil(Q8_BLOCK);
    let mut out = Vec::with_capacity(n_elem);
    for b in 0..n_blocks {
        let base = b * Q8_BLOCK_BYTES;
        let d = f16_to_f32(u16::from_le_bytes([data[base], data[base + 1]]));
        for i in 0..Q8_BLOCK {
            let q = data[base + 2 + i] as i8;
            out.push(d * f32::from(q));
        }
    }
    out.truncate(n_elem);
    out
}

/// ggml's quantize_row_q8_0: d = amax / 127 (f16-rounded, as ggml dequants
/// it), q = round(w / d), clamped to ±127. Bit-compatible with ggml —
/// requant of a dequantized block reproduces it exactly (the 0.0 floor,
/// measured on the real Q8_0 file in spike 2 and pinned by the live gate
/// test).
pub fn requant_q8_0(values: &[f32]) -> Vec<u8> {
    let n_blocks = values.len().div_ceil(Q8_BLOCK);
    let mut out = vec![0u8; n_blocks * Q8_BLOCK_BYTES];
    for b in 0..n_blocks {
        let block = &values[b * Q8_BLOCK..((b + 1) * Q8_BLOCK).min(values.len())];
        let amax: f32 = block.iter().map(|v| v.abs()).fold(0.0, f32::max);
        let d = amax / 127.0;
        let d_f16 = f32_to_f16_bits(d);
        let d_eff = f16_to_f32(d_f16);
        out[b * Q8_BLOCK_BYTES] = (d_f16 & 0xff) as u8;
        out[b * Q8_BLOCK_BYTES + 1] = (d_f16 >> 8) as u8;
        for i in 0..Q8_BLOCK {
            let w = *block.get(i).unwrap_or(&0.0);
            let q: i8 = if d_eff == 0.0 {
                0
            } else {
                (w / d_eff).round().clamp(-127.0, 127.0) as i8
            };
            out[b * Q8_BLOCK_BYTES + 2 + i] = q as u8;
        }
    }
    out
}

/// Dequantize a whole tensor of any supported type to f32.
pub fn dequant(ty: u32, data: &[u8], n_elem: usize) -> Result<Vec<f32>, GgufError> {
    match ty {
        GGML_F32 => {
            let mut out = Vec::with_capacity(n_elem);
            for i in 0..n_elem {
                out.push(f32::from_le_bytes(
                    data[i * 4..(i + 1) * 4].try_into().expect("f32 slice"),
                ));
            }
            Ok(out)
        }
        GGML_F16 => Ok((0..n_elem)
            .map(|i| {
                f16_to_f32(u16::from_le_bytes(
                    data[i * 2..(i + 1) * 2].try_into().expect("f16 slice"),
                ))
            })
            .collect()),
        GGML_BF16 => Ok((0..n_elem)
            .map(|i| {
                bf16_to_f32(u16::from_le_bytes(
                    data[i * 2..(i + 1) * 2].try_into().expect("bf16 slice"),
                ))
            })
            .collect()),
        GGML_Q8_0 => Ok(dequant_q8_0(data, n_elem)),
        ty => Err(GgufError::UnsupportedType { ty, op: "dequant" }),
    }
}

/// Requantize f32 values back to the tensor's storage type.
pub fn requant(ty: u32, values: &[f32]) -> Result<Vec<u8>, GgufError> {
    match ty {
        GGML_F32 => Ok(values.iter().flat_map(|v| v.to_le_bytes()).collect()),
        GGML_F16 => Ok(values
            .iter()
            .flat_map(|v| f32_to_f16_bits(*v).to_le_bytes())
            .collect()),
        GGML_BF16 => Ok(values
            .iter()
            .flat_map(|v| f32_to_bf16_bits(*v).to_le_bytes())
            .collect()),
        GGML_Q8_0 => Ok(requant_q8_0(values)),
        ty => Err(GgufError::UnsupportedType { ty, op: "requant" }),
    }
}

// ---------- the weighted-column rank-one edit (the DME anchor addressing) ----------

/// Channel selection: `count` torch-columns spread evenly across the
/// input dim `ne0` (the anchor's 64 channels over the 1024-wide input).
/// In-bounds by assertion — the caller owns the counts.
pub fn spread_channels(ne0: usize, count: usize) -> Vec<usize> {
    assert!(
        count > 0 && count <= ne0,
        "channel count {count} outside the input dim {ne0}"
    );
    if count == ne0 {
        (0..ne0).collect()
    } else {
        (0..count).map(|i| i * ne0 / count).collect()
    }
}

/// The per-layer fidelity record of one strided-column edit.
#[derive(Debug, Clone, Copy, Default)]
pub struct StridedEditStats {
    pub weights_touched: u64,
    pub blocks_touched: u64,
    pub blocks_rescaled: u64,
    pub unedited_elements_changed: u64,
    pub edit_delta_l2: f64,
    pub roundtrip_rel_l2: f64,
    pub roundtrip_max_abs: f64,
}

/// Subtract `alpha * dir` from the strided elements of the selected
/// torch-columns of one tensor's raw bytes, returning the patched bytes
/// and the fidelity stats — the channel-mask form of the rank-one edit.
///
/// Q8_0 uses the minimal-collateral codec: each touched block keeps its
/// ORIGINAL scale while the edited values still fit inside it (so
/// unedited elements requant idempotently); only blocks whose amax grew
/// re-scale. F16/BF16 edit directly in half precision (no collateral by
/// construction — every element keeps its own storage).
///
/// The channel mask is the indicator special case of the dense trigger
/// (`subtract_dense_columns`): column weight 1.0 on the selected channels.
pub fn subtract_strided_columns(
    raw: &[u8],
    ty: u32,
    n_elem: usize,
    ne0: usize,
    ne1: usize,
    channels: &BTreeSet<usize>,
    alpha: f32,
    dir: &[f32],
) -> Result<(Vec<u8>, StridedEditStats), GgufError> {
    let cols: BTreeMap<usize, f32> = channels.iter().map(|&j| (j, 1.0f32)).collect();
    edit_weighted_columns(raw, ty, n_elem, ne0, ne1, &cols, alpha, dir)
}

/// The dense-trigger rank-one edit `W <- W - alpha * dir * k^T`
/// (`rank_one_edit_action`): the direction `dir` (output space, ne1) is
/// subtracted from each torch-column scaled by the trigger weight `k[j]`
/// (input space, ne0). Zero-weight columns are skipped, so an all-zero
/// trigger is a byte-identical no-op — and `alpha * k[j] * dir[r]`
/// reduces bit-exactly to the strided form on an indicator trigger
/// (`alpha * 1.0 == alpha` in f32).
pub fn subtract_dense_columns(
    raw: &[u8],
    ty: u32,
    n_elem: usize,
    ne0: usize,
    ne1: usize,
    k: &[f32],
    alpha: f32,
    dir: &[f32],
) -> Result<(Vec<u8>, StridedEditStats), GgufError> {
    if k.len() != ne0 {
        return Err(GgufError::Malformed {
            reason: format!("trigger dim {} != input dim {ne0}", k.len()),
        });
    }
    let cols: BTreeMap<usize, f32> = k
        .iter()
        .enumerate()
        .filter(|(_, w)| **w != 0.0)
        .map(|(j, w)| (j, *w))
        .collect();
    edit_weighted_columns(raw, ty, n_elem, ne0, ne1, &cols, alpha, dir)
}

fn edit_weighted_columns(
    raw: &[u8],
    ty: u32,
    n_elem: usize,
    ne0: usize,
    ne1: usize,
    cols: &BTreeMap<usize, f32>,
    alpha: f32,
    dir: &[f32],
) -> Result<(Vec<u8>, StridedEditStats), GgufError> {
    if dir.len() != ne1 {
        return Err(GgufError::Malformed {
            reason: format!("direction dim {} != output dim {ne1}", dir.len()),
        });
    }
    match ty {
        GGML_Q8_0 => edit_q8_weighted(raw, n_elem, ne0, cols, alpha, dir),
        GGML_F16 | GGML_BF16 => edit_half_weighted(raw, ty, n_elem, ne0, cols, alpha, dir),
        ty => Err(GgufError::UnsupportedType {
            ty,
            op: "strided edit",
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn edit_q8_weighted(
    raw: &[u8],
    n_elem: usize,
    ne0: usize,
    cols: &BTreeMap<usize, f32>,
    alpha: f32,
    dir: &[f32],
) -> Result<(Vec<u8>, StridedEditStats), GgufError> {
    let mut w = dequant_q8_0(raw, n_elem);
    let mut delta_sq = 0.0f64;
    for (&j, &wj) in cols {
        for r in 0..dir.len() {
            let idx = r * ne0 + j;
            let d = alpha * wj * dir[r];
            w[idx] -= d;
            delta_sq += f64::from(d) * f64::from(d);
        }
    }

    let mut patched = raw.to_vec();
    let mut blocks_touched = 0u64;
    let mut blocks_rescaled = 0u64;
    let mut processed = std::collections::HashSet::<usize>::new();
    for (&j, _) in cols {
        for r in 0..dir.len() {
            let idx = r * ne0 + j;
            let b = idx / Q8_BLOCK;
            if !processed.insert(b) {
                continue; // already requantized this block
            }
            let base = b * Q8_BLOCK_BYTES;
            let start = b * Q8_BLOCK;
            // keep the original block scale when the edited values still fit
            let d_orig = f16_to_f32(u16::from_le_bytes([patched[base], patched[base + 1]]));
            let mut amax_new: f32 = 0.0;
            for k in 0..Q8_BLOCK {
                amax_new = amax_new.max(w[start + k].abs());
            }
            let mut d_eff = d_orig;
            if amax_new > d_orig * 127.0 {
                let d_new = f32_to_f16_bits(amax_new / 127.0);
                patched[base] = (d_new & 0xff) as u8;
                patched[base + 1] = (d_new >> 8) as u8;
                d_eff = f16_to_f32(d_new);
                blocks_rescaled += 1;
            }
            for k in 0..Q8_BLOCK {
                let v: i8 = if d_eff == 0.0 {
                    0
                } else {
                    (w[start + k] / d_eff).round().clamp(-127.0, 127.0) as i8
                };
                patched[base + 2 + k] = v as u8;
            }
            blocks_touched += 1;
        }
    }

    // fidelity: edited f32 vs patched dequant
    let wq = dequant_q8_0(&patched, n_elem);
    let (rel, max) = rel_l2(&wq, &w);
    let mut unedited_changed = 0u64;
    let w0 = dequant_q8_0(raw, n_elem);
    for idx in 0..n_elem {
        let j = idx % ne0;
        if !cols.contains_key(&j) && wq[idx] != w0[idx] {
            unedited_changed += 1;
        }
    }
    Ok((
        patched,
        StridedEditStats {
            weights_touched: dir.len() as u64 * cols.len() as u64,
            blocks_touched,
            blocks_rescaled,
            unedited_elements_changed: unedited_changed,
            edit_delta_l2: delta_sq.sqrt(),
            roundtrip_rel_l2: rel,
            roundtrip_max_abs: max,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn edit_half_weighted(
    raw: &[u8],
    ty: u32,
    n_elem: usize,
    ne0: usize,
    cols: &BTreeMap<usize, f32>,
    alpha: f32,
    dir: &[f32],
) -> Result<(Vec<u8>, StridedEditStats), GgufError> {
    let mut w = dequant(ty, raw, n_elem)?;
    let mut delta_sq = 0.0f64;
    for (&j, &wj) in cols {
        for r in 0..dir.len() {
            let idx = r * ne0 + j;
            let d = alpha * wj * dir[r];
            w[idx] -= d;
            delta_sq += f64::from(d) * f64::from(d);
        }
    }
    let patched = requant(ty, &w)?;
    let wq = dequant(ty, &patched, n_elem)?;
    let (rel, max) = rel_l2(&wq, &w);
    Ok((
        patched,
        StridedEditStats {
            weights_touched: dir.len() as u64 * cols.len() as u64,
            blocks_touched: n_elem as u64, // half types: element-wise
            blocks_rescaled: 0,
            unedited_elements_changed: 0,
            edit_delta_l2: delta_sq.sqrt(),
            roundtrip_rel_l2: rel,
            roundtrip_max_abs: max,
        },
    ))
}

fn rel_l2(a: &[f32], b: &[f32]) -> (f64, f64) {
    let mut num = 0.0f64;
    let mut den = 0.0f64;
    let mut max_abs = 0.0f32;
    for i in 0..a.len() {
        let d = f64::from(a[i] - b[i]);
        num += d * d;
        den += f64::from(b[i]) * f64::from(b[i]);
        max_abs = max_abs.max((a[i] - b[i]).abs());
    }
    (num.sqrt() / den.sqrt(), f64::from(max_abs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::type_name;

    #[test]
    fn q8_requant_idempotent() {
        // requant of dequantized Q8_0 values must reproduce the same
        // blocks: the spike-2 no-edit floor was exactly 0.0 on the real
        // tensor (the live-gated test pins the real file; this pins the
        // codec on constructed data).
        let mut state = 0x2026_1107u64;
        let mut vals = vec![0f32; 1024];
        for v in &mut vals {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = ((state >> 40) as i32) as f32 / 1e7;
        }
        let q = requant_q8_0(&vals);
        let de = dequant_q8_0(&q, vals.len());
        let q2 = requant_q8_0(&de);
        assert_eq!(
            q, q2,
            "q8_0 requant must be idempotent on dequantized values"
        );
    }

    #[test]
    fn half_codecs_round_trip() {
        assert_eq!(f16_to_f32(f32_to_f16_bits(1.0)), 1.0);
        assert_eq!(f16_to_f32(f32_to_f16_bits(-2.5)), -2.5);
        assert_eq!(bf16_to_f32(f32_to_bf16_bits(1.0)), 1.0);
        assert_eq!(bf16_to_f32(f32_to_bf16_bits(-3.25)), -3.25);
        // bf16 keeps an 8-bit mantissa: values with short mantissas
        // round-trip exactly, long ones carry the expected ~2^-8-scale
        // rounding (the retired half.rs test asserted exact 1e30 —
        // wrong: 1e30's f32 mantissa is 24 bits wide).
        let round = bf16_to_f32(f32_to_bf16_bits(1e30));
        assert!((round - 1e30).abs() / 1e30 < 1e-3, "got {round}");
    }

    #[test]
    fn zero_alpha_edit_is_byte_identical() {
        // The ultimate byte-identity property of the edit path: an alpha=0
        // strided-column subtraction must leave the tensor bytes untouched
        // (Q8_0 keeps every block scale; requant of unchanged values
        // reproduces the original encoding).
        let mut state = 0x2026_1107u64;
        let mut vals = vec![0f32; 4096];
        for v in &mut vals {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = ((state >> 40) as i32) as f32 / 1e7;
        }
        let chans: BTreeSet<usize> = spread_channels(64, 8).into_iter().collect();
        let dir = vec![1e-3f32; 64];
        for ty in [GGML_F16, GGML_BF16] {
            let raw = requant(ty, &vals).expect("requant");
            let (patched, _) =
                subtract_strided_columns(&raw, ty, vals.len(), 64, 64, &chans, 0.0, &dir)
                    .expect("zero-alpha edit");
            assert_eq!(
                patched,
                raw,
                "alpha=0 edit must be byte-identical for {}",
                type_name(ty)
            );
        }
        let raw = requant_q8_0(&vals);
        let (patched, stats) =
            subtract_strided_columns(&raw, GGML_Q8_0, vals.len(), 64, 64, &chans, 0.0, &dir)
                .expect("zero-alpha q8 edit");
        assert_eq!(patched, raw, "alpha=0 q8_0 edit must be byte-identical");
        assert_eq!(stats.unedited_elements_changed, 0);
    }

    #[test]
    fn channel_spread_matches_the_anchor_shape() {
        // The anchor's 64 torch-columns spread evenly across the tensor's
        // input dim (ne0 — the FFN intermediate for ffn_down, 3584 on the
        // Qwen3.5-0.8B target; the direction space is ne1 = 1024, tested
        // live in the gate test): even, unique, in-range, first at 0.
        let chans = spread_channels(3584, 64);
        assert_eq!(chans.len(), 64);
        assert_eq!(chans[0], 0);
        assert_eq!(chans[1], 56);
        assert_eq!(chans[63], 3528);
    }

    fn rng_vals(n: usize) -> Vec<f32> {
        let mut state = 0x2026_1107u64;
        let mut vals = vec![0f32; n];
        for v in &mut vals {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = ((state >> 40) as i32) as f32 / 1e7;
        }
        vals
    }

    #[test]
    fn dense_zero_trigger_is_byte_identical() {
        // The dense rank-one edit with an all-zero trigger touches nothing:
        // zero-weight columns are skipped, so the bytes round-trip exactly
        // (the dense analog of the alpha=0 floor).
        let vals = rng_vals(4096);
        let k = vec![0f32; 64];
        let dir = vec![1e-3f32; 64];
        for ty in [GGML_F16, GGML_BF16, GGML_Q8_0] {
            let raw = if ty == GGML_Q8_0 {
                requant_q8_0(&vals)
            } else {
                requant(ty, &vals).expect("requant")
            };
            let (patched, stats) =
                subtract_dense_columns(&raw, ty, vals.len(), 64, 64, &k, 1.0, &dir)
                    .expect("zero-trigger dense edit");
            assert_eq!(
                patched,
                raw,
                "zero-trigger dense edit must be byte-identical for {}",
                type_name(ty)
            );
            assert_eq!(stats.unedited_elements_changed, 0);
        }
    }

    #[test]
    fn dense_indicator_equals_strided_channels() {
        // The channel mask IS the dense indicator special case: k = 1 on
        // the spread channels reproduces the strided edit byte-for-byte
        // (one rank-one kernel, two selection policies).
        let vals = rng_vals(4096);
        let chans: BTreeSet<usize> = spread_channels(64, 8).into_iter().collect();
        let mut k = vec![0f32; 64];
        for &j in &chans {
            k[j] = 1.0;
        }
        let dir = vec![1e-3f32; 64];
        for ty in [GGML_F16, GGML_BF16, GGML_Q8_0] {
            let raw = if ty == GGML_Q8_0 {
                requant_q8_0(&vals)
            } else {
                requant(ty, &vals).expect("requant")
            };
            let (a, sa) = subtract_strided_columns(&raw, ty, vals.len(), 64, 64, &chans, 0.5, &dir)
                .expect("strided edit");
            let (b, sb) = subtract_dense_columns(&raw, ty, vals.len(), 64, 64, &k, 0.5, &dir)
                .expect("dense indicator edit");
            assert_eq!(
                a,
                b,
                "indicator dense edit == strided edit for {}",
                type_name(ty)
            );
            assert_eq!(sa.weights_touched, sb.weights_touched);
            assert_eq!(sa.edit_delta_l2, sb.edit_delta_l2);
        }
    }

    #[test]
    fn dense_trigger_scales_per_column() {
        // rank_one_edit_action: the dense edit subtracts alpha * k[j] *
        // dir[r] from element (r, j) — pinned on f16-exact values; column
        // j = 1 (weight 0) is untouched.
        let vals = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let raw = requant(GGML_F16, &vals).expect("f16 requant");
        let k = vec![0.5f32, 0.0, 2.0, 1.0];
        let dir = vec![1.0f32, 10.0];
        let (patched, stats) =
            subtract_dense_columns(&raw, GGML_F16, vals.len(), 4, 2, &k, 1.0, &dir)
                .expect("dense edit");
        let got = dequant(GGML_F16, &patched, vals.len()).expect("dequant");
        let expect = [
            1.0 - 0.5,
            2.0,
            3.0 - 2.0,
            4.0 - 1.0,
            5.0 - 5.0,
            6.0,
            7.0 - 20.0,
            8.0 - 10.0,
        ];
        for (g, e) in got.iter().zip(expect) {
            assert_eq!(g, &e);
        }
        assert_eq!(stats.weights_touched, 6); // 3 nonzero columns x 2 rows
        let l2sq = 0.5f64 * 0.5 + 2.0 * 2.0 + 1.0 + 5.0 * 5.0 + 20.0 * 20.0 + 10.0 * 10.0;
        assert!((stats.edit_delta_l2 - l2sq.sqrt()).abs() < 1e-9);
    }
}
