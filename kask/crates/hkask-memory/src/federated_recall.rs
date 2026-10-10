//! Read-only, identity-bound external passage retrieval for federated recall.
//!
//! External evidence stores remain separate from Curator memory. A source is
//! admitted only when its current manifest, sealed run identity, database
//! digest, schema, entity prefix, and embedding identity all agree. Retrieval
//! projects `embeddings.passage_text` directly, so corpus h_mems such as
//! `method_signals` never enter the result surface.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::database::value::DbValue;
use hkask_storage::{Database, EmbeddingStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::lexical;

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const RUN_IDENTITY_SCHEMA_VERSION: u32 = 3;
const REPRESENTATIONS_SCHEMA_VERSION: u32 = 2;
const BUNDLE_SCHEMA_VERSION: u32 = 1;
const RECEIPT_SCHEMA_VERSION: u32 = 1;

const CURRENT_HMEM_COLUMNS: &[&str] = &[
    "id",
    "entity",
    "attribute",
    "value",
    "valid_from",
    "recalled_at",
    "confidence",
    "perspective",
    "visibility",
    "owner_webid",
    "ontology",
    "provenance",
];
const CURRENT_EMBEDDING_COLUMNS: &[&str] = &[
    "id",
    "entity_ref",
    "vector",
    "dimensions",
    "model",
    "passage_text",
    "created_at",
];

#[derive(Debug, thiserror::Error)]
pub enum FederatedRecallError {
    #[error("Cannot read {artifact} at {path}: {source}")]
    ReadArtifact {
        artifact: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Cannot parse {artifact} at {path}: {source}")]
    ParseArtifact {
        artifact: &'static str,
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("Unsupported {artifact} schema version {actual}; current version is {expected}")]
    UnsupportedSchema {
        artifact: &'static str,
        expected: u32,
        actual: u32,
    },
    #[error("Invalid federated source manifest: {0}")]
    InvalidManifest(String),
    #[error("Source {source_id} run identity has no index named {index_name}")]
    MissingIndex {
        source_id: String,
        index_name: String,
    },
    #[error("Source {source_id} database digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch {
        source_id: String,
        expected: String,
        actual: String,
    },
    #[error("Source {source_id} run identity mismatch: expected {expected}, recomputed {actual}")]
    RunIdentityMismatch {
        source_id: String,
        expected: String,
        actual: String,
    },
    #[error("Source {source_id} is not sealed: nonempty WAL at {path} ({bytes} bytes)")]
    UnsealedWal {
        source_id: String,
        path: PathBuf,
        bytes: u64,
    },
    #[error("Source {source_id} has a non-current database schema: {reason}")]
    SchemaMismatch { source_id: String, reason: String },
    #[error("Source {source_id} embedding identity is incompatible: {reason}")]
    IncompatibleEmbedding { source_id: String, reason: String },
    #[error("Source {source_id} entity identity is incompatible: {reason}")]
    IncompatibleEntity { source_id: String, reason: String },
    #[error("Source {source_id} database unavailable: {source}")]
    Database {
        source_id: String,
        #[source]
        source: hkask_storage::DatabaseError,
    },
    #[error("Source {source_id} retrieval failed: {source}")]
    Retrieval {
        source_id: String,
        #[source]
        source: hkask_storage::EmbeddingError,
    },
}

/// Provenance inputs for a materialized (shipped-bundle) federated source.
///
/// The sealed byte digest in `run_identity.indexes` is bound to the
/// producing machine's passphrase: a database materialized from a release
/// bundle under this machine's passphrase has different bytes by design.
/// When this block is present, admission verifies the provenance chain
/// instead — the git-pinned bundle manifest, the materialization receipt
/// with its verified asset hashes, and the live content checks (model,
/// dimensions, passage count) against the bundle's pins — while every
/// other seal check (run identity shape and self-digest, representations
/// manifest digest, schema, model identity, entity prefix) runs unchanged.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedProvenance {
    /// The git-pinned bundle manifest naming the release assets and the
    /// sealed run's expected content (run id, dimensions, passage count).
    pub bundle_manifest_path: PathBuf,
    /// The receipt written by the materialization script recording the
    /// verified asset hashes and the materialized database path.
    pub receipt_path: PathBuf,
}

/// One release asset of a shipped bundle: a split part of the database,
/// identified by name and SHA-256.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BundleAsset {
    pub name: String,
    pub sha256: String,
}

/// The git-pinned bundle manifest for a shipped corpus. The asset hashes
/// are the transport integrity chain the materialization script verifies;
/// the content pins are what admission verifies against the materialized
/// database.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub source_id: String,
    /// Human-readable registration display name — the manifest is the one
    /// artifact the installer reads, so the registration's self-description
    /// rides it.
    pub display_name: String,
    pub run_id: String,
    pub expected_dimensions: usize,
    pub expected_passage_count: usize,
    /// The openly-known key the shipped database is locked with. The corpus
    /// is openly licensed — the encryption is a transport wrapper required
    /// by the database format, not secrecy — so the key is public data,
    /// pinned in this git-committed manifest. A materialized database
    /// opens under THIS key, never the local machine's passphrase; a fresh
    /// install therefore needs no per-machine rekey step.
    pub bundle_key: String,
    pub assets: Vec<BundleAsset>,
}

