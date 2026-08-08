---
title: "Prediction Markets Integration — Project Status"
audience: [developers, architects, agents, operators]
last_updated: 2026-08-06
version: "0.33.3"
status: "Active — portfolio extraction + CMP index storage + DR-AS volatility landed"
domain: "Forecasting"
mds_categories: [domain, composition, lifecycle]
---

# Prediction Markets Integration — Project Status

**Diataxis type:** Explanation (project state + rationale for the stopping point).
**Scope:** the prediction-market data-service workstream — research, design,
implementation, verification. Status as of 2026-08-05.

## Where we stopped

The workstream is **complete through its planned four phases plus hardening**,
and has been **extended with portfolio extraction, CMP index storage, and
the DR-AS structural volatility model**. The system is live, self-feeding,
and tested; what remains is a short human UI pass and a set of
deliberately deferred depth items with documented re-entry triggers.

### Shipped and verified

| Capability | State | Evidence |
|---|---|---|
| `hkask-mcp-prediction-markets` server (18 market tools + status) | ✅ live | 85 server tests; 3 live smoke probes |
| `hkask-mcp-portfolio` server (13 portfolio tools) | ✅ shipped | 42 server tests; extracted from companies |
| Annotated `MarketRecord` contract (never a bare probability) | ✅ enforced | contract tests; live lookups |
| Dual-axis ontology mapping (PKO + Dublin Core) | ✅ live-verified | `market_ontology_map` over stdio |
| Polymarket + Kalshi read-only providers | ✅ live-verified | T0 spike + fixtures + live calls |
| Entity-resolution matcher (`market_match`) | ✅ tested | deterministic, refusal-gated |
| Calibration feedback loop (negative-only) | ✅ **live-firing** | live demotion of a poisoned bucket |
| Self-feeding resolution scanner | ✅ live-verified | idempotent; 19 recorded on first scan |
| CMP curve + published index + slope | ✅ live-verified | 6-tenor FED curve from 12 cohorts |
| CMP index storage as transaction ledgers | ✅ shipped | `market_cmp_index_store` + `market_cmp_portfolio_store` |
| DR-AS structural volatility model (arXiv:2607.08199) | ✅ shipped | `market_volatility` tool; 16 volatility tests |
| Strike extraction from Kalshi market titles | ✅ shipped | 8 extraction tests; auto-fills `predicted_level` |
| Curated default economic context + live FRED/CoinGecko fetch | ✅ shipped | `market_cmp_context_suggest` tool |
| Residual-risk decomposition | ✅ live-verified | 81 observations on real FED pair |
| Realized variance + volatility regime | ✅ live-verified | 94-observation FED history |
| Scenarios bridge (`scenario_from_markets`) | ✅ live-verified | base_rate + provenance end-to-end |
| Superforecasting + scenario-builder injection | ✅ wired | FlowDef `market_context` inputs |
| Settings plumbing + UI sub-page | ✅ compiles | `mcp_env()` fix; full `zed` binary builds |
| Portfolio widget holdings rendering (any portfolio type) | ✅ shipped | `render_holdings` + `HoldingsBody` |
| No-trading boundary | ✅ pinned by test | `credentials: Some(&["HKASK_FRED_API_KEY"])`; Kalshi WS excluded |

**Totals:** 299+ tests passing across the touched crates; `cargo clippy` clean; full `zed` binary compiles.

## The one remaining human step

A ~5-minute in-editor click-through (settings page render, tool picker
presence, one live cascade, superforecasting stage-2 anchors). The checklist
is in `06-verification.md`. The server layer is fully verified; this step only
exercises the GPUI/settings surface. **This is the only blocker to calling the
workstream "done" rather than "paused."**

## Deliberately deferred (with re-entry triggers)

| Item | Why deferred | Reopens when |
|---|---|---|
| Graph event base | T12 deletion test: zero demonstrated relationship queries | ≥2 consumers need multi-hop traversal a flat store can't serve (`03-event-base-decision.md`) |
| Embedding-based matcher | lexical matcher has no documented failures yet | a wrong-event match or missed match is observed in practice |
| Kalshi WebSocket | requires a trading-capable credential — permanently excluded by the no-trading boundary | never (hard boundary) |
| Scheduled CMP index snapshots | index computes on-demand; no consumer yet needs a daily published series | a consumer asks for curve *history*, not the current curve |
| Polymarket event-level CMP (`bucketed_sparse`) | Kalshi covers the current registry's tenor needs | the registry adds a Polymarket-only base event family |
| Third platform (Manifold, Metaculus) | two providers validate the contract | a real consumer need a second source can't meet |

## Key design decisions (the "why" record)

1. **Separate read-only server** — the scenarios server stays pure-compute;
   market IO lives in its own MCP server (essentialist deletion test, §2 of
   `02-zed-kask-integration.md`).
2. **Never a bare probability** — the contract's load-bearing invariant; every
   probability carries reliability/calibration/volatility/ontology annotation.
3. **Negative-only calibration loop** — poor calibration demotes, good
   calibration never promotes; a failed read is `stale`, never `brier: 0`.
4. **Deterministic bias correction in the bridge** — the politics
   underconfidence correction (arXiv:2602.19520) is applied in code, not left
   to the LLM consumer (closes the consumer-adherence gap mechanically).
5. **No fabrication anywhere** — 50-50 resolutions, thin overlaps, sparse CMP
   coverage, and unparseable deadlines all degrade to explicit
   null/stale/refusal, never a synthesized value.

## Artifacts (the full record)

- `00-api-shape-spike.md` — live API shapes + CMP feasibility (T0)
- `01-prediction-markets-research.md` — the academic + API research base
- `02-zed-kask-integration.md` — the integration design + seams + risks
- `03-event-base-decision.md` — flat-store decision + revisit triggers
- `04-base-event-registry.md` — the CMT-analog base-event selection
- `05-architecture.md` — Diataxis diagrams (context, loop, contract, index)
- `06-verification.md` — the live verification record + human checklist
- `kask/docs/plans/prediction-plan.md` — the phased build plan (T0–T15)
- `kask/docs/reference/mcp-servers/prediction-markets.md` — the tool reference

## Footnotes

[^tetlock]: Tetlock, P. E., & Gardner, D. (2015). *Superforecasting*. Crown. — outside-view anchoring discipline.
[^jia]: Jia, Zhou, Zhang, Cong, Li, Sun (2026). *Unlocking the Forecasting Economy.* arXiv:2604.20421. — full-lifecycle data model; 50-50 resolution finding.
[^le]: Le, N. A. (2026). *Decomposing Crowd Wisdom.* arXiv:2602.19520. — politics underconfidence; domain/horizon calibration structure.
[^xi]: Xi, Moallemi, Pai, Wang (2026). *Volatility in Prediction Markets.* arXiv:2607.08199. — structural volatility (deadline + coin-flip effects).
[^madrigal]: Madrigal-Cianci, Monsalve Maya, Breakey (2026). *Prediction Markets as Bayesian Inverse Problems.* arXiv:2601.18815. — price+volume as evidence, not point estimate.
