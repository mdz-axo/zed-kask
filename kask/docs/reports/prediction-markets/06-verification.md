# Operational Verification Record — Prediction-Markets Data Service

**Date:** 2026-08-05
**Scope:** everything verifiable without a human at the UI. The remaining click-through is a 5-minute human checklist (bottom).

---

## What was verified programmatically (all live)

### 1. Build & launch-path wiring

- `cargo build -p zed -p hkask-mcp-prediction-markets -p hkask-mcp-scenarios` — full editor binary + both servers compile (3m39s clean).
- Binary resolution (`resolve_mcp_binary`, main.rs:2478): `HKASK_MCP_PREDICTION_MARKETS_BIN` override → sibling-of-zed-exe → PATH. The descriptor (`KaskMcpDescriptor::command`, main.rs:2514) resolves env at call time and applies the per-server credential/config allowlists (`filter_credentials_for_server`, `filter_config_env_for_server`).
- `mcp_env()` now emits `HKASK_PREDICTION_MARKETS_{DATA,CACHE_TTL_SECS,BASE_EVENTS}` from `KaskPredictionMarketsSettings` (the gap found and fixed this session — without it the child ran on defaults).
- Registry: `id: "prediction-markets"` in `BUILT_IN_MCP_SERVERS` + IDS + PAIRS; 20/20 registry tests green.

### 2. End-to-end consumer chain (live, over stdio)

- `market_lookup {query:"federal funds"}` → annotated Polymarket record (p=0.879, tier high).
- `scenario_from_markets {market_record, match_confidence:"high"}` → ScenarioEvent with `base_rate: 0.8795`, `basis: prediction_market:polymarket`, provenance `Polymarket:616902`. Zero-edit consumer path confirmed working.

### 3. The calibration feedback loop (live, closed)

- Recorded 6 confidently-wrong resolutions into bucket `Elections` via `market_record_resolution` → journal persisted to `HKASK_PREDICTION_MARKETS_DATA/calibration.jsonl` (atomic).
- `market_calibration {bucket:"politics"}` after reload → `brier: 0.81, n: 6, stale: false` (canonical_bucket unifies the dialects — verified).
- `market_lookup {query:"presidential election", category:"politics"}` → a high-volume election market **demoted to `medium` tier** with `brier: 0.81` and `domain_bias: "underconfident"` visible on the record. **The negative loop fires live.**

### 4. CMP index (live)

- `market_cmp_index {series:"KXFEDDECISION"}` → 6-tenor curve from 12 cohorts, slope +0.79 log-odds/yr (see `05-architecture.md` xychart).

## Test/clippy state

481+ tests passing across the 5 touched crates; `./script/clippy` (`--deny warnings`) clean on all.

---

## Human checklist (the part I cannot do) — ~5 minutes

1. **Settings page:** open Zed settings → Kask → _Prediction Markets_ sub-page. Confirm the three fields render (Data Directory, Cache TTL, Base-Event Registry) and a value you enter persists to `settings.json` under `kask.prediction_markets`.
2. **Tool picker:** in the agent panel, confirm `hkask-mcp-prediction-markets` tools appear (12 tools, `market_*` + `prediction_markets_status`).
3. **Live cascade:** ask the agent _"what does the market imply about the Fed in December 2027, and anchor a scenario on it"_ — confirm it calls `market_match`/`market_lookup` then `scenario_from_markets` and shows the annotated record (tier, bias, ontology).
4. **Superforecasting skill:** invoke the superforecasting skill on a market-covered question and confirm the stage-2 outside view renders the "Prediction-Market Anchors" block.

If any of steps 1–3 fail, the failure is in the GPUI/settings surface (not the server — that layer is fully verified above); check the log for the `reg.tool` span and the server-registration line.
