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

---

## Docs-set 2: plans/ (4 docs → 3)

### Metacognition prediction (recorded after the "Closed" status observation, before the §1/§10/§13 reads)

- Predict: guardrails DELETE (implemented-design class — goedel/inference-plan precedent) if §13's four open questions carry resolutions; logisheets VERIFIED-CURRENT (Phase 5 record current with the shipped `WorkbookWhatIf` surface); repair VERIFIED-CURRENT via change-history with a possible `last_updated` metadata fix; cybernetic VERIFIED-CURRENT. Confidence: 0.7.

### Compare evidence

- Guardrails §1 execution record: Slice 0 (lisp_eval teaching) + Slice 1 (C2 repetition stop-loss) landed 2026-10-02 (`1d4bff6fb2`); Slice 2 (C1a watchdog) landed and was REMOVED the same day by operator directive (three live false positives killed working turns); Slice 3 (C1b ladder) charter dropped (root cause removed by D42; zero live occurrences). §13's four open questions all carry 2026-10-02 resolutions — (1) chartered+landed, (2) moot with C1a, (3) resolved (shared ladder contract), (4) superseded by the operator-directed audit. §12: Lean spec machine-checked (five theorems, axiom audit `[propext]` only).
- Inbound references (2): `DIVERGENCE.md:212` (the tool-retry row cites "the plan §7" — the row itself carries the full design-revision narrative) and `kask/lean/repetition_guard_spec.lean` (§12 mapping + "tests named in the plan document"). Both repointed to git history + the DIVERGENCE row.
- Logisheets: Phase 5 record "COMPLETE (2026-09-18)" at `:575` — `portfolio_what_if`'s presentation choice and §6's no-hidden-threshold rule recorded; last synced 2026-09-28 (`14becc23f2`); live design authority (MDS crate table + `hkask-spreadsheet` source comments cite §5.1).
- Repair plan: 13 content commits since 2026-10-01 (through `5c68d3603b`, 2026-10-02), each code change carrying its docs sync; `last_updated` read 2026-09-30 — one content cycle stale.
- Cybernetic: synced through 2026-10-02; P2 row updated by this run; `last_updated` 2026-10-05, version 0.3.9.

### Decisions

| Document | Decision | Basis |
| --- | --- | --- |
| `agent-loop-guardrails-plan.md` | **DELETED** | Status "Closed — executed and audited 2026-10-02"; every workstream shipped/removed/dropped; all §13 questions resolved. Successors: DIVERGENCE.md's tool-retry row (contract + design revision + pin names), `kask/lean/repetition_guard_spec.lean`, the lisp_eval teaching contract (hkask-lisp pins + `.rules`), git history (full kata record + C1a post-mortem). The C1a re-land bar is carried in the ledger tombstone. |
| `logisheets-spreadsheet-capability-plan.md` | **VERIFIED-CURRENT** | Phase 5 COMPLETE record current with the shipped surface; live design authority; portal row added and enriched (Phases 0–5). |
| `hkask-core-mcp-repair-improvement-plan.md` | **VERIFIED-CURRENT** | 13 synced content commits through 2026-10-02; frontmatter corrected (`last_updated` 2026-09-30 → 2026-10-02, version 0.10.0 → 0.10.1). |
| `cybernetic-nervous-system-alignment.md` | **VERIFIED-CURRENT** | Synced through 2026-10-02; P2 row marked executed (this run); `last_updated` 2026-10-05, version 0.3.9. |

Cross-set edits: `DIVERGENCE.md:212` citation repointed; Lean spec comment repointed; README corpus line → 73, checklist → 73, guardrails ledger tombstone (with the C1a re-land bar), LogiSheets portal row enriched.

### Kata checkpoint (five questions)

