# Continuation Prompt — zed-kask migration (T0.6: MCP servers phase)

You are continuing the migration of hKask into the zed-kask fork of Zed. Here is everything you need to pick up cleanly.

## What we're doing

Migrating hKask into the `zed-kask` fork of Zed (`Clones/zed-kask`, origin `mdz-axo/zed-kask`, upstream `zed/zed`). hKask is being **fully merged** into zed-kask under a `kask/` namespace. The `mdz-axo/hKask` repo (`Clones/hKask`) will be archived. zed-kask is the single source of truth.

## Key files

- `kask/docs/architecture/zed-host-architecture-plan.md` (564 lines) — the full architecture + migration plan
- `DIVERGENCE.md` (repo root) — the fork's divergence manifest + sync procedure + dependency policy
- `kask/docs/specs/seam-specs.md` (~258 lines) — D1–D10 seam specs with port contracts + AC + T0.6-storage spec
- `kask/scripts/check-hkask-no-zed-deps.sh` — the §13.1 invariant CI gate (tested, passing)

## Governing invariant (§13.1)

**hKask crates NEVER depend on zed-kask crates.** The sole bidirectional seam is `kask/crates/kask_bridge` (D8). Enforced by `kask/scripts/check-hkask-no-zed-deps.sh`.

## Dependency policy (conform to zed)

Do NOT bump zed's workspace deps. The `libsqlite3-sys` conflict: zed pins `0.30.1` (bundled, `SQLITE_ENABLE_LOAD_EXTENSION=1`); no `rusqlite` is compatible. **Resolution:** `StorageDriver` trait in `hkask-types::storage`; `kask_bridge` implements it over `sqlez`. All hKask crates use `StorageDriver` + `EmbeddingPort` port traits, not `rusqlite`/`r2d2`/`sqlite-vec`.

## Current state — 14 workspace members, 12 compiling

| Crate | Status |
|---|---|
| `hkask-types` | ✅ + DTOs (`HMem`, `HMemError`, `SimilarityResult`, `EmbeddingError`) + `EmbeddingPort` trait (7 methods) + `storage` module (`DbValue`, `DbRow`, `StorageDriver`, `query_map`, `query_row`) |
| `hkask-capability` | ✅ |
| `hkask-goal` | ✅ |
| `hkask-keystore` | ✅ |
| `hkask-regulation` | ✅ |
| `hkask-guard` | ✅ (added `llm-guard = "0.2"` to workspace) |
| `hkask-forecast` | ✅ |
| `hkask-templates` | ✅ (full port-ify: `registry_sqlite.rs` rewritten to `StorageDriver`; `registry/manifests/` copied to `kask/registry/`) |
| `hkask-memory` | ✅ (port-ify: `HMemStore` moved to `hkask-memory` over `StorageDriver`; `EmbeddingStore` → `EmbeddingPort` trait; `SemanticMemory` holds `Arc<dyn EmbeddingPort>`; `open()` constructor removed) |
| `hkask-mcp` | ✅ (added `rmcp = "1"`, `tokio-util` to workspace; fixed `jsonschema` 0.28→0.37 API) |
| `hkask-pods` | ✅ (full port-ify: `deployment.rs` rewritten — `PerPodStorage` holds `Arc<dyn StorageDriver>` + `Arc<dyn EmbeddingPort>`; `PodFactory::deploy` accepts driver+embedding; `CuratorSync` uses `driver_factory` closure; `test_stubs.rs` with `StubStorageDriver`/`StubEmbeddingPort`) |
| `hkask-mcp-server` | ✅ (port-ify: `open_database()` returns `Result<Arc<dyn StorageDriver>, McpError>` — bridge needed for file-based opening; `reqwest` needs `json` feature per-crate) |
| `kask_bridge` | stub |
| `kask_panel` | stub |

§13.1 invariant holds. All 12 crates compile together with zero errors (only pre-existing `sql` cfg warnings in hkask-types + 3 dead-code warnings in hkask-pods).

## YOU ARE HERE — MCP server source migration

The 16 MCP server source crates are in `Clones/hKask/mcp-servers/hkask-mcp-*/`. They need to be migrated to `kask/mcp-servers/hkask-mcp-*/`.

### Architecture plan decisions (critical for MCP servers)

From `zed-host-architecture-plan.md` line 68 + lines 175-181:

- **`hkask-inference` → DELETED** (T5.1). Keep only the `InferencePort` *trait* in `hkask-types`. Servers that use `hkask-inference` must be refactored to use `InferencePort` from `hkask-types`.
- **`hkask-services-*` → DELETION CANDIDATES** (T5.7). Functionality absorbed into `kask_bridge`/`KaskCore`. Servers that depend on them need those deps removed/stubbed.
- **`hkask-condenser` → DELETION CANDIDATE** (T5.7).
- **`hkask-communication` → DELETED** (T5.4). `hkask-mcp-communication` → DELETED.
- **`hkask-mcp-filesystem` → DELETED** (decided, §2.4 — zed provides fs tools).
- **`hkask-ledger` → keep-crate** (line 62: "rJoule energy budget + hMem accounting") but NOT yet migrated.
- **`hkask-test-harness` → NOT in keep-list.** Remove from dev-deps.
- **`hkask-bridge-dublincore` → NOT in keep-list.** Remove/stub.
- 12 loaded by default + 2 kept-unloaded (curator, skill) + 2 deleted (communication, filesystem) = 16 original.

### Server survey (from this session)

Run `cargo check` from `Clones/zed-kask` to verify state. The 16 servers:

**READY — only need storage/rusqlite port-ify (migrate first):**

| Server | src files | lines | storage | rusqlite | deps (already-migrated only) |
|---|---|---|---|---|---|
| `hkask-mcp-scenarios` | 5 | 4853 | 0 | 0 | mcp-server, types, forecast — **CLEAN, no port-ify needed** |
| `hkask-mcp-regulation` | 2 | 298 | 1 | 0 | mcp-server, types, storage — uses `RegulationArchive`, `SqliteDriver`, `open_or_repair`, `DatabaseDriver` |
| `hkask-mcp-research` | 23 | 5616 | 0 | 3 | mcp-server, types — uses `rusqlite::Connection`, `Transaction`, `open_database_with_extensions`; has `src/research/db.rs` module with rusqlite |
| `hkask-mcp-codegraph` | 17 | 3603 | 0 | 1 | mcp-server, types — uses `rusqlite` extensively + `sqlite-vec` FFI (`sqlite3_vec_init` in `codegraph/graph/store.rs`) |

**NEED inference removed (use InferencePort from hkask-types instead of hkask-inference):**

| Server | also needs | notes |
|---|---|---|
| `hkask-mcp-skill` | templates ✅ | 2 src files, 348 lines — small |
| `hkask-mcp-memory` | storage, test-harness | 4 src files, 1239 lines |
| `hkask-mcp-training` | capability ✅, memory ✅, storage | 32 src files, 13400 lines — LARGE |
| `hkask-mcp-media` | pods ✅, storage | 18 src files, 6267 lines |
| `hkask-mcp-condenser` | condenser (deleted), memory ✅, storage | 2 src files, 612 lines — needs rewrite (condenser deleted) |
| `hkask-mcp-docproc` | bridge-dublincore, memory ✅, storage, guard ✅, services-core (deleted) | 40 src files, 12619 lines — VERY LARGE, needs significant rewrite |

**NEED services-* removed:**

| Server | also needs | notes |
|---|---|---|
| `hkask-mcp-curator` | storage, memory ✅, services-context (deleted), capability ✅ | 3 src files, 646 lines — kept but unloaded |
| `hkask-mcp-kata-kanban` | services-kata-kanban (not migrated), storage, test-harness | 4 src files, 1501 lines |
| `hkask-mcp-replica` | many services (deleted), inference (deleted), storage, keystore ✅, test-harness | 4 src files, 1389 lines — needs major rewrite |

**NEED other unmigrated deps:**

| Server | dep | notes |
|---|---|---|
| `hkask-mcp-companies` | `hkask-ledger` (keep-crate, NOT yet migrated) | 24 src files, 13592 lines — LARGE. Also needs storage port-ify + forecast ✅ |

**SKIP (being deleted per architecture plan):**
- `hkask-mcp-communication` — deps: `hkask-communication` (deleted)
- `hkask-mcp-filesystem` — decided deleted (zed provides fs tools)

### Specific port-ify patterns observed

**`hkask-mcp-regulation`** (lib.rs lines 24-25, 226, 249):
```rust
use hkask_storage::RegulationArchive;
use hkask_storage::database::sqlite::SqliteDriver;
let db = match hkask_storage::open_or_repair(&db_path, &passphrase) { ... };
let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = Arc::new(SqliteDriver::new(pool));
```
→ Replace with `StorageDriver` port. `RegulationArchive` may need to be moved or stubbed.

