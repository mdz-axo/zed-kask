//! Shared utilities used across corpus tool modules.

use crate::{HkaskSettings, json};
use hkask_mcp_server::server::McpToolError;
use serde::de::DeserializeOwned;

/// Classify a `std::io::Error` from a caller-facing file operation into the
/// appropriate `McpToolError` kind. Alias of the canonical
/// `hkask_mcp_server::server::map_io_error` (NotFound/PermissionDenied are
/// caller-fixable; others remain internal) kept under the corpus-local name
/// so existing call sites stay readable.
pub(crate) use hkask_mcp_server::server::map_io_error as map_corpus_io_error;

/// Classify a `MemoryStoreError` from a memory-DB operation into the
/// appropriate `McpToolError` kind. Alias of the canonical
/// `hkask_mcp_server::server::map_memory_store_error` (shared with the
/// training server) kept under the corpus-local name so existing call sites
/// stay readable.
pub(crate) use hkask_mcp_server::server::map_memory_store_error;

/// Classify a `TriageError` from the PDF triage pipeline into the appropriate
/// `McpToolError` kind: an invalid page spec is caller input
/// (`invalid_argument`); `pdftotext`/`pdfimages` failures include spawn
/// failures (binary missing — operator-fixable, `unavailable`); a page-count
/// mismatch between the two tools is an internal inconsistency (`internal`).
pub(crate) fn map_triage_error(error: crate::ocr::triage::TriageError) -> McpToolError {
    use crate::ocr::triage::TriageError;
    let message = format!("triage failed: {error}");
    match error {
        TriageError::InvalidPageSpec(_) => McpToolError::invalid_argument(message),
        TriageError::PdftotextFailed(_) | TriageError::PdfimagesFailed(_) => {
            McpToolError::unavailable(message)
        }
        TriageError::PageCountMismatch { .. } => McpToolError::internal(message), // rr0044-ok: mapper-fallback-arm
    }
}

/// Classify a `DatabaseError` from opening a memory DB into the appropriate
/// `McpToolError` kind: a passphrase mismatch or key-derivation failure is
/// an auth/credential failure (`permission_denied`) — `KeyDerivation` is
/// raised when `HKASK_DB_PASSPHRASE` is unset/empty, which is a missing
/// credential, not an infrastructure fault; a corrupted DB file is a
/// caller-fixable data problem (`invalid_argument`); SQLite/SQLCipher
/// failures are infrastructure (`internal`).
pub(crate) fn map_database_error(
    error: hkask_storage::DatabaseError,
    context: &str,
) -> McpToolError {
    use hkask_storage::DatabaseError;
    let message = format!("{context}: {error}");
    match error {
        DatabaseError::PassphraseMismatch(_) => McpToolError::permission_denied(message),
        DatabaseError::KeyDerivation(_) => McpToolError::permission_denied(format!(
            "{message}. Set HKASK_DB_PASSPHRASE to a non-empty passphrase"
        )),
        DatabaseError::Corrupted(_) => McpToolError::invalid_argument(message),
        DatabaseError::Sqlite(_) | DatabaseError::SqlCipher(_) => {
            McpToolError::internal(message) // rr0044-ok: infra-db-failure
        }
        // Non-exhaustive enum: future variants stay internal (conservative).
        _ => McpToolError::internal(message), // rr0044-ok: non-exhaustive-fallback
    }
}

/// Classify an `EmbeddingError` from an embedding-store operation into the
/// appropriate `McpToolError` kind: missing refs → `not_found`, dimension
/// mismatches → `invalid_argument` (caller stored vectors with a different
/// model), infrastructure → shared `map_infra_error`, storage/decode →
/// `internal`.
pub(crate) fn map_embedding_error(
    error: hkask_storage::EmbeddingError,
    context: &str,
) -> McpToolError {
    use hkask_storage::EmbeddingError;
    let message = format!("{context}: {error}");
    match error {
        EmbeddingError::NotFound(_) => McpToolError::not_found(message),
        EmbeddingError::DimensionMismatch { .. } => McpToolError::invalid_argument(message),
        EmbeddingError::Infrastructure(ref infra) => {
            hkask_mcp_server::server::map_infra_error(infra, context)
        }
        EmbeddingError::Storage(_) | EmbeddingError::Decode(_) => McpToolError::internal(message), // rr0044-ok: storage-decode-failure
    }
}

