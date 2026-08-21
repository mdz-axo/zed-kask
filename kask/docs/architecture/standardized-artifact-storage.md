# Standardized Artifact Storage

> **Status:** Required. All persistent artifacts produced or consumed by
> zed-kask's built-in MCP servers, user skills, user agent files, and
> archived chat threads MUST conform to this layout.
>
> **Authority:** `kask/crates/hkask-types/src/agent_paths.rs` is the canonical
> path-primitive module. `resolve_data_dir()` and `resolve_under_data_dir()`
> are the only sanctioned resolvers.
>
> **D-seam:** D28 (archived-threads + skills relocation). See `DIVERGENCE.md`
> D28 and D1.
>
> **Related:** [`zed-host-architecture-plan.md`](zed-host-architecture-plan.md)
> §13 (the kask/ vs upstream-Zed invariant).

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

```mermaid
erDiagram
    DATA_ROOT ||--o{ AGENTS : "agents/{name}/"
    DATA_ROOT ||--o{ MCP : "mcp/{server_id}/"
    DATA_ROOT ||--o{ SKILLS : "skills/{name}/"
    DATA_ROOT ||--o{ THREADS : "threads/"
    AGENTS ||--o{ USER_AGENT : "{username}/"
    AGENTS ||--o{ CURATOR : "curator/"
    USER_AGENT ||--|| USER_DB : "{username}.db"
    USER_AGENT ||--|| USER_MEM : "memory.db"
    CURATOR ||--|| CURATOR_DB : "curator.db"
    MCP ||--o{ KATA_KANBAN : "kata-kanban/kanban.db"
    MCP ||--o{ SWARM : "swarm/ledger.db"
    MCP ||--o{ TRAINING : "training/training.db"
    SKILLS ||--o{ SKILL_DIR : "{skill_name}/"
    SKILLS ||--o{ REGISTRY : "registry/"
    THREADS ||--|| THREADS_DB : "threads.db"
```

| Artifact class | Root | Subdir pattern | Naming rule | Programmatic contract |
|---|---|---|---|---|
| MCP servers | `{data_dir}` | `mcp/{server_id}/` | `server_id` matches `BUILT_IN_MCP_SERVERS[].id` (`kask/crates/kask_bridge/src/mcp_servers.rs:53`); files named `{purpose}.db` | `resolve_under_data_dir(Path::new("mcp/{server_id}/{purpose}.db"))` |
| User skills | `{data_dir}` | `skills/{skill_name}/` | `skill_name` sanitized via `sanitize_name()` (`agent_paths.rs:157-187`); files: `SKILL.md`, `*.j2` | `resolve_under_data_dir(Path::new("skills/{skill_name}/"))` |
| User agent files | `{data_dir}` | `agents/{agent_name}/` | `agent_name` via `sanitize_name()`; DB file is `{agent_name}.db` (e.g., `agents/curator/curator.db`); memory DB is `memory.db` | `agent_dir(name)` + `agent_db(name)` (existing, `agent_paths.rs:79-81`, `agent_paths.rs:141-143`) |
| Archived chat threads | `{data_dir}` | `threads/` | files: `threads.db` (SQLite) | `resolve_under_data_dir(Path::new("threads/threads.db"))` |

## 3. Ownership principle

An artifact lives under the class subdir of the entity that owns it.
Ownership is determined by: **which agent or MCP server produces and
consumes this artifact?**

### Agent model

The system has three agent classes:

1. **User agent** — the human user. Provisioned by `provision_agent`.
   Has `agents/{username}/` with `{username}.db` (sovereign DB) and
   `memory.db` (episodic + semantic memory). The user is the sovereign
   party — all kask artifacts ultimately serve the user's agency.

2. **Curator agent** — the system's cybernetic regulator. Hardcoded name
   `"curator"`. Has `agents/curator/curator.db` (memory + regulation +
   escalation). The curator is an in-process agent (`Agent::Curator`,
   D2) that escalates *to the user* rather than acting autonomously.

