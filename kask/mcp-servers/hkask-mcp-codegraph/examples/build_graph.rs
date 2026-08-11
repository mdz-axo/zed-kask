//! Standalone driver: build a code graph of a directory and run queries.
//!
//! Usage:
//!   cargo run --example build_graph --release -- <root_dir> [db_path]
//!
//! Defaults: root = repo root (`../../..` relative to this crate), db = in-memory.
//! Indexes every `.rs` file under <root_dir> with tree-sitter, computes
//! PageRank, then runs a handful of representative queries (stats, FTS5
//! search, reverse traversal / callers, top PageRank) and prints them.

use hkask_mcp_codegraph::codegraph::Direction;
use hkask_mcp_codegraph::codegraph::graph::search;
use hkask_mcp_codegraph::codegraph::graph::store::GraphStore;
use hkask_mcp_codegraph::codegraph::graph::traversal;
use hkask_mcp_codegraph::codegraph::indexer::pipeline::IndexPipeline;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../.."));
    let db_path = std::env::args().nth(2);

    println!("== codegraph driver ==");
    println!("root  : {}", root.display());
    println!(
        "db    : {}",
        db_path.clone().unwrap_or_else(|| "<in-memory>".into())
    );
    println!();

    let store = match db_path.as_deref() {
        Some(path) => GraphStore::open(path).expect("open store"),
        None => GraphStore::open_in_memory().expect("open in-memory store"),
    };
    let mut pipeline = IndexPipeline::new(store);

    // ── 1. Index ───────────────────────────────────────────────────────
    let t = Instant::now();
    let results = pipeline.index_directory(&root).expect("index_directory");
    let elapsed = t.elapsed();
    let files_indexed = results.iter().filter(|r| !r.skipped).count();
    let files_skipped = results.iter().filter(|r| r.skipped).count();
    let symbols: usize = results.iter().map(|r| r.symbols).sum();
    let edges: usize = results.iter().map(|r| r.edges).sum();
    println!("indexed   : {files_indexed} files (+{files_skipped} unchanged)");
    println!("symbols    : {symbols}");
    println!("edges      : {edges}");
    println!("elapsed    : {:.2?}", elapsed);
    println!();

    // ── 2. Finalize (PageRank) + stats ─────────────────────────────────
    pipeline.finalize().expect("finalize");
    let stats = pipeline.stats().expect("stats");
    println!(
        "stats      : files={}, symbols={}, edges={}",
        stats.files, stats.symbols, stats.edges
    );
    println!();

    let conn = pipeline.store().conn();

    // ── 3. FTS5 search: "resolve_mcp_binary" ───────────────────────────
    println!("--- search: \"resolve_mcp_binary\" ---");
    let hits = search::search(conn, "resolve_mcp_binary", 10).expect("search");
    for h in &hits {
        println!(
            "  {:<40} {:<10} {}:{}  (bm25={:.3})",
            h.symbol.name,
            format!("{:?}", h.symbol.kind),
            h.symbol.file,
            h.symbol.start_line,
            h.rank,
        );
    }
    println!();

    // ── 4. FTS5 search: "McpRuntime" ───────────────────────────────────
    println!("--- search: \"McpRuntime\" ---");
    let hits = search::search(conn, "McpRuntime", 10).expect("search");
    for h in &hits {
        println!(
            "  {:<40} {:<10} {}:{}  (bm25={:.3})",
            h.symbol.name,
            format!("{:?}", h.symbol.kind),
            h.symbol.file,
            h.symbol.start_line,
            h.rank,
        );
    }
    println!();

    // ── 5. Reverse traversal: who calls `resolve_mcp_binary`? ─────────
    let target_id: Option<i64> = conn
        .query_row(
            "SELECT id FROM symbols WHERE name = 'resolve_mcp_binary' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    if let Some(id) = target_id {
        println!("--- traverse(reverse) from resolve_mcp_binary (id={id}) ---");
        let nodes = traversal::traverse(conn, id, Direction::Reverse, 2).expect("traverse");
        if nodes.is_empty() {
            println!("  (no callers found — cross-file edges may be unresolved)");
        }
        for n in &nodes {
            println!(
                "  d{} [{:<8}] {:<40} {}:{}",
                n.depth, n.edge_kind, n.symbol.name, n.symbol.file, n.symbol.start_line,
            );
        }
    } else {
        println!("--- resolve_mcp_binary not found in graph ---");
    }
    println!();

    // ── 6. Top-10 symbols by PageRank ──────────────────────────────────
    println!("--- top-10 symbols by PageRank ---");
    let mut stmt = conn
        .prepare(
            "SELECT s.name, f.path, s.pagerank
             FROM symbols s JOIN code_files f ON s.file_id = f.id
             ORDER BY s.pagerank DESC LIMIT 10",
        )
        .expect("prepare");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })
        .expect("query_map");
    for r in rows.flatten() {
        println!("  {:.6}  {:<36} {}", r.2, r.0, r.1);
    }

    println!("\ndone.");
}
