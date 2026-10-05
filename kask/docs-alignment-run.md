# kask/docs deep-alignment run — 2026-10-05

Status: IN PROGRESS — updated per docs-set; every decision recorded as made.
Goal record: `f277c85d-2621-4161-803f-14e6e36b6cb9` (intake prediction 0.8).

## Intake bindings (recorded before any editing)

- **Count target**: fewer-than-75 cap — `kask/scripts/check-docs-count-gate.sh` fails on `actual >= cap`; the STANDARDS §3 canonical sentence is the parsed authority. Working target 60 is condensation direction, not a gate. The operator named no different number at intake; the run prompt's "≤ 75" restates the skill's <75 — skill + script are the authority the prompt points at.
- **Authorities**: `architecture/DOCUMENTATION_STANDARDS.md` (writing quality, §10 checklist, four perspective tests incl. Gentle agent-correctness), `architecture/core/MDS.md` (crate/server inventory), `README.md` (portal + lifecycle ledger).
- **Commit granularity** (technical decision): one pathspec'd commit per docs-set; `git diff --cached --stat` checked before each.
- **Component reference models enumerated at intake** (aligned against, not discovered mid-edit):
  1. `research/cybernetic-nervous-system-reference-model.md` — loop layers A/B/C, INV1–INV6 (admitted 2026-09-30)
  2. `research/kanban-board-reference-models.md` — Wekan/Planka/Kan/Kanboard; rules R1/R6 cited by `.rules`
  3. `research/cmp-gap-methodology.md` — CMP term structures (cited by the `cmp-term-structure` skill)
  4. `research/syntax-semantic-probabilistic-deterministic-space.md` — region space; the `region-routing` skill's reference model
  5. `research/chunking-for-rag-research.md` — RAG chunking prior art
  6. `research/deterministic-vs-probabilistic-compute-routing.md` — routing hypotheses (no implementation claimed)
  7. `research/artificial-curiosity-capability-space.md` — curiosity mechanisms (no capability claimed)
  8. Per-server published models cited by `reference/mcp-servers/*.md` (FactSet Universal Screening/Bloomberg EQS/GuruFocus — companies; Reduct.video/MovieLabs OMC — media; Schwartz/Tetlock/Chermack — scenarios; Rother kata — kata-kanban; ISA 500 — grounding-verify; Diataxis/Gentle/Carroll — the docs system itself)
