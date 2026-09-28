---
title: "Deterministic vs Probabilistic Compute: Routing, Entropy, and the Problem Matrix"
audience: [researchers, architects, agents]
last_updated: 2026-09-27
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# Deterministic vs Probabilistic Compute: Routing, Entropy, and the Problem Matrix

> **IS/OUGHT boundary.** Sections 2–4 report established literature, corpus passages, and checks that were actually run. Sections 1, 3 (matrix placement), and 6 are the author's synthesis: hypotheses to test, not observed behavior of zed-kask. No routing mechanism described here is implemented.

**Claim labels.** **[Lit]** established literature · **[Corpus]** John Brooks corpus (`john-brooks-clean-sealed-v13-reference`, record_id given) · **[Synth]** this report's own synthesis.

**Prior work built on.** Curator thread `1b064ad1` (records `f944e2da`, `8e6277ee`) framed the LLM as an *entropy source* (it proposes candidates), deterministic checkers as *entropy sinks* (they collapse a candidate to verified or rejected), a propose→verify ratchet between them, a compilation ladder (implicit knowledge → learned function → candidate claim → verified fact or calibrated forecast), and the D/P line as the current *frontier of verification cost*. New in this report:

1. The compute axis restated as *how correctness is guaranteed*.
2. A demonstration that the compute and reasoning axes are independent.
3. The game axis split into a recurrence dimension and a world-closure dimension.
4. A second breakdown of the entropy analogy, sourced from Bennett and Shenker in the corpus.

**Ontology anchors.** At first probe (`onto_anchor`, 2026-09-27) `entropy`, `deterministic computation`, `Bayesian inference`, `finite game`, and `repeated game` all resolved only to the coarse 5W1H core rung. Since then the operator has ruled the vocabulary into the derived registry: the five routing terms (`entropy`, `deterministic_computation`, `probabilistic_computation`, `verification_oracle`, `calibrated_forecast`) resolve on the derived rung (verified live 2026-09-27), and the three game-axis terms (`bayesian_inference`, `finite_game`, `repeated_game`) resolve on the derived rung as well (verified live 2026-09-28, after the editor rebuild). Published definitions remain the grounding: Shannon 1948; Gibbs, and Jaynes 1957; Bayes–Laplace; Carse, *Finite and Infinite Games* (1986); the repeated game as in Fudenberg & Tirole, *Game Theory* (1991).

## 1. Routing claim

**Claim [Synth, built on Lit].** Use deterministic compute when the specification fixes the answer, i.e. the conditional entropy H(answer | spec) = 0, and writing the procedure down is affordable. Otherwise use probabilistic compute to *propose* answers, provided a cheap deterministic check can *decide* them. Where neither holds, the right output is a calibrated distribution, not a sampled answer.

- **Mechanism.** Three costs decide the route: writing the function, verifying a candidate, and sampling one. LLMs amortize inference over knowledge no one ever wrote down as a function (Gershman & Goodman 2014 [Lit]). Checking a candidate is usually far cheaper than searching for it. Each step therefore goes to its cheapest adequate guarantee.
- **The temperature-0 objection.** At temperature 0 an LLM is a deterministic function, apart from floating-point nondeterminism in batched inference. What stays "probabilistic" is that its *correctness* is only estimated from training data, not guaranteed by construction. The operative axis is therefore **correct by construction vs correct in expectation**, not whether the sampler draws random numbers.
- **Falsifiers.**
  1. A task class with H = 0 and a known, cheap deterministic procedure (exact arithmetic, parsing a fixed grammar) where an LLM pipeline is more accurate at equal cost.
  2. A domain whose knowledge is implicit and has no cheap verifier, where hand-written rules match LLM accuracy at lower total cost across many such domains. One narrow domain does not count.
  3. Aleatory-uncertainty forecasts where LLM point outputs beat deterministic base-rate models on Brier score. This would undercut the calibrated-distribution branch.

## 2. The entropy analogy, tested

**Correspondence [Lit].** Shannon H = −Σ p log₂ p and Gibbs S = −k_B Σ p ln p are the same function up to a constant: S = k_B ln 2 · H.

**Numeric check (run, `lisp_eval`, 2026-09-27; ln computed by an atanh series because the tool has no `log` builtin):**

| Quantity | Result |
|---|---|
| H for p = (½, ¼, ¼) | 1.5 bits |
| S / (k_B ln 2) for the same p | 1.5 (identical) |
| H for a uniform 4-way choice | 2 bits |
| H for a certain outcome | 0 |
| Landauer bound k_B T ln 2 at 300 K | ≈ 2.87 × 10⁻²¹ J per bit |

