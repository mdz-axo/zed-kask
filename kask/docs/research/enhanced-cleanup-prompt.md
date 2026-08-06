# Enhanced Prompt: Cleanup and Validation of Interdisciplinary Constraint-Forces Skills

**Prompt type**: agent-task (reliability-focused, multi-tool, bounded loops)
**Effort tier**: medium
**Original prompt**: "please proceed with work on items 1 through 12. re: #9 -- get the transcipt from kauffman's talk here on youtube through serpapi... re 10, yes please review the skills and validate them with skill maintenance and review them with bug hunt and graph audit and grill-me and essentialist. re #11, skip the rules change and then look at running diataxis diagram with the document specifications and mds.md in mind to recompose and consolidate and complete and clean up the document base."

## Enhanced Prompt

Execute the following 12 cleanup and validation tasks for the `gradient-seeded-recombination` (GSR) and `constraint-forces-recast` (CFR) skills scaffolded in `kask/registry/manifests/` and `kask/registry/templates/`. The skills' design documents are in `kask/docs/research/interdisciplinary-constraint-forces-*.md`. Execute in dependency order: items 1-5 (bug fixes) first, then 9 (Kauffman transcript, independent), then 10 (validation + review), then 12 (document consolidation). Skip item 11 (rules change).

### Bug fixes (items 1-5, sequential — each touches the manifests)

**Task 1 — Fix CFR loop seed advancement.** The CFR process manifest (`kask/registry/manifests/constraint-forces-recast.yaml`) step 1 uses `seed_concepts[0]` — it processes only the first seed. Step 9 (loop) re-enters at step 1 but the `input_mapping` doesn't advance the seed index. Fix: add a seed-index counter that increments on each loop iteration, so the evolutionary loop advances through the seed set. When all seeds are processed, the loop should test mutations from reflected rules (per the Phase F evolution design in the frameworks document). Acceptance: the manifest's loop step references an advancing seed index, not a hardcoded `[0]`.

**Task 2 — Wire or remove the `rater` input in CFR.** The `rater` input is declared in CFR's `inputs` but never consumed by any template's `input_mapping`. Either wire it into `cfr-three-criterion.j2` (so the template knows whether to expect reasoner output vs human judgment) or remove it from the manifest. Acceptance: no declared input is dead — every input is consumed by at least one template.

**Task 3 — Sync the T-spectrum into the GSR manifest.** The translational amendment (`kask/docs/research/interdisciplinary-constraint-forces-translational-amendment.md` §4.2) added the NCATS T-spectrum as a GSR substrate ontology, but the actual scaffolded `gradient-seeded-recombination.yaml` manifest's `ontology_registry` description doesn't mention it. Add the T-spectrum as a directed-process ontology option in the `ontology_registry` input description. Acceptance: the manifest's `ontology_registry` description mentions the T-spectrum (T0–T4) as a directed-process ontology that GSR can map gradients along.

**Task 4 — Wire `gsr-gradient-shapes.yaml` into the flow or mark it as reference-only.** The YAML exists as a RenderAct template but no step's `input_mapping` references it. The detect template (`gsr-detect.j2`) describes the 8 shapes inline. Either wire the YAML into the detect step's `input_mapping` (so the template can reference the taxonomy programmatically) or add a comment in the manifest noting it's a reference document for human readers, not a flow input. Acceptance: the YAML's role is explicit — either wired or documented as reference-only.

**Task 5 — Verify the `lisp.eval` input shape.** The CFR manifest's step 8 uses `compute_ref: lisp.eval` with `expression` and variable mappings (`hypervolume_delta`, `new_non_dominated`). Verify that the actual `lisp.eval` compute_ref accepts this exact input shape by checking the executor code. If it expects a different structure (e.g., a single `form` string with variables inlined), fix the manifest's `input_mapping` to match. Acceptance: the `lisp.eval` step's input_mapping matches the executor's expected input shape.

### Kauffman transcript (item 9, independent — can run in parallel with bug fixes)

