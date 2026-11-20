//! The research database schema: the research-run ledger.

/// Schema DDL for use with `Database::open_with_extensions()`.
///
/// Creates the three run-ledger tables (research_runs,
/// research_run_status_history, run_sources) and one index.
///
/// The feed tables this server created until 2026-11-12 (feeds,
/// subscriptions, entries, entry_states, entries_fts, synthetic_feeds) are
/// no longer created and nothing reads or writes them. A database that
/// already holds them keeps them untouched: no statement here drops a table.
///
/// PRAGMA ordering invariant: `busy_timeout` MUST be set before
/// `journal_mode = WAL`. See `hkask_storage::database::init_wal_pragmas` for the
/// shared helper (not used here because this is a const DDL string, not
/// a function call, and the PRAGMAs are embedded in the DDL).
pub const RESEARCH_SCHEMA_DDL: &str = "
    PRAGMA busy_timeout=5000;
    PRAGMA journal_mode=WAL;
    PRAGMA foreign_keys=ON;

    -- Research-run ledger: the server's non-repudiable record of what its
    -- tools actually returned under a run, and the agent's declared
    -- verification states against that record.
    CREATE TABLE IF NOT EXISTS research_runs (
        run_id     TEXT PRIMARY KEY,  -- blake3(question || began_at)[..16]
        question   TEXT NOT NULL,
        status     TEXT NOT NULL,    -- planned|running|completed|partial|blocked|failed
        began_at   TEXT NOT NULL,    -- RFC 3339
        updated_at TEXT NOT NULL
    );

    -- run_status_history: one append-only row per status transition. The
    -- sources ledger is never mutated; the run row carries the LATEST
    -- status while this table preserves the full transition audit trail
    -- (operator ruling 2026-09-29, D6: runs are markable finished).
    CREATE TABLE IF NOT EXISTS research_run_status_history (
        run_id      TEXT NOT NULL REFERENCES research_runs(run_id) ON DELETE CASCADE,
        from_status TEXT NOT NULL,
        to_status   TEXT NOT NULL,    -- completed|partial|blocked|failed
        note        TEXT,
        at          TEXT NOT NULL,    -- RFC 3339
        PRIMARY KEY (run_id, at)
    );

    -- run_sources: one row per (run, url). `excerpt` is the audit copy of
    -- what the server actually returned (capped); `corpus_ref` is the
    -- agent's entity_ref for the durable recall copy ingested via the
    -- corpus server (the composition seam — different job, different
    -- owner). `recorded_by` distinguishes server-observed rows from
    -- agent-declared ones; `verified` requires a server-recorded row
    -- (Commit 3's validation gate).
    CREATE TABLE IF NOT EXISTS run_sources (
        run_id             TEXT NOT NULL REFERENCES research_runs(run_id),
        url                TEXT NOT NULL,
        provider           TEXT,
        title              TEXT,
        published          TEXT,
        source             TEXT,
        excerpt            TEXT,
        corpus_ref         TEXT,
        recorded_by        TEXT NOT NULL,
        verification_state TEXT,
        verification_basis TEXT,
        recorded_at        TEXT NOT NULL,
        PRIMARY KEY (run_id, url)
    );

    -- Cross-run audit queries (was a URL ever consulted?) need only
    -- the index now; a lookup tool waits for a named consumer.
    CREATE INDEX IF NOT EXISTS idx_run_sources_url ON run_sources(url);

";
