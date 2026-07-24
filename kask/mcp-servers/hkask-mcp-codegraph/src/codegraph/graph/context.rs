//! Context assembly for LLM prompts (Component 11).
//!
//! Assembles token-budgeted code context from the graph.
//! The assembled context can then be fed to hkask-condenser for compression
//! before being sent to the LLM.

use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::codegraph::error::Result;

// Used in test module
#[cfg(test)]
use crate::codegraph::types::Symbol;

/// Budget tiers for context assembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudget {
    /// ~512 tokens: function signatures only.
    Minimal,
    /// ~2048 tokens: signatures + doc comments.
    Focused,
    /// ~4096 tokens: signatures + doc + key implementations.
    #[default]
    Standard,
    /// ~8192 tokens: everything relevant.
    Full,
}

impl ContextBudget {
    fn token_limit(self) -> usize {
        match self {
            ContextBudget::Minimal => 512,
            ContextBudget::Focused => 2048,
            ContextBudget::Standard => 4096,
            ContextBudget::Full => 8192,
        }
    }

    fn max_symbols(self) -> usize {
        match self {
            ContextBudget::Minimal => 10,
            ContextBudget::Focused => 20,
            ContextBudget::Standard => 40,
            ContextBudget::Full => 80,
        }
    }
}

/// An assembled context bundle, ready to send (or compress).
#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// Unique ID for feedback tracking (G6 fix).
    pub context_id: Uuid,
    /// The assembled text.
    pub text: String,
    /// Which symbols were included.
    pub symbols: Vec<String>,
    /// Estimated token count.
    pub estimated_tokens: usize,
}

/// Assemble context from the code graph for a given search query.
///
/// Searches for relevant symbols, ranks by importance (PageRank, if available),
/// and assembles a token-budgeted text block.
pub fn assemble_context(
    conn: &Connection,
    query: &str,
    budget: ContextBudget,
) -> Result<AssembledContext> {
    let context_id = Uuid::new_v4();

    // Step 1: Search for relevant symbols
    let search_results = super::search::search(conn, query, budget.max_symbols())?;

    if search_results.is_empty() {
        return Ok(AssembledContext {
            context_id,
            text: format!("[CodeGraph] No symbols found matching query: '{query}'"),
            symbols: vec![],
            estimated_tokens: estimate_tokens(&format!("No symbols for '{query}'")),
        });
    }

    // Step 2: Assemble context within token budget
    let mut lines = Vec::new();
    let mut tokens_used = 0;
    let token_limit = budget.token_limit();
    let mut included_symbols = Vec::new();

    for result in &search_results {
        let sym = &result.symbol;
        let chunk = match budget {
            ContextBudget::Minimal => format!(
                "// {}:{} — {}\n{}",
                sym.file, sym.start_line, sym.kind, sym.signature
            ),
            ContextBudget::Focused => {
                let mut s = format!(
                    "// {}:{} — {} ({})\n{}",
                    sym.file,
                    sym.start_line,
                    sym.kind,
                    match sym.visibility {
                        crate::codegraph::types::Visibility::Public => "pub",
                        crate::codegraph::types::Visibility::Crate => "pub(crate)",
                        crate::codegraph::types::Visibility::Private => "private",
                    },
                    sym.signature
                );
                if let Some(ref doc) = sym.doc_comment {
                    s.push_str(&format!("\n/// {doc}"));
                }
                s
            }
            ContextBudget::Standard => {
                format!(
                    "// {}:{} — {} lines {}-{}\n{}",
                    sym.file, sym.start_line, sym.kind, sym.start_line, sym.end_line, sym.signature
                )
            }
            ContextBudget::Full => {
                format!("// {sym}\n{sig}\n", sym = sym.name, sig = sym.signature,)
            }
        };

        let chunk_tokens = estimate_tokens(&chunk);
        if tokens_used + chunk_tokens > token_limit {
            break;
        }

        lines.push(chunk);
        tokens_used += chunk_tokens;
        included_symbols.push(sym.name.clone());
    }

    let header = format!(
        "[CodeGraph] {total} symbols found for '{query}', showing {shown} within {budget:?} budget ({limit} tokens):\n\n",
        total = search_results.len(),
        shown = included_symbols.len(),
        budget = budget,
        limit = token_limit,
    );

    let header_tokens = estimate_tokens(&header);
    let mut text = header;
    text.push_str(&lines.join("\n\n"));

    Ok(AssembledContext {
        context_id,
        text,
        symbols: included_symbols,
        estimated_tokens: tokens_used + header_tokens,
    })
}

/// Estimate tokens from character count (~4 chars per token).
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::graph::store::GraphStore;
    use crate::codegraph::types::{SymbolKind, Visibility};

    #[test]
    fn test_assemble_context_empty() {
        let store = GraphStore::open_in_memory().unwrap();
        let result = assemble_context(store.conn(), "nonexistent", ContextBudget::Minimal).unwrap();
        assert!(result.symbols.is_empty());
        assert!(result.text.contains("No symbols found"));
    }

    #[test]
    fn test_assemble_context_finds_symbols() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();
        store
            .insert_symbols(
                &[
                    Symbol {
                        id: None,
                        name: "auth::login".into(),
                        kind: SymbolKind::Function,
                        file: "test.rs".into(),
                        start_line: 10,
                        end_line: 20,
                        signature: "pub fn login(user: &str, pass: &str) -> Result<Token>".into(),
                        visibility: Visibility::Public,
                        doc_comment: Some("Authenticate a user".into()),
                        complexity: Default::default(),
                    },
                    Symbol {
                        id: None,
                        name: "auth::logout".into(),
                        kind: SymbolKind::Function,
                        file: "test.rs".into(),
                        start_line: 22,
                        end_line: 25,
                        signature: "pub fn logout(token: Token)".into(),
                        visibility: Visibility::Public,
                        doc_comment: Some("Log out a user".into()),
                        complexity: Default::default(),
                    },
                ],
                fid,
            )
            .unwrap();

        let result = assemble_context(store.conn(), "login", ContextBudget::Focused).unwrap();
        assert!(!result.symbols.is_empty());
        assert!(result.text.contains("login"));
        assert!(result.text.contains("Authenticate"));
        assert!(!result.context_id.is_nil());
    }

    #[test]
    fn test_budget_limits_symbols() {
        let store = GraphStore::open_in_memory().unwrap();
        let fid = store.upsert_file("test.rs", "abc").unwrap();
        let mut syms = Vec::new();
        for i in 0..50 {
            syms.push(Symbol {
                id: None,
                name: format!("fn_{i}"),
                kind: SymbolKind::Function,
                file: "test.rs".into(),
                start_line: i * 2,
                end_line: i * 2 + 1,
                signature: format!("fn fn_{i}()"),
                visibility: Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            });
        }
        store.insert_symbols(&syms, fid).unwrap();

        let result = assemble_context(store.conn(), "fn", ContextBudget::Minimal).unwrap();
        assert!(
            result.symbols.len() <= 10,
            "Minimal budget should cap at 10 symbols"
        );
    }
}
