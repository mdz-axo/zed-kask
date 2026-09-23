//! External-source registry for Curator federated search.
//!
//! Loading never hides a source failure: a missing manifest, malformed current
//! contract, incompatible sealed identity, or unavailable database becomes a
//! typed status returned by the public tool. Successfully loaded sources are
//! immutable `ReadOnlyPassageSource` handles.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use hkask_memory::{FederatedRecallError, FederatedSourcesManifest, ReadOnlyPassageSource};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FederatedSourceState {
    Ready,
    Unconfigured,
    Invalid,
    Incompatible,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FederatedSourceStatus {
    pub source_id: String,
    pub source_kind: &'static str,
    pub state: FederatedSourceState,
    pub reason: Option<String>,
    pub result_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FileStamp {
    Missing,
    Unreadable(String),
    Present {
        bytes: u64,
        modified: Option<SystemTime>,
        #[cfg(unix)]
        device: u64,
        #[cfg(unix)]
        inode: u64,
    },
}

impl FileStamp {
    fn read(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => Self::Present {
                    bytes: metadata.len(),
                    modified: Some(modified),
                    #[cfg(unix)]
                    device: {
                        use std::os::unix::fs::MetadataExt;
                        metadata.dev()
                    },
                    #[cfg(unix)]
                    inode: {
                        use std::os::unix::fs::MetadataExt;
                        metadata.ino()
                    },
                },
                Err(error) => Self::Unreadable(error.to_string()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.to_string()),
        }
    }
}

pub(crate) struct FederatedSourceRegistry {
    sources: Vec<Arc<ReadOnlyPassageSource>>,
    statuses: Vec<FederatedSourceStatus>,
    watched: Vec<(PathBuf, FileStamp)>,
}

impl FederatedSourceRegistry {
    pub(crate) fn unchanged(&self) -> bool {
        // A failed admission is not a validated snapshot. Retry on the next
        // query even when the files have stopped changing; otherwise a single
        // mid-open modification leaves an empty `unavailable` cache forever.
        !self
            .statuses
            .iter()
            .any(|status| status.state == FederatedSourceState::Unavailable)
            && self
                .watched
                .iter()
                .all(|(path, stamp)| &FileStamp::read(path) == stamp)
    }

    fn snapshot(paths: &[PathBuf]) -> Vec<(PathBuf, FileStamp)> {
        paths
            .iter()
            .map(|path| (path.clone(), FileStamp::read(path)))
            .collect()
    }

    fn watched_paths(
        manifest_path: &Path,
        sources: &[hkask_memory::FederatedSourceSpec],
    ) -> Vec<PathBuf> {
        let mut paths = vec![manifest_path.to_path_buf()];
        for source in sources {
            let mut wal = source.database_path.as_os_str().to_os_string();
            wal.push("-wal");
            paths.extend([
                source.database_path.clone(),
                PathBuf::from(wal),
                source.run_identity_path.clone(),
                source.representations_manifest_path.clone(),
            ]);
        }
        paths
    }
    pub(crate) fn load(manifest_path: &Path, passphrase: Option<&str>) -> Self {
        if manifest_path.as_os_str().is_empty() || !manifest_path.exists() {
            return Self::single_status(
                manifest_path,
                "external_sources",
                FederatedSourceState::Unconfigured,
                format!(
                    "federated source manifest is not configured at {}",
                    manifest_path.display()
                ),
            );
        }
        let manifest = match FederatedSourcesManifest::load(manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                return Self::single_status(
                    manifest_path,
                    "external_sources",
                    FederatedSourceState::Invalid,
                    error.to_string(),
                );
            }
        };
        if manifest.sources.is_empty() {
            return Self::single_status(
                manifest_path,
                "external_sources",
                FederatedSourceState::Unconfigured,
                "federated source manifest contains no sources".to_string(),
            );
        }
        let watched_paths = Self::watched_paths(manifest_path, &manifest.sources);
        let before = Self::snapshot(&watched_paths);
        let Some(passphrase) = passphrase else {
            return Self {
                watched: before,
                statuses: manifest
                    .sources
                    .iter()
                    .map(|source| FederatedSourceStatus {
                        source_id: source.id.clone(),
                        source_kind: "corpus",
                        state: FederatedSourceState::Unavailable,
                        reason: Some("HKASK_DB_PASSPHRASE is unavailable".to_string()),
                        result_count: 0,
                    })
                    .collect(),
                sources: Vec::new(),
            };
        };

