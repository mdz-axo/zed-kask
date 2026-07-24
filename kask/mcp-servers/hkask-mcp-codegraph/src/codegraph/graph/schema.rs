//! SQLite schema for the code graph.
//!
//! Tables:
//! - `code_files` — tracked source files with content hashes
//! - `symbols` — extracted symbols (functions, structs, traits, etc.)
//! - `edges` — relationships between symbols (calls, imports, etc.)
//!
//! Virtual tables:
//! - `symbols_fts` — FTS5 for keyword search
//! - `symbols_vec` — sqlite-vec for semantic vector search (optional)
//!
//! Key design decisions:
//! - Recursive CTEs for graph traversal (not in-memory BFS/DFS)
//! - WAL mode for concurrent readers during write transactions
//! - Foreign keys enforced for edge integrity

use rusqlite::Connection;

/// Default embedding vector dimension. Must match the output dimension of
/// the configured embedding model. The default model
/// `DI/Qwen/Qwen3-Embedding-0.6B` produces 1024-dim vectors.
/// Override via the `HKASK_EMBEDDING_DIM` env var if using a different model.
///
/// **Migration note:** changing the dimension on an existing database requires
/// dropping and recreating the `symbols_vec` table — existing embeddings are
/// not compatible with a new dimension. The schema init uses `CREATE VIRTUAL
/// TABLE IF NOT EXISTS`, so it will NOT migrate an existing table.
pub const DEFAULT_EMBEDDING_DIM: usize = 1024;

/// Resolve the embedding dimension from the `HKASK_EMBEDDING_DIM` env var,
/// falling back to `DEFAULT_EMBEDDING_DIM` (1024, matching the default model
/// `DI/Qwen/Qwen3-Embedding-0.6B`).
fn resolve_embedding_dim() -> usize {
    std::env::var("HKASK_EMBEDDING_DIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&d: &usize| d > 0)
        .unwrap_or(DEFAULT_EMBEDDING_DIM)
}

/// Initialize the code graph schema in a SQLite connection.
///
/// Idempotent — safe to call on an existing database.
///
/// PRAGMA ordering invariant: `busy_timeout` MUST be set before
/// `journal_mode = WAL`. See `hkask_storage::database::init_wal_pragmas` for the
/// shared helper and rationale (this crate doesn't depend on
/// `hkask-database` to stay lightweight).
///
/// Embedding dimension: reads `HKASK_EMBEDDING_DIM` env var (default 1024).
/// If the existing `symbols_vec` table has a different dimension, the CREATE
/// IF NOT EXISTS is a no-op and inserts will fail — drop and recreate the
/// table manually to migrate dimensions.
pub fn initialize_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    conn.execute_batch(
        "
        -- Tracked source files
        CREATE TABLE IF NOT EXISTS code_files (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            path        TEXT NOT NULL UNIQUE,
            content_hash TEXT NOT NULL,
            indexed_at  TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Extracted symbols
        CREATE TABLE IF NOT EXISTS symbols (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            file_id         INTEGER NOT NULL REFERENCES code_files(id) ON DELETE CASCADE,
            name            TEXT NOT NULL,
            kind            TEXT NOT NULL,
            signature       TEXT NOT NULL DEFAULT '',
            visibility      TEXT NOT NULL DEFAULT 'private',
            start_line      INTEGER NOT NULL,
            end_line        INTEGER NOT NULL,
            doc_comment     TEXT,
            complexity_json TEXT,  -- JSON: {\"NotComputed\"} | {\"Computed\":{...}} | {\"Unparseable\"}
            pagerank        REAL NOT NULL DEFAULT 0.0,
            UNIQUE(file_id, name, kind, start_line)
        );

        -- Relationship edges between symbols
        CREATE TABLE IF NOT EXISTS edges (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id  INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
            to_id    INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
            kind     TEXT NOT NULL,
            file_id  INTEGER NOT NULL REFERENCES code_files(id) ON DELETE CASCADE,
            line     INTEGER NOT NULL,
            UNIQUE(from_id, to_id, kind, line)
        );

        -- Indexes for common queries
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_pagerank ON symbols(pagerank DESC);
        CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id);
        CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);
        CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
        ",
    )?;

    // FTS5 virtual table for keyword search
    // Uses content= to keep a single source of truth (the symbols table)
    conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
            name,
            signature,
            doc_comment,
            content='symbols',
            content_rowid='id'
        );
        ",
    )?;

    // Triggers to keep FTS5 in sync
    conn.execute_batch(
        "
        CREATE TRIGGER IF NOT EXISTS symbols_ai AFTER INSERT ON symbols BEGIN
            INSERT INTO symbols_fts(rowid, name, signature, doc_comment)
            VALUES (new.id, new.name, new.signature, new.doc_comment);
        END;

        CREATE TRIGGER IF NOT EXISTS symbols_ad AFTER DELETE ON symbols BEGIN
            INSERT INTO symbols_fts(symbols_fts, rowid, name, signature, doc_comment)
            VALUES ('delete', old.id, old.name, old.signature, old.doc_comment);
        END;

        CREATE TRIGGER IF NOT EXISTS symbols_au AFTER UPDATE ON symbols BEGIN
            INSERT INTO symbols_fts(symbols_fts, rowid, name, signature, doc_comment)
            VALUES ('delete', old.id, old.name, old.signature, old.doc_comment);
            INSERT INTO symbols_fts(rowid, name, signature, doc_comment)
            VALUES (new.id, new.name, new.signature, new.doc_comment);
        END;
        ",
    )?;

    // sqlite-vec virtual table for semantic search (G13)
    // Requires sqlite-vec runtime extension. If not loaded, vector search is unavailable
    // but FTS5 keyword search still works.
    //
    // Dimension is configurable via HKASK_EMBEDDING_DIM (default 1024, matching
    // DI/Qwen/Qwen3-Embedding-0.6B). The previous hardcoded 384 was wrong for
    // the default model — INSERTs silently failed via INSERT OR IGNORE.
    let embedding_dim = resolve_embedding_dim();
    let vec_sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS symbols_vec USING vec0(
            embedding float[{embedding_dim}] distance_metric=cosine
        );"
    );
    if let Err(e) = conn.execute_batch(&vec_sql) {
        tracing::warn!(
            target: "hkask.codegraph",
            error = %e,
            embedding_dim = embedding_dim,
            "sqlite-vec not available — vector search disabled"
        );
    }

    Ok(())
}

