//! Wave-6 int8 quantization layer — the storage contract of the int8 vec0
//! column.
//!
//! These tests pin the MECHANISM: fresh stores quantize (the schema
//! default), legacy float32 tables serve unchanged (dispatch), and the
//! explicit conversion preserves rows and recall. The recall cost at
//! corpus scale is not asserted here — it is measured by the calibration
//! harness's replicate spread (`run-retrieval-replicates.sh`) on the real
//! index, which is the gate the operator set for shipping the layer.
use std::sync::Arc;

use hkask_storage::EmbeddingStore;
use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::vec0::{Int8Conversion, Vec0Storage, convert_vec0_to_int8};

const PASSPHRASE: &str = "test-passphrase";

/// One-hot vectors are quantization-exact: component 1 maps to int8 127
/// and every 0 maps to 0 (`trunc((0 + 1) * 127.5 - 128) = trunc(-0.5) =
/// 0`), so storing and reading a one-hot under int8 is lossless and KNN
/// against a stored one-hot returns it with distance ~0.
fn one_hot(index: usize, dim: usize) -> Vec<f32> {
    let mut vector = vec![0.0; dim];
    vector[index % dim] = 1.0;
    vector
}

/// A smooth unit vector — quantization-lossy by less than one quantum per
/// component (the property the decode tolerance pins).
fn smooth_unit(dim: usize) -> Vec<f32> {
    let mut vector: Vec<f32> = (0..dim)
        .map(|index| ((index as f32 * 0.37).sin()) * 0.03)
        .collect();
    let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    for value in &mut vector {
        *value /= norm;
    }
    vector
}

/// Open a database through the production path and construct the store
/// over its driver (element-type detection runs here, exactly as in
/// production).
fn open_store(path: &std::path::Path) -> (hkask_storage::Database, EmbeddingStore) {
    let path_str = path.to_str().expect("utf-8 test path");
    let database = hkask_storage::open_or_repair(path_str, PASSPHRASE).expect("open database");
    let pool = database.sqlite_pool().expect("pool");
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool, path_str));
    let store = EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim()).expect("store");
    (database, store)
}

/// Construct a second store over an already-open database (after a vec0
/// rebuild or conversion changed the declared element type).
fn store_over(database: &hkask_storage::Database, path: &std::path::Path) -> EmbeddingStore {
    let path_str = path.to_str().expect("utf-8 test path");
    let pool = database.sqlite_pool().expect("pool");
    let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_labeled(pool, path_str));
    EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim()).expect("store")
}

/// Rebuild `vec_embeddings` at the pre-quantization float32 form — the
/// state every database created before the wave-6 schema default is in,
/// forever (the memory test_support fixture pattern).
fn rebuild_float32(database: &hkask_storage::Database, dim: usize) {
    let connection = database
        .sqlite_pool()
        .expect("pool")
        .get()
        .expect("connection");
    connection
        .execute_batch("DROP TABLE vec_embeddings;")
        .expect("drop int8 vec table");
    connection
        .execute_batch(&format!(
            "CREATE VIRTUAL TABLE vec_embeddings USING vec0(\
             embedding float[{dim}] distance_metric=cosine);"
        ))
        .expect("rebuild float32 vec table");
}

#[test]
fn fresh_stores_declare_int8_and_round_trip_one_hot_vectors() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fresh.db");
    let (database, store) = open_store(&path);
    let dim = hkask_storage::embedding_dim();

    {
        let connection = database.sqlite_pool()?.get()?;
        let declared = Vec0Storage::detect(&connection)?;
        assert_eq!(
            declared,
            Vec0Storage::Int8,
            "fresh stores declare the int8 schema default"
        );
        let create_sql: String = connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'vec_embeddings'",
            [],
            |row| row.get(0),
        )?;
        assert!(
            create_sql.contains("int8["),
            "declared CREATE: {create_sql}"
        );
    }

    for index in 0..4 {
        store.store(
            &format!("probe:{index}"),
            &one_hot(index, dim),
            "fixture-model",
            Some("probe passage"),
        )?;
    }

    // int8 stores one byte per dimension in the vec0 shadow chunk.
    {
        let connection = database.sqlite_pool()?.get()?;
        let blob_len: i64 = connection.query_row(
            "SELECT length(embedding) FROM vec_embeddings LIMIT 1",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            blob_len, dim as i64,
            "int8 shadow stores 1 byte per dimension"
        );
    }

    for index in 0..4 {
        let vector = one_hot(index, dim);
        let outcome = store.search(&vector, 1, "fixture-model", None)?;
        let top = outcome.results.first().expect("knn hit");
        assert_eq!(top.embedding.entity_ref, format!("probe:{index}"));
        assert!(
            top.distance.abs() < 1e-6,
            "quantization-exact one-hot recalls at distance ~0: {}",
            top.distance
        );
        let stored = store.get(&format!("probe:{index}"))?;
        let max_error = stored
            .vector
            .iter()
            .zip(&vector)
            .map(|(approx, original)| (approx - original).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_error <= 2.0 / 255.0 + 1e-5,
            "the one-hot decodes within one quantum (the int8 contract): {max_error}"
        );
    }
    Ok(())
}

