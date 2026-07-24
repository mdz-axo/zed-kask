//! FTS5 keyword search over the code graph.
//!
//! Uses SQLite FTS5 with BM25 ranking. Searches symbol names,
//! signatures, and doc comments.

use rusqlite::Connection;

use crate::codegraph::error::Result;
use crate::codegraph::types::Symbol;
use serde::{Deserialize, Serialize};

/// A search result with a relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub symbol: Symbol,
    /// BM25 score from FTS5 (lower = better match).
    pub rank: f64,
}

/// Search the code graph for symbols matching a query.
///
/// Uses FTS5 with BM25 ranking. The query supports FTS5 syntax:
/// - Simple terms: `McpRuntime`
/// - Prefix: `Mcp*`
/// - Phrase: `"run server"`
/// - Boolean: `server OR runtime`
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let mut results = Vec::new();

    // Try FTS5 first
    let sql = "SELECT s.id, s.name, s.kind, f.path, s.signature, s.visibility,
            s.start_line, s.end_line, s.doc_comment, s.complexity_json, s.pagerank,
            rank
     FROM symbols_fts
     JOIN symbols s ON symbols_fts.rowid = s.id
     JOIN code_files f ON s.file_id = f.id
     WHERE symbols_fts MATCH ?1
     ORDER BY rank
     LIMIT ?2";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
        Ok(SearchResult {
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
            rank: row.get(10)?,
        })
    })?;

    for result in rows.flatten() {
        results.push(result);
    }

    // If FTS5 returned nothing, fall back to LIKE
    if results.is_empty() {
        let like_query = format!("%{query}%");
        let sql = "SELECT s.id, s.name, s.kind, f.path, s.signature, s.visibility,
                s.start_line, s.end_line, s.doc_comment, s.complexity_json, s.pagerank
         FROM symbols s
         JOIN code_files f ON s.file_id = f.id
         WHERE s.name LIKE ?1
         ORDER BY s.pagerank DESC
         LIMIT ?2";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params![like_query, limit as i64], |row| {
            Ok(SearchResult {
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
                rank: 0.0, // no FTS5 rank for LIKE fallback
            })
        })?;

        for result in rows.flatten() {
            results.push(result);
        }
    }

    Ok(results)
}

/// Search by symbol name prefix (for autocomplete).
pub fn search_prefix(conn: &Connection, prefix: &str, limit: usize) -> Result<Vec<Symbol>> {
    let like = format!("{prefix}%");
    let sql = "SELECT s.id, s.name, s.kind, f.path, s.signature, s.visibility,
            s.start_line, s.end_line, s.doc_comment, s.complexity_json, s.pagerank
     FROM symbols s
     JOIN code_files f ON s.file_id = f.id
     WHERE s.name LIKE ?1
     ORDER BY s.name
     LIMIT ?2";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![like, limit as i64], |row| {
        Ok(Symbol {
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
        })
    })?;

    let mut results = Vec::new();
    for sym in rows.flatten() {
        results.push(sym);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::SymbolKind;

    #[test]
    fn test_search_finds_symbol() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();
        store
            .insert_symbols(
                &[Symbol {
                    id: None,
                    name: "test_function".into(),
                    kind: SymbolKind::Function,
                    file: "test.rs".into(),
                    start_line: 1,
                    end_line: 3,
                    signature: "fn test_function()".into(),
                    visibility: crate::codegraph::types::Visibility::Private,
                    doc_comment: None,
                    complexity: Default::default(),
                }],
                fid,
            )
            .unwrap();

        let results = search(store.conn(), "test_function", 10).unwrap();
        assert!(!results.is_empty(), "should find test_function");
        assert_eq!(results[0].symbol.name, "test_function");
    }

    #[test]
    fn test_search_like_fallback() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();
        store
            .insert_symbols(
                &[Symbol {
                    id: None,
                    name: "long_function_name".into(),
                    kind: SymbolKind::Function,
                    file: "test.rs".into(),
                    start_line: 1,
                    end_line: 3,
                    signature: "fn long_function_name()".into(),
                    visibility: crate::codegraph::types::Visibility::Private,
                    doc_comment: None,
                    complexity: Default::default(),
                }],
                fid,
            )
            .unwrap();

        // Search for a substring that FTS5 might not tokenize
        let results = search(store.conn(), "long_func", 10).unwrap();
        assert!(
            !results.is_empty(),
            "LIKE fallback should find partial match"
        );
    }
}