impl BundleManifest {
    pub fn load(path: &Path) -> Result<Self, FederatedRecallError> {
        let bytes = read_artifact("bundle manifest", path)?;
        let manifest: Self = parse_artifact("bundle manifest", path, &bytes)?;
        if manifest.schema_version != BUNDLE_SCHEMA_VERSION {
            return Err(FederatedRecallError::UnsupportedSchema {
                artifact: "bundle manifest",
                expected: BUNDLE_SCHEMA_VERSION,
                actual: manifest.schema_version,
            });
        }
        if manifest.source_id.trim().is_empty()
            || manifest.display_name.trim().is_empty()
            || manifest.run_id.trim().is_empty()
            || manifest.bundle_key.trim().is_empty()
            || manifest.expected_dimensions == 0
            || manifest.expected_passage_count == 0
            || manifest.assets.is_empty()
        {
            return Err(FederatedRecallError::InvalidManifest(
                "bundle manifest requires a non-empty source id, display name, run id, bundle key, positive dimensions and passage count, and at least one asset"
                    .to_string(),
            ));
        }
        for asset in &manifest.assets {
            if asset.name.trim().is_empty() || asset.sha256.len() != 64 {
                return Err(FederatedRecallError::InvalidManifest(format!(
                    "bundle asset '{}' must carry a non-empty name and a 64-hex sha256",
                    asset.name
                )));
            }
        }
        Ok(manifest)
    }
}

/// The materialization receipt written by the materialization script: the
/// asset hashes it verified (in bundle order) and the database path it
/// produced. Admission compares these against the bundle manifest before
/// trusting the materialized database.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializationReceipt {
    pub schema_version: u32,
    pub asset_sha256: Vec<String>,
    pub database_path: String,
}

