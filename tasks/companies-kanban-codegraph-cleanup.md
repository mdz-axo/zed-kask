# Companies & Kata-Kanban Codegraph Cleanup — Review and Step Design

Date: 2026-09-27. Status: **proposal for operator approval** — nothing below is executed except the two priority items recorded at the end.

## Scope and method

Servers: `hkask-mcp-companies` (25,429 src lines), `hkask-mcp-kata-kanban` (7,475). Skills and their interactions: `company-research-deep`, `company-research-flash`, `listening`, `wardley-mapper`; the shared `kask/registry/templates/company-research/` registry (21 templates); the kanban panel and the goal loop as consumers.

Lenses applied: refactor-architecture (discover → rank → walk → audit → strangler-fig → verify), essentialist (Exist/Surface/Contract with constraint-force labels), falsifiability (every finding carries its killing check; eliminated hypotheses recorded, not averaged), hypothesis-framer (FINER/PICO framing of the review questions), grill-me (escalating challenge; weak findings killed before reaching this plan), metacognition (predictions recorded before the review, scored after). **`capabilities-reasoner` was removed as a skill on 2026-09-24** — its discipline (name the capability each module claims, probe whether it is exercised) is folded into the deletion-test axis and labeled as a substitution, not an invocation.

## Current condition (the graph)

- **companies**: `providers.rs` 2,544 · `screening.rs` 2,427 (saved-screen lifecycle) · `acquisition_tests.rs` 3,518 (`#[cfg(test)]`) · `tools/` tree (valuation 1,556, analysis 1,291, expectations 589, economic_profit 608, analytics 588) · `screener.rs` 1,251 (prompt parser) · `research_store.rs` 1,137 (screen-job persistence) · `financial_model/` tree (driver_model 1,027) · `research.rs` 669 (multi-provider search engine — alive, called by tools/analysis and tools/expectations) · service modules (economic_profit, valuation_service, forecast, superforecast, transcript, data_quality, learning, fibo_cache).
- **kata-kanban**: router `hkask_mcp_kata_kanban.rs` 1,808 · `service_impl/` tree (service 963, goals 906, tests 1,114, kata 190, comments 55, spawn 33, **phases — empty**) · `types.rs` 728 · `idempotency.rs` 359 · `mermaid.rs` 343.
- **Template registry**: 21 company-research templates. Live consumption: 13 (deep renders 10 — company-8part, falstaffian, wardley-anchor, gorilla-4dim, gorilla-capability, hidden-champions, imagine, industry-outside-view, thesis, evidence-review; flash renders 4 — intel-mosaic, intel-semantic-classify, kata-calibration-measure, evidence-review shared). **Dead: 8.**

## Findings

### F1 — Dead template surface with a live misinformation hazard (Prohibition)

