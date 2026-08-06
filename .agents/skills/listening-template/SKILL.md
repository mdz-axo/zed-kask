---
name: listening-template
description: >
  Apply the MAIA v3 listening template to an earnings-call transcript. Extracts
  per-section verdicts with verbatim evidence quotes, classifies claims by
  horizon (seam checkpoint / tactical / strategic / short-term-only), and emits
  the checkpoint map. Enforces the no-fabrication invariant: every evidence
  field is a verbatim substring of the source transcript. Single-pass
  (sense→act, not iterative). Anchored to the MAIA guidebook (company-analysis,
  financial-signposts, time-horizons, company-template) and the operator's
  2026-08-05 seam clarification (12–36mo = tactical→strategic transition zone).
---

# Listening Template

Applies the MAIA v3 listening template to an earnings-call transcript. The
template is a semantic evaluation procedure over text — it extracts claims,
classifies them by horizon, and emits per-section verdicts with verbatim
evidence quotes. The no-fabrication invariant is load-bearing: every evidence
field is a verbatim substring of the source transcript.

## When to Use

- When analyzing an earnings-call transcript for MAIA-style company analysis.
- When you need per-section verdicts (margin trajectory, working capital,
  moat, capital allocation, expectations gap, guidance, management consistency)
  with verbatim evidence quotes.
- When you need the checkpoint map (dated milestones linked to strategic goals)
  for the FUTURE section of the company template.
- When you need to filter short-term-only guidance changes (no strategic-path
  linkage) into `ignored_short_term` so they don't influence verdicts.

## Instructions

1. Provide the transcript text (from `company_transcript` tool output or pasted).
2. Optionally provide prior-quarter transcripts for `management_consistency`
   (checkpoint drift detection across quarters).
3. The template applies the stance block (horizon classification + admissibility
   rule) to every extracted claim before emitting a verdict.
4. The output is JSON: `per_section` (7 sections, each with verdict + evidence +
   certainty), `horizon_summary` (checkpoint_map, strategic_goals,
   ignored_short_term, speculative_far).
5. Every `evidence` field is a verbatim substring of the source transcript.
   Fabricated quotes fail the golden-file test.
6. `ignored_short_term` entries never influence verdicts or forecast inputs.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `listening-template/apply-template.j2` | KnowAct | Apply the v3 listening template to a transcript; emit per-section verdicts + checkpoint map. |

## Constraints

- Single-pass (sense→act, not iterative) — the design says "the loop terminates
  per quarter (one pass over one transcript, fixed template, no iteration)."
- No fabrication: every evidence field is a verbatim substring of the source.
- The linkage, not the calendar date, is the admissibility bar: a near-term
  event that is a nameable checkpoint on the path to a stated strategic goal
  IS primary material.
- Certainty vocabulary: proximate (≥67%) / probable (33–66%) / possible (<32%) —
  the guidebook tier, matching `hkask_forecast.rs:158 certainty_tier`.
- No verdict or forecast input may be derived from `ignored_short_term` entries.
