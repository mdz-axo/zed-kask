---
dcterms:title: "T0 Keystone — Belief-Hierarchy ↔ EventDependency Mapping Verification"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
pko:procedure-target: "Three-outcome gate: holds exactly / holds approximately / fails"
---

# T0 — Keystone Mapping Verification

**Gate question**: does `EventDependency.conditionals` (finite, parent-independent,
bitmap-indexed CPTs over *states of nature*) satisfy Bhattacharya's belief-hierarchy
recursion (infinite, interactive, over others' *strategies and beliefs*)?

**Verdict: HOLDS APPROXIMATELY — by made-precise analogy, with two explicit truncation
boundaries and one structural exclusion.** The foundation may proceed to T4/T8 under the
approximate license documented here. The exact-holds outcome was never available (the two
formalisms range over different spaces); the fails outcome is avoided because the
approximation is controllable and its error is measurable.

Source basis: arXiv:2211.03244 full text (ar5iv), Definitions 1–6, Propositions 1–6,
Theorems 1–3, extracted 2026-08-05. Platform basis:
`kask/mcp-servers/hkask-mcp-scenarios/src/types.rs` L225–284, L534–560;
`superforecast.rs` L64–262 (read 2026-08-05).

## 1. The two formalisms, stated precisely

### 1.1 Bhattacharya's hierarchy (the paper)

- Uncertainty decomposes into layer-0: `Y_i^0 = S × ∏_{j≠i} A_j` (states of nature ×
  others' strategies), and layer-k: `Y_i^k = Y_i^{k-1} × ∏_{j≠i} B_j^k` (others' k-th
  order beliefs), with `b_i^{k+1} ∈ Δ(Y_i^k)` (Eq. 14–15).
- **Coherence** (Eq. 16): `marg_{Y_i^{k-1}} b_i^{k+1} = b_i^k` — higher-order beliefs
  marginalize to lower-order ones.
- **Consistency**: coherence + common knowledge of coherence (Eq. 17).
- **Canonical homeomorphism** (Eq. 18): `B_i ≅ Δ(Y_i^0 × ∏_{j≠i} B_j)` — the infinite
  hierarchy collapses to a single measure over the universal space (Mertens–Zamir /
  Brandenburger–Dekel lineage). Finite ordinals suffice for induction.
- **W_i^k sets** (Eq. 21–26): inductive restriction — agent believes others don't use
  dominated (k−1)-th order responses, and believes others believe she doesn't.
  Undominated response sets are **nested**: `UD_i^k ⊆ UD_i^{k-1}` (Prop. 2–3).
- **Arbitrage theorems**: arbitrage ⇔ agent anticipated order k but actual responses
  come from order k+1+ (Thm 1 necessity, Thm 2 sufficiency); no-arbitrage ⇔ everyone
  exhausts the hierarchy or is "just high enough" (Thm 3); one-to-one aggregation ⇒
  arms race to infinity (Prop. 4); **tâtonnement steps weakly raise the operative
  order by α ≥ 0** (Prop. 6) — iterated one-step-ahead trading is outcome-equivalent
  to higher-order reasoning.
- **Symmetric payoff information is assumed throughout**: `b_i^0 = P` is common
  knowledge; the paper "switches off" updates about fundamentals (§1, §4.1).

### 1.2 The platform's event-tree algebra (the code)

- `ScenarioEvent.depends_on: Vec<EventDependency>`; each `EventDependency` has
  `parent_event_ids: Vec<String>` + `conditionals: Vec<f64>` of length 2^n_parents —
  a bitmap-ordered CPT: P(child | parent assignment a) for each of 2^n assignments
  (types.rs L278–284).
- `compute_marginal_probabilities`: P(E) = Σ_a P(E|a)·Π_i P(p_i)^{a_i}(1−P(p_i))^{1−a_i}
  in topological order — exact marginalization **under parent independence**
  (superforecast.rs L64–109).
- `EventTree`: nodes, root_ids, topo_order, joint_probability (product of
  all-parents-true conditionals), per-node variance_contribution (types.rs L534–560).
- Only `depends_on[0]` is consumed (superforecast.rs L86) — one effective dependency
  group per event.
- Events are propositions about **states of nature** (will X happen by date D), not
  about other agents' strategies or beliefs.

## 2. The mapping, component by component

| Paper construct | Platform construct | Mapping verdict |
|---|---|---|
| State space S with common-knowledge measure P | Root events with market-implied or base-rate probabilities | **Holds** — root priors play the role of b⁰ = P; the market-implied provenance (reliability tiers, refusal gates) is the platform's enforcement that root priors are shared, not private |
| Layer-0 uncertainty Y⁰ = S × ∏A_j | One level of CPT conditioning: child event given parent assignment | **Holds approximately** — see §3 |
| k-th order belief b^k ∈ Δ(Y^{k-1}) | Depth-k chain of CPT conditioning (event conditioned on event conditioned on …) | **Holds approximately, by truncation** — see §3 |
| Coherence (Eq. 16): marg b^{k+1} = b^k | `compute_marginal_probabilities` in topo order | **Holds exactly** — marginalization consistency is precisely what the CPT algebra computes; a child's marginal is coherent with its parents' marginals by construction |
| Consistency (common knowledge of coherence) | Shared `EventTree` artifact consumed by all downstream tools | **Holds structurally** — one tree, one joint; all consumers read the same object |
| Canonical homeomorphism (hierarchy = one measure on universal space) | `joint_probability` over the tree | **Holds in the finite case** — the tree's joint IS the single measure; the homeomorphism is trivially satisfied for finite trees and is the finite analog of Mertens–Zamir |
| W_i^k nesting (UD sets shrink as k rises) | Nothing | **Absent — structural exclusion, see §4** |
| Arbitrage = order-underestimation (Thm 1–2) | Nothing | **Absent — the tree has no arbitrage semantics; see §4** |
| Tâtonnement ⇒ order climb (Prop. 6) | Refresh/re-bridge cycle (T10) | **Holds by design adoption** — each refresh step is a deliberate one-step-ahead update; Prop. 6 licenses reading the sequence as an order climb |

## 3. Where the approximation lives: strategy-hierarchy vs state-hierarchy

The paper's recursion ranges over **others' strategies and beliefs**; the tree's
conditioning ranges over **states of nature**. These are different spaces, so exact
equivalence is not merely unproven but **ill-typed** — the exact-holds outcome was never
available. The approximate license rests on three observations:

1. **The paper's own symmetric-information assumption does the bridging work.**
   Because b⁰ = P is common knowledge and updates about fundamentals are switched off,
   all hierarchy content above layer 0 is *strategic*. A platform event tree makes the
   opposite modeling choice: it puts fundamentals (states of nature) in the tree and
   leaves the strategic layer implicit in *where the probabilities come from*
   (market prices = aggregated others'-beliefs, per arXiv:2205.08913). The two are
   complementary decompositions of the same total uncertainty Ω = S × ∏A_j × ∏B_j:
   the paper varies (∏A_j, ∏B_j) at fixed P; the tree varies S with (∏A_j, ∏B_j)
   frozen into the market-implied priors. **The tree is the paper's model with the
   roles reversed — not a special case of it.**

2. **Depth-k truncation is the operative approximation, and Prop. 6 bounds its cost.**
   A finite tree of depth d truncates the hierarchy at order d. The paper's Prop. 6
   says each tâtonnement round weakly raises the operative order; empirically,
   level-k literature (Stahl & Wilson 1994, Nagel 1995 — cited in the paper) finds
   deliberate reasoning rarely exceeds k ≈ 2–3. A tree of depth 2–3 therefore covers
   the *deliberate* hierarchy; deeper structure is reachable only through the
   tâtonnement dynamics (refresh cycles), which the platform implements as T10.
   **Truncation error**: the paper provides no metric on hierarchy depth (its
   hierarchy spaces carry the weak topology, not a norm), so a formal ε-bound is not
   derivable from the text — this is an honest limitation, recorded as such. The
   *practical* bound is: truncation at depth d loses exactly those arbitrage
   opportunities that require detecting order-(d+1)+ reasoning (Thm 1), and Prop. 4
   says the number of such opportunities falls as the aggregation mapping becomes
   less responsive — i.e., in thin/illiquid markets, which is precisely where the
   platform's reliability tiers already refuse to bridge.

3. **Coherence — the property that does the computational work — holds exactly.**
   Eq. 16 (marginal consistency) is the only recursion property the risk core (WS4)
   actually needs: scenario-weighted valuation and factor loadings consume marginals
   and joints, not W_i^k sets. The CPT algebra computes exactly coherent marginals
   under its parent-independence assumption. The approximation thus does not touch
   the downstream consumers.

## 4. The structural exclusion: what the tree cannot express

- **W_i^k nesting and the arbitrage theorems (Thm 1–3)** have no tree analog. The
  tree expresses P(events); it has no representation of "agent i anticipated order k
  but the market used order k+1." The foundation's *arbitrage-relevance* claim (H3)
  therefore cannot rest on the tree satisfying the recursion — it must rest on the
  pricing test (H3/T1) directly. This downgrades the theoretical license for H3 from
  "theorem-backed" to "theorem-inspired": the tree is a **belief-hierarchy-shaped
  object over states**, whose factor relevance is an empirical question, exactly as
  the falsification suite treats it.
- **Parent independence** (the algebra's maintained assumption) corresponds in the
  paper's terms to assuming others' strategies are independent conditional on the
  state — violated precisely when events share latent causes (the GAP-2
  multiply-connected case). The independence diagnostics in T4a are the detection
  mechanism; the paper offers no repair, so the repair is a platform decision
  (junction-tree vs sampling, GAP-2).

## 5. Consequences for the plan (license terms)

1. **Proceed to T4/T8** under the approximate license. No STOP condition triggered.
2. **H3's evidential status is unchanged** (open, tested by pricing test) but its
   *framing* is corrected: the tree is not a theorem-backed factor model; it is a
   coherent, finite, state-space projection of the belief-hierarchy structure whose
   pricing power is to be measured. Territory-map C5–C7 confidence updated below.
3. **Tree depth guidance**: composition (T4a) should target depth 2–3 deliberately;
   deeper trees buy no theoretical coverage (level-k evidence) and cost CPT
   combinatorics (variety amplifier iv already caps this).
4. **T10's tâtonnement journal gains its license**: Prop. 6 is the theorem that lets
   the refresh cycle be read as equilibrium discovery. This is the strongest exact
   result the foundation gets from the paper.
5. **New named risk**: if the platform ever bridges markets whose aggregation is
   near-one-to-one (very liquid, tight-spread contracts), Prop. 4's arms race applies
   and finite trees systematically under-represent the operative reasoning depth.
   The reliability-tier gate (which prefers liquid markets) pushes *toward* this
   regime — the mitigation is that Prop. 4's arbitrage is exactly what the platform
   does not trade on (it reads prices, it doesn't exploit order-flow).

## 6. Territory-map updates (write-back per T0 AC)

- C5: confidence 0.9 → **0.95** (full text now the basis, including proofs).
- C6: confidence 0.9 → **0.95**; amended: the state/strategy asymmetry is now the
  *basis* of the mapping verdict, not a caveat.
- C7 (Prop. 4 arms race): confidence 0.85 → **0.9**; scope note added (§5.5 above).
- F1 (scenario-graph APT-relevance): unchanged at 0.3 FRAGILE — T0 confirms this must
  be settled empirically (H3/T1), not by the recursion mapping.
- The parallel plan's "Inference/0.7 UNVERIFIED" entry (their 01-territory-map L70–75):
  **resolved → this document**. Verdict: holds approximately, §3; structural
  exclusion, §4.

## 7. Falsifiability log entry

- **Claim under test**: "the platform's CPT algebra satisfies Bhattacharya's
  belief-hierarchy recursion."
- **Outcome**: the claim as stated is *refuted in its exact form* (ill-typed: the
  recursion ranges over strategies/beliefs, the algebra over states) and *corroborated
  in its truncated, coherence-restricted form* (marginal coherence holds exactly;
  depth-2–3 truncation covers the deliberate hierarchy per level-k evidence; Prop. 6
  covers the rest dynamically).
- **What would change this verdict**: discovery that WS4 consumers need W_i^k-set
  semantics (not just marginals/joints) — none currently do; or empirical failure of
  depth-2–3 trees in the H-tests, which would force the depth question open again.
