# Refactor-Architecture Review — Kask↔Zed Seam

> Method: `refactor-architecture` (codegraph simplification, deepening
> candidates, strangler-fig) + `essentialist` deletion test (G1 Exist / G2
> Surface / G3 Contract) on every proposed deletion. Per `.rules`, dead surface
> comes in three forms: trait-with-one-impl, convention-helpers-with-only-
> test-callers, folded-service re-exports. Caller counts are grep-verified.

## Summary

- **~2,400 lines** of dead surface, concentrated in `hkask-templates` (the
  registry trait layer) + two test-only files + one zero-caller loader.
- **`hkask-mcp-corpus` folded services are clean** — the `inference_svc`/
  `model_cache`/`runtime`/`services`/`compose`/`cost`/`batch` modules are all
  wired to tool consumers. The prior `AdapterPort`/`huggingface.rs` registry
  cleanup (per `.rules`) held.
- **One `unwrap_or(0)` trap on a DB query** (`SqliteRegistry::count`) — but the
  function is dead, so the trap is moot until revived.

## Findings

| ID | kind | crate | symbol | file:line | deletion_test | force | deferred |
|----|------|-------|--------|-----------|---------------|-------|----------|
| RA-01 | trait-one-impl | hkask-types+templates | `SkillRegistryIndex` (6/7 methods dead) | `ports/registry.rs:286` | complexity_vanishes | directing | no |
| RA-02 | helper-test-only | hkask-templates | `SkillLoader` + `SkillFrontMatter`/`SkillLoadResult` re-exports | `skill_loader.rs:64` | complexity_vanishes | blocking | no |
| RA-03 | helper-test-only | hkask-templates | `resolve_manifest` + `load_manifest_from_file` | `manifest_loader.rs:207` | complexity_vanishes | directing | no |
| RA-04 | folded-dead-surface | hkask-storage | `hmem::archive` (557 lines, test-only) | `hmem/archive.rs:60` | complexity_vanishes | directing | no |
| RA-05 | helper-test-only | hkask-ledger | `namespace_balances` + `AccountBalance` | `hkask_ledger.rs:326` | complexity_vanishes | enabling | no |
| RA-06 | folded-dead-surface | hkask-forecast | `falsification` module (492 lines) | `falsification.rs:14` | unclear | enabling | **yes** |
| RA-07 | helper-test-only | hkask-forecast | `isotonic_fit`, `isotonic_apply`, `scenario_node_loading`, `fuse_volatility` | `hkask_forecast.rs:266` | complexity_vanishes | enabling | no |
| RA-08 | helper-test-only | hkask-templates | `SqliteRegistry::count` + `unwrap_or(0)` on query | `registry_sqlite.rs:235` | complexity_vanishes | enabling | no |
| RA-09 | shallowness | hkask-templates | `Registry` in-memory cache (896 lines, test-only) | `registry.rs:182` | complexity_vanishes | directing | **yes** |
| RA-10 | trait-one-impl | hkask-templates | `BundleRegistryIndex` (5/5 dead) | `bundle/mod.rs:22` | complexity_vanishes | directing | **yes** |
| RA-11 | helper-test-only | hkask-types | `list_with_capabilities` + `list_skills_visible_to` (dead defaults) | `ports/registry.rs:317` | complexity_vanishes | enabling | no |

## Top deletion candidates by line count

1. **RA-04** `hmem::archive` — 557 lines, test-only, no production caller.
2. **RA-06** `hkask-forecast::falsification` — 492 lines, research artifact
   compiled as `pub mod`, zero external callers. (deferred — user decision)
3. **RA-02** `SkillLoader` — 440 lines, zero callers anywhere, exported via `pub use`.

## The `hkask-templates` dead-surface cluster

`hkask-templates`'s crate-root `pub use` list is majority-orphaned:
- `pub use skill_loader::{SkillFrontMatter, SkillLoadResult, SkillLoader}` — dead (RA-02, verified zero callers).
- `pub use manifest_loader::{..., load_manifest_from_file, ..., resolve_manifest}` — 2 of 4 exports dead (RA-03, verified).
- `pub use bundle::BundleRegistryIndex` — dead (RA-10, depends on RA-03).
- `pub use registry::{... Registry ...}` — `Registry` test-only (RA-09).

Live re-exports that must stay: `BundleManifest`,
`executor::{CascadeEvent, ManifestExecutor, extract_final_step_result}`,
`inputs::*`, `manifest_loader::{load_manifest_from_yaml, ManifestLoadError}`,
`ports::*`, `prompt_strategy::PromptStrategy`, `registry_sqlite::SqliteRegistry`.

## Deepening note

The two-tier `Registry`/`SqliteRegistry` cache (RA-09) is the clearest
deepening candidate. Its doc comment advertises "always used in tandem" but
production reads manifests from disk via `BridgeManifestExecutor`
(`skill_executor.rs:129`) and uses `SqliteRegistry::get_entry` directly in
`KataEngine` (`kata.rs:79`). The in-memory tier is speculative generality;
deleting it makes complexity vanish (the bootstrap test can use `SqliteRegistry`
directly). This is the Ousterhout deletion test: delete the module — if
complexity reappears elsewhere, it deserves to exist; here it vanishes.

## Strangler-fig migration plan (for the deferred items)

If the operator chooses to delete the `hkask-templates` registry abstraction
(RA-01/09/10) rather than gate it:
1. Migrate `bootstrap_test.rs` to assert against `SqliteRegistry` directly.
2. Move `register_skill` from the trait to an inherent `SqliteRegistry` method.
3. Delete `Registry` (in-memory), `SkillRegistryIndex`/`BundleRegistryIndex`
   traits, and the `*_owned`/`query_skills` forwarder helpers.
4. Pin each removal with a grep-based test asserting the symbol is gone.
Keep `load_manifest_from_yaml` (live) and `MANIFEST_YAMLS` seeds.

## Verification caveat

Caller counts were verified by grep across `kask/crates`, `kask/mcp-servers`,
and zed-side `crates/`. Trait-impl forwarders and inherent self-calls were
filtered. Path-qualified `Arc<dyn Trait>` usage was accounted for — no live
trait was mis-classified as dead. RA-06/RA-09/RA-10 are marked deferred because
they may be intentional research/test scaffolding; the user decides delete vs
cfg-gate vs wire-a-real-consumer.