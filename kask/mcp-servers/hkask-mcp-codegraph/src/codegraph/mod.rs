//! hkask-codegraph — native code understanding engine for hKask.
//!
//! Provides:
//! - Semantic code graph construction from Rust source (tree-sitter)
//! - SQLite-backed symbol + edge storage with FTS5 keyword search
//! - Recursive CTE graph traversal (forward/reverse dependencies)
//! - Impact analysis with risk classification
//! - Dead code detection and complexity analysis
//! - Token-budgeted context assembly for LLM prompts
//!
//! Design principles:
//! - Native Rust, zero external binaries
//! - Integrates with hKask Regulation, OCAP, condenser, and MCP framework
//! - Recursive CTE traversal (SQL, not in-memory) for persistence and concurrency

// sqlite-vec provides the vec0 virtual table extension at runtime;
// no Rust-level API calls, but the dependency is required.
use sqlite_vec as _;

pub mod error;
pub mod graph;
pub mod indexer;
pub mod types;

pub use error::{CodeGraphError, IndexError, Result};
pub use graph::analysis;
pub use graph::context::{AssembledContext, ContextBudget, assemble_context};
pub use graph::search::search;
pub use graph::traversal;
pub use indexer::pipeline::{FileIndexResult, IndexPipeline, IndexStats};
pub use types::{Complexity, Direction, Edge, EdgeKind, Symbol, SymbolKind, Visibility};