1. **Target**: one decision per plan; the closed-plan question resolved. Met.
2. **Actual**: 1 DELETED (closed-on-arrival), 3 VERIFIED-CURRENT; 2 live citations repointed; 1 metadata correction.
3. **Obstacles**: none new. Pattern noted: a plan that lands and closes the same day is a condensation candidate the moment it closes — and its addition bypassed the corpus-line discipline entirely (the gate was red before this run started).
4. **Next step**: architecture docs-set (the authorities: MDS, STANDARDS, zed-host plan).
5. **Check point**: count gate at 73 verified below; commit hash recorded in this report.

### Metacognition gap (predicted vs measured)

Predicted (post-status-observation, pre-read): guardrails DELETE, the other three VERIFIED-CURRENT, one metadata fix. Measured: exactly that — zero decision misses. Process miss outside the set: the set-1 commit initially failed on an untracked pathspec (`kask/docs-alignment-run.md` — pathspec commits need untracked files staged first); recovered by staging before the pathspec commit.

---

## Docs-set 3: architecture/ (11 docs)

### Metacognition prediction (before compare)

- Predict: the authorities (MDS, STANDARDS, zed-host plan) verify current via the 2026-09-28 realignment + active maintenance; 1–2 metadata/frontmatter misses from recent additions (the compaction spec, ratified 2026-09-29, is the prime suspect); possible count drift where the tree grew after 09-28 (skills, crates, servers). Confidence: 0.7.

### Compare evidence

