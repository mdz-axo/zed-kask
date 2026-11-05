//! Shared live-shard fixture for the integration tests — one copy of the
//! HKASK_P0_GGUF_DIR-gated shard-collection preamble, consolidated from its
//! three copies per the duplication ratchet (a new instance of a pattern is
//! a signal to consolidate, not copy).

use std::path::PathBuf;

/// The HKASK_P0_GGUF_DIR shard set, sorted — or `None` when the probe env
/// var is unset (each caller keeps its own skip-or-fail handling).
///
/// # Panics
/// Panics when the set directory is unreadable or holds no `.gguf` shards:
/// a probe directory with no shards is a broken fixture, never a pass.
#[must_use]
pub fn p0_shard_paths() -> Option<Vec<PathBuf>> {
    let dir = std::env::var_os("HKASK_P0_GGUF_DIR")?;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read HKASK_P0_GGUF_DIR")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "gguf"))
        .collect();
    assert!(!paths.is_empty(), "no .gguf shards in {dir:?}");
    paths.sort();
    Some(paths)
}
