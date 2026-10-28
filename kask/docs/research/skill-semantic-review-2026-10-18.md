---
title: "Skill semantic review — 2026-10-18"
audience: [developers, architects, operators, agents]
last_updated: 2026-10-18
version: "1.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust, lifecycle]
---

# Skill semantic review — 2026-10-18

Live census: 59 skills, 220 templates.
Set-equal to `/tmp/live-skills.txt` (see reconciliation note at the foot).

Columns: **tpl** templates in the skill's registry dir; **gates** pinned `lisp_eval` gate ids;
**miss** referenced templates that do not resolve (0 = all resolve); **edges** skill-to-skill
composition edges that resolve to a live skill; **anch** `onto_anchor`→derived citations;
**docs** in-tree doc citations; **loop** the four S13 anatomy parts (I/T/C/B/A).

| skill | tpl | gates | miss | edges | anch | docs | loop | reference model | D/P | verdict | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| adhd-mode | 3 | ADHD-1,ADHD-2 | 0 | 0 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | Counts extract from the model's own draft (semantic extraction); verdicts D via Forms G/C |
| algedonic-review | 0 | ALG-1,ALG-2 | 0 | 0 | 2 | 1 | y/y/y/n/y | aligned | clean | clean | Board reads + ALG-1/ALG-2 D; severity triage P (operator's) |
| bug-hunt | 4 | BH-1 | 0 | 1 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | Bach & Bolton heuristic model encoded in BH-1 |
| build-corpus-pipeline | 0 | BCP-1 | 0 | 1 | 0 | 2 | y/y/y/y/y | aligned | clean | clean | Stage gates D; BCP-1 measured counts |
| code-review | 5 | CR-0,CR-1,CR-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | Fagan/PERFECT encoded; CR-0..2 D; applied_count is a runtime observation, correctly P |
| coding-guidelines | 3 | CG-1,CG-2 | 0 | 1 | 0 | 0 | y/y/y/y/n | aligned | clean | clean | Karpathy four principles; CG-1/CG-2 D |
| hypothesis-testing | 3 | HT-1 | 0 | 6 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | HT-1 D; delegates scientific design to experimental-design before registry use |
| experimental-design | 3 | ED-0,ED-1,ED-2 | 0 | 4 | 6 | 1 | y/y/y/y/y | aligned | clean | clean | ED gates D over classified inputs; executor reports and operator are the scientific oracles |
| create-skill | 2 | CS-1 | 1 | 4 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | CS-1 D; my-skill/analyze.j2 is a documented placeholder in the scaffold example |
| deep-module | 3 | DM-1,DM-2 | 0 | 0 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | Ousterhout Ch.4/Ch.7.1 via derived deep_module + deletion_test (both verified present) |
| diagnose | 4 | DIAG-1,DIAG-2 | 0 | 2 | 2 | 0 | y/y/y/y/y | aligned | clean | clean | Zeller/Agans encoded; DIAG-1/2 D |
| diataxis-diagram | 6 | DD-1,DD-2 | 0 | 2 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | DD-1/DD-2 D |
| doc-update | 1 | DU-1 | 0 | 2 | 0 | 4 | y/y/y/y/y | aligned | clean | clean | DU-1 D |
| eqm | 4 | EQM-1,EQM-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | EQM-1/EQM-2 D; directional_match_pct is caller-supplied (not template-computable), correctly P |
| essentialist | 1 | ESS-1,ESS-3 | 0 | 1 | 2 | 0 | y/y/y/y/y | aligned | clean | clean | ESS-1/ESS-3 D; zero-delta eq over lists |
| falsifiability | 5 | FAL-1,FAL-2 | 0 | 5 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | Popper via derived falsifiability; FAL-1/FAL-2 D; multi-line env parses |
| gpui-bench | 0 | GPB-1,GPB-2 | 0 | 0 | 0 | 0 | y/y/y/y/y | unverified-external | clean | clean | GPB-1/GPB-2 D; benchmark thresholds are operator-declared |
| gradient-hunter | 6 | GH-1,GH-2 | 0 | 2 | 4 | 0 | y/y/y/y/y | aligned | clean | clean | GH-1/GH-2 D |
| grill-me | 2 | GM-1,GM-2 | 0 | 1 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | Socratic method + Bloom + ZPD all verified present in derived registry |
| grounding-verify | 4 | GV-1,GV-2 | 0 | 2 | 0 | 1 | y/y/y/y/y | aligned | clean | repaired | GV-1 had no env block: corrected_count/e_count/w_count/n_count/min_why_len all unbound. Env added from the producer's own totals contract |
| hypothesis-framer | 3 | HF-1,HF-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | FINER criteria verified present; HF-1/HF-2 D |
| idiomatic-rust | 3 | IR-1,IR-2 | 0 | 2 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | IR-1/IR-2 D |
| improv | 6 | IMP-1,IMP-2,IMP-3 | 0 | 1 | 2 | 0 | y/y/y/n/n | aligned | clean | clean | Johnstone; plussing/yes_and verified present; IMP-1..3 D |
| kanban-task-management | 7 | KTM-1,KTM-2 | 0 | 1 | 2 | 0 | y/y/y/n/y | aligned | clean | clean | KTM-1/KTM-2 D; INVEST via derived invest_criteria |
| kask-seam-audit | 11 | KSA-1,KSA-2,KSA-3 | 0 | 5 | 0 | 2 | y/y/y/y/y | aligned | clean | clean | KSA-1..3 D |
| kata-improvement | 11 | KI-1,KI-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | Rother Toyota Kata via derived improvement_kata; KI-1/KI-2 D |
| lean-prover | 3 | LP-1,LP-2 | 0 | 2 | 0 | 0 | y/y/y/y/y | unverified-external | clean | clean | LP-1/LP-2 D; lean_check is the external oracle |
| local-research-swarm | 2 | LRS-1,LRS-2 | 0 | 4 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | LRS-1/LRS-2 D; boundary text updated by the retirement commit |
| mcda | 3 | MCDA-1,MCDA-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | Belton & Stewart + Keeney & Raiffa; MCDA-1/2 D |
| mcp-tool-review | 1 | MTR-1,MTR-2 | 0 | 3 | 0 | 0 | n/y/y/y/y | aligned | clean | clean | MTR-1/MTR-2 D; Initial stated as "**Initial:**" (false positive in the sweep) |
| media-workflow | 0 | MWF-1,MWF-2 | 0 | 1 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | MWF-1/MWF-2 D |
| metacognition | 7 | MC-1 | 0 | 2 | 4 | 0 | y/y/y/y/y | aligned | clean | clean | MC-1 D; falstaffian-rotation.j2 referenced extensionlessly at line 92 (resolves) |
| onto-anchor | 0 | OA-1,OA-2 | 0 | 0 | 0 | 1 | y/y/y/y/n | aligned | clean | clean | OA-1/OA-2 D; PDCA exemption stated (one deterministic call) |
| pragmatic-cybernetics | 3 | PC-1,PC-2 | 0 | 2 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | Beer/Ashby; PC-1/PC-2 D |
| pragmatic-semantics | 4 | PS-1,PS-2,PS-3 | 0 | 3 | 0 | 0 | y/y/y/y/y | aligned | clean | repaired | Modifier prose in two templates stated FIBO +0.10 / SUMO +0.05 against the PS-1 gate's +0.05 / +0.03. Prose bound to the gate by a derived pin |
| product-manager | 1 | PMB-1,PMB-2 | 0 | 1 | 0 | 0 | y/y/y/n/y | aligned | clean | clean | PMB-1/PMB-2 D |
| program-manager | 3 | PM-1 | 0 | 2 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | PM-1 D; dod-checklist to closeout-report handoff repaired earlier (3b3688a8ae) and verified live |
| prompt-enhance | 4 | PE-2 | 0 | 5 | 0 | 0 | y/y/y/y/n | aligned | clean | clean | PE-2 derives blocking_defects from structured findings (verified four branches) |
| provider-audit | 3 | PA-1,PA-2 | 0 | 3 | 0 | 2 | y/y/y/n/y | aligned | clean | clean | PA-1/PA-2 D |
| refactor-architecture | 5 | RA-1,RA-2 | 0 | 4 | 0 | 2 | y/y/y/y/y | aligned | clean | clean | RA-1/RA-2 D; Fowler strangler verified present |
| region-routing | 1 | RR-1 | 0 | 4 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | RR-1 D |
| sankey-flow | 4 | SFW-1,SFW-2 | 0 | 2 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | SFW-1/SFW-2 D; CSV rendered from validated edges |
| scenario-planning | 5 | SP-1,SP-2 | 0 | 1 | 2 | 0 | y/y/y/y/y | aligned | clean | clean | Schwartz/Chermack; SP-1/SP-2 D |
| self-improvement | 10 | SI-1,SI-2,SI-3 | 0 | 3 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | SI-1..3 D |
| skill-bundler | 1 | SB-1 | 0 | 0 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | SB-1 D |
| skill-discovery | 3 | SD-1,SD-2 | 0 | 4 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | SD-1/SD-2 D |
| skill-maintenance | 7 | SM-1,SM-2 | 0 | 4 | 2 | 1 | y/y/y/y/y | aligned | clean | clean | Fagan + Goodhart verified; SM-1/SM-2 D |
| structured-extraction | 3 | SE-1,SE-2,SE-3 | 0 | 1 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | SE-1..3 D |
| superforecasting | 9 | SF-1,SF-2 | 0 | 0 | 2 | 0 | y/y/y/y/y | aligned | clean | clean | SF-1/SF-2 D; Tetlock/Brier/fermi/reference_class all verified present; overconfidence_bias is EQM-derived, not an ontology anchor |
| swarm-intelligence | 6 | SWM-1,SWM-2,SWM-3 | 0 | 2 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | SWM-1..3 D |
| system-maintenance | 0 | SMN-1,SMN-2 | 0 | 0 | 3 | 0 | y/y/y/n/y | aligned | clean | clean | SMN-1/SMN-2 D |
| task-breakdown | 4 | TB-1,TB-2 | 0 | 0 | 0 | 0 | y/y/y/n/y | aligned | clean | clean | TB-1/TB-2 D |
| tdd | 2 | TDD-1,TDD-2 | 0 | 5 | 0 | 1 | y/y/y/n/n | aligned | clean | clean | Beck TDD; TDD-1/TDD-2 D |
| therapy | 3 | TH-1,TH-2 | 0 | 1 | 1 | 1 | y/y/y/y/y | aligned | clean | clean | TH-1/TH-2 D; passphrase claim verified accurate (DEFAULT_PASSPHRASE, tested provision path) |
| transcript-reel | 0 | TR-1,TR-2 | 0 | 0 | 1 | 0 | y/y/y/y/y | aligned | clean | clean | TR-1/TR-2 D; MovieLabs OMC vocabulary |
| upstream-rebase | 5 | UR-1,UR-2,UR-3 | 0 | 2 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | UR-1..3 D |
| verification-compression | 3 | VC-1,VC-2 | 0 | 3 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | VC-1/VC-2 D |
| wardley-mapper | 4 | WM-1,WM-2 | 0 | 0 | 1 | 0 | y/y/y/y/n | aligned | clean | clean | Wardley + Porter verified present; WM-1/WM-2 D |
| writing-style | 2 | WS-1,WS-2 | 0 | 0 | 0 | 0 | y/y/y/y/y | aligned | clean | clean | WS-1/WS-2 D |

## Reconciliation

The ledger's skill set is reconciled against the live census by exact set equality
(not count equality): live **58**, ledger **58**.

Unreviewed remainder: **zero** — every live skill has a row, and no row names a
skill that is not live. Reproduce with:

```bash
bash kask/scripts/audit/skill-review-ledger-base.sh | tail -n +2 | cut -f1 | sort > /tmp/ledger-skills.txt
(cd .agents/skills && ls -d */ | sed 's#/##' | sort) > /tmp/live-skills.txt
diff /tmp/live-skills.txt /tmp/ledger-skills.txt   # no output = set-equal
```

## Evidence and instruments

| check | instrument | scope | result |
|---|---|---|---|
| Lisp parse + AST lint | `kask/scripts/check-skill-lisp-forms.sh` | 276 files, 301 candidates | 0 defects |
| S12 symbol resolution | `check-skill-lisp-forms` test `pinned_gate_forms_resolve_every_symbol` | every pinned (form, env) pair | 1 defect (GV-1), repaired |
| Gate selftests | `kask/scripts/check-skill-lisp-forms-selftest.sh` | 2 pass + 6 fail controls | OK |
| Composition edges | `kask/scripts/audit/skill-review-edges.sh` | 101 edges | all resolve |
| Template refs | `kask/scripts/audit/skill-review-ledger-base.sh` (miss column) | 216 templates | 1 non-resolving (a documented placeholder) |
| Template contracts | `kask/scripts/check-template-contracts.sh` | 216 templates | 0 flagged, 7 skipped |
| Ontology anchors | `kask/scripts/audit/skill-review-anchors.sh` | 37 skill-key citations (29 distinct keys) | 0 dangling |
| Doc citations | `kask/scripts/audit/skill-review-doc-cites.sh` | 25 in-tree doc citations | 0 missing |
| S13 anatomy | `kask/scripts/audit/skill-review-loop-anatomy.sh` + read-triage | 58 bodies | all state initial + target; flags read-triaged as false positives |
| S14 leads | `kask/scripts/audit/skill-review-s14-leads.sh` | 17 contract leads | all read-triaged (P judgment or runtime observation) |

## Reference-model scope

Each skill's cited external source was checked at the depth the tree supports:
the **derived-registry encoding** (`kask/crates/hkask-bridge-ontology/src/derived.rs`)
was read and compared against the skill's claim for every `onto_anchor`→derived
citation (37 skill-key citations over 29 distinct keys, 0 dangling; chapter-level
precision where the source warrants it).
The **primary sources themselves** — the books and papers — are not vendored in this
tree, so no skill is marked `aligned` on the strength of having read its book.
Two skills (gpui-bench, lean-prover) whose oracle is an external toolchain rather
than a published model are marked `unverified-external` for the model itself while
their deterministic gates are green.

## Composition

101 skill-to-skill references resolve to a live skill. Eighteen mutual pairs exist
(create-skill/skill-maintenance/skill-discovery, pragmatic-semantics/
pragmatic-cybernetics, grounding-verify/structured-extraction, diataxis-diagram/
doc-update/sankey-flow, idiomatic-rust/lean-prover); every one was read and is a
`When NOT to Use` boundary note or a named-consumer reference, not an invocation
cycle. No self-loops. The invocation graph is acyclic.

## Template accounting

216 registry templates decompose exactly:

- **207** skill-owned (the ledger's tpl column sums to 207 across 51 skills).
- **9** server/process-owned, rendered by Rust callers rather than by a skill:
  `docproc/` (6, the corpus pipeline), `media/` (2, the media server),
  `experimentation/` (1, the experiment protocol).
- **Seven** live skills carry no templates of their own (the tpl column reads 0):
  five tool-driven — algedonic-review, gpui-bench, onto-anchor, system-maintenance,
  transcript-reel — and two that render the server-owned templates above:
  build-corpus-pipeline (`docproc/`) and media-workflow (`media/`).

Every template was therefore either read against its owning skill or accounted
to the server that renders it — none is unaccounted for.

## Independent assessment

An independent agent (no prior context on this work, instructed to falsify) re-ran
the instruments and reported **partially-achieved, confidence 0.82**. Its four
per-criterion verdicts were PASS/PASS/PASS/PASS, but it refuted three of the
ledger's own claims — all three reproduced and are repaired (commit 8b6af9c0297):

1. The anchors instrument matched a bare `derived `key`` anywhere, reading a
   skill's prose about something "EQM-derived" as an ontology anchor. Tightened to
   require the `onto_anchor` context; the true figure is 37 skill-key citations
   over 29 distinct keys, 0 dangling.
2. Both audit scripts exited 1 on a clean sweep (`grep -o` returns 1 on no-match
   under `pipefail`), so a green result read as failure.
3. The template accounting named four zero-template skills; the tpl column reads
   seven. Corrected.

Two evidence gaps the assessor could not close, recorded as concerns:

- `check-skill-crossrefs.sh` only checks refs carrying an explicit extension, so
  the one bare ref (`create-skill` → `my-skill/analyze`, a documented placeholder)
  is outside its reach — the passing gate is consistent with a miss it cannot see.
- Reference-model verdicts rest on the in-tree derived-registry encoding; the primary
  sources are not vendored.

## Memory consolidation (companion pass, 2026-10-18)

Run alongside this review because the skill corpus and the curator store share the
same growth pressure. Store at pass time: **5,483 h_mems** against a 4,000 ceiling,
composition 2,691 thread-tier / 1,749 knowledge / 724 goal-events / 285 other.

All three mechanical valves returned clean, which is the finding:

- **Dedup**: 5,509 scanned, **0 duplicate groups** — the writer-side canonical-key and
  same-key dedup fixes hold.
- **Prune** (skill_use_issue, 30d, spare-recall 7d): **0 candidates** — no stale incidents.
- **Backfill**: **0 missing embeddings** (4 unsupported rows are non-string values, by design).
- **Thread-tier shed working**: 0 rows older than 30 days; only 110 older than 7.
- **Confidence floor holds**: 5,249 at floor, 1 below (a legacy row).

The 204 apparent embedding orphans are all `curator:distilled:*` watermark rows —
the distillation timer's coverage markers, bookkeeping by design, not content.

What remains is **additive-only knowledge growth** (1,861 rows added in October, the
layer that has no automatic forgetting path) plus **68 space-keyed entities** carrying
canonical-form drift. Both are consolidation work for a therapy session, not mechanical
valve work: renaming a drifted entity key must carry its embeddings (the `entity_ref`
recall JOIN), and condensing near-duplicate lesson keys (`prompt-enhancement` 24 vs
`prompt-enhance-skill` 11; `agent-workflow` 13 vs `agent-work` 11) needs per-group
semantic judgment. Both require operator approval per the therapy skill's Phase 4.

## Findings

Verified defects, both repaired in this review:

1. **GV-1 (grounding-verify) had no env block.** The form referenced
   `corrected_count`, `e_count`, `w_count`, `n_count` and `min_why_len`, none
   bound by anything — the step could not run as written. Env added from the
   producer's own contract (`compile-error-log.j2` emits
   `totals: {corrected, data_notes, warnings}`).
2. **Pragmatic-semantics modifier drift.** Two companion templates stated
   FIBO +0.10 / SUMO +0.05 against the PS-1 gate's +0.05 / +0.03 — residue the
   earlier fix missed in the Tier-5 table and the provenance layer table. Both
   now state the gate's values, and a pin derives the expected spelling from the
   live gate so a future gate edit cannot leave the prose behind.

Concerns requiring further evidence (not defects):

- The primary sources are not vendored, so reference-model verdicts rest on the
  in-tree derived-registry encoding rather than the source text.
- The S13 anatomy sweep's narrow pattern flagged 28 skills whose bounds and
  re-entry points are stated in other words; each was read and cleared, but the
  sweep is a lead generator, not a gate.
- `falstaffian-rotation.j2` is referenced extensionlessly (`metacognition/
  falstaffian-rotation`) rather than with the `.j2` suffix; it resolves, but a
  suffix-matching instrument would miss it.

## Follow-on pass: anachronistic pins and citations (2026-10-18, later session)

Four capability removals (companies/FIBO, training, portfolio, constant-maturity)
landed after the review's ledger was composed. Each left a pin or citation naming
surface that no longer exists — the operator's named defect class. The HEAD at this
pass was `3adc75c9e1` (Remove the FIBO ontology rung).

**Verified defects, all repaired:**

1. **The PS-1 modifier pin was RED at HEAD.** The FIBO-rung removal renamed the
   PS-1 env flag `fibo_anchored` → `domain_anchored` in the skill body, three
   templates, and the pin — but wrote the pin's expected literals inconsistently
   (bare `domain` where the prose reads `domain_anchored`) and deleted the
   classify template's per-namespace `+0.05` row the pin still asserted. The
   CI-wired gate (`check-skill-lisp-forms.sh`) failed three assertions. Repaired
   by binding each carrier to the live prose; four prose-drift controls and the
   gate-mutation control all discriminate. Commit `97f027ef37`.
2. **`onto_anchor_tool`'s rung-1 pin asserted a non-existent concept.** The FIBO
   removal rewrote the test to expect `sepio:0000030`, a lowercase URI the
   resolver never produces — `assertion` maps to `sepio::ASSERTION` =
   `SEPIO:0000001`. Rebound to the constant so a constant edit fails the pin
   (control: a mutated constant fails). Commit `64d5d4c1d9`.
3. **The retirement record was missing five shipped names.**
   `RETIRED_SKILL_NAMES` (`crates/agent_skills/agent_skills.rs`) is the single
   source of truth for both the installed-copy sweep and the crossrefs gate, and
   its own contract says each retirement appends its name in the same commit.
   `company-research-deep`, `company-research-flash`, `listening`,
   `portfolio-review`, and `cmp-term-structure` were absent. A derived
   reconciliation by that contract now reads **56/56, no gap either direction**.
   Commit `64d5d4c1d9`.
4. **Six derived-ontology citations named deleted template paths.**
   `derived.rs` entries for `maia_listening`, `adjacent_possible`,
   `earnings_quality`, `value_chain`, `cash_conversion_cycle`, `audit_evidence`,
   and `materiality` cited `listening/apply-template.j2` and four
   `company-research/*.j2` templates that no longer exist. Each re-anchored to
   its surviving consumer or marked a research-content concept, following the
   file's own precedent (the `lora-training` note). These edits rode a concurrent
   stream's commit `f978c08ead` (shared tree); their content is verified present
   in HEAD.
5. **Test fixtures named deleted skills as opaque strings** in
   `tool_retry_tracker`, `curator_stores` (two tests), `memory_store`, and
   `hkask_lisp`; repointed to live names. Commit `64d5d4c1d9`.

**New read-only instruments** (self-tested, commit `a0e77870ad`):

- `kask/scripts/audit/derive-retired-skill-gap.sh` — re-derives the retirement
  set by the record's own stated method and diffs it against the curated array.
- `kask/scripts/audit/check-retired-shipped-status.sh` — reports whether a named
  retired skill ever shipped (and so needs a sweep entry).

**Gates at this pass's end:** prescreen 58/58 D/P + 58/58 conditions + 216
contracts; crossrefs, crate-refs, lisp-forms (276 files / 301 candidates / 0
defects), template-contracts, s9-s10, callsite, forecast-conformance all OK;
retirement instruments reconcile. Suites: agent --lib 1076/0, agent_skills 100/0,
hkask-lisp 41+7/0, hkask-memory 82+16/0, kask_bridge 274/0,
hkask-bridge-ontology 75+4/0; clippy `-D warnings` clean over the touched scope.

**Remainder at this pass:** unchanged at **zero** — the census still reads 58
skills / 216 templates, set-equal to `/tmp/live-skills.txt`.

## S14 cohort: deterministic aggregates off the model (2026-10-18, later session)

Two of the S14 cards from the D/P sweep were executed (the operator authorized
immediate repair of reproduced mechanical/contract defects):

- **`3bcdb017` — eqm-validate re-typed caller-computed statistics** (commit
  `014cf1ac6f`, 4+/8−). `correlation_r` and `directional_match_pct` left the LLM
  output contract; the caller reads both from `outcome_statistics`, which the
  template's own header and Step 2 already required. The contract now agrees with
  the body and with `eqm/SKILL.md:129,137` (the step is `D`).
- **`3c5fd9bf` — layout-review emitted cross-probe aggregates** (commit
  `d0eeee1e4c`, 17+/12−). `all_probes_passed` and `failing_gates` left the
  contract; a new `KSA-4` `lisp_eval` gate folds the probe list and set-differences
  the gate results with dedup. Verified live: all-pass → `[true, []]`; mixed →
  `[false, ["overflow"]]`; duplicate failing gates dedup to one. The parser-backed
  gate now reads **302** candidates (was 301), 0 defects.

Remaining S14 cards: **none** — the cohort is complete. All eight executed in this
session (the two recorded above plus the six below), each verified through live
`lisp_eval` fixtures rather than asserted:

- `0a7016fd` upstream-rebase — `execute.j2`'s five measured states
  (`compiled`/`tests_passed`/`invariant_holds`/`marker_count`/`call_site_count`)
  and `decide.j2`'s `full_reapplication` and `document.j2`'s
  `divergence_md_updated` left the contracts; the caller measures and `UR-3`
  evaluates the measured values (commit `e3b6279efe`).
- `1d4286de` diagnose-fix — the three post-application booleans left the
  contract; `DIAG-2`'s caller-measured inputs are now named explicitly
  (`57345244fd`).
- `5ea2e646` code-review-implement — `fixes_generated` (`(length fix_plan)`) and
  `applied_count` (the agent's `edit_file` receipts) left the contract
  (`57345244fd`).
- `a3f3270e` superforecasting — stage_0's two derived booleans left the contract
  (SF-1 already computes the conjunction) and `gate_pass` moved into a new `SF-3`
  gate computing the weighted totals (`02a110ed68`).
- `2a84b7ab` grounding-verify — Step 5's ten reported counts and GV-1's class
  counts are computed by new `GV-3`/`GV-3b` drivers (`f81e9b4484`).
- `c1b87090` create-skill — the core-rung count and `research_findings` are
  computed by a new `CS-3` gate, so the Phase 5 loop no longer branches on a
  model-asserted scalar (`32c48a646c`).

The corpus's pinned-form count rose from 301 to 310 across the cohort (the new
`SF-3`, `GV-3`, `GV-3b`, `CS-3`, and `KSA-4` forms), all parser-clean with every
symbol resolving.
