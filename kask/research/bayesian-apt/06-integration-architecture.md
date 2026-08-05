# Integration Architecture Proposal

**How the four MCPs compose to enable the target analyses; new
surfaces/capabilities required, ranked via MCDA.**

---

## 1. Current composition (one-way)

```
prediction_markets ──┐
                     ├──> scenarios ──> (event tree, probabilities)
companies ───────────┘         │
                               └──> (no reverse bridge to company risk/return)

research ──> (grounding, but not citation-gated into scenarios)
```

The bridge is one-directional: markets and companies *feed* scenarios, but
scenarios do not *adjust* company forecasts. The research server is
unstructured relative to scenarios.

---

## 2. Target composition (bidirectional, citation-gated)

```
                ┌─────────────────────────────────────────────────────┐
                │                                                     │
                ▼                                                     │
prediction_markets ──> scenario_compose ──> EventTree ──> risk_core ──┤
        │                 │                  │              │          │
        │                 │                  │              ▼          │
        │                 │                  │      scenario_factor_   │
        │                 │                  │      loadings           │
        │                 │                  │              │          │
        │                 │                  │              ▼          │
        │                 │                  │      apply_scenario_    │
        │                 │                  │      tree(company)      │
        │                 │                  │              │          │
        │                 │                  │              ▼          │
        │                 │                  │      ForecastEnvelope ──┘
        │                 │                  │      (expected return,
        │                 │                  │       σ_scenario,
        │                 │                  │       factor loadings)
        │                 │                  │
        ▼                 ▼                  ▼
   duration          EventTree          branch_return
   (per market)      (per-branch        (DCF/RIM re-eval
                     joint prob)        per branch)
        │                 │                  │
        └────────────────> match_durations <──┘
                                │
                                ▼
                        DurationGap (maturity
                        transformation operator)

research ──> citation_gate ──> ScenarioEvent.basis (enforced)
```

**Key new flows:**
1. `scenario_compose` (N markets → tree) — the composition algebra.
2. `risk_core` (tree + company → σ_scenario + factor loadings) — the risk
   calculation core.
3. `apply_scenario_tree` (reverse bridge: tree → company forecast envelope).
4. `match_durations` (equity duration vs. market duration → gap).
5. `citation_gate` (research → enforced `basis` on every event).

---

## 3. New surfaces/capabilities required

| ID | Surface | Server | Type | Leverage | Effort | Risk | Falsifiability |
|---|---|---|---|---|---|---|---|
| S1 | `duration` on `MarketRecord` | prediction-markets | Field + formula | High | Low | Low | High (enables H2) |
| S2 | Continuous `duration` on valuation | companies | Expose existing | High | Low | Low | High (enables H2) |
| S3 | `match_durations` | shared crate | Pure function | High | Low | Low | High |
| S4 | `scenario_from_markets_set` | scenarios | New tool | High | Medium | Medium | High (enables H3) |
| S5 | Per-branch joint probability | scenarios | Extend `EventTree` | High | Medium | Low | High |
| S6 | `branch_return` | companies | New function | High | Medium | Medium | High (enables H1) |
| S7 | `scenario_risk_measure` | scenarios/companies | New tool | Critical | High | High | High (enables H1, H3) |
| S8 | `scenario_factor_loadings` | scenarios | New tool | Critical | High | High | High (enables H3) |
| S9 | `fuse_volatility` | shared crate | Fusion operator | Medium | Medium | Medium | Medium |
| S10 | `apply_scenario_tree` (reverse bridge) | companies | New tool | Critical | High | High | High |
| S11 | Citation-gate validation | scenarios + research | Schema + check | Medium | Low | Low | High (enables H5) |
| S12 | Tâtonnement feedback loop | new (cybernetics) | Design + impl | Medium | High | High | Medium (enables H5) |

---

## 4. MCDA ranking

**Criteria (weighted):**
- **Leverage** (0.30): how many hypotheses / downstream tasks this unblocks.
- **Effort** (0.20): inverse — lower effort scores higher.
- **Falsifiability** (0.25): does this surface enable a discriminating test?
- **Risk** (0.15): inverse — lower implementation risk scores higher.
- **Novelty** (0.10): how much new capability vs. exposing existing.

**Scoring (1–5, higher = better):**

| ID | Leverage | Effort(inv) | Falsif. | Risk(inv) | Novelty | Weighted |
|---|---|---|---|---|---|---|
| S1 | 4 | 5 | 5 | 5 | 2 | **4.35** |
| S2 | 4 | 5 | 5 | 5 | 2 | **4.35** |
| S3 | 4 | 5 | 5 | 5 | 3 | **4.45** |
| S4 | 5 | 3 | 5 | 3 | 4 | **4.10** |
| S5 | 5 | 3 | 5 | 4 | 3 | **4.20** |
| S6 | 5 | 3 | 5 | 3 | 4 | **4.10** |
| S7 | 5 | 2 | 5 | 2 | 5 | **3.85** |
| S8 | 5 | 2 | 5 | 2 | 5 | **3.85** |
| S9 | 3 | 3 | 3 | 3 | 3 | **3.00** |
| S10 | 5 | 2 | 5 | 2 | 5 | **3.85** |
| S11 | 3 | 5 | 5 | 5 | 2 | **4.15** |
| S12 | 3 | 1 | 3 | 1 | 5 | **2.50** |

**Compensation-masking check:** S7, S8, S10 are all "Critical leverage, High
effort, High risk." Their high weighted scores come from falsifiability, not
leverage — they are *not* dominated by the easy wins (S1–S3). The easy wins
are enablers; the critical-risk-core items are the actual research. No
compensation masking detected: the ranking correctly separates "enablers"
from "the work."

**Sensitivity analysis:** Vary weights ±0.05. The top 3 (S3, S1, S2) are
stable across all weight perturbations — they are unambiguous quick wins.
S7/S8/S10 remain in the top 6 across perturbations. S12 (tâtonnement) is
the most weight-sensitive: it drops to last under "effort-heavy" weighting
but rises under "novelty-heavy" weighting. This reflects its status as
exploratory framing, not core infrastructure.

---

## 5. Recommended build order (MCDA-informed)

**Phase 1 — Quick wins (enablers):** S1, S2, S3, S11. (Unblocks H2, H5
falsifiability. ~2 weeks.)

**Phase 2 — Composition:** S4, S5. (Unblocks WS4. ~4 weeks.)

**Phase 3 — Risk core (the work):** S6, S7, S8, S9, S10. (The heart. ~12
weeks.)

**Phase 4 — Equilibrium framing:** S12. (Exploratory. ~4 weeks, in
parallel with Phase 3's later tasks.)

**Phase 5 — Falsification:** Execute H1, H2, H3 tests. (~4 weeks.)

**Total:** ~26 weeks of focused build + test, within the ~1-year envelope
allowing for theory extraction (WS1), data collection, and iteration.
