//! Pod-directory-based backup operations — snapshot_pod_dir, log_pod, resolve_date, restore_file_from_commit.

use super::tree::commit_tree_oid;
use super::{GixCasAdapter, oid_to_commit_hash, open_or_init_repo, spawn_blocking_io};
use hkask_types::NotFound;
use hkask_types::git_cas::{CommitHash, GitCasError, LogEntry};
use std::path::Path;

// ── Directory tree helper ────────────────────────────────────────────────

/// Walk a directory recursively and write it as a git tree object.
/// Skips .git directory. Returns the tree OID.
fn write_dir_as_tree(
    repo: &gix::Repository,
    _root: &Path,
    dir: &Path,
) -> Result<gix::ObjectId, GitCasError> {
    let mut entries: Vec<(String, gix::ObjectId)> = Vec::new();

    for entry in std::fs::read_dir(dir).map_err(|e| GitCasError::Io(format!("read_dir: {e}")))? {
        let entry = entry.map_err(|e| GitCasError::Io(format!("dir entry: {e}")))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if name == ".git" {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|e| GitCasError::Io(format!("file_type: {e}")))?;

        if file_type.is_dir() {
            let subtree_oid = write_dir_as_tree(repo, _root, &path)?;
            entries.push((name, subtree_oid));
        } else if file_type.is_file() {
            let content = std::fs::read(&path)
                .map_err(|e| GitCasError::Io(format!("read file '{}': {e}", path.display())))?;
            let oid = repo
                .write_object(gix::objs::BlobRef { data: &content })
                .map_err(|e| GitCasError::Git(format!("write_object '{}': {e}", path.display())))?;
            entries.push((name, oid.detach()));
        }
    }

    if entries.is_empty() {
        let empty: Vec<gix::objs::tree::EntryRef<'_>> = Vec::new();
        return Ok(repo
            .write_object(gix::objs::TreeRef { entries: empty })
            .map_err(|e| GitCasError::Git(format!("gix write empty tree: {e}")))?
            .detach());
    }

    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    let entries_refs: Vec<gix::objs::tree::EntryRef<'_>> = entries
        .iter()
        .map(|(name, oid)| gix::objs::tree::EntryRef {
            mode: gix::objs::tree::EntryMode::from(gix::objs::tree::EntryKind::Blob),
            oid: oid.as_ref(),
            filename: name.as_str().into(),
        })
        .collect();

    Ok(repo
        .write_object(gix::objs::TreeRef {
            entries: entries_refs,
        })
        .map_err(|e| GitCasError::Git(format!("gix write tree: {e}")))?
        .detach())
}

impl GixCasAdapter {
    /// Snapshot a pod directory: walk the tree, create git blobs for all files,
    /// build a nested tree, commit. Skips .git directory.
    #[must_use = "result must be used"]
    pub async fn snapshot_pod_dir(
        &self,
        pod_dir: &Path,
        message: &str,
    ) -> Result<CommitHash, GitCasError> {
        let dir = pod_dir.to_path_buf();
        let msg = message.to_string();
        spawn_blocking_io(move || {
            if !dir.exists() {
                return Err(GitCasError::NotFound(NotFound {
                    entity_type: "pod_dir".to_string(),
                    id: format!("Pod directory does not exist: {}", dir.display()),
                }));
            }
            let repo = open_or_init_repo(&dir)?;
            let tree_oid = write_dir_as_tree(&repo, &dir, &dir)?;

            let parent = repo.head_commit().ok().map(|c| c.id().detach());
            let parents: Vec<gix::ObjectId> = parent.into_iter().collect();

            let commit_oid = repo
                .commit("HEAD", &msg, tree_oid, parents)
                .map_err(|e| GitCasError::Git(format!("gix commit: {e}")))?;

            Ok(oid_to_commit_hash(&commit_oid.detach()))
        })
        .await
    }

    /// List commit history for a pod directory, newest first.
    #[must_use = "result must be used"]
    pub async fn log_pod(
        &self,
        pod_dir: &Path,
        max_count: usize,
    ) -> Result<Vec<LogEntry>, GitCasError> {
        let dir = pod_dir.to_path_buf();
        let max = max_count;
        spawn_blocking_io(move || {
            let repo = match gix::open(&dir) {
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

    /// Find the commit closest to (but not after) a target date.
    /// Walks the log newest-first, returns the first commit with timestamp <= target_secs.
    #[must_use = "result must be used"]
    pub async fn resolve_date(
        &self,
        pod_dir: &Path,
        target_secs: u64,
    ) -> Result<Option<CommitHash>, GitCasError> {
        let entries = self.log_pod(pod_dir, 1000).await?;
        Ok(entries
            .into_iter()
            .find(|e| e.timestamp_secs <= target_secs)
            .map(|e| e.commit))
    }

    /// Restore a file from a prior commit to a destination path.
    #[must_use = "result must be used"]
    pub async fn restore_file_from_commit(
        &self,
        pod_dir: &Path,
        commit: &CommitHash,
        file_path: &str,
        dest: &Path,
    ) -> Result<(), GitCasError> {
        let dir = pod_dir.to_path_buf();
        let commit_str = commit.to_string();
        let fp = file_path.to_string();
        let d = dest.to_path_buf();
        spawn_blocking_io(move || {
            let repo = gix::open(&dir).map_err(|e| GitCasError::Git(format!("gix::open: {e}")))?;
            let id = repo
                .rev_parse_single(commit_str.as_str())
                .map_err(|e| GitCasError::Git(format!("gix rev_parse '{commit_str}': {e}")))?;
            let tree_oid = commit_tree_oid(&repo, &id.detach())?;
            let obj = repo
                .find_object(tree_oid)
                .map_err(|e| GitCasError::Git(format!("find_object tree: {e}")))?;
            let tree = obj
                .try_into_tree()
                .map_err(|e| GitCasError::Git(format!("try_into_tree: {e}")))?;

            let mut found = None;
            for entry in tree.iter() {
                let entry = entry.map_err(|e| GitCasError::Git(format!("tree entry: {e}")))?;
                if entry.filename().as_ref() as &[u8] == fp.as_bytes() {
                    if entry.mode().is_tree() {
                        return Err(GitCasError::NotFound(NotFound {
                            entity_type: "file".to_string(),
                            id: format!("'{fp}' is a directory, not a file"),
                        }));
                    }
                    let blob = repo
                        .find_object(entry.oid().to_owned())
                        .map_err(|e| GitCasError::Git(format!("find_object blob: {e}")))?;
                    found = Some(blob.data.to_vec());
                    break;
                }
            }

            let data = found.ok_or_else(|| {
                GitCasError::NotFound(NotFound {
                    entity_type: "file".to_string(),
                    id: format!("File '{fp}' not found in commit {commit_str}"),
                })
            })?;

            std::fs::write(&d, &data)
                .map_err(|e| GitCasError::Io(format!("Failed to write restored file: {e}")))?;
            Ok(())
        })
        .await
    }
}
