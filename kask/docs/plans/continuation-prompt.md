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

## Current state — 16 workspace members, 15 compiling

| Crate | Status |
|---|---|
| `hkask-types` | ✅ + DTOs + `EmbeddingPort` trait + `storage` module (`DbValue`, `DbRow`, `StorageDriver`, `query_map`, `query_row`, `define_driver_store!`) |
| `hkask-capability` | ✅ |
| `hkask-goal` | ✅ |
| `hkask-keystore` | ✅ |
| `hkask-regulation` | ✅ |
| `hkask-guard` | ✅ (added `llm-guard = "0.2"` to workspace) |
| `hkask-forecast` | ✅ |
| `hkask-templates` | ✅ (full port-ify: `registry_sqlite.rs` rewritten to `StorageDriver`) |
| `hkask-memory` | ✅ (port-ify: `HMemStore` over `StorageDriver`; `EmbeddingStore` → `EmbeddingPort` trait) |
| `hkask-mcp` | ✅ (added `rmcp = "1"`, `tokio-util` to workspace) |
| `hkask-pods` | ✅ (full port-ify: `PerPodStorage` holds `Arc<dyn StorageDriver>` + `Arc<dyn EmbeddingPort>`; `test_stubs.rs` with `StubStorageDriver`/`StubEmbeddingPort`) |
| `hkask-mcp-server` | ✅ (port-ify: `open_database()` returns `Result<Arc<dyn StorageDriver>, McpError>`) |
| `kask_bridge` | stub |
| `kask_panel` | stub |
| `hkask-mcp-scenarios` | ✅ migrated (clean, no port-ify needed) |
| `hkask-mcp-regulation` | ✅ migrated (port-ify: `RegulationArchive` moved into the server crate over `StorageDriver`) |
| `hkask-mcp-skill` | ✅ migrated (removed `hkask-inference`; `run()` accepts `Arc<dyn InferencePort>` param) |
| `hkask-mcp-research` | ✅ migrated (port-ify: `rss_db` is `Option<Arc<dyn StorageDriver>>`; `db.rs` STUBBED — all functions return "not yet ported" error; inline rusqlite transactions removed) |

§13.1 invariant holds. All 15 crates compile together with zero errors.

## CRITICAL METHODOLOGY LESSON (do NOT repeat)

**The previous session slowed to a halt by stubbing individual functions inside crates one at a time.** This is wrong. The MCP servers and core crates are deeply interconnected — they must be moved together, then the dependency graph fixed holistically, then everything compiled at once.

**The correct approach (agreed with user):**
1. **Move ALL remaining source first.** Copy every remaining MCP server crate and every remaining dependency crate from `Clones/hKask/` to `kask/` in one pass. Don't port-ify anything yet.
2. **Add ALL to workspace.** Update root `Cargo.toml` `[workspace.members]` and `[workspace.dependencies]` for all new crates at once.
3. **Fix the dependency graph holistically.** Now that everything is in `kask/`, fix the `Cargo.toml` path deps (all `../../crates/hkask-*` from `kask/mcp-servers/` resolve to `kask/crates/hkask-*`). Strip deleted-crate deps. Add `hkask-types` storage/embedding port imports where needed.
4. **Get `cargo check` passing for ALL crates.** This may require stubbing entire modules (not individual functions) for crates that depend on deleted services. Use `todo!()` / `unimplemented!()` / return-Error stubs at the module level.
5. **DO NOT touch tests.** Delete all `tests/` directories from migrated crates. Tests are a separate pass at the very end.
6. **Run `cargo test` ONCE at the very end** after all crates compile.

## YOU ARE HERE — Move ALL remaining crates, then fix deps holistically

### Remaining MCP server crates to migrate (10)

