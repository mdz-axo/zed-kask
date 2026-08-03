//! PageRank computation for the code graph.
//!
//! Implements iterative PageRank over the symbol graph.
//! PR(A) = (1-d)/N + d * Σ(PR(B)/L(B)) for all B linking to A
//! where d = 0.85 (damping), N = total nodes, L(B) = out-degree of B.
//!
//! Runs in SQL with iterative updates for efficiency.

use rusqlite::Connection;

use crate::codegraph::error::Result;

/// Damping factor — probability of following an edge vs. jumping randomly.
const DAMPING: f64 = 0.85;

/// Maximum PageRank iterations.
const MAX_ITERATIONS: usize = 50;

/// Convergence threshold (L1 norm of delta).
const EPSILON: f64 = 1e-6;

/// Compute PageRank for all symbols and store in the `pagerank` column.
///
/// Returns the number of iterations until convergence.
pub fn compute_pagerank(conn: &Connection) -> Result<usize> {
    let n: f64 = conn.query_row("SELECT CAST(COUNT(*) AS REAL) FROM symbols", [], |row| {
        row.get(0)
    })?;

    if n == 0.0 {
        return Ok(0);
    }

    // Initialize all PageRank values to 1/N
    conn.execute(
        "UPDATE symbols SET pagerank = ?1",
        rusqlite::params![1.0 / n],
    )?;

    // Build out-degree lookup: for each node, count outgoing edges
    let random_jump = (1.0 - DAMPING) / n;

    // Pre-compute out-degrees. Nodes with no outgoing edges are dangling nodes;
    // their PageRank is distributed evenly across all nodes.
    // (Out-degrees are materialized into a temp table inside iterate_pagerank so
    // the per-link contribution divides by the linker's actual out-degree L(B),
    // not by N — the standard PageRank dilution across out-edges.)

    for iter in 1..=MAX_ITERATIONS {
        let delta = iterate_pagerank(conn, random_jump, n)?;

        if delta < EPSILON {
            tracing::info!(
                target: "hkask.codegraph",
                iterations = iter,
                delta = delta,
                "PageRank converged"
            );
            return Ok(iter);
        }
    }

    tracing::warn!(
        target: "hkask.codegraph",
        "PageRank did not converge within {MAX_ITERATIONS} iterations"
    );
    Ok(MAX_ITERATIONS)
}

// Out-degrees are materialized into a temp table inside iterate_pagerank.

/// Perform one iteration of PageRank. Returns the L1 norm of the change.
///
/// Out-degrees are materialized into a temp `out_deg` table so the per-link
/// contribution is `s2.pagerank / out_deg(s2)` (standard PageRank dilution),
/// not `s2.pagerank / N`. Dividing by N silently dropped the dilution of a
/// hub's rank across its out-edges, producing wrong (though often
/// order-preserving) PageRank values.
fn iterate_pagerank(conn: &Connection, random_jump: f64, n: f64) -> Result<f64> {
    // Compute dangling-node contribution (nodes with no outgoing edges)
    let dangling_sum: f64 = conn.query_row(
        "SELECT COALESCE(SUM(s.pagerank), 0.0)
         FROM symbols s
         WHERE s.id NOT IN (SELECT DISTINCT from_id FROM edges)",
        [],
        |row| row.get(0),
    )?;
    let dangling_contribution = DAMPING * dangling_sum / n;

    // Materialize out-degrees so the SQL can divide by the linker's actual
    // out-degree. Every `edges.from_id` has deg >= 1 by construction, so there
    // is no division-by-zero risk on the joined rows.
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS out_deg (
            id INTEGER PRIMARY KEY,
            deg INTEGER NOT NULL
        )",
    )?;
    conn.execute("DELETE FROM out_deg", [])?;
    conn.execute(
        "INSERT INTO out_deg (id, deg)
         SELECT from_id, COUNT(*) FROM edges GROUP BY from_id",
        [],
    )?;

    // Store new PageRank in a temporary table
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS pagerank_new (
            id INTEGER PRIMARY KEY,
            value REAL NOT NULL
        )",
    )?;
    conn.execute("DELETE FROM pagerank_new", [])?;

    // Insert new PageRank: sum of contributions from incoming edges, each
    // divided by the source node's out-degree (L(B)) and scaled by the damping
    // factor d. The damping multiplier on the link term was previously missing,
    // so the formula was `(1-d)/N + d*dangling/N + Σ PR(B)/L(B)` instead of the
    // standard `(1-d)/N + d*dangling/N + d*Σ PR(B)/L(B)`.
    conn.execute(
        "INSERT INTO pagerank_new (id, value)
         SELECT s.id,
                ?1 + ?2 + ?3 * COALESCE(
                    SUM(s2.pagerank / CAST(od.deg AS REAL)),
                    0.0
                )
         FROM symbols s
         LEFT JOIN edges e ON e.to_id = s.id
         LEFT JOIN symbols s2 ON e.from_id = s2.id
         LEFT JOIN out_deg od ON od.id = e.from_id
         GROUP BY s.id",
        rusqlite::params![random_jump, dangling_contribution, DAMPING],
    )?;

    // Handle nodes with no incoming edges — give them the random jump + dangling bonus
    conn.execute(
        "INSERT OR IGNORE INTO pagerank_new (id, value)
         SELECT s.id, ?1 + ?2
         FROM symbols s
         WHERE s.id NOT IN (SELECT DISTINCT to_id FROM edges)",
        rusqlite::params![random_jump, dangling_contribution],
    )?;

    // Compute delta (L1 norm of change) and update
    let delta: f64 = conn.query_row(
        "SELECT COALESCE(SUM(ABS(s.pagerank - p.value)), 0.0)
         FROM symbols s JOIN pagerank_new p ON s.id = p.id",
        [],
        |row| row.get(0),
    )?;

    // Apply new PageRank values
    conn.execute(
        "UPDATE symbols SET pagerank = (
            SELECT p.value FROM pagerank_new p WHERE p.id = symbols.id
        )",
        [],
    )?;

    // Normalize: ensure sum = 1.0
    let total: f64 = conn.query_row(
        "SELECT COALESCE(SUM(pagerank), 0.0) FROM symbols",
        [],
        |row| row.get(0),
    )?;
    if total > 0.0 {
        conn.execute(
            "UPDATE symbols SET pagerank = pagerank / ?1",
            rusqlite::params![total],
        )?;
    }

    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::{EdgeKind, Symbol, SymbolKind};

    #[test]
    fn test_pagerank_simple_graph() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();

        // Create a simple 3-node graph: A -> B -> C, A -> C
        let syms = vec![
            Symbol {
                id: None,
                name: "A".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 1,
                end_line: 3,
                signature: "fn A()".into(),
                visibility: crate::codegraph::types::Visibility::Public,
                doc_comment: None,
                complexity: Default::default(),
            },
            Symbol {
                id: None,
                name: "B".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 5,
                end_line: 7,
                signature: "fn B()".into(),
                visibility: crate::codegraph::types::Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            },
            Symbol {
                id: None,
                name: "C".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 9,
                end_line: 11,
                signature: "fn C()".into(),
                visibility: crate::codegraph::types::Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            },
        ];

        let mapping = store.insert_symbols(&syms, fid).unwrap();
        let ids: std::collections::HashMap<&str, i64> =
            mapping.iter().map(|(n, id)| (n.as_str(), *id)).collect();

        // A -> B, A -> C, B -> C
        store
            .insert_edge(ids["A"], ids["B"], &EdgeKind::Calls, fid, 2)
            .unwrap();
        store
            .insert_edge(ids["A"], ids["C"], &EdgeKind::Calls, fid, 3)
            .unwrap();
        store
            .insert_edge(ids["B"], ids["C"], &EdgeKind::Calls, fid, 6)
            .unwrap();

        let iterations = compute_pagerank(store.conn()).unwrap();
        assert!(iterations > 0, "PageRank should run at least 1 iteration");

        // C should have the highest PageRank (2 incoming edges)
        let rank_c: f64 = store
            .conn()
            .query_row("SELECT pagerank FROM symbols WHERE name = 'C'", [], |row| {
                row.get(0)
            })
            .unwrap();

        let rank_a: f64 = store
            .conn()
            .query_row("SELECT pagerank FROM symbols WHERE name = 'A'", [], |row| {
                row.get(0)
            })
            .unwrap();

        assert!(
            rank_c > rank_a,
            "C should have higher PageRank than A (2 incoming vs 0)"
        );
    }

    #[test]
    fn test_pagerank_empty_graph() {
        let store = GraphStore::open_in_memory().unwrap();
        let iterations = compute_pagerank(store.conn()).unwrap();
        assert_eq!(iterations, 0);
    }

    #[test]
    fn test_pagerank_dangling_node() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();

        // A single node with no edges — dangling node
        store
            .insert_symbols(
                &[Symbol {
                    id: None,
                    name: "lonely".into(),
                    kind: SymbolKind::Function,
                    file: "test.rs".into(),
                    start_line: 1,
                    end_line: 3,
                    signature: "fn lonely()".into(),
                    visibility: crate::codegraph::types::Visibility::Private,
                    doc_comment: None,
                    complexity: Default::default(),
                }],
                fid,
            )
            .unwrap();

        let iterations = compute_pagerank(store.conn()).unwrap();
        assert_eq!(iterations, 1); // should converge immediately

        let rank: f64 = store
            .conn()
            .query_row(
                "SELECT pagerank FROM symbols WHERE name = 'lonely'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!((rank - 1.0).abs() < 0.001, "Single node should have PR=1.0");
    }

    /// Reference PageRank in pure Rust, dividing each linker's contribution by
    /// its out-degree L(B). This is the falsifier for the `÷N` bug: the SQL
    /// implementation must match this reference within epsilon. The previous
    /// implementation divided by N (total nodes) and produced different values
    /// that were silently order-preserving on small graphs.
    fn reference_pagerank(edges: &[(usize, usize)], n: usize, d: f64, iters: usize) -> Vec<f64> {
        let mut pr = vec![1.0 / n as f64; n];
        let out_deg: Vec<usize> = (0..n)
            .map(|i| edges.iter().filter(|(f, _)| *f == i).count())
            .collect();
        for _ in 0..iters {
            let mut new = vec![0.0; n];
            let dangling: f64 = (0..n).filter(|i| out_deg[*i] == 0).map(|i| pr[i]).sum();
            for &(f, t) in edges {
                new[t] += d * pr[f] / out_deg[f] as f64;
            }
            for i in 0..n {
                new[i] += (1.0 - d) / n as f64 + d * dangling / n as f64;
            }
            let total: f64 = new.iter().sum();
            if total > 0.0 {
                for v in &mut new {
                    *v /= total;
                }
            }
            pr = new;
        }
        pr
    }

    #[test]
    fn test_pagerank_matches_out_degree_reference() {
        // Graph with a hub (out-deg 2) and a leaf (out-deg 1) so the dilution
        // across out-edges actually matters — a uniform `÷N` would diverge.
        //   A -> B, A -> C   (A out-deg 2)
        //   D -> B             (D out-deg 1)
        //   B -> C             (B out-deg 1)
        //   C is dangling
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();
        let names = ["A", "B", "C", "D"];
        let syms: Vec<Symbol> = names
            .iter()
            .map(|n| Symbol {
                id: None,
                name: (*n).into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 1,
                end_line: 2,
                signature: format!("fn {n}()"),
                visibility: crate::codegraph::types::Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            })
            .collect();
        let mapping = store.insert_symbols(&syms, fid).unwrap();
        let id_of: std::collections::HashMap<&str, i64> =
            mapping.iter().map(|(n, id)| (n.as_str(), *id)).collect();
        // Stable name -> reference-vector index (A=0,B=1,C=2,D=3). The reference
        // implementation indexes nodes positionally, so this map must align with
        // the `names` array order used by the assertion loop — NOT with the
        // randomized HashMap iteration order of `id_of.values()`.
        let name_index: std::collections::HashMap<&str, usize> =
            names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        // edges as (from_name, to_name)
        let edge_list: [(&str, &str); 4] = [("A", "B"), ("A", "C"), ("D", "B"), ("B", "C")];
        for (from, to) in edge_list {
            store
                .insert_edge(id_of[from], id_of[to], &EdgeKind::Calls, fid, 1)
                .unwrap();
        }

        compute_pagerank(store.conn()).unwrap();

        let ref_edges: Vec<(usize, usize)> = edge_list
            .iter()
            .map(|(f, t)| (name_index[*f], name_index[*t]))
            .collect();
        let reference = reference_pagerank(&ref_edges, names.len(), DAMPING, MAX_ITERATIONS);

        for (i, name) in names.iter().enumerate() {
            let actual: f64 = store
                .conn()
                .query_row(
                    "SELECT pagerank FROM symbols WHERE name = ?1",
                    rusqlite::params![name],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                (actual - reference[i]).abs() < 1e-4,
                "PageRank for {name}: SQL={actual:.6} reference={:.6} (÷out-degree)",
                reference[i]
            );
        }
    }
}
