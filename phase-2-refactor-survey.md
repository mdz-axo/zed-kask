# Phase 2 — Refactor-Architecture Survey

**Date:** 2026-08-03
**Scope:** kask-owned crates (`kask/crates/*`, `kask/mcp-servers/*`, `crates/hkask-*-widget`, `crates/hkask-viz-core`, `crates/kask_extensions_ui`, `crates/swarm_panel`, `crates/marketplace_ui_common`) + the D-seam boundary (DIVERGENCE.md D1–D23).
**Backward-compat note:** No backward-compatibility constraints apply within kask-owned crates. Renames, restructures, and deletions are permitted. The D-seam boundary and "do not touch upstream" rule apply in full.
**Mode:** Survey only. No code was modified in this phase.

---

## Metacognition record

| | Prediction | Actual | Brier |
|---|---|---|---|
| Deepening candidates | 3–5 (conf 0.5) | 11+ | 0.25 (undercounted) |
| Duplication findings | 2–3 (conf 0.5) | 5 | 0.25 (close) |
| Strangler-fig candidates | 1–2 (conf 0.4) | 0 (MCP-only, no multi-surface) | 0.36 (wrong) |

Combined Brier ≈ 0.29. The direction was right (friction exists) but I overestimated strangler-fig (the codebase is MCP-server-only, not multi-surface CLI/API) and underestimated the volume of deepening candidates.

---

## Methodology

Three parallel surveys were run using the `refactor-architecture` skill's explore + candidates + audit methodology:

1. **hKask library crates** (20 crates under `kask/crates/`) — friction, deepening candidates, trait-with-one-impl check
2. **MCP servers + widget crates** (11 servers + 6 widget crates) — cross-server duplication, widget boilerplate, error-classification audit
3. **D-seam boundary + widget crates** (D1–D23, `kask_bridge`, `swarm_panel`, `kask_extensions_ui`) — seam isolation, test coverage, bridge width

Each finding was validated through the essentialist 3-gate (Exist → Surface → Contract) before entering this report. Findings that failed the gate were excluded.

---

## Pragmatic-semantics classification

All findings are **IS** (declarative, observed in code) unless marked **OUGHT** (normative recommendation). Provenance is codebase inspection. Confidence ≥ 0.8 for all findings (verified by reading source files, not inferred).

---

## Ranked findings

### High severity