**`hkask-mcp-research`** (lib.rs line 19, db.rs):
```rust
use rusqlite::Connection;
let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Deferred);
tx.execute("INSERT INTO subscriptions ...", rusqlite::params![...]);
```
Also calls `context.open_database_with_extensions("HKASK_RSS_DB", db::RSS_SCHEMA_DDL)` which now returns error (mcp-server port-ify).
→ Replace rusqlite with `StorageDriver::query`/`execute` + `DbValue`. Transactions via `execute_batch("BEGIN")` + `commit_tx()`.

**`hkask-mcp-codegraph`** (codegraph/graph/store.rs):
```rust
use rusqlite::{Connection, params};
fn init_sqlite_vec_on(conn: &Connection) -> rusqlite::Result<()> {
    // FFI: sqlite3_vec_init with raw sqlite3 pointer
}
```
Uses `sqlite-vec` for code graph vector search. **Decision needed:** pure-Rust cosine similarity (like EmbeddingPort) or stub the vector search. The `init_sqlite_vec_on` function uses `rusqlite::ffi` raw pointer access — can't be ported to `StorageDriver` without raw handle access.

### Migration pattern (for each server)

1. Copy from `Clones/hKask/mcp-servers/hkask-mcp-<name>` to `kask/mcp-servers/hkask-mcp-<name>`.
2. Adapt `Cargo.toml`: pin `version = "0.31.0"`, `edition = "2024"`, `license = "MIT"`, `publish = false`. Fix path deps to `../../crates/hkask-*` (relative from `kask/mcp-servers/`). Keep `.workspace = true` for shared deps. Strip `hkask-storage`/`rusqlite`/`r2d2`/`sqlx`/`hkask-inference`/`hkask-services-*`/`hkask-test-harness`/`hkask-condenser`/`hkask-bridge-dublincore`/`hkask-communication` deps. Add `hkask-types` storage/embedding port imports if needed. Check `schemars` — may need to add to workspace or pin inline.
3. If the server imports `hkask_storage`, replace with `hkask_types::storage::*` (port-ify).
4. If the server uses `rusqlite` directly, replace with `StorageDriver::query`/`execute` + `DbValue`/`DbRow`.
5. If the server uses `hkask_inference`, replace with `hkask_types::InferencePort` (the trait). Move any model constants needed to `hkask-types` or inline them.
6. If the server uses `hkask_services_*` or `hkask_condenser` or `hkask_bridge_dublincore`, stub/remove those imports with TODO comments — the functionality moves to `kask_bridge`/`KaskCore`.
7. If the server uses `hkask_test_harness` in dev-deps, strip it. Tests that need it should use `hkask-pods::test_stubs` or be disabled for now.
8. Add to workspace members + `[workspace.dependencies]` in root `Cargo.toml`.
9. `cargo check -p hkask-mcp-<name>` + `bash kask/scripts/check-hkask-no-zed-deps.sh`.

### Suggested migration order

1. **`hkask-mcp-scenarios`** — clean, no port-ify needed. Just copy + adapt Cargo.toml paths. ⬅️ START HERE
2. **`hkask-mcp-regulation`** — small (298 lines), port-ify `RegulationArchive`/`SqliteDriver` → `StorageDriver`.
3. **`hkask-mcp-skill`** — small (348 lines), remove `hkask-inference` dep, use `InferencePort`.
4. **`hkask-mcp-research`** — medium (5616 lines), port-ify rusqlite → `StorageDriver`.
5. **`hkask-mcp-codegraph`** — medium (3603 lines), port-ify rusqlite + decide on sqlite-vec (pure-Rust cosine or stub).
6. **`hkask-mcp-memory`** — small (1239 lines), port-ify storage + remove test-harness.
7. **`hkask-mcp-condenser`** — small (612 lines), needs rewrite (condenser deleted).
8. **`hkask-mcp-curator`** — small (646 lines), kept but unloaded. Remove services-context dep.
9. **`hkask-mcp-kata-kanban`** — medium (1501 lines), remove services-kata-kanban + test-harness.
10. **`hkask-mcp-media`** — medium (6267 lines), remove inference + port-ify storage.
11. **`hkask-mcp-training`** — large (13400 lines), remove inference + port-ify storage.
12. **`hkask-mcp-companies`** — large (13592 lines), needs `hkask-ledger` migrated first (or stubbed).
13. **`hkask-mcp-replica`** — medium (1389 lines), needs major rewrite (many deleted deps).
14. **`hkask-mcp-docproc`** — very large (12619 lines), needs significant rewrite.

