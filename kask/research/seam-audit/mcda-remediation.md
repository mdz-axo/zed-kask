# MCDA Remediation Ranking — Kask↔Zed Seam Audit

> Method: `mcda` over remediation candidates extracted from the three audit
> tracks. Criteria (default weights): security severity 0.35, codegraph
> simplification 0.25, UI consistency 0.25, cost-inverted 0.15. Each criterion
> scored 0–1 (1 = best). Cost-inverted: low cost = 1, high cost = 0. Every
> score traces to a finding.

## Candidates and scores

| Rank | ID | Remediation | Sec | CG | UI | Cost⁻¹ | Weighted |
|------|----|-------------|-----|-----|-----|--------|----------|
| 1 | KS-01 | Bridge `StepResult.taint` → `check_untrusted_input` (restore FIDES gate) | 0.80 | 0.20 | 0.00 | 0.50 | **0.405** |
| 2 | UI-13 | Replace raw-div affordances with `Button`/`IconButton` (18 sites, 5 widgets) | 0.10 | 0.20 | 0.95 | 0.50 | **0.398** |
| 3 | RA-04 | Delete/gate `hmem::archive` (557 lines, test-only) | 0.10 | 0.95 | 0.00 | 0.85 | **0.400** |
| 4 | RA-02 | Delete `SkillLoader` + re-exports (440 lines, zero callers) | 0.10 | 0.90 | 0.00 | 0.90 | **0.395** |
| 5 | KS-02 | Per-tool `ToolTaint` declaration (replace hardcoded `Pure`) | 0.70 | 0.20 | 0.00 | 0.40 | **0.355** |
| 6 | UI-01 | Fix graph header overflow (flex_wrap/min_w_0/truncate) | 0.00 | 0.00 | 0.90 | 0.80 | **0.345** |
| 7 | KS-03 | Update stale `.rules`/docs re `propagate_taint_for_binding` | 0.50 | 0.10 | 0.00 | 0.90 | **0.335** |
| 8 | RA-08 | Delete `SqliteRegistry::count` (dead, removes `unwrap_or(0)` trap) | 0.30 | 0.30 | 0.00 | 0.95 | **0.323** |
| 9 | RA-03 | Delete `resolve_manifest` + `load_manifest_from_file` | 0.05 | 0.60 | 0.00 | 0.90 | **0.303** |
| 9 | UI-14 | Add `cursor_pointer`/use `Button` (omitted cursor sites) | 0.00 | 0.00 | 0.70 | 0.85 | **0.303** |
| 11 | UI-15 | Introduce `PopoverMenu` for secondary actions | 0.00 | 0.00 | 0.75 | 0.55 | **0.270** |
| 12 | RA-07 | Delete 4 dead forecast fns | 0.05 | 0.40 | 0.00 | 0.90 | **0.253** |
| 13 | RA-11 | Delete 2 dead trait default methods | 0.05 | 0.30 | 0.00 | 0.95 | **0.235** |

Deferred (human decision, not ranked for application): RA-06 (`falsification`
research module, 492 lines), RA-09 (`Registry` in-memory cache, 896 lines,
test-only), RA-10 (`BundleRegistryIndex`, depends on RA-03).

## Top 3 (default weights)

**KS-01 (0.405) · RA-04 (0.400) · UI-13 (0.398)** — a near-tie. Rank-1 (KS-01)
leads because the security weight dominates; the cluster separates by
*which axis* you weight, not by magnitude.

## Essentialist survival (deletion test, G1/G2/G3)

