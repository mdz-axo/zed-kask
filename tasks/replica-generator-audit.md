# Replica Generator — Logic Flow & PDCA Audit

## 1. Replica category coverage

### The three replica classes

The template set defines three replica classes, distinguished by source
material, ontology, and structural scaffolding:

| Class | Template | `corpus_type` | Works source | Foundational rules | Dimension centroids | Tag sets/weights | Budget | Ontology |
|-------|----------|---------------|--------------|--------------------|--------------------|------------------|--------|----------|
| **Academic** | `academic-corpus.j2` | `academic` | Semantic Scholar, arXiv, web, YouTube | none (concepts extracted from papers) | none | none | PerPage | PKO, ESO, Dublin Core |
| **Literary** | `literary-corpus.j2` | `literary` | Gutenberg (public-domain novels/essays) | critical essays, style manifestos | none | none | Flat | GOLEM, Dublin Core |
| **Exemplar/theme** | `exemplar-corpus.j2` | `literary` (hybrid, configurable) | mixed (Gutenberg + local corpus + docs + web) | thematic principles | 4 quality dimensions | full MDS taxonomy | Absolute | Dublin Core, PKO, FIBO, ESO, GOLEM |

### Existing replicas mapped to classes

| Replica | Class | Evidence |
|---------|-------|----------|
| `david-dunning` | Academic | `corpus_type: academic`; no `dimension_centroids`; `PerPage` budget; discovered via Semantic Scholar/arXiv |
| `woolf`, `hemingway`, `jane-wilde`, `agatha-eliot`, `ulysses-s-twain` | Literary | `corpus_type: literary` (default); Gutenberg works; `foundational_rules` (critical essays); `Flat` budget; no `dimension_centroids` |
| `gentle-lovelace` | Exemplar | `dimension_centroids` (4); `tag_sets` (4); `tag_weights`; `Flat` budget (not Absolute — see gap below); mixed sources (Gutenberg + docs + local) |
| `john-brooks` | Exemplar | `dimension_centroids` (4); `Absolute` budget (`max_triples: 0`); local corpus; thematic (capabilities research) |

**Coverage: all three classes are represented.** 5 literary, 1 academic, 2 exemplar.

### Category boundary ambiguity (gap)

`gentle-lovelace` is classified as exemplar (has `dimension_centroids` + `tag_sets`) but uses a `Flat` budget, not `Absolute`. The `exemplar-corpus.j2` template hardcodes `Absolute`. This means `gentle-lovelace` **cannot be regenerated from `exemplar-corpus.j2`** without making the budget shape configurable.

**Gap:** The exemplar template's `budget` block is rigid (`Absolute` only). The two existing exemplar replicas disagree on budget shape (`gentle-lovelace` = Flat, `john-brooks` = Absolute). The template should accept a budget-shape parameter or the `budget` block should be delegated to a per-replica override.

---

## 2. Manifest logic flow audit

### `replica-discovery.yaml` — the only discovery manifest

This manifest drives the **academic** discovery pipeline (13 phases). It is the
only replica-discovery manifest in the registry. There are **no equivalent
manifests for literary or exemplar replica creation** — those YAMLs are
hand-authored.

### Phase-by-phase flow

```
Phase 1  (ordinal 1):  Name disambiguation          [execute, web_search]
Phase 2a (ordinal 2):  Semantic Scholar search      [execute, web_search]
Phase 2b (ordinal 3):  arXiv search                 [execute, web_search]
Phase 3  (ordinal 4):  Web/institutional search     [execute, web_search, conditional: include_web]
Phase 4  (ordinal 5):  YouTube discovery            [execute, web_search, conditional: include_transcripts]
Phase 5  (ordinal 6):  Content extraction           [execute, web_extract, loop_over merged_works]
Phase 6  (ordinal 7):  Cache to disk                [execute, corpus_cache_work, loop_over step6_extracted_pages]
Phase 7  (ordinal 8):  YouTube transcript extract   [execute, web_search, loop_over step5_youtube_urls, conditional: include_transcripts]
Phase 8  (ordinal 9):  Concept extraction (LLM)     [execute, minijinja, template: replica/extract-concepts]
Phase 9  (ordinal 10): Method inference (LLM)       [execute, minijinja, template: replica/infer-methods, conditional: include_methods]
Phase 10 (ordinal 11): Corpus YAML generation       [populate, minijinja, template: replica/academic-corpus]
Phase 11 (ordinal 12): Curator gate                 [choice, conditional: mode == 'curated']
Phase 12 (ordinal 13): Regulation feedback          [feedback]
```

