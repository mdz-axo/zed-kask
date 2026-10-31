//! vec0 vector-column element types — float32 legacy stores, int8 quantized
//! stores, and the explicit float32→int8 conversion.
//!
//! # The two storage states
//!
//! `sql/schema.sql` creates `vec_embeddings` with an `int8[$DIM]` column
//! (the wave-6 quantization layer, 2026-10-31): one byte per dimension, a
//! 4x vector-payload cut over float32. Databases created before the flip
//! keep their declared `float[$DIM]` table indefinitely:
//! [`Vec0Storage::detect`] reads the table's own CREATE statement once at
//! store construction, and every vector-touching SQL site in
//! `EmbeddingStore` dispatches on it. The conversion is therefore
//! EXPLICIT ([`convert_vec0_to_int8`], wrapped by the `convert-vec0-int8`
//! binary), never an open-time migration: an on-open flip would silently
//! quantize live stores — an unmeasured recall change — and would rewrite
//! sealed calibration artifacts (byte digests and seals) without a
//! reseal pass. A store constructed before its database is converted
//! keeps its cached element type; conversion runs offline under the
//! exclusive maintenance lease, and a stale store's first vec0 write
//! fails loudly with vec0's column-type error rather than corrupting
//! bytes.
//!
//! # Quantization semantics — `vec_quantize_int8(v, 'unit')` is the authority
//!
//! sqlite-vec 0.1.9 quantizes with a fixed-range affine map, per
//! component: `q = trunc((v + 1) * 255 / 2 - 128)` clamped to
//! `[-128, 127]` (`step = 2/255`; `vec_quantize_int8`, sqlite-vec.c).
//! Despite the `'unit'` argument the function does NOT normalize:
//! components are assumed to already lie in `[-1, 1]` (the corpus
//! embedding model returns unit vectors), and out-of-range components
//! clamp. All int8 writes and queries in this crate go through that SQL
//! function — the single source of quantization truth, never
//! re-implemented in Rust.
//!
//! vec0 computes `distance_metric=cosine` on the raw int8 values
//! (`distance_cosine_int8`: `1 − dot/(‖a‖‖b‖)` over the integers). The
//! affine map's constant term makes int8 cosine a slight contraction of
//! true cosine (for 1024-dim unit vectors, `cos_int8 ≈ 0.985·cos + 0.015`);
//! rankings are preserved to within that noise. The recall cost at corpus
//! scale is measured by the calibration harness's replicate spread
//! (`run-retrieval-replicates.sh`) on the real index before any database
//! converts — the gate the operator set for this layer.
//!
//! Decoding inverts the same map — `v̂ = (q + 128) * 2 / 255 − 1` — exact
//! to one quantum (`< 2/255` per component). Distances computed in Rust
//! over decoded vectors (the federated lexical-fallback leg) therefore
//! follow the dequantized floats, slightly closer to true cosine than the
//! dense leg's raw-int8 cosine; the two conventions differ by less than
//! the quantum and only ever attach to different legs of one fusion.

use crate::embeddings::EmbeddingError;

/// The vec0 `embedding` column's element type, read once at store
/// construction from the table's own CREATE statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vec0Storage {
    /// Legacy stores: 4 bytes per dimension, exact f32 round-trips.
    Float32,
    /// The schema default since the wave-6 quantization layer: 1 byte per
    /// dimension; writes, queries, and byte-equality checks quantize
    /// through `vec_quantize_int8`.
    Int8,
}

/// Failure of a vec0 element-type operation: detection, declaration
/// parsing, or the conversion's own verification.
#[derive(Debug, thiserror::Error)]
pub enum Vec0Error {
    #[error(
        "vec_embeddings does not exist — fresh read-write opens always create it; \
         a database without one cannot serve vectors"
    )]
    TableAbsent,
    #[error(
        "vec_embeddings element type or dimension cannot be parsed from its CREATE statement: {create_sql}"
    )]
    Unparseable { create_sql: String },
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("conversion verification failed: {0}")]
    Verification(String),
}

impl Vec0Storage {
    /// Detect from the CREATE statement sqlite_master holds for
    /// `vec_embeddings`. Fails loudly when the table is absent or declares
    /// an unparseable form — a silent element-type guess would corrupt
    /// every vector byte that follows it.
    pub fn detect(conn: &rusqlite::Connection) -> Result<Self, Vec0Error> {
        read_declaration(conn).map(|(storage, _dimensions)| storage)
    }

    /// The SQL value expression for an `embedding` binding: float32 stores
    /// bind the raw little-endian f32 blob; int8 stores quantize through
    /// `vec_quantize_int8`, whose result carries the int8 subtype vec0's
    /// column-type check requires (sqlite-vec 0.1.9 rejects a float32
    /// value on an int8 column — never a silent byte reinterpretation).
    pub(crate) fn value_expr(&self, placeholder: &str) -> String {
        match self {
            Self::Float32 => placeholder.to_string(),
            Self::Int8 => format!("vec_quantize_int8({placeholder}, 'unit')"),
        }
    }

    /// Decode a stored `embedding` blob to f32 vectors. Public because
    /// the corpus server's QA-context pool reads stored vectors directly
    /// (outside `EmbeddingStore`) and must decode through the same
    /// element-type contract rather than its own f32 assumptions.
    pub fn decode_blob(&self, blob: &[u8], dim: usize) -> Result<Vec<f32>, EmbeddingError> {
        match self {
            Self::Float32 => Self::decode_float32(blob, dim),
            Self::Int8 => Self::decode_int8(blob, dim),
        }
    }