impl MaterializationReceipt {
    pub fn load(path: &Path) -> Result<Self, FederatedRecallError> {
        let bytes = read_artifact("materialization receipt", path)?;
        let receipt: Self = parse_artifact("materialization receipt", path, &bytes)?;
        if receipt.schema_version != RECEIPT_SCHEMA_VERSION {
            return Err(FederatedRecallError::UnsupportedSchema {
                artifact: "materialization receipt",
                expected: RECEIPT_SCHEMA_VERSION,
                actual: receipt.schema_version,
            });
        }
        if receipt.asset_sha256.is_empty() || receipt.database_path.trim().is_empty() {
            return Err(FederatedRecallError::InvalidManifest(
                "materialization receipt requires at least one verified asset hash and a database path"
                    .to_string(),
            ));
        }
        Ok(receipt)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedSourceSpec {
    pub id: String,
    pub display_name: String,
    pub database_path: PathBuf,
    pub run_identity_path: PathBuf,
    pub representations_manifest_path: PathBuf,
    pub index_name: String,
    /// Materialized-provenance admission (the shipped-bundle path). Absent
    /// on locally sealed sources, which keep the byte-digest check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_provenance: Option<MaterializedProvenance>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedSourcesManifest {
    pub schema_version: u32,
    pub sources: Vec<FederatedSourceSpec>,
}

impl FederatedSourcesManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FederatedRecallError> {
        let path = path.as_ref();
        let bytes = read_artifact("federated sources manifest", path)?;
        let manifest: Self = parse_artifact("federated sources manifest", path, &bytes)?;
        if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(FederatedRecallError::UnsupportedSchema {
                artifact: "federated sources manifest",
                expected: MANIFEST_SCHEMA_VERSION,
                actual: manifest.schema_version,
            });
        }
        let mut ids = HashSet::with_capacity(manifest.sources.len());
        for source in &manifest.sources {
            if source.id.trim().is_empty()
                || source.display_name.trim().is_empty()
                || source.index_name.trim().is_empty()
            {
                return Err(FederatedRecallError::InvalidManifest(
                    "source id, display_name, and index_name must be non-empty".to_string(),
                ));
            }
            if !ids.insert(source.id.as_str()) {
                return Err(FederatedRecallError::InvalidManifest(format!(
                    "duplicate source id '{}'",
                    source.id
                )));
            }
        }
        Ok(manifest)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FederatedSourceIdentity {
    pub source_id: String,
    pub display_name: String,
    pub database_path: PathBuf,
    pub run_id: String,
    pub index_name: String,
    pub entity_ref_prefix: String,
    pub requested_embedding_model: String,
    pub actual_embedding_model: String,
    pub dimensions: usize,
    pub passage_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalPassageHit {
    pub source_id: String,
    pub display_name: String,
    pub run_id: String,
    pub embedding_id: String,
    pub entity_ref: String,
    pub text: String,
    pub model: String,
    pub distance: f64,
    pub source_rank: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalPassageBatch {
    pub source_id: String,
    pub hits: Vec<ExternalPassageHit>,
    pub missing_text: usize,
    /// Stored rows excluded from the KNN window because their recorded
    /// model matches neither the query's model nor the source's declared
    /// actual model — the loud signal of a partially re-embedded sealed
    /// source. Zero on a model-homogeneous source.
    pub excluded_model_mismatch: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FederatedSourceKind {
    Curator,
    Corpus,
}

#[derive(Debug, Clone, Serialize)]
pub struct FederatedHit {
    pub source_id: String,
    pub source_kind: FederatedSourceKind,
    pub record_id: String,
    pub entity_ref: String,
    pub text: String,
    pub run_id: Option<String>,
    pub model: String,
    pub confidence: Option<f64>,
    pub distance: f64,
    pub source_rank: usize,
    pub fused_rank: usize,
}

#[derive(Debug, Clone)]
pub struct RankedSourceBatch {
    pub source_id: String,
    pub hits: Vec<FederatedHit>,
}

/// Interleave already-ranked source batches without comparing unlike scores.
///
/// Source order is authority order: callers place Curator first, so an exact
/// text duplicate retains Curator provenance. Every non-exhausted source gets
/// one turn per round; duplicates advance within that source rather than
/// consuming its reserved turn.
pub fn interleave_ranked_batches(
    batches: Vec<RankedSourceBatch>,
    limit: usize,
) -> Vec<FederatedHit> {
    if limit == 0 || batches.is_empty() {
        return Vec::new();
    }
    let mut cursors = vec![0_usize; batches.len()];
    let mut seen = HashSet::new();
    let mut fused = Vec::with_capacity(limit);
    while fused.len() < limit {
        let mut progressed = false;
        for (batch_index, batch) in batches.iter().enumerate() {
            while cursors[batch_index] < batch.hits.len() {
                let mut hit = batch.hits[cursors[batch_index]].clone();
                cursors[batch_index] += 1;
                let digest = *blake3::hash(hit.text.as_bytes()).as_bytes();
                if !seen.insert(digest) {
                    continue;
                }
                hit.fused_rank = fused.len() + 1;
                fused.push(hit);
                progressed = true;
                break;
            }
            if fused.len() == limit {
                break;
            }
        }
        if !progressed {
            break;
        }
    }
    fused
}

pub struct ReadOnlyPassageSource {
    identity: FederatedSourceIdentity,
    embeddings: EmbeddingStore,
    /// The lexical leg of hybrid retrieval — `None` when the index build
    /// failed at admission (logged there); retrieval then degrades to
    /// dense-only for this source.
    lexical: Option<lexical::LexicalIndex>,
}

impl ReadOnlyPassageSource {
    pub fn open(
        spec: &FederatedSourceSpec,
        passphrase: &str,
    ) -> Result<Self, FederatedRecallError> {
        let run_bytes = read_artifact("run identity", &spec.run_identity_path)?;
        let run_identity: RunIdentity =
            parse_artifact("run identity", &spec.run_identity_path, &run_bytes)?;
        if run_identity.schema_version != RUN_IDENTITY_SCHEMA_VERSION {
            return Err(FederatedRecallError::UnsupportedSchema {
                artifact: "run identity",
                expected: RUN_IDENTITY_SCHEMA_VERSION,
                actual: run_identity.schema_version,
            });
        }
        run_identity.validate_shape(&spec.id)?;
        verify_run_id(
            &spec.id,
            &spec.run_identity_path,
            &run_bytes,
            &run_identity.run_id,
        )?;
        validate_required_identity(
            &spec.id,
            "requested_embedding_model",
            &run_identity.requested_embedding_model,
        )?;
        validate_required_identity(
            &spec.id,
            "actual_embedding_model",
            &run_identity.actual_embedding_model,
        )?;
        let expected_digest = run_identity.indexes.get(&spec.index_name).ok_or_else(|| {
            FederatedRecallError::MissingIndex {
                source_id: spec.id.clone(),
                index_name: spec.index_name.clone(),
            }
        })?;
        ensure_checkpointed(&spec.id, &spec.database_path)?;
        // Admission fork: a locally sealed source proves itself by byte
        // digest (the seal is bound to this machine's passphrase). A
        // materialized source — shipped as a release bundle under its
        // manifest-declared public key — cannot reproduce those bytes, so
        // it proves itself by provenance chain: the git-pinned bundle
        // manifest, the receipt of verified asset hashes, and (below, after
        // the identity query) the content pins for dimensions and passage
        // count. Every other seal check runs identically on both paths.
        // The digest is hashed ONLY on the byte-digest path — a multi-GB
        // hash per admission is the seal comparison itself there, and pure
        // waste on the materialized path, where nothing consumes it.
        let bundle = match &spec.materialized_provenance {
            Some(provenance) => {
                let bundle = BundleManifest::load(&provenance.bundle_manifest_path)?;
                if !bundle.run_id.eq_ignore_ascii_case(&run_identity.run_id) {
                    return Err(FederatedRecallError::RunIdentityMismatch {
                        source_id: spec.id.clone(),
                        expected: bundle.run_id,
                        actual: run_identity.run_id.clone(),
                    });
                }
                let receipt = MaterializationReceipt::load(&provenance.receipt_path)?;
                let receipt_assets: Vec<&str> =
                    receipt.asset_sha256.iter().map(String::as_str).collect();
                let bundle_assets: Vec<&str> = bundle
                    .assets
                    .iter()
                    .map(|asset| asset.sha256.as_str())
                    .collect();
                if receipt_assets != bundle_assets {
                    return Err(FederatedRecallError::DigestMismatch {
                        source_id: spec.id.clone(),
                        expected: format!("bundle asset hashes {bundle_assets:?}"),
                        actual: format!("receipt asset hashes {receipt_assets:?}"),
                    });
                }
                let receipt_db =
                    std::fs::canonicalize(&receipt.database_path).map_err(|source| {
                        FederatedRecallError::ReadArtifact {
                            artifact: "materialization receipt database path",
                            path: PathBuf::from(&receipt.database_path),
                            source,
                        }
                    })?;
                let spec_db = std::fs::canonicalize(&spec.database_path).map_err(|source| {
                    FederatedRecallError::ReadArtifact {
                        artifact: "materialized source database",
                        path: spec.database_path.clone(),
                        source,
                    }
                })?;
                if receipt_db != spec_db {
                    return Err(FederatedRecallError::InvalidManifest(format!(
                        "materialization receipt names database '{}' but the spec registers '{}'",
                        receipt_db.display(),
                        spec_db.display()
                    )));
                }
                Some(bundle)
            }
            None => {
                let actual_digest = sha256_file(&spec.database_path)?;
                if !expected_digest.eq_ignore_ascii_case(&actual_digest) {
                    return Err(FederatedRecallError::DigestMismatch {
                        source_id: spec.id.clone(),
                        expected: expected_digest.clone(),
                        actual: actual_digest,
                    });
                }
                None
            }
        };

        let representation_bytes = read_artifact(
            "representations manifest",
            &spec.representations_manifest_path,
        )?;
        let actual_manifest_digest = format!("{:x}", Sha256::digest(&representation_bytes));
        if !run_identity
            .representations_manifest_sha256
            .eq_ignore_ascii_case(&actual_manifest_digest)
        {
            return Err(FederatedRecallError::DigestMismatch {
                source_id: spec.id.clone(),
                expected: run_identity.representations_manifest_sha256.clone(),
                actual: actual_manifest_digest,
            });
        }
        let representations: RepresentationsManifest = parse_artifact(
            "representations manifest",
            &spec.representations_manifest_path,
            &representation_bytes,
        )?;
        if representations.schema_version != REPRESENTATIONS_SCHEMA_VERSION {
            return Err(FederatedRecallError::UnsupportedSchema {
                artifact: "representations manifest",
                expected: REPRESENTATIONS_SCHEMA_VERSION,
                actual: representations.schema_version,
            });
        }
        if !representations.validation.boilerplate_filter_applied {
            return Err(FederatedRecallError::SchemaMismatch {
                source_id: spec.id.clone(),
                reason: "representations manifest does not attest canonical boilerplate filtering"
                    .to_string(),
            });
        }
        if representations.boilerplate_exclusion_reports.len()
            != representations.validation.accepted_source_count
        {
            return Err(FederatedRecallError::SchemaMismatch {
                source_id: spec.id.clone(),
                reason: format!(
                    "representations manifest reports exclusions for {} of {} accepted sources",
                    representations.boilerplate_exclusion_reports.len(),
                    representations.validation.accepted_source_count
                ),
            });
        }
        validate_required_identity(
            &spec.id,
            "entity_ref_prefix",
            &representations.entity_ref_prefix,
        )?;
        // Compose the expected prefix exactly as the calibration producer
        // composes refs (`{prefix}:{index}:`, verbatim — no normalization).
        // The producer builds refs from the manifest prefix as-is, so a
        // trailing-colon prefix (the corpus pipeline's `corpus:researcher:`)
        // yields double-colon refs; the legacy trim here rejected every
        // embedding of the first real sealed build.
        let entity_ref_prefix =
            format!("{}:{}:", representations.entity_ref_prefix, spec.index_name);

        let database_path = std::fs::canonicalize(&spec.database_path).map_err(|source| {
            FederatedRecallError::ReadArtifact {
                artifact: "source database",
                path: spec.database_path.clone(),
                source,
            }
        })?;
        let database_str = database_path.to_str().ok_or_else(|| {
            FederatedRecallError::InvalidManifest(format!(
                "source {} database path is not UTF-8",
                spec.id
            ))
        })?;
        ensure_checkpointed(&spec.id, &database_path)?;
        // A materialized source opens under its bundle's openly-known key
        // (see `BundleManifest::bundle_key`) — the shipped database is
        // locked with the public wrapper key, so the local machine's
        // passphrase is neither needed nor used. A locally sealed source
        // keeps the caller's passphrase: its byte-digest seal is bound to
        // this machine's key.
        let open_passphrase = bundle
            .as_ref()
            .map(|bundle| bundle.bundle_key.as_str())
            .unwrap_or(passphrase);
        let database =
            Database::open_read_only(database_str, open_passphrase).map_err(|source| {
                FederatedRecallError::Database {
                    source_id: spec.id.clone(),
                    source,
                }
            })?;
        let pool = database
            .sqlite_pool()
            .map_err(|source| FederatedRecallError::Database {
                source_id: spec.id.clone(),
                source,
            })?;
        let driver: Arc<dyn DatabaseDriver> =
            Arc::new(SqliteDriver::new_labeled(pool, database_str));
        validate_current_schema(&spec.id, driver.as_ref())?;
        let identity_rows = driver
            .query(
                "SELECT model, dimensions, COUNT(*),
                        SUM(CASE WHEN passage_text IS NULL OR trim(passage_text) = '' THEN 1 ELSE 0 END),
                        SUM(CASE WHEN substr(entity_ref, 1, length(?1)) = ?1 THEN 1 ELSE 0 END)
                 FROM embeddings
                 GROUP BY model, dimensions",
                &[DbValue::Text(entity_ref_prefix.clone())],
            )
            .map_err(|error| FederatedRecallError::SchemaMismatch {
                source_id: spec.id.clone(),
                reason: error.to_string(),
            })?;
        let identities = identity_rows
            .iter()
            .map(|row| {
                Ok(EmbeddingIdentityRow {
                    model: row.get_str(0)?.to_string(),
                    dimensions: row.get_int(1)?,
                    count: row.get_int(2)?,
                    missing_text: row.get_int(3)?,
                    matching_prefix: row.get_int(4)?,
                })
            })
            .collect::<Result<Vec<_>, hkask_storage::database::types::DbError>>()
            .map_err(|error| FederatedRecallError::SchemaMismatch {
                source_id: spec.id.clone(),
                reason: error.to_string(),
            })?;
        if identities.len() != 1 {
            return Err(FederatedRecallError::IncompatibleEmbedding {
                source_id: spec.id.clone(),
                reason: format!(
                    "expected exactly one current model/dimension pair, found {}",
                    identities.len()
                ),
            });
        }
        let stored = &identities[0];
        if stored.model != run_identity.actual_embedding_model {
            return Err(FederatedRecallError::IncompatibleEmbedding {
                source_id: spec.id.clone(),
                reason: format!(
                    "stored model '{}' differs from sealed actual model '{}'",
                    stored.model, run_identity.actual_embedding_model
                ),
            });
        }
        if stored.dimensions <= 0 || stored.count <= 0 {
            return Err(FederatedRecallError::IncompatibleEmbedding {
                source_id: spec.id.clone(),
                reason: "embedding dimension and passage count must be positive".to_string(),
            });
        }
        // Materialized sources carry the bundle's content pins: the live
        // database must hold exactly the dimensions and passage count the
        // git-pinned bundle manifest declares for the sealed run. A
        // truncated, partial, or tampered materialization fails here.
        if let Some(bundle) = &bundle {
            let (Ok(expected_dimensions), Ok(expected_count)) = (
                i64::try_from(bundle.expected_dimensions),
                i64::try_from(bundle.expected_passage_count),
            ) else {
                return Err(FederatedRecallError::InvalidManifest(
                    "bundle content pins exceed the platform's row-integer range".to_string(),
                ));
            };
            if stored.dimensions != expected_dimensions {
                return Err(FederatedRecallError::SchemaMismatch {
                    source_id: spec.id.clone(),
                    reason: format!(
                        "materialized dimensions {} differ from the bundle pin {expected_dimensions}",
                        stored.dimensions
                    ),
                });
            }
            if stored.count != expected_count {
                return Err(FederatedRecallError::SchemaMismatch {
                    source_id: spec.id.clone(),
                    reason: format!(
                        "materialized passage count {} differs from the bundle pin {expected_count}",
                        stored.count
                    ),
                });
            }
        }
        if stored.matching_prefix != stored.count {
            return Err(FederatedRecallError::IncompatibleEntity {
                source_id: spec.id.clone(),
                reason: format!(
                    "{} of {} embeddings match current prefix '{}'",
                    stored.matching_prefix, stored.count, entity_ref_prefix
                ),
            });
        }
        if stored.missing_text != 0 {
            return Err(FederatedRecallError::SchemaMismatch {
                source_id: spec.id.clone(),
                reason: format!(
                    "{} embeddings lack current passage_text",
                    stored.missing_text
                ),
            });
        }
        let dimensions = usize::try_from(stored.dimensions).map_err(|error| {
            FederatedRecallError::IncompatibleEmbedding {
                source_id: spec.id.clone(),
                reason: error.to_string(),
            }
        })?;
        let passage_count = usize::try_from(stored.count).map_err(|error| {
            FederatedRecallError::IncompatibleEmbedding {
                source_id: spec.id.clone(),
                reason: error.to_string(),
            }
        })?;
        let embeddings = EmbeddingStore::from_driver(driver, dimensions).map_err(|source| {
            FederatedRecallError::Retrieval {
                source_id: spec.id.clone(),
                source,
            }
        })?;
        // The lexical leg of hybrid retrieval: a rare-term inverted index
        // derived in memory from the sealed rows at admission. The sealed
        // file is never modified; a build failure degrades this source to
        // dense-only retrieval with the reason logged — never silent.
        let lexical = match embeddings.list_passage_rows() {
            Ok(rows) => {
                let index = lexical::LexicalIndex::build(
                    rows.into_iter()
                        .map(|row| (row.id, row.entity_ref, row.passage_text)),
                );
                tracing::info!(
                    target: "reg.memory",
                    source_id = %spec.id,
                    rows = index.row_count(),
                    terms = index.term_count(),
                    "Hybrid lexical index built for sealed source"
                );
                Some(index)
            }
            Err(error) => {
                tracing::warn!(
                    target: "reg.memory",
                    source_id = %spec.id,
                    error = %error,
                    "Lexical index build failed; retrieval degrades to dense-only for this source"
                );
                None
            }
        };
        Ok(Self {
            identity: FederatedSourceIdentity {
                source_id: spec.id.clone(),
                display_name: spec.display_name.clone(),
                database_path,
                run_id: run_identity.run_id,
                index_name: spec.index_name.clone(),
                entity_ref_prefix,
                requested_embedding_model: run_identity.requested_embedding_model,
                actual_embedding_model: run_identity.actual_embedding_model,
                dimensions,
                passage_count,
            },
            embeddings,
            lexical,
        })
    }

    pub fn identity(&self) -> &FederatedSourceIdentity {
        &self.identity
    }

    pub fn search(
        &self,
        query_model: &str,
        query_text: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<ExternalPassageBatch, FederatedRecallError> {
        if query_model != self.identity.requested_embedding_model
            && query_model != self.identity.actual_embedding_model
        {
            return Err(FederatedRecallError::IncompatibleEmbedding {
                source_id: self.identity.source_id.clone(),
                reason: format!(
                    "query model '{query_model}' differs from current requested '{}' and actual '{}' identities",
                    self.identity.requested_embedding_model, self.identity.actual_embedding_model
                ),
            });
        }
        if query_vector.len() != self.identity.dimensions {
            return Err(FederatedRecallError::IncompatibleEmbedding {
                source_id: self.identity.source_id.clone(),
                reason: format!(
                    "query dimension {} differs from sealed dimension {}",
                    query_vector.len(),
                    self.identity.dimensions
                ),
            });
        }
        if limit == 0 {
            return Err(FederatedRecallError::InvalidManifest(
                "search limit must be positive".to_string(),
            ));
        }
        ensure_checkpointed(&self.identity.source_id, &self.identity.database_path)?;
        // Model gate: the identity check above verified the query's model
        // against this source's declared identities; this gate covers the
        // rows themselves — a sealed DB whose rows were written under a
        // different model than the query's (a partially re-embedded store)
        // must degrade loudly, never rank cross-model cosine distances. The
        // source's declared actual model is the second accepted identity:
        // durable rows record the provider-confirmed form, which can differ
        // from the requested form the query was embedded with.
        let outcome = self
            .embeddings
            .search(
                query_vector,
                limit,
                query_model,
                Some(&self.identity.actual_embedding_model),
            )
            .map_err(|source| FederatedRecallError::Retrieval {
                source_id: self.identity.source_id.clone(),
                source,
            })?;
        let dense_results = outcome.results;

        // Hybrid fusion: the dense KNN ranks are fused with the lexical
        // leg's ranks by reciprocal-rank fusion — the legs' scores are
        // never compared, each contributes 1/(RRF_K + rank). With no
        // lexical index (build failed at admission) or no rare-term signal
        // in the query, the fused order degenerates to the dense order:
        // the lexical leg is silent without signal.
        let lexical = self.lexical.as_ref();
        let lexical_hits = lexical.map(|index| index.search(query_text, limit));
        let mut contributions: HashMap<&str, f64> = HashMap::new();
        for (index, result) in dense_results.iter().enumerate() {
            // Build-metadata rows (the corpus's own MANIFEST) keep their
            // dense candidacy but never on rank parity with content —
            // they crowded 5 of the top-10 slots for real mechanics
            // queries before the penalty (observed 2026-10-09).
            let is_metadata = lexical
                .map(|index| {
                    index
                        .row_by_id(&result.embedding.id)
                        .is_some_and(|row| index.is_metadata(row))
                })
                .unwrap_or(false);
            let rank = index + 1 + usize::from(is_metadata) * lexical::METADATA_RANK_PENALTY;
            *contributions
                .entry(result.embedding.id.as_str())
                .or_insert(0.0) += lexical::rrf_contribution(rank);
        }
        if let (Some(index), Some(hits)) = (lexical, &lexical_hits) {
            for (position, hit) in hits.iter().enumerate() {
                if let Some(id) = index.row_id(hit.row) {
                    *contributions.entry(id).or_insert(0.0) +=
                        lexical::rrf_contribution(position + 1);
                }
            }
        }
        let mut ordered: Vec<(&str, f64)> = contributions.into_iter().collect();
        // Deterministic tie-break on the embedding id: equal fused scores
        // order stably across runs.
        ordered.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(b.0))
        });
        ordered.truncate(limit);

        // Materialize the fused rows. Dense-leg rows carry their measured
        // vec0 distance; lexical-only rows fall outside the dense top-K
        // window — the case this leg exists for — so their full rows are
        // fetched by id and the cosine distance computed against the
        // query vector. Admission verified exactly one model/dimension
        // pair for the whole sealed table, so fetched rows share the
        // sealed model and no per-row model gate is needed here.
        let dense_by_id: HashMap<&str, &hkask_storage::SimilarityResult> = dense_results
            .iter()
            .map(|result| (result.embedding.id.as_str(), result))
            .collect();
        let missing: Vec<&str> = ordered
            .iter()
            .map(|(id, _contribution)| *id)
            .filter(|id| !dense_by_id.contains_key(id))
            .collect();
        let mut fetched: HashMap<String, hkask_storage::StoredEmbedding> = HashMap::new();
        if !missing.is_empty() {
            let rows = self.embeddings.get_by_ids(&missing).map_err(|source| {
                FederatedRecallError::Retrieval {
                    source_id: self.identity.source_id.clone(),
                    source,
                }
            })?;
            if rows.len() != missing.len() {
                return Err(FederatedRecallError::SchemaMismatch {
                    source_id: self.identity.source_id.clone(),
                    reason: format!(
                        "{} fused rows are absent from the sealed embeddings table",
                        missing.len() - rows.len()
                    ),
                });
            }
            fetched = rows.into_iter().map(|row| (row.id.clone(), row)).collect();
        }

        let mut hits = Vec::with_capacity(ordered.len());
        let mut missing_text = 0;
        for (position, (id, _contribution)) in ordered.iter().enumerate() {
            let (entity_ref, text, model, distance) = match (dense_by_id.get(*id), fetched.get(*id))
            {
                (Some(result), _) => (
                    result.embedding.entity_ref.clone(),
                    result.embedding.passage_text.clone(),
                    result.embedding.model.clone(),
                    result.distance,
                ),
                (None, Some(row)) => (
                    row.entity_ref.clone(),
                    row.passage_text.clone(),
                    row.model.clone(),
                    cosine_distance(query_vector, &row.vector),
                ),
                (None, None) => {
                    return Err(FederatedRecallError::SchemaMismatch {
                        source_id: self.identity.source_id.clone(),
                        reason: format!("fused row '{id}' is absent from both retrieval legs"),
                    });
                }
            };
            if !entity_ref.starts_with(&self.identity.entity_ref_prefix) {
                return Err(FederatedRecallError::IncompatibleEntity {
                    source_id: self.identity.source_id.clone(),
                    reason: format!(
                        "retrieved entity '{entity_ref}' falls outside prefix '{}'",
                        self.identity.entity_ref_prefix
                    ),
                });
            }
            let Some(text) = text.filter(|text| !text.trim().is_empty()) else {
                missing_text += 1;
                continue;
            };
            hits.push(ExternalPassageHit {
                source_id: self.identity.source_id.clone(),
                display_name: self.identity.display_name.clone(),
                run_id: self.identity.run_id.clone(),
                embedding_id: (*id).to_string(),
                entity_ref,
                text,
                model,
                distance,
                source_rank: position + 1,
            });
        }
        Ok(ExternalPassageBatch {
            source_id: self.identity.source_id.clone(),
            hits,
            missing_text,
            excluded_model_mismatch: outcome.excluded_model_mismatch,
        })
    }
}

/// Cosine distance matching vec0's `distance_metric=cosine` (1 − cos):
/// lexical-only fused rows fall outside the dense KNN window, so their
/// distance is computed directly against the query vector.
fn cosine_distance(query: &[f32], vector: &[f32]) -> f64 {
    let mut dot = 0.0;
    let mut query_norm = 0.0;
    let mut vector_norm = 0.0;
    for (query_component, vector_component) in query.iter().zip(vector) {
        dot += *query_component as f64 * *vector_component as f64;
        query_norm += *query_component as f64 * *query_component as f64;
        vector_norm += *vector_component as f64 * *vector_component as f64;
    }
    if query_norm == 0.0 || vector_norm == 0.0 {
        return 1.0;
    }
    1.0 - dot / (query_norm.sqrt() * vector_norm.sqrt())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunIdentity {
    schema_version: u32,
    run_id: String,
    preseal_run_id: String,
    accepted_sources_sha256: String,
    run_spec_sha256: String,
    queries_sha256: String,
    requested_embedding_model: String,
    actual_embedding_model: String,
    policies_sha256: String,
    retriever_sha256: String,
    evaluator_sha256: String,
    representations_manifest_sha256: String,
    representations: BTreeMap<String, String>,
    indexes: BTreeMap<String, String>,
}

impl RunIdentity {
    fn validate_shape(&self, source_id: &str) -> Result<(), FederatedRecallError> {
        for (field, digest) in [
            ("run_id", &self.run_id),
            ("preseal_run_id", &self.preseal_run_id),
            ("accepted_sources_sha256", &self.accepted_sources_sha256),
            ("run_spec_sha256", &self.run_spec_sha256),
            ("queries_sha256", &self.queries_sha256),
            ("policies_sha256", &self.policies_sha256),
            ("retriever_sha256", &self.retriever_sha256),
            ("evaluator_sha256", &self.evaluator_sha256),
            (
                "representations_manifest_sha256",
                &self.representations_manifest_sha256,
            ),
        ] {
            validate_digest(source_id, field, digest)?;
        }
        validate_digest_map(
            source_id,
            "representations",
            &self.representations,
            &["child_parent_map", "current", "fine", "parent", "reference"],
        )?;
        validate_digest_map(
            source_id,
            "indexes",
            &self.indexes,
            &["current", "fine", "reference"],
        )?;
        Ok(())
    }
}

fn validate_digest(source_id: &str, field: &str, digest: &str) -> Result<(), FederatedRecallError> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FederatedRecallError::InvalidManifest(format!(
            "source {source_id} run identity {field} must be a SHA-256 digest"
        )));
    }
    Ok(())
}

fn validate_digest_map(
    source_id: &str,
    field: &str,
    digests: &BTreeMap<String, String>,
    expected: &[&str],
) -> Result<(), FederatedRecallError> {
    if digests.keys().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(FederatedRecallError::InvalidManifest(format!(
            "source {source_id} run identity {field} has a non-current key set"
        )));
    }
    for (name, digest) in digests {
        validate_digest(source_id, &format!("{field}.{name}"), digest)?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct RepresentationsManifest {
    schema_version: u32,
    entity_ref_prefix: String,
    boilerplate_exclusion_reports: BTreeMap<String, serde_json::Value>,
    validation: RepresentationsValidation,
}

#[derive(Debug, Deserialize)]
struct RepresentationsValidation {
    accepted_source_count: usize,
    boilerplate_filter_applied: bool,
}

struct EmbeddingIdentityRow {
    model: String,
    dimensions: i64,
    count: i64,
    missing_text: i64,
    matching_prefix: i64,
}

fn read_artifact(artifact: &'static str, path: &Path) -> Result<Vec<u8>, FederatedRecallError> {
    std::fs::read(path).map_err(|source| FederatedRecallError::ReadArtifact {
        artifact,
        path: path.to_path_buf(),
        source,
    })
}

fn parse_artifact<T: for<'de> Deserialize<'de>>(
    artifact: &'static str,
    path: &Path,
    bytes: &[u8],
) -> Result<T, FederatedRecallError> {
    serde_json::from_slice(bytes).map_err(|source| FederatedRecallError::ParseArtifact {
        artifact,
        path: path.to_path_buf(),
        source,
    })
}

fn validate_required_identity(
    source_id: &str,
    field: &str,
    value: &str,
) -> Result<(), FederatedRecallError> {
    if value.trim().is_empty() {
        return Err(FederatedRecallError::InvalidManifest(format!(
            "source {source_id} {field} must be non-empty"
        )));
    }
    Ok(())
}

/// The producer hashes `jq -cS .` over the identity without `run_id`.
/// `preserve_order` is enabled in this workspace, so every nested object must
/// be sorted explicitly before compact serialization; jq appends one newline.
fn sorted_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(value) = object.get(key) {
                    sorted.insert(key.clone(), sorted_json(value));
                }
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(sorted_json).collect())
        }
        other => other.clone(),
    }
}

