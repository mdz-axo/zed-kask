//! Cross-crate test support for sealed federated-source fixtures.
//!
//! The producer-side fixture builder that this crate's integration tests and
//! the consumer crates' tests (kask_bridge, hkask-mcp-curator) all need —
//! previously hand-copied three times (the ratchet's 74/87/89/91/109/254-
//! token cross-file clusters), each copy free to drift from the seal
//! contract. Compiled unconditionally (the house `for_tests` pattern:
//! `CuratorStore::for_tests`, `LanguageModelEmbeddingPort::for_tests`); the
//! functions here are test fixtures, never called in production paths.

use crate::federated_recall::{FederatedSourceSpec, FederatedSourcesManifest, sealed_run_id};
use crate::memory_store::MemoryStore;
use anyhow::anyhow;
use sha2::Digest as _;
use std::path::{Path, PathBuf};

/// Build one sealed federated source inside `directory`:
///
/// - a `reference.db` seeded with `passage` (a `text` h_mem, a
///   `method_signals` h_mem, and one embedding stored under
///   `actual_model`), WAL-checkpointed and stripped of its sidecars so the
///   file is sealed;
/// - a `representations-manifest.json` describing the source;
/// - a `run-identity.json` sealed by the production [`sealed_run_id`];
/// - the `federated-sources.json` manifest naming the source, returned as
///   the manifest path.
///
/// The entity ref is `calibration:fixture:sealed-v1:reference:utf8-<hex>:0`
/// where `<hex>` is `source_filename`'s UTF-8 encoding — the producer's ref
/// scheme. The manifest's boilerplate-exclusion report key is the constant
/// `"fixture.txt"` every existing fixture carries.
pub fn sealed_federated_fixture(
    directory: &Path,
    source_id: &str,
    display_name: &str,
    source_filename: &str,
    passage: &str,
    requested_model: &str,
    actual_model: &str,
) -> anyhow::Result<PathBuf> {
    sealed_federated_fixture_indexed(
        directory,
        source_id,
        display_name,
        source_filename,
        passage,
        requested_model,
        actual_model,
        "calibration:fixture:sealed-v1",
        "reference",
    )
}

/// The parameterized core of [`sealed_federated_fixture`] — see
/// [`sealed_federated_fixture_core`], which both public builders delegate
/// to. (This doc comment is retained from the original single builder.)
pub fn sealed_federated_fixture_indexed(
    directory: &Path,
    source_id: &str,
    display_name: &str,
    source_filename: &str,
    passage: &str,
    requested_model: &str,
    actual_model: &str,
    manifest_prefix: &str,
    index_name: &str,
) -> anyhow::Result<PathBuf> {
    sealed_federated_fixture_core(
        directory,
        source_id,
        display_name,
        source_filename,
        passage,
        requested_model,
        actual_model,
        manifest_prefix,
        index_name,
        hkask_storage::embedding_dim(),
    )
}

/// A width-parameterized fixture: identical to [`sealed_federated_fixture`]
/// but sealed at `dim` dimensions instead of the configured default. For
/// tests that exercise a federated source whose sealed width differs from
/// the curator store's (the shipped-bundle case).
pub fn sealed_federated_fixture_dimmed(
    directory: &Path,
    source_id: &str,
    display_name: &str,
    source_filename: &str,
    passage: &str,
    requested_model: &str,
    actual_model: &str,
    dim: usize,
) -> anyhow::Result<PathBuf> {
    sealed_federated_fixture_core(
        directory,
        source_id,
        display_name,
        source_filename,
        passage,
        requested_model,
        actual_model,
        "calibration:fixture:sealed-v1",
        "reference",
        dim,
    )
}

