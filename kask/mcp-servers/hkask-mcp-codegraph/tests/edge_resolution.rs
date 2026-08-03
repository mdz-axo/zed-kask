//! Integration test: index a multi-file fixture and assert the call-graph
//! topology that the unit tests on single in-memory graphs cannot exercise.
//!
//! Pins two behaviours fixed in this pass:
//!   * cross-file call resolution — a call in `caller.rs` to a symbol defined
//!     in `callee.rs` produces a `Calls` edge (the old per-file-only resolver
//!     dropped every cross-file edge);
//!   * deterministic ambiguous resolution — two symbols sharing a last
//!     segment (`shared` in both files) resolve a call to the SAME-FILE
//!     candidate, and the other-file candidate is NOT picked arbitrarily.

use hkask_mcp_codegraph::codegraph::graph::store::GraphStore;
use hkask_mcp_codegraph::codegraph::indexer::pipeline::IndexPipeline;
use std::fs;
use std::path::PathBuf;

/// `(caller_name, callee_name, callee_file, edge_kind)` for every edge.
fn call_edges(pipeline: &IndexPipeline) -> Vec<(String, String, String, String)> {
    let conn = pipeline.store().conn();
    let mut stmt = conn
        .prepare(
            "SELECT s_from.name, s_to.name, f_to.path, e.kind
             FROM edges e
             JOIN symbols s_from ON e.from_id = s_from.id
             JOIN symbols s_to   ON e.to_id   = s_to.id
             JOIN code_files f_to ON s_to.file_id = f_to.id
             ORDER BY s_from.name, s_to.name",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap();
    rows.filter_map(|r| r.ok()).collect()
}

fn fixture_dir() -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("hkask-codegraph-edge-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cross_file_and_ambiguous_call_resolution() {
    let dir = fixture_dir();
    let callee_path = dir.join("callee.rs");
    let caller_path = dir.join("caller.rs");
    fs::write(&callee_path, "pub fn alpha() {}\npub fn shared() {}\n").unwrap();
    fs::write(
        &caller_path,
        "pub fn caller() {\n    alpha();\n    shared();\n}\npub fn shared() {}\n",
    )
    .unwrap();

    let pipeline = IndexPipeline::new(GraphStore::open_in_memory().unwrap());

    // Index the callee FIRST so its symbols are in the DB when the caller's
    // edges are resolved. This mirrors the common case where a dependency is
    // indexed before its dependent; re-index would resolve the reverse order.
    pipeline.index_file(&callee_path, "callee.rs").unwrap();
    pipeline.index_file(&caller_path, "caller.rs").unwrap();

    let edges = call_edges(&pipeline);
    let calls: Vec<&(String, String, String, String)> =
        edges.iter().filter(|(_, _, _, k)| k == "calls").collect();

    // 1. Cross-file: caller (caller.rs) calls alpha (callee.rs).
    let cross = calls
        .iter()
        .find(|(from, to, to_file, _)| from == "caller" && to == "alpha" && to_file == "callee.rs");
    assert!(
        cross.is_some(),
        "expected a cross-file Calls edge caller -> alpha (callee.rs), got: {calls:?}"
    );

    // 2. Ambiguous same-name: the call to `shared` resolves to the SAME-FILE
    //    `shared` (caller.rs), deterministically.
    let same_file_shared = calls.iter().find(|(from, to, to_file, _)| {
        from == "caller" && to == "shared" && to_file == "caller.rs"
    });
    assert!(
        same_file_shared.is_some(),
        "expected the ambiguous `shared` call to resolve to caller.rs's shared, got: {calls:?}"
    );

    // 3. The other-file `shared` (callee.rs) must NOT be picked as a callee of
    //    `caller` — the old nondeterministic resolver sometimes did this.
    let wrong = calls
        .iter()
        .any(|(from, to, to_file, _)| from == "caller" && to == "shared" && to_file == "callee.rs");
    assert!(
        !wrong,
        "caller must not call callee.rs's shared (ambiguous resolution should prefer same-file), got: {calls:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
