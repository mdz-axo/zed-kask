---
title: "Standardized Artifact Storage"
audience: [developers, architects, operators, agents]
last_updated: 2026-09-24
version: "2.3.0"
status: "Active"
domain: "Lifecycle"
mds_categories: [lifecycle, composition, trust]
---

# Standardized Artifact Storage

> **Status:** Required. All persistent artifacts produced or consumed by
> zed-kask's built-in MCP servers, user skills, user agent files, and
> archived chat threads MUST conform to this layout.
>
> **Authority:** `kask/crates/hkask-types/src/agent_paths.rs` is the canonical
> path-primitive module. `resolve_data_dir()` / `resolve_under_data_dir()`
> and `resolve_artifacts_dir()` / `resolve_under_artifacts_dir()` are the
> only sanctioned resolvers.
>
> **D-seam:** D28 (archived-threads + skills relocation). See `DIVERGENCE.md`
> D28 and D1.
>
> **Related:** [`zed-host-architecture-plan.md`](zed-host-architecture-plan.md)
> §13 (the kask/ vs upstream-Zed invariant).

## 0. The storage requirement (normative)

There are exactly two rooted trees, split by what the artifact IS:

1. **Infrastructure tree (hidden).** `resolve_data_dir()` →
   `~/.local/share/zed-kask/` (Linux). This tree holds ONLY databases and
   machine state — SQLCipher/SQLite DBs, journals, agent state, skills,
   threads. Nothing the user is meant to open, read, or take elsewhere
   goes here. Layout: `mcp/{server_id}/{purpose}.db` via `mcp_server_db`.

2. **Artifacts tree (visible).** `resolve_artifacts_dir()` → the user's
   Documents directory with a `zk-data` subfolder:
   `~/Documents/zk-data/` (Linux/macOS). This tree holds EVERY artifact
   file and output the MCP servers and the system produce for the user —
   reports, screens, transaction files, generated media, corpus cache
   files, exports. Layout: `zk-data/{server}-mcp/{artifact-type}/` via
   `mcp_artifacts_subdir(server_id, artifact_type)` +
   `resolve_under_artifacts_dir`. The `{server}` segment is the server's
   root name (research, companies, portfolio, corpus, media, ...) with an
   `-mcp` suffix; `{artifact-type}` is the human-readable artifact class
   (reports, screens, transactions, generated, cache).

**Producer-named layout (operator ruling 2026-09-24).** Every artifact folder
names what produced it, so the user can tell from the folder name alone which
server, skill, or review wrote a file, what kind of file it is, and which run
it came from:

| Folder | Producer | Route helper |
|---|---|---|
| `{server}-mcp/{artifact-type}/` | an MCP server tool | `mcp_artifacts_subdir` |
| `skills/{skill-name}/{date}-{run}/` | one skill run; carries `manifest.json` (skill, thread, time, inputs, outputs) | `skill_run_dir` |
| `curator/reviews/{date}/` | the algedonic review (operator + Curator) — gemba-walk findings, skill verdicts, action receipts | `curator_review_dir` |
| `curator/proposals/{skill-name}/` | executing skills filing skill-change proposals; only the review accepts or rejects them | `curator_proposals_dir` |
| `agent-traces/` | the agent's on-demand tool tracing | `crates/agent/src/tool_trace.rs` |

Nothing is written at the top level of the tree or in an agent-invented folder.
The split between `curator/proposals/` (written by executing skills) and
`curator/reviews/` (written only by the review) is the storage form of the
separation of skill evaluation from skill execution (repair plan P6).

**Enforcement.** `contain_for_write` confines an MCP server's writes into the
artifacts tree to its own `{server}-mcp/` folder: `run_stdio_server` records
the owner from the binary name via `set_artifact_owner`, and a write to the
tree's top level or another server's folder is rejected. Reads may use the
whole tree (one server's output is another's input). Pinned by
`artifact_writes_are_confined_to_the_owning_server`
(`kask/crates/hkask-mcp-server/src/server/validation.rs`); the route helpers by
`skill_and_curator_routes_name_their_producer` (`agent_paths.rs`). Skill bodies
that write files name their `skills/{name}/` route; skills write there through
the owning MCP tool or `terminal`, because the built-in file tools are confined
to the project.

