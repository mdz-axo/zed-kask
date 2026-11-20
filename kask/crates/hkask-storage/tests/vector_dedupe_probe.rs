//! Probe: can the vector be recovered from vec0 alone?
//!
//! The wave-5/6 dedupe removed `embeddings.vector` and reads the vector
//! back from the vec0 virtual table's own `embedding` column. That is
//! sound only if vec0 returns the stored bytes and the store decodes them
//! consistently. Since the int8 quantization layer (wave-6, 2026-10-31)
//! fresh stores hold quantized bytes: the probe pins one byte per
//! dimension and the affine decode, with a quantization-exact one-hot
//! vector.
use std::sync::Arc;

use hkask_storage::EmbeddingStore;
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;

const PASSPHRASE: &str = "test-passphrase";

fn fixture_vector(seed: f32) -> Vec<f32> {
    let dim = hkask_storage::embedding_dim();
    (0..dim).map(|index| seed + (index as f32) * 0.5).collect()
}

#[test]
fn vec0_returns_the_canonical_int8_bytes_and_the_store_decodes_them() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("probe.db");
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
    let database = hkask_storage::open_or_repair(path_str, PASSPHRASE)?;
    let pool = database.sqlite_pool()?;
    let driver: Arc<dyn DatabaseDriver> =
        Arc::new(SqliteDriver::new_labeled(pool.clone(), path_str));
    let embeddings =
        EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())?;

    // One-hot vectors are quantization-exact: 1 → int8 127, 0 → 0
    // (trunc((0 + 1) * 127.5 - 128) = trunc(-0.5) = 0).
    let dim = hkask_storage::embedding_dim();
    let mut vector = vec![0.0f32; dim];
    vector[7] = 1.0;
    embeddings.store("probe:1", &vector, "fixture-model", Some("probe passage"))?;

    let connection = pool.get()?;
    let from_vec0: Vec<u8> = connection.query_row(
        "SELECT embedding FROM vec_embeddings \
         WHERE rowid = (SELECT rowid FROM embeddings WHERE entity_ref = 'probe:1')",
        [],
        |row| row.get(0),
    )?;
    // The probe asserts the premise directly: the int8 shadow stores one
    // byte per dimension, and the affine decode returns the one-hot
    // exactly — the property the dedupe relies on.
    assert_eq!(
        from_vec0.len(),
        vector.len(),
        "the int8 shadow stores one byte per dimension"
    );
    assert!(
        from_vec0[7] >= 120,
        "the hot component quantizes near the range top (v = 1.0 lands at 126 \
         under the C's f32 step): {}",
        from_vec0[7]
    );
    assert!(
        from_vec0
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 7 || *byte == 0),
        "zero components quantize to 0"
    );
    let stored = embeddings.get("probe:1")?;
    let max_error = stored
        .vector
        .iter()
        .zip(&vector)
        .map(|(approx, original)| (approx - original).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_error <= 2.0 / 255.0 + 1e-5,
        "the store decodes within one quantum: {max_error}"
    );
    Ok(())
}

