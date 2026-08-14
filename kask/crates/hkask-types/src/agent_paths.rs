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
pub const AGENTS_DIR: &str = "agents";

/// Root directory for MCP server artifacts (D28 — Standardized Artifact Storage).
/// Each server owns a subtree: `mcp/{server_id}/{purpose}.db`.
pub const MCP_DIR: &str = "mcp";

/// Root directory for user skills (D28 — Standardized Artifact Storage).
/// Each skill owns a subtree: `skills/{skill_name}/`.
/// Marketplace skills nest as `skills/_marketplace/{source_user}/{skill_name}/`.
pub const SKILLS_DIR: &str = "skills";

/// Root directory for archived chat threads (D28 — Standardized Artifact Storage).
/// Contains `threads.db` (SQLite).
pub const THREADS_DIR: &str = "threads";

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

// ── Skills paths (D28 — Standardized Artifact Storage) ───────────────────────

/// Relative path for the skills root directory.
///
/// Returns `skills/` — the caller resolves this against the data dir via
/// `resolve_under_data_dir`.
pub fn skills_dir() -> PathBuf {
    PathBuf::from(SKILLS_DIR)
}

// ── Threads paths (D28 — Standardized Artifact Storage) ──────────────────────

/// Relative path for the archived threads SQLite DB.
///
/// Returns `threads/threads.db` — the caller resolves this against the data
/// dir via `resolve_under_data_dir`.
pub fn threads_db_path() -> PathBuf {
    PathBuf::from(THREADS_DIR).join("threads.db")
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

// ── Initialization ───────────────────────────────────────────────────────────

/// Create the agent's root directory on disk.
///
/// Called during agent provisioning to ensure the agent's space exists
/// before any databases are deployed. Safe to call multiple times
/// (idempotent — directories already existing are not errors).
///
/// D28: the scaffolding subdirs (`gallery`, `documents`, `library`,
/// `sessions`, `adapters`, `portfolios`, `artifacts`) were removed — they
/// were created on disk but never read/written by any production code.
/// MCP-server artifacts now live under `mcp/{server_id}/`, not under
/// `agents/{name}/`. Agent DBs (`agent_db`, `agent_memory_db`) create
/// their own parent dir on open.
pub fn ensure_agent_dirs(name: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(agent_dir(name))
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
            agent_db("alice"),
            PathBuf::from("agents").join("alice").join("alice.db")
        );
        assert_eq!(
            agent_memory_db("alice"),
            PathBuf::from("agents").join("alice").join("memory.db")
        );
    }

    #[test]
    fn ensure_dirs_creates_agent_root() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        ensure_agent_dirs("testagent").expect("create dirs");

        assert!(agent_dir("testagent").exists());

        // Idempotent: calling again should not error
        ensure_agent_dirs("testagent").expect("idempotent");

        std::env::set_current_dir(cwd).unwrap();
    }

    // D28 — pins the four standardized storage class-dir constants.
    #[test]
    fn storage_layout_has_four_class_dirs() {
        assert_eq!(AGENTS_DIR, "agents");
        assert_eq!(MCP_DIR, "mcp");
        assert_eq!(SKILLS_DIR, "skills");
        assert_eq!(THREADS_DIR, "threads");
        // All four must be distinct.
        let dirs = [AGENTS_DIR, MCP_DIR, SKILLS_DIR, THREADS_DIR];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j], "class dirs must be distinct");
            }
        }
    }

    // D28 — pins the path-constructor helpers.
    #[test]
    fn storage_path_helpers() {
        assert_eq!(
            mcp_server_db("codegraph", "codegraph"),
            PathBuf::from("mcp").join("codegraph").join("codegraph.db")
        );
        assert_eq!(skills_dir(), PathBuf::from("skills"));
        assert_eq!(
            threads_db_path(),
            PathBuf::from("threads").join("threads.db")
        );
    }

    // ── Property-based tests (D28) ──────────────────────────────────────────

    use proptest::prelude::*;
    proptest! {
        /// P4 (panic freedom): `sanitize_name` never panics on any input.
        #[test]
        fn sanitize_name_never_panics(name in ".*") {
            let _ = sanitize_name(&name);
        }

        /// P1 (invariant): `sanitize_name` output is never `.` or `..`.
        #[test]
        fn sanitize_name_never_produces_path_traversal(name in ".*") {
            let result = sanitize_name(&name);
            prop_assert!(
                result != "." && result != "..",
                "sanitize_name produced path traversal: {:?} -> {:?}",
                name, result,
            );
        }

        /// P1 (invariant): `sanitize_name` output contains no path separators.
        #[test]
        fn sanitize_name_never_contains_separators(name in ".*") {
            let result = sanitize_name(&name);
            prop_assert!(
                !result.contains('/') && !result.contains('\\'),
                "sanitize_name produced a path separator: {:?} -> {:?}",
                name, result,
            );
        }

        /// P1 (idempotency): `sanitize_name(sanitize_name(x)) == sanitize_name(x)`.
        #[test]
        fn sanitize_name_is_idempotent(name in ".*") {
            let once = sanitize_name(&name);
            let twice = sanitize_name(&once);
            prop_assert!(once == twice, "not idempotent: {:?} -> {:?} -> {:?}", name, once, twice);
        }

        /// P1 (invariant): `agent_dir(name)` always starts with `AGENTS_DIR`.
        #[test]
        fn agent_dir_always_under_agents_dir(name in ".*") {
            let dir = agent_dir(&name);
            prop_assert!(dir.starts_with(AGENTS_DIR), "not under AGENTS_DIR: {:?} -> {:?}", name, dir);
        }

        /// P1 (invariant): `agent_dir(name)` second component is `sanitize_name(name)`
        /// when the sanitized name is non-empty.
        #[test]
        fn agent_dir_uses_sanitized_name(name in ".*") {
            let sanitized = sanitize_name(&name);
            prop_assume!(!sanitized.is_empty(), "empty sanitized name is an edge case");
            let dir = agent_dir(&name);
            let components: Vec<_> = dir.components().collect();
            prop_assert!(
                components.len() == 2
                    && components[1] == std::path::Component::Normal(std::ffi::OsStr::new(&sanitized)),
                "second component must be sanitized: {:?} -> {:?} (sanitized: {:?})",
                name, dir, sanitized,
            );
        }

        /// P1 (invariant): `mcp_server_db` always starts with `MCP_DIR`.
        #[test]
        fn mcp_server_db_always_under_mcp_dir(
            server_id in "[a-z][a-z0-9-]*",
            purpose in "[a-z][a-z0-9-]*",
        ) {
            let path = mcp_server_db(&server_id, &purpose);
            prop_assert!(path.starts_with(MCP_DIR), "not under MCP_DIR: ({:?}, {:?}) -> {:?}", server_id, purpose, path);
        }

        /// P1 (invariant): `mcp_server_db` has exactly 3 components.
        #[test]
        fn mcp_server_db_has_three_components(
            server_id in "[a-z][a-z0-9-]*",
            purpose in "[a-z][a-z0-9-]*",
        ) {
            let path = mcp_server_db(&server_id, &purpose);
            let components: Vec<_> = path.components().collect();
            prop_assert_eq!(components.len(), 3, "not 3 components: ({:?}, {:?}) -> {:?}", server_id, purpose, path);
        }

        /// P1 (invariant): `mcp_server_db` sanitizes `server_id` — the
        /// output's second component is `sanitize_name(server_id)` when the
        /// sanitized name is non-empty.
        #[test]
        fn mcp_server_db_sanitizes_server_id(
            server_id in ".*",
            purpose in "[a-z][a-z0-9-]*",
        ) {
            let sanitized = sanitize_name(&server_id);
            prop_assume!(!sanitized.is_empty(), "empty sanitized server_id is an edge case");
            let path = mcp_server_db(&server_id, &purpose);
            let components: Vec<_> = path.components().collect();
            prop_assert!(
                components.len() >= 2
                    && components[1] == std::path::Component::Normal(std::ffi::OsStr::new(&sanitized)),
                "server_id not sanitized: ({:?}, {:?}) -> {:?} (sanitized: {:?})",
                server_id, purpose, path, sanitized,
            );
        }

        /// P1 (invariant): `agent_db(name)` filename is `{sanitize_name(name)}.db`
        /// for names that sanitize to non-empty.
        #[test]
        fn agent_db_filename_matches_sanitized_name(name in "[a-z][a-z0-9-]*") {
            let sanitized = sanitize_name(&name);
            prop_assume!(!sanitized.is_empty());
            let path = agent_db(&name);
            let expected = format!("{sanitized}.db");
            prop_assert_eq!(path.file_name(), Some(std::ffi::OsStr::new(&expected)), "filename mismatch: {:?} -> {:?}", name, path);
        }

        /// P1 (invariant): `agent_db(name)` is always under `agent_dir(name)`.
        #[test]
        fn agent_db_under_agent_dir(name in ".*") {
            let db_path = agent_db(&name);
            let dir = agent_dir(&name);
            prop_assert!(db_path.starts_with(&dir), "not under agent_dir: {:?} -> {:?} (dir: {:?})", name, db_path, dir);
        }
    }
}