**Existing folders.** `~/Documents/zk-data/INDEX.md` indexes every folder with
its producer. Corpus runs created before this ruling stay where they are
(operator decision 2026-09-24, option A): their manifests embed absolute paths
under SHA-256 seals, and the sealed v13 reference is the Curator's federated
search source, so moving them would break the seal or the references.
`INDEX-moves-2026-09-24.log` records every move that was made.

The classification test for any new artifact: **would the user ever want
 to open this file, copy it elsewhere, or back it up by hand?** If yes, it
 is an artifact and MUST go under `{server}-mcp/{artifact-type}/` in the
 visible tree. If it is a database or machine-maintained state that only
 the system reads and writes, it is infrastructure and stays in the hidden
 tree. When in doubt, the artifact goes in the visible tree — hiding
 user-facing output is the failure mode this requirement exists to
 prevent.

**Self-healing (the chosen protection posture):** every code path that
resolves an artifact or DB location MUST `create_dir_all` its parent before
writing, so a user accidentally deleting or moving a directory cannot break
the system — the directory is recreated on the next write or server start.
Deletion of an artifact file by the user is always tolerated: servers never
require an artifact file to exist, and they never delete user artifacts
themselves.

**Decision record — artifact protection (2026-08-28).** Three postures were
considered for the visible tree:

1. **Self-healing only** (chosen). Directories and files stay normally
   permissioned; the system recreates missing directories and tolerates
   missing files. Chosen for simplicity and usability: the canonical
   structure plus self-healing is understandable at a glance, and artifacts
   are regenerable outputs — the authoritative state lives in the hidden
   databases.
2. **Write-once file protection** (deferred). `chmod 0444` each artifact
   immediately after write: users can read and copy but not accidentally
   modify. Rejected for now because it does not prevent deletion (the
   directory must stay writable so servers can add new files) and it
   assumes artifacts are never rewritten in place — an assumption that
   must be audited across every writer before it is safe. Revisit only if
   accidental-modification incidents occur; the audit precondition is that
   every artifact writer produces a new file per artifact, never an
   in-place edit.
3. **OS-level snapshots/backup** (operator-side, out of scope for the
   codebase). The only posture that protects against deletion. Operators
   who need deletion protection should run btrfs/ZFS snapshots, Timeshift,
   or a periodic `tar` of `~/Documents/zk-data/`.

Because the MCP servers run as the same user, no filesystem permission
scheme can distinguish server writes from user writes — this is why the
posture decision is a trade-off rather than a hard guarantee.

## 1. Root

All kask internal data (databases, traces, MCP state, skills, threads) lives under a single data root, resolved by
`hkask_types::agent_paths::resolve_data_dir()`
(`kask/crates/hkask-types/src/agent_paths.rs`):

| Precedence | Path (Linux) |
|---|---|
| 1 | `$HKASK_DATA_DIR` |
| 2 | `$XDG_DATA_HOME/zed-kask` |
| 3 | `$HOME/.local/share/zed-kask` |
| 4 | current working directory (fallback, warns) |

macOS: `~/Library/Application Support/zed-kask`. Windows: `%LOCALAPPDATA%\zed-kask`.

User-facing artifacts (reports, screens, exports, transaction files,
corpus cache files) are stored separately in a visible directory via
`resolve_artifacts_dir()`:

| Precedence | Path (Linux) |
|---|---|
| 1 | `$HKASK_ARTIFACTS_DIR` |
| 2 | `$XDG_DOCUMENTS_DIR/zk-data` |
| 3 | `$HOME/Documents/zk-data` |
| 4 | `$HOME/zk-data` (fallback) |

The two roots, the classification rule (databases = hidden infrastructure;
artifact files and outputs = visible `{server}-mcp/` routes), and the
self-healing/protection posture are specified normatively in §0 — this
section only defines the resolution precedence.

`KaskSettings::mcp_env()` emits both `HKASK_DATA_DIR` and
`HKASK_ARTIFACTS_DIR`; `build_mcp_server_env` filters them through each
`BuiltinMcpServer.config_env` allowlist before child launch
(`kask/crates/kask_bridge/src/mcp_servers.rs:28-38,55-547`). Servers therefore
receive only the roots they actually resolve.