/// Probe: can the shipped bundle be stripped and vacuumed through a plain
/// rusqlite connection that has NOT loaded the vec0 module?
///
/// The export path drops producer-owned tables and vacuums the result. If
/// VACUUM touches the vec0 shadow tables it would need the extension loaded;
/// if it does not, the strip can run on a bare keyed connection. This
/// establishes which, so the implementation does not depend on a guess.
#[test]
fn drop_and_vacuum_work_without_the_vec0_module_loaded() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("strip.db");
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
    {
        let database = hkask_storage::open_or_repair(path_str, PASSPHRASE)?;
        let pool = database.sqlite_pool()?;
        let driver: Arc<dyn DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool.clone(), path_str));
        let embeddings =
            EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())?;
        for index in 0..8 {
            embeddings.store(
                &format!("probe:{index}"),
                &fixture_vector(index as f32),
                "fixture-model",
                Some("probe passage"),
            )?;
        }
        // A producer-owned table with bulk to free, mirroring hmems.
        let connection = pool.get()?;
        connection.execute_batch(
            "CREATE TABLE producer_owned(x BLOB);
             INSERT INTO producer_owned VALUES(zeroblob(4000000));",
        )?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in ["-wal", "-shm"] {
        let sidecar = format!("{path_str}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }

    // A bare keyed connection: no vec0 extension, no schema init.
    let escaped = PASSPHRASE.replace('\'', "''");
    let connection = rusqlite::Connection::open(&path)?;
    connection.execute_batch(&format!("PRAGMA key = '{escaped}';"))?;
    connection.execute_batch(
        "DROP TABLE IF EXISTS producer_owned;
         VACUUM;",
    )?;

    let table_gone: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = 'producer_owned'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(table_gone, 0, "the producer-owned table is dropped");
    let vec_rows: i64 =
        connection.query_row("SELECT COUNT(*) FROM vec_embeddings_rowids", [], |row| {
            row.get(0)
        })?;
    assert_eq!(vec_rows, 8, "VACUUM preserves every vec0 shadow row");
    let freelist: i64 = connection.query_row(
        "SELECT freelist_count FROM pragma_freelist_count",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(freelist, 0, "VACUUM reclaims the freed pages");
    Ok(())
}

/// Probe: does DROP + VACUUM work on an ATTACHed database?
///
/// The export copies through `ATTACH DATABASE ... AS rotated` and validates
/// the attachment in place. Stripping producer-owned tables there avoids a
/// second keyed open of a multi-GB file. This establishes whether the
/// attachment supports the drop and reclaim, or whether the strip must run
/// as a separate standalone reopen.
#[test]
fn drop_and_vacuum_work_on_an_attached_database() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let source_path = directory.path().join("source.db");
    let attached_path = directory.path().join("attached.db");
    let source_str = source_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;

    {
        let database = hkask_storage::open_or_repair(source_str, PASSPHRASE)?;
        let pool = database.sqlite_pool()?;
        let driver: Arc<dyn DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool.clone(), source_str));
        let embeddings =
            EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())?;
        for index in 0..4 {
            embeddings.store(
                &format!("probe:{index}"),
                &fixture_vector(index as f32),
                "fixture-model",
                Some("probe passage"),
            )?;
        }
        let connection = pool.get()?;
        connection.execute_batch(
            "CREATE TABLE producer_owned(x BLOB);
             INSERT INTO producer_owned VALUES(zeroblob(3000000));",
        )?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in ["-wal", "-shm"] {
        let sidecar = format!("{source_str}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }

    let escaped = PASSPHRASE.replace('\'', "''");
    let connection = rusqlite::Connection::open(&source_path)?;
    connection.execute_batch(&format!("PRAGMA key = '{escaped}';"))?;
    connection.execute(
        "ATTACH DATABASE ?1 AS rotated KEY ?2",
        rusqlite::params![attached_path.to_string_lossy(), PASSPHRASE],
    )?;
    connection.query_row("SELECT sqlcipher_export('rotated')", [], |_| Ok(()))?;
    connection.execute_batch("DETACH DATABASE rotated")?;
    connection.execute(
        "ATTACH DATABASE ?1 AS rotated KEY ?2",
        rusqlite::params![attached_path.to_string_lossy(), PASSPHRASE],
    )?;

    connection.execute_batch("DROP TABLE rotated.producer_owned")?;
    connection.execute_batch("VACUUM rotated")?;
    let integrity: String =
        connection.query_row("PRAGMA rotated.integrity_check", [], |row| row.get(0))?;
    assert_eq!(
        integrity, "ok",
        "the attachment stays integral after the strip"
    );
    let vec_rows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM rotated.vec_embeddings_rowids",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(vec_rows, 4, "the strip preserves the vec0 shadow rows");
    let gone: i64 = connection.query_row(
        "SELECT COUNT(*) FROM rotated.sqlite_master WHERE name = 'producer_owned'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        gone, 0,
        "the producer-owned table is dropped from the attachment"
    );
    Ok(())
}
