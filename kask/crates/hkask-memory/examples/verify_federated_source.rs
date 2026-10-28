//! Verify one registered federated source end-to-end through the real
//! admission path — no fixture, no mock. Runs the production
//! [`ReadOnlyPassageSource::open`] over the real on-disk artifacts: the
//! manifest, the sealed run identity (shape + `run_id` self-digest), the
//! byte digest of the database, the embeddings-schema check, the embedding
//! identity, and one probe search.
//!
//! The reseal driver (`kask/scripts/audit/reseal-calibration-run.sh`)
//! calls this after a schema migration to prove the re-sealed source
//! admits. Standalone use:
//!
//! ```text
//! HKASK_DB_PASSPHRASE=… cargo run -p hkask-memory --example verify_federated_source \
//!     -- <manifest.json> <source-id> [probe-text]
//! ```
//!
//! The passphrase is read from `HKASK_DB_PASSPHRASE` and never printed.
//! Exit 0 = admitted and searched; any admission failure prints the
//! typed [`FederatedRecallError`] and exits 1.

use hkask_memory::federated_recall::{FederatedSourcesManifest, ReadOnlyPassageSource};
use std::path::Path;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let manifest_path = args
        .next()
        .ok_or("usage: verify_federated_source <manifest.json> <source-id> [probe-text]")?;
    let source_id = args
        .next()
        .ok_or("usage: verify_federated_source <manifest.json> <source-id> [probe-text]")?;
    let probe_text = args
        .next()
        .unwrap_or_else(|| "calibration probe".to_string());
    if args.next().is_some() {
        return Err(
            "usage: verify_federated_source <manifest.json> <source-id> [probe-text]".into(),
        );
    }
    let passphrase = std::env::var("HKASK_DB_PASSPHRASE").map_err(|_| {
        "HKASK_DB_PASSPHRASE not set — the admission path needs the SQLCipher key (never printed)"
            .to_string()
    })?;
    if passphrase.is_empty() {
        return Err("HKASK_DB_PASSPHRASE is empty — fail closed, never an empty-key open".into());
    }
    if !Path::new(&manifest_path).is_file() {
        return Err(format!("manifest does not exist: {manifest_path}"));
    }

    let manifest = FederatedSourcesManifest::load(&manifest_path)
        .map_err(|error| format!("manifest load failed: {error}"))?;
    let spec = manifest
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| {
            let ids: Vec<&str> = manifest.sources.iter().map(|s| s.id.as_str()).collect();
            format!("source '{source_id}' not in manifest (have {ids:?})")
        })?
        .clone();

    // The real admission: run-identity shape, run_id self-digest, byte
    // digest, embeddings schema, embedding identity. Any failure is the typed
    // admission error — print it, exit 1.
    let source = ReadOnlyPassageSource::open(&spec, &passphrase)
        .map_err(|error| format!("ADMISSION FAILED for '{source_id}': {error}"))?;
    let identity = source.identity();
    println!(
        "admitted: {} ({}) index={} run_id={} model={} dimensions={} passages={}",
        identity.source_id,
        identity.display_name,
        identity.index_name,
        identity.run_id,
        identity.actual_embedding_model,
        identity.dimensions,
        identity.passage_count,
    );

    // One probe search through the admitted source — a unit vector at the
    // sealed dimension exercises the KNN path (a ZERO vector does not:
    // cosine distance is undefined and returns NULL — the fixture pattern
    // is e_0); the probe text exercises the lexical leg. Hit quality is
    // irrelevant; the path is the point.
    let query_model = identity.requested_embedding_model.clone();
    let dimensions = identity.dimensions;
    let mut query_vector = vec![0.0f32; dimensions];
    query_vector[0] = 1.0;
    let batch = source
        .search(&query_model, &probe_text, &query_vector, 4)
        .map_err(|error| format!("PROBE SEARCH FAILED for '{source_id}': {error}"))?;
    println!(
        "probe search ok: hits={} missing_text={} excluded_model_mismatch={}",
        batch.hits.len(),
        batch.missing_text,
        batch.excluded_model_mismatch,
    );
    Ok(())
}
