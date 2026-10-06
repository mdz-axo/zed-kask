---
title: "Media MCP Server — Redesign Review Findings"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - media-server-redesign-review.md
---

# Findings — Media MCP Server Redesign Review

Companion to `media-server-redesign-review.md` (method, coverage, ground
truth). **State:** the five operator-named target conditions are recorded
below as the review's first accepted inputs (MF-1…MF-5, each re-grasped at
current file:line 2026-10-06); Phase 1 is opened with its derivation plan;
Phases 2–4 are pending and fill in as their loops run. Findings from the
fidelity review will be numbered MF-6 onward so every finding id maps to
exactly one proposal or no-action record (reconciled via `lisp_eval` at
Phase 5, the scenarios precedent).

## The five target conditions (onboarding record §6, re-grasped 2026-10-06)

These are the primary inputs to Phases 5/6: the review's findings and
proposals feed them, and the plan's slices execute them. When the operator
directs a specific condition, its kata loop runs directly — the phases are
its method, not a prerequisite re-run.

### MF-1 — Reduct cloud write surface (condition 1)

- **Current condition:** strikethroughs, highlight **writes**, redactions,
  publishing, media download, and transcript correction remain unimplemented
  (`kask/mcp-servers/hkask-mcp-media/README.md:340-344`). Page 41 of the
  v3 reference lists strikethrough paths without body/response schema, and
  there is no private Reel render/download contract. Publication never
  happens by default — a share token creates a publicly accessible link
  (`README.md:338`).
- **Target condition:** each unimplemented capability is either implemented
  against the pinned provider contract (fixture-tested request path/body,
  parsed acknowledgement, HTTP refusal — the way `reduct_create_reel` et
  al. were) or explicitly documented as out of scope with the reason.
- **Reached when:** the capability has contract tests and a docs-reference
  row, or the README's unimplemented list no longer names it.
- **Decision status:** technical execution (no operator decision required
  beyond slice acceptance); the publication-never-by-default invariant is
  non-negotiable in every slice.

### MF-2 — Transcript re-alignment (condition 2)

- **Current condition:** a correction that changes token cardinality leaves
  the corrected text readable and corpus-exportable, but timing-dependent
  navigation (`educt_locate`), highlighting, and SRT export fail with an
  explicit unaligned precondition instead of silently using stale source
  text or inventing timestamps (`README.md:252-259`). Reduct can re-align
  such edits server-side; local re-transcription/re-alignment remains the
  honest capability gap. The immutable source bundle remains available for
  audit.
- **Target condition:** a re-alignment path (re-transcription or word-level
  re-anchoring) that restores timed consumers after cardinality-changing
  corrections, with the unaligned precondition retained as the failure mode
  for the cases it genuinely cannot handle.
- **Reached when:** a correction that inserts/deletes words can
  subsequently drive `educt_locate`, highlighting, and SRT export — pinned
  by a test that does exactly that.
- **Decision status:** technical execution; the unaligned precondition must
  survive as the honest failure mode, not be deleted by the fix.

### MF-3 — Face-recognition build-out (condition 3) — OPERATOR DECISION

- **Current condition:** full build-out is explicitly **deferred**; the
  working core is the two vision-LLM templates (`validate_face_ref`,
  `match_faces` in `src/templates.rs`), and any expansion is meant to stay
  on the LLM-template surface (`README.md:379-383`). There is no local
  embedding model and no local geometric matching — a previous
  LLM-produced-"embedding" cosine path was removed because LLMs cannot
  emit geometrically consistent vectors, and its store column was dropped
  with it.
- **Target condition:** a decision, recorded with the operator, on whether
  the deferred build-out happens (e.g. better matching prompts,
  multi-reference voting) and what its acceptance criteria are — then
  either the implementation with its tests or an explicit "still deferred"
  with the triggering condition written down.
- **Reached when:** the README's deferral note names a decision date and
  outcome instead of an open deferral.
- **Decision status:** **operator decision required.** Options in
  functional terms: (a) keep deferred — the working core stays the two
  templates, the README records the ruling and the triggering condition;
  (b) build out on the LLM-template surface — better matching prompts
  and/or multi-reference voting, with acceptance criteria stated before
  implementation. Presented with costs when this condition's loop runs.

### MF-4 — Embedding-error typing (condition 4)

- **Current condition:** `classify_embedding_error` string-matches
  credential-missing substrings via `is_credential_missing_error`
  (`src/error.rs:181-184`, used at `:220-227`) because
  `EmbeddingGenerationError` has no typed `NotConfigured` variant — its
  variants are `InvalidRequest`, `Connection`, `Api`, `Json`,
  `EmptyResponse`, `DimensionMismatch`
  (`kask/crates/hkask-types/src/ports/embedding.rs:30-42`). The doc comment
  at `src/error.rs:174-181` names the migration path: add a
  `NotConfigured(String)` variant to `EmbeddingGenerationError` and match
  on the variant instead.
- **Target condition:** the typed variant exists and the string-matching is
  deleted, with the per-variant `permission_denied` classification
  preserved.
- **Reached when:** `classify_embedding_error` matches on variants, and a
  test pins missing-credential → `permission_denied` without substring
  matching.
- **Decision status:** technical execution. Note the adjacent 2026-10-05
  commit `d17d9e7a92` fixed `ImageNotFound` → `not_found` and
  `GalleryNotInitialized` → `failed_precondition` — a different slice of
  the error contract; this condition remains open. The change touches
  `hkask-types` (shared), so its sweep covers every
  `EmbeddingGenerationError` consumer, and the deletion removes
  `is_credential_missing_error` outright.

### MF-5 — DNS-rebinding limitation (condition 5) — OPERATOR DECISION

- **Current condition:** the widget's preflight treats every block locator
  as untrusted (local media under the artifacts root or exact
  gallery-observed paths; symlink escapes fail; inline data MIME-checked
  and capped at 32 MiB; public fetches validate DNS and reject redirects),
  but it cannot pin opaque yt-dlp/FFmpeg connect-time DNS — DNS rebinding
  inside those subprocesses remains an explicitly documented transport
  limitation rather than a claimed guarantee (`README.md:83-85`).
- **Target condition:** either a documented accepted-risk ruling from the
  operator, or a mitigation (e.g. subprocess DNS pinning) with tests.
- **Reached when:** the README paragraph states a ruling or a mitigation,
  never an open limitation.
- **Decision status:** **operator decision required.** Options in
  functional terms: (a) accepted risk — the README records the ruling and
  its scope (which subprocesses, which threat); (b) mitigation —
  subprocess DNS pinning with tests, at the cost of the pinning
  infrastructure's complexity and its own failure modes. Presented with
  costs when this condition's loop runs.

## Phase 1 — Primitives derivation (opened, pending)

Derive the primitive model from the four verified reference models BEFORE
scoring the implementation against it:

1. **OMC creation vocabulary** — the tool→concept mapping's source: for
   each primitive, which OMC concept carries it across the server's seams
   (the shared concept→explain dispatch is the widget contract).
2. **Reduct.video v3 API** — the cloud-surface primitives: project,
   recording, reel, block, highlight, publication state; which are
   read-only vs. write, and what the unimplemented capabilities would add.
3. **Transcript discipline** (`transcript_linked_media`, ruling
   2026-09-25) — the educt primitives: the immutable word-anchored source
   bundle, the validated layer kinds (speaker, paragraph, correction,
   highlight, EDL), the working-transcript projection, the Keep-range
   union composition, and the re-alignment boundary (MF-2).
4. **Ratified README contracts** — the storage primitive
   (`persist_slim_and_enrich`: one SQLite transaction for file, Asset
   row, lineage, OMC graph), the error-classification contract, the
   publication-never-by-default invariant, and the unencrypted-gallery-DB
   startup contract.

For each primitive: properties and methods, reference-model source,
exists?, and what carries across the seams. The derivation's output seeds
Phase 2's per-tool fidelity review (98 tools, findings numbered MF-6
onward).

## Phases 2–4 — pending

Phase 2 (fidelity, per tool with file:line), Phase 3 (integration,
essentialist gates), and Phase 4 (formal layer — EDL Keep-range union
semantics and layer-validation invariants as Lean candidates, finite
structural checks via `lisp_eval`) fill in as their PDCA loops run. No
findings recorded yet beyond MF-1…MF-5.
