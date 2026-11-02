---
title: "Goal Systems — Reference Models from Goal-Setting, Alignment, and Kata Literature"
audience: [architects, developers, agents]
last_updated: 2026-11-02
version: "1.0.1"
status: "Active"
domain: "Composition"
mds_categories: [composition, trust]
---

# Goal Systems — Reference Models from Goal-Setting, Alignment, and Kata Literature

This document records the reference models for the **canonical goal system** —
the human-authority verification ledger over immutable requirements
(`kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/types/goal.rs`,
`service_impl/goals.rs`, `GOAL_INTERFACE.md`, the `kask/crates/kask_bridge/src/goals.rs`
drainer, and the agent-side native ActionChoice surface
`crates/agent/src/tools/goal_tools.rs`) — drawn from the goal-setting,
AI-alignment, and kata/kanban literature, following the kanban-board
precedent. It exists because the goal system took two schema shocks (the
trust-system addition and its removal, 2026-10-27..2026-11-01) and the
2026-11-02 recomposition must anchor on published models rather than
re-invent. Research run `6eb1d124a8cb38c4` (2026-11-02) supplies the
citations; the anchor selection was stress-tested through grill-me,
essentialist, improv, hypothesis-framer and diagnose passes (10-artifact
evidence set: 6 domains, 10 independent clusters, sensitivity stable).

## The anchor models

1. **Goal-setting theory** — Locke & Latham (2002), *American Psychologist*
   57(9). Specific, appropriately challenging goals with feedback outperform
   vague "do your best" goals; for complex or novel tasks, learning goals
   outperform performance goals. → the criterion schema (1–4 explicit
   criteria, each covered exactly once), the assessment feedback loop, and
   the learning-goal layer delegated to the task/kata level.

