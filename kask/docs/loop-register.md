---
title: "Loop Register — zed-kask canonical loops"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-27
version: "0.10.0"
status: "Phase 1–4 partial: L3 validated; L5 Json error fixed, other loops open"
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
from memory). This Phase 0 refresh also checked all 19 core-crate and 12 MCP
server package directories against the register and sampled the zed-side
integration points; it does not claim an exhaustive call-graph proof. A row is
separate when it has its own trigger, retained state and observable return path;
a straight-through request or a stage of another cycle stays folded into that
row. Row detail is entry-point-level for Phase 0; Phase 1 deepens each unaudited
row to a full functional graph with one IS citation per node. Earlier commits
already executed Phase 1–3 on L3, L4, L8 and L14; this refresh neither
retroactively authorizes those commits. The operator subsequently approved proceeding;
each further slice still has its own graph and validation gate.

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
| Extensions found in tree | recorded | L11–L23 |

No expected family was absent; the tree shows additional loop families beyond
the spec's minimum list, recorded below rather than narrowed away.

## Register

### L1 — Agent turn loop
- **Crate/path:** `crates/agent` (zed-side, D-seamed), participants in `crates/agent/src/`
- **Entry point:** `crates/agent/src/thread.rs:3178` `run_turn_internal`; spawned via `thread.rs:2984` / `agent.rs:2472` `run_turn`; submissions at `agent.rs:3615`, `:3754`, `:3900`
- **Participants:** `agent.rs` (10,419 ln), `thread.rs` (12,970 ln), `tools.rs`, `tool_retry_tracker.rs`, `tool_trace.rs`, `tool_permissions.rs`, `sandboxing.rs`, `kask_compaction.rs`, `kask_thread_state.rs`, `templates.rs`; `crates/hkask-conversation-injector`, `crates/hkask-tool-invoker`; condensation via `kask_bridge::condenser_bridge` → `kask/crates/hkask-condenser/src/engine.rs`
- **Trigger:** user prompt submit from the agent panel; tool-result continuation within a turn
- **Hands off to:** L3 (tool dispatch), L5 (model streaming), L2 (skill spans/outcomes), L10 (turn-end memory ingest), L7 (thread events → panel), L12 (research-run sources from web tools)
- **Prediction:** 3 / 2 / 0.50
- **Phase 1 scoped graph (IS):** pending message/tools sensed in `crates/agent/src/thread.rs:2984-3006` → request/context assembled (`:3280-3334`) → streamed tool/refusal/truncation interpreted (`:4037-4115`) → tool dispatched (`:4371-4426`) → result marked completed/failed for the next round (`:3955-3980`), or the turn ends (`:3541-3595`). L3 takes MCP dispatch via `tools/context_server_registry.rs:705-732`; L7 observes thread events via `crates/agent_ui/src/conversation_view.rs:1475-1477`; L10 turn ingestion follows asynchronously (`thread.rs:3019-3086`). The ordinary model turn calls `stream_completion` directly (`thread.rs:3349-3354`): the L5 IPC handoff listed above is **not** claimed for that particular route. Five properties in this bounded scope: closed for tool-result continuation; timely conditional on retries and detached ingestion; accurate for stored tool status; complete only for the inspected path; actionable for tool errors, while a memory-write failure is log-only.
- **Phase 2 observation (IS, deferral):** `EndTurn` precedes detached memory ingestion (`thread.rs:3022-3086`), and the panel stop handler has no memory receipt (`conversation_view.rs:1802-1815`). INFERRED: a completed turn does not guarantee later recall. Falsifier: force ingestion failure and observe a distinct memory-success acknowledgement on the completed turn. A synchronous-ingest change would alter turn latency and fails behavior preservation; defer until a functional guarantee is specified. No deletion candidate admitted.

