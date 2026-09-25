---
title: "Regulation Span Registry — Reference"
audience: [developers, operators, agents]
last_updated: 2026-09-17
version: "0.40.0"
status: "Active"
domain: "Core"
mds_categories: [domain, curation]
---

# Regulation Span Registry

## 1. Purpose

Regulation uses two related observability forms:

1. tracing events, emitted with a `reg.*` target for process-local diagnostics; and
2. persisted `RegulationRecord` values, carrying a validated `Span`, actor, cybernetic phase, observation, and optional outcome metadata (`kask/crates/hkask-types/src/event.rs:375-438`, `kask/crates/hkask-types/src/event.rs:536-553`).

A tracing target is not automatically a persisted Regulation record. The tool paths below are intentionally separated so documentation does not turn a log label into a fictional persisted operation.[^otel-spans][^beer-cybernetics]

## 2. Canonical namespace and typed-span surface

`CANONICAL_NAMESPACES` is the source of truth for accepted `reg.*` roots and sub-namespaces (`kask/crates/hkask-types/src/event.rs:75-154`). `SpanNamespace::new` validates a full namespace; `SpanNamespace::parse` accepts short or full forms, and hierarchical validation allows descendants of a registered root (`kask/crates/hkask-types/src/event.rs:156-271`).

`SpanKind` currently has 12 variants. `Span::from_kind` converts each variant to the canonical namespace/path pair in `namespace_and_path` (`kask/crates/hkask-types/src/event.rs:446-499`):

| Group | Typed variants |
| --- | --- |
| Tool dispatch | `ToolCompleted` |
| Curation and variety | `CurationDirectiveAcknowledged`, `VarietyAlgedonicAlert` |
| Outcome assessment | `ImpactVerified`, `AdviceReviewObserved`, `ActionSubstituted`, `ActionBlocked`, `RegulatoryPlateauDetected`, `LoopMetricsTelemetry`, `ToolOutcomeBreakdown` |
| Inference resilience | `InferenceCircuitTransition`, `InferenceObservedRecovery` |

`CyclePhase` is `Sense | Compute | Compare | Act`; there is no `Verify` phase (`kask/crates/hkask-types/src/event.rs`). `AdviceReviewObserved` uses the Sense phase because it is an observational receipt, not a causal impact verdict.

`LoopMetricsTelemetry` exposes rollout and advice review as separate channels. `rollout_progress_score` is evidence-bearing and nullable; `advice_review_progress_score` is observational and nullable. The record also carries `advisories_computed`, `interventions_confirmed`, `rollout_impact_reports`, finalized-review outcome counts, and the advice review's unverified causal-attribution state. An unchanged persistent condition is summarized with `steady_state_heartbeat` and `suppressed_steady_state_cycles`; clearing is marked with `condition_cleared`. Changes in the separately observed `interventions_confirmed` count or its availability bypass suppression. When the escalation queue is unavailable, successful archive fallback also latches the condition until it clears; failed persistence retries and recurrence emits again. Idle heartbeat, archive retention, and the ledger alert-log cap remain separate mechanisms.

## 3. Actual MCP tool outcome paths

### 3.1 Child-server tracing: `reg.tool` with an outcome field

Every server using the framework-level `execute_tool` wrapper creates a `ToolSpanGuard`, awaits the business future, and finishes the guard (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:145-169`). The guard emits one tracing event at target `reg.tool` with these fields:

- `tool`
- `outcome`: `ok`, `error`, or `dropped`
- `duration_ms`
- `error_kind`
- `caller`

The exact emission is `tracing::info!(target: "reg.tool", ..., "REG")` (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:92-118`). It does not call an ontology-tagging wrapper and does not emit a separate invocation subpath.

This child-process trace goes to stderr. It is an observability signal, not the Regulation loop's durable outcome input; the framework documents that separation at `kask/crates/hkask-mcp-server/src/server/tool_span.rs:123-139`.

### 3.2 Governed runtime dispatch: `reg.mcp` path with `ToolCompleted`

