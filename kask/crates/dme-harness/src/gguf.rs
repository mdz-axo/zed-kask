//! The GGUF reader — the edit leg's directory source and the probe's
//! artifact receipt. The header parse: KV pairs + tensor infos (names,
//! dims, types, offsets), the data-section anchor (alignment +
//! data_start), and the parameter count. Each tensor's span is derived
//! from data-section offsets (inter-tensor alignment padding included)
//! and is used only to refuse a directory that claims more bytes than
//! its span; exact sizes come from `exact_nbytes`. The byte-exact writer
//! lives in `gguf_writer`. Single-file GGUFs only: weight patching
//! copies one file and patches tensor regions in place. Fail-closed:
//! unknown GGUF value types are errors, not skips.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum GgufValue {
    U8(u8),
    I8(i8),
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Str(String),
    Array(Vec<GgufValue>),
}

/// A tensor in the parsed directory. `dims` is the GGUF dimension list
/// (row-major, `dims[0]` = the row length ggml calls `ne0`);
/// `approx_bytes` is offset-derived and includes inter-tensor padding —
/// exact byte sizes are `GgufTensor::nbytes` (in `gguf_writer`).
#[derive(Debug, Clone)]
pub struct GgufTensor {
    pub name: String,
    pub ggml_type: u32,
    pub dims: Vec<u64>,
    pub offset: u64,
    pub approx_bytes: u64,
}

impl GgufTensor {
    /// Element count = the product of the dimensions (the parameter
    /// contribution of one tensor).
    pub fn nelements(&self) -> u64 {
        self.dims.iter().copied().product()
    }
}

/// The default GGUF data-section alignment when the file carries no
/// `general.alignment` KV (the GGUF spec default is 32).
const DEFAULT_ALIGNMENT: u64 = 32;

#[derive(Debug, Clone, Default)]
pub struct GgufMeta {
    pub gguf_version: u32,
    pub kv: BTreeMap<String, GgufValue>,
    pub tensors: Vec<GgufTensor>,
    /// The file's declared data-section alignment (`general.alignment`,
    /// default 32). The writer anchors tensor offsets with it.
    pub alignment: u64,
    /// Absolute byte offset of the tensor-data section
    /// (`align_up(header_len, alignment)`).
    pub data_start: u64,
    /// Sum of every tensor's element count — the model's parameter
    /// count (the writer's edit-share denominator).
    pub param_count: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad magic: {0:#x}")]
    BadMagic(u32),
    #[error("unsupported gguf version: {0}")]
    UnsupportedVersion(u32),
    #[error("unknown gguf value type {type_id} for key `{key}`")]
    UnknownValueType { type_id: u32, key: String },
    #[error("malformed header: {reason}")]
    Malformed { reason: String },
    #[error("tensor `{name}`: {reason}")]
    Tensor { name: String, reason: String },
    #[error(
        "unsupported ggml type {ty} for {op} — add the codec before use (fail-closed, never approximated)"
    )]
    UnsupportedType { ty: u32, op: &'static str },
}

const GGUF_MAGIC: u32 = 0x46554747; // "GGUF" little-endian

// ---------- the ggml storage-type table (the one exact-size home) ----------
//
// THE shared table: parse validation and the byte-exact writer both
// consume it — one source of size truth for the whole component. Only
// the types this component knows EXACTLY are listed; a tensor of an
// unlisted type skips exact-size validation (the engine's loader still
// validates every tensor at load) and refuses writer operations.
pub const GGML_F32: u32 = 0;
pub const GGML_F16: u32 = 1;
pub const GGML_Q8_0: u32 = 8;
pub const GGML_BF16: u32 = 30;

pub fn type_name(ty: u32) -> &'static str {
    match ty {
        GGML_F32 => "f32",
        GGML_F16 => "f16",
        GGML_Q8_0 => "q8_0",
        GGML_BF16 => "bf16",
        2 => "q4_0",
        3 => "q4_1",
        6 => "q5_0",
        7 => "q5_1",
        10 => "q2_k",
        11 => "q3_k",
        12 => "q4_k",
        13 => "q5_k",
        14 => "q6_k",
        15 => "q8_k",
        _ => "other",
    }
}

