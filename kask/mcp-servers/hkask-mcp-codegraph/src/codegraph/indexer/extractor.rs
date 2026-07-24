//! Extract symbols and edges from a tree-sitter CST.
//!
//! Walks the concrete syntax tree and produces:
//! - `Vec<Symbol>` — every function, struct, trait, impl, module, etc.
//! - `Vec<Edge>` — call relationships, imports, container relationships, impls.
//!
//! Qualified names are built by containment (walking up parent nodes),
//! matching CodeGraph's approach — works without language-specific logic.

use crate::codegraph::types::{Complexity, Edge, EdgeKind, Symbol, SymbolKind, Visibility};
use tree_sitter::Node;

/// Extract all symbols and edges from a parsed source file.
///
/// `file_path` is relative to the workspace root (used for `Symbol.file` and `Edge.file`).
pub fn extract_symbols(
    tree: &tree_sitter::Tree,
    source: &[u8],
    file_path: &str,
) -> (Vec<Symbol>, Vec<Edge>) {
    let mut extractor = Extractor {
        source,
        file_path: file_path.to_string(),
        symbols: Vec::new(),
        edges: Vec::new(),
        nesting: 0,
        cyclomatic: 0,
        cognitive: 0,
    };

    let root = tree.root_node();
    extractor.walk(&root, 0);

    (extractor.symbols, extractor.edges)
}

struct Extractor<'a> {
    source: &'a [u8],
    file_path: String,
    symbols: Vec<Symbol>,
    edges: Vec<Edge>,
    /// Current nesting depth (for cognitive complexity).
    nesting: usize,
    /// Current function's complexity counters.
    cyclomatic: u32,
    cognitive: u32,
}

