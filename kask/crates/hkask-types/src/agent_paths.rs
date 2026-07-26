//! Filesystem path helpers for per-agent storage.
//!
//! Each agent (1:1 with a user) owns a directory tree under `{data_dir}/agents/{name}/`
//! containing its pod DB, memory DB, wallet DB, sessions, artifacts, etc.
//! These helpers compute those paths and bootstrap the directory structure.

use std::path::PathBuf;

/// Root directory for agent artifacts.
pub const AGENTS_DIR: &str = "agents";

/// Resolve a relative agent path against the hKask data directory.
///
/// Checks `HKASK_DATA_DIR` env var, falls back to CWD. This ensures
/// agent databases end up in a predictable location regardless of where
/// the MCP server process is spawned from.
#[must_use]
pub fn resolve_under_data_dir(relative: &std::path::Path) -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HKASK_DATA_DIR") {
        return std::path::PathBuf::from(dir).join(relative);
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("hkask").join(relative);
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("hkask")
            .join(relative);
    }
    relative.to_path_buf()
}

/// Get the directory for a specific agent.
pub fn agent_dir(name: &str) -> PathBuf {
    PathBuf::from(AGENTS_DIR).join(sanitize_name(name))
}

// ── Database paths ───────────────────────────────────────────────────────────

/// Pod database — HMemStore, EmbeddingStore, Regulation events.
pub fn agent_pod_db(name: &str) -> PathBuf {
    agent_dir(name).join("pod.db")
}

/// Memory database — episodic + semantic tool storage.
pub fn agent_memory_db(name: &str) -> PathBuf {
    agent_dir(name).join("memory.db")
}

/// Style database — corpus embeddings and centroids for style composition.
#[allow(dead_code)]
pub(crate) fn agent_style_db(name: &str) -> PathBuf {
    agent_dir(name).join("style.db")
}

/// Kanban database — tasks, unjam items, board state for the agent.
pub fn agent_kanban_db(name: &str) -> PathBuf {
    agent_dir(name).join("kanban.db")
}

/// Training database — LoRA adapter training jobs (model, dataset, status).
pub fn agent_training_db(name: &str) -> PathBuf {
    agent_dir(name).join("training.db")
}

/// Wallet database — per-agent rJoule balances, API keys, encumbrances.
#[allow(dead_code)]
pub(crate) fn agent_wallet_db(name: &str) -> PathBuf {
    agent_dir(name).join("wallet.db")
}

// ── Directory paths ──────────────────────────────────────────────────────────

/// Gallery directory — media server assets (images, video, audio).
#[allow(dead_code)]
pub(crate) fn agent_gallery_dir(name: &str) -> PathBuf {
    agent_dir(name).join("gallery")
}

/// Documents directory — docproc parsed/extracted documents.
#[allow(dead_code)]
pub(crate) fn agent_documents_dir(name: &str) -> PathBuf {
    agent_dir(name).join("documents")
}

/// Library directory — research materials, downloaded papers, RSS feeds.
#[allow(dead_code)]
pub(crate) fn agent_library_dir(name: &str) -> PathBuf {
    agent_dir(name).join("library")
}

/// Sessions directory — MCP session transcripts.
#[allow(dead_code)]
pub(crate) fn agent_sessions_dir(name: &str) -> PathBuf {
    agent_dir(name).join("sessions")
}

/// Adapters directory — LoRA adapter weight files.
pub fn agent_adapters_dir(name: &str) -> PathBuf {
    agent_dir(name).join("adapters")
}

/// Portfolios directory — financial portfolio/watchlist data.
#[allow(dead_code)]
pub(crate) fn agent_portfolios_dir(name: &str) -> PathBuf {
    agent_dir(name).join("portfolios")
}

/// Artifacts directory — agent-specific styles, bots, templates, bundles.
#[allow(dead_code)]
pub(crate) fn agent_artifacts_dir(name: &str) -> PathBuf {
    agent_dir(name).join("artifacts")
}

/// Artifact manifest — per-agent index of published artifacts.
#[allow(dead_code)]
pub(crate) fn agent_manifest_json(name: &str) -> PathBuf {
    agent_dir(name).join("manifest.json")
}

// ── Initialization ───────────────────────────────────────────────────────────

/// All subdirectories created by `ensure_agent_dirs`.
pub const AGENT_SUBDIRS: &[&str] = &[
    "gallery",
    "documents",
    "library",
    "sessions",
    "adapters",
    "portfolios",
    "artifacts",
];