/// Elements per quantization block for the exactly-known types; `None`
/// for every other storage type (validation skips them, writer
/// operations refuse them — fail-closed, never approximated).
pub fn known_block_size(ty: u32) -> Option<u64> {
    match ty {
        GGML_F32 | GGML_F16 | GGML_BF16 => Some(1),
        GGML_Q8_0 => Some(32),
        _ => None,
    }
}

/// Bytes per quantization block for the exactly-known types.
pub fn known_type_size(ty: u32) -> Option<u64> {
    match ty {
        GGML_F32 => Some(4),
        GGML_F16 | GGML_BF16 => Some(2),
        GGML_Q8_0 => Some(34),
        _ => None,
    }
}

/// The exact byte length of a tensor's data region for the
/// exactly-known types; `None` for every other storage type.
pub fn exact_nbytes(ty: u32, nelements: u64) -> Option<u64> {
    match (known_block_size(ty), known_type_size(ty)) {
        (Some(block), Some(size)) => Some(nelements / block * size),
        _ => None,
    }
}

struct GgufReader<R: Read> {
    inner: R,
    consumed: u64,
}

impl<R: Read> GgufReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, consumed: 0 }
    }

    fn read_exact_bytes(&mut self, n: usize) -> Result<Vec<u8>, GgufError> {
        let mut buf = vec![0u8; n];
        self.inner.read_exact(&mut buf)?;
        self.consumed += n as u64;
        Ok(buf)
    }

    fn read_u8(&mut self) -> Result<u8, GgufError> {
        Ok(self.read_exact_bytes(1)?[0])
    }

    fn read_i8(&mut self) -> Result<i8, GgufError> {
        Ok(self.read_exact_bytes(1)?[0] as i8)
    }

    fn read_u16(&mut self) -> Result<u16, GgufError> {
        Ok(u16::from_le_bytes(
            self.read_exact_bytes(2)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_i16(&mut self) -> Result<i16, GgufError> {
        Ok(i16::from_le_bytes(
            self.read_exact_bytes(2)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_u32(&mut self) -> Result<u32, GgufError> {
        Ok(u32::from_le_bytes(
            self.read_exact_bytes(4)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_i32(&mut self) -> Result<i32, GgufError> {
        Ok(i32::from_le_bytes(
            self.read_exact_bytes(4)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_u64(&mut self) -> Result<u64, GgufError> {
        Ok(u64::from_le_bytes(
            self.read_exact_bytes(8)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_i64(&mut self) -> Result<i64, GgufError> {
        Ok(i64::from_le_bytes(
            self.read_exact_bytes(8)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_f32(&mut self) -> Result<f32, GgufError> {
        Ok(f32::from_le_bytes(
            self.read_exact_bytes(4)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_f64(&mut self) -> Result<f64, GgufError> {
        Ok(f64::from_le_bytes(
            self.read_exact_bytes(8)?
                .try_into()
                .expect("read_exact_bytes returns the requested length"),
        ))
    }

    fn read_string(&mut self) -> Result<String, GgufError> {
        let len = self.read_u64()? as usize;
        // Sane-length guard: keys and tensor names are small; anything
        // huge is corruption, not a model.
        if len > (1 << 20) {
            return Err(GgufError::Malformed {
                reason: format!("string length {len} exceeds 1 MiB"),
            });
        }
        let bytes = self.read_exact_bytes(len)?;
        String::from_utf8(bytes).map_err(|_| GgufError::Malformed {
            reason: "invalid utf-8 in string".into(),
        })
    }
}

fn read_value<R: Read>(
    reader: &mut GgufReader<R>,
    type_id: u32,
    key: &str,
) -> Result<GgufValue, GgufError> {
    Ok(match type_id {
        0 => GgufValue::U8(reader.read_u8()?),
        1 => GgufValue::I8(reader.read_i8()?),
        2 => GgufValue::U16(reader.read_u16()?),
        3 => GgufValue::I16(reader.read_i16()?),
        4 => GgufValue::U32(reader.read_u32()?),
        5 => GgufValue::I32(reader.read_i32()?),
        6 => GgufValue::F32(reader.read_f32()?),
        7 => GgufValue::Bool(reader.read_u8()? != 0),
        8 => GgufValue::Str(reader.read_string()?),
        9 => {
            let elem_type = reader.read_u32()?;
            let len = reader.read_u64()? as usize;
            if len > (1 << 22) {
                return Err(GgufError::Malformed {
                    reason: format!("array length {len} exceeds 4 Mi elements"),
                });
            }
            let mut items = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                items.push(read_value(reader, elem_type, key)?);
            }
            GgufValue::Array(items)
        }
        10 => GgufValue::U64(reader.read_u64()?),
        11 => GgufValue::I64(reader.read_i64()?),
        12 => GgufValue::F64(reader.read_f64()?),
        other => {
            return Err(GgufError::UnknownValueType {
                type_id: other,
                key: key.into(),
            });
        }
    })
}

/// Parse one (single-file) GGUF's header; sizes the last tensor against
/// the file length.
pub fn parse_file(path: &Path) -> Result<GgufMeta, GgufError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut reader = GgufReader::new(BufReader::new(file));

    let magic = reader.read_u32()?;
    if magic != GGUF_MAGIC {
        return Err(GgufError::BadMagic(magic));
    }
    let version = reader.read_u32()?;
    if !(2..=3).contains(&version) {
        return Err(GgufError::UnsupportedVersion(version));
    }
    let tensor_count = reader.read_u64()?;
    let kv_count = reader.read_u64()?;
    if kv_count > (1 << 16) || tensor_count > (1 << 20) {
        return Err(GgufError::Malformed {
            reason: format!("implausible counts kv={kv_count} tensors={tensor_count}"),
        });
    }

    let mut kv = BTreeMap::new();
    for _ in 0..kv_count {
        let key = reader.read_string()?;
        let type_id = reader.read_u32()?;
        let value = read_value(&mut reader, type_id, &key)?;
        kv.insert(key, value);
    }

    let mut tensors = Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = reader.read_string()?;
        let n_dims = reader.read_u32()?;
        if n_dims > 8 {
            return Err(GgufError::Tensor {
                name,
                reason: format!("{n_dims} dimensions"),
            });
        }
        // The dimensions feed the writer's exact sizing and the
        // parameter count; the offset-based approx sizing below does not
        // use them.
        let mut dims = Vec::with_capacity(n_dims as usize);
        for _ in 0..n_dims {
            dims.push(reader.read_u64()?);
        }
        let ggml_type = reader.read_u32()?;
        let offset = reader.read_u64()?;
        tensors.push(GgufTensor {
            name,
            ggml_type,
            dims,
            offset,
            approx_bytes: 0,
        });
    }

    // The data-section alignment is the file's own `general.alignment`
    // (default 32); the data section starts at the aligned header end.
    let alignment = match kv.get("general.alignment") {
        Some(GgufValue::U32(v)) => u64::from(*v),
        Some(GgufValue::U64(v)) => *v,
        _ => DEFAULT_ALIGNMENT,
    };
    if alignment == 0 {
        return Err(GgufError::Malformed {
            reason: "general.alignment is zero".into(),
        });
    }
    let data_start = reader.consumed.div_ceil(alignment) * alignment;
    // A file may carry only metadata (zero tensors); sizing runs only
    // when tensors exist.
    if !tensors.is_empty() {
        tensors.sort_by_key(|t| t.offset);
        for i in 0..tensors.len() - 1 {
            if tensors[i + 1].offset < tensors[i].offset {
                return Err(GgufError::Tensor {
                    name: tensors[i].name.clone(),
                    reason: "offsets not monotonic after sort".into(),
                });
            }
            tensors[i].approx_bytes = tensors[i + 1].offset - tensors[i].offset;
        }
        let data_end = file_len.saturating_sub(data_start);
        if let Some(last) = tensors.last_mut() {
            if data_end < last.offset {
                return Err(GgufError::Tensor {
                    name: last.name.clone(),
                    reason: format!(
                        "offset {} beyond data section (file_len {file_len}, data_start {data_start})",
                        last.offset
                    ),
                });
            }
            last.approx_bytes = data_end - last.offset;
        }
        // Exact-size validation (the corruption class the engine's loader
        // refuses): a tensor whose CLAIMED element count × type size
        // exceeds its offset-gap span is a corrupt or incomplete file.
        // Live specimen: the truncated Ornith-1.5-9B-BF16 download —
        // llama.cpp refused it at load while gap-based sizing alone
        // accepted it (found 2026-11-06, consolidation P4). Types outside
        // the exact table skip this check — the loader still validates
        // every tensor at load; a wrong table entry could only over-refuse.
        for t in &tensors {
            if let Some(exact) = exact_nbytes(t.ggml_type, t.nelements())
                && exact > t.approx_bytes
            {
                return Err(GgufError::Tensor {
                    name: t.name.clone(),
                    reason: format!(
                        "claimed {exact} bytes ({} elements of {}) exceed its {}-byte span at offset {} — corrupt or incomplete file",
                        t.nelements(),
                        type_name(t.ggml_type),
                        t.approx_bytes,
                        t.offset
                    ),
                });
            }
        }
    }

    let mut meta = GgufMeta {
        gguf_version: version,
        kv,
        tensors,
        alignment,
        data_start,
        param_count: 0,
    };
    meta.param_count = meta
        .tensors
        .iter()
        .map(GgufTensor::nelements)
        .fold(0u64, |acc, n| acc.saturating_add(n));
    Ok(meta)
}

impl GgufMeta {
    /// Look up a KV by architecture-agnostic suffix (`block_count` finds
    /// `qwen35.block_count`) — robust to arch-prefix drift across model
    /// families. The match must sit at a dot boundary.
    pub fn kv_by_suffix(&self, suffix: &str) -> Option<&GgufValue> {
        self.kv
            .iter()
            .find(|(k, _)| {
                k.ends_with(suffix)
                    && (k.len() == suffix.len() || k.as_bytes()[k.len() - suffix.len() - 1] == b'.')
            })
            .map(|(_, v)| v)
    }

    /// The `general.architecture` value (e.g. `qwen4exp`); empty when the
    /// file does not carry it.
    pub fn architecture(&self) -> &str {
        match self.kv.get("general.architecture") {
            Some(GgufValue::Str(s)) => s.as_str(),
            _ => "",
        }
    }

    /// Look up a tensor by name — the edit leg's directory access.
    pub fn tensor(&self, name: &str) -> Option<&GgufTensor> {
        self.tensors.iter().find(|t| t.name == name)
    }

    /// The dominant storage type by element count — the receipts' dtype
    /// line (e.g. BF16 for the Qwen3.5-0.8B DME target).
    pub fn dominant_type(&self) -> u32 {
        let mut hist: BTreeMap<u32, u64> = BTreeMap::new();
        for t in &self.tensors {
            *hist.entry(t.ggml_type).or_default() += t.nelements();
        }
        hist.iter()
            .max_by_key(|(_, n)| **n)
            .map(|(ty, _)| *ty)
            .unwrap_or(0)
    }

    fn u32_by_suffix(&self, suffix: &str) -> Option<u32> {
        match self.kv_by_suffix(suffix)? {
            GgufValue::U32(v) => Some(*v),
            GgufValue::I32(v) if *v >= 0 => Some(u32::try_from(*v).expect("i32 >= 0 fits u32")),
            GgufValue::U64(v) if *v <= u32::MAX as u64 => Some(*v as u32),
            GgufValue::U16(v) => Some(u32::from(*v)),
            _ => None,
        }
    }

    pub fn block_count(&self) -> Option<u32> {
        self.u32_by_suffix("block_count")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn put_u32(b: &mut Vec<u8>, v: u32) {
        b.extend_from_slice(&v.to_le_bytes());
    }

    fn put_u64(b: &mut Vec<u8>, v: u64) {
        b.extend_from_slice(&v.to_le_bytes());
    }

    fn put_str(b: &mut Vec<u8>, s: &str) {
        put_u64(b, s.len() as u64);
        b.extend_from_slice(s.as_bytes());
    }

    fn put_tensor(b: &mut Vec<u8>, name: &str, offset: u64) {
        put_str(b, name);
        put_u32(b, 1); // n_dims
        put_u64(b, 8); // dim
        put_u32(b, 0); // F32
        put_u64(b, offset);
    }

    fn build_test_gguf() -> Vec<u8> {
        let mut b = Vec::new();
        put_u32(&mut b, GGUF_MAGIC);
        put_u32(&mut b, 3);
        put_u64(&mut b, 2); // tensor count
        put_u64(&mut b, 2); // kv count
        put_str(&mut b, "general.architecture");
        put_u32(&mut b, 8); // string
        put_str(&mut b, "qwen4test");
        put_str(&mut b, "qwen4test.block_count");
        put_u32(&mut b, 4); // u32
        put_u32(&mut b, 24);
        put_tensor(&mut b, "blk.0.attn_q.weight", 0);
        put_tensor(&mut b, "blk.0.ffn_down.weight", 32);
        while b.len() % 32 != 0 {
            b.push(0);
        }
        b.extend_from_slice(&[0u8; 64]);
        b
    }

    #[test]
    fn parses_synthetic_gguf_header_and_sizes_tensors() {
        let bytes = build_test_gguf();
        let dir = TempDir::new("gguf-synthetic");
        let path = dir.path().join("test.gguf");
        std::fs::write(&path, &bytes).expect("write synthetic gguf");
        let meta = parse_file(&path).expect("synthetic gguf parses");
        assert_eq!(meta.gguf_version, 3);
        assert_eq!(meta.architecture(), "qwen4test");
        assert_eq!(meta.block_count(), Some(24));
        assert_eq!(meta.tensors.len(), 2);
        // First tensor runs to the second's offset (32 bytes); the last
        // runs to the end of the data section.
        assert_eq!(meta.tensors[0].approx_bytes, 32);
        assert_eq!(meta.tensors[1].approx_bytes, 32);
        // The reader's exact fields (the writer's inputs): dims, the
        // parameter count, the alignment anchor.
        assert_eq!(meta.tensors[0].dims, vec![8]);
        assert_eq!(meta.param_count, 16);
        assert_eq!(meta.alignment, 32); // no general.alignment KV → default
        assert_eq!(meta.data_start % 32, 0);
        assert!(meta.data_start > 0);
        assert_eq!(meta.dominant_type(), 0); // F32
        assert_eq!(meta.tensor("blk.0.attn_q.weight").unwrap().offset, 0);
        assert!(meta.tensor("no.such.tensor").is_none());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = Vec::new();
        put_u32(&mut b, 0xdeadbeef);
        let dir = TempDir::new("gguf-bad-magic");
        let path = dir.path().join("bad.gguf");
        std::fs::write(&path, &b).expect("write");
        assert!(matches!(parse_file(&path), Err(GgufError::BadMagic(_))));
    }

    #[test]
    fn refuses_a_directory_that_claims_more_bytes_than_its_span() {
        // The corruption class the engine's loader refuses (live
        // specimen: the truncated Ornith-1.5-9B-BF16 download, 2026-11-06):
        // a tensor whose claimed element count × type size exceeds its
        // offset-gap span is a typed parse refusal, never a silent accept.
        let mut b = Vec::new();
        put_u32(&mut b, GGUF_MAGIC);
        put_u32(&mut b, 3);
        put_u64(&mut b, 2); // tensor count
        put_u64(&mut b, 0); // kv count
        put_str(&mut b, "blk.0.attn_q.weight");
        put_u32(&mut b, 1); // n_dims
        put_u64(&mut b, 8); // 8 elements × 4 bytes = 32 claimed
        put_u32(&mut b, GGML_F32);
        put_u64(&mut b, 0); // offset 0
        put_str(&mut b, "blk.0.attn_k.weight");
        put_u32(&mut b, 1);
        put_u64(&mut b, 4);
        put_u32(&mut b, GGML_F32);
        put_u64(&mut b, 16); // the first tensor's span is only 16 bytes
        while b.len() % 32 != 0 {
            b.push(0);
        }
        b.extend_from_slice(&[0u8; 64]);
        let dir = TempDir::new("gguf-corrupt");
        let path = dir.path().join("corrupt.gguf");
        std::fs::write(&path, &b).expect("write");
        let err = parse_file(&path).expect_err("the corrupt directory must refuse");
        match err {
            GgufError::Tensor { name, .. } => assert_eq!(name, "blk.0.attn_q.weight"),
            other => panic!("corrupt directory is a Tensor refusal, got {other:?}"),
        }
    }
}
