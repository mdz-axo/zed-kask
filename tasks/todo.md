# zed-kask Diataxis Documentation Pass — Checklist

## Phase 0 — Restore governing specs (prerequisite)

- [x] **T-00: Restore DOCUMENTATION_STANDARDS.md and MDS.md from git history**
  - Restored both files from commit `a32a7847a4~1` to their original paths
  - Bumped `last_updated` to 2026-07-27 with a restoration note (v0.31.1)
  - `DIAGRAMS_INDEX.md` footnote references resolve

## Phase 1 — Foundation (deepest crates first)

- [x] **T-01: hkask-types Reference** — class diagram of shared traits
  - Artifact at `kask/docs/diataxis/hkask-types/reference.md`
  - Every cited struct/trait has a `grep` hit (13/13 verified)
  - Class diagram renders with `DIAGRAM_ALIGNMENT` block
  - Gates passed: pragmatic-semantics (no OUGHT), pragmatic-cybernetics (9/9 implementors exist), essentialist (6 sections), brand-voice (0 taboo), grill-me Mechanism+Rationale
- [x] **T-02: hkask-types Explanation** — sequence diagram of port mediation
  - Artifact exists; OUGHT claims only in design-rationale section
  - Feedback loop closes (each port has a real implementor cited)
  - Gates passed: pragmatic-semantics, pragmatic-cybernetics, essentialist (7 sections), brand-voice
- [x] **T-03: hkask-capability Reference** — class diagram of OCAP tokens
  - Artifact exists; every symbol cited (20/20); diagram renders with `DIAGRAM_ALIGNMENT`
  - Gates passed: pragmatic-semantics (no OUGHT), essentialist (6 sections), brand-voice
- [x] **T-04: hkask-capability Explanation** — state diagram of verification outcomes
  - Artifact exists; OUGHT claims scoped to sovereignty rationale; loop closes
  - Gates passed: pragmatic-semantics (OUGHT in rationale), essentialist (6 sections), brand-voice

**Checkpoint 1:** `kask/docs/diataxis/` tree exists, INDEX.md seeded, 4 foundation artifacts pass all gates.

## Phase 2 — Core subsystems

- [x] **T-05: hkask-storage Reference** — ERD of SQLCipher schema
  - 25 tables cited to schema.sql:line or inline init_schema; ERD with Crow's Foot
- [x] **T-06: hkask-storage How-to** — adding a new migration
  - Procedural flowchart citing init_schema pattern + store_macros contract
- [x] **T-07: hkask-regulation Reference** — class diagram of Regulation ledger/loop/wallet
  - 30 symbols cited; 6 sections (merged to pass essentialist ≤7)
- [x] **T-08: hkask-regulation Explanation** — state diagram of homeostatic loop
  - 5-phase loop (sense→compare→compute→act→verify) verified against runtime.rs
- [x] **T-09: hkask-inference Reference** — class diagram of config + provider routing
- [x] **T-10: hkask-inference How-to** — configuring a new provider
- [x] **T-11: hkask-templates Reference** — ERD/class of skill manifest schema
- [x] **T-12: hkask-templates Explanation** — sequence of ManifestExecutor invocation
- [x] **T-13: hkask-condenser Reference** — class diagram of condensation algorithms
- [x] **T-14: hkask-condenser Explanation** — state diagram of 2-phase condensation
- [x] **T-15: hkask-mcp-server Reference** — class diagram of MCP server framework
- [x] **T-16: hkask-mcp-server Explanation** — sequence of MCP server launch

**Checkpoint 2:** all core subsystem reference + explanation artifacts pass gates.

## Phase 3 — Zed integration layer

- [x] **T-17: kask_bridge Reference** — class diagram of KaskSettings + bridges
- [x] **T-18: kask_bridge Explanation** — sequence of composition root (D1–D10)
- [x] **T-19: kask_panel Reference** — class diagram of panel view + curator variant
- [x] **T-20: kask_panel How-to** — adding a new panel action
- [x] **T-21: kask_bridge How-to** — wiring a new kask hook (set_* OnceLock pattern)
- [x] **T-22: kask_bridge Tutorial** — "Your first kask hook"

**Checkpoint 3:** zed-side integration artifacts pass gates; `// zed-kask:` deviations documented.

## Phase 4 — Tutorials and remaining how-tos