In `Clones/hKask/mcp-servers/`:
- `hkask-mcp-codegraph` (17 src files, 3603 lines) — **partially copied already** to `kask/mcp-servers/hkask-mcp-codegraph/` but NOT in workspace. Uses `rusqlite` + `sqlite-vec` extensively. Needs full port-ify or module-level stub.
- `hkask-mcp-memory` (4 src files, 1239 lines) — deps: `hkask-storage` (deleted), `hkask-memory` ✅, `hkask-test-harness` (deleted)
- `hkask-mcp-condenser` (2 src files, 612 lines) — deps: `hkask-condenser` (deleted), `hkask-memory` ✅, `hkask-storage` (deleted), `hkask-inference` (deleted). **Needs rewrite** — condenser is deleted.
- `hkask-mcp-curator` (3 src files, 646 lines) — deps: `hkask-storage` (deleted), `hkask-memory` ✅, `hkask-services-context` (deleted), `hkask-capability` ✅. Kept but unloaded.
- `hkask-mcp-kata-kanban` (4 src files, 1501 lines) — deps: `hkask-services-kata-kanban` (not migrated), `hkask-storage` (deleted), `hkask-test-harness` (deleted)
- `hkask-mcp-media` (18 src files, 6267 lines) — deps: `hkask-pods` ✅, `hkask-inference` (deleted), `hkask-storage` (deleted)
- `hkask-mcp-training` (32 src files, 13400 lines) — deps: `hkask-capability` ✅, `hkask-memory` ✅, `hkask-storage` (deleted), `hkask-inference` (deleted). LARGE.
- `hkask-mcp-companies` (24 src files, 13592 lines) — deps: `hkask-storage` (deleted), `hkask-ledger` (keep-crate, NOT yet migrated), `hkask-forecast` ✅. LARGE.
- `hkask-mcp-replica` (4 src files, 1389 lines) — deps: many `hkask-services-*` (deleted), `hkask-inference` (deleted), `hkask-storage` (deleted), `hkask-keystore` ✅, `hkask-test-harness` (deleted). **Needs major rewrite.**
- `hkask-mcp-docproc` (40 src files, 12619 lines) — deps: `hkask-bridge-dublincore` (deleted), `hkask-inference` (deleted), `hkask-memory` ✅, `hkask-storage` (deleted), `hkask-guard` ✅, `hkask-services-core` (deleted). VERY LARGE, needs significant rewrite.

**SKIP (being deleted per architecture plan):**
- `hkask-mcp-communication` — deps: `hkask-communication` (deleted)
- `hkask-mcp-filesystem` — decided deleted (zed provides fs tools)

### Remaining dependency crates to migrate (keep-crates only)

In `Clones/hKask/crates/` — these are referenced by the MCP servers and MUST be migrated for the servers to compile:

- **`hkask-ledger`** (3 src files) — keep-crate (line 62: "rJoule energy budget + hMem accounting"). Needed by `hkask-mcp-companies`. NOT yet migrated.
- **`hkask-mcp-cloud-gateway`** (4 src files) — in `Clones/hKask/crates/` (not `mcp-servers/`). Clean (no storage refs). deps: mcp-server, types, capability. Migrate after the 16 servers.

**DO NOT migrate these (deleted per architecture plan):**
- `hkask-storage` → DELETED. Replace all `hkask_storage::*` imports with `hkask_types::storage::*` (port-ify).
- `hkask-inference` → DELETED (T5.1). Keep only the `InferencePort` *trait* in `hkask-types`. Replace all `hkask_inference::*` with `hkask_types::InferencePort`.
- `hkask-services-*` → DELETION CANDIDATES (T5.7). Functionality absorbed into `kask_bridge`/`KaskCore`. Stub/remove imports.
- `hkask-condenser` → DELETION CANDIDATE (T5.7).
- `hkask-communication` → DELETED (T5.4).
- `hkask-test-harness` → NOT in keep-list. Remove from dev-deps.
- `hkask-bridge-dublincore` → NOT in keep-list. Remove/stub.
- All other `hkask-services-*` (chat, compose, context, corpus, inference, kata-kanban, onboarding, runtime, self-heal, skill, wallet) → DELETION CANDIDATES.
- `hkask-acp`, `hkask-api`, `hkask-cli`, `hkask-git-cas`, `hkask-identity`, `hkask-repl`, `hkask-wallet` → NOT in keep-list for this phase.

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

### Migration pattern (DO THIS FOR ALL CRATES IN ONE PASS)

**Step 1: Move ALL source at once.**
```bash
# MCP servers (10 remaining — skip communication, filesystem)
for s in codegraph memory condenser curator kata-kanban media training companies replica docproc; do
  cp -r /home/mdz-axolotl/Clones/hKask/mcp-servers/hkask-mcp-$s kask/mcp-servers/
  rm -rf kask/mcp-servers/hkask-mcp-$s/tests  # DELETE all tests
done

# hkask-ledger (keep-crate needed by companies)
cp -r /home/mdz-axolotl/Clones/hKask/crates/hkask-ledger kask/crates/
rm -rf kask/crates/hkask-ledger/tests

# hkask-mcp-cloud-gateway (in crates/, not mcp-servers/)
cp -r /home/mdz-axolotl/Clones/hKask/crates/hkask-mcp-cloud-gateway kask/mcp-servers/
rm -rf kask/mcp-servers/hkask-mcp-cloud-gateway/tests
```