impl<'a> Extractor<'a> {
    /// Recursively walk the CST, extracting symbols and edges at each node.
    fn walk(&mut self, node: &Node<'_>, depth: usize) {
        let kind = node.kind();

        // ── Complexity counting (G9) ─────────────────────────────
        // Count branch points for the current function.
        let is_block_like = matches!(
            kind,
            "if_statement"
                | "for_expression"
                | "while_expression"
                | "loop_expression"
                | "match_expression"
                | "block"
        );
        if is_block_like {
            self.nesting += 1;
        }

        match kind {
            "if_statement" | "else_if_clause" | "for_expression" | "while_expression"
            | "loop_expression" | "match_arm" => {
                self.cyclomatic += 1;
                self.cognitive += 1 + self.nesting.saturating_sub(1) as u32; // nesting penalty
            }
            "&&" | "||" => {
                self.cyclomatic += 1;
            }
            "return_expression" | "break_expression" | "continue_expression" => {
                // Only count early exits (not at depth 0 of function body)
                if depth > 1 {
                    self.cognitive += 1;
                }
            }
            _ => {}
        }

        // Track whether this node is a function — if so, we need to snapshot
        // complexity AFTER walking its body, not before (G14 fix: the previous
        // code snapshotted before the walk, so function N got function N-1's
        // complexity).
        let is_function = matches!(kind, "function_item" | "function_signature_item");
        // For functions, remember the Symbol index so we can update its complexity
        // after walking the body.
        let function_symbol_index = if is_function {
            self.extract_function(node, SymbolKind::Function)
        } else {
            None
        };

        if !is_function {
            match kind {
                // ── Declarations ──────────────────────────────────────────
                "struct_item" => {
                    self.extract_named_item(node, SymbolKind::Struct);
                }
                "enum_item" => {
                    self.extract_named_item(node, SymbolKind::Enum);
                    // Walk into enum body for variants *without* recursing into
                    // variant children (they're extracted in the variant handler).
                    self.walk_enum_variants(node);
                    return; // Don't double-walk children
                }
                "trait_item" => {
                    self.extract_named_item(node, SymbolKind::Trait);
                    // Extract trait bounds as Inherits edges.
                    self.extract_trait_bounds(node);
                }
                "impl_item" => {
                    self.extract_impl(node);
                }
                "mod_item" => {
                    self.extract_named_item(node, SymbolKind::Module);
                }
                "const_item" => {
                    self.extract_named_item(node, SymbolKind::Const);
                }
                "static_item" => {
                    self.extract_named_item(node, SymbolKind::Static);
                }
                "type_item" => {
                    self.extract_named_item(node, SymbolKind::TypeAlias);
                }
                "macro_definition" => {
                    self.extract_named_item(node, SymbolKind::Macro);
                }

                // ── Relationships ─────────────────────────────────────────
                "call_expression" => {
                    self.extract_call(node);
                }
                "use_declaration" => {
                    self.extract_import(node);
                }
                "field_declaration" => {
                    self.extract_field_reference(node);
                }

                _ => {}
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(&child, depth + 1);
        }

        // G14 fix: after walking a function's body, snapshot the accumulated
        // complexity onto the function's Symbol. The counters now hold THIS
        // function's complexity (not the previous function's).
        if let Some(idx) = function_symbol_index {
            let complexity = if self.cyclomatic > 1 || self.cognitive > 0 {
                Complexity::Computed {
                    cyclomatic: self.cyclomatic,
                    cognitive: self.cognitive,
                }
            } else {
                Complexity::NotComputed
            };
            if let Some(sym) = self.symbols.get_mut(idx) {
                sym.complexity = complexity;
            }
            // Reset counters so they don't leak into the next sibling function.
            // The parent context (module/impl) doesn't accumulate complexity.
            self.cyclomatic = 0;
            self.cognitive = 0;
        }

        // Decrement nesting after leaving block-like nodes (G9)
        if is_block_like {
            self.nesting = self.nesting.saturating_sub(1);
        }
    }

    // ── Declaration Extractors ───────────────────────────────────────

    /// Extract a function declaration. Returns the index of the pushed Symbol
    /// in `self.symbols`, so the caller (`walk`) can update its complexity
    /// after walking the function body (G14 fix: complexity must be
    /// snapshotted AFTER the body is walked, not before).
    fn extract_function(&mut self, node: &Node<'_>, kind: SymbolKind) -> Option<usize> {
        let name = self.child_text(node, "name");
        if name.is_empty() {
            return None;
        }

        // Reset complexity counters for this function. The body will be walked
        // by `walk()` after this returns, accumulating into these counters.
        // `walk()` then snapshots the accumulated values onto the Symbol.
        self.cyclomatic = 1; // base complexity = 1
        self.cognitive = 0;

        let qualified = self.qualified_name(node, &name);
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        let signature = self.first_line(node);
        let visibility = self.visibility(node);
        let doc = self.doc_comment(node);

        // Check for #[test] attribute
        let is_test = self.has_test_attribute(node);
        let kind = if is_test { SymbolKind::Test } else { kind };

        // Check for method (function inside impl block)
        let kind = if self.is_method(node) {
            SymbolKind::Method
        } else {
            kind
        };

        self.symbols.push(Symbol {
            id: None,
            name: qualified,
            kind,
            file: self.file_path.clone(),
            start_line,
            end_line,
            signature,
            visibility,
            doc_comment: doc,
            // Placeholder — `walk()` fills this in after walking the body.
            complexity: Complexity::NotComputed,
        });
        Some(self.symbols.len() - 1)
    }

    /// Extract a named declaration (struct, enum, trait, module, etc.).
    fn extract_named_item(&mut self, node: &Node<'_>, kind: SymbolKind) {
        let name = self.child_text(node, "name");
        if name.is_empty() {
            return;
        }

        let qualified = self.qualified_name(node, &name);
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        let signature = self.first_line(node);
        let visibility = self.visibility(node);
        let doc = self.doc_comment(node);

        self.symbols.push(Symbol {
            id: None,
            name: qualified,
            kind,
            file: self.file_path.clone(),
            start_line,
            end_line,
            signature,
            visibility,
            doc_comment: doc,
            complexity: Default::default(),
        });
    }

    /// Extract an impl block.
    fn extract_impl(&mut self, node: &Node<'_>) {
        let type_name = self.child_text(node, "type");
        let trait_name = self.child_text(node, "trait");

        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;

        if !trait_name.is_empty() && !type_name.is_empty() {
            // `impl Trait for Type` — create an impl symbol
            let qualified = format!("impl {trait_name} for {type_name}");
            let signature = self.first_line(node);

            self.symbols.push(Symbol {
                id: None,
                name: qualified,
                kind: SymbolKind::Impl,
                file: self.file_path.clone(),
                start_line,
                end_line,
                signature,
                visibility: Visibility::Private,
                doc_comment: None,
                complexity: Default::default(),
            });
        } else if !type_name.is_empty() {
            // `impl Type` — inherent impl
            // Walk children to find methods (handled by function_item recursion)
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk(&child, 0);
            }
        }
    }

