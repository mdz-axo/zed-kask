---
title: "Loop Register — zed-kask canonical loops"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-27
version: "0.1.0"
status: "Phase 0 — operator approval pending"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle]
---

# Loop Register

One row per canonical loop: a persistent feedback or control cycle with an
identifiable sense → orient → decide → act → observe path. This register is
the audit's worklist for the Canonical Loop Audit & Consolidation program
(operator spec, 2026-09-27): Phase 1 maps each row's functional graph, Phase 2
detects bugs/impedances, Phase 3 consolidates under the deletion test, Phase 4
verifies and scores the Phase 0 predictions recorded here.

**Method.** Rows were derived from the tree on 2026-09-27 (directory walks +
targeted greps; every entry point was located in the working tree, not recalled
from memory). Row detail is entry-point-level for Phase 0; Phase 1 deepens each
row to a full functional graph with one IS citation per node.

**Prediction semantics.** Each row carries a Phase 0 calibration prior —
(expected defect count / expected impedance count / confidence). These are
scored in Phase 4 against what the audit actually found; they are priors, not
findings.

## Family coverage (spec minimum list, verified against tree)

| Spec family | Status | Row |
| --- | --- | --- |
| Agent turn loop | present | L1 |
| Regulation/curator cycle | present | L2 |
| MCP server request cycles | present | L3 (client runtime), L4 (server framework) |
| Inference bridge | present | L5 |
| Corpus pipeline | present | L6 |
| Agent-panel update/render loops | present | L7 |
| Forecast/calibration loop | present | L8 |
| Kanban/goal loop | present | L9 |
| Memory recall/ingest cycle | present | L10 |
| Extensions found in tree | recorded | L11–L19 |

No expected family was absent; the tree shows ten loop families beyond the
spec's minimum list, recorded below rather than narrowed away.

## Register

### L1 — Agent turn loop
- **Crate/path:** `crates/agent` (zed-side, D-seamed), participants in `crates/agent/src/`
- **Entry point:** `crates/agent/src/thread.rs:3178` `run_turn_internal`; spawned via `thread.rs:2984` / `agent.rs:2472` `run_turn`; submissions at `agent.rs:3615`, `:3754`, `:3900`
- **Participants:** `agent.rs` (10,419 ln), `thread.rs` (12,970 ln), `tools.rs`, `tool_retry_tracker.rs`, `tool_trace.rs`, `tool_permissions.rs`, `sandboxing.rs`, `kask_compaction.rs`, `kask_thread_state.rs`, `templates.rs`; `crates/hkask-conversation-injector`, `crates/hkask-tool-invoker`; condensation via `kask_bridge::condenser_bridge` → `kask/crates/hkask-condenser/src/engine.rs`
- **Trigger:** user prompt submit from the agent panel; tool-result continuation within a turn
- **Hands off to:** L3 (tool dispatch), L5 (model streaming), L2 (skill spans/outcomes), L10 (turn-end memory ingest), L7 (thread events → panel), L12 (research-run sources from web tools)
- **Prediction:** 3 / 2 / 0.50

### L2 — Regulation/curator cybernetic cycle
- **Crate/path:** `kask/crates/hkask-regulation` (runtime.rs, cybernetics_loop.rs, cybernetics_loop/cycle.rs, metacognition.rs, set_points.rs, energy.rs, dampener.rs, sensor_provider.rs)
- **Entry point:** `kask/crates/hkask-regulation/src/metacognition.rs:346` `run` / `:405` `tick`; cycle stages at `cybernetics_loop/cycle.rs:305` `sense`, `:409` `compute`, `:450` `act`, `:723` `verify_impact`; facade `cybernetics_loop.rs:259` `CyberneticsLoop::new`
- **Participants:** `RegulationLedger` (constructed `crates/zed/src/main.rs:790`), set points (`main.rs:773-784`), alert channel (`main.rs:632-641`), directive inbox (`main.rs:796-803`), email sink (`main.rs:656`, `kask/crates/hkask-email`); zed-side sensor bridges: `kask_bridge/src/context_server_health_bridge.rs:39`, `ocr_health_bridge.rs:36`, `rollout_event_bridge.rs:104` `poll_once`, `algedonic_log_bridge.rs`, `inference_resilience.rs`, `metacognition_bridge.rs`, `directive_bridge.rs`; wired on the kask tokio runtime (`crates/zed/src/main.rs:579-593`)
- **Trigger:** periodic tick at zed launch; alert channel; `curator_directive` tool
- **Hands off to:** L16 (skill outcomes), algedonic board (`kask_bridge/src/algedonic_board.rs`), `reg_query`/`curator_algedonic_log` tools, energy budgets/call caps governing L1 tool calls
- **Prediction:** 2 / 2 / 0.55