- [x] **T-23: hkask-types Tutorial** — "Understanding the port traits"
- [x] **T-24: hkask-types How-to** — "Implementing a new port"
- [x] **T-25: hkask-capability Tutorial** — "Your first capability token"
- [x] **T-26: hkask-capability How-to** — "Attenuating a token for a sub-task"
- [x] **T-27: hkask-storage Tutorial** — "Your first migration"
- [x] **T-28: hkask-storage Explanation** — bitemporal hMem model
- [x] **T-29: hkask-regulation Tutorial** — "Reading a Regulation span"
- [x] **T-30: hkask-regulation How-to** — "Adding a new span namespace"
- [x] **T-31: hkask-inference Tutorial** — "Routing your first inference request"
- [x] **T-32: hkask-inference Explanation** — provider selection rationale
- [x] **T-33: hkask-templates Tutorial** — "Your first skill manifest"
- [x] **T-34: hkask-templates How-to** — "Adding a PDCA step to a manifest"
- [x] **T-35: hkask-condenser Tutorial** — "Condensing your first thread"
- [x] **T-36: hkask-condenser How-to** — "Tuning salience weights"
- [x] **T-37: hkask-mcp-server Tutorial** — "Your first MCP server"
- [x] **T-38: hkask-mcp-server How-to** — "Registering a new tool"
- [x] **T-39: kask_panel Tutorial** — "Your first panel action"
- [x] **T-40: kask_panel Explanation** — curator variant lifecycle

**Checkpoint 4:** all 40 slices complete.

## Phase 5 — INDEX and finalization

- [x] **T-41: Write INDEX.md and update README**
  - `kask/docs/diataxis/INDEX.md` lists the full set with links
  - Every in-scope crate has 4 entries; every out-of-scope crate has "N/A — reason"
  - `kask/docs/README.md` links to the Diataxis set (pending)

**Checkpoint 5:** INDEX.md complete, all slices recorded.

## Per-slice gate checklist (applies to every slice T-01..T-40)

- [ ] kata-improvement PDCA cycle complete (direction → current → target → experiment)
- [ ] diataxis-diagram quality gate passes (≤ 0.15 weighted total)
- [ ] Diagram carries `DIAGRAM_ALIGNMENT` block with `verified_against` citing code file:line
- [ ] pragmatic-semantics: no uncited Inference claims (confidence ≤ 0.3 rejected)
- [ ] pragmatic-semantics: OUGHT claims only in Explanation quadrant
- [ ] pragmatic-cybernetics: feedback loop closes (documented code path exists + behaves as described)
- [ ] essentialist: deletion test passed, ≤ 7 top-level sections, no organizational comments
- [ ] grill-me: Mechanism round survived
- [ ] grill-me: Rationale round survived
- [ ] brand-voice rubric: all 8 criteria score 4+
- [ ] brand-voice: zero taboo phrases (no hype, no em dash chains, no exclamation points)
- [ ] DOCUMENTATION_STANDARDS §2: 6-field frontmatter present
- [ ] DOCUMENTATION_STANDARDS §5: every design-choice `##` section has ≥1 external footnote citation
- [ ] DOCUMENTATION_STANDARDS §11: `mds_categories` field maps to ≥1 MDS category
- [ ] Artifact links to ≥1 source file and ≥1 sibling artifact in the same crate's Diataxis set

---

## Kata Convergence Migration — Future Work

The convergence engineering has been migrated from the old self-grade model
(LLM grades its own plan quality on a [0,1] scale) to the Kata model
(deterministic gap + Cauchy + Brier). The executor (`hkask-templates`) and all
49 manifests are migrated. The remaining work is upgrading individual skills
from Cauchy-only (iterates stopped moving) to the full Kata model (gap +
Cauchy + Brier), which requires defining the target condition, experiment, and
prediction for each skill.

### What's done

- [x] **Executor: Kata convergence model** — `ConvergenceConfig`,
  `ConvergenceTracker`, `push_cycle_from_context`, `prev_*` snapshotting,
  5 compute primitives (`kata.object_gap`, `kata.process_gap`,
  `kata.hypotenuse`, `kata.prediction_vs_result`, `kata.convergence_check`)
- [x] **Executor: Three canonical stop conditions** — gap (limit of a
  sequence), Cauchy (iterates stopped moving), calibration (Brier score)