    /// Walk enum body to extract variant declarations,
    /// then recurse into each variant's children.
    fn walk_enum_variants(&mut self, enum_node: &Node<'_>) {
        let body = enum_node.child_by_field_name("body");
        let Some(body) = body else {
            return;
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "enum_variant" {
                let name = self.child_text(&child, "name");
                if name.is_empty() {
                    continue;
                }

                let qualified = self.qualified_name(enum_node, &name);
                let start_line = child.start_position().row + 1;
                let signature = self.first_line(&child);

                self.symbols.push(Symbol {
                    id: None,
                    name: qualified,
                    kind: SymbolKind::EnumVariant,
                    file: self.file_path.clone(),
                    start_line,
                    end_line: child.end_position().row + 1,
                    signature,
                    visibility: Visibility::Public, // enum variants inherit visibility
                    doc_comment: self.doc_comment(&child),
                    complexity: Default::default(),
                });
            }
        }
    }

    /// Extract trait bounds (`trait Foo: Bar + Baz` → Inherits edges).
    fn extract_trait_bounds(&mut self, trait_node: &Node<'_>) {
        let mut cursor = trait_node.walk();
        for child in trait_node.children(&mut cursor) {
            if child.kind() == "supertrait" || child.kind() == "trait_bounds" {
                let text = self.node_text(&child);
                // Parse supertraits: "Bar + Baz"
                for bound in text.split('+') {
                    let bound = bound.trim();
                    if !bound.is_empty() {
                        let line = child.start_position().row + 1;
                        self.edges.push(Edge {
                            id: None,
                            from_id: 0, // placeholder — resolved after symbol insert
                            to_id: 0,   // placeholder
                            kind: EdgeKind::Inherits,
                            file: self.file_path.clone(),
                            line,
                            target_name: bound.to_string(),
                        });
                    }
                }
            }
        }
    }

    // ── Relationship Extractors ──────────────────────────────────────

    /// Extract a function call: `foo(args)` → Calls edge.
    fn extract_call(&mut self, node: &Node<'_>) {
        let func = node.child_by_field_name("function");
        let Some(func) = func else {
            return;
        };

        let callee_name = self.call_name(&func);
        if callee_name.is_empty() {
            return;
        }

        let line = node.start_position().row + 1;
        self.edges.push(Edge {
            id: None,
            from_id: 0, // placeholder
            to_id: 0,   // placeholder
            kind: EdgeKind::Calls,
            file: self.file_path.clone(),
            line,
            target_name: callee_name,
        });
    }

    /// Extract a use import.
    fn extract_import(&mut self, node: &Node<'_>) {
        // Extract the full use path as a string
        let text = self.node_text(node);
        // Strip "use " prefix
        let path = text.strip_prefix("use ").unwrap_or(&text).trim();
        // Strip trailing semicolon
        let path = path.strip_suffix(';').unwrap_or(path);
        // Take the last segment as the import name (e.g., "std::collections::HashMap" → "HashMap")
        let target_name = path.split("::").last().unwrap_or(path).to_string();

        if path.is_empty() {
            return;
        }

        let line = node.start_position().row + 1;
        self.edges.push(Edge {
            id: None,
            from_id: 0, // placeholder
            to_id: 0,   // placeholder
            kind: EdgeKind::Imports,
            file: self.file_path.clone(),
            line,
            target_name,
        });
    }

