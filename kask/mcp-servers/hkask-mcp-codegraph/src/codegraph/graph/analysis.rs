//! Static analysis over the code graph.
//!
//! Provides:
//! - Dead code detection (symbols with zero inbound non-test edges)
//! - Complexity analysis (cyclomatic + cognitive from AST)

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::codegraph::error::Result;

/// A dead code finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeFinding {
    pub symbol_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
}

/// Find potentially dead code: symbols that have zero inbound edges
/// from non-test code, are not public, and are not in test modules.
pub fn find_dead_code(conn: &Connection) -> Result<Vec<DeadCodeFinding>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, f.path, s.start_line
         FROM symbols s
         JOIN code_files f ON s.file_id = f.id
         WHERE s.id NOT IN (
             SELECT DISTINCT to_id FROM edges
         )
         AND s.visibility != 'public'
         AND s.kind NOT IN ('module', 'test', 'variant')
         AND f.path NOT LIKE '%/tests/%'
         AND f.path NOT LIKE '%test%'
         ORDER BY s.name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(DeadCodeFinding {
            symbol_name: row.get(0)?,
            kind: row.get(1)?,
            file: row.get(2)?,
            line: row.get::<_, i64>(3)? as usize,
        })
    })?;

    let mut results = Vec::new();
    for finding in rows.flatten() {
        results.push(finding);
    }
    Ok(results)
}

/// A complexity finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityFinding {
    pub symbol_name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub cyclomatic: u32,
    pub cognitive: u32,
}

/// Find symbols with high complexity from the stored complexity data.
///
/// Returns symbols that have been computed and exceed the given thresholds.
pub fn find_high_complexity(
    conn: &Connection,
    min_cyclomatic: u32,
    min_cognitive: u32,
) -> Result<Vec<ComplexityFinding>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, f.path, s.start_line, s.complexity_json
         FROM symbols s
         JOIN code_files f ON s.file_id = f.id
         WHERE s.complexity_json IS NOT NULL
         AND s.complexity_json != '{\"NotComputed\"}'
         AND s.complexity_json != '{\"Unparseable\"}'
         ORDER BY s.name",
    )?;

    let mut results = Vec::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? as usize,
            row.get::<_, String>(4)?,
        ))
    })?;

    for (name, kind, file, line, json) in rows.flatten() {
        if let Ok(complexity) = serde_json::from_str::<serde_json::Value>(&json) {
            // Handle both tagged and untagged formats:
            // Tagged: {{"state":"computed","cyclomatic":12,"cognitive":8}}
            // Untagged: {{"Computed":{{"cyclomatic":12,"cognitive":8}}}}
            let (cyclomatic, cognitive) = if let Some(computed) = complexity.get("Computed") {
                (
                    computed["cyclomatic"].as_u64().unwrap_or(0) as u32,
                    computed["cognitive"].as_u64().unwrap_or(0) as u32,
                )
            } else {
                (
                    complexity["cyclomatic"].as_u64().unwrap_or(0) as u32,
                    complexity["cognitive"].as_u64().unwrap_or(0) as u32,
                )
            };
            if cyclomatic >= min_cyclomatic || cognitive >= min_cognitive {
                results.push(ComplexityFinding {
                    symbol_name: name,
                    kind,
                    file,
                    line,
                    cyclomatic,
                    cognitive,
                });
            }
        }
    }

    Ok(results)
}

/// Analyze the code graph and return all findings.
pub fn analyze(
    conn: &Connection,
    min_cyclomatic: u32,
    min_cognitive: u32,
) -> Result<AnalysisReport> {
    Ok(AnalysisReport {
        dead_code: find_dead_code(conn)?,
        high_complexity: find_high_complexity(conn, min_cyclomatic, min_cognitive)?,
    })
}

/// Combined analysis report.
#[derive(Debug, Clone)]
pub struct AnalysisReport {
    pub dead_code: Vec<DeadCodeFinding>,
    pub high_complexity: Vec<ComplexityFinding>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::{Complexity, Symbol, SymbolKind};

    #[test]
    fn test_dead_code_detection() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("src/lib.rs", "abc").unwrap();

        // Function with no incoming edges → dead code (if not public)
        store
            .insert_symbols(
                &[
                    Symbol {
                        id: None,
                        name: "unused_helper".into(),
                        kind: SymbolKind::Function,
                        file: "src/lib.rs".into(),
                        start_line: 1,
                        end_line: 3,
                        signature: "fn unused_helper()".into(),
                        visibility: crate::codegraph::types::Visibility::Private,
                        doc_comment: None,
                        complexity: Default::default(),
                    },
                    Symbol {
                        id: None,
                        name: "pub_api".into(),
                        kind: SymbolKind::Function,
                        file: "src/lib.rs".into(),
                        start_line: 5,
                        end_line: 7,
                        signature: "pub fn pub_api()".into(),
                        visibility: crate::codegraph::types::Visibility::Public,
                        doc_comment: None,
                        complexity: Default::default(),
                    },
                ],
                fid,
            )
            .unwrap();

        let dead = find_dead_code(store.conn()).unwrap();
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].symbol_name, "unused_helper");
    }

    #[test]
    fn test_complexity_analysis() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("src/main.rs", "abc").unwrap();

        store
            .insert_symbols(
                &[
                    Symbol {
                        id: None,
                        name: "simple".into(),
                        kind: SymbolKind::Function,
                        file: "src/main.rs".into(),
                        start_line: 1,
                        end_line: 3,
                        signature: "fn simple()".into(),
                        visibility: crate::codegraph::types::Visibility::Private,
                        doc_comment: None,
                        complexity: Complexity::Computed {
                            cyclomatic: 1,
                            cognitive: 0,
                        },
                    },
                    Symbol {
                        id: None,
                        name: "complex_fn".into(),
                        kind: SymbolKind::Function,
                        file: "src/main.rs".into(),
                        start_line: 5,
                        end_line: 30,
                        signature: "fn complex_fn()".into(),
                        visibility: crate::codegraph::types::Visibility::Private,
                        doc_comment: None,
                        complexity: Complexity::Computed {
                            cyclomatic: 12,
                            cognitive: 8,
                        },
                    },
                ],
                fid,
            )
            .unwrap();

        let high = find_high_complexity(store.conn(), 10, 5).unwrap();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].symbol_name, "complex_fn");
        assert_eq!(high[0].cyclomatic, 12);
    }
}