### After MCP servers

15. **`hkask-mcp-cloud-gateway`** — in `Clones/hKask/crates/` (not `mcp-servers/`). 4 src files, clean (no storage refs). deps: mcp-server, types, capability.
16. **Update `BUILTIN_SERVERS`** in `hkask-mcp-server/src/lib.rs` — remove `communication` + `filesystem` entries (deleted per architecture plan).
17. **Skills registry** → `kask/skills/` (`manifest.yaml` + `*.j2`). Copy from `Clones/hKask/registry/` (already partially copied to `kask/registry/` for templates).
18. **Archive `mdz-axo/hKask`**.

### Key decisions already made (this session + prior)

- **Full merge** into `kask/` namespace; hKask repo archived. No backward compatibility.
- **Conform to zed deps** — don't bump zed's versions. Add new deps (like `rmcp`, `llm-guard`, `minijinja`, `serde_yaml_neo`) to workspace `[workspace.dependencies]`.
- **`StorageDriver` port** in `hkask-types::storage`; `kask_bridge` implements over `sqlez`.
- **`EmbeddingPort` port** in `hkask-types::ports::embedding_port`; `kask_bridge` implements with **pure-Rust brute-force cosine similarity** (NOT sqlite-vec — personal agent scale, sub-ms for 10K × 1024-dim embeddings; no C deps, no global side effects, trivially debuggable). Escape hatch: pure-Rust HNSW (`hnsw_rs`) if scale ever demands it.
- **`HMemStore`** is a concrete struct (not a trait) — already provider-agnostic, moved to `hkask-memory` using `StorageDriver`.
- **`MemoryPort`** (D6): `ingest_thread` + `recall_semantic` — bridge implements over in-process memory handles.
- **`CuratorSync`** uses `driver_factory` closure (Arc<dyn Fn(&Path) -> Result<Arc<dyn StorageDriver>, String>) — bridge provides real impl.
- **`PodFactory::deploy`** accepts `driver: Arc<dyn StorageDriver>` + `embedding: Arc<dyn EmbeddingPort>` as params — composition root moves to `KaskCore`/`kask_bridge`.
- **`test_stubs`** module in `hkask-pods` — `StubStorageDriver` + `StubEmbeddingPort` for test harnesses.
- **12 MCP servers loaded by default** (`communication` + `filesystem` deleted; `curator` + `skill` kept but unloaded).
- **kask panel** = native GPUI (reuses `ui::prelude::*`; no ratatui).
- **`KaskSettings`** = new `"kask": {...}` settings.json section + kask credentials namespace.
- **App-identity**: `APP_NAME = "Zed-Kask"`, distinct single-instance port, `.zed-kask_server`, binary `zed-kask`. Keep `*.zed.dev` account.

### Workspace deps added this session

Added to `[workspace.dependencies]` in root `Cargo.toml`:
- `llm-guard = "0.2"`
- `minijinja = { version = "2", features = ["loader", "serde", "json", "internal_safe_search"] }`
- `serde_yaml_neo = "0.11"`
- `rmcp = "1"`
- `tokio-util = { version = "0.7", features = ["rt"] }`

`schemars` IS in zed workspace (v1.0, features ["indexmap2"]) — MCP servers can use `.workspace = true`.

### What to do right now

1. Start with `hkask-mcp-scenarios` (clean, no port-ify needed — just copy + fix Cargo.toml paths).
2. Then `hkask-mcp-regulation` (small, port-ify `RegulationArchive` → `StorageDriver`).
3. Then `hkask-mcp-skill` (small, remove `hkask-inference` → `InferencePort`).
4. Continue through the suggested order above.
5. After all servers: update `BUILTIN_SERVERS`, copy skills registry, archive hKask.

---

The prompt is also saved at `Clones/zed-kask/kask/docs/plans/continuation-prompt.md` for reference. Paste it into a fresh Zed agent session to continue from exactly where we left off — 4 ready-to-migrate MCP servers away from the next checkpoint, then 10 more needing dep removal/refactoring.