        let mut sources = Vec::new();
        let mut statuses = Vec::with_capacity(manifest.sources.len());
        for source in &manifest.sources {
            match ReadOnlyPassageSource::open(source, passphrase) {
                Ok(opened) => {
                    statuses.push(FederatedSourceStatus {
                        source_id: source.id.clone(),
                        source_kind: "corpus",
                        state: FederatedSourceState::Ready,
                        reason: None,
                        result_count: 0,
                    });
                    sources.push(Arc::new(opened));
                }
                Err(error) => statuses.push(FederatedSourceStatus {
                    source_id: source.id.clone(),
                    source_kind: "corpus",
                    state: classify_error(&error),
                    reason: Some(error.to_string()),
                    result_count: 0,
                }),
            }
        }
        let after = Self::snapshot(&watched_paths);
        if before != after {
            return Self {
                sources: Vec::new(),
                statuses: manifest
                    .sources
                    .iter()
                    .map(|source| FederatedSourceStatus {
                        source_id: source.id.clone(),
                        source_kind: "corpus",
                        state: FederatedSourceState::Unavailable,
                        reason: Some(
                            "source identity changed during admission; retry search".to_string(),
                        ),
                        result_count: 0,
                    })
                    .collect(),
                watched: after,
            };
        }
        Self {
            sources,
            statuses,
            watched: after,
        }
    }

    pub(crate) fn sources(&self) -> &[Arc<ReadOnlyPassageSource>] {
        &self.sources
    }

    pub(crate) fn statuses(&self) -> Vec<FederatedSourceStatus> {
        self.statuses.clone()
    }

    fn single_status(
        manifest_path: &Path,
        source_id: &str,
        state: FederatedSourceState,
        reason: String,
    ) -> Self {
        Self {
            sources: Vec::new(),
            watched: Self::snapshot(&[manifest_path.to_path_buf()]),
            statuses: vec![FederatedSourceStatus {
                source_id: source_id.to_string(),
                source_kind: "corpus",
                state,
                reason: Some(reason),
                result_count: 0,
            }],
        }
    }
}

pub(crate) fn default_manifest_path() -> PathBuf {
    let relative = hkask_types::agent_paths::agent_dir("curator").join("federated-sources.json");
    hkask_types::agent_paths::resolve_under_data_dir(&relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "A source unavailable during admission retries after its files settle." [P8]
    #[test]
    fn unavailable_source_does_not_cache_a_ready_fingerprint() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let manifest = directory.path().join("sources.json");
        std::fs::write(&manifest, "{}")?;
        let registry = FederatedSourceRegistry {
            sources: Vec::new(),
            watched: FederatedSourceRegistry::snapshot(&[manifest]),
            statuses: vec![FederatedSourceStatus {
                source_id: "changing-source".to_string(),
                source_kind: "corpus",
                state: FederatedSourceState::Unavailable,
                reason: Some("source identity changed during admission; retry search".to_string()),
                result_count: 0,
            }],
        };
        assert!(
            !registry.unchanged(),
            "a transient failure must retry without requiring another file change"
        );
        Ok(())
    }
}

pub(crate) fn classify_error(error: &FederatedRecallError) -> FederatedSourceState {
    match error {
        FederatedRecallError::UnsupportedSchema { .. }
        | FederatedRecallError::DigestMismatch { .. }
        | FederatedRecallError::RunIdentityMismatch { .. }
        | FederatedRecallError::UnsealedWal { .. }
        | FederatedRecallError::SchemaMismatch { .. }
        | FederatedRecallError::IncompatibleEmbedding { .. }
        | FederatedRecallError::IncompatibleEntity { .. } => FederatedSourceState::Incompatible,
        FederatedRecallError::InvalidManifest(_)
        | FederatedRecallError::MissingIndex { .. }
        | FederatedRecallError::ParseArtifact { .. } => FederatedSourceState::Invalid,
        FederatedRecallError::ReadArtifact { .. }
        | FederatedRecallError::Database { .. }
        | FederatedRecallError::Retrieval { .. } => FederatedSourceState::Unavailable,
    }
}
