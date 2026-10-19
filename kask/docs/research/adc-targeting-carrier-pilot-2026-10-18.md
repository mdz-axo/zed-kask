---
title: "ADC targeting versus carrier-exposure pilot"
audience: [researchers, operators, agents]
last_updated: 2026-10-18
version: "1.0.0"
status: "Partial — design and published-data pilot complete; biological execution and decoupled audit blocked"
domain: "Oncology pharmacology"
mds_categories: [domain, trust, lifecycle]
---

# ADC targeting versus carrier-exposure pilot

This is the real functional trial for the `experimental-design` and `hypothesis-testing` skills. It records the design and the evidence actually obtained; it does not claim a biological experiment was performed.

## Operator hypothesis

> ADCs (antibody drug conjugates) work against cancer not because of targeting but because of trucking — or an area-under-the-curve effect mediated by solubility.

The testable formulation keeps four live rivals:

1. **Carrier/exposure dominant:** antibody carriage and linker/payload solubility determine intact/released payload exposure, which dominates efficacy.
2. **Targeting dominant:** antigen-specific binding and internalization deliver a practically meaningful efficacy advantage at matched exposure.
3. **Release/bystander dominant:** released membrane-permeable payload and bystander diffusion dominate both intact-carrier exposure and target binding.
4. **Mixed mechanism:** targeting, exposure and release interact; dominance changes with antigen density, tumor transport and linker/payload properties.

## Versioned design artifacts

- `HypothesisSpec`: `hypothesis-spec-v1`, id `adc-targeting-vs-carrier-v1`.
- Selected `DesignSpec`: `design-spec-v1`, id `adc-factorial-v1`.
- Executed exploratory design: `design-spec-v1`, id `adc-published-reanalysis-v1`.
- Findings: `findings-spec-v1`, outcome `inconclusive` for the operator's mechanism claim.

## Candidate designs and gates

| Candidate | Core contrast | ED-1 result | Disposition |
|---|---|---|---|
| Published Hamblett E2/E4/E8 reanalysis | DAR/clearance/exposure proxy versus reported efficacy | `design_void: no_discrimination` as a causal test | Executed only as an exploratory measurement; no nonbinding specificity arm and no independent solubility manipulation. |
| Blocked factorial biological study | targeted/nonbinding antibody × antigen+/isogenic antigen− tumor × hydrophilic/hydrophobic matched linker-payload condition | `execute` | Selected. Strongest discrimination among carrier, targeting, release and mixed rivals; biological execution is operator-bound. |
| PBPK/spheroid transport calibration | fit/validate transport, binding, internalization and release parameters on held-out distributions | `design_void: not_identified` for antitumor mechanism until calibrated and linked to efficacy | Strongest rejected alternative: cheaper and reversible, but model-dependent and not sufficient to identify causal efficacy. |

### Selected factorial protocol

- **Units:** blocked isogenic antigen-positive and antigen-negative organoid/xenograft systems; random allocation within model/batch.
- **Factors:** antibody specificity (targeted versus matched nonbinding isotype), antigen state (positive/negative), linker-payload hydrophilicity/solubility condition; dose adjusted or modeled to support exposure matching.
- **Controls:** unconjugated targeted/nonbinding antibodies; free payload at matched released-payload exposure; vehicle; positive cytotoxic control; orthogonal target-occupancy/internalization assay.
- **Measurements:** intact ADC, total/conjugated antibody and released payload PK; plasma and tumor AUC; antigen density; target occupancy/internalization; spatial penetration; tumor-growth-inhibition AUC/regression; toxicity; linker stability/DAR over time.
- **Identification:** specificity × antigen-state interaction at matched intact/released-payload exposure identifies targeting contribution; hydrophilicity/exposure main effect identifies carrier/solubility contribution; released-payload/bystander measurements distinguish the release rival; material interactions support the mixed mechanism.
- **Information:** simulate the full interaction model using pilot variance; power/precision target is the primary interaction confidence width relative to a domain-ratified practical margin. The provisional 20% margin is not executable until ratified.
- **Analysis:** preregister mixed-effects model, primary interactions, missing-data/exclusion rules, multiplicity, sensitivity to exposure matching and measurement error.
- **Stopping:** fixed design unless an independently justified sequential method is preregistered; no naive peeking. Positive `max_runs` is set by the final power/simulation result.
- **Executor:** qualified human biological laboratory under applicable ethics/safety approvals. The agent did not execute it.