| # | Area | File(s) | Friction | Deepening candidate | Lev | Loc | Test | IS/OUGHT |
|---|------|---------|----------|---------------------|-----|-----|------|----------|
| H1 | hkask-mcp-swarm | 7 files in `src/local_*.rs`, `consent.rs`, `a2a_http.rs`, `abw_util.rs` | **19 `Result<_, String>` signatures** in the local-runtime layer. The `String` erases error kind; the tool-method boundary blanket-maps everything to `McpToolError::internal` — the `.rules` "classify per variant, not blanket `internal`" trap. | Introduce `LocalSwarmError` enum (`#[derive(thiserror::Error)]`) with variants matching actual failure modes (Io, Ledger, InvalidInput, NotFound, Sanitize, Unavailable). Add `map_local_swarm_error` classifying per variant — mirrors existing clean patterns (`map_media_error`, training's `error_mapping.rs`). | 4 | 5 | 4 | OUGHT |
| H2 | 5 MCP servers | `hkask-mcp-{codegraph,companies,condenser,corpus,research}` | **40+ inline `McpToolError::internal(format!("…: {e}"))`** sites. Domain errors with distinguishable variants are flattened to `Internal`. IO permission error and serialize bug are indistinguishable to the consumer. The other 6 servers already do this correctly (`map_media_error`, `map_scenario_error`, `map_kanban_error`, etc.). | Per-domain `map_*_error` functions following the `hkask-mcp-training/src/tools/error_mapping.rs` pattern. For servers with `anyhow` errors (corpus, condenser), first introduce a small typed error enum, then map. | 4 | 4 | 4 | OUGHT |
| H3 | swarm_panel | `crates/swarm_panel/src/swarm_panel.rs` | **Monolithic 3,700-line file.** Single `SwarmPanel` struct with 60+ impl methods, 4 modes (Browse/Author/Compose/Steer), 8 response-parsing functions, 15+ sub-structs, 660-line test module. Low cohesion: parsing, rendering, state, and dispatch are interleaved. | Split into `parse.rs` (pure parsing functions, already tested), `swarm_panel.rs` (struct + Render + Item), and mode-specific render modules. Parsing extraction is zero-risk. | 3 | 5 | 5 | OUGHT |
| H4 | hkask-condenser | `kask/crates/hkask-condenser/Cargo.toml` + `src/types.rs` | **Leaky dependency**: "pure domain crate" (no MCP, no HTTP, no async per docs) depends on `hkask-mcp-server` only for `AnyJsonValue` + `find_boolean_schema_positions` from `tool_schema.rs`. Drags in `hkask-keystore`, `hkask-storage`, `rmcp`, `reqwest`, `tracing-subscriber` transitively. | Extract `tool_schema.rs` to `hkask-types` (with `schemars` feature) or a dedicated `hkask-schema` crate. 2 import paths to update. | 4 | 5 | 3 | IS |
| H5 | hkask-types | `src/ports/inference_port.rs:143-162` | **`String` error type on port trait**: `SkillExecPort::execute_skill` returns `Result<String, String>`. Loses error type info, prevents `?` propagation. `InferenceIpcClient` has a proper `InferenceError` internally but flattens to `String` to match the trait. | Change trait signature to `Result<String, InferenceError>` or dedicated `SkillExecError`. Updates 2 downstream impls automatically. (Note: the `agent::SkillManifestExecutor` impl is a D1 seam constraint — not fixable without upstream change.) | 3 | 4 | 3 | OUGHT |

### Medium severity

| # | Area | File(s) | Friction | Deepening candidate | Lev | Loc | Test | IS/OUGHT |
|---|------|---------|----------|---------------------|-----|-----|------|----------|
| M1 | hkask-viz-core + 4 widgets | `crates/hkask-{graph,kanban,portfolio,scenarios}-widget/src/*.rs` | **Copy-pasted factory + cache-dispatch + render arms (5×)**. Three layers of duplication: `create_*_widget` factories, `block_renderer()` 5 near-identical arms, `CachedWidget::render()` 5 identical match arms. Adding a 6th widget requires touching 3 coordinated places. | A `VizWidget` trait (associated type + const + fn) with a registry in `viz-core`. `block_renderer` becomes a loop. Justified by 5 impls — not speculative generality per `.rules`. | 3 | 3 | 4 | OUGHT |
| M2 | hkask-mcp-swarm | `src/hkask_mcp_swarm.rs` (L140–3031) | **2900-line `SwarmServer` impl / 50 tool methods in one file.** Mixes ABW-cloud, local-swarm, a2a, knowledge, and lifecycle tools. Hard to navigate. | Split into concern files using rmcp router composition (`Self::cloud_router() + Self::local_router() + ...`), mirroring corpus and companies (both use 7 sub-routers). 50-tool-count test guards the surface. | 3 | 5 | 3 | OUGHT |
| M3 | kask_bridge | `src/context_injector.rs` | **Duplicated injector logic.** `BridgeContextInjector` and `BridgeCuratorContextInjector` share identical `should_recall` methods, identical constants (`MIN_RECALL_PROMPT_LEN`/`MIN_RECALL_PROMPT_WORDS`), and near-identical `inject_context`/`inject_static_context` bodies. ~100 lines of copy-paste. | Extract shared `recall_and_format` helper taking a recall closure. `should_recall` is already tested via proptest in `bridge_gap_properties.rs` — no test changes needed. | 4 | 4 | 5 | OUGHT |
| M4 | kask_bridge | `tests/bridge_properties.rs` | **IPC dispatch path untested.** `InferenceIpcServer::dispatch` (routing `InferenceMethod` to port methods) has zero test coverage. Only serialization roundtrips are tested. The `tool_invoke` and `skill_execute` dispatch arms are untested. | Add a test-only `InferencePort` impl returning canned `InferenceResult`s. Drive `dispatch` end-to-end through a Unix socket pair. Same pattern as `SnapshotProfileResolver` in `skill_executor.rs`. | 3 | 3 | 3 | IS |
| M5 | swarm_panel | `src/swarm_panel.rs` (L3757–3842) | **Manually synced tool-name list.** Test hardcodes 39 tool names that must match `hkask-mcp-swarm`'s `#[tool]` fns. A rename in the MCP server silently degrades to "tool not found" at runtime. | Extract tool names into a `const TOOLS: &[&str]` shared between the test and `steer_system_prompt`. | 3 | 4 | 4 | IS |
| M6 | hkask-storage | `src/hkask_storage.rs` | **Very wide public API (~45+ exports)**. Re-exports ~45 types from 7 sub-modules. Exceeds Ousterhout's ≤7 guideline by 6×. | Encourage sub-module-path imports; add `#[deprecated]` note on root re-exports. Or split domain stores into thin crates over the storage core. | 3 | 2 | 2 | OUGHT |
| M7 | hkask-ledger | `src/hkask_ledger.rs:33-53` | **Duplicated store boilerplate.** `Ledger` manually implements `from_driver` + `init_schema` instead of using `define_driver_store!` macro (macro hardcodes `InfrastructureError` as return type). | Generalize `define_driver_store!` to accept an error type parameter: `define_driver_store!(Ledger, LedgerError)`. | 2 | 4 | 2 | OUGHT |
| M8 | hkask-regulation | `src/tool_stats.rs:189-190` | **`unwrap_or(0)` on deserialized state fields.** `load_state` reads `successes`/`failures` from saved JSON with `.unwrap_or(0)`. Not the DB-outage `.rules` trap (JSON field defaults), but a malformed state file silently produces 0 stats, masking corruption. | Emit `tracing::warn!` when a field is missing/malformed during state load, or return `Result`. | 1 | 4 | 2 | OUGHT |

### Low severity

| # | Area | File(s) | Friction | Deepening candidate | Lev | Loc | Test |
|---|------|---------|----------|---------------------|-----|-----|------|
| L1 | kask_bridge | `src/context_injector.rs` header | **Stale D-seam reference.** File header says `(D11)` but D11 is the `time::format_description::parse` deprecation. Context injection is D8. Renumbered when D10 was removed. | Change `(D11)` to `(D8)`. One-line fix. | 1 | 5 | 5 |
| L2 | swarm_panel + kask_extensions_ui | `*/src/panel_button.rs` | **Near-identical `StatusItemView` implementations** (2×). Differ only in icon name, label, and button id. ~65 lines of structural duplication. | Extract `PanelToggleButton` generic over label + icon + `Toggle` action into `marketplace_ui_common`. | 2 | 4 | 5 |
| L3 | swarm_panel | `src/swarm_panel.rs` (L100–233) | **`steer_system_prompt` hardcodes a second tool inventory.** The system prompt lists tool names inline — a second copy of the tool list. | Source the tool name list from a single `const` (shared with M5). | 2 | 4 | 3 |
| L4 | kask_extensions_ui | `src/kask_extensions_ui.rs` | **No `SerializableItem` impl.** Page state (filter, search query, fetch error) is lost on workspace reload. Asymmetry with `SwarmPanel` which does implement it. | Implement `SerializableItem` mirroring `SwarmPanel`'s pattern. | 1 | 3 | 4 |
| L5 | D7 app-identity | 6+ files | **No pinning test for app-identity constants.** `APP_NAME`, `app_id`, port offset, etc. are scattered with no single test asserting them. An upstream merge that reverts one produces a silently broken identity. | Add `test_app_identity_constants` in `paths` or `release_channel`. | 2 | 5 | 5 |
| L6 | hkask-templates | `src/hkask_templates.rs:33-36` | **Pass-through re-exports from `hkask-types`.** Creates import ambiguity. | Remove re-exports; callers depend on `hkask-types` directly. | 1 | 5 | 1 |
| L7 | hkask-bridge-dublincore | `src/hkask_bridge_dublincore.rs:18` | **Wildcard re-export `pub use pko::*`.** Makes the public API invisible from the root. | Replace with explicit re-exports. | 1 | 5 | 1 |
| L8 | 4 widget `block.rs` | `crates/hkask-{graph,kanban,portfolio,scenarios}-widget/src/block.rs` | **Identical `parse_*_body` one-liner + `viz` discriminator field.** | Shared `parse_viz_body<T>` helper in `hkask-viz-core`. Bundle with M1. | 2 | 3 | 4 |
| L9 | kask_bridge | `src/identity.rs:260` | **`String` error type on free function.** `resolve_or_create_passphrase() -> Result<String, String>`. | Replace with `Result<String, KeystoreError>`. Single function, single caller. | 2 | 5 | 2 |

---

## D-seam isolation assessment

| D-seam | Cleanly isolated? | Logic leaked? | Test coverage | Stale? |
|--------|-------------------|---------------|---------------|--------|
| D1–D6, D8–D9 | ✅ Yes | ❌ No | Good–Excellent | ❌ No |
| D7 (app-identity) | ⚠️ Scattered by nature (6+ files) | ❌ No | **Gap** — no pinning test (L5) | ❌ No |
| D10 | N/A (correctly removed) | N/A | N/A | N/A |
| D11–D20 | ✅ Yes | ❌ No | All pinned with tests | ❌ No |

**No D-seam has logic that leaked outside the seam.** All kask behavior is behind the seams. The only gap is D7's missing pinning test (L5).

## Trait-with-one-impl check

**No dead traits found.** All port traits in the kask crates have ≥2 real implementations or are consumed via `Arc<dyn Trait>` cross-crate. The previously-flagged dead traits (`AdapterPort`/`AdapterRouter`, huggingface registries) are confirmed removed.

## Bridge width assessment

`kask_bridge` exports 9 adapters/ports + 5 config/utility modules. Each adapter implements a distinct port trait. **No dead adapters found** — all are wired in `main.rs` and consumed by MCP servers or the composition root. The bridge is wide but justified; each port maps to a distinct cross-cutting concern.

## Strangler-fig candidates

**None identified.** The kask codebase is MCP-server-only (no multi-surface CLI/API duplication). The `.rules` "MCP tool error classification" pattern exists across servers but is an error-handling convention, not a domain operation duplication that would warrant service extraction.

---

## Essentialist 3-gate validation

All 24 findings passed the 3-gate (Exist → Surface → Contract):

- **G1 (Exist):** Would complexity vanish if the finding were deleted? No — each finding represents real friction that would reappear across callers.
- **G2 (Surface):** Is the proposed fix's surface minimal? Yes — all deepening candidates propose ≤7-item interfaces or one-line fixes.
- **G3 (Contract):** Does the abstraction add value? Yes — justified by multiple implementations, tested behavior, or direct navigation benefit.

Findings that would have failed (e.g., extracting a trait for a single-impl case) were excluded by the trait-with-one-impl check.

---

## Grill-me self-challenge

**Recall:** What is the single root cause behind the two highest-severity findings (H1 + H2)?
**Mechanism:** Both are **error-kind erasure** — H1 erases it at the source (`String` return type), H2 erases it at the boundary (`McpToolError::internal(format!())`). The root cause is the same: domain errors with distinguishable variants are flattened to a single type before the consumer can classify them. The fix is the same pattern (per-domain `map_*_error`), already proven in 6 of the 11 MCP servers. The remaining 5 servers + the swarm local-runtime layer are the laggards, not architectural outliers.

---

## Pragmatic-cybernetics loop analysis

The `check-string-errors.sh` → `Result<_, String>` → `McpToolError::internal` chain is a broken feedback loop:
- **Polarity:** Corrective (the check is supposed to surface the anti-pattern)
- **Delay:** High — the check runs locally (not CI), so the signal doesn't reach PR review
- **Gain:** Low — the check reports but doesn't block merges, so the anti-pattern accumulates
- **Closure:** Broken — `check-string-errors.sh` runs but its findings aren't promoted to CI gates
- **Fidelity:** Medium — the check catches `Result<_, String>` but not `McpToolError::internal(format!())` (the downstream symptom)

**Recommendation:** Promote `check-mcp-tool-tests.sh` to CI (advisory → blocking) to close the loop on error classification.

---

## Recommended priority (no implementation — survey only)

1. **H1 + H2** (error-kind erasure) — highest leverage, proven pattern, self-contained
2. **H3** (swarm_panel split) — zero-risk extraction of tested pure functions
3. **H4** (condenser dep extraction) — highest locality, one file moves
4. **H5** (SkillExecPort trait) — root cause for downstream String flattening
5. **M1** (widget trait) — highest-value if adding new viz types is planned
6. **L1 + L5** (stale comment + D7 test) — trivial one-line fixes

**No code was modified in this phase.** All findings are survey output for the user to prioritize.