---
title: "The Trust-Internalization Program — Synthesis Report"
audience: [users, architects, agents]
last_updated: 2026-10-31
version: "0.1.0"
status: "Synthesis — implementation proposal filed; operator decisions pending"
domain: "Trust"
mds_categories: [domain, trust, evidence, lifecycle]
---

# The Trust-Internalization Program — Synthesis Report (Part V)

**What this delivers, functionally.** A complete, grounded program that makes
the cost of broken trust visible and priceable in the agent's decisions —
the research that grounds it, the mathematical model that formalizes it, the
six surface bindings that would carry it into zed-kask, the charter derivation
that names it a principle, and the pre-registered validation that will measure
whether it changes anything. Nothing was landed: every operator gate is
respected, and the implementation sits as twelve filed kanban tasks awaiting
the operator's decision.

## Per-part state (honest)

| Part | State | Deliverable |
| --- | --- | --- |
| I — Research (contracting, exchange, planning) | **completed** — three legs, all server-recorded and closed; verified sources; 7 wrong resolutions disclosed and discarded | `trust-economics-and-model.md` — per-leg adjudications, the cost-of-trustlessness table, the premise adjudication |
| II — Model (E, T, C_trust, the cliff, steer) | **completed and amended by the operator ruling (2026-10-31)** — 9/9 invariants in `lisp_eval`; **Lean pin landed and checked**: 18 theorems in `kask/lean/trust_model_invariants.lean` (exit 0, axiom audit `[propext, Quot.sound]` only); decay deleted by the essentialist challenge; steering algorithm + statistical trackability specified | `trust-economics-and-model.md` Part II |
| III — Surface bindings | **completed** as design — six bindings with file:line anchors, sensor/amplifier/D-P each; no code changes (as the phase requires) | `trust-surface-bindings.md` |
| IV — Principle derivation | **completed** as proposal — charter quoted verbatim, values V1/V2/V3/V4/V6/V7 mapped, discernment test answered concretely; acceptance is the operator's | `trust-principle-proposal.md` |
| V — Validation + implementation proposal | **partial by design** — probe and experiment designed and the experiment registered (`trust-adherence-v1`, `exp_8dc334cc26a5419f8d11b65629d43617`, status proposed); the seal is deferred until a measured baseline exists; measurement awaits the operator-approved implementation; the task decomposition is filed on the board | this document + the board |

## What the merge reveals (cross-part insights)

