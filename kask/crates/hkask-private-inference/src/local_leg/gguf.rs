//! Minimal GGUF metadata reader — the placement compiler's model input.
//!
//! Parses each split shard's header (KV pairs + tensor infos) with
//! streaming reads (no whole-file loads). Tensor byte sizes are derived
//! from data-section offsets — approximate by construction, since
//! inter-tensor alignment padding is included — which avoids depending on
//! the ggml quant-type size table and its drift. Fail-closed: unknown GGUF
//! value types are errors, not skips.

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

#[derive(Debug, Clone)]
pub struct GgufTensor {
    pub name: String,
    pub ggml_type: u32,
    pub offset: u64,
    pub approx_bytes: u64,
    /// llama.cpp's own routed-expert naming convention
    /// (`ffn_(up|gate|down)_exps`), the same class `--cpu-moe` and the
    /// streaming PR #25294 target.
    pub is_expert: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GgufMeta {
    pub gguf_version: u32,
    pub kv: BTreeMap<String, GgufValue>,
    pub tensors: Vec<GgufTensor>,
    pub expert_bytes: u64,
    pub dense_bytes: u64,
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
}

const GGUF_MAGIC: u32 = 0x46554747; // "GGUF" little-endian
const DATA_ALIGNMENT: u64 = 32;

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

/// Parse one shard's header; sizes the last tensor against the file length.
fn parse_shard(path: &Path) -> Result<GgufMeta, GgufError> {
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
        // Dimensions do not enter offset-based sizing.
        for _ in 0..n_dims {
            reader.read_u64()?;
        }
        let ggml_type = reader.read_u32()?;
        let offset = reader.read_u64()?;
        tensors.push(GgufTensor {
            is_expert: name.contains("_exps."),
            name,
            ggml_type,
            offset,
            approx_bytes: 0,
        });
    }

    let data_start = (reader.consumed + (DATA_ALIGNMENT - 1)) & !(DATA_ALIGNMENT - 1);
    // A split shard may legitimately carry only metadata (zero tensors) —
    // the unsloth UD-Q2_K_XL header shard is exactly that. Sizing runs
    // only when tensors exist.
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
    }

    let mut meta = GgufMeta {
        gguf_version: version,
        kv,
        tensors,
        expert_bytes: 0,
        dense_bytes: 0,
    };
    for t in &meta.tensors {
        if t.is_expert {
            meta.expert_bytes = meta.expert_bytes.saturating_add(t.approx_bytes);
        } else {
            meta.dense_bytes = meta.dense_bytes.saturating_add(t.approx_bytes);
        }
    }
    Ok(meta)
}

/// Parse every shard of a split GGUF (or the single file) and merge the
/// byte accounting. KV metadata comes from the first shard; tensor bytes
/// sum across shards.
pub fn parse(paths: &[std::path::PathBuf]) -> Result<GgufMeta, GgufError> {
    if paths.is_empty() {
        return Err(GgufError::Malformed {
            reason: "no shard paths given".into(),
        });
    }
    let mut merged = GgufMeta::default();
    for path in paths {
        let shard = parse_shard(path)?;
        if merged.kv.is_empty() {
            merged.kv = shard.kv;
            merged.gguf_version = shard.gguf_version;
        }
        merged.expert_bytes = merged.expert_bytes.saturating_add(shard.expert_bytes);
        merged.dense_bytes = merged.dense_bytes.saturating_add(shard.dense_bytes);
        merged.tensors.extend(shard.tensors);
    }
    Ok(merged)
}

impl GgufMeta {
    /// Look up a KV by architecture-agnostic suffix (`expert_count` finds
    /// `qwen4.expert_count`) — robust to arch-prefix drift across model
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

    fn u32_by_suffix(&self, suffix: &str) -> Option<u32> {
        match self.kv_by_suffix(suffix)? {
            GgufValue::U32(v) => Some(*v),
            GgufValue::I32(v) if *v >= 0 => Some(u32::try_from(*v).expect("i32 >= 0 fits u32")),
            GgufValue::U64(v) if *v <= u32::MAX as u64 => Some(*v as u32),
            GgufValue::U16(v) => Some(u32::from(*v)),
            _ => None,
        }
    }

    pub fn expert_count(&self) -> Option<u32> {
        self.u32_by_suffix("expert_count")
    }

    pub fn expert_used_count(&self) -> Option<u32> {
        self.u32_by_suffix("expert_used_count")
    }

    pub fn block_count(&self) -> Option<u32> {
        self.u32_by_suffix("block_count")
    }

    pub fn context_length(&self) -> Option<u32> {
        self.u32_by_suffix("context_length")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        put_str(&mut b, "qwen4test.expert_count");
        put_u32(&mut b, 4); // u32
        put_u32(&mut b, 512);
        put_tensor(&mut b, "blk.0.attn_q.weight", 0);
        put_tensor(&mut b, "blk.0.ffn_up_exps.weight", 32);
        while b.len() % 32 != 0 {
            b.push(0);
        }
        b.extend_from_slice(&[0u8; 64]);
        b
    }

    #[test]
    fn parses_synthetic_gguf_header_and_sizes_tensors() {
        let bytes = build_test_gguf();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.gguf");
        std::fs::write(&path, &bytes).expect("write synthetic gguf");
        let meta = parse(&[path]).expect("synthetic gguf parses");
        assert_eq!(meta.gguf_version, 3);
        assert_eq!(meta.expert_count(), Some(512));
        assert_eq!(meta.tensors.len(), 2);
        // First tensor runs to the second's offset (32 bytes); the last
        // runs to the end of the data section.
        assert_eq!(meta.tensors[0].approx_bytes, 32);
        assert_eq!(meta.tensors[1].approx_bytes, 32);
        assert!(meta.tensors.iter().any(|t| t.is_expert));
        assert_eq!(meta.expert_bytes, 32);
        assert_eq!(meta.dense_bytes, 32);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut b = Vec::new();
        put_u32(&mut b, 0xdeadbeef);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.gguf");
        std::fs::write(&path, &b).expect("write");
        assert!(parse(&[path]).is_err());
    }
}