**Task 9 — Fetch and analyze Kauffman's adjacent-possible talk.** Fetch the YouTube transcript for `https://www.youtube.com/watch?v=nEtATZePGmg` via SerpAPI. The SerpAPI key is `HKASK_SERPAPI_API_KEY` in `kask/.env`. The YouTube transcript fetch is wired in `hkask-mcp-corpus` (`corpus/discover/search.rs:107-260`, `youtube_video_transcript` engine) and `hkask-mcp-research`. Extract the central claim about the adjacent possible in one sentence. Then run that claim through the falsifiability admissibility gate (Phase C from the research report): state the concrete observation that would contradict it. Update the Pass 1 provenance table (A6) from "Hypothesis-tier, not verified" to either "corroborated" or "falsified" based on the transcript. Acceptance: A6 has a verified verdict (not "not verified") with the central claim stated in one sentence and a falsifier.

### Validation and review (item 10, after bug fixes 1-5)

**Task 10 — Validate and review both skills.** Run the following validation and review passes on both GSR and CFR:

1. **`skill-maintenance-validate`**: validate both skills against R1-R12, Z1-Z8, X1-X4, E1-E11. Fix any validation failures.
2. **`bug-hunt`**: run exploratory testing on both skills' manifests and templates — look for logic errors, missing error handling, dead inputs, contract mismatches.
3. **`graph-audit`** (code mode): extract the dependency graph of both skills' templates and check for cycles, orphans, missing edges.
4. **`grill-me`** (decoupled): escalate Recall → Mechanism → Rationale → Edge Cases → Synthesis on both skills' designs. The critic must be decoupled from the generator.
5. **`essentialist`**: run G1 (deletion test) → G2 (≤7 public surfaces) → G3 (contract trace) on both skills. The skills currently have 8 templates each (1 over the ≤7 limit) — the essentialist should evaluate whether the 8th surface is justified.

Acceptance: all 5 passes complete with verdicts recorded. Validation failures are fixed. The skills are installable (pass R1-R12, Z1-Z8, X1-Z4, E1-E11).

### Document consolidation (item 12, after all prior items)

**Task 12 — Recompose and consolidate the document base.** Run `diataxis-diagram` with the document specifications and `kask/docs/architecture/core/MDS.md` (Minimal Domain Specification — not "mds.md") in mind to recompose, consolidate, complete, and clean up the research document base in `kask/docs/research/`. The documents are:
- `interdisciplinary-constraint-forces-report.md` (Pass 1)
- `interdisciplinary-constraint-forces-frameworks.md` (Pass 2)
- `interdisciplinary-constraint-forces-skills.md` (Pass 3)
- `interdisciplinary-constraint-forces-translational-amendment.md` (amendment)

Goals: (a) consolidate the four documents into a coherent document set (not necessarily one file — Diataxis may recommend tutorial/explanation/reference/how-to splits); (b) ensure the documents reference the now-scaffolded skills (not just design specs); (c) add an index or README linking the documents; (d) ensure MDS category coverage (domain, composition, trust, lifecycle, curation) is visible in the document set. Acceptance: the document base is consolidated, indexed, and references the scaffolded skills; MDS categories are covered.

### Skipped

**Item 11 — `.rules` change**: skipped per operator instruction. The proposed trap ("Terms of art are not common words") remains in the translational amendment document for future reviewer consideration.

## Acceptance Criteria

- [ ] Tasks 1-5: all bug fixes applied to the manifests; no dead inputs; `lisp.eval` input shape verified.
- [ ] Task 9: Kauffman transcript fetched via SerpAPI; A6 verdict updated.
- [ ] Task 10: all 5 review passes complete; validation failures fixed; skills installable.
- [ ] Task 12: document base consolidated, indexed, MDS-covered.
- [ ] Item 11: skipped.

## Mutation Log

| Finding | Constraint force | Mutation |
|---|---|---|
| Vague scope ("items 1 through 12") | Guideline | Inlined all 12 items with acceptance criteria |
| Tool-use contract gap (SerpAPI path) | Evidence | Named the specific tool path (`hkask-mcp-corpus`, `youtube_video_transcript` engine) |
| File reference error ("mds.md") | Prohibition | Corrected to `MDS.md` at `kask/docs/architecture/core/MDS.md` |
| Missing acceptance criteria | Guideline | Added per-task acceptance criteria and overall checklist |
| Dependency ordering not explicit | Guideline | Added dependency order (1-5 → 9 → 10 → 12) |
| Item 11 ambiguity | Guideline | Explicitly skipped per operator instruction |