#[test]
fn legacy_float32_tables_serve_exact_vectors_and_are_never_converted() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("legacy.db");
    let dim = hkask_storage::embedding_dim();
    let (database, _fresh_store) = open_store(&path);
    rebuild_float32(&database, dim);
    // A store constructed AFTER the rebuild dispatches the legacy type.
    let store = store_over(&database, &path);

    let vector = smooth_unit(dim);
    store.store("legacy:1", &vector, "fixture-model", Some("passage"))?;

    let connection = database.sqlite_pool()?.get()?;
    assert_eq!(
        Vec0Storage::detect(&connection)?,
        Vec0Storage::Float32,
        "dispatch detects the legacy element type; nothing converted the table"
    );
    let stored = store.get("legacy:1")?;
    assert_eq!(
        stored.vector, vector,
        "float32 stores round-trip exact bytes — no quantization"
    );
    let outcome = store.search(&vector, 1, "fixture-model", None)?;
    assert_eq!(
        outcome
            .results
            .first()
            .expect("knn hit")
            .embedding
            .entity_ref,
        "legacy:1"
    );
    Ok(())
}

#[test]
fn convert_vec0_to_int8_preserves_rows_and_recall_and_is_idempotent() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("convert.db");
    let dim = hkask_storage::embedding_dim();
    let (database, _fresh_store) = open_store(&path);
    rebuild_float32(&database, dim);
    // Store through a float32-dispatching store (the pre-flip state).
    let float_store = store_over(&database, &path);
    for index in 0..3 {
        float_store.store(
            &format!("conv:{index}"),
            &one_hot(index, dim),
            "fixture-model",
            Some("p"),
        )?;
    }
    let smooth = smooth_unit(dim);
    float_store.store("conv:smooth", &smooth, "fixture-model", Some("p"))?;

    let connection = database.sqlite_pool()?.get()?;
    let outcome = convert_vec0_to_int8(&connection)?;
    assert_eq!(
        outcome,
        Int8Conversion {
            rows: 4,
            dimensions: dim,
            already_int8: false
        }
    );
    assert_eq!(Vec0Storage::detect(&connection)?, Vec0Storage::Int8);

    let blob_len: i64 = connection.query_row(
        "SELECT length(embedding) FROM vec_embeddings LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        blob_len, dim as i64,
        "converted shadows store 1 byte per dimension"
    );

    // A store constructed AFTER the conversion dispatches int8 and still
    // recalls: the one-hots quantize exactly, so each is its own top-1.
    let int8_store = store_over(&database, &path);
    for index in 0..3 {
        let outcome = int8_store.search(&one_hot(index, dim), 1, "fixture-model", None)?;
        assert_eq!(
            outcome
                .results
                .first()
                .expect("knn hit")
                .embedding
                .entity_ref,
            format!("conv:{index}")
        );
    }
    let decoded = int8_store.get("conv:smooth")?;
    let max_error = decoded
        .vector
        .iter()
        .zip(&smooth)
        .map(|(approx, original)| (approx - original).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_error <= 2.0 / 255.0 + 1e-5,
        "dequantization stays within one quantum: {max_error}"
    );

    // Idempotent: a second conversion reports the steady state.
    let again = convert_vec0_to_int8(&connection)?;
    assert!(again.already_int8);
    assert_eq!(again.rows, 4);
    Ok(())
}

#[test]
fn the_model_gate_excludes_other_models_under_int8() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (_database, store) = open_store(&directory.path().join("gate.db"));
    let dim = hkask_storage::embedding_dim();
    store.store(
        "gate:ours",
        &one_hot(1, dim),
        "provider/actual-model",
        Some("p"),
    )?;
    store.store(
        "gate:theirs",
        &one_hot(2, dim),
        "provider/other-model",
        Some("p"),
    )?;

    let outcome = store.search(
        &one_hot(1, dim),
        5,
        "provider/actual-model",
        Some("provider/actual-model"),
    )?;
    assert_eq!(outcome.results.len(), 1, "only the matching model serves");
    assert_eq!(outcome.results[0].embedding.entity_ref, "gate:ours");
    assert_eq!(outcome.excluded_model_mismatch, 1);
    Ok(())
}

#[test]
fn contains_entity_passage_with_model_compares_quantized_forms_under_int8() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let (_database, store) = open_store(&directory.path().join("exact.db"));
    let dim = hkask_storage::embedding_dim();
    let vector = one_hot(3, dim);
    store.store(
        "exact:1",
        &vector,
        "provider/actual-model",
        Some("the passage"),
    )?;
    assert!(store.contains_entity_passage_with_model(
        "exact:1",
        "the passage",
        "provider/actual-model",
        &vector
    )?);
    let different = one_hot(9, dim);
    assert!(!store.contains_entity_passage_with_model(
        "exact:1",
        "the passage",
        "provider/actual-model",
        &different
    )?);
    Ok(())
}
