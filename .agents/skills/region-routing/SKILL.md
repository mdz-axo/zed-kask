---
name: region-routing
description: Classify a task's steps into the syntax-semantic x deterministic-probabilistic 2x2 regions and emit a routing plan — named tools, a deterministic gate per generating step, and human/curator role assignments with checkpoints at semantic and dependency boundaries. Use when planning or reviewing a multi-step task whose work spans formal and semantic regions.
---

# Region Routing

## Initial and target condition

- **Initial condition:** a task description (with an optional step
  decomposition), the available oracles (agent tools, MCP tools, the
  operator, the curator), and the region model — SD (syntactic x
  deterministic), SP (syntactic x probabilistic), Sem-D (semantic x
  deterministic), Sem-P (semantic x probabilistic) — grounded in Morris (1938) / Carnap (1942) semiotics (`onto_anchor` → `derived:semiotics`) and Turing (1936/1937 `doi:10.1112/plms/s2-42.1.230`; 1939 `doi:10.1112/plms/s2-45.1.161` → `derived:deterministic_computation` / `derived:verification_oracle`), as defined in
  `kask/docs/research/syntax-semantic-probabilistic-deterministic-space.md`.
- **Target condition:** a consumed routing plan in which every step carries a
  region, at least one named tool, a named deterministic gate for every
  generating step (SP, Sem-P), and a human/curator role assignment; the
  consumer (executing agent or operator) has worked against it; deviations
  observed at task end are recorded.

## When to Use

- Planning a multi-step task whose steps mix formal and semantic work
  (agent-, operator-, or curator-initiated).
- Deciding where operator or curator involvement belongs in a task — which
  steps need a human checkpoint and which run ungated.
- Reviewing a mis-routed task post-mortem (a step executed in the wrong
  region produced the failure).

## When NOT to Use

- Routing a single computation step to its D/P regime — `pragmatic-semantics`
  semantics-route-step does that per step; this skill routes whole tasks
  across regions.
- Decomposing work into tasks — `task-breakdown` owns decomposition; this
  skill consumes a decomposition when one exists and derives a minimal one
  when not.
- Choosing which installed skill fits a request — `skill-discovery` owns
  that match; this skill assigns tools and roles within a chosen approach.

## Instructions

### Step 1: Assemble inputs

Collect `task_description`, the step list (empty when no decomposition
exists — the template derives a minimal step model), and
`available_oracles`: the concrete tools at hand, including the operator and
the curator as oracles for semantic questions.

### Step 2: Classify and route

Render `region-routing/route` with those inputs. Follow the template's
output schema: per step produce `step`, `depends_on`, `region`, `tools`, `gate`,
`verification_state`, `role`, `changes_scope`, and `checkpoint`, applying the
routing rule — input formal and answer pinned -> deterministic tool, oracle
named; input messy and output formal -> model proposes, deterministic gate
iterates, both named; semantic output with a cheap boundary check -> gate at
the boundary; semantic output with no cheap oracle -> probabilistic tool with
a verification state and a named calibration mechanism.

### Step 3: Check structural invariants (D)

Call `lisp_eval` once over the raw `step_model`, raw `routing_plan`, and the
original `provided_steps` array. Do not ask the model to count its own plan.
`RR-1` validates shape, sequential step ids, dependencies on earlier existing
steps, nonempty tool lists, region/role vocabularies, collapse paths,
cross-region handoff gates, scope-changing Sem-P checkpoints, and operator plus
agent/curator role coverage; it returns the exact bad step ids:

- form: `(begin (define blank (lambda (v) (or (is_null v) (and (stringp v) (= (length v) 0))))) (define valid-region (lambda (r) (and (stringp r) (or (string= r "SD") (string= r "SP") (string= r "Sem-D") (string= r "Sem-P"))))) (define valid-role (lambda (r) (and (stringp r) (or (string= r "operator") (string= r "curator") (string= r "agent"))))) (define find-step (lambda (n rows) (if (is_null rows) nil (if (and (numberp (assoc "step" (car rows))) (= n (assoc "step" (car rows)))) (car rows) (find-step n (cdr rows)))))) (define deps-ok (lambda (deps row rows) (if (is_null deps) true (let ((d (car deps))) (let ((src (if (numberp d) (find-step d rows) nil))) (and (numberp d) (>= d 1) (< d (assoc "step" row)) (not (is_null src)) (if (string= (assoc "region" src) (assoc "region" row)) true (not (blank (assoc "gate" row)))) (deps-ok (cdr deps) row rows))))))) (define row-ok (lambda (row rows expected) (let ((region (assoc "region" row)) (gate (assoc "gate" row)) (vs (assoc "verification_state" row))) (and (listp row) (numberp (assoc "step" row)) (= (assoc "step" row) expected) (valid-region region) (listp (assoc "tools" row)) (> (length (assoc "tools" row)) 0) (valid-role (assoc "role" row)) (listp (assoc "depends_on" row)) (deps-ok (assoc "depends_on" row) row rows) (if (string= region "Sem-P") (or (not (blank gate)) (not (blank vs))) (not (blank gate))) (if (and (eq (assoc "changes_scope" row) true) (string= region "Sem-P")) (not (blank (assoc "checkpoint" row))) true))))) (define all-rows-ok (lambda (rows all expected) (if (is_null rows) true (and (row-ok (car rows) all expected) (all-rows-ok (cdr rows) all (+ expected 1)))))) (define bad-steps (lambda (rows all expected) (if (is_null rows) (list) (if (row-ok (car rows) all expected) (bad-steps (cdr rows) all (+ expected 1)) (cons (assoc "step" (car rows)) (bad-steps (cdr rows) all (+ expected 1))))))) (define strings-ok (lambda (xs) (if (is_null xs) true (and (stringp (car xs)) (> (length (car xs)) 0) (strings-ok (cdr xs)))))) (define count-role (lambda (rows role) (if (is_null rows) 0 (+ (if (and (stringp (assoc "role" (car rows))) (string= (assoc "role" (car rows)) role)) 1 0) (count-role (cdr rows) role))))) (let ((n (length routing_plan)) (m (length step_model)) (p (length provided_steps)) (operator-roles (count-role routing_plan "operator")) (other-roles (+ (count-role routing_plan "agent") (count-role routing_plan "curator")))) (let ((shape-ok (and (> n 0) (= n m) (strings-ok step_model) (if (= p 0) (and (>= m 3) (<= m 7)) (= m p)))) (rows-ok (all-rows-ok routing_plan routing_plan 1))) (list (list "gate_passed" (and shape-ok rows-ok (> operator-roles 0) (> other-roles 0))) (list "bad_steps" (bad-steps routing_plan routing_plan 1)) (list "operator_roles" operator-roles) (list "other_roles" other-roles) (list "shape_ok" shape-ok)))))`
- env: `{ "provided_steps": <the Step-1 input array, empty when the template derived steps>, "step_model": <the route output's string array>, "routing_plan": <the route output's row array verbatim> }`

