//! Filesystem path helpers for per-userpod storage.
//!
//! Each userpod (1:1 with a user) owns a directory tree under `{data_dir}/userpods/{name}/`
//! containing its pod DB, memory DB, wallet DB, sessions, artifacts, etc.
//! These helpers compute those paths and bootstrap the directory structure.

use std::path::PathBuf;

/// Root directory for userpod artifacts.
pub const USERPODS_DIR: &str = "userpods";

/// Resolve a relative userpod path against the hKask data directory.
///
/// Checks `HKASK_DATA_DIR` env var, falls back to CWD. This ensures
/// userpod databases end up in a predictable location regardless of where
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

/// Get the directory for a specific userpod.
pub fn userpod_dir(name: &str) -> PathBuf {
    PathBuf::from(USERPODS_DIR).join(sanitize_name(name))
}

// ── Database paths ───────────────────────────────────────────────────────────

/// Pod database — HMemStore, EmbeddingStore, Regulation events.
pub fn userpod_pod_db(name: &str) -> PathBuf {
    userpod_dir(name).join("pod.db")
}

/// Memory database — episodic + semantic tool storage via hkask-mcp-memory.
pub fn userpod_memory_db(name: &str) -> PathBuf {
    userpod_dir(name).join("memory.db")
}

/// Style database — corpus embeddings and centroids for style composition.
pub fn userpod_style_db(name: &str) -> PathBuf {
    userpod_dir(name).join("style.db")
}

/// Kanban database — tasks, unjam items, board state for the userpod.
pub fn userpod_kanban_db(name: &str) -> PathBuf {
    userpod_dir(name).join("kanban.db")
}

/// Training database — LoRA adapter training jobs (model, dataset, status).
pub fn userpod_training_db(name: &str) -> PathBuf {
    userpod_dir(name).join("training.db")
}

/// Wallet database — per-userpod rJoule balances, API keys, encumbrances.
pub fn userpod_wallet_db(name: &str) -> PathBuf {
    userpod_dir(name).join("wallet.db")
}

// ── Directory paths ──────────────────────────────────────────────────────────

/// Gallery directory — media server assets (images, video, audio).
pub fn userpod_gallery_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("gallery")
}

/// Documents directory — docproc parsed/extracted documents.
pub fn userpod_documents_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("documents")
}

/// Library directory — research materials, downloaded papers, RSS feeds.
pub fn userpod_library_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("library")
}

/// Sessions directory — MCP session transcripts.
pub fn userpod_sessions_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("sessions")
}

/// Adapters directory — LoRA adapter weight files.
pub fn userpod_adapters_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("adapters")
}

/// Portfolios directory — financial portfolio/watchlist data.
pub fn userpod_portfolios_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("portfolios")
}

/// Artifacts directory — userpod-specific styles, bots, templates, bundles.
pub fn userpod_artifacts_dir(name: &str) -> PathBuf {
    userpod_dir(name).join("artifacts")
}

/// Artifact manifest — per-userpod index of published artifacts.
pub fn userpod_manifest_json(name: &str) -> PathBuf {
    userpod_dir(name).join("manifest.json")
}

// ── Initialization ───────────────────────────────────────────────────────────

/// All subdirectories created by `ensure_userpod_dirs`.
pub const USERPOD_SUBDIRS: &[&str] = &[
    "gallery",
    "documents",
    "library",
    "sessions",
    "adapters",
    "portfolios",
    "artifacts",
];

/// Create the full userpod directory structure on disk.
///
/// Called during userpod onboarding to ensure the userpod's space exists
/// before any databases are deployed. Safe to call multiple times
/// (idempotent — directories already existing are not errors).
///
/// Creates the userpod root directory and all subdirectories listed in
/// `USERPOD_SUBDIRS`.
pub fn ensure_userpod_dirs(name: &str) -> std::io::Result<()> {
    let dir = userpod_dir(name);
    std::fs::create_dir_all(&dir)?;
    for sub in USERPOD_SUBDIRS {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    Ok(())
}

/// Publish an artifact to the userpod's manifest for Curator indexing.
///
/// Called when a userpod produces a shareable artifact (style, bot, gallery
/// item, trained adapter). The CuratorSync reads manifest files to build
/// the cross-userpod artifact index.
pub fn publish_artifact(
    name: &str,
    artifact_type: &str,
    artifact_name: &str,
    content_hash: &str,
) -> std::io::Result<()> {
    let manifest_path = userpod_manifest_json(name);
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

/// Sanitize a userpod name for filesystem use.
///
/// Replaces characters that are problematic in filenames with hyphens.
/// Userpod names can contain spaces but filenames shouldn't.
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
    fn sanitize_userpod_names() {
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
            userpod_pod_db("alice"),
            PathBuf::from("userpods").join("alice").join("pod.db")
        );
        assert_eq!(
            userpod_memory_db("alice"),
            PathBuf::from("userpods").join("alice").join("memory.db")
        );
        assert_eq!(
            userpod_wallet_db("alice"),
            PathBuf::from("userpods").join("alice").join("wallet.db")
        );
    }

    #[test]
    fn dir_paths() {
        assert_eq!(
            userpod_gallery_dir("alice"),
            PathBuf::from("userpods").join("alice").join("gallery")
        );
        assert_eq!(
            userpod_sessions_dir("alice"),
            PathBuf::from("userpods").join("alice").join("sessions")
        );
    }

    #[test]
    fn ensure_dirs_creates_all_subdirs() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        ensure_userpod_dirs("testuserpod").expect("create dirs");

        assert!(userpod_dir("testuserpod").exists());
        for sub in USERPOD_SUBDIRS {
            assert!(
                userpod_dir("testuserpod").join(sub).exists(),
                "missing subdir: {sub}"
            );
        }

        // Idempotent: calling again should not error
        ensure_userpod_dirs("testuserpod").expect("idempotent");

        std::env::set_current_dir(cwd).unwrap();
    }
}
