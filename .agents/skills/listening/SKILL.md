---
name: listening
description: "Apply the MAIA v3 listening template to an earnings-call transcript using a retrieve-cite-verify process. Splits the transcript into chunks, searches for evidence, and verifies each cited substring is present. Enforces no-fabrication by process."
---

# Listening

Applies the MAIA v3 listening template to an earnings-call transcript. The
template is a semantic evaluation procedure over text — it extracts claims,
classifies them by horizon, and emits per-section verdicts with evidence.

## When NOT to Use

- Documents that are not the company's own narrative — a generic interview, media coverage or sell-side reports need a different frame. A single earnings call uses `apply-template.j2`; the company's own multi-document narrative (calls, 10-K business and MD&A, investor days, shareholder letters) uses `apply-template-rag.j2`.
- Unverified summarization — the process enforces verbatim-evidence quotes; a summary without the retrieve-cite-verify loop is a different (weaker) artifact.
- Live capture — use `transcript-reel` for record/transcribe; this skill consumes an existing transcript.

## D/P labelling

Chunking is D — a structural split at speaker-turn or paragraph boundaries;
oracle: the numbered chunk list. Evidence search and citation are P —
critiqued by step 4's mechanical check. Verification is D: every cited quote
is checked with `lisp_eval` `string-contains` (needle first) against its own
passage — or, for educt-stored transcripts, `educt_locate`'s word-aligned
match; a miss under either oracle counts as a failed verification toward the
twice-fail drop. The counts are computed, never estimated — the model cannot
fabricate a citation that survives this check. A quote that twice fails
verification is dropped, not reinterpreted. Verdict interpretation is P —
critiqued by the operator and, downstream, `grounding-verify` when the
verdicts enter a research note.

## Instructions

The no-fabrication invariant is enforced by the process, not by the prompt:

1. **Chunk** — the transcript is split into numbered chunks (by speaker turns
   or paragraph boundaries).