**Step 2: Add ALL to workspace at once.**
Add every new crate to `[workspace.members]` in root `Cargo.toml`. Add `hkask-ledger = { path = "kask/crates/hkask-ledger" }` to `[workspace.dependencies]`.

**Step 3: Fix ALL Cargo.toml path deps at once.**
For each `kask/mcp-servers/hkask-mcp-*/Cargo.toml`:
- Pin `version = "0.31.0"`, `edition = "2024"`, `license = "MIT"`, `publish = false`.
- Fix path deps to `../../crates/hkask-*` (relative from `kask/mcp-servers/`).
- Keep `.workspace = true` for shared deps.
- **Strip these deleted-crate deps:** `hkask-storage`, `rusqlite`, `r2d2`, `r2d2_sqlite`, `sqlite-vec`, `sqlx`, `hkask-inference`, `hkask-services-*`, `hkask-test-harness`, `hkask-condenser`, `hkask-bridge-dublincore`, `hkask-communication`.
- Add `hkask-types = { path = "../../crates/hkask-types" }` if not present (for storage/embedding ports).
- For tokio bins, add `features = ["rt-multi-thread", "macros"]`.

**Step 4: Fix ALL source imports holistically.**
For each crate's `src/`:
- Replace `use hkask_storage::*` with `use hkask_types::storage::*` (port-ify).
- Replace `use hkask_inference::*` with `use hkask_types::InferencePort` (the trait). Move any model constants to `hkask-types` or inline.
- Replace `rusqlite::Connection` params with `&dyn StorageDriver`. Replace `rusqlite::params![...]` with `&[DbValue::...]`. Replace `rusqlite::Row` access with `DbRow`.
- For `hkask_services_*`, `hkask_condenser`, `hkask_bridge_dublincore` imports: **stub at the module level** — replace the `use` with a TODO comment and stub the imported types/functions with `todo!()` or return-Error stubs.
- For `hkask_test_harness` in dev-deps: strip it. (Tests already deleted.)
- For `sqlite-vec` FFI (`init_sqlite_vec_on`): **stub the vector search** — return empty results or `todo!()`. Pure-Rust cosine similarity is a bridge responsibility (decision already made).

**Step 5: `cargo check` ALL crates.**
```bash
cargo check -p hkask-mcp-codegraph -p hkask-mcp-memory -p hkask-mcp-condenser -p hkask-mcp-curator -p hkask-mcp-kata-kanban -p hkask-mcp-media -p hkask-mcp-training -p hkask-mcp-companies -p hkask-mcp-replica -p hkask-mcp-docproc -p hkask-ledger -p hkask-mcp-cloud-gateway
```
Fix errors. For crates with deep deleted-dep entanglement (replica, docproc, condenser), **stub entire modules** rather than individual functions. A module-level stub is:
```rust
//! STUB (T0.6): original module depended on deleted crate `hkask-services-*`.
//! Functionality moves to `kask_bridge`/`KaskCore` (T5.7). Re-implement over ports.
pub fn <name>(...) -> Result<_, anyhow::Error> {
    Err(anyhow::anyhow!("not yet ported — see kask/docs/specs/seam-specs.md T0.6"))
}
```

**Step 6: Verify §13.1 invariant.**
```bash
bash kask/scripts/check-hkask-no-zed-deps.sh
```

### After ALL MCP servers compile

7. **Update `BUILTIN_SERVERS`** in `hkask-mcp-server/src/lib.rs` — remove `communication` + `filesystem` entries (deleted per architecture plan).
8. **Skills registry** → `kask/skills/` (`manifest.yaml` + `*.j2`). Copy from `Clones/hKask/registry/` (already partially copied to `kask/registry/` for templates).
9. **Recompose tests** — write fresh tests based on zed-kask integration requirements. Use `hkask-pods::test_stubs` (`StubStorageDriver`/`StubEmbeddingPort`) for storage-backed tests. For tests needing a real `StorageDriver`, wait for `kask_bridge` (T1.4) or add an in-memory `StorageDriver` to `hkask-pods::test_stubs`.
10. **Archive `mdz-axo/hKask`**.

