//! `D100`-isolated, `BLAKE3` content-addressed spillover store for truncated and
//! condensed tool outputs (`kask://spillover/<blake3_hex>`).
//!
//! Design invariants:
//! 1. **Content addressing**: Every handle URI encodes the exact 64-character
//!    lowercase hex `BLAKE3` digest of the unmodified raw output bytes.
//! 2. **Non-interference (`D100` / `cso:access_control`)**: Entries ingested
//!    exclusively under [`SpilloverPrivacy::Private`] (`public_seen == false`)
//!    are unreadable from [`SpilloverPrivacy::Public`] contexts, returning
//!    [`SpilloverError::PrivateBoundaryRefused`]. Separate threads own distinct
//!    [`SpilloverVault`] instances (shared only with inherited subagents).
//! 3. **Single-line JSON sliceability**: Compact single-line JSON payloads
//!    (such as `KaskServerTool` wire outputs) or JSON documents containing
//!    lines `> 1800` bytes are deterministically pretty-projected via
//!    `serde_json::to_string_pretty` for line-indexed slicing, avoiding
//!    `read_file`'s 2,000-char per-line truncation while preserving the raw
//!    `BLAKE3` digest.
//! 4. **Bounded memory & bounded slices**: Vault capacity is bounded by entry
//!    count and total stored bytes with deterministic LRU eviction; every
//!    `read_slice` call caps output to [`MAX_SLICE_LINES`] and
//!    [`MAX_SLICE_BYTES`] with pagination metadata.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Canonical URI scheme prefix for in-memory tool-output spillover handles.
pub const SPILLOVER_URI_PREFIX: &str = "kask://spillover/";

/// Default maximum number of distinct spillover entries retained per thread vault.
pub const DEFAULT_MAX_ENTRIES: usize = 128;

/// Default maximum aggregate sliceable byte budget per thread vault (`32 MiB`).
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Default maximum byte budget for a single spillover entry (`8 MiB`).
pub const DEFAULT_MAX_ENTRY_BYTES: usize = 8 * 1024 * 1024;

/// Default line window returned when `read_file` omits `end_line`.
pub const DEFAULT_SLICE_LINES: u32 = 200;

/// Hard ceiling on lines returned in a single slice read.
pub const MAX_SLICE_LINES: u32 = 400;

/// Hard ceiling on UTF-8 bytes returned in a single slice read (`32 KiB`).
pub const MAX_SLICE_BYTES: usize = 32 * 1024;

/// Minimum net byte savings required after attaching a spillover notice onto a
/// condensed tool result (`compressed.len() + notice.len() + MIN_NET_SAVINGS_BYTES <= raw.len()`).
pub const MIN_NET_SAVINGS_BYTES: usize = 64;

/// Line-length threshold above which valid JSON inputs are pretty-projected so
/// `read_file`'s 2,000-character per-line cap never truncates interior fields.
const JSON_LINE_LEN_PRETTY_THRESHOLD: usize = 1800;

/// Privacy domain of a spillover write or slice-read operation (`D100`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpilloverPrivacy {
    /// Public-provider turn (`private_inference == false`).
    Public,
    /// Privacy-marked turn (`private_inference == true`, `D100`).
    Private,
}

impl SpilloverPrivacy {
    /// Map the `D100` boolean privacy flag into [`SpilloverPrivacy`].
    #[must_use]
    pub const fn from_private_flag(is_private: bool) -> Self {
        if is_private {
            Self::Private
        } else {
            Self::Public
        }
    }
}

/// Receipt returned when storing or referencing an output in [`SpilloverStore`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpilloverReceipt {
    /// Canonical `kask://spillover/<64-hex>` handle URI.
    pub uri: String,
    /// 64-character lowercase hex `BLAKE3` digest of the unmodified raw output.
    pub blake3_hex: String,
    /// Byte length of the unmodified raw output.
    pub raw_bytes: usize,
    /// Total 1-based lines available in the sliceable view.
    pub total_lines: usize,
    /// Whether single-line/long-line JSON was pretty-projected into multi-line JSON.
    pub pretty_projected_json: bool,
    /// Whether the stored sliceable view was capped at `max_entry_bytes`.
    pub truncated_entry: bool,
}

