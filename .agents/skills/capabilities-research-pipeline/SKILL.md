---
name: capabilities-research-pipeline
visibility: public
description: "Corpus extraction pipeline for the Capabilities Researcher persona. Each phase is wrapped in a PDCA COMPLETION gate that demands ALL outputs before proceeding. The lisp.eval gate returns a numeric distance-from-complete (count of missing items). The loop iterates until that number reaches 0. The Act phase embeds metacognition and kata-improvement to diagnose failures and pivot strategies."
---

# Capabilities Research Pipeline

Corpus extraction pipeline for the Capabilities Researcher persona. Each phase
is wrapped in a PDCA COMPLETION gate that demands ALL outputs before
proceeding. The `lisp.eval` gate returns a numeric distance-from-complete
(count of missing items); the loop iterates until that number reaches 0. The
Act phase embeds metacognition and kata-improvement to diagnose failures and
pivot strategies.

## Canonical source

The process manifest `registry/manifests/capabilities-research-pipeline.yaml`
is the ground truth. This SKILL.md is a derived companion; where they disagree,
the registry wins.

## When to Use

- When you need to extract a source library into a queryable corpus (convert →
  chunk → tag → embed → assert → consolidate → dedup → verify) with completion
  gates that refuse to advance until every source file is represented.
- When you want deterministic `lisp.eval` completion gates rather than
  LLM-mediated "did it work?" checks — the gate returns a numeric
  distance-from-complete and the loop closes the gap to 0.
- When you want each phase's Act step to drive a strategic pivot (different
  method, not just tweaked params) via the shared `pdca-kata-pivot` template,
  following the kata cycle: target condition → actual condition → obstacles →
  next experiment.
- When you want the corpus MCP tools (`corpus_convert`, `corpus_chunk`,
  `corpus_tag_chunks`, `corpus_embed`, `corpus_extract_assertions`,
  `corpus_consolidate_chunks`, `corpus_dedup_chunks`, `corpus_query`) called
  as native `action: execute` flowdef steps, with their JSON output bound into
  the Check step via `env`.

## Instructions

### Phase 0 — Initialize

1. `corpus_clear_index`: clear the in-memory vector index (`researcher-corpus`).
2. `inventory_sources` (`lisp.eval`): inventory the source library and fix the
   COMPLETION TARGET (default 138 files). This is the ground truth every gate
   checks against — not "does a file exist" but "are all N source files
   represented."

### Phase 1 — CONVERT (PDCA gate)

1. `convert_do` (`corpus_convert`): extract text from ALL source documents.
   Strategy selected by `prior_iteration.convert_strategy` (default
   `corpus_convert`); resumes non-empty outputs.
2. `convert_check` (`lisp.eval`): COMPLETION GATE. Returns
   `distance_from_complete = (target - extracted) + failed`. 0 = complete. No
   silent defaults — missing `files_extracted` / `files_failed` error loudly.
3. `convert_act` (`select`, template `pdca-kata-pivot`, condition
   `step_3_result.verdict == 'fail'`): metacognition + kata. Diagnoses why the
   current strategy failed and proposes a strategic pivot. Strategy ladder:
   `corpus_convert` → `corpus_ocr` → `direct_runpod_api` → `pdftotext` →
   `skip_and_escalate`.
4. `convert_loop` (`loop`): re-enters Do with the pivoted strategy until
   `distance_from_complete` reaches 0 or strategies are exhausted.

### Phase 2 — CHUNK (PDCA gate)

1. `chunk_do` (`corpus_chunk`): split ALL extracted text into passages
   (multi-tier by default: coarse 2048 / medium 512 / fine 128 tokens).
2. `chunk_check` (`lisp.eval`): COMPLETION GATE. `distance_from_complete =
   missing_sources + empty_chunks`. Pass requires `distance == 0` AND
   `total_chunks >= 100`.
3. `chunk_act` (`select`, `pdca-kata-pivot`, condition
   `step_7_result.verdict == 'fail'`): pivot strategy (single-tier, smaller
   tokens, larger tokens, re-run convert, skip-and-escalate).
4. `chunk_loop` (`loop`): re-enters until complete.

### Phase 3 — TAG (PDCA gate)

1. `tag_do` (`corpus_tag_chunks`): tag ALL chunks.
2. `tag_check` (`lisp.eval`): COMPLETION GATE. `distance_from_complete =
   untagged + empty_tags`. Pass requires `distance == 0` AND
   `unique_sources == target`.
3. `tag_act` (`select`, `pdca-kata-pivot`, condition
   `step_11_result.verdict == 'fail'`): pivot tagging strategy.
4. `tag_loop` (`loop`): re-enters until complete.

### Phase 4 — EMBED (PDCA gate)

1. `embed_do` (`corpus_embed`): embed ALL tagged chunks.
2. `embed_check` (`lisp.eval`): COMPLETION GATE. `distance_from_complete =
   unembedded + failed`. Pass requires `distance == 0` AND
   `embedded == total`.
3. `embed_act` (`select`, `pdca-kata-pivot`, condition
   `step_15_result.verdict == 'fail'`): pivot embedding strategy.
4. `embed_loop` (`loop`): re-enters until complete.

### Phase 5 — ASSERT (PDCA gate)

1. `assert_do` (`corpus_extract_assertions`): extract assertions from ALL
   chunks.
2. `assert_check` (`lisp.eval`): COMPLETION GATE. `distance_from_complete =
   missing_assertions + empty`. Pass requires `distance == 0`.
3. `assert_act` (`select`, `pdca-kata-pivot`, condition
   `step_19_result.verdict == 'fail'`): pivot assertion-extraction strategy.
4. `assert_loop` (`loop`): re-enters until complete.

### Phase 6 — Consolidate, dedup, verify

1. `consolidate_do` (`corpus_consolidate_chunks`): consolidate chunks.
2. `dedup_do` (`corpus_dedup_chunks`): deduplicate chunks.
3. `verify_do` (`corpus_query`): verify the corpus is queryable.
4. `aggregate_check` (`lisp.eval`): final COMPLETION GATE across all phases.

### Phase 7 — Report

1. `report` (`select`, template `extraction-report`): compile the final
   extraction report with per-phase completion status, drawing on the
   convert/chunk/tag/embed/assert/verify step results.

## Convergence

- `convergence_mode: cauchy`, `cauchy_epsilon: 1`, `cauchy_window: 3`.
- `max_iterations: 5`, `min_iterations: 1`.
- `on_not_reached: escalate`. Per-gate PDCA loops close the
  `distance_from_complete` signal to 0; the outer cauchy convergence checks
  the aggregate signal across the window.

## Constraints

- The process manifest `registry/manifests/capabilities-research-pipeline.yaml`
  is the canonical source of truth; this SKILL.md is a derived companion.
- Gates are TARGET CONDITIONS, not pass/fail checkboxes. The `lisp.eval` gate
  returns a numeric distance from complete; the loop iterates until 0.
- Gates demand COMPLETION, not "file exists": `files_extracted ==
  total_files`, `files_failed == 0`, `unique_sources == total_sources`,
  `tagged == total`, `embedded == total`.
- No silent defaults on critical fields — a missing field in a tool's JSON
  output errors loudly (broken-feedback-loop trap).
- Direct MCP tool calls for execution; `lisp.eval` for deterministic gates;
  `select` + `pdca-kata-pivot` for the Act-phase strategic pivot.
