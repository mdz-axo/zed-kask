use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{Database, EmbeddingStore, HMem, HMemStore};
use hkask_types::WebID;

const PASSPHRASE: &str = "test-passphrase";

fn fixture_vector(first: f32, second: f32) -> Vec<f32> {
    let mut vector = vec![0.0; hkask_storage::embedding_dim()];
    vector[0] = first;
    vector[1] = second;
    vector
}

fn stores(database: &Database, label: &str) -> anyhow::Result<(HMemStore, EmbeddingStore)> {
    let pool = database.sqlite_pool()?;
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool, label));
    let h_mems = HMemStore::from_driver(Arc::clone(&driver))?;
    let embeddings = EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())?;
    Ok((h_mems, embeddings))
}

/// expect: "A sealed evidence store supports real vector retrieval without any durable mutation." [P1]
#[test]
fn sealed_store_searches_and_refuses_every_write() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("sealed.db");
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
    {
        let database = hkask_storage::open_or_repair(path_str, PASSPHRASE)?;
        let (h_mems, embeddings) = stores(&database, path_str)?;
        h_mems.insert(&HMem::new(
            "corpus:test:1",
            "text",
            serde_json::json!("sealed evidence"),
            WebID::new(),
        ))?;
        embeddings.store(
            "corpus:test:1",
            &fixture_vector(1.0, 0.0),
            "fixture-model",
            Some("sealed evidence"),
        )?;
        let pool = database.sqlite_pool()?;
        let connection = pool.get()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        let sidecar = format!("{path_str}{suffix}");
        if std::path::Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }
    let before = std::fs::read(&path)?;
    let modified_before = std::fs::metadata(&path)?.modified()?;

    let database = Database::open_read_only(path_str, PASSPHRASE)?;
    let (h_mems, embeddings) = stores(&database, path_str)?;
    let matches = embeddings.search(&fixture_vector(1.0, 0.0), 1, "fixture-model", None)?;
    assert_eq!(matches.results.len(), 1);
    assert_eq!(matches.results[0].embedding.entity_ref, "corpus:test:1");
    assert_eq!(
        matches.results[0].embedding.passage_text.as_deref(),
        Some("sealed evidence")
    );
    assert!(
        h_mems
            .insert(&HMem::new(
                "corpus:test:blocked",
                "text",
                serde_json::json!("must fail"),
                WebID::new(),
            ))
            .is_err()
    );
    assert!(
        embeddings
            .store(
                "corpus:test:blocked",
                &fixture_vector(0.0, 1.0),
                "fixture-model",
                Some("must fail"),
            )
            .is_err()
    );
    drop(embeddings);
    drop(h_mems);
    drop(database);

    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::metadata(&path)?.modified()?, modified_before);
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        assert!(
            !std::path::Path::new(&format!("{path_str}{suffix}")).exists(),
            "read-only retrieval created forbidden sidecar {suffix}"
        );
    }
    Ok(())
}

/// expect: "Reclaiming orphaned vectors never changes what a query returns" [P1]
/// pre:  a store with live embeddings and interior orphan vec0 rows
/// post: the orphans are gone and every surviving query returns the same hit
#[test]
fn sweeping_orphaned_vectors_preserves_retrieval() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("sweep.db");
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
    let database = hkask_storage::open_or_repair(path_str, PASSPHRASE)?;
    let (h_mems, embeddings) = stores(&database, path_str)?;
    h_mems.insert(&HMem::new(
        "corpus:test:keep",
        "text",
        serde_json::json!("kept evidence"),
        WebID::new(),
    ))?;
    embeddings.store(
        "corpus:test:keep",
        &fixture_vector(1.0, 0.0),
        "fixture-model",
        Some("kept evidence"),
    )?;

    // Manufacture an interior orphan the way a raw SQL delete would: give a
    // metadata row a vector row, then delete only the metadata. The pool
    // connection loads vec0, so the insert into vec_embeddings exercises the
    // same path a therapy SQL pass takes.
    let pool = database.sqlite_pool()?;
    {
        let connection = pool.get()?;
        connection.execute(
            "INSERT INTO embeddings (id, entity_ref, dimensions, model, passage_text) \
             VALUES ('orphan-meta', 'corpus:test:orphan', X'00', 1024, 'fixture-model', 'x')",
            [],
        )?;
        let rowid: i64 = connection.query_row(
            "SELECT rowid FROM embeddings WHERE id = 'orphan-meta'",
            [],
            |row| row.get(0),
        )?;
        let mut vector = vec![0.0_f32; hkask_storage::embedding_dim()];
        vector[0] = 1.0;
        let blob: Vec<u8> = vector
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        connection.execute(
            "INSERT INTO vec_embeddings(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![rowid, blob],
        )?;
        connection.execute("DELETE FROM embeddings WHERE id = 'orphan-meta'", [])?;
        assert!(rowid > 0);
    }
    let orphans_before: i64 = pool.get()?.query_row(
        "SELECT COUNT(*) FROM vec_embeddings_rowids \
         WHERE rowid NOT IN (SELECT rowid FROM embeddings)",
        [],
        |row| row.get(0),
    )?;

    let before = embeddings.search(&fixture_vector(1.0, 0.0), 5, "fixture-model", None)?;
    let removed = embeddings.delete_orphaned_vectors()?;
    let orphans_after: i64 = pool.get()?.query_row(
        "SELECT COUNT(*) FROM vec_embeddings_rowids \
         WHERE rowid NOT IN (SELECT rowid FROM embeddings)",
        [],
        |row| row.get(0),
    )?;
    let after = embeddings.search(&fixture_vector(1.0, 0.0), 5, "fixture-model", None)?;

    assert!(
        orphans_before > orphans_after,
        "the fixture must actually create an orphan to sweep"
    );
    assert_eq!(removed as i64, orphans_before - orphans_after);
    assert_eq!(orphans_after, 0, "the sweep leaves no orphaned vector row");
    assert_eq!(
        after
            .results
            .iter()
            .map(|hit| hit.embedding.entity_ref.clone())
            .collect::<Vec<_>>(),
        before
            .results
            .iter()
            .map(|hit| hit.embedding.entity_ref.clone())
            .collect::<Vec<_>>(),
        "retrieval is unchanged by the sweep"
    );
    assert_eq!(
        after.results.len(),
        1,
        "the live embedding survives and is still the only hit"
    );
    Ok(())
}

/// expect: "Read-only opening cannot create a source database by typo." [P1]
#[test]
fn sealed_store_rejects_missing_path_without_creation() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let parent = directory.path().join("absent-parent");
    let path = parent.join("missing.db");
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
    assert!(Database::open_read_only(path_str, PASSPHRASE).is_err());
    assert!(!parent.exists());
    Ok(())
}