## Executed exploratory measurement

Primary source: Hamblett et al., “Effects of Drug Loading on the Antitumor Activity of a Monoclonal Antibody Drug Conjugate,” *Clinical Cancer Research* 10 (2004), DOI `10.1158/1078-0432.CCR-04-0789`.

At a 10 mg/kg antibody-component dose, Table 2 reports:

| ADC | Nominal DAR | Antibody/ADC AUC (μg-day/mL) | AUC × DAR proxy |
|---|---:|---:|---:|
| E2 | 2 | 2313 | 4626 |
| E4 | 4 | 1689 | 6756 |
| E8 | 8 | 520 | 4160 |

`lisp_eval` recomputed every proxy and ratio from the table: E4/E8 = `1.6240384615`; E2/E8 = `1.1120192308`.

**Interpretation boundary:** AUC × nominal DAR is a platform-derived proxy for payload still carried on circulating antibody, not measured free-payload AUC, tumor-payload AUC or bioavailable intracellular exposure. It assumes DAR stability that the same paper does not establish (the authors note MMAE release over time).

Hamblett also reported:

- E4 and E8 had comparable in-vivo efficacy at equal antibody dose despite E4 carrying half the MMAE; the authors said E4's approximately three-fold larger antibody AUC was apparently sufficient to compensate for lower potency.
- CD30-negative WSU-NHL cells were insensitive to all cAC10 ADCs (`IC50 > 1000 ng/mL`).
- A physical mixture of free MMAE and cAC10 had little effect in vivo.

These observations support an exposure-compensation mechanism within a targeted anti-CD30 ADC series. They contradict the stronger interpretation that the paper shows targeting is irrelevant: every conjugated arm used the same targeting antibody, and the available target-negative/free-payload controls point toward a necessary targeting/linkage role in that system.

## FindingsSpec

- **Outcome:** `inconclusive` for “not targeting but trucking/solubility.” ED-2 inputs: design valid for the narrow reanalysis; measurement valid for the published table; no prediction contradiction; no prespecified equivalence test; precision inadequate for mechanism attribution; not severe against all rivals.
- **Exposure finding:** E4's lower DAR and slower clearance yielded greater circulating conjugated-payload proxy exposure than E8; exposure can plausibly compensate for lower per-antibody payload.
- **Identification limitation:** no matched nonbinding ADC, no independent solubility manipulation, no exposure-matched targeting contrast.
- **Measurement limitation:** plasma antibody/ADC ELISA AUC is not released or tumor payload AUC; AUC×DAR is derived and assumes nominal DAR over time.
- **Transportability limitation:** one anti-CD30 xenograft system cannot establish mechanism dominance across ADCs or human tumors.
- **Exploratory implication:** retain carrier/exposure, targeting and mixed mechanisms. The release/bystander rival remains untested by this dataset.

## Registry adapter result

`registry_schema_mismatch`. The selected study has a meaningful control structure but no honest mandatory `baseline_pass_rate`, a non-binary outcome vocabulary, an operator-bound executor, and no completed feedback/selection partitions. No fake experiment was declared.

Three Curator schema-evolution directives were accepted and are tracked by task `f1a90cd2-a625-4dba-82ae-4ee0bd65b1fa`:

- typed first-class `design_spec` on `experiment_propose`;
- design-family `scientific_headroom` on `experiment_seal`;
- typed `scientific_outcome` on `selection_record` with explicit calibration rules.

## Verification ledger

### Executed

- Research run `b2a919f746200d85` opened before ADC design.
- Hamblett 2004 primary full text retrieved and agent-annotated verified.
- Thurber et al. 2008 and McCombs/Owen 2015 full texts retrieved and agent-annotated verified.
- Hamblett AUC×DAR arithmetic executed through `lisp_eval`.
- ED-0 returned `ready`.
- ED-1 returned `design_void: no_discrimination` for the observational pilot, `execute` for the factorial design, and `design_void: not_identified` for the uncalibrated modeling alternative.
- ED-2 returned `inconclusive` for the executed reanalysis.
- Registry adapter rendered and exposed the schema mismatch; three directives and one tracked task were created.
- **Pin-case set closed (all five named cases, plus exhaustive branch coverage).** Executed through `lisp_eval` against the gate forms exactly as they appear in `.agents/skills/experimental-design/SKILL.md` §3 and §6. Every ED-1 branch (10) and every ED-2 branch (6) was reached, and each returned a distinct, correct classification:

