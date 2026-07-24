//! Core types for the code graph.
//!
//! D1 fix: Visibility is an enum, not a string (idiomatic-rust Principle 1).
//! D2 fix: Complexity is a type-state enum (`Option<usize>` → NotComputed | Computed | Unparseable).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A symbol extracted from source code — a function, struct, trait, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Database ID (assigned on insert).
    pub id: Option<i64>,
    /// Qualified name, e.g. "hkask_mcp_server::runtime::McpRuntime::start".
    pub name: String,
    /// What kind of symbol.
    pub kind: SymbolKind,
    /// File path relative to workspace root.
    pub file: String,
    /// Line range in the source file (1-based, inclusive).
    pub start_line: usize,
    pub end_line: usize,
    /// First line of the definition (signature).
    pub signature: String,
    /// Visibility: pub, pub(crate), or private.
    #[serde(default)]
    pub visibility: Visibility,
    /// Doc comment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
    /// Complexity (cyclomatic + cognitive). Computed lazily.
    #[serde(default)]
    pub complexity: Complexity,
}

/// The kind of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    EnumVariant,
    Trait,
    Impl,
    Module,
    Const,
    Static,
    TypeAlias,
    Macro,
    /// A test function (`#[test]` or `#[cfg(test)] mod tests`).
    Test,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::EnumVariant => "variant",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Module => "module",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Macro => "macro",
            SymbolKind::Test => "test",
        };
        write!(f, "{s}")
    }
}

/// Visibility of a symbol.
///
/// D1 fix: was `String`, now enum — invalid states unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// `pub` — visible everywhere.
    Public,
    /// `pub(crate)` or `pub(super)` — visible within crate.
    Crate,
    /// No visibility modifier — private.
    #[default]
    Private,
}

/// Complexity metrics for a symbol.
///
/// D2 fix: type-state enum replaces `Option<usize>`. Makes "has this been
/// computed?" a type-level question.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Complexity {
    /// Not yet computed.
    #[default]
    NotComputed,
    /// Successfully computed.
    Computed { cyclomatic: u32, cognitive: u32 },
    /// Parse error prevented computation (e.g., macro-heavy code).
    Unparseable,
}

/// An edge between two symbols — a relationship in the code graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Database ID.
    pub id: Option<i64>,
    /// Source symbol ID (caller / importer / container).
    pub from_id: i64,
    /// Target symbol ID (callee / importee / contained).
    pub to_id: i64,
    /// What kind of relationship.
    pub kind: EdgeKind,
    /// File where the relationship occurs.
    pub file: String,
    /// Line where the relationship occurs.
    pub line: usize,
    /// Target name for resolution (callee name, import path, etc.).
    /// Set by the extractor, used by the pipeline to resolve to_id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_name: String,
}

/// The kind of relationship between two symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// A function/method call.
    Calls,
    /// A `use` import or direct path reference.
    Imports,
    /// An `impl Trait for Type` relationship.
    Implements,
    /// A parent-child containment (module contains function, struct contains method).
    Contains,
    /// An ownership/passing reference (type appears in a field, parameter, or return type).
    References,
    /// A trait inheritance (`trait Foo: Bar`).
    Inherits,
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Implements => "implements",
            EdgeKind::Contains => "contains",
            EdgeKind::References => "references",
            EdgeKind::Inherits => "inherits",
        };
        write!(f, "{s}")
    }
}

/// Direction for graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Follow edges from source to target (dependencies).
    #[default]
    Forward,
    /// Follow edges from target to source (callers, dependents).
    Reverse,
}
