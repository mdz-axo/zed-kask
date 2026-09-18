---
title: "Gödel-Machine Gap Closure — Refactor Architecture Plan (Tracks A and B)"
audience: [architects, operators, agents]
last_updated: 2026-09-18
version: "0.1.0"
status: "Active"
domain: "Self-improvement"
mds_categories: [trust, lifecycle, composition]
---

# Gödel-Machine Gap Closure — Refactor Architecture Plan (Tracks A and B)

## 1. Purpose and status

This document is the reviewable output of the `refactor-architecture`
process (explore → candidates → deepen → route → audit → strangle →
verify) for closing the gaps between zed-kask's self-change machinery
and the Gödel-machine reference model established by the 2026-09-18
deep-research report[^report].

It became an implementation program when the operator directed the
work and chartered goal
`5e12d79e-5f9f-4314-8f94-5e5f165e58e4` (2026-09-18). §10 records the
first implementation pass. Commits are withheld pending an explicit
operator request; landed work is uncommitted and named as such.

### Functional target

Every self-change (skill-body edit, adapter training, prompt
reconfiguration, schema evolution) carries a falsifiable, Brier-scored
outcome claim registered before execution; nothing ships as an
improvement without a measured before/after delta; the
acceptance-criteria system is a named, machine-checkable trust basis
that cannot drift unnoticed; and the training math-contract gates
carry proof-grade verification for their core iff-properties.

## 2. Reference model

The Gödel machine[^goedel] is a fully self-referential problem solver
that rewrites any part of its own code only when a proof search
demonstrates the rewrite improves expected utility, making accepted
self-improvements provably optimal (the Global Optimality Theorem).
zed-kask adopts the architecture's shape without its formal
machinery, on the evidence of the successor literature: the
construct's own implementation lineage relaxed proof search into
restricted change classes and empirical gating[^steunebrink], and the
2026-09-18 implementation survey found zero Gödel-machine
implementations at the operator's Rust ≥100-star bar.

Two readings are carried as explicit, never-merged tracks:

- **Track A — formal machinery.** Literal proof search over
  self-rewrites with provable improvement guarantees. Scoped to a
  bounded Kani[^kani] proof pilot on the training math-contract gates
  (slice S4); literal proof search is declined (§3, decision record).
- **Track B — principles.** Self-reference, explicit utility signal,
  utility-gated change acceptance, verification-gated
  self-modification, homeostatic monitoring. The reference pattern is
  the empirically-gated self-rewrite loop of the Darwin Gödel
  Machine[^dgm] and the eval-gated code evolution it descends from.

## 3. Ratified distinctions and boundaries

### Trust basis (B-R3.i)

The acceptance criteria of this system are fixed in
`kask/docs/architecture/principle-constraints.yaml` and the
enforcement points it names. That registry is the axiom layer.
Changes to the registry, to any `enforced_at` path, or to the
checking scripts require explicit operator ratification. The operator
is the fixed point of the trust basis — the defense the Löbian
obstacle lesson prescribes against quiet gate drift.

### Decision record: literal proof search declined (A-R1)

