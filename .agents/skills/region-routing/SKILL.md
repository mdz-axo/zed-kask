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
  deterministic), Sem-P (semantic x probabilistic) — as defined in
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
output schema: per step produce region, tools, gate, role, and a checkpoint
field (Step 4 governs it), applying the
routing rule — input formal and answer pinned -> deterministic tool, oracle
named; input messy and output formal -> model proposes, deterministic gate
iterates, both named; semantic output with a cheap boundary check -> gate at
the boundary; semantic output with no cheap oracle -> probabilistic tool with
a verification state and a named calibration mechanism.

### Step 3: Check structural invariants (D)

Call `lisp_eval`:
- form: `(and (= unclassified 0) (= ungated 0) (= bad_region 0) (>= operator_roles 1) (>= other_roles 1))`
- env: `{ "unclassified": <steps with no region>, "ungated": <SP/Sem-P steps with no named gate>, "bad_region": <regions outside the four>, "operator_roles": <steps assigning the operator a role>, "other_roles": <steps assigning the curator or executing agent a role> }` — the five counts are P (model-performed counting over the delivered plan, critiqued by any consumer's recomputation); the invariant over them is D. A plan in which the operator is the only role is checkably wrong: the executing agent or curator must hold at least one step (the agent's traverse is the only function that routinely crosses all four regions in one task).

If false, re-enter Step 2 with the failing steps named — max 2 iterations,
then deliver with the failures listed. Never ship a plan the form rejects.

### Step 4: Place human checkpoints

Every Sem-P step that changes scope (spec, priorities, acceptance) carries
an operator-confirm checkpoint question. Every cross-region dependency — a
step whose output feeds a step in a different region — carries its gate
before the handoff. Operator checkpoints are asked questions, never assumed
consent.

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
| `region-routing/route` | Classify task steps into the 2x2 regions and emit the routing plan: region, tools, gate, and role per step. |

Template context variables (from the template's [inference] contract):
- `route.j2`: `task_description`, `steps`, `available_oracles`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-11 audit):

- Structural invariant, green: `{unclassified: 0, ungated: 0, bad_region: 0,
  operator_roles: 1, other_roles: 2}` → `true`.
- Structural invariant, ungated generating step: same env with `ungated: 1` →
  `false` — the primary defect class (an SP/Sem-P step with no named gate).
- Structural invariant, no operator role: `operator_roles: 0` → `false`.
- Convergence, done: `{ratio: 0.75, iteration: 1}` → `done` (≥ 70% as-planned).
- Convergence, iterate: `{ratio: 0.5, iteration: 1}` → `iterate`.
- Convergence, stop-and-report: `{ratio: 0.5, iteration: 2}` → `stop-and-report`.

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Region assignments are judgments (P) made checkable by the Step 3
  invariant (D) and by the gates that fire during execution — never present
  a plan as verified because it is well-formed.
- A generating step (SP, Sem-P) without a named deterministic gate is the
  primary defect class the invariant exists to catch; a plan that ships one
  is checkably wrong.
- The operator is an oracle, not a rubber stamp: operator-role steps produce
  an asked question, or the plan states that confirmation is pending —
  assumed consent is not confirmation.
- Deviations are data, not embarrassments: a plan that never deviates on a
  task class with any entropy is suspect, not exemplary.
- Max 2 classification iterations, then deliver with failures listed.
