//! The spreadsheet artifact store: contained path resolution, immutable
//! atomic revision publication, content digests, operation (idempotency)
//! records, and per-artifact metadata (plan §6, §7).
//!
//! Layout beneath the root (§7: `~/Documents/zk-data/spreadsheet-mcp/workbooks/`,
//! resolved through `hkask_types::agent_paths`):
//!
//! ```text
//! {root}/
//!   {artifact_id}/
//!     artifact.json            # origin, title, sheet, dimensions
//!     {revision_id}.xlsx       # immutable revision files
//!     ops/{key-digest}.json    # completed-operation (idempotency) records
//! ```
//!
//! Every revision write is atomic (temp file + fsync + rename); the base
//! revision file is never rewritten. Operation-record filenames are the
//! SHA-256 of the idempotency key — the key itself stays inside the record —
//! so caller-chosen keys never touch the filesystem as-is and distinct keys
//! can never collide.

use std::io::Write as _;
use std::path::PathBuf;

use hkask_types::agent_paths;
use hkask_types::spreadsheet::{ArtifactOrigin, SpreadsheetArtifactRef, SpreadsheetError};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// One artifact's publication metadata, written once at publish and read back
/// when constructing blocks for later revisions of the same artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub origin: ArtifactOrigin,
    pub title: String,
    pub sheet_name: String,
    /// Total workbook rows (header + data), for the initial viewport window.
    pub rows: usize,
    /// Total workbook columns, for the initial viewport window.
    pub cols: usize,
}

/// A completed operation record, keyed by the caller's idempotency key
/// (§7: repeated identity returns the same result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRecord {
    pub idempotency_key: String,
    pub base: SpreadsheetArtifactRef,
    pub result: SpreadsheetArtifactRef,
}

/// The store is owned by the engine actor thread; every method runs there.
pub struct ArtifactStore {
    root: PathBuf,
}

/// The production artifact root (plan §7): the visible artifacts tree under
/// `spreadsheet-mcp/workbooks/`, honoring `HKASK_ARTIFACTS_DIR`.
pub fn production_root() -> PathBuf {
    agent_paths::resolve_under_artifacts_dir(&agent_paths::mcp_artifacts_subdir(
        "spreadsheet",
        "workbooks",
    ))
}

/// Lowercase hex SHA-256 of `bytes` — the content digest of a revision.
pub fn digest_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256.digest(bytes))
}