Literal Gödel-machine proof search over arbitrary self-rewrites is
declined, on the 2026-09-18 evidence: the operator's Rust ≥100-star
bar returns a clean null (the only Rust repositories matching "godel
machine" are Gödel-numbering coursework utilities at 1 and 0 stars);
the construct's own lineage relaxed it[^steunebrink]; practitioner
consensus holds the proof searcher to be the hard component. This
record prevents re-litigation; revisit only with new evidence.

### Per-channel candidate bias (B-R3.ii)

The Gödel machine's "first found proof" encodes an enumeration bias.
Each self-change channel's candidate generator is named so that bias
is auditable:

| Channel | Candidate generator | Ordering bias |
| --- | --- | --- |
| Skill bodies | Agent or operator edits, on demand | Recency and operator salience |
| Adapters (θ) | Verdict-labeled rollouts (`training_bridge_rollouts`) | Failure-rich tasks dominate |
| Prompts | `gpa-evolution` population-based evolution | Pareto-frontier-anchored exploration |
| Agent cards | Operator or `swarm_select_agent_local` reconfiguration | Measured tasks only |
| MCP schemas | `evolve_mcp_tool_schema` directives from skill-use failures | Failure-driven |
| Regulation set points | Operator tuning | Operator attention |
| Repo code | Agent PRs; acceptance = operator review + tests | Whichever stream the operator steers |

### Change-class boundary

No self-change auto-promotes on a green gate. Operator ratification
remains the acceptance authority (research report §5, Q2 default). The
process gates added by this plan make acceptance *evidence* mandatory;
they do not remove the operator from acceptance *authority*.

## 4. Codebase ground

The plan is constrained by these current structures (all paths
verified at write time):

- `kask/crates/hkask-regulation/src/` — the cybernetics loop: sense
  (`cybernetics_loop/cycle.rs`) against explicit set points
  (`set_points.rs`), policy-mapped actions (`regulation_policy.rs`),
  capped algedonic ring (`algedonic.rs`, default 200).
- `kask/mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/` —
  goal lifecycle (`goals.rs`: `goal_create`/`goal_judge`/`goal_score`)
  and the task→goal citation primitive, test-pinned by
  `task_create_validates_goal_citations`.
- `kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs`
  — pure math-contract gate functions (G-M1..G-M4, G-Q*, G-H1) over
  `LoraParams` (`r: u32`, `alpha: u32`, bias/init enums), enforced at
  the tool seam by `training_validate_config` and at submission by
  `training_submit`'s refusal check.
- `.agents/skills/adapter-lifecycle/SKILL.md` — the verifier-gated
  adapter loop: rollout-harness baseline → dataset bridge → gate
  validation → training → evaluation → `lisp_eval` convergence gate →
  `memory_insert` verdict.
- `kask/scripts/audit/skill-corpus-*.sh` — the skill-corpus structural
  gates (S9/S10 sweep, prescreen) and
  `kask/scripts/check-principle-constraints.sh` — the constraint
  registry hook.
- `kask/docs/reference/lora-training-catalog.md` — the written gate
  spec (G-M1..G-M5 assertions and sources) that any proof must anchor
  to.

## 5. Selected architecture

Six slices. Each names the gap it closes (research report §1 rows),
the change, and its falsifier.

**S1 — Axiom entries in the constraint registry** (closes A3-lack,
L3/L6-partial). The registry
`kask/docs/architecture/principle-constraints.yaml` gains its first
principle entry (`self-change-acceptance`) with nine constraints
(enforced/gap), each carrying `assertion`, `status`, `enforced_at`,
`falsifier`. Validated by the existing hook
`kask/scripts/check-principle-constraints.sh`. A parallel
axiom-artifact-plus-checker was refused by the deletion test: it
would duplicate the registry.

**S2 — Utility ledger, process layer** (closes A2/B2-partial).
`self-improvement` and `adapter-lifecycle` now require: register a
falsifiable outcome claim (`kanban_goal_create` with prediction; task
linked via `advances`) before executing, and judge it
(`kanban_goal_judge`) at commit/rollback; `adapter-lifecycle` adds the
goal-judge step (11) after the verdict record. No new code — the
linkage primitive and its test already exist.

**S3 — Mandatory empirical acceptance gate, process layer** (closes
B3/B4-partial, lesson L4). `self-improvement` requires before/after
`swarm_eval_agent_local` measurement plus a `lisp_eval` convergence
gate before `si-commit-or-rollback` may decide; `skill-maintenance`
requires a measured eval delta for material SKILL.md process changes.
This is the DGM/OpenEvolve acceptance pattern at scaffold scope.

**S4 — Kani proof pilot** (closes the bounded A5 slice). A
`#[cfg(kani)]` proofs module in `param_gates.rs` proves, over the
symbolic config space: G-M3 refuses exactly degenerate scaling
(r=0 or α=0); G-M4 findings track the 128/256 thresholds exactly; G-M1
is clean exactly for None/Default/EVA initializers; the safe region
(r ∈ 1..=128, α ≥ 1, defaults elsewhere) produces zero refusals
across G-M1..G-M4. Zero dependency churn: the `kani` library is
toolchain-provided at `cargo kani` time; the crates.io `kani` crate is
a 3-line placeholder and must not be added. Run:
`cargo kani -p hkask-mcp-training` (toolchain absent on host —
execution parked, §7).

**S5 — This plan** plus its `kask/docs/README.md` index row.

**S6 — Parked operator decisions** (§9).

## 6. Migration sequence

One domain per commit, commits withheld. S1–S4 land as one review
unit (this pass); S6 items convert to work only on operator decision.
Dependency order was: S5 records → S1 names the trust basis → S2/S3
extend the executing skills → S4 proofs (independent of S1–S3).

## 7. Verification plan

Oracle tiers, stated honestly:

- **Runs today**: the constraint-registry hook (S1 falsifier);
  `cargo test -p hkask-mcp-training` (existing gate smoke tests pin
  the gate behavior the proofs reason about); the skill-corpus S9/S10
  sweep over the edited skills; residue greps for new identifiers;
  `find kask/docs -type f` count against the under-70 cap.
- **Parked (owner: operator)**: `cargo kani -p hkask-mcp-training` —
  semantic proof execution requires the Kani toolchain, absent on
  this host. Regular builds syntax-check the proofs module but do not
  compile it; the proofs' claims are asserted, not yet verified, and
  are recorded as `status: gap` in the registry until run.

## 8. Risk register

| Risk | Likelihood | Severity | Mitigation |
| --- | --- | --- | --- |
| Kani proofs unexecuted (toolchain absent) | Certain today | The S4 claims rest unverified | Registry records `gap`; plan §7 names the run command |
| `check-principle-constraints.sh` silently reports "no principles" when python3/PyYAML is absent | Low on this host (verified present) | Registry contents unchecked — a broken feedback loop | Named here; fix is out of this plan's scope (pre-existing defect) |
| B-R1 not CI-enforced (no hook blocks a self-change PR without a goal link) | Certain until Q3 | Process-only enforcement | Operator decision §9 Q3 |
| Auto-promotion boundary drifts | Low | High (the trust basis) | Fixed in plan §3; registry entry `ax-goal-outcome-claims` |

## 9. Operator decisions

Defaults taken under the operator's directive; each is a veto point:

1. **Q2 — Autonomy boundary.** Default: no auto-promotion of any
   self-change class; the operator ratifies every acceptance.
2. **Q3 — B-R1 enforcement strength.** Default: process-layer
   mandatory (skills + registry), no CI hard-gate yet. A commit-time
   hook checking goal citations on self-change paths is the follow-up
   if you want it.
3. **Q4 — Kani toolchain.** Default: proofs parked as `gap` until you
   install Kani (https://model-checking.github.io/kani/install.html)
   and run `cargo kani -p hkask-mcp-training`.

## 10. Implementation record (2026-09-18 pass)

Landed (uncommitted):

- `kask/docs/plans/goedel-gap-closure-plan.md` (this document).
- `kask/docs/architecture/principle-constraints.yaml` — axiom-layer
  entries (S1).
- `.agents/skills/self-improvement/SKILL.md` — B-R1/B-R2 constraints
  (S2, S3).
- `.agents/skills/adapter-lifecycle/SKILL.md` — goal registration at
  Phase 1, goal judge step 11 (S2).
- `.agents/skills/skill-maintenance/SKILL.md` — measured-delta gate
  for material skill-body changes (S3).
- `kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs`
  — `#[cfg(kani)]` proof module (S4).
- The deterministic closure-ledger check (`program-manager` Phase 5.5
  form) ran green on this pass's ledger and red on a deliberately
  abandoned item — validated in both directions. Two earlier arity
  errors were a transcription slip in the caller's tool invocation,
  not a defect in the skill: the file's form (committed in
  2988f090f8, pre-dating this pass) is correct.
- `kask/mcp-servers/hkask-mcp-training/README.md` — gate-verification
  section (S4 docs).
- `kask/docs/README.md` — plans-table row, corpus count, version bump.

Commit state (2026-09-18): a concurrent stream committed mid-pass and
swept five of the files above into commit `6054422cc3` ("Add
InferenceUsage reported flag, sweep dead knobs") under a message that
does not describe them — the S2/S3 skill gates, the S4 proofs module,
and the training README are in that commit. The axiom registry
entries, the constraint-hook fix, this plan, and the docs index row
remain uncommitted.

Validation outputs are recorded in the session closeout report and in
the goal record (`5e12d79e`), not restated here.

---

[^report]: Gödel-machine deep-research report (2026-09-18, session
artifact; research run `7bbdccd59bb5146a`): characteristics table,
literature synthesis, null-result implementation survey, and
gap-closing recommendations. The load-bearing content is restated in
§2 and §3; the raw ledger survives under the research server.

[^goedel]: Schmidhuber, J. (2003). *Goedel Machines: Self-Referential
Universal Problem Solvers Making Provably Optimal Self-Improvements.*
arXiv:cs/0309048. https://arxiv.org/abs/cs/0309048

[^steunebrink]: Steunebrink, B. R., & Schmidhuber, J. (2011). *A
Family of Gödel Machine Implementations.* AGI 2011, LNCS 6830,
pp. 275–280. https://link.springer.com/chapter/10.1007/978-3-642-22887-2_29

[^dgm]: Zhang, J., Hu, S., Lu, C., Lange, R., & Clune, J. (2025).
*Darwin Gödel Machine: Open-Ended Evolution of Self-Improving Agents.*
arXiv:2505.22954. https://arxiv.org/abs/2505.22954

[^kani]: Kani Rust Verifier. https://github.com/model-checking/kani