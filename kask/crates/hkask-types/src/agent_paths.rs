//! Filesystem path helpers for per-agent storage.
//!
//! The system has three agent classes:
//! - **User agent** — the human user. Provisioned by `provision_agent`.
//!   Has `agents/{username}/{username}.db` (sovereign DB) + `memory.db`.
//! - **Curator agent** — the system regulator. Has `agents/curator/curator.db`.
//! - **Replica agents** — static memory built from a corpus. Not provisioned;
//!   their DBs are opened from agent-provided paths, not from `agents/`.
//!
//! These helpers compute agent paths and bootstrap the directory structure.
//!
//! # Standardized Artifact Storage
//!
//! All persistent kask artifacts live under four class subdirs of
//! `resolve_data_dir()` (see `kask/docs/architecture/standardized-artifact-storage.md`):
//!
//! - `agents/`  — per-agent files (sovereign DB, memory DB)
//! - `mcp/`     — per-MCP-server artifacts (`mcp/{server_id}/{purpose}.db`)
//! - `skills/`  — user skills (`skills/{skill_name}/`)
//! - `threads/` — archived chat threads (`threads/threads.db`)

use std::path::PathBuf;

/// Root directory for agent artifacts.
pub(crate) const AGENTS_DIR: &str = "agents";

/// Root directory for MCP server artifacts (D28 — Standardized Artifact Storage).
/// Each server owns a subtree: `mcp/{server_id}/{purpose}.db`.
pub const MCP_DIR: &str = "mcp";

/// Root directory for user skills (D28 — Standardized Artifact Storage).
/// Each skill owns a subtree: `skills/{skill_name}/`.
/// Marketplace skills nest as `skills/_marketplace/{source_user}/{skill_name}/`.
pub const SKILLS_DIR: &str = "skills";

/// Default filename for the primary hKask database.
///
/// Resolved relative to `resolve_data_dir()` unless overridden via `HKASK_DB_PATH`.
pub const DEFAULT_DB_PATH: &str = "hkask.db";

/// Resolve the hKask data directory.
///
/// Order of precedence:
/// 1. `HKASK_DATA_DIR` environment variable (honored only when absolute or
///    `.`-prefixed — a relative value is treated as misconfig and falls
///    through, so agent DBs don't silently land in an arbitrary CWD)
/// 2. `$XDG_DATA_HOME/hkask`
/// 3. `$HOME/.local/share/hkask`
/// 4. Current working directory (fallback, with a `warn!`)
///
/// All relative database paths in `ServiceConfig` are resolved against
/// this directory, ensuring agent databases stay in a predictable location
/// regardless of where `kask` is invoked from. This is the single regulator for
/// the data-dir rule — `resolve_under_data_dir` delegates here so the two
/// helpers cannot diverge (F4: they previously did, splitting agent DBs across
/// two trees on a relative `HKASK_DATA_DIR`).
#[must_use]
pub fn resolve_data_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HKASK_DATA_DIR") {
        let path = std::path::PathBuf::from(&dir);
        if path.is_absolute() || path.starts_with(".") {
            return path;
        }
        // A relative `HKASK_DATA_DIR` is almost certainly a misconfig — agent
        // DBs would land in whatever CWD the process happened to start from.
        // Fall through to XDG/HOME rather than honoring it.
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
    tracing::warn!(
        target: "hkask.paths",
        "No data directory resolved (HKASK_DATA_DIR, XDG_DATA_HOME, HOME all unset) — \
         falling back to CWD. Agent databases may be created in \
         an unpredictable location across restarts. Set HKASK_DATA_DIR or HOME."
    );
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Resolve a relative agent path against the hKask data directory.
///
/// Delegates to `resolve_data_dir()` so the `HKASK_DATA_DIR` / XDG / HOME
/// fallback chain has exactly one regulator. Previously this duplicated the
/// chain but honored a relative `HKASK_DATA_DIR` unconditionally while
/// `resolve_data_dir` rejected it — the divergence could split agent DBs across
/// two trees (F4). Now both helpers apply the same rule.
#[must_use]
pub fn resolve_under_data_dir(relative: &std::path::Path) -> std::path::PathBuf {
    resolve_data_dir().join(relative)
}

/// Get the directory for a specific agent.
pub fn agent_dir(name: &str) -> PathBuf {
    PathBuf::from(AGENTS_DIR).join(sanitize_name(name))
}

// ── MCP server paths (D28 — Standardized Artifact Storage) ───────────────────

/// Relative path for a per-server MCP artifact.
///
/// Returns `mcp/{server_id}/{purpose}.db` — the caller resolves this against
/// the data dir via `resolve_under_data_dir`.
pub fn mcp_server_db(server_id: &str, purpose: &str) -> PathBuf {
    PathBuf::from(MCP_DIR)
        .join(sanitize_name(server_id))
        .join(format!("{purpose}.db"))
}

// ── Database paths ───────────────────────────────────────────────────────────

/// Agent sovereign database — HMemStore, EmbeddingStore, Regulation events.
///
/// Renamed from `agent_pod_db` (the "pod" concept was deprecated; the name
/// is anachronistic). The on-disk filename is `{agent_name}.db` (e.g.
/// `agents/curator/curator.db`), not `pod.db`.
pub fn agent_db(name: &str) -> PathBuf {
    let sanitized = sanitize_name(name);
    agent_dir(name).join(format!("{sanitized}.db"))
}

/// Memory database — episodic + semantic tool storage.
pub fn agent_memory_db(name: &str) -> PathBuf {
    agent_dir(name).join("memory.db")
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
