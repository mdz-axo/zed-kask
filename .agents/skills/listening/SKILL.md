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

| Template | Purpose |
|----------|---------|
| `apply-template.j2` | Apply the MAIA v3 listening template (stance block + 7 sections + horizon model) to an earnings-call transcript. Emits per-section verdicts with verbatim evidence quotes, the checkpoint map, and ignored_short_term entries. The no-fabrication invariant is enforced: every evidence field is a verbatim substring of the source transcript. Context: `transcript_chunks` (array of `{speaker, text}` chunk objects), `prior_transcript_chunks` (array, earlier calls for trend context), `company_symbol` (string). **Cascade-invoked** (call `render_template` with template_ref `listening/apply-template` at step 2). |
| `apply-template-rag.j2` | Apply the MAIA v3 listening template over a company knowledge graph. Takes RAG-retrieved passages from multiple documents (earnings calls, 10-Ks, investor days) plus KG triples linking them. Emits per-section verdicts with cross-source citations — a verdict can cite evidence from document A and document B. The no-fabrication invariant extends to the full corpus: every evidence field is a verbatim substring of one of the source passages. **Legacy — registered in the crate but NOT referenced by the skill execution** (the skill calls `render_template` with `apply-template.j2` at step 2 only; this template is available for standalone RAG-corpus invocation). |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- Single-pass (sense→act, not iterative).
- No-fabrication invariant is process-embedded: the model retrieves from
  numbered chunks and cites what it found; the process verifies each citation
  mechanically (substring match). The model cannot fabricate a quote because
  the process never gives it a "write a quote" step.
- The linkage, not the calendar date, is the admissibility bar.
- Certainty vocabulary: proximate (≥67%) / probable (33–66%) / possible (<32%).
- No verdict or forecast input may be derived from `ignored_short_term` entries.
