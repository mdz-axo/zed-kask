#![forbid(unsafe_code)]
//! `migrate-vec0-int8` — convert legacy float32 `vec_embeddings` virtual tables
//! to the current `int8[$DIM]` sqlite-vec schema in place, preserving exact
//! rowids and quantizing through `vec_quantize_int8(v, 'unit')`.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let databases: Vec<String> = std::env::args().skip(1).collect();
    if databases.is_empty() {
        eprintln!("usage: migrate-vec0-int8 <db> [<db> ...]");
        return ExitCode::from(2);
    }
    let passphrase = match std::env::var("HKASK_DB_PASSPHRASE") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("HKASK_DB_PASSPHRASE must be set and non-empty");
            return ExitCode::from(69);
        }
    };

    let mut failures = 0_usize;
    for path in &databases {
        match migrate_one(path, &passphrase) {
            Ok(outcome) => {
                println!(
                    "{path}\tstatus={}\tdim={}\trows={}",
                    outcome.status, outcome.dim, outcome.rows
                );
            }
            Err(error) => {
                eprintln!("{path}: FAILED — {error}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

struct Outcome {
    status: &'static str,
    dim: usize,
    rows: usize,
}

fn migrate_one(path: &str, passphrase: &str) -> Result<Outcome, String> {
    if !Path::new(path).is_file() {
        return Err("not found".to_string());
    }
    let database = hkask_storage::Database::open(path, passphrase)
        .map_err(|error| format!("open failed: {error}"))?;
    let pool = database
        .sqlite_pool()
        .map_err(|error| format!("pool failed: {error}"))?;
    let mut connection = pool.get().map_err(|error| format!("pool get: {error}"))?;

    let create_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'vec_embeddings'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("read vec_embeddings DDL: {error}"))?;

    if create_sql.contains("int8[") {
        let rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM vec_embeddings", [], |row| row.get(0))
            .map_err(|error| format!("count rows: {error}"))?;
        let dim = hkask_storage::embedding_dim();
        return Ok(Outcome {
            status: "already_int8",
            dim,
            rows: rows as usize,
        });
    }

    // Read all legacy float32 (rowid, embedding) pairs before recreating vec0.
    let pairs: Vec<(i64, Vec<u8>)> = {
        let mut stmt = connection
            .prepare("SELECT rowid, embedding FROM vec_embeddings ORDER BY rowid")
            .map_err(|error| format!("prepare read: {error}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|error| format!("query float32 rows: {error}"))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|error| format!("read row: {error}"))?);
        }
        out
    };

    let dim = pairs
        .first()
        .map(|(_, blob)| blob.len() / 4)
        .unwrap_or_else(hkask_storage::embedding_dim);
    if dim == 0 {
        return Err("cannot infer non-zero vector dimension".to_string());
    }

    let expected_bytes = dim * 4;
    for (rowid, blob) in &pairs {
        if blob.len() != expected_bytes {
            return Err(format!(
                "rowid {rowid} has {} bytes, expected {expected_bytes} (float32[{dim}])",
                blob.len()
            ));
        }
    }

    let create_int8 = format!(
        "CREATE VIRTUAL TABLE vec_embeddings USING vec0(\
         embedding int8[{dim}] distance_metric=cosine)"
    );

    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| format!("begin tx: {error}"))?;
    tx.execute("DROP TABLE vec_embeddings", [])
        .map_err(|error| format!("drop old vec_embeddings: {error}"))?;
    tx.execute(&create_int8, [])
        .map_err(|error| format!("create int8 vec_embeddings: {error}"))?;
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO vec_embeddings (rowid, embedding) \
                 VALUES (?1, vec_quantize_int8(?2, 'unit'))",
            )
            .map_err(|error| format!("prepare insert: {error}"))?;
        for (rowid, blob) in &pairs {
            insert
                .execute(rusqlite::params![rowid, blob])
                .map_err(|error| format!("insert rowid {rowid}: {error}"))?;
        }
    }
    tx.commit().map_err(|error| format!("commit tx: {error}"))?;

    connection
        .execute_batch("VACUUM;")
        .map_err(|error| format!("vacuum failed: {error}"))?;

    let after_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM vec_embeddings", [], |row| row.get(0))
        .map_err(|error| format!("verify count: {error}"))?;
    if after_rows as usize != pairs.len() {
        return Err(format!(
            "row count mismatch after migration: before={} after={after_rows}",
            pairs.len()
        ));
    }

    Ok(Outcome {
        status: "migrated_to_int8",
        dim,
        rows: pairs.len(),
    })
}