/// Classify a `ServiceError` from the compose pipeline into the appropriate
/// `McpToolError` kind via its semantic `ErrorKind`: `NotFound` → `not_found`,
/// `Forbidden` → `permission_denied`, `BadRequest` → `invalid_argument`,
/// `Conflict` → `failed_precondition`, `ServiceUnavailable` → `unavailable`.
pub(crate) fn map_service_error(
    error: hkask_services_core::ServiceError,
    context: &str,
) -> McpToolError {
    use hkask_services_core::ErrorKind;
    let message = format!("{context}: {error}");
    match error.kind() {
        ErrorKind::NotFound => McpToolError::not_found(message),
        ErrorKind::Forbidden => McpToolError::permission_denied(message),
        ErrorKind::BadRequest => McpToolError::invalid_argument(message),
        ErrorKind::Conflict => McpToolError::failed_precondition(message),
        ErrorKind::ServiceUnavailable => McpToolError::unavailable(message),
    }
}

/// Contained, size-capped read of a caller-supplied text file as UTF-8.
///
/// Single enforcement point for caller-supplied `*_jsonl` tool-argument paths
/// (CWE-22/200/400): the path is resolved through
/// `crate::path_safety::contain_for_read` and read with the shared
/// `MAX_READ_BYTES` cap (via `read_capped`), so an escaping path
/// (`/etc/passwd`, `../../escape`) or an oversized file is rejected with
/// `invalid_argument` before any bytes reach a tool. `label` is the
/// parameter-name context used in error messages.
pub(crate) fn read_text_capped(path: &str, label: &str) -> Result<String, McpToolError> {
    let bytes = crate::path_safety::read_capped(path, crate::path_safety::MAX_READ_BYTES)?;
    String::from_utf8(bytes).map_err(|e| {
        McpToolError::invalid_argument(format!("{label} '{path}' is not valid UTF-8: {e}"))
    })
}

/// Open a `MemoryStore` at the caller-supplied `db_path` with the corpus
/// embedding dimension. Single enforcement point for the `MemoryStore::open`
/// + `map_database_error` pattern duplicated across `corpus_dedup_chunks`,
/// `corpus_ingest_qa`, `corpus_purge_qa`, `embed_batch_from_jsonl`, and the
/// consolidation/prompt-builder/assertions services. A passphrase mismatch or
/// missing `HKASK_DB_PASSPHRASE` surfaces as `permission_denied` (the
/// `.rules` missing-credential pattern); a corrupted DB is `invalid_argument`;
/// SQLite/SQLCipher failures are `internal`.
pub(crate) fn open_memory_store(
    db_path: &str,
    passphrase: &str,
) -> Result<hkask_memory::MemoryStore, McpToolError> {
    let dim = crate::embedding_dim();
    hkask_memory::MemoryStore::open(db_path, passphrase, dim)
        .map_err(|e| map_database_error(e, "Cannot open memory DB"))
}

/// Resolve `output` through `contain_for_write` and write `content` to the
/// contained path. Single enforcement point for the `contain_for_write` +
/// `std::fs::write` + `map_corpus_io_error` pattern duplicated across
/// `corpus_dedup_chunks`, `corpus_ingest_qa`, `corpus_prepare_training_dataset`,
/// `corpus_tag_chunks`, `ConsolidationService::consolidate`, and
/// `PromptBuilderService::build_prompts`. An escaping path is rejected with
/// `invalid_argument` before any write reaches disk.
pub(crate) fn write_contained(output: &str, content: &str) -> Result<(), McpToolError> {
    let path = crate::path_safety::contain_for_write(output)?;
    std::fs::write(&path, content)
        .map_err(|e| map_corpus_io_error(e, &format!("Cannot write output '{output}'")))
}