/// Bounded slice returned by [`SpilloverStore::read_slice`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpilloverSlice {
    /// Canonical `kask://spillover/<64-hex>` handle URI.
    pub uri: String,
    /// Originating tool name recorded at ingestion.
    pub tool_name: String,
    /// 64-character lowercase hex `BLAKE3` digest of the raw output.
    pub blake3_hex: String,
    /// Byte length of the unmodified raw output.
    pub raw_bytes: usize,
    /// Total 1-based lines in the stored sliceable view.
    pub total_lines: usize,
    /// 1-based inclusive start line of this slice.
    pub start_line: u32,
    /// 1-based inclusive end line of this slice (`0` when `start_line > total_lines`).
    pub end_line: u32,
    /// Raw slice text (without `cat -n` line-number prefixes).
    pub text: String,
    /// Whether additional lines exist after `end_line`.
    pub has_more_lines: bool,
}

/// Typed errors returned by spillover URI parsing and slice retrieval.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SpilloverError {
    /// Malformed `kask://spillover/...` handle URI.
    #[error("Invalid spillover URI `{uri}`: expected `kask://spillover/<64-char-blake3-hex>`.")]
    InvalidUri {
        /// The rejected URI string.
        uri: String,
    },
    /// Handle does not exist in this thread's spillover vault (evicted or from another thread).
    #[error(
        "Spillover handle `{uri}` was not found in this thread's vault (handles are scoped to the originating thread and bounded by LRU capacity)."
    )]
    NotFound {
        /// The requested URI string.
        uri: String,
    },
    /// `D100` non-interference refusal: a public context attempted to read a private entry.
    #[error(
        "[permission_denied] Spillover handle `{uri}` was recorded under private inference (D100) and cannot be read from a public-provider context."
    )]
    PrivateBoundaryRefused {
        /// The requested URI string.
        uri: String,
    },
}

struct StoredSpilloverEntry {
    tool_name: String,
    blake3_hex: String,
    raw_bytes: usize,
    sliceable_text: String,
    line_offsets: Vec<usize>,
    pretty_projected_json: bool,
    truncated_entry: bool,
    /// True iff at least one `SpilloverPrivacy::Public` turn independently
    /// produced these exact raw bytes in this vault.
    public_seen: bool,
}

/// Pure synchronous `BLAKE3` content-addressed LRU spillover store.
pub struct SpilloverStore {
    max_entries: usize,
    max_total_bytes: usize,
    max_entry_bytes: usize,
    total_stored_bytes: usize,
    lru_order: VecDeque<String>,
    entries: HashMap<String, StoredSpilloverEntry>,
}