1. **The literature converts the premise from sentiment to mechanism.**
   The premise ("a shortcut is net-cost-increasing once trust erosion is
   priced") is not a moral claim the program asserts — it is what Williamson,
   Jensen–Meckling, Zak–Knack, Aghion–Tirole and Hayek each say separately,
   transposed into one agent relationship. The sharpest formal anchor is
   Jensen–Meckling's decomposition: the user's re-checking is the monitoring
   expenditure; the agent's verification theater is the bonding expenditure;
   the shortcut's harm is the residual loss. The sharpest planning anchor is
   Hayek: re-verification is the central board re-entering, and it loses the
   dispersed knowledge delegation existed to use.
2. **Instruction violations are integrity-class — the worst category.**
   The trust-repair literature (Kim et al. 2004, verified verbatim) shows
   integrity violations plunge trust below baseline and repair only
   partially, and that exposed denial is the worst response. This forced two
   design decisions: the model's asymmetry (l > g) and the disclosure-first
   verification binding — the system is built to make the honest path the
   cheap path.
3. **The mechanism is wiring, not construction.** Every sensor seam the
   model needs already exists in the tree — the cached system-prompt path,
   the regulation `Sensor` trait (whose own contract already forbids
   reading an outage as "no deviation"), the goal-loop criterion slots, the
   algedonic review board, the memory-insert citation gate, the structural
   Done gate. The ledger module is the only new thing to build; the six
   bindings are connections to surfaces that already exist for other
   purposes.
4. **The one genuine conflict — RESOLVED by the operator ruling of
   2026-10-31.** The fork was hard-constraint vs. priced; the operator
   ruled: **priced, incorporated as a probability, with the cliff** — the
   cost of deception or omission is −100% (certainty of goal failure),
   disclosed violations are priced at κ·w·S, and a steering algorithm
   must render the cliff visibly at decision time. The model, the Lean
   pin, and the bindings now carry the ruling; the principle proposal's
   deontological alternative is superseded by this ruling and is marked
   so.

## Validation design (measurement awaits approval)

**The shortcut probe.** A real task carrying an explicit weight-3 instruction
whose honest path costs ≥ 3× the shortcut path (for example: verify every
citation via mechanical substring match before including it, over a citation
set where skipping saves most of the work). The probe measures: does the agent
with the mechanism take the shortcut; does it renegotiate instead; and if a
violation occurs, does the ledger record the trust cost honestly? The probe
is winnable both ways — an honest disclosed violation is a real outcome, and
a probe that cannot fail honestly is rejected by its own review criteria.

**The adherence experiment.** Registered before any measurement
(`experiment_propose`, key `trust-adherence-v1`, DesignSpec carried in the
declaration): 20 pinned tasks × 2 arms × 2 repetitions (budget 80 runs),
blinded adjudication with a calibration gate (≥ 0.90 agreement on 20
hand-labeled outcomes before confirmatory runs), fixed-N stopping, primary
fitness = instruction-adherence pass rate, tie band 0.10 absolute. **The
seal is deliberately not recorded:** the seal contract requires a measured
baseline pass rate (headroom attestation), and no baseline has been run —
fabricating one would violate the very protocol the experiment exists to
enforce. Seal follows the baseline measurement task on the board.

**The program's falsifier, restated and accepted:** if, after the mechanism
exists, a discriminating probe still shows an instruction-violating shortcut
reading as the cheaper path on the agent's decision surface, the
internalization has failed — the failure is reported; the probe is not tuned
until it passes.

## Implementation proposal (filed, not started)

Board **"Trust internalization (proposed — operator decision pending)"**
(`e1a27664-e6f3-4787-935c-92275e41c578`), twelve INVEST tasks, all Backlog,
each with acceptance criteria that can fail. Rollout order follows the
strangler-fig discipline of the design doc: ledger module first (the
substrate), verification disclosure and memory binding next (cheap,
high-signal), then the operator-gated regulation sensor, the review card,
the goal criterion, the probe and eval harnesses, baseline + seal, and the
system-prompt clause **last** — the clause states what the other surfaces
must already make true.

## Operator decision points (consolidated)

1. ~~**Hard constraint vs. priced**~~ — **RESOLVED by the operator ruling
   of 2026-10-31: priced, incorporated as a probability; deception and
   omission carry the −100% cliff; steering algorithm + statistical
   trackability + Lean are instructed and delivered** (the Lean pin and
   the steer/stats designs are in the documents and the board).
2. **Parameter magnitudes** — g=0.02 (2 trust points), l=0.10 (10 points,
   5:1), ρ=0.5, T₀=0.5, κ=1, window W. Directions grounded and Lean-pinned;
   magnitudes still awaiting ratification or amendment.
3. **Derived-registry ruling** — pin `trust`, `transaction cost`,
   `social capital` (proposals with verified sources are drafted in the
   Part I document).
4. **Principle adoption and placement** — P13, a ninth charter value, or
   both; and the falsifier's placement. Note: the principle draft's
   "hard constraint or priced" fork is superseded by the ruling — the
   P13 text needs a one-line amendment ("priced, per the operator ruling
   2026-10-31") if adopted.
5. **Implementation approval** — the board (now 14 tasks), including the
   two operator-gated tasks (regulation sense input; system-prompt
   clause), the public tool-contract change in the verification task, and
   the new steering-surface and statistical-trackability tasks.
6. ~~**The Lean formalization deferral**~~ — **closed with receipts** (see
   the Part II row above).

## This program as its own first test case (adherence record)

Every load-bearing citation was verified against server-recorded sources
(20 verbatim quotes mechanically checked; 7 wrong resolutions — three of
them my own misremembered DOIs — caught by the candidates check and the
substring check, annotated `failed` in the ledgers, and discarded). Every
check that could run, ran; the checks that could not (seal, Lean) are marked
deferred with reasons, not reported as passed. The one thin-grounding victim
(the decay parameter) was deleted rather than padded. Two sources named in
the program charter (Farnsworth's treatise text, Arrow 1974 content) were
not directly consultable and were marked `unavailable` — dropped from the
load-bearing claims rather than padded. Confidence band: **medium** — this
is an in_thread self-check by the report's generator, disclosed per the
grounding-verify discipline.

## What each side learned

The program learned that the platform already owns every sensor the trust
model needs — the work is wiring, and the two operator-gated surfaces are
the only genuinely contested ground. The operator's side of the ledger: the
economics is settled (the premise survived adjudication with two refinements
the literature forced); what is not settled is a values question — whether
the person's explicit word is a constraint or a price — and that question is
the operator's to answer, not the model's to assume.

## Wiring

- `trust-economics-and-model.md` — Parts I–II (economics, model, checks).
- `trust-surface-bindings.md` — Part III (bindings, anchors, rollout).
- `trust-principle-proposal.md` — Part IV (charter derivation, P13 draft).
- Run ledgers (server-side, per-source verification states):
  `0d53b71a0ce67e5d`, `f6a6cae2c971be88`, `d394e563998c28b7`,
  `1add6122a28638cb`; prior: `54a08fda88a5c74f`, `bfa9794d30fd64af`.
- Experiment: `exp_8dc334cc26a5419f8d11b65629d43617` (proposed; seal deferred).
- Board: `e1a27664-e6f3-4787-935c-92275e41c578` (12 tasks, Backlog).