### L2 — Regulation/curator cybernetic cycle — prior minimalism pass closed; error-path diagnosis open
- **Crate/path:** `kask/crates/hkask-regulation` (runtime.rs, cybernetics_loop.rs, cybernetics_loop/cycle.rs, metacognition.rs, set_points.rs, energy.rs, dampener.rs, sensor_provider.rs)
- **Entry point:** `kask/crates/hkask-regulation/src/metacognition.rs:346` `run` / `:405` `tick`; cycle stages at `cybernetics_loop/cycle.rs:305` `sense`, `:409` `compute`, `:450` `act`, `:723` `verify_impact`; facade `cybernetics_loop.rs:259` `CyberneticsLoop::new`
- **Participants:** `RegulationLedger` (constructed `crates/zed/src/main.rs:790`), set points (`main.rs:773-784`), alert channel (`main.rs:632-641`), directive inbox (`main.rs:796-803`), email sink (`main.rs:656`, `kask/crates/hkask-email`); zed-side sensor bridges: `kask_bridge/src/context_server_health_bridge.rs:39`, `ocr_health_bridge.rs:36`, `rollout_event_bridge.rs:104` `poll_once`, `algedonic_log_bridge.rs`, `inference_resilience.rs`, `metacognition_bridge.rs`, `directive_bridge.rs`; wired on the kask tokio runtime (`crates/zed/src/main.rs:579-593`)
- **Trigger:** composition-root tick drivers gated on `kask.curator.always_on` (`main.rs:1225-1243`, D8/F10): CyberneticsLoop @10s (`:1227-1236`), MetacognitionLoop @30s (`:1239-1242`, self-interval `metacognition.rs:346-354`), harness-regression monitor @60s (`:1256-1300`, backpressure/retry semantics `:1272-1296`); alert channel (`main.rs:632-641`); `curator_directive` tool → `process_inbox` (`cybernetics_loop.rs:648-663`)
- **Functional graph (Phase 1, IS-cited per node):** CyberneticsLoop::tick (`cybernetics_loop.rs:756-870`): sense (sensor providers + observations cache) → escalation-sink reconcile (`:766-770`) → compare → compute (advisories) → act (`cycle.rs:450-562`: E04 cap-exhaustion captured BEFORE the per-tick reset `:479-497`, `reset_all_caps`, alert fan-out) → `route_action_as_alert` (`:568+`: board + live channel + archive fallback + email, dedup latches, retention authority `:450-477`) → `verify_impact` (evidence-bearing submitted checks only) → strategy evaluator → `ledger.record_cycle_outcome` (`:853`) → loop-quality telemetry (fingerprint suppression, hourly heartbeat, `:667-749`). MetacognitionLoop::tick (`metacognition.rs:405-435`): ledger + regulation health + skill-feedback drift sense (`:447+`) → compare (`:535`) → act (`:592`, drains the CyberneticsLoop alert channel at `:659`) → snapshot surfaced via `curator_status`.
- **Findings (Phase 2, adjudicated):** **F1 IS, no action** — the two-loop split is the minimal shape: two required cadences (10s actuation vs 30s observability) and a one-way channel decoupling failure domains; merging couples them (a slow drift pass would delay cap-exhaustion escalation) — merge REJECTED on behavior grounds (also pinned by D8/F3/F10). **F2 IS, no action** — sensor no-data discipline enforced and documented (`sensor_provider.rs:138`/`:158`, the `unwrap_or(0)` trap named and avoided); the `dampener.rs:319`/`extrapolation.rs:55` hits are computation guards with local invariants, not sensor reads. **F3 IS, verified** — `always_on` has a real enforcement point (`main.rs:1226`). **F4 IS, informational** — the tick loops carry no cancellation tokens; process-lifetime loops owning no child processes (contrast: the MCP runtime's lifecycle latch exists for child-process death, L3). **F5 IS** — harness-monitor degradation surfaced (`Backpressured`/`Err` logged). **Bridge fleet examined:** each bridge implements a distinct hkask-regulation trait across a documented GPUI/tokio boundary; a shared snapshot-cell generic over the two health bridges adds indirection and saves ~20-40 lines — FAILS the admission test, rejected.
- **Five properties:** closed — IS (sense → compare → compute → act → verify_impact → ledger → re-sense, plus the alert channel closing the operator loop through `curator_status`); timely — IS (bounded cadences; E04 pre-reset ordering documented); accurate — IS (fingerprinted telemetry, no-data-not-zero sensors); complete — IS (context-server, OCR, inference-resilience, memory, rollout/harness, skill-feedback sensors); actionable — IS (four-way fan-out with retention authority; every channel has a named consumer).
- **Prediction vs actual:** predicted 2 defects / 2 impedances / conf 0.55 → actual: 0 defects, 0 impedances, 2 candidates examined and rejected with reasons — third consecutive overestimate on incident-hardened surface, a Phase 4 calibration finding. Brier-scored at Phase 4.
- **Status:** prior doc-only minimalism pass closed (running committed production net at this point: −13 + 38 = +25); **error-path review reopened pending diagnosis**. IS: `cybernetics_loop.rs:777-788` drains submitted rollout checks before `verify_impact`; `cybernetics_loop/cycle.rs:748-772` skips a report on a store-query error. INFERRED: absent a resubmission path, a transient error may lose the assessment. Falsifier: force a store-query error and observe the same check retried or reported on the next tick. This is not yet a reproduced defect. Compilation is restored; the specific error-path regression test has not been run.

### L3 — MCP client runtime: spawn / health-supervise / request cycle — consolidated; current-tree tests and build passed
- **Crate/path:** `kask/crates/hkask-mcp/src/runtime.rs` (21 library tests + 16 serialized `tests/reconnect_integration.rs` tests observed at `ff3bae88e6`)
- **Entry point (current tree):** `runtime.rs:1505` `ToolPort::invoke` → `:1642` `call_tool_inner` → `:1701` `dispatch`; `:1282` `try_reconnect`; `:633` `start_server_with_env` → `:658` `start_recorded`; `:1050` `spawn_health_supervisor`; `:1428` `stop_server`
- **Participants:** `McpRuntime` per-server `ServerEntry` (`runtime.rs:466-481`) under one `entries` map (`:490`), generation-stamped keeper task, zed-side registry `kask_bridge/src/mcp_servers.rs` (L14 boundary), 12 child servers
- **Trigger:** agent tool invocation (`runtime.rs:1505`); periodic health tick (`:1050-1064`); explicit start/stop (`:633`/`:1428`)
- **Functional graph (Phase 1, IS-cited per node):** invoke (`:1477`) → governance charge + runaway-loop breaker (`:1498-1536`, the one pre-dispatch refusal; auto-registration instead of denial `:1509-1517`) → `call_tool_inner` (`:1615`): live-peer check → `try_reconnect` (cooldown check + stamp under one write lock, `:1273-1288`) → `dispatch` (`:1674`): three-way failure classification — `NotDelivered` (provably not run; reconnect and retry once `:1641-1657`), `Interrupted` (effect unknown, never auto-retried `:1708-1722`; `DispatchError` `:1779-1805`), `Failed` (`:1723`); call timeout inside `TokioContext` (`:1698-1705`); post-call span emit (`:1542-1554`), per-server reliability `record_outcome` + variety `record_variety` (`:1574-1585`). Parallel supervision cycle (`:1041-1209`): interval tick → classify Healthy/TransportClosed/Missing (`:1063-1070`) → reset or increment failures, saturating (`:1116-1121`) → remove a dead entry only if still closed (`:1092-1101`) → restart via recorded spec, concurrent-safe with the call path (`:1126-1128`) → circuit breaker stops the respawn loop with an operator-actionable error (`:1155-1177`, the 2026-08-29 crash-loop fix); deliberately stopped servers are never resurrected (`:1133-1142`).
- **Findings (Phase 2, adjudicated):** **F1 impedance, DEFERRED** — the typed error kind crosses the L4→L3 seam string-marshalled: `dispatch` formats `[kind] text` (`:1732-1738`) and governance re-parses it (`:1572`) via shared `error_kind_from_display` (`hkask-types/src/tool_response.rs:123`); both ends single-copy and tested. Typed carry through `ToolPortError` would add a field plus construction and matches against a pinned display contract — net-positive lines — so deferred. **F2 IS, no action** — dual reapers (keeper `:860-881`, supervisor removal `:1092-1101`) are both load-bearing (event-driven reap vs poll-window closer) and cannot race destructively (generation stamp `:861-864`, liveness re-check `:1095-1098`). **F3 IS, no action** — dual rate-limiters (call-path cooldown `:1276-1288`, supervisor interval + circuit breaker) bound different triggers. **F4 OUGHT, out of scope** — no ping-based health check; a hung-but-alive server reads Healthy (`:1029-1030`, documented future enhancement; adding it is a new feature and fails the admission test).
- **Five properties:** closed — IS (spawn → supervise → reap → reconnect → circuit-break; transitions pinned by the 22 in-file tests and `reconnect_integration.rs`); timely — IS (call timeout, cooldown, interval, breaker all bounded); accurate — IS (three-way delivery classification, unknown-effect never retried, typed-kind ledger breakdown); complete — IS (12 servers; tool-surface membership is event-driven, `:574-585`); actionable — IS (the breaker's error names the operator action `:1167-1175`; `unavailable_error` distinguishes NotFound / never-started / not-connected `:1746-1773`).
- **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects, 1 impedance deferred with reason (F1). Brier-scored at Phase 4.
- **Hands off to:** L4 (server side of each call), L2 (record_outcome/record_variety + spans), L14 (zed-side registry and env).
- **Current-tree supersession of the historical line numbers above:** commit `16271e3c60` replaced the six per-server maps with `ServerEntry` (`runtime.rs:466-481`), adding 264 and removing 226 production lines (**net +38**, contrary to the Phase 0 estimate of −150–250). The old graph and F1–F4 citations above describe the pre-consolidation tree. In the current tree: invoke meters then emits settled span and ledger outcome (`:1505-1624`); call path checks live peer, reconnects with cooldown, and retries only `NotDelivered` (`:1642-1686`); dispatch classifies unknown-effect `Interrupted` without replay (`:1701-1750`); supervisor senses closed/missing transport, increments failure count, attempts restart or circuit-breaks (`:1050-1217`); stop clears entry and cancels supervisor/children (`:1428-1456`). These nodes form the return path from tool-call outcome and next health tick to renewed dispatch. **Verified in the current worktree at `ff3bae88e6`:** `bash kask/scripts/cargo-test-nonzero.sh -p hkask-mcp --lib` (21 passed), `bash kask/scripts/cargo-test-nonzero.sh -p hkask-mcp --features test-fixture --test reconnect_integration -- --test-threads=1` (16 passed), `./script/clippy` (completed including kask-scoped machete and buf checks), `cargo check -p zed` (passed). An earlier integration invocation without serialized threads failed; it did not follow the test file's required protocol (`tests/reconnect_integration.rs:20-33`) and is not counted as a regression. The deferred L4→L3 typed-error impedance still requires a current-line recheck; no further L3 deletion admitted.

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

### L5 — Inference bridge (zed ↔ hkask IPC) — Json error-path fixed locally; Api readback deferred
- **Crate/path:** `kask/crates/kask_bridge/src/inference_*.rs` + `kask/crates/hkask-inference`
- **Entry point:** `kask_bridge/src/inference_ipc_server.rs:388` `UnixListener::bind` (2,630 ln); chat surface `inference_chat.rs` (2,001 ln); socket state `inference_socket.rs:24` `set_inference_socket_path` (env-injected into MCP servers via `mcp_env.rs`, `crates/zed/src/main.rs:204-209`, `:674`); client `kask/crates/hkask-inference/src/inference_ipc_client.rs`
- **Participants:** providers (`hkask-inference/src/provider.rs`, `openai_compat.rs`), rerank, media router; zed `LanguageModelRegistry` (D24)
- **Trigger:** chat/embedding/rerank/tool-dispatch requests from MCP server children and zed-side providers
- **Functional graph (Phase 1, IS-cited per node):** MCP child → client (`inference_ipc_client.rs`, constructs no error payloads — one `From` conversion `:987`) → Unix socket with peer-uid ownership gate (`inference_ipc_server.rs:302`), private socket dir (`:270`), `CappedReader` line cap (`:198`) → `handle_connection` `:599` (one in-flight per connection; EOF during dispatch cancels provider work `:658-669`; peer-cancellation EPIPE classified debug-not-warn `:684-717`) → `dispatch` `:731` — per method: Embed → embedding port; ListModels / CreateWorktreeThread → GPUI-side channel round-trips (`AsyncApp` not `Send`); ToolDefinition / ToolInvoke → ToolPort with the request allowlist as the REAL authority boundary (the DelegationToken self-comparison fix documented `:899-910`, fail-closed missing-allowlist); Rerank → API key via keychain channel, MCP servers never see keys (`:1090-1143`); Generate/GenerateWithModel/GenerateWithMessages/GenerateVision → `InferencePort` fall-through with a DoS-safe unreachable arm (`:1207-1232`) → newline-JSON `InferenceOutcome` back. Port side: `inference_chat.rs` — `LanguageModelInferencePort` (`:426`, `InferencePort` impl `:889`), `NoModelInferencePort` boot-grace stub (`:1172`), request lifetime/deadline + in-flight guards, `InferenceResilienceSource` (`:1092`, feeds L2). Protocol types shared in `hkask-types/src/inference_ipc.rs` — no client/server duplication.
- **Findings (Phase 2, adjudicated):** **F1 CONSOLIDATED (`4eaca76874`)** — 29 hand-built `InferenceOutcome::Error` payload constructions (5-6 lines each) collapsed into a private `ipc_error(code, message)` helper; net **−95 production lines** (+114/−209); behavior byte-identical — the module's 33 tests green (including the authority tests pinning the payloads), `cargo check` + `./script/clippy -p kask_bridge` green (validated in a detached worktree during that prior pass). The 2 payload-only constructions in `WorktreeSpawnRequest::execute` (`:84/:88`) left: different shape, already compact; a second constructor for 2 sites fails the admission test. **F2 IS, no action** — the four Generate* arms map 1:1 to distinct port methods. **F3 IS, no action** — protocol shared in hkask-types; client never constructs payloads. **F4 IS** — the security layer (peer-uid, private dir, line cap, EOF-cancel, EPIPE classification) is incident-documented; no findings.
- **Deferred with reason:** `inference_chat.rs` internal minimalism (2,001 ln — StreamAccumulator, deadline/guard machinery, error classification) deferred to the Batch C window where the big GPUI/LLM surfaces (L1, L7) get dedicated passes; the request-cycle audit above landed this loop's consolidation.
- **Five properties:** closed — IS (request → dispatch → port → outcome → client; the L2 resilience sensor closes the observation arm); timely — IS (request deadlines, EOF-cancel); accurate — partial IS (Json class now preserved, Api status readback still flattened; EPIPE classified); complete — IS (10 methods, all dispatched); actionable — IS (codes name the failure class; permission-denied messages name the setting to fix).
- **Prediction vs observed follow-up:** prior pass scored 0 defects / 0 impedances but missed the embedding readback: IS — server emits `Json` (`inference_ipc_server.rs:772-793`) and the client formerly mapped it to `Connection` (`inference_ipc_client.rs:483-488` before this edit). At the approved IPC seam, `embedding_ipc_preserves_json_error_class` failed before and passed after the client maps `Json` to `EmbeddingGenerationError::Json` (`inference_ipc_client.rs:483-487`). The existing `InvalidRequest` and fallback behavior remain represented in the same match; `hkask-inference --lib` ran 54/54 tests. The source change is **uncommitted**, net −1 production line (+5/−6), with 25 test lines added. **Remaining impedance deferred:** `Api` errors are sent as a status-bearing string (`inference_ipc_server.rs:778-780`) but still read as `Connection` (`inference_ipc_client.rs:486`); structured status recovery needs a separately agreed protocol/test seam and may add lines rather than delete them. Risk: a provider API failure can be misclassified as retryable. Falsifier for a future slice: an IPC test sends a server-shaped `Api` response and observes `EmbeddingGenerationError::Api` with the original status. Re-score the Phase 0 prediction only when the row closes.
- **Hands off to:** L1 (streamed tokens), L2 (inference-resilience sensor, `cybernetics_loop/cycle.rs:144` `sense_inference_resilience`), L6 (embeddings)

### L6 — Corpus pipeline cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-corpus/src`
- **Entry point:** `tools/document.rs:35` `corpus_convert`, `:355` `corpus_chunk`; `tools/tagging/ops.rs:263` `corpus_tag_chunks`; `tools/semantic.rs:228` `corpus_embed`, `:170` `corpus_generate_qa_batch`; `tools/corpus.rs:447` `corpus_ground_generated_qa`, `:174` `corpus_ingest_qa`
- **Trigger:** per-tool requests chained by skills (convert → triage/OCR → chunk → tag → embed → prompts → QA → ground → ingest → assemble)
- **Hands off to:** L5 (embeddings/rerank), L10 (corpus DB), L18 (assembled training datasets)
- **Prediction:** 3 / 2 / 0.55
- **Phase 1 scoped graph (IS):** source extraction/chunking (`tools/document.rs:35-72,355-419`) → model classification (`tools/tagging/ops.rs:263-310`) → embedding (`tools/semantic.rs:228-250`, L5) → prepared prompts (`services/prompt_builder.rs:45-91`) → generation (`services/qa_pipeline.rs:1216-1277`) → grounding (`services/qa_grounding.rs:223-269`) → ingestion (`tools/corpus.rs:174-247`, L10 corpus DB) → explicit corpus-DB selection for training assembly (`hkask-mcp-training/src/tools/dataset.rs:86-129`, L18). The skill drives decisions and reconciles results; this is not one automatic server cycle. Five properties: closed conditional on caller reconciliation/retrieval; timely conditional on bounded waves; accurate only at the mechanical-citation gate; complete only after every source/stage count reconciles; actionable through surfaced failures and stop rules.
- **Phase 2 seam and process correction (IS):** generation can output `status="skipped"` (`services/qa_pipeline.rs:1252-1277`), while `read_grounding_candidates` rejects any skip-or-error row (`services/qa_grounding.rs:223-269`); the old skill Stage 9 passed the mixed generated file directly. Falsifier: show a mixed file accepted by the grounding gate or a prior candidate-only projection. The existing `build-corpus-pipeline` skill now replaces that handoff with a candidate-only projection filtering **only** reconciled skips, retaining the original file and reconciling counts/hashes before grounding and ingestion; no server contract or additional script was introduced. A synthetic `jq` probe kept candidate and error rows and excluded the skip; **no full end-to-end corpus run or next-stage tool invocation was performed**, so the capability is not verified and L6 remains open. Further addition is deferred until a caller-selected corpus exercises the chain. The candidate-only file is the one whose hash the grounding manifest binds.

### L7 — Agent panel & kask widget update/render loops
- **Crate/path:** `crates/agent_ui/src/agent_panel.rs` (14,366 ln) + kask widget/panel crates
- **Entry point:** `agent_panel.rs` observe/subscribe web (`:374` `observe_new`, subscriptions at `:1491`, `:1501`, `:2201`, `:2780`, `:3093`, `:4662`; `render_title_view` `:5437`)
- **Participants:** widgets `crates/hkask-{kanban,swarm,portfolio,media,scenarios,spreadsheet,graph}-widget` + `hkask-viz-core` (D18); panels `crates/{swarm_panel,kanban_panel,media_panel,portfolio_panel}` (D33), `crates/hkask-steer`; compose-back seam D21; sibling host D23
- **Trigger:** GPUI entity events from threads/tasks; user interaction
- **Hands off to:** L1 (prompt submit), L9 (kanban widget ↔ server), L13 (swarm panel ↔ server)
- **Prediction:** 2 / 2 / 0.45
- **Phase 1 scoped graph (IS):** `AcpThreadEvent::NewEntry` reaches `conversation_view.rs:1475-1477,1736-1738` → entry/view sync (`:1742-1755`) → active-view change notifies `agent_panel.rs:4662-4677` → render consumes view (`:6641-6648`). For a kanban task move: click stages intent (`crates/hkask-kanban-widget/src/view.rs:662-692`), confirmation dispatches (`:279-289`), `move_controller.rs:194-229` applies optimistic state and invokes L9 tool, then clears/rolls back and notifies (`:230-253`). L1 compose-back is a separate editor prefill (`view.rs:919-931`); no L13 refresh claim follows solely from a swarm badge. Five properties in these two paths: closed conditional on authoritative update; timely unmeasured; accurate conditional on server readback; complete not established for other panels/widgets; actionable via dispatch status/error.
- **Phase 2 bounded observations:** IS — `set_body` declines an incoming body while a task move is pending/in flight (`view.rs:173-203`); INFERRED — a concurrent authoritative update may remain unseen after completion. Falsifier: prove a fresh authoritative `set_body` is guaranteed after every completion. INFERRED — optimistic mutation without an immediate explicit notify (`move_controller.rs:215-229`) may delay visible feedback; falsifier: a GPUI rendered-frame check showing immediate repaint. No such runtime checks ran; no deletion candidate admitted and no zed-side edits made. Defer pending a measured panel seam test.

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
- **Entry point:** `hkask_mcp_kata_kanban.rs:461` `kanban_goal_create`, `:517` `kanban_goal_judge`, `:564` `kanban_goal_score`, `:605` `kanban_goal_memory_acknowledge`; `kanban/service_impl.rs` and `idempotency.rs` own the board/task side of the cycle
- **Trigger:** agent/user MCP actions during work; panels (L7)
- **Hands off to:** L10 (resolved-goal outcome → curator memory), L7 (widget updates), L2 (goal intake predictions are Brier-scored at resolution)
- **Prediction:** 1 / 1 / 0.50
- **Phase 1 graph (IS):** `kanban_goal_create` receives a user-owned target (`hkask_mcp_kata_kanban.rs:461-500`) → service stores criteria (`kanban/service_impl/goals.rs:59-107`) → `kanban_goal_judge` validates coverage and appends a verdict (`goals.rs:208-247`) → `goal_score` stores outcome/Brier as a retained outbox row (`goals.rs:264-297`) → turn-end curator ingestion/acknowledgment (`crates/agent/src/thread.rs:332-374`) → `kanban_goal_list` and score readback (`hkask_mcp_kata_kanban.rs:564-658`). Five properties: closed **conditional** on ingestion/ack; timely **partial** (no evidenced automatic retry after failed turn ingestion); accurate **partial**; complete **partial**; actionable **partial** (manual list/readback, no verified automatic recovery).
- **Phase 2 open findings:** IS — Steer tells agents goals are ephemeral (`crates/kanban_panel/src/kanban_panel.rs:380-384`), but the scored row persists until acknowledgement (`goals.rs:287-297`). **Falsifier:** show that `steer_system_prompt` is not used by live Steer sessions (`kanban_panel.rs:1034-1047` shows its construction). **Deferred:** fixing this zed-side instruction requires a D-seam update and panel-level pin in the same change; concurrent zed-side edits are in progress, so do not edit that seam piecemeal. IS — `goal_acknowledge_memory` checks owner/resolved status and prunes without confirming a memory receipt (`goals.rs:319-337`); INFERRED risk: direct acknowledgment could delete an un-ingested scored row. **Falsifier:** a server-enforced memory receipt gate or a test proving direct acknowledgment cannot prune before ingestion. Receipt enforcement needs a cross-server contract and fails the simple deletion test; defer for operator decision with the risk stated. IS — `Done` does not check `passed` values before append (`goals.rs:208-247`); **falsifier:** a rejection check on the service path. Do not count an unrun runtime test as a confirmed defect.

### L10 — Memory recall/ingest cycle
- **Crate/path:** `kask/crates/hkask-memory` + `kask/mcp-servers/hkask-mcp-curator`
- **Entry point:** `kask/crates/hkask-memory/src/memory_store.rs:288` `store`, `:331` `query_deduped`, `:447` `touch_recall`, `:261` `with_ledger`; consolidation `consolidation_service.rs:38`; curator ingest `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1739` `curator_memory_extract` (turn-discovery contract `thread_turns.rs`, cited at `:1557`, `:1730-1751`), `distillation.rs`, `forgetting.rs`
- **Participants:** `federated_recall.rs`, `recall_dedup.rs`, `salience.rs`, `bayesian.rs`; zed-side injection `kask_bridge/src/memory.rs`, `context_injector.rs`
- **Trigger:** turn-end extraction; recall queries; prune/decay cycles
- **Hands off to:** L1 (context injection), L2 (ledger), therapy/consolidation skills
- **Prediction:** 2 / 2 / 0.55
- **Phase 1 scoped graph (IS):** L1 turn completion hands a record to `RealMemoryPort::ingest_turn` (`crates/agent/src/thread.rs:3019-3086`; `kask_bridge/src/memory.rs:520-537`); `memory/ingest.rs:395-442,543-610` writes goal/chunk memories; `kask_bridge/src/memory.rs:795-1049` retrieves/ranks/touches curator memories; `context_injector.rs:292-348` injects them into a later curator turn. L9's scored-goal row is acknowledged only after the turn-ingestion call returns success (`thread.rs:351-373`). Five properties in inspected scope: closed for curator recall after successful ingest, timely conditional on detached turn task, accurate/complete conditional on store reads, actionable for curator via subsequent context or memory tools. Ordinary-agent recall returning empty is **intentional IS** (`memory.rs:540-559` and `:201-205`), not evidence that the curator cycle is broken.
- **Phase 2 open finding (IS + INFERRED consequence):** keyword recall discards a DB query error via `if let Ok` (`memory.rs:927-967`) and exact-thread recall does likewise (`:1067-1085`), so on these legs a failed store read can produce an empty candidate set rather than a surfaced read failure. **Falsifier:** inject a query failure and observe a distinct caller-visible error. Do not reclassify the deliberate empty-store fallback (`memory.rs:711-745`) without operator agreement. No test of a real store failure has run; changing the result/error contract without it would violate behavior preservation. Defer further L10 consolidation pending this test and an approved failure signal; no net-negative deletion candidate has been established.

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

### L14 — Settings → MCP server sync/restart cycle — AUDITED & CLOSED 2026-09-27 (minimal by design)
- **Crate/path:** `kask/crates/kask_bridge/src` + zed-side wiring (`crates/zed/src/main.rs`, `crates/settings_ui/src/pages/kask_page.rs`)
- **Entry point:** `sync_kask_mcp_runtime_servers` `crates/zed/src/main.rs:3585` (observer wired `main.rs:1455-1461`, D45; baseline + latch established `main.rs:1435-1453`); `nudge_mcp_servers` `kask_page.rs:342`; env assembly `build_mcp_server_env` `kask_bridge/src/mcp_servers.rs:681` (single canonical path; config half = `mcp_env.rs` emit_* translators); socket `inference_socket.rs:24`
- **Trigger:** settings change (`cx.observe_global::<SettingsStore>`); credential keychain write/delete → `nudge_mcp_servers` → `notify_observers` (`kask_page.rs:280/:307`, funnel doc `:324-341`, D32 interplay `:332-336`); launch pass sets baselines + latch
- **Functional graph (Phase 1, IS-cited per node):** notify → sync (`main.rs:3585`): load-state resolution, same expression as the launch loop (`:3593-3606`) → per-server env (`kask_server_env` `:3499`) → baseline diff → classify `to_stop` (`:3636`), `to_start` (latch-gated `:3662`), `to_restart` (env changed, changed keys named in log `:3637-3649`) → `Tokio::spawn` stop/start/restart (`:3677-3765`) with baseline bookkeeping (insert-not-expect `:3737-3745`) and retry-on-next-pass failure semantics (`:3716-3727`, `:3747-3761`; failed starts keep their launch spec so tool calls reconnect on demand).
- **Minimalism verdict (ideal-method pass):** minimal by design — event-driven single funnel (N triggers → 1 notify → 1 sync), env-diff restarts exactly the changed servers, no polling, racing observer passes collapse via runtime idempotency (`:3690-3694`). **Collapse of the launch/sync mirror REJECTED:** the launch pass owns startup ordering and the inference-socket existence window; the latch + empty-baseline semantics are behavior, not structure (`main.rs:1435-1461`, `:3653-3662`). Classification living in `main.rs` (untestable without gpui) is noted; moving it to `kask_bridge` is net-neutral lines, not a consolidation.
- **Prediction vs actual:** predicted 1 defect / 2 impedances / conf 0.50 → actual: 0 defects, 0 impedances. Brier-scored at Phase 4.
- **Hands off to:** L3 (runtime respawns servers), L5 (socket env injection).

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
- **Phase 1–2 read-only observation (IS + INFERRED impact, diagnosis pending):** `crates/agent/src/kask_thread_state.rs:62-84` retains the most recently activated skill on the thread; the observed write at `crates/agent/src/thread.rs:4448-4450` sets it on activation, and no turn-end reset was found in the `active_skill_handle` reference sweep. INFERRED: a later unrelated tool failure might be attributed to an earlier skill. **Falsifier:** a cross-turn test where the later failure is not recorded under the earlier skill. Do not count as a confirmed defect or consolidate before reproduction and a behavior-preserving contract; compilation is restored, but the cross-turn test has not run.

### L17 — Scenario quantification/Brier loop
- **Crate/path:** `kask/mcp-servers/hkask-mcp-scenarios/src`
- **Entry point:** `hkask_mcp_scenarios.rs:1106` `scenario_quantify`, `:1227` `scenario_update`, `:1279` `scenario_score` (resolution Brier); Tetlock pipeline in `superforecast/`
- **Trigger:** scenario project events → quantification → Bayesian updates → outcome scoring
- **Hands off to:** L8 (shares calibration discipline), companies impact valuation
- **Prediction:** 1 / 1 / 0.45

### L18 — Training job cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-training/src`
- **Entry point:** `tools/submit.rs:28` `training_submit`, `tools/status.rs:17` `training_status`, `tools/cancel.rs:13` `training_cancel`; `lora_validation.rs` gates the submission
- **Trigger:** operator-confirmed training submit → status polling → cancel/complete
- **Hands off to:** L6 (consumes assembled datasets), L16 (adapter evaluation feeds skill feedback)
- **Prediction:** 1 / 1 / 0.45

### L19 — Portfolio returns/review cycle (classification pending)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-portfolio/src`
- **Entry point:** `server.rs:892` `portfolio_seed_price`, `:986` `portfolio_materialize_returns`; `returns.rs`, `analysis.rs`, `store.rs`
- **Trigger:** ledger append → price seed → returns materialization → review (TWR/MWR, attribution)
- **Phase 1 classification:** request-driven compute pipeline vs canonical loop — decide on graph shape, do not assume
- **Prediction:** 1 / 1 / 0.45

### L20 — RSS subscription / conditional feed sync cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src`
- **Entry point:** `hkask_mcp_research.rs:789` `rss_subscribe`, `:875` `rss_fetch`, `:960` `rss_get_entries`; conditional-fetch state and writeback at `:883-944`
- **Participants:** research DB subscriptions/entries and cached ETag/Last-Modified, `rss_client`, synthetic-feed extraction (`:895-900`), agent tool caller (L1), L4 request envelope
- **Trigger:** subscribe or explicit fetch by agent/user; there is no observed autonomous polling in the inspected tool path
- **Hands off to:** L4 (tool calls); L12 (research source selection); L1 (readback of stored entries)
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35

### L21 — Gallery scan / metadata reconciliation cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-media/src/tools/gallery.rs`
- **Entry point:** `gallery_organize` `:159` (scan/reconcile `:189-193`); `gallery_refresh` `:542` (rescan/reconcile `:552-554`, analysis `:579-580`); query surface `gallery_search` `:250`
- **Participants:** active gallery state, on-disk scan and persisted gallery index/metadata, optional inference analysis (L5), media panel (L7)
- **Trigger:** explicit organize/refresh/search calls; no background scan claimed
- **Hands off to:** L7 (gallery/panel), L5 (analysis), L11 (media jobs/assets); remains distinct from job admission and completion
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35

### L22 — Spreadsheet optimistic-revision / interrupted-operation reconciliation cycle
- **Crate/path:** `kask/crates/hkask-spreadsheet/src/service.rs` + `kask/mcp-servers/hkask-mcp-spreadsheet/src/server.rs`
- **Entry point:** `server.rs:94` `spreadsheet_apply` → `service.rs:200` `apply`; reconciliation `server.rs:124` `spreadsheet_operation_get` → `service.rs:217` `operation_get`
- **Participants:** immutable workbook revision and idempotency record, caller-held digest/key, L4 tool envelope, L7 spreadsheet widget
- **Trigger:** user/agent edit; after an interrupted request the caller asks for recorded operation outcome before deciding whether to retry
- **Hands off to:** L7 (editable workbook block), L4 (typed conflict and recovery status); remains distinct from the ordinary one-shot cell calculation
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35

### L23 — Research-provider selection / observed-performance cycle
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src/research/{providers,performance}.rs`
- **Entry point:** `providers.rs:683` `score_providers` → `:730-743` live penalty/readback; provider outcome `providers.rs:315-332` → `performance.rs:76` `record_outcome`; next selection from `hkask_mcp_research.rs:313`
- **Participants:** `ProviderPool`, per-provider bounded recent-outcome samples (`performance.rs:64-81`), search providers, L2 Regulation span archive (`providers.rs:315-322`)
- **Trigger:** each search outcome; subsequent intent-selected search uses the observed success/latency after the minimum sample count (`performance.rs:32-34`, `:109-115`)
- **Hands off to:** L12 (research results), L2 (provider spans), L4 (web search request); separate fast-path in-process selection from the durable Regulation history
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35

## Boundary notes (sub-cycles folded into rows above, not separate rows)

- Email alert delivery — sensor leg inside L2 (`hkask-email`, `main.rs:656`).
- Conversation/context injection — leg inside L1 (`hkask-conversation-injector`, `kask_bridge/context_injector.rs`).
- Thread condensation — leg inside L1 (`hkask-condenser` via `condenser_bridge`).
- Polymarket resolution subscription — leg inside L8 (`market_subscribe_resolutions`, paired with `market_record_resolution` per `hkask_mcp_prediction_markets.rs:264-298`).
- Ordinary spreadsheet cell calculation is request-driven; interrupted-operation reconciliation and persistent revision identity form L22.
- Market health/`web_ping` style probes — legs inside L3/L4.

## Decomposition into audit slices (INVEST)

One slice = one register row through Phase 1 → 4 (map → detect → consolidate →
verify), sized so each slice lands or defers independently. Slice order is a
technical decision (program manager's per the Division of Responsibilities),
vetoable on functional grounds:

1. **Batch A (early deletion candidates, known duplication signals):** L8
   (closed 2026-09-27 — Brier signal refuted; real finding was the scenarios
   wrapper, consolidated), L3+L4 (closed 2026-09-27 under the duplication
   rubric; L3 REOPENED for the minimalism pass — six per-server maps → one),
   L14 (closed 2026-09-27 — minimal by design). Next: L3-minimalism, then
   Batch B.
2. **Batch B (control core, highest connectivity):** L2, L5, L16.
3. **Batch C (large surfaces):** L1, L7, L6.
4. **Batch D (bounded server loops):** L9, L10, L11, L12, L13, L17, L18, L19, L20, L21, L22, L23.

Rationale: bank consolidation wins on small, duplicated surfaces first; audit
the high-connectivity control core before the largest surfaces, so impedances
found there inform the big-surface audits. A functional priority (a loop whose
behavior matters most to the operator) overrides this order on request.

### Per-loop INVEST worklist (pending approval)

Each row below is a separate, bounded audit task, not a command to start it.
**Shared acceptance gate for each open slice:** (1) cite every sense/orient/decide/act/observe node and cross-loop edge from the current tree; (2) classify the five feedback properties, each finding as IS/OUGHT/INFERRED with file:line and a falsifier; (3) either remove the redundant path with behavior-preserving tests, full-repo identifier sweep and build, or name the rejected candidate and a risk-priced deferral. Any behavioral-bug hypothesis must first pass the diagnose/reproduction gate; any zed-side edit needs its D-seam and test in the same pass. The local verifier below is additional to these shared gates. L3/L4/L8/L14 have historical results above: re-check only their reopened or new-edge scope; do not treat their earlier audit as authorization for further work.

| Task | Independent slice and observable check | Dependency / local verification |
| --- | --- | --- |
| L3 | Reduce or explicitly reject the six-map server-state consolidation without changing delivery semantics. | L4 contract; 22 runtime tests + `reconnect_integration.rs` and unknown-effect retry pin. |
| L4 | Re-check typed-error handoff to L3; do not merge child processes if isolation would change. | L3 seam; server-framework tests and per-server credential isolation. |
| L8 | Confirm the already landed scoring consolidation retains outcome readback; close only a newly evidenced gap. | L17 scoring edge; forecast/scenarios tests and existing commit `50cba394fd`. |
| L14 | Confirm settings and credential changes still restart exactly affected servers. | L3, L5; settings-sync tests + launch-order invariant. |
| L15 | Trace pending passphrase rotation through every DB to the last keychain write, including recovery on partial failure. | L10, L4; passphrase-rotation tests and keychain-last invariant. |
| L2 | Trace one Regulation tick from sensor to verified effect or surfaced alert. | L16 outcomes; regulation-cycle tests. |
| L5 | Trace one IPC inference request through response/error to caller. | L3 environment; inference IPC tests. |
| L16 | Trace one skill activation and failure to durable outcome and algedonic readback. | L2, L1; skill-outcome tests. |
| L1 | Trace one user turn including tool continuation and memory/panel observation. | L3, L5; agent turn tests. |
| L7 | Trace a thread event and one widget action through GPUI update to observable UI. | L1, L9; targeted panel/widget tests. |
| L6 | Trace source conversion to grounded QA ingestion with counts reconciled across stages. | L5, L10; corpus pipeline seam tests. |
| L9 | Trace goal creation to operator-scored outcome and memory acknowledgement. | L10; goal lifecycle tests. |
| L10 | Trace stored memory through retrieval to context injection and deletion hygiene. | L1, L2; recall/ingest round-trip tests. |
| L11 | Trace async media submit through cancel/finish to job status. | L7; job state tests. |
| L12 | Trace a run-scoped search through recorded evidence to a retrievable run. | L4; research-run tests. |
| L13 | Trace delegation result into swarm recall and the panel/task-board observation. | L7, L10; swarm thread tests. |
| L17 | Trace quantified event through update and scored resolution into readback. | L8; scenarios scoring tests. |
| L18 | Trace authorized training job to terminal status without launching a training run for this audit. | L6; offline submit/status/cancel tests only. |
| L19 | Test whether portfolio returns/review closes a feedback path; if not, reclassify row rather than fabricate a loop. | L7; portfolio materialization tests. |
| L20 | Trace conditional RSS fetch through cache-header/entry persistence to readback. | L12; RSS 304/new-entry tests. |
| L21 | Trace scan/reconcile through metadata refresh to gallery query. | L7, L5; gallery reconciliation tests. |
| L22 | Trace interrupted edit by idempotency key through operation readback. | L7, L4; workbook conflict/recovery tests. |
| L23 | Trace search outcome into rolling sample and changed next-provider ranking. | L12, L2; provider-ranking tests. |

**Checkpoints:** approval of this register precedes any *new* Phase 1 work;
verify each slice before starting another touching the same shared contract;
review cross-loop edges L3↔L4, L1↔L5, L7↔L9/L13/L21/L22,
L6↔L10/L18 and L12↔L20/L23 after their endpoint slices; do a final
register-to-tree coverage walk before declaring global completion. Tasks may
run independently only where their write scopes and contracts do not overlap.
The worklist has 23 tasks because the acceptance criterion requires a slice
per loop, not because 23 independent implementations are proposed.

**Open risks at checkpoint:** L3 state-map rewrite is cross-contract and
could change retry/stop behavior (high impact; revert the atomic slice if its
existing or targeted tests fail); L19 may be a pipeline rather than a loop
(low impact; reclassify on Phase 1 evidence); an undocumented additional
feedback path may remain after the package-level inventory (coverage risk;
close only after the final tree-to-register walk). Owner for each is the
technical program manager; approval to resume Phase 1 belongs to the operator.

## Working rules

- Graphs and findings live in register rows and the final report — no
  per-loop documents are created in any phase.
- Operator checkpoints carry only functional, blocking decisions; technical
  decisions arrive as recorded decisions with veto rights; neighboring
  systems' bookkeeping (e.g., docs-tree governance) stays out of the audit.
- Every slice runs the ideal-method pass (Ousterhout): what is the smallest
  code that does the job in the common case, disregarding existing structure?
  Close ideal-vs-actual gaps where behavior is preserved. "No duplication
  found" is not a complete slice result — the graph must also be the small
  graph, and the critical path the short path (operator correction,
  2026-09-27).

## Change log

- 2026-09-27 — v0.10.0 bounded Phase 1–2 maps added for L1, L6, L7,
  L9 and L10. The Stage 7→9 corpus QA seam was source-confirmed: skips in
  generator output fail grounding. The existing `build-corpus-pipeline` skill
  now projects candidate-only rows, retains and reconciles the mixed original;
  a synthetic jq check confirmed skip-only filtering but **no live corpus run**
  verified the repaired capability. No new production source lines for that
  skill change; L1/L7/L9/L10 findings remain open or deferred with falsifiers.
  This update is uncommitted, as is the L5 fix; no global completion claimed.
- 2026-09-27 — v0.9.0 L3 current-tree library/integration tests and full gates passed; the earlier fixture-suite failure was an invalid parallel invocation. L5's JSON-error classification failed at the existing IPC seam, then passed after a −1-production-line change; 25 test lines added and 54/54 library tests passed. The Api error-shape impedance remains deferred. This follow-up is uncommitted; do not cite a completion hash for it.
- 2026-09-27 — v0.8.0 operator approval recorded; L3 current-tree entry
  points and the actual +38 production-line delta from `16271e3c60` supersede
  its pre-commit reduction estimate. Read-only error-path reviews reopened
  L2/L5 and identified a cross-turn attribution question for L16, each with a
  falsifier; none counted as a reproduced defect. A local L3 test run stopped
  before tests at uncommitted bridge-ontology compilation errors. This
  register update remains uncommitted; no further production edits were made
  by this pass.
- 2026-09-27 — v0.7.0 L5 audited and closed: the inference IPC request cycle
  fully mapped (security layer, one-in-flight EOF-cancel, per-method dispatch
  with allowlist-as-authority, GPUI channel round-trips); 29 error-payload
  constructions collapsed into one `ipc_error` helper — net −95 production
  lines in `4eaca76874`, byte-identical behavior, 33 module tests + clippy
  green (detached-worktree validation reported in that prior pass; not
  rerun in this shared tree). inference_chat internal minimalism deferred
  to Batch C. Running committed production ledger: −13 + 38 − 95 = −70. Register edit additive on the operator's uncommitted refresh, not
  committed with the slice.
- 2026-09-27 — v0.6.0 L2 audited and closed (minimal by design): two-loop
  architecture verified as the ideal shape for its requirements (10s
  actuation vs 30s observability, one-way alert channel decoupling failure
  domains — merge rejected on behavior grounds); bridge-fleet generic
  rejected on the admission test; sensor no-data discipline and `always_on`
  enforcement verified; E04 pre-reset capture and four-way alert fan-out
  mapped with citations. Doc-only slice; production ledger unchanged (+25
  after `50cba394fd` and `16271e3c60`). Register edit additive on the operator's uncommitted v0.5.0
  refresh, not committed with this slice.
- 2026-09-27 — v0.5.0 Phase 0 refresh at the operator checkpoint: retained
  the 19 committed rows and their earlier audit history, checked the current
  package inventory, added L20–L23 for four separately triggered feedback
  paths, replaced non-line-specific L6/L9/L18 entries, and recorded per-row
  verification tasks. No new Phase 1 audit or production edit was performed
  in this refresh; register work is uncommitted pending operator review.
- 2026-09-27 — v0.4.0 operator goal correction banked: the slice unit of
  value is graph consolidation and logical minimalism (smallest code for the
  common case, faster critical path), not defect detection. Working rules
  updated; L3 reopened with the six-maps→one design (cancellation tokens
  exist in both the token map and `LaunchSpec.cancel`); L14 closed minimal by
  design (launch/sync mirror rejected for collapse — startup ordering is
  behavior). Doc-only slice; production total remains −13 from `50cba394fd`.
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
