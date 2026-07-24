//! Parse Rust source files into tree-sitter concrete syntax trees.
//!
//! This module owns the tree-sitter lifecycle: language setup, parser creation,
//! and conversion of raw source into CST nodes.

use crate::codegraph::error::{CodeGraphError, Result};
use tree_sitter::Parser;

/// Parse a Rust source file into a tree-sitter tree.
///
/// Returns the parse tree along with the source bytes (needed for node text extraction).
pub fn parse_rust_file(source: &[u8]) -> Result<(tree_sitter::Tree, Vec<u8>)> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| CodeGraphError::Parse {
            file: "<unknown>".to_string(),
            message: format!("failed to set Rust language: {e}"),
        })?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CodeGraphError::Parse {
            file: "<unknown>".to_string(),
            message: "tree-sitter returned None (parse failed)".to_string(),
        })?;

    Ok((tree, source.to_vec()))
}

/// Parse a Rust source file from a path on disk.
pub fn parse_rust_file_at(path: &std::path::Path) -> Result<(tree_sitter::Tree, Vec<u8>)> {
    let source = std::fs::read(path).map_err(|e| CodeGraphError::Parse {
        file: path.display().to_string(),
        message: format!("failed to read file: {e}"),
    })?;
    parse_rust_file(&source)
}

/// Build a fresh parser. Used by the parallel indexing pipeline
/// where each thread needs its own parser instance.
pub fn new_parser() -> Result<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| CodeGraphError::Parse {
            file: "<unknown>".to_string(),
            message: format!("failed to set Rust language: {e}"),
        })?;
    Ok(parser)
}