For tools invoked through `McpRuntime`, governance performs the per-tick call-meter check, dispatches the tool, then constructs a `RegulationRecord` using `SpanKind::ToolCompleted` and persists it through the configured event sink (`kask/crates/hkask-mcp/src/runtime.rs:1475-1543`). The observation records `server`, `tool`, `calls`, and success/failure status. Persistence failure is surfaced with a warning at target `reg.mcp`.

The same dispatch then records the result in `CyberneticsLoop::record_outcome` and the tool name in the variety feed (`kask/crates/hkask-mcp/src/runtime.rs:1545-1573`). Reliability is aggregated by MCP server name; tool names provide the observed variety states.

### 3.3 Agent context-server dispatch: outcome forwarding

Agent-initiated context-server tools do not pass through `McpRuntime::invoke`. `ContextServerTool::run` therefore wraps `run_inner`, classifies the result, and calls `agent::record_mcp_tool_outcome(server, tool, success, error_kind)` (`crates/agent/src/tools/context_server_registry.rs:775-815`).

The composition root installs the recorder. Its closure logs a trace at `reg.tool.agent`, then asynchronously forwards the outcome and tool-name variety state into the shared Regulation ledger (`crates/zed/src/main.rs:908-953`). This makes agent-path and governed-runtime calls converge on the same per-server reliability domain without pretending that the child stderr trace is the durable input.

## 4. Other live span families

The canonical registry currently includes these live roots and selected descendants (`kask/crates/hkask-types/src/event.rs:75-154`):

| Family | Selected current namespaces |
| --- | --- |
| MCP | `reg.mcp`, `reg.mcp.cap`, `reg.mcp.media.face` |
| Tool tracing | `reg.tool`; hierarchical descendants such as `reg.tool.agent` are valid |
| Inference | `reg.inference` and the typed circuit-transition/recovery paths |
| Outcome | `reg.outcome`, `reg.outcome.predictive` |
| Memory | `reg.memory`, `reg.memory.decay`, `reg.memory.encode` |
| Skills | `reg.skill` plus hierarchical per-skill outcome and operator-feedback descendants |
| Pipelines | `reg.pipeline` and registered triage, PDF, chunk, and OCR descendants |

`RegulationSpan` remains the small cross-cutting enum for `Curation` and `MemoryEncode`; its `emit` method writes target `reg` with `reg_domain` and `operation` fields (`kask/crates/hkask-types/src/regulation.rs:108-144`). Domain-specific emitters use their own registered namespace strings.

## 5. Query and feedback loop

The live query surface is programmatic. `RegulationLedger` exposes health, alerts, variety, per-domain calibration, outcomes, and skill feedback through `kask/crates/hkask-regulation/src/runtime.rs`. The cycle consumes per-domain outcome rates and emits a tool-domain breakdown via `SpanKind::ToolOutcomeBreakdown`. `curator_algedonic_log` uses the archive's newest-first operational query and declares `ordering: "newest_first"`; chronological replay keeps the separate oldest-first query.

The default variety window and expected-variety controls are defined in the Regulation runtime and algedonic manager (`kask/crates/hkask-regulation/src/runtime.rs`, `kask/crates/hkask-regulation/src/algedonic.rs`). Settings-dependent thresholds are wired at the composition root rather than inferred from tracing output.

## 6. Adding a span

1. Add or confirm the namespace root in `CANONICAL_NAMESPACES` (`kask/crates/hkask-types/src/event.rs:75-154`).
2. Add a `SpanKind` only when a typed constructor is shared and useful; map it in `namespace_and_path` (`kask/crates/hkask-types/src/event.rs:446-499`).
3. Add a validation test proving the constructed namespace is canonical.
4. Choose the real path: tracing for diagnostics, `RegulationRecord` persistence for durable cybernetic evidence, or both.
5. If the observation feeds reliability or variety, wire it to `record_outcome` or `increment_variety`; a trace line alone does not close the loop.
6. Mirror canonical-registry changes in `kask/scripts/check-reg-canonical.sh`.

## Footnotes

[^otel-spans]: OpenTelemetry. (2024). *OpenTelemetry Specification*. Cloud Native Computing Foundation. https://opentelemetry.io/docs/specs/otel/

[^beer-cybernetics]: Beer, S. (1979). *The Heart of Enterprise*. John Wiley & Sons.
