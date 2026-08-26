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
//! All persistent kask artifacts live under class subdirs of either
//! `resolve_data_dir()` (internal app data) or `resolve_artifacts_dir()`
//! (user-facing artifacts), per `kask/docs/architecture/standardized-artifact-storage.md`:
//!
//! Internal data dir (`~/.local/share/zed-kask/`):
//! - `agents/`  — per-agent files (sovereign DB, memory DB)
//! - `mcp/`     — per-MCP-server artifacts (`mcp/{server_id}/{purpose}.db`)
//! - `skills/`  — user skills (`skills/{skill_name}/`)
//! - `threads/` — archived chat threads (`threads/threads.db`)
//!
//! Artifacts dir (`~/Documents/zk-data/`):
//! - `companies-mcp/reports/` — company research reports
//! - `companies-mcp/screens/` — company screens

use std::path::PathBuf;

/// Root directory for agent artifacts.
pub(crate) const AGENTS_DIR: &str = "agents";

/// Root directory for MCP server artifacts (D28 — Standardized Artifact Storage).
/// Each server owns a subtree: `mcp/{server_id}/{purpose}.db`.
pub const MCP_DIR: &str = "mcp";

/// Root directory for user skills (D28 — Standardized Artifact Storage).
/// Each skill owns a subtree: `skills/{skill_name}/`.
pub const SKILLS_DIR: &str = "skills";

/// Default filename for the primary hKask database.
///
/// Resolved relative to `resolve_data_dir()` unless overridden via `HKASK_DB_PATH`.
pub const DEFAULT_DB_PATH: &str = "hkask.db";

/// Resolve the zed-kask data directory (internal app data).
///
/// Order of precedence:
/// 1. `HKASK_DATA_DIR` environment variable (honored only when absolute or
///    `.`-prefixed — a relative value is treated as misconfig and falls
///    through, so agent DBs don't silently land in an arbitrary CWD)
/// 2. `$XDG_DATA_HOME/zed-kask`
/// 3. `$HOME/.local/share/zed-kask`
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
        return std::path::PathBuf::from(xdg).join("zed-kask");
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("zed-kask");
    }
    tracing::warn!(
        target: "hkask.paths",
        "No data directory resolved (HKASK_DATA_DIR, XDG_DATA_HOME, HOME all unset) — \
         falling back to CWD. Agent databases may be created in \
         an unpredictable location across restarts. Set HKASK_DATA_DIR or HOME."
    );
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Resolve a relative agent path against the zed-kask data directory.
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

/// Root directory name for user-facing artifacts.
const ARTIFACTS_DIR_NAME: &str = "zk-data";

/// Resolve the zed-kask artifacts directory (user-facing output).
///
/// This is separate from `resolve_data_dir()` (internal app data) because
/// user-facing artifacts like reports and exports should live in a visible,
/// intuitive location — not buried in a hidden XDG cache directory.
///
/// Order of precedence:
/// 1. `HKASK_ARTIFACTS_DIR` environment variable (honored only when absolute
///    or `.`-prefixed — a relative value is treated as misconfig and falls
///    through)
/// 2. `$XDG_DOCUMENTS_DIR/zk-data`
/// 3. `$HOME/Documents/zk-data`
/// 4. `$HOME/zk-data` (fallback)
#[must_use]
pub fn resolve_artifacts_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HKASK_ARTIFACTS_DIR") {
        let path = std::path::PathBuf::from(&dir);
        if path.is_absolute() || path.starts_with(".") {
            return path;
        }
    }
    // XDG documents dir (respects XDG_DOCUMENTS_DIR or falls back to
    // $HOME/Documents on most Linux desktops).
    if let Some(docs) = dirs::document_dir() {
        return docs.join(ARTIFACTS_DIR_NAME);
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join("Documents")
            .join(ARTIFACTS_DIR_NAME);
    }
    tracing::warn!(
        target: "hkask.paths",
        "No artifacts directory resolved (HKASK_ARTIFACTS_DIR, XDG_DOCUMENTS_DIR, HOME all unset) — \
         falling back to CWD. User-facing artifacts may be created in an unpredictable location."
    );
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(ARTIFACTS_DIR_NAME)
}

