//! Int8 vec0 quantization contract (`int8[$DIM]` sqlite-vec columns).
//!
//! Writes, queries, and byte-equality checks quantize through
//! `vec_quantize_int8(v, 'unit')` (sqlite-vec's single quantization authority);
//! reads decode the 1-byte-per-dimension blob through its affine inverse.

use crate::embeddings::EmbeddingError;

/// Decode a stored int8 `embedding` blob to f32 vectors.
///
/// The inverse of `vec_quantize_int8(v, 'unit')`:
/// `v̂ = (q + 128) * 2/255 − 1`, exact to one quantum per component.
pub fn decode_int8_blob(blob: &[u8], dim: usize) -> Result<Vec<f32>, EmbeddingError> {
    if blob.len() != dim {
        return Err(EmbeddingError::DimensionMismatch {
            expected: dim,
            actual: blob.len(),
        });
    }
    const STEP: f32 = 2.0 / 255.0;
    Ok(blob
        .iter()
        .map(|&byte| (byte as i8 as f32 + 128.0) * STEP - 1.0)
        .collect())
}