**Claim:** 8 of 21 company-research templates are rendered by no skill: `scout-alpha-score`, `forensic-pre-screen`, `critical-factor`, `forensic-full`, `valuation-8step`, `communication-enter`, `lens-five-frameworks` (flash-side) and `economic-trajectory` (deep-side).
**Check:** render-reference sweep over both SKILL.mds plus Rust trees; step-body reads.
**Evidence:** flash's step bodies inline the full methodology with the deterministic `lisp_eval` forms (scout's alpha weights, valuation's four forms, ENTER's gate form, LENS's five frameworks) — the prose is the live copy. `economic-trajectory.j2` has already *diverged* from its step (the step carries the strategy-literature probe; the template does not) — divergence is the proof of staleness. The hazard is live: both skills' execution-order lines still say "Render each named synthesis template," both Registry tables document the dead ones, and the how-to-render example ref in both skills points at `scout-alpha-score` — a dead template.
**Grill record:** the defense "templates are structured reference versions of the same content" (both SKILL.mds' constraint) was raised and killed — reference copies that have diverged are misinformation, and 13 templates *are* rendered, so the reference-only reading never applied uniformly.
**Fix (Step 1 below).**

### F2 — Empty placeholder module (Prohibition)

**Claim:** `kata-kanban/src/kanban/service_impl/phases.rs` has zero callers and no content.
**Check:** the file's own header: "Phase management methods were removed — board_add_phase was dead code… The module is retained for future re-introduction."
**Verdict:** speculative retention — the essentialist G1 failure self-declared. Delete the module and its `mod` line.

### F3 — Naming collision: two unrelated capabilities both called "research" (Guideline)

**Claim:** `research.rs` (multi-provider search engine) and `research_store.rs` (saved-screen job persistence) share a name for unrelated capabilities.
**Check:** module headers and caller sets (search engine: tools/analysis, tools/expectations; store: screening/screener).
**Fix:** rename `research_store` → `screen_store` (kills the collision at minimal churn).

### F4 — Screening capability split across three top-level modules (Guideline, optional)

**Claim:** one capability (screening) lives in `screener.rs` (parse) + `screening.rs` (lifecycle) + `research_store.rs` (persistence), ~4,800 lines.
**Grill record:** all three are alive with distinct callers — this is organization, not dead code. Consolidating under a `screening/` tree (parser/lifecycle/store) is navigability value at medium churn. Operator call; the F3 rename delivers most of the value alone.

### F5 — Task criteria text unreachable (Guardrail)

**Claim:** `TaskInfo` carries `criteria_count` without the criteria text — the same class as the goal-list gap fixed today.
**Fix:** the same three-line pattern (field + population + test).

### F6 — Redundant cross-documentation (Guideline)

Deep's Registry table documents flash-only templates that flash's own table also documents. Trim after F1 (the dead rows disappear; live flash-only rows get documented once).

### Eliminated / killed findings (the falsification log)

- **K1** "in-src test modules compile into the binary" — ELIMINATED: `acquisition_tests` and `forecast_loop_tests` are `#[cfg(test)]`-gated (verified).
- **K2** "duplicated error-classification belongs in hkask-mcp-server" — ELIMINATED: shared helpers exist (`map_io_error`, `map_join_error`, `map_infra_error`) and are adopted (companies uses `map_join_error`); the domain mappers (`map_kanban_error`, `map_scenario_impact_error`) are legitimately domain-specific.
- **K3** "companies types.rs (786) should be split" — KILLED at grill: churn without interface gain.
- **K4** "kata-kanban router (1,808) should be split" — KILLED: leaf-crate churn warning (.rules); the router is the tool surface.

## The cleanup step (strangler-fig sequence — each step leaves the tree green)

1. **Template dead-surface removal (F1).** Per template: diff the `.j2` content against its step prose to confirm nothing unique is lost → delete the file → remove its Registry-table row → fix the two how-to-render example refs to a live template → fix flash's execution-order line to name the four actually-rendered templates. Verify: `all_registry_templates_conform` passes on the reduced walk; live render probes on the four live flash templates; full-repo sweep per deleted name (code + docs). Expected net: ≈ −8 files, several hundred lines.
2. **`phases.rs` deletion (F2).** `cargo check -p hkask-mcp-kata-kanban` + test run; sweep for `phases` references.
3. **`research_store` → `screen_store` rename (F3).** Mechanical rename + `acquisition_tests` doc-comment path updates; full build + sweep.
4. **`TaskInfo` criteria text (F5).** Same pattern as today's goal fix, plus a test asserting the text round-trips.
5. **Optional consolidation (F4 + F6)** — only on operator approval.

Every step ends with the .rules removal protocol: full-repo symbol sweep over code and docs, full build, net lines reported.

## Metacognition closure (predictions scored)

- **P1** "companies server carries ≥2 flash-only/dead modules post-rebuild" — **half-corroborated**: the dead surface is real but lives in the template registry, not the server's Rust modules. The prediction named the wrong layer; that miss is the informative one.
- **P2** "≥1 duplicated envelope/error-classification pattern" — **eliminated** by the adoption check (K2).
- **P3** "≥1 service_impl subtree with no external production callers" — **corroborated**, and worse than predicted: `phases.rs` is empty.

## What was NOT checked

- Per-template content diffs (Step 1's pre-deletion gate — not yet run).
- Runtime performance profiling — this review is graph shape, not performance.
- Zed-side readers of the goal-list JSON (criteria_count retained for compatibility; the new field is additive).
- The corpus/listening template namespaces (out of scope).
- Live JSON verification of the new criteria field — requires rebuild + restart.

## Executed remediation (2026-09-27, operator-ruled) — supersedes F1's original verdict

The operator halted the deletion batch and required a semantic review ("dead from a process gap vs dead from redundancy"). The contract-diff review — output contracts vs step emit lists, plus consumption-edge traces — superseded the original render-reference-count verdict:

- **REDUNDANT (deleted)**: `critical-factor`, `communication-enter` — full contract parity with the surviving step prose.
- **NEAR-REDUNDANT (semantics folded into the step prose, then deleted)**: `scout-alpha-score` (research_query, alpha_components), `forensic-full` (severity-revision authority), `economic-trajectory` (trajectory_velocity; the step keeps the literature probe and MAIA anchors as the richer carrier).
- **PROCESS GAP (render calls RESTORED — live again)**: `forensic-pre-screen` (the severity-scaled eps_haircut/dr_add_bps chain VALUATION consumes), `valuation-8step` (forensic_profile, cf_scenarios bear case, downstream_mode operating mode, lens_tensions re-entry), `lens-five-frameworks` (lens_scores, lens_findings, recommendations, per-lens checklists).

Root cause: the flash rebuild moved the lisp forms into step prose but not the stage contracts — the steps became lossy summaries while "render each named template" survived with nothing to render. The original pre-deletion gate checked three hand-picked content items instead of diffing contracts.

Also executed per the approved plan: `phases.rs` empty placeholder removed; `research_store` → `screen_store` rename (module, file, 7 reference files; the `ResearchStore` struct name remains a named follow-up); `TaskInfo` criteria text exposed with a round-trip test; README template counts corrected (275 `.j2` / 2 `.jinja` / 56 namespaces — both stale spots fixed).

**Proposed .rules addition (semantic-first carrier changes):** before deleting or de-rendering any skill carrier (template, step, tool output), produce the concept→carrier map — (1) diff the carrier's output contract against every surviving carrier's emit list, (2) diff the framework/threshold inventory, (3) trace the consumption edges (who reads this output, what it feeds). A carrier is removable only when every semantic element maps to a surviving carrier. Render-reference counts are a syntax signal, never a deadness verdict. (Observed 2026-09-27: 8 templates judged dead on render-refs; the contract diff found 3 with missing output fields and 3 with lost consumption edges.)

Validation: kata-kanban 66+5+29+1 green; companies 146+25 green; agent render_template_tool 21/21; research_store residue zero; template census 275/2/56.

## Earlier pass (priorities 2 and 3, executed before the semantic review)

- **Priority 2 (criteria directive):** `GoalInfo.criteria: Vec<String>` added and populated in `kanban_goal_list`; `cargo test -p hkask-mcp-kata-kanban` green (66+5+29+1 batches). Live after next rebuild/restart.
- **Priority 3 (present-map contract):** wardley-mapper SKILL.md now documents the pure-render context (`map_diagram`, `recommendations` object shape, `rationale`) — the silent-N/A class is closed at the doc boundary.
