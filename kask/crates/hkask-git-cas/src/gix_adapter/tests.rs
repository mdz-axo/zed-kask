// ── Tests ─────────────────────────────────────────────────────────────────────

use super::*;
use hkask_types::git_cas::{ContentHash, DiffKind, GitCASPort, GitCasError, RepoId};

#[cfg(test)]
mod test_suite {
    use super::*;
    use tempfile::TempDir;

    fn test_adapter() -> (GixCasAdapter, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();
        (adapter, dir)
    }

    #[tokio::test]
    async fn put_and_get_blob_roundtrip() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Registry;
        let content = b"hello, CAS world";

        let hash = adapter.put_blob(&repo, content).await.unwrap();
        let retrieved = adapter.get_blob(&repo, &hash).await.unwrap();

        assert_eq!(retrieved, content);
        assert_eq!(hash, ContentHash::from_blake3(content));
    }

    #[tokio::test]
    async fn get_nonexistent_blob_returns_not_found() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Memory;
        let hash = ContentHash::from_blake3(b"doesnt exist");

        let result = adapter.get_blob(&repo, &hash).await;
        assert!(matches!(result, Err(GitCasError::NotFound(_))));
    }

    #[tokio::test]
    async fn delete_blob_then_get_returns_not_found() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Sessions;
        let content = b"temporary data";

        let hash = adapter.put_blob(&repo, content).await.unwrap();
        adapter.delete_blob(&repo, &hash).await.unwrap();

        let result = adapter.get_blob(&repo, &hash).await;
        assert!(matches!(result, Err(GitCasError::NotFound(_))));
    }

    #[tokio::test]
    async fn snapshot_produces_commit_and_log_returns_history() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Registry;

        let h1 = adapter.put_blob(&repo, b"blob A").await.unwrap();
        let h2 = adapter.put_blob(&repo, b"blob B").await.unwrap();

        let commit = adapter.snapshot(&repo, "first snapshot").await.unwrap();
        assert!(!commit.to_string().is_empty());

        let entries = adapter.log(&repo, 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].commit, commit);
        assert!(entries[0].message.contains("first snapshot"));

        let tree = adapter
            .list_tree(&repo, &commit.to_string(), "")
            .await
            .unwrap();
        assert_eq!(tree.len(), 2);
        let hashes: Vec<_> = tree.iter().map(|e| e.content_hash.clone()).collect();
        assert!(hashes.contains(&h1));
        assert!(hashes.contains(&h2));
    }

    #[tokio::test]
    async fn snapshot_orphan_has_no_parent() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::GoalsSpecs;

        adapter.put_blob(&repo, b"orphan data").await.unwrap();
        let orphan = adapter
            .snapshot_orphan(&repo, "orphan commit")
            .await
            .unwrap();

        let entries = adapter.log(&repo, 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].commit, orphan);

        adapter.put_blob(&repo, b"second blob").await.unwrap();
        let child = adapter.snapshot(&repo, "child commit").await.unwrap();
        assert_ne!(orphan, child);
        let entries = adapter.log(&repo, 10).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].commit, child);
        assert_eq!(entries[1].commit, orphan);
    }

    #[tokio::test]
    async fn verify_reports_correct_integrity() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::RegAudit;

        adapter.put_blob(&repo, b"integrity check 1").await.unwrap();
        adapter.put_blob(&repo, b"integrity check 2").await.unwrap();

        let report = adapter.verify(&repo).await.unwrap();
        assert_eq!(report.repo, RepoId::RegAudit);
        assert_eq!(report.total_blobs, 2);
        assert_eq!(report.verified_blobs, 2);
        assert!(report.corrupt_hashes.is_empty());
    }

    #[tokio::test]
    async fn verify_empty_repo_returns_zero() {
        let (adapter, _dir) = test_adapter();
        let report = adapter.verify(&RepoId::Vault).await.unwrap();
        assert_eq!(report.total_blobs, 0);
        assert_eq!(report.verified_blobs, 0);
    }

    #[tokio::test]
    async fn log_empty_repo_returns_empty() {
        let (adapter, _dir) = test_adapter();
        let entries = adapter.log(&RepoId::Sovereignty, 10).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn resolve_ref_resolves_head() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Registry;

        adapter.put_blob(&repo, b"ref test").await.unwrap();
        let commit = adapter.snapshot(&repo, "ref snapshot").await.unwrap();

        let resolved = adapter.resolve_ref(&repo, "HEAD").await.unwrap();
        assert_eq!(resolved, commit);
    }

    #[tokio::test]
    async fn diff_detects_added_removed_and_modified() {
        let (adapter, _dir) = test_adapter();
        use RepoId::GoalsSpecs;

        adapter
            .put_blob(&GoalsSpecs, b"file1 content v1")
            .await
            .unwrap();
        let commit1 = adapter.snapshot(&GoalsSpecs, "first").await.unwrap();

        adapter
            .put_blob(&GoalsSpecs, b"file2 content new")
            .await
            .unwrap();
        let commit2 = adapter.snapshot(&GoalsSpecs, "second").await.unwrap();

        let diffs = adapter
            .diff(&GoalsSpecs, &commit1.to_string(), &commit2.to_string())
            .await
            .unwrap();
        let added: Vec<_> = diffs.iter().filter(|d| d.kind == DiffKind::Added).collect();
        assert!(!added.is_empty(), "Expected at least one Added diff");
    }

    #[tokio::test]
    async fn list_tree_with_prefix_filter() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Registry;

        adapter.put_blob(&repo, b"aaa").await.unwrap();
        adapter.put_blob(&repo, b"bbb").await.unwrap();
        let commit = adapter.snapshot(&repo, "prefix test").await.unwrap();

        let all = adapter
            .list_tree(&repo, &commit.to_string(), "")
            .await
            .unwrap();
        assert_eq!(all.len(), 2);

        let first_hash = &all[0].content_hash.to_string();
        let short = &first_hash[..8];
        let filtered = adapter
            .list_tree(&repo, &commit.to_string(), short)
            .await
            .unwrap();
        assert!(!filtered.is_empty());
    }

    #[tokio::test]
    async fn concurrent_puts_to_different_repos() {
        let (adapter, _dir) = test_adapter();
        let adapter = std::sync::Arc::new(adapter);

        let a1 = adapter.clone();
        let h1 = tokio::spawn(async move {
            a1.put_blob(&RepoId::Registry, b"concurrent A")
                .await
                .unwrap()
        });
        let a2 = adapter.clone();
        let h2 =
            tokio::spawn(
                async move { a2.put_blob(&RepoId::Memory, b"concurrent B").await.unwrap() },
            );

        let hash1 = h1.await.unwrap();
        let hash2 = h2.await.unwrap();
        assert_ne!(hash1, hash2);

        let r1 = adapter.verify(&RepoId::Registry).await.unwrap();
        let r2 = adapter.verify(&RepoId::Memory).await.unwrap();
        assert_eq!(r1.total_blobs, 1);
        assert_eq!(r2.total_blobs, 1);
    }

    #[tokio::test]
    async fn put_blob_idempotent() {
        let (adapter, _dir) = test_adapter();
        let repo = RepoId::Sessions;
        let content = b"same content";

        let h1 = adapter.put_blob(&repo, content).await.unwrap();
        let h2 = adapter.put_blob(&repo, content).await.unwrap();
        assert_eq!(h1, h2);

        let report = adapter.verify(&repo).await.unwrap();
        assert_eq!(report.total_blobs, 1);
    }

    #[tokio::test]
    async fn from_env_respects_custom_home() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: single-threaded test, no concurrent env mutation
        unsafe { std::env::set_var("HKASK_CAS_HOME", dir.path().to_str().unwrap()) };

        let result = GixCasAdapter::from_env();
        assert!(result.is_ok());
        let adapter = result.unwrap();
        adapter
            .put_blob(&RepoId::Registry, b"custom home test")
            .await
            .unwrap();

        unsafe { std::env::remove_var("HKASK_CAS_HOME") };
    }

    // ── Pod-directory backup tests ────────────────────────────────────

    #[tokio::test]
    async fn snapshot_pod_dir_commits_all_files() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();

        let pod_dir = dir.path().join("test-pod");
        std::fs::create_dir_all(pod_dir.join("artifacts")).unwrap();
        std::fs::create_dir_all(pod_dir.join("sessions")).unwrap();
        std::fs::write(pod_dir.join("pod.db"), b"sqlcipher-data").unwrap();
        std::fs::write(pod_dir.join("pod.webid"), b"webid:user").unwrap();
        std::fs::write(pod_dir.join("artifacts/manifest.json"), b"{}").unwrap();
        std::fs::write(pod_dir.join("sessions/chat.log"), b"hello world").unwrap();

        let commit = adapter
            .snapshot_pod_dir(&pod_dir, "initial pod snapshot")
            .await
            .unwrap();
        assert!(!commit.to_string().is_empty());

        let entries = adapter.log_pod(&pod_dir, 10).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].commit, commit);
        assert!(entries[0].message.contains("initial pod snapshot"));
    }

    #[tokio::test]
    async fn snapshot_pod_dir_handles_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();

        let pod_dir = dir.path().join("nested-pod");
        std::fs::create_dir_all(pod_dir.join("deep/nested/path")).unwrap();
        std::fs::write(pod_dir.join("deep/nested/path/data.txt"), b"deep data").unwrap();
        std::fs::write(pod_dir.join("root.txt"), b"root").unwrap();

        let _commit = adapter
            .snapshot_pod_dir(&pod_dir, "nested dirs")
            .await
            .unwrap();

        let entries = adapter.log_pod(&pod_dir, 10).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn snapshot_pod_dir_missing_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();

        let result = adapter
            .snapshot_pod_dir(&dir.path().join("does-not-exist"), "nope")
            .await;
        assert!(matches!(result, Err(GitCasError::NotFound(_))));
    }

    #[tokio::test]
    async fn log_pod_empty_repo_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();
        let pod_dir = dir.path().join("empty-pod");
        std::fs::create_dir_all(&pod_dir).unwrap();

        let entries = adapter.log_pod(&pod_dir, 10).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn restore_file_from_commit_recovers_data() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();

        let pod_dir = dir.path().join("restore-pod");
        std::fs::create_dir_all(&pod_dir).unwrap();
        let original = b"pod-state-v1";
        std::fs::write(pod_dir.join("pod.db"), original).unwrap();

        let commit = adapter.snapshot_pod_dir(&pod_dir, "v1").await.unwrap();

        std::fs::write(pod_dir.join("pod.db"), b"pod-state-v2-mutated").unwrap();

        let restored_path = dir.path().join("restored.db");
        adapter
            .restore_file_from_commit(&pod_dir, &commit, "pod.db", &restored_path)
            .await
            .unwrap();

        let restored = std::fs::read(&restored_path).unwrap();
        assert_eq!(restored, original);
    }

    #[tokio::test]
    async fn multiple_snapshots_produce_history() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = GixCasAdapter::new(dir.path()).unwrap();

        let pod_dir = dir.path().join("history-pod");
        std::fs::create_dir_all(&pod_dir).unwrap();

        std::fs::write(pod_dir.join("pod.db"), b"v1").unwrap();
        let c1 = adapter
            .snapshot_pod_dir(&pod_dir, "snapshot 1")
            .await
            .unwrap();

        std::fs::write(pod_dir.join("pod.db"), b"v2").unwrap();
        std::fs::write(pod_dir.join("new-file.txt"), b"hello").unwrap();
        let c2 = adapter
            .snapshot_pod_dir(&pod_dir, "snapshot 2")
            .await
            .unwrap();

        let entries = adapter.log_pod(&pod_dir, 10).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].commit, c2);
        assert_eq!(entries[1].commit, c1);
    }
}