/// The parameterized core of [`sealed_federated_fixture`]. `manifest_prefix`,
/// `index_name`, and `dim` are the producer's composition inputs, used
/// verbatim: the entity ref is `{manifest_prefix}:{index_name}:utf8-<hex>:0`
/// (a trailing-colon prefix such as the corpus pipeline's
/// `corpus:researcher:` yields the producer's double-colon refs), the
/// database digest lands under `indexes[{index_name}]`, the database file
/// is `{index_name}.db`, and the sealed embedding width is `dim`.
fn sealed_federated_fixture_core(
    directory: &Path,
    source_id: &str,
    display_name: &str,
    source_filename: &str,
    passage: &str,
    requested_model: &str,
    actual_model: &str,
    manifest_prefix: &str,
    index_name: &str,
    dim: usize,
) -> anyhow::Result<PathBuf> {
    const PASSPHRASE: &str = "test-passphrase";
    let database_path = directory.join(format!("{index_name}.db"));
    let database = database_path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF-8 database path"))?;
    let hex: String = source_filename
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let entity = format!("{manifest_prefix}:{index_name}:utf8-{hex}:0");
    let mut vector = vec![0.0; dim];
    vector[0] = 1.0;
    if dim != hkask_storage::embedding_dim() {
        // `MemoryStore::open`'s `dim` parameter configures only the
        // Rust-side embedding validation; the schema's vec0 virtual table is
        // created at the configured environment width on the pool's first
        // connection (`initialize_schema` reads `embedding_dim()`, not the
        // caller's dim). A fixture sealed at a different width must rebuild
        // vec0 at that width before any embedding is stored: the
        // `embeddings` BLOB table is width-agnostic, but vec0's width is
        // fixed at creation. Later pools' `CREATE VIRTUAL TABLE IF NOT
        // EXISTS` no-ops against the rebuilt table, so the sealed width
        // stays `dim` for the store below.
        let handle = hkask_storage::open_or_repair(database, PASSPHRASE)?;
        let conn = handle.sqlite_pool()?.get()?;
        conn.execute_batch("DROP TABLE vec_embeddings;")?;
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE vec_embeddings USING vec0(\
             embedding float[{dim}] distance_metric=cosine);"
        ))?;
    }
    {
        let store = MemoryStore::open(database, PASSPHRASE, dim)?;
        store.store(hkask_storage::HMem::new(
            &entity,
            "text",
            serde_json::json!(passage),
            hkask_types::WebID::new(),
        ))?;
        store.store(hkask_storage::HMem::new(
            &entity,
            "method_signals",
            serde_json::json!({"parataxis_ratio": 1.0}),
            hkask_types::WebID::new(),
        ))?;
        store.store_embedding(&entity, &vector, actual_model, Some(passage))?;
    }
    {
        let database = hkask_storage::open_or_repair(database, PASSPHRASE)?;
        database
            .sqlite_pool()?
            .get()?
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    for suffix in [".maintenance-lock", "-wal", "-shm"] {
        let sidecar = format!("{database}{suffix}");
        if Path::new(&sidecar).exists() {
            std::fs::remove_file(sidecar)?;
        }
    }

    let digest = sha256_file(&database_path)?;
    let representations_path = directory.join("representations-manifest.json");
    std::fs::write(
        &representations_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 2,
            "entity_ref_prefix": manifest_prefix,
            "boilerplate_exclusion_reports": {
                "fixture.txt": {"input_words": 3, "retained_words": 3, "exclusions": []}
            },
            "validation": {"accepted_source_count": 1, "boilerplate_filter_applied": true}
        }))?,
    )?;
    let manifest_digest = sha256_file(&representations_path)?;
    let run_identity_path = directory.join("run-identity.json");
    let mut identity = serde_json::json!({
        "schema_version": 3,
        "preseal_run_id": "a".repeat(64),
        "accepted_sources_sha256": "b".repeat(64),
        "run_spec_sha256": "c".repeat(64),
        "queries_sha256": "d".repeat(64),
        "requested_embedding_model": requested_model,
        "actual_embedding_model": actual_model,
        "policies_sha256": "e".repeat(64),
        "retriever_sha256": "f".repeat(64),
        "evaluator_sha256": "0".repeat(64),
        "representations_manifest_sha256": manifest_digest,
        "representations": {
            "reference": "1".repeat(64),
            "current": "2".repeat(64),
            "fine": "3".repeat(64),
            "child_parent_map": "4".repeat(64),
            "parent": "5".repeat(64)
        },
        "indexes": {
            "reference": "8".repeat(64),
            "current": "6".repeat(64),
            "fine": "7".repeat(64)
        }
    });
    identity["indexes"][index_name] = serde_json::json!(digest);
    identity["run_id"] = serde_json::json!(sealed_run_id(&identity)?);
    std::fs::write(&run_identity_path, serde_json::to_vec_pretty(&identity)?)?;
    let manifest_path = directory.join("federated-sources.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&FederatedSourcesManifest {
            schema_version: 1,
            sources: vec![FederatedSourceSpec {
                id: source_id.to_string(),
                display_name: display_name.to_string(),
                database_path,
                run_identity_path,
                representations_manifest_path: representations_path,
                index_name: index_name.to_string(),
                materialized_provenance: None,
            }],
        })?,
    )?;
    Ok(manifest_path)
}

/// Re-seal a mutated run identity through the production seal computation:
/// drop the stale `run_id` and recompute it over the remaining fields, so a
/// test mutation cannot seal against a hand-copied algorithm.
pub fn reseal_fixture_identity(identity: &mut serde_json::Value) -> anyhow::Result<()> {
    identity
        .as_object_mut()
        .ok_or_else(|| anyhow!("run identity must be an object"))?
        .remove("run_id");
    identity["run_id"] = serde_json::json!(sealed_run_id(identity)?);
    Ok(())
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        sha2::Digest::update(&mut hasher, &buffer[..read]);
    }
    Ok(format!("{:x}", sha2::Digest::finalize(hasher)))
}
