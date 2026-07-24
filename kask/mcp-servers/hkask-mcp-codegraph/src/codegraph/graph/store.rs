//! CRUD operations for the code graph.
//!
//! Provides batched insert for symbols and edges with automatic ID assignment
//! and call-target resolution. Uses prepared statement caching for performance.

use rusqlite::{Connection, params};

use crate::codegraph::error::Result;
use crate::codegraph::types::{EdgeKind, Symbol};

/// Load the sqlite-vec extension into a single connection.
///
/// This helper is intentionally duplicated in `hkask-storage-core` and here.
/// Extracting it to `hkask-database` would force sqlite-vec as a mandatory
/// dependency on every consumer of hkask-database, even those that never use
/// vector search. The duplication (15 lines) is the lesser evil — it keeps
/// hkask-database sqlite-vec-free and avoids unnecessary coupling.
///
/// Per-connection loading avoids `sqlite3_auto_extension`, whose
/// process-global registration is deprecated on Apple platforms and is a
/// known teardown-segfault source (the sqlite-vec author reports unreliable
/// segfaults from the auto-extension path). Scoping the extension's lifetime
/// to the connection means its state is torn down with the connection, not
/// orphaned at process exit. Must run before schema init, which creates
/// `vec0` virtual tables.
///
/// SAFETY: `sqlite3_vec_init` is the canonical C entry point
/// `int sqlite3_vec_init(sqlite3*, char**, const sqlite3_api_routines*)`.
/// The `sqlite_vec` crate declares it with no Rust args, so we transmute to
/// the real 3-arg signature and pass a live `sqlite3*` handle. The two
/// pointer args are NULL — the documented static-link invocation.
#[allow(unsafe_code)]
fn init_sqlite_vec_on(conn: &Connection) -> rusqlite::Result<()> {
    type Sqlite3ExtInitFn = unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *mut std::os::raw::c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::os::raw::c_int;
    // SAFETY: transmuting the zero-arg Rust import to the real 3-arg C entry
    // point is the documented sqlite-vec static-link pattern. The handle is
    // live for the duration of the call; the two pointer args are NULL.
    let init_fn: Sqlite3ExtInitFn = unsafe {
        std::mem::transmute::<_, Sqlite3ExtInitFn>(sqlite_vec::sqlite3_vec_init as *const ())
    };
    let rc = unsafe { init_fn(conn.handle(), std::ptr::null_mut(), std::ptr::null()) };
    if rc != rusqlite::ffi::SQLITE_OK {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rc),
            Some(format!("sqlite3_vec_init failed (rc={rc})")),
        ));
    }
    Ok(())
}

/// Store for code graph data. Wraps a rusqlite connection.
pub struct GraphStore {
    conn: Connection,
}

impl GraphStore {
    /// Open a store on an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_sqlite_vec_on(&conn)?;
        super::schema::initialize_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Open a store on a file-backed database.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        init_sqlite_vec_on(&conn)?;
        super::schema::initialize_schema(&conn)?;
        Ok(Self { conn })
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ── File tracking ──────────────────────────────────────────────

    /// Register or update a tracked file. Returns the file's database ID.
    pub fn upsert_file(&self, path: &str, content_hash: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO code_files (path, content_hash, indexed_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(path) DO UPDATE SET
                 content_hash = excluded.content_hash,
                 indexed_at = excluded.indexed_at",
            params![path, content_hash],
        )?;

        let id: i64 = self.conn.query_row(
            "SELECT id FROM code_files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )?;

        Ok(id)
    }

    /// Get the stored content hash for a file, if tracked.
    pub fn get_file_hash(&self, path: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT content_hash FROM code_files WHERE path = ?1")?;
        let result = stmt.query_row(params![path], |row| row.get(0)).ok();
        Ok(result)
    }

    // ── Symbol insertion ───────────────────────────────────────────

    /// Insert a batch of symbols. Returns the (name, id) mapping for **all**
    /// symbols in this file (not just newly inserted ones) — `INSERT OR IGNORE`
    /// means re-inserting an unchanged file is a no-op at the SQL level, but
    /// the returned mapping covers every symbol in the file so the caller can
    /// resolve edge targets by name regardless of whether the symbol was new.
    pub fn insert_symbols(&self, symbols: &[Symbol], file_id: i64) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR IGNORE INTO symbols
             (file_id, name, kind, signature, visibility, start_line, end_line, doc_comment, complexity_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        for sym in symbols {
            let visibility = match sym.visibility {
                crate::codegraph::types::Visibility::Public => "public",
                crate::codegraph::types::Visibility::Crate => "crate",
                crate::codegraph::types::Visibility::Private => "private",
            };
            let complexity_json = serde_json::to_string(&sym.complexity).unwrap_or_default();

            stmt.execute(params![
                file_id,
                sym.name,
                sym.kind.to_string(),
                sym.signature,
                visibility,
                sym.start_line as i64,
                sym.end_line as i64,
                sym.doc_comment,
                complexity_json,
            ])?;
        }

        // Return (name, id) mapping for all symbols in this file
        let mut map = Vec::new();
        let mut query = self
            .conn
            .prepare("SELECT name, id FROM symbols WHERE file_id = ?1")?;
        let rows = query.query_map(params![file_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            map.push(row?);
        }
        Ok(map)
    }

    /// Get a symbol by name. Returns the database ID if found.
    pub fn find_symbol_by_name(&self, name: &str) -> Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM symbols WHERE name = ?1 LIMIT 1")?;
        let result = stmt.query_row(params![name], |row| row.get(0)).ok();
        Ok(result)
    }

    // ── Edge insertion ─────────────────────────────────────────────

    /// Insert a single edge with resolved IDs.
    pub fn insert_edge(
        &self,
        from_id: i64,
        to_id: i64,
        kind: &EdgeKind,
        file_id: i64,
        line: usize,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO edges (from_id, to_id, kind, file_id, line)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![from_id, to_id, kind.to_string(), file_id, line as i64],
        )?;
        Ok(())
    }

