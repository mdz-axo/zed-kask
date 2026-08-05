---
title: "Prediction Markets → zed-kask: Implementation Plan"
audience: [architects, developers]
last_updated: 2026-08-04
version: "0.1.0"
status: "Active"
domain: "Forecasting"
mds_categories: [domain, composition]
---

# Prediction Markets → zed-kask: Implementation Plan

**Companion to:** `docs/reports/prediction-markets/01-prediction-markets-research.md`, `02-zed-kask-integration.md`
**Method:** task-breakdown PDCA (PLAN → DECOMPOSE → EVALUATE → QUALITY-GATE → WRITE). Vertical slices, bottom-up dependency order, high-risk-first (Phase 0 spike), checkpoints between phases.
**PKO anchor:** `pko:Procedure` targeting "Prediction-market data service for zed-kask forecasting".

---

## Overview

A new read-only MCP data-service server, `hkask-mcp-prediction-markets`, exposes Polymarket (Gamma/CLOB) and Kalshi (Predictions REST) as a unified, **annotated** market-implied-probability feed. The feed is consumed by the existing `hkask-mcp-scenarios` server (caller-mediated, then a native bridge) and the `superforecasting` skill cascade (FlowDef context injection). A closed calibration loop feeds resolved-market Brier scores back into source-reliability gating.