2. **Retrieve** — render the selected template at this step (`listening/apply-template` for a single earnings call, `listening/apply-template-rag` for the company's own multi-document narrative; context per the Registry Templates table) — the rendered prompt carries the numbered chunks and the per-section `listen_for` criteria — then the model searches the chunks for evidence relevant to each section's criteria.
3. **Cite** (P — critique: step 4's mechanical check) — the model returns
   the chunk_id and the exact substring it found. No character offset: the
   verifier matches substrings, and a model-estimated offset would be an
   unchecked number.
4. **Verify** (D — `lisp_eval`) — call `lisp_eval` `string-contains` for each cited substring
   against its referenced chunk:
   - form: `"(string-contains cited_substring chunk_text)"`
   - env: `{ "cited_substring": <the cited text>, "chunk_text": <the referenced chunk's text> }`
   Fabricated quotes are rejected — the check is mechanical, not
   model-mediated. (The template's "the verdict is rejected" line addresses
   the model reading the rendered prompt; the process bound below governs the
   pipeline.) On a failed citation, re-retrieve once from the chunks
   (the Act); a citation that fails verification twice is dropped and the
   claim is reported as unverifiable — never delivered as verified. Bound: one
   re-retrieval per cited claim; the overall process stays single-pass
   (sense→act). For educt-stored transcripts, `educt_locate` is the
   deterministic word-aligned locator — prefer it there; an `educt_locate`
   miss is a failed verification and counts toward the twice-fail drop.

The model never "writes" a quote — it "finds" one and points to where it found
it. The verification is mechanical (substring match), not model-mediated.

5. **Count** (D — `lisp_eval`) — after verification, compute
   `(list verified total (- total verified) sections_without_verified_evidence)`
   from the step 4 results — env: `{ "verified": <citations that passed>, "total": <citations made>, "sections_without_verified_evidence": <count> }`. Every check is against the citation's OWN chunk:
   a quote found in some other chunk is a misattribution, not a pass.

**Target condition (T2):** every emitted verdict carries at least one
citation that passed step 4, or is reported `neutral` with no evidence;
zero unverified citations are delivered. **Initial condition (T1):** the numbered
chunks and the template's `listen_for` criteria per section.

**Loop exemption (T3).** Single-pass by design: the only correction is the
bounded one-re-retrieval per failed citation above. The verdicts are P
(model judgment over verified quotes; critique: the operator, and
downstream `grounding-verify` when the verdicts enter a research note).

**Reference model.** The MAIA method — its v3 earnings-call listening
template (`onto_anchor` → derived `maia_listening`, operator ruling
2026-09-25): the seven sections, horizon model and admissibility rule are
copied from that template, which this SKILL.md deliberately does not restate.
The retrieve-cite-verify discipline is part of the `maia_listening`
definition itself; `grounding-verify` reuses it from here — not the reverse.
Certainty tiers: `hkask_forecast::certainty_tier` is the single source of
truth on drift (the template's "guidebook verbatim" label names the text it
quotes, not a competing authority). The chunk-boundary rule and the count
form are project operationalizations — unanchored.

## When to Use

- When analyzing an earnings-call transcript for MAIA-style company analysis.
- When you need per-section verdicts (margin trajectory, working capital,
  moat, capital allocation, expectations gap, guidance, management consistency)
  with verbatim evidence quotes.
- When you need the checkpoint map (dated milestones linked to strategic goals)
  for the FUTURE section of the company template.
- When you need to filter short-term-only guidance changes (no strategic-path
  linkage) into `ignored_short_term` so they don't influence verdicts.

## Regression case

From `kask/registry/listening-fixtures/sample_transcript.txt`, chunk the Amy Hood margin passage and the Satya Nadella checkpoint passage, and render `listening/apply-template` with `company_symbol`. Then: (i) ACCEPT probe — verify a verbatim quote from the margin chunk with `(string-contains cited_substring chunk_text)` → true (this pins needle-first order: a reversed-args call silently returns false whenever the quote is shorter than the chunk); (ii) REJECT probe — a fabricated quote returns false, and after the one re-retrieval still fails: the claim is reported unverifiable, never delivered as verified; (iii) COUNT — the step 5 form with its declared env over the probes' results. RAG leg: render `listening/apply-template-rag` with one `corpus_passages` element drawn from `cross_doc_earnings.txt` and verify a verbatim quote against its own passage.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `apply-template.j2` | Apply the MAIA v3 listening template (stance block + 7 sections + horizon model) to an earnings-call transcript. Emits per-section verdicts with verbatim evidence quotes, the checkpoint map, and ignored_short_term entries. The no-fabrication invariant is enforced: every evidence field is a verbatim substring of the source transcript. Context: `transcript_chunks` (array; each element renders wholesale into the prompt — pass speaker-prefixed strings or readable chunk objects), `prior_transcript_chunks` (same shape, earlier calls for trend context), `company_symbol` (string). **Cascade-invoked** (step 2 renders the selected template). |
| `apply-template-rag.j2` | Apply the MAIA v3 listening template across the company's own narrative documents. Context: `corpus_passages` (array of `{source, entity_ref, text}` verbatim passages; `source` is the passage's citation key in the calling pipeline's source records), `kg_triples` (array, may be empty), `company_symbol`, `focus_query`. Emits per-section verdicts with cross-source citations, judged against the company's own stated strategy. Same retrieve-cite-verify steps (each quote checked against its OWN passage). Used by the LISTEN step of `company-research-deep` and `company-research-flash`; it filters strategic narrative and is not a materiality check. |

To render a template, call the `render_template` tool with the template ref (e.g., `listening/apply-template`) and a context object with the required variables.

## Constraints

- Single-pass (sense→act, not iterative).
- No-fabrication invariant is process-embedded: the model retrieves from
  numbered chunks and cites what it found; the process verifies each citation
  mechanically (`lisp_eval` `string-contains`). The model cannot fabricate a
  quote because the process never gives it a "write a quote" step.
- The linkage, not the calendar date, is the admissibility bar.
- Certainty vocabulary: proximate (≥67%) / probable (33–66%) / possible (<33%) — the tiers of `hkask_forecast::certainty_tier`, the single source of truth.
- No verdict or forecast input may be derived from `ignored_short_term` entries.
