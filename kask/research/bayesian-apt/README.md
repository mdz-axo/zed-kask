# Bayesian Arbitrage Pricing via Composable Prediction-Market Scenarios

Research plan and territory map for a multi-quarter investigation linking
prediction markets, scenario event trees, Bayesian probabilities, and
arbitrage pricing theory (APT) to equity-company forecasts.

## Deliverables

| # | Document | Description |
|---|---|---|
| 1 | [01-territory-map.md](01-territory-map.md) | Structured survey of theoretical terrain + current MCP-server state, every claim classified by pragmatic-semantics. |
| 2 | [02-research-plan.md](02-research-plan.md) | 8 workstreams decomposed into verifiable, vertically sliced tasks with acceptance criteria, checkpoints, and dependency DAG. |
| 3 | [03-hypothesis-dossier.md](03-hypothesis-dossier.md) | H1–H5 with FINER/PICO framing, null hypotheses, discriminating tests, evidential status. |
| 4 | [04-three-axes-specification.md](04-three-axes-specification.md) | Time/duration (simple), return (simple), risk (complex) — with deletion-test justification for the complexity allocation. |
| 5 | [05-mcp-capability-gap.md](05-mcp-capability-gap.md) | Current capabilities vs. target foundation, per server, grounded in source reading + deep-module assessment. |
| 6 | [06-integration-architecture.md](06-integration-architecture.md) | How the four MCPs compose; new surfaces ranked via MCDA. |
| 7 | [07-falsification-suite.md](07-falsification-suite.md) | Discriminating tests for H1–H5 with falsifier thresholds. |
| 8 | [08-metacognitive-closeout.md](08-metacognitive-closeout.md) | Brier-scored self-assessment + highest-leverage next experiment. |

## Key findings

- The `hkask-mcp-scenarios` server **already implements** a Bayesian event-tree
  algebra (`EventDependency` conditional tables, `compute_marginal_probabilities`,
  `variance_contribution`) — the structural foundation exists.
- The `hkask-mcp-companies` server has a Residual Income Model with competitive
  fade horizons (a primitive duration model) and a Gordon Growth DCF.
- The `hkask-mcp-prediction-markets` server has reliability tiers, structural
  volatility flags, and calibration data, but **no duration field** and **no
  composition algebra**.
- The **central gap** is the reverse bridge: scenario trees do not currently
  adjust company risk/return forecasts. The risk calculation core
  (σ_scenario + factor loadings) is the largest piece of new work.
- arXiv:2211.03244 (Bhattacharya, "Arbitrage from a Bayesian's Perspective")
  provides the theoretical license: arbitrage arises from belief-hierarchy
  updates, bridging finance and game theory. The scenario event tree is the
  candidate structure for those belief hierarchies.

## Highest-leverage next step

Extract the full text of arXiv:2211.03244 and verify that the existing
`EventDependency` conditional-table algebra satisfies (or approximates)
Bhattacharya's belief-hierarchy recursion. This keystone task determines
whether the foundation is a theorem-backed extension or a novel construction
requiring its own proof.