- Frontmatter: 9 of 10 md carry the 7-field header; `compaction-pipeline-spec.md` carries none (ratified 2026-09-29 without it).
- MDS counts verified against the tree: 19 crates (18 `hkask-*` + `kask_bridge`) ✓; 13 MCP servers ✓ (experimentation and spreadsheet both present in the tables).
- Skills drift: tree holds 61 skills / 57 template namespaces / 276 `.j2` + 2 `.jinja`; the registry claimed 60/56/274 — `mcp-tool-review` (added 2026-10-02, `a353d629f5`) bumped all three counts and its registry row was absent.
- Diagram registry: mechanical count — 108 mermaid blocks, 107 real records (108 `id: DIAG-` lines minus the standards' §4.2 format placeholder), 107 index rows. The gap: the compaction spec's own pipeline diagram (landed 2026-09-29 unregistered). The index prose claimed "109 blocks; 108 carry records; the one exception is STANDARDS §3" — wrong on all three counts (STANDARDS §3 carries `DIAG-STD-001`, verified 2026-09-28; the real exception was the compaction diagram).
- Compaction spec content verified against code: `Thread::compact` (`thread.rs:2892`), `perform_compaction_if_needed` (`:3723`), `perform_prompt_too_large_rescue` (`:3792`) — the three entry points exist as specified; pin sample verified (`pre_shrink_windows_oversized_results_and_keeps_small_ones` at `kask_compaction.rs:930`, `test_manual_compact_forces_summary` at `thread.rs:10394`, `test_prompt_too_large_rescue_compacts_and_retries` at `:11417`).
- `principle-constraints.yaml`: consumed by `check-principle-constraints.sh` — runs, reports "Inventory valid" — live.
- Remaining docs (magna-carta, PRINCIPLES, functional-interaction-spec, memory-system-specification, skills-and-composition, standardized-artifact-storage, zed-host-architecture-plan, MDS, STANDARDS): `last_updated` 2026-09-28/09-30, subjects stable, no drifted claims in targeted checks (skills-and-composition carries no count claims; the zed-host plan's logisheets reference remains true).

### Decisions

| Document | Decision | Basis |
| --- | --- | --- |
| `compaction-pipeline-spec.md` | **RECOMPOSED** (metadata + diagram registration) | 7-field frontmatter added; the pipeline diagram registered as `DIAG-ARCH-COMPACTION-001` (verified against `thread.rs:2892,3723,3792` + `kask_compaction.rs:930`); content verified current (entry points + pin sample). |
| `core/MDS.md` | **VERIFIED-CURRENT** | 19 crates + 13 servers match the tree exactly. |
| `DOCUMENTATION_STANDARDS.md` | **VERIFIED-CURRENT** | §3 cap sentence parses (the gate reads it); §10 checklist current; no drift found. |
| `core/PRINCIPLES.md`, `core/magna-carta.md`, `functional-interaction-spec.md`, `memory-system-specification.md`, `skills-and-composition.md`, `standardized-artifact-storage.md`, `zed-host-architecture-plan.md` | **VERIFIED-CURRENT** | `last_updated` 2026-09-28+; subjects stable; targeted checks found no drift. |
| `principle-constraints.yaml` | **VERIFIED-CURRENT** | Live governance inventory; its check script runs and reports valid. |

Cross-set edits (the count facts changed, so every carrier updates in the same change): `reference/skills/README.md` (counts 61/57/276, the `mcp-tool-review` row, frontmatter 0.41.0), `DIAGRAMS_INDEX.md` (row + corrected prose + frontmatter 2.7.0), README portal rows (skills, diagrams) + the checklist diagram-parity item (108, tree-verified 2026-10-05).

### Kata checkpoint (five questions)

1. **Target**: 11 docs, one decision each; the frontmatter gap closed; counts reconciled. Met.
2. **Actual**: 1 RECOMPOSED, 10 VERIFIED-CURRENT; three registries reconciled (skills, diagrams, plus DIVERGENCE in set 1).
3. **Obstacles**: the concurrent-stream addition class again — compaction spec (09-29: no frontmatter, no diagram record), mcp-tool-review (10-02: no registry row, no count bump). Four instances now: systemic.
4. **Next step**: reference/ docs-set (22 docs; per-server tool counts vs pin tests).
5. **Check point**: gates re-run below; commit hash recorded.

### Metacognition gap (predicted vs measured)

Predicted 1–2 metadata misses + possible count drift. Measured: exactly that — the compaction frontmatter + diagram record, three skills numbers + a missing row, and three wrong numbers in the diagram-index prose. The prediction held; the cause is confirmed systemic (additions landing without the docs-discipline touchpoints). Candidate process fix, recorded as a finding for the operator: a CI check that new `.md` files under kask/docs carry the 7-field header and that count-carrying registries are re-measured — `check-docs-count-gate.sh` already models the pattern for the file count.

---

## Docs-set 4: reference/ (22 docs)

### Metacognition prediction (at set open, after the `939397ab19` commit-message observation, before the pin-sum measurement)

- Predict: per-server docs current (maintained with their servers); 1–2 count drifts where the research 26→27 correction didn't propagate (the fleet total is the prime suspect); the 7 top reference docs verify via change-history. Confidence: 0.7.

### Compare evidence

- Pin-sum measurement: `40+26+15+6+27+98+18+32+27+19+2+90+9 = 409`; the fleet README claimed **408** with an arithmetic line carrying research at 26 — the `939397ab19` correction fixed `research.md` but not the fleet README (total, table row, verification bullet, arithmetic — four spots, one doc).
- Per-server doc counts all match their pins (companies 40, corpus 26, curator 15, experimentation 6, kata-kanban 27, media 98, portfolio 18, prediction-markets 32, research 27, scenarios 19, spreadsheet 2, swarm 90, training 9).
- The concurrent stream's pin unification (`ede1ba0ca8`, 2026-10-05 10:05 — landed mid-run) superseded the heterogeneous per-row verification methods with the shared `tool_surface_pin!` macro + CI gate (`check-mcp-registration.sh`, `kask-invariants.yml`) — the README's method section described the pre-unification landscape with line numbers the unification moved.
- The evolution→experimentation rename sweep verified complete (zero stale references; the `evolution_sensor_returns_none_on_broken_source` citation in `loop-register.md:646` is the test's live name at `sensor_provider.rs:860`).
- Top reference docs (kask-settings, lisp-eval-dialect, lora-training-catalog, ontology-bridge, regulation-spans, testing-protocol, upstream-rebase-process): `last_updated` 2026-09-28/09-30, last commits are sync commits, no drift signals.
- High-churn server crates (corpus 22, research 17, companies 11 commits since 09-28): subjects are behavior-level with docs synced in-commit; no tool-surface changes (pin counts unchanged).

### Decisions

| Document | Decision | Basis |
| --- | --- | --- |
| `mcp-servers/README.md` | **RECOMPOSED** | Fleet total 408→409 (research 26→27 in the table row and arithmetic); the verification-method section rewritten to the unified `tool_surface_pin!` landscape with live pin-site citations (all 13 verified 2026-10-05); frontmatter 0.42.0. |
| `research.md` | **VERIFIED-CURRENT** | "Tools: 27" — corrected by `939397ab19`; the drift was only in the fleet README. |
| `companies.md`, `corpus.md`, `curator.md`, `experimentation.md`, `kata-kanban.md`, `media.md`, `portfolio.md`, `prediction-markets.md`, `scenarios.md`, `spreadsheet.md`, `swarm.md`, `training.md` | **VERIFIED-CURRENT** | Counts match pins; names pinned by the unified macro; behavior commits carried docs syncs; curator/training created 2026-10-01; experimentation current post-rename. |
| `kask-settings.md`, `lisp-eval-dialect.md`, `lora-training-catalog.md`, `ontology-bridge.md`, `regulation-spans.md`, `testing-protocol.md`, `upstream-rebase-process.md` | **VERIFIED-CURRENT** | Change-history: sync commits through 09-28/09-30; no drift signals in targeted checks. |
| `skills/README.md` | **RECOMPOSED** (set 3, cross-set) | Counts 61/57/276 + the `mcp-tool-review` row. |

### Kata checkpoint (five questions)

1. **Target**: 22 docs, one decision each; the fleet count reconciled. Met.
2. **Actual**: 1 RECOMPOSED (fleet README), 21 VERIFIED-CURRENT; the concurrent pin-unification absorbed mid-set (the method section rewritten to the new landscape in the same pass).
3. **Obstacles**: concurrent-stream coordination — the unification landed mid-run; its docs implications adopted rather than raced.
4. **Next step**: diataxis + diagrams/root sets (change-history + registry parity, partially verified already).
5. **Check point**: gates at commit; hash recorded.

### Metacognition gap (predicted vs measured)

Predicted 1–2 count drifts with the fleet total as prime suspect. Measured: exactly one drift cluster (four spots in one doc) plus the unification-driven method-section staleness (not predicted — a mid-run concurrent change). Lesson: in a shared tree, re-verify registry-style docs against the LIVE mechanism at edit time, not the mechanism at set-open.

---

## Docs-set 5: diataxis/ (20 files) and diagrams/ + root (8 files)

### Metacognition prediction (before compare)

- Predict: all diataxis sets VERIFIED-CURRENT via the 2026-09-28 realignment (frontmatter complete, subjects stable); 0–1 stale-class hits where the high-churn crates (kask_bridge 27, mcp-server 9, regulation 7 commits) deleted or renamed something the references document; the diagram files verify via registry parity (already established in set 3). Confidence: 0.75.

### Compare evidence

- Frontmatter: 20/20 diataxis files carry the 7-field header; all `last_updated` 2026-09-28.
- Deleted-surface check: the strategy evaluator (deleted `3267b2df20`) appears in zero regulation docs — never documented, no stale claim. The pin unification's `build_support` module is out of the mcp-server docs' scope (they document the framework, not build pins).
- Changed-default check: the inference reference's DeepInfra mentions are provider descriptors/adapters (legitimate), not the embedding default — no stale default.
- Citation check: `SignalMetric` cited at `loops/signals.rs:14-112` — the enum lives at `:14` ✓ current.
- INDEX claim "19 retained artifacts across 10 cross-cutting sets" verified by arithmetic: 19 set files (2+2+3+2+2+1+2+2+1+2) across 10 sets.
- Diagrams: registry parity 108 records = 108 rows (set 3); the redraw-grid removal (`3f6f844703`) left zero doc claims (no `redraw` hits in diagrams/diataxis); the consolidated-files count 34 verified (13+3+4+6+8).
- Loop register: 25 canonical loops (L1–L25; the `L14,L15` row is a combined row, not a 26th loop — the README's "25 system loops" claim is correct); L24/L25 current through 2026-10-01; the `evolution_sensor_returns_none_on_broken_source` citation is the test's live name (`sensor_provider.rs:860`).

### Decisions

| Document | Decision | Basis |
| --- | --- | --- |
| All 19 diataxis set files + `INDEX.md` | **VERIFIED-CURRENT** | Frontmatter complete; subjects stable; no deleted-surface or changed-default claims; citation spot-checks resolve; INDEX count verified. P3 (explanation folds) remains deferred to the operator. |
| `DIAGRAMS_INDEX.md` | **RECOMPOSED** (set 3) | Row + corrected prose + parity 108=108. |
| `diagrams/architecture.md`, `kanban.md`, `mcp-dispatch.md`, `swarm.md`, `ui-widgets.md` | **VERIFIED-CURRENT** | Registry parity holds; record/block 1:1 per file (mechanically counted); no orphaned claims from the redraw-grid removal. |
| `loop-register.md` | **VERIFIED-CURRENT** | 25 canonical loops; L24/L25 current; the sensor-test citation is live. |
| `README.md` | **RECOMPOSED** (across sets) | Corpus line 74→73 (measured 2026-10-05); checklist count 73; portal rows +2 plans +two-symptom −onboarding; ledger +2 tombstones; skills/diagrams rows reconciled; diagram-parity item 108; citations item refreshed with this run's verification. |

### Kata checkpoint (five questions)

1. **Target**: every remaining doc one decision; all gates green. Met.
2. **Actual**: 25 VERIFIED-CURRENT, 2 RECOMPOSED (README across sets, DIAGRAMS_INDEX in set 3); zero deletions needed in these sets.
3. **Obstacles**: none — the 09-28 realignment's quality held for the diataxis layer.
4. **Next step**: final gates + convergence check + report.
5. **Check point**: gates below; commit hash recorded.

### Metacognition gap (predicted vs measured)

Predicted all-current with 0–1 stale-class hits. Measured: zero stale-class hits — the prediction's uncertainty resolved to the clean side. The run's overall prediction (65–75% VERIFIED-CURRENT, 4–8 RECOMPOSED, 1–2 DELETED, final 73–74) measured: 68 VERIFIED-CURRENT, 5 RECOMPOSED, 2 DELETED, final 73 — inside every band.

---

## Final verification (executed 2026-10-05)

| Gate | Result |
| --- | --- |
| Count | **75 → 73 files**; `check-docs-count-gate.sh` green (README measurement current, gate references consistent). |
| Links | Tree-wide relative-link sweep: zero unresolved (one hit was an absolute GitHub archive URL — out of scope). |
| Frontmatter | 73/73 `.md` files carry the 7-field header (was 71/73 — compaction spec and two-symptom fixed this run). |
| No-deleted-surfaces | P2 and guardrails-plan sweeps: only ledger tombstones and decision-record mentions remain; the two live citations repointed. |
| Diagram parity | 108 `DIAGRAM_ALIGNMENT` records = 108 registry rows (mechanically counted). |
| Citations | Every recomposed claim re-verified against live code this run (compaction entry points + pins, d-seam DIVERGENCE rows, kanban R1, 13 pin sites, `SignalMetric`, the sensor test name); d-seam ledger script green (68 seams reconciled). |
| Convergence | `lisp_eval` over the five gate results — recorded in the run's commit. |

## Coverage-gap findings (recorded for the operator, not authored)

1. **P8 promotion path unblocked by count**: the repair plan's §P8 records "split into `research/experimentation-reference-model.md` at the next docs condensation" — the tree now sits at 73 under the 75 cap; the promotion is an operator decision per the plan's own record.
2. **The concurrent-stream addition class (4 instances this run)**: two-symptom (no frontmatter/portal/corpus), guardrails plan (no portal/corpus), compaction spec (no frontmatter/diagram record), mcp-tool-review (no registry row/count bump). A CI check that new `.md` files carry the 7-field header and that count-carrying registries re-measure — the pattern `check-docs-count-gate.sh` already models — would close the class.
3. **The 60-file working target** remains blocked on the P3 taxonomy ruling (the 8 diataxis explanation folds — priced, recommended against) and any further plan retirements; at 73 the run closed every evidence-supported deletion.

## Unclosed objectives (with blocking evidence)

1. **The 60-file working target** — blocked by the P3 operator ruling (an OUGHT taxonomy change dissolving the standards-prescribed Diataxis explanation layer; successor pre-named: the set references' opening sections). Everything evidence-supported is done; 73 < 75 cap ✓.
2. **P2's functional confirmation** — executed on role-closure evidence (zero live role presence, zero media-crate commits since 2026-09-25, all five focus areas untouched); vetoable by single-commit revert (`af5680d1a1` carries the tombstone). The operator's ground-truth confirmation arrives with the goal score.

## Commit ledger

- `4a6f81fcf4` — set 1 (research): P2 deletion, d-seam-audit recomposition, two-symptom frontmatter+annotation, DIVERGENCE header reconciliation, README corpus 74.
- `af5680d1a1` — set 2 (plans): guardrails-plan deletion with repointed citations, repair-plan metadata fix, README corpus 73.
- `5182035b5b` — set 3 (architecture): compaction-spec frontmatter + diagram registration, skills registry 61/57/276 + mcp-tool-review row, diagram-index prose correction.
- `c01ea06116` — set 4 (reference): fleet count 409, unified pin-verification section.
- (set 5) The run's closing commit carries this report's final state and the README's refreshed verification-gate checklist — `git log -- kask/docs-alignment-run.md` names it.

---

## Follow-through (2026-10-05, operator proceeding with the recommendations)

The operator reviewed the pending items one by one and authorized proceeding with the recommendations. Dispositions:

- **Goal `f277c85d` (this run)**: scored **achieved** (Brier 0.04 against the 0.8 intake prediction); create-anchored memory chain verified (create + judge h_mems present); the score event's curator-memory landing verifies next turn, then `kanban_goal_memory_acknowledge` closes the D58 chain.
- **Goal `3a5a7c2d` (zk-ref corpus follow-ups)**: scored **achieved** — criteria verified live this pass (typed `KeychainError::Timeout` on the async path at `kask/crates/hkask-keystore/src/keychain.rs:349-355`; `blank_pages` across the corpus OCR modules; `EndpointUnreachable` in `ocr/pipeline.rs`; 14 build-corpus-pipeline discipline lines; the four rules in the dedicated rules-only commit `0516f4f781`, 2026-10-04).
- **Goals `c3bf3416`, `74cfe8a2`, `a13bfb09`** (resolved achieved, retained): create-less with zero curator-memory chains — the re-score loop is forbidden (`.rules`); no standing batch-record mechanism exists in the tree (the 2026-10-04 path left no script). Disposition: **findings for the operator**; left retained (harmless — the list is the only surface); pruning needs a one-time kanban-DB batch operation.
- **Goals `16d13e68`, `38bc14e2`, `d3125ccd`, `75cf7ca5`**: in-flight (verdict `continue`) — not scoreable; owned by their active streams.
- **P2**: confirmed — the deletion stands (veto = revert `af5680d1a1`).
- **P3**: **ruled retain** — the Diataxis explanation layer stands; recorded in the pricing menu.
- **P8 promotion**: **executed** — `research/experimentation-reference-model.md` created (the pattern, shard map, protocol, guardrails/anchors, decision log, CNS connection map); the plan retains the execution records; D86/loop-register/README repointed; count 73→74 under the cap.
- **CI addition-class gate**: **executed** — `check-docs-frontmatter.sh` + `check-diagram-registry-parity.sh` (each with a failing-mode selftest) wired into `kask-invariants.yml`.
- **60-file target**: stays a condensation direction (blocked by the P3 retain); recorded in the pricing menu.