**Why it is more than a resemblance of form [Lit]:**

- **Jaynes (1957).** Statistical mechanics can be read as maximum-entropy inference. The Gibbs distribution is the maximum-H distribution under an energy constraint, so thermodynamic entropy is a special case of inferential entropy.
- **Landauer (1961) and Bennett.** Every logically irreversible step must export at least k_B T ln 2 of heat per bit erased [Corpus `5b6efe78`].

**Where it breaks for the compute-choice question:**

1. **Thermodynamics does not set the price [Lit + Synth].** Real hardware dissipates orders of magnitude above the Landauer bound per operation. Any deterministic computation can be made logically reversible if it keeps a copy of its input [Corpus `642e4659`, `535bf593`]. The D/P cost is economic (writing and verifying functions), not thermodynamic.
2. **The relevant entropy belongs to the observer, not the machine [Corpus `539d723b`, `0d08274e`].** Data in a deterministic run is fixed by the initial state and has zero entropy. The uncertainty that drives routing is the observer's uncertainty about the correct answer. An LLM's sampling "randomness" is itself produced by a deterministic pseudorandom generator.
3. **Model entropy is not correctness [Synth].** A confidently wrong model has low output entropy and a high error rate. Low predictive entropy does not earn deterministic-level trust; this is the calibration gap.
4. **Open worlds have no defined H [Synth].** A Carse infinite game has no fixed outcome space, so there is nothing to take p log p over.

## 3. The matrix, tested

The `falsifiability` method was applied inline as a judgment pass (P). Its templates were not rendered and no external observations were applied, so the findings below are corroborated survivors, not confirmations.

### 3.1 Compute axis × reasoning axis: independent

All four combinations have real instances, so the axes are not collinear:

| | Deduction | Bayesian updating |
|---|---|---|
| **Correct by construction** | Compiler, SAT solver, Lean kernel | Seeded MCMC, Kalman filter (deterministic code performing probabilistic inference) |
| **Correct in expectation** | Miller–Rabin primality test (error ≤ 4⁻ᵏ); LLM proposing a Lean proof (the combined system becomes correct by construction once the kernel accepts) | LLM answering from implicit knowledge |

### 3.2 The game axis splits

The two senses of "open-ended" separate. Each occurs without the other:

- **Repeated but not infinite (Carse):** the finitely repeated Prisoner's Dilemma with a known horizon, played to win. Backward induction deduces defection in every round [Lit].
- **Infinite but not repeated:** science, a natural language, a market. The stage game itself changes.

Axis 3 is therefore restructured into:

- **3a. Recurrence:** one-shot, or a fixed stage game repeated.
- **3b. World closure:** closed means fixed rules and a fixed hypothesis space. Open means the rules and players change (Carse).

### 3.3 Cells

| Kind of game | Example | Kind of knowledge | Method that fits |
|---|---|---|---|
| One-shot, closed, tractable | Type-check a program | Explicit, H = 0 | Correct by construction + deduction |
| One-shot, closed, intractable | Best move in a chess position | Explicit rules, search space too large to exhaust | Probabilistic proposal (MCTS or a policy network) + deterministic rule check. Suggests **tractability** as a hidden axis. |
| One-shot, closed, aleatory | Rain in Philadelphia one year out [Corpus `b4947fb5`] | Irreducible uncertainty | Calibrated distribution. Base rates are deterministically computable; no LLM needed. |
| Repeated, known horizon | Finitely repeated Prisoner's Dilemma | Complete | Deduction (backward induction) |
| Repeated, unknown horizon | Axelrod tournament [Corpus `7aba9181`, `1c659b29`] | Opponent type, learned from history | Bayesian updating. The winning strategy (tit-for-tat) is a two-line deterministic program: adaptivity lives in the strategy, not in the compute. |
| Open world (Carse) | Research program, product strategy | Unknown unknowns; the hypothesis space shifts | Probabilistic generation of new hypotheses + falsification to prune + human judgment as the final check |
| Open world × correct by construction + deduction | — | **Empty for the game as a whole.** There are no fixed axioms. Sub-games frozen into closed games can still be deduced. | — |

