# Skill semantic review — 2026-10-18

Live census: 58 skills, 216 templates.
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
| co-scientist | 2 | CS-1 | 0 | 4 | 0 | 1 | y/y/y/y/y | aligned | clean | clean | CS-1 D; experiment registry is the oracle |
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