### L3 — MCP client runtime: spawn / health-supervise / request cycle
- **Crate/path:** `kask/crates/hkask-mcp/src/runtime.rs` (2,676 ln)
- **Entry point:** `runtime.rs:1041` `spawn_health_supervisor` (spawned at `:1003-1006` on server start); reconnect-once in `call_tool_inner` (header contract `:20-21`, min interval `:59-68`); circuit breaker config `:233-234`
- **Participants:** zed `context_server` host (`kask_bridge/src/mcp_servers.rs`, D45/D51), `BUILT_IN_MCP_SERVERS` (`crates/zed/src/main.rs:606`), 12 child servers under `kask/mcp-servers/`
- **Trigger:** agent tool invocation (on-demand spawn/reconnect); periodic health check; circuit breaker stop
- **Hands off to:** L4 (server side of each call), L2 (health signals via `context_server_health_bridge`)
- **Prediction:** 2 / 1 / 0.50

### L4 — MCP server request cycle (shared framework, 12 servers)
- **Crate/path:** `kask/crates/hkask-mcp-server/src/server/` + `kask/mcp-servers/*`
- **Entry point:** `server/transport.rs:32` `run_stdio_server`; validation (`validation.rs`), credentials (`credentials.rs`), spans (`tool_span.rs`), errors (`error.rs`); per-server `execute_tool` pattern (e.g. `kask/mcp-servers/hkask-mcp-prediction-markets/src/hkask_mcp_prediction_markets.rs:528`)
- **Participants:** servers: `hkask-mcp-{companies,corpus,curator,kata-kanban,media,portfolio,prediction-markets,research,scenarios,spreadsheet,swarm,training}`
- **Trigger:** stdio JSON-RPC request from the zed host
- **Hands off to:** per-domain loops L6, L8–L13, L17–L19; L2 (tool spans/outcomes)
- **Prediction:** 2 / 1 / 0.50

### L5 — Inference bridge (zed ↔ hkask IPC)
- **Crate/path:** `kask/crates/kask_bridge/src/inference_*.rs` + `kask/crates/hkask-inference`
- **Entry point:** `kask_bridge/src/inference_ipc_server.rs:388` `UnixListener::bind` (2,630 ln); chat surface `inference_chat.rs` (2,001 ln); socket state `inference_socket.rs:24` `set_inference_socket_path` (env-injected into MCP servers via `mcp_env.rs`, `crates/zed/src/main.rs:204-209`, `:674`); client `kask/crates/hkask-inference/src/inference_ipc_client.rs`
- **Participants:** providers (`hkask-inference/src/provider.rs`, `openai_compat.rs`), rerank, media router; zed `LanguageModelRegistry` (D24)
- **Trigger:** chat/embedding/rerank requests from MCP servers and zed-side providers
- **Hands off to:** L1 (streamed tokens), L2 (inference-resilience sensor, `cybernetics_loop/cycle.rs:144` `sense_inference_resilience`), L6 (embeddings)
- **Prediction:** 2 / 2 / 0.50