- [x] **Metacognition: Full Kata model** — 4 new templates (grasp, target,
  predict, experiment), deterministic convergence, Brier scoring
- [x] **Sequential-inquiry: Full Kata model** — 3 new templates (grasp,
  target, predict), deterministic convergence, Brier scoring
- [x] **All 47 remaining skills: Cauchy-only** — convergence block replaced,
  LLM convergence-check step replaced with `kata.convergence_check` compute,
  `kata_hypotenuse` added to loop steps, all convergence-check templates
  deleted
- [x] **All SKILL.md files updated** — stale convergence-check references
  removed, convergence model descriptions updated
- [x] **All template manifests updated** — deleted template entries removed
- [x] **All OCAP blocks updated** — deleted template capabilities removed

### What remains: upgrade Cauchy-only skills to full Kata model

Each of the 47 Cauchy-only skills can be upgraded to the full Kata model
(gap + Cauchy + Brier) by defining three things:

1. **Target condition** — what measurable state is the skill trying to reach?
   Expressed as Dublin Core artifacts (object space) + PKO procedure steps
   (process space). The hypotenuse `sqrt(object_gap² + process_gap²)` is
   the total distance to the target.

2. **Experiment** — what intervention does the agent perform? The "Do" in
   PDCA. After the experiment, the current condition is re-measured.

3. **Prediction** — what does the agent predict about the experiment's
   effect? "If I do X, the hypotenuse will decrease by Y" with a confidence
   in [0,1]. The Brier score tracks whether the confidence is calibrated.

The upgrade requires domain knowledge — the target conditions must be
articulated per skill. Below are proposed target conditions for the first
batch, to be reviewed and refined.

### Batch 1: Skills with clear target conditions

#### diagnose

- **Target condition**: root cause identified and fix verified. The bug no
  longer reproduces after the fix.
- **Object space (Dublin Core)**: bug_description (anchored), repro_steps
  (deterministic), hypothesis (ranked, falsifiable), instrumentation
  (targeted), fix (with regression test), verification (repro no longer
  reproduces).
- **Process space (PKO)**: spec-anchor → repro-loop → hypothesize →
  instrument → fix → verify. All steps complete.
- **Experiment**: apply the fix and run the repro. The intervention is the
  code change; the measurement is whether the bug still reproduces.
- **Prediction**: "If I apply fix X, the bug will no longer reproduce"
  with confidence based on how well the hypothesis explains the symptom.
- **Brier outcome**: did the bug reproduce after the fix? (binary: 1 =
  fixed, 0 = still reproduces)

#### falsifiability

- **Target condition**: all testable hypotheses have been eliminated or
  corroborated. No untestable hypotheses remain.
- **Object space**: admitted question (testable), hypotheses (each with
  counterfactual), discriminating tests (each hypothesis has a test),
  elimination results (each hypothesis eliminated or corroborated).
- **Process space**: admit → hypothesize → counterfactual → discriminate →
  eliminate. All steps complete.
- **Experiment**: run the discriminating test for the top hypothesis. The
  intervention is the test; the measurement is whether the hypothesis
  survived.
- **Prediction**: "If I run test X, hypothesis Y will be eliminated"
  with confidence based on the test's discriminating power.
- **Brier outcome**: was the hypothesis eliminated? (binary)

#### tdd

- **Target condition**: all planned behaviors have passing tests with
  minimal implementations, and the code has been refactored.
- **Object space**: plan (behaviors enumerated), tracer bullets (each
  behavior: test → minimal impl → green), refactor proposals (safe),
  verification (contract discipline), gap analysis (coverage complete).
- **Process space**: plan → tracer → refactor → verify → gap-check. All
  steps complete.
- **Experiment**: write the test and minimal implementation for one
  behavior. The intervention is the code change; the measurement is
  whether the test passes.
- **Prediction**: "If I implement behavior X with minimal code Y, the test
  will pass" with confidence based on how well the plan defines the
  behavior.
- **Brier outcome**: did the test pass? (binary)

#### bug-hunt

- **Target condition**: the charter's quality threats have been probed,
  oracles applied, and bugs taxonomized. No unexplored charter areas
  remain.