**Finding [Synth].** The dividing line is less "deterministic vs probabilistic" than **whether the hypothesis space is closed** and **whether a cheap verifier exists**. Bayesian updating needs a fixed hypothesis space and deduction needs fixed axioms, so both are closed-world methods. Open-world play needs a generator (the LLM's comparative advantage) paired with a pruner. The chaos passage [Corpus `49f36d8c`] points the same way: a deterministic system can still require probabilistic prediction, so compute type and knowledge type are distinct.

## 4. Corpus perspectives

Eight `curator_federated_search` queries were run, which reached the cap.

| record_id | Source | Text (verbatim) | Informs |
|---|---|---|---|
| `642e4659` | Bennett, *Notes on Landauer's principle* | "it is possible to reprogram any deterministic computation as a sequence of logically reversible steps, provided the computation is allowed to save a copy of its input." | §2 breakdown 1 |
| `539d723b` | Bennett | "the data in the course of the usual deterministic digital computation is not random, but on the contrary determined by the computer's initial state." | §2 breakdown 2 |
| `0d08274e` | Bennett, reporting Shenker | "Shenker (2000) argues (Fig. 3) that the 1:1 sequence of states actually visited in the course of a deterministic computation means that deterministic computers are not bound by Landauer's principle." | §2; a contested position that Bennett argues against |
| `fcf70395` | Fabozzi, *Mathematics of Financial Modeling* | "Information with a minus sign is well known in statistical physics as entropy, which is a measure of disorder: E = –I." | §2; shows how loose the cross-field sign conventions are |
| `b4947fb5` | Tetlock, *Superforecasting* | "Aleatory uncertainty is something you not only don't know; it is unknowable." | §1 third branch; §3.3 aleatory row |
| `7aba9181` | Kauffman, *At Home in the Universe* | "if you and I play a repeated game and we do not know how many times we will play the same game, then cooperation does tend to emerge. For a single game, the rational strategy is defect–defect" | §3.2 axis split |
| `1c659b29` | Kauffman | "Among the best "strategies" to emerge is "tit for tat." Here each player cooperates unless the other defects." | §3.3; a deterministic program wins a repeated game |
| `49f36d8c` | Beaubien, *Great Mental Models* Vol. 3 | "for chaotic systems. Without perfect accuracy, we can't make useful, comprehensive predictions about them. It's often only possible to make probability-based predictions" | §1, §3.3; compute type ≠ knowledge type |

**Gap.** The corpus has no substantive passage on Carse's infinite game. `1eb5aa6e` (*Playing Software*) uses the term loosely and is not counted.

## 5. Skill map

Fits come from the `skill-discovery` route phase. The dimension scores are judgments (P); the composites were recomputed in `lisp_eval` (D). The role of the deleted `capabilities-reasoner` is covered by `metacognition` together with `pragmatic-semantics`.

| Step | Skill | Fit |
|---|---|---|
| Framing | `hypothesis-framer` (+ `pragmatic-semantics`) | 0.78 / 0.65 |
| Generating hypotheses | `falsifiability` | 0.85 (full) |
| Testing the axes | `falsifiability` (+ `essentialist`) | 0.83 / 0.48 |
| Formal checks | `lean-prover` (+ `grounding-verify`) | 0.65 / 0.58 |
| Probabilistic estimates | `superforecasting` | 0.80 (full) |
| Capability limits | `metacognition` + `pragmatic-semantics` | 0.58 / 0.65 |
| Synthesis | `grounding-verify` (+ `skill-bundler`) | 0.65 / 0.58 |

**Uncovered:** no installed skill is dedicated to physics-level formal checks. The nearest is `lean-prover`, which is only a partial fit.

## 6. Open questions, ranked

1. **Does model output entropy track actual error rate on zed-kask's real step types?** The answer decides whether predictive entropy can serve as a routing signal or whether every P step needs an external verifier.
2. **In open-world problems, what decides which generated hypotheses get pruned?** The answer decides whether generation plus falsification suffices or human judgment is the check that cannot be removed. That sets how much routing can be automated.
3. **Is tractability a fourth axis?** The chess cell suggests closure alone is insufficient. The answer decides whether the matrix is 2 × 2 × 3 or has another dimension.

## 7. Check ledger

| Check | Status |
|---|---|
| Entropy formulas on a toy distribution | Run (`lisp_eval`), passed |
| Landauer bound at 300 K | Run (`lisp_eval`) |
| Skill fit composites | Run (`lisp_eval`) |
| Axis independence; routing-claim falsifiability | Run inline as judgment (P); `falsifiability` templates not rendered; no external observations |
| Lean proof of S = k_B ln 2 · H | Not run |
| Ontology anchors | Run; all five coarse (5W1H core) |
