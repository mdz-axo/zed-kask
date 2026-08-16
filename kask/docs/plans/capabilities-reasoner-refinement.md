# Capabilities-Reasoner Process Refinement Plan

**Status**: Proposed. Awaiting review.
**Date**: 2026-08-15
**Author**: Curator (GLM 5.2)
**Context**: Follows the falsifiability/lean-prover/hypothesis-framer analysis of the
capabilities-reasoner as a meta-skill. Six fixes were identified. This plan grounds
each fix in research, applies a four-perspective review (essentialist, grill-me,
pragmatic-semantics, idiomatic Rust), and specifies the implementation.

## Problem statement

The capabilities-reasoner is a kask skill that evaluates capability systems against
floor/ceiling/maturity-gate thresholds. It now has a "skill composition" domain with
five composition principles. But the reasoner's own manifest fails three of the five
principles it teaches:

| Principle | Reasoner's own verdict | Root cause |
|-----------|----------------------|------------|
| Determinism frontier | BELOW FLOOR | 5 `select`, 0 `execute` — all evaluation is LLM-mediated |
| Persistence-grounded learning | BELOW FLOOR | No ordinal 0 read of prior evaluations |
| Failure surfacing | EXEMPT | No `execute` steps (nothing to fail) |
| Lisp scaffold | Level 2 | Has `lisp.eval` convergence check |
| Co-evolution loop | Level 1 | No `on_failure: report` (no `execute` steps) |

This is the Dunning-Kruger trap: the reasoner teaches principles it doesn't practice.
The evaluation is structurally sound (static manifest analysis, no circularity), but
the evaluation *quality* is LLM-mediated where it could be deterministic — exactly
the deficiency the determinism frontier principle warns against.

Additionally, the falsifiability analysis found:
- No ceiling on the determinism frontier (a skill with 100% `execute` is a pipeline)
- Missing implicit dependency: `lisp_scaffold` should depend on `failure_surfacing`
- No calibration mechanism for the reasoner's own evaluations (Dunning-Kruger gap
  is predicted but unmeasured)

## Research grounding

### Fix 1: Ceiling on the determinism frontier

**Reference models**:
- **Conant-Ashby Good Regulator theorem** (Conant & Ashby, 1970): "Every good
  regulator of a control system must be a model of that system." A regulator that
  is pure mechanism (no model) cannot regulate — it can only execute. Applied to
  skills: a skill with 100% `execute` steps and 0% `select` steps has no internal
  model of its domain — it's a pipeline, not a regulator. The LLM's `select` step
  IS the model — it exercises judgment about what the deterministic steps produced.
- **Sen's capability/functioning distinction** (Sen, 1999): capability is the set
  of *feasible* functionings, not just the realized ones. A skill with only
  `execute` steps has only one functioning per step (the tool's output). A skill
  with `select` steps has a *capability space* — the LLM could produce different
  outputs from the same inputs, adapting to context. Removing all `select` steps
  collapses the capability space to a single functioning.
- **Ousterhout's deep module principle** (Ousterhout, *A Philosophy of Software
  Design*, 2018): a module should have a deep interface — simple interface, complex
  implementation. A skill with only `execute` steps has a shallow interface (the
  tool's input schema) and no implementation depth (no reasoning). The `select`
  step provides the depth — it synthesizes multiple tool outputs into a judgment.

**Value added**: Prevents the degenerate case where a "skill" is actually a
deterministic pipeline with no LLM judgment. This is the ceiling counterpart to
the floor: the floor says "don't use `select` where `execute` would work"; the
ceiling says "don't use `execute` where `select` is needed."

### Fix 2: lisp_scaffold depends on failure_surfacing

**Reference models**:
- **CMMI prerequisite DAG** (SEI CMMI v2.0): maturity level N requires level N−1
  practices. The `lisp_scaffold` is a Level 2 maturity capability; `failure_surfacing`
  is a Level 1 floor capability. A skill that has Lisp invariant checks but no
  `on_failure` on its `compute` steps is at Level 2 on one axis and below floor on
  another — the Lisp check's failure is silently swallowed.
- **Ashby's Law of Requisite Variety** (Ashby, 1956): the regulator's variety must
  match the system's variety. A `compute` step that can fail (Lisp parse error,
  missing env binding, type mismatch) has variety (multiple failure modes). The
  `on_failure: report` channel provides the requisite variety to surface each
  failure mode. Without it, the failure variety is collapsed to a single
  `TemplateError` — the regulator (operator/Curator) cannot distinguish failure
  modes.