impl Default for SpilloverStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SpilloverStore {
    /// Construct a store with default thread bounds (`128` entries, `32 MiB` total, `8 MiB` per entry).
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_MAX_ENTRIES,
            DEFAULT_MAX_TOTAL_BYTES,
            DEFAULT_MAX_ENTRY_BYTES,
        )
    }

    /// Construct a store with explicit capacity limits (clamped to `>= 1` entry and `>= 256` bytes).
    #[must_use]
    pub fn with_limits(max_entries: usize, max_total_bytes: usize, max_entry_bytes: usize) -> Self {
        let max_entry_bytes = max_entry_bytes.max(256);
        let max_total_bytes = max_total_bytes.max(max_entry_bytes);
        Self {
            max_entries: max_entries.max(1),
            max_total_bytes,
            max_entry_bytes,
            total_stored_bytes: 0,
            lru_order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    /// Number of distinct entries currently resident in the store.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current sum of stored `sliceable_text` bytes across resident entries.
    #[must_use]
    pub fn total_stored_bytes(&self) -> usize {
        self.total_stored_bytes
    }

    /// Store `raw_text` under its `BLAKE3` content address and `privacy` mark.
    pub fn put(
        &mut self,
        tool_name: &str,
        raw_text: &str,
        privacy: SpilloverPrivacy,
    ) -> SpilloverReceipt {
        let blake3_hex = blake3::hash(raw_text.as_bytes()).to_hex().to_string();
        let uri = format!("{SPILLOVER_URI_PREFIX}{blake3_hex}");

        if let Some(existing) = self.entries.get_mut(&blake3_hex) {
            if privacy == SpilloverPrivacy::Public {
                existing.public_seen = true;
            }
            let receipt = SpilloverReceipt {
                uri,
                blake3_hex: blake3_hex.clone(),
                raw_bytes: existing.raw_bytes,
                total_lines: existing.line_offsets.len().max(1),
                pretty_projected_json: existing.pretty_projected_json,
                truncated_entry: existing.truncated_entry,
            };
            self.touch_lru(&blake3_hex);
            return receipt;
        }

        let (sliceable_text, pretty_projected_json, truncated_entry) =
            build_sliceable_view(raw_text, self.max_entry_bytes);
        let line_offsets = compute_line_offsets(&sliceable_text);
        let total_lines = line_offsets.len().max(1);
        let entry_bytes = sliceable_text.len();

        while !self.entries.is_empty()
            && (self.entries.len() >= self.max_entries
                || self.total_stored_bytes.saturating_add(entry_bytes) > self.max_total_bytes)
        {
            self.evict_oldest();
        }

        let entry = StoredSpilloverEntry {
            tool_name: tool_name.to_string(),
            blake3_hex: blake3_hex.clone(),
            raw_bytes: raw_text.len(),
            sliceable_text,
            line_offsets,
            pretty_projected_json,
            truncated_entry,
            public_seen: privacy == SpilloverPrivacy::Public,
        };
        self.total_stored_bytes = self.total_stored_bytes.saturating_add(entry_bytes);
        self.entries.insert(blake3_hex.clone(), entry);
        self.lru_order.push_back(blake3_hex.clone());

        SpilloverReceipt {
            uri,
            blake3_hex,
            raw_bytes: raw_text.len(),
            total_lines,
            pretty_projected_json,
            truncated_entry,
        }
    }

    /// Rehydrate a saved output only if `blake3(raw_text)` matches `expected_uri`'s hash.
    ///
    /// Returns `Some(SpilloverReceipt)` when verified and stored, or `None` if
    /// `expected_uri` is invalid or `raw_text` does not match the digest (e.g.,
    /// when a saved tool result only persisted a truncated preview).
    pub fn rehydrate_verified(
        &mut self,
        tool_name: &str,
        raw_text: &str,
        expected_uri: &str,
        privacy: SpilloverPrivacy,
    ) -> Option<SpilloverReceipt> {
        let expected_hash = parse_spillover_hash(expected_uri).ok()?;
        let actual_hash = blake3::hash(raw_text.as_bytes()).to_hex();
        if !actual_hash.as_str().eq_ignore_ascii_case(&expected_hash) {
            return None;
        }
        Some(self.put(tool_name, raw_text, privacy))
    }

    /// Look up metadata for an existing handle if accessible under `caller_privacy`.
    pub fn receipt_for_uri(
        &self,
        uri: &str,
        caller_privacy: SpilloverPrivacy,
    ) -> Result<SpilloverReceipt, SpilloverError> {
        let hash = parse_spillover_hash(uri)?;
        let Some(entry) = self.entries.get(&hash) else {
            return Err(SpilloverError::NotFound {
                uri: uri.trim().to_string(),
            });
        };
        if caller_privacy == SpilloverPrivacy::Public && !entry.public_seen {
            return Err(SpilloverError::PrivateBoundaryRefused {
                uri: uri.trim().to_string(),
            });
        }
        Ok(SpilloverReceipt {
            uri: format!("{SPILLOVER_URI_PREFIX}{hash}"),
            blake3_hex: entry.blake3_hex.clone(),
            raw_bytes: entry.raw_bytes,
            total_lines: entry.line_offsets.len().max(1),
            pretty_projected_json: entry.pretty_projected_json,
            truncated_entry: entry.truncated_entry,
        })
    }

    /// Read a bounded 1-based inclusive line slice from `uri`, enforcing `D100`
    /// non-interference (`caller_privacy`) and per-call line/byte ceilings.
    pub fn read_slice(
        &mut self,
        uri: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
        caller_privacy: SpilloverPrivacy,
    ) -> Result<SpilloverSlice, SpilloverError> {
        let hash = parse_spillover_hash(uri)?;
        let canonical_uri = format!("{SPILLOVER_URI_PREFIX}{hash}");

        let (tool_name, blake3_hex, raw_bytes, total_lines, start, actual_end, text, has_more) = {
            let Some(entry) = self.entries.get(&hash) else {
                return Err(SpilloverError::NotFound {
                    uri: uri.trim().to_string(),
                });
            };
            if caller_privacy == SpilloverPrivacy::Public && !entry.public_seen {
                return Err(SpilloverError::PrivateBoundaryRefused { uri: canonical_uri });
            }

            let total_lines = entry.line_offsets.len().max(1);
            let start = start_line.unwrap_or(1).max(1);
            if entry.sliceable_text.is_empty() || (start as usize) > total_lines {
                (
                    entry.tool_name.clone(),
                    entry.blake3_hex.clone(),
                    entry.raw_bytes,
                    total_lines,
                    start,
                    0u32,
                    String::new(),
                    false,
                )
            } else {
                let requested_end = match end_line {
                    Some(end) => end.max(start),
                    None => start.saturating_add(DEFAULT_SLICE_LINES - 1),
                };
                let max_end_by_lines = start
                    .saturating_add(MAX_SLICE_LINES - 1)
                    .min(total_lines as u32);
                let target_end = requested_end.min(max_end_by_lines);

                let start_idx = (start as usize) - 1;
                let byte_start = entry.line_offsets[start_idx];
                let mut chosen_end_idx = start_idx;

                for line_idx in start_idx..(target_end as usize) {
                    let line_byte_end = if line_idx + 1 < entry.line_offsets.len() {
                        entry.line_offsets[line_idx + 1]
                    } else {
                        entry.sliceable_text.len()
                    };
                    let candidate_bytes = line_byte_end.saturating_sub(byte_start);
                    if line_idx > start_idx && candidate_bytes > MAX_SLICE_BYTES {
                        break;
                    }
                    chosen_end_idx = line_idx;
                }

                let raw_slice_end = if chosen_end_idx + 1 < entry.line_offsets.len() {
                    entry.line_offsets[chosen_end_idx + 1]
                } else {
                    entry.sliceable_text.len()
                };
                let bounded_end = if raw_slice_end.saturating_sub(byte_start) > MAX_SLICE_BYTES {
                    floor_utf8_boundary(&entry.sliceable_text, byte_start + MAX_SLICE_BYTES)
                } else {
                    raw_slice_end
                };

                let actual_end = (chosen_end_idx + 1) as u32;
                let has_more = (actual_end as usize) < total_lines || bounded_end < raw_slice_end;
                (
                    entry.tool_name.clone(),
                    entry.blake3_hex.clone(),
                    entry.raw_bytes,
                    total_lines,
                    start,
                    actual_end,
                    entry.sliceable_text[byte_start..bounded_end].to_string(),
                    has_more,
                )
            }
        };

        self.touch_lru(&hash);
        Ok(SpilloverSlice {
            uri: canonical_uri,
            tool_name,
            blake3_hex,
            raw_bytes,
            total_lines,
            start_line: start,
            end_line: actual_end,
            text,
            has_more_lines: has_more,
        })
    }

    fn touch_lru(&mut self, hash: &str) {
        if self.lru_order.back().is_some_and(|back| back == hash) {
            return;
        }
        self.lru_order.retain(|item| item != hash);
        self.lru_order.push_back(hash.to_string());
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_hash) = self.lru_order.pop_front()
            && let Some(removed) = self.entries.remove(&oldest_hash)
        {
            self.total_stored_bytes = self
                .total_stored_bytes
                .saturating_sub(removed.sliceable_text.len());
        }
    }
}

