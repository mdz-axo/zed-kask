---
dcterms:title: "MCP Capability Gap Report"
dcterms:creator: "zed-kask research architect agent"
dcterms:date: "2026-08-05"
rdf:type: bibo:Document
---

# MCP Capability Gap Report

Capabilities-reasoner framing: capability definition declared = **elicitation**
(what the server can do when properly invoked — Password-Locked principle), checked against
**observed behavior** in source. Registry entries carry floor (minimum for the target
foundation), ceiling (current elicited capability), and maturity gate (prerequisite DAG).
Grounded in the full source read (see territory-map.md §2 for line references).

## Registry

### CAP-S: scenario event trees
| ID | Capability | Floor (target) | Ceiling (elicited today) | Maturity gate | Verdict |
|---|---|---|---|---|---|
| S1 | Event representation w/ CPTs | multi-parent CPTs | multi-parent CPTs, but only `depends_on[0]` effective (C28) | — | **restrict-gap**: lift single-group limit |
| S2 | Marginalization / joint probability | exact inference over tree | exact under parent-independence via `hkask_forecast::marginalize` | S1 | **maintain** |
| S3 | Bayesian update | tree-level propagation | per-event scalar Bayes only (C30) | S2 | **expand**: tree-level propagation |
| S4 | Market→event bridging | N markets → dependent tree | 1 market → root event (C29) | S1 | **expand**: composition algebra (WS3) |
| S5 | Calibration feedback | node + bucket calibration w/ source demotion | bucket calibration + Brier persistence | S3 | **expand**: node-level |
| S6 | Challenge gates | tree-time gates | bridge-time refusal gates only | S4 | **expand** |
| S7 | Factor-exposure mapping | scenario→APT loadings | absent (C31) | S4 | **authorize-new** (WS4) |
| S8 | Duration semantics | first-class duration on events | 3-bucket horizon enum (C32) | — | **expand** |

### CAP-C: company forecasts
| ID | Capability | Floor | Ceiling | Gate | Verdict |
|---|---|---|---|---|---|
| C1 | DCF valuation | 2-stage, scenario-weighted | 2-stage + MC + reverse DCF + EP | — | **maintain** (deepest module) |
| C2 | Scenario weighting | tree-driven probabilities | independence-assuming 2x2 multipliers (C34) | S4 | **expand**: consume tree weights |
| C3 | Equity duration | D_e scalar from PV timing | stage years exist; no duration metric (C35) | C1 | **authorize-new** (cheap: DCF outputs suffice) |
| C4 | Factor exposures | cash-flow sensitivity to scenario nodes | absent | S7 | **authorize-new** |
| C5 | Assumption provenance | claims linked to assumptions | claims unlinked; gap decomposition post-hoc only (C36) | R2 | **expand** |

### CAP-P: prediction markets
| ID | Capability | Floor | Ceiling | Gate | Verdict |
|---|---|---|---|---|---|
| P1 | Annotated probability | never-bare probability | enforced structurally (C38) | — | **maintain** (tightest server) |
| P2 | Time-to-maturity | first-class `time_to_maturity` + ladder profiles | deadline string, ad-hoc days (C39) | — | **expand** (small) |
| P3 | Price history | series for realized variance | snapshot only (C39) | — | **authorize-new** |
| P4 | Reliability tiers | demotion on stale performance | implemented w/ negative feedback (C37) | — | **maintain** |
| P5 | Resolution risk | resolution-source/UMA status surfaced | present | — | **maintain** |

### CAP-R: research
| ID | Capability | Floor | Ceiling | Gate | Verdict |
|---|---|---|---|---|---|
| R1 | Multi-provider search/extract | RRF fusion + provenance | implemented (C41) | — | **maintain** |
| R2 | Citation infrastructure | stable citation IDs + content-hash pinning + claim-level spans | none (blake3 = cache keys only, C42) | R1 | **authorize-new** — the largest research gap |
| R3 | Durable citation store | citation store queryable by scenarios/companies | RSS entries only | R2 | **authorize-new** |

## Deep-module assessment (per server)

- **scenarios**: `superforecast.rs` deep (real math behind ~15 functions); dispatch layer
  appropriately thin; `record_experience` is a vestigial name (only calls `check_sequence`)
  — **G1 fail candidate, rename or remove**. Prompt-template emitters (`scenario_build`,
  `scenario_frame`, `scenario_brainstorm`) are shallow by design (LLM does the work).
- **companies**: deepest of the four. `financial_model.rs` genuinely deep; `financial_data.rs`
  shallow-but-legitimate proxy; `research.rs` classifier shallow-ish with a hand-rolled
  percent-encoder (**G3 fail: reinvented wheel** — use `urlencoding` crate).
- **prediction-markets**: uniformly good; every module passes the deletion test.
- **research**: deep where it matters (`ProviderPool::search_compound`, `db.rs`).

## Cross-server integration gaps

1. One code edge (scenarios→prediction-markets type reuse); everything else is
   caller-mediated paste bridging (C43). Target foundation needs **typed bridges**:
   markets→tree composition (S4), tree→company weighting (C2), research→citation pinning (R2).
2. Stale-anchor risk: bridged base rates frozen at bridge time; no refresh path (C43).
3. Duplicated capability: `companies.research_search` ∥ research server (C44) — strangler-fig
   candidate: route companies' research through the research server or shared crate.

## Maturity-gate DAG (prerequisite order)

```
R2 (citations) ──┐
P2 (maturity) ───┤
S1 (CPT fix) ──> S4 (composition) ──> S3 (propagation) ──> S7 (factor mapping) ──> C4
                                        C2 (tree weights) ──^
C3 (equity duration) ─────────────────> H2 tests ──────────> S7
```

Metric-stability check (mirage guard): verdicts above are stable across both capability
definitions that apply (task-performance on the tool surface; elicitation via composed
calls). S4's ceiling differs under the two definitions — an LLM caller *can* hand-author
`depends_on` trees today (elicitation), but the server offers no composition support
(task-performance) — verdict recorded as expand under both, stable.