/// Stream a JSONL file line-by-line, parsing each non-empty line into `T`.
///
/// Unlike [`read_jsonl`], this does NOT read the entire file into memory —
/// it opens the file with `BufReader` and reads line-by-line, making it
/// suitable for files that exceed `MAX_READ_BYTES` (e.g. 71.8 MB
/// `chunks.jsonl`). Path containment is still enforced via
/// `contain_for_read`, but the `MAX_READ_BYTES` cap is NOT applied — the
/// file can be arbitrarily large. The caller is responsible for batching
/// the returned iterator if memory is a concern.
///
/// Lines are trimmed and empty lines are skipped. Parse errors are
/// propagated with the 1-based file line number.
pub(crate) fn read_jsonl_stream<T: DeserializeOwned>(
    path: &str,
    label: &str,
) -> Result<Vec<T>, McpToolError> {
    let resolved = crate::path_safety::contain_for_read(path)?;
    let file = std::fs::File::open(&resolved).map_err(|e| {
        McpToolError::invalid_argument(format!("{label} '{path}' cannot be opened: {e}"))
    })?;
    let reader = std::io::BufReader::new(file);
    let mut out = Vec::new();
    for (i, line_result) in std::io::BufRead::lines(reader).enumerate() {
        let line = line_result.map_err(|e| {
            McpToolError::invalid_argument(format!(
                "{label} '{path}' line {} read error: {e}",
                i + 1
            ))
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: T = serde_json::from_str(line).map_err(|e| {
            McpToolError::invalid_argument(format!("{label} line {} is not valid JSON: {e}", i + 1))
        })?;
        out.push(v);
    }
    Ok(out)
}

/// Read a JSONL file and parse each non-empty line into `T`.
///
/// Path containment and the `MAX_READ_BYTES` size cap are enforced here (via
/// [`read_text_capped`]) — callers pass raw MCP tool-argument paths and this
/// helper is the enforcement point. Cannot-read failures (escape, unresolvable
/// path, oversized file) classify as `invalid_argument` (caller-supplied path).
///
/// Lines are split on newlines, trimmed, and empty lines are skipped. Parse errors
/// are propagated with the 1-based file line number (counting all lines,
/// including empty ones, to match the original per-site error messages).
/// `label` is the parameter-name context used in error messages (e.g.
/// `"chunks_jsonl"`, `"prompts_jsonl"`, `"tagged_jsonl"`).
pub(crate) fn read_jsonl<T: DeserializeOwned>(
    path: &str,
    label: &str,
) -> Result<Vec<T>, McpToolError> {
    let content = read_text_capped(path, label)?;
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: T = serde_json::from_str(line).map_err(|e| {
            McpToolError::invalid_argument(format!("{label} line {} is not valid JSON: {e}", i + 1))
        })?;
        out.push(v);
    }
    Ok(out)
}

/// Read a JSONL file, dropping malformed lines and returning the count dropped.
///
/// Path containment and the `MAX_READ_BYTES` size cap are enforced here (via
/// [`read_text_capped`]), same as [`read_jsonl`].
///
/// Like [`read_jsonl`] but lenient: lines that fail to parse are silently
/// dropped (no error propagated), and the number of dropped lines is returned
/// so callers can warn or report. Empty lines are not counted as dropped.
pub(crate) fn read_jsonl_lenient<T: DeserializeOwned>(
    path: &str,
    label: &str,
) -> Result<(Vec<T>, usize), McpToolError> {
    let content = read_text_capped(path, label)?;
    let mut out = Vec::new();
    let mut dropped = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(line) {
            Ok(v) => out.push(v),
            Err(_) => dropped += 1,
        }
    }
    Ok((out, dropped))
}

/// Cosine similarity between two vectors. Consolidated from ocr/semantic.rs (C4).
/// Returns 0.0 if either vector is empty or dimensions mismatch.
pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(0.0, 1.0)
}

/// Approximate token-to-word conversion: 1 word ≈ 1.33 tokens.
/// Returns 0.0 for identical, 1.0 for orthogonal, 2.0 for opposite or degenerate.
#[must_use]
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 2.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) * (*y as f64))
        .sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 2.0;
    }
    let similarity = dot / (norm_a * norm_b);
    let distance = 1.0 - similarity;
    distance.clamp(0.0, 2.0)
}

