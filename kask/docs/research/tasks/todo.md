# Todo: Cleanup and Validation of Interdisciplinary Constraint-Forces Skills

## Phase 1: Bug Fixes (sequential)

- [ ] **Task 1**: Fix CFR loop seed advancement
  - [ ] Loop step references advancing seed index, not hardcoded `[0]`
  - [ ] When all seeds processed, loop tests mutations from reflected rules
- [ ] **Task 2**: Wire or remove the `rater` input in CFR
  - [ ] No declared input is dead — every input consumed by ≥1 template
- [ ] **Task 3**: Sync T-spectrum into GSR manifest
  - [ ] `ontology_registry` description mentions T-spectrum (T0–T4) as directed-process ontology
- [ ] **Task 4**: Wire or document `gsr-gradient-shapes.yaml`
  - [ ] YAML's role is explicit (wired or reference-only)
- [ ] **Task 5**: Verify `lisp.eval` input shape
  - [ ] Step 8's input_mapping matches executor's expected shape
- [ ] **Checkpoint 1**: All bug fixes applied; both manifests clean

## Phase 2: Kauffman Transcript (parallel with Phase 1)

- [ ] **Task 9**: Fetch and analyze Kauffman's adjacent-possible talk
  - [ ] YouTube transcript fetched via SerpAPI for `nEtATZePGmg`
  - [ ] Central claim stated in one sentence
  - [ ] Falsifier stated
  - [ ] A6 in Pass 1 provenance table updated to verified verdict
- [ ] **Checkpoint 2**: Kauffman transcript analyzed; A6 verified

## Phase 3: Validation and Review (after Phase 1)

- [ ] **Task 10**: Validate and review both skills
  - [ ] `skill-maintenance-validate` passes R1-R12, Z1-Z8, X1-Z4, E1-E11 for both skills
  - [ ] `bug-hunt` exploratory testing complete; logic errors fixed
  - [ ] `graph-audit` (code mode) dependency graph checked; no cycles/orphans
  - [ ] `grill-me` (decoupled) Recall→Mechanism→Rationale→Edge→Synthesis complete
  - [ ] `essentialist` G1→G2→G3 complete; 8th surface justified or merged
- [ ] **Checkpoint 3**: Both skills validated and reviewed; installable

## Phase 4: Document Consolidation (after Phase 3)

- [ ] **Task 12**: Recompose and consolidate the document base
  - [ ] `diataxis-diagram` run with MDS.md categories in mind
  - [ ] 4 documents consolidated into coherent set (tutorial/explanation/reference/how-to)
  - [ ] Index/README added to `kask/docs/research/`
  - [ ] MDS categories covered (domain, composition, trust, lifecycle, curation)
  - [ ] Documents reference scaffolded skills (not just design specs)
- [ ] **Checkpoint 4**: Document base consolidated; all tasks complete

## Skipped

- [x] **Item 11**: `.rules` change — skipped per operator instruction