### L6 — Corpus pipeline cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-corpus/src`
- **Entry point:** `main.rs` → `tools.rs` dispatch to `tools/{gather, document, corpus, tagging, semantic, compose_tools, calibration, storage}`; stages `convert.rs`, `ocr.rs`, `index.rs`, `compose.rs`, `batch.rs`, `backend/`, `services/`
- **Trigger:** per-tool requests chained by skills (convert → triage/OCR → chunk → tag → embed → prompts → QA → ground → ingest → assemble)
- **Hands off to:** L5 (embeddings/rerank), L10 (corpus DB), L18 (assembled training datasets)
- **Prediction:** 3 / 2 / 0.55

### L7 — Agent panel & kask widget update/render loops
- **Crate/path:** `crates/agent_ui/src/agent_panel.rs` (14,366 ln) + kask widget/panel crates
- **Entry point:** `agent_panel.rs` observe/subscribe web (`:374` `observe_new`, subscriptions at `:1491`, `:1501`, `:2201`, `:2780`, `:3093`, `:4662`; `render_title_view` `:5437`)
- **Participants:** widgets `crates/hkask-{kanban,swarm,portfolio,media,scenarios,spreadsheet,graph}-widget` + `hkask-viz-core` (D18); panels `crates/{swarm_panel,kanban_panel,media_panel,portfolio_panel}` (D33), `crates/hkask-steer`; compose-back seam D21; sibling host D23
- **Trigger:** GPUI entity events from threads/tasks; user interaction
- **Hands off to:** L1 (prompt submit), L9 (kanban widget ↔ server), L13 (swarm panel ↔ server)
- **Prediction:** 2 / 2 / 0.45

### L8 — Forecast/calibration loop
- **Crate/path:** `kask/crates/hkask-forecast` + `kask/mcp-servers/hkask-mcp-{companies,prediction-markets}`
- **Entry point:** `kask/crates/hkask-forecast/src/hkask_forecast.rs:190` `brier_score`, `:244` `wilson_bounds`, `:279` `apply_calibration_adjustment`, `:308` `isotonic_fit`; market leg: `hkask-mcp-prediction-markets/src/calibration.rs:134` `brier`, `hkask_mcp_prediction_markets.rs:211` `market_record_resolution`, `:524` `market_check_resolutions`; equity leg: `hkask-mcp-companies` `forecast_persist`/`forecast_record`/`calibrate_forecast` (surface header `hkask_mcp_companies.rs:18`)
- **Trigger:** forecast creation → outcome recording → calibration readback
- **Consolidation signal (Phase 3 candidate):** two Brier implementations — `hkask_forecast.rs:190` and `prediction-markets/src/calibration.rs:134`
- **Prediction:** 1 / 1 / 0.50

### L9 — Kanban/goal loop
- **Crate/path:** `kask/mcp-servers/hkask-mcp-kata-kanban/src`
- **Entry point:** `kanban/service_impl.rs` (+ `mermaid.rs`, `types.rs`), `pko.rs`, `idempotency.rs`; goal lifecycle create → judge → score → memory-acknowledge (state machine documented in `kask/docs/diagrams/kanban.md`)
- **Trigger:** agent/user MCP actions during work; panels (L7)
- **Hands off to:** L10 (resolved-goal outcome → curator memory), L7 (widget updates), L2 (goal intake predictions are Brier-scored at resolution)
- **Prediction:** 1 / 1 / 0.50

### L10 — Memory recall/ingest cycle
- **Crate/path:** `kask/crates/hkask-memory` + `kask/mcp-servers/hkask-mcp-curator`
- **Entry point:** `kask/crates/hkask-memory/src/memory_store.rs:288` `store`, `:331` `query_deduped`, `:447` `touch_recall`, `:261` `with_ledger`; consolidation `consolidation_service.rs:38`; curator ingest `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1739` `curator_memory_extract` (turn-discovery contract `thread_turns.rs`, cited at `:1557`, `:1730-1751`), `distillation.rs`, `forgetting.rs`
- **Participants:** `federated_recall.rs`, `recall_dedup.rs`, `salience.rs`, `bayesian.rs`; zed-side injection `kask_bridge/src/memory.rs`, `context_injector.rs`
- **Trigger:** turn-end extraction; recall queries; prune/decay cycles
- **Hands off to:** L1 (context injection), L2 (ledger), therapy/consolidation skills
- **Prediction:** 2 / 2 / 0.55

