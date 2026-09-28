---
title: "Loop Register — zed-kask canonical loops"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-27
version: "0.3.0"
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

### L3 — MCP client runtime: spawn / health-supervise / request cycle — AUDITED & CLOSED 2026-09-27
- **Crate/path:** `kask/crates/hkask-mcp/src/runtime.rs` (2,676 ln; 22 in-file tests + `tests/reconnect_integration.rs`)
- **Entry point:** `spawn_health_supervisor` `runtime.rs:1041`; on-demand cycle `call_tool_inner` `:1615` → `dispatch` `:1674`; reconnect `try_reconnect` `:1268` → `restart_on_runtime` `:1317`; spawn `start_server_with_env` `:615` (liveness-based idempotency `:609-613`) → `start_recorded` `:643`
- **Participants:** `McpRuntime` state (`:462-491`: connections, cancellation tokens, launch specs, reconnect cooldown, health-failure counters); generation-stamped keeper task (reap-on-death `:860-881`, generation guard `:429-430`); zed-side registry `kask_bridge/src/mcp_servers.rs` (`BuiltinMcpServer`, `build_mcp_server_env` `:681` — the L14 boundary); `BUILT_IN_MCP_SERVERS` (`crates/zed/src/main.rs:606`); 12 child servers
- **Trigger:** agent tool invocation via `ToolPort::invoke` (`:1477`); periodic health tick (`:1050-1057`); explicit start/stop (`:615`/`:1403`)
- **Functional graph (Phase 1, IS-cited per node):** invoke (`:1477`) → governance charge + runaway-loop breaker (`:1498-1536`, the one pre-dispatch refusal; auto-registration instead of denial `:1509-1517`) → `call_tool_inner` (`:1615`): live-peer check → `try_reconnect` (cooldown check + stamp under one write lock, `:1273-1288`) → `dispatch` (`:1674`): three-way failure classification — `NotDelivered` (provably not run; reconnect and retry once `:1641-1657`), `Interrupted` (effect unknown, never auto-retried `:1708-1722`; `DispatchError` `:1779-1805`), `Failed` (`:1723`); call timeout inside `TokioContext` (`:1698-1705`); post-call span emit (`:1542-1554`), per-server reliability `record_outcome` + variety `record_variety` (`:1574-1585`). Parallel supervision cycle (`:1041-1209`): interval tick → classify Healthy/TransportClosed/Missing (`:1063-1070`) → reset or increment failures, saturating (`:1116-1121`) → remove a dead entry only if still closed (`:1092-1101`) → restart via recorded spec, concurrent-safe with the call path (`:1126-1128`) → circuit breaker stops the respawn loop with an operator-actionable error (`:1155-1177`, the 2026-08-29 crash-loop fix); deliberately stopped servers are never resurrected (`:1133-1142`).
- **Findings (Phase 2, adjudicated):** **F1 impedance, DEFERRED** — the typed error kind crosses the L4→L3 seam string-marshalled: `dispatch` formats `[kind] text` (`:1732-1738`) and governance re-parses it (`:1572`) via shared `error_kind_from_display` (`hkask-types/src/tool_response.rs:123`); both ends single-copy and tested. Typed carry through `ToolPortError` would add a field plus construction and matches against a pinned display contract — net-positive lines — so deferred. **F2 IS, no action** — dual reapers (keeper `:860-881`, supervisor removal `:1092-1101`) are both load-bearing (event-driven reap vs poll-window closer) and cannot race destructively (generation stamp `:861-864`, liveness re-check `:1095-1098`). **F3 IS, no action** — dual rate-limiters (call-path cooldown `:1276-1288`, supervisor interval + circuit breaker) bound different triggers. **F4 OUGHT, out of scope** — no ping-based health check; a hung-but-alive server reads Healthy (`:1029-1030`, documented future enhancement; adding it is a new feature and fails the admission test).
- **Five properties:** closed — IS (spawn → supervise → reap → reconnect → circuit-break; transitions pinned by the 22 in-file tests and `reconnect_integration.rs`); timely — IS (call timeout, cooldown, interval, breaker all bounded); accurate — IS (three-way delivery classification, unknown-effect never retried, typed-kind ledger breakdown); complete — IS (12 servers; tool-surface membership is event-driven, `:574-585`); actionable — IS (the breaker's error names the operator action `:1167-1175`; `unavailable_error` distinguishes NotFound / never-started / not-connected `:1746-1773`).
- **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects, 1 impedance deferred with reason (F1). Brier-scored at Phase 4.
- **Hands off to:** L4 (server side of each call), L2 (record_outcome/record_variety + spans), L14 (zed-side registry and env).

### L4 — MCP server request cycle (shared framework, 12 servers) — AUDITED & CLOSED 2026-09-27
- **Crate/path:** `kask/crates/hkask-mcp-server/src/server/` (transport 131, error 163, validation 611, credentials 144, context 163, tool_span 170) + `kask/mcp-servers/*`
- **Entry point:** `server/transport.rs:32` `run_stdio_server` — the single shared bootstrap: tracing init → DB catalog (`:52-68`, startup refused on inventory failure) → credential resolution, required and optional each surfaced (`:72-91`) → WebID (`:93-105`) → capability tier (`:108`) → factory-gated construction (`:119`, no ambient env authority) → rmcp stdio serve (`:125-129`)
- **Participants:** shared `execute_tool` (ONE definition, `server/tool_span.rs:162`, used by all 12 servers); shared envelope `hkask_types::tool_response` (`unwrap_tool_envelope`, `parse_tool_error_value` `:100`); typed error taxonomy (`server/error.rs:16` `McpError` per-variant; `McpToolError` carrying `kind: McpErrorKind`); shared input validation (`validation.rs`: identifier/path validation, per-source error mapping `:82-139`, path containment `:302-375`, capped reads `:590`); 12 thin binary mains (9 lines each, e.g. `hkask-mcp-companies/src/main.rs` — library servers for fuzz testability)
- **Trigger:** stdio JSON-RPC request from the zed host
- **Functional graph (Phase 1, IS-cited per node):** host spawn (L3 `start_server_with_env`, env built by `build_mcp_server_env` `kask_bridge/src/mcp_servers.rs:681`) → bootstrap (`transport.rs:32`) → rmcp dispatch → `execute_tool` (`tool_span.rs:162`, span emission) → tool fn → `{"content": ...}` envelope or typed `McpToolError` with `structured_content` kind (consumed by L3's `dispatch` `:1732-1738`).
- **Findings (Phase 2, adjudicated):** framework single-copy verified — envelope, error taxonomy, span emission, validation, and bootstrap each have ONE implementation; no per-server duplication found. **Considered and rejected:** merging the 12 binary mains into one multi-server binary would delete ~99 lines but breaks per-server process isolation — per-server credential env allowlists and crash domains are functional requirements (`.rules` MCP server patterns), so behavior preservation rejects it.
- **Five properties:** closed — IS (request → validate → execute → envelope → span; L3's `record_outcome` closes the loop at the governance layer); timely — IS (stdio, no polling); accurate — IS (typed per-variant errors; startup refuses on inventory failure rather than degrading); complete — IS (12 servers, one framework); actionable — IS (named missing credentials `:87-91`; per-variant `McpError` context).
- **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects, 0 impedances (the F1 seam impedance is recorded on L3, its formatting side). Brier-scored at Phase 4.
- **Hands off to:** per-domain loops L6, L8–L13, L17–L19; L2 (tool spans/outcomes); L3 (the client side of every call).

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

### L8 — Forecast/calibration loop — AUDITED & CLOSED 2026-09-27
- **Crate/path:** `kask/crates/hkask-forecast` + `kask/mcp-servers/hkask-mcp-{companies,prediction-markets}`
- **Entry point:** `kask/crates/hkask-forecast/src/hkask_forecast.rs:190` `brier_score`, `:196` `brier_score_multi`, `:244` `wilson_bounds`, `:279` `apply_calibration_adjustment`, `:308` `isotonic_fit`; market leg: `hkask-mcp-prediction-markets/src/calibration.rs:134` `brier` (delegates to the lib, `:13`/`:141`), `hkask_mcp_prediction_markets.rs:211` `market_record_resolution`, `:524` `market_check_resolutions`; equity leg: `hkask-mcp-companies` `forecast_persist`/`forecast_record` (`tools/valuation.rs:1101`/`:1219`, model `src/forecast.rs`), `calibrate_forecast` (`tools/valuation.rs:874`)
- **Trigger:** forecast creation → outcome recording → calibration readback
- **Functional graph (Phase 1, IS-cited per node):** snapshot arm (`market_check_resolutions` `hkask_mcp_prediction_markets.rs:524` → `CalibrationStore::record_pending` `calibration.rs:172`, earliest snapshot kept, test `:527`) → resolution arm (`market_record_resolution` `:211` → `record` `calibration.rs:125`; subscribe leg `:264-298` logs notifications, never fabricates observations) → scoring (`brier` `calibration.rs:134` → shared `hkask_forecast::brier_score_multi` `hkask_forecast.rs:196`) → readback (`market_calibration` `:187` → `read_calibration` `calibration.rs:313`; missing/empty bucket → `stale: true`, `brier: None`, never a synthetic 0, tests `:410-419`) → act (`reliability_tier` demotion on annotated lookups, `types.rs:251` wired `:433`→`:463`). Equity leg: `dcf_valuation`/`calibrate_forecast` → `forecast_persist` → `forecast_record` (Brier + decomposition at record); feedback application on the equity leg is agent-mediated (OUGHT — no automatic path applies equity calibration history to future priors; INFERRED from absence).
- **Findings (Phase 2, adjudicated):** **F1 REFUTED** — the Phase 0 "two Brier implementations" signal: `calibration.rs:13`/`:141` delegates to the shared lib and `companies/superforecast.rs:3-9` documents the no-pass-through layering; signal withdrawn. **F2 informational, kept as IS** — `calibration.rs:141` `map_err(|_| ())` collapses only unreachable `ForecastError` variants into the designed `stale: true` semantic (empty bucket pre-checked `:136-137`; length mismatch impossible — both vectors built from one iterator). **F3 verified** — the tier-demotion claim is enforced (`types.rs:251`/`:433`/`:463`): the market loop is CLOSED. **F4 CONSOLIDATED** — `scenarios/superforecast/math.rs:35` `brier_score_multi` was a pure `ForecastError`→`ScenarioError` wrapper while the same module re-exports `brier_score` directly from the lib (`superforecast.rs:16`); the wrapper was deleted and the lib function re-exported (`ScenarioError` carries `#[from] ForecastError`, `types.rs:45`), keeping the `superforecast::brier_score_multi` path stable for callers.
- **Five properties:** closed — IS (market leg), agent-mediated OUGHT (equity leg); timely — IS (staleness surfaced; scan cadence operator-driven, `zero_scan_reason` on empty scans); accurate — IS (earliest-snapshot discipline `calibration.rs:168-177`, identity-based dedup `:144-162`, no-fabrication contracts, tested); complete — IS with stated boundary (equity and market observations use separate stores by reference class); actionable — IS (tier demotion changes lookup annotations, `matcher.rs:7`).
- **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.50 → actual: 0 defects, 1 module-convention inconsistency consolidated (F4, net −13 lines), 1 Phase 0 signal refuted (F1). Brier-scored at Phase 4.

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

## Decomposition into audit slices (INVEST)

One slice = one register row through Phase 1 → 4 (map → detect → consolidate →
verify), sized so each slice lands or defers independently. Slice order is a
technical decision (program manager's per the Division of Responsibilities),
vetoable on functional grounds:

1. **Batch A (early deletion candidates, known duplication signals):** L8
   (closed 2026-09-27 — Brier signal refuted; real finding was the scenarios
   wrapper, consolidated), L3+L4 (closed 2026-09-27 — audited clean, no
   deletion candidate survives the test), L14.
2. **Batch B (control core, highest connectivity):** L2, L5, L16.
3. **Batch C (large surfaces):** L1, L7, L6.
4. **Batch D (bounded server loops):** L9, L10, L11, L12, L13, L17, L18, L19.

Rationale: bank consolidation wins on small, duplicated surfaces first; audit
the high-connectivity control core before the largest surfaces, so impedances
found there inform the big-surface audits. A functional priority (a loop whose
behavior matters most to the operator) overrides this order on request.

## Working rules

- Graphs and findings live in register rows and the final report — no
  per-loop documents are created in any phase.
- Operator checkpoints carry only functional, blocking decisions; technical
  decisions arrive as recorded decisions with veto rights; neighboring
  systems' bookkeeping (e.g., docs-tree governance) stays out of the audit.

## Change log

- 2026-09-27 — v0.3.0 L3+L4 audited and closed clean: the runtime pair is
  already the deep module (single shared framework; generation-stamped keeper
  + supervisor + cooldown; three-way dispatch classification; circuit
  breaker). No deletion candidate survives the test — the one impedance
  (string-marshalled error kind across the L4→L3 seam) is deferred with
  reason, and the multi-server-binary merge is rejected on isolation
  grounds. Doc-only slice; no production lines changed (running production
  total remains −13 from L8's `50cba394fd`).
- 2026-09-27 — v0.2.0 L8 audited and closed: Phase 0 Brier-duplication signal
  refuted (delegation, not duplication); tier-demotion act arm verified
  (`types.rs:251`/`:433`/`:463`); scenarios' pure `brier_score_multi` wrapper
  deleted and re-exported from `hkask-forecast` (net −13 lines); 25 scenarios
  tests green via `cargo-test-nonzero`, `./script/clippy -p hkask-mcp-scenarios`
  green, machete clean, 12/12 `brier_score_multi` sweep references legitimate.
  The consolidation and this register update land in one pathspec-limited
  commit; the hash is cited in the audit session report and at Phase 4.
- 2026-09-27 — v0.1.1 checkpoint correction after operator review: removed
  the docs-count and slice-order operator questions (docs are out of audit
  scope; slice order decided and recorded), removed the redundant
  predictions-summary section, added the working rules. Correction: this
  edit was reported to the operator but not actually applied at that time —
  commit `3f7175bb26` carries the uncorrected register; it lands here, in the
  same commit as the L8 closure.
- 2026-09-27 — v0.1.0 Phase 0 inventory, 19 rows, all spec families verified
  present, predictions recorded. Operator approval pending before Phase 1.
