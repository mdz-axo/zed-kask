# Plan: Correct bugs and build missing capabilities in `hkask-graph-widget`

**Creator:** agent session (metacognition → hypothesis-framer/grill-me/falsifiability/lean-prover assessment → task-breakdown/essentialist/pragmatic-semantics/pragmatic-cybernetics plan)
**Date:** 2026-08-06
**Document type:** `bibo:Document` (PKO `pko:Procedure` targeting `pko:ProcedureTarget`: a correct, honest, and algorithmically more complete graph reasoning widget)

## Overview

The graph reasoning widget (`crates/hkask-graph-widget/`) renders ```` ```graph ```` fenced blocks as a Bayesian event-tree DAG. An assessment using hypothesis-framer, grill-me, falsifiability, and lean-prover found the widget is **correct for its declared subset** (single-`depends_on[0]`, independent parents, binomial events, forward marginalization) but has:

- **One real Prohibition** (silent fallback on `>20` parents with no `warn!` — violates `.rules` "silent fallback on a computed value")
- **Two Guardrails** (schema lies about multi-dep support; `joint_probability` goes stale after evidence; `certainty_tier` duplicated across crates)
- **Three Guideline capabilities** the Bayesian-network canon requires and the widget lacks (backward inference, soft evidence, multi-dependency conjunctive conditions)

Two initially-identified "gaps" were **dropped after essentialist interrogation**: sensitivity analysis / value-of-information (no consumer today — future feature) and provenance attribution on inferred marginals (no consumer — future feature). One initially-identified "bug" (panic on short `conditionals`) was **falsified against the code**: `marginalize` uses `conditionals.get(assignment)` and missing entries contribute 0, pinned by `marginalize_missing_conditionals_contribute_zero`. The plan self-corrects.

## Architecture decisions

1. **Prohibition → Guardrail → Guideline ordering** (from pragmatic-semantics constraint-force hierarchy). Correctness first, then honesty, then capabilities.
2. **Polytree-only backward inference** (Pearl 1988 π-λ message passing). Exact, $O(n)$ on singly-connected DAGs. Multiply-connected DAGs fall back to forward-only with a visible notice + `tracing::warn!`. Junction-tree for multiply-connected is a separate future plan.
3. **Multi-dep semantics: joint conditional table per `depends_on` entry, combined by independence (product).** Matches what `scenario_quantify` already emits. No noisy-OR.
4. **Soft evidence as a new click action** ("observe with likelihood"), not a mode toggle. Hard evidence (current click-to-set-0.9/0.1) stays the default. `EvidenceKind = Hard(f64) | Soft(likelihood_ratio: f64)`.
5. **`certainty_tier` de-duplication** in the kask server crate (no D-seam concern — `hkask-mcp-scenarios` is ours).
6. **S4 validation layer** (cybernetics): `validate_conditionals` called from `recompute_marginals` so the math boundary emits a signal, not just `layout.rs`.

## Phased task list with checkpoints

### Phase 1 — Correctness (Prohibition)

- **T2.** Warn on `>20 parents` silent fallback in `propagate.rs` — XS
- **Checkpoint 1:** `cargo test -p hkask-graph-widget` — all pass, warn fires on high-fan-in.

### Phase 2 — Honesty (Guardrails)

- **T3.** Validate conditional tables at the math boundary (S4 layer) — S
- **T4.** Make `depends_on` schema honest: implement multi-dep conjunctive conditions — M
- **T7.** Recompute `joint_probability` after evidence — S
- **T8.** De-duplicate `certainty_tier` across `hkask-forecast` and `hkask-mcp-scenarios` — XS
- **T9.** Delete stale TODO in `propagate.rs` — XS
- **Checkpoint 2:** `cargo test -p hkask-forecast && cargo test -p hkask-graph-widget && cargo test -p hkask-mcp-scenarios` — all pass. Schema honest, validation fires, no duplication, no stale TODO.

### Phase 3 — Capabilities (Guidelines)

- **T5a.** Polytree detection + backward inference core (Pearl π-λ) — M
- **T5b.** Backward inference fallback + view integration — M
- **T6.** Soft evidence mode (likelihood ratios) — M
- **Checkpoint 3:** Full `cargo test` across the three crates. Manual smoke: 3-node chain, set evidence on leaf, observe root marginal update (backward). Set soft evidence, observe Bayesian update. Compare two branches, observe recomputed joint probability.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Backward inference requires a different algorithm (belief updating), not a tweak to forward marginalization | High — T5 could balloon | Scope T5 to polytree belief updating (Pearl 1988). Defer multiply-connected (junction tree) to a future plan. State the limit in the AC. |
| Multi-dep semantics are ambiguous (AND? OR? noisy-OR?) | Medium — T4 design deadlock | Pin to joint conditional table per `depends_on` entry, combined by independence (product). Matches `scenario_quantify` output. |
| Soft evidence + hard evidence conflict on the same node | Medium — T6 design | `EvidenceKind` enum per node. Backward inference fires for both; forward-only falls back for hard evidence on a leaf (current behavior preserved). |
| `certainty_tier` de-dup touches the server crate (cross-crate) | Low — `.rules` flags cross-crate constant duplication | T8 makes the server import `hkask_forecast::certainty_tier`. Test in both crates. |
| Validation at the math boundary double-warns (layout + propagate) | Low — log noise | T3 has `propagate.rs` call `validate_conditionals` and emit a single warn per malformed node; `layout.rs` keeps its existing warn (different lifecycle — parse time vs propagate time). Acceptable: two signals for two different consumers. |

## Open questions (resolved)

1. **Backward inference scope:** polytree only, multiply-connected deferred. ✓ (decided)
2. **Multi-dep semantics:** joint conditional table per entry, combined by independence. ✓
3. **Soft evidence UI:** new click action, hard evidence stays default. ✓
4. **`certainty_tier` de-dup:** modify `hkask-mcp-scenarios` (kask crate, no D-seam). ✓

## Refinement history

- **PDCA iteration 1 (assessment):** Initial hypothesis-framer/grill-me/falsifiability/lean-prover run identified 10 gaps. Essentialist cut #9 (sensitivity analysis) and #10 (provenance attribution) — no consumer today. Hypothesis-framer PICO + falsifiability admissibility gate admitted the remaining 8.
- **PDCA iteration 2 (grounding):** Before writing tasks, re-verified T1 (panic on short `conditionals`) against the code. **Falsified:** `marginalize` uses `conditionals.get(assignment)`, missing entries contribute 0, pinned by `marginalize_missing_conditionals_contribute_zero` test. T1 dropped. This is the `.rules` trap "Convention priors drawn from .rules must be verified against the codebase" — the prior was stale. Plan corrected from 9 tasks to 8.
- **PDCA iteration 3 (sizing):** T5 was L → broken into T5a (polytree detect + core) and T5b (fallback + view). No task > M. Plan stable.
- **PDCA iteration 4 (T8 verification):** Before implementing T8 (de-duplicate `certainty_tier`), verified against the code. **Falsified:** `hkask-mcp-scenarios` `CertaintyTier::from_probability` already delegates to `hkask_forecast::certainty_tier` (types.rs L200-210, comment: "share one source of truth and cannot drift"). The server has a thin enum wrapper over the shared string — correct design, not drift. T8 dropped. Same `.rules` trap as iteration 2: inferred duplication from two grep hits without reading the bodies. Plan corrected from 8 tasks to 7.
- **PDCA iteration 5 (tooling):** `terminal` tool rejected `cargo test` command shape ~70 times ("tool input was not fully received"). Broke the loop via metacognition: switched to `diagnostics` tool, which confirmed T2's edit compiles. T2 code-complete (runtime pass deferred to user). Lesson re-learned: when a tool rejects a parameter shape, stop retrying that shape and route around it.