The plan is **phased**: Phase 0 falsifies the live-API assumptions before any implementation; Phase 1 ships the standalone data service (the user's primary ask — "prediction market APIs as data services for the companies and/or scenarios servers"); Phase 2 wires consumers; Phase 3 closes the feedback loop and adds streaming.

## Architecture decisions

1. **New crate `hkask-mcp-prediction-markets`** under `kask/mcp-servers/` — read-only, no trading. Reuses `hkask-mcp-server` scaffolding and `hkask-forecast` math. Does **not** add `reqwest` to `hkask-mcp-scenarios`.
2. **Annotated contract is load-bearing.** Every probability is paired with spread/volume/liquidity/last-update/calibration/reliability_tier. A bare-probability tool is forbidden by design (research §4 guardrail).
3. **Two HTTP providers** (Polymarket, Kalshi) behind a shared `MarketFeed` trait, each with its own `reqwest::Client` (research-server `provider_http_client()` pattern) and per-variant error mapping (`map_market_error`) — not blanket `McpToolError::internal`.
4. **Credentials: `Some(&[])`** start (both platforms expose public market-data reads). Only `config_env` for cache TTL. `all_servers_have_credential_allowlist` + allowlist-alignment tests must pass.
5. **Stale-signal rule:** calibration/price read failures propagate `stale: true` (or a typed error), never a synthetic 0 — the `.rules` "unwrap_or(0) on regulation sense inputs" trap generalized to market signals.
6. **Superforecasting integration is FlowDef-context injection**, not template edits. Templates stay sandboxed.

## Dependency graph (bottom-up)

```
T0 (spike) ── no deps (fail-fast gate)
   │
   ├─> T1 (crate skeleton + registry) ── depends T0
   │      │
   │      ├─> T2 (Polymarket Gamma provider) ── depends T1
   │      ├─> T3 (Kalshi REST provider)      ── depends T1   (parallel with T2)
   │      │      │
   │      ├─> T4 (unified annotated contract + reliability_tier + dual-axis ontology block) ── depends T2,T3
   │      │      │
   │      │      ├─> T4b (market_ontology_map tool: mapping as first-class output) ── depends T4
   │      │      ├─> T4c (market_match: event ↔ market entity resolution) ── depends T4
   │      │      │
   │      ├─> T5 (calibration math via hkask-forecast + ForecastStore) ── depends T4
   │      │      │
   │      └─> T6 (cache + stale-signal + error mapping) ── depends T4   (parallel with T5)
   │             │
   │             └─[CHECKPOINT 1: server builds, tools respond, schema tests pass]
   │
   ├─> T7 (scenarios caller-mediated consumption guide + test) ── depends T6 (no scenarios edit)
   │      │
   │      └─> T8 (scenario_from_markets native bridge in scenarios server) ── depends T7  (optional, Phase 2)
   │             │
   │             └─[CHECKPOINT 2: market→ScenarioEvent end-to-end]
   │
   ├─> T9 (superforecasting FlowDef context injection) ── depends T6
   │
   └─> T10 (calibration loop closure: Brier feedback → reliability_tier) ── depends T5,T6
          │
          └─> T11 (scenario-builder pre-weighting + streaming/WS) ── depends T10  (Phase 3)
                 │
                 └─[CHECKPOINT 3: closed feedback loop, streaming live updates]

Phase 4 (deterministic stats + CMP):
T13 (hkask-forecast expansion: bias correction, isotonic, vol regime, log-odds) ── depends T5
   │
   └─> T14 (CMP construction + base-event registry) ── depends T0 (feasibility sample), T13, T4
          │
          └─> T15 (residual risk decomposition) ── depends T14
                 │
                 └─[CHECKPOINT 4: no statistics left to prompts]
```

## Risks (carried from integration report §8)

R1 Polymarket Gamma field shapes unverified — **Phase 0 resolves**. R2 Kalshi percentile-history shape unverified — **Phase 0 resolves**. R3 rate limits — cache TTL. R4 politics underconfidence bias — `domain_bias` in contract + test. R5 stale-signal trap — typed error + test. R6 thin-market misread — `reliability_tier` + test. R7 bucket reconstruction — Phase 2. R8 reqwest/tokio reactor — use existing MCP launch paths, test. R9 credential allowlist — `Some(&[])` + alignment test. R10 geo-block — out of scope for reads. R11 polymonitor.club terminal-only — no dependency. R12 Kalshi cents→dollar migration — parser tolerates both + units test (T3). R13 Kalshi bids-only orderbook — ask = 1−best-no-bid, fixture test (T3). R14 ontology-mapping precedent unverified (Q-O1/Q-O2) — resolved in T4 before the contract shape is pinned.

## Open questions

- Q1 (Phase 0): exact Polymarket Gamma `/events` + `/markets` JSON field names (docs SPA 404'd via fetch; must live-test).
- Q2 (Phase 0): Kalshi `forecast-percentile-history` response shape and whether public reads need any header.
- Q3 (Phase 1): should `reliability_tier` thresholds be per-domain (politics vs sports volume differ by orders of magnitude)? Defer to after T0 spike data.
- Q4 (Phase 2): does `scenario_from_markets` call the prediction-markets MCP server over the in-process tool boundary (like `scenario_from_companies`), or share a lib? Confirm against the companies-bridge implementation during T7.
- Q-O1 (T4): how/whether existing kask MCP servers annotate tool outputs with PKO/Dublin Core mappings — grep before pinning the `ontology` block shape; follow precedent if found.
- Q-O2 (T4): does a `hkask:` forecasting ontology namespace already exist? Grep before defining new terms (calibration vocabulary is domain-supplement tier regardless).

## Refinement history (PDCA visibility)

- **Iteration 1 → 2 (sizing/red-flag):** initial draft folded "build markets server" and "wire scenarios consumer" into one task titled "build markets server and wire scenarios" — violated the "no 'and' in a title" rule. Split into T1–T6 (server) and T7/T8 (consumer). Added T0 spike as a fail-fast gate for R1/R2 (the highest-impact unverified assumptions).
- **Iteration 2 → 3 (vertical-slice integrity):** T4 was originally a horizontal "define contract types" layer shared across T2/T3. Re-sliced: T2/T3 each return provider-local raw structs; T4 is the vertical slice that unifies them into the annotated contract + exposes the first end-to-end `market_lookup` tool returning the full annotated record. This makes T4 independently testable rather than a shared layer.
- **Iteration 3 (quality-gate):** added explicit stale-signal test requirements to T6 and bias-surfacing test to T4 after quality-gate flagged "red-flag absence: no verification that the cybernetic guardrails are enforced." Convergence reached: weighted_total < 0.15, no criterion > 0.30.

## Phase summary

- **Phase 0 — Spike (fail-fast):** T0. Verifies live API shapes. Blocks everything.
- **Phase 1 — Data-service server (the primary deliverable):** T1–T6. Checkpoint 1.
- **Phase 2 — Consumer wiring:** T7–T9. Checkpoint 2.
- **Phase 3 — Calibration loop + streaming:** T10–T11. Checkpoint 3.
- **Phase 4 — Deterministic statistics + CMP:** T13–T15. Checkpoint 4.

## Detailed tasks

### Phase 0 — Spike (fail-fast gate)

#### T0 — Live API shape spike
- **slice_id:** `spike/api-shapes`
- **Description:** Issue live read requests against Polymarket Gamma and Kalshi REST to pin exact response field names for the events/markets/forecast-percentile-history endpoints. Produce a short findings note (`docs/reports/prediction-markets/00-api-shape-spike.md`) with the concrete JSON shapes. This is the falsification test for R1/R2 — it must pass before T1.
- **Acceptance criteria:**
  - A live `GET gamma-api.polymarket.com/events` (and a market detail) returns ≥1 record; the fields used by the annotated contract (id, slug, question, deadline, outcome prices, volume, liquidity, status) are identified.
  - A live `GET docs.kalshi.com`-resolved `/events` and `/events/{ticker}/forecast-percentile-history` returns ≥1 record; percentile-history field shape is recorded.
  - The spike note records any field gaps that require the Phase-2 bucket-reconstruction or on-chain fallback.
  - **CMP feasibility sample:** for 2–3 candidate base events (e.g. Kalshi `FED` series, a major election event), count related markets with *distinct deadlines* and record their price-history depth. CMP interpolation (T14) needs per-tenor coverage; this sample decides whether T14 builds full interpolation or degrades to tenor bucketing with wide error bars.
- **Verification:** spike note exists and references real response excerpts; CI does not run this (manual/recorded), but the contract types in T4 must cite it.
- **Dependencies:** None.
- **Files likely touched:** `docs/reports/prediction-markets/00-api-shape-spike.md` (new).
- **Estimated scope:** S.

---

### Phase 1 — Data-service server (primary deliverable)

#### T1 — Prediction-markets server crate skeleton + registry entry
- **slice_id:** `markets/crate-skeleton`
- **Description:** Create `kask/mcp-servers/hkask-mcp-prediction-markets/` with `Cargo.toml` (`[lib] name = "hkask_mcp_prediction_markets" path = "src/hkask_mcp_prediction_markets.rs"`, no `mod.rs`), a minimal server struct via `mcp_server!`, a `run()` factory declaring `CredentialRequirement::optional` for cache TTL, and the `BuiltinMcpServer` entry in `kask_bridge/src/mcp_servers.rs` (`id: "prediction-markets"`, `credentials: Some(&[])`, `config_env: Some(&["HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS"])`). Add the crate to the workspace.
- **Acceptance criteria:**
  - `cargo check -p hkask-mcp-prediction-markets` passes; the binary `hkask-mcp-prediction-markets` builds.
  - `all_servers_have_credential_allowlist` test still passes (and covers the new entry).
  - `mcp_servers` registry lists `prediction-markets` with the expected allowlist shape.
- **Verification:** `./script/clippy -p hkask-mcp-prediction-markets` clean; the credential-allowlist test green.
- **Dependencies:** T0.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/{Cargo.toml,src/hkask_mcp_prediction_markets.rs,src/main.rs}`, `kask/crates/kask_bridge/src/mcp_servers.rs`, workspace `Cargo.toml`.
- **Estimated scope:** S.

#### T2 — Polymarket Gamma provider
- **slice_id:** `markets/polymarket-provider`
- **Description:** Add `src/provider_polymarket.rs` with a `reqwest::Client` (research `provider_http_client()` pattern) hitting Gamma `/events` and `/markets`; return provider-local raw structs. Use `hkask_mcp_server::server::http_helpers::{api_get, classify_http_error}`. Read-only; no auth. No face-value probability tool — raw provider structs only at this layer.
- **Acceptance criteria:**
  - A `polymarket_events(query)` call returns parsed events with the fields identified in T0.
  - HTTP 404/429/503 classify to `not_found`/`rate_limited`/`unavailable` (not blanket `internal`).
  - A unit test with a recorded response fixture parses correctly.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets provider_polymarket`; clippy clean.
- **Dependencies:** T1.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/provider_polymarket.rs`, tests.
- **Estimated scope:** M.

#### T3 — Kalshi REST provider (parallel with T2)
- **slice_id:** `markets/kalshi-provider`
- **Description:** Add `src/provider_kalshi.rs` hitting Kalshi `/events`, `/markets`, `/series`, and `/events/{ticker}/forecast-percentile-history`. Same HTTP-helper reuse and error mapping. Prefer `forecast-percentile-history` as the probability source where available (`probability_method: "kalshi_percentile_history"`).
- **Acceptance criteria:**
  - `kalshi_events`/`kalshi_market`/`kalshi_forecast_history` parse the T0 shapes.
  - Errors classify per-variant.
  - A fixture-based unit test passes.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets provider_kalshi`.
- **Dependencies:** T1.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/provider_kalshi.rs`, tests.
- **Estimated scope:** M.

#### T4 — Unified annotated contract + `market_lookup` tool
- **slice_id:** `markets/annotated-contract`
- **Description:** Define the annotated `MarketRecord` (the contract from integration report §4, **including the dual-axis `ontology` block**) and the first end-to-end tool `market_lookup { query, deadline?, category? } → MarketRecord[]` that calls T2/T3 and returns the full annotated record including `reliability_tier` (derived from volume/spread/last-update thresholds). Surface `calibration.domain_bias` from a static per-domain table seeded from 2602.19520 (politics → underconfident) until T5 computes it from data. **Ontology mapping work:** (a) resolve open questions Q-O1/Q-O2 by grepping existing kask MCP servers + registry for a PKO/Dublin Core output-annotation precedent and any existing `hkask:` forecasting vocabulary — follow the precedent if one exists; (b) implement the dual-axis mapping: `ontology.process` = PKO (`pko:ProcedureExecution` + 2604.20421 lifecycle stage + probability-as-StepExecution-output), `ontology.state` = Dublin Core (`dcterms:identifier` = `{source}:{market_id}`, `title` ← question, `description`, `temporal` ← deadline, `provenance` ← resolution_source), plus `mapping_version`. Use `AnyJsonValue` for any arbitrary-JSON field. Run `find_boolean_schema_positions` on `schema_for!(MarketLookupRequest)`.
**Ontology mapping work:** (a) resolve open questions Q-O1/Q-O2 by grepping existing kask MCP servers + registry for a PKO/Dublin Core output-annotation precedent and any existing `hkask:` forecasting vocabulary — follow the precedent if one exists; (b) implement the dual-axis mapping: `ontology.process` = PKO (`pko:ProcedureExecution` + 2604.20421 lifecycle stage + probability-as-StepExecution-output), `ontology.state` = Dublin Core (`dcterms:identifier` = `{source}:{market_id}`, `title` ← question, `description`, `temporal` ← deadline, `provenance` ← resolution_source), plus `mapping_version`. **Volatility annotation:** compute `volatility.realized_variance` from `price_history` deltas and set `volatility.structural_flag` per 2607.08199 (`near_deadline`, `near_coinflip`) — consumers get an interpretable stability signal without re-implementing the math. Use `AnyJsonValue` for any arbitrary-JSON field. Run `find_boolean_schema_positions` on `schema_for!(MarketLookupRequest)`.
- **Acceptance criteria:**
  - `market_lookup` returns records with non-null `probability`, `spread`, `volume`, `last_update`, `calibration`, `reliability_tier`, `volatility` for any live query.
  - A politics-category record carries `domain_bias: "underconfident"` (the 2602.19520 guardrail) — pinned by a test.
  - Every returned record carries a populated `ontology` block with both `process` (PKO) and `state` (Dublin Core) sub-blocks — pinned by a test.
  - A market near its deadline at ~0.50 price carries `structural_flag: "near_deadline_and_coinflip"` — pinned by a test.
  - Q-O1/Q-O2 resolution is recorded in the spike note or a short comment: precedent followed, or new shape justified.
  - Tool-input schema has no bare-boolean positions (AnyJsonValue enforced).
- **Verification:** `cargo test -p hkask-mcp-prediction-markets market_lookup`; boolean-schema test green; ontology-block test green.
- **Dependencies:** T2, T3.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{types.rs,tools.rs}`, tests.
- **Estimated scope:** M.

#### T4c — Event ↔ market matcher (`market_match`)
- **slice_id:** `markets/market-match`
- **Description:** Implement `market_match { question, deadline?, category? } → [{ market: MarketRecord, match_confidence, match_basis }]`: given a scenario/forecast question, find the market(s) about the *same underlying event*. Owns question normalization (entity extraction, date/horizon alignment), candidate retrieval via T2/T3 search, and confidence-tiered ranking. Match confidence must be conservative: wrong-entity matches (different Fed meeting, different election cycle) score low. This is the mechanical path T8's `scenario_from_markets` bridge and T9's FlowDef injection consume — without it, the agent improvises the event mapping per-invocation, the highest-error-risk step in the whole pipeline.
- **Acceptance criteria:**
  - A query for a known live market's own question returns that market at high confidence (fixture test).
  - A query with a mismatched deadline (same entities, different cycle) returns low confidence or no match — pinned by a test.
  - Low-confidence matches are refusable downstream (confidence field consumed by T8's gate).
  - `schema_for!(MarketMatchRequest)` has no bare-boolean positions.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets market_match`.
- **Dependencies:** T4.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{tools.rs,matcher.rs}`, tests.
- **Estimated scope:** M.

#### T4b — Ontology-mapping tool (`market_ontology_map`)
- **slice_id:** `markets/ontology-map-tool`
- **Description:** Expose the dual-axis mapping itself as a first-class tool, `market_ontology_map { } → { mapping_version, process_axis: {...}, state_axis: {...}, lifecycle_stages: [...] }`, so consumers of the feed can fetch the mapping independent of any specific market record (e.g. a FlowDef context step that needs the vocabulary before interpreting injected records). The returned document is the single source of truth the per-record `ontology` blocks are instances of; both are generated from the same Rust constants so they cannot drift.
- **Acceptance criteria:**
  - `market_ontology_map` returns the full PKO + Dublin Core mapping document with a `mapping_version` matching the per-record blocks.
  - A test asserts the tool output and the `MarketRecord.ontology` block are generated from shared constants (change one, both change).
  - `schema_for!(MarketOntologyMapRequest)` has no bare-boolean positions.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets ontology_map`.
- **Dependencies:** T4.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{tools.rs,ontology.rs}`, tests.
- **Estimated scope:** S.

#### T5 — Calibration math via `hkask-forecast` + resolved-outcome store
- **slice_id:** `markets/calibration`
- **Description:** Reuse `hkask-forecast::brier_score` to compute per-`series`/`category` Brier from resolved markets (resolved_outcome vs probability at a chosen horizon). Persist to a small store (mirror the scenarios `ForecastStore` journal pattern). Expose `market_calibration { series?|category? } → CalibrationSummary`. Compute `domain_bias` from data once sample size ≥ threshold; fall back to the static table below threshold.
- **Acceptance criteria:**
  - `market_calibration` returns `{brier, domain_bias, sample_size, stale}`.
  - `sample_size < threshold` ⇒ `stale: true` (never `brier: 0` when data is thin — the stale-signal rule).
  - A read-error path returns `stale: true`, not 0 — pinned by a test (R5).
- **Verification:** `cargo test -p hkask-mcp-prediction-markets calibration`.
- **Dependencies:** T4.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{calibration.rs,store.rs}`, tests.
- **Estimated scope:** M.

#### T6 — Cache + stale-signal wiring + error mapping
- **slice_id:** `markets/cache-and-stale`
- **Description:** Add a TTL cache (`HKASK_PREDICTION_MARKETS_CACHE_TTL_SECS`) keyed by provider+query. Wire `map_market_error` (per-variant classification) across all tools. Ensure all read failures surface as typed errors or `stale: true` — grep the crate for `unwrap_or(0)` / `unwrap_or_default()` on calibration or price fields and remove. Confirm no `cx.background_spawn` of reqwest futures (use the MCP launch path's tokio reactor — `.rules` trap).
- **Acceptance criteria:**
  - Repeated identical queries hit cache (asserted via a fake-clock test).
  - A provider error propagates a typed `McpToolError` variant, never a silent default.
  - `grep -R "unwrap_or(0)" src/` returns no matches on signal fields.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets cache`; clippy clean.
- **Dependencies:** T4 (parallel with T5).
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{cache.rs,error.rs,hkask_mcp_prediction_markets.rs}`.
- **Estimated scope:** S.

> **CHECKPOINT 1** — `hkask-mcp-prediction-markets` builds, all six tools respond with annotated records, schema + stale-signal + bias tests green, credential-allowlist test green. Human reviews the live-query output before Phase 2.

---

### Phase 2 — Consumer wiring

#### T7 — Scenarios caller-mediated consumption (no scenarios edit)
- **slice_id:** `consumer/scenarios-caller-mediated`
- **Description:** Document + test the zero-edit consumption path: the agent calls `market_lookup`, then passes the result into `scenario_calibrate`/`scenario_cross_validate`/`scenario_synthesize` as `base_rate` and `Perspective{source:"polymarket"/"kalshi", probability, historical_brier}` and `CrossValidateRequest{source_b:"market"}`. Add a scenarios-server integration test that constructs a `Perspective` and `CrossValidateRequest` from a recorded `MarketRecord` fixture and asserts the tools accept it. Confirm Q4 (how `scenario_from_companies` bridges) to inform T8.
- **Acceptance criteria:**
  - A recorded `MarketRecord` feeds `scenario_cross_validate` and produces a divergence value.
  - A market `Perspective` flows through `scenario_synthesize` without error.
  - No file under `kask/mcp-servers/hkask-mcp-scenarios/src/` is modified.
- **Verification:** `cargo test -p hkask-mcp-scenarios market_consumer`.
- **Dependencies:** T6.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-scenarios/tests/market_consumer.rs` (new), docs.
- **Estimated scope:** S.

#### T8 — `scenario_from_markets` native bridge (optional, Phase 2)
- **slice_id:** `consumer/scenario-from-markets`
- **Description:** Add a `scenario_from_markets` tool to `hkask-mcp-scenarios` mirroring `scenario_from_companies` (`hkask_mcp_scenarios.rs:598`, request at L125): it calls the prediction-markets MCP server over the in-process tool boundary (or a shared lib — confirm via Q4), resolves the event via T4c's `market_match`, and returns `ScenarioEvent` JSON with `basis` tagged `polymarket:slug`/`kalshi:ticker` and `base_rate` = market probability (gated by `reliability_tier`). **No `reqwest` added to the scenarios crate.**
- **Acceptance criteria:**
  - `scenario_from_markets` returns a `ScenarioEvent` with `base_rate` from a market record and a `basis` provenance tag.
  - The scenarios crate `Cargo.toml` gains no `reqwest` dependency.
  - A low-`reliability_tier` market yields `base_rate = None` with a warning field (refuses to anchor on unreliable data).
  - A low-`match_confidence` result from `market_match` yields `base_rate = None` with a warning (refuses to anchor on a wrong-event match).
  - The returned `base_rate` has the T13 deterministic domain-bias correction already applied (raw and corrected values both returned, correction provenance recorded) — the LLM consumer never sees an uncorrected politics price as the default anchor.
- **Verification:** `cargo test -p hkask-mcp-scenarios scenario_from_markets`.
- **Dependencies:** T7.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs`, `src/types.rs` (add `basis` provenance convention only), tests.
- **Estimated scope:** M.

#### T9 — Superforecasting FlowDef context injection
- **slice_id:** `consumer/superforecasting-flowdef`
- **Description:** Edit `kask/registry/manifests/superforecasting.yaml` (FlowDef) to inject market data as `knowns` (stage 2, `stage_2_outside_view.j2:7`) and `new_evidence` (stage 4, `stage_4_evidence_update.j2:8`) into the cascade context, populated via the `task` field. **No template edits** — keep templates sandboxed. Add a registry test that the FlowDef carries market context keys when a market-anchored task is run.
- **Acceptance criteria:**
  - A superforecasting cascade with market context produces a stage-2 `knowns` entry sourced from a market record and a stage-4 `new_evidence` entry from a price move.
  - No `.j2` template under `kask/registry/templates/superforecasting/` is modified.
- **Verification:** `cargo test` for the superforecasting FlowDef; skill-maintenance validate.
- **Dependencies:** T6.
- **Files likely touched:** `kask/registry/manifests/superforecasting.yaml`, a registry test.
- **Estimated scope:** S.

> **CHECKPOINT 2** — market data reaches `ScenarioEvent` and the superforecasting cascade end-to-end; no scenarios-server HTTP added; templates untouched. Human reviews a sample forecast that used a market anchor.

---

### Phase 3 — Calibration loop closure + streaming

#### T10 — Calibration loop closure (Brier → reliability_tier)
- **slice_id:** `loop/calibration-feedback`
- **Description:** Close the cybernetic loop: T5's per-source Brier feeds back into T4's `reliability_tier` weighting (a source with high Brier over ≥N resolved markets is down-weighted). Ensure the loop is **negative** (corrective) — a poorly-calibrated source's weight *decreases*. Propagate calibration-read failures as `stale` (never 0). Add an integration test simulating a resolved market and asserting the source's next-query reliability reflects its Brier.
- **Acceptance criteria:**
  - A resolved market with Brier > threshold lowers that source's `reliability_tier` on subsequent queries.
  - A calibration-read failure sets `stale`, not `brier: 0` (the reinforcing-loop trap is absent — pinned by test).
  - The loop is demonstrably negative (calibration ↑ ⇒ weight ↓), asserted by the test.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets calibration_loop`.
- **Dependencies:** T5, T6.
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{calibration.rs,reliability.rs}`, tests.
- **Estimated scope:** M.

#### T11 — Scenario-builder pre-weighting + streaming (WebSocket)
- **slice_id:** `phase3/scenario-builder-and-streaming`
- **Description:** (a) Pre-weight `scenario-builder` `key_forces` (`driving-forces.j2:5`) with market probabilities via FlowDef context (weaker-fit, lower priority). (b) Add a WebSocket streaming option to `hkask-mcp-prediction-markets` (Polymarket market WS / Kalshi ticker WS) so the scenarios panel and superforecasting can receive live updates without polling. Streaming runs on the existing MCP tokio reactor (no `background_spawn` of reqwest/WS futures).
- **Acceptance criteria:**
  - `scenario-builder` cascade with market context ranks `key_forces` influenced by market probabilities.
  - A WS subscription delivers a market update within a test window (fake-stream test).
  - No `cx.background_spawn` of a tokio-dependent future (`.rules` trap) — grep-asserted.
- **Verification:** `cargo test` for scenario-builder context + a streaming unit test.
- **Dependencies:** T10.
- **Files likely touched:** `kask/registry/manifests/scenario-builder.yaml`, `kask/mcp-servers/hkask-mcp-prediction-markets/src/streaming.rs`, tests.
- **Estimated scope:** L → split into T11a (pre-weighting, S) and T11b (streaming, M) if it exceeds one session.

> **CHECKPOINT 3** — closed negative feedback loop; live streaming updates; full integration reviewed against the research report's six findings.

#### T12 — Event-base persistence decision (graph DB vs flat store)
- **slice_id:** `phase3/event-base-decision`
- **Description:** Decide the persistence shape for the resolved-outcome store, match history, and cross-server event relationships. Apply the essentialist deletion test: adopt a graph "event base" **only if** flat-store (SQLite/JSONL) relationship queries demonstrably reappear in consumers (scenarios event trees, companies entities, corpus docs referencing markets). Research findings (verified 2026-08-04): Grafeo (Apache-2.0, pure-Rust embedded, GQL/Cypher/SPARQL + vector/BM25, pre-1.0) ranked first; CozoDB (MPL-2.0, Datalog, SQLite backend, time-travel) fallback; SurrealDB (BSL 1.1 license caution, heavy) third; IndraDB/Kuzu disqualified. **No candidate has CRDT** — if multi-device sync is ever needed, layer automerge/yrs above the store; do not select on CRDT claims. If adopted: storage goes behind a thin trait (ADR-042 port pattern) so the backend is swappable while the event-graph schema stabilizes, and the same graph can serve companies/scenarios/corpus relationship queries.
- **Acceptance criteria:**
  - Deletion test documented: either (a) ≥2 concrete consumer relationship queries that a flat store cannot serve without re-deriving joins in Rust, or (b) decision record keeping the flat store with the revisit trigger written down.
  - If graph adopted: spike `cargo add grafeo` in a scratch crate; embedded open + insert + GQL relationship query works; compile-time/dep-weight impact recorded.
  - The decision record states the CRDT-layering position explicitly.
- **Verification:** decision record committed to `docs/reports/prediction-markets/03-event-base-decision.md`; spike compiles if (b) is not the outcome.
- **Dependencies:** T5 (the store exists), T7 (first consumer evidence of relationship-query need).
- **Files likely touched:** `docs/reports/prediction-markets/03-event-base-decision.md`, possibly a new `kask/crates/hkask-eventgraph` crate.
- **Estimated scope:** M.

### Phase 4 — Deterministic statistics + CMP

#### T13 — Deterministic statistics expansion (`hkask-forecast`)
- **slice_id:** `stats/deterministic-expansion`
- **Description:** Move every closed-form computation out of LLM reasoning into library code (integration report §5.6): (a) `domain_bias_correction(p, domain, horizon) -> p'` — shrinkage-away-from-0.5 per 2602.19520 domain/horizon buckets, seeded from the paper's estimates, later fit from data via T10's calibration loop; (b) `isotonic_recalibrate(pairs: &[(f64, bool)]) -> fn` — per-domain/series recalibration over resolved (price, outcome) pairs; (c) `volatility_regime(price_history) -> Smooth | JumpLike` classifier (2607.08199); (d) log-odds interpolation utilities (shared with T14). All functions return `Result` — thin samples produce an error/insufficient-data variant, never a silent default.
- **Acceptance criteria:**
  - Each function has unit tests incl. boundary cases (p=0.01/0.99, empty history, single observation).
  - `domain_bias_correction` on a politics market moves the probability away from 0.5; on NBA/sports it is near-identity (per 2604.20421 §6.1's already-calibrated finding) — pinned by tests.
  - Insufficient-data inputs return the typed insufficient-data variant, not a best-effort number.
- **Verification:** `cargo test -p hkask-forecast`; `./script/clippy` clean.
- **Dependencies:** T5 (resolved-outcome pairs exist for isotonic).
- **Files likely touched:** `kask/crates/hkask-forecast/src/hkask_forecast.rs` (extend; new crate `hkask-curves` only if surface exceeds ~7 functions — deep-module budget).
- **Estimated scope:** M.

#### T14 — CMP construction + base-event registry
- **slice_id:** `stats/cmp-construction`
- **Description:** Build the Constant Maturity Prediction machinery (§5.6): (a) base-event registry as config (`config_env`: `HKASK_PREDICTION_MARKETS_BASE_EVENTS` — domain → benchmark event/series list); (b) constant-tenor synthesis: bucket a base-event family's price histories by time-to-resolution, interpolate in log-odds space to synthetic 30d/90d/1y series; (c) CMP-standardized volatility (variance on the constant-tenor series, removing deadline-driven vol inflation); (d) `market_cmp { domain, tenor } → { series, coverage, method }` tool. `method` records `interpolated` vs `bucketed_sparse` (the T0 feasibility sample decides which); sparse coverage widens returned error bars rather than failing.
- **Acceptance criteria:**
  - Given a synthetic two-market family (30d and 90d deadlines), the 60d CMP value lies between the two endpoints in log-odds space — pinned by a unit test.
  - Sparse coverage returns `method: "bucketed_sparse"` with widened uncertainty, never a fabricated tight curve.
  - Base-event registry comes only from config (no auto-promotion of markets to benchmark status) — pinned by a test asserting an unregistered event cannot become a base event.
- **Verification:** `cargo test -p hkask-mcp-prediction-markets cmp`.
- **Dependencies:** T0 (feasibility sample), T13 (log-odds utilities), T4 (price_history in the contract).
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/{cmp.rs,tools.rs}`, `mcp_servers.rs` (`config_env` addition + allowlist-alignment test).
- **Estimated scope:** L → split T14a (registry + bucketing) / T14b (interpolation + tool) if >1 session.

#### T15 — Residual risk decomposition
- **slice_id:** `stats/residual-risk`
- **Description:** Base-event exposure analysis (§5.6 item 4): regress a niche event's log-odds changes on its base event's log-odds changes over overlapping windows; return `{ beta, residual_series, r_squared, observations }`. Tool: `market_residual { market_id, base_event } → {...} | insufficient_overlap`. Gated on overlapping-history depth (minimum N observations); below N returns the typed insufficient variant (never a fabricated residual).
- **Acceptance criteria:**
  - A synthetic niche event constructed as base + known residual recovers β ≈ 1 and the injected residual — pinned by a unit test.
  - Fewer than N overlapping observations returns `insufficient_overlap` — pinned.
  - Output carries `observations` and `r_squared` so consumers can judge fit quality (no bare-number returns).
- **Verification:** `cargo test -p hkask-mcp-prediction-markets residual`.
- **Dependencies:** T14 (base events + aligned series exist).
- **Files likely touched:** `kask/mcp-servers/hkask-mcp-prediction-markets/src/residual.rs`, tests.
- **Estimated scope:** M.

> **CHECKPOINT 4** — deterministic stats tested at library level; CMP + residual tools respond with provenance-tagged, uncertainty-carrying outputs; no statistical computation left to LLM prompts. Human reviews a CMP curve against a live base event.

---

## Quality-gate summary

| Criterion | Weight | Score (0=perfect) | Note |
|---|---|---|---|
| Task sizing | 0.25 | 0.05 | all S/M; T11 flagged L→split |
| Vertical-slice integrity | 0.20 | 0.05 | each slice end-to-end testable |
| AC specificity | 0.20 | 0.05 | each task has ≤3 specific ACs + verification |
| Dependency ordering | 0.15 | 0.05 | bottom-up, high-risk spike first |
| Checkpoint presence | 0.10 | 0.00 | 3 checkpoints between phases |
| Red-flag absence | 0.10 | 0.05 | no "and" titles; stale/bias/bare-probability guardrails have tests |
| **Weighted total** | | **0.05** | gate_pass: true (≤0.15, no criterion >0.30) |

The plan is converged (Cauchy-stable after iteration 3; no criterion exceeded 0.30; refinement history recorded above).