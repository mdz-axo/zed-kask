---
name: product-manager
description: The operator's side of the Division of Responsibilities — the product manager's judgment deliverables in the AI age. Requirements as falsifiable outcome claims, spec provenance, acceptance criteria that can fail, decisions on the wire-or-remove queue, and ground-truth confirmation. Dual audience: the operator reads it as their working guide; the agent consults it when a request arrives underspecified, to structure what to ask for instead of improvising the requirement.
---

# Product Manager

The Division of Responsibilities makes the operator the product manager:
the keeper of the functional requirements, the spec authority, and the
judge of what the work is for. The agent is the program manager: it
recovers specs, designs, executes, verifies, and reports outcomes. This
skill is the delivery contract for the product-manager side.

## The PM role in the AI age

Production is cheap; judgment is the edge. AI agents execute — they
write the code, run the gates, sweep the dead surface — which strips
away the performative parts of product management (backlog grooming,
status chasing, requirements transcription) and leaves the demanding
core (per Marty Cagan's framing and the 2026 role literature): **deciding
what is worth producing, and judging whether it was**. In this system
that means the operator's deliverables are judgment artifacts —
requirements worth authoring, decisions worth making, ground truth worth
giving. Each one has a failure mode when missing: the agent improvises
the requirement (observed: a hallucinated model id baked into eval
cards), defers the decision (the wire-or-remove queue grows as
requirement debt), or guesses the verdict (the goal loop never closes).

## Ontological Anchors

- **Product management practice** (IBM's lifecycle framing; Product
  School's PM/TPM split): the PM owns vision, requirements,
  prioritization, and post-launch judgment; delivery coordination
  belongs to the program manager. Requirements flow PM → TPM;
  decisions flow back.
- **The AI-age shift** (Cagan; 2026 role literature): from information
  gathering to judgment and decision-making — the PM's edge is deciding
  what is worth producing, not producing fast.
- **Requirements engineering + hypothesis framing**: a requirement is a
  falsifiable outcome claim; acceptance criteria are its measurement
  plan. A criterion that cannot fail is not a criterion.
- **This project's functional-interaction spec** (the four moves): the
  operator's words are the goal; the agent interprets, never revises;
  the goal loop Brier-scores predictions on both sides.
- **Cybernetics (VSM)**: the operator is the policy level (S5); decisions
  owed to the agent are the algedonic channel's return path — an
  unanswered decision is a blocked loop, and the queue of them is
  requirement debt.
- **The Toyota Improvement Kata**: the PM sets direction and target
  condition; the agent experiments; the PM's confirmation is the check.

## When to Use

- **Operator-facing**: starting a bit of work and handing over a
  complete order; asked for a decision; wanting to know what the agent
  needs from you.
- **Agent-facing**: a request arrives without a stated functional
  outcome, spec provenance, or acceptance criteria — consult this skill
  to structure what you ask for before writing code.

## When NOT to Use

- The request already carries the requirement, the spec pointers, and
  the criteria — proceed (the program-manager skill takes over).
- Trivial mechanical tasks with no functional target.

## What the product manager delivers

### At intake (Move 1 — point at the same target)

1. **The requirement, as a falsifiable outcome claim.** What you will be
   able to do, or what stops being a problem — in your own words; these
   are the goal's words, and the agent interprets them back to you for
   correction. A requirement that cannot fail ("improve the pipeline")
   is not yet a requirement — state the claim the work will prove or
   refute.
2. **Spec provenance.** If a spec exists, say where (commit, doc, prior
   decision). If none exists, say so — "no spec exists; design this with
   me" is a complete answer. Recovering a spec that exists and designing
   one that doesn't are different jobs; never let the agent guess which.
3. **Acceptance criteria (2–4, observable, able to fail).** How you will
   judge the outcome — what you will see or be able to do. These are the
   target condition: measurable, and specific enough that the work could
   fail them. Implementation metrics (test counts, lint passes) are not
   criteria.
4. **Things that bind.** Two kinds, stated once: constraints on this
   work (what not to touch, budgets, prior decisions that bind — "the
   AIMD ramp is the ratified spec") and standing rules for every
   engagement ("shortcuts are unacceptable", "fix the tool, don't work
   around it"). A rule you don't state is a correction you'll deliver
   later at higher cost.

### At decision points (Move 2 — the decision queue)

5. **Decisions, made.** When the agent surfaces a choice — wire-or-remove
   on a designed-but-unwired capability, an experience-changing option —
   expect an MCDA-shaped brief: the criteria, the weights, the scored
   options, and the sensitivity note ("this flips if you weight X
   differently"). Your call is the ratification the record needs. Every
   deferred decision is requirement debt with a carrying cost — the
   agent re-surfaces it, blocks on it, or silently attenuates around it.
   If you must defer, say so; a priced deferral is a plan.

### At confirmation (Moves 3–4 — evaluate and close)

6. **Ground truth, with the grill.** Judge the outcome by what it lets
   you do — and interrogate the report as deeply as you need to:
   what was done → how it works → why this design → what breaks at the
   edges → what was learned. An agent that cannot survive the ladder
   has not finished. Your confirmation resolves the goal and scores
   the intake prediction; honest calibration on both sides depends on
   it.
7. **Corrections as context.** When work misses, the correction is
   load-bearing — say it in the words you want repeated back. The agent
   cannot generate your corrections; it can only receive and bank them.

## The kata alignment

The collaboration IS the Improvement Kata, split across the division:
you set the direction and the target condition (the requirement and
criteria); the agent grasps the current condition, experiments (PDCA
against one obstacle at a time), and reports; your confirmation is the
check; the banked learning starts the next cycle. When a requirement is
too large for one goal, it is a program: state the direction, let the
agent decompose it into vertical slices, and ratify the plan before
execution begins.

## What to expect back

The program-manager skill governs the agent's side in full. In brief:
an interpretation of your requirement before code; choices as
experiences with recommendations; outcome reports that lead with what
you can now do; every open item named with an owner and a closure path;
durable decisions recorded in the curator's memory.

## Instructions (agent use — structuring an underspecified request)

1. Render the intake brief (`render_template`,
   `product-manager/intake-brief`) with what the request already
   provides; the empty fields are your questions.
2. Ask for the missing fields in one round, ordered by what blocks work
   first: requirement → spec provenance → criteria → binding rules. Do
   not start coding on an improvised requirement.
3. If the operator says a spec exists but cannot point to it, recover it
   (`git log -S`, docs, commit messages) and label the reconstruction
   for their ratification — per the spec-loss rule.
4. Record the delivered requirement in the operator's words as the
   `goal_text` when the goal loop is active.

## Constraints

- The functional requirement is the operator's. The agent interprets,
  never revises — an "improved" requirement is a revision, and
  revisions are the operator's call.
- A requirement is a falsifiable claim; acceptance criteria must be able
  to fail. "Improve X" with no failure condition is not intake — it is
  an unformed requirement; ask.
- Never fill a missing spec silently: recover it and label the
  reconstruction, or ask. "No spec exists" is a valid answer to receive.
- Decisions owed to the operator are a queue, not a suggestion box:
  surface them with criteria, weights, and sensitivity; record the
  deferrals with their price.
- The operator's corrections are context the agent cannot generate —
  treat every correction as a standing instruction for the rest of the
  work, and bank it in the learning.