| Pin case | Gate | Inputs varied | Returned |
|---|---|---|---|
| Wright-style transportability / measurement failure | ED-1 | `measurement_validity=false` | `measurement_void: measurement_invalid` |
| Underpowered design, pre-spend | ED-1 | `information_adequacy=false` | `no-headroom: no_information` |
| Confounded observational causal claim | ED-1 | `identification=false` | `design_void: not_identified` |
| Naive optional stopping | ED-1 | `preregistration=false` | `design_void: not_preregistered` |
| Valid equivalence design admitted | ED-1 | `rivals_differ=false`, `equivalence_prespecified=true` | `execute` |
| Valid equivalence evidence | ED-2 | `equivalence_prespecified/met/precision_adequate=true` | `evidence_of_practical_equivalence` |

  Remaining ED-1 branches confirmed reachable and distinct: `blocked: ethics_safety`, `blocked: infeasible`, `design_void: no_discrimination` (no rivals differ **and** no prespecified equivalence), `design_void: bias_uncontrolled`, `design_void: not_reproducible`. Remaining ED-2 branches: `design_void`, `measurement_void`, `rejected`, `corroborated`, `inconclusive`.

- **Seam gates added and exercised in both directions (2026-10-19 correction pass).** The original single-pass templates let a model emit a selected design before the validity gate ran, type its own findings outcome, and type registry compatibility. Those three verdicts were removed from the templates and replaced with deterministic gates, each run against positive and negative controls:

| Gate | Enforces | Positive control | Negative control |
|---|---|---|---|
| ED-0R | Missing-field/owner reconciliation, partition coverage and null-spec consistency | complete spec, and 2 missing fields with 2 owner rows across distinct partitions → `true` | a dropped owner row, a non-null spec with a missing field, the same field claimed by two owner partitions, and a partition field absent from `missing_fields` → `false` |
| ED-C | 2–5 materially different candidates, unique non-empty ids, single-candidate justification | 3 unique drafts, and 1 draft with a non-empty explanation → `true` | duplicate ids, and 1 draft with an empty explanation → `false` |
| ED-1S | Selection only from ED-1 `execute` survivors, with a distinct rejected alternative | selected `execute` candidate + distinct rejected alternative → `true`; single-candidate run with no alternative → `true` | selecting a `design_void` candidate, a draft with no gate receipt, and selected==rejected → `false` |
| ED-2I | Internally consistent findings classification, with report presence derived from the real array | contradiction with valid measurement over 2 real reports, and zero reports correctly marked invalid → `true` | contradiction asserted together with equivalence met, and zero reports claimed valid → `false` |
| HT-R | Registry compatibility verdict | all six scientific preconditions true → `compatible` | non-binary claim without partitions → `registry_schema_mismatch` |
| HT-RM | Declaration mapping completeness before any write | complete mapping → `true` | zero implementation/grounded-report rubrics → `false` |

  One defect was found and fixed by this exercise rather than by inspection: the first ED-1S form used a `null` literal the Lisp dialect does not bind, so it errored instead of returning a verdict. An erroring gate is a broken gate; the form now uses an empty-string sentinel and returns correctly in all four controls.

  Three further edge defects were closed in the same pass, each pinned by a new negative control: (i) ED-0R originally compared only partition **lengths**, so a field claimed by two different owners, or a partition field missing from `missing_fields`, summed correctly and passed — it now checks partition-set uniqueness and coverage directly; (ii) `interpret-findings` emitted its own `report_count`, which let a model assert reports existed while supplying none — the count is removed from the template and ED-2I now derives presence from `(length run_reports)`, the exact array the caller passed in; (iii) the ED-1 measurement branch returns `measurement_void: measurement_invalid`, distinct in spelling from the registry's `measurement-void` reject-reason marker (`is_void_marker`, `kask/mcp-servers/hkask-mcp-experimentation/src/types.rs:356-358`) — the scientific outcome and the registry's calibration-exclusion token are different vocabularies and must not be substituted for one another.

