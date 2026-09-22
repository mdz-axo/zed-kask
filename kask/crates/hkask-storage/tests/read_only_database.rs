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
    let matches = embeddings.search(&fixture_vector(1.0, 0.0), 1)?;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].embedding.entity_ref, "corpus:test:1");
    assert_eq!(
        matches[0].embedding.passage_text.as_deref(),
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