2. **Goal pathologies and reward hacking** — Ordóñez, Schweitzer, Galinsky &
   Bazerman (2009), *Academy of Management Perspectives* (Goals Gone Wild);
   Amodei et al. (2016), arXiv:1606.06565 (Concrete Problems in AI Safety);
   Manheim & Garrabrant (2018), arXiv:1803.04585 (Goodhart's-law variants);
   the 2026 Springer survey of reward hacking in agentic LLM systems.
   Narrow measurable goals invite gaming and tunnel vision; protection is
   structural, not motivational. → the human-only terminal (native
   ActionChoice closure), the disclosure-before-read prompt, digest-matched
   evidence, and "reading a file does not prove a claimed command ran".

3. **Proxy overoptimization and hidden objectives** — Gao et al. (2022),
   arXiv:2210.10760; Marks & Treutlein (2025), arXiv:2503.10965. Proxies
   diverge from true objectives under optimization pressure; audit observed
   behavior, not stated intent. → the blinded assessor: frozen specification
   + evidence manifest + observed artifacts only; never worker claims, human
   labels, forecast or scores.

4. **Protected expectations** — Hadfield-Menell & Hadfield (2018)
   (Incomplete Contracting and AI Alignment); the expectation-interest
   framework (Fuller & Perdue 1936–37; Cornell Wex). The goal is a contract: the frozen specification is the
   contract identity, the human's as-if-performed position is the protected
   expectation, and changed requirements declare a successor — never a silent
   edit. → `spec_digest` validation on closure, immutable criteria, the
   `supersedes` chain, human corrections that append without moving the
   original event.

5. **Toyota Kata and PDCA** — Rother (2010); already published in this
   corpus (`13800577.toyota-kata.html`). The improvement kata (target
   condition, one focus obstacle, one experiment, coaching questions) is the
   worker-side convergence loop. → `kanban_task_kata_record` and the kata
   prompts on TASKS; the goal seam is deliberately evidence-free (see
   Boundaries).

6. **Expectations investing** — Rappaport & Mauboussin (2001); already
   published in this corpus (`expectations-investing.md`). Reading reality
   against pre-committed expectations rather than post-hoc narratives. →
   optional prospectively frozen forecasts, resolved only against the human
   outcome (Brier-scored against a matched baseline; unknown is unscored;
   no probability is synthesized for an ordinary goal).

## The anti-model

The trust system (2026-10-27..2026-10-31) priced worker compliance on goals
via an agent-operated ledger (`trust_clause`, `trust_events`,
`trust_summary`). It failed exactly as the reward-hacking literature
predicts: a self-reported proxy under optimization pressure is a Goodhart
channel. The operator ruling of 2026-10-31/2026-11-01 deleted it — the goal
verdicts included — and the 2026-11-02 recomposition does not resurrect it.
Where the trust clause tried to protect expectations with agent accounting,
the structural protections (anchors 2–4) already do: consent, frozen
specification, independent assessment, human-only closure.

## The schema-evolution contract (2026-11-02)

A change to the stored row shape bumps `schema_version` — never an in-place
field swap. The current field set is pinned key-for-key by test
(`goal_row_shape_is_pinned_to_the_current_schema`); retired names are pinned
as visible decode failures that leave stored bytes unchanged. Obsolete rows
(superseded versions, or retired-field shapes) are removed by an explicit
operator-ruled one-time reset under the Forward-only policy — never by
runtime compatibility. The 2026-11-01 removal of `verdicts` /
`trust_clause` / `trust_events` without a bump made every stored schema-3
row undecodable (the outage that made this discipline load-bearing); the
executed reset of 2026-11-02 removed the ten obsolete rows and their recall
projections (`GOAL_INTERFACE.md` records it).

## Boundaries

- **Kata below the goal seam.** Improvement-kata records are task-scoped
  worker convergence state. They are deliberately NOT goal evidence: the
  assessor reads bounded local artifacts with digest verification, not
  process claims. Tasks cite goal criteria (the `advances` citations) so
  kata convergence serves the goal, but no kata state crosses the evidence
  boundary.
- **Forecasts resolve against the human.** Only an explicit known human
  outcome whose original closure time falls inside the frozen window is
  eligible; corrections relabel the original event; unknown is unscored.
- **Human authority is native.** Closure and correction are separate native
  human interactions; worker inputs never carry identity or source fields; a
  concrete ActionChoice is the only record.

## Citations

Edwin A. Locke, Gary P. Latham (2002). Building a practically useful theory
of goal setting and task motivation: A 35-year odyssey. American Psychologist.
https://doi.org/10.1037/0003-066x.57.9.705

Lisa D. Ordóñez, Maurice E. Schweitzer, Adam D. Galinsky, Max H. Bazerman
(2009). Goals Gone Wild: The Systematic Side Effects of Overprescribing Goal
Setting. Academy of Management Perspectives.
https://doi.org/10.5465/amp.2009.37007999

Amodei, Olah, Steinhardt, Christiano, Schulman et al. (2016). Concrete
Problems in AI Safety. https://arxiv.org/abs/1606.06565

Gao, Schulman, Hilton et al. (2022). Scaling Laws for Reward Model
Overoptimization. https://arxiv.org/abs/2210.10760

Manheim & Garrabrant (2018). Categorizing Variants of Goodhart's Law.
https://arxiv.org/abs/1803.04585

Samuel Marks, Johannes Treutlein et al. (2025). Auditing language models for
hidden objectives. https://arxiv.org/abs/2503.10965

A survey of reward hacking in agentic large language model systems (2026).
Discover Artificial Intelligence (Springer).
https://doi.org/10.1007/s44163-026-01980-z

Dylan Hadfield-Menell, Gillian K. Hadfield (2018). Incomplete Contracting
and AI Alignment. USC Law School Legal Studies Working Paper Series.
https://law.bepress.com/cgi/viewcontent.cgi?article=1400&context=usclwps-lss

Expectation damages (Wex). Cornell Law School Legal Information Institute.
https://www.law.cornell.edu/wex/expectation_damages