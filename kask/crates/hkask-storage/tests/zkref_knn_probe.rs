//! TEMPORARY measurement probe (deleted after recording): disk-KNN latency
//! at full zk-ref-open scale — read-only pool open (KDF) vs vec0 KNN search,
//! against the installed bundle-keyed corpus (public key, public data).
//! Not part of the test surface: #[ignore]-gated, run explicitly.

use hkask_storage::{Database, DatabaseDriver, EmbeddingStore, SqliteDriver};
use std::sync::Arc;
use std::time::Instant;

#[test]
#[ignore = "measurement probe against the installed zk-ref-open corpus"]
fn zkref_knn_probe() {
    let home = std::env::var("HOME").expect("HOME");
    let path = format!("{home}/.local/share/zed-kask/agents/curator/zk-ref-open/zk-ref-open.db");
    let key = "zkref-open-public-bundle-key-2026-10";
    let model = "Qwen/Qwen3-Embedding-8B";

    let t_open = Instant::now();
    let database = Database::open_read_only(&path, key).expect("read-only open");
    let open_cost = t_open.elapsed();

    let t_pool = Instant::now();
    let pool = database
        .sqlite_pool()
        .expect("pool (first connection pays the KDF)");
    let pool_cost = t_pool.elapsed();

    let driver: Arc<dyn DatabaseDriver> =
        Arc::new(SqliteDriver::new_labeled(pool.clone(), path.as_str()));
    let store = EmbeddingStore::from_driver(driver, 1024).expect("embedding store");
    let mut query = vec![0.0_f32; 1024];
    query[0] = 1.0;

    let t_knn = Instant::now();
    let outcome = store.search(&query, 5, model, None).expect("knn search");
    let knn_cost = t_knn.elapsed();

    let t_warm = Instant::now();
    let warm = store.search(&query, 5, model, None).expect("warm knn");
    let warm_cost = t_warm.elapsed();

    let rows: i64 = pool
        .get()
        .expect("connection")
        .query_row("SELECT COUNT(*) FROM embeddings", [], |row| row.get(0))
        .expect("row count");

    eprintln!(
        "PROBE rows={rows} open={open_cost:?} pool_kdf={pool_cost:?} knn_cold={knn_cost:?} \
         knn_warm={warm_cost:?} hits={} excluded={}",
        outcome.results.len(),
        outcome.excluded_model_mismatch
    );
    assert!(warm.results.len() == 5, "k=5 over a populated corpus");
}
