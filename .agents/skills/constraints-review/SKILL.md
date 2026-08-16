---
name: constraints-review
core: true
visibility: public
description: "Constraint-set coherence review. Elicits constraints from .rules, DIVERGENCE.md, ADRs, and manifest ocap blocks; classifies each by force (Prohibition/Guardrail/Guideline/Evidence/Hypothesis); gates against floor/ceiling/maturity per Ashby requisite variety; measures drift against the reference models in kask/docs/architecture/review-reference-models.md. Produces a drift score per constraint (0=aligned, 3=divergent with no exception)."
---

# Constraints Review

Constraint-set coherence review for the zed-kask fork. Distinct from
`coherence-review` (which asks "does the *system* hang together?"):
`constraints-review` asks "do the *rules governing the system* hang
together, and are they drifting from the reference models?"

## When to Use

- When you suspect the constraint set (`.rules`, `kask/.rules`,
  `DIVERGENCE.md`, ADRs, manifest `ocap:` blocks) has accumulated drift
  from the reference models that anchor the project's design.
- When a new constraint is proposed and you need to know whether it
  aligns with, or diverges from, the reference models — and whether the
  divergence is a documented choice or undocumented drift.
- When you need a drift baseline before a major change (upstream rebase,
  new crate, new MCP server) so you can measure whether the change
  introduced constraint drift.
- When an advertised invariant (doc comment claiming a security gate,
  audit surface, migration) needs to be checked for an enforcement point
  — the project rule: "advertised invariants must point to the
  enforcement line, or say 'not yet enforced.'"

## Reference Models

This skill is anchored to the reference models documented in
`kask/docs/architecture/review-reference-models.md`. The drift
measurement is the core deliverable: each constraint is scored against
the references, and score-3 findings (divergent with no documented
exception) are the actionable drift signal.

Load-bearing references (see the calibration doc for full citations):

- **Ashby's Law of Requisite Variety** — the constraint set's variety
  must match the failure-mode variety at each level. Grounds the
  floor/ceiling/maturity gate.
- **SEI ATAM** — constraints are trade-offs, not absolutes. Grounds the
  force classification (a Prohibition is a hard trade-off; a Guideline is
  a soft one).
- **Simon's near-decomposability** — the kask/upstream boundary is a
  near-decomposability constraint. Grounds the constraint elicitation
  from `DIVERGENCE.md`.
- **SEI ATAM (intended vs evaluated)** — the reference models are the
  *intended* constraint set; the live `.rules` are the *evaluated*. Drift
  is the divergence. (See the calibration doc for the Murphy et al.
  historical note — the IS/OUGHT framing was re-grounded in ATAM after
  the Murphy citation could not be verified.)

## Instructions

### cr-elicit

1. Read the constraint sources: `.rules`, `kask/.rules`,
   `DIVERGENCE.md` (the D-seam table), `AGENTS.md`, ADRs in
   `kask/docs/architecture/`, and `ocap:` blocks in skill manifests.
2. For each constraint, record: the source file and line, the
   constraint text, the level it operates at (L1 boundary, L2 crate
   graph, L3 module, L4 surface, L5 code), and the failure mode it
   guards against.
3. Do not classify yet — elicitation is exhaustive capture only.

### cr-classify

1. Classify each elicited constraint by constraint force, using the
   pragmatic-semantics hierarchy:
   - **Prohibition** — hard-enforced (compiler, CI, runtime gate).
     Example: "no `unwrap()`".
   - **Guardrail** — soft-enforced (review, convention). Example:
     "prefer existing files over creating new ones".
   - **Guideline** — advisory. Example: "comments explain why, not what".
   - **Evidence** — empirical observation. Example: "smol timers break
     `run_until_parked()`".
   - **Hypothesis** — speculative. Example: "this layering will hold
     under upstream rebases".
2. For each constraint, identify the enforcement point (the line where
   the constraint is actually enforced) or mark it "not yet enforced."
3. Flag advertised invariants with no enforcement point — these are
   score-3 drift candidates.

### cr-gate

1. For each level (L1–L5), check Ashby's requisite variety: does the
   constraint set at that level have enough variety to catch the
   failure modes that occur at that level?
2. **Floor check:** minimum constraints to be safe. A level below floor
   is under-constrained — failure modes will slip through.
3. **Ceiling check:** constraints that would over-constrain and kill the
   system's adaptivity. A level above ceiling is over-constrained —
   legitimate changes become impossible.
4. **Maturity check:** is each constraint enforced yet? An unenforced
   constraint is a Hypothesis dressed as a Prohibition — reclassify it
   or wire the enforcement.

### cr-drift

1. Compare each constraint against the reference models in
   `kask/docs/architecture/review-reference-models.md`.
2. Assign a drift score:
   - `0` — aligns with a reference model.
   - `1` — neutral (no reference applies).
   - `2` — diverges from a reference, exception documented in the
     calibration doc's "Where we deviate" section.
   - `3` — diverges from a reference, no documented exception.
3. Score-3 findings are the drift signal. The fix is either (a) document
   the exception in the calibration doc (making it a score-2) or (b)
   change the constraint to align.
4. Produce a drift report: per-constraint scores, per-level variety
   verdicts, and the list of score-3 findings with recommended actions.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `cr-elicit.j2` | `KnowAct` | Elicit constraints from .rules, DIVERGENCE.md, ADRs, manifest ocap blocks. Exhaustive capture, no classification. |
| `cr-classify.j2` | `KnowAct` | Classify each constraint by force (Prohibition/Guardrail/Guideline/Evidence/Hypothesis) and identify the enforcement point or mark "not yet enforced." |
| `cr-gate.j2` | `KnowAct` | Gate the constraint set against floor/ceiling/maturity per Ashby requisite variety, per level. |
| `cr-drift.j2` | `KnowAct` | Measure drift against the reference models. Produce per-constraint drift scores and the score-3 findings list. |

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- The reference models are the benchmark; the live constraint set is the
  instrument under test. Do not invert these.
- A score-3 finding is actionable; a score-2 is a documented choice. Do
  not collapse score-3 to score-2 without adding the exception to the
  calibration doc.
- Registry is authoritative — when this SKILL.md disagrees with registry
  templates, the registry wins.