### PDCA mapping

| PDCA phase | Manifest phases | Present? |
|------------|-----------------|----------|
| **Plan** | (implicit — parameters define the plan) | Partial — no explicit plan step; parameters are documented in comments |
| **Do** | Phases 1–10 (search → extract → cache → concepts → methods → generate YAML) | Yes |
| **Check** | Phase 11 (Curator gate — human review of generated corpus) | Partial — human-in-the-loop only in curated mode; no automated quality check |
| **Act** | Phase 12 (Regulation feedback — emit outcome) | Partial — feedback is emitted but no iteration/revision loop |

### PDCA gaps

**Gap 1: No convergence block.** `scenario-builder.yaml` and `superforecasting.yaml` both have a top-level `convergence:` block with `threshold`, `max_iterations`, `convergence_field`, and `on_not_reached: escalate`. `replica-discovery.yaml` has **none**. The pipeline runs once and stops — there is no iteration if the Curator rejects (Phase 11 choice "modify" has no loop-back target).

**Gap 2: No automated quality check (Check phase is human-only).** The Curator gate (Phase 11) is the only check, and it is conditional on `mode == 'curated'`. In agentic mode, there is **no check at all** — the generated corpus.yaml goes straight to Regulation feedback. Compare `scenario-builder.yaml` which has an independent quality-gate step (Step 5, `scenario-builder/scenario-quality-gate` template) that runs regardless of mode.

**Gap 3: No iteration/revision loop (Act phase is feedback-only).** Phase 12 emits Regulation feedback but does not re-enter the pipeline. The Curator's "modify" choice (Phase 11) has no `restart_at` or loop-back — the manifest does not define what happens after "modify". Compare `scenario-builder.yaml` Step 7: "Re-enter scenario cycle if convergence is not met. Restarts at narrative generation (Step 3)."

**Gap 4: No template-validation step.** The pipeline generates `corpus.yaml` via `academic-corpus.j2` (Phase 10) but never validates that the output parses as `CorpusConfig`. The parse tests in `corpus_config_parse_test.rs` run in CI, not in the pipeline. A malformed template output would be discovered only when `corpus_build_persona` fails at runtime.