- **Object space**: charter (quality definition, threat model), probes
  (each probe targets a charter area), oracle results (each probe has an
  oracle verdict), bug taxonomy (each bug classified by Beizer category).
- **Process space**: charter → probe → oracle → taxonomize → report. All
  steps complete.
- **Experiment**: run a probe against an unexplored charter area. The
  intervention is the probe; the measurement is whether it found a bug.
- **Prediction**: "If I probe area X with technique Y, I will find a bug
  of category Z" with confidence based on the charter's threat model.
- **Brier outcome**: did the probe find a bug? (binary)

#### superforecasting

- **Target condition**: a calibrated probability estimate with Brier
  score below the threshold, informed by outside view, inside view, and
  Bayesian updating.
- **Object space**: triage (question classified), Fermi decomposition
  (sub-questions), outside view (base rates), inside view (specific
  factors), Bayesian update (posterior), dragonfly synthesis (final
  estimate), calibration (Brier score).
- **Process space**: triage → Fermi → outside view → inside view →
  Bayesian update → synthesis → calibration. All steps complete.
- **Experiment**: update the probability estimate based on new evidence.
  The intervention is the Bayesian update; the measurement is the
  posterior probability.
- **Prediction**: "If I incorporate evidence X, the probability will
  shift by Y" with confidence based on the evidence's likelihood ratio.
- **Brier outcome**: did the event occur? (binary, resolved at forecast
  deadline) OR the prediction's confidence vs the actual probability shift.

### Batch 2: Skills with moderate target conditions

#### kata-improvement

- **Target condition**: the learner can articulate all four Kata steps
  (direction, current condition, target condition, experiment) with
  specificity, and the experiment's prediction matched its result.
- **Object space**: direction (challenge understood), current condition
  (data-grounded), target condition (measurable, time-bounded),
  experiment (specific, testable, with prediction).
- **Process space**: direction → current → target → experiment →
  convergence. All steps complete.
- **Experiment**: run the PDCA experiment (one obstacle, one prediction,
  one measurement). The intervention is the experiment; the measurement
  is whether the prediction matched the result.
- **Prediction**: "If I do X, obstacle Y will be resolved" with
  confidence.
- **Brier outcome**: was the obstacle resolved? (binary)

#### kata-coaching

- **Target condition**: the learner can answer all 5 coaching questions
  with specificity (target, actual, obstacles, next step, feedback
  timing).
- **Object space**: Q1 (target condition specific), Q2 (actual condition
  data-grounded), Q3 (obstacles prioritized, one selected), Q4 (next step
  with prediction), Q5 (feedback timing defined).
- **Process space**: Q1 → Q2 → Q3 → Q4 → Q5. All questions answered.
- **Experiment**: the coach asks the next question. The intervention is
  the question; the measurement is whether the learner's answer is
  specific enough.
- **Prediction**: "If I ask question X, the learner's answer will be
  specific enough to proceed" with confidence.
- **Brier outcome**: was the answer specific enough? (binary)

#### mcda

- **Target condition**: a decision made with sensitivity analysis showing
  robustness (the top alternative doesn't change under reasonable weight
  perturbations).
- **Object space**: criteria (identified, weighted), alternatives
  (scored), ranking (top alternative identified), sensitivity analysis
  (robustness verified).
- **Process space**: identify criteria → score alternatives → rank →
  sensitivity analysis. All steps complete.
- **Experiment**: perturb the weights and re-rank. The intervention is
  the weight change; the measurement is whether the top alternative
  changes.
- **Prediction**: "If I perturb weights by X%, the top alternative will
  not change" with confidence based on the margin between top and second.
- **Brier outcome**: did the top alternative change? (binary)

#### hypothesis-framer

- **Target condition**: a FINER-evaluated, PICO-structured, testable
  hypothesis with a clear null hypothesis.
- **Object space**: FINER evaluation (all 5 criteria checked), PICO
  structure (Population, Intervention, Comparison, Outcome), null
  hypothesis (stated), aims (operationalized).
- **Process space**: FINER → PICO → null hypothesis → aims. All steps
  complete.
- **Experiment**: refine the hypothesis based on FINER feedback. The
  intervention is the refinement; the measurement is whether the
  hypothesis passes all FINER criteria.
- **Prediction**: "If I refine the hypothesis to address FINER criterion
  X, it will pass all 5 criteria" with confidence.
