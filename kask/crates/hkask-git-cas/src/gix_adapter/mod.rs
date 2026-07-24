//! Gix-based Git CAS Adapter — implements `GitCASPort` with the `gix` crate.
//! # REQ: F8 — pure Rust gitoxide, no CLI git subprocess.
//! expect: "Git CAS operations use pure Rust gitoxide without CLI subprocess"
//!
//! Blob storage: BLAKE3-addressed flat files in `cas/<hash>` (unchanged).
//! Git operations: pure `gix` crate v0.81.
//!
//! Snapshot strategy: reads files from `cas/`, writes each as a git blob object,
//! builds a tree from blob OIDs, commits the tree. No index needed.

use hkask_types::git_cas::{CommitHash, GitCasError, RepoId};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

mod admin;
mod git_cas_port;
mod pod_backup;
mod tree;

#[cfg(test)]
mod tests;

pub struct GixCasAdapter {
    base_path: PathBuf,
    initialized: RwLock<std::collections::HashSet<String>>,
}

pub(crate) fn resolve_cas_home() -> PathBuf {
    std::env::var("HKASK_CAS_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".hkask").join("repos")
        })
}

// ── gix helpers ─────────────────────────────────────────────────────────

fn open_or_init_repo(path: &Path) -> Result<gix::Repository, GitCasError> {
    if path.join(".git").exists() {
        gix::open(path).map_err(|e| GitCasError::Git(format!("gix::open: {e}")))
    } else {
        gix::init(path).map_err(|e| GitCasError::Git(format!("gix::init: {e}")))?;
        // gix::commit requires author identity — set a default in the repo config
        // so commits work in environments without global git config (e.g. CI).
        let config_path = path.join(".git").join("config");
        std::fs::write(
            &config_path,
            "[user]\n\tname = hkask\n\temail = hkask@localhost\n",
        )
        .map_err(|e| GitCasError::Io(format!("Failed to write git config: {e}")))?;
        // Re-open to pick up the config (the init-returned Repository cached the empty config).
        gix::open(path).map_err(|e| GitCasError::Git(format!("gix::open after init: {e}")))
    }
}

fn oid_to_commit_hash(oid: &gix::ObjectId) -> CommitHash {
    let bytes = oid.as_bytes();
    let mut arr = [0u8; 20];
    let len = bytes.len().min(20);
    arr[..len].copy_from_slice(&bytes[..len]);
    CommitHash::from_bytes(arr)
}

fn build_tree(
    repo: &gix::Repository,
    entries: &[(String, gix::ObjectId)],
) -> Result<gix::ObjectId, GitCasError> {
    if entries.is_empty() {
        let empty: Vec<gix::objs::tree::EntryRef<'_>> = Vec::new();
        return Ok(repo
            .write_object(gix::objs::TreeRef { entries: empty })
            .map_err(|e| GitCasError::Git(format!("gix write empty tree: {e}")))?
            .detach());
    }
    let mut sorted: Vec<_> = entries.to_vec();
    sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
    let entries_refs: Vec<gix::objs::tree::EntryRef<'_>> = sorted
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

async fn spawn_blocking_io<F, T>(f: F) -> Result<T, GitCasError>
where
    F: FnOnce() -> Result<T, GitCasError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| GitCasError::Io(format!("Task join error: {e}")))?
}

impl GixCasAdapter {
    /// Create a new GixCasAdapter at the given base path.
    ///
    /// pre:  base_path is a valid directory path (created if missing)
    /// post: returns GixCasAdapter with initialized set
    #[must_use = "result must be used"]
    pub fn new(base_path: impl Into<PathBuf>) -> Result<Self, GitCasError> {
        let base_path = base_path.into();
        std::fs::create_dir_all(&base_path)
            .map_err(|e| GitCasError::Io(format!("Failed to create base path: {e}")))?;
        Ok(Self {
            base_path,
            initialized: RwLock::new(std::collections::HashSet::new()),
        })
    }

    /// Create a GixCasAdapter from the HKASK_CAS_HOME environment variable.
    ///
    /// post: returns GixCasAdapter at resolved CAS home path
    #[must_use = "result must be used"]
    pub fn from_env() -> Result<Self, GitCasError> {
        Self::new(resolve_cas_home())
    }

    pub(crate) async fn ensure_repo_dir(&self, repo: &RepoId) -> Result<PathBuf, GitCasError> {
        let dir_name = repo.dir_name().to_string();
        {
            let init = self.initialized.read().await;
            if init.contains(&dir_name) {
                return Ok(self.base_path.join(&dir_name));
            }
        }
        let repo_path = self.base_path.join(&dir_name);
        let cas_path = repo_path.join("cas");
        spawn_blocking_io(move || {
            std::fs::create_dir_all(&cas_path)
                .map_err(|e| GitCasError::Io(format!("Failed to create CAS dir: {e}")))?;
            Ok(repo_path)
        })
        .await?;
        let mut init = self.initialized.write().await;
        init.insert(dir_name);
        Ok(self.base_path.join(repo.dir_name()))
    }
}
