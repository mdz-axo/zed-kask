# Prediction Markets → zed-kask: Task Checklist

Companion to `tasks/plan.md`. Grouped by phase. ☐ = pending.

## Phase 0 — Spike (fail-fast gate)
- ☐ **T0 — Live API shape spike** (`spike/api-shapes`)
  - [ ] Live `GET gamma-api.polymarket.com/events` (+ market detail) returns ≥1 record; annotated-contract fields identified
  - [ ] Live Kalshi `/events` + `/events/{ticker}/forecast-percentile-history` shapes recorded
  - [ ] `docs/reports/prediction-markets/00-api-shape-spike.md` written; gaps noted
  - Verify: contract types in T4 cite the spike note

## Phase 1 — Data-service server (primary deliverable)
- ☐ **T1 — Markets server crate skeleton + registry entry** (`markets/crate-skeleton`)
  - [ ] `cargo check -p hkask-mcp-markets` passes; `hkask-mcp-markets` binary builds
  - [ ] `all_servers_have_credential_allowlist` test green (covers new `markets` entry, `credentials: Some(&[])`)
  - [ ] Workspace `Cargo.toml` includes the crate
  - Verify: `./script/clippy -p hkask-mcp-markets` clean
- ☐ **T2 — Polymarket Gamma provider** (`markets/polymarket-provider`)
  - [ ] `polymarket_events` parses T0 fields; read-only, no auth
  - [ ] HTTP 404/429/503 classify to `not_found`/`rate_limited`/`unavailable` (not blanket `internal`)
  - [ ] Fixture unit test passes
  - Verify: `cargo test -p hkask-mcp-markets provider_polymarket`
- ☐ **T3 — Kalshi REST provider** (`markets/kalshi-provider`) — parallel with T2
  - [ ] `kalshi_events`/`kalshi_market`/`kalshi_forecast_history` parse T0 shapes
  - [ ] `forecast-percentile-history` preferred as probability source
  - [ ] Errors classify per-variant; fixture test passes
  - Verify: `cargo test -p hkask-mcp-markets provider_kalshi`
- ☐ **T4 — Unified annotated contract + `market_lookup` tool** (`markets/annotated-contract`)
  - [ ] `market_lookup` returns full annotated `MarketRecord` (probability + spread + volume + last_update + calibration + reliability_tier)
  - [ ] Politics-category record carries `domain_bias: "underconfident"` (pinned by test)
  - [ ] Every record carries populated `ontology.process` (PKO) + `ontology.state` (Dublin Core) blocks (pinned by test)
  - [ ] Q-O1/Q-O2 resolved: existing PKO/DC annotation precedent + `hkask:` namespace greped; resolution recorded
  - [ ] `schema_for!(MarketLookupRequest)` has no bare-boolean positions (AnyJsonValue)
  - Verify: `cargo test -p hkask-mcp-markets market_lookup` + boolean-schema test
- ☐ **T4b — Ontology-mapping tool (`market_ontology_map`)** (`markets/ontology-map-tool`)
  - [ ] Tool returns full dual-axis mapping document with `mapping_version` matching per-record blocks
  - [ ] Test asserts tool output and `MarketRecord.ontology` share the same constants (no drift)
  - [ ] `schema_for!(MarketOntologyMapRequest)` has no bare-boolean positions
  - Verify: `cargo test -p hkask-mcp-markets ontology_map`
- ☐ **T5 — Calibration math via `hkask-forecast` + store** (`markets/calibration`)
  - [ ] `market_calibration` returns `{brier, domain_bias, sample_size, stale}`
  - [ ] Thin sample ⇒ `stale: true`, not `brier: 0` (pinned by test)
  - [ ] Read-error path ⇒ `stale: true`, not 0 (R5, pinned by test)
  - Verify: `cargo test -p hkask-mcp-markets calibration`
- ☐ **T6 — Cache + stale-signal + error mapping** (`markets/cache-and-stale`)
  - [ ] TTL cache hits on repeated queries (fake-clock test)
  - [ ] Provider errors propagate typed `McpToolError` variants
  - [ ] `grep -R "unwrap_or(0)" src/` — no matches on signal fields; no `background_spawn` of reqwest futures
  - Verify: `cargo test -p hkask-mcp-markets cache`; clippy clean

> **CHECKPOINT 1** — server builds, seven tools respond with annotated records (incl. ontology blocks), schema + stale-signal + bias + ontology + allowlist tests green. Human reviews live-query output.

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
  - Verify: `cargo test -p hkask-mcp-markets calibration_loop`
- ☐ **T11 — Scenario-builder pre-weighting + streaming** (`phase3/scenario-builder-and-streaming`) — split T11a/T11b if >1 session
  - [ ] `scenario-builder` `key_forces` ranked by market probabilities via FlowDef context
  - [ ] WS subscription delivers update in test window (fake-stream test)
  - [ ] No `background_spawn` of tokio-dependent futures (grep-asserted)
  - Verify: scenario-builder context test + streaming unit test

> **CHECKPOINT 3** — closed negative feedback loop; live streaming; full integration reviewed against research report findings.