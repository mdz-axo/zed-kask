---
title: "ADR: Split hkask-types into core primitives and domain types"
audience: [architects, developers, agents]
last_updated: 2026-08-04
version: "0.33.0"
status: "Draft"
domain: "architecture"
mds_categories: [composition, lifecycle]
---

# ADR: Split `hkask-types` into core primitives and domain types

**Status:** Draft — not yet decided. This ADR records the decision context and options so the split is not undertaken lightly or reversed silently. No code moves until a consumer-dependency audit (graph-audit semantic mode) confirms the chosen option is cycle-free.

## Context

`hkask-types` is the dependency root of the hKask crate tree — depended on by every `hkask-*` crate, `kask_bridge`, and all 11 MCP servers (24+ consumers). It currently exposes **~197 public items** across 34 files.

Its declared purpose has drifted from its actual surface:

- The original `Cargo.toml` `description` was *"ID types, nu-event, and visibility types for hKask"* — covering only the core-primitive layer (~60 items: `id`, `error`, `event`, `visibility`, `time`, `crypto`). This was corrected (2026-08-02) to *"Foundation types for the hKask platform — IDs, errors, events, visibility, hexagonal ports, and shared domain primitives"*, but the correction documents the drift rather than resolving it.
- The remaining items split into the **port traits** (~32 items: `ports/*` hexagonal seams) and **domain types** (~105 items) pushed into `hkask-types` to break circular dependencies: `loops/*` (moved from `hkask-regulation` — see the `loops/mod.rs` doc: *"moved to `hkask-types` to break the circular dependency that prevented extracting Regulation subcrates"*), `wallet_types`, `voice`, `transcript`, `curator`, `corpus`, `document`, `template`, `skill`, `inference_ipc`, plus `agent_paths`, `keychain_keys`, `json_extract`, `macros`, `server_config`, `goal`, `tool_taint`.

The result is a crate that is, in effect, three buckets wearing one name: a **core primitive layer** (~60 items), the **port traits** (~32 items), and a **domain layer** (~105 items). (Counts are approximate; the audit fixes them.) Symptoms of the multi-purpose shape:

1. Three wildcard re-exports (`pub use ports::*`, `pub use core::*`, `pub use webid::*`) — surface bloat from trying to expose everything from one root.
2. The README and `description` repeatedly under-stated the surface (stale `agent_registry`/`NuEvent` references removed 2026-08-02).
3. Domain types that *belong* with their domain crate (e.g. `wallet_types` logically lives with a wallet domain; `loops` with regulation) are stranded in the foundation crate.

The cycle-break that motivated the consolidation may no longer be necessary: the Regulation subcrates that were blocked by the cycle have since been extracted, and `hkask-types` explicitly forbids depending on `hkask-capability` (the original cycle source). Whether the domain types can now return to their owning crates — or whether a clean two-crate split is the safer move — is the decision this ADR records.[^evans-ddd]

## Decision

**Audit complete (2026-08-02); execution pending approval.** Three options were on the table; the consumer-dependency audit (results below) determined which are cycle-free. The informed recommendation is a hybrid (see Recommendation). This ADR exists so the trade-offs are visible before any move.[^evans-ddd]

### Option A — Split into two crates: `hkask-types-core` + `hkask-types-domain`

- `hkask-types-core` (~92 items): the core primitives (`id`, `error`, `event`, `observable_span`, `visibility`, `time`, `crypto`, `secret` — ~60 items) plus the `ports/*` hexagonal seams (~32 items), since the ports are the dependency-inversion abstractions every consumer binds to. (A third `hkask-types-ports` crate is an alternative if the port surface grows; default proposal keeps ports in core.)
- `hkask-types-domain` (~105 items): `loops`, `regulation`, `curator`, `wallet_types`, `voice`, `transcript`, `corpus`, `document`, `template`, `template_type`, `skill`, `tool_taint`, `inference_ipc`, `server_config`, `goal`, `agent_paths`, `keychain_keys`, `json_extract`, `macros`.
- Consumers needing only IDs/errors/ports import `hkask-types-core`; consumers needing domain types import `hkask-types-domain`. `hkask-types` becomes a thin facade re-exporting both for backward compatibility during migration.