- **Skills composed**: doc-update (spine), metacognition (per-set predictions), kata-improvement (per-set checkpoints), diataxis-diagram (inside recompose, only if a recomposed section's diagram structure changed).

## Ground (measured 2026-10-05)

- **Before count: 75 files** (74 md + `architecture/principle-constraints.yaml`) — AT the fewer-than-75 cap. `check-docs-count-gate.sh` **FAILS on entry** (red gate).
- **README corpus line records 74** (measured 2026-10-01) — stale by exactly one: `plans/agent-loop-guardrails-plan.md` added 2026-10-02 (`313d1b2c11`) with no corpus bump and no portal row.
- **Portal drift**: the README plans table carries 2 rows; the tree holds 4 plans. `agent-loop-guardrails-plan.md` and `logisheets-spreadsheet-capability-plan.md` are unreachable from the portal.
- Git state: `kask/docs` clean; nothing staged.
- Prior alignment: 2026-09-28 deep realignment (525 sampled citations resolved); **P1 fold already executed** 2026-10-01 — ledger row present; repo sweep shows 3 hits, all legitimate historical mentions (ledger tombstone, plan decision row, sibling §4 self-reference). No action.
- Docs-sets: research/ (10), plans/ (4), architecture/ 8 + core/ 3 incl. yaml (11), reference/ 7 + mcp-servers 14 + skills 1 (22), diataxis/ (19), diagrams/ 5 + root 3 (8). Total 75.

## Kata framing (run level)

- **Direction**: deep alignment of the docs tree with current code and the enumerated reference models, under the cap.
- **Current condition**: 75 files, gate red, README measurement stale by 1, two plans unportaled, P2 pending.
- **Target condition**: every doc exactly one terminal decision; count ≤ 74 with the gate green; every deletion swept + ledgered; recomposed claims carry resolving file:line; README reconciled (corpus line, checklist, portal rows, ledger).
- **Focus obstacle**: the red count gate — blocks everything; the only evidence-supported deletion candidate is P2.
- **Next step**: research/ docs-set.

## Metacognition — run-level prediction (recorded before compare)

- Predict: 65–75% of 75 docs VERIFIED-CURRENT via change-history + targeted spot-checks (tree deeply realigned 2026-09-28 and actively maintained since); 4–8 docs RECOMPOSED (README certainly — corpus/checklist/portal/ledger; logisheets plan §1 status possibly; 1–3 reference docs with count drift); 1–2 DELETED (P2 executed; plans refuted as deletion candidates — both live and load-bearing). Final count 73–74.
- Confidence: 0.7.

## Pending operator rulings carried in (not re-litigated by this run)

- **P3** (fold the 8 diataxis `explanation.md` files into set references): deferred — priced, recommended against (dissolves the standards-prescribed Diataxis explanation layer; an OUGHT taxonomy change, not condensation). Successor pre-named: the set references' opening sections.
- **60-file working target**: unreachable without P3 plus further rulings; reported as condensation direction, not closed by this run.

---

## Docs-set 1: research/ (10 docs)

### Metacognition prediction (before compare)

- Predict: 8 of 10 VERIFIED-CURRENT (research records stable by nature — hypotheses/prior-art docs make no current-code claims); 1 DELETED (P2 — media-lead onboarding, role-closure evidence); 0–1 RECOMPOSED (d-seam-audit if its worklist lags the live DIVERGENCE.md seam set). Confidence: 0.75.

### P2 decision evidence (recorded before execution)

- Role liveness: `rg 'media[- _]server[- _]lead'` over the repo minus docs → **0 hits**. Local agent registry: `local_critic`, `local_extractor`, `local_narrator` — no media lead. No code, skill, or D-seam references the role.
- Media crate commits since 2026-09-25: **0**. All five §6 focus areas (Reduct write surface, transcript re-alignment, face-recognition decision, embedding-error typing, DNS-rebinding ruling) untouched.
- Content duplication: §1 inventory + architecture duplicated in `reference/mcp-servers/media.md` (per-tool rows) and the crate README; §3–4 principles/rules duplicated in `.rules`, STANDARDS, and the product-manager/program-manager skills; §6 current-conditions are cited FROM the crate README (`:340-344`, `:252-259`, `:379-383`, `:78-85`) and `media.md:560` — the canonical docs already carry them.
- Inbound references: 2 (README portal row; the plan's P2 row). Both updated in this set's commit.
- **Decision: DELETED** — successors: `reference/mcp-servers/media.md` + `kask/mcp-servers/hkask-mcp-media/README.md` + git history. Executed on role-closure evidence; vetoable at operator review (single-commit revert).

### Compare evidence

- Frontmatter: 9 of 10 research docs carry the full 7-field header; `two-symptom-latency-findings.md` carried none (the 2026-10-01 concurrent-stream addition skipped it).
- `check-d-seam-audit-ledger.sh` FAILED on entry: ledger held retired D85 (retired 2026-09-30, upstream parity) and lacked D86/D87/D89 (landed 2026-09-30/10-03). Live seam count measured at 68; DIVERGENCE.md's own header claimed "D1–D86 — 66 active; 20 retired" — stale on all three numbers (mechanical retired count: 21, including the combined `**D17, D19**` entry).
- P1 fold verified landed: sibling carries §4 "The three-party collaboration overlay (folded 2026-10-01)" at `syntax-semantic-probabilistic-deterministic-space.md:314`.
- Kanban reference model R1 verified live in code: `KANBAN_BOARD_NAME_MAX_CHARS` at `kask/mcp-servers/hkask-mcp-kata-kanban/src/types.rs:16` cross-cites "reference model R1"; implementation record §10 present.
- Two-symptom doc: §1.1 instrument table claimed `[DIAG-anr]` "alive" — the probe was removed per its own contract 2026-10-02 (recorded in the doc's own §7.11); the round-1 table needed the annotation. The doc was also absent from the README Research table (unportaled concurrent-stream addition).
- Research-record docs (hypotheses/prior-art, no current-code claims): verified stable by nature + frontmatter + last_updated (2026-09-28/10-01, all post-realignment).

### Decisions

| Document | Decision | Basis |
| --- | --- | --- |
| `media-server-lead-onboarding.md` | **DELETED** (P2 executed) | Role-closure evidence: 0 live role presence repo-wide, 0 media-crate commits since 2026-09-25, all five §6 focus areas untouched; content duplicated in `media.md` + crate README + `.rules`/skills. Successors named in the README ledger tombstone. Vetoable by single-commit revert. |
| `d-seam-audit.md` | **RECOMPOSED** | D85 retired row removed; D86/D87/D89 registered `pending` per the doc's own registration pattern (pins named from the DIVERGENCE rows, no inherited verdicts); snapshot updated to live count 68; frontmatter 1.0.2→1.0.3, last_updated 2026-10-05. |
| `two-symptom-latency-findings.md` | **RECOMPOSED** | 7-field frontmatter added (was absent); §1.1 `[DIAG-anr]` status cell annotated with its 2026-10-02 contract removal (cross-ref §7.11); README portal row added (was unportaled). |
| `syntax-semantic-probabilistic-deterministic-space.md` | **VERIFIED-CURRENT** | P1 fold target; §4 overlay present (:314); last_updated 2026-10-01; structure + change-history verified. |
| `kanban-board-reference-models.md` | **VERIFIED-CURRENT** | R1 rule live in code with mutual cross-citation (`types.rs:16`); implementation record §10; last_updated 2026-09-28. |
| `cybernetic-nervous-system-reference-model.md` | **VERIFIED-CURRENT** | Admitted 2026-09-30, citations resolved (research run 4b9a3b3ed05bf860); research record, stable by nature. |
| `cmp-gap-methodology.md` | **VERIFIED-CURRENT** | Process spec; worked instances live in companies-mcp reports; no current-code claims to drift. |
| `artificial-curiosity-capability-space.md` | **VERIFIED-CURRENT** | Research record (hypotheses/pilot; no capability claimed). |
| `chunking-for-rag-research.md` | **VERIFIED-CURRENT** | Prior-art study; recommendations only, not implemented. |
| `deterministic-vs-probabilistic-compute-routing.md` | **VERIFIED-CURRENT** | Hypotheses only; no routing implemented. |

Cross-set edits in this cycle: README (corpus line → 74 measured 2026-10-05; checklist date; LogiSheets portal row; two-symptom Research row replacing the deleted onboarding row; P2 ledger tombstone), cybernetic plan P2 row → executed, DIVERGENCE.md header reconciliation (D1–D89, 68 active, 21 retired — a registry-count claim, Phase 5.3).

### Kata checkpoint (five questions)

1. **Target**: every research doc one decision, P2 resolved, count gate green — met pending the gate re-run below.
2. **Actual**: 10 docs; 2 real defects (audit-ledger seam mismatch; missing frontmatter+portal on the 10-01 addition) + the P2 deletion.
3. **Obstacles**: the red count gate (resolved via P2); a systemic one — concurrent-stream doc additions bypass frontmatter/portal/corpus-line discipline (two for two: two-symptom, guardrails plan).
4. **Next step**: plans docs-set — the guardrails plan's status reads "Closed — executed and audited 2026-10-02"; decide keep-as-record vs delete-as-implemented (DIVERGENCE.md:212 and `kask/lean/repetition_guard_spec.lean:22` cite its §7/§12 — references must survive either way).
5. **Check point**: gates re-run immediately; commit hash recorded in this report.

### Metacognition gap (predicted vs measured)

Predicted 8/10 VERIFIED-CURRENT, 1 DELETED, 0–1 RECOMPOSED (confidence 0.75). Measured: 7/10 VERIFIED-CURRENT, 1 DELETED, 2 RECOMPOSED. The d-seam-audit recomposition was the predicted 0–1; the two-symptom recomposition was not predicted — the missed class is concurrent-stream additions skipping docs discipline (frontmatter, portal row, corpus line). Correction for the remaining sets: check additions by add-date, not content alone.
