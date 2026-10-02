---
title: "Loop Register — zed-kask canonical loops"
audience: [developers, architects, agents, operators]
last_updated: 2026-10-01
version: "0.25.5"
status: "Active"
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

## Pass 2 re-audit — closed 2026-09-28; dispositions ruled 2026-09-29

Pass 2 (2026-09-28) reviewed the full MCP tool surface — 403 tools across
12 servers plus the shared framework (S1–S12, smallest-first) and the 23
loop re-slices; 14 findings were fixed, each with its commit hash in the
change log's v0.23.x entries, and every operator decision was ruled
(2026-09-29) and executed (the D1–D12 dispositions and F1–F6 follow-ups
below). The durable records live in the register rows, the per-server
reference docs, and the change log; the process narrative (premise
verification, prior-findings reconciliation, tree staleness, the
loose-ends ledger, the tool inventory, the per-server review tables, the
predictions, and the INVEST decomposition) is condensed here per the
alignment plan §8 CU-5 — git history is the archive. The anchor ledger
below is retained: the admitted reference model (Pass 3) cites it.
### Reference-model anchor ledger (axis 4 — anchor or explicit gap per row; no invented anchors)

| Row | Anchor | Record |
| --- | --- | --- |
| L1 | GAP | no recorded model for the turn loop |
| L2 | ANCHOR | `pragmatic-cybernetics` skill `## Reference models` section; Diataxis `hkask-regulation` docs |
| L3 | GAP | `.rules` traps only |
| L4 | PARTIAL | repair plan (plan-form: findings + execution program) + `.rules` MCP patterns; no prior-art model |
| L5 | PARTIAL | Diataxis `hkask-inference` reference docs (internal, evidence-cited); no prior-art model |
| L6 | PARTIAL | `kask/docs/research/chunking-for-rag-research.md`; `build-corpus-pipeline` skill lacks a `## Reference models` section |
| L7 | GAP | no recorded model |
| L8 | ANCHOR | superforecasting methodology — `## Reference model` section (Tetlock & Gardner 2015, onto_anchor derived superforecasting; the commandment map in kask/registry/templates/superforecasting/README.md Theoretical Foundation) |
| L9 | ANCHOR | `kask/docs/research/kanban-board-reference-models.md` (the exemplar) |
| L10 | PARTIAL | `kask/docs/architecture/memory-system-specification.md` (internal spec-form) |
| L11 | GAP | no recorded model |
| L12 | GAP | no recorded model |
| L13 | PARTIAL | Diataxis `swarm_system` docs; no recorded model for the thread/memory cycle |
| L14 | GAP | no recorded model |
| L15 | GAP | no recorded model |
| L16 | PARTIAL | pragmatic-cybernetics (VSM S1–S5) applies; `algedonic-review` skill lacks a recorded section |
| L17 | ANCHOR | `scenario-planning` skill `## Reference models` (Schwartz/Tetlock/Chermack) |
| L18 | PARTIAL | `kask/docs/reference/lora-training-catalog.md` (reference doc); `lora-training` skill lacks a recorded section |
| L19 | ANCHOR | `portfolio-review` skill `## Reference models and labels` |
| L20 | GAP | HTTP conditional GET (RFC 9110) is the implicit, unrecorded model |
| L21 | GAP | no recorded model |
| L22 | ANCHOR | `kask/docs/plans/logisheets-spreadsheet-capability-plan.md` (plan-form, chartered 2026-09-18) |
| L23 | GAP | no recorded model |
| L24 | PARTIAL | the repair plan's §P8 experiment protocol (plan-form, `kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md`); no prior-art model recorded |
| L25 | PARTIAL | the admitted CNS reference model — the afferent-pathway structure is this row's own shape; no domain prior-art model recorded |

(L24/L25 ledger rows added 2026-10-01, post-Pass-3, completing the
per-row claim.)

Inventory shape: 274 `.j2` templates, 2 with `Reference model:` headers
(`company-research/thesis-three-pillars.j2` — MAIA;
`prompt-enhance/enhance-classify.j2` — Liu et al., FCS 2026);
method-named templates without headers are IS by absence (e.g.
`wardley-anchor.j2`, `gorilla-4dim.j2`,
`falstaffian-competitive-rotation.j2`). 15 SKILL.md files carry
`## Reference models` sections (of 59 catalog skills). Creating missing
records is a separate operator decision — this pass records gaps,
assesses alignment where anchors exist, and invents none.

## Pass 3 — Three-layer cybernetic nervous system review (2026-09-30; operator spec 2026-09-30)

Phase 1 of the three-layer review. **Fixed definitions (operator spec):**
Layer A — autonomous regulation (fast, deterministic; feedback handled
entirely in code — retries, degradation surfacing, backpressure,
self-healing — logged in a form later review can learn from); Layer B —
in-thread sensing (conscious, in-process; signals surfaced to the agent
and/or human inside the active thread, responded to in that thread's
context, also logged); Layer C — periodic reflection (curator and human
periodically reviewing logs and curator memory through the therapy and
algedonic-review skills). **Cybernetic nervous system** = the unified
sense → report → actuate substrate connecting the layers. **Expectation
signal** = every actuation carries an expectation; the log records the
prediction-error delta. Code-facing names for these categories are
proposed only after the naming survey below — none invented here.
Prediction recorded at open in the change log (v0.24.0); scored at
close-out in the same entry.

### Naming survey (production `src/` only; IS, file:line)

The tree already carries a rich feedback vocabulary concentrated in
`hkask-regulation` and the curator surfaces; the expectation/surprise
vocabulary is the one notable absence.

| Internal name | Sites (IS) |
| --- | --- |
| sense / observe / Sensor / SensorBus | `hkask-regulation/src/sensor_provider.rs:27-29` (trait `Sensor`, `observe`), `:45` (`SensorBus`); stage `sense` `cybernetics_loop/cycle.rs:305` |
| Signal / set_point / Deviation | `hkask-regulation/src/loops/signals.rs:332-336` (`Deviation { signal, magnitude, direction }`); `:343` (`value - set_point`); `set_points.rs:439` `load_set_points` (0.80 default `:406`) |
| advisory / act / escalate | `cybernetics_loop/cycle.rs:409` `compute`, `:450` `act`, `:781` `escalate_exhausted_checks`; `route_action_as_alert` `:569` |
| algedonic | `hkask-regulation/src/algedonic.rs:1-3` ("algedonic (pain/pleasure) feedback for cybernetic control"), `:247` `AlgedonicManager`, `:68` email sink ("S1->S5"), `:88` review-board sink; `kask_bridge/src/algedonic_board.rs:1-2`, `algedonic_log_bridge.rs`; `hkask-mcp-curator/src/hkask_mcp_curator.rs:1199` `curator_algedonic_log` |
| RegulationRecord / Span / SpanKind | `hkask-types/src/event.rs:16-28` (record: span, phase, observation, regulation, outcome, visibility), `:364-369` (`Span`), `:411-443` (`SpanKind`, 11 typed `reg.*` variants) |
| curator memory / h_mem | `hkask-memory/src/memory_store.rs:288` `store`, `:331` `query_deduped`, `:447` `touch_recall`; `kask_bridge/src/memory/ingest.rs`; curator tools `hkask_mcp_curator.rs` (insert/update/extract/prune/dedup) |
| drift | `hkask-regulation/src/metacognition.rs:447+` (skill-outcome/operator-feedback drift sense) |
| feedback | `hkask-mcp-companies/src/tools/valuation.rs:1478` `result_feedback`; `crates/agent/src/tools/record_skill_feedback_tool.rs:69-90`; `hkask_mcp_curator.rs:1312` `curator_report_skill_use_issue` |
| Brier / calibration | `hkask-forecast/src/hkask_forecast.rs:190/:196`; `hkask-mcp-prediction-markets/src/calibration.rs:134`; goal Brier `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:521` |
| toast / AlertSink | `hkask-regulation/src/metacognition.rs:150` `AlertEvent`, `:167` `AlertSink`; `crates/zed/src/main.rs:3984` `ToastAlertSink`, `:3994` impl, wired `:1117` |
| loop-quality telemetry (coalesced) | `cybernetics_loop.rs:895-968` — "Coalesce only semantically identical persistent deviation/advisory cycles. Changed values, clearing, and rollout measurements always emit" |
| `expect:` (doc convention) | `hkask-regulation` doc comments (`algedonic.rs:156`, `sensor_provider.rs:50`); validated statically by `bin/check_test_evidence.rs` (`:43` `InvalidExpectation`) — a design-evidence discipline, not a runtime signal |
| expectation / surprise | **absent as feedback names**: "surprise" — 3 production hits, none cybernetic (`hkask-memory/src/memory_store.rs:1682` comment; `hkask-mcp-scenarios/src/templates.rs:437` prompt text); "expectation" hits are the finance concept (`hkask-bridge-ontology/src/derived.rs:404-409`) and test-evidence contracts |

Occurrence counts (production `src/`, this pass): algedonic 251, regulation
774, curator 1455, set_point 106, deviation 127, sensor 195, escalate 102,
degrad 420, feedback 351, drift 198, h_mem 1160, brier 203, calibration
380, telemetry 363, toast 102, expectation 108, surprise 3.

### Mechanism inventory (IS, file:line, layer(s) currently served)

**Report (sense → report) pathways — twelve distinct ones:**

| # | Mechanism | Sites | Layers served |
| --- | --- | --- | --- |
| R1 | zed `log` macros → `zlog` → log file | init `crates/zed/src/main.rs:346` `zlog::init()`, `:355` `init_output_file(paths::log_file())`; logger `crates/zlog/src/zlog.rs:19-20` (`log::set_logger`); 102 warn sites zed-side + 5 kask | A, B |
| R2 | `.log_err()` Result idiom | def `crates/gpui_util/src/lib.rs:222/:245`; 250 scoped sites (agent 20, agent_ui 115, zed 115) | A, B |
| R3 | kask `tracing` macros | 558 warn sites (kask/crates 202, mcp-servers 356) + 9 in zed main's kask sections; child subscriber `hkask-mcp-server/src/server/transport.rs:42-46` (stderr writer, EnvFilter default info) | A (children), B |
| R4 | child stderr → host forward | `hkask-mcp/src/runtime.rs:813-830` (`info!(target: "hkask.mcp.child")`, per line) | A |
| R5 | persisted RegulationRecords → curator.db archive | `hkask-types/src/event.rs:16-28`; `kask_bridge/src/memory/curator_stores.rs:38-45` `open_curator_regulation_archive`, skill outcomes `:63/:74`; readback `reg_query` `hkask_mcp_curator.rs:1234` | A, B, C |
| R6 | algedonic events → in-memory log + board cards + email | `algedonic.rs` (manager, cap, `AlgedonicLogApproachingCap` `:563-564`); `kask_bridge/src/algedonic_board.rs:1-2` (kanban cards; CONDITION/CONTEXT markers `:16-17`); `hkask-email` | A → C |
| R7 | alert channel → GPUI toasts | `metacognition.rs:150/:167`; `main.rs:3984-3994`, wired `:1117`, drainer `:5107` | B |
| R8 | curator memory h_mems | `memory_store.rs:288/:331/:447`; turn ingestion `crates/agent/src/thread.rs:3019-3086` → `curator:thread:{id}` chunks | B → C |
| R9 | tool traces (JSON, per session) | `crates/agent/src/tool_trace.rs:22` `ToolTrace`, `:158` `trace_directory` (artifacts `agent-traces`), `:192` `write_trace` | B, C |
| R10 | loop-quality telemetry spans (surprise-gated) | `cybernetics_loop.rs:895-968`; `SpanKind::LoopMetricsTelemetry` `event.rs:433-434`; hourly heartbeat + suppressed count | A, C |
| R11 | GPUI render signal `cx.notify()` | 622 production sites; panel subscriptions `crates/agent_ui/src/agent_panel.rs:374+` | B |
| R12 | kanban board/cards/comments | `hkask_mcp_kata_kanban.rs:974` `kanban_task_verify`, `:1001` `kanban_task_comment`; escalation cards `cycle.rs:781-804` | B, C |

**Actuation mechanisms:** regulation act stage (`cycle.rs:450-562` cap
reset + alert fan-out; `route_action_as_alert` `:569`); dampener
(`dampener.rs`); bounded retry (impact checks `cycle.rs:723`; L1
`retry_completion_error`; L3 reconnect `hkask-mcp/src/runtime.rs:1308`);
circuit breaker (`runtime.rs:1155-1177`); supervisor restart
(`runtime.rs:1050-1217`); settings-sync restart (`main.rs:3585`);
passphrase rotation + rollback (`kask_bridge/src/passphrase_rotation.rs:212-275`);
degradation surfacing (typed per-variant `McpError`,
`hkask-mcp-server/src/server/error.rs:16`); memory writes
(`memory_insert`/`update`/`resolve_contradiction`); kanban moves;
calibration tier demotion (`hkask-mcp-prediction-markets/src/types.rs:251`);
skill-outcome recording (`main.rs:995` `persist_skill_outcome`);
operator directives → inbox → `process_inbox` (`cybernetics_loop.rs:672-687`;
tool `crates/agent/src/tools/curator_tools.rs:807-830` `curator_directive`);
backpressure (`main.rs:1256-1300`).

### Pathway scoping (S6, 2026-09-30; operator-approved) — canonical vs diagnostics roles

Target-condition clause 2: one canonical report pathway per layer,
every other pathway explicitly scoped. The twelve pathways classify
as:

