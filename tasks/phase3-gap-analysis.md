# Phase 3 — Gap Interrogation

> Skills composed: sequential-inquiry (reasoning engine), grill-me
> (self-challenge), with delegation to falsifiability for counterfactuals.
> Per-artifact gap analysis with Solid / Partial / Gap ratings per area.

## T5 — Replica YAML cluster

### sequential-inquiry branches

**Branch 1: Why is `company-researcher.yaml` divergent from `CorpusConfig`?**

Hypotheses (ranked):
- **H1 (stale template):** The YAML predates the `DimensionCentroid` /
  `Entity { name, appears_in }` / `ValidationConfig.exemplar_count_*`
  refactor. It was authored against an older `CorpusConfig` and never
  updated. **Most likely.** Supported by: `validation.per_dimension`,
  `budget.mode`, `dimension_centroids[].dimension` all look like an older
  schema vocabulary.
- **H2 (never run against current code):** The persona was built once
  (against the older schema) and the YAML has not been re-loaded since.
  Supported by: the header references a `replica_build` tool that does
  not exist in the current MCP server (the tool is `corpus_build_persona`).
- **H3 (intentionally divergent):** The persona uses a different shape on
  purpose. **Rejected** — no code path accepts the divergent shape; the
  consumer is a single `CorpusConfig` deserializer.

**falsifiability delegation (counterfactual):** "If `company-researcher.yaml`
is loaded by current `embed_corpus`, does it parse?" Prediction: **No** —
at least 3 missing-required-field errors (`centroid_entity_ref`,
`validation.exemplar_count_min`, `validation.exemplar_count_max`,
`foundational_rules`). To be confirmed empirically in Phase 4 (T7 test).

**Branch 2: Is `company-researcher.yaml` still used?**

- Grep for `company-researcher.yaml` / `company-researcher` in `*.rs`,
  `*.md`, `*.sh`, `*.toml` (excluding the artifact itself and `tasks/`)
  returns **zero references**. The persona is not referenced by any Rust
  code, doc, or script outside this audit.
- The header's `replica_build { config_path: ... }` example references a
  tool name that no longer exists.