### Key decisions already made (this session + prior)

- **Full merge** into `kask/` namespace; hKask repo archived. No backward compatibility.
- **Conform to zed deps** — don't bump zed's versions. Add new deps (like `rmcp`, `llm-guard`, `minijinja`, `serde_yaml_neo`) to workspace `[workspace.dependencies]`.
- **`StorageDriver` port** in `hkask-types::storage`; `kask_bridge` implements over `sqlez`.
- **`EmbeddingPort` port** in `hkask-types::ports::embedding_port`; `kask_bridge` implements with **pure-Rust brute-force cosine similarity** (NOT sqlite-vec — personal agent scale, sub-ms for 10K × 1024-dim embeddings; no C deps, no global side effects, trivially debuggable). Escape hatch: pure-Rust HNSW (`hnsw_rs`) if scale ever demands it.
- **`HMemStore`** is a concrete struct (not a trait) — already provider-agnostic, moved to `hkask-memory` using `StorageDriver`.
- **`MemoryPort`** (D6): `ingest_thread` + `recall_semantic` — bridge implements over in-process memory handles.
- **`CuratorSync`** uses `driver_factory` closure (`Arc<dyn Fn(&Path) -> Result<Arc<dyn StorageDriver>, String>)`) — bridge provides real impl.
- **`PodFactory::deploy`** accepts `driver: Arc<dyn StorageDriver>` + `embedding: Arc<dyn EmbeddingPort>` as params — composition root moves to `KaskCore`/`kask_bridge`.
- **`test_stubs`** module in `hkask-pods` — `StubStorageDriver` + `StubEmbeddingPort` for test harnesses.
- **12 MCP servers loaded by default** (`communication` + `filesystem` deleted; `curator` + `skill` kept but unloaded).
- **kask panel** = native GPUI (reuses `ui::prelude::*`; no ratatui).
- **`KaskSettings`** = new `"kask": {...}` settings.json section + kask credentials namespace.
- **App-identity**: `APP_NAME = "Zed-Kask"`, distinct single-instance port, `.zed-kask_server`, binary `zed-kask`. Keep `*.zed.dev` account.
- **`RegulationArchive`** was port-ified INTO `hkask-mcp-regulation` (the only consumer of its query methods). Pattern: when a deleted-crate's struct is only used by one server, move the struct into that server crate over `StorageDriver`.
- **`hkask-mcp-skill::run()`** accepts `inference_port: Arc<dyn InferencePort>` param — composition root moves to `KaskCore`/`kask_bridge`. The standalone binary returns an error directing callers to the in-process path.
- **`hkask-mcp-research::db.rs`** was STUBBED (all functions return "not yet ported" error) — the full rusqlite → StorageDriver port is a dedicated task. The `rss_db` field is `Option<Arc<dyn StorageDriver>>`.

### Workspace deps already added

Added to `[workspace.dependencies]` in root `Cargo.toml`:
- `llm-guard = "0.2"`
- `minijinja = { version = "2", features = ["loader", "serde", "json", "internal_safe_search"] }`
- `serde_yaml_neo = "0.11"`
- `rmcp = "1"`
- `tokio-util = { version = "0.7", features = ["rt"] }`

`schemars` IS in zed workspace (v1.0, features `["indexmap2"]`) — MCP servers can use `.workspace = true`.

### What to do right now

1. **Move ALL remaining source in one pass** (Step 1 above). Don't port-ify anything yet.
2. **Add ALL to workspace** (Step 2).
3. **Fix ALL Cargo.toml path deps** (Step 3).
4. **Fix ALL source imports** (Step 4) — stub modules for deleted-dep crates, port-ify storage/inference.
5. **`cargo check` ALL** (Step 5) — fix errors holistically.
6. **Verify §13.1** (Step 6).
7. Then: update `BUILTIN_SERVERS`, copy skills registry, recompose tests, archive hKask.

**DO NOT stub individual functions inside crates.** Move everything first, then fix the dependency graph holistically. The MCP servers and core crates are interconnected — they must be moved together.

---

The prompt is also saved at `Clones/zed-kask/kask/docs/plans/continuation-prompt.md` for reference. Paste it into a fresh Zed agent session to continue from exactly where we left off — 4 MCP servers migrated + 1 partially copied, 10 remaining to move + 1 keep-crate (`hkask-ledger`) + 1 cloud-gateway, then fix deps holistically, then recompose tests.
