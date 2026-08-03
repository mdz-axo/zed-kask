//! Incremental indexing pipeline.
//!
//! Coordinates the full flow:
//!   1. Walk files, compute BLAKE3 hashes
//!   2. Compare against stored hashes (skip unchanged files)
//!   3. Parse changed files with tree-sitter
//!   4. Extract symbols and edges
//!   5. Insert into database, resolve edge targets by name
//!
//! G1 fix: per-file BLAKE3 hash-on-read before any tool use.
//! Indexing is sequential — one file at a time, parse + extract + insert
//! all in the calling thread. Parallel parsing was considered but rejected
//! because `rusqlite::Connection` is `!Sync`, requiring a channel-based
//! writer or per-thread connections to do safely.

use std::collections::HashMap;
use std::path::Path;

use crate::codegraph::error::{IndexError, Result};
use crate::codegraph::graph::store::GraphStore;
use crate::codegraph::indexer::{extractor::extract_symbols, parser};
use crate::codegraph::types::{Edge, Symbol};

/// The indexing pipeline.
pub struct IndexPipeline {
    store: GraphStore,
}

/// Result of indexing a single file.
#[derive(Debug)]
pub struct FileIndexResult {
    pub path: String,
    pub symbols: usize,
    pub edges: usize,
    pub duration_ms: u64,
    pub skipped: bool,
}

impl IndexPipeline {
    /// Create a new pipeline backed by the given store.
    pub fn new(store: GraphStore) -> Self {
        Self { store }
    }

    /// Get a reference to the underlying store.
    pub fn store(&self) -> &GraphStore {
        &self.store
    }

    /// Index a single file. Returns the indexing result.
    ///
    /// If the file's content hash matches the stored hash, skips re-indexing
    /// and returns `skipped: true`.
    pub fn index_file(&self, path: &Path, relative_path: &str) -> Result<FileIndexResult> {
        let start = std::time::Instant::now();

        // Read file and compute hash
        let source = std::fs::read(path).map_err(|e| IndexError::FileNotAccessible {
            path: path.display().to_string(),
            source: Some(e),
        })?;
        let hash = blake3::hash(&source).to_hex().to_string();

        // Check if unchanged
        if let Some(stored_hash) = self.store.get_file_hash(relative_path)?
            && stored_hash == hash
        {
            return Ok(FileIndexResult {
                path: relative_path.to_string(),
                symbols: 0,
                edges: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                skipped: true,
            });
        }

        // Parse and extract
        let (tree, src_bytes) = parser::parse_rust_file(&source).map_err(|e| {
            crate::codegraph::error::CodeGraphError::Parse {
                file: relative_path.to_string(),
                message: e.to_string(),
            }
        })?;
        let (symbols, edges) = extract_symbols(&tree, &src_bytes, relative_path);

        // Insert into database
        let file_id = self.store.upsert_file(relative_path, &hash)?;
        let name_to_id = self.store.insert_symbols(&symbols, file_id)?;
        let inserted_edges =
            self.resolve_and_insert_edges(&edges, &name_to_id, &symbols, file_id)?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Emit Regulation event for indexed file (G7)
        tracing::info!(
            target: "hkask.codegraph.file_indexed",
            file = %relative_path,
            symbols = symbols.len(),
            edges = inserted_edges,
            duration_ms = duration_ms,
        );

        Ok(FileIndexResult {
            path: relative_path.to_string(),
            symbols: symbols.len(),
            edges: inserted_edges,
            duration_ms,
            skipped: false,
        })
    }

