#![forbid(unsafe_code)]
//! `purge-orphaned-vectors` — reclaim vec0 shadow rows whose metadata row is
//! gone.
//!
//! # Why this exists
//!
//! `vec_embeddings` is keyed on the implicit integer rowid, joined to
//! `embeddings` on that rowid at query time. When a metadata row is deleted
//! through a path that does not also delete its vector — a therapy SQL pass,
//! an interrupted re-embed, a raw `DELETE FROM embeddings` — the vector row
//! survives as an interior orphan. KNN's inner join already ignores it, so it
//! is invisible to retrieval; it only costs space. On the zk-ref-open
//! authoring databases the residue measured 32,797 rows across four files.
//!
//! The distribution path purges this at export (`rebuild_vector_index`
//! deletes and rebuilds vec0 from canonical rows), so shipped copies are
//! clean. The authoring databases are not, and nothing swept them.
//!
//! # Why a binary rather than a SQL one-liner
//!
//! `DELETE FROM vec_embeddings` must go through the vec0 module so its shadow
//! tables (`_rowids`, `_chunks`, `_vector_chunks00`, `_info`) stay
//! consistent. The bundled sqlite-vec extension is statically linked into
//! this crate and loaded per connection by `Database::open`; the system
//! `sqlcipher` CLI has no vec0 module, so issuing the delete there would
//! corrupt the index. This binary therefore opens each database through the
//! same pool every production reader uses.
//!
//! # Usage
//!
//!   purge-orphaned-vectors <db> [<db> ...]
//!
//! The passphrase resolves from `HKASK_DB_PASSPHRASE` (the same variable the
//! MCP servers receive). Each database is processed independently: a failure
//! on one is reported and does not stop the others. Exit is non-zero if any
//! database failed.
//!
//! Each database is checkpointed, swept, and vacuumed. A database that is
//! open read-write elsewhere must not be swept concurrently — the exclusive
//! maintenance lease enforces this and reports `ConsumersActive` rather than
//! interleaving writes.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use hkask_storage::EmbeddingStore;
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;

fn main() -> ExitCode {
    let databases: Vec<String> = std::env::args().skip(1).collect();
    if databases.is_empty() {
        eprintln!("usage: purge-orphaned-vectors <db> [<db> ...]");
        return ExitCode::from(2);
    }
    let passphrase = match std::env::var("HKASK_DB_PASSPHRASE") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => {
            eprintln!("HKASK_DB_PASSPHRASE is set but empty — refusing an empty-key open");
            return ExitCode::from(69);
        }
        Err(_) => {
            eprintln!(
                "HKASK_DB_PASSPHRASE is not set — it is the variable the MCP servers receive; \
                 export it before running this sweep"
            );
            return ExitCode::from(69);
        }
    };

    let mut failures = 0_usize;
    for path in &databases {
        match sweep_one(path, &passphrase) {
            Ok(outcome) => {
                println!(
                    "{path}\tembeddings={}\torphans_removed={}\tremaining={}",
                    outcome.embeddings, outcome.removed, outcome.remaining
                );
            }
            Err(error) => {
                eprintln!("{path}: FAILED — {error}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!("{failures} of {} database(s) failed", databases.len());
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

struct Outcome {
    embeddings: usize,
    removed: usize,
    remaining: usize,
}

fn sweep_one(path: &str, passphrase: &str) -> Result<Outcome, String> {
    if !Path::new(path).is_file() {
        return Err("not found".to_string());
    }
    let database = hkask_storage::Database::open(path, passphrase)
        .map_err(|error| format!("open failed: {error}"))?;
    let pool = database
        .sqlite_pool()
        .map_err(|error| format!("pool failed: {error}"))?;
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool.clone(), path));
    let embeddings = EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())
        .map_err(|error| format!("embedding store failed: {error}"))?;

    let before = embeddings
        .count()
        .map_err(|error| format!("count failed: {error}"))?;
    let removed = embeddings
        .delete_orphaned_vectors()
        .map_err(|error| format!("orphan delete failed: {error}"))?;

    // Reclaim the freed pages with a FULL vacuum. `Database::checkpoint` runs
    // `incremental_vacuum`, which is a no-op when the database was created
    // with `auto_vacuum = NONE` — the default for every store here — so it
    // reports success while freeing nothing (measured: freelist moved by 2
    // pages on reference.db). vec0 also packs vectors into 1024-row chunks,
    // so deleting interior rows frees little until a chunk empties; only a
    // full vacuum repacks. Measured on reference.db: 49.7 MiB reclaimed,
    // vec0 shadow tables intact and every row preserved.
    {
        let connection = pool.get().map_err(|error| format!("pool get: {error}"))?;
        connection
            .execute_batch("VACUUM;")
            .map_err(|error| format!("vacuum failed: {error}"))?;
    }

    let remaining = remaining_orphans(&pool)?;
    Ok(Outcome {
        embeddings: before,
        removed,
        remaining,
    })
}

/// Count vec0 rows with no matching metadata row, read through the shadow
/// table so the check itself does not depend on the KNN path.
fn remaining_orphans(
    pool: &r2d2::Pool<hkask_storage::SqliteConnectionManager>,
) -> Result<usize, String> {
    let connection = pool.get().map_err(|error| format!("pool get: {error}"))?;
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM vec_embeddings_rowids \
             WHERE rowid NOT IN (SELECT rowid FROM embeddings)",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("orphan re-count failed: {error}"))?;
    Ok(usize::try_from(count).unwrap_or(0))
}
