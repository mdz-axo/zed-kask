//! Admin-level operations — resolve_ref, diff (not part of GitCASPort).

use super::tree::{collect_paths, commit_tree_oid};
use super::{GixCasAdapter, oid_to_commit_hash, spawn_blocking_io};
use hkask_types::git_cas::{CommitHash, DiffKind, FileDiff, GitCasError, RepoId};

impl GixCasAdapter {
    /// Resolve a symbolic ref (branch, tag) to a commit SHA.
    ///
    /// This is an admin-level operation — not part of the `GitCASPort`
    /// backup contract. Used by the API git archive route.
    #[must_use = "result must be used"]
    pub async fn resolve_ref(
        &self,
        repo: &RepoId,
        reference: &str,
    ) -> Result<CommitHash, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let ref_name = reference.to_string();
        spawn_blocking_io(move || {
            let repo =
                gix::open(&repo_dir).map_err(|e| GitCasError::Git(format!("gix::open: {e}")))?;
            let id = repo
                .rev_parse_single(ref_name.as_str())
                .map_err(|e| GitCasError::Git(format!("gix rev_parse '{ref_name}': {e}")))?;
            Ok(oid_to_commit_hash(&id.detach()))
        })
        .await
    }

    /// Diff two commits.
    ///
    /// This is an admin-level operation — not part of the `GitCASPort`
    /// backup contract. Used by the CLI `kask git diff` command.
    #[must_use = "result must be used"]
    pub async fn diff(
        &self,
        repo: &RepoId,
        from: &str,
        to: &str,
    ) -> Result<Vec<FileDiff>, GitCasError> {
        let repo_dir = self.ensure_repo_dir(repo).await?;
        let from_ref = from.to_string();
        let to_ref = to.to_string();
        spawn_blocking_io(move || {
            let repo =
                gix::open(&repo_dir).map_err(|e| GitCasError::Git(format!("gix::open: {e}")))?;
            let from_id = repo
                .rev_parse_single(from_ref.as_str())
                .map_err(|e| GitCasError::Git(format!("gix rev_parse '{from_ref}': {e}")))?;
            let to_id = repo
                .rev_parse_single(to_ref.as_str())
                .map_err(|e| GitCasError::Git(format!("gix rev_parse '{to_ref}': {e}")))?;

            let from_tree = commit_tree_oid(&repo, &from_id.detach())?;
            let to_tree = commit_tree_oid(&repo, &to_id.detach())?;

            let mut from_paths: std::collections::BTreeMap<String, gix::ObjectId> =
                std::collections::BTreeMap::new();
            let mut to_paths: std::collections::BTreeMap<String, gix::ObjectId> =
                std::collections::BTreeMap::new();
            collect_paths(&repo, &from_tree, "", &mut from_paths)?;
            collect_paths(&repo, &to_tree, "", &mut to_paths)?;

            let mut diffs = Vec::new();
            for p in to_paths.keys() {
                if !from_paths.contains_key(p) {
                    diffs.push(FileDiff {
                        path: p.clone(),
                        kind: DiffKind::Added,
                        content: String::new(),
                    });
                }
            }
            for p in from_paths.keys() {
                if !to_paths.contains_key(p) {
                    diffs.push(FileDiff {
                        path: p.clone(),
                        kind: DiffKind::Removed,
                        content: String::new(),
                    });
                }
            }
            for (p, from_oid) in &from_paths {
                if let Some(to_oid) = to_paths.get(p)
                    && from_oid != to_oid
                {
                    diffs.push(FileDiff {
                        path: p.clone(),
                        kind: DiffKind::Modified,
                        content: String::new(),
                    });
                }
            }
            Ok(diffs)
        })
        .await
    }
}