- **Conclusion:** The persona is **likely dead** — no consumer, no doc
  reference. But "likely dead" is not "confirmed dead"; an operator could
  still invoke `corpus_build_persona` with this path manually. **Open
  question for the human:** is `company-researcher.yaml` still intended
  to be buildable? If yes → must fix (Phase 4). If no → should delete
  (out of scope for this audit's fix phase; flag for follow-up).

**Branch 3: Why do the two YAMLs disagree on shapes?**

- `john-brooks.yaml` uses `entities.concepts: [{name:...}]`,
  `dimension_centroids[].{name,ref_name,weight}`, `budget: {max_triples}`.
- `company-researcher.yaml` uses bare strings, `dimension`, `budget.mode`.
- They were authored against different `CorpusConfig` versions.
  `john-brooks.yaml` is current; `company-researcher.yaml` is stale.

### grill-me (5 rounds, `company-researcher.yaml`)

| Round | Question | Answer | Rating |
|-------|----------|--------|--------|
| Recall | What does this artifact do? | Configures a "company-researcher" persona for `embed_corpus`. | Solid |
| Mechanism | How does the consumer use it? | `serde_yaml_neo::from_str::<CorpusConfig>` then iterate `works`, compute centroids, store. | Solid |
| Rationale | Why this shape and not `john-brooks.yaml`'s shape? | No defensible reason — it is stale. The `embedding.centroid_entity_ref` nesting, `validation.per_dimension`, `budget.mode`, `dimension_centroids[].dimension` are all pre-refactor vocabulary. | **Gap** |
| Edge Cases | What inputs break the template? | Any load attempt — 3 required fields missing. Even if fixed, `works: []` means zero passages are processed (the persona would not actually build). | **Gap** |
| Synthesis | Is the artifact still correct given current Rust? | No. It does not parse, and even if it did, `works: []` produces no embeddings. | **Gap** |

### grill-me (5 rounds, `john-brooks.yaml`)

| Round | Question | Answer | Rating |
|-------|----------|--------|--------|
| Recall | What does this artifact do? | Configures the "john-brooks" persona. | Solid |
| Mechanism | How does the consumer use it? | Same as above. | Solid |
| Rationale | Why this shape? | Matches current `CorpusConfig` exactly. `dimension_centroids` weights sum to 1.0; `budget.max_triples: 0` disables triples for a style-only persona. | Solid |
| Edge Cases | What inputs break it? | None observed — all required fields present, all shapes correct. `works[0].local_path: "corpus/extracted/researcher"` depends on the directory existing at build time (operator concern, not artifact defect). | Solid |
| Synthesis | Is it still correct? | Yes — it is the reference instance. | Solid |

### Per-artifact gap analysis (T5)

**`company-researcher.yaml`:**
- Recall: **Solid**
- Mechanism: **Solid**
- Rationale: **Gap** (stale schema, no defensible reason for divergence)
- Edge Cases: **Gap** (3 missing required fields; `works: []` produces no passages)
- Synthesis: **Gap** (does not parse; persona cannot build)
- **Overall: Gap.** Promotes to Phase 4.

**`john-brooks.yaml`:**
- Recall: **Solid**
- Mechanism: **Solid**
- Rationale: **Solid**
- Edge Cases: **Solid**
- Synthesis: **Solid**
- **Overall: Solid.** No Phase 4 action; serves as the fix reference.

---

## T6 — Pipeline manifest runbook

### sequential-inquiry branches

**Branch 1: Are the hardcoded model names current?**

- Verified in Phase 2: all 3 (`HKASK_CLASSIFIER_MODEL`,
  `HKASK_QA_MODEL`, `HKASK_EMBEDDING_MODEL`) match `kask/.env` exactly.
- `base_model: "unsloth/Qwen3.6-27B"` matches
  `kask/mcp-servers/hkask-mcp-training/src/providers/mod.rs` (6 occurrences)
  and `kask/registry/manifests/training/capabilities-researcher.yaml`.
- **Solid.** No drift.

**Branch 2: Is `max_tokens: 512` superseded by a Rust default?**

- `corpus_chunk` at `kask/mcp-servers/hkask-mcp-corpus/src/tools/document.rs:886`:
  `"max_tokens": max_tokens.unwrap_or(512)`. **512 IS the Rust default.**
- The runbook's "UNVALIDATED" comment is honest about the *tuning* state,
  but the value itself is canonical (matches the default).
- **Solid** (value is canonical; the unvalidated-tuning note is accurate).

**Branch 3: Is `dedup_threshold: 0.89` correct?**

- `default_dedup_threshold() = 0.85` at
  `kask/mcp-servers/hkask-mcp-corpus/src/tools/corpus/mod.rs:1057`.
- The runbook uses **0.89**, diverging from the Rust default (0.85).
- The runbook does not document *why* 0.89. No design doc found.
- **Partial** — the value diverges from the default with no documented
  justification. This is a **Hypothesis**-tier finding: 0.89 may be a
  deliberate tightening (fewer near-duplicates merged) or may be stale.
  Recommend the human confirm intent. Not a bug (the tool accepts any
  threshold), but a provenance gap.

**Branch 4: Is the absolute path `/home/mdz-axolotl/Clones/Library/Researcher` stale?**

- Cannot verify from the codebase — it is operator-local. The runbook is
  operator-scoped, so an absolute path is acceptable. Flag as
  **Hypothesis** — the human must confirm the corpus still lives there.

**Branch 5: Do the `verify:` field names match tool return shapes?**

- `verify: { field: total_documents, min: 80 }` (Phase 0, `corpus_convert`) —
  `corpus_convert` returns `total_documents`? To confirm in Phase 4 if
  this runbook is in scope for a fix. For now: **Hypothesis**.
- `verify: { field: stored_h_mems, min: 1 }` (Phase 7, `corpus_ingest_qa`) —
  **Hypothesis**.
- `verify: { field: train_examples, min: 10 }` (Phase 8,
  `training_assemble_dataset`) — **Hypothesis**.
- These are runbook-level assertions; no Rust enforces them. The field
  names are plausible but unverified. Not bugs — the runbook is advisory.

### grill-me (5 rounds, runbook)

| Round | Question | Answer | Rating |
|-------|----------|--------|--------|
| Recall | What does this artifact do? | Coordinates a 9-phase corpus→LoRA pipeline. | Solid |
| Mechanism | How is it used? | Operator reads it and invokes MCP tools manually. No Rust consumer. | Solid |
| Rationale | Why this shape? | Linear phase list with `tool`/`params`/`verify` per step — a reasonable runbook shape. | Solid |
| Edge Cases | What inputs break it? | Tool return-shape changes silently invalidate `verify:` field names. `dedup_threshold: 0.89` diverges from default (0.85) with no justification. | **Partial** |
| Synthesis | Is it still correct? | Model names and training params are current (match `.env` and registry manifest). `max_tokens: 512` matches Rust default. `dedup_threshold: 0.89` is an unjustified divergence. `verify:` blocks are unenforced. | **Partial** |

### Per-artifact gap analysis (T6)

**`pipeline-capabilities-researcher.yaml`:**
- Recall: **Solid**
- Mechanism: **Solid**
- Rationale: **Solid**
- Edge Cases: **Partial** (`dedup_threshold` divergence, `verify:` fragility)
- Synthesis: **Partial** (mostly current; one unjustified divergence; unenforced verifies)
- **Overall: Partial.** No Phase 4 bug-hunt (no Rust consumer). Recommend
  a runbook edit to document `dedup_threshold: 0.89` rationale (or revert to
  0.85) — but this is a **runbook edit with no automated verification**,
  so it requires human judgment, not a diagnose loop. Flag as a follow-up.

---

## Checkpoint C3

- [x] T5 gap analysis produced — `company-researcher.yaml` is **Gap** (Phase 4).
- [x] T6 gap analysis produced — runbook is **Partial** (no Phase 4; follow-up).
- [x] Confirmed bugs promoted to Phase 4: `company-researcher.yaml` contract drift.
- [x] Open question for human: is `company-researcher.yaml` still intended to
  be buildable? (If no, the fix is deletion, not conformance.)
- [x] Open question for human: is `dedup_threshold: 0.89` in the runbook
  intentional? (If no, revert to 0.85.)
