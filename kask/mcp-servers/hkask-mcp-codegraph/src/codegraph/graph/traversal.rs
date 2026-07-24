//! Graph traversal via recursive CTEs.
//!
//! All traversal is done in SQL using recursive Common Table Expressions.
//! This means:
//! - No in-memory graph loading (memory-efficient)
//! - Concurrent-safe reads (SQLite WAL mode)
//! - Persisted results (can be cached as views)

use rusqlite::Connection;

use crate::codegraph::error::Result;
use crate::codegraph::types::{Direction, Symbol};
use serde::{Deserialize, Serialize};

/// A node in a traversal result, with metadata about its position in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalNode {
    pub symbol: Symbol,
    /// Distance from the start symbol (1 = direct neighbor).
    pub depth: usize,
    /// The kind of edge that led to this node.
    pub edge_kind: String,
}

/// Traverse the graph from a symbol in the given direction.
///
/// - `Forward`: follow `from_id → to_id` edges (dependencies)
/// - `Reverse`: follow `to_id → from_id` edges (callers/dependents)
pub fn traverse(
    conn: &Connection,
    symbol_id: i64,
    direction: Direction,
    max_depth: usize,
) -> Result<Vec<TraversalNode>> {
    let (from_col, to_col) = match direction {
        Direction::Forward => ("from_id", "to_id"),
        Direction::Reverse => ("to_id", "from_id"),
    };

    let sql = format!(
        "WITH RECURSIVE trav AS (
            SELECT e.{to_col} AS node_id, s.name, s.kind, f.path, s.signature,
                   s.visibility, s.start_line, s.end_line, s.doc_comment,
                   s.complexity_json, s.pagerank,
                   e.kind AS edge_kind, 1 AS depth
            FROM edges e
            JOIN symbols s ON e.{to_col} = s.id
            JOIN code_files f ON s.file_id = f.id
            WHERE e.{from_col} = ?1

            UNION

            SELECT e.{to_col}, s.name, s.kind, f.path, s.signature,
                   s.visibility, s.start_line, s.end_line, s.doc_comment,
                   s.complexity_json, s.pagerank,
                   e.kind, t.depth + 1
            FROM edges e
            JOIN symbols s ON e.{to_col} = s.id
            JOIN code_files f ON s.file_id = f.id
            JOIN trav t ON e.{from_col} = t.node_id
            WHERE t.depth < ?2
        )
        SELECT DISTINCT node_id, name, kind, path, signature, visibility,
               start_line, end_line, doc_comment, complexity_json, pagerank,
               edge_kind, depth
        FROM trav
        ORDER BY depth, name"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![symbol_id, max_depth as i64], |row| {
        Ok(TraversalNode {
            symbol: Symbol {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                kind: super::store::parse_kind(&row.get::<_, String>(2)?),
                file: row.get(3)?,
                signature: row.get(4)?,
                visibility: super::store::parse_visibility(&row.get::<_, String>(5)?),
                start_line: row.get::<_, i64>(6)? as usize,
                end_line: row.get::<_, i64>(7)? as usize,
                doc_comment: row.get(8)?,
                complexity: super::store::parse_complexity(&row.get::<_, String>(9)?),
            },
            depth: row.get::<_, i64>(12)? as usize,
            edge_kind: row.get(11)?,
        })
    })?;

    let mut results = Vec::new();
    for node in rows.flatten() {
        results.push(node);
    }
    Ok(results)
}

/// Impact analysis: find all symbols transitively dependent on a given symbol,
/// classified by risk level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    pub symbol: Symbol,
    pub depth: usize,
    pub risk: RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        };
        write!(f, "{s}")
    }
}

/// Analyze the blast radius of changing a symbol.
///
/// Returns all symbols that transitively depend on the given symbol,
/// with each classified by risk: Critical (public traits), High (public types),
/// Medium (implementations), Low (private/test code).
pub fn impact_analysis(
    conn: &Connection,
    symbol_id: i64,
    max_depth: usize,
) -> Result<Vec<ImpactResult>> {
    // Forward traversal — find everything that depends on this symbol.
    // Actually, we want REVERSE — who calls/imports this symbol?
    let dependents = traverse(conn, symbol_id, Direction::Reverse, max_depth)?;

    let results: Vec<ImpactResult> = dependents
        .into_iter()
        .map(|node| {
            let risk = classify_risk(&node.symbol);
            ImpactResult {
                symbol: node.symbol,
                depth: node.depth,
                risk,
            }
        })
        .collect();

    Ok(results)
}