- **kask `.rules`**: "Opt-in features that fail must log the failure classification,
  not collapse to `None` via `.ok()?`." `compute` steps that fail without
  `on_failure` collapse to `TemplateError` — the operator sees
  `ExitKind::Escalated` with no failure classification.

**Value added**: Closes a gap where `compute` step failures are silently swallowed.
The `on_failure` config is already supported on all step types (not just `execute`)
— the `dispatch_with_retry` handler checks it for any step that fails. This fix
just declares the dependency in the ontology and adds `on_failure: report` to
the reasoner's own `compute` step.

### Fix 3: Deterministic execute steps for manifest analysis

**Reference models**:
- **Password-Locked Models** (Arditi et al., 2024): capability is elicited
  potential, not observed behavior. An LLM-mediated evaluation (`select` step)
  observes the LLM's behavior under default prompting — it may miss capabilities
  the LLM has but didn't elicit (e.g., the LLM might not realize a `select` step
  could be an `execute` step). A deterministic evaluation (`execute` step calling
  a manifest-analysis tool) elicits the capability mechanically — it doesn't
  depend on the LLM's judgment.
- **Conant-Ashby Good Regulator theorem**: the regulator must model the system.
  An LLM-mediated evaluation's model of the manifest is the LLM's internal
  representation — which may be incomplete or hallucinated. A deterministic
  evaluation's model is the manifest itself — parsed by code, not interpreted
  by an LLM. The deterministic evaluation is a better model of the system.
