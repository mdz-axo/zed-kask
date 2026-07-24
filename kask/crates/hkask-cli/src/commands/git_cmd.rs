//! Git command handlers for `kask git`
//!
//! Implements the CLI display logic for registry git archival operations
//! and local CAS operations (verify, diff, log, snapshot).

use std::sync::Arc;

use crate::archival::ArchivalService;
use crate::cli::GitAction;
use hkask_git_cas::GixCasAdapter;
use hkask_types::git_cas::{GitCASPort, RepoId, TreeEntryKind};

/// Resolve the concrete `GixCasAdapter` for admin operations
/// (diff, resolve_ref) that are not part of the GitCASPort contract.
fn resolve_gix_adapter() -> GixCasAdapter {
    super::helpers::or_exit(
        GixCasAdapter::from_env(),
        "Failed to initialize CAS adapter",
    )
}

/// Resolve the hexagonal `GitCASPort` from the environment.
fn resolve_git_cas_port() -> Arc<dyn GitCASPort> {
    let adapter = resolve_gix_adapter();
    Arc::new(adapter) as Arc<dyn GitCASPort>
}

/// Parse a RepoId from a string, returning Registry as default.
fn parse_repo_id(repo: &str) -> RepoId {
    match repo {
        "registry" | "" => RepoId::Registry,
        "memory" => RepoId::Memory,
        "reg-audit" => RepoId::RegAudit,
        "sovereignty" => RepoId::Sovereignty,
        "goals-specs" => RepoId::GoalsSpecs,
        "sessions" => RepoId::Sessions,
        "vault" => RepoId::Vault,
        "pods" => RepoId::Pods,
        _ => {
            eprintln!("Unknown repo '{}', defaulting to 'registry'", repo);
            RepoId::Registry
        }
    }
}

/// pre:  rt is a valid tokio Runtime; action is a valid GitAction variant
/// post: dispatches to archive, restore, list, snapshot, or CAS operations (verify, diff, log, snapshot, restore)
pub fn run(rt: &tokio::runtime::Runtime, action: GitAction) {
    match action {
        // ── GitHub API operations (existing) ──────────────────────────────
        GitAction::Archive {
            owner,
            repo,
            branch,
            path,
            content,
            file,
        } => {
            let content_str = if let Some(c) = content {
                c
            } else if let Some(f) = file {
                super::helpers::or_exit(std::fs::read_to_string(&f), "Failed to read file")
            } else {
                eprintln!("Either --content or --file must be provided");
                std::process::exit(1);
            };
            let result = block_on!(
                rt,
                ArchivalService::archive_to_git(&owner, &repo, &branch, &path, &content_str,),
                "Archive failed"
            );
            println!("Archived to {} (commit {})", result.path, result.commit_sha);
        }

        GitAction::Restore {
            owner,
            repo,
            r#ref,
            target,
        } => {
            let content = block_on!(
                rt,
                ArchivalService::restore_from_git(&owner, &repo, &r#ref, &target,),
                "Restore failed"
            );
            println!("{}", content);
        }

        GitAction::List { owner, repo } => {
            let commits = block_on!(
                rt,
                ArchivalService::list_archives(&owner, &repo,),
                "List failed"
            );
            println!("Archived versions for {}/{}:", owner, repo);
            for (i, sha) in commits.iter().enumerate() {
                println!("  {}. {}", i + 1, sha);
            }
        }

        GitAction::Snapshot {
            owner,
            repo,
            message,
        } => {
            let result = block_on!(
                rt,
                ArchivalService::create_snapshot(&owner, &repo, &message),
                "Snapshot failed"
            );
            println!("Snapshot created (commit {})", result.commit_sha);
        }

        // ── Local CAS operations (Phase 5) ──────────────────────────────
        GitAction::CasVerify { repo } => {
            let port = resolve_git_cas_port();
            let repo_id = parse_repo_id(&repo);
            let report = block_on!(rt, port.verify(&repo_id), "Verify failed");

            println!("Verification report for '{}':", report.repo.dir_name());
            println!("  Total blobs:   {}", report.total_blobs);
            println!("  Verified:      {}", report.verified_blobs);
            if report.corrupt_hashes.is_empty() {
                println!("  Integrity:     ✓ OK");
            } else {
                println!("  Integrity:    ✗ CORRUPT");
                for hash in &report.corrupt_hashes {
                    println!("    Corrupt: {}", hash);
                }
            }
        }

        GitAction::CasDiff { repo, from, to } => {
            let adapter = resolve_gix_adapter();
            let repo_id = parse_repo_id(&repo);
            let diffs = block_on!(rt, adapter.diff(&repo_id, &from, &to), "Diff failed");

            println!("Diff for '{}' ({} → {}):", repo_id.dir_name(), from, to);
            if diffs.is_empty() {
                println!("  No changes.");
            } else {
                for d in &diffs {
                    println!("  {:?} {}", d.kind, d.path);
                }
            }
        }

        GitAction::CasLog { repo, max_count } => {
            let port = resolve_git_cas_port();
            let repo_id = parse_repo_id(&repo);
            let entries = block_on!(rt, port.log(&repo_id, max_count), "Log failed");

            if entries.is_empty() {
                println!("No snapshots found for '{}'.", repo_id.dir_name());
            } else {
                println!("Snapshots for '{}':", repo_id.dir_name());
                for (i, entry) in entries.iter().enumerate() {
                    println!("  {}. {} {}", i + 1, entry.commit, entry.message,);
                }
            }
        }

        GitAction::CasSnapshot { repo, message } => {
            let port = resolve_git_cas_port();
            let repo_id = parse_repo_id(&repo);
            let commit = block_on!(rt, port.snapshot(&repo_id, &message), "Snapshot failed");

            println!("Snapshot created for '{}': {}", repo_id.dir_name(), commit);
        }

        GitAction::CasRestore {
            repo,
            r#ref,
            prefix,
        } => {
            let port = resolve_git_cas_port();
            let repo_id = parse_repo_id(&repo);
            let reference = r#ref.as_deref().unwrap_or("HEAD");
            let prefix_str = prefix.as_deref().unwrap_or("");

            let entries = block_on!(
                rt,
                port.list_tree(&repo_id, reference, prefix_str),
                "List tree failed"
            );

            if entries.is_empty() {
                println!(
                    "No entries found for '{}' at '{}'.",
                    repo_id.dir_name(),
                    reference
                );
            } else {
                println!(
                    "Restoring {} entries from '{}' at '{}':",
                    entries.len(),
                    repo_id.dir_name(),
                    reference
                );
                for entry in &entries {
                    if entry.kind == TreeEntryKind::Blob {
                        let content = block_on!(
                            rt,
                            port.get_blob(&repo_id, &entry.content_hash),
                            "Get blob failed"
                        );
                        println!(
                            "  {} ({} bytes, hash: {})",
                            entry.path,
                            content.len(),
                            entry.content_hash
                        );
                    } else {
                        println!("  {} (tree)", entry.path);
                    }
                }
            }
        }
    }
}