/// Drop and recreate all code graph tables. For testing only.
#[cfg(test)]
pub fn reset_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        DROP TABLE IF EXISTS edges;
        DROP TABLE IF EXISTS symbols;
        DROP TABLE IF EXISTS code_files;
        DROP TABLE IF EXISTS symbols_fts;
        ",
    )?;
    initialize_schema(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_initialize_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        // Second call should succeed without errors
        initialize_schema(&conn).unwrap();
    }

    #[test]
    fn test_insert_and_query_symbol() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Insert a file
        conn.execute(
            "INSERT INTO code_files (path, content_hash) VALUES (?1, ?2)",
            rusqlite::params!["src/main.rs", "abc123"],
        )
        .unwrap();

        // Insert a symbol
        conn.execute(
            "INSERT INTO symbols (file_id, name, kind, signature, visibility, start_line, end_line)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![1, "main", "function", "fn main()", "public", 1, 5],
        )
        .unwrap();

        // Query via FTS5
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols_fts WHERE symbols_fts MATCH 'main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_and_query_edges() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO code_files (path, content_hash) VALUES (?1, ?2)",
            rusqlite::params!["lib.rs", "def456"],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO symbols (file_id, name, kind, signature, start_line, end_line)
             VALUES (1, 'caller', 'function', 'fn caller()', 1, 3)",
            rusqlite::params![],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbols (file_id, name, kind, signature, start_line, end_line)
             VALUES (1, 'callee', 'function', 'fn callee()', 5, 7)",
            rusqlite::params![],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO edges (from_id, to_id, kind, file_id, line)
             VALUES (1, 2, 'calls', 1, 2)",
            rusqlite::params![],
        )
        .unwrap();

        // Recursive CTE: traverse forward from caller
        let mut stmt = conn
            .prepare(
                "WITH RECURSIVE deps AS (
                    SELECT e.to_id, s.name, s.kind, e.kind as edge_kind, 1 as depth
                    FROM edges e JOIN symbols s ON e.to_id = s.id
                    WHERE e.from_id = 1
                    UNION
                    SELECT e.to_id, s.name, s.kind, e.kind, d.depth + 1
                    FROM edges e
                    JOIN symbols s ON e.to_id = s.id
                    JOIN deps d ON e.from_id = d.to_id
                    WHERE d.depth < 10
                )
                SELECT DISTINCT name FROM deps ORDER BY depth",
            )
            .unwrap();

        let names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(names, vec!["callee"]);
    }
}