### Option B — Push domain types back to their owning crates

- Each domain bucket returns to its owning crate now that the blocking cycle is gone: `loops` → `hkask-regulation`, `wallet_types` → a wallet domain crate (or `hkask-ledger`), `voice`/`transcript` → media, `corpus`/`document` → corpus, `curator` → curator, `template`/`template_type`/`skill` → `hkask-templates`, `inference_ipc` → `hkask-inference`, `server_config` → `hkask-mcp-server`, `goal` → regulation.
- `hkask-types` shrinks to ~92 core+port items (or ~60 if ports also separate), finally matching its declared purpose. No second crate.
- **Best locality** (types live with the domain that owns them) but **highest churn** and **re-introduces cycle risk** — the exact failure mode that motivated the original consolidation. Feasible only if the audit proves each move is acyclic.

### Option C — Keep as-is (one crate), accept the dual purpose

- No code move. The corrected `description` and README (2026-08-02) document the actual surface; the wildcard re-exports remain.
- Zero churn, zero risk, but the crate keeps its "two crates in a trenchcoat" shape and the locality debt persists.

## Trade-offs

| | Option A (two crates) | Option B (push to owners) | Option C (status quo) |
|---|---|---|---|
| Locality | medium — domain types grouped, not with owners | high — types live with their domain | low — all domain types stranded in foundation |
| Cycle risk | none (core has no domain deps) | **high** — must re-audit each move | none |
| Churn | medium — 2 crates, facade for back-compat | high — many crate moves + import updates | none |
| Reversibility | moderate (merge back) | hard (many moves to undo) | trivial |
| Matches declared purpose | partial (core does; domain crate doesn't) | yes (core shrinks to purpose) | no (purpose doc admits the drift) |
| Wildcard re-exports | can be scoped per-crate | removed (types leave) | remain |[^ousterhout]

## Audit results (2026-08-02)

The consumer-dependency audit is complete (graph-audit semantic mode, manual grep-based; consumers verified via actual `use hkask_types::` imports, not type-name greps, to avoid common-word false positives like `Signal`/`Deviation`). A move of bucket -> owner is cycle-free iff the owner's transitive dependency closure contains no consumer of the bucket.

**Cycle-free (viable Option-B moves):**
- `loops`, `regulation`, `curator`, `goal` -> `hkask-regulation`
- `wallet_types` -> `hkask-storage` (zero new edges: all three consumers already depend on storage)
- `template_type`, `skill` -> `hkask-templates` (sole consumer each)
- `tool_taint` -> `hkask-capability` (capability-closure = {types})
- `inference_ipc` -> `hkask-inference` (kask_bridge already deps inference)
- `keychain_keys` -> `hkask-keystore` (kask_bridge already deps keystore)

**Internalize into single-consumer MCP server (no external edge):** `voice` + `transcript` -> `hkask-mcp-media`; `document` + `corpus` -> `hkask-mcp-corpus`. (For `transcript`: media already imports from `hkask-types`; the dead local duplicate was deleted 2026-08-02 in T1.2a.)

**Cycle (stays core):** `template` (`LLMParameters`/`TemplateFile`) -> `hkask-templates` cycles because templates depends on `hkask-guard` and guard uses `LLMParameters`. `LLMParameters` is a foundational config primitive (11 consumers), not a domain type — stays core.

**Dead code (delete):** `server_config` is not root-re-exported, has zero module-path imports, and no test/doc references. Deletion held while another agent is active in `hkask-types`.

**Stay core (foundational):** `id`/`error`/`event`/`observable_span`/`visibility`/`time`/`crypto`/`secret`, `ports` (hexagonal seams, 16 consumers), `agent_paths` (path primitives), `json_extract` (utility), `macros` (`enum_str_ops!`).

**Key correction to the original framing:** the worry that Option B re-introduces the cycle that motivated the original consolidation applies to *only* `template`, not to `loops` (the original consolidation's target). `hkask-storage` no longer imports `loops` — that cycle is already broken — so `loops` can return to `hkask-regulation`.[^fowler-strangler]

## Recommendation

**Execute the hybrid revealed by the audit** (not pure A or B as originally framed): Option-B moves for the 10 viable buckets, internalize the 4 single-consumer buckets, delete `server_config`, keep the foundational buckets in core. After the moves, `hkask-types` shrinks from ~197 to ~80 items, matching its declared purpose. Execute one domain per commit; each move gated on `cargo check` + `./script/clippy` for affected consumers. The audit gate is satisfied — execution may proceed when the active agent's `hkask-types` work lands.[^fowler-strangler]

## Consequences

- **If A:** `hkask-types` becomes a re-export facade; two new crates appear in `kask/crates/`; consumers update imports incrementally (facade keeps old paths working during migration). The `pub use ports::*` wildcard can be scoped to `hkask-types-core`.
- **If B:** ~105 items move across ~10 crates; `hkask-types` drops to ~92 core+port items; `kask/Cargo.toml` and many `Cargo.toml` deps shift; the `loops/mod.rs` "moved to break the cycle" doc is deleted (the move reverses). This is the highest-touch option and must be done one domain per commit (strangler-fig discipline).
- **If C:** no change; the corrected docs stand; the wildcard re-exports and stranded domain types remain as accepted debt.[^evans-ddd]

## Verification

- Whichever option is chosen: `cargo check` across all 24+ consumers, `./script/clippy` (per `.rules`), and the existing test suites (e.g. `hkask-types` id/visibility tests, `hkask-regulation` loop tests) must pass.
- For Option B specifically: a dependency-graph cycle check before and after each domain move (graph-audit semantic mode).
- A structural-pin test should assert the new crate(s)' public surface matches the intended core/domain split, so the split doesn't silently drift back.[^fowler-refactoring]

## Non-goals

This ADR does **not** decide the MCP server error consolidation. A separate analysis (2026-08-02) established that the MCP servers already map their errors onto the MCP wire type `McpToolError` (via `map_*_error` fns or inline `.map_err(|e| McpToolError::internal(e.to_string()))`), not onto `hkask_types::McpErrorKind`. The remaining error work is per-server structured-source preservation (replacing inline `McpToolError::internal(e.to_string())` with `From<E> for McpToolError` / `map_*` fns), not a blanket consolidation onto `McpErrorKind`. That is independent of this split.[^evans-ddd]

---

## Footnotes

[^evans-ddd]: Evans, E. (2003). *Domain-driven design: Tackling complexity in the heart of software*. Addison-Wesley. https://www.domainlanguage.com/ddd/
    Cited for the bounded context pattern — the core/domain type separation follows DDD's principle of keeping domain types within their owning bounded context.

[^ousterhout]: Ousterhout, J. (2021). *A philosophy of software design* (2nd ed.). Yaknymer Press. https://web.stanford.edu/~ouster/cgi-bin/book.php
    Cited for the deep-module trade-off analysis applied to the three split options (locality, cycle risk, churn, reversibility).

[^fowler-strangler]: Fowler, M. (2004). *StranglerFigApplication*. https://martinfowler.com/bliki/StranglerFigApplication.html
    Cited for the strangler-fig discipline of executing one domain move per commit with cycle-free verification.

[^fowler-refactoring]: Fowler, M. (2018). *Refactoring: Improving the design of existing code* (2nd ed.). Addison-Wesley. https://martinfowler.com/books/refactoring.html
    Cited for the structural-pin test pattern that prevents the split from silently drifting back.

## References

- `kask/docs/architecture/zed-host-architecture-plan.md` — D1–D20 seams, crate inventory.
- `kask/crates/hkask-types/src/loops/mod.rs` — the "moved to break the circular dependency" note that motivates this ADR.
- `kask/crates/hkask-types/Cargo.toml` — the `description` correction and the `hkask-capability` cycle guard.
- 2026-08-02 type-system refactoring analysis (essentialist + graph-audit + refactor-architecture skills).