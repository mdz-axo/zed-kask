//! Git tree helpers — commit_tree_oid, list_tree_recursive, collect_paths.

use hkask_types::git_cas::{ContentHash, GitCasError, TreeEntry, TreeEntryKind};

pub(crate) fn commit_tree_oid(
    repo: &gix::Repository,
    oid: &gix::ObjectId,
) -> Result<gix::ObjectId, GitCasError> {
    let obj = repo
        .find_object(*oid)
        .map_err(|e| GitCasError::Git(format!("find_object: {e}")))?;
    let commit = obj
        .try_into_commit()
        .map_err(|e| GitCasError::Git(format!("try_into_commit: {e}")))?;
    Ok(commit
        .tree_id()
        .map_err(|e| GitCasError::Git(format!("tree_id: {e}")))?
        .detach())
}

pub(crate) fn list_tree_recursive(
    repo: &gix::Repository,
    tree_oid: &gix::ObjectId,
    path_prefix: &str,
    filter_prefix: &str,
    out: &mut Vec<TreeEntry>,
) -> Result<(), GitCasError> {
    let obj = repo
        .find_object(*tree_oid)
        .map_err(|e| GitCasError::Git(format!("find_object tree: {e}")))?;
    let tree = obj
        .try_into_tree()
        .map_err(|e| GitCasError::Git(format!("try_into_tree: {e}")))?;
    for entry in tree.iter() {
        let entry = entry.map_err(|e| GitCasError::Git(format!("tree entry: {e}")))?;
        let name = entry.filename().to_string();
        let full_path = if path_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", path_prefix, name)
        };
        if entry.mode().is_tree() {
            list_tree_recursive(
                repo,
                &entry.oid().to_owned(),
                &full_path,
                filter_prefix,
                out,
            )?;
        } else if filter_prefix.is_empty() || full_path.starts_with(filter_prefix) {
            let blob_obj = repo
                .find_object(entry.oid().to_owned())
                .map_err(|e| GitCasError::Git(format!("find_object blob: {e}")))?;
            let content_hash = ContentHash::from_blake3(&blob_obj.data);
            out.push(TreeEntry {
                path: full_path,
                content_hash,
                kind: TreeEntryKind::Blob,
            });
        }
    }
    Ok(())
}

pub(crate) fn collect_paths(
    repo: &gix::Repository,
    tree_oid: &gix::ObjectId,
    prefix: &str,
    out: &mut std::collections::BTreeMap<String, gix::ObjectId>,
) -> Result<(), GitCasError> {
    let obj = repo
        .find_object(*tree_oid)
        .map_err(|e| GitCasError::Git(format!("find_object tree: {e}")))?;
    let tree = obj
        .try_into_tree()
        .map_err(|e| GitCasError::Git(format!("try_into_tree: {e}")))?;
    for entry in tree.iter() {
        let entry = entry.map_err(|e| GitCasError::Git(format!("tree entry: {e}")))?;
        let name = entry.filename().to_string();
        let full_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        if entry.mode().is_tree() {
            collect_paths(repo, &entry.oid().to_owned(), &full_path, out)?;
        } else {
            out.insert(full_path, entry.oid().to_owned());
        }
    }
    Ok(())
}
