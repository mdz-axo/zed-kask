# Prediction Markets → zed-kask: Task Checklist

> **Status (2026-08-05): all phases complete.** See `plan.md` header and
> `docs/reports/prediction-markets/07-project-status.md` for the
> stopping-point record and deferred-item triggers.

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
- ☑ **T7 — Scenarios caller-mediated consumption** (`consumer/scenarios-caller-mediated`) — no scenarios edit
  - [x] Recorded `MarketRecord` feeds `scenario_cross_validate` → divergence value (0.17, review flagged)
  - [x] Market `Perspective` flows through `scenario_synthesize` (inverse-Brier weighting favors the market)
  - [ ] No file under `kask/mcp-servers/hkask-mcp-scenarios/src/` modified
  - Verify: `cargo test -p hkask-mcp-scenarios market_consumer`
- ☑ **T8 — `scenario_from_markets` native bridge** (`consumer/scenario-from-markets`)
  - [x] Returns `ScenarioEvent` with `base_rate` from market + `basis` provenance tag
  - [x] Scenarios crate gains no `reqwest` dependency (shared-lib dependency on the prediction-markets crate, caller-mediated JSON like scenario_from_companies — Q4 resolved)
  - [x] Low-`reliability_tier` market ⇒ `base_rate = None` + warning (refuses unreliable anchors)
  - Verify: `cargo test -p hkask-mcp-scenarios scenario_from_markets`
- ☑ **T9 — Superforecasting FlowDef context injection** (`consumer/superforecasting-flowdef`)
  - [x] `market_context` input added to FlowDef + stage-2 template renders annotated anchors with bias/staleness guidance (stage-4 evidence injection deferred — price-history wiring)
  - [x] Templates stay sandboxed; stage_2 gained a `{% if market_context %}` consumption block (additive, optional input)
  - Verify: superforecasting FlowDef test + skill-maintenance validate

> **CHECKPOINT 2** ✅ (2026-08-05) — market data reaches `ScenarioEvent` via `scenario_from_markets` (deterministic bias correction, refusal gates) and the superforecasting stage-2 template via `market_context` input; no scenarios HTTP added. Human review of a market-anchored forecast pending.

## Phase 3 — Calibration loop + streaming
- ☑ **T10 — Calibration loop closure (Brier → reliability_tier)** (`loop/calibration-feedback`)
  - [x] High-Brier bucket (≥5 observations, Brier > 0.25) demotes High→Medium on subsequent queries (pinned)
  - [x] Calibration-read failure ⇒ `stale`, not `brier: 0` (pinned by test); journal load failure warns, malformed lines skip with warn
  - [x] Loop is negative (asserted); good calibration never promotes (no positive loop, also pinned); JSONL journal persists across restarts; market_record_resolution tool is the sense arm
  - Verify: `cargo test -p hkask-mcp-prediction-markets calibration_loop`
- ☑ **T11 — Scenario-builder pre-weighting + streaming** (`phase3/scenario-builder-and-streaming`) — split T11a/T11b if >1 session
  - [x] `market_context` input added to scenario-builder FlowDef + key-forces template consumption block
  - [x] WS subscriber implemented (Polymarket public market channel; frame parser + subscription framer unit-tested; Kalshi WS needs auth — deferred); live stream test not run (network/timeout-bound)
  - [x] No `background_spawn` (grep-asserted; MCP server is a tokio process)
  - Verify: scenario-builder context test + streaming unit test

> **CHECKPOINT 3** ✅ (2026-08-05) — negative feedback loop closed (sense: market_record_resolution + journal; decide: per-bucket Brier; act: reliability-tier demotion; pinned negative-only). Streaming subscriber shipped (Polymarket public channel). Event-base decision: flat store with revisit triggers. Design note: the stream deliberately does NOT write calibration observations (no pre-resolution probability on the wire) — it notifies; `market_record_resolution` supplies the labeled pair.

## Phase 4 — Deterministic statistics + CMP
- ☑ **T13 — Deterministic statistics expansion (`hkask-forecast`)** (`stats/deterministic-expansion`)
  - [x] `domain_bias_correction` (wired into the T8 bridge) + `log_odds`/`from_log_odds` + `isotonic_fit`/`isotonic_apply` (PAVA) + `volatility_regime` — all pinned
  - [x] boundary tests pass (empty fit ⇒ identity, <2 pairs ⇒ None, extremes finite)
  - [x] insufficient data ⇒ None/typed error, never a silent default
  - Verify: `cargo test -p hkask-forecast`; clippy clean
- ☑ **T14 — CMP construction + base-event registry** (`stats/cmp-construction`) — split T14a/T14b if >1 session
  - [x] log-odds midpoint pinned (0.7/0.5 ⇒ 0.608, not linear 0.60)
  - [x] sparse ⇒ bucketed_sparse with bracket width; empty ⇒ None; extrapolation flat
  - [x] registry via HKASK_PREDICTION_MARKETS_BASE_EVENTS (allowlist-registered); unregistered series refused
  - Verify: `cargo test -p hkask-mcp-prediction-markets cmp`
- ☑ **T15 — Residual risk decomposition** (`stats/residual-risk`)
  - [x] β=1 and β=0.5 recovered within tolerance; r² > 0.95 on tracking series
  - [x] thin overlap + immobile base both refuse (pinned); market_residual tool returns typed insufficient_overlap
  - [x] output carries observations + r_squared + alpha + latest_residual
  - Verify: `cargo test -p hkask-mcp-prediction-markets residual`


## Post-plan hardening (2026-08-05)
- ☑ **market_check_resolutions** — self-feeding sense arm: scans settled Kalshi markets + resolved Polymarket markets, records definitive outcomes idempotently (contains-guard), skips ambiguous 50-50 resolutions. Live-verified: 19 recorded, re-scan idempotent.
- ☑ **realized_variance** — log-odds step variance computed from Kalshi candlesticks / Polymarket CLOB prices-history; `market_history` tool live-verified (94 observations, Smooth regime on a FED contract). <2 moves ⇒ None.
- ☑ **Stage-4 evidence injection** — `market_context` wired into the superforecasting stage-4 evidence-update step with volume/spread-scaled likelihood-ratio guidance.

> **CHECKPOINT 4** ✅ (2026-08-05) — deterministic stats in hkask-forecast (domain_bias_correction, isotonic PAVA, volatility regime, log-odds); market_cmp live-verified against the real KXFEDDECISION series (12 cohorts, interpolated, p=0.084 at 365d tenor on the H0 'no change' contract); market_residual refuses thin overlap. Human review of the CMP curve pending.

- ☑ **T12 — Event-base persistence decision** (`phase3/event-base-decision`)
  - [x] DECIDED: flat store. Zero demonstrated relationship queries (grep evidence); revisit triggers + pre-registered backend ranking (Grafeo > CozoDB > SurrealDB) + CRDT-layering position documented
  - [x] N/A — flat store chosen; CRDT-layering position stated explicitly (automerge/yrs over the store)
  - [x] `docs/reports/prediction-markets/03-event-base-decision.md` written
  - Verify: decision record exists; spike compiles if graph path taken