# Prediction Markets → zed-kask: Task Checklist

Companion to `tasks/plan.md`. Grouped by phase. ☐ = pending.

## Phase 0 — Spike (fail-fast gate)
- ☑ **T0 — Live API shape spike** (`spike/api-shapes`)
  - [x] Live `GET gamma-api.polymarket.com/events` (+ market detail) returns ≥1 record; annotated-contract fields identified
  - [x] Live Kalshi `/events` + `/events/{ticker}/forecast-percentile-history` shapes recorded
  - [x] CMP feasibility: 2–3 base events sampled for per-tenor market density + price-history depth
  - [x] `docs/reports/prediction-markets/00-api-shape-spike.md` written; gaps noted
  - Verify: contract types in T4 cite the spike note

## Phase 1 — Data-service server (primary deliverable)
- ☑ **T1 — Prediction-markets server crate skeleton + registry entry** (`markets/crate-skeleton`)
  - [x] `cargo check -p hkask-mcp-prediction-markets` passes; `hkask-mcp-prediction-markets` binary builds
  - [x] `all_servers_have_credential_allowlist` test green (covers new `prediction-markets` entry, `credentials: Some(&[])`)
  - [x] Workspace `Cargo.toml` includes the crate
  - Verify: `./script/clippy -p hkask-mcp-prediction-markets` clean
- ☑ **T2 — Polymarket Gamma provider** (`markets/polymarket-provider`)
  - [x] `polymarket_events` parses T0 fields; read-only, no auth
  - [x] HTTP 404/429/503 classify to `not_found`/`rate_limited`/`unavailable` (not blanket `internal`)
  - [x] Fixture unit test passes
  - Verify: `cargo test -p hkask-mcp-prediction-markets provider_polymarket`
- ☑ **T3 — Kalshi REST provider** (`markets/kalshi-provider`) — parallel with T2
  - [x] `kalshi_events`/`kalshi_market` parse T0 shapes (percentile-history dropped — 404 live, candlesticks instead)
  - [x] ~~forecast-percentile-history~~ endpoint 404'd live; midpoint + candlesticks used
  - [x] Errors classify per-variant; fixture test passes (+ missing-field regression test)
  - Verify: `cargo test -p hkask-mcp-prediction-markets provider_kalshi`
- ☑ **T4 — Unified annotated contract + `market_lookup` tool** (`markets/annotated-contract`)
  - [x] `market_lookup` returns full annotated `MarketRecord` (live-verified: Polymarket Fed markets) (probability + spread + volume + last_update + calibration + reliability_tier)
  - [x] Politics-category record carries `domain_bias: "underconfident"` (pinned by test)
  - [x] Every record carries populated `ontology.process` (PKO) + `ontology.state` (Dublin Core) blocks (pinned by test)
  - [x] Record carries `volatility` block with `structural_flag` (realized_variance pending price-history wiring) + `structural_flag`; near-deadline ~0.50 market flags `near_deadline_and_coinflip` (pinned by test)
  - [x] Q-O1/Q-O2 resolved: hkask-bridge-dublincore is canonical vocabulary; no hkask: forecasting namespace (domain-supplement tier): existing PKO/DC annotation precedent + `hkask:` namespace greped; resolution recorded
  - [x] `schema_for!(MarketLookupRequest)` has no bare-boolean positions (AnyJsonValue)
  - Verify: `cargo test -p hkask-mcp-prediction-markets market_lookup` + boolean-schema test
- ☑ **T4b — Ontology-mapping tool (`market_ontology_map`)** (`markets/ontology-map-tool`)
  - [x] Tool returns full dual-axis mapping document (live-verified over stdio) with `mapping_version` matching per-record blocks
  - [x] Test asserts tool output and `MarketRecord.ontology` share the same constants (no drift)
  - [x] `schema_for!(MarketOntologyMapRequest)` has no bare-boolean positions
  - Verify: `cargo test -p hkask-mcp-prediction-markets ontology_map`
