# Continuation Prompt — zed-kask migration (T0.6 in progress)

You are continuing the migration of hKask into the zed-kask fork of Zed. Here is everything you need to pick up cleanly.

## What we're doing

Migrating hKask into the `zed-kask` fork of Zed (`Clones/zed-kask`, origin `mdz-axo/zed-kask`, upstream `zed/zed`). hKask is being **fully merged** into zed-kask under a `kask/` namespace. The `mdz-axo/hKask` repo (`Clones/hKask`) will be archived. zed-kask is the single source of truth — one clone, one build, one CI.

## Key files

- `kask/docs/architecture/zed-host-architecture-plan.md` (564 lines) — the full architecture + migration plan (14 sections, D1–D10 divergence seams, §13 composition root, §14 repository consolidation).
- `DIVERGENCE.md` (repo root, ~63 lines) — the fork's divergence manifest + upstream-sync procedure + dependency policy.
- `kask/docs/specs/seam-specs.md` (~258 lines) — D1–D10 seam specifications with port contracts + acceptance criteria + T0.6-storage spec.
- `kask/docs/plans/upstream-sync-runbook.md` — the sync procedure.
- `kask/scripts/check-hkask-no-zed-deps.sh` — the §13.1 invariant CI gate (tested, passing).

## Governing invariant (§13.1)

**hKask crates NEVER depend on zed-kask crates.** The sole bidirectional seam is `kask/crates/kask_bridge` (D8). Enforced by `kask/scripts/check-hkask-no-zed-deps.sh` which scans `hkask-*` Cargo.tomls for zed-crate denylist names + path-deps escaping `kask/`.

## Dependency policy (conform to zed)

hKask conforms to zed's dependency versions where there are conflicts. **Do NOT bump zed's workspace deps.** The `libsqlite3-sys` conflict: zed pins `0.30.1` (via `sqlez` + `sqlx-sqlite` → `collab`); no `rusqlite` version is compatible. **Resolution:** `hkask-storage` will be rewritten to use a `StoragePort` trait (now defined in `hkask-types` as `StorageDriver`), implemented by `kask_bridge` over zed's `sqlez`. SQLCipher → application-layer encryption (encrypt before store, decrypt after read). See DIVERGENCE.md + seam-specs.md "T0.6-storage".

## Current state — 6 workspace members, 4 compiling

| Crate | Status | Notes |
|---|---|---|
| `hkask-types` | ✅ compiles | Now includes `storage.rs` with `DbValue`, `DbRow`, `StorageDriver` trait, `define_driver_store!` macro, `query_map`/`query_row` helpers |
| `hkask-capability` | ✅ compiles | OCAP ToolPort + DelegationToken |

| `hkask-goal` | ✅ compiles | Goal types |
| `hkask-keystore` | ✅ compiles | OS keychain, AES-256-GCM (trimmed: sovereignty crypto only) |
| `kask_bridge` | stub (empty) | D8 — the sole bidirectional seam |
| `kask_panel` | stub (empty) | D10 — native GPUI Panel |
| **`hkask-regulation`** | **2 errors remaining** | Port-ified: all `hkask_storage` refs replaced with `hkask_types::storage`. See below. |

Workspace deps added for hKask: `blake3 = "1"`, `ed25519-dalek = "2"`, `keyring`, `aes-gcm`, `argon2`, `hmac`, `hex` (with `serde` feature), `hkask-types`, `hkask-capability`, `hkask-goal`, `hkask-keystore`, `hkask-regulation`.

Workspace members in `Cargo.toml`: `kask/crates/kask_bridge`, `kask/crates/kask_panel`, `kask/crates/hkask-types`, `kask/crates/hkask-capability`, `kask/crates/hkask-goal`, `kask/crates/hkask-keystore`, `kask/crates/hkask-regulation`.

## The 2 errors to fix in `hkask-regulation`

Run `cargo check -p hkask-regulation` from `Clones/zed-kask` to reproduce.

**Error 1 — `seam_watcher.rs:34`:**
```
include_str!("../../../docs/status/public-seam-inventory.json")
```
The crate moved from `crates/hkask-regulation/` to `kask/crates/hkask-regulation/`, so the relative path is one level too shallow. Fix: either copy the JSON from `Clones/hKask/docs/status/public-seam-inventory.json` to `kask/docs/status/` and change the path to `"../../../docs/status/public-seam-inventory.json"` → `"../../../../docs/status/public-seam-inventory.json"`, OR make the include_str! fall back to an empty string (the seam watcher is non-fatal — it silently disables when no inventory is available). The simplest fix: change the path to `concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/status/public-seam-inventory.json")` or just copy the file and fix the depth.

**Error 2 — `agent_wallet_store.rs:30`:**
The wholesale `hkask_storage → hkask_types::storage` replacement doubled a path:
```
hkask_types::storage::hkask_types::storage::StorageDriver
```
The `define_driver_store!` macro generates `$crate::storage::StorageDriver` (which resolves to `hkask_types::storage::StorageDriver`), but the `init_schema` function has a hand-written signature that got doubled by the replacement. Fix: find line 30 in `kask/crates/hkask-regulation/src/agent_wallet_store.rs`, replace the doubled path `hkask_types::storage::hkask_types::storage::StorageDriver` with `hkask_types::storage::StorageDriver`.

After these 2 fixes: `cargo check -p hkask-regulation` should compile. Then verify the invariant: `bash kask/scripts/check-hkask-no-zed-deps.sh`.

## Migration pattern (for each remaining keep-crate)

1. Copy the crate from `Clones/hKask/crates/<name>` to `kask/crates/<name>`.
2. Adapt `Cargo.toml`: pin `version = "0.31.0"`, `edition = "2024"`, `license = "MIT"`, `publish = false`. Keep `.workspace = true` for shared deps. Fix any missing features (e.g., `hex` needs `serde`, `tokio` needs `features = ["full"]`). Strip dev-deps that reference not-yet-migrated hKask crates (re-add as they arrive).
3. If the crate imports `hkask_storage`, replace with `hkask_types::storage` (port-ify — use `StorageDriver` instead of concrete storage types).
4. Add to workspace members + `[workspace.dependencies]` in root `Cargo.toml`.
5. `cargo check -p <crate>` — verify it compiles.
6. `bash kask/scripts/check-hkask-no-zed-deps.sh` — verify the §13.1 invariant.

## Remaining migration order (dependency chain, after hkask-regulation compiles)

1. **`hkask-regulation`** — fix the 2 errors above. ⬅️ YOU ARE HERE
2. **`hkask-guard`** — depends on `hkask-regulation` + `hkask-types`. External dep: `llm-guard = "0.2"` (add to workspace deps).
3. **`hkask-forecast`** — check deps; `hkask-templates` depends on it.
4. **`hkask-templates`** — depends on `hkask-types`, `hkask-capability`, `hkask-forecast`, `hkask-guard`, `hkask-regulation`. External deps: `minijinja`, `r2d2`, `r2d2_sqlite` (deferred — strip rusqlite/r2d2 deps, port-ify if needed).
5. **`hkask-memory`** — depends on `hkask-types`, `hkask-storage` (port-ify).
6. **`hkask-pods`** — depends on `hkask-types`, `hkask-capability`, `hkask-mcp`, `hkask-templates`, `hkask-regulation`, `hkask-keystore`, `hkask-storage` (port-ify), `hkask-memory`. The Curator + UserPod crate.
7. **`hkask-mcp` / `hkask-mcp-server`** — the MCP framework. Check deps.
8. **The 15 MCP servers** — each under `kask/mcp-servers/`. The 12 loaded by default: `memory`, `condenser`, `research`, `companies`, `media`, `docproc`, `training`, `replica`, `kata-kanban`, `codegraph`, `scenarios`, `regulation`. Kept but not loaded: `curator`, `skill`. Deleted: `communication`, `filesystem`.
9. **The skills registry** — copy `.agents/skills/` to `kask/skills/` (the `manifest.yaml` + `*.j2` templates — Pattern A source of truth).
10. **Archive `mdz-axo/hKask`** — GitHub archive + root `README.md` pointing to `zed-kask/kask/`.

## Key decisions already made

- **Full merge** (§14): hKask → zed-kask under `kask/`; hKask repo archived.
- **No backward compatibility** (§1): build-then-delete, no coexistence shims.
- **Conform to zed deps** (DIVERGENCE.md): don't bump zed's versions; refactor hKask to use zed's stack.
- **StoragePort** (T0.6-storage): `StorageDriver` trait in `hkask-types`; `kask_bridge` implements over `sqlez`; SQLCipher → application-layer encryption.
- **12 MCP servers loaded by default** (§2.4): `communication` + `filesystem` deleted; `curator` + `skill` kept but unloaded.
- **kask panel = native GPUI** (§12, option B): reimplement `McpScopedWindow` as a zed `Panel` reusing `ui::prelude::*`; no ratatui terminal.
- **KaskSettings** (§11, D9): new `"kask": {...}` settings.json section + kask credentials namespace in `CredentialsProvider`.
- **App-identity separation** (§7, D7): `APP_NAME = "Zed-Kask"`, distinct single-instance port, `.zed-kask_server` remote dirs, binary `zed-kask`. Keep shared `*.zed.dev` account endpoints.

## Port set (§13.2) — all in `hkask-types` / `hkask-capability`, implemented by `kask_bridge`

- `InferencePort` (exists in `hkask-types`) — over zed's `LanguageModel` (streaming → non-streaming adapter)
- `ToolPort` (exists in `hkask-capability`) — over the in-process MCP tool registry
- `StorageDriver` (NEW in `hkask-types::storage`) — over zed's `sqlez`
- `SecretsPort` (NEW, to define in `hkask-types`) — over `CredentialsProvider`
- `CuratorTurnPort` (NEW, to define in `hkask-types`) — over native-agent turn
- `MemoryPort` (NEW, to define in `hkask-types`) — over in-process memory handles

## Composition root (§13.3)

zed-kask startup constructs one `KaskCore` (per-pod SQLCipher storage + Regulation + memory + singleton Curator + 12 MCP servers + ManifestExecutor), wires the bridge adapters, spawns the regulation/Curator tokio loops on `gpui_tokio`, registers the UserPod + Curator agents + KaskPanel, loads KaskSettings → KaskCore params, runs config migration.

## What to do right now

1. Fix the 2 errors in `hkask-regulation` (see above).
2. `cargo check -p hkask-regulation` — verify it compiles.
3. `bash kask/scripts/check-hkask-no-zed-deps.sh` — verify invariant.
4. Continue the migration: `hkask-guard` → `hkask-forecast` → `hkask-templates` → `hkask-memory` → `hkask-pods` → `hkask-mcp` → MCP servers → skills → archive hKask.
5. For each crate, follow the migration pattern above.