impl ArtifactStore {
    /// Open the store at `root`, creating the root directory eagerly so a
    /// misconfigured artifacts path fails at startup, not mid-publication.
    pub fn at(root: PathBuf) -> Result<Self, SpreadsheetError> {
        std::fs::create_dir_all(&root).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot create artifact root {}: {error}", root.display()),
        })?;
        Ok(Self { root })
    }

    fn artifact_dir(&self, artifact_id: &str) -> PathBuf {
        self.root.join(artifact_id)
    }

    /// Resolve a revision file beneath the root, rejecting containment
    /// escapes (the backstop behind the contract's single-segment rule).
    fn revision_path(
        &self,
        artifact_id: &str,
        revision_id: &str,
    ) -> Result<PathBuf, SpreadsheetError> {
        let artifact_dir = self.artifact_dir(artifact_id);
        let canonical = std::fs::canonicalize(&artifact_dir).map_err(|_| {
            SpreadsheetError::UnknownArtifact {
                artifact_id: artifact_id.to_string(),
            }
        })?;
        if !canonical.starts_with(&self.root) {
            return Err(SpreadsheetError::PathEscape {
                id: artifact_id.to_string(),
            });
        }
        Ok(canonical.join(format!("{revision_id}.xlsx")))
    }

    /// Atomically publish an immutable revision: temp file in the same
    /// directory, fsync, rename. The final name is never overwritten by this
    /// path — a fresh revision id is minted for every publication.
    pub fn write_revision(
        &self,
        artifact_id: &str,
        revision_id: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, SpreadsheetError> {
        let dir = self.artifact_dir(artifact_id);
        std::fs::create_dir_all(dir.join("ops")).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot create artifact dir {}: {error}", dir.display()),
        })?;
        let path = self.revision_path(artifact_id, revision_id)?;
        if path.exists() {
            return Err(SpreadsheetError::Engine {
                detail: format!(
                    "revision {revision_id} of artifact {artifact_id} already exists; revisions are immutable"
                ),
            });
        }
        let tmp = dir.join(format!(".{revision_id}.xlsx.tmp"));
        {
            let mut file =
                std::fs::File::create(&tmp).map_err(|error| SpreadsheetError::Engine {
                    detail: format!(
                        "cannot create temp revision file {}: {error}",
                        tmp.display()
                    ),
                })?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| SpreadsheetError::Engine {
                    detail: format!("cannot write temp revision file {}: {error}", tmp.display()),
                })?;
        }
        std::fs::rename(&tmp, &path).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot publish revision file {}: {error}", path.display()),
        })?;
        Ok(path)
    }

    /// Read one revision's bytes; `UnknownArtifact` when the artifact or the
    /// revision does not exist.
    pub fn read_revision(
        &self,
        artifact_id: &str,
        revision_id: &str,
    ) -> Result<Vec<u8>, SpreadsheetError> {
        let path = self.revision_path(artifact_id, revision_id)?;
        std::fs::read(&path).map_err(|error| SpreadsheetError::UnknownArtifact {
            artifact_id: format!("{artifact_id}/{revision_id} ({error})"),
        })
    }

    pub fn write_metadata(
        &self,
        artifact_id: &str,
        meta: &ArtifactMeta,
    ) -> Result<(), SpreadsheetError> {
        let path = self.artifact_dir(artifact_id).join("artifact.json");
        let bytes = serde_json::to_vec(meta).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot serialize artifact metadata: {error}"),
        })?;
        std::fs::write(path, bytes).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot write artifact metadata: {error}"),
        })
    }

    pub fn read_metadata(&self, artifact_id: &str) -> Result<ArtifactMeta, SpreadsheetError> {
        let path = self.artifact_dir(artifact_id).join("artifact.json");
        let bytes = std::fs::read(&path).map_err(|_| SpreadsheetError::UnknownArtifact {
            artifact_id: artifact_id.to_string(),
        })?;
        serde_json::from_slice(&bytes).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot parse artifact metadata for {artifact_id}: {error}"),
        })
    }

    fn op_path(&self, artifact_id: &str, idempotency_key: &str) -> PathBuf {
        // The key never touches the filesystem as-is: hashed filename, key
        // stored inside the record.
        self.artifact_dir(artifact_id)
            .join("ops")
            .join(format!("{}.json", digest_of(idempotency_key.as_bytes())))
    }

    /// Record a completed operation under its idempotency key. Called after
    /// the result revision is durably published.
    pub fn record_operation(
        &self,
        artifact_id: &str,
        record: &OperationRecord,
    ) -> Result<(), SpreadsheetError> {
        let path = self.op_path(artifact_id, &record.idempotency_key);
        let bytes = serde_json::to_vec(record).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot serialize operation record: {error}"),
        })?;
        std::fs::write(&path, bytes).map_err(|error| SpreadsheetError::Engine {
            detail: format!("cannot write operation record: {error}"),
        })
    }

    /// Find a completed operation by idempotency key, if one exists.
    pub fn find_operation(
        &self,
        artifact_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<OperationRecord>, SpreadsheetError> {
        let path = self.op_path(artifact_id, idempotency_key);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(SpreadsheetError::Engine {
                    detail: format!("cannot read operation record: {error}"),
                });
            }
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| SpreadsheetError::Engine {
                detail: format!("cannot parse operation record: {error}"),
            })
    }
}