/// Create the full agent directory structure on disk.
///
/// Called during agent provisioning to ensure the agent's space exists
/// before any databases are deployed. Safe to call multiple times
/// (idempotent — directories already existing are not errors).
///
/// Creates the agent root directory and all subdirectories listed in
/// `AGENT_SUBDIRS`.
pub fn ensure_agent_dirs(name: &str) -> std::io::Result<()> {
    let dir = agent_dir(name);
    std::fs::create_dir_all(&dir)?;
    for sub in AGENT_SUBDIRS {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    Ok(())
}

/// Publish an artifact to the agent's manifest for Curator indexing.
///
/// Called when an agent produces a shareable artifact (style, bot, gallery
/// item, trained adapter). The CuratorSync reads manifest files to build
/// the cross-agent artifact index.
#[allow(dead_code)]
pub(crate) fn publish_artifact(
    name: &str,
    artifact_type: &str,
    artifact_name: &str,
    content_hash: &str,
) -> std::io::Result<()> {
    let manifest_path = agent_manifest_json(name);
    let entry = serde_json::json!({
        "type": artifact_type,
        "name": artifact_name,
        "hash": content_hash,
        "published_at": chrono::Utc::now().to_rfc3339(),
    });

    // Read existing manifest, append, write back
    let mut manifest: serde_json::Value = if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(serde_json::json!({"artifacts": []}))
    } else {
        serde_json::json!({"artifacts": []})
    };

    if let Some(artifacts) = manifest.get_mut("artifacts").and_then(|a| a.as_array_mut()) {
        // Replace existing entry with same type+name, or append new
        if let Some(existing) = artifacts.iter_mut().find(|a| {
            a.get("type").and_then(|t| t.as_str()) == Some(artifact_type)
                && a.get("name").and_then(|n| n.as_str()) == Some(artifact_name)
        }) {
            *existing = entry;
        } else {
            artifacts.push(entry);
        }
    }

    let json = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| String::from("{}"));
    std::fs::write(&manifest_path, json)
}

/// Sanitize an agent name for filesystem use.
///
/// Replaces characters that are problematic in filenames with hyphens.
/// Agent names can contain spaces but filenames shouldn't.
/// Guards against path traversal: names that sanitize to `.` or `..` are
/// replaced with `unnamed` to prevent directory escape.
pub fn sanitize_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '(' | ')' | ' ' => '-',
            other => other,
        })
        .collect::<String>();
    // Collapse consecutive dashes into one (e.g. "Jacques (Zuck)" → "Jacques-Zuck",
    // not "Jacques--Zuck").
    let mut collapsed = String::with_capacity(sanitized.len());
    let mut prev_dash = false;
    for c in sanitized.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push(c);
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }
    let result = collapsed.trim_matches('-').to_string();
    // Guard against path traversal: `.` and `..` resolve to current/parent dir.
    if result == "." || result == ".." {
        "unnamed".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_agent_names() {
        assert_eq!(sanitize_name("alice"), "alice");
        assert_eq!(sanitize_name("a/b\\c:d"), "a-b-c-d");
    }

    #[test]
    fn sanitize_rejects_path_traversal() {
        assert_eq!(sanitize_name(".."), "unnamed");
        assert_eq!(sanitize_name("."), "unnamed");
    }

    #[test]
    fn db_paths() {
        assert_eq!(
            agent_pod_db("alice"),
            PathBuf::from("agents").join("alice").join("pod.db")
        );
        assert_eq!(
            agent_memory_db("alice"),
            PathBuf::from("agents").join("alice").join("memory.db")
        );
        assert_eq!(
            agent_wallet_db("alice"),
            PathBuf::from("agents").join("alice").join("wallet.db")
        );
    }

    #[test]
    fn dir_paths() {
        assert_eq!(
            agent_gallery_dir("alice"),
            PathBuf::from("agents").join("alice").join("gallery")
        );
        assert_eq!(
            agent_sessions_dir("alice"),
            PathBuf::from("agents").join("alice").join("sessions")
        );
    }

    #[test]
    fn ensure_dirs_creates_all_subdirs() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        ensure_agent_dirs("testagent").expect("create dirs");

        assert!(agent_dir("testagent").exists());
        for sub in AGENT_SUBDIRS {
            assert!(
                agent_dir("testagent").join(sub).exists(),
                "missing subdir: {sub}"
            );
        }

        // Idempotent: calling again should not error
        ensure_agent_dirs("testagent").expect("idempotent");

        std::env::set_current_dir(cwd).unwrap();
    }
}
