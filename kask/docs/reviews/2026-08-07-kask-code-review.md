# zed-kask Kask Code Review — Cleanup, Codegraph Consolidation, Dependency-Graph Simplification

**Date:** 2026-08-07
**Scope:** `kask/crates/` (20 crates), `kask/mcp-servers/` (13 servers), zed-side kask crates, `crates/zed/src/main.rs` composition root
**Skills applied:** pragmatic-semantics, pragmatic-cybernetics, graph-audit, essentialist, grill-me, metacognition
**Baseline:** green (`cargo check` clean, 10 shell gates pass)
**Prior review:** `kask/docs/reviews/2026-08-06-debugging-and-improvement-plan.md` — R-1, R-2, R-3, R-5, R-7, R-8, R-9 **landed**; R-4, R-6 **open** (out of this review's scope — they pin zed-side disables, not kask codegraph health)

---

## Findings

### Theme A — Broken feedback loops in the dependency hygiene gate

#### A-1: `#![allow(unused_crate_dependencies)]` on lib roots suppresses the gate it claims to work around
**Force:** Prohibition (the attribute inverts its stated rationale)
**Evidence:** 25 lib-root files carry `#![allow(unused_crate_dependencies)]` with comments like *"Bin target — deps used in main.rs, lint checks lib target only"*. The rationale is inverted: a lib-root attribute suppresses the lint on the **lib** target it's meant to measure. Every `src/main.rs` is a 10–11 line `run().await` wrapper, so bin targets need no suppression either.
**Proven empirically:** removing the attribute from `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` yields `error: extern crate 'tokio' is unused in crate 'hkask_mcp_curator'`. CI runs `cargo machete` (job `deps` in `.github/workflows/kask-ci.yml`), which reports clean — it cannot see this class. `kask/scripts/check-unused-deps.sh` uses the nightly lint but is not wired into CI.
**Verdict:** Real. The gate is broken in both directions: the lint is suppressed where it should fire, and the CI fallback (`cargo machete`) cannot detect crate-level unused deps. Confirmed unused: `tokio` in `hkask-mcp-curator`, `hkask-mcp-condenser`, `hkask-mcp-codegraph`; `dotenvy` in `hkask-mcp-corpus` (test-only use).
**Status:** Partially acted on — `dotenvy` moved to `[dev-dependencies]` (see W-3). The 25 lib-root `allow` attributes and the `tokio` removals are **not acted on** (mechanical but voluminous; see plan W-1).

### Theme B — Stale `#[allow(dead_code)]` hiding live and dead code

#### B-1: 10 dead path accessors in `agent_paths.rs`
**Force:** Evidence (zero references repo-wide)
**Evidence:** `kask/crates/hkask-types/src/agent_paths.rs` — 10 `pub(crate)` + `#[allow(dead_code)]` fns with zero references anywhere: `agent_style_db`, `agent_wallet_db` (remnant of deleted `hkask-wallet`), `agent_gallery_dir`, `agent_documents_dir`, `agent_library_dir`, `agent_sessions_dir`, `agent_portfolios_dir`, `agent_artifacts_dir`, `agent_manifest_json`, `publish_artifact`. The dirs themselves are still created by `ensure_agent_dirs` + `AGENT_SUBDIRS` (live, used by `kask_bridge/src/identity.rs:239`).
**Verdict:** Real dead code. `publish_artifact` (63 lines) references `agent_manifest_json`, so both go together.
**Status:** **Acted on** — all 10 fns + `publish_artifact` deleted; test assertions referencing them removed (see W-4).

#### B-2: Stale `#[allow(dead_code)]` on live code in `regulation_policy.rs`
**Force:** Evidence (the allow suppresses a lint that would now correctly fire if code became dead)
**Evidence:** `kask/crates/hkask-regulation/src/regulation_policy.rs` — `extract_deficit_threshold` (L451), `classify_decision` (L467), `default_substitution_ladder` (L491) all carry `#[allow(dead_code)]` but are consumed by `cybernetics_loop.rs` (L42-43 import, L369/1032/1198 use sites). The `ProposedAction` struct (L94) carries `#[allow(dead_code)]` — its fields `target`/`action_type` are constructed 25+ times in the policy table but read only in tests; production dispatch reads only `reason`.
**Verdict:** The three fn-level allows are stale (the fns are live). The struct-level allow is **legitimate** — the fields are genuinely test-only (production reads only `reason`).
**Status:** **Acted on** — removed the 3 stale fn-level allows; kept the struct-level allow with an updated doc comment explaining the fields are documentation-of-intent (see W-5).

### Theme C — Duplicated code

#### C-1: Duplicated `map_portfolio_error`
**Force:** Guideline (DRY; canonical extractor exists)
**Evidence:** Byte-identical 6-line body at `kask/mcp-servers/hkask-mcp-portfolio/src/server.rs:27` (`pub`) and `kask/mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs:187` (private). `hkask-mcp-prediction-markets` already imports the shared one (`use hkask_mcp_portfolio::map_portfolio_error`). `hkask-mcp-companies` already depends on `hkask-mcp-portfolio` and `PortfolioError` is the same type (re-exported via the local `portfolio.rs` module).
**Verdict:** Real. Delete the companies copy, import.
**Status:** **Acted on** — local copy deleted, `use hkask_mcp_portfolio::map_portfolio_error;` added, unused `PortfolioError` import removed (see W-2).

### Theme D — Dependency-graph smells

#### D-1: MCP-server→MCP-server type coupling
**Force:** Hypothesis (build-order coupling; may be legitimate library-plus-server)
**Evidence:**
- `hkask-mcp-scenarios` → `hkask-mcp-prediction-markets` (for `types::MarketRecord`, `types::MarketStatus`, `types::ReliabilityTier`, `matcher::token_overlap`)
- `hkask-mcp-kata-kanban` → `hkask-mcp-swarm` (for `LocalDelegateResult`, `TaskSuccessVerdict`, `TaskSuccessProvenance`, `LazyLocalSwarmRuntime`, `LocalAgentRegistry`, `LocalAgentCard`, `LocalAgentCapabilities`)
- `hkask-mcp-companies` + `hkask-mcp-prediction-markets` → `hkask-mcp-portfolio`

Server binaries become build-order-coupled. Precedent exists for extraction: `kanban_wire.rs` was extracted to `hkask-types` (commit `87123bee55`).
**Verdict:** **Accept** with rationale. `hkask-mcp-portfolio` is a legitimate library-plus-server (the portfolio storage layer is reusable domain logic, not server-specific). The `scenarios → prediction-markets` and `kata-kanban → swarm` edges are tighter (server→server for wire types), but extracting them to `hkask-types` would grow the dependency root for 2 consumers each — the ADR's "move when a second consumer materializes" rule cuts the other way here (the second consumer is the server itself, not a third party). Defer until a third consumer appears or the build-order coupling causes a real problem.
**Status:** Not acted on (design decision; documented for the record).

#### D-2: `hkask_types::loops` cycle justification is stale — move to `hkask-regulation`
**Force:** Evidence (the cycle was broken by deleting the subcrates; sole consumer is `hkask-regulation`)
**Evidence:** `kask/crates/hkask-types/src/loops/mod.rs` says the types were moved out of `hkask-regulation` "to break the circular dependency that prevented extracting Regulation subcrates (storage guard, SLO, seam watcher)". All three are **deleted** (plan Appendix A.4). Sole consumer is `hkask-regulation` (via the re-export shim `kask/crates/hkask-regulation/src/types/loops/mod.rs`). The Draft ADR already ruled this Option-B move cycle-free. No external consumer imports `hkask_types::loops` or the root re-exported types (`LoopId`, `Signal`, etc.).
**Verdict:** Real. `loops` can return to `hkask-regulation`, deleting the `hkask-types` module and the re-export.
**Status:** **Acted on** — 5 files moved to `hkask-regulation/src/types/loops/`, shim updated to define locally, `hkask-types/src/loops/` deleted, root re-exports removed (see W-6).

### Theme E — Documentation drift (pragmatic-semantics: Specification vs Implementation)

#### E-1: `DIVERGENCE.md` references renamed crate `hkask-bridge-dublincore`
**Force:** Evidence (the crate was renamed; the doc names a nonexistent crate)
**Evidence:** `DIVERGENCE.md:80` lists `hkask-bridge-dublincore`; actual crate is `hkask-bridge-ontology` (per `kask/docs/reference/ontology-bridge.md` and `ls kask/crates/`). The "19 hKask crates" count is correct; only the name is stale.
**Verdict:** Real.
**Status:** **Acted on** — renamed in DIVERGENCE.md (see W-7).

#### E-2: `check-hkask-no-zed-deps.sh` references deleted `kask_panel` crate
**Force:** Evidence (the crate was deleted; the script's comments and denylist are stale)
**Evidence:** `kask/scripts/check-hkask-no-zed-deps.sh` L14-18 (comment), L41 (`ZED_CRATES` denylist), L44-46 (comment) all reference `kask_panel`, which was deleted (D10). Harmless (the denylist entry never matches) but stale.
**Verdict:** Real.
**Status:** **Acted on** — `kask_panel` removed from comments and denylist (see W-8).

#### E-3: `main.rs` kask pin test is mostly theater
**Force:** Guardrail (advertised invariant without enforcement)
**Evidence:** `kask_wiring_symbols_exist` (`crates/zed/src/main.rs:4193`) claims to pin "the 28 functional units" but references only F2, F3, F6, F9, F22, F23 — 6 of 26 declared. Most assertions are `std::any::TypeId::of::<...>()` (pins type existence, not wiring). F25 (`sync_kask_mcp_runtime_servers`) is unpinned. Per the `.rules` "advertised invariants need enforcement points" trap.
**Verdict:** Real. Either strengthen the pin or correct the doc comment.
**Status:** Not acted on (zed-side; out of this review's kask codegraph scope).

#### E-4: MCP-server count reconciliation
**Force:** Evidence (three sources, one was wrong)
**Evidence:** DIVERGENCE.md L81 says **13** — **correct** (matches `ls kask/mcp-servers | wc -l` = 13 and `BuiltinMcpServer` literals = 13). The plan §2.4's "12" claim was the error. MDS.md L486 also says "12 MCP servers" — stale.
**Verdict:** DIVERGENCE.md is correct; MDS.md is stale.
**Status:** Not acted on (MDS.md is a design doc; noted for the record).

### Theme F — `hkask-types` shape (Draft ADR, status: not decided)

#### F-1: ADR consumer re-verification — `goal` and `skill` types are dead
**Force:** Evidence (zero consumers)
**Evidence:**
- `goal` types: `GoalState` is re-exported from `hkask_types` root but **only referenced in doc comments** (`hkask-regulation/src/types/loops/channels.rs:44,46,54`). No actual code use. The `hkask-goal` crate was deleted; `Goal`/`GoalArtifact`/`GoalCriterion` removed; `GoalState` retained "for rusqlite FromSql/ToSql orphan rule" but has no consumer.
- `skill` types: `hkask_types::skill` — **zero consumers** anywhere in the repo.
- `voice` types: `hkask_types::voice` — **zero consumers** anywhere in the repo.
**Verdict:** `goal`, `skill`, and `voice` are dead code candidates (not move candidates). The ADR's recommendation to "move `goal` → `hkask-regulation`" is wrong — there's nothing to move because there's no consumer. These are deletion candidates.
**Status:** Not acted on (deletion of public API types needs a separate decision; noted for the record).

#### F-2: ADR consumer re-verification — moves that add new dependency edges
**Force:** Hypothesis (the move is feasible but adds edges)
**Evidence:**
- `regulation` types → `hkask-regulation`: would force `hkask-mcp-curator` and `hkask-memory` to add `hkask-regulation` deps. `hkask-memory` is **out of scope** (memory refactor). `hkask-mcp-curator` adding `hkask-regulation` is a new edge.
- `tool_taint` → `hkask-capability`: `hkask-regulation` would need to depend on `hkask-capability` (currently doesn't). New edge.
- `inference_ipc` → `hkask-inference`: `kask_bridge` already depends on `hkask-inference` ✓. `main.rs` would need `hkask-inference` dep.
- `keychain_keys` → `hkask-keystore`: `hkask-mcp-server` already depends on `hkask-keystore` ✓. `kask_bridge` already depends on `hkask-keystore` ✓.
- `template_type` → `hkask-templates`: consumers are `hkask-templates` (4 files). Feasible.
- `transcript` → `hkask-mcp-companies`: consumers are `hkask-mcp-companies` (2 files). Feasible.
- `document`/`corpus` → `hkask-mcp-corpus`: consumers are `hkask-mcp-corpus` (many files). Feasible.
- `template`/`LLMParameters` → keep in `hkask-types` (real cycle via `hkask-guard`, many consumers).
**Verdict:** The `loops` move (D-2) was the cleanest and is done. The `keychain_keys`, `inference_ipc`, `template_type`, `transcript`, `document`/`corpus` moves are feasible and add no new edges (consumers already depend on the target crate). The `regulation` and `tool_taint` moves add new edges and need evaluation.
**Status:** Not acted on (structural; see plan W-8 onward).

### Theme G — Inline `McpToolError::internal` classification (non-goal of ADR)

#### G-1: Widespread inline `internal` despite `.rules` trap
**Force:** Guideline (per-variant classification required)
**Evidence:** Counts: kata-kanban 20, corpus 19, prediction-markets 14, media 8, training 7, companies 6, swarm 6, condenser 5, codegraph 4, portfolio 3, curator 2, research 2, scenarios 1. Some carry `// rr0044-ok: mapper-fallback` (legitimate sanctioned fallbacks).
**Verdict:** Explicitly listed as a **non-goal** of the types-split ADR. Treat as separate work. Distinguish sanctioned fallbacks from unclassified ones before proposing work.
**Status:** Not acted on (out of scope).

---

## Implementation plan (ordered, independently shippable)

### Acted on in this session

#### W-1: (Not acted on) Remove `#![allow(unused_crate_dependencies)]` from 25 lib roots + remove unused `tokio`
**Files:** 25 lib-root files in `kask/crates/` and `kask/mcp-servers/`; `Cargo.toml` for `hkask-mcp-curator`, `hkask-mcp-condenser`, `hkask-mcp-codegraph` (remove `tokio`).
**Validation:** `RUSTFLAGS="--force-warn unused_crate_dependencies" cargo check --lib -p <crate>` for each; `./script/clippy`.
**Rollback risk:** Low — mechanical. If a crate genuinely needs `tokio` in lib, the check fails immediately and the dep is re-added.
**Why deferred:** Voluminous (25 files + 3 Cargo.toml edits); better as a focused PR.

#### W-2: Delete duplicated `map_portfolio_error` in `hkask-mcp-companies` ✅
**Files:** `kask/mcp-servers/hkask-mcp-companies/src/hkask_mcp_companies.rs`
**Change:** Deleted local `fn map_portfolio_error` (L186-191); added `use hkask_mcp_portfolio::map_portfolio_error;`; removed unused `PortfolioError` from `use portfolio::{...}` import.
**Validation:** `cargo check -p hkask-mcp-companies` — clean (0 warnings).

#### W-3: Move `dotenvy` to `[dev-dependencies]` in `hkask-mcp-corpus` ✅
**Files:** `kask/mcp-servers/hkask-mcp-corpus/Cargo.toml`
**Change:** Removed `dotenvy.workspace = true` from `[dependencies]`; added `dotenvy.workspace = true` to `[dev-dependencies]`. Sole use is `ocr/decimation.rs:564` inside `#[cfg(test)] mod tests`.
**Validation:** `cargo check -p hkask-mcp-corpus` — clean; `kask/scripts/check-unused-deps.sh` — no `corpus`/`dotenvy` hits.

#### W-4: Delete 10 dead path accessors + `publish_artifact` from `agent_paths.rs` ✅
**Files:** `kask/crates/hkask-types/src/agent_paths.rs`
**Change:** Deleted `agent_style_db`, `agent_wallet_db`, `agent_gallery_dir`, `agent_documents_dir`, `agent_library_dir`, `agent_sessions_dir`, `agent_portfolios_dir`, `agent_artifacts_dir`, `agent_manifest_json`, `publish_artifact` (63 lines). Removed test assertions referencing `agent_wallet_db`, `agent_gallery_dir`, `agent_sessions_dir`. Kept `AGENT_SUBDIRS` + `ensure_agent_dirs` (live).
**Validation:** `cargo test -p hkask-types --lib` — 108 passed, 0 failed.

#### W-5: Remove stale `#[allow(dead_code)]` from live fns in `regulation_policy.rs` ✅
**Files:** `kask/crates/hkask-regulation/src/regulation_policy.rs`
**Change:** Removed `#[allow(dead_code)]` from `extract_deficit_threshold`, `classify_decision`, `default_substitution_ladder` (all consumed by `cybernetics_loop.rs`). Kept `#[allow(dead_code)]` on `ProposedAction` struct with updated doc comment (fields `target`/`action_type` are test-only; production reads only `reason`).
**Validation:** `cargo check -p hkask-regulation` — clean (0 warnings).

#### W-6: Move `loops` module from `hkask-types` to `hkask-regulation` ✅
**Files:**
- Moved: `kask/crates/hkask-types/src/loops/{actions,core,episodic,signals}.rs` → `kask/crates/hkask-regulation/src/types/loops/`
- Deleted: `kask/crates/hkask-types/src/loops/` (entire directory)
- Updated: `kask/crates/hkask-regulation/src/types/loops/mod.rs` (defines modules locally, no re-export from `hkask-types`)
- Updated: `kask/crates/hkask-types/src/hkask_types.rs` (removed `pub mod loops;` and root `pub use loops::{...}`)
**Validation:** `cargo check -p hkask-types -p hkask-regulation -p hkask-mcp-server -p kask_bridge` — clean; `cargo test -p hkask-types --lib` (108 passed) + `cargo test -p hkask-regulation --lib` (85 passed, including moved `types::loops::tests`); `kask/scripts/check-hkask-no-zed-deps.sh` — OK; `kask/scripts/check-reg-canonical.sh` — OK; `kask/scripts/check-mcp-servers.sh` — OK; `kask/scripts/check-string-errors.sh` — OK; `kask/scripts/check-unsafe-forbid.sh` — OK.

#### W-7: Fix `DIVERGENCE.md` stale crate name ✅
**Files:** `DIVERGENCE.md`
**Change:** `hkask-bridge-dublincore` → `hkask-bridge-ontology` (L80).

#### W-8: Fix `check-hkask-no-zed-deps.sh` stale `kask_panel` references ✅
**Files:** `kask/scripts/check-hkask-no-zed-deps.sh`
**Change:** Removed `kask_panel` from comments (L14-18, L44-46) and `ZED_CRATES` denylist (L41).
**Validation:** `bash kask/scripts/check-hkask-no-zed-deps.sh` — OK.

### Not acted on (deferred / out of scope)

#### W-9: Remove `#![allow(unused_crate_dependencies)]` from 25 lib roots
See W-1. Mechanical, voluminous. One commit per crate-group.

#### W-10: Strengthen or correct `kask_wiring_symbols_exist` test
**Files:** `crates/zed/src/main.rs:4193`
**Change:** Either add assertions for the 20 unpinned functional units, or correct the doc comment from "the 28 functional units" to "6 key symbols (F2, F3, F6, F9, F22, F23)".
**Validation:** `cargo test -p zed kask_wiring_symbols_exist` + `./script/clippy`.
**Rollback risk:** Low.

#### W-11: Delete dead `goal`, `skill`, `voice` types from `hkask-types`
**Files:** `kask/crates/hkask-types/src/goal.rs`, `kask/crates/hkask-types/src/skill.rs`, `kask/crates/hkask-types/src/voice.rs` (verify paths); `kask/crates/hkask-types/src/hkask_types.rs` (remove module declarations + re-exports).
**Validation:** `cargo check -p hkask-types` + `./script/clippy` + grep for any missed consumers.
**Rollback risk:** Low — zero consumers. But `GoalState` is a public API type that may have external consumers (downstream forks); needs a decision.

#### W-12: ADR structural moves (feasible, no new edges)
- `keychain_keys` → `hkask-keystore` (consumers already depend on `hkask-keystore`)
- `inference_ipc` → `hkask-inference` (`kask_bridge` already depends; `main.rs` needs dep)
- `template_type` → `hkask-templates` (sole consumer)
- `transcript` → `hkask-mcp-companies` (sole consumer)
- `document`/`corpus` → `hkask-mcp-corpus` (sole consumer)
**Validation:** `cargo check` per affected crate + `./script/clippy`.
**Rollback risk:** Medium — each move touches multiple files. One commit per move.

#### W-13: ADR structural moves that add new edges (need evaluation)
- `regulation` types → `hkask-regulation` (adds `hkask-regulation` dep to `hkask-mcp-curator` and `hkask-memory` — **memory is out of scope**)
- `tool_taint` → `hkask-capability` (adds `hkask-capability` dep to `hkask-regulation`)
**Validation:** `cargo check` per affected crate + `./script/clippy`.
**Rollback risk:** Medium — new dependency edges.

#### W-14: Inline `McpToolError::internal` classification (non-goal)
Distinguish sanctioned `// rr0044-ok: mapper-fallback` sites from unclassified ones. Per-variant `map_*_error` fns. Separate workstream.

---

## Explicitly out of scope

- **In-flight memory refactor:** `kask/crates/hkask-memory/`, `kask/crates/kask_bridge/src/memory.rs`, `kask/crates/hkask-storage/src/hmem.rs`, and anything about `MemoryStore` / `HMemEntry` / `HMemOntology` / episodic-semantic consolidation.
- **Deferred memory-adjacent findings:** `AlertEscalationSink` (0 production impls; sole impl `BridgeAlertEscalationSink` is in `kask_bridge/src/memory.rs:637` — memory-refactor file). `hkask-mcp-server` → `hkask-memory` framework→domain inversion (`map_memory_store_error`, `server/validation.rs:139` — memory-adjacent).
- **Pre-existing uncommitted changes:** `hkask-mcp-companies` and `hkask-mcp-scenarios` (per `git status`) — not mine, left alone.
- **Prior review R-4, R-6:** `run_skills_scan` pin test and `kask_extensions_ui` test module — zed-side disables, not kask codegraph health.
- **`ManifestExecutor` split:** 1,760 prod lines, 6 public methods — passes the deep-module test. Do not split.
- **Macro-driven traits:** `ToolContext` (generated by `mcp_server!` macro) — justified, keep.

---

## Suggested `.rules` additions

(Do not edit `.rules` inline — root `.rules` "Rules Hygiene" forbids drive-by additions. Reviewers decide what gets merged.)

### S-K1: Crate-root `#![allow(unused_crate_dependencies)]` on a lib target suppresses the gate it claims to work around

A `#![allow(unused_crate_dependencies)]` attribute on a **lib root** (the file named in `[lib] path` in `Cargo.toml`) suppresses the lint on the lib target it's meant to measure. The common comment rationale — *"Bin target — deps used in main.rs, lint checks lib target only"* — is inverted: the attribute is on the lib root, not the bin root, so it suppresses the lib check. Bin targets that are thin `run().await` wrappers (10–11 lines) need no suppression either — the lint doesn't fire on a bin that only calls `run().await` because the deps are used in the lib.

The failure mode is silent: `cargo machete` (the CI fallback) cannot detect crate-level unused deps, and the nightly lint is suppressed where it should fire. Found in 25 lib-root files across `kask/crates/` and `kask/mcp-servers/`; removing the attribute from `hkask-mcp-curator` immediately surfaced `tokio` as unused.

**Enforcement:** `RUSTFLAGS="--force-warn unused_crate_dependencies" cargo check --lib -p <crate>` overrides the `allow` and surfaces real unused deps. Do not add `#![allow(unused_crate_dependencies)]` to a lib root; if a dep is genuinely only used in tests, move it to `[dev-dependencies]` instead.

### S-K2: Stale `#[allow(dead_code)]` on live code hides the lint signal

A `#[allow(dead_code)]` attribute that was added when the code was genuinely dead, but is now consumed by production code, is stale — it suppresses the lint that would correctly fire if the code became dead again. The failure mode is the opposite of the usual dead-code trap: the lint is silenced, so a future refactor that removes the consumer produces no warning. When you wire a previously-dead function into production, remove the `#[allow(dead_code)]` in the same commit. Found in `regulation_policy.rs` — `extract_deficit_threshold`, `classify_decision`, `default_substitution_ladder` all carried stale allows despite being consumed by `cybernetics_loop.rs`.

**Exception:** a struct whose fields are constructed but only read in tests (not production) legitimately carries `#[allow(dead_code)]` — the fields are documentation-of-intent. Document this in the doc comment so the allow is not blindly removed. Found in `regulation_policy.rs::ProposedAction` — fields `target`/`action_type` are constructed 25+ times in the policy table but production dispatch reads only `reason`; the allow is legitimate with an explanatory doc comment.

### S-K3: ADR consumer sets must be re-verified before proposing moves

A Draft ADR's consumer-set audit can be stale by the time implementation begins — types are deleted, consumers are added, modules are renamed. The `.rules` trap "Convention priors drawn from .rules must be verified against the codebase" generalizes to ADR audits: before proposing a move, grep for actual consumers. Found in the `hkask-types` shape ADR: `goal` types were recommended to move to `hkask-regulation`, but `GoalState` has zero code consumers (only doc-comment references) — there's nothing to move. `skill` and `voice` types have zero consumers and are deletion candidates, not move candidates. The ADR's "move `goal` → `hkask-regulation`" recommendation was wrong because the consumer set changed between the audit (2026-08-02) and this review (2026-08-07).