/// Thread-scoped shared handle around [`SpilloverStore`].
#[derive(Clone, Default)]
pub struct SpilloverVault {
    inner: Arc<Mutex<SpilloverStore>>,
}

impl SpilloverVault {
    /// Create a fresh isolated [`SpilloverVault`] with default limits.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a [`SpilloverVault`] with explicit limits (for deterministic capacity tests).
    #[must_use]
    pub fn with_limits(max_entries: usize, max_total_bytes: usize, max_entry_bytes: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SpilloverStore::with_limits(
                max_entries,
                max_total_bytes,
                max_entry_bytes,
            ))),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SpilloverStore> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Store `raw_text` under its `BLAKE3` content address and `privacy` mark.
    pub fn put(
        &self,
        tool_name: &str,
        raw_text: &str,
        privacy: SpilloverPrivacy,
    ) -> SpilloverReceipt {
        self.lock().put(tool_name, raw_text, privacy)
    }

    /// Rehydrate a persisted tool output only if its `BLAKE3` digest matches `expected_uri`.
    pub fn rehydrate_verified(
        &self,
        tool_name: &str,
        raw_text: &str,
        expected_uri: &str,
        privacy: SpilloverPrivacy,
    ) -> Option<SpilloverReceipt> {
        self.lock()
            .rehydrate_verified(tool_name, raw_text, expected_uri, privacy)
    }

    /// Retrieve receipt metadata for `uri` if accessible under `caller_privacy`.
    pub fn receipt_for_uri(
        &self,
        uri: &str,
        caller_privacy: SpilloverPrivacy,
    ) -> Result<SpilloverReceipt, SpilloverError> {
        self.lock().receipt_for_uri(uri, caller_privacy)
    }

    /// Read a bounded 1-based line slice from `uri` under `caller_privacy`.
    pub fn read_slice(
        &self,
        uri: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
        caller_privacy: SpilloverPrivacy,
    ) -> Result<SpilloverSlice, SpilloverError> {
        self.lock()
            .read_slice(uri, start_line, end_line, caller_privacy)
    }

    /// Current resident entry count and stored byte total `(entries, bytes)`.
    #[must_use]
    pub fn stats(&self) -> (usize, usize) {
        let guard = self.lock();
        (guard.len(), guard.total_stored_bytes())
    }
}

