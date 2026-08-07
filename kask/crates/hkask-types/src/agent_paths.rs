//! Filesystem path helpers for per-agent storage.
//!
//! Each agent (1:1 with a user) owns a directory tree under `{data_dir}/agents/{name}/`
//! containing its pod DB, memory DB, wallet DB, sessions, artifacts, etc.
//! These helpers compute those paths and bootstrap the directory structure.

use std::path::PathBuf;

/// Root directory for agent artifacts.
pub const AGENTS_DIR: &str = "agents";

/// Default filename for the primary hKask database.
///
/// Resolved relative to `resolve_data_dir()` unless overridden via `HKASK_DB_PATH`.
pub const DEFAULT_DB_PATH: &str = "hkask.db";

/// Resolve the hKask data directory.
///
/// Order of precedence:
/// 1. `HKASK_DATA_DIR` environment variable
/// 2. `$XDG_DATA_HOME/hkask`
/// 3. `$HOME/.local/share/hkask`
/// 4. Current working directory (fallback)
///
/// All relative database paths in `ServiceConfig` are resolved against
/// this directory, ensuring agent databases stay in a predictable location
/// regardless of where `kask` is invoked from.
#[must_use]
pub fn resolve_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HKASK_DATA_DIR") {
        let p = std::path::PathBuf::from(&dir);
        if p.is_absolute() || p.starts_with(".") {
            return p;
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("hkask");
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("hkask");
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

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
    tracing::warn!(
        target: "hkask.paths",
        relative = %relative.display(),
        "No data directory resolved (HKASK_DATA_DIR, XDG_DATA_HOME, HOME all unset) — \
         falling back to CWD-relative path. Agent databases may be created in \
         an unpredictable location across restarts. Set HKASK_DATA_DIR or HOME."
    );
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

/// Kanban database — tasks, unjam items, board state for the agent.
pub fn agent_kanban_db(name: &str) -> PathBuf {
    agent_dir(name).join("kanban.db")
}

/// Training database — LoRA adapter training jobs (model, dataset, status).
pub fn agent_training_db(name: &str) -> PathBuf {
    agent_dir(name).join("training.db")
}

// ── Directory paths ──────────────────────────────────────────────────────────

/// Adapters directory — LoRA adapter weight files.
pub fn agent_adapters_dir(name: &str) -> PathBuf {
    agent_dir(name).join("adapters")
}

// ── Initialization ───────────────────────────────────────────────────────────

/// All subdirectories created by `ensure_agent_dirs` during agent provisioning.
///
/// Only `adapters` has a live accessor (`agent_adapters_dir`, used by
/// `hkask-mcp-training`). The remaining dirs are scaffolding — created on
/// disk as part of the agent directory structure but not yet read/written
/// by any code. They are retained so future features (gallery, documents,
/// library, sessions, portfolios, artifacts) have a pre-created home without
/// needing a migration. Removing a name from this list is safe (the dir is
/// simply not created); adding one requires updating `ensure_agent_dirs`'s
/// test.
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