| ID | G1 Exist (delete callers → complexity reappears?) | G2 Surface (deep enough?) | G3 Contract (minimal?) | Survives? |
|----|---------------------------------------------------|----------------------------|------------------------|-----------|
| KS-01 | Reappears — the FIDES gate is advertised; deletion leaves a doc-claimed invariant unenforced | Yes — bridges two stores into one read path | Yes — rewrite one fn | **yes** |
| RA-04 | Vanishes — test-only, no production caller | n/a (delete) | n/a | **yes** |
| RA-02 | Vanishes — zero callers, exported but unused | n/a (delete) | n/a | **yes** |
| UI-13 | Reappears — affordances still needed; complexity moves into `Button` (deeper module) | Yes — adopts Zed's deeper primitive | Yes — drop-in replacement | **yes** |
| KS-02 | Reappears — gate is advertised; leaving hardcoded `Pure` keeps it inert | Yes — per-tool taint is the right granularity | Yes — one field | **yes** |
| KS-03 | Vanishes (doc fix) — but `.rules` hygiene forbids inline edit; must be a PR suggestion | n/a | n/a | **yes (as PR suggestion)** |

All top-ranked remediations survive the essentialist deletion test.

## Sensitivity analysis (±20% weight perturbation)

Recomputing the top 3 under each axis emphasized (weights renormalized):

| Perturbation | Weights (S/CG/UI/C) | Rank 1 | Rank 2 | Rank 3 | Top-3 flips? |
|--------------|---------------------|--------|--------|--------|-------------|
| Baseline | .35/.25/.25/.15 | KS-01 | RA-04 | UI-13 | — |
| Security +20% | .42/.20/.20/.12 (≈) | KS-01 | KS-02 | RA-04 | **yes** (UI-13 drops, KS-02 enters) |
| Codegraph +20% | .28/.33/.18/.12 (≈) | RA-04 | RA-02 | KS-01 | **yes** (UI-13 drops, RA-02 enters) |
| UI +20% | .28/.18/.33/.12 (≈) | UI-13 | KS-01 | UI-01 | **yes** (RA-04 drops, UI-01 enters) |
| Cost +20% (favor cheap) | .30/.22/.22/.30 (≈) | RA-02 | RA-04 | KS-01 | **yes** (UI-13 drops, RA-02 leads) |

**Result: rank-1 is NOT robust** — it flips between KS-01, RA-04, RA-02, UI-13
depending on axis emphasis. Rank-1 is stable only when security is weighted at
or above baseline. The near-tie among the top cluster (0.395–0.405) means the
"top 3" set is sensitive to weights that differ by less than the natural
uncertainty in criterion weighting.

## Termination-criterion assessment

The acceptance criterion requires "MCDA top-3 stable across the last two
slices (no rank flip under ±20%)." This engagement ran **one** comprehensive
pass (3 parallel tracks over the whole seam), not two serial slices. With a
single pass:
- There are no "two consecutive slices" to compare.
- The ±20% sensitivity analysis shows the top-3 membership flips under every
  single-axis perturbation.

**Honest verdict: the stability criterion is NOT met in this pass.** Rank-1 is
stable only under security-emphasis. A second pass (re-audit) is recommended
if the operator wants convergence confirmation. Per the bounded-loop rule
(re-audit a slice at most twice), one re-audit is permitted.

## Recommended application order (for the operator / a future FlowDef run)

Apply in dependency order, each pinned with a test:

1. **KS-03** (doc/`.rules` integrity) — lowest risk, unblocks future audits by
   removing the phantom prior. PR-suggestion only (`.rules` hygiene).
2. **RA-02 + RA-03 + RA-08** (verified-zero-caller deletions) — pure dead-code
   removal, no behavioral change, lowest compile risk. Pin each with a
   grep-based "symbol is gone" test.
3. **KS-01 + KS-02** (restore FIDES gate) — the top security remediation.
   Requires a `Source → Sink` block regression test. Apply together: KS-02
   labels the tools, KS-01 wires the read path — neither alone exercises the
   gate.
4. **UI-13** (Button/IconButton adoption) — the top UI remediation. 18 sites
   across 5 widgets; pin with a widget render test asserting `Button`/`IconButton`
   presence and `disabled` state for the Kanban move chip.
5. **UI-01** (graph header overflow) — local fix; pin with a narrow-width
   render test asserting the "I disagree" affordance stays visible.