    /// Extract a field type reference for References edges.
    fn extract_field_reference(&mut self, node: &Node<'_>) {
        let ty = node.child_by_field_name("type");
        let Some(ty) = ty else {
            return;
        };

        let type_text = self.node_text(&ty).trim().to_string();
        if type_text.is_empty() || type_text == "Self" {
            return;
        }

        let line = node.start_position().row + 1;
        self.edges.push(Edge {
            id: None,
            from_id: 0, // placeholder
            to_id: 0,   // placeholder
            kind: EdgeKind::References,
            file: self.file_path.clone(),
            line,
            target_name: type_text,
        });
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /// Get the text of a direct child node by field name, falling back to
    /// a direct `identifier` child if the named field is not found.
    fn child_text(&self, node: &Node<'_>, field: &str) -> String {
        if let Some(n) = node.child_by_field_name(field) {
            return self.node_text(&n);
        }
        // Fallback: look for a direct `identifier` child (used by mod_item, etc.)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return self.node_text(&child);
            }
        }
        String::new()
    }

    /// Get the source text for a node.
    fn node_text(&self, node: &Node<'_>) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Get the first line of a node (used for signatures).
    fn first_line(&self, node: &Node<'_>) -> String {
        let text = self.node_text(node);
        text.lines().next().unwrap_or("").trim().to_string()
    }

    /// Build a qualified name by walking up containment.
    ///
    /// Follows CodeGraph's approach: join parent declaration names with `::`.
    /// For example, a function `start` inside `impl McpRuntime` inside `mod runtime`
    /// becomes `runtime::McpRuntime::start`.
    fn qualified_name(&self, node: &Node<'_>, own_name: &str) -> String {
        let mut parts = vec![own_name.to_string()];
        let mut current = node.parent();

        while let Some(parent) = current {
            let kind = parent.kind();
            // Only certain node kinds contribute to the qualified name.
            // `block` is intentionally excluded: bare blocks have no name field,
            // so including them was a dead arm that misled readers.
            if matches!(
                kind,
                "impl_item" | "trait_item" | "struct_item" | "enum_item" | "mod_item"
            ) {
                if let Some(name_node) = parent.child_by_field_name("name") {
                    let name = self.node_text(&name_node);
                    if !name.is_empty() && name != *own_name {
                        parts.push(name);
                    }
                }
                // For impl_item without a name, look for the type
                if kind == "impl_item"
                    && let Some(type_node) = parent.child_by_field_name("type")
                {
                    let type_name = self.node_text(&type_node);
                    if !type_name.is_empty() {
                        parts.push(type_name);
                    }
                }
            }
            current = parent.parent();
        }

        parts.reverse();
        parts.join("::")
    }

    /// Determine the visibility of a declaration node.
    fn visibility(&self, node: &Node<'_>) -> Visibility {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "visibility_modifier" => {
                    let text = self.node_text(&child);
                    if text.contains("crate") || text.contains("super") {
                        return Visibility::Crate;
                    }
                    if text.contains("pub") {
                        return Visibility::Public;
                    }
                }
                "pub" | "pub(crate)" | "pub(super)" => {
                    return if child.kind() == "pub" {
                        Visibility::Public
                    } else {
                        Visibility::Crate
                    };
                }
                _ => {}
            }
        }

        // Check if parent is an impl block → methods inherit visibility context.
        // For now, default to Private for items without explicit visibility.
        Visibility::Private
    }

    /// Extract doc comments preceding a node.
    fn doc_comment(&self, node: &Node<'_>) -> Option<String> {
        let mut prev = node.prev_sibling();
        let mut docs = Vec::new();

        while let Some(sibling) = prev {
            let kind = sibling.kind();
            if kind == "doc_comment" || kind == "block_comment" || kind == "line_comment" {
                let text = self.node_text(&sibling);
                // Only include doc comments (/// or //!), not regular comments (//).
                if text.starts_with("///") || text.starts_with("//!") {
                    let trimmed = text
                        .strip_prefix("/// ")
                        .or_else(|| text.strip_prefix("///"))
                        .or_else(|| text.strip_prefix("//! "))
                        .or_else(|| text.strip_prefix("//!"))
                        .unwrap_or(&text);
                    docs.push(trimmed.to_string());
                }
            } else if kind != "attribute_item" {
                // Stop at non-comment, non-attribute siblings
                break;
            }
            prev = sibling.prev_sibling();
        }

        docs.reverse();
        if docs.is_empty() {
            None
        } else {
            Some(docs.join("\n"))
        }
    }

    /// Check if a node has a `#[test]` attribute.
    fn has_test_attribute(&self, node: &Node<'_>) -> bool {
        let mut prev = node.prev_sibling();
        while let Some(sibling) = prev {
            match sibling.kind() {
                "attribute_item" => {
                    let text = self.node_text(&sibling);
                    if text.contains("test") {
                        return true;
                    }
                }
                "doc_comment" | "line_comment" | "block_comment" => {
                    // Keep looking past comments
                }
                _ => break,
            }
            prev = sibling.prev_sibling();
        }
        false
    }

    /// Check if a function is a method (declared inside an impl block).
    fn is_method(&self, node: &Node<'_>) -> bool {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == "impl_item" {
                return true;
            }
            if parent.kind() == "trait_item" {
                return true;
            }
            current = parent.parent();
        }
        false
    }

    /// Extract the call target name from a call expression's function node.
    fn call_name(&self, func: &Node<'_>) -> String {
        match func.kind() {
            "identifier" => self.node_text(func),
            "field_expression" => {
                // method call: self.foo() or obj.bar()
                let field = func.child_by_field_name("field");
                field.map(|n| self.node_text(&n)).unwrap_or_default()
            }
            "scoped_identifier" => {
                // qualified call: crate::module::func()
                let name = func.child_by_field_name("name");
                name.map(|n| self.node_text(&n)).unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    // ── Placeholder resolution note ──────────────────────────────────
    //
    // Edges are extracted with `from_id: 0, to_id: 0` placeholders.
    // These are resolved AFTER symbol insertion into SQLite:
    //  1. Insert all symbols → get back `(name, id)` mapping
    //  2. Insert all edges → resolve call targets by name lookup
    //
    // This is done by the `store` module (Component 2), not here.
    // The extractor only extracts raw (name, file, line) data.
}