/// Approximate token-to-word conversion: 1 word ≈ 1.33 tokens.
/// So tokens ÷ 1.33 = words. This is the standard BPE ratio for English text.
pub(crate) fn tokens_to_words(tokens: usize) -> usize {
    ((tokens as f64) / 1.33) as usize
}

/// Compute (max_words, min_words) from (max_tokens, overlap_tokens).
/// `overlap_tokens` determines the minimum chunk size (hard floor below which
/// a buffer won't flush). Falls back to HkaskSettings::chunk_max_tokens() when
/// max_tokens is None.
pub(crate) fn chunk_word_bounds(
    max_tokens: Option<usize>,
    overlap_tokens: Option<usize>,
) -> (usize, usize) {
    let default_max = HkaskSettings::load().chunk_max_tokens();
    let max_w = tokens_to_words(max_tokens.unwrap_or(default_max));
    let min_w = tokens_to_words(overlap_tokens.unwrap_or(64)).max(max_w / 4);
    (max_w, min_w)
}

/// Serialize (entity_ref, text) pair slice into json.
pub(crate) fn serialize_passages(passages: &[(String, String)]) -> Vec<serde_json::Value> {
    passages
        .iter()
        .map(|(entity_ref, passage_text)| json!({"entity_ref": entity_ref, "text": passage_text}))
        .collect()
}

/// Chunk a `DocStructure` into passages, respecting heading boundaries.
///
/// Groups blocks under their nearest preceding heading. Each group becomes
/// one or more passages via `crate::text::chunk_text`. When a group exceeds
/// `max_words`, it is split at sentence boundaries within the group. When a
/// group is smaller than `min_words`, it is merged with the next group if
/// possible (to avoid tiny chunks).
///
/// Falls back to flat `chunk_text` when the structure has no headings.
pub(crate) fn chunk_structure(
    structure: &hkask_types::document::DocStructure,
    entity_ref_prefix: &str,
    min_words: usize,
    max_words: usize,
    boundary: &str,
) -> Vec<(String, String)> {
    use hkask_types::document::Block;

    // Collect all blocks across pages, tracking heading starts.
    let blocks: Vec<&Block> = structure.iter_blocks().collect();

    // If no headings, flatten to text and use the standard chunker.
    let has_headings = blocks.iter().any(|b| b.is_heading());
    if !has_headings {
        let flat_text = structure.text();
        return crate::text::chunk_text(
            &flat_text,
            entity_ref_prefix,
            min_words,
            max_words,
            boundary,
        );
    }

    // Group blocks by section (each heading starts a new section).
    let mut sections: Vec<(String, String)> = Vec::new(); // (heading_text, body_text)
    let mut current_heading = String::new();
    let mut current_body = String::new();

    for block in &blocks {
        match block {
            Block::Heading { text, .. } => {
                // Flush previous section
                if !current_body.trim().is_empty() || !current_heading.is_empty() {
                    sections.push((current_heading.clone(), current_body.clone()));
                }
                current_heading = text.clone();
                current_body.clear();
            }
            _ => {
                let block_text = block.text();
                if !current_body.is_empty() {
                    current_body.push_str("\n\n");
                }
                current_body.push_str(&block_text);
            }
        }
    }
    // Flush final section
    if !current_body.trim().is_empty() || !current_heading.is_empty() {
        sections.push((current_heading, current_body));
    }

    // Chunk each section, prepending the heading as context.
    let mut passages = Vec::new();
    for (idx, (heading, body)) in sections.iter().enumerate() {
        if body.trim().is_empty() {
            continue;
        }
        // Prepend heading to body so each chunk knows its section.
        let section_text = if heading.is_empty() {
            body.clone()
        } else {
            format!("{heading}\n\n{body}")
        };
        let section_ref = format!("{entity_ref_prefix}:sec{idx}");
        let section_passages =
            crate::text::chunk_text(&section_text, &section_ref, min_words, max_words, boundary);
        passages.extend(section_passages);
    }
    passages
}