- **Precision is now load-bearing in ED-2, not advisory.** `rejected` requires `prediction_contradicted` **and** `precision_adequate`. Executed: a contradicted prediction with inadequate realized precision returns `inconclusive`; the same contradiction with adequate precision returns `rejected`. This closes the path by which an imprecise apparent contradiction could be reported as a refutation.

- **Fault-localization ordering checked mechanically (the Wright lesson as executable precedence).** With `prediction_contradicted=true` **and** `measurement_valid=false`, ED-2 returns `measurement_void`, not `rejected`: an invalid instrument cannot manufacture a refutation. The converse also holds — with a valid instrument the same contradicted prediction returns `rejected`, so the precedence localizes the fault rather than shielding H1. This is the NASA-documented Wright pattern (failed 1900–1901 predictions traced to the assumed Smeaton coefficient and to Lilienthal coefficients that were valid for his wing geometry but did not transport to the Wrights' different geometry) encoded as gate order, not as a disposition to blame the instrument first.
- **Imprecision never becomes support.** With `prediction_contradicted=false`, `severe=true` and `precision_adequate=false`, ED-2 returns `inconclusive`; `corroborated` requires `severe` **and** `precision_adequate` together. Verified by executing both input sets.
- Branch-count correction: the ED-1 form carries **10** branches and ED-2 **6**. An earlier working note recorded ED-1 as 8; the count was re-measured by exercising every branch rather than by re-reading the note.
- Gate-name correction: the ED-1 measurement branch now returns `measurement_void: measurement_invalid` rather than `instrument_invalid`. The narrower name implied the instrument was the fault; the Wright case is precisely a reference-data and transportability failure, so the auxiliary ledger must localize among instrument, reference data, analysis and domain shift.
- Structural gates re-run green at this tree state with the two new templates included: `check-skill-lisp-forms.sh` (283 files, 59 skills, 319 Lisp candidates, 0 defects), `check-skill-crossrefs.sh`, `check-template-contracts.sh` (222 templates, 0 flagged), prescreen (59/59 D/P labelled, 59/59 initial+target), contract audit (222 templates, 0 flagged), call-site audit (210 call sites, 0 flagged), S9/S10 sweep (59 SKILL.mds, 0 flagged), `check-skill-crate-refs.sh` (0 dangling), `cargo test -p agent --lib corpus_` (4 render/strip tests), `-p agent_skills` (100 tests), `-p hkask-bridge-ontology --lib` (77 tests), `-p kask_bridge classifier` (3 tests). Test runs used `kask/scripts/cargo-test-nonzero.sh` so a filter matching zero tests fails rather than reporting a false green.

### Proposed, not executed

- No new organoid, animal or human-subject measurement was performed.
- No independent confirmatory exposure-matched factorial study was performed.
- No model-calibrated PBPK/spheroid validation was performed.
- Decoupled `grounding-verify` could not run: three independent inference routes failed because provider catalogs named retired models. This report therefore carries an **in-thread self-check only**, with its confidence capped accordingly; no blanket verification label is claimed.
- The Shadish–Cook–Campbell validity framework remains **partially verified**: research run `24d7fc791edc83f2` reached only the publisher's bibliographic record (Houghton Mifflin, 2002; ISBN 0395615569; 623 pages) and its stated four-part structure, with no accessible primary full text. The framework's four validity types are therefore cited as a structural organizer, not quoted; a direct citation still requires the primary text.
- `hkask-bridge-ontology`'s `ontology_literal_guard` integration test fails at this tree state on `kask/mcp-servers/hkask-mcp-kata-kanban/src/ontology.rs:371`. That file belongs to a concurrent stream, is unmodified from HEAD, and the guard scans only `.rs` files — this correction pass modified no `.rs` file. Reported, not fixed: it is outside this work's write scope.

## Reference-model research status

Research run `1de4616cbe7c793d` bound Platt, NIST/SEMATECH, JCGM/VIM, Wasserstein/Lazar, Greenland et al., Lakens equivalence and sample-size justification, Wald, Nosek, Munafò, NASA/Smithsonian Wright history, Mayo, Lakatos and anytime-valid guidance to DesignSpec fields or gates. Shadish–Cook–Campbell was retrieved only through secondary sources and remains partially verified pending direct primary-text retrieval.