| Pathway | Role | Deletion-test verdict |
| --- | --- | --- |
| R5 RegulationRecord archive | **canonical Layer A** — the durable report, read via `reg_query` | keeps (the load-bearer) |
| R10 loop-quality telemetry | **canonical Layer A producer** — the surprise-gated emitter into R5's archive (`emit_regulation_span`) | keeps (INV3's exemplar; its raw duplicate was S3-deleted) |
| R6 algedonic events → board/email | **canonical Layer A escalation arm** — the A→C bridge (INV4) | keeps (rides R12's board substrate) |
| R7 alert channel → toasts | **canonical Layer B** — the in-process human alert surface | keeps |
| R11 `cx.notify()` render signal | **canonical Layer B substrate** — the panels' refresh mechanism, not a feedback report | keeps (the surface itself) |
| R8 curator memory h_mems | **canonical Layer C** — the slow-tier model substrate | keeps |
| R12 kanban board/cards/comments | **canonical B/C work-state** — the durable work record both layers act on | keeps |
| R1 zed `log` macros | diagnostics (zed-side process logs) | keeps — distinct role; deleting re-spawns inline error prints at 100+ sites |
| R2 `.log_err()` | diagnostics (zed-side Result idiom) | keeps — one idiom, 250 sites; deleting re-spawns the boilerplate |
| R3 kask `tracing` macros | diagnostics (child stderr + the recorded in-host fallback, S2) | keeps — recorded as designed in `regulation-spans.md` §1 |
| R4 child stderr → host forward | diagnostics carrier (feeds R3's in-host arm) | keeps — the child-diagnostics bridge |
| R9 tool traces | diagnostics (per-session tool-call traces, artifacts `agent-traces`) | keeps — eval/debug consumer |

**No-candidate finding, with evidence:** no pathway is a duplicate of
another — the one true duplication (R10's raw per-tick copy of the
coalesced span) was deleted as S3 (net −14). Each diagnostics pathway
has a distinct role and consumer; the deletion test re-spawns
complexity at every call site. The scoping closes target-condition
clause 2: three canonical pathways (A: the R5 archive with R10's
producer and R6's escalation arm; B: the tool envelope plus R7's
toasts on R11's substrate; C: R8's memory with R12's work-state),
eight diagnostics pathways each carrying an explicit role, one
substrate. CU-6 in the alignment plan §8 owns the strangler-fig
execution if a diagnostics pathway is later displaced.

### Premise verdict — "logging and actuation are not unified and canonical": IS (confirmed), with per-loop nuance

1. **Two logging frameworks split exactly at the D-seam — and they ARE
bridged, by an incidental feature flag.** kask crates emit `tracing` (558
warn sites); zed-side crates emit `log` (102 warn sites) plus `.log_err()`
(250 sites). The host installs only a `log` logger (`zlog.rs:19-20`); the
only global `tracing` subscribers are cfg-gated (Tracy,
`crates/ztracing/src/lib.rs:87` via `crates/zed/Cargo.toml:22`). **This
survey's initial finding — that in-host tracing events are discarded into
the no-op default — was REFUTED by the Pass 4 live-log experiment** (kata
Check, recorded in the alignment plan): tracing's `log` feature, enabled
graph-wide by zed's own `crates/rpc/Cargo.toml:35`
(`tracing = { features = ["log"] }`), makes tracing macros emit `log`
records when no subscriber is active (tracing-0.1.43 `__tracing_log`,
`src/lib.rs:1048-1069`), and zlog writes them to the log file, printing
the record's module path (`zlog.rs:73-78`). Empirically confirmed:
2,173 `[hkask_mcp::runtime]`-wrapped child-stderr lines in the live
`Zed-Kask.log` (2026-09-30), including `reg.tool: REG` spans and curator
distillation/forgetting passes. **Residual IS finding (adjudicated Pass
4): the bridge is an undocumented incidental coupling** — nothing in
`kask/docs` or `DIVERGENCE.md` records that kask's in-host tracing
visibility depends on zed's rpc crate enabling that feature (grep for
tracy/ztracing/tracing-log over `kask/docs` + `DIVERGENCE.md`: zero
hits); dropping the feature would silently vanish these lines, and under
a Tracy build (subscriber active) the fallback stops firing and the same
lines leave the log file. Minor observed note: the child-stderr forward's
`server_id` field does not appear in the log wrapper (the child lines
self-identify via their own `server_id` field), so the "tagged with the
server_id" comment's tagging does not survive the fallback rendering.
2. **Twelve report pathways, no canonical one per layer.** R1–R12 span file
logs, a DB archive, an in-memory event log, kanban cards, toasts, memory,
trace files, telemetry spans, and the render loop. Within single loops the
paths are often single-copy (L4's one `execute_tool` span; L2's one alert
fan-out) — the fragmentation is cross-cutting, not internal duplication.
3. **The expectation signal exists in exactly one loop.** `Signal` carries
`value` + `set_point` and `Deviation` is their delta (`loops/signals.rs:332-343`);
alert records carry `deficit` + `threshold` (`cycle.rs:600-611`). No other
report pathway records an expectation alongside an observation — R1–R4 log
raw events; R9 traces record calls/results; R12 cards carry
condition/context. "surprise" as a name is absent.
4. **One surprise-gated pathway already exists** — R10, the loop-quality
telemetry coalescer (`cybernetics_loop.rs:895-968`): exact steady-state
repeats are suppressed with a counted accumulator, changes/clearing always
emit, and an hourly heartbeat re-announces liveness with the suppressed
count. This is the in-tree precedent for the spec's expectation/surprise
logging principle.

### Layer coverage (which layers each register row currently touches; IS)

Per-row evidence pointers are the row's own pass-2-verified citations plus
this pass's mechanism inventory; the primary/secondary classification with
grilling is Phase 3's deliverable (per-row blocks below).

| Row | Touches | Mechanism evidence |
| --- | --- | --- |
| L1 | A, B, C | retries/truncation typing (A); in-thread tool results (B); turn-end memory ingest (C) |
| L2 | A, B, C | 10s actuation + bounded retry (A); toasts/`curator_status` (B); archive + board (C) |
| L3 | A, B | reconnect/breaker (A); operator-actionable breaker error (B) |
| L4 | A | typed errors, envelope, validation |
| L5 | A | deadlines, EOF-cancel, resilience sensor → L2 |
| L6 | A, B | mechanical gates (A); agent-driven chain + surfaced failures (B) |
| L7 | B | panel/widget surfaces, toasts |
| L8 | A, B, C | tier demotion (A); readback (B); calibration history (C) |
| L9 | B, C | in-thread actions/panels (B); goal score → memory, Brier (C) |
| L10 | B, C | context injection (B); curator memory substrate (C) |
| L11 | A, B | admission/cancel/rollback (A); panel polling (B) |
| L12 | A, B | append receipts, first-observation (A); agent annotations (B) |
| L13 | B, C | in-thread delegation (B); durable threads (C) |
| L14 | A | event-driven restart |
| L15 | A | startup rotation + rollback |
| L16 | A, B, C | span/outcome recording (A); in-thread activation (B); algedonic review (C) |
| L17 | B, C | caller-driven (B); calibration readback (C) |
| L18 | B | operator-confirmed submit + polling |
| L19 | B, C | request-boundary recompute (B); review skill (C) |
| L20 | A, B | conditional GET/upsert (A); caller-driven (B) |
| L21 | A, B | scan/reconcile (A); explicit calls (B) |
| L22 | A, B | idempotency/digest/reconcile (A); widget edits (B) |
| L23 | A, B | in-process penalty (A); surfaced rationale (B) |
| L24 | B, C | agent-driven protocol calls (B); the fossil-record registry + population readback (C) |
| L25 | A, B | autonomous snapshot sensing + deviation vs the zero set-point (A); L2-mediated escalation/toast surfacing (B) |

(L24/L25 rows added 2026-10-01, post-Pass-3, completing the per-row
coverage claim; full classifications in the row blocks.)

### Pass 3 classification record (D3) — saturation and the alignment gap table

**Saturation signal (spec):** the one new finding of the per-row slices —
the in-host tracing drop (adjudicated at L2, shared by L3) — was
subsequently **REFUTED and corrected by the Pass 4 live-log experiment**
(see the premise verdict and the alignment plan); the corrected finding
(the undocumented log-feature bridge) stands in those rows. Slices L4,
L5, L6 produced three consecutive no-new-finding results → **saturation
reported at L6**; L7–L23 are recorded at classification-only depth
(classification, grill, alignment; standing impedances re-cited, not
re-adjudicated). Every row is classified — D3's coverage requirement is
met by the per-row blocks; the signal changed depth, never coverage.

Alignment gap table (the `kanban-board-reference-models.md` pattern —
named invariants from the Phase 2 model, file:line, verdict):

| Invariant | Holds where (file:line) | Verdict |
| --- | --- | --- |
| INV1 — one canonical pathway per layer | within-loop single-copy: L4's one `execute_tool` span (`tool_span.rs:162`), L2's one alert fan-out (`cycle.rs:568+`), L10's one store (`memory_store.rs:288`), L14's one sync funnel (`main.rs:3585`), L25's afferent sensor feeding L2's canonical arms (a new loop adding no new report pathway) | **PARTIAL** — canonical within loops; cross-cutting, twelve report pathways serve three layers (Pass 3 inventory R1–R12) |
| INV2 — expectation carriage | `Signal.set_point` + `Deviation` (`loops/signals.rs:273-280,326-333`); alert deficit/threshold (`cycle.rs:628-639`); goal intake prediction (Brier at resolution); forecast probability (`forecast_persist`); L22's base digest | **PARTIAL, scoped by design** (operator ruling 2026-10-01, closing O3) — held in L2 (exemplar), L8/L9/L17 (stored priors), L22 (digest), and the board cards' deficit/threshold pair; absent from R1–R4, R9, R12 *by design* — event-record surfaces where expectation fields would be ceremony with no scoring consumer |
| INV3 — surprise-gated reporting | loop-quality telemetry coalescer (`cybernetics_loop.rs:895-968`); algedonic binary threshold (`algedonic.rs:247+`); L20's conditional GET (protocol level) | **GAP** — one in-process pathway of twelve; the rest log raw activity. S3 (2026-09-30, operator-approved) deleted the per-tick raw duplicate of the coalesced span (net −14) — the direction held; the count changes only with S6 pathway scoping |
| INV4 — escalation, never silent drop | exhaustion escalation (`cycle.rs:781-804`); circuit breaker (`runtime.rs:1155-1177`); L18's `RunningUnknown` (the F1 repair); L22's explicit unknown; L23's `live_stats_degraded` (the S4 repair, 2026-09-30), L25's broken-source warn-and-None (D87, pinned by test) | **HELD at the audited sites** — both named violations resolved (L23 repaired by S4; the suspected in-host tracing drop refuted by the Pass 4 experiment); a full per-row INV4 sweep rides the §8 cleanup (alignment plan) |
| INV5 — model revision at the top | curator distillation/consolidation (`consolidation_service.rs:38`); set-point loading (`set_points.rs:439`); skill verdicts (algedonic review); calibration readback (L8/L17) | **PARTIAL** — the C-tier machinery exists; the B→C handoffs carry standing receipt deferrals (L1/L7 memory receipt, L9 acknowledgment gate) |
| INV6 — afferent/efferent direction discipline | directive inbox (`cybernetics_loop.rs:672-687`, efferent); alert channel (`main.rs:632-641`, afferent); L10 inject-down/ingest-up | **PARTIAL** — the channels exist and are clean; the log/tracing framework split (R1 vs R3) is bridged only by an incidental feature flag (`crates/rpc/Cargo.toml:35`) — functional but undocumented (the L2/L3 corrected finding) |

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
| Extensions found in tree | recorded | L11–L25 |

No expected family was absent; the tree shows additional loop families beyond
the spec's minimum list, recorded below rather than narrowed away.

## Register

### L1 — Agent turn loop
- **Crate/path:** `crates/agent` (zed-side, D-seamed), participants in `crates/agent/src/`
- **Entry point:** `crates/agent/src/thread.rs:3176` `run_turn_internal`; spawned via `thread.rs:2984` / `agent.rs:2472` `run_turn`; submissions at `agent.rs:3615`, `:3754`, `:3900`
- **Participants:** `agent.rs` (10,419 ln), `thread.rs` (12,970 ln), `tools.rs`, `tool_retry_tracker.rs`, `tool_trace.rs`, `tool_permissions.rs`, `sandboxing.rs`, `kask_compaction.rs`, `kask_thread_state.rs`, `templates.rs`; `crates/hkask-conversation-injector`, `crates/hkask-tool-invoker`; condensation via `kask_bridge::condenser_bridge` → `kask/crates/hkask-condenser/src/engine.rs`
- **Trigger:** user prompt submit from the agent panel; tool-result continuation within a turn
- **Hands off to:** L3 (tool dispatch), L5 (model streaming), L2 (skill spans/outcomes), L10 (turn-end memory ingest), L7 (thread events → panel), L12 (research-run sources from web tools)
- **Prediction:** 3 / 2 / 0.50
- **Phase 1 scoped graph (IS):** pending message/tools sensed in `crates/agent/src/thread.rs:2984-3006` → request/context assembled (`:3280-3334`) → streamed tool/refusal/truncation interpreted (`:4037-4115`) → tool dispatched (`:4371-4426`) → result marked completed/failed for the next round (`:3955-3980`), or the turn ends (`:3541-3595`). L3 takes MCP dispatch via `tools/context_server_registry.rs:705-732`; L7 observes thread events via `crates/agent_ui/src/conversation_view.rs:1475-1477`; L10 turn ingestion follows asynchronously (`thread.rs:3019-3086`). The ordinary model turn calls `stream_completion` directly (`thread.rs:3349-3354`): the L5 IPC handoff listed above is **not** claimed for that particular route. Five properties in this bounded scope: closed for tool-result continuation; timely conditional on retries and detached ingestion; accurate for stored tool status; complete only for the inspected path; actionable for tool errors, while a memory-write failure is log-only.
- **Phase 2 observation (IS, deferral):** `EndTurn` precedes detached memory ingestion (`thread.rs:3022-3086`), and the panel stop handler has no memory receipt (`conversation_view.rs:1802-1815`). INFERRED: a completed turn does not guarantee later recall. Falsifier: force ingestion failure and observe a distinct memory-success acknowledgement on the completed turn. A synchronous-ingest change would alter turn latency and fails behavior preservation; defer until a functional guarantee is specified. No deletion candidate admitted.
- **Phase 1–2 closure (2026-09-27, full scope):** the three submission routes — ordinary prompt (`agent.rs:3754-3767`), trace-wrapped prompt (`agent.rs:3615-3626`), and resume (`agent.rs:3897-3904`) — are thin closures over the ONE `run_turn` entry (`thread.rs:2984`); the loop tail (`thread.rs:3541-3597`) is a single path (end-turn decision → one `process_tool_result` for early and streamed results → bounded error retry via `retry_completion_error` → steering boundary → intent transition); request assembly and D6/D8 context injection re-verified at `thread.rs:3284-3335`, the deferred-results drain at `:3271-3278` (drain-on-next-iteration, no busy-spin — the .rules trap avoided by design). The auxiliary model calls (compaction `:3784`, summary `:4769`, title `:6497`) are one-shot paths outside the loop. **Ideal-method verdict:** the graph is already the small graph — one entry, one loop, one result processor, one retry function; the critical path is the short path. The only duplication signal (the block-conversion+send closure shared by the two prompt routes, ~6-8 lines) FAILS the admission test — rejected. **Five properties at full scope:** closed — IS (tool-result continuation and error retry both loop back; turn end flushes and emits EndTurn); timely — IS (bounded retries, per-iteration cancellation check; detached memory ingest remains the recorded deferral); accurate — IS (per-result tool status; typed truncation/refusal per D25/D36); complete — IS (all three submission routes verified through the one entry); actionable — IS for tool errors, log-only for memory-write failure (the standing deferral). **Prediction vs actual:** predicted 3 defects / 2 impedances / conf 0.50 → actual: 0 defects, 0 impedances, 1 deferral standing with falsifier (EndTurn precedes detached ingestion — awaiting an operator-specified functional guarantee), 1 consolidation candidate rejected on the admission test. Brier-scored at Phase 4.
- **Pass-2 delegation-authority arm (folded in 2026-09-28, IS):** the turn loop's dispatch arm carries a persisted, monotonically narrowing tool ceiling — `DelegationAuthority` (`crates/agent/src/delegation_authority.rs:16-19`, a `BTreeSet<DelegatedToolIdentity>`; `from_mcp_tools` rejects bare names and wildcards `:21-34`), threaded through thread state (`thread.rs:1064`), narrowed only by intersection (`:5176-5182`), inherited by worktree children via `delegation_for_child` (`:1618`), and persisted (`:2093`, `:2203`). The enforcement point is inside the loop's tool-dispatch arm: `run_tool` rechecks the hard ceiling per call — "even for calls cached or streamed before narrowing" (`thread.rs:4408-4421`) — failing with the typed result "tool is outside this thread's delegation authority"; `delegation_allows` (`:5195-5207`) blocks the ambient kata-kanban spawn route with the documented reason (the MCP child holds a server grant, not the initiating thread's grant — the P2 invocation-identity deferral, cited at the site) and otherwise consults `authority.allows(tool.delegation_identity())` (identity impls at `context_server_registry.rs:653/:890`). The arm is closed (recheck per dispatch), timely (synchronous), accurate (typed refusal), complete (builtin + MCP identities), actionable (the refusal names the cause). No finding: the P2 end-to-end invocation-identity carry remains the recorded operator-decision item.

- **Pass 3 layer classification (2026-09-30):** primary **B**; secondaries A (retry/truncation-typing/drain machinery), C (turn-end memory ingest). Sense→report→actuate: sense pending messages/tools (`thread.rs:2984-3006`) → report tool/refusal/truncation in-thread + panel events (`thread.rs:4037-4115`; `conversation_view.rs:1690-1704`) → actuate next-round dispatch or turn end (`thread.rs:4371-4426`, `:3541-3595`). Five properties: the pass-1 full-scope verdicts stand, no delta. Impedance (standing, L1-shared): EndTurn precedes detached memory ingest (`thread.rs:3022-3086`) — endpoints turn completion (L1) × curator ingest receipt (L10) — dimension: B→C handoff receipt. Grill: a primary-A reading (the loop's deterministic machinery) mistakes the arm for the loop; the loop's function is in-thread response. Reclassification falsifier: a headless batch mode with no in-thread consumer would make it A+C. Alignment: INV2 absent on the in-thread report path (tool status recorded, no expected-outcome carriage); INV4 held (typed truncation/refusal); INV1 partial (the report fans to panel, memory, and traces with no single pathway).

### L2 — Regulation/curator cybernetic cycle — accepted-check loss fixed by bounded in-queue retry; exhaustion escalates to the board
- **Crate/path:** `kask/crates/hkask-regulation` (runtime.rs, cybernetics_loop.rs, cybernetics_loop/cycle.rs, metacognition.rs, set_points.rs, energy.rs, dampener.rs, sensor_provider.rs)
- **Entry point:** `kask/crates/hkask-regulation/src/metacognition.rs:346` `run` / `:405` `tick`; cycle stages at `cybernetics_loop/cycle.rs:305` `sense`, `:409` `compute`, `:450` `act`, `:723` `prepare_impact_checks`, `:781` `escalate_exhausted_checks`, `:810` `verify_impact`; facade `cybernetics_loop.rs:278` `CyberneticsLoop::new`
- **Participants:** `RegulationLedger` (constructed `crates/zed/src/main.rs:790`), set points (`main.rs:773-784`), alert channel (`main.rs:632-641`), directive inbox (`main.rs:796-803`), email sink (`main.rs:656`, `kask/crates/hkask-email`); zed-side sensor bridges: `kask_bridge/src/context_server_health_bridge.rs:39`, `ocr_health_bridge.rs:36`, `rollout_event_bridge.rs:104` `poll_once`, `algedonic_log_bridge.rs`, `inference_resilience.rs`, `metacognition_bridge.rs`, `directive_bridge.rs`; wired on the kask tokio runtime (`crates/zed/src/main.rs:579-593`)
- **Trigger:** composition-root tick drivers gated on `kask.curator.always_on` (`main.rs:1225-1243`, D8/F10): CyberneticsLoop @10s (`:1227-1236`), MetacognitionLoop @30s (`:1239-1242`, self-interval `metacognition.rs:346-354`), harness-regression monitor @60s (`:1256-1300`, backpressure/retry semantics `:1272-1296`); alert channel (`main.rs:632-641`); `curator_directive` tool → `process_inbox` (`cybernetics_loop.rs:672-687`)
- **Functional graph (Phase 1, IS-cited per node; citations re-measured 2026-10-01):** CyberneticsLoop::tick (`cybernetics_loop.rs:806-946`, serialized by `impact_tick` `:807-809`): sense (sensor providers + observations cache) → escalation-sink reconcile (`:819-823`) → compare → compute (advisories) → act (`cycle.rs:477-593`: E04 cap-exhaustion captured BEFORE the per-tick reset `:506-524`, `reset_all_caps`, alert fan-out) → `route_action_as_alert` (`:596+`: board + live channel + archive fallback + email, dedup latches, retention authority `:477-505`) → claim accepted checks (`:826-837`) → `prepare_impact_checks` (`cycle.rs:752`: bounded read retry, ready/retry/exhausted) → worklist reconcile before any awaited effect (`:840-853`) → `escalate_exhausted_checks` (`cycle.rs:810`: board + live channel, no verdict) → `verify_impact` (`cycle.rs:840`, evidence already read) → `ledger.record_cycle_outcome` (`:877`) → loop-quality telemetry (fingerprint suppression, hourly heartbeat, `:891-946`). MetacognitionLoop::tick (`metacognition.rs:405-435`): ledger + regulation health + skill-feedback drift sense (`:447+`) → compare (`:535`) → act (`:592`, drains the CyberneticsLoop alert channel at `:659`) → snapshot surfaced via `curator_status`.
- **Findings (Phase 2, adjudicated):** **F1 IS, no action** — the two-loop split is the minimal shape: two required cadences (10s actuation vs 30s observability) and a one-way channel decoupling failure domains; merging couples them (a slow drift pass would delay cap-exhaustion escalation) — merge REJECTED on behavior grounds (also pinned by D8/F3/F10). **F2 IS, no action** — sensor no-data discipline enforced and documented (`sensor_provider.rs:138`/`:158`, the `unwrap_or(0)` trap named and avoided); the `dampener.rs:319`/`extrapolation.rs:55` hits are computation guards with local invariants, not sensor reads. **F3 IS, verified** — `always_on` has a real enforcement point (`main.rs:1226`). **F4 IS, informational** — the tick loops carry no cancellation tokens; process-lifetime loops owning no child processes (contrast: the MCP runtime's lifecycle latch exists for child-process death, L3). **F5 IS** — harness-monitor degradation surfaced (`Backpressured`/`Err` logged). **Bridge fleet examined:** each bridge implements a distinct hkask-regulation trait across a documented GPUI/tokio boundary; a shared snapshot-cell generic over the two health bridges adds indirection and saves ~20-40 lines — FAILS the admission test, rejected.
- **Five properties (post-slice):** closed for ordinary alert/ledger resensing and for accepted rollout checks — a store-read error retains the check in the same bounded queue within `MAX_ROLLOUT_READ_ATTEMPTS` (3) (`cybernetics_loop.rs:814-827`; `cycle.rs:751-765`), and a check exhausting those retries escalates to the review board without a verdict (`cycle.rs:760-765`, `:781-804`), keeping the absent-verdict restart rescan as the recovery backstop; timely at the periodic tick with a bounded retry horizon; accurate for surfaced query warnings and exhaustion escalations; actionable for ordinary alerts and for a dropped check (board card `rollout_check_unverifiable:<metric>`). The pre-slice closure gap (recorded below) is fixed and pinned.
- **Prediction vs actual (finalized by this audit, 2026-09-27):** predicted 2 defects / 2 impedances / conf 0.55 → actual: 1 reproduced behavioral defect (the accepted-check loss — found by the audit's reproduction, fixed by the landed slice `a2321f0df2`), 0 impedances, 0 surviving consolidation candidates (the two-loop split, the bridge fleet, and the exhaustion path were each examined and resolved on evidence). Brier-scored at Phase 4.
- **Phase 2 reproduced finding and Phase 3 gate:** IS — a temporarily inserted public `submit_rollout_impact_check` → `tick` test accepted one check, forced `RolloutEventSource::metric_before_and_after` to return a query error, then observed the pending queue length **0 rather than 1** (command `bash kask/scripts/cargo-test-nonzero.sh -p hkask-regulation --lib accepted_impact_check_survives_transient_read_failure`, 1 failed at `cycle.rs:1949` in the temporary test). The diagnostic test/import were removed after the red result so the tree is not left broken. Root path: tick drains with `mem::take` (`cybernetics_loop.rs:777`), verifier warns and skips (`cycle.rs:763-772`); next tick cannot observe that accepted check. Falsifier: a future public-seam regression test sees a retained check after error and one verdict after recovery. The operator selected **bounded automatic retry**. A naive requeue after the await is unsafe: concurrent submissions can fill the 64-slot queue (`cybernetics_loop.rs:555-565`) during verification, so restoring accepted checks would exceed the cap or discard newer accepted checks. Reserving in-flight capacity and retry attempts requires additional state; no behavior-preserving, net-negative replacement has survived the deletion test. **Operator ruling (2026-09-27 checkpoint): the bounded corrective slice was permitted.** An implementation matching the approved design (read-before-removal, bounded `read_attempts`, capacity-safe retention) validated green on a 2026-09-27 worktree snapshot (hkask-regulation --lib 96/96, kask_bridge rollout-filtered 19/19, `./script/clippy -p hkask-regulation`/`-p kask_bridge` clean, `cargo check -p zed` passed) but was **not landed by this audit**: the authoring stream is live on the same files and has extended the design — exhausted checks now escalate to the review board (`cycle.rs:781` `escalate_exhausted_checks`) — with one red test mid-iteration at observation time (`accepted_impact_check_exhausts_bounded_read_retries`). This audit verifies L2 after that stream lands; the stale `metric_before_and_after` comment fix in `hkask-mcp-swarm/src/local_tools.rs` (already in the worktree) must land with that slice. **Landed (2026-09-27, authoring stream):** the permitted design landed with the exhaustion-escalation extension — `prepare_impact_checks` returns ready/retry/exhausted, the tick reconciles the claimed prefix before any awaited effect (`cybernetics_loop.rs:822-827`) and escalates exhausted checks to the board and live channel without a verdict (`cycle.rs:781-804`). The red test observed mid-iteration is green; its root cause was test-environmental — the third tick crossed the inference-wiring grace, so the unwired-model alert also reached the board — fixed by wiring `HealthyResilienceSource` in the test, with no production change. Pins: `accepted_impact_check_retries_a_failed_read_then_verifies_once` (the named falsifier), `accepted_impact_check_exhausts_bounded_read_retries` (queue empty, no verdict, one board escalation), `retained_impact_checks_count_against_the_admission_bound` (retained failures count against the 64 bound, no displacement), `verify_impact_store_error_retries_without_verdict`. Receipts: hkask-regulation --lib 96/96, kask_bridge --lib 251/251, rustfmt --check clean on the four files, `./script/clippy` clean, `cargo check -p zed` passed. The swarm stale-comment fix (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:3526`) lands in the same commit.

- **Pass 3 layer classification (2026-09-30):** primary **A**; secondaries B (toasts via `ToastAlertSink`, `curator_status` snapshot, board cards in panels), C (archive → `reg_query`/algedonic review). Sense→report→actuate: sense (`cycle.rs:305` sensor bus) → report deviations/advisories through the alert fan-out (`cycle.rs:568+`; pathways R5/R6/R7/R10) → actuate (`cycle.rs:450-562`). Five properties: post-slice verdicts stand. Impedance (**corrected in Pass 4 — the initial drop finding was REFUTED by the live-log experiment**): the in-host tracing→log bridge is an undocumented incidental coupling — endpoints kask `tracing` emission (558 warn sites) × zed's `crates/rpc/Cargo.toml:35` (the graph-wide `log` feature that makes the fallback fire) — dimension: provenance/documentedness (the pathway WORKS — empirically confirmed, 2,173 forwarded lines in the live log — but its existence depends on an upstream crate's feature flag that no kask doc records; dropping the flag, or activating a Tracy subscriber, silently removes these lines from the log). Grill: a primary-C reading confuses the loop with its consumer (the review); per-action human approval of actuation would force reclassification. Alignment: the register's exemplar — INV2 held (`Signal.set_point` + `Deviation`, `loops/signals.rs:332-343`), INV3 held (coalescer, `cybernetics_loop.rs:895-968`), INV4 held (exhaustion escalation), INV1 partial (four report pathways serve this one loop).

### L3 — MCP client runtime: spawn / health-supervise / request cycle — consolidated; current-tree tests and build passed
- **Crate/path:** `kask/crates/hkask-mcp/src/runtime.rs` (21 library tests + 16 serialized `tests/reconnect_integration.rs` tests observed at `ff3bae88e6`)
- **Entry point (current tree):** `runtime.rs:1505` `ToolPort::invoke` → `:1642` `call_tool_inner` → `:1701` `dispatch`; `:1282` `try_reconnect`; `:633` `start_server_with_env` → `:658` `start_recorded`; `:1050` `spawn_health_supervisor`; `:1428` `stop_server`
- **Participants:** `McpRuntime` per-server `ServerEntry` (`runtime.rs:466-481`) under one `entries` map (`:490`), generation-stamped keeper task, zed-side registry `kask_bridge/src/mcp_servers.rs` (L14 boundary), 12 child servers
- **Trigger:** agent tool invocation (`runtime.rs:1505`); periodic health tick (`:1050-1064`); explicit start/stop (`:633`/`:1428`)
- **Functional graph (Phase 1, IS-cited per node):** invoke (`:1477`) → governance charge + runaway-loop breaker (`:1498-1536`, the one pre-dispatch refusal; auto-registration instead of denial `:1509-1517`) → `call_tool_inner` (`:1615`): live-peer check → `try_reconnect` (cooldown check + stamp under one write lock, `:1273-1288`) → `dispatch` (`:1674`): three-way failure classification — `NotDelivered` (provably not run; reconnect and retry once `:1641-1657`), `Interrupted` (effect unknown, never auto-retried `:1708-1722`; `DispatchError` `:1779-1805`), `Failed` (`:1723`); call timeout inside `TokioContext` (`:1698-1705`); post-call span emit (`:1542-1554`), per-server reliability `record_outcome` + variety `record_variety` (`:1574-1585`). Parallel supervision cycle (`:1041-1209`): interval tick → classify Healthy/TransportClosed/Missing (`:1063-1070`) → reset or increment failures, saturating (`:1116-1121`) → remove a dead entry only if still closed (`:1092-1101`) → restart via recorded spec, concurrent-safe with the call path (`:1126-1128`) → circuit breaker stops the respawn loop with an operator-actionable error (`:1155-1177`, the 2026-08-29 crash-loop fix); deliberately stopped servers are never resurrected (`:1133-1142`).
- **Findings (Phase 2, adjudicated):** **F1 impedance, DEFERRED** — the typed error kind crosses the L4→L3 seam string-marshalled: `dispatch` formats `[kind] text` (`:1732-1738`) and governance re-parses it (`:1572`) via shared `error_kind_from_display` (`hkask-types/src/tool_response.rs:123`); both ends single-copy and tested. Typed carry through `ToolPortError` would add a field plus construction and matches against a pinned display contract — net-positive lines — so deferred. **F2 IS, no action** — dual reapers (keeper `:860-881`, supervisor removal `:1092-1101`) are both load-bearing (event-driven reap vs poll-window closer) and cannot race destructively (generation stamp `:861-864`, liveness re-check `:1095-1098`). **F3 IS, no action** — dual rate-limiters (call-path cooldown `:1276-1288`, supervisor interval + circuit breaker) bound different triggers. **F4 OUGHT, out of scope** — no ping-based health check; a hung-but-alive server reads Healthy (`:1029-1030`, documented future enhancement; adding it is a new feature and fails the admission test).
- **Five properties:** closed — IS (spawn → supervise → reap → reconnect → circuit-break; transitions pinned by the 22 in-file tests and `reconnect_integration.rs`); timely — IS (call timeout, cooldown, interval, breaker all bounded); accurate — IS (three-way delivery classification, unknown-effect never retried, typed-kind ledger breakdown); complete — IS (13 servers; tool-surface membership is event-driven, `:574-585`); actionable — IS (the breaker's error names the operator action `:1167-1175`; `unavailable_error` distinguishes NotFound / never-started / not-connected `:1746-1773`).
- **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects, 1 impedance deferred with reason (F1). Brier-scored at Phase 4.
- **Hands off to:** L4 (server side of each call), L2 (record_outcome/record_variety + spans), L14 (zed-side registry and env).
- **Current-tree supersession of the historical line numbers above:** commit `16271e3c60` replaced the six per-server maps with `ServerEntry` (`runtime.rs:466-481`), adding 264 and removing 226 production lines (**net +38**, contrary to the Phase 0 estimate of −150–250). The old graph and F1–F4 citations above describe the pre-consolidation tree. In the current tree: invoke meters then emits settled span and ledger outcome (`:1505-1624`); call path checks live peer, reconnects with cooldown, and retries only `NotDelivered` (`:1642-1686`); dispatch classifies unknown-effect `Interrupted` without replay (`:1701-1750`); supervisor senses closed/missing transport, increments failure count, attempts restart or circuit-breaks (`:1050-1217`); stop clears entry and cancels supervisor/children (`:1428-1456`). These nodes form the return path from tool-call outcome and next health tick to renewed dispatch. **Verified in the current worktree at `ff3bae88e6`:** `bash kask/scripts/cargo-test-nonzero.sh -p hkask-mcp --lib` (21 passed), `bash kask/scripts/cargo-test-nonzero.sh -p hkask-mcp --features test-fixture --test reconnect_integration -- --test-threads=1` (16 passed), `./script/clippy` (completed including kask-scoped machete and buf checks), `cargo check -p zed` (passed). An earlier integration invocation without serialized threads failed; it did not follow the test file's required protocol (`tests/reconnect_integration.rs:20-33`) and is not counted as a regression. The deferred L4→L3 typed-error impedance still requires a current-line recheck; no further L3 deletion admitted.
- **Pass 2 (2026-09-28): E1 typed-error impedance CLOSED, resolved-by-drift.** The kind now crosses the wire structurally: `dispatch` reads `structured_content` through `parse_tool_error_value` (`runtime.rs:1785-1791`, envelope kind validated via `McpErrorKind::from_kind_str`, `tool_response.rs:100-109`) and formats `[kind] text` only as the display detail; `invoke` extracts the kind through the enum-validating `error_kind_from_display` (`runtime.rs:1618-1626`, `tool_response.rs:123-133`), pinned by seven tests including the unknown-kind-returns-full-text discipline (`tool_response.rs:215-374`). The original "string-marshalled seam" finding no longer holds; the residual display round-trip is intra-L3, validated, and pinned. No lines changed — the drift landed via the concurrent streams.
- **Pass-2 citation re-map (2026-09-28, post-`a7445fa213`/`ac58daba43` drift):** current entry points verified in-tree — `invoke` `runtime.rs:1531`, `call_tool_inner` `:1668`, `dispatch` `:1727`, `try_reconnect` `:1308`; the E1 citations hold at current lines (`error_kind_from_display` `:1625`, `parse_tool_error_value` `:1788`). The row's earlier supersession-paragraph line numbers (`:1642`/`:1701`) describe the pre-drift tree and are superseded by these.

- **Pass 3 layer classification (2026-09-30):** primary **A**; secondary B (the breaker's operator-actionable error surfaces in-thread). Sense→report→actuate: sense health tick (`runtime.rs:1050-1064`) → report reliability `record_outcome`/`record_variety` + spans (`runtime.rs:1574-1585`) → actuate reconnect/breaker/restart (`runtime.rs:1308`, `:1155-1177`). Five properties stand. Impedance: the child-stderr forward (`runtime.rs:813-830`) reaches the log only through the incidental tracing→log fallback (L2's corrected finding) — endpoints child stderr × zed log file — dimension: provenance (the forward WORKS — empirically confirmed in the live log — but its `server_id` wrapper field does not survive the fallback rendering; the child lines self-identify). Grill: primary-B fails — the surfaced breaker error is one arm; the loop's responses are autonomous. Reclassification falsifier: operator-approved reconnection. Alignment: INV4 held (the breaker names the operator action); INV2 absent (outcomes recorded without expected-reliability carriage — the comparison lives in L2).

### L4 — MCP server request cycle (shared framework, 13 servers) — AUDITED & CLOSED 2026-09-27
- **Crate/path:** `kask/crates/hkask-mcp-server/src/server/` (transport 131, error 163, validation 611, credentials 144, context 163, tool_span 170) + `kask/mcp-servers/*`
- **Entry point:** `server/transport.rs:32` `run_stdio_server` — the single shared bootstrap: tracing init → DB catalog (`:52-68`, startup refused on inventory failure) → credential resolution, required and optional each surfaced (`:72-91`) → WebID (`:93-105`) → capability tier (`:108`) → factory-gated construction (`:119`, no ambient env authority) → rmcp stdio serve (`:125-129`)
- **Participants:** shared `execute_tool` (ONE definition, `server/tool_span.rs:162`, used by all 13 servers); shared envelope `hkask_types::tool_response` (`unwrap_tool_envelope`, `parse_tool_error_value` `:100`); typed error taxonomy (`server/error.rs:16` `McpError` per-variant; `McpToolError` carrying `kind: McpErrorKind`); shared input validation (`validation.rs`: identifier/path validation, per-source error mapping `:82-139`, path containment `:302-375`, capped reads `:590`); 13 thin binary mains (9 lines each, e.g. `hkask-mcp-companies/src/main.rs` — library servers for fuzz testability)
- **Trigger:** stdio JSON-RPC request from the zed host
- **Functional graph (Phase 1, IS-cited per node):** host spawn (L3 `start_server_with_env`, env built by `build_mcp_server_env` `kask_bridge/src/mcp_servers.rs:681`) → bootstrap (`transport.rs:32`) → rmcp dispatch → `execute_tool` (`tool_span.rs:162`, span emission) → tool fn → `{"content": ...}` envelope or typed `McpToolError` with `structured_content` kind (consumed by L3's `dispatch` `:1732-1738`).
- **Findings (Phase 2, adjudicated):** framework single-copy verified — envelope, error taxonomy, span emission, validation, and bootstrap each have ONE implementation; no per-server duplication found. **Considered and rejected:** merging the 12 binary mains into one multi-server binary would delete ~99 lines but breaks per-server process isolation — per-server credential env allowlists and crash domains are functional requirements (`.rules` MCP server patterns), so behavior preservation rejects it.
- **Five properties:** closed — IS (request → validate → execute → envelope → span; L3's `record_outcome` closes the loop at the governance layer); timely — IS (stdio, no polling); accurate — IS (typed per-variant errors; startup refuses on inventory failure rather than degrading); complete — IS (13 servers, one framework); actionable — IS (named missing credentials `:87-91`; per-variant `McpError` context).
- **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects, 0 impedances (the F1 seam impedance is recorded on L3, its formatting side). Brier-scored at Phase 4.
- **Hands off to:** per-domain loops L6, L8–L13, L17–L19; L2 (tool spans/outcomes); L3 (the client side of every call).

- **Pass 3 layer classification (2026-09-30):** primary **A**; secondary B (the typed error reaches the in-thread caller). Sense→report→actuate: sense request (`transport.rs:32`) → report envelope + span (`tool_span.rs:162`) → actuate typed error or result. Five properties stand; no impedance. Grill: primary-B fails — server feedback is deterministic classification, not in-thread response. Alignment: INV1 held (one framework, one span path); INV2 absent (spans record outcome, not expectation).

### L5 — Inference bridge (zed ↔ hkask IPC) — audited & closed: error classes and Api status round-trip; minimalism pass landed
- **Crate/path:** `kask/crates/kask_bridge/src/inference_*.rs` + `kask/crates/hkask-inference`
- **Entry point:** `kask_bridge/src/inference_ipc_server.rs:388` `UnixListener::bind` (2,630 ln); chat surface `inference_chat.rs` (2,001 ln); socket state `inference_socket.rs:24` `set_inference_socket_path` (env-injected into MCP servers via `mcp_env.rs`, `crates/zed/src/main.rs:204-209`, `:674`); client `kask/crates/hkask-inference/src/inference_ipc_client.rs`
- **Participants:** providers (`hkask-inference/src/provider.rs`, `openai_compat.rs`), rerank, media router; zed `LanguageModelRegistry` (D24)
- **Trigger:** chat/embedding/rerank/tool-dispatch requests from MCP server children and zed-side providers
- **Functional graph (Phase 1, IS-cited per node):** MCP child → client (`inference_ipc_client.rs`, constructs no error payloads — one `From` conversion `:987`) → Unix socket with peer-uid ownership gate (`inference_ipc_server.rs:302`), private socket dir (`:270`), `CappedReader` line cap (`:198`) → `handle_connection` `:601` (one in-flight per connection; EOF during dispatch cancels provider work `:658-669`; peer-cancellation EPIPE classified debug-not-warn `:684-717`) → `dispatch` `:776` — per method: Embed → embedding port; ListModels / CreateWorktreeThread → GPUI-side channel round-trips (`AsyncApp` not `Send`); ToolDefinition / ToolInvoke → ToolPort with the request allowlist as the REAL authority boundary (the DelegationToken self-comparison fix documented `:883-894`, fail-closed missing-allowlist); Rerank → API key via keychain channel, MCP servers never see keys (`:999-1083`); Generate/GenerateWithModel/GenerateWithMessages/GenerateVision → `InferencePort` fall-through with a DoS-safe unreachable arm (`:1130-1140`) → newline-JSON `InferenceOutcome` back. Port side: `inference_chat.rs` — `LanguageModelInferencePort` (`:426`, `InferencePort` impl `:920`), `NoModelInferencePort` boot-grace stub (`:1151`), request lifetime/deadline + in-flight guards, `InferenceResilienceSource` (`:1071`, feeds L2). Protocol types shared in `hkask-types/src/inference_ipc.rs` — no client/server duplication.
- **Findings (Phase 2, adjudicated):** **F1 CONSOLIDATED (`4eaca76874`)** — 29 hand-built `InferenceOutcome::Error` payload constructions (5-6 lines each) collapsed into a private `ipc_error(code, message)` helper; net **−95 production lines** (+114/−209); behavior byte-identical — the module's 33 tests green (including the authority tests pinning the payloads), `cargo check` + `./script/clippy -p kask_bridge` green (validated in a detached worktree during that prior pass). The 2 payload-only constructions in `WorktreeSpawnRequest::execute` (`:84/:88`) left: different shape, already compact; a second constructor for 2 sites fails the admission test. **F2 IS, no action** — the four Generate* arms map 1:1 to distinct port methods. **F3 IS, no action** — protocol shared in hkask-types; client never constructs payloads. **F4 IS** — the security layer (peer-uid, private dir, line cap, EOF-cancel, EPIPE classification) is incident-documented; no findings.
- **Minimalism pass (landed 2026-09-28, closing this row's last deferral):** four behavior-preserving consolidations admitted under the deletion test — `prompt_messages` (the `[system, user]` pair was built verbatim in four trait methods), `dispatch_completion` (the verbatim oneshot-dispatch tail of `generate_with_messages`/`generate_vision`), `complete_circuit` (the verbatim transient/permanent classification tail of both receiver arms; consumes the permit, as `complete` records one completion per permit), and `generate` delegating to `generate_with_model(None)`. Net **−21 production lines** (+64/−85), all above the tests boundary; kask_bridge --lib 252/252 green over the consolidated code (every circuit/deadline/semaphore pin intact), rustfmt clean, scoped clippy clean. **Rejected with reason:** merging the two receiver arms (two channel types with different reply semantics — a generic dispatch interface would cost more complexity than the duplication saves); `StreamAccumulator::into_result`/`into_final_chunk` (distinct output types); the error-classification fns and deadline/guard machinery (incident-hardened, each pinned, already minimal).
- **Five properties:** closed — IS (request → dispatch → port → outcome → client; the L2 resilience sensor closes the observation arm); timely — IS (request deadlines, EOF-cancel); accurate — IS (error classes and the provider's Api status round-trip the seam; EPIPE classified); complete — IS (10 methods, all dispatched); actionable — IS (codes name the failure class; permission-denied messages name the setting to fix).
- **Pass-2 citation re-map (2026-09-28, post-`df49e1497b` drift):** verified in-tree — `dispatch` `inference_ipc_server.rs:776` holds, the Embed arm's separate dispatch at `:791-796` (embedding-port-missing is a typed `ipc_error` naming the wiring bug), `handle_connection` `:601` holds; `inference_chat.rs` is 1,980 lines post-consolidation with the minimalism-pass helpers in place (`complete_circuit` `:686`, `dispatch_completion` `:791`, `prompt_messages` `:913`, `generate_with_messages` `:945`). The row's earlier citations otherwise hold; no new finding — the drift was the recorded consolidation landing.
- **Prediction vs observed follow-up:** prior pass scored 0 defects / 0 impedances but missed the embedding readback: IS — server emits `Json` (`inference_ipc_server.rs:772-793`) and the client formerly mapped it to `Connection` (`inference_ipc_client.rs:483-488` before this edit). At the approved IPC seam, `embedding_ipc_preserves_json_error_class` failed before and passed after the client maps `Json` to `EmbeddingGenerationError::Json` (`inference_ipc_client.rs:483-487`). The existing `InvalidRequest` and fallback behavior remain represented in the same match; `hkask-inference --lib` ran 54/54 tests. The source change is committed in `e1f1b51cad` (alongside unrelated ontology work), net −1 production line (+5/−6), with 25 test lines added. **Remaining impedance deferred:** `Api` errors are sent as a status-bearing string (`inference_ipc_server.rs:778-780`) but still read as `Connection` (`inference_ipc_client.rs:486`); structured status recovery needs a separately agreed protocol/test seam and may add lines rather than delete them. Risk: a provider API failure can be misclassified as retryable. Falsifier for a future slice: an IPC test sends a server-shaped `Api` response and observes `EmbeddingGenerationError::Api` with the original status. Re-score the Phase 0 prediction only when the row closes. **Api readback landed (2026-09-28, this slice):** the protocol seam was agreed and landed — `InferenceErrorPayload` carries a wire-optional `status: Option<u16>` (`inference_ipc.rs:297`; absent parses as `None`, `None` serializes without the field, so the pre-field wire shape is preserved — pinned by `error_payload_status_is_wire_optional`), the server classifies through the extracted `embed_error_outcome` (`inference_ipc_server.rs:749`) with the status traveling structurally instead of the former `status {status}: {m}` message prefix, and the client reconstructs `EmbeddingGenerationError::Api(status, _)` (`inference_ipc_client.rs:492`); status-less Api-coded payloads (`EmptyResponse`, `DimensionMismatch`) fall back to `Connection` — absence, never a fabricated status. The named falsifier is pinned red-then-green: `embedding_ipc_preserves_api_error_status` observed `Connection("Api: rate limited")` before the client fix and `Api(429, "rate limited")` after. The former copy-pinned classification test (it re-implemented the dispatch match inside the test) was replaced by a real pin over the extracted classifier, `embed_error_outcome_classifies_variants_and_preserves_api_status`. Receipts: hkask-inference --lib 55/55, hkask-types --lib 87/87, kask_bridge --lib 252/252, rustfmt --check clean, scoped `./script/clippy` clean; `cargo check -p zed` is blocked by an unrelated committed syntax error in `hkask-kanban-widget/src/view.rs:2011` (`f6806d5461`, the concurrent widget-subtraction stream — named for that stream, not fixed here). Deltas: production +56/−23 (net +33; this row predicted the fix "may add lines rather than delete them"), tests +94/−21. The row's Phase 1 graph citations were re-verified and updated to the current tree in the same pass (dispatch `:776`, DelegationToken `:883-894`, Rerank `:999-1083`, unreachable arm `:1130-1140`, port-side impls `:920`/`:1151`/`:1071`).
- **Hands off to:** L1 (streamed tokens), L2 (inference-resilience sensor, `cybernetics_loop/cycle.rs:144` `sense_inference_resilience`), L6 (embeddings)

- **Pass 3 layer classification (2026-09-30):** primary **A**; secondary B (error classes round-trip to the in-thread caller). Sense→report→actuate: sense request/deadline/EOF (`inference_ipc_server.rs:601`) → report `InferenceOutcome` (protocol in `hkask-types/src/inference_ipc.rs`) → actuate cancel/classify. Five properties stand; no open impedance (the Api-status readback landed). Grill: primary-B fails as L4 — the bridge's feedback is deterministic. Alignment: INV4 held (EOF-cancel, EPIPE classification); INV2 absent.

### L6 — Corpus pipeline cycle — audited & closed; end-to-end deferred to a caller-selected corpus
- **Crate/path:** `kask/mcp-servers/hkask-mcp-corpus/src`
- **Entry point:** `tools/document.rs:35` `corpus_convert`, `:355` `corpus_chunk`; `tools/tagging/ops.rs:263` `corpus_tag_chunks`; `tools/semantic.rs:228` `corpus_embed`, `:170` `corpus_generate_qa_batch`; `tools/corpus.rs:447` `corpus_ground_generated_qa`, `:174` `corpus_ingest_qa`
- **Trigger:** per-tool requests chained by skills (convert → triage/OCR → chunk → tag → embed → prompts → QA → ground → ingest → assemble)
- **Hands off to:** L5 (embeddings/rerank), L10 (corpus DB), L18 (assembled training datasets)
- **Prediction:** 3 / 2 / 0.55
- **Phase 1 graph (IS, all citations re-verified 2026-09-27):** convert (`document.rs:35`, OCR staging + quality-gated resume) → chunk (`document.rs:353`, ONE bounded structural/sentence engine for all modes) → tag (`tagging/ops.rs:259`) → embed (`semantic.rs:226`, ontology-anchored via the L5 router, requested/actual model identity surfaced) → calibration (`calibration.rs:161` `corpus_build_chunk_representations`, fails closed on provenance/fidelity mismatch) → prompts (`corpus.rs:136` `corpus_build_prompts`) → QA generation (`semantic.rs:166`, admitted ONLY through the identity-bound prepared-qa-adjudication-v2 contract; one bounded generator-owned correction per failure class; generation never authorizes ingestion) → grounding (`corpus.rs:445`, deterministic zero-inference bundle, authorizes nothing) → ingestion (`corpus.rs:172`, the gate re-executes every mechanical check — the artifact is never authority; `model_inference` answers are never relabelled verified) → retrieval feedback (`storage.rs:81` `corpus_query`, KNN over stored passages; `answer_error` reports why grounding was unavailable), closing the calibration cycle against the build-corpus-pipeline skill's verification stages.
- **Phase 2 adjudication (closed 2026-09-27):** the one ideal-method candidate — the ingest gate's re-execution of the grounding checks — is NOT consolidatable duplication: its independence from the artifact is the pinned fails-closed invariant (a mixed batch with an earlier valid candidate and a later ungrounded citation cannot produce training output or a DB, on dry-run and real ingestion alike). Merging the gates would weaken that contract — REJECTED on behavior grounds. The known trap set (tag_batch_size array parsing, max_pairs default semantics, training_assemble_dataset db_path bridging) is fixed and pinned by prior landed slices.
- **Five properties:** closed — IS for the mechanical chain (every gate fails closed; the correction loop is bounded at one per failure class and terminal); the policy-feedback arm is agent/operator-mediated by design (Stage 8 semantic acceptance belongs to the operator); timely — IS (per-request, no background cycles); accurate — IS (deterministic zero-inference grounding; ingest re-execution; provenance lattice enforced); complete — IS for the inspected chain with the stated boundary: a source-complete end-to-end run requires training-dataset construction, which this audit's Phase 4 rules exclude — the tool-seam evidence stands instead (201/201 corpus library tests green on the current tree, 2026-09-27; Stage 7→9 seam confirmed: generator skips fail grounding); actionable — IS (typed per-stage errors naming identity, bijection, citation, provenance).
- **Prediction vs actual:** predicted 3 defects / 2 impedances / conf 0.55 → actual: 0 new defects, 0 impedances, 1 consolidation candidate examined and rejected on behavior grounds, 1 boundary stated with reason — another overestimate on an incident-hardened surface, a Phase 4 calibration finding. Brier-scored at Phase 4.
- **Phase 1 scoped graph (IS):** source extraction/chunking (`tools/document.rs:35-72,355-419`) → model classification (`tools/tagging/ops.rs:263-310`) → embedding (`tools/semantic.rs:228-250`, L5) → prepared prompts (`services/prompt_builder.rs:45-91`) → generation (`services/qa_pipeline.rs:1216-1277`) → grounding (`services/qa_grounding.rs:223-269`) → ingestion (`tools/corpus.rs:174-247`, L10 corpus DB) → explicit corpus-DB selection for training assembly (`hkask-mcp-training/src/tools/dataset.rs:86-129`, L18). The skill drives decisions and reconciles results; this is not one automatic server cycle. Five properties: closed conditional on caller reconciliation/retrieval; timely conditional on bounded waves; accurate only at the mechanical-citation gate; complete only after every source/stage count reconciles; actionable through surfaced failures and stop rules.
- **Phase 2 seam and process correction (IS):** generation can output `status="skipped"` (`services/qa_pipeline.rs:1252-1277`), while `read_grounding_candidates` rejects any skip-or-error row (`services/qa_grounding.rs:223-269`); the old skill Stage 9 passed the mixed generated file directly. Falsifier: show a mixed file accepted by the grounding gate or a prior candidate-only projection. The existing `build-corpus-pipeline` skill now replaces that handoff with a candidate-only projection filtering **only** reconciled skips, retaining the original file and reconciling counts/hashes before grounding and ingestion; no server contract or additional script was introduced. A synthetic `jq` probe kept candidate and error rows and excluded the skip; a public-tool test at `tools/corpus/ingest_tests.rs:mixed_qa_dispositions_project_to_grounded_candidates` then proved mixed input is rejected, candidate-only input grounds, dry-run ingestion retains one QA, and no training output is written (1/1 targeted; 201/201 corpus library tests). No source-complete/paid corpus run occurred, so L6 is seam-verified but not end-to-end complete. Further addition is deferred until a caller-selected corpus exercises the chain. The candidate-only file is the one whose hash the grounding manifest binds.

- **Pass 3 layer classification (2026-09-30):** primary **B**; secondary A (mechanical gates fail closed deterministically; bounded correction). Sense→report→actuate: sense per-tool results → report typed per-stage errors/counts → actuate the skill's next-stage decision (agent-mediated by design). Five properties stand (closed conditional on caller reconciliation). No new impedance. Grill: the gates alone are A, but the loop's driver is the in-thread chain; a fully automated corpus pipeline would reclassify to A. Alignment: INV4 held (fails closed); INV2 partial (identity-bound grounding contracts carry expected hashes/counts — expectation-like); INV3 absent (per-stage logs are raw activity).

### L7 — Agent panel & kask widget update/render loops — seams verified single-copy; the measured seam test landed: the optimistic-move notify bug fixed, the authoritative-refresh half pinned healthy
- **Crate/path:** `crates/agent_ui/src/agent_panel.rs` (14,366 ln) + kask widget/panel crates
- **Entry point:** `agent_panel.rs` observe/subscribe web (`:374` `observe_new`, subscriptions at `:1491`, `:1501`, `:2201`, `:2780`, `:3093`, `:4662`; `render_title_view` `:5437`)
- **Participants:** widgets `crates/hkask-{kanban,swarm,portfolio,media,scenarios,spreadsheet,graph}-widget` + `hkask-viz-core` (D18); panels `crates/{swarm_panel,kanban_panel,media_panel,portfolio_panel}` (D33), `crates/hkask-steer`; compose-back seam D21; sibling host D23
- **Trigger:** GPUI entity events from threads/tasks; user interaction
- **Hands off to:** L1 (prompt submit), L9 (kanban widget ↔ server), L13 (swarm panel ↔ server)
- **Prediction:** 2 / 2 / 0.45
- **Phase 1 graph (IS, citations re-verified 2026-09-27):** panel event→render cycle — `observe_new` `agent_panel.rs:374` (panel registration) and subscriptions wiring external entities to state mutation + `cx.notify()` (extension store `:1490`, project worktrees `:1500`, thread metadata `:1512`, terminal items `:2194`, draft editor `:3092`, conversation root-thread `:4685`; `render_title_view` `:5435` — the row's older cites drifted by ≤23 lines and one recorded site (`:2780`) no longer matches a subscription in the current tree). Widget render path — ONE block renderer: `hkask_viz_core::block_renderer()` composes all seven widgets behind the unchanged D18 callback (`render_agent_markdown`, `conversation_view.rs`), each widget recording render provenance via the shared `hkask_tool_invoker::record_render` (kanban `view.rs:133`, media `:269`, portfolio `:90`). Widget action paths — widget→MCP through the ONE `shared_tool_invoker` seam (a missing invoker surfaces as a visible error, pinned); widget→agent through the ONE `compose_back_via_injector` helper (D21 — its doc records the per-widget `compose_back` copies it already replaced), with draft-surfacing on inject error, never a silent no-op. Panel substrate — all four panels share the `hkask-steer` lifecycle (`SteerSurface`/`ensure_steer`/`ThreadPicker`/`VerticalSplitState`; D2 records the 2026-08-27 deletion of the per-panel hand-rolls and the hand-mirrored tool lists), and every Steer prompt renders its tool advertisement from the server's generated `TOOL_NAMES` with `verify_tool_advertisement` plus per-panel prompt-token tests as CI enforcement.
- **Phase 2 adjudication (closed 2026-09-27):** the duplication this row's prediction anticipated is already consolidated — by history, not by this pass: the per-panel Steer lifecycle (D2, 2026-08-27), the per-widget compose-back copies (D21 helper), the per-widget renderers (D18 viz-core registry), and the hand-mirrored tool lists (generated `TOOL_NAMES`) each landed with pins. The remaining per-panel code is legitimately specific (prompt grouping labels, viewer surfaces). No deletion candidate survives; the ideal-method verdict is that the graph is already the small graph — one renderer, two action seams, one panel lifecycle. **Reconciliation (v0.20.1):** this structural closure does not close the row's prior Phase 2 bounded observations — the two INFERRED findings there (a concurrent authoritative update possibly unseen after move completion; optimistic mutation possibly delaying visible repaint) remain OPEN with their falsifiers, deferred behind the measured panel seam test, which is itself deferred behind the concurrent in-flight widget subtraction (v0.19.1 hazard note — the row's citations name lines being rewritten). The L1-shared memory deferral (panel stop handler has no memory receipt, `conversation_view.rs:1802-1815`) stands with L1's row. Four of the seven widget crates were under live concurrent edit during this pass (kanban, graph, portfolio, scenarios); this slice is doc-only and touched none of them.
- **Five properties:** closed — IS (action → state → notify → re-render through the GPUI frame loop; widget actions close through the invoker/injector seams and the server display-hint path); timely — IS for notify-batched paths (D14 pins the 50ms streaming-reveal interval bounding event amplification), UNMEASURED for the optimistic-mutation path (the prior pass's inferred repaint-delay finding stands; falsifier: a GPUI rendered-frame check); accurate — IS (render reads entity state; advertisement verification prevents prompt drift; render provenance spanned); complete — IS for the inspected paths (one renderer composes all seven widgets; all four panels on the shared lifecycle); actionable — IS (missing invoker is a visible error; compose-back surfaces a draft on failure). **Prediction vs actual:** predicted 2 defects / 2 impedances / conf 0.45 → actual so far: 0 confirmed defects, 2 inferred findings open (deferred behind the widget rework, falsifiers recorded), 0 impedances — the anticipated duplication was already consolidated by the D2/D18/D21 refactors, each with cited pins. Finalized when the measured seam test lands. Brier-scored at Phase 4.
- **Phase 1 scoped graph (IS):** `AcpThreadEvent::NewEntry` reaches `conversation_view.rs:1475-1477,1736-1738` → entry/view sync (`:1742-1755`) → active-view change notifies `agent_panel.rs:4662-4677` → render consumes view (`:6641-6648`). For a kanban task move: click stages intent (`crates/hkask-kanban-widget/src/view.rs:662-692`), confirmation dispatches (`:279-289`), `move_controller.rs:194-229` applies optimistic state and invokes L9 tool, then clears/rolls back and notifies (`:230-253`). L1 compose-back is a separate editor prefill (`view.rs:919-931`); no L13 refresh claim follows solely from a swarm badge. Five properties in these two paths: closed conditional on authoritative update; timely unmeasured; accurate conditional on server readback; complete not established for other panels/widgets; actionable via dispatch status/error.
- **Pass-2 re-map + measured seam test (2026-09-28, landed `9142f4f03e`):** citations re-mapped against the landed widget state — `NewEntry` handling now at `conversation_view.rs:1690-1704` (entry/view sync via `sync_entry`), the confirm click at `kanban-widget view.rs:254-261`, `set_body`'s in-flight guard at `:170-196`, `dispatch_move` at `move_controller.rs:174-246`, `apply_optimistic_move` `:305-318`. **Finding 2 (optimistic repaint) CONFIRMED and FIXED:** `dispatch_move`'s success path applied the optimistic move and took the pending banner down without `cx.notify()` — only the error paths and the completion callback notified, so the moved card and the banner's removal stayed stale until the tool call resolved, defeating the comment's stated intent ("the UI reflects the move while the dispatch is in flight"). Diagnosed red-first: an observer on the widget entity with a never-resolving dispatch isolates the synchronous path (`confirm_move_notifies_before_the_dispatch_resolves` failed red, green after the one-line notify, symmetric with the error paths). **Finding 1 (authoritative refresh) widget-half PINNED HEALTHY:** `authoritative_body_lands_after_dispatch_completes` proves a post-completion `set_body` is accepted (the guard declines only while in flight). Residual design note (not a defect): the widget itself never fetches — the authoritative refresh arrives via the conversation's next block render; if the agent turn ends without re-emitting the block, the optimistic state stands. Both tests live in the widget's suite (62/62 green). The L1-shared memory deferral stands.

- **Pass 3 layer classification (2026-09-30):** primary **B** (the conscious surface — panels, widgets, toasts). Sense→report→actuate: sense entity events (`agent_panel.rs:374` subscriptions) → report render via `cx.notify()` (R11) → actuate user action through the invoker/injector seams. Five properties stand (timely now measured — the seam test landed `9142f4f03e`). Impedance (standing, L1-shared): the panel stop handler has no memory receipt (`conversation_view.rs:1802-1815`) — endpoints panel stop (L7) × memory ingest (L10) — dimension: B→C handoff completeness. Grill: the GPUI frame loop alone is A, but this row is the human-facing surface; a never-observed headless panel would dissolve into A. Alignment: INV1 held by design (one renderer, two action seams — the D18/D21 consolidations); INV2 absent.

### L8 — Forecast/calibration loop — AUDITED & CLOSED 2026-09-27
- **Crate/path:** `kask/crates/hkask-forecast` + `kask/mcp-servers/hkask-mcp-{companies,prediction-markets}`
- **Entry point:** `kask/crates/hkask-forecast/src/hkask_forecast.rs:190` `brier_score`, `:196` `brier_score_multi`, `:244` `wilson_bounds`, `:279` `apply_calibration_adjustment`, `:308` `isotonic_fit`; market leg: `hkask-mcp-prediction-markets/src/calibration.rs:134` `brier` (delegates to the lib, `:13`/`:141`), `hkask_mcp_prediction_markets.rs:211` `market_record_resolution`, `:524` `market_check_resolutions`; equity leg: `hkask-mcp-companies` `forecast_persist`/`forecast_record` (`tools/valuation.rs:1101`/`:1219`, model `src/forecast.rs`), `calibrate_forecast` (`tools/valuation.rs:874`)
- **Trigger:** forecast creation → outcome recording → calibration readback
- **Functional graph (Phase 1, IS-cited per node):** snapshot arm (`market_check_resolutions` `hkask_mcp_prediction_markets.rs:524` → `CalibrationStore::record_pending` `calibration.rs:172`, earliest snapshot kept, test `:527`) → resolution arm (`market_record_resolution` `:211` → `record` `calibration.rs:125`; subscribe leg `:264-298` logs notifications, never fabricates observations) → scoring (`brier` `calibration.rs:134` → shared `hkask_forecast::brier_score_multi` `hkask_forecast.rs:196`) → readback (`market_calibration` `:187` → `read_calibration` `calibration.rs:313`; missing/empty bucket → `stale: true`, `brier: None`, never a synthetic 0, tests `:410-419`) → act (`reliability_tier` demotion on annotated lookups, `types.rs:251` wired `:433`→`:463`). Equity leg: `dcf_valuation`/`calibrate_forecast` → `forecast_persist` → `forecast_record` (Brier + decomposition at record); feedback application on the equity leg is agent-mediated (OUGHT — no automatic path applies equity calibration history to future priors; INFERRED from absence).
- **Findings (Phase 2, adjudicated):** **F1 REFUTED** — the Phase 0 "two Brier implementations" signal: `calibration.rs:13`/`:141` delegates to the shared lib and `companies/superforecast.rs:3-9` documents the no-pass-through layering; signal withdrawn. **F2 informational, kept as IS** — `calibration.rs:141` `map_err(|_| ())` collapses only unreachable `ForecastError` variants into the designed `stale: true` semantic (empty bucket pre-checked `:136-137`; length mismatch impossible — both vectors built from one iterator). **F3 verified** — the tier-demotion claim is enforced (`types.rs:251`/`:433`/`:463`): the market loop is CLOSED. **F4 CONSOLIDATED** — `scenarios/superforecast/math.rs:35` `brier_score_multi` was a pure `ForecastError`→`ScenarioError` wrapper while the same module re-exports `brier_score` directly from the lib (`superforecast.rs:16`); the wrapper was deleted and the lib function re-exported (`ScenarioError` carries `#[from] ForecastError`, `types.rs:45`), keeping the `superforecast::brier_score_multi` path stable for callers.
- **Five properties:** closed — IS (market leg), agent-mediated OUGHT (equity leg); timely — IS (staleness surfaced; scan cadence operator-driven, `zero_scan_reason` on empty scans); accurate — IS (earliest-snapshot discipline `calibration.rs:168-177`, identity-based dedup `:144-162`, no-fabrication contracts, tested); complete — IS with stated boundary (equity and market observations use separate stores by reference class); actionable — IS (tier demotion changes lookup annotations, `matcher.rs:7`).
- **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.50 → actual: 0 defects, 1 module-convention inconsistency consolidated (F4, net −13 lines), 1 Phase 0 signal refuted (F1). Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **C** (calibration is the periodic-reflection discipline: score resolved forecasts over history, adjust the forecaster); secondaries A (tier demotion applies automatically on lookup; snapshot/resolution recording), B (readback in-thread via `market_calibration`). Sense→report→actuate: sense resolutions (`market_check_resolutions`) → report Brier/calibration readback (`market_calibration`) → actuate tier demotion (`types.rs:251`) + revised agent priors (agent-mediated). Five properties stand. Impedance (standing): the equity leg is agent-mediated — endpoints equity calibration store × future equity priors — dimension: closure (no automatic application; INFERRED from absence per the row). Grill: primary-A fails because the loop's product is a calibrated *forecaster* (the C-tier model), not an automatic actuator; calibration consumed only by automatic actuation would force reclassification. Alignment: INV2 held (the forecast's own probability is the stored expectation — the H7 fix); INV3 held (staleness surfaced, zero-scans attributed); INV5 partial (market leg revises tiering automatically; equity model revision is agent-mediated).

### L9 — Kanban/goal loop — Steer prompt consolidated; two operator decisions deferred
- **Crate/path:** `kask/mcp-servers/hkask-mcp-kata-kanban/src`
- **Entry point:** `hkask_mcp_kata_kanban.rs:461` `kanban_goal_create`, `:517` `kanban_goal_judge`, `:564` `kanban_goal_score`, `:605` `kanban_goal_memory_acknowledge`; `kanban/service_impl.rs` and `idempotency.rs` own the board/task side of the cycle
- **Trigger:** agent/user MCP actions during work; panels (L7)
- **Hands off to:** L10 (resolved-goal outcome → curator memory), L7 (widget updates), L2 (goal intake predictions are Brier-scored at resolution)
- **Prediction:** 1 / 1 / 0.50
- **Phase 1 graph (IS):** `kanban_goal_create` receives a user-owned target (`hkask_mcp_kata_kanban.rs:461-500`) → service stores criteria (`kanban/service_impl/goals.rs:59-107`) → `kanban_goal_judge` validates coverage and appends a verdict (`goals.rs:208-247`) → `goal_score` stores outcome/Brier as a retained outbox row (`goals.rs:264-297`) → turn-end curator ingestion/acknowledgment (`crates/agent/src/thread.rs:332-374`) → `kanban_goal_list` and score readback (`hkask_mcp_kata_kanban.rs:564-658`). Five properties: closed **conditional** on ingestion/ack; timely **partial** (no evidenced automatic retry after failed turn ingestion); accurate **partial**; complete **partial**; actionable **partial** (manual list/readback, no verified automatic recovery).
- **Phase 2 open findings:** IS — the Steer prompt previously called goals ephemeral (`crates/kanban_panel/src/kanban_panel.rs:380-384` before this change), contradicting the retained scored row (`goals.rs:287-297`). **Consolidated:** replaced five stale prompt lines with four lines describing durable resolution/acknowledgment (`kanban_panel.rs:380-383`), removed the superseded ephemeral test comment (`kask_bridge/src/memory.rs:1702-1705`), and updated D2 in `DIVERGENCE.md` in the same pass. The existing panel seam's `steer_prompt_describes_durable_goal_acknowledgment` failed red then passed green, and the 32-test `kanban_panel` library suite passed. Falsifier: that rendered Steer prompt contains `EPHEMERAL` or fails to say scored goals remain until memory acknowledgment. This panel/bridge/D2/test slice landed in pathspec-limited commit `1113d8d85d`; `./script/clippy` and `cargo check -p zed` passed before that commit. IS — `goal_acknowledge_memory` checks owner/resolved status and prunes without confirming a memory receipt (`goals.rs:319-337`); INFERRED risk: direct acknowledgment could delete an un-ingested scored row. **Falsifier:** a server-enforced memory receipt gate or a test proving direct acknowledgment cannot prune before ingestion. Receipt enforcement needs a cross-server contract and fails the simple deletion test; defer for operator decision with the risk stated. IS — `Done` does not check `passed` values before append (`goals.rs:208-247`); **falsifier:** a rejection check on the service path. Do not count an unrun runtime test as a confirmed defect.
- **Closure (2026-09-27):** one finding consolidated and landed (the Steer durable-goal prompt, `1113d8d85d`, red→green pinned); two items deferred as operator decisions with risks stated — the direct-acknowledgment memory-receipt gate (cross-server contract, fails the simple deletion test) and the Done-verdict/criteria-passed rejection check (a behavior change; falsifier: a rejection check on the service path). **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.50 → actual: 1 defect found and fixed, 0 impedances, 2 operator decisions pending. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **B** (in-thread MCP actions, panels, judge/score during work); secondary C (scored goals retained until memory acknowledgment — the durable outcome record feeding curator memory; Brier at resolution). Sense→report→actuate: sense MCP actions/panel events → report board/goal state + judge verdicts → actuate task moves, goal scoring. Five properties stand (partial per the row). Impedances (standing operator decisions): the memory-receipt gate (`goals.rs:319-337` × curator memory — dimension: B→C handoff receipt enforcement) and the Done-verdict/criteria-passed check (`goals.rs:208-247` — dimension: verdict accuracy). Grill: primary-C fails — the daily loop is in-thread work management; the C leg is the resolution outbox. Reclassification falsifier: review-only use of boards with no in-thread work. Alignment: INV2 held (the intake prediction is Brier-scored — expectation carriage by design); INV4 partial (no evidenced automatic retry after failed turn ingestion).

### L10 — Memory recall/ingest cycle — audited; recall failure-signal contract deferred to the operator
- **Crate/path:** `kask/crates/hkask-memory` + `kask/mcp-servers/hkask-mcp-curator`
- **Entry point:** `kask/crates/hkask-memory/src/memory_store.rs:288` `store`, `:331` `query_deduped`, `:447` `touch_recall`, `:261` `with_ledger`; consolidation `consolidation_service.rs:38`; curator ingest `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:1833` `curator_memory_extract` (turn-discovery contract `thread_turns.rs`, cited at `:1557`, `:1730-1751`), `distillation.rs`, `forgetting.rs`
- **Participants:** `federated_recall.rs`, `recall_dedup.rs`, `salience.rs`, `bayesian.rs`; zed-side injection `kask_bridge/src/memory.rs`, `context_injector.rs`
- **Trigger:** turn-end extraction; recall queries; prune/decay cycles
- **Hands off to:** L1 (context injection), L2 (ledger), therapy/consolidation skills
- **Prediction:** 2 / 2 / 0.55
- **Phase 1 scoped graph (IS):** L1 turn completion hands a record to `RealMemoryPort::ingest_turn` (`crates/agent/src/thread.rs:3019-3086`; `kask_bridge/src/memory.rs:520-537`); `memory/ingest.rs:395-442,543-610` writes goal/chunk memories; `kask_bridge/src/memory.rs:795-1049` retrieves/ranks/touches curator memories; `context_injector.rs:292-348` injects them into a later curator turn. L9's scored-goal row is acknowledged only after the turn-ingestion call returns success (`thread.rs:351-373`). Five properties in inspected scope: closed for curator recall after successful ingest, timely conditional on detached turn task, accurate/complete conditional on store reads, actionable for curator via subsequent context or memory tools. Ordinary-agent recall returning empty is **intentional IS** (`memory.rs:540-559` and `:201-205`), not evidence that the curator cycle is broken.
- **Phase 2 open finding (IS + INFERRED consequence):** keyword recall discards a DB query error via `if let Ok` (`memory.rs:927-967`) and exact-thread recall does likewise (`:1067-1085`), so on these legs a failed store read can produce an empty candidate set rather than a surfaced read failure. **Falsifier:** inject a query failure and observe a distinct caller-visible error. Do not reclassify the deliberate empty-store fallback (`memory.rs:711-745`) without operator agreement. No test of a real store failure has run; changing the result/error contract without it would violate behavior preservation. Defer further L10 consolidation pending this test and an approved failure signal; no net-negative deletion candidate has been established.
- **Closure (2026-09-27):** the error-discarding legs are IS-confirmed by reading; changing the result/error contract is a behavior change, deferred for an operator ruling on the failure signal (the deliberate empty-store fallback stays untouched per its own note). No deletion candidate exists. **Prediction vs actual:** predicted 2 defects / 2 impedances / conf 0.55 → actual: 1 defect-class finding deferred for operator ruling, 0 impedances, 0 deletion candidates. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **C** (curator memory is the Layer-C substrate: chunked threads, distillation, forgetting, consolidation); secondary B (context injection into later turns — the C→B efferent arm). Sense→report→actuate: sense turn-end records (`thread.rs:3019-3086`) → report h_mems/chunks (`memory_store.rs:288`) → actuate consolidation/decay (`consolidation_service.rs:38`, `forgetting.rs`) + injection (`context_injector.rs:292-348`). Five properties stand (the recall failure-signal legs fixed per operator ruling D5). No open impedance. Grill: primary-B fails — in-thread recall is the efferent arm; the retention/consolidation loop is the slow tier. Reclassification falsifier: a per-turn cache with no cross-session retention. Alignment: INV5 held (distillation/consolidation revise the model); INV6 held (inject down, ingest up — clean directions).

### L11 — Media job queue cycle — audited; page-visibility impedance deferred
- **Crate/path:** `kask/mcp-servers/hkask-mcp-media/src`
- **Entry point:** `jobs.rs:167` `admit`, `:210` `mark_running`, `:229` `cancel`, `:477` `finish`, `:304` `active_count`; tools `job_submit`/`job_status`/`job_list`/`job_cancel`
- **Trigger:** async generation job submit → poll → terminal
- **Hands off to:** L7 (media panel/gallery), transcript layer passes (`transcript_pass.rs`)
- **Prediction:** 1 / 1 / 0.45
- **Phase 1 scoped graph (IS):** job submission validates and admits against bounded capacity (`tools/jobs.rs:99-125`; `jobs.rs:166-187`) → transitions to running (`tools/jobs.rs:157-181`) → races generation against cancellation (`:184-228`) → publishes or rolls back terminal status (`:268-320`; `jobs.rs:314-411`) → status/list readback (`tools/jobs.rs:332-408`). L7 media panel reads newest jobs (`crates/media_panel/src/media_viewer.rs:731-760`) and polls when a visible row is nonterminal (`:795-836`). Five properties: closed for a visible job within process lifetime; timely conditional on queue-tab polling; accurate for typed terminal/failure status; complete only within ephemeral capped history (`jobs.rs:511-530`); actionable through cancel/status.
- **Phase 2 open impedance (IS + INFERRED):** `job_list` sorts newest-first then takes the limit (`tools/jobs.rs:355-368`); L7 asks for 20 rows (`media_viewer.rs:746-750`) and stops polling when none **in that page** is nonterminal (`:795-820`). INFERRED: an older running job behind 20 newer completed jobs loses automatic status observation. Falsifier: a panel test with that ordering still polls and updates the older active job. No runtime reproduction; no behavior-preserving negative-line change admitted. Defer any list-order/visibility change until the panel contract and test are agreed.
- **Closure (2026-09-27):** the sort-then-limit readback re-verified in the current tree (`tools/jobs.rs:356-359`); the page-visibility impedance stays deferred pending the panel-contract decision, with its falsifier recorded. No deletion candidate. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 0 confirmed defects, 1 inferred impedance deferred with falsifier. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (admission control, cancellation, publish/rollback — deterministic); secondary B (panel polling/status). Sense→report→actuate: sense job submit → report status/list (`tools/jobs.rs:332-408`) → actuate admit/cancel/publish/rollback (`jobs.rs:167-411`). Five properties stand. Impedance (standing): page-visibility — endpoints `job_list` sort-then-limit (`tools/jobs.rs:355-368`) × panel polling stop (`media_viewer.rs:795-820`) — dimension: B-surface observability coverage (an older running job behind 20 newer completions loses automatic observation). Grill: primary-B fails — the panel observes, the queue acts. Reclassification falsifier: a panel-driven admission policy. Alignment: INV4 held (typed terminal/failure status); INV2 absent.

### L12 — Research run ledger cycle — audited; closable-vs-append-only deferred to the operator
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src`
- **Entry point:** `hkask_mcp_research.rs:1604` `begin_research_run`, `:1658` `annotate_research_run`; run state in `research/runs.rs`
- **Trigger:** research question → run opened → run-scoped search/extract recorded → evidence evaluation → annotation
- **Hands off to:** L4 (tool surface), L1 (agent-driven research), corpus/companies consumers of run ledgers
- **Prediction:** 1 / 1 / 0.45
- **Phase 1 scoped graph (IS):** caller begins an identified question (`hkask_mcp_research.rs:1604-1624`; `research/runs.rs:32-46`) → run-scoped search/extract records server-returned URLs with first observation retained (`hkask_mcp_research.rs:484-503,564-583,710-722`; `runs.rs:60-99`) → annotation checks an observed URL before granting `verified` and updates the declared state/basis (`runs.rs:362-427`; `hkask_mcp_research.rs:1658-1700`) → manifest readback recomputes validation and source evidence (`runs.rs:106-239`; `hkask_mcp_research.rs:1629-1653`). The L4 tool response includes a `run_ledger` append receipt/error (`hkask_mcp_research.rs:1714-1763`); L1 agent supplies the run ID and consumes that receipt. Five properties: source provenance closed by readback, timely per request, accurate for recorded-by distinction, complete conditional on checked append receipts, actionable for agent annotation; **run lifecycle completion is not evidenced**.
- **Phase 2 open finding (IS + INFERRED):** begin stores `status='planned'` (`runs.rs:32-46`); the inspected source has no `UPDATE research_runs`, while the validator accepts six statuses (`runs.rs:254-302`). INFERRED: a run with sources and annotations can still report `planned` in `get_research_run` (`:231-239`), so the status field may mislead a caller about completion. Falsifier: locate and exercise a production status-transition writer; the repository-wide `UPDATE research_runs` search found none. No automated finish operation is in the tool surface; adding one or removing lifecycle claims changes functional behavior. Defer for operator decision on whether runs should be explicitly closable or remain append-only; do not invent a completion event. No net-negative candidate admitted yet.
- **Closure (2026-09-27):** the repository-wide `UPDATE research_runs` search re-verified on the current tree — the only match is this register's own record; no status-transition writer exists. The closable-vs-append-only ruling stays with the operator; no completion event invented. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 1 IS finding deferred as the operator's lifecycle decision, 0 impedances, 0 deletion candidates. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **B** (agent-driven runs, annotations, receipts in-thread); secondary A (append receipts, first-observation-wins — deterministic ledger mechanics). Sense→report→actuate: sense run-scoped search/extract (`runs.rs:60-99`) → report manifest + append receipts (`hkask_mcp_research.rs:1714-1763`) → actuate annotations (`runs.rs:362-427`). Five properties stand (lifecycle closed — `finish_research_run` landed per ruling D6). No open impedance. Grill: primary-A fails — the ledger mechanics are deterministic, but the verification sense-making is the agent's in-thread work. Reclassification falsifier: a fully automated evidence verifier. Alignment: INV2 held (`verified` granted only for server-recorded URLs — the expectation "the server observed this" is enforced); INV4 held (append failures surfaced as receipts).

### L13 — Swarm thread/memory cycle — CLOSED pass 2: seam test ran; the asymmetry is pinned design; unscoped attach clusters consolidated
- **Crate/path:** `kask/mcp-servers/hkask-mcp-swarm/src` + `kask/crates/hkask-event-store`
- **Entry point:** `local_tools.rs:278` `dispatch_in_thread` / `:354` `swarm_delegate_in_thread_local`; `knowledge_tools.rs:67` `swarm_recall_local`; event store `kask/crates/hkask-event-store/src/hkask_event_store.rs`; zed-side feed `kask_bridge/src/rollout_event_bridge.rs:104` `poll_once`
- **Trigger:** delegation dispatches; recall queries; ABW sync; task-board updates
- **Hands off to:** L2 (rollout events → regulation sensors), L7 (swarm panel), L10 (agent prefix-scoped memories in `MemoryStore`)
- **Prediction:** 1 / 1 / 0.45
- **Phase 1 scoped graph (IS):** a scoped delegation checks roster membership (`local_tools.rs:278-307`), reads prior ordered turns (`:309-333`), invokes the member model and commits a turn (`:335-343`), then `swarm_thread_local` returns retained turns even after roster deletion (`:378-405`). Separately, `attach_narrative_memory` embeds a local response (`:161-179`) and `swarm_recall_local` retrieves passages with a surfaced unavailable note (`knowledge_tools.rs:67-116`). L7 swarm panel fetches durable turns (`crates/swarm_panel/src/member_turns.rs:277-309`); L2's rollout bridge is separate. Five properties in this scoped path: closed for ordered thread readback, timely per dispatch, accurate/complete for successful thread append, but shared semantic recall is not shown to cover every scoped dispatch; actionable via thread read and degraded-memory note.
- **Phase 2 impedance (IS + INFERRED effect):** `dispatch_in_thread` appends to the durable thread without `attach_narrative_memory` (`local_tools.rs:335-343`); direct `swarm_delegate_in_thread_local` returns it without indexing (`:368-375`), and scoped fanout similarly takes the result (`:594-603`), while scoped pipeline does attach after dispatch (`:842-865`). INFERRED: a direct scoped turn may be visible in `swarm_thread_local` but absent from `swarm_recall_local`. Falsifier: a scoped-delegation → semantic-recall integration test retrieves that turn without a separate caller ingest. Potential consolidation: one ingestion at the common dispatch seam, delete per-caller copies to avoid double indexing; **not admitted yet** because moving inference work into every scoped dispatch changes timing/result shape and needs a red-green public-seam test. Defer with this risk rather than bolt on another caller-specific hook.
- **Closure (2026-09-27):** the dispatch-seam ingestion consolidation is named but NOT admitted — moving embedding into every scoped dispatch changes timing and result shape and needs a red-green public-seam test first; the impedance stays deferred with its falsifier. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 0 confirmed defects, 1 impedance deferred with a candidate consolidation gated on its seam test. Brier-scored at Phase 4.
- **Pass 2 closure (2026-09-28):** the red-green public-seam test ran. RED (observed): a direct scoped dispatch via `swarm_delegate_in_thread_local` followed by `swarm_recall_local` returned `count: 0, note: ""` — healthy machinery, zero passages, the exact falsifier. GREEN after a seam attach was attempted — and the full suite then exposed the existing pin `thread_tests.rs:220-223` ("thread turns must not write semantic memory"): the asymmetry is DOCUMENTED DESIGN, not an accident — the encrypted durable thread IS the record for scoped dispatch, and shared semantic memory is an opt-in composition callers take (pipeline/plan attach their steps; the direct thread tool deliberately does not). The seam consolidation is REJECTED on the same grounds as L16's sticky attribution (contradicts a pinned design decision; overriding it is an operator ruling, not an audit action). The refuted test was removed — the pin already covers the design. The surviving, behavior-preserving consolidation landed: `delegate_and_ingest` (one helper replacing the three unscoped attach clusters in `swarm_delegate_local`, sequential fanout, and eval-suite; the parallel fanout keeps its own attach — batch API), plus a design-boundary comment at the dispatch seam so the next audit does not re-propose this. Landed in `57c2bdea7a` (mixed-purpose commit, the stream's): local_tools.rs +41/−40 production (net +1, of which +6 is the seam comment; code motion net −5). Receipts: swarm --lib 210/210 green, rustfmt clean on the touched files, scoped `./script/clippy` clean (machete + buf included).

- **Pass 3 layer classification (2026-09-30):** primary **B** (delegation and recall within threads); secondary C (durable ordered threads as the retained record — the C substrate for swarm history). Sense→report→actuate: sense delegation (`local_tools.rs:278`) → report ordered turns (`swarm_thread_local`) + recall (`knowledge_tools.rs:67`) → actuate next dispatch. Five properties stand. No open impedance (the scoped-dispatch non-ingestion asymmetry is pinned design, `thread_tests.rs:220-223`). Grill: primary-C fails — the durable thread is the record, but the loop's motion is in-thread delegation. Reclassification falsifier: a swarm used only for post-hoc review. Alignment: INV6 held (thread append and semantic memory are separate channels by design); INV2 absent.

### L14 — Settings → MCP server sync/restart cycle — AUDITED & CLOSED 2026-09-27 (minimal by design)
- **Crate/path:** `kask/crates/kask_bridge/src` + zed-side wiring (`crates/zed/src/main.rs`, `crates/settings_ui/src/pages/kask_page.rs`)
- **Entry point:** `sync_kask_mcp_runtime_servers` `crates/zed/src/main.rs:3585` (observer wired `main.rs:1455-1461`, D45; baseline + latch established `main.rs:1435-1453`); `nudge_mcp_servers` `kask_page.rs:342`; env assembly `build_mcp_server_env` `kask_bridge/src/mcp_servers.rs:681` (single canonical path; config half = `mcp_env.rs` emit_* translators); socket `inference_socket.rs:24`
- **Trigger:** settings change (`cx.observe_global::<SettingsStore>`); credential keychain write/delete → `nudge_mcp_servers` → `notify_observers` (`kask_page.rs:280/:307`, funnel doc `:324-341`, D32 interplay `:332-336`); launch pass sets baselines + latch
- **Functional graph (Phase 1, IS-cited per node):** notify → sync (`main.rs:3585`): load-state resolution, same expression as the launch loop (`:3593-3606`) → per-server env (`kask_server_env` `:3499`) → baseline diff → classify `to_stop` (`:3636`), `to_start` (latch-gated `:3662`), `to_restart` (env changed, changed keys named in log `:3637-3649`) → `Tokio::spawn` stop/start/restart (`:3677-3765`) with baseline bookkeeping (insert-not-expect `:3737-3745`) and retry-on-next-pass failure semantics (`:3716-3727`, `:3747-3761`; failed starts keep their launch spec so tool calls reconnect on demand).
- **Minimalism verdict (ideal-method pass):** minimal by design — event-driven single funnel (N triggers → 1 notify → 1 sync), env-diff restarts exactly the changed servers, no polling, racing observer passes collapse via runtime idempotency (`:3690-3694`). **Collapse of the launch/sync mirror REJECTED:** the launch pass owns startup ordering and the inference-socket existence window; the latch + empty-baseline semantics are behavior, not structure (`main.rs:1435-1461`, `:3653-3662`). Classification living in `main.rs` (untestable without gpui) is noted; moving it to `kask_bridge` is net-neutral lines, not a consolidation.
- **Prediction vs actual:** predicted 1 defect / 2 impedances / conf 0.50 → actual: 0 defects, 0 impedances. Brier-scored at Phase 4.
- **Hands off to:** L3 (runtime respawns servers), L5 (socket env injection).

- **Pass 3 layer classification (2026-09-30):** primary **A** (event-driven restart, env-diff classification — deterministic). Sense→report→actuate: sense settings/credential changes (`main.rs:1455-1461` observer) → report changed-keys log (`main.rs:3637-3649`) → actuate stop/start/restart (`main.rs:3677-3765`). Five properties stand; no impedance (minimal by design). Grill: primary-B fails — the changed-keys log is operator-visible, but the loop acts autonomously. Reclassification falsifier: restart requiring approval. Alignment: INV1 held (single funnel); INV2 partial (the env diff is expected-vs-actual env — expectation-shaped).

### L15 — Passphrase rotation cycle — AUDITED & CLOSED 2026-09-27 (minimal by design)
- **Crate/path:** `kask/crates/kask_bridge/src/passphrase_rotation.rs`
- **Entry point:** `:94` `schedule_db_passphrase_rotation`, `:167` `run_pending_db_passphrase_rotation`, `:212` `apply_db_rotation`; startup hook `crates/zed/src/main.rs:373`
- **Trigger:** operator rotation request → pending state on disk → applied at next launch (keychain slot written last)
- **Hands off to:** all SQLCipher DBs (`hkask-storage`, every MCP server DB), `hkask-keystore`
- **Prediction:** 1 / 1 / 0.50
- **Phase 1 scoped graph (IS):** operator schedules a pending record and keychain intent without touching the active key (`passphrase_rotation.rs:90-118`); at startup the runner senses pending state (`:167-172`), classifies old/new/neither-opening DBs (`:212-245`), rotates with rollback on failure (`:246-275`), and only after all DBs agree writes the main keychain slot (`:193-197`). The next startup re-reads pending state and DB keys, providing the return path; failure records the last error for operator recovery (`:173-190`). Five properties from code: closed across restart, timely at next launch rather than immediate, accurate through explicit key classification, complete only for confirmed inventory, actionable via surfaced error/pending record; none of these is a live rotation claim.
- **Phase 2/4 result:** no duplicate implementation or net-negative removal candidate was established: the keychain-last ordering and rollback are distinct load-bearing paths. After the unrelated type edit became buildable, `bash kask/scripts/cargo-test-nonzero.sh -p kask_bridge passphrase_rotation --lib` ran **6/6 tests passing** (pending record, same-key, neither-key, crashed partial, full rotation, rollback). Earlier E0599 compilation failure ran zero tests and is superseded by this targeted result. This is an offline test verdict, not a live rotation claim; close L15 as minimal-by-design after the current-tree full gates.
- **Closure (2026-09-27):** minimal by design — the keychain-last ordering and rollback are distinct load-bearing paths, no deletion candidate; 6/6 offline rotation tests green, and the current-tree full gates passed at the L2 landing (v0.16.1 receipts: full clippy clean, cargo check -p zed). **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.50 → actual: 0 defects, 0 impedances. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (startup automation with rollback). Sense→report→actuate: sense pending record (`passphrase_rotation.rs:167-172`) → report last-error/pending state (`:173-190`) → actuate rotate-with-rollback (`:212-275`). Five properties stand; no impedance. Grill: primary-B/C fails — the operator schedules, but the cycle is autonomous at startup. Reclassification falsifier: an interactive rotation wizard. Alignment: INV4 held (failure records the last error for operator recovery); INV2 partial (old/new key classification is expectation-like).

### L16 — Skill activation → outcome → algedonic review cycle
- **Crate/path:** `crates/zed/src/main.rs` + `kask/crates/hkask-regulation` + `kask_bridge`
- **Entry point:** `crates/zed/src/main.rs:982-996` (`CuratorRegulationArchive` + `persist_skill_outcome`); regulation side `kask/crates/hkask-regulation/src/runtime.rs:776` `record_skill_span`, `:822` `record_outcome`, `:848` `check_outcome`, `:805` `skill_ids_with_feedback`; board `kask_bridge/src/algedonic_board.rs` (D59, D64, D79)
- **Trigger:** skill activation during turns; tool failures; operator algedonic-review sessions (`record_skill_feedback`)
- **Hands off to:** L2 (ledger/alerts), curator memory, skill-maintenance proposals
- **Prediction:** 2 / 1 / 0.50
- **Phase 1 scoped graph (IS):** skill activation supplies success/failure to the process recorder (`crates/agent/src/tools/skill_tool.rs:194-308`); the zed host persists it when the deferred curator archive exists and always queues a Regulation ledger span (`crates/zed/src/main.rs:974-1013`); metacognition reads skill-outcome/operator-feedback drift (`hkask-regulation/src/metacognition.rs:447-529`); the operator/Curator decides acceptance in algedonic review and `record_skill_feedback` calls the feedback recorder (`crates/agent/src/tools/record_skill_feedback_tool.rs:69-90`); the host persists that feedback before updating the live ledger (`crates/zed/src/main.rs:1723-1756`), which the next drift sense can observe. Five properties: closed when archive and operator verdict are available, timely conditional on review, accurate for activation reliability but not quality until operator feedback, complete conditional on archive readiness, actionable through review/feedback. No operator verdict was created by this audit.
- **Phase 1 graph (IS, citations verified 2026-09-27):** activation — the `skill` tool resolves, authorizes, and renders (`tools/skill_tool.rs:194-238`), `activate_skill` records the outcome (`skill_tool.rs:295-310` → `agent.rs:4853` `record_skill_outcome` → recorder hook → `crates/zed/src/main.rs:995` `persist_skill_outcome` → durable `reg.skill.<id>.outcome`, `curator_stores.rs:63`); failure — `run_tool` captures the thread's shared skill cell at dispatch (`thread.rs:4431`), and a non-`skill` tool error (authorization errors excluded) records under whatever skill the cell holds (`thread.rs:4503-4518` → `agent.rs:4860` → `persist_skill_tool_failure` `curator_stores.rs:74`); readback — algedonic board (D59/D64/D79), `reg_query` (D60), drift sense (`metacognition.rs:447`). The only write to the cell is activation success (`thread.rs:4448-4450`); the full `active_skill` sweep finds no clear site — the cell is thread-lifetime.
- **Phase 2 adjudication (closed 2026-09-27):** the row's INFERRED misattribution is confirmed as MECHANISM (IS) but refuted as DEFECT: sticky attribution is D59's documented, pinned design — "`KaskThreadState::active_skill` holds the last skill a thread activated successfully" (`DIVERGENCE.md` D59, 2026-09-26; field doc `kask_thread_state.rs:62-63`; pin `test_tool_failure_under_active_skill_is_recorded`, `tests/mod.rs:10672`) — with the recorded mitigation that tool-failure records are unclassified evidence for the operator+curator algedonic review while `curator_report_skill_use_issue` remains the classified channel. The residual trade-off — a long-lived thread attributes much-later unrelated failures to the skill whose body remains in context — is the documented design, not a consolidation candidate; windowed attribution would be a behavior change requiring an operator ruling. The row's proposed cross-turn falsifier is moot: the existing pin documents attribution-after-activation, and cross-turn persistence follows from the thread-lifetime cell by construction.
- **Five properties:** closed — IS (activation → span → durable outcome → board/`reg_query` readback); timely — IS (durable at write, restart-hydrated per D59); accurate — partial by documented design (sticky attribution; records are unclassified evidence); complete — IS (activation, failure, and operator-feedback phases all recorded); actionable — IS (board cards + `reg_query`). **Prediction vs actual:** predicted 2 defects / 1 impedance / conf 0.50 → actual: 0 defects (the one inferred risk refuted as documented design), 0 impedances. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **C** (the loop closes in the operator+curator algedonic review — the slow reflection that decides acceptance and changes the skill); secondaries A (durable span/outcome recording), B (in-thread activation and failure attribution to the active skill). Sense→report→actuate: sense skill activation/tool failures (`skill_tool.rs:194-308`; `thread.rs:4431-4518`) → report durable `reg.skill.*` outcomes (`curator_stores.rs:63/:74`) + board cards (`algedonic_board.rs`) → actuate review verdicts (`record_skill_feedback` → persist → live ledger). Five properties stand. No open impedance (sticky attribution is pinned design D59). Grill: primary-B fails — activation is in-thread, but the loop's closure — the verdict that changes the skill — is the review. Reclassification falsifier: outcomes consumed only within the same thread. Alignment: INV5 held (the review's product is a changed skill — the model); INV4 partial (unclassified failure evidence vs the classified channel — the recorded mitigation).

### L17 — Scenario quantification/Brier loop — audited; posterior carry-forward boundary deferred
- **Crate/path:** `kask/mcp-servers/hkask-mcp-scenarios/src`
- **Entry point:** `hkask_mcp_scenarios.rs:1108` `scenario_quantify`, `:1229` `scenario_update`, `:1281` `scenario_score`, `:1397` `scenario_calibrate`; Tetlock pipeline in `superforecast/`
- **Trigger:** scenario project events → quantification → Bayesian updates → outcome scoring
- **Hands off to:** L8 (shares calibration discipline), companies impact valuation
- **Prediction:** 1 / 1 / 0.45
- **Phase 1 scoped graph (IS):** events are quantified into marginal/joint probabilities and cached (`hkask_mcp_scenarios.rs:1108-1119`); caller-supplied evidence revises a prior to a posterior (`:1229-1273`); resolved outcomes receive Brier scoring (`:1281-1341`) and are journaled to the forecast store (`:1344-1385`); the later `scenario_calibrate` reads resolved forecasts and applies bias/isotonic calibration when available (`:1397-1453`). The L8 shared Brier functions supply the scoring primitive; the caller must pass updated event state between requests. Five properties in this scope: closed through journal→calibrate readback, timely per explicit call, accurate conditional on supplied outcomes, complete for recorded events not unsubmitted histories, actionable through calibrated output; no live forecast resolution checked.
- **Phase 2 boundary (IS, deferred):** `scenario_update` returns a posterior in a response (`:1253-1272`), not a persisted change to the cached tree; `scenario_score` takes its own events array (`:1285-1298`). INFERRED: a caller that fails to pass the new posterior forward can score an older prior. Falsifier: a test showing the revised event is carried by the same request path to scoring without caller intervention. Automatically mutating cached events would change the caller-controlled scenario contract, not a behavior-preserving deletion; defer any automation until that functional choice is confirmed.
- **Closure (2026-09-27):** the posterior-carry boundary stays deferred — the caller-controlled scenario contract is documented behavior, and automating the carry would change it; no deletion candidate. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 0 confirmed defects, 1 inferred boundary deferred with falsifier. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **B** (caller-driven quantification/updates in-thread); secondary C (calibration over resolved forecasts — the slow leg). Sense→report→actuate: sense events/outcomes (`scenario_score :1281`) → report marginals/joints + calibrated readback (`scenario_calibrate :1397`) → actuate revised priors (caller-carried). Five properties stand. Impedance (standing): posterior carry — endpoints `scenario_update`'s returned posterior (`:1253-1272`) × `scenario_score`'s supplied events (`:1285-1298`) — dimension: state carry across requests (a caller that fails to pass the posterior forward can score a stale prior). Grill: primary-C fails — calibration is one leg; the loop's motion is in-thread quantification. Reclassification falsifier: calibration-only use. Alignment: INV2 held (priors are explicit probabilities); INV4 partial (the carry boundary can drop state between requests).

### L18 — Training job cycle — audited; Nebius status-degradation contract deferred
- **Crate/path:** `kask/mcp-servers/hkask-mcp-training/src`
- **Entry point:** `tools/submit.rs:28` `training_submit`, `tools/status.rs:17` `training_status`, `tools/cancel.rs:13` `training_cancel`; `lora_validation.rs` gates the submission
- **Trigger:** operator-confirmed training submit → status polling → cancel/complete
- **Hands off to:** L6 (consumes assembled datasets), L16 (adapter evaluation feeds skill feedback)
- **Prediction:** 1 / 1 / 0.45
- **Phase 1 scoped graph (IS):** `training_submit` refuses unconfirmed GPU spending before dataset/model work (`tools/submit.rs:28-61`), checks artifact persistence (`:63-100`) and submits to the configured host; `training_status` reads the host and, for Running, probes the completion manifest (`tools/status.rs:17-35`; `hkask_mcp_training.rs:237-282`), persists the observed status and registers a completed adapter (`tools/status.rs:81-107`); `training_cancel` sends cancellation to the host (`tools/cancel.rs:13-23`). L6 supplies the prepared dataset, and adapter evaluation is a separate L16 feedback handoff (`tools/evaluate.rs:97`). Five properties: closed for Runpod jobs with a readable manifest, timely only at caller polling cadence, accurate/complete conditional on host and manifest access, actionable via status/cancel; **no training was submitted or paid for in this audit**.
- **Phase 2 finding (IS, deferred):** the Nebius submission branch warns it lacks completion detection (`tools/submit.rs:84-91`), and `check_completion_manifest` converts missing configuration, failed artifact lookup or fetch error to `None` (`hkask_mcp_training.rs:245-281`); the status response then reports Running without a machine-readable degradation (`tools/status.rs:27-41`). INFERRED: a finished or unobservable job can appear indefinitely active. Falsifier: a Nebius or manifest-failure status test emits an explicit non-running/unknown or degraded status to the caller. Altering the result contract would add semantics and likely code; defer for a separate functional decision, not a silent success claim.
- **Closure (2026-09-27):** the Nebius/manifest-degradation contract stays deferred for a functional decision on the status semantics; no training was submitted or paid for by this audit. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 1 IS finding (unobservable jobs read Running without machine-readable degradation) deferred for the status-contract decision, 0 impedances. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **B** (operator-confirmed submit, status polling in-thread); secondary A (the `RunningUnknown` status semantics — deterministic degradation classification, landed as follow-up F1). Sense→report→actuate: sense host status (`tools/status.rs:17`) → report status + completion manifest (`:81-107`) → actuate cancel/complete. Five properties stand (D7/F1 landed). No open impedance. Grill: primary-A fails — the job runs autonomously on the host, but the loop's feedback — submit/poll/cancel — is in-thread. Reclassification falsifier: a fully automated training scheduler. Alignment: INV4 held (`RunningUnknown` names the degradation — the F1 fix is an INV4 repair); INV2 absent.

### L19 — Portfolio returns/review cycle — classified: request-boundary recompute cycle, not an automatic controller
- **Crate/path:** `kask/mcp-servers/hkask-mcp-portfolio/src`
- **Entry point:** `server.rs:892` `portfolio_seed_price`, `:986` `portfolio_materialize_returns`; `returns.rs`, `analysis.rs`, `store.rs`
- **Trigger:** ledger append → price seed → returns materialization → review (TWR/MWR, attribution)
- **Phase 1 classification (IS + INFERRED boundary):** `ledger_apply` writes transaction state (`server.rs:439`), price seeding invalidates materialized views (`:892-963`), `portfolio_materialize_returns` recomputes from ledger and cached prices (`:986-1012`), and `portfolio_returns` reads the result (`:525`). This closes a **cache invalidation/recompute/readback cycle** at request boundaries; deciding whether the return warrants a new allocation is agent/operator mediated, not an automatic controller in this server. Five properties: closed for materialization/readback, timely per caller, accurate conditional on prices, complete conditional on date coverage, actionable to a reviewer; no live portfolio review ran. No evidence supports a separate automatic return→trade feedback arm, so that arm is explicitly outside this row. No deletion candidate established; an automatic rebalance would change behavior and is not admitted.
- **Prediction:** 1 / 1 / 0.45
- **Closure (2026-09-27):** classified per the worklist's own instruction — a request-boundary cache invalidation/recompute/readback cycle, a genuine loop, not an automatic controller; the return→trade arm is explicitly outside the row and no rebalance behavior was invented. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.45 → actual: 0 defects, 0 impedances, no deletion candidate. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **B** (request-boundary recompute; agent/operator-mediated review); secondary C (the portfolio-review skill — the periodic reflection discipline). Sense→report→actuate: sense ledger/price seeds (`server.rs:892`) → report materialized returns/attribution (`:986-1012`) → actuate review decisions (agent/operator-mediated; explicitly not an automatic controller). Five properties stand; no impedance. Grill: primary-A fails by the row's own classification (no automatic return→trade arm). Reclassification falsifier: an auto-rebalancer. Alignment: INV6 held (the row explicitly bounds the efferent arm); INV2 absent.

### L20 — RSS subscription / conditional feed sync cycle — AUDITED & CLOSED 2026-09-27 (no candidate)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src`
- **Entry point:** `hkask_mcp_research.rs:789` `rss_subscribe`, `:875` `rss_fetch`, `:960` `rss_get_entries`; conditional-fetch state and writeback at `:883-944`
- **Participants:** research DB subscriptions/entries and cached ETag/Last-Modified, `rss_client`, synthetic-feed extraction (`:895-900`), agent tool caller (L1), L4 request envelope
- **Trigger:** subscribe or explicit fetch by agent/user; there is no observed autonomous polling in the inspected tool path
- **Hands off to:** L4 (tool calls); L12 (research source selection); L1 (readback of stored entries)
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35
- **Phase 1 scoped graph (IS):** subscription writes a feed identity (`hkask_mcp_research.rs:789-835`); `rss_fetch` reads cached ETag/Last-Modified and validates the stored URL (`:875-910`), conditionally fetches (`:913-928`), atomically upserts entries and next cache headers (`:935-954`); `rss_get_entries` reads stored results (`:960-970`). The next explicit fetch reuses those headers; synthetic feeds branch to their own extractor (`:895-900`). Five properties: closed for repeated fetches, timely caller-driven, accurate for 304/new-entry distinction, complete per subscribed feed, actionable via retrieval/mark-read tools. No autonomously timed poll is claimed and no live feed was probed. No measured duplication or negative-line candidate; defer further consolidation rather than merge RSS with research-run evidence state (L12).
- **Closure (2026-09-27):** graph verified, no deletion candidate; the merge with L12's evidence state is rejected (distinct retained state and contracts). **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.35 → actual: 0 defects, 0 impedances. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (conditional GET, atomic upsert — deterministic); secondary B (caller-driven fetch/readback). Sense→report→actuate: sense ETag/Last-Modified (`hkask_mcp_research.rs:875-910`) → report 304/new-entry distinction (`:913-954`) → actuate atomic upsert. Five properties stand; no impedance. Grill: primary-B fails — fetch is caller-triggered, but the cycle's feedback (conditional negotiation) is deterministic; a scheduled autonomous poller would still be A. Alignment: INV2 held (the cached validators ARE the expectation — 304 is "as expected"); INV3 held at the protocol level (conditional GET is surprise-gating — the row's implicit RFC 9110 model, still unrecorded as an anchor).

### L21 — Gallery scan / metadata reconciliation cycle — AUDITED & CLOSED 2026-09-27 (no candidate)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-media/src/tools/gallery.rs`
- **Entry point:** `gallery_organize` `:159` (scan/reconcile `:189-193`); `gallery_refresh` `:542` (rescan/reconcile `:552-554`, analysis `:579-580`); query surface `gallery_search` `:250`
- **Participants:** active gallery state, on-disk scan and persisted gallery index/metadata, optional inference analysis (L5), media panel (L7)
- **Trigger:** explicit organize/refresh/search calls; no background scan claimed
- **Hands off to:** L7 (gallery/panel), L5 (analysis), L11 (media jobs/assets); remains distinct from job admission and completion
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35
- **Phase 1 scoped graph (IS):** organize validates and persists the active gallery after scan/reconcile (`tools/gallery.rs:159-199`); refresh rescans to detect missing/changed assets (`:542-558`), analyzes a bounded set (`:567-581`), and returns explicit scan/analysis/errors and pending counts (`:622-665`). `gallery_search` (`:250`) reads the updated index; the media panel is an L7 consumer, separate from L11 job state. Five properties: closed across a repeated refresh/search, timely explicit-call/bounded-analysis, accurate/complete conditional on scan and analysis status, actionable from returned errors/pending. No full-gallery refresh was executed here and no evidenced deletion candidate survives the behavior-preservation test.
- **Closure (2026-09-27):** graph verified, no deletion candidate. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.35 → actual: 0 defects, 0 impedances. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (scan/reconcile — deterministic); secondary B (explicit calls, surfaced errors/pending counts). Sense→report→actuate: sense scan (`gallery.rs:189-193`) → report scan/analysis/errors/pending counts (`:622-665`) → actuate reconcile/persist. Five properties stand; no impedance. Grill: primary-B fails — the counts are surfaced but reconciliation is deterministic. Alignment: INV4 held (explicit errors/pending); INV2 absent.

### L22 — Spreadsheet optimistic-revision / interrupted-operation reconciliation cycle — audited; orphan-revision boundary deliberate and tested
- **Crate/path:** `kask/crates/hkask-spreadsheet/src/service.rs` + `kask/mcp-servers/hkask-mcp-spreadsheet/src/server.rs`
- **Entry point:** `server.rs:94` `spreadsheet_apply` → `service.rs:200` `apply`; reconciliation `server.rs:124` `spreadsheet_operation_get` → `service.rs:217` `operation_get`
- **Participants:** immutable workbook revision and idempotency record, caller-held digest/key, L4 tool envelope, L7 spreadsheet widget
- **Trigger:** user/agent edit; after an interrupted request the caller asks for recorded operation outcome before deciding whether to retry
- **Hands off to:** L7 (editable workbook block), L4 (typed conflict and recovery status); remains distinct from the ordinary one-shot cell calculation
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35
- **Phase 1 scoped graph (IS):** the user/widget submits an edit with base digest and idempotency identity (`server.rs:94-119`); actor checks recorded key, compares the immutable base digest, applies edits, publishes a new revision and operation record (`service.rs:537-615`); after an interrupted call `spreadsheet_operation_get` queries the record (`server.rs:124-140`; `service.rs:213-229`). The recorded result is observable on the next read, while `None` remains explicitly unknown and is not auto-retried. Five properties: closed for recorded operations, timely actor/caller-driven, accurate on digest conflict, complete only for committed records, actionable via reconciliation; no live workbook was mutated by this audit.
- **Phase 2 boundary (IS, deferred):** revision bytes are written before the operation record (`service.rs:590-604`); an interruption in between can leave an orphan revision and `operation_get=None`, deliberately tested at `property_tests.rs:225-264`. This is a surfaced unknown rather than a falsely reported success; changing crash semantics requires an atomic publish contract and does not pass the simple deletion test. No new implementation proposed.
- **Closure (2026-09-27):** the orphan-revision window is deliberate, tested (`property_tests.rs:225-264`), and surfaced as an explicit unknown — not a falsely reported success; an atomic publish contract would be a behavior change, not a deletion. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.35 → actual: 0 defects, 0 impedances, 1 deliberate boundary documented. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (idempotency, digest conflict, reconciliation — deterministic); secondary B (widget edits; the caller's retry decision). Sense→report→actuate: sense edit + base digest (`server.rs:94-119`) → report new revision or conflict/unknown (`service.rs:200`, `:213-229`) → actuate apply/publish. Five properties stand; no open impedance (the orphan-revision window is deliberate and tested). Grill: primary-B fails — the caller decides retry, but the cycle's feedback is deterministic. Alignment: INV2 held (the base digest is the caller's expectation of the base state — optimistic concurrency is expectation-carriage by construction); INV4 held (`None` stays explicitly unknown).

### L23 — Research-provider selection / observed-performance cycle — degraded-status contract landed (S4, 2026-09-30)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-research/src/research/{providers,performance}.rs`
- **Entry point:** `providers.rs:683` `score_providers` → `:730-743` live penalty/readback; provider outcome `providers.rs:315-332` → `performance.rs:76` `record_outcome`; next selection from `hkask_mcp_research.rs:313`
- **Participants:** `ProviderPool`, per-provider bounded recent-outcome samples (`performance.rs:64-81`), search providers, L2 Regulation span archive (`providers.rs:315-322`)
- **Trigger:** each search outcome; subsequent intent-selected search uses the observed success/latency after the minimum sample count (`performance.rs:32-34`, `:109-115`)
- **Hands off to:** L12 (research results), L2 (provider spans), L4 (web search request); separate fast-path in-process selection from the durable Regulation history
- **Prediction (pre-audit, 2026-09-27):** 1 defect / 1 impedance / confidence 0.35
- **Phase 1 scoped graph (IS):** `score_providers` selects from static profiles plus live penalty (`research/providers.rs:683-743`); a provider outcome is spanned and added to a bounded per-provider sample window (`:315-332`; `research/performance.rs:64-81`); after three samples its success/latency measures affect the next selection (`performance.rs:109-171`), and the recommendation carries the snapshot. L2 receives the durable `reg.web.provider` span separately; L12 gets the selected provider's search result. Five properties: closed in one process, timely at next search, accurate conditional on recent samples, complete only for configured/searchable providers, actionable via ranking/rationale.
- **Phase 2 impedance (IS + INFERRED):** a poisoned performance lock drops the outcome (`providers.rs:323-333`) and readback silently substitutes zero penalty/empty live stats (`performance.rs:138-147,177-186`). INFERRED: the operator cannot distinguish a broken feedback channel from a provider with too few samples. Falsifier: force lock poisoning and observe a surfaced degraded-status/rationale or error. Changing this to an explicit status requires a same-seam test and replacement of the current fallback; defer without treating empty stats as validation.
- **Closure (2026-09-27):** the poisoned-lock leg re-verified in the current tree (`providers.rs:323-333`, "Best-effort — a poisoned lock skips the live path"); the degraded-status contract stays deferred with its falsifier — replacing the silent fallback is a behavior change, not a deletion. **Prediction vs actual:** predicted 1 defect / 1 impedance / conf 0.35 → actual: 0 confirmed defects, 1 impedance deferred with falsifier. Brier-scored at Phase 4.

- **Pass 3 layer classification (2026-09-30):** primary **A** (in-process penalty, bounded samples — deterministic); secondary B (surfaced ranking/rationale). Sense→report→actuate: sense provider outcomes (`providers.rs:315-332`) → report ranking/rationale + live stats (`performance.rs:76`) → actuate penalty/selection (`providers.rs:683-743`). Five properties stand. Impedance (standing): poisoned-lock silent fallback — endpoints outcome drop (`providers.rs:323-333`) × zero-penalty readback (`performance.rs:138-147`) — dimension: degradation surfacing (a broken feedback channel reads as "few samples"). Grill: primary-B fails — the rationale is surfaced but the selection loop is in-process and deterministic. Reclassification falsifier: operator-managed provider choice. Alignment: INV4 violated on the poisoned-lock leg (the standing deferral is an INV4 repair candidate); INV2 partial (success/latency expectations form over bounded samples).
- **S4 landed (2026-09-30, operator "proceed" instruction; red-first, two tdd cycles):** the degraded-status contract is implemented — `score_providers` no longer drops zero-penalty rationales (the poisoned arm's message now reaches every recommendation), and `ProviderRecommendation.live_stats_degraded` (serialized only when `true`, so healthy responses are unchanged) distinguishes a broken channel from thin samples; the write-leg comment now states the read side surfaces the skip. Pins: `poisoned_performance_channel_surfaces_the_degradation_in_every_recommendation` (observed RED: the rationale carried only "not configured (no API key)" — the exact falsifier) and `live_stats_degraded_distinguishes_poisoned_channel_from_thin_samples`. Receipts: 145/145 crate tests, rustfmt clean, scoped clippy clean. Source landed in the concurrent stream's `49b5f1518a`; the test-file half (two stub initializers) landed as `dfc2f292d0` after `49b5f1518a` left the `tool_behavior` target uncompilable at HEAD. The standing deferral is discharged; the row's Phase 2 impedance is repaired.

### L24 — Evolution experiment-registry cycle (added 2026-09-30; the server landed in the concurrent stream's `184d828c25`)
- **Crate/path:** `kask/mcp-servers/hkask-mcp-evolution`
- **Entry point:** `server.rs:160` `experiment_propose`, `:211` `variant_register`, `:239` `fitness_record`, `:266` `selection_record`, `:301` `lineage_read`, `:367` `population_query`; store `store.rs:93` `EvolutionStore`
- **Trigger:** agent/operator tool calls through the four-step protocol (Declare → Vary → Test → Select); no autonomous polling
- **Functional graph (IS):** propose (with the pre-registered `Prediction`, `types.rs:28-38`) → register variants (lineage via parent ids) → record grounded fitness (report references only, never simulated) → record the selection verdict (fossils retain selected AND rejected; the first selection resolves; a further selection is `failed_precondition` — race-safe since `301c5a29d5`: the insert and the resolve-flip are one transaction, a lost race rolling the insert back) → lineage/population readback (the fossil record; the population view serves the algedonic agenda and the curator's ORIENT)
- **Hands off to:** L9 (the linked kanban goal scores the prediction — the Brier edge), L16 (the `algedonic_reference` names the review record chairing the selection)
- **Pass 3 layer classification (2026-09-30):** primary **B** (agent-driven in-thread tool calls; the selection decision is agent/operator-mediated); secondary **C** (the durable registry is the fossil record — the reflection substrate; `population_query` is the curator's ORIENT view). Sense→report→actuate: sense = the grounded fitness records; report = the lineage/population readback + the registry; actuate = the selection verdict. Five properties: closed for the protocol lifecycle (propose→select resolves; the readback returns the fossils); timely per explicit call; accurate (grounded fitness — report refs only); complete (selected and rejected both retained); actionable (the fossils prevent blindly retrying rejected mutations). Grill: primary-A fails — no autonomous actuation; an autonomous selection controller would force reclassification. Alignment: **INV2 held by design** (the pre-registered `Prediction` with confidence is the stored expectation, scored via the linked kanban goal — the fleet's second INV2-exemplary loop after L2); INV4 held (the double-selection conflict is surfaced; nothing is dropped); INV5 partial (the registry feeds the algedonic agenda — the C-tier consumption is review-mediated).

### L25 — Evolution registry health sensing (added 2026-10-01; D87 landed in the concurrent stream's `b43a704c9c`)
- **Crate/path:** sensor `kask/crates/hkask-regulation/src/sensor_provider.rs`; bridge `kask/crates/kask_bridge/src/evolution_health_bridge.rs`; producer `kask/mcp-servers/hkask-mcp-evolution`; wiring `crates/zed/src/main.rs`
- **Entry point:** `sensor_provider.rs:514` `EvolutionHealthSensor` (`:526` `observe`); bridge `evolution_health_bridge.rs:47` `open`, `:80` `stuck_running_experiments`; producer `store.rs:709` `health_snapshot` (re-measured after the selection-race transaction landed in `301c5a29d5`); metric `loops/signals.rs:76-83`; wiring `main.rs:2081-2117`, setter `cybernetics_loop.rs:564` `set_evolution_health_source`
- **Trigger:** L2's autonomous @10s sense cycle (`main.rs:1227-1236`, gated on `kask.curator.always_on`); no agent or user involvement
- **Functional graph (IS):** `health_snapshot` reads the registry (status, created_at, max_runs, prediction, verdict, recorded runs) → `stuck_running(7)` keeps STATUS_RUNNING experiments unresolved past the D-3 stale set point or budget-spent with no verdict; an unparseable timestamp reads as stuck — visible, never silent (`types.rs:127-137`) → the bridge re-reads per sense call over the same SQLCipher registry the MCP child serves (path parity, `evolution_health_bridge.rs:16-21`) → the sensor emits `Signal(Cybernetics, EvolutionStuckExperiments, stuck.len(), set_point 0.0)`; a broken source warns and returns `None` — never an empty-Ok collapse (`sensor_provider.rs:526-549`) → L2's canonical deviation → alert-condition → regulation-action machinery → escalated advisory on the board (the efferent half completed 2026-10-01, same day as classification — see the correction below; `impact_direction Some(false)`, `signals.rs:196-198`)
- **Hands off to:** L2 (the entire report/actuate half — alert fan-out, escalation arms, telemetry coalescing), L24 (the sensed substrate; the corrective — recording a verdict — is L24's agent-driven protocol)
- **Pass 3 layer classification (2026-10-01):** primary **A** (autonomous regulation: periodic sensing, deviation vs the zero set-point, standard advisory/escalation — the bridge's own doc names it "the Layer-A afferent pathway for the evolution program", `evolution_health_bridge.rs:14`); secondary **B** (the escalation/toast arm surfaces in-thread through L2's pathway when severity warrants; the wiring `log::warn!` on a failed open is operator-visible, `main.rs:2115-2117`). The C-tier consumption (stuck-experiment escalations feeding the algedonic agenda) is L2/L16-mediated — recorded as a hands-off, not a secondary. Sense→report→actuate: sense = the snapshot re-read per cycle; report = the deviation signal into L2's alert channel; actuate = L2's regulation action, with the corrective delegated to L24. Five properties: closed for sensing (re-observation; "a real zero proves recovery", `sensor_provider.rs:542`) — the corrective arc is delegated, so the A-tier closure ends at the advisory; timely (a 10s cadence against a 7-day stale set point); accurate (the authoritative registry — the bridge opens the same DB the child serves through the shared `hkask_mcp_evolution::registry_path()`; parity is structural, pinned by `registry_path_override_and_default` — the 2026-10-02 v0.25.2 repair of this row's original "documented only" note); complete for the two named stuck classes — an abandoned STATUS_PROPOSED experiment is out of scope by the metric's named boundary (`signals.rs:76-83`); actionable with identity (the board card's context carries the stuck experiment ids — repaired 2026-10-01, same day as classification; the count-only form never shipped). **Classification correction (2026-10-01, same day):** the row's original graph claimed an "advisory per severity policy" — overclaimed. D87 landed the sensor and metric but **no policy rule**: `RegulationPolicy::decide` returned empty for `EvolutionStuckExperiments`, so the deviation was sensed and then silently dropped (no advisory, no escalation, no board card) — the `.rules` broken-feedback-loop class, and the policy's advertised "covers all variants" Ashby claim had no enforcement. The efferent half was completed the same day: the `EvolutionStuckExperimentsExceeded` rule (Escalate to Curation, mirroring the OCR precedent), the `RegulationData` variant carrying count+threshold, the `build_regulation_action` arm, and the delivery-boundary id enrichment (`error_context["stuck_experiments"]`, re-read from the retained source — the outcome-breakdown precedent); the closure is now pinned by `every_signal_metric_has_a_rule_or_documented_allowlist_entry` (an exhaustive or-pattern — a new variant breaks the build until the test carries it, rule or allowlisted-with-reason). Impedance repaired with it: endpoints `stuck_running_experiments` (returns ids, `evolution_health_bridge.rs:80-86`) × the board card (now carries the ids at delivery) — the identity fidelity gap the row originally recorded is closed; `population_query` remains the deeper readback, no longer the required bridge. Grill: the strongest alternative is a fold into L2's participant list — the other zed-side sensor bridges (context-server health, OCR health, rollout events) are L2 participants, not rows. Discriminator: those bridges are thin readbacks of already-rowed loops, while this pathway's stuck semantics (stale days, budget-spent, unparseable-reads-stuck) are domain policy living in the sensed crate (`types.rs:127-137`), and the cycle's corrective closes through L24 — a two-row cycle, not an internal leg. Reclassification falsifiers: moving the stuck semantics into the sensor (a thin readback) would fold the row into L2's participants; an autonomous verdict-timeout actuator (the loop itself resolving or voiding stuck experiments) would drop the B secondary; an advisory consumed only via the review agenda would demote B to C. Alignment: **INV1 held by construction** — an afferent sensor feeding L2's existing canonical pathways; no thirteenth report pathway (the R1–R12 inventory unchanged; the broken-source warn rides R3, the escalation rides R6/R7 through L2); INV2 held at the signal level (set-point 0.0 + deviation — the loop's generic expectation carriage; the stored-prediction form is L24's); INV3 partial (per-tick signal emission; the report side is coalesced by L2's telemetry coalescer — the fleet-wide standing state); **INV4 held** (broken-source warn-and-None — the `.rules` `unwrap_or(0)` trap explicitly avoided and pinned by `evolution_sensor_returns_none_on_broken_source`); INV5 partial (B→C consumption is L2-mediated); INV6 held (a clean one-direction afferent: registry → bridge → sensor → loop; no efferent arm of its own).

## Boundary notes (sub-cycles folded into rows above, not separate rows)

- Email alert delivery — sensor leg inside L2 (`hkask-email`, `main.rs:656`).
- Conversation/context injection — leg inside L1 (`hkask-conversation-injector`, `kask_bridge/context_injector.rs`).
- Thread condensation — leg inside L1 (`hkask-condenser` via `condenser_bridge`).
- Polymarket resolution subscription — leg inside L8 (`market_subscribe_resolutions`, paired with `market_record_resolution` per `hkask_mcp_prediction_markets.rs:264-298`).
- Ordinary spreadsheet cell calculation is request-driven; interrupted-operation reconciliation and persistent revision identity form L22.
- Market health/`web_ping` style probes — legs inside L3/L4.

## Decomposition into audit slices (INVEST) — executed

The pass-1 INVEST worklist (one slice per register row, Batches A–D)
was approved and executed 2026-09-27/28: every row's outcome — closed,
consolidated, or deferred with falsifier — is recorded in the row
itself, with commit hashes in the change log. The per-loop worklist
table, the checkpoint rules, and the open-risk notes are condensed here
per the alignment plan §8 CU-5; git history is the archive.

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

### Pass-2 loop re-slice delta verification (2026-09-28 — closes the re-slice stage)

Delta-first triage per the approved plan: the four spec-named rows got full
re-maps (L1/L3/L5/L7, recorded in their rows above); the remaining rows —
which already carry pass-1/early-pass-2 Phase 1–2 content — were verified
against the current tree (entry-point citations, mechanism intact, anchor
status per the ledger, loose-end status unchanged). Every citation drift
found is attributable; no row's mechanism changed.

| Row | Verdict | Drift (current lines) |
| --- | --- | --- |
| L2 | HOLDS | `metacognition.rs:346/:405`, `cybernetics_loop.rs:780` all exact |
| L4 | HOLDS | S13 framework re-check (2026-09-28) at current lines |
| L6 | holds, cites drifted | `corpus_ingest_qa` `:174→:202`, `ground_generated_qa` `:447→:442` (the S6 `grounding_fields` fix); convert `:35`/chunk `:355` exact |
| L8 | HOLDS | `calibrate_forecast :874`, `forecast_record :1219` exact (S10 verified; S10's fix touched providers.rs only) |
| L9 | holds, cites drifted | `kanban_goal_create` `:461→:465` (the S8 classification fix) |
| L10 | HOLDS | `memory_store.rs:288/:331/:447` all exact |
| L11 | HOLDS | `jobs.rs:167/:210/:229/:304` all exact |
| L12 | holds, cites drifted | `begin_research_run` `:1604→:1632` (the S7 intent-validation fix) |
| L13 | closed pass 2 | prior-pass closure stands (`57c2bdea7a`) |
| L14, L15 | closed | minimal-by-design closures stand |
| L16 | holds, cites drifted | `agent.rs:4853→:4860`, `:4860→:4867`; `thread.rs:4431→:4468`, `:4503-4518→:4542+` (concurrent streams); `main.rs:996` holds; the Phase 2 adjudication (sticky attribution = D59 pinned design) unaffected |
| L17 | HOLDS | `scenario_score :1281` exact (S5 verified) |
| L18 | HOLDS | `training_submit :28` exact (S2 verified) |
| L19 | HOLDS | S4 review-only — no line changes |
| L20 | holds, cites drifted | `rss_subscribe :789→:817`, `rss_fetch :875→:903`, `rss_get_entries :960→:988` (the S7 fix) |
| L21 | HOLDS | `gallery_organize :159`, `gallery_refresh :542` exact (S12 review-only) |
| L22 | HOLDS | S1 review-only — no line changes |
| L23 | HOLDS | `score_providers :683/:1029` (S7 verified; the intent vocabulary fix is in the handler, not the scorer) |

No new findings from the delta pass: the drift is line-shift only. The
re-slice stage is closed; the register's rows now describe the current tree
at the cited lines, with the four re-mapped rows carrying their own
supersession notes.

## Phase 4 — pass-2 closure (2026-09-28)

### Lessons (written down; promotion to curator memory via algedonic review)

1. **The defect population of a mature codebase is lies of omission, not
   broken machinery.** 13 of 14 pass-2 findings were a false fallback claim,
   a discarded error, a silently-degrading input, a stale doc advertising a
   default that no longer exists, or a misclassification — each machinery
   intact, each truth missing. The fix pattern that held every time: surface
   with classification + consequence (what failed, what the operator now
   cannot see). Evidence: S3–S12 rows, L23.
2. **Decisions must be visible where they're made.** Media's gallery DB
   carries its reasoned unencrypted-by-choice rationale at the site; the
   portfolio family's identical-looking silence is a finding. The absence
   of a recorded decision is itself the defect. Evidence: F-P1 vs S12.
3. **GPUI notify behavior is measurable red-first.** An observer on the
   entity + a never-resolving task isolates the synchronous path; the
   missing-notify bug failed red before the one-line fix. Evidence:
   `9142f4f03e`.

(Recording note, corrected per operator 2026-09-29: this audit ran in a
**curator thread** — the turns themselves are the episodic record, chunked
by the normal ingestion path (`curator:thread:{id}`), and the distillation
pass extracts the durable lessons from those chunks through the same
evidence + 0.5-floor invariants as any memory. No separate "promotion"
step exists or is needed; the earlier algedonic-review framing here was
wrong. A mid-thread semantic-search probe for this thread's chunks
returned none — the instrument's own recorded limitation (unembedded
chunks are invisible; `curator_memory_backfill_embeddings` exists for
exactly that lag), not evidence of absence.)

### Pass-2 operator decision queue — DISPOSITIONS (operator rulings 2026-09-29)

| # | Decision | Ruling | Disposition |
| --- | --- | --- | --- |
| D1 | F-P1 plaintext DBs | **RESOLVED — split is the design:** encryption on core-crate activities; MCP-server data stays plaintext so servers can be broken off and run in an independent MCP-server pattern (no zed keychain, no HKASK_DB_PASSPHRASE) | Rationale recorded at the three sites (portfolio `store.rs`, companies `screen_store.rs`, research `hkask_mcp_research.rs`); F-P1 CLOSED |
| D2 | Reference-model records | **A, executed honestly-narrowed** | Of the 9 recorded skill gaps, **8 were false gaps** — the skills carry recorded reference models in other forms the plural-heading grep missed (cmp-term-structure/listening prose `**Reference model.**`; media-workflow `## Reference model`; transcript-reel `## Reference model and labels`; eqm's opening paper citation; local-research-swarm's `## Anchors and boundary`; build-corpus-pipeline's `## Anchors and contract`; swarm-intelligence's inline author-year citations) — the spec's "or another recorded form" clause covers them, and the instrument lesson (probe validity before reading absence) is recorded. **Fixed:** lora-training's true gap (a `## Reference models` section citing LoRA arXiv:2106.09685, QLoRA arXiv:2305.14314, the method-init catalog, and the gate contracts) and the kata-kanban per-server doc (created with its reference models, storage/credentials, surfaces, and testing standard). SKILL.md pin suite re-run green (28/28) |
| D3 | Memory receipts | **B** — the loose coupling was a deliberate resilience decision; tight coupling and receipts make rigid failure-prone code | Deferral stands with the rationale recorded |
| D4 | Done-verdict check | **B** — deliberate loose coupling for resilience | Stands |
| D5 | Recall failure-signal | **A** | **DONE:** the keyword leg warns naming the failure (partial results stand); the thread leg propagates — a failed read is an error, not "no memories"; kask_bridge 253/253 green |
| D6 | Closable research runs | **A — IMPLEMENTED** (`220ef6e394`) | `finish_research_run` landed: terminal statuses completed\|partial\|blocked\|failed; the sources ledger never mutates — transitions append to `research_run_status_history` (the audit trail) and update the run's latest status; completed/partial enforce the server-recorded-source rule as `failed_precondition`; surface pin 26→27; two contract tests; 143 green |
| D7 | Nebius status contract | **A, implementation deferred** to a later follow-up (not a current worry) | Ruling recorded; no code yet |
| D8 | F4 TOCTOU | **A — IMPLEMENTED** (`971185a553`) | The shared `write_contained` primitive landed in `hkask-mcp-server`: containment check then an `O_NOFOLLOW` final-component open (libc, unix) — a symlink planted between check and open is refused as ELOOP, classified `invalid_argument` naming the refusal; non-symlink IO classifies through `map_io_error`; the residual intermediate-directory window documented at the primitive. Seven call sites swapped (corpus helpers/document×3, training rollout-bridge×2/dataset); **media had no caller-supplied write paths** — the priced gallery site was wrong on inspection (gallery writes are server-computed). Pinned by the symlink-swap rejection test (outside target never created) + the plain-write test; 274 tests green, scoped clippy clean; the full-repo gate was blocked by a concurrent actor's uncommitted agent-crate work at landing (not this change) |
| D9 | Invocation-identity | **B** | Stands (the block is deliberate and documented) |
| D10 | Spreadsheet crash-durability | **B** | Stands |
| D11 | Passphrase onboarding | **B, wontfix-by-design** — the current behavior is the design: default provisioned on install, the user rotates when they want; R1 CLOSED (the audit's option A was over-engineering) | Stands |
| D12 | Repair-plan §8 #2–#8 | Deferred as a batch (not addressed this round) | Stands |

### Follow-up plan execution (2026-09-29, operator-approved order F3→F2→F1→F4)

| # | Follow-up | Disposition |
| --- | --- | --- |
| F3 | Register pin-citation freshness sweep | **DONE** (`cfb89021f8`): six drifted inventory citations refreshed (scenarios +7, training +4, companies +2, corpus +9, swarm +4, media +21) — all eleven now match the tree |
| F2 | Legitimate-boundary pin sweep | **DONE** (`8f60b45efa`): the corpus credential declaration pinned in the kata-kanban source-pin form; the intent-validation and thread-recall legitimate sides verified pinned; the D5 propagation arm recorded as source-verified (unreachable through the public seam without disproportionate harness); the corpus tagging-tests fmt drift fixed |
| F1 | D7 Nebius status contract | **DONE** (`83c5f27b95`): `RunningUnknown` + `completion_check_unavailable` — a failed completion check no longer claims Running; the legitimate short-circuit (unconfigured HF) keeps Running, pinned; the failed-check path source-verified (a live HF fetch is the only in-test route; injection seam disproportionate); 45 training tests green |
| F4 | finish_research_run polish | **DONE** (`bbac2563f9`): the manifest surfaces `status_history` (from/to/note/at per transition), pinned by the extended contract test; the one-second PK edge needs no code — `now_rfc3339` carries sub-second precision, accepted and documented |
| F5 | `.rules` proposals | **ADMITTED (operator ruling 2026-09-29, essentialist-tested):** both rules passed the 3-gate test in reduced forms — Rule 1 ("a fix that restricts behavior pins the legitimate case it must not break — in the same change") admitted to the test-protocol traps (G2 reduction: "security fix" → "behavior-restricting fix", since D7 was an honesty fix; G1 evidence: the O_NOFOLLOW catch + the D7 prevention); Rule 2 ("a sweep pattern that under-matches the semantic target reports false absence — validate against a known-positive") admitted to the agent-loop traps (G2 reduction: the general epistemic form was a map, not a trap; distinct from the bare-glob rule) |
| F6 | D12 repair-plan §8 walk-through | **EXECUTED** — three of seven resolved by prior rulings/inspection (#5 by D11 wontfix-by-design; #6 by inspection: `.github/workflows/kask-invariants.yml:151` runs the feature-gated reconnect suite; #4 largely by D10, H1's dynamic test a small follow-up if loss is observed); four ruled 2026-09-29: **#2 B** (no destructive-deletion override — the mode is the consent gate; current behavior ratified), **#3 B** (single-webid attribution stands; folds into the P2 deferral per D9), **#7 A-as-scheduled-follow-up / B-until-landed** (normalize containment to the active project root — one authority, matching the D8 shared-canonical-paths principle — but a behavior change with spawn-plumbing implications, so the documented dual-root behavior stands until then), **#8 A IMPLEMENTED** — both `Interrupted` construction sites now carry one canonical effect note ("the call was accepted, so effects may have been applied; inspect the server's state before re-invoking") via a shared helper; reconnect suite 17/17 green at the process boundary |

**F5 — proposed `.rules` additions (operator ruling requested):**

1. *A security fix's legitimate-use boundary is pinned in the same change
   as the attack it closes.* Non-obvious (the refusal test alone stays
   green while a legitimate use regresses — observed: the O_NOFOLLOW
   primitive's inside-root-symlink boundary was reasoned but unpinned
   until the review caught it); specific enough to act on; encountered
   once expensively.
2. *Probe instrument validity before reading absence as evidence — a
   too-narrow sweep reports false gaps.* Non-obvious (the plural-heading
   grep produced 8 false gap records; the same class recurred as the
   register's own inventory drift); repeatedly encountered; the curator
   already carries it as a memory — the `.rule` would bind audit work
   generally.

## Change log

- 2026-10-01 — v0.25.5 closed the per-server doc gap (the operator's
  proceed ruling; the last open finding from the pricing pass).
  `reference/mcp-servers/curator.md` (15 tools — the sovereign-memory
  and regulation-readback surface; the shared-store design; the
  passphrase required) and `reference/mcp-servers/training.md` (9 tools
  — the consent-gated training job surface; the passphrase optional
  with the WARNED in-memory fallback, documented as the deliberate
  contrast with the start-refusing durable stores) created per the
  fleet's per-server discipline. Both credential declarations
  verified honest against the startup code before writing (the F-K1
  audit rode the survey): the curator's `required` matches its
  refusing startup; the training server's `optional` matches its
  surfaced degradation. Corpus 72 → **74** (under the 75 cap; the
  60-file target's floor moves with it — the discipline requires the
  docs). The plan's sixth-slice deferral note closed.

- 2026-10-01 — v0.25.4 closed O3 as designed (operator ruling "close it
  out"; records-only, zero code). Expectation carriage is scoped to
  actuation against a stored reference: the scored loops hold it (L2's
  set-points, the L8/L9/L17 Brier priors, L22's digest, L24's
  pre-registered predictions) and the board cards already carry the
  expectation pair (deficit vs threshold in every escalated alert's
  context). The event-record pathways (R1–R4, R9, R12) stay
  expectation-free by design — fields there would be ceremony with no
  scoring consumer, failing the deletion test. The INV2 gap-table
  verdict gains the scope (its citations re-measured after the
  v0.25.1/v0.25.3 deletions); the reference model's INV2 gains the
  scope note (paper v1.0.2); the plan's parking lot is now **fully
  dispositioned** — O1/O2/O5 landed, O3 closed-as-designed, O4/O6
  ruled design.

- 2026-10-01 — v0.25.3 deleted the strategy evaluator — self-tracking
  dead surface since its birth (the operator-approved consumption
  sweep). **Finding (IS):** `StrategyEvaluator` advertised
  "multi-model strategy selection" with a ladder table, but
  `RegulationStrategy` carried only a name — no ladder content, no
  behavior difference between "default" and "aggressive"; the active
  strategy was a private field with **no read path** (no getter, no
  consumer — the promotion's only observables were an info log and
  an `ActionSubstituted` span); and that span kind's only emitter was
  the promotion site itself. Added with the original regulation
  crate (`72a37d9507`), never wired. **Deleted:** the module
  (173 lines), the loop field + import + initializer, the tick
  integration block (~47 lines), the `SpanKind::ActionSubstituted`
  variant + mapping (decode-safe: `Span` persists as plain strings
  with no reverse mapping — old archive records unaffected), and
  the two doc rows (`regulation-spans.md`'s span-kind table, the
  diataxis hkask-regulation module table). Net **−209 lines**
  (+25/−234). The L2 row's tick graph
  drops the node and its citations are re-measured (the drift rode
  the v0.25.0 enrichment block and this deletion). Receipts:
  hkask-regulation --lib 104/104, hkask-types --lib 87/87, rustfmt
  clean, scoped clippy clean, `cargo check -p zed` passed.

- 2026-10-01 — v0.25.2 made the L25 bridge/server registry-path parity
  structural (the recommended pin, executed as the deeper fix). The
  evolution crate gains `registry_path()` / `registry_path_from()`
  (the pure core, testable without process-global env mutation); the
  server's `run()` and the in-host bridge both resolve through it —
  the bridge's copied resolution deleted, the server's inline block
  replaced. Pinned by `registry_path_override_and_default` (a
  non-empty override honored; empty/whitespace/absent all fall to the
  same per-agent default — the two sides cannot resolve different
  registries). One deliberate startup delta, unpinned by any test:
  the parent-directory creation now also applies to an overridden
  path (previously only the default branch created it — an override
  whose parent was absent failed at open; it now succeeds). Net
  ~+30 lines (a structure-and-pin slice, not condensation). Receipts:
  hkask-mcp-evolution --lib 14/14, kask_bridge --lib 258/258, rustfmt
  clean, scoped clippy clean, `cargo check -p zed` passed.

- 2026-10-01 — v0.25.1 deleted the legacy Loop-6 metric vocabulary (the
  operator's proceed ruling; the v0.25.0 closure test's allowlist was
  the finding instrument). **Evidence first:** the four metrics
  (`EnergyRemaining`, `ErrorRate`, `ConnectorLatency`,
  `CommunicationQueueDepth`) had zero production emission sites (the
  2026-10-01 producer sweep), no policy rule, no set-point, and zero
  doc mentions; both `from_str_name` production callers degrade
  unknown names to a visible warn-and-skip, so no persisted or
  submitted record can crash on the deletion. **Deleted:** the four
  variants and their `as_str`/`from_str_name`/`impact_direction`/
  `from_signal` arms; the strategy evaluator's four inert seed
  entries; the rollout bridge's two unreachable string arms (one a
  naming lie — "energy_remaining" extracted token usage); the stale
  `RegulationRule` doc example naming the long-deleted
  `Throttle`/`AdjustEnergyBudget` action types; fixtures repointed to
  live metrics. The round-trip name list and the closure test's
  allowlist shrank with them (the allowlist is now the three
  deliberate no-producer entries: PassRate event-driven,
  TestCoverage/MutationScore decode-only). Net **−356 lines** across
  six files (+38/−394). **Landing provenance:** the concurrent stream
  absorbed the working-tree deletion into `301c5a29d5` ("prune dead
  signal metrics") together with its selection-race transaction —
  the `49b5f1518a` pattern; the deletion set verified intact at HEAD
  (zero variant/bridge-arm matches). Receipts re-run at HEAD:
  hkask-regulation --lib 104/104, kask_bridge --lib 258/258,
  hkask-mcp-evolution --lib 13/13 (the stream's race tests), rustfmt
  clean, scoped clippy clean, `cargo check -p zed` passed. **Rode
  along:** the L25 `health_snapshot` citation re-measured to
  `store.rs:709` (the promised refresh, due when the stream's work
  landed); the L24 row notes the selection gate is race-safe since
  the same commit.

- 2026-10-01 — v0.25.0 completed the D87 pathway's efferent half (the
  operator's code-work direction; L25's classification correction
  rides in the row). **Finding (IS):** D87 landed the evolution
  sensor and metric but no policy rule — `RegulationPolicy::decide`
  returned empty for `EvolutionStuckExperiments`, so the stuck-experiment
  deviation was sensed and then silently dropped (no advisory, no
  escalation, no board card); the policy's advertised "covers all
  variants" Ashby claim (ADR-056) had no enforcement, and the
  L25 row's original graph had overclaimed the actuation accordingly.
  **Fix:** the `EvolutionStuckExperimentsExceeded` rule (Escalate to
  Curation, mirroring the OCR precedent), the `RegulationData`
  variant carrying count+threshold, the `build_regulation_action` arm,
  and the delivery-boundary identity enrichment — the board card's
  `error_context["stuck_experiments"]` carries the stuck ids re-read
  from the retained source (the outcome-breakdown precedent; a failed
  re-read warns and delivers the count — visible degradation, never
  silent). **Closure pinned:**
  `every_signal_metric_has_a_rule_or_documented_allowlist_entry` — an
  exhaustive or-pattern (a new variant breaks the build until the test
  carries it, rule or allowlisted-with-reason; the seven no-producer
  variants allowlisted with their verified reasons). Red-first: the
  closure test, the alert-ids test, and the all-reasons case list all
  observed RED before the implementation. Net **~+100 production
  lines** (measured: 254 crate insertions, ~150 of them tests; a pathway
  completion closing a broken feedback loop, not a
  condensation candidate — the subtractive constraint governs
  condensation; this slice replaces silent-drop behavior, which has no
  lines to delete). Receipts: rustfmt clean; hkask-regulation --lib
  **104/104** via cargo-test-nonzero; scoped `./script/clippy` clean
  (machete + buf); `cargo check -p zed` passed.

- 2026-10-01 — v0.24.9 classified the D87 evolution-health pathway as
  **L25** (landed in the stream's `b43a704c9c`; classified per the
  operator's ruling on the deferred decision item, after verifying the
  stream's in-flight selection-race diff is orthogonal to the pathway —
  zero `health` hunks; the one at-risk citation, `store.rs:707`, is
  commit-anchored at `c57e1706db`). Primary A, secondary B; the C-tier
  consumption is a hands-off (L2/L16-mediated). INV1 held by
  construction — a new loop adding zero report pathways (an afferent
  sensor feeding L2's canonical arms; R1–R12 unchanged); INV4 held
  (broken-source warn-and-None, pinned by test). One impedance
  recorded (IS): ids dropped at the sensor boundary — the signal
  carries the count, the actuation needs the identity; bridged today
  by L24's `population_query`. The fold-into-L2 alternative was
  grilled and rejected with the discriminator recorded in the row.
  **Table completion repair (this pass):** v0.24.7's L24 addition left
  the layer-coverage, anchor-ledger, and family-coverage tables
  un-extended (the session review did not catch it); L24 and L25 rows
  now complete the per-row claims, and the INV1/INV4 gap-table
  exemplars gain L25. The reference-model paper's §1 scope statement
  refreshed L1–L23 → L1–L25 (paper v1.0.1) — L25 is a direct instance
  of the paper's own afferent-pathway structure.

- 2026-10-01 — v0.24.8 closed the v0.24.7 S4 re-run deferral. The
  concurrent stream's provider refactor landed (Crossref title
  resolution `b7c0b15f6f`; retrieval-honesty pins `4e265ba2f8`) and the
  full re-run executed against the current tree: hkask-mcp-research
  --lib **88/88** via cargo-test-nonzero — both S4 contract tests green
  (`live_stats_degraded_distinguishes_poisoned_channel_from_thin_samples`,
  `poisoned_performance_channel_surfaces_the_degradation_in_every_recommendation`);
  the stream's tool-count pin grew to 27 registered tools (their
  update, green). The same pass re-verified S3 in the evolved tree:
  hkask-regulation --lib **102/102** (grown from 99 by the stream's D87
  evolution-health sensor tests), rustfmt clean on both touched crates,
  scoped clippy clean (machete + buf), `cargo check -p zed` passed.
  Provenance: the battery ran on the working tree including the
  stream's uncommitted-but-compiling storage/evolution edits (a
  mid-flight storage half-edit briefly blocked the battery and settled
  before the run). Register note: the stream's `22544f9fa8` flipped
  L8 GAP → ANCHOR (the superforecasting reference model) directly in
  the anchor ledger.

- 2026-09-30 — v0.24.7 closed the evolution server's recorded gaps (the
  session review's next focus). **L24 added** (the evolution
  experiment-registry cycle — the server landed in the concurrent
  stream's `184d828c25`): primary B, secondary C, **INV2 held by
  design** (the pre-registered `Prediction` scored via the linked
  kanban goal — the fleet's second INV2-exemplary loop after L2).
  **The per-server doc created**
  (`reference/mcp-servers/evolution.md`), and the docs README gained
  its row plus the previously missing kata-kanban row (a D2 loose end
  the review's grounding surfaced). **One repair in the stream's new
  code, per the fleet's adjudicated pattern:** the server declared
  `vec![]` credentials while consuming `HKASK_DB_PASSPHRASE` and
  refusing to start without it — the F-K1 class; fixed to the
  kata-kanban `required` declaration (the honest form for a
  startup-refusing durable-storage server) with a source-pin test
  mirroring the corpus/kata-kanban pins. The S4 re-verification: the
  concurrent stream's provider refactor is still in flight; its diff
  does not touch the S4 regions (verified), and the full test re-run
  waits for their landing.

- 2026-09-30 — v0.24.6 executed the operator's aeneas ruling ("delete"):
  the three-file Aeneas record set removed (the Proposed plan, the
  adoption-gate record, the feasibility evidence) — a blocked upstream
  program (Aeneas issue #838) that authorized nothing. Successors
  named in the README lifecycle ledger tombstone; git history is the
  archive. `DOCUMENTATION_STANDARDS` §258's example row updated in the
  same change. Corpus: 74 → **71 files** (70 md + 1 yaml). Gates
  re-run green (count, links, no-deleted-surfaces — only tombstone
  mentions remain).

- 2026-09-30 — v0.24.5 executed the alignment plan §8 CU-5's first
  condensation pass over this register (the named first candidate).
  The superseded pass-1/pass-2 process narrative is condensed with
  successors named: the pass-2 checkpoint's process subsections
  (premise verification, prior-findings reconciliation, tree staleness,
  loose-ends ledger, MCP tool inventory, the S1–S12 per-server
  tool-review ledger, predictions, INVEST decomposition) → the
  register rows, the per-server reference docs, and the change log's
  v0.23.x entries; the Phase 4 partial ledger → the change log's
  per-commit ledgers; the closure's scored-predictions/AC6/gap-ledger/
  acceptance subsections → the change log and Pass 3's INV gap table;
  the executed pass-1 INVEST worklist → the rows. **Retained:** the
  reference-model anchor ledger (the admitted model cites it), the
  lessons with the operator's recording-note correction, the D1–D12
  dispositions, and the F1–F6 follow-up records. Net **−373 lines**
  (1,882 → 1,509); git history is the archive. The corpus count is
  unchanged (in-file condensation); the whole-file candidates are
  priced separately (the aeneas trio — operator-retained Proposed,
  blocked upstream; the LogiSheets plan — Active with open phases
  6–7, kept).

- 2026-09-30 — v0.24.4 executed the alignment plan §8's second cleanup
  slice (CU-3/CU-4, the advertised-invariant sweep), verified against
  the current tree throughout — past audits are context, not verdict.
  One deletion: the `tool_schema` backward-compat re-export in
  `hkask-mcp-server` (the shim + its comment) with 16 import sites
  across 8 servers repointed to the canonical `hkask_types::` root
  re-export and the module doc repaired — net **−10 production lines**
  (+21/−31). The seven load-bearing advertised invariants all verified
  with cited enforcement lines (`write_turn`, `validate_board_name`,
  the >=8 passphrase minimum, the legacy-threshold rejection gate, the
  never-untagged ladder, the `busy_timeout` ordering, the migration
  drop-safety transaction); `#[deprecated]` zero in kask. Receipts:
  cargo check on all 10 affected crates, **1340 tests green**, rustfmt
  clean, scoped clippy clean, `cargo check -p zed` passed. Running §8
  production ledger: **−18**.

- 2026-09-30 — v0.24.3 executed the alignment plan §8's first cleanup
  slice (CU-1 + CU-2 + CU-7's named candidates). One deletion:
  `hkask-storage` `Database::in_memory_with_extensions`
  (`connection.rs:349`, zero references repo-wide including tests and
  docs) plus the `in_memory_impl` helper its deletion orphaned (inlined
  into `in_memory()`) — net **−8 production lines** (+2/−10). All other
  instruments clean with evidence: the dead-code-allow baseline's one
  kask site is a documented test fixture (rejected); 32 traits, 1,479
  pub fns, and 1,426 pub types swept with zero further candidates;
  `cargo machete` clean in kask scope; CU-7's named candidates all
  alive with cited consumers (the standalone subscriber is a test
  fixture in a live module; `HKASK_USE_FAL_DOCRES` is a removal guard,
  not an opt-in; no dead settings knobs). Receipts: hkask-storage
  --lib 72/72, rustfmt clean, scoped clippy clean, `cargo check -p
  zed` passed; full-repo symbol sweep over code AND docs clean.
  Running §8 production ledger: **−8**.

- 2026-09-30 — v0.24.2 executed the remaining open items under the
  operator's proceed instruction and recorded the cleanup program.
  **S4 landed** (L23's degraded-status contract): red-first, two tdd
  cycles — `score_providers` no longer drops zero-penalty rationales
  and `ProviderRecommendation.live_stats_degraded` (serialized only
  when true) distinguishes a broken channel from thin samples; 145/145
  crate tests, rustfmt clean, scoped clippy clean; source in the
  concurrent stream's `49b5f1518a`, the test-file half in `dfc2f292d0`
  (repairing `49b5f1518a`'s uncompilable `tool_behavior` target at
  HEAD). **S6 recorded** (pathway scoping): the R1–R12 role table —
  three canonical pathways (A: the R5 archive with R10's producer and
  R6's escalation arm; B: the envelope + R7 toasts on R11's substrate;
  C: R8 memory with R12 work-state), eight diagnostics pathways with
  explicit roles, one substrate; the no-candidate finding stands (the
  one true duplication was S3). **C decided** (standards diagram):
  the self-illustration exemption added to DOCUMENTATION_STANDARDS
  §4.2 and the registry note updated to cite the rule. **§8 added to
  the alignment plan** (operator direction): the legacy/orphan cleanup
  and strangler-fig removal program (CU-1–CU-8) under the
  no-backward-compatibility rule — deletions are outright; no compat
  shims, no deprecated attributes, no kept-for-compatibility states.

- 2026-09-30 — v0.24.1 executed the operator's checkpoint rulings ("proceed
  as proposed — confirmed"). **Reference model ADMITTED** —
  `research/cybernetic-nervous-system-reference-model.md` (status Active,
  v1.0.0) is now the structural anchor for the layer taxonomy; the pass-2
  anchor ledger's GAP rows gain this shared anchor alongside their
  row-specific records. **S2 landed:** the in-host tracing→log bridge
  recorded as a designed pathway in `reference/regulation-spans.md` §1
  (v0.42.0, +21/−2 doc lines) — the undocumented-coupling finding closed
  by record. **S3 landed:** the per-tick `tracing::debug!` raw-activity
  duplicate of the coalesced `LoopMetricsTelemetry` span deleted from
  `hkask-regulation/src/cybernetics_loop.rs` — **net −14 production
  lines** (0 insertions), the INV3 direction (delete raw activity, keep
  the surprise-gated form). Falsifier pre-check: zero consumers of the
  debug format (only the `SpanKind` doc comment remains, documenting the
  span); 0 live-log occurrences (zlog filters debug — the emission was
  inert output; the Tracy-build destination is the only behavior delta,
  as priced). Receipts: rustfmt --check clean; hkask-regulation --lib
  99/99 via cargo-test-nonzero; scoped `./script/clippy` clean (machete
  + buf included); `cargo check -p zed` passed. The coalescer region's
  citations re-measured to `cybernetics_loop.rs:895-968` in this change.
  The gap table's INV3 verdict stays GAP (one coalesced pathway of
  twelve) with the S3 direction note recorded; S6 (pathway scoping)
  remains the named direction. Goal ground truth confirmed by the
  operator — the 0.75 intake prediction Brier-scored at 0.0625.
- 2026-09-30 — v0.24.0 OPENED the three-layer cybernetic nervous system
  review (operator spec 2026-09-30). Phase 1 prediction recorded at open,
  per the spec's D5: (a) expected unclassified rows **23/23** — the A/B/C
  layer vocabulary does not exist in this register (no row carries a layer
  classification), confidence 0.85; (b) expected cross-layer impedances
  **~3–5**, specifically: no expectation signal in any logging path outside
  L2's set-point comparison (logs record events, not prediction-error
  deltas); plural reporting pathways per layer (log/tracing macros,
  regulation spans, algedonic board, curator memory, panel surfaces) with
  no canonical one per layer; Layer C (periodic reflection) mediated
  entirely by skills with no machine-readable expectation record,
  confidence 0.55; (c) expected citation-resolution failures **0–2**
  (Clark 2023 and Hawkins 2021 are well-known; Thousand Brains Project
  publications and interoception literature may present ambiguous
  resolutions), confidence 0.60. Overall phase confidence 0.60. Scored at
  close-out in this same entry.
- 2026-09-30 — v0.24.0 CLOSED the three-layer cybernetic nervous system
  review. **Prediction scored at close-out:** (a) 23/23 unclassified —
  CONFIRMED exactly (conf 0.85 was right). (b) impedances — CONFIRMED on
  the logging-path reading (twelve report pathways, no canonical one per
  layer; expectation signal absent from R1–R4/R9/R12) and PARTIALLY
  REFUTED on the broader reading (machine-readable expectation records DO
  exist outside L2: the Brier priors in L8/L9/L17 and L22's base digest);
  the in-pass discovery — the in-host tracing drop — was itself REFUTED by
  the Pass 4 live-log experiment and corrected to the undocumented
  log-feature bridge (`crates/rpc/Cargo.toml:35`); ~3–5 held as a class
  count (1 new corrected finding + the standing cross-layer deferrals
  re-confirmed). (c) 0 citation-resolution failures — in range at the
  favorable end. **Deliverables:** D1 (Pass 3 instrumentation section:
  naming survey, mechanism inventory R1–R12, premise verdict, layer
  coverage), D2 (`research/cybernetic-nervous-system-reference-model.md`,
  admission pending), D3 (23/23 per-row classifications + saturation
  report + alignment gap table), D4
  (`plans/cybernetic-nervous-system-alignment.md`), D5 (this entry).
  **Net production lines: 0** (doc-only pass; the S3 deletion candidate is
  priced and gated on the checkpoint ruling). Calibration: the phase
  confidence 0.60 was about right; the lesson banked — an INFERRED
  consequence is not an IS finding until the running system confirms it —
  is recorded in the alignment plan §2.
- 2026-09-29 — v0.23.24 closed F5 and F6. The two proposed `.rules`
  passed the essentialist 3-gate test in reduced forms and were admitted
  by operator ruling: the behavior-restricting-fix boundary pin (test
  protocol traps) and the under-matching-sweep false-absence trap
  (agent loop traps). The repair-plan §8 walk-through executed: three
  items resolved by prior rulings/inspection, four ruled — #2 B (no
  destructive-deletion override), #3 B (attribution folds into P2), #7
  A-as-follow-up (project-root normalization scheduled; dual-root
  stands until then), #8 A **implemented**: the `Interrupted` report now
  names the effect consequence and the inspection step through one
  canonical helper at both construction sites; hkask-mcp lib 21/21,
  reconnect suite 17/17 green. **The repair plan's §8 decision queue is
  empty; every follow-up plan item (F1–F6) is closed.**
- 2026-09-29 — v0.23.23 executed the approved follow-up plan in order.
  F3 (`cfb89021f8`): six drifted pin citations refreshed. F2
  (`8f60b45efa`): the corpus credential declaration pinned; the
  legitimate boundaries verified; the D5 propagation arm recorded
  source-verified. F1 (`83c5f27b95`): **D7 landed** — `RunningUnknown`
  + the surfaced reason; the short-circuit boundary pinned; the
  failed-check path source-verified. F4 (`bbac2563f9`): the manifest
  surfaces the status history; the PK edge needs no code (sub-second
  timestamps). F5 packaged: two proposed `.rules` additions awaiting
  the operator's ruling. F6 pending on request. Full gate green at
  every landing.
- 2026-09-29 — v0.23.22 the four-lens post-landing review (refactor-
  architecture, essentialist, hypothesis-framer, grill-me) over the
  pass's 14 code commits. Findings fixed: the register's tool inventory
  was stale after `finish_research_run` (research 26→27, total 402→403,
  pin citation refreshed); the corpus helpers doc comment still
  described the pre-D8 `fs::write` pattern its body no longer uses;
  the primitive's doc now states the non-Unix arm's weaker guarantee
  explicitly; the dataset writer's identity `Err(e) => Err(e)` arm
  collapsed to `?`. New pin: the legitimate-symlink boundary test — an
  inside-root symlink resolves at check time and the write lands on its
  target (only the check-to-open race refuses), the strongest
  hypothesis-framer finding, previously reasoned but unpinned. Noted,
  not acted: the write path's double-containment (idempotent, cold
  paths, one extra canonicalize — a Speculative deepening candidate at
  best); `finish_research_run`'s history PK edge (two finishes in one
  second collide as an honest internal error); the kanban double-notify
  on fast completions (GPUI coalesces). Full gate green — the
  concurrent actor's agent-crate work landed, unblocking the repo gate.
- 2026-09-29 — v0.23.21 **D8 IMPLEMENTED** (`971185a553`): the shared
  `O_NOFOLLOW` write primitive closed the F4 check-to-open symlink race —
  seven call sites swapped across corpus and training; the pinned
  symlink-swap rejection test proves the outside target is never created.
  Media's priced site was wrong on inspection (no caller-supplied write
  paths) — recorded. **All twelve operator decisions now carry EXECUTED
  dispositions; the pass-2 decision queue is empty.** The full-repo gate
  was blocked at landing by a concurrent actor's uncommitted agent-crate
  work (documented, not this change's breakage); the three touched crates
  are scoped-clippy clean with 274 tests green.
- 2026-09-29 — v0.23.20 executed the operator's final rulings. **D6
  IMPLEMENTED** (`220ef6e394`): `finish_research_run` — terminal statuses,
  journaled transitions (`research_run_status_history`), the sources
  ledger untouched, completed/partial gated on server-recorded sources,
  pin 26→27, 143 tests green. **D2 executed honestly-narrowed:** 8 of the
  9 recorded skill gaps were false — recorded reference models exist in
  other forms the plural-heading grep missed (the probe-instrument-validity
  lesson applied to the audit's own ledger); the true gaps fixed: the
  lora-training `## Reference models` section and the kata-kanban
  per-server doc; SKILL.md pin suite green (28/28). **D8 priced:** one
  `O_NOFOLLOW`-class primitive + ~7-10 call-site swaps, ~+120-160
  production / ~+80 test, one session — ready to build on go. All twelve
  decisions now carry executed dispositions.
- 2026-09-29 — v0.23.19 recorded the operator's D1–D12 rulings and executed
  the decidable ones. **D1 RESOLVED — the split is the design:**
  encryption on core-crate activities; MCP-server data stays plaintext so
  servers run in an independent MCP-server pattern — rationale recorded at
  the three sites, F-P1 closed. **D5 DONE:** the recall failure-signal
  legs fixed (keyword leg warns with partial results standing; thread leg
  propagates — a failed read is an error, not "no memories"; kask_bridge
  253/253). **D11 B, wontfix-by-design** — the current passphrase
  bootstrap is the design; R1 closed. D3/D4/D9/D10 stand as deliberate
  contracts with the resilience rationale recorded. D7 ruled A,
  implementation deferred. D8 ruled A by the operator's stated principle
  (no races; shared canonical paths) — pricing next. D6 pending the
  operator's ruling after the delivered explanation. D2 (the reference
  records) is the remaining work item.
- 2026-09-29 — v0.23.18 packaged the operator decision queue as a durable
  register artifact: twelve evidence-backed option pairs (D1–D12) with
  consequences and recommendations, so each ruling is a one-word decision.
  The thread's executable work is complete; what remains is the operator's:
  the D1–D12 rulings and the goal's ground-truth scoring.
- 2026-09-29 — v0.23.17 corrected the Phase 4 lesson-recording note per
  operator: this audit ran in a curator thread — the turns are the
  episodic record, the chunking path owns them, and distillation
  extracts the lessons; no separate promotion step exists (the
  algedonic-review framing was wrong). Pass 2 otherwise stands complete
  as recorded at v0.23.16.
- 2026-09-28 — v0.23.16 closed Phase 4 (pass-2 closure): prediction
  scoring (the family reading is the honest one — the class was
  anticipated, the count was mildly overconfident, the net-lines
  prediction missed that the remaining defects were additive-by-nature);
  the AC6 no-candidate finding stated with evidence (+100 net production
  = the price of 14 truths previously silent or false; no consolidation
  candidate beyond L13/S6 survived the deletion test); the final gap
  ledger; three lessons written down (promotion to curator memory via
  algedonic review — the episodic-citation path is the L1 deferral, live);
  the 10 acceptance criteria checked. **Pass 2 is complete.**
- 2026-09-28 — v0.23.15 closed the loose-end disposition stage (Stage 2).
  Two items closed with evidence: #3 (L7 seam test — `9142f4f03e`) and
  #12 (L23 poisoned-lock degradation surfaced — `c98e29aa36`, +8/−2).
  Two re-deferred with stated reasons: #7 (L11 page-visibility — a
  panel-contract product question, folded into the operator batch) and
  #10 (L17 posterior carry-forward — the documented caller-controlled
  contract). Eleven items remain operator decisions, packaged in Stage 3
  with the server phase's additions (F-P1, the reference-model-record
  proposals). Zero silent carry-overs — every ledger row now carries a
  closure, a re-deferral reason, or an operator-decision status.
- 2026-09-28 — v0.23.14 closed the loop re-slice stage (Stage 1 of the
  completion plan). The four spec-named rows re-mapped in v0.23.13; the
  remaining 19 delta-verified in one pass — every citation drift
  attributable (the pass's own fixes: S6/S7/S8 line shifts; concurrent
  streams in agent.rs/thread.rs for L16), no mechanism changed, no new
  findings. All 23 register rows now describe the current tree. Next:
  Stage 2 (loose-end disposition).
- 2026-09-28 — v0.23.13 opened the loop re-slice stage (delta-first
  triage) and closed all four spec-named drift re-maps. **L7's measured
  seam test landed (`9142f4f03e`)**: finding 2 (optimistic-move repaint)
  was CONFIRMED as a behavioral bug — `dispatch_move`'s success path
  never notified, so the moved card and the pending banner stayed stale
  until the tool call resolved — diagnosed red-first with an
  observer-based test isolating the synchronous path, fixed with one
  notify (symmetric with the error paths), widget suite 62/62 green;
  finding 1's widget-half pinned healthy (post-completion `set_body`
  lands), residual conversation-side dependency recorded as a design
  note. **L1** folded in the delegation-authority arm (the `run_tool`
  hard-ceiling recheck `thread.rs:4408-4421`, `delegation_allows`
  `:5195-5207` with the P2 deferral documented at the site — no
  finding). **L3** citations re-mapped post-drift (`invoke :1531`,
  `call_tool_inner :1668`, `dispatch :1727`; E1 citations hold at
  `:1625`/`:1788`). **L5** re-mapped post-`df49e1497b` (embed arm
  `:791-796`; minimalism helpers verified in place) — no finding. Next:
  the delta rows (L2/L4/L6/L8-L23), then loose-end disposition.
- 2026-09-28 — v0.23.12 closed S12 (media server review, review-only) —
  **completing the per-server review phase: all 12 slices closed, every
  one of the 402 pinned tools reviewed**. 98/98 tools mapped; 361
  production fns inventoried, deep map deferred (432 tests green — the
  largest suite). Zero findings, zero `.rules` violations, zero
  production lines changed. Exemplars: the gallery DB's recorded
  unencrypted-by-reasoned-choice decision (the F-P1 contrast), the
  no-in-memory-fallback startup refusal, gallery mode enforcement, the
  REDUCT key pattern, fail-visible model resolution, the educt layer
  invariant. Anchor gaps recorded: `media-workflow` and `transcript-reel`
  skills lack `## Reference models` sections. Per-server phase totals:
  5 clean slices (S1, S2, S4, S9, S12), 7 with findings (12 fixes: S3×2,
  S5×2, S6×3, S7×2, S8×1, S10×1, S11×1); production line delta across
  the server fixes: +159/−73 (net +86 — honesty wording, warn arms, and
  one dedup; tracked to Phase 4 AC6 adjudication alongside L13's −40).
- 2026-09-28 — v0.23.11 closed S11 (swarm server review). 90/90 tools
  mapped across four files; 311 production fns inventoried, deep map
  deferred (210 tests green). One finding fixed in `85b17dcd72` (+1/−1,
  net 0; running total +87): F-S1 — the cloud module doc's stale tool
  count (27 vs 48 registered). Verified-sound: require_auth (the
  `.rules` canonical), the spend gate's hold/release settlement, the
  execute-route consent gate, workspace confirmation semantics, the
  KA-01 sanitization surface, the default-passphrase bootstrap warns,
  and the prior pass's `delegate_and_ingest` consolidation in place.
  Anchor gaps recorded: `swarm-intelligence` and `local-research-swarm`
  skills lack `## Reference models` sections.
- 2026-09-28 — v0.23.10 closed S10 (companies server review). 40/40 tools
  mapped; 351 production fns inventoried, deep map deferred (171 tests
  green). One finding fixed in `47f7bb2e5c` (+13/−1, net +12; running
  total +87): F-C1 — the primary provider's error was discarded at the
  fallback arm, so a double provider failure surfaced only the
  secondary's classification; the primary failure now warns before the
  fallback runs. Verified-sound: the H7 forecast-probability fix (with
  its warned 0.7 fallback and the persist-time consequence note), the
  screener's no-criteria remediation and FX honesty, the SERPAPI
  spelling fix, and the credential mix (2 required + 4 optional).
  F-P1 extent confirmed for companies (shared portfolio master.db +
  fibo_cache, both plaintext rusqlite). Anchor gaps recorded: `listening`
  skill lacks a `## Reference models` section.
- 2026-09-28 — v0.23.9 closed S9 (prediction-markets server review,
  review-only). 32/32 tools mapped; 163 production fns inventorized, deep
  map deferred (74 tests green). **Zero findings, zero `.rules`
  violations, zero production lines changed** — the cleanest large
  server in the audit. Exemplars recorded: §7 zero-scan attribution +
  per-provider series-scope surfacing; honest probability-at-observation
  snapshots (the pre-fix Brier≈0 design documented at the site); the
  subscribe tool's refusal to fabricate pre-resolution probabilities
  (reinforcing-loop trap); §8 orientation-separated curves; the FRED
  key pattern; the shared per-variant EconomicDataError classifier.
  Anchor gaps recorded: `cmp-term-structure` and `eqm` skills lack
  `## Reference models` sections. Post-rebuild verification of S8
  performed at slice open (fix in place at `:393`, register v0.23.8).
- 2026-09-28 — v0.23.8 closed S8 (kata-kanban server review). 27/27 tools
  mapped; 130 production fns inventoried, deep map deferred (105-test
  contract suite green — the README's named exemplar pattern). One finding
  fixed in `2787f8c71e` (+5/−1, net +4; running total +75): F-KK1
  (Guardrail) — the non-owner board-delete arm misclassified as
  `invalid_argument` where both sibling ownership gates and the service's
  own mapper use `permission_denied`; the reference server's one local
  deviation from its own discipline, now aligned. The idempotency
  machinery, goal-outbox retry semantics, closed-vocabulary discipline,
  and no-prediction Brier surfacing all verified-sound and recorded as
  exemplars. Anchor: richest in the audit (research doc + inline
  R-citations + testing-standard exemplar); gap recorded — no per-server
  doc under `kask/docs/reference/mcp-servers/` (proposal: separate
  operator decision).
- 2026-09-28 — v0.23.7 closed S7 (research server review). 19/19 tools
  mapped; 251 production fns inventoried, deep map deferred (141 tests
  green; full-repo clippy gate healthy again after the concurrent lisp
  work landed). Two findings fixed in `bfd1b57e1b` (+35/−6, net +29;
  running total +71): F-R1 — the DB-unavailability messages named
  `HKASK_RESEARCH_DB` as the fix while the DB opens at its default path
  without it (real cause: passphrase or open failure) — three sites
  corrected, pin survived; F-R2 — `web_search`'s `intent` accepted any
  string and silently degraded unknowns to generic ranking — closed
  vocabulary enforced, `research` (recognized but undocumented) now
  documented. The run-ledger non-repudiation path, cache-only-clean,
  and the rerank/duplication degradation contracts verified-sound and
  recorded as exemplars. Post-rebuild verification of S3/S6 changes
  performed at slice open (in place, compiles).
- 2026-09-28 — v0.23.6 closed S6 (corpus server review). 26/26 tools
  mapped; coverage: handlers + helper seam fully mapped, 325 production
  fns inventoried with deep map deferred (202 lib tests green). Three
  findings fixed in `5cc9ad34c9` (+35/−37, **net −2 — the first
  net-negative slice**; running total +42): F-K1 — `vec![]` credential
  declaration on a SQLCipher-backed server → `optional` with an honest
  degraded-mode description (behavior unchanged; adds startup
  observability); F-K2 — stale "default GLM-5.2" doc claim on
  `corpus_extract_assertions` → fail-visible truth; F-K3 — duplicated
  per-candidate grounding derivation in `corpus_ingest_qa` → one
  `grounding_fields` helper. `corpus_query`'s degradation surfacing and
  the path-containment single-enforcement points recorded as exemplars.
  Anchor gap recorded: `build-corpus-pipeline` skill has no `## Reference
  models` section. Validation caveat: `hkask-mcp-corpus` clippy/tests/fmt
  green scoped; the full-repo `./script/clippy` gate is currently blocked
  by a concurrent actor's staged, non-compiling
  `crates/agent/src/tools/lisp_eval_conformance.rs` (4 errors against
  their in-flight `hkask_lisp.rs` changes — the concurrent half-edit
  trap; not this work's breakage, left untouched). Same actor's landed
  `951ed5f2b8` carries one pre-existing fmt drift in
  `tools/tagging/tests.rs:318` (noted, not mine to fix).
- 2026-09-28 — v0.23.5 closed S5 (scenarios server review). 19/19 tools
  mapped; coverage 32/77 production fns fully mapped, 45 seam-verified
  with deep map deferred (27 tests green). Two findings fixed in
  `4aca2cbc56`: F-S1 (Guardrail) — `ForecastStore::load` silently
  swallowed corrupt/unreadable snapshot and unparseable journal lines, so
  a data failure was indistinguishable from no data and the calibration
  feedback loop quietly reset — every arm now warns with classification
  and session consequence; F-S2 (Guideline) — stale predecessor comment
  naming three nonexistent tools, updated to the actual None-arm set.
  Production +59/−20 (net +39); the pass-wide AC6 running total is now
  positive (+44: L13 +1, S3 +4, S5 +39), tracked to Phase 4 where AC6 is
  adjudicated across the change set. Verified-sound: all unwrap_or sites
  unreachable-defensive behind engine/store filters; single-slot tree
  cache documented design with explicit `tree_implied` escape; isotonic
  and hit-rate withholding exemplars. Plaintext JSON at rest (forecast
  journal/snapshot) recorded as F-P1 family extent — lower sensitivity
  (forecast history, not financial ledgers); folds into the F-P1
  operator decision.
- 2026-09-28 — v0.23.4 closed S4 (portfolio server review, review-only).
  18/18 tools mapped; coverage 27/98 production fns fully mapped, 71
  seam-verified with deep map deferred (store/types/analysis/returns —
  every public method exercised through a tool call site, 64 tests
  green). One operator-decision finding: F-P1 (Guardrail, IS) — the
  portfolio store is plaintext rusqlite SQLite at rest, and the
  plaintext family spans portfolio + companies (embeds the store) +
  research (own rusqlite), vs SQLCipher for corpus/curator/kata-kanban/
  training; no recorded decision exists. Proposed as an operator ruling
  (migration + passphrase dependency), NOT implemented. Zero `.rules`
  violations on the tool surface; date validation, missing-price
  degradation, what-if immutability, and the SF-4 pin all verified.
  Zero production lines changed (review-only, S1/S2 precedent).
- 2026-09-28 — v0.23.3 closed S3 (curator server review). 15/15 tools
  mapped; coverage 50/73 non-test fns (23 deferred: distillation 18 +
  forgetting 5, background loops with own rows and 43 in-file tests).
  Two findings, both fixed: F-C1 (Guardrail, IS) — `HKASK_DB_PASSPHRASE`
  declared `optional` with no in-memory fallback (`open_curator_stores`
  returns `CuratorStores::empty()`; 14/15 tools dead; two log messages
  claimed an in-memory mode that does not exist) → flipped to `required`
  per the kata-kanban reference, false wording replaced, comment-only pin
  module upgraded to a real source-pin test; F-C2 (Guideline, IS) —
  federated-search embedding-model error aligned with sibling env-var
  naming. The S13 per-server credential item is now resolved for curator
  (required) and training (optional, honest in-memory fallback); the
  remaining servers resolve in S4–S12. Validation: 84 tests green,
  `./script/clippy` green, rustfmt clean. Production +11/−7 (net +4 —
  message/comment honesty, no new path); tests +24/−13 (exempt).
  Landed in `2135f1591d` (concurrent mixed commit; content verified by
  hash — the commit message documents the curator change in its bullet
  list).
- 2026-09-28 — v0.23.2 continued pass 2 (post-rebuild verification + S1/S2).
  Verified the landed state on the rebuilt tree: `delegate_and_ingest`
  unchanged since `57c2bdea7a`, register at v0.23.1, working tree clean,
  swarm --lib re-run **210/210 green**. **S1 spreadsheet CLOSED clean**
  (2/2 tools, 6 fns, zero violations, credentials correctly empty —
  file-based server). **S2 training CLOSED clean** (9/9 tools mapped;
  consent gate, F3 ordering, G-P1 Nebius naming, per-variant mappers,
  the db_path bridge trap, containment, degraded-store permission_denied
  all verified; ~180 support fns seam-verified with deep-map deferred to
  continuation). Framework addendum: `run_server` is a one-line delegate
  of `run_stdio_server` — single-copy confirmed, L4 citation corrected.
  No production lines changed by this pass (review-only); zero new
  findings on either server — both clean against the recorded patterns,
  consistent with the pass-2 prediction of ~8–15 violations across all
  402 tools (2/56 tools reviewed so far, 0 violations — running under
  the predicted rate; prediction scored at Phase 4).

- 2026-09-28 — v0.23.1 pass-2 execution began (operator approved the
  Phase 0 checkpoint and directed work on the queued issues). **S13
  framework review completed:** all 46 fn items across the six
  hkask-mcp-server framework files mapped (transport, error, credentials,
  context, tool_span, validation) — zero `.rules` violations; the F4 TOCTOU
  window located at `canonicalize_lenient` (`validation.rs:207-235`) plus
  the caller's non-atomic write; a per-server review item extracted for
  S1–S12 (DB-path credential declared required vs optional-with-surfaced-
  degradation, checked against `open_database`'s in-memory fallback). **E1
  CLOSED resolved-by-drift** (see L3 row). **L13 CLOSED by refutation**
  (see L13 row): the red-green public-seam test ran — RED observed the
  exact falsifier (`count: 0, note: ""`), then the existing pin
  `thread_tests.rs:220-223` revealed the asymmetry is documented design;
  the seam consolidation was reverted, the surviving `delegate_and_ingest`
  consolidation landed in `57c2bdea7a` (+41/−40 production, net +1 of
  which +6 is the design-boundary comment at the seam; the refuted test
  was removed — the pin already covers it). Receipts: swarm --lib 210/210
  green, rustfmt clean on the touched files, scoped `./script/clippy`
  clean (machete + buf included). The terminal tool's git-status
  truncation (5 failures, skill-use issue filed) was resolved by
  `633e0c052a` (retry hard-refuse now limited to identical inputs).

- 2026-09-28 — v0.23.0 opened the pass-2 re-audit at Phase 0 and stopped
  at the operator checkpoint. Premise verified FALSE-as-stated: the
  2026-09-18 core+MCP review exists as
  `kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md` with pinned
  dispositions; its open items (F4 TOCTOU race, P2 invocation/identity
  contract, R3/H1 crash-durability, R1 passphrase onboarding, §8
  decisions) enter the loose-ends ledger — 17 items total, zero silent
  carry-overs. Register re-verified against the tree: entry points hold
  (L1/L2/L9/L5 spot-checked); L3/L5 drifted ~20–25 lines from
  `a7445fa213`/`ac58daba43`; the `embed_with_dimensions` arm mapped
  through types/inference/bridge/memory/corpus; the kanban-widget break
  is resolved (file clean under rust-analyzer). Reference-model anchor
  ledger added: 5 anchored, 7 partial, 11 gap rows; 2/273 templates and
  15/59 skills carry recorded sections. MCP tool inventory pinned: 402
  tools, 12 servers, 9 count-pins, 3 name-pins, 9/12 per-server docs.
  Pass-2 predictions and the INVEST decomposition (S13 + S1–S12 + 23
  loop re-slices + E1–E6) recorded above. Terminal tool failed on all
  git-status-shaped inputs (5 truncations; skill-use issue filed,
  provider_transport) — working-tree git state unverified at this
  checkpoint. Doc-only pass; no production lines changed. Operator
  approval pending before Phase 1.

- 2026-09-28 — v0.22.2 landed the L5 `inference_chat.rs` minimalism pass,
  closing the row's last deferral. Four behavior-preserving consolidations
  (`prompt_messages`, `dispatch_completion`, `complete_circuit`, `generate` →
  `generate_with_model`) net −21 production lines (+64/−85); the receiver-arm
  merge, the accumulator conversions, and the error-classification surface
  were examined and rejected with reasons (interface cost, distinct output
  types, incident-hardened pins). kask_bridge --lib 252/252 over the
  consolidated code; the row's Phase 1 graph citations re-verified and
  updated to the current tree. The loop-audit production ledger returns to
  −18 (−30 + 33 Api readback − 21 minimalism). `cargo check -p zed` was
  again blocked mid-pass, this time by the concurrent swarm_panel refactor
  (in-flight, uncommitted, that stream's surface); kask_bridge compiled and
  tested standalone.
- 2026-09-28 — v0.22.1 landed L5's deferred Api status readback. The IPC
  error payload carries a wire-optional `status: Option<u16>` (absent
  parses as `None`; `None` serializes without the field, preserving the
  old wire shape), the server classifies embedding failures through the
  extracted `embed_error_outcome` with the status traveling structurally,
  and the client reconstructs `EmbeddingGenerationError::Api(status, _)`
  so retry policy stays status-accurate (a 401 is not retryable). The
  named falsifier is pinned red-then-green; the copy-pinned classification
  test was replaced by a real pin over the extracted classifier. Receipts:
  hkask-inference --lib 55/55, hkask-types --lib 87/87, kask_bridge --lib
  252/252, rustfmt --check clean, scoped clippy clean; `cargo check -p
  zed` is blocked by an unrelated committed syntax error in
  `hkask-kanban-widget/src/view.rs:2011` (`f6806d5461`, the concurrent
  widget stream). Deltas: production +56/−23 (net +33, as this row
  predicted), tests +94/−21. The row's inference_chat.rs minimalism
  deferral stands with its window-passed note.
- 2026-09-27 — v0.19.1 confirmed L6's closure on the current tree and
  recorded an L7 deferral hazard. L6: the corpus surface
  (hkask-mcp-corpus, hkask-mcp-training, the build-corpus-pipeline skill)
  is unchanged since the audit commit `1113d8d85d` (git log verified — no
  commits, no dirty files), so the row's re-verified citations stand, and
  the 201/201 corpus library receipt was re-run green on the current tree
  (`92b9f541f8`). L6's title now carries its closed state, matching the
  ledger. L7: the measured panel seam test is additionally deferred behind
  the concurrent in-flight widget subtraction — the row's citations name
  lines being rewritten, so the scoped graph must be re-mapped against the
  landed widget state first. Doc-only pass; no production lines changed.
- 2026-09-27 — v0.22.1 blocker follow-up: the owning widget-rework stream
  repaired the kanban-widget compile break in its live worktree (braces
  balanced, `cargo check -p hkask-kanban-widget` green on the uncommitted
  state); this audit did not touch their in-flight files. The fresh
  full-tree gate receipt follows their landing. The audit itself is
  complete: all 23 rows closed or deferred-with-reason, calibration
  recorded, coverage walk passed, change set net −30 production lines.
- 2026-09-27 — v0.22.0 Phase 4 finalization: the aggregate prediction
  scoring computed across all 23 closed rows (lisp_eval: MAE 1.22
  defects/loop and 1.22 impedances/loop, both error sums 28/23; 34
  predicted defects vs 6 confirmed, 30 predicted impedances vs 2
  confirmed — the systematic-overestimate pattern quantified); the final
  register-to-tree coverage walk passed (19 core crates, 12 MCP
  servers, 7 widgets, 4 kask panels — inventory unchanged, no new
  crate since the register build, no new loop family); and the fresh
  full gates were run and fail on the current tree — hkask-kanban-widget
  does not compile (unexpected closing delimiter, view.rs:2011), a
  landed state owned by the concurrent widget-rework stream, flagged to
  the operator as a release blocker. The audit's own production changes
  remain gated green at their landings (receipts in the ledger). The
  audit's change set is complete: −30 Rust source lines, 212 test
  lines, all rows closed or deferred-with-reason.
- 2026-09-27 — v0.21.0 Batch D closed: L9–L13 and L15, L17–L23 all
  carried complete Phase 1 graphs and adjudicated findings from prior
  passes; this pass re-read every row in its current form (the L7
  lesson), spot-verified the load-bearing finding citations (L9
  acknowledge path goals.rs:317, L12 no UPDATE research_runs writer —
  repo-wide search re-run, L23 poisoned-lock leg providers.rs:323-333,
  L11 sort-then-limit tools/jobs.rs:356-359), finalized each
  prediction-vs-actual, and recorded the closures. No deletion
  candidate survives anywhere in Batch D; every open item is an
  operator decision or a deferred contract with a falsifier. All 23
  register rows are now closed or deferred-with-reason; the audit's
  remaining work is Phase 4 scoring and the operator decision queue.
  Doc-only pass; no production lines changed.
- 2026-09-27 — v0.20.1 L7 reconciliation: the v0.20.0 closure missed the
  row's prior Phase 1 scoped graph and Phase 2 bounded observations (a
  kanban-move trace carrying two INFERRED findings: a concurrent
  authoritative update possibly unseen after completion, and optimistic
  mutation possibly delaying visible repaint). The structural verdict
  stands — the seams are single-copy and no deletion candidate survives —
  but the row is NOT fully closed: both inferred findings stay open with
  their falsifiers, deferred behind the measured panel seam test, which
  the v0.19.1 hazard note further defers behind the in-flight widget
  subtraction; 'timely' remains UNMEASURED for the optimistic-mutation
  path rather than IS. Title, worklist row, and five-properties amended.
  Doc-only correction; no production lines changed.
- 2026-09-27 — v0.20.0 L7 closed: the update/render graph verified as
  already-small — one viz-core block renderer composes all seven widgets
  behind the unchanged D18 callback, widget actions flow through the two
  single-copy seams (shared_tool_invoker, compose_back_via_injector), and
  all four panels share the hkask-steer lifecycle with generated
  TOOL_NAMES advertisements. The duplication the Phase 0 prediction
  anticipated was already consolidated by the D2/D18/D21 refactors, each
  with pins; no deletion candidate survives. Subscription citations
  re-verified (drift ≤23 lines, one stale site noted). Four widget crates
  were under live concurrent edit during this pass; the slice is doc-only
  and touched none of them. Batch C is complete (L1, L6, L7).
- 2026-09-27 — v0.19.0 L2 verification closed the row: this audit
  independently re-ran the gate on the landed state — the named falsifier
  `accepted_impact_check_retries_a_failed_read_then_verifies_once` green,
  `accepted_impact_check_exhausts_bounded_read_retries` green (the
  mid-iteration red was test-environmental, per the landing stream's
  root cause), `retained_impact_checks_count_against_the_admission_bound`
  green, hkask-regulation --lib 96/96, kask_bridge rollout-filtered
  19/19 — and finalized the prediction count (1 reproduced defect,
  fixed; 0 impedances). Ledger updated: the landed slice is +57
  production / +105 test; audit total now −30 Rust source lines, 212
  test lines. Doc-only pass; no production lines changed by this
  verification.
- 2026-09-27 — v0.16.1 landed the operator-permitted L2 corrective slice with
  the exhaustion-escalation extension: accepted checks survive store-read
  errors in the same 64-slot queue for up to 3 read attempts, exhausted
  checks escalate to the review board without a verdict (the absent-verdict
  restart rescan stays the recovery backstop), and the unified
  `RolloutMetricObservation` carries values and sample sizes from the same
  events. The L2 row is retitled and re-cited to the landed graph; the
  mid-iteration red test the v0.16.0 pass observed is green (test-
  environmental root cause, fixed in the test). The swarm stale-comment fix
  rides in the same commit. Receipts: hkask-regulation --lib 96/96,
  kask_bridge --lib 251/251, rustfmt --check clean, `./script/clippy` clean,
  `cargo check -p zed` passed. This register update landed with the
  concurrent L1-row pass in `6d1e441a43`; the code and the swarm comment
  fix land in the slice's own pathspec-limited commit.
- 2026-09-27 — v0.18.0 L6 closed: all pipeline nodes re-verified in the
  current tree (convert, chunk, tag, embed, calibration, prompts, QA
  generation, grounding, ingestion, plus the corpus_query retrieval
  feedback node at storage.rs:81); the ingest gate's re-execution of the
  grounding checks examined and REJECTED as a consolidation target — its
  independence from the artifact is the pinned fails-closed invariant.
  201/201 corpus library tests green on the current tree. The
  source-complete run stays outside the audit's no-dataset-construction
  rule; the boundary is stated in the row. Doc-only slice; no production
  lines changed.
- 2026-09-27 — v0.17.0 L1 closed at full scope: the three submission
  routes are thin closures over one run_turn entry, the loop tail is a
  single path, and the ideal-method verdict is that the graph is already
  the small graph (the two-prompt-route helper candidate, ~6-8 lines,
  fails the admission test). Citations re-verified against the current
  tree (run_turn thread.rs:2984, request assembly :3284-3335, loop tail
  :3541-3597, submission routes agent.rs:3615/:3754/:3900). The
  EndTurn-before-detached-ingest deferral stands with its falsifier,
  awaiting an operator-specified functional guarantee. Doc-only slice;
  no production lines changed. L2 remains with its live authoring
  stream (not landed at this pass).
- 2026-09-27 — v0.16.0 operator approved the register and ruled on L2 (the
  bounded corrective slice was permitted). The L2 implementation validated
  green on a worktree snapshot (96/96 regulation lib, 19/19 bridge rollout,
  scoped clippy, cargo check -p zed) but was NOT landed: the authoring
  stream is live on the regulation files, mid-iteration on an
  exhaustion-escalation extension (one red test observed); this audit
  verifies L2 after that stream lands, and the swarm stale-comment fix
  waits for that landing. L16 closed: the graph is complete with verified
  citations, and the inferred cross-turn misattribution is refuted as
  DEFECT — sticky attribution is D59's documented, pinned design with the
  unclassified-evidence mitigation; windowed attribution would be a
  behavior change for the operator, not a consolidation. No production
  code changed by this pass.
- 2026-09-27 — v0.15.1 Phase 0 checkpoint re-verification (new session):
  all 23 rows' primary entry points re-located in the current tree by symbol
  grep; two citation drifts corrected (L1 `run_turn_internal`
  thread.rs:3178→:3176, L10 `curator_memory_extract`
  hkask_mcp_curator.rs:1739→:1833), every other citation verified within
  ±2 lines. `hkask-services-core` and `hkask-steer-core` confirmed
  participant libraries (no spawn/interval/loop machinery) — no loop family
  is missing. Register recovered from an accidental stale-buffer overwrite:
  the working-tree copy was byte-identical to the v0.9.0 blob (in history at
  `31bcd62267`) and had clobbered the committed v0.15.0 records; restored
  from HEAD, stale copy preserved at /tmp/loop-register.stale-wt.md,
  nothing lost. Concurrent uncommitted L2 retry work observed in the
  regulation sources (`prepare_impact_checks` refactor) — left untouched;
  that row's committed state stands until the slice lands. No row content
  changed beyond the two citation corrections; no production code touched.
  This register update is uncommitted pending operator approval of the
  checkpoint.
- 2026-09-27 — v0.15.0 committed L6/L9 audit files in pathspec-limited
  `1113d8d85d`. A temporary L2 public-seam test reproduced accepted-check
  loss after a store-read error (queue 0 rather than 1 on the next tick);
  the diagnostic test/import were removed after that red result. Automatic
  bounded retry was selected by the operator but blocked at the deletion
  gate: safe capacity reservation for in-flight accepted checks requires
  additional state, so no L2 production edit was admitted. This version's
  register update is uncommitted pending a ruling on that exception.
- 2026-09-27 — v0.14.0 operator-approved longer `kanban_panel` run observed
  the L9 Steer prompt pin fail red and pass green; 32/32 panel tests and the
  affected memory test passed. Replaced five stale goal-guidance lines with
  four, removed one stale test-comment line, updated D2 in the same pass and
  swept exact obsolete phrases. Full `./script/clippy` and `cargo check -p zed`
  passed. The code, D-seam entry and v0.14 register update landed together
  in pathspec-limited commit `1113d8d85d`.
- 2026-09-27 — v0.13.0 attempted the existing Kanban Steer test seam
  for L9's stale ephemeral-goal prompt; `kanban_panel` cold compilation
  timed out after 180 seconds before the test ran. Removed only the new
  unrun test, made no zed-side or D-seam edit, and left the L9 finding
  deferred with the observed blocker. No longer runtime limit was silently
  selected; other audit rows remain at their recorded states.
- 2026-09-27 — v0.12.0 offline mixed-QA-to-candidate projection exercised
  through the real corpus grounding and dry-run ingestion tools (201 library
  tests green, no training dataset constructed). L6 remains open for a
  source-complete run outside this audit's no-dataset-construction scope;
  the 71 test lines later landed with L9 in `1113d8d85d`.
- 2026-09-27 — v0.11.0 bounded maps and classified impedances added for
  L11–L13 and L15–L23; L15 offline rotation tests 6/6 green after a
  concurrent dependency build failure was repaired. Removed 14 stale L3
  source-comment lines in `b28e893fde` and recorded the partial Phase 4
  accounting above. This register edit is not a final audit verdict; work
  remains open at the named gates. No training or paid pipeline ran.
- 2026-09-27 — v0.10.0 bounded Phase 1–2 maps added for L1, L6, L7,
  L9 and L10. The Stage 7→9 corpus QA seam was source-confirmed: skips in
  generator output fail grounding. The existing `build-corpus-pipeline` skill
  now projects candidate-only rows, retains and reconciles the mixed original;
  a synthetic jq check confirmed skip-only filtering but **no live corpus run**
  verified the repaired capability. No new production source lines for that
  skill change; L1/L7/L9/L10 findings remain open or deferred with falsifiers.
  These edits were swept into `31bcd62267` (skill and register) and
  `e1f1b51cad` (L5 code) alongside unrelated work; no global completion claimed.
- 2026-09-27 — v0.9.0 L3 current-tree library/integration tests and full gates passed; the earlier fixture-suite failure was an invalid parallel invocation. L5's JSON-error classification failed at the existing IPC seam, then passed after a −1-production-line change; 25 test lines added and 54/54 library tests passed. The Api error-shape impedance remains deferred. The follow-up was subsequently carried by `e1f1b51cad` (code) and `31bcd62267` (register), not by an audit-specific commit.
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