/// Classify the risk level of changing a symbol based on its kind and visibility.
fn classify_risk(symbol: &Symbol) -> RiskLevel {
    use crate::codegraph::types::{SymbolKind, Visibility};

    match (&symbol.kind, &symbol.visibility) {
        // Critical: public traits — changing these breaks external contract
        (SymbolKind::Trait, Visibility::Public) => RiskLevel::Critical,
        // High: public types and functions
        (_, Visibility::Public) => RiskLevel::High,
        // Medium: crate-visible types and implementations
        (SymbolKind::Impl, _) | (_, Visibility::Crate) => RiskLevel::Medium,
        // Low: everything else (private, test code)
        _ => RiskLevel::Low,
    }
}

/// Find the symbol ID by name. Returns `None` if not found.
pub fn find_symbol_id(conn: &Connection, name: &str) -> Result<Option<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM symbols WHERE name = ?1 LIMIT 1")?;
    let result = stmt
        .query_row(rusqlite::params![name], |row| row.get(0))
        .ok();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::{EdgeKind, SymbolKind, Visibility};

    fn setup_graph() -> GraphStore {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();

        // Create symbols
        let syms = vec![
            Symbol {
                id: None,
                name: "caller".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 1,
                end_line: 5,
                signature: "fn caller()".into(),
                visibility: Visibility::Public,
                doc_comment: None,
                complexity: Default::default(),
            },
            Symbol {
                id: None,
                name: "callee".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 7,
                end_line: 9,
                signature: "fn callee()".into(),
                visibility: Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            },
            Symbol {
                id: None,
                name: "deep_callee".into(),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: 11,
                end_line: 13,
                signature: "fn deep_callee()".into(),
                visibility: Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            },
        ];

        let mapping = store.insert_symbols(&syms, fid).unwrap();
        let name_to_id: std::collections::HashMap<&str, i64> =
            mapping.iter().map(|(n, id)| (n.as_str(), *id)).collect();

        let caller_id = name_to_id["caller"];
        let callee_id = name_to_id["callee"];
        let deep_id = name_to_id["deep_callee"];

        // caller → callee
        store
            .insert_edge(caller_id, callee_id, &EdgeKind::Calls, fid, 3)
            .unwrap();

        // callee → deep_callee
        store
            .insert_edge(callee_id, deep_id, &EdgeKind::Calls, fid, 8)
            .unwrap();

        store
    }

    #[test]
    fn test_traverse_forward() {
        let store = setup_graph();
        let caller_id = find_symbol_id(store.conn(), "caller").unwrap().unwrap();

        let results = traverse(store.conn(), caller_id, Direction::Forward, 10).unwrap();

        // Should find callee (depth 1) and deep_callee (depth 2)
        let names: Vec<String> = results.iter().map(|n| n.symbol.name.clone()).collect();
        assert!(
            names.contains(&"callee".to_string()),
            "should find callee, got: {names:?}"
        );
        assert!(
            names.contains(&"deep_callee".to_string()),
            "should find deep_callee transitively, got: {names:?}"
        );
    }

    #[test]
    fn test_traverse_reverse() {
        let store = setup_graph();
        let deep_id = find_symbol_id(store.conn(), "deep_callee")
            .unwrap()
            .unwrap();

        let results = traverse(store.conn(), deep_id, Direction::Reverse, 10).unwrap();

        let names: Vec<String> = results.iter().map(|n| n.symbol.name.clone()).collect();
        assert!(
            names.contains(&"callee".to_string()),
            "should find callee as caller of deep_callee"
        );
        assert!(
            names.contains(&"caller".to_string()),
            "should find caller transitively"
        );
    }

    #[test]
    fn test_impact_analysis() {
        let store = setup_graph();
        let callee_id = find_symbol_id(store.conn(), "callee").unwrap().unwrap();

        let results = impact_analysis(store.conn(), callee_id, 10).unwrap();

        // caller depends on callee (reverse traversal from callee)
        let callers: Vec<&str> = results.iter().map(|r| r.symbol.name.as_str()).collect();
        assert!(
            callers.contains(&"caller"),
            "impact analysis should find caller"
        );
    }
}
