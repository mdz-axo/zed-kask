# Infrastructure Jinja2 Template & YAML Manifest Audit — Checklist

## Phase 0 — Plan & Inventory (done)

- [x] **T0: Inventory infrastructure `.j2`/`.yaml` artifacts**
  - 0 `.j2` files outside `kask/registry/` (all 335 are skill-registry).
  - 3 in-scope YAMLs: `kask/corpus/replica/company-researcher.yaml`,
    `kask/corpus/replica/john-brooks.yaml`,
    `kask/corpus/pipeline-capabilities-researcher.yaml`.
  - 7 out-of-scope infra YAMLs (no Rust consumer / generator output / ops config).

## Phase 1 — Functional Role Discovery (done)

- [x] **T1: graph-audit (dual mode) — replica YAML cluster**
  - Confirmed `EmbedService::embed_corpus` → `CorpusConfig` parse path.
  - Mapped every `CorpusConfig` field to its YAML source.
  - Classified edges by constraint force; detected 7 Prohibition + 5 Guardrail.
  - Output: `tasks/phase1-functional-roles.md`.
- [x] **T2: graph-audit (dual mode) — pipeline manifest runbook**
  - Re-verified no Rust consumer.
  - Mapped 13 referenced MCP tools to Rust entry points (all match).
  - Output: `tasks/phase1-functional-roles.md`.

**Checkpoint C1**: ✅ consumer code unchanged; functional-role statements produced.

## Phase 2 — Logic & Semantics Audit (done)

- [x] **T3: pragmatic-semantics + pragmatic-cybernetics + essentialist — replica YAMLs**
  - 7 Prohibition, 5 Guardrail, 2 Guideline, 3 Hypothesis.
  - Output: `tasks/phase2-logic-semantics.md`.
- [x] **T4: pragmatic-semantics + pragmatic-cybernetics + essentialist — pipeline manifest**
  - 0 Prohibition, 3 Guardrail, 2 Guideline, 5 Hypothesis (3 resolved as Evidence).
  - Output: `tasks/phase2-logic-semantics.md`.

**Checkpoint C2**: ✅ Prohibition findings promoted to Phase 3.

## Phase 3 — Gap Interrogation (done)

- [x] **T5: sequential-inquiry + grill-me — replica YAMLs**
  - `company-researcher.yaml`: **Gap** (Rationale/Edge-Cases/Synthesis).
  - `john-brooks.yaml`: **Solid** (all 5 rounds).
  - Output: `tasks/phase3-gap-analysis.md`.
- [x] **T6: sequential-inquiry + grill-me — pipeline manifest**
  - Runbook: **Partial** (Recall/Mechanism/Rationale Solid; Edge-Cases/Synthesis Partial).
  - Confirmed `max_tokens: 512` = Rust default; `dedup_threshold: 0.89` diverges from default 0.85.
  - Output: `tasks/phase3-gap-analysis.md`.

**Checkpoint C3**: ✅ BUG-001 promoted to Phase 4.

## Phase 4 — Bug Hunt & Diagnosis (done)

- [x] **T7: bug-hunt expedition — replica YAMLs + `hkask-services-corpus`**
  - Wrote `tests/replica_persona_parse_test.rs` (2 tests).
  - Confirmed BUG-001 (`company-researcher.yaml` parse failure: missing `exemplar_count_min`).
  - Discovered BUG-002 (`john-brooks.yaml` `budget` silent PerPage mismatch).
  - Output: `tasks/phase4-bughunt-report.md`.
- [x] **T8: diagnose — fix `company-researcher.yaml` contract drift**
  - falsifiability: 3 ranked hypotheses; H1 (missing required) confirmed via `[DIAG-0001]`.
  - Fix applied: 8 changes conforming YAML to `CorpusConfig`.
  - Regression test flipped to positive assertion; instrumentation cleaned.
  - Post-mortem written.
- [x] **T9: diagnose — verify `john-brooks.yaml`**
  - Parses; BUG-002 is a contract defect (deferred to Phase 5).

**Checkpoint C4**: ✅ `cargo test -p hkask-services-corpus` 21 passed; `./script/clippy -p hkask-services-corpus` clean.

## Phase 5 — Architectural Refactor (done — decision only)

- [x] **T10: refactor-architecture decision — replica YAML cluster**
  - Candidate: `BudgetConfig` untagged-enum variant reorder.
  - Decision: **DEFER** with ADR (cross-crate, affects 7 out-of-scope registry YAMLs).
  - Other candidates (`deny_unknown_fields`, surface width, runbook `verify:`): **Reject** (scope/effort).
  - Output: `tasks/phase5-refactor-decision.md`.

**Checkpoint C5**: ✅ ADR recorded; no Phase 5 code changes (correct).

## Phase 6 — Convergence & Report (done)

- [x] **T11: convergence check + final report**
  - All slices converged: Slice A (replica) — fixed + tested; Slice B (runbook) — audited, no fix needed.
  - Per-artifact health scores: company-researcher 0.85, john-brooks 0.80, runbook 0.90.
  - Aggregate report: `tasks/audit-report.md`.
  - 6 recommended follow-ups documented.