/// The seal computation shared by the verifier and every federated test
/// fixture builder: hash the canonical (sorted-keys, compact) serialization
/// of `identity` — which must NOT yet carry its `run_id` field — plus one
/// trailing newline, matching the producer's `jq -cS .` pipeline.
///
/// Public so federated test fixtures across crates (kask_bridge, curator)
/// compute seals through the same code the verifier checks against — a
/// hand-copied canonicalizer in a test can drift and still pass, sealing
/// against a stale algorithm.
pub fn sealed_run_id(identity: &serde_json::Value) -> Result<String, serde_json::Error> {
    let mut canonical = serde_json::to_vec(&sorted_json(identity))?;
    canonical.push(b'\n');
    Ok(format!("{:x}", Sha256::digest(&canonical)))
}

fn verify_run_id(
    source_id: &str,
    path: &Path,
    bytes: &[u8],
    expected: &str,
) -> Result<(), FederatedRecallError> {
    let mut identity: serde_json::Value = parse_artifact("run identity", path, bytes)?;
    let fields = identity.as_object_mut().ok_or_else(|| {
        FederatedRecallError::InvalidManifest(format!(
            "source {source_id} run identity must be a JSON object"
        ))
    })?;
    fields.remove("run_id");
    let actual = sealed_run_id(&identity).map_err(|error| FederatedRecallError::ParseArtifact {
        artifact: "run identity",
        path: path.to_path_buf(),
        source: error,
    })?;
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(FederatedRecallError::RunIdentityMismatch {
            source_id: source_id.to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

fn ensure_checkpointed(source_id: &str, path: &Path) -> Result<(), FederatedRecallError> {
    let mut wal_path = path.as_os_str().to_os_string();
    wal_path.push("-wal");
    let wal_path = PathBuf::from(wal_path);
    match std::fs::metadata(&wal_path) {
        Ok(metadata) if metadata.len() > 0 || !metadata.is_file() => {
            Err(FederatedRecallError::UnsealedWal {
                source_id: source_id.to_string(),
                path: wal_path,
                bytes: metadata.len(),
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(FederatedRecallError::ReadArtifact {
            artifact: "source WAL",
            path: wal_path,
            source,
        }),
    }
}

fn sha256_file(path: &Path) -> Result<String, FederatedRecallError> {
    let mut file =
        std::fs::File::open(path).map_err(|source| FederatedRecallError::ReadArtifact {
            artifact: "source database",
            path: path.to_path_buf(),
            source,
        })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| FederatedRecallError::ReadArtifact {
                artifact: "source database",
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn table_columns(
    driver: &dyn DatabaseDriver,
    table: &str,
) -> Result<Vec<String>, hkask_storage::database::types::DbError> {
    driver
        .query(&format!("PRAGMA table_info({table})"), &[])?
        .iter()
        .map(|row| row.get_str(1).map(ToString::to_string))
        .collect()
}

fn validate_current_schema(
    source_id: &str,
    driver: &dyn DatabaseDriver,
) -> Result<(), FederatedRecallError> {
    for (table, expected) in [
        ("hmems", CURRENT_HMEM_COLUMNS),
        ("embeddings", CURRENT_EMBEDDING_COLUMNS),
    ] {
        let actual =
            table_columns(driver, table).map_err(|error| FederatedRecallError::SchemaMismatch {
                source_id: source_id.to_string(),
                reason: format!("cannot inspect {table}: {error}"),
            })?;
        let expected: Vec<String> = expected
            .iter()
            .map(|column| (*column).to_string())
            .collect();
        if actual != expected {
            return Err(FederatedRecallError::SchemaMismatch {
                source_id: source_id.to_string(),
                reason: format!(
                    "table {table} columns differ from current schema: expected {expected:?}, got {actual:?}"
                ),
            });
        }
    }
    let rows = driver
        .query(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'vec_embeddings'",
            &[],
        )
        .map_err(|error| FederatedRecallError::SchemaMismatch {
            source_id: source_id.to_string(),
            reason: error.to_string(),
        })?;
    let vec_table = rows
        .first()
        .ok_or_else(|| FederatedRecallError::SchemaMismatch {
            source_id: source_id.to_string(),
            reason: "vec_embeddings existence query returned no row".to_string(),
        })?
        .get_int(0)
        .map_err(|error| FederatedRecallError::SchemaMismatch {
            source_id: source_id.to_string(),
            reason: error.to_string(),
        })?;
    if vec_table != 1 {
        return Err(FederatedRecallError::SchemaMismatch {
            source_id: source_id.to_string(),
            reason: "current vec_embeddings table is absent".to_string(),
        });
    }
    Ok(())
}
