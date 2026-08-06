# Task Plan: Cleanup and Validation of Interdisciplinary Constraint-Forces Skills

**Source**: `enhanced-cleanup-prompt.md`
**Date**: 2026-08-06
**PKO anchor**: `pko:Procedure` targeting `pko:ProcedureTarget` = "installable, validated, documented GSR + CFR skills"

## Overview

6 tasks across 4 phases. Bug fixes first (they touch the manifests that validation will check), then Kauffman transcript (independent), then validation + review (depends on bug fixes), then document consolidation (depends on all prior).

## Architecture Decisions

- **Dependency order**: bug fixes (1-5) → Kauffman (9, parallel) → validation (10) → docs (12). Validation must run after bug fixes because the validator checks the manifests that the bug fixes modify.
- **Item 11 skipped**: the `.rules` change is deferred per operator instruction.
- **Items 6-8 merged into task 10**: the honest limitations (single-rater, M3 untested, Kauffman unread) are addressed by task 9 (Kauffman) and recorded as limitations in the document consolidation (task 12). No separate tasks needed.

## Phased Task List

### Phase 1: Bug Fixes (sequential — each touches the manifests)

#### Task 1: Fix CFR loop seed advancement
- **Slice**: cfr-loop-seed-fix
- **Description**: Fix the CFR process manifest's loop step so it advances through the seed set instead of re-processing `seed_concepts[0]` every iteration.
- **Acceptance criteria**:
  - The manifest's loop step references an advancing seed index, not a hardcoded `[0]`
  - When all seeds are processed, the loop tests mutations from reflected rules
- **Verification**: read the manifest; confirm the loop step's `input_mapping` advances the seed index
- **Dependencies**: None
- **Files**: `kask/registry/manifests/constraint-forces-recast.yaml`
- **Scope**: XS

#### Task 2: Wire or remove the `rater` input in CFR
- **Slice**: cfr-rater-input-fix
- **Description**: The `rater` input is declared but never consumed. Wire it into `cfr-three-criterion.j2` or remove it.
- **Acceptance criteria**:
  - No declared input is dead — every input is consumed by at least one template
- **Verification**: grep the manifest's `inputs` against all templates' `input_mapping`; confirm every input is consumed
- **Dependencies**: None
- **Files**: `kask/registry/manifests/constraint-forces-recast.yaml`, `kask/registry/templates/constraint-forces-recast/cfr-three-criterion.j2`
- **Scope**: XS

#### Task 3: Sync T-spectrum into GSR manifest
- **Slice**: gsr-t-spectrum-sync
- **Description**: Add the NCATS T-spectrum (T0–T4) as a directed-process ontology option in the GSR manifest's `ontology_registry` input description.
- **Acceptance criteria**:
  - The manifest's `ontology_registry` description mentions the T-spectrum (T0–T4) as a directed-process ontology
- **Verification**: read the manifest; confirm the T-spectrum is mentioned
- **Dependencies**: None
- **Files**: `kask/registry/manifests/gradient-seeded-recombination.yaml`
- **Scope**: XS

#### Task 4: Wire or document `gsr-gradient-shapes.yaml`
- **Slice**: gsr-gradient-shapes-wiring
- **Description**: Either wire the gradient-shapes YAML into the detect step's `input_mapping` or document it as reference-only.
- **Acceptance criteria**:
  - The YAML's role is explicit — either wired into a step's input_mapping or documented as reference-only in the manifest
- **Verification**: read the manifest; confirm the YAML's role is explicit
- **Dependencies**: None
- **Files**: `kask/registry/manifests/gradient-seeded-recombination.yaml`
- **Scope**: XS

#### Task 5: Verify `lisp.eval` input shape
- **Slice**: cfr-lisp-eval-verify
- **Description**: Verify that the `lisp.eval` compute_ref accepts the `expression` + variable mappings shape used in CFR's step 8. If not, fix the manifest.
- **Acceptance criteria**:
  - The `lisp.eval` step's input_mapping matches the executor's expected input shape
- **Verification**: check the executor code for `lisp.eval`; confirm the input shape matches
- **Dependencies**: None
- **Files**: `kask/registry/manifests/constraint-forces-recast.yaml`, executor source
- **Scope**: S

**Checkpoint 1**: All bug fixes applied. Read both manifests; confirm no dead inputs, no hardcoded seed index, T-spectrum mentioned, gradient-shapes role explicit, `lisp.eval` shape verified.

