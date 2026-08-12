---
name: listening
description: "Apply the MAIA v3 listening template to an earnings-call transcript using a retrieve-cite-verify process. Splits the transcript into chunks, searches for evidence, and verifies each cited substring is present. Enforces no-fabrication by process."
---

# Listening

Applies the MAIA v3 listening template to an earnings-call transcript. The
template is a semantic evaluation procedure over text — it extracts claims,
classifies them by horizon, and emits per-section verdicts with evidence.

## The retrieve-cite-verify process

The no-fabrication invariant is enforced by the process, not by the prompt:

1. **Chunk** — the transcript is split into numbered chunks (by speaker turns
   or paragraph boundaries).
2. **Retrieve** — the model searches the chunks for evidence relevant to each
   section's `listen_for` criteria.
3. **Cite** — the model returns the chunk_id, the exact substring it found,
   and the character offset where it starts.
4. **Verify** — a post-processing step checks that each cited substring is
   actually present in the referenced chunk. Fabricated quotes are rejected.

The model never "writes" a quote — it "finds" one and points to where it found
it. The verification is mechanical (substring match), not model-mediated.

## When to Use

- When analyzing an earnings-call transcript for MAIA-style company analysis.
- When you need per-section verdicts (margin trajectory, working capital,
  moat, capital allocation, expectations gap, guidance, management consistency)
  with verbatim evidence quotes.
- When you need the checkpoint map (dated milestones linked to strategic goals)
  for the FUTURE section of the company template.
- When you need to filter short-term-only guidance changes (no strategic-path
  linkage) into `ignored_short_term` so they don't influence verdicts.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `listening/apply-template.j2` | KnowAct | Apply the v3 listening template to a single transcript (chunked); retrieve-cite-verify process. |
| `listening/apply-template-rag.j2` | KnowAct | Apply the v3 listening template over RAG-retrieved corpus passages + KG triples; cross-source citations. |

## Constraints

- Single-pass (sense→act, not iterative).
- No-fabrication invariant is process-embedded: the model retrieves from
  numbered chunks and cites what it found; the process verifies each citation
  mechanically (substring match). The model cannot fabricate a quote because
  the process never gives it a "write a quote" step.
- The linkage, not the calendar date, is the admissibility bar.
- Certainty vocabulary: proximate (≥67%) / probable (33–66%) / possible (<32%).
- No verdict or forecast input may be derived from `ignored_short_term` entries.