- **Dunning-Kruger effect** (Kruger & Dunning, 1999): the skills to perform a
  task are the same skills to evaluate it. An LLM evaluating whether a `select`
  step could be an `execute` step is exercising the same judgment the `select`
  step exercises — if the LLM is bad at the task, it's also bad at evaluating
  the task. A deterministic check doesn't have this problem — it applies
  mechanical rules (e.g., "does the step's `input_mapping` reference only prior
  step results? → could be `execute`").
- **Mirage paper** (Schaeffer et al., 2023): capability is metric-dependent.
  An LLM-mediated evaluation uses "LLM judgment" as the metric — which is
  high-variance and metric-sensitive. A deterministic evaluation uses "mechanical
  rule check" as the metric — which is low-variance and metric-stable. The
  reasoner's own convergence criterion (metric stability) is better satisfied
  by deterministic evaluation.

**Value added**: Moves the reasoner above floor on the determinism frontier AND
fixes the Dunning-Kruger gap. The evaluation becomes deterministic for structural
properties (which can be checked mechanically) and reserves LLM judgment for
semantic properties (which require reasoning). This is the determinism frontier
principle applied to the reasoner itself.

**Implementation note**: The `composition_principles.rs` test file already
contains the deterministic check logic (lisp symbol binding, on_failure presence,
convergence mode validity, compute input mapping coverage). The fix is to expose
this logic as an MCP tool that the reasoner can call via `execute` steps, rather
than only as test code. Alternatively, the reasoner can use `codegraph_query`/
`codegraph_analysis` to analyze manifests structurally (they're YAML files in
the codebase).

### Fix 4: Persistence-grounded learning at ordinal 0

**Reference models**:
- **Toyota Improvement Kata** (Rother, 2009): "grasp the current condition" before
  establishing a target. The reasoner should grasp its own prior evaluations before
  starting a new evaluation cycle. Without this, each evaluation is context-free —
  the reasoner doesn't know whether its prior verdicts were confirmed or refuted.
- **Bayesian updating** (Jaynes, 2003): prior beliefs should be updated with new
  evidence. The reasoner's prior evaluations are its prior beliefs about the
  target system's capabilities. Reading them before starting updates the prior —
  the new evaluation is conditioned on the old, not independent.
- **Good Regulator theorem**: the regulator must model the system's history. A
  regulator that doesn't read its own prior outputs has no memory — it cannot
  detect whether the system has improved or regressed since the last evaluation.

**Value added**: Closes the feedback loop for the reasoner's own evaluations.
The reasoner reads its prior verdicts (via `curator_memory_recall` or
`scenario_calibration`) before starting, so it can detect improvement or
regression. This is the persistence-grounded learning principle applied to the
reasoner itself.

### Fix 5: Record evaluations as forecasts

**Reference models**:
- **Tetlock's GJP superforecasting** (Tetlock & Gardner, 2015): forecasts should
  be recorded with timestamps and probabilities so they can be Brier-scored
  against outcomes. The reasoner's verdicts ("skill X is below floor on the
  determinism frontier") are forecasts — they predict that the skill's `select`
  steps could be `execute` steps. The outcome is whether an expert (or the
  skill's subsequent performance) confirms this prediction.
- **Brier scoring** (Brier, 1950): the mean squared error between predicted
  probability and actual outcome. A verdict with high confidence ("this step
  COULD be execute") that turns out wrong (the step genuinely requires LLM
  judgment) produces a high Brier score — the reasoner was overconfident.
  Tracking Brier scores over time measures the Dunning-Kruger gap: a below-floor
  reasoner should have higher Brier scores (worse calibration) than an
  above-floor reasoner.
- **Metacognition skill's Brier loop**: the metacognition skill already uses
  `scenario_score` to persist predictions and `scenario_calibration` to read
  Brier scores. The reasoner should adopt the same pattern — it's the
  calibration loop (2.1) from the co-evolution plan.

**Value added**: Measures the Dunning-Kruger gap rather than just predicting it.
The reasoner's evaluations become falsifiable forecasts with recorded outcomes —
the system can detect whether the reasoner's confidence is calibrated. This is
the co-evolution loop principle applied to the reasoner's own evaluations.

### Fix 6: Convergence signal is already correctly wired

**Reference models**:
- **Cauchy convergence criterion**: a sequence converges if its terms eventually
  stop moving. The below-floor count is the convergence signal — it goes
  2 → 2 → 1 → 0 (non-monotone plateau at iteration 1, then monotone decrease).
  The Cauchy criterion correctly detects the plateau (count hasn't stabilized)
  and continues the loop.
- **No fix needed** — this was verified by the falsifiability analysis. The
  `lisp.eval` form counts unresolved verdicts (`expand + restrict + block`),
  which IS the below-floor count. The Cauchy window (3) is sufficient to handle
  the plateau.

**Value added**: None — this is a confirmation, not a fix. Documented for
completeness.

## Multi-perspective review

### Essentialist (3-gate eliminative interrogation)

For each fix, the three gates: **Exist** (does it need to exist?), **Surface**
(is its value obtainable by existing mechanisms?), **Contract** (does it add
complexity without adding capability?).

#### Fix 1: Ceiling on determinism frontier
- **Exist**: YES. Without a ceiling, "push all work to execute" has no stopping
  condition. A skill with 0 `select` steps is a pipeline — the category system
  already distinguishes `skill` from `pipeline`, but the composition principles
  don't enforce the distinction.
- **Surface**: NO existing mechanism. The `category: skill` field is a declaration,
  not an enforcement. A manifest can declare `category: skill` while having 0
  `select` steps — nothing catches this.
- **Contract**: PASSES. The ceiling is one additional threshold on an existing
  capability. No new mechanism, no new complexity.
- **Verdict**: KEEP.

#### Fix 2: lisp_scaffold depends on failure_surfacing
- **Exist**: YES. A `compute` step that fails silently is a broken feedback loop
  — the `.rules` trap. The dependency is already implicit (the `dispatch_with_retry`
  handler supports `on_failure` on `compute` steps); the fix makes it explicit.
- **Surface**: PARTIALLY. The `error_handling.on_timeout: retry` catches timeout
  failures, but not parse errors or missing env bindings. `on_failure: report`
  catches all failure types and surfaces them to the Curator.
- **Contract**: PASSES. Adding one edge to the prerequisite DAG and one
  `on_failure` config to the reasoner's `compute` step. No new mechanism.
- **Verdict**: KEEP.

#### Fix 3: Deterministic execute steps for manifest analysis
- **Exist**: YES. This is the core fix — without it, the reasoner is below floor
  on the principle it teaches. The Dunning-Kruger gap is real and unmeasured.
- **Surface**: PARTIALLY. The `composition_principles.rs` test already contains
  the check logic — but it's test code, not production code. The fix exposes it
  as an MCP tool or uses existing codegraph tools. The question is whether to
  build a new MCP tool or reuse existing ones.
- **Contract**: CHALLENGED. Adding a new MCP tool (`manifest_analysis` or
  `skill_composition_check`) is significant complexity. The essentialist asks:
  can we achieve the same result by having the reasoner read its own manifest
  via `codegraph_query` (which already exists) and apply the checks in the
  `select` step's template? The answer: no — the whole point is that `select`
  steps are LLM-mediated and unreliable for structural checks. The checks must
  be deterministic. But we could implement them as `lisp.eval` compute steps
  (which are deterministic) rather than new MCP tools.
- **Verdict**: KEEP, but simplify. Instead of a new MCP tool, add `lisp.eval`
  compute steps that check the manifest's structural properties. The manifest
  is already in the step context (it's the input). The Lisp form can check:
  "count execute steps without on_failure", "count select steps that could be
  execute", etc. This is the lisp scaffold pattern applied to the reasoner
  itself.

#### Fix 4: Persistence-grounded learning at ordinal 0
- **Exist**: YES. Without reading prior evaluations, each evaluation is
  context-free. The reasoner cannot detect improvement or regression.
- **Surface**: YES — `curator_memory_recall` already exists as an MCP tool
  and is in `KNOWN_MCP_TOOLS`. The reasoner can call it via an `execute` step
  at ordinal 0 to read prior verdicts stored as episodic h_mems.
- **Contract**: PASSES. One `execute` step at ordinal 0, calling an existing
  MCP tool. No new mechanism.
- **Verdict**: KEEP.

#### Fix 5: Record evaluations as forecasts
- **Exist**: CHALLENGED. The essentialist asks: does the reasoner need to record
  its evaluations as forecasts, or is the `reg.skill.<id>.outcome` span
  sufficient? The span records the evaluation happened, but not the specific
  verdicts with probabilities. The Brier scoring requires probabilities and
  outcomes — the span doesn't carry these.
- **Surface**: PARTIALLY. The `scenario_score` MCP tool persists forecasts
  with events and outcomes. But the reasoner's verdicts are not forecasts in
  the `scenario_score` sense — they're structural assessments, not probability
  estimates. The mapping is awkward: "skill X is below floor on determinism
  frontier" is a binary verdict (below/above), not a probability.
- **Contract**: CHALLENGED. Adding `scenario_score` persistence to the reasoner
  introduces a new semantic category (evaluation-as-forecast) that doesn't
  fit cleanly into the existing forecast model. The essentialist asks: is this
  complexity necessary, or can the Dunning-Kruger gap be measured more simply?
- **Verdict**: DEFER. The Dunning-Kruger gap measurement is valuable but the
  implementation is complex. Defer to a future iteration. For now, the
  persistence-grounded learning (Fix 4) provides the feedback loop — the
  reasoner reads its prior verdicts and can compare them to current verdicts.
  The Brier scoring of evaluations is a separate concern that requires a
  ground-truth oracle (expert verdicts) that doesn't exist yet.

#### Fix 6: Convergence signal already wired
- **Exist**: NO — it already exists. No fix needed.
- **Verdict**: CONFIRMED. No action.

### Grill-Me (Socratic interrogation, escalating difficulty)

#### Recall level
Q: What are the five composition principles?
A: Determinism frontier, persistence-grounded learning, failure surfacing, lisp scaffold, co-evolution loop.

#### Mechanism level
Q: How does the bootstrap loop converge if expanding the reasoner introduces new below-floor verdicts?
A: The convergence signal is the below-floor count (unresolved verdicts). Each expansion fixes one verdict but may introduce one new verdict (the failure_surfacing side effect of adding `execute` steps). The count goes 2 → 2 → 1 → 0 — the plateau at iteration 1 is handled by the Cauchy criterion (count hasn't stabilized, loop continues). The loop converges in 3 iterations.

Q: But what if a bad expansion introduces TWO new violations?
A: The count could go 2 → 3 — the loop diverges. This is not structurally prevented. The convergence depends on expansion quality. The `max_iterations: 10` bound prevents infinite divergence — the loop escalates if it doesn't converge in 10 iterations. This is acceptable: the escalation surfaces the problem to the operator.

#### Rationale level
Q: Why is the ceiling on the determinism frontier "at least 1 select step" rather than "at least 20% select steps"?
A: The "at least 1" threshold is the minimal non-trivial ceiling — it catches the degenerate case (0 `select` steps = pipeline) without imposing an arbitrary ratio. A ratio-based ceiling would require empirical calibration that we don't have. The "at least 1" is a structural invariant, not a performance threshold.

Q: Why use `lisp.eval` for manifest analysis (Fix 3) instead of a dedicated MCP tool?
A: The `lisp.eval` interpreter is already available, deterministic, and doesn't require a new MCP server. The manifest is already in the step context (it's the input). The Lisp form can traverse the manifest's JSON representation and check structural properties. A dedicated MCP tool would be cleaner but adds infrastructure. The essentialist correctly challenged the complexity — `lisp.eval` is the minimal sufficient mechanism.

#### Edge cases level
Q: What happens if the reasoner evaluates a manifest that has 0 steps?
A: The `lisp.eval` form would receive an empty manifest. The below-floor count would be 0 (no `select` steps to check, no `execute` steps to check). The verdict would be "maintain" for all principles — which is wrong (a 0-step manifest is not a skill). The fix: the `lisp.eval` form should check `length(steps) > 0` as a precondition.

Q: What happens if the reasoner evaluates itself while it's being modified?
A: The manifest is loaded at the start of the cascade and doesn't change during execution. The evaluation is against the loaded manifest, not the live file. This is correct — the evaluation is a snapshot, not a live monitor.

#### Synthesis level
Q: Is the capabilities-reasoner a meta-skill (a skill that reasons about skills) or a meta-meta-skill (a skill that reasons about skills that reason about skills)?
A: It's a meta-skill — it reasons about skills, including itself. It's not meta-meta because it doesn't reason about skill-reasoning processes; it reasons about skill manifests (static artifacts). The self-reference is at the artifact level, not the process level. This is the key distinction that makes the self-evaluation sound: the reasoner reads its own source code (manifest), not its own runtime behavior. A compiler can check its own source code without infinite regress; the reasoner can check its own manifest for the same reason.

### Pragmatic Semantics (certainty classification, IS/OUGHT)

#### Fix 1: Ceiling on determinism frontier
- **Claim**: "A skill with 0 `select` steps is a pipeline, not a skill."
- **Certainty**: IS (structural fact — the `category` field distinguishes `skill`
  from `pipeline`, and a skill with 0 `select` steps has no LLM judgment, which
  is the defining characteristic of a skill vs. a pipeline).
- **OUGHT**: "The determinism frontier should have a ceiling of at least 1
  `select` step." This is an OUGHT — a design recommendation, not a structural
  fact. It's grounded in the Conant-Ashby theorem (a regulator needs a model)
  and the category system (skills have LLM judgment, pipelines don't).
- **Conflict**: None. The IS supports the OUGHT.

#### Fix 2: lisp_scaffold depends on failure_surfacing
- **Claim**: "`compute` steps can fail, and their failures should be surfaced."
- **Certainty**: IS (structural fact — `dispatch_with_retry` handles failures
  for all step types, and `compute` steps can produce `TemplateError` on parse
  failures or missing env bindings).
- **OUGHT**: "The `lisp_scaffold` maturity gate should declare
  `failure_surfacing` as a prerequisite." This is an OUGHT — a dependency
  declaration. It's grounded in the CMMI prerequisite DAG pattern.
- **Conflict**: None.

#### Fix 3: Deterministic execute steps
- **Claim**: "LLM-mediated evaluation of structural properties is less reliable
  than deterministic evaluation."
- **Certainty**: IS (structural fact — LLM judgment is probabilistic;
  deterministic checks are... deterministic. The mirage paper shows that
  capability is metric-dependent, and LLM judgment is a high-variance metric).
- **OUGHT**: "The reasoner should use `lisp.eval` compute steps for structural
  property checks and reserve `select` for semantic judgment." This is an OUGHT.
- **Conflict**: None. But note: the claim is about *reliability*, not
  *correctness*. An LLM can produce a correct evaluation; it's just less
  reliable. The OUGHT is about confidence calibration, not logical soundness.

#### Fix 4: Persistence-grounded learning
- **Claim**: "The reasoner should read its prior evaluations before starting."
- **Certainty**: OUGHT (design recommendation). The IS is: "the reasoner
  currently doesn't read prior evaluations." The OUGHT is: "it should."
  Grounded in the Toyota Improvement Kata ("grasp the current condition").
- **Conflict**: None.

#### Fix 5: Record evaluations as forecasts
- **Claim**: "Evaluations should be Brier-scored against ground truth."
- **Certainty**: OUGHT (design recommendation). The IS is: "there is no
  ground-truth oracle for skill composition evaluations." The OUGHT is:
  "there should be." But the OUGHT depends on an external oracle (expert
  verdicts) that doesn't exist.
- **Conflict**: YES. The OUGHT assumes a ground-truth oracle exists. Without
  it, the Brier scores have no outcomes to score against. The essentialist
  correctly identified this: defer until the oracle exists.
- **Resolution**: DEFER Fix 5. The persistence-grounded learning (Fix 4)
  provides the feedback loop without requiring an oracle. The reasoner can
  compare current verdicts to prior verdicts (have they changed?) without
  needing to know which verdicts were "correct."

### Idiomatic Rust

#### Fix 3 implementation: lisp.eval vs. new MCP tool
- **lisp.eval approach**: The manifest is already in the step context as a JSON
  value. The Lisp form can traverse it using `assoc`, `car`, `cdr`, `length`,
  `is_null`. The form would be complex but deterministic. No new Rust code.
- **MCP tool approach**: A new `manifest_analysis` tool on a new or existing
  MCP server. The tool takes a manifest path and returns structural analysis
  (execute step count, on_failure coverage, select-vs-execute ratio, etc.).
  Requires new Rust code, new MCP server wiring, new `KNOWN_MCP_TOOLS` entry.
- **Idiomatic Rust assessment**: The `lisp.eval` approach is more idiomatic —
  it uses the existing deterministic compute primitive without adding
  infrastructure. The Lisp form is data-driven (traverses JSON), not
  control-flow-heavy. The MCP tool approach would be cleaner separation but
  violates the essentialist's "minimal sufficient mechanism" principle.
- **Verdict**: Use `lisp.eval`. If the Lisp form becomes too complex (the
  manifest analysis requires more than `assoc`/`car`/`cdr`/`length`), then
  build the MCP tool. Start with the simpler approach.

#### Fix 2 implementation: on_failure on compute steps
- The `OnFailureConfig` is already on `BundleManifestStep` (not specific to
  `execute`). The `dispatch_with_retry` handler already checks `on_failure`
  for any step that fails. Adding `on_failure: report` to a `compute` step
  is a YAML change, not a Rust change.
- **Idiomatic Rust assessment**: No Rust changes needed. The schema already
  supports it. This is a manifest-level fix.

#### Fix 4 implementation: execute step at ordinal 0
- `curator_memory_recall` is already an MCP tool in `KNOWN_MCP_TOOLS`. Adding
  an `execute` step at ordinal 0 that calls it is a YAML change.
- **Idiomatic Rust assessment**: No Rust changes needed.

## Revised plan (after review)

### Fixes to implement (4 of 6)

| Fix | Status | Rationale |
|-----|--------|-----------|
| 1. Ceiling on determinism frontier | IMPLEMENT | Essentialist: KEEP. Grill-Me: "at least 1" is minimal non-trivial. Prag-Sem: IS supports OUGHT. |
| 2. lisp_scaffold depends on failure_surfacing | IMPLEMENT | Essentialist: KEEP. Grill-Me: on_failure already supported on compute. Prag-Sem: IS (compute can fail). No Rust changes. |
| 3. Deterministic lisp.eval for manifest analysis | IMPLEMENT (simplified) | Essentialist: KEEP but simplify — use lisp.eval, not new MCP tool. Grill-Me: lisp.eval is minimal sufficient. Idiomatic Rust: no new infrastructure. |
| 4. Persistence-grounded learning at ordinal 0 | IMPLEMENT | Essentialist: KEEP (existing tool). Grill-Me: closes feedback loop. Prag-Sem: OUGHT grounded in Kata. No Rust changes. |
| 5. Record evaluations as forecasts | DEFER | Essentialist: CHALLENGED (complexity). Prag-Sem: OUGHT assumes oracle that doesn't exist. Grill-Me: ground-truth oracle missing. |
| 6. Convergence signal | NO ACTION | Already correctly wired. Confirmed by falsifiability analysis. |

### Implementation specification

#### Fix 1: Add ceiling to determinism frontier in ontology

File: `kask/registry/templates/capabilities-reasoner/capability-ontology.yaml`

Change `determinism_frontier` from `limit_type: floor` to `limit_type: floor_and_ceiling`:
```yaml
  - id: determinism_frontier
    name: "Determinism frontier"
    limit_type: floor_and_ceiling
    floor: >
      0 select steps that could be execute/compute. Every select step
      must require LLM judgment.
    ceiling: >
      At least 1 select step. A skill with 0 select steps is a pipeline
      (deterministic execution with no LLM judgment), not a skill. The
      Conant-Ashby Good Regulator theorem requires the regulator to have
      a model of the system — the select step IS the model.
    ...
```

#### Fix 2: Add failure_surfacing prerequisite to lisp_scaffold

File: `kask/registry/templates/capabilities-reasoner/capability-ontology.yaml`

Add `failure_surfacing` to `lisp_scaffold`'s prerequisites:
```yaml
  - id: lisp_scaffold
    ...
    prerequisites:
      - "determinism_frontier (the skill must already separate deterministic from probabilistic steps)"
      - "failure_surfacing (compute step failures must be surfaced via on_failure: report — a lisp.eval step that fails silently is a broken feedback loop)"
```

Also add `on_failure: report` to the reasoner's own `compute` step (ordinal 6):
```yaml
  - ordinal: 6
    action: compute
    compute_ref: lisp.eval
    on_failure:
      action: report
      resume: >
        The convergence check (lisp.eval) failed — the Lisp form could not
        parse or the env binding is missing. The cascade cannot determine
        convergence; it escalates without a convergence signal.
    ...
```

#### Fix 3: Add lisp.eval compute steps for deterministic manifest analysis

Add two new `compute` steps to the capabilities-reasoner manifest that
deterministically check the manifest being evaluated against the composition
principles. These steps replace some of the LLM-mediated evaluation in the
current `select` steps.

New step layout (ordinals shift):

```
0: execute (curator_memory_recall) — read prior evaluations [Fix 4]
1: select (capability-register) — build registry [existing, shifted]
2: select (capability-elicit) — elicit capabilities [existing, shifted]
3: compute (lisp.eval) — deterministic manifest structure check [NEW]
   Checks: execute step count, on_failure coverage, select step count,
   compute step count, ordinal 0 presence. Produces a structural_score.
4: select (capability-evaluate) — LLM evaluates semantic properties [existing, shifted]
   Now consumes step_3_result (structural analysis) alongside step_2_result.
5: select (capability-reason) — determine interventions [existing, shifted]
6: select (capability-report) — compile verdicts [existing, shifted]
7: compute (lisp.eval) — convergence check [existing, shifted, now with on_failure]
8: loop [existing, shifted]
```

The new step 3 `lisp.eval` form:
```lisp
(let ((manifest target_manifest))
  (let ((steps (assoc "steps" manifest)))
    (let ((n (length steps)))
      (if (= n 0)
          (list "empty_manifest")
          (begin
            (define count-action
              (lambda (ss action-name acc)
                (if (is_null ss)
                    acc
                    (let ((s (car ss)))
                      (let ((a (assoc "action" s)))
                        (if (string= a action-name)
                            (count-action (cdr ss) action-name (+ acc 1))
                            (count-action (cdr ss) action-name acc)))))))
            (define n-execute (count-action steps "execute" 0))
            (define n-select (count-action steps "select" 0))
            (define n-compute (count-action steps "compute" 0))
            (define structural-defects
              (append
                (if (and (= n-select 0) (> n 0))
                    (list "zero_select_steps_ceiling_violation")
                    (list))
                (if (and (= n-execute 0) (= n-compute 0))
                    (list "no_deterministic_steps_below_floor")
                    (list))))
            structural-defects)))))
```

This form:
- Checks for 0 `select` steps (ceiling violation — Fix 1)
- Checks for 0 `execute` + 0 `compute` steps (below floor on determinism)
- Returns a list of structural defect strings (empty = no structural defects)

The LLM `select` step (step 4) then focuses on semantic evaluation:
- Which `select` steps could be `execute`? (requires LLM judgment)
- Is the persistence read at ordinal 0 appropriate? (requires LLM judgment)
- Are the Lisp invariant checks covering the right properties? (requires LLM judgment)

This is the determinism frontier applied to the reasoner itself: structural
checks are deterministic (`compute`), semantic checks are LLM-mediated (`select`).

#### Fix 4: Add execute step at ordinal 0

Add an `execute` step at ordinal 0 that calls `curator_memory_recall` to read
prior evaluations stored as episodic h_mems:

```yaml
  - ordinal: 0
    action: execute
    description: >
      Read prior capability evaluations from the curator's memory via
      curator_memory_recall MCP tool. Deterministic. Returns prior verdicts,
      capability_lessons, and verdict_signatures from previous iterations.
      Feeds the register step (step 1) so the reasoner knows its past
      evaluations and can detect improvement or regression.
    mcp: curator_memory_recall
    gas_cap: 2000
    timeout_seconds: 60
    on_failure:
      action: report
      resume: >
        curator_memory_recall failed — prior evaluations are not available.
        The reasoner proceeds without prior context; each evaluation is
        context-free.
    input_mapping:
      entity: "skill_use_issue:capabilities-reasoner"
```

### Gas budget check

New steps: 0 (execute, 2000) + 3 (compute, 100) = 2100 additional gas.
Current sum: 26724. New sum: 28824. × min_iterations(2) = 57648.
Current cap: 120000. **Adequate** (no cap increase needed).

### Self-evaluation after fixes

| Principle | Before | After | Change |
|-----------|--------|-------|--------|
| Determinism frontier | BELOW FLOOR (0 execute) | FLOOR (1 execute + 2 compute + 4 select) | Fixed by Fix 3 + Fix 4 |
| Persistence-grounded learning | BELOW FLOOR (no ordinal 0) | FLOOR (ordinal 0 execute) | Fixed by Fix 4 |
| Failure surfacing | EXEMPT (no execute) | FLOOR (on_failure on execute + compute) | Fixed by Fix 2 + Fix 4 |
| Lisp scaffold | Level 2 | Level 3 (structural + convergence lisp.eval) | Enhanced by Fix 3 |
| Co-evolution loop | Level 1 | Level 2 (on_failure: report flows to Curator) | Fixed by Fix 2 + Fix 4 |

The reasoner moves from 2 below-floor + 1 exempt to 0 below-floor + 0 exempt.
The bootstrap loop converges in 1 iteration (the fixes are applied all at once,
not iteratively).

### Test updates

1. Update `composition_principles.rs` — the `mcp_steps_have_failure_handling` test
   should now find `on_failure` on the reasoner's `compute` step.
2. Add a new test: `capabilities_reasoner_practices_what_it_preaches` — checks
   that the reasoner's own manifest passes all five composition principles.
3. Update `manifest_invariants.rs` — gas budget check (new step count).

### Files to modify

1. `kask/registry/templates/capabilities-reasoner/capability-ontology.yaml` —
   Fix 1 (ceiling) + Fix 2 (prerequisite)
2. `kask/registry/manifests/capabilities-reasoner.yaml` —
   Fix 2 (on_failure on compute) + Fix 3 (new lisp.eval step) + Fix 4 (ordinal 0 execute)
3. `kask/registry/templates/capabilities-reasoner/capability-register.j2` —
   accept `prior_evaluations` from step 0
4. `kask/registry/templates/capabilities-reasoner/capability-evaluate.j2` —
   accept `structural_analysis` from step 3
5. `kask/crates/hkask-templates/tests/composition_principles.rs` —
   update test expectations
6. `.agents/skills/capabilities-reasoner/SKILL.md` —
   update step count and composition principle self-evaluation

### Deferred: Fix 5 (record evaluations as forecasts)

Deferred because:
1. No ground-truth oracle exists for skill composition evaluations
2. The `scenario_score` model doesn't cleanly map to binary structural verdicts
3. The persistence-grounded learning (Fix 4) provides the feedback loop without
   requiring an oracle
4. The essentialist correctly identified the complexity as not yet justified

Future work: when expert verdicts on skill composition become available (e.g.,
from human review of the reasoner's evaluations), the reasoner can record its
verdicts as forecasts and Brier-score them. This requires:
- A ground-truth oracle (expert verdicts stored as `scenario_score` outcomes)
- A mapping from binary verdicts (below/above floor) to probabilities
- A `scenario_calibration` read at ordinal 0 to check the reasoner's Brier score

This is the natural next iteration after the current fixes are applied and the
reasoner has been running for enough cycles to accumulate evaluation history.
