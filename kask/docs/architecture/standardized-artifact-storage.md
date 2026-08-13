# Standardized Artifact Storage

> **Status:** Required. All persistent artifacts produced or consumed by
> zed-kask's built-in MCP servers, user skills, user agent files, and
> archived chat threads MUST conform to this layout.
>
> **Authority:** `kask/crates/hkask-types/src/agent_paths.rs` is the canonical
> path-primitive module. `resolve_data_dir()` and `resolve_under_data_dir()`
> are the only sanctioned resolvers.
>
> **D-seam:** D28 (archived-threads relocation). See `DIVERGENCE.md`.

## 1. Root

All kask artifacts live under a single data root, resolved by
`hkask_types::agent_paths::resolve_data_dir()`
(`kask/crates/hkask-types/src/agent_paths.rs:29-46`):

| Precedence | Path (Linux) |
|---|---|
| 1 | `$HKASK_DATA_DIR` |
| 2 | `$XDG_DATA_HOME/hkask` |
| 3 | `$HOME/.local/share/hkask` |
| 4 | current working directory (fallback, warns) |

macOS: `~/Library/Application Support/hkask`. Windows: `%LOCALAPPDATA%\hkask`.

This root is injected as `HKASK_DATA_DIR` into every MCP server child process
by `KaskSettings::mcp_env()`
(`kask/crates/kask_bridge/src/settings.rs:717-736`) so servers resolve paths
consistently regardless of launch context.

## 2. Artifact-class → path mapping

| Artifact class | Root | Subdir pattern | Naming rule | Programmatic contract |
|---|---|---|---|---|
| MCP servers | `{data_dir}` | `mcp/{server_id}/` | `server_id` matches `BUILT_IN_MCP_SERVERS[].id` (`kask/crates/kask_bridge/src/mcp_servers.rs:53`); files named `{purpose}.db` | `resolve_under_data_dir(Path::new("mcp/{server_id}/{purpose}.db"))` |
| User skills | `{data_dir}` | `skills/{skill_name}/` (marketplace skills nest as `skills/_marketplace/{source_user}/{skill_name}/`) | `skill_name` sanitized via `sanitize_name()` (`agent_paths.rs:157-187`); files: `manifest.yaml`, `*.j2`, `SKILL.md` | `resolve_under_data_dir(Path::new("skills/{skill_name}/"))` |
| User agent files | `{data_dir}` | `agents/{agent_name}/` | `agent_name` via `sanitize_name()`; subdirs from `AGENT_SUBDIRS` (`agent_paths.rs:124-132`) | `agent_dir(name)` (existing, `agent_paths.rs:79-81`) |
| Archived chat threads | `{data_dir}` | `threads/` | files: `threads.db` (SQLite) | `resolve_under_data_dir(Path::new("threads/threads.db"))` |

## 3. Naming convention

- **Folders:** human-readable, kebab-case, sanitized via `sanitize_name()`
  (`agent_paths.rs:157-187`). An operator `ls {data_dir}/` sees the four
  class names: `agents/`, `mcp/`, `skills/`, `threads/`.
- **Files:** `{purpose}.db` for databases, `{artifact}.json` for JSON
  artifacts, `manifest.yaml` / `*.j2` / `SKILL.md` for skills. The filename
  identifies the artifact's purpose without reading its contents.
- **No opaque IDs at the browse level:** server IDs, tool names, skill names,
  and agent names are all human-readable strings, not UUIDs.

## 4. Shared-vs-parallel decision per class

| Class | Decision | Rationale |
|---|---|---|
| MCP servers | Parallel within class (`mcp/{server_id}/`) | Each server owns distinct DBs and credentials (`mcp_servers.rs:67-107`); server-ID segment enables browse-by-server. |
| User skills | Shared (flat `skills/{skill_name}/`) | Skills are consumed across servers and the skill tool (`HKASK_SKILLS_DIR` is shared, `settings.rs:946-952`). Marketplace skills nest as `skills/_marketplace/{source_user}/{skill_name}/` — a provenance partition within the class, not a separate class. |
| User agent files | Shared (flat `agents/{agent_name}/`) | Agents are user-scoped, not server-scoped (`agent_paths.rs:79-81`). |
| Archived chat threads | Shared (flat `threads/`) | Threads are user chat history, not server-scoped. |

## 5. Archived threads path

**Upstream path (replaced):** `paths::data_dir().join("threads").join("threads.db")`
→ `~/.local/share/zed-kask/threads/threads.db`
(`crates/agent/src/db.rs`).

**Canonical kask path:** `resolve_under_data_dir(Path::new("threads/threads.db"))`
→ `~/.local/share/hkask/threads/threads.db`.

Pre-release: no back-compat window. The kask path is always used once
`set_threads_db_path_override` is wired (early in `main.rs`, user-independent).
The `None` arm in `ThreadsDatabase::new` is a defensive fallback for the
pre-wiring window only. No content transformation — the on-disk SQLite
format is unchanged; only the path relocates.

## 6. D-seam discipline

Every new or moved path in this layout is pinned by a test asserting the
location is used (per `.rules` zed-kask integration traps). The archived-
threads migration is D-seam D28: an edit to `crates/agent/src/db.rs`
(upstream file) + `crates/agent/src/agent.rs` (upstream file) +
`crates/zed/src/main.rs` (upstream file), carrying `// zed-kask: D28`
comments and pinned by `test_threads_db_override_hook_round_trips`
in `db.rs`. The `crates/agent` crate does NOT depend on `hkask-types` —
the path is passed through a global `Mutex<Option<PathBuf>>` hook
(`set_threads_db_path_override`), preserving the §13.1 invariant that
upstream crates don't depend on kask crates.