### L11 — Media job queue cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-media/src`
- **Entry point:** `jobs.rs:167` `admit`, `:210` `mark_running`, `:229` `cancel`, `:477` `finish`, `:304` `active_count`; tools `job_submit`/`job_status`/`job_list`/`job_cancel`
- **Trigger:** async generation job submit → poll → terminal
- **Hands off to:** L7 (media panel/gallery), transcript layer passes (`transcript_pass.rs`)
- **Prediction:** 1 / 1 / 0.45

### L12 — Research run ledger cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src`
- **Entry point:** `hkask_mcp_research.rs:1604` `begin_research_run`, `:1658` `annotate_research_run`; run state in `research/runs.rs`
- **Trigger:** research question → run opened → run-scoped search/extract recorded → evidence evaluation → annotation
- **Hands off to:** L4 (tool surface), L1 (agent-driven research), corpus/companies consumers of run ledgers
- **Prediction:** 1 / 1 / 0.45

### L13 — Swarm thread/memory cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-swarm/src` + `kask/crates/hkask-event-store`
- **Entry point:** `knowledge_tools.rs:67` `swarm_recall_local` (surface doc `hkask_mcp_swarm.rs:61`); event store `kask/crates/hkask-event-store/src/hkask_event_store.rs`; zed-side feed `kask_bridge/src/rollout_event_bridge.rs:104` `poll_once` (`crates/zed/src/main.rs:837`)
- **Trigger:** delegation dispatches; recall queries; ABW sync; task-board updates
- **Hands off to:** L2 (rollout events → regulation sensors), L7 (swarm panel), L10 (agent prefix-scoped memories in `MemoryStore`)
- **Prediction:** 1 / 1 / 0.45

### L14 — Settings → MCP server sync/restart cycle
- **Crate/path:** `kask/crates/kask_bridge/src`
- **Entry point:** `mcp_servers.rs` (runtime load/unload + nudge; surface span `kask_bridge/src/mcp_servers.rs:55-547` per `kask/docs/README.md:13`), `settings.rs` (`KaskSettings`, `crates/zed/src/main.rs:674`), `inference_socket.rs:24`; D45, D51
- **Trigger:** settings change, credential keychain write, `INFERENCE_SOCKET_PATH` change → server restart with updated env
- **Hands off to:** L3 (runtime respawns servers), L5 (env injection)
- **Prediction:** 1 / 2 / 0.50

### L15 — Passphrase rotation cycle
- **Crate/path:** `kask/crates/kask_bridge/src/passphrase_rotation.rs`
- **Entry point:** `:94` `schedule_db_passphrase_rotation`, `:167` `run_pending_db_passphrase_rotation`, `:212` `apply_db_rotation`; startup hook `crates/zed/src/main.rs:373`
- **Trigger:** operator rotation request → pending state on disk → applied at next launch (keychain slot written last)
- **Hands off to:** all SQLCipher DBs (`hkask-storage`, every MCP server DB), `hkask-keystore`
- **Prediction:** 1 / 1 / 0.50

### L16 — Skill activation → outcome → algedonic review cycle
- **Crate/path:** `crates/zed/src/main.rs` + `kask/crates/hkask-regulation` + `kask_bridge`
- **Entry point:** `crates/zed/src/main.rs:982-996` (`CuratorRegulationArchive` + `persist_skill_outcome`); regulation side `kask/crates/hkask-regulation/src/runtime.rs:776` `record_skill_span`, `:822` `record_outcome`, `:848` `check_outcome`, `:805` `skill_ids_with_feedback`; board `kask_bridge/src/algedonic_board.rs` (D59, D64, D79)
- **Trigger:** skill activation during turns; tool failures; operator algedonic-review sessions (`record_skill_feedback`)
- **Hands off to:** L2 (ledger/alerts), curator memory, skill-maintenance proposals
- **Prediction:** 2 / 1 / 0.50