    // ── Symbol queries ─────────────────────────────────────────────

    /// Get a symbol by its database ID.
    pub fn get_symbol(&self, id: i64) -> Result<Option<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.kind, f.path, s.signature, s.visibility, s.start_line, s.end_line, s.doc_comment, s.complexity_json, s.pagerank
             FROM symbols s JOIN code_files f ON s.file_id = f.id
             WHERE s.id = ?1",
        )?;
        let result = stmt
            .query_row(params![id], |row| {
                Ok(Symbol {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    kind: parse_kind(&row.get::<_, String>(2)?),
                    file: row.get(3)?,
                    signature: row.get(4)?,
                    visibility: parse_visibility(&row.get::<_, String>(5)?),
                    start_line: row.get::<_, i64>(6)? as usize,
                    end_line: row.get::<_, i64>(7)? as usize,
                    doc_comment: row.get(8)?,
                    complexity: parse_complexity(&row.get::<_, String>(9)?),
                })
            })
            .ok();
        Ok(result)
    }

    /// Count total symbols in the database.
    pub fn symbol_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Count total edges in the database.
    pub fn edge_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Count tracked files.
    pub fn file_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM code_files", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// ── Deserialization helpers ───────────────────────────────────────

pub(crate) fn parse_kind(s: &str) -> crate::codegraph::types::SymbolKind {
    match s {
        "function" => crate::codegraph::types::SymbolKind::Function,
        "method" => crate::codegraph::types::SymbolKind::Method,
        "struct" => crate::codegraph::types::SymbolKind::Struct,
        "enum" => crate::codegraph::types::SymbolKind::Enum,
        "variant" => crate::codegraph::types::SymbolKind::EnumVariant,
        "trait" => crate::codegraph::types::SymbolKind::Trait,
        "impl" => crate::codegraph::types::SymbolKind::Impl,
        "module" => crate::codegraph::types::SymbolKind::Module,
        "const" => crate::codegraph::types::SymbolKind::Const,
        "static" => crate::codegraph::types::SymbolKind::Static,
        "type_alias" => crate::codegraph::types::SymbolKind::TypeAlias,
        "macro" => crate::codegraph::types::SymbolKind::Macro,
        "test" => crate::codegraph::types::SymbolKind::Test,
        _ => crate::codegraph::types::SymbolKind::Function, // fallback
    }
}

pub(crate) fn parse_visibility(s: &str) -> crate::codegraph::types::Visibility {
    match s {
        "public" => crate::codegraph::types::Visibility::Public,
        "crate" => crate::codegraph::types::Visibility::Crate,
        _ => crate::codegraph::types::Visibility::Private,
    }
}

pub(crate) fn parse_complexity(json: &str) -> crate::codegraph::types::Complexity {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegraph::types::{Complexity, Visibility};

    #[test]
    fn test_upsert_and_query_file() {
        let store = GraphStore::open_in_memory().unwrap();

        let id1 = store.upsert_file("src/main.rs", "hash1").unwrap();
        let id2 = store.upsert_file("src/main.rs", "hash2").unwrap();

        // Same file should get the same ID
        assert_eq!(id1, id2);

        // Hash should be updated
        let hash = store.get_file_hash("src/main.rs").unwrap();
        assert_eq!(hash, Some("hash2".to_string()));
    }

    #[test]
    fn test_insert_symbols() {
        let store = GraphStore::open_in_memory().unwrap();
        let file_id = store.upsert_file("lib.rs", "abc").unwrap();

        let symbols = vec![
            Symbol {
                id: None,
                name: "main".into(),
                kind: crate::codegraph::types::SymbolKind::Function,
                file: "lib.rs".into(),
                start_line: 1,
                end_line: 5,
                signature: "fn main()".into(),
                visibility: Visibility::Public,
                doc_comment: Some("Entry point".into()),
                complexity: Complexity::NotComputed,
            },
            Symbol {
                id: None,
                name: "helper".into(),
                kind: crate::codegraph::types::SymbolKind::Function,
                file: "lib.rs".into(),
                start_line: 7,
                end_line: 9,
                signature: "fn helper()".into(),
                visibility: Visibility::Private,
                doc_comment: None,
                complexity: Complexity::Computed {
                    cyclomatic: 1,
                    cognitive: 0,
                },
            },
        ];

        let mapping = store.insert_symbols(&symbols, file_id).unwrap();
        assert_eq!(mapping.len(), 2);

        // Verify counts
        assert_eq!(store.symbol_count().unwrap(), 2);
    }
}
