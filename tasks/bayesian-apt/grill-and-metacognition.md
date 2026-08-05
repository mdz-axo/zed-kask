---
dcterms:title: "Grill-Me Gap Analysis (Risk Core) + Metacognitive Close-Out"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Grill-Me — Risk Calculation Core + Bayesian Event-Tree Probabilities

Self-interrogation at escalating levels on the hardest sub-problem. Answers grounded in
the extracted sources; gaps rated honestly.

## Level 1 — Recall
**Q: What probability representation do scenario trees use today?**
A: Bitmap-ordered CPTs per `EventDependency` (length 2^n_parents), marginalization under
parent-independence via `hkask_forecast::marginalize`; only `depends_on[0]` effective (C27, C28).
**Q: What warrants reading market prices as probabilities?**
A: AMM convergence results (C16) + informational-substitutes conditions (C17); reliability
tiers and calibration feedback as the empirical guard (C37). **Solid.**

## Level 2 — Mechanism
**Q: How does a tree-level update propagate?**
A: Not implemented (C30). Required: message-passing/exact inference over the CPT network
(Koller & Friedman machinery), with the joint recomputed over root-to-leaf paths. The
single-group limitation must be lifted first (T3→T5 ordering). **Solid on the what; the
how is unbuilt.**
**Q: How do scenario nodes become APT factors?**
A: Loadings = sensitivity of company cash flows to node outcomes; pricing relation tested
cross-sectionally per sr216's static-portfolio warrant. **Partial** — the sensitivity
elicitation procedure (how a cash-flow line responds to a discrete event) is the least
specified step in the whole plan. **GAP-1.**

## Level 3 — Rationale
**Q: Why CPTs and not a full Bayesian network library?**
A: The existing representation is already a BN in embryo; the deletion test says extend it,
don't import a dependency. But: parent-independence marginalization is exact only for
tree-structured (singly-connected) graphs — if composition produces multiply-connected
graphs (market events sharing latent causes), exact inference cost jumps. **GAP-2: no
connectedness diagnostic exists in the plan.** Added to T4a as "independence diagnostics"
but the escalation path (junction tree vs sampling) is undecided.
**Q: Why is the dynamic layer outside APT's warrant?**
A: sr216 C2: no-arbitrage preclusion covers static portfolios; probability-updating trees
are dynamic. The plan labels the tâtonnement layer as Bhattacharya-justified (Prop. 6),
not APT-justified. **Solid.**

## Level 4 — Edge cases
**Q: What happens when a bridged market resolves while its event sits mid-tree?**
A: Unhandled today. Resolution must collapse the node (probability → 0/1) and propagate —
a special case of T5, but resolution *timing* vs contract settlement disputes (UMA status
in `MarketRecord`) means "resolved" is itself probabilistic. **GAP-3: resolution-uncertainty
nodes are not modeled.**
**Q: Near-deadline, near-coinflip contracts (structural vol flags)?**
A: Their prices are variance-dominated; using them as node priors near deadline injects
noise. Mitigation exists as flags (C37) but no gate policy consumes them at tree-time. **GAP-4.**
**Q: Cross-venue duplicates of the same event?**
A: C18 says 2–4% persistent deviations; naive merging fabricates arbitrage. Plan scopes
single-venue (T9/H1d) but the venue-choice open question (plan OQ-1) is unresolved. **Partial.**

## Level 5 — Synthesis
**Q: Could the whole risk core be replaced by regressing returns on raw contract prices?**
A: That is exactly H3's counterfactual (do(no scenario graph)). If raw factors price as
well, the composition algebra fails the deletion test and T8 is cut. The risk core earns
its complexity only if tree structure (conditionality, challenge gates, provenance) adds
pricing or calibration power over raw prices. **This is the plan's load-bearing admission.**

## Gap register (prioritized)
1. **GAP-1**: cash-flow-sensitivity elicitation procedure for factor loadings — highest
   leverage; blocks T8a. Study: Damodaran scenario-DCF chapters + `decompose_gap` machinery.
2. **GAP-2**: multiply-connected tree inference escalation path — decide junction-tree vs
   likelihood-weighted sampling before T4a completes.
3. **GAP-3**: resolution-uncertainty modeling (probabilistic "resolved" state).
4. **GAP-4**: tree-time gate policy consuming volatility structural flags.

---

# Metacognitive Close-Out (Kata)

**Grasp current condition.** Grounded claims: 44 classified claims (territory map), all with
provenance; 6/6 required resources extracted or fallback-documented (sr216 PDF binary →
abstract + Palgrave citation; Emerald → Crossref abstract; Morris → global-games corpus);
4 MCP servers read at source level. Obstacles: GAP-1..4 above; F1–F3 fragile claims;
OQ-1..4 open questions.

**Target condition.** A plan where every workstream task is verifiable, every hypothesis has
a discriminating test, and the highest-risk claim (H3) has a kill gate before major spend.

**Prediction** (Brier-tracked): I predicted with confidence 0.75 that the MCP source read
would reveal the composition algebra as the largest gap. **Outcome: correct** (S4 is the
keystone of the DAG; only one code edge exists). Brier = (0.75−1)² = 0.0625.
Secondary prediction, confidence 0.6: equity-duration literature would not be on arXiv.
**Outcome: correct** (negative result, C23). Brier = (0.6−1)² = 0.16.
Mean Brier = 0.111 — calibrated on the confident-and-right side; the 0.6 prediction was
under-confident.

**Gap to target**: small. The plan meets its own acceptance criteria; residual gap is
GAP-1 (sensitivity elicitation), which no amount of further planning closes — it requires
the T8a prototype.

**Single highest-leverage next experiment**: **T8a prototype** — hand-build one scenario
tree for one liquid-contract-covered company (e.g. a Fed-decision-linked bank or a
tariff-exposed manufacturer), elicit cash-flow sensitivities via the existing
`decompose_gap` machinery, and run the H3/T1 pricing comparison against FF5. It
simultaneously: tests the most refutable hypothesis (H3), exercises the whole vertical
slice (T2→T4→T5→T7), and resolves GAP-1 by forcing the sensitivity-elicitation procedure
into existence. If H3 dies, ~40% of the plan's build cost is avoided before it is spent.

**Convergence**: gap < ε on plan quality per the task-breakdown quality gate (0.075 ≤ 0.15,
no criterion > 0.30). Loop closed.