**Gap 5: No literary or exemplar discovery manifests.** Only the academic path has a discovery manifest. Literary replicas (woolf, hemingway, etc.) and exemplar replicas (gentle-lovelace, john-brooks) are hand-authored with no pipeline. This is acceptable if literary/exemplar replicas are inherently hand-curated (they require foundational_rules text and dimension weights that aren't discoverable), but the gap should be documented.

---

## 3. Template logic flow audit

### `corpus-base.j2` — shared scaffolding

Emits: `author`, `embedding`, `centroid_entity_ref`, `chunking`, `validation`, `budget` (block), `entities`, `methods`, `foundational_rules` (block), `works` (block), `class_specific` (block).

**Logic flow:** linear — no conditionals except the `entities`/`methods` presence checks. All blocks are overridden by class templates.

**Gap 6: `foundational_rules` block is in the base but only populated by literary/exemplar.** The base template emits the block comment, but the academic template overrides it with `foundational_rules: []`. This is correct but means the base template's `foundational_rules` comment is dead text for academic replicas. Minor — not a bug, just noise.

**Gap 7: `budget` block is in the base but always overridden.** No default budget in the base — every class template must override. This is correct (budget shape varies by class) but means the base template cannot render standalone. Minor — the base is documented as "not rendered directly."

### `academic-corpus.j2`

**Logic flow:** extends base → overrides `ontology`, `budget` (PerPage), `foundational_rules` (empty), `works` (academic shape), `class_specific` (`corpus_type: academic`).

**Gap 8: No `triple_classifier` emitted.** The academic template does not emit `triple_classifier`. `CorpusConfig` defaults it to `"h_mem-extractor"` via `#[serde(default = "default_triple_classifier")]`. This is correct (the default applies) but inconsistent with the literary/exemplar templates which emit it explicitly. Minor — the default is correct for academic.

### `literary-corpus.j2`

**Logic flow:** extends base → overrides `ontology` (GOLEM), `budget` (Flat), `foundational_rules` (critical essays), `works` (Gutenberg shape), `class_specific` (`corpus_type: literary` + `triple_classifier`).

**Gap 9: `dimension_centroids` not emitted.** Literary replicas don't use dimension centroids (woolf/hemingway/etc. have none). Correct — but the template should document *why* it's omitted, not just omit it silently. The `class_specific` block should note "literary replicas do not use dimension centroids — quality is assessed via stylometric signals, not dimension weights."

### `exemplar-corpus.j2`

**Logic flow:** extends base → overrides `ontology` (multi), `budget` (Absolute), `foundational_rules` (thematic), `works` (mixed), `class_specific` (`corpus_type` + `dimension_centroids` + `tag_sets` + `tag_weights` + `classifier` + `triple_classifier`).

**Gap 10 (the budget rigidity gap from §1):** The exemplar template hardcodes `Absolute` budget. `gentle-lovelace` (an exemplar) uses `Flat`. The template cannot regenerate `gentle-lovelace`. Fix: make the budget block accept a `budget_shape` parameter, or delegate budget to a per-replica block override.

**Gap 11: `fusion` field not emitted.** `CorpusConfig` has an optional `fusion: Option<FusionConfig>` field. No template emits it. This is correct (none of the current replicas use fusion) but the templates should document that fusion is not supported by the generator.

---

## 4. Summary of gaps

| # | Gap | Severity | Fix |
|---|-----|----------|-----|
| 1 | No convergence block in manifest | Medium | Add `convergence:` block with threshold, max_iterations, convergence_field |
| 2 | No automated quality check (Check is human-only) | Medium | Add a `replica/validate-corpus` template step that parses the generated YAML via `EmbedService::parse_config` |
| 3 | No iteration/revision loop (Act is feedback-only) | Medium | Add `restart_at` to the Curator "modify" choice; define loop-back target |
| 4 | No template-validation step in pipeline | Medium | Add a validation step after Phase 10 that calls `corpus_build_persona` with `--dry-run` or parses the YAML |
| 5 | No literary/exemplar discovery manifests | Low | Document that literary/exemplar are hand-curated by design (foundational_rules and dimension weights aren't discoverable) |
| 6 | Base `foundational_rules` comment is dead for academic | Low | No fix needed — block override is correct |
| 7 | Base `budget` block has no default | Low | No fix needed — documented as "not rendered directly" |
| 8 | Academic template doesn't emit `triple_classifier` | Low | No fix needed — default is correct |
| 9 | Literary template doesn't document why `dimension_centroids` is omitted | Low | Add a comment in `class_specific` block |
| 10 | Exemplar template hardcodes `Absolute` budget | **High** | Make budget shape configurable or delegate to per-replica override |
| 11 | `fusion` field not emitted by any template | Low | Document that fusion is not supported by the generator |

---

## 5. Recommendations

### High priority (blocks regeneration)

**Fix Gap 10 (exemplar budget rigidity):** The exemplar template's `budget` block should accept a `budget_shape` parameter (`"absolute"` | `"flat"`) and render the appropriate shape. This unblocks regenerating `gentle-lovelace` from the template.

### Medium priority (PDCA completeness)

**Fix Gaps 1–4 (PDCA loop):** Add to `replica-discovery.yaml`:
- A `convergence:` block (threshold 0.15, max_iterations 3, `on_not_reached: escalate`).
- A validation step after Phase 10 (ordinal 11.5) that renders `replica/validate-corpus` — a new template that calls `EmbedService::parse_config` on the generated YAML and reports parse success/failure.
- A `restart_at: 11` on the Curator "modify" choice (Phase 11) so revision loops back to corpus generation.
- The convergence_field should reference the validation step's result.

### Low priority (documentation)

**Fix Gaps 5, 9, 11:** Add comments documenting that literary/exemplar are hand-curated by design, that literary replicas omit dimension_centroids by design, and that fusion is not supported by the generator.
