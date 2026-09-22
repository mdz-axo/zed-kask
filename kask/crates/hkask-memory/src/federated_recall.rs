//! Read-only, identity-bound external passage retrieval for federated recall.
//!
//! External evidence stores remain separate from Curator memory. A source is
//! admitted only when its current manifest, sealed run identity, database
//! digest, schema, entity prefix, and embedding identity all agree. Retrieval
//! projects `embeddings.passage_text` directly, so corpus h_mems such as
//! `method_signals` never enter the result surface.

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::database::value::DbValue;
use hkask_storage::{Database, EmbeddingStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const RUN_IDENTITY_SCHEMA_VERSION: u32 = 2;
const REPRESENTATIONS_SCHEMA_VERSION: u32 = 2;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FederatedSourceSpec {
    pub id: String,
    pub display_name: String,
    pub database_path: PathBuf,
    pub run_identity_path: PathBuf,
    pub representations_manifest_path: PathBuf,
    pub index_name: String,
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
    pub database_sha256: String,
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
        validate_required_identity(&spec.id, "run_id", &run_identity.run_id)?;
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
        let actual_digest = sha256_file(&spec.database_path)?;
        if !expected_digest.eq_ignore_ascii_case(&actual_digest) {
            return Err(FederatedRecallError::DigestMismatch {
                source_id: spec.id.clone(),
                expected: expected_digest.clone(),
                actual: actual_digest,
            });
        }

        let representation_bytes = read_artifact(
            "representations manifest",
            &spec.representations_manifest_path,
        )?;
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
        let entity_ref_prefix = format!(
            "{}:{}:",
            representations.entity_ref_prefix.trim_end_matches(':'),
            spec.index_name
        );

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
        let database = Database::open_read_only(database_str, passphrase).map_err(|source| {
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
        Ok(Self {
            identity: FederatedSourceIdentity {
                source_id: spec.id.clone(),
                display_name: spec.display_name.clone(),
                database_path,
                database_sha256: actual_digest,
                run_id: run_identity.run_id,
                index_name: spec.index_name.clone(),
                entity_ref_prefix,
                requested_embedding_model: run_identity.requested_embedding_model,
                actual_embedding_model: run_identity.actual_embedding_model,
                dimensions,
                passage_count,
            },
            embeddings,
        })
    }

    pub fn identity(&self) -> &FederatedSourceIdentity {
        &self.identity
    }

    pub fn search(
        &self,
        query_model: &str,
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
        let results = self
            .embeddings
            .search(query_vector, limit)
            .map_err(|source| FederatedRecallError::Retrieval {
                source_id: self.identity.source_id.clone(),
                source,
            })?;
        let mut hits = Vec::with_capacity(results.len());
        let mut missing_text = 0;
        for (index, result) in results.into_iter().enumerate() {
            if !result
                .embedding
                .entity_ref
                .starts_with(&self.identity.entity_ref_prefix)
            {
                return Err(FederatedRecallError::IncompatibleEntity {
                    source_id: self.identity.source_id.clone(),
                    reason: format!(
                        "retrieved entity '{}' falls outside prefix '{}'",
                        result.embedding.entity_ref, self.identity.entity_ref_prefix
                    ),
                });
            }
            let Some(text) = result
                .embedding
                .passage_text
                .filter(|text| !text.trim().is_empty())
            else {
                missing_text += 1;
                continue;
            };
            hits.push(ExternalPassageHit {
                source_id: self.identity.source_id.clone(),
                display_name: self.identity.display_name.clone(),
                run_id: self.identity.run_id.clone(),
                embedding_id: result.embedding.id,
                entity_ref: result.embedding.entity_ref,
                text,
                model: result.embedding.model,
                distance: result.distance,
                source_rank: index + 1,
            });
        }
        Ok(ExternalPassageBatch {
            source_id: self.identity.source_id.clone(),
            hits,
            missing_text,
        })
    }
}

#[derive(Deserialize)]
struct RunIdentity {
    schema_version: u32,
    run_id: String,
    requested_embedding_model: String,
    actual_embedding_model: String,
    indexes: BTreeMap<String, String>,
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