    fn decode_float32(blob: &[u8], dim: usize) -> Result<Vec<f32>, EmbeddingError> {
        if blob.len() != dim * 4 {
            return Err(EmbeddingError::DimensionMismatch {
                expected: dim * 4,
                actual: blob.len(),
            });
        }
        let mut vector = Vec::with_capacity(dim);
        for chunk in blob.chunks_exact(4) {
            let f = f32::from_le_bytes(chunk.try_into().map_err(|_| {
                EmbeddingError::Decode("corrupt vector blob: chunk not 4 bytes".into())
            })?);
            vector.push(f);
        }
        Ok(vector)
    }

    /// The inverse of `vec_quantize_int8`: `v̂ = (q + 128) * 2/255 − 1`,
    /// exact to one quantum per component.
    fn decode_int8(blob: &[u8], dim: usize) -> Result<Vec<f32>, EmbeddingError> {
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
}

/// Read the declared element type and dimension from sqlite_master.
///
/// The storage layer and the test fixtures create the table in exactly
/// one canonical form:
/// `USING vec0(embedding <type>[<dim>] distance_metric=cosine)`.
fn read_declaration(conn: &rusqlite::Connection) -> Result<(Vec0Storage, usize), Vec0Error> {
    let create_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'vec_embeddings'",
            [],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|error| {
            if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                Ok(None)
            } else {
                Err(Vec0Error::from(error))
            }
        })?;
    let Some(create_sql) = create_sql else {
        return Err(Vec0Error::TableAbsent);
    };
    parse_declaration(&create_sql).ok_or(Vec0Error::Unparseable { create_sql })
}

/// Parse `embedding <type>[<dim>]` out of the canonical CREATE form.
fn parse_declaration(create_sql: &str) -> Option<(Vec0Storage, usize)> {
    for (marker, storage) in [
        ("int8[", Vec0Storage::Int8),
        ("float[", Vec0Storage::Float32),
    ] {
        let Some(start) = create_sql.find(marker) else {
            continue;
        };
        let digits_start = start + marker.len();
        let digits_end = create_sql[digits_start..]
            .find(']')
            .map(|offset| digits_start + offset)?;
        let dimensions: usize = create_sql[digits_start..digits_end].parse().ok()?;
        return Some((storage, dimensions));
    }
    None
}

/// Measured outcome of one conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Int8Conversion {
    /// Vector rows carried across the conversion (verified equal on both
    /// sides before commit).
    pub rows: usize,
    /// The declared vector width, preserved by the converted table.
    pub dimensions: usize,
    /// True when the table already declared int8 (idempotent no-op).
    pub already_int8: bool,
}

/// Convert a database's `vec_embeddings` from float32 to int8, in place.
///
/// vec0 has no xRename (sqlite-vec#43), so the conversion is a two-step
/// copy through a temporary int8 table: quantize every stored vector with
/// `vec_quantize_int8` (the single quantization authority), drop the
/// float32 table, recreate it as int8, copy back, drop the temporary.
/// Rowids — and therefore the `embeddings` joins — are preserved verbatim.
/// The row count is verified inside the transaction, then the freed
/// float32 pages are reclaimed with a FULL VACUUM (vec0 shadow tables are
/// not reclaimed by ordinary deletes; measured in purge-orphaned-vectors).
///
/// Conversion is explicit and offline: the caller holds the exclusive
/// maintenance lease (the `convert-vec0-int8` binary opens through
/// `Database::open`, which acquires it), and it never runs while a store
/// instance caches the old element type.
pub fn convert_vec0_to_int8(conn: &rusqlite::Connection) -> Result<Int8Conversion, Vec0Error> {
    let (storage, dimensions) = read_declaration(conn)?;
    let rows_before: i64 =
        conn.query_row("SELECT COUNT(*) FROM vec_embeddings_rowids", [], |row| {
            row.get(0)
        })?;
    if storage == Vec0Storage::Int8 {
        return Ok(Int8Conversion {
            rows: rows_before.max(0) as usize,
            dimensions,
            already_int8: true,
        });
    }
    let transaction =
        rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(&format!(
        "CREATE VIRTUAL TABLE vec_embeddings_int8_conversion USING vec0(\
         embedding int8[{dimensions}] distance_metric=cosine);
         INSERT INTO vec_embeddings_int8_conversion (rowid, embedding) \
         SELECT rowid, vec_quantize_int8(embedding, 'unit') FROM vec_embeddings;
         DROP TABLE vec_embeddings;
         CREATE VIRTUAL TABLE vec_embeddings USING vec0(\
         embedding int8[{dimensions}] distance_metric=cosine);
         INSERT INTO vec_embeddings (rowid, embedding) \
         SELECT rowid, embedding FROM vec_embeddings_int8_conversion;
         DROP TABLE vec_embeddings_int8_conversion;"
    ))?;
    let rows_after: i64 =
        transaction.query_row("SELECT COUNT(*) FROM vec_embeddings_rowids", [], |row| {
            row.get(0)
        })?;
    if rows_after != rows_before {
        return Err(Vec0Error::Verification(format!(
            "row count changed across the conversion: {rows_before} -> {rows_after}"
        )));
    }
    transaction.commit()?;
    // Reclaim the freed float32 pages: VACUUM cannot run inside the
    // transaction, and the checkpoint leaves the sealed state with an
    // empty WAL (federated admission refuses a nonempty WAL).
    conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(Int8Conversion {
        rows: rows_before.max(0) as usize,
        dimensions,
        already_int8: false,
    })
}