    /// Index all `.rs` files in a directory recursively.
    pub fn index_directory(&self, dir: &Path) -> Result<Vec<FileIndexResult>> {
        let mut results = Vec::new();
        let mut rs_files = Vec::new();

        // Collect all .rs files first
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                rs_files.push(path.to_path_buf());
            }
        }

        if rs_files.is_empty() {
            return Ok(results);
        }

        // Indexing is sequential — `rusqlite::Connection` is `!Sync`, so
        // parallel parsing would require a channel-based writer or per-thread
        // connections. The current design is simple and correct; parallelism is
        // a future optimization if profiling shows it's needed.
        let dir = dir.to_path_buf();
        for path in &rs_files {
            if let Ok(rel) = path.strip_prefix(&dir) {
                let rel_str = rel.to_string_lossy().to_string();
                match self.index_file(path, &rel_str) {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.codegraph",
                            file = %rel_str,
                            error = %e,
                            "Failed to index file"
                        );
                    }
                }
            }
        }

        Ok(results)
    }

    /// Resolve call/import/reference edges to database IDs and insert them.
    ///
    /// `to_id` resolution is **global** (against every symbol in the DB, not just
    /// the current file's): a call in file A frequently targets a symbol defined
    /// in file B, and the previous per-file-only map dropped every such cross-file
    /// edge. Resolution is deterministic and scope-aware:
    ///   1. exact full-qualified-name match (e.g. a top-level `fn foo` whose
    ///      qualified name is exactly the callee's last segment);
    ///   2. else last-segment match across all symbols;
    ///   3. within the chosen candidate set, if there is exactly one, resolve it;
    ///      if several, prefer a candidate defined in the same file as the edge;
    ///      if still ambiguous, leave the edge unresolved rather than picking an
    ///      arbitrary callee (the old HashMap-iteration pick produced
    ///      nondeterministic, wrong call edges).
    ///
    /// `from_id` resolution stays per-file (a call occurs inside a symbol in the
    /// current file, located by line-range containment).
    fn resolve_and_insert_edges(
        &self,
        edges: &[Edge],
        name_to_id: &[(String, i64)],
        symbols: &[Symbol],
        file_id: i64,
    ) -> Result<usize> {
        // Per-file map: qualified name -> id (for from_id / index_to_id only).
        let name_map: HashMap<&str, i64> = name_to_id
            .iter()
            .map(|(name, id)| (name.as_str(), *id))
            .collect();

        // Build a map from symbol index to database ID
        let index_to_id: HashMap<usize, i64> = symbols
            .iter()
            .enumerate()
            .filter_map(|(i, sym)| name_map.get(sym.name.as_str()).map(|&id| (i, id)))
            .collect();

        // Global to_id resolution maps across ALL symbols in the DB (this file's
        // just-inserted symbols plus every previously-indexed file).
        let global = self.store.all_symbols_with_file()?;
        let mut by_name: HashMap<&str, Vec<(i64, &str)>> = HashMap::new();
        let mut by_last_seg: HashMap<&str, Vec<(i64, &str)>> = HashMap::new();
        for (name, id, file) in &global {
            let name_ref = name.as_str();
            by_name
                .entry(name_ref)
                .or_default()
                .push((*id, file.as_str()));
            if let Some(last) = name.rsplit("::").next() {
                by_last_seg
                    .entry(last)
                    .or_default()
                    .push((*id, file.as_str()));
            }
        }

        let mut inserted = 0;
        for edge in edges {
            let from_id = self.find_containing_symbol(symbols, edge.line, &index_to_id);
            let to_id = resolve_target_global(edge, &by_name, &by_last_seg);

            if let (Some(from), Some(to)) = (from_id, to_id) {
                self.store
                    .insert_edge(from, to, &edge.kind, file_id, edge.line)?;
                inserted += 1;
            }
        }

        Ok(inserted)
    }

    /// Find which symbol contains the given line number.
    fn find_containing_symbol(
        &self,
        symbols: &[Symbol],
        line: usize,
        index_to_id: &HashMap<usize, i64>,
    ) -> Option<i64> {
        // Find the innermost symbol that contains this line
        symbols
            .iter()
            .enumerate()
            .filter(|(_, sym)| sym.start_line <= line && line <= sym.end_line)
            .min_by_key(|(_, sym)| sym.end_line - sym.start_line) // innermost (smallest span)
            .and_then(|(i, _)| index_to_id.get(&i).copied())
    }

    /// Finalize indexing: compute PageRank and emit a health event.
    ///
    /// (Staleness tracking was removed: the previous `staleness_seconds` field
    /// was never read by any Regulation sense input, and this log emitted a
    /// hardcoded `staleness_seconds = 0` that looked measured but wasn't.)
    pub fn finalize(&mut self) -> Result<()> {
        // Compute PageRank (G8)
        if let Err(e) = crate::codegraph::graph::ranking::compute_pagerank(self.store.conn()) {
            tracing::warn!(target: "hkask.codegraph.pagerank_failed", error = %e);
        }

        // Emit index health event (G7)
        let stats = self.stats()?;
        tracing::info!(
            target: "hkask.codegraph.index_health",
            total_symbols = stats.symbols,
            total_edges = stats.edges,
            files = stats.files,
        );

        Ok(())
    }

    /// Get index statistics.
    pub fn stats(&self) -> Result<IndexStats> {
        Ok(IndexStats {
            files: self.store.file_count()?,
            symbols: self.store.symbol_count()?,
            edges: self.store.edge_count()?,
        })
    }
}

/// Resolve an edge's `to_id` against the global symbol set.
///
/// `by_name` maps a fully-qualified name to its candidate symbols; `by_last_seg`
/// maps the last `::`-segment of a qualified name to candidates. Exact full-name
/// matches are preferred (more specific). When a candidate set has more than one
/// entry, a same-file candidate (relative to the edge's file) is preferred; ties
/// or no same-file match leave the edge unresolved rather than guessing, so call
/// edges never point at a nondeterministically-chosen wrong callee.
fn resolve_target_global<'a>(
    edge: &Edge,
    by_name: &HashMap<&'a str, Vec<(i64, &'a str)>>,
    by_last_seg: &HashMap<&'a str, Vec<(i64, &'a str)>>,
) -> Option<i64> {
    if edge.target_name.is_empty() {
        return None;
    }
    let target = edge.target_name.as_str();
    // Prefer an exact full-qualified-name match (e.g. a top-level `fn foo` whose
    // qualified name is exactly the callee). Fall back to last-segment matching.
    let candidates: &[(i64, &'a str)] = by_name
        .get(target)
        .filter(|v| !v.is_empty())
        .or_else(|| by_last_seg.get(target))
        .map(|v| v.as_slice())?;
    pick_candidate(candidates, edge.file.as_str())
}

/// Pick a single candidate from a set, preferring one in the same file as the
/// edge. Returns `None` when the set is ambiguous (no deterministic choice).
fn pick_candidate(candidates: &[(i64, &str)], edge_file: &str) -> Option<i64> {
    match candidates.len() {
        0 => None,
        1 => Some(candidates[0].0),
        _ => {
            let same_file: Vec<&(i64, &str)> =
                candidates.iter().filter(|(_, f)| *f == edge_file).collect();
            if same_file.len() == 1 {
                Some(same_file[0].0)
            } else {
                None
            }
        }
    }
}

/// Statistics about the indexed codebase.
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
}
