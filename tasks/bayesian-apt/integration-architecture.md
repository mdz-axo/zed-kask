---
dcterms:title: "Integration Architecture Proposal + Cybernetic Loop Analysis"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# Integration Architecture Proposal

## Cybernetic analysis of the target loop (pragmatic-cybernetics)

The end-state framing — *the economy discovering equilibrium between risk and return factors* —
is a feedback loop claim. Analyze before building.

**Loop L1 (equilibrium discovery)**: market prices → scenario probabilities → company
forecasts → capital allocation → (back into) market prices.

| Property | Assessment | Evidence |
|---|---|---|
| Polarity | **negative (corrective)** if forecasts move allocation against mispricing; but C12 (prices as endogenous public signals) can flip it positive (self-fulfilling) | Angeletos & Werning 2006 |
| Delay | **long and heterogeneous**: contract horizons days–months; equity cash flows years; the duration axis exists precisely to manage this delay structure | H2 |
| Gain | unknown; leverage-cycle models (C15) show gain regimes where the loop goes cyclic/chaotic | arXiv:1507.04136 |
| Closure | **broken today**: no path from company forecasts back to any market-facing action; loop closes only through the human analyst | C43 |
| Fidelity | degraded at every paste bridge (JSON strings, frozen base rates, no citation pinning) | C43, C42 |

**Loop L2 (calibration)**: predictions → resolutions → Brier scores → reliability tiers →
gate decisions. Polarity negative, closure **present** (CalibrationStore + demotion), delay
= resolution lag, fidelity good. This is the platform's healthiest loop — build on it.

**Variety check (Ashby)**: disturbance classes the foundation must absorb — (i) venue price
fragmentation (C18), (ii) stale bridged anchors (C43), (iii) fabricated-probability risk
(refusal gates handle), (iv) tree-combinatorics explosion (2^n CPT growth), (v) model-risk
from duration estimation sensitivity (F2). Current regulator variety: refusal gates (iii only).
**Deficit: 4 classes unregulated.** Required amplifiers: refresh/re-bridge policy (ii),
CPT-size caps + independence diagnostics (iv), duration sensitivity reporting (v),
single-venue scoping or venue-adjustment (i).

**Good Regulator check (Conant–Ashby)**: the platform's model of the market is the
`MarketRecord` annotation stack — good fidelity per record, but the *tree* is the platform's
model of the event system, and it currently cannot update itself (C30). S3 (tree propagation)
is the Good-Regulator fix.

**Tâtonnement framing verdict**: the equilibrium-discovery framing is **valid as metaphor,
not yet as mechanism**. What the platform can actually implement is Bhattacharya Prop. 6's
iterated one-step-ahead updating (C5): each refresh cycle (re-bridge → re-propagate →
re-weight → re-forecast) is a tâtonnement step. The four Walrasian critiques (C9) apply and
must be answered in design: (i) the auctioneer is the refresh policy; (ii) behavioral
microfoundation is the calibration loop L2; (iii) strategic manipulation is out of scope
(platform reads markets, doesn't move them — but see C12); (iv) decentralization is
inherited from the venues.

## Architecture options (MCDA)

**Decision**: how should the four servers compose for the target analyses?

**Alternatives**:
- **A. Typed bridges in a shared crate** (`hkask-forecast` extension): composition algebra,
  propagation, duration, and factor mapping live in the shared math crate; servers stay
  data/services. Strangler-fig compatible.
- **B. New orchestrator MCP server** (`hkask-mcp-pricing`): a fifth server owning the
  foundation, calling the other four over MCP.
- **C. Skill-layer composition**: a `bayesian-apt` skill whose FlowDef orchestrates existing
  tools via paste bridging, no server changes.

**Criteria** (weights, direct): type safety/provenance fidelity 0.25; build cost 0.20;
governance fit (OCAP/gas at MCP boundary) 0.15; latency/round-trips 0.10; testability 0.15;
evolvability (strangler-fig) 0.15.

**Scores** (0–10):
| Criterion | A | B | C |
|---|---|---|---|
| Type safety/provenance | 9 | 6 | 3 |
| Build cost (10=cheap) | 6 | 4 | 9 |
| Governance fit | 7 | 9 | 6 |
| Latency | 9 | 5 | 7 |
| Testability | 9 | 7 | 5 |
| Evolvability | 8 | 6 | 7 |
| **Composite** | **8.15** | **6.05** | **5.75** |

**Compensation-masking check**: A's weakest criterion is build cost (6, non-critical,
weight 0.20 > 0.1 → watch but not a veto). C scores 3 on the highest-weight criterion —
masked by its cheapness; flagged as major compensation risk if chosen.

**Sensitivity (OAT ±10%)**: A remains top under all single-weight perturbations; the nearest
reversal is governance weight +10% AND type-safety −10% jointly (correlated pair not flagged,
both <0.7 correlation) — **robust**.

**Recommendation**: **A now, C as the interim workflow** (C is what the plan's early
experiments use before A lands), B rejected (premature server; violates trait-with-one-impl /
deep-module discipline until a second consumer materializes).

## Required new surfaces (ranked by MCDA order of dependency)

1. **`hkask-forecast`: tree propagation + composition algebra** (S3, S4) — the math core.
2. **`MarketRecord.time_to_maturity` + ladder endpoint** (P2) — one field + one tool.
3. **Citation store in research server** (R2/R3): blake3-pinned content, stable citation IDs,
   claim-level spans; consumed by scenarios (challenge gates) and companies (assumption links).
4. **Tree-weighted valuation path in companies** (C2): replace independence-assuming 2x2
   weights with tree joint probabilities.
5. **Equity duration tool in companies** (C3): D_e from existing DCF outputs.
6. **Factor-exposure mapping** (S7/C4): scenario-node loadings + pricing-test harness.
7. **Refresh policy** (closes stale-anchor loop): re-bridge on schedule or on price-move
   trigger; every refresh is a tâtonnement step, journaled for the equilibrium analysis.
