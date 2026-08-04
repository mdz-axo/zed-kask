# Phase C — Refactor-Architecture Survey (Post-Refactor Codebase)

Date: 2026-08-04. Read-only survey of kask-owned crates after the H1–H5 / M1–M8 refactor batch.

**Verdict: the prior refactors are net-positive.** Every target landed as designed; dependency direction is clean; new modules are predominantly deep. Remaining friction is mostly pre-existing and made *more visible*, not worse.

## Deletion test on new modules

| Module | Verdict |
|--------|---------|
| `swarm_panel/src/parse.rs` (511 lines, 16 tests) | Deep — only cleanly-testable parsing surface |
| SwarmServer 5 tool files (cloud 1635 / local 950 / knowledge 172 / lifecycle 106 / a2a 105) | Deep — each owns a `#[tool_router]` arm set |
| `crates/hkask-viz-core` (342 lines) | Deep — exemplar: tiny interface (`block_renderer` + `VizWidget`), large hidden complexity (LRU cache, dispatch) |
| `marketplace_ui_common/src/panel_button.rs` (92 lines) | Borderline-shallow but justified — 2 real consumers, not speculative |
| `swarm_panel/src/tool_invoker.rs` (40 lines) | Deep enough — 2 consumers per `.rules` |

## Other checks

- **Error mappers**: same shape, genuinely different content per domain — correct per the `.rules` classify-per-variant trap. No shared helper warranted for domain mappers.
- **Dependency direction**: `hkask-types` is a leaf; `hkask-condenser` no longer references `hkask-mcp-server` (H4 fully resolved). No cycles.
- **Interface width**: LocalSwarmError (7), SkillExecError (3), ProvisionError (4), InputValidationError (1), VizWidget (6) — all ≤7. New error types carry fewer public items and more information than the String APIs they replaced.
- **VizWidget trait**: 4 impls + documented non-impl; exists *because* impls share an identical pattern — the anti-"trait-with-one-impl" done right.
- **PanelToggleButton**: generic only over the action type — forced by the type system, not gratuitous.

## Findings

### Medium

**M1 — `swarm_panel.rs` remainder is still a 4,720-line shallow-aggregator.** 78 functions, 15+ sub-structs, 5 render-mode methods, DTOs at L368–L482.
*Remedy:* move response DTOs into `parse.rs` (cohesion win), then extract `render_author`/`render_compose` + their forms into `author.rs`/`compose.rs`. Mechanical.

**M2 — `map_join_error` duplicated verbatim across two servers.** `hkask-mcp-research:113` and `hkask-mcp-companies:196` (the latter adds a `context` param).
*Remedy:* promote the companies version into `hkask-mcp-server` next to the shared `map_io_error`; delete both local copies. Meets the two-consumer port-promotion bar.

### Low

- **L1 — `lifecycle_tools.rs` is a misnomer**: holds fund/balance/history (local ledger/wallet); agent lifecycle lives in `cloud_tools.rs`. Rename to `ledger_tools.rs`.
- **L2 — `cloud_tools.rs` (1,635 lines / 27 tools)** could split along documented seams (catalogue / spend-gated / publish). Isolating the spend group shrinks the consent-gate review surface. Optional.
- **L3 — corpus has 4 inline `internal(format!` backend-error sites** (`hkask_mcp_corpus.rs:379,396,413,425`); classification may be correct but they're unclassified pass-throughs. Add `map_corpus_backend_error` only if the backend enum has distinguishable variants.
- **L4 — `local_tools.rs` at 950 lines** could halve via a 9-tool local-registry CRUD extraction. Optional.

## Validation

Read-only; no builds run. Executable pins exist (`tool_surface_is_exactly_50_registered_tools`, `viz_factories_cover_four_widgets`, 16 parse tests) but were not executed in this pass.