/// Resolve a relative artifact path against the zed-kask artifacts directory.
///
/// Use this for user-facing outputs (reports, screens, exports) that should
/// be visible to the user, not for internal app data (DBs, traces, MCP state).
#[must_use]
pub fn resolve_under_artifacts_dir(relative: &std::path::Path) -> std::path::PathBuf {
    resolve_artifacts_dir().join(relative)
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

/// Returns the relative path `mcp/{server_id}/{subdir}` for a server's
/// non-DB artifact directory (e.g. `mcp/portfolio/transactions`,
/// `mcp/swarm/agents/curated`, `mcp/companies/screens`). The caller
/// resolves this against the data dir via `resolve_under_data_dir`.
///
/// This is the directory equivalent of [`mcp_server_db`] — use it for any
/// server-owned artifact that is not a `.db` file. Centralizing the path
/// construction here means a layout change (e.g. renaming `mcp/` to
/// `servers/`) touches one helper, not 10+ call sites.
pub fn mcp_server_subdir(server_id: &str, subdir: &str) -> PathBuf {
    let base = PathBuf::from(MCP_DIR).join(sanitize_name(server_id));
    if subdir.is_empty() {
        base
    } else {
        base.join(subdir)
    }
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

    // D28 — Standardized Artifact Storage pins. The layout contract:
    // every persistent kask artifact resolves under one rooted data tree
    // with class subdirs (agents/, mcp/, skills/, threads/). If a helper
    // here changes shape, these tests fail until the D28 doc and every
    // consumer move together.

    #[test]
    fn agent_db_follows_agents_class_layout() {
        assert_eq!(
            agent_db("curator"),
            PathBuf::from("agents/curator/curator.db")
        );
    }

    #[test]
    fn mcp_server_db_follows_mcp_class_layout() {
        assert_eq!(
            mcp_server_db("kata-kanban", "kanban"),
            PathBuf::from("mcp/kata-kanban/kanban.db")
        );
        assert_eq!(
            mcp_server_db("swarm", "ledger"),
            PathBuf::from("mcp/swarm/ledger.db")
        );
    }

    #[test]
    fn mcp_server_subdir_handles_empty_and_nested() {
        assert_eq!(
            mcp_server_subdir("portfolio", "transactions"),
            PathBuf::from("mcp/portfolio/transactions")
        );
        assert_eq!(mcp_server_subdir("swarm", ""), PathBuf::from("mcp/swarm"));
    }

    #[test]
    fn all_layout_helpers_resolve_under_one_root() {
        // Every class-subdir helper must compose with resolve_under_data_dir
        // without escaping the root. Uses a relative probe path so the join
        // is observable regardless of what the env resolves to.
        for relative in [
            agent_db("curator"),
            mcp_server_db("swarm", "ledger"),
            mcp_server_subdir("portfolio", "transactions"),
        ] {
            let resolved = resolve_under_data_dir(&relative);
            assert!(resolved.starts_with(resolve_data_dir()));
        }
    }

    #[test]
    fn sanitize_name_replaces_filesystem_hostile_characters() {
        assert_eq!(sanitize_name("Jacques (Zuck)"), "Jacques-Zuck");
        assert_eq!(
            sanitize_name("a/b\\c:d*e?f\"g<h>i|j"),
            "a-b-c-d-e-f-g-h-i-j"
        );
    }

    #[test]
    fn sanitize_name_blocks_path_traversal() {
        assert_eq!(sanitize_name("."), "unnamed");
        assert_eq!(sanitize_name(".."), "unnamed");
        // A name of only hostile characters collapses to empty → must not
        // become an empty filename or a traversal vector.
        assert_eq!(sanitize_name("///"), "");
        assert_ne!(sanitize_name(".."), "..");
    }
}
