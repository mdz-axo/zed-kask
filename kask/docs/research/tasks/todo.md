# Todo: Cleanup and Validation of Interdisciplinary Constraint-Forces Skills

## Phase 1: Bug Fixes (sequential)

- [x] **Task 1**: Fix CFR loop seed advancement
  - [x] Loop step references `seed_index` (top-level context key), not `_loop.seed_index`
  - [x] All steps (1-5) now use `seed_concepts[seed_index | default(0)]` consistently
- [x] **Task 2**: Wire or remove the `rater` input in CFR
  - [x] `rater` wired into `cfr-three-criterion.j2` procedure body with `{% if rater == 'reasoner' %}` branch
- [x] **Task 3**: Sync T-spectrum into GSR manifest
  - [x] `ontology_registry` description mentions T-spectrum (T0–T4) as directed-process ontology
- [x] **Task 4**: Wire or document `gsr-gradient-shapes.yaml`
  - [x] Deleted — was dead surface area (unwired RenderAct duplicate of inline taxonomy in `gsr-detect.j2`)
- [x] **Task 5**: Verify `lisp.eval` input shape
  - [x] Fixed: uses `form` + `env` (not `expression` + flat variables), matching executor's expected shape
- [x] **Checkpoint 1**: All bug fixes applied; both manifests clean

## Phase 2: Kauffman Transcript (parallel with Phase 1)

- [x] **Task 9**: Fetch and analyze Kauffman's adjacent-possible talk
  - [x] YouTube transcript fetched via SerpAPI for `nEtATZePGmg`
  - [x] Central claim: biosphere innovates by combinatorial recombination, producing delay-and-burst trajectory
  - [x] Falsifier: domain where substrate grows but novelty rate does not accelerate
  - [x] A6 in Pass 1 provenance table updated to "admitted (provenance + admissibility verified), empirically untested"
- [x] **Checkpoint 2**: Kauffman transcript analyzed; A6 verified

## Phase 3: Validation and Review (after Phase 1)

- [x] **Task 10**: Validate and review both skills
  - [x] **bug-hunt**: 18 findings (5 blockers, 7 should-fix, 6 nits) — all blockers and key should-fixes fixed
  - [x] **graph-audit**: no cycles, no missing templates, no broken refs; 2 orphans found and deleted; delegate existence confirmed
  - [x] **grill-me** (decoupled): verdict pass-with-conditions; 5 conditions identified and 3 fixed (convergence formula, forcing operator enforcement, T1' on canonical artifact); 2 remaining (no-violations manifest guard, generate-then-test inefficiency) documented as limitations
  - [x] **essentialist**: G1 PASS (both skills), G2 PASS after deleting dead RenderAct templates (7/7 each), G3 PASS after deleting pass-through duplicates
  - [ ] `skill-maintenance-validate` against R1-R12, Z1-Z8, X1-Z4, E1-E11 — not run (requires validation tooling)
- [x] **Checkpoint 3**: Both skills reviewed through 4 passes; blockers fixed; skills structurally sound (formal validation deferred)

## Phase 4: Document Consolidation (after Phase 3)

- [x] **Task 12**: Recompose and consolidate the document base
  - [x] Diataxis classification applied (explanation/reference/how-to)
  - [x] Document index created (`kask/docs/research/README.md`)
  - [x] Architecture diagram created (`kask/docs/diagrams/architecture-constraint-forces-skills.md`)
  - [x] MDS categories covered (domain, composition, trust, lifecycle, curation)
  - [x] Documents reference scaffolded skills (not just design specs)
- [x] **Checkpoint 4**: Document base consolidated; all tasks complete

## Skipped

- [x] **Item 11**: `.rules` change — skipped per operator instruction

## Remaining Limitations (recorded honestly)

- `skill-maintenance-validate` not run (requires the validation tooling)
- Phase F evolution was single-rater (author); multi-rater study deferred
- M3 (shared substrate) not directly tested
- CFR forcing operator (minimality) is human-approximated, not mechanically enforced
- GSR convergence uses `1.0 - field_coverage` as hypotenuse (a proxy for gradient-map stability, not a direct measurement of site-count stability per the spec)
