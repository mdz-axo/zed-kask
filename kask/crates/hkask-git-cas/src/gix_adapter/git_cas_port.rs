//! GitCASPort trait implementation — put_blob, get_blob, delete_blob, snapshot, etc.

use super::{GixCasAdapter, build_tree, oid_to_commit_hash, open_or_init_repo, spawn_blocking_io};
use crate::gix_adapter::tree::{commit_tree_oid, list_tree_recursive};
use hkask_types::NotFound;
use hkask_types::git_cas::{
    CommitHash, ContentHash, GitCASPort, GitCasError, GitCasVerificationReport, LogEntry, RepoId,
    TreeEntry,
};

#[async_trait::async_trait]
impl GitCASPort for GixCasAdapter {
    async fn put_blob(&self, repo: &RepoId, content: &[u8]) -> Result<ContentHash, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let hash = ContentHash::from_blake3(content);
        let cas_dir = repo_dir.join("cas");
        let blob_path = cas_dir.join(hash.to_string());
        let content = content.to_vec();
        spawn_blocking_io(move || {
            std::fs::create_dir_all(&cas_dir)
                .map_err(|e| GitCasError::Io(format!("Failed to create CAS dir: {e}")))?;
            std::fs::write(&blob_path, &content)
                .map_err(|e| GitCasError::Io(format!("Failed to write blob: {e}")))
        })
        .await?;
        Ok(hash)
    }

    async fn get_blob(&self, repo: &RepoId, hash: &ContentHash) -> Result<Vec<u8>, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let blob_path = repo_dir.join("cas").join(hash.to_string());
        spawn_blocking_io(move || {
            std::fs::read(&blob_path).map_err(|e| {
                GitCasError::NotFound(NotFound {
                    entity_type: "blob".to_string(),
                    id: format!("Blob not found: {e}"),
                })
            })
        })
        .await
    }

    async fn delete_blob(&self, repo: &RepoId, hash: &ContentHash) -> Result<(), GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let blob_path = repo_dir.join("cas").join(hash.to_string());
        spawn_blocking_io(move || {
            if blob_path.exists() {
                std::fs::remove_file(&blob_path)
                    .map_err(|e| GitCasError::Io(format!("Failed to delete blob: {e}")))?;
            }
            Ok(())
        })
        .await
    }

    async fn snapshot(&self, repo: &RepoId, message: &str) -> Result<CommitHash, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let msg = message.to_string();
        spawn_blocking_io(move || {
            let repo = open_or_init_repo(&repo_dir)?;
            let cas_dir = repo_dir.join("cas");

            let mut tree_entries: Vec<(String, gix::ObjectId)> = Vec::new();
            if cas_dir.exists() {
                for entry in std::fs::read_dir(&cas_dir)
                    .map_err(|e| GitCasError::Io(format!("read_dir cas: {e}")))?
                {
                    let entry = entry.map_err(|e| GitCasError::Io(format!("dir entry: {e}")))?;
                    let path = entry.path();
                    if path.is_dir() {
                        continue;
                    }
                    let filename = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let content = std::fs::read(&path)
                        .map_err(|e| GitCasError::Io(format!("read blob: {e}")))?;
                    let oid = repo
                        .write_object(gix::objs::BlobRef { data: &content })
                        .map_err(|e| GitCasError::Git(format!("gix write_object: {e}")))?;
                    tree_entries.push((filename, oid.detach()));
                }
            }

            let tree_id = build_tree(&repo, &tree_entries)?;

            let parent = repo.head_commit().ok().map(|c| c.id().detach());
            let parents: Vec<gix::ObjectId> = parent.into_iter().collect();

            let commit_oid = repo
                .commit("HEAD", &msg, tree_id, parents)
                .map_err(|e| GitCasError::Git(format!("gix commit: {e}")))?;

            Ok(oid_to_commit_hash(&commit_oid.detach()))
        })
        .await
    }

    async fn snapshot_orphan(
        &self,
        repo: &RepoId,
        message: &str,
    ) -> Result<CommitHash, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let msg = message.to_string();
        spawn_blocking_io(move || {
            let repo = open_or_init_repo(&repo_dir)?;
            let cas_dir = repo_dir.join("cas");

            let mut tree_entries: Vec<(String, gix::ObjectId)> = Vec::new();
            if cas_dir.exists() {
                for entry in std::fs::read_dir(&cas_dir)
                    .map_err(|e| GitCasError::Io(format!("read_dir cas: {e}")))?
                {
                    let entry = entry.map_err(|e| GitCasError::Io(format!("dir entry: {e}")))?;
                    let path = entry.path();
                    if path.is_dir() {
                        continue;
                    }
                    let filename = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let content = std::fs::read(&path)
                        .map_err(|e| GitCasError::Io(format!("read blob: {e}")))?;
                    let oid = repo
                        .write_object(gix::objs::BlobRef { data: &content })
                        .map_err(|e| GitCasError::Git(format!("gix write_object: {e}")))?;
                    tree_entries.push((filename, oid.detach()));
                }
            }

            let tree_id = build_tree(&repo, &tree_entries)?;

            let parents: Vec<gix::ObjectId> = Vec::new();
            let commit_oid = repo
                .commit("HEAD", &msg, tree_id, parents)
                .map_err(|e| GitCasError::Git(format!("gix commit: {e}")))?;

            Ok(oid_to_commit_hash(&commit_oid.detach()))
        })
        .await
    }

    async fn list_tree(
        &self,
        repo: &RepoId,
        reference: &str,
        prefix: &str,
    ) -> Result<Vec<TreeEntry>, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let ref_name = reference.to_string();
        let prefix_filter = prefix.to_string();
        spawn_blocking_io(move || {
            let repo =
                gix::open(&repo_dir).map_err(|e| GitCasError::Git(format!("gix::open: {e}")))?;
            let id = repo
                .rev_parse_single(ref_name.as_str())
                .map_err(|e| GitCasError::Git(format!("gix rev_parse '{ref_name}': {e}")))?;
            let oid = id.detach();
            let tree_oid = commit_tree_oid(&repo, &oid)?;

            let mut entries = Vec::new();
            list_tree_recursive(&repo, &tree_oid, "", &prefix_filter, &mut entries)?;
            Ok(entries)
        })
        .await
    }

    async fn verify(&self, repo: &RepoId) -> Result<GitCasVerificationReport, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let cas_dir = repo_dir.join("cas");
        let repo_id = repo.clone();
        spawn_blocking_io(move || {
            if !cas_dir.exists() {
                return Ok(GitCasVerificationReport {
                    repo: repo_id,
                    total_blobs: 0,
                    verified_blobs: 0,
                    corrupt_hashes: vec![],
                });
            }
            let entries = std::fs::read_dir(&cas_dir)
                .map_err(|e| GitCasError::Io(format!("Failed to read CAS dir: {e}")))?;
            let mut total = 0usize;
            let mut verified = 0usize;
            let mut corrupt = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| GitCasError::Io(format!("Dir entry error: {e}")))?;
                let path = entry.path();
                if path.is_dir() {
                    continue;
                }
                let content = std::fs::read(&path)
                    .map_err(|e| GitCasError::Io(format!("Failed to read blob: {e}")))?;
                let actual_hash = ContentHash::from_blake3(&content);
                let expected_hash: ContentHash = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .parse()
                    .map_err(|e: hkask_types::git_cas::ParseHashError| {
                        GitCasError::PathValidation(format!(
                            "Invalid blob hash filename '{}': {e}",
                            path.display()
                        ))
                    })?;
                total += 1;
                if actual_hash == expected_hash {
                    verified += 1;
                } else {
                    corrupt.push(expected_hash);
                }
            }
            Ok(GitCasVerificationReport {
                repo: repo_id,
                total_blobs: total,
                verified_blobs: verified,
                corrupt_hashes: corrupt,
            })
        })
        .await
    }

    async fn log(&self, repo: &RepoId, max_count: usize) -> Result<Vec<LogEntry>, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let max = max_count;
        spawn_blocking_io(move || {
            let repo = match gix::open(&repo_dir) {
                Ok(r) => r,
                Err(_) => return Ok(Vec::new()),
            };
            let head_commit = match repo.head_commit() {
                Ok(c) => c,
                Err(_) => return Ok(Vec::new()),
            };
            let platform = repo.rev_walk(Some(head_commit.id().detach()));
            let mut entries = Vec::new();
            let walk = match platform.all() {
                Ok(w) => w,
                Err(_) => return Ok(Vec::new()),
            };
            for (count, item) in walk.enumerate() {
                if count >= max {
                    break;
                }
                let Ok(info) = item else { continue };
                let commit = match info.object().ok() {
                    Some(c) => c,
                    None => continue,
                };
                let message = commit
                    .message_raw()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                let timestamp_secs = commit.time().map(|t| t.seconds as u64).unwrap_or(0);
                entries.push(LogEntry {
                    commit: oid_to_commit_hash(&info.id),
                    message,
                    timestamp_secs,
                });
            }
            Ok(entries)
        })
        .await
    }
}