3. **Replica agents** — static memory built from a corpus of text
   materials using the corpus MCP server. Replicas are *not* provisioned
   agents — they have no `agents/` directory. Their memory DBs are
   opened from agent-provided paths (tool parameters), not from the
   `agents/` tree. If replicas gain a canonical home in the future, they
   would live under `mcp/corpus/replicas/{replica_name}/` (server-scoped,
   not agent-scoped), since the corpus server owns them.

The `agent_db(name)` function produces `{name}.db` — for the user, that's
`{username}.db`; for the curator, that's `curator.db`. The name always
matches the agent, making the DB identifiable at a glance.

### Ownership rules

- If the artifact is owned by an **agent** (user-scoped, identity-bound),
  it lives under `agents/{agent_name}/`.
- If the artifact is owned by an **MCP server** (server-scoped, not
  identity-bound), it lives under `mcp/{server_id}/`.
- If the artifact is owned by the **user** (not server-scoped, not
  agent-scoped — e.g., skills, chat threads), it lives under the flat
  class dir (`skills/`, `threads/`).

Within the owner's subtree, the naming rule is:
- `{purpose}.db` for SQLite databases
- `{purpose}.json` for JSON artifacts
- `{purpose}/` for directories of binary artifacts (e.g., `adapters/`,
  `cache/`, `transactions/`, `sources/`)

The `{purpose}` name must be human-readable and identify what the artifact
is for, not its format.

### Binary artifacts

Binary artifacts (LoRA adapter weights, ingested corpus documents, cached
web content) are owned by the MCP server that produces them. They live
under `mcp/{server_id}/{purpose}/`:

| Binary artifact | Owner | Path |
|---|---|---|
| LoRA adapter weights | training server | `mcp/training/adapters/` |
| Corpus ingested documents | corpus server | `mcp/corpus/sources/` |
| Corpus cache files | corpus server | `mcp/corpus/cache/` |
| Portfolio transaction files | portfolio server | `mcp/portfolio/transactions/` |

### Agent DBs that MCP servers read

Some agent DBs are read by MCP servers (e.g., the curator's `curator.db`
is read by the curator MCP server). These are **agent artifacts** (owned
by the agent), not MCP-server artifacts. They stay under
`agents/{agent_name}/`. The MCP server reads them via an env-var override
(e.g., `HKASK_CURATOR_DB`) injected by `mcp_env()`, not by resolving
under `mcp/{server_id}/`.

## 4. Naming convention

- **Folders:** human-readable, kebab-case, sanitized via `sanitize_name()`
  (`agent_paths.rs:157-187`). An operator `ls {data_dir}/` sees the four
  class names: `agents/`, `mcp/`, `skills/`, `threads/`.
- **Files:** `{purpose}.db` for databases, `{artifact}.json` for JSON
  artifacts, `SKILL.md` / `*.j2` for skills. The filename
  identifies the artifact's purpose without reading its contents.
- **No opaque IDs at the browse level:** server IDs, tool names, skill names,
  and agent names are all human-readable strings, not UUIDs.

## 5. Shared-vs-parallel decision per class

| Class | Decision | Rationale |
|---|---|---|
| MCP servers | Parallel within class (`mcp/{server_id}/`) | Each server owns distinct DBs and credentials (`mcp_servers.rs:67-107`); server-ID segment enables browse-by-server. |
| User skills | Shared (flat `skills/{skill_name}/`) | Skills are consumed across servers and the skill tool (`HKASK_SKILLS_DIR` is shared, `settings.rs:946-952`). |
| User agent files | Shared (flat `agents/{agent_name}/`) | Agents are user-scoped, not server-scoped (`agent_paths.rs:79-81`). |
| Archived chat threads | Shared (flat `threads/`) | Threads are user chat history, not server-scoped. |

## 6. Archived threads path

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

## 7. D-seam discipline

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