If `gate_passed` is false, re-enter Step 2 with `bad_steps` and any false shape
or role reading named — max 2 iterations, then deliver with the failures listed.
Never ship a plan `RR-1` rejects.

### Step 4: Place human checkpoints

Every Sem-P step that changes scope (spec, priorities, acceptance) carries
`changes_scope: true` and an operator-confirm checkpoint question. Every
cross-region dependency — a step whose output feeds a step in a different
region — carries the consuming step's handoff gate before execution. Operator
checkpoints are asked questions, never assumed consent. `RR-1` above enforces
both properties from the raw rows; there is no second model-counted checkpoint
gate.

### Step 5: Deliver the plan (Act)

Hand the plan to the consumer and execute against it. The plan is a
commitment device: deviations become visible against it.

### Step 6: Close the loop

At task end, record what the plan got wrong: steps that changed region
mid-flight, gates that fired (defects caught), human overrides. Feed the
pattern into the next routing of the same task class; propose durable
changes via `skill-maintenance` or the algedonic review — never edit this
skill ad hoc.

## Convergence

The improvement signal is external: steps routed as-planned versus
re-routed or overridden, counted from observed outcomes at task end. After
each use, check in `lisp_eval`:
- form: `(if (>= ratio 0.7) (quote done) (if (>= iteration 2) (quote stop-and-report) (quote iterate)))`
- env: `{ "ratio": <as-planned steps / total steps, observed>, "iteration": <1-based> }`

`done` requires at least 70% as-planned; `iterate` re-enters Step 2 for the
task class; `stop-and-report` delivers the deviation record. The count comes
from observed outcomes, never from the model scoring its own plan.

## D/P labelling

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Assemble inputs | P | critiqued by the Step 3 invariant over the resulting plan |
| 2 Classify and route | P (the render itself is D feeding P) | the Step 3 form; the gates that fire at task end |
| 3 Check invariants | D | `lisp_eval` |
| 4 Place checkpoints | P | the operator's actual confirmations — asked, not assumed |
| 5 Deliver and execute | P/D per the plan's own assignments | the named gates |
| 6 Close the loop | P (deviation counts are observed) + D (ratio check) | recorded deviations; algedonic review |

## Registry Templates

| Template | Purpose |
|----------|---------|
| `route.j2` | Classify task steps into the 2x2 regions and emit the routing plan: region, tools, gate, and role per step. |

Template context variables (from the template's [inference] contract):
- `route.j2`: `task_description`, `steps`, `available_oracles`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-11 audit):

- Structural invariant, green: three sequential rows with valid regions,
  dependencies, tools, gates, roles, one scope-changing Sem-P checkpoint and
  both operator/other role coverage → `gate_passed: true`, `bad_steps: []`.
- Structural invariant, defects: remove step 1's tools, step 2's cross-region
  handoff gate and step 3's scope checkpoint → `gate_passed: false`,
  `bad_steps: [1,2,3]`.
- Derived shape: an empty `provided_steps` input accepts a 3–7 item
  `step_model`; a nonempty input requires exact length parity.
- Convergence, done: `{ratio: 0.75, iteration: 1}` → `done` (≥ 70% as-planned).
- Convergence, iterate: `{ratio: 0.5, iteration: 1}` → `iterate`.
- Convergence, stop-and-report: `{ratio: 0.5, iteration: 2}` → `stop-and-report`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Region assignments are judgments (P) made checkable by the Step 3
  invariant (D) and by the gates that fire during execution — never present
  a plan as verified because it is well-formed.
- A row without tools, a valid region/role, a collapse/verification path,
  valid earlier dependencies, or a required scope checkpoint is the primary
  defect class `RR-1` exists to catch; a rejected plan carries exact bad step
  ids, never a model-authored counter.
- The operator is an oracle, not a rubber stamp: operator-role steps produce
  an asked question, or the plan states that confirmation is pending —
  assumed consent is not confirmation.
- Deviations are data, not embarrassments: a plan that never deviates on a
  task class with any entropy is suspect, not exemplary.
- Max 2 classification iterations, then deliver with failures listed.