- **Brier outcome**: did the hypothesis pass all FINER criteria? (binary)

### Batch 3: Skills requiring further discussion

These skills need more discussion to define their target conditions:

- **gradient-hunter** — target = gradient explained? What's the experiment?
- **scenario-builder** — target = scenarios developed? What's the
  prediction?
- **wardley-mapper** — target = map complete? What's the experiment?
- **graph-audit** — target = audit complete? What's the prediction?
- **kali-audit** — target = vulnerabilities found? What's the experiment?
- **supply-chain-sentinel** — target = dependencies audited? What's the
  prediction?
- **runtime-posture-monitor** — target = threats detected? What's the
  experiment?
- **adversarial-red-team** — target = attack resistance measured? What's
  the prediction?
- **create-skill** — target = skill created and validated? What's the
  experiment?
- **task-breakdown** — target = tasks decomposed? What's the prediction?
- **prompt-enhance** — target = prompt enhanced? What's the experiment?
- **sankey-flow** — target = diagram generated? What's the prediction?
- **diataxis-diagram** — target = diagram generated? What's the
  experiment?
- **logo-builder** — target = logo refined? What's the prediction?
- **media-workflow** — target = workflow executed? What's the experiment?
- **structured-extraction** — target = data extracted? What's the
  prediction?
- **gpa-evolution** — target = Pareto frontier stable? What's the
  experiment?
- **self-improvement** — target = improvement applied? What's the
  prediction?
- **refactor-architecture** — target = architecture improved? What's the
  experiment?
- **lora-training** — target = training config validated? What's the
  prediction?
- **goal-analysis** — target = goal verified? What's the experiment?

### Batch 4: Skills that may stay Cauchy-only

These skills may not benefit from the full Kata model — they're
transformations or single-dimensional refinements where "the output stopped
changing" is the right convergence signal:

- **caveman** — text compression; no target condition beyond "stable
  output"
- **improv** — communication mode; no target condition
- **coding-guidelines** — behavioral guardrails; no experiment
- **essentialist** — elimination; target = "nothing more to eliminate"
  (Cauchy is the right signal)
- **grill-me** — Socratic interrogation; target = "no more gaps"
  (Cauchy is the right signal)
- **pragmatic-cybernetics** — VSM analysis; target = "analysis complete"
  (Cauchy)
- **pragmatic-semantics** — classification; target = "classification
  stable" (Cauchy)
- **idiomatic-rust** — design inquiry; target = "design converged"
  (Cauchy)
- **deep-module** — module assessment; target = "assessment stable"
  (Cauchy)

### Infrastructure manifests (stay Cauchy-only)

These are pipelines and runtime configs, not skills. Cauchy-only is
appropriate:

- **replica-discovery**, **skill-translation**, **stt-tts**,
  **voice-models**

### How to upgrade a skill from Cauchy-only to full Kata

For each skill being upgraded:

1. **Replace the convergence block** from `convergence_mode: "cauchy"` to
   `convergence_mode: "gap_or_cauchy_or_calibration"` and add the Kata
   target-condition fields (`target_artifacts_field`,
   `current_artifacts_field`, `target_procedure_field`,
   `current_procedure_field`, `prediction_field`, `result_field`).

2. **Restructure the steps** to follow the Kata PDCA pattern:
   - Step 1: Grasp current condition (measure, produce
     `current_artifacts` + `current_procedure`)
   - Step 2: Establish target condition (declare, produce
     `target_artifacts` + `target_procedure`)
   - Step 3: Make prediction (hypothesize, produce `prediction` with
     `confidence`)
   - Step 4: Experiment (apply intervention, re-measure, produce new
     `current_artifacts` + `current_procedure`)
   - Step 5: Compute object gap (`kata.object_gap`)
   - Step 6: Compute process gap (`kata.process_gap`)
   - Step 7: Compute hypotenuse (`kata.hypotenuse`)
   - Step 8: Score prediction (`kata.prediction_vs_result`)
   - Step 9: Check convergence (`kata.convergence_check`)
   - Step 10: Loop

3. **Create new templates** for the grasp, target, and predict steps
   (the experiment step reuses the skill's existing core template).

4. **Update the SKILL.md** to describe the Kata model.

5. **Test** by running the skill and checking that the Brier score
   converges across iterations.