## 2. Artifact-class → path mapping

```mermaid
flowchart TD
    R{Artifact classification}
    R -->|database or machine state| D[Hidden data root]
    R -->|user-facing file or export| A[Visible artifacts root]

    D --> AG[agents/{name}/]
    D --> MC[mcp/{server_id}/]
    D --> SK[skills/{name}/]
    D --> TH[threads/threads.db]

    A --> CO[companies-mcp/reports and screens]
    A --> PO[portfolio-mcp/transactions]
    A --> CA[corpus-mcp/cache]
    A --> ME[media-mcp/generated]
    A --> SP[spreadsheet-mcp/workbooks]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-ARTIFACT-001
verified_date: 2026-09-19
verified_against: kask/crates/hkask-types/src/agent_paths.rs:65-75,101-103,110-156,168-218,310-340; kask/crates/kask_bridge/src/mcp_servers.rs:28-38,55-547; kask/crates/hkask-spreadsheet/src/artifact_store.rs:1-27 (spreadsheet-mcp/workbooks visible-artifact root)
status: VERIFIED
-->

| Artifact class | Root | Subdir pattern | Naming rule | Programmatic contract |
|---|---|---|---|---|
| MCP servers | `{data_dir}` | `mcp/{server_id}/` | `server_id` matches `BUILT_IN_MCP_SERVERS[].id` (`kask/crates/kask_bridge/src/mcp_servers.rs:55`); files named `{purpose}.db` | `mcp_server_db(server_id, purpose)` (`agent_paths.rs:167`) or `mcp_server_subdir(server_id, subdir)` (`agent_paths.rs:182`) |
| User skills | `{data_dir}` | `skills/{skill_name}/` | `skill_name` sanitized via `sanitize_name()` (`agent_paths.rs:209-241`); files: `SKILL.md`, `*.j2` | `resolve_under_data_dir(Path::new("skills/{skill_name}/"))` |
| User agent files | `{data_dir}` | `agents/{agent_name}/` | `agent_name` via `sanitize_name()`; DB file is `{agent_name}.db` (e.g., `agents/curator/curator.db`); memory DB is `memory.db` | `agent_dir(name)` (`agent_paths.rs:157`) + `agent_db(name)` (`agent_paths.rs:198`) |
| Archived chat threads | `{data_dir}` | `threads/` | files: `threads.db` (SQLite) | `resolve_under_data_dir(Path::new("threads/threads.db"))` |
| User-facing MCP outputs | `{artifacts_dir}` | `{server}-mcp/{artifact-type}/` | readable purpose names such as `reports`, `transactions`, `cache`, `generated`, `workbooks` | `resolve_under_artifacts_dir(mcp_artifacts_subdir(server_id, artifact_type))` (`agent_paths.rs:154-156,202-218`) |

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

3. **Corpus/style stores** — tool-supplied corpus DB paths are opened by the
   corpus server and are not provisioned agent directories. This document does
   not assign a future canonical location that the current resolvers do not
   enforce.

The `agent_db(name)` function (`agent_paths.rs:198`) produces `{name}.db` — for the user, that's
`{username}.db`; for the curator, that's `curator.db`. The name always
matches the agent, making the DB identifiable at a glance. The function was
renamed from `agent_pod_db` in the 2026-08-27 cleanup (the "pod" concept was
deprecated; the rename is documented in the doc comment at `agent_paths.rs:193-197`).

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

Binary and file artifacts (transaction files, cached corpus text) are owned
by the MCP server that produces them. They are user-facing outputs and live
under the visible artifacts dir at `{server}-mcp/{artifact-type}/`:

| File artifact | Owner | Path |
|---|---|---|
| Portfolio transaction files | portfolio server | `portfolio-mcp/transactions/` (artifacts dir) |
| Corpus cache files | corpus server | `corpus-mcp/cache/` (artifacts dir) |
| Company research reports | companies server | `companies-mcp/reports/` (artifacts dir) |
| Company screens | companies server | `companies-mcp/screens/` (artifacts dir) |
| Generated media | media server | `media-mcp/generated/` (artifacts dir) |

LoRA adapter weights are hosted on HuggingFace (`AdapterSource::HuggingFace`);
only SQLite metadata is local (`mcp/training/training.db`).

### Agent DBs that MCP servers read

Some agent DBs are read by MCP servers (e.g., the curator's `curator.db`
is read by the curator MCP server). These are **agent artifacts** (owned
by the agent), not MCP-server artifacts. They stay under
`agents/{agent_name}/`. The MCP server reads them via an env-var override
(e.g., `HKASK_CURATOR_DB`) injected by `mcp_env()`, not by resolving
under `mcp/{server_id}/`.

## 4. Naming convention

- **Folders:** human-readable, kebab-case, sanitized via `sanitize_name()`
  (`agent_paths.rs:209-241`). An operator `ls {data_dir}/` sees the four
  class names: `agents/`, `mcp/`, `skills/`, `threads/`.
- **Files:** `{purpose}.db` for databases, `{artifact}.json` for JSON
  artifacts, `SKILL.md` / `*.j2` for skills. The filename
  identifies the artifact's purpose without reading its contents.
- **No opaque IDs at the browse level:** server IDs, tool names, skill names,
  and agent names are all human-readable strings, not UUIDs.

## 5. Shared-vs-parallel decision per class

| Class | Decision | Rationale |
|---|---|---|
| MCP servers | Parallel within class (`mcp/{server_id}/`) | Each server owns distinct DBs and credentials — per-entry `credentials`/`config_env` allowlists on `BUILT_IN_MCP_SERVERS` (`mcp_servers.rs:55-431`); server-ID segment enables browse-by-server. |
| User skills | Shared (flat `skills/{skill_name}/`) | Skills are user-owned, not server-scoped. The skill tool resolves them through the D28 `GLOBAL_SKILLS_DIR_OVERRIDE` hook (`crates/agent_skills/agent_skills.rs:962-972`). |
| User agent files | Shared (flat `agents/{agent_name}/`) | Agents are user-scoped, not server-scoped (`agent_paths.rs:157`). |
| Archived chat threads | Shared (flat `threads/`) | Threads are user chat history, not server-scoped. |

## 6. Archived threads path

**Upstream path (replaced):** `paths::data_dir().join("threads").join("threads.db")`
→ `~/.local/share/zed-kask/threads/threads.db`
(`crates/agent/src/db.rs`).

**Canonical kask path:** `resolve_under_data_dir(Path::new("threads/threads.db"))`
→ `~/.local/share/zed-kask/threads/threads.db`.

Pre-release: no back-compat window. The kask path is always used once
`set_threads_db_path_override` is wired (early in `main.rs`, user-independent).
The `None` arm in `ThreadsDatabase::new` is a defensive fallback for the
pre-wiring window only. No content transformation — the on-disk SQLite
format is unchanged; only the path relocates.

## 7. D-seam discipline

Every new or moved path in this layout is pinned by a test asserting the
location is used (per `.rules` zed-kask integration traps). The layout
helpers are pinned in `hkask-types` itself:
`agent_db_follows_agents_class_layout`,
`mcp_server_db_follows_mcp_class_layout`,
`mcp_server_subdir_handles_empty_and_nested`, and
`all_layout_helpers_resolve_under_one_root`
(`kask/crates/hkask-types/src/agent_paths.rs:247-313`). The archived-
threads migration is D-seam D28: an edit to `crates/agent/src/db.rs`
(upstream file) + `crates/agent/src/agent.rs` (upstream file) +
`crates/zed/src/main.rs` (upstream file), carrying `// zed-kask: D28`
comments and pinned by `test_threads_db_override_hook_round_trips`
(`crates/agent/src/db.rs:1270`), with the override wired at
`crates/zed/src/main.rs:676-678`. The `crates/agent` crate does NOT depend
on `hkask-types` — the path is passed through a global
`Mutex<Option<PathBuf>>` hook (`set_threads_db_path_override`,
`crates/agent/src/agent.rs:2985`), preserving the §13.1 invariant that
upstream crates don't depend on kask crates.