### Phase 2: Kauffman Transcript (parallel with Phase 1)

#### Task 9: Fetch and analyze Kauffman's adjacent-possible talk
- **Slice**: kauffman-transcript-analysis
- **Description**: Fetch the YouTube transcript for `https://www.youtube.com/watch?v=nEtATZePGmg` via SerpAPI (`HKASK_SERPAPI_API_KEY` in `kask/.env`; `youtube_video_transcript` engine in `hkask-mcp-corpus`). Extract the central claim about the adjacent possible in one sentence. Run it through the falsifiability admissibility gate. Update A6 in the Pass 1 provenance table.
- **Acceptance criteria**:
  - A6 has a verified verdict (not "not verified") with the central claim in one sentence and a falsifier
- **Verification**: read the updated provenance table; confirm A6 is verified
- **Dependencies**: None (parallel with Phase 1)
- **Files**: `kask/docs/research/interdisciplinary-constraint-forces-report.md`
- **Scope**: M

**Checkpoint 2**: Kauffman transcript analyzed. A6 verdict updated. (Can be reached in parallel with Checkpoint 1.)

### Phase 3: Validation and Review (after Phase 1)

#### Task 10: Validate and review both skills
- **Slice**: skill-validation-review
- **Description**: Run 5 validation/review passes on both GSR and CFR: (1) `skill-maintenance-validate` against R1-R12, Z1-Z8, X1-Z4, E1-E11; (2) `bug-hunt` exploratory testing; (3) `graph-audit` (code mode) dependency graph; (4) `grill-me` (decoupled) Recall→Mechanism→Rationale→Edge→Synthesis; (5) `essentialist` G1→G2→G3. Fix validation failures.
- **Acceptance criteria**:
  - All 5 passes complete with verdicts recorded
  - Validation failures are fixed
  - Both skills pass R1-R12, Z1-Z8, X1-Z4, E1-E11
- **Verification**: run `skill-maintenance-validate`; confirm pass
- **Dependencies**: Tasks 1-5 (bug fixes must be applied before validation)
- **Files**: both skills' manifests, templates, SKILL.md files
- **Scope**: L (may need to break down if validation failures are extensive)

**Checkpoint 3**: Both skills validated and reviewed. All 5 passes complete. Skills installable.

### Phase 4: Document Consolidation (after Phase 3)

#### Task 12: Recompose and consolidate the document base
- **Slice**: document-consolidation
- **Description**: Run `diataxis-diagram` with the document specifications and `kask/docs/architecture/core/MDS.md` (Minimal Domain Specification) in mind to recompose, consolidate, complete, and clean up the research document base in `kask/docs/research/`. Consolidate the 4 documents (report, frameworks, skills, translational amendment) into a coherent document set. Add an index/README. Ensure MDS category coverage (domain, composition, trust, lifecycle, curation).
- **Acceptance criteria**:
  - Document base is consolidated, indexed, and references the scaffolded skills
  - MDS categories are covered
  - Diataxis classification (tutorial/explanation/reference/how-to) is applied
- **Verification**: read the consolidated document set; confirm index, MDS coverage, Diataxis classification
- **Dependencies**: Task 10 (validation must be complete so docs reference the final skill state)
- **Files**: `kask/docs/research/*.md`
- **Scope**: M

**Checkpoint 4**: Document base consolidated. All tasks complete.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| `lisp.eval` input shape is wrong | High — convergence step fails at runtime | Task 5 verifies against executor code before validation |
| Validation failures are extensive | Medium — Task 10 may need breakdown | If >5 validation failures, break Task 10 into per-skill tasks |
| Kauffman transcript is not available via SerpAPI | Low — fallback to reading the book | If SerpAPI fetch fails, note in A6 and defer to manual reading |
| Document consolidation loses content | Medium — 4 documents have overlapping content | Diataxis classification preserves content by type, not by source |

## Open Questions

1. **`lisp.eval` input shape**: does the executor accept `expression` + variable mappings, or a single `form` string? (Resolved by Task 5)
2. **Kauffman transcript quality**: will the YouTube transcript be usable for extracting the central claim? (Resolved by Task 9)
3. **Validation failure count**: how many failures will `skill-maintenance-validate` find? (Resolved by Task 10)

## Refinement History

No refinement iterations needed — the plan was stable on first decomposition (6 tasks, all XS-M-L, dependency-ordered, checkpoints present).