- ☑ **T4c — Event ↔ market matcher (`market_match`)** (`markets/market-match`)
  - [x] Query for a known market's own question returns it at high confidence (fixture test)
  - [x] Mismatched deadline (same entities, different cycle) scores strictly lower (deadline only penalizes beyond extraction precision) — pinned by test
  - [x] Low-confidence matches are refusable downstream (confidence consumed by T8's gate)
  - [x] `schema_for!(MarketMatchRequest)` has no bare-boolean positions
  - Verify: `cargo test -p hkask-mcp-prediction-markets market_match`
- ☑ **T5 — Calibration math via `hkask-forecast` + store** (`markets/calibration`)
  - [x] `market_calibration` returns `{bucket, brier, sample_size, stale}` (domain_bias lives on the record contract)
  - [x] Thin sample ⇒ `stale: true`, not `brier: 0` (pinned by test)
  - [x] Read-error/missing-bucket path ⇒ `stale: true`, not 0 (R5, pinned by test)
  - Verify: `cargo test -p hkask-mcp-prediction-markets calibration`
- ☑ **T6 — Cache + stale-signal + error mapping** (`markets/cache-and-stale`)
  - [x] TTL cache hits on repeated queries (fake-clock test)
  - [x] Provider errors propagate typed `McpToolError` variants
  - [x] `grep -R "unwrap_or(0)" src/` — no matches on signal fields; no `background_spawn` of reqwest futures
  - Verify: `cargo test -p hkask-mcp-prediction-markets cache`; clippy clean

> **CHECKPOINT 1** ✅ (2026-08-05) — server builds; 5 tools (`prediction_markets_status`, `market_lookup`, `market_match`, `market_ontology_map`, `market_calibration`); 39 tests green; live smoke test returned annotated Polymarket Fed markets. Known limitation: `realized_variance` deferred (needs CLOB prices-history wiring); matcher is deterministic lexical (token Jaccard + deadline), embedding-based retrieval is a future upgrade.

## Phase 2 — Consumer wiring
- ☐ **T7 — Scenarios caller-mediated consumption** (`consumer/scenarios-caller-mediated`) — no scenarios edit
  - [ ] Recorded `MarketRecord` feeds `scenario_cross_validate` → divergence value
  - [ ] Market `Perspective` flows through `scenario_synthesize`
  - [ ] No file under `kask/mcp-servers/hkask-mcp-scenarios/src/` modified
  - Verify: `cargo test -p hkask-mcp-scenarios market_consumer`
- ☐ **T8 — `scenario_from_markets` native bridge** (`consumer/scenario-from-markets`)
  - [ ] Returns `ScenarioEvent` with `base_rate` from market + `basis` provenance tag
  - [ ] Scenarios crate gains no `reqwest` dependency
  - [ ] Low-`reliability_tier` market ⇒ `base_rate = None` + warning (refuses unreliable anchors)
  - Verify: `cargo test -p hkask-mcp-scenarios scenario_from_markets`
- ☐ **T9 — Superforecasting FlowDef context injection** (`consumer/superforecasting-flowdef`)
  - [ ] Cascade with market context produces stage-2 `knowns` + stage-4 `new_evidence` from market data
  - [ ] No `.j2` template under `kask/registry/templates/superforecasting/` modified
  - Verify: superforecasting FlowDef test + skill-maintenance validate

> **CHECKPOINT 2** — market data reaches `ScenarioEvent` and superforecasting cascade end-to-end; no scenarios HTTP added; templates untouched. Human reviews a market-anchored forecast.

## Phase 3 — Calibration loop + streaming
- ☐ **T10 — Calibration loop closure (Brier → reliability_tier)** (`loop/calibration-feedback`)
  - [ ] High-Brier resolved market lowers source `reliability_tier` on subsequent queries
  - [ ] Calibration-read failure ⇒ `stale`, not `brier: 0` (pinned by test)
  - [ ] Loop is negative (calibration ↑ ⇒ weight ↓), asserted by test
  - Verify: `cargo test -p hkask-mcp-prediction-markets calibration_loop`
- ☐ **T11 — Scenario-builder pre-weighting + streaming** (`phase3/scenario-builder-and-streaming`) — split T11a/T11b if >1 session
  - [ ] `scenario-builder` `key_forces` ranked by market probabilities via FlowDef context
  - [ ] WS subscription delivers update in test window (fake-stream test)
  - [ ] No `background_spawn` of tokio-dependent futures (grep-asserted)
  - Verify: scenario-builder context test + streaming unit test

> **CHECKPOINT 3** — closed negative feedback loop; live streaming; full integration reviewed against research report findings.

## Phase 4 — Deterministic statistics + CMP
- ☐ **T13 — Deterministic statistics expansion (`hkask-forecast`)** (`stats/deterministic-expansion`)
  - [ ] `domain_bias_correction`: politics moves away from 0.5, sports near-identity (pinned tests)
  - [ ] `isotonic_recalibrate` + `volatility_regime` + log-odds utilities; boundary tests pass
  - [ ] Insufficient data ⇒ typed error variant, never a silent default
  - Verify: `cargo test -p hkask-forecast`; clippy clean
- ☐ **T14 — CMP construction + base-event registry** (`stats/cmp-construction`) — split T14a/T14b if >1 session
  - [ ] Synthetic 30d/90d family ⇒ 60d CMP between endpoints in log-odds space (pinned test)
  - [ ] Sparse coverage ⇒ `method: "bucketed_sparse"` + widened uncertainty, never a fabricated curve
  - [ ] Base events come only from config registry (no auto-promotion) — pinned test
  - Verify: `cargo test -p hkask-mcp-prediction-markets cmp`
- ☐ **T15 — Residual risk decomposition** (`stats/residual-risk`)
  - [ ] Synthetic base+residual event recovers β ≈ 1 and injected residual (pinned test)
  - [ ] < N overlapping observations ⇒ `insufficient_overlap` (pinned test)
  - [ ] Output carries `observations` + `r_squared` (no bare-number returns)
  - Verify: `cargo test -p hkask-mcp-prediction-markets residual`

> **CHECKPOINT 4** — deterministic stats tested at library level; CMP + residual tools carry provenance + uncertainty; no statistical computation left to LLM prompts. Human reviews a CMP curve against a live base event.

- ☐ **T12 — Event-base persistence decision** (`phase3/event-base-decision`)
  - [ ] Deletion test documented: ≥2 consumer relationship queries a flat store can't serve, OR flat-store decision with revisit trigger
  - [ ] If graph adopted: Grafeo embedded spike compiles; dep-weight recorded; CRDT-layering position stated
  - [ ] `docs/reports/prediction-markets/03-event-base-decision.md` committed
  - Verify: decision record exists; spike compiles if graph path taken