/// Whether `path` starts with the `kask://spillover/` URI scheme.
#[must_use]
pub fn is_spillover_uri(path: &str) -> bool {
    path.trim().starts_with(SPILLOVER_URI_PREFIX)
}

/// Validate a `kask://spillover/<64-hex>` URI and return its normalized lowercase hash.
pub fn parse_spillover_hash(uri: &str) -> Result<String, SpilloverError> {
    let trimmed = uri.trim();
    let Some(hex_part) = trimmed.strip_prefix(SPILLOVER_URI_PREFIX) else {
        return Err(SpilloverError::InvalidUri {
            uri: trimmed.to_string(),
        });
    };
    if hex_part.len() != 64 || !hex_part.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SpilloverError::InvalidUri {
            uri: trimmed.to_string(),
        });
    }
    Ok(hex_part.to_ascii_lowercase())
}

/// Scan `text` for every valid `kask://spillover/<64-hex>` handle URI in order.
#[must_use]
pub fn extract_spillover_uris(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(SPILLOVER_URI_PREFIX) {
        let start = search_from + rel;
        let hash_start = start + SPILLOVER_URI_PREFIX.len();
        let hash_end = hash_start + 64;
        if hash_end <= text.len()
            && text.is_char_boundary(hash_end)
            && text.as_bytes()[hash_start..hash_end]
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            let next_byte_is_hex = text
                .as_bytes()
                .get(hash_end)
                .is_some_and(u8::is_ascii_hexdigit);
            if !next_byte_is_hex {
                let uri = format!(
                    "{SPILLOVER_URI_PREFIX}{}",
                    text[hash_start..hash_end].to_ascii_lowercase()
                );
                if !found.contains(&uri) {
                    found.push(uri);
                }
                search_from = hash_end;
                continue;
            }
        }
        search_from = hash_start.min(text.len());
    }
    found
}

/// Format the model-facing spillover notice attached as a separate text part
/// when a tool output is condensed.
#[must_use]
pub fn format_condensed_spillover_notice(
    raw_bytes: usize,
    compressed_bytes: usize,
    receipt: &SpilloverReceipt,
) -> String {
    format!(
        "\n\n[Condensed tool output: {raw_bytes} -> {compressed_bytes} bytes. Full output ({} lines) retained at {}; call `read_file` with `path: \"{}\"` and `start_line`/`end_line` to inspect elided sections.]",
        receipt.total_lines, receipt.uri, receipt.uri
    )
}

/// Reconcile a condensed tool-result text part with `vault`, enforcing:
/// 1. Passthrough when `compressed` did not shrink `raw_text`.
/// 2. Preservation of an existing `kask://spillover/<hash>` handle (e.g. from
///    `terminal_tool` `> 16 KiB` truncation) so condensation never replaces a
///    full-output handle with a truncated-preview handle or drops the handle line.
/// 3. Strict net-byte-savings gate (`compressed.len() + notice.len() + MIN_NET_SAVINGS_BYTES <= raw_text.len()`):
///    marginal condensations that would not yield net savings after the handle
///    notice pass `raw_text` through uncondensed so neither fidelity nor token
///    budget is lost.
pub fn condense_text_with_spillover(
    vault: &SpilloverVault,
    tool_name: &str,
    raw_text: &str,
    compressed: String,
    privacy: SpilloverPrivacy,
) -> (String, Option<String>) {
    if compressed.len() >= raw_text.len() || compressed == raw_text {
        return (raw_text.to_string(), None);
    }

    if let Some(existing_uri) = extract_spillover_uris(raw_text).into_iter().next() {
        if compressed.contains(&existing_uri) {
            return (compressed, None);
        }
        let preserved_notice = match vault.receipt_for_uri(&existing_uri, privacy) {
            Ok(receipt) => format!(
                "\n\n[Full untruncated output ({} bytes, {} lines) retained at {}; call `read_file` with `path: \"{}\"` and `start_line`/`end_line` to inspect elided sections.]",
                receipt.raw_bytes, receipt.total_lines, receipt.uri, receipt.uri
            ),
            Err(_) => format!(
                "\n\n[Full output retained at {existing_uri}; call `read_file` with `path: \"{existing_uri}\"` and `start_line`/`end_line` to inspect elided sections.]"
            ),
        };
        if compressed.len() + preserved_notice.len() < raw_text.len() {
            return (compressed, Some(preserved_notice));
        }
        return (raw_text.to_string(), None);
    }

    // Estimate worst-case notice bytes before inserting into the vault so
    // marginal condensations do not churn vault entries needlessly.
    let min_possible_notice_len = 160usize;
    if compressed.len() + min_possible_notice_len + MIN_NET_SAVINGS_BYTES > raw_text.len() {
        return (raw_text.to_string(), None);
    }

    let receipt = vault.put(tool_name, raw_text, privacy);
    let notice = format_condensed_spillover_notice(raw_text.len(), compressed.len(), &receipt);
    if compressed.len() + notice.len() + MIN_NET_SAVINGS_BYTES <= raw_text.len() {
        (compressed, Some(notice))
    } else {
        (raw_text.to_string(), None)
    }
}

fn build_sliceable_view(raw_text: &str, max_entry_bytes: usize) -> (String, bool, bool) {
    if raw_text.len() > max_entry_bytes {
        let cut = floor_utf8_boundary(raw_text, max_entry_bytes);
        return (raw_text[..cut].to_string(), false, true);
    }

    if should_pretty_project_json(raw_text)
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw_text)
        && let Ok(pretty) = serde_json::to_string_pretty(&parsed)
    {
        if pretty.len() <= max_entry_bytes {
            return (pretty, true, false);
        }
        let cut = floor_utf8_boundary(&pretty, max_entry_bytes);
        return (pretty[..cut].to_string(), true, true);
    }

    (raw_text.to_string(), false, false)
}

fn should_pretty_project_json(raw_text: &str) -> bool {
    let trimmed = raw_text.trim();
    let looks_like_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    if !looks_like_json {
        return false;
    }

    let mut line_count = 0usize;
    let mut max_line_len = 0usize;
    for line in raw_text.lines() {
        line_count += 1;
        if line.len() > max_line_len {
            max_line_len = line.len();
        }
        if max_line_len > JSON_LINE_LEN_PRETTY_THRESHOLD {
            return true;
        }
    }
    line_count <= 4 && raw_text.len() > 256
}

fn compute_line_offsets(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return vec![0];
    }
    let mut offsets = vec![0usize];
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' && idx + 1 < text.len() {
            offsets.push(idx + 1);
        }
    }
    offsets
}

fn floor_utf8_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn put_and_read_slice_preserves_blake3_and_enforces_d100_non_interference() {
        let vault = SpilloverVault::new();
        let secret = "line 1\nPRIVATE_TOKEN_99\nline 3\n";
        let receipt = vault.put("terminal", secret, SpilloverPrivacy::Private);
        assert_eq!(
            receipt.blake3_hex,
            blake3::hash(secret.as_bytes()).to_hex().to_string()
        );
        assert_eq!(receipt.total_lines, 3);

        // Private caller succeeds.
        let ok_slice = vault
            .read_slice(&receipt.uri, Some(2), Some(2), SpilloverPrivacy::Private)
            .expect("private context must read its own private spillover");
        assert_eq!(ok_slice.text, "PRIVATE_TOKEN_99\n");

        // Public caller on the same vault is refused fail-closed (D100).
        let err = vault
            .read_slice(&receipt.uri, Some(1), Some(3), SpilloverPrivacy::Public)
            .expect_err("public context must never read private spillover");
        assert!(matches!(err, SpilloverError::PrivateBoundaryRefused { .. }));

        // Independent vault (another thread) returns NotFound even for Private caller.
        let other_vault = SpilloverVault::new();
        let other_err = other_vault
            .read_slice(&receipt.uri, Some(1), Some(3), SpilloverPrivacy::Private)
            .expect_err("distinct thread vault must not resolve foreign handle");
        assert!(matches!(other_err, SpilloverError::NotFound { .. }));
    }

    #[test]
    fn compact_single_line_json_is_pretty_projected_without_altering_raw_blake3() {
        let vault = SpilloverVault::new();
        let compact = serde_json::to_string(&serde_json::json!({
            "series_id": "TEST_SERIES",
            "observations": (0..30).map(|i| serde_json::json!({
                "idx": i,
                "label": format!("ITEM_MARKER_{i:04}"),
                "value": i as f64 * 1.5
            })).collect::<Vec<_>>()
        }))
        .unwrap();
        assert_eq!(compact.lines().count(), 1);

        let receipt = vault.put("dbnomics_get_series", &compact, SpilloverPrivacy::Public);
        assert!(receipt.pretty_projected_json);
        assert!(receipt.total_lines > 90);
        assert_eq!(
            receipt.blake3_hex,
            blake3::hash(compact.as_bytes()).to_hex().to_string()
        );

        let slice = vault
            .read_slice(&receipt.uri, Some(1), Some(200), SpilloverPrivacy::Public)
            .expect("public read succeeds");
        assert!(slice.text.contains("ITEM_MARKER_0015"));
    }

    #[test]
    fn lru_eviction_and_uri_validation_enforce_bounds() {
        let vault = SpilloverVault::with_limits(2, 2048, 1024);
        let r1 = vault.put("t", &"a\n".repeat(100), SpilloverPrivacy::Public);
        let r2 = vault.put("t", &"b\n".repeat(100), SpilloverPrivacy::Public);
        // Touch r1 so r2 becomes LRU victim.
        let _ = vault
            .read_slice(&r1.uri, Some(1), Some(2), SpilloverPrivacy::Public)
            .unwrap();
        let r3 = vault.put("t", &"c\n".repeat(100), SpilloverPrivacy::Public);

        assert!(
            vault
                .receipt_for_uri(&r1.uri, SpilloverPrivacy::Public)
                .is_ok()
        );
        assert!(
            vault
                .receipt_for_uri(&r3.uri, SpilloverPrivacy::Public)
                .is_ok()
        );
        assert!(matches!(
            vault.receipt_for_uri(&r2.uri, SpilloverPrivacy::Public),
            Err(SpilloverError::NotFound { .. })
        ));
        let (entries, bytes) = vault.stats();
        assert!(entries <= 2 && bytes <= 2048);

        assert!(matches!(
            vault.read_slice(
                "kask://spillover/not-a-valid-hex",
                None,
                None,
                SpilloverPrivacy::Public
            ),
            Err(SpilloverError::InvalidUri { .. })
        ));
    }

    #[test]
    fn condense_text_with_spillover_enforces_strict_net_savings_and_preserves_existing_uri() {
        let vault = SpilloverVault::new();
        // Marginal shrink (saving only 40 bytes) must pass raw_text through without notice expansion.
        let raw_small = "x".repeat(400);
        let marginal_compressed = "x".repeat(360);
        let (primary, notice) = condense_text_with_spillover(
            &vault,
            "tool",
            &raw_small,
            marginal_compressed,
            SpilloverPrivacy::Public,
        );
        assert_eq!(primary, raw_small);
        assert!(notice.is_none());

        // Existing terminal spillover URI must be preserved if condenser elides the header line.
        let full_raw = "line\n".repeat(5000);
        let full_receipt = vault.put("terminal", &full_raw, SpilloverPrivacy::Public);
        let preview_with_uri = format!(
            "Command output too long.\nThe full output was saved to: {}\n{}",
            full_receipt.uri,
            "preview line\n".repeat(300)
        );
        let condensed_without_header = "preview line\n...\npreview line".to_string();
        let (primary2, notice2) = condense_text_with_spillover(
            &vault,
            "terminal",
            &preview_with_uri,
            condensed_without_header,
            SpilloverPrivacy::Public,
        );
        let combined_len = primary2.len() + notice2.as_deref().map_or(0, str::len);
        assert!(combined_len < preview_with_uri.len());
        assert!(notice2.unwrap().contains(&full_receipt.uri));
    }

    proptest! {
        #[test]
        fn prop_condense_with_spillover_never_expands_bytes(
            raw in ".{0,4000}",
            keep_len in 0usize..4000usize,
            is_private in any::<bool>(),
        ) {
            let vault = SpilloverVault::new();
            let cut = floor_utf8_boundary(&raw, keep_len.min(raw.len()));
            let compressed = raw[..cut].to_string();
            let privacy = SpilloverPrivacy::from_private_flag(is_private);
            let (primary, notice) = condense_text_with_spillover(
                &vault,
                "prop_tool",
                &raw,
                compressed,
                privacy,
            );
            let delivered_bytes = primary.len() + notice.as_deref().map_or(0, str::len);
            prop_assert!(
                delivered_bytes <= raw.len(),
                "delivered_bytes {} exceeded raw.len() {}",
                delivered_bytes,
                raw.len()
            );
        }

        #[test]
        fn prop_read_slice_always_respects_caps_and_privacy(
            lines in prop::collection::vec("[^\n]{0,120}", 1..500),
            start in 0u32..520u32,
            span in 1u32..600u32,
            write_private in any::<bool>(),
            read_private in any::<bool>(),
        ) {
            let vault = SpilloverVault::new();
            let raw = lines.join("\n");
            let w_priv = SpilloverPrivacy::from_private_flag(write_private);
            let r_priv = SpilloverPrivacy::from_private_flag(read_private);
            let receipt = vault.put("prop_tool", &raw, w_priv);

            let res = vault.read_slice(
                &receipt.uri,
                Some(start),
                Some(start.saturating_add(span)),
                r_priv,
            );

            if write_private && !read_private {
                let is_refused = matches!(res, Err(SpilloverError::PrivateBoundaryRefused { .. }));
                prop_assert!(is_refused, "expected PrivateBoundaryRefused");
            } else {
                let slice = res.expect("authorized slice read must succeed");
                prop_assert!(slice.text.len() <= MAX_SLICE_BYTES);
                if slice.end_line >= slice.start_line && slice.end_line > 0 {
                    prop_assert!(slice.end_line - slice.start_line < MAX_SLICE_LINES);
                }
            }
        }
    }
}
