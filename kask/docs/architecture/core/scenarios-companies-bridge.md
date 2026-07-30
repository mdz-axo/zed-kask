---
title: "Scenarios ↔ Companies Bridge"
audience: [developers, architects]
last_updated: 2026-07-29
version: "0.31.0"
status: "Active"
domain: "MCP Servers"
mds_categories: [domain, composition]
---

# Scenarios ↔ Companies Bridge

**Diataxis type:** Architecture
**Status:** Current (v0.31.0)
**Related:** `mcp-servers/hkask-mcp-scenarios` (scenario forecasting), `mcp-servers/hkask-mcp-companies` (financial modeling)

## Purpose

The scenarios server and the companies server share the same math engine (`hkask-forecast`) but serve different domains. The companies server specializes in FIBO-anchored financial modeling (DCF, Schwartz 2×2 scenario analysis, intrinsic value distributions). The scenarios server specializes in Tetlock/Chermack forecast tracking (event trees, Brier scoring, calibration curves, project assessment).

The `scenario_from_companies` tool bridges them: financial projections from the companies server become trackable binomial forecasts in the scenarios server.

## Bridge Path

```
hkask-mcp-companies                    hkask-mcp-scenarios
─────────────────                      ───────────────────
calibrate_forecast                     scenario_from_companies
  ↓                                      ↓
  Schwartz 2×2 scenarios          convert_companies_output()
  intrinsic_per_share               ↓
  applied_growth                  ScenarioEvent[] (binomial)
  applied_margin                    ↓
                                  scenario_quantify (event tree)
                                    ↓
                                  scenario_calibrate (Fermi + base rate)
                                    ↓
                                  scenario_score (Brier tracking)
```

## Ontology Translation

| Companies (FIBO) | Scenarios (Dublin Core) |
|-------------------|------------------------|
| `scenarios[].name` | `ScenarioEvent.name` |
| `intrinsic_per_share` | Drives `probability` via upside heuristic |
| `applied_growth` | `SubQuestion` — "Will revenue growth reach X%?" |
| `applied_margin` | `SubQuestion` — "Will gross margins hold at X%?" |
| `current_price` | Used to compute `upside` → probability bucket |
| — | `ScenarioEvent.basis = "financial_model"` |
| Schwartz 2×2 | `reference_class = "Company DCF scenario analysis, 2×2 Schwartz matrix"` |

## Design Decisions

1. **Probability heuristic:** When Fermi sub-questions are available, `calibrate_from_fermi` determines the probability. Otherwise, a simple upside-based bucketing heuristic applies: `upside > 20% → 0.65`, `0-20% → 0.55`, `-20-0% → 0.40`, `< -20% → 0.25`.

2. **Deadline derivation:** Deadlines are computed from the `TimeHorizon` enum: Tactical = +540 days, Strategic = +1460 days, LongTerm = +2920 days.

3. **No reverse bridge:** There is no `companies_from_scenarios` tool. The bridge is one-directional: financial model → trackable forecast. This is by design — the companies server owns the financial domain.

## Cross-links

- [Scenario Forecasting Pipeline Diagram](../../reference/mcp-servers/scenarios.md) — tool flow including the companies bridge entry point (DIAG-RF-005, inline)
- [Superforecasting: Layered Model](../../explanation/forecasting-and-scenarios.md) — shared math engine architecture
- Scenarios Adversarial Review — code review including `convert_companies_output` analysis