### L17 — Scenario quantification/Brier loop
- **Crate/path:** `kask/mcp-servers/hkask-mcp-scenarios/src`
- **Entry point:** `hkask_mcp_scenarios.rs:1106` `scenario_quantify`, `:1227` `scenario_update`, `:1279` `scenario_score` (resolution Brier); Tetlock pipeline in `superforecast/`
- **Trigger:** scenario project events → quantification → Bayesian updates → outcome scoring
- **Hands off to:** L8 (shares calibration discipline), companies impact valuation
- **Prediction:** 1 / 1 / 0.45

### L18 — Training job cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-training/src`
- **Entry point:** `tools.rs` (submit/status/cancel tool surface), `hkask_mcp_training.rs`, `lora_validation.rs` (gate contracts), `adapters.rs`, `providers.rs`
- **Trigger:** operator-confirmed training submit → status polling → cancel/complete
- **Hands off to:** L6 (consumes assembled datasets), L16 (adapter evaluation feeds skill feedback)
- **Prediction:** 1 / 1 / 0.45

### L19 — Portfolio returns/review cycle (classification pending)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-portfolio/src`
- **Entry point:** `server.rs:892` `portfolio_seed_price`, `:986` `portfolio_materialize_returns`; `returns.rs`, `analysis.rs`, `store.rs`
- **Trigger:** ledger append → price seed → returns materialization → review (TWR/MWR, attribution)
- **Phase 1 classification:** request-driven compute pipeline vs canonical loop — decide on graph shape, do not assume
- **Prediction:** 1 / 1 / 0.45

## Boundary notes (sub-cycles folded into rows above, not separate rows)

- Email alert delivery — sensor leg inside L2 (`hkask-email`, `main.rs:656`).
- Conversation/context injection — leg inside L1 (`hkask-conversation-injector`, `kask_bridge/context_injector.rs`).
- Thread condensation — leg inside L1 (`hkask-condenser` via `condenser_bridge`).
- Polymarket resolution subscription — leg inside L8 (`market_subscribe_resolutions`, paired with `market_record_resolution` per `hkask_mcp_prediction_markets.rs:264-298`).
- Spreadsheet revision append cycle — request-driven; no persistent cycle identified at Phase 0; revisit if Phase 1 disagrees.
- Market health/`web_ping` style probes — legs inside L3/L4.

## Phase 0 predictions summary

Recorded per row above; totals: expected defects ≈ 32, expected impedances ≈
26, mean confidence ≈ 0.49. Scored against actual findings in Phase 4.

## Decomposition into audit slices (INVEST)

One slice = one register row through Phase 1 → 4 (map → detect → consolidate →
verify), sized so each slice lands or defers independently. Proposed batching:

1. **Batch A (early deletion candidates, known duplication signals):** L8
   (two Brier implementations), L3+L4 (client/server runtime pair), L14.
2. **Batch B (control core, highest connectivity):** L2, L5, L16.
3. **Batch C (large surfaces):** L1, L7, L6.
4. **Batch D (bounded server loops):** L9, L10, L11, L12, L13, L17, L18, L19.

Ordering rationale: bank consolidation wins on small, duplicated surfaces
first; audit the high-connectivity control core before the largest surfaces,
so impedances found there inform the big-surface audits.

## Open items for the operator (checkpoint)

1. **Docs-tree cap:** `find kask/docs -name '*.md' | wc -l` = 71 before this
   register (README's verification gate already records 72 as above the
   fewer-than-70 cap, and the README's 2026-09-24 corpus note says 69 — count
   drift is itself a doc-hygiene finding). This register is +1,
   operator-mandated. Decide: accept +1, or name a doc to fold so Phase 3/4
   restores the count.
2. **Slice order:** Batch A→D above is the default; reorder on functional
   priority if you want specific loops audited first.

## Change log

- 2026-09-27 — v0.1.0 Phase 0 inventory, 19 rows, all spec families verified
  present, predictions recorded. Operator approval pending before Phase 1.
