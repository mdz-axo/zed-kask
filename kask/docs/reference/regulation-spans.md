---
title: "Regulation Span Registry — Reference"
audience: [developers, operators, agents]
last_updated: 2026-08-28
version: "0.39.0"
status: "Active"
domain: "Core"
mds_categories: [domain, curation]
---

## 1. Purpose

Regulation spans are the observability substrate of hKask's Cybernetic Nervous
System (Loop 6). Every operation that affects system state — tool invocations,
loop outcomes, skill feedback — emits a **span** through the
Regulation tracing infrastructure.[^otel-spans][^beer-cybernetics]

> **Hosting note (updated 2026-08-28):** hKask runs in-process inside
> zed-kask. The standalone `kask` CLI, `hkask-api` HTTP server, and the kask
> panel (D10) are deleted; span surfaces are programmatic only. Skill
> execution is upstream-Zed body injection: `SkillTool::run`
> (`crates/agent/src/tools/skill_tool.rs`) reads the `SKILL.md` body via
> `agent_skills::read_skill_body` and injects it via `render_skill_envelope`
> (DIVERGENCE.md D1, L22).

A **span** is a typed identifier that pins an observation to a canonical
dot-separated namespace (e.g. `reg.tool.web_search`). Spans carry an
**operation** verb (e.g. `invoked`, `completed`) and optional structured
fields. They flow through two paths:

1. **tracing infrastructure** — `tracing::info!(target: "reg", reg_domain = …, operation = …, "REG")`
   — for structured logging (e.g. `RegulationSpan::emit()`,
   `kask/crates/hkask-types/src/regulation.rs:128-144`).
2. **ν-event persistence** — `RegulationRecord::new()` →
   `RegulationSink::persist()` — for the cybernetic audit trail
   (`kask/crates/hkask-types/src/event.rs:745-762`).

### Span vs. RegulationRecord

| Concept | Role | Contains |
|---|---|---|
| **Span** (struct) | Pair of `SpanNamespace` + path (e.g. `reg.tool` + `invoked` → `reg.tool.invoked`) | Namespace + full path (`event.rs:623-641`) |
| **SpanNamespace** (newtype) | Validated string wrapper; construction validates against `CANONICAL_NAMESPACES` (hierarchically) | Dot-separated namespace string (`event.rs:447-505`) |
| **SpanKind** (enum) | Typed constructors for 8 common span paths (eliminates string typos) | Canonical (namespace, path) pairs (`event.rs:670-710`) |
| **RegulationSpan** (enum) | The 2 core cross-cutting span identifiers | `Curation`, `MemoryEncode` (`kask/crates/hkask-types/src/regulation.rs:108-113`) |
| **RegulationRecord** (struct) | Full cybernetic observation: who observed, what span, what phase, what was observed | Span, `WebID`, `CyclePhase`, `observation` (JSON), `regulation`, `outcome`, recursion depth (`event.rs:15-27`) |

Spans describe *what* happened; RegulationRecords describe *who observed it,
when, in what context, and what they saw*.

> **Deleted surface:** the `ObservableSpan` trait (with `as_str()`/`emit()`/
> `to_event()`/`emit_to()` and `SpanNamespace::from_observable()`) no longer
> exists — only doc-comment references remain in `event.rs:73,596`. The
> `ToolSubsystem` enum and the `RegulationSpan::Tool`/`Inference`/`AgentPod`/
> `SelfHeal` variants are likewise gone. Do not cite them as current.

### Span validation

All namespace strings are registered in `CANONICAL_NAMESPACES`
(`kask/crates/hkask-types/src/event.rs:75-431`) — a private const array that
is the single source of truth. `SpanNamespace::new()` returns `None` on
unknown namespaces; `SpanNamespace::parse()` accepts short (`"tool"`) and full
(`"reg.tool"`) forms (`event.rs:455-478`). Validation is **hierarchical**:
`is_canonical()` (`event.rs:434-445`) accepts a sub-namespace like
`reg.pipeline.decimation.binarize` if any prefix segment is registered. The
logic is mirrored in `scripts/check-reg-canonical.sh` (per the comment at
`event.rs:435`) — update both together.

The `reg.*` prefix is reserved for canonical spans. Performative telemetry
uses `hkask.*` tracing targets (e.g. `hkask.training.job.submit`); those are
observability logs, not loop variables, and `SpanNamespace::new` rejects them
(`kask/crates/hkask-types/src/regulation.rs:8-15`).

---

## 2. Span Namespace Taxonomy

Namespaces form a tree rooted at `reg`. The namespace prefix maps to a
`SpanCategory` for typed dispatch (`SpanCategory::from_short_name()`,
`event.rs:529-560`):

| Category | Prefixes (`short_name`) | Examples |
|---|---|---|
| **Cybernetics** | `variety`, `outcome`, `alert` | `reg.variety`, `reg.outcome.impact_verified` |
| **Curation** | `curation`, `spec` | `reg.curation.directive_acknowledged` |
| **Inference** | `inference` | `reg.inference` |
| **Memory** | `pod`, `connector` | `reg.pod` |
| **Skill** | `skill` | `reg.skill.convergence.converged` |
| **Unknown** | everything else | `reg.tool.web_search`, `reg.consent`, `reg.api.request` |

`Span::from_kind()` constructs spans from `SpanKind` variants without string
literals (`event.rs:649-661`).

---

## 3. Typed Span Enums

### 3.1 SpanKind — canonical (namespace, path) pairs

**File:** `kask/crates/hkask-types/src/event.rs:670-710`

Eight variants. Note the actual namespace mapping in `namespace_and_path()`
(`event.rs:696-710`) — the v0.31.0 impact-gate variants map to `reg.outcome.*`,
not the `reg.regulation.*` strings their doc comments claim:

| Variant | Actual namespace path | Meaning |
|---|---|---|
| `ToolCompleted` | `reg.tool.completed` | Tool invocation completed |
| `CurationDirectiveAcknowledged` | `reg.curation.directive_acknowledged` | Curation directive acknowledged |
| `VarietyAlgedonicAlert` | `reg.variety.algedonic_alert` | Algedonic alert emitted |
| `ImpactVerified` | `reg.outcome.impact_verified` | Fermi impact-gate verification completed |
| `ActionSubstituted` | `reg.outcome.action_substituted` | Action substituted after repeated ineffectiveness |
| `ActionBlocked` | `reg.outcome.action_blocked` | Action blocked (severe counterproductivity) |
| `RegulatoryPlateauDetected` | `reg.outcome.plateau_detected` | Regulatory plateau — escalation triggered |
| `LoopMetricsTelemetry` | `reg.outcome.loop_quality` | Loop-quality telemetry recorded; idle cycles emit one `heartbeat: true` span per hour (tick 1, then every 360 ticks) so a converged loop is distinguishable from a dead ticker |

### 3.2 RegulationSpan — core cross-cutting spans

**File:** `kask/crates/hkask-types/src/regulation.rs:108-179`

Only spans constructed in 2+ crates from different dependency domains live
here. Domain-specific spans moved to their domain crates as plain namespace
strings registered in `CANONICAL_NAMESPACES` (`regulation.rs:96-106`).

| Variant | Namespace | Emitted When |
|---|---|---|
| `Curation` | `reg.curation` | Curation loop operations (registry sync, directive issuance) |
| `MemoryEncode` | `reg.memory.encode` | Memory encoding operations |

`RegulationSpan::emit(operation)` writes
`tracing::info!(target: "reg", reg_domain = …, operation = …, "REG")`
(`regulation.rs:128-144`); `as_str()` output must match
`CANONICAL_NAMESPACES` byte-for-byte (P8 — Semantic Grounding).

### 3.3 CyclePhase

**File:** `kask/crates/hkask-types/src/event.rs:715-720`

`CyclePhase` has four variants: `Sense | Compute | Compare | Act`
(`event.rs:715-719`). The `Verify` variant has been removed — do not cite it.
`from_str` falls back to `Sense` for unknown strings (`event.rs:733-741`).

### 3.4 Deleted typed enums (retained namespaces)

The following typed span enums are **deleted**; their namespace strings remain
in `CANONICAL_NAMESPACES` for tracing-target stability and historical-record
queryability, but no typed emitter exists:

| Deleted enum | Retained namespaces |
|---|---|
| `AcpSpan` | `reg.acp.ide.connection_state`, `reg.acp.agent.memory_size` |
| `ClassifySpan` | `reg.classify.dual_fidelity`, `reg.classify.drift` |
| `ContractSpan` | `reg.contract.proposed/accepted/rejected/violated/coverage/quality.violated` |
| `SloSpan` | `reg.slo.evaluated` |
| `ApiRequestSpan` | `reg.api.request` (the `hkask-api` HTTP server is deleted; nothing meters it) |
| `InfraSpan`, `QaSpan` | `reg.ci.invariant.violation`, `reg.curator.consolidation`, `reg.chat`, `reg.qa.repair_attempted/verified/exhausted`, `reg.qa.run.*` — emitted, if at all, as raw tracing events |

The `SeamSpan` row (`reg.architecture.seam.coverage`/`.drift`) was removed
2026-08-30 along with the namespaces — the "seam watcher" that would have
emitted them was never built.

### 3.5 Skill spans

All namespaced under `reg.skill.*`, registered in `CANONICAL_NAMESPACES`
(`event.rs:75-431`). The hierarchical `is_canonical()` makes
`reg.skill.<any-id>.*` valid without per-skill registration. Two live
mechanisms:

- **Per-skill feedback store** — the regulation runtime stores
  `reg.skill.<skill_id>.<phase>` span payloads so skills can read their own
  prior feedback (`kask/crates/hkask-regulation/src/runtime.rs:765-786`), and
  the metacognition loop consumes `reg.skill.<id>.outcome` spans
  (`kask/crates/hkask-regulation/src/metacognition.rs:372`).
- **`reg.skill.lifecycle/registry/frontmatter/routing/discovery`** — canonical
  strings for the skill execution layer.

The `reg.skill.cascade.*` and `reg.skill.convergence.*` sub-namespaces are
**retained-but-unemitted**: their emitters were the deleted manifest-executor
machinery (`StepMachine`/`ConvergenceTracker`), and `skill_tool.rs` (836
lines) contains no `reg.skill` tracing emission — skill execution is
upstream-Zed body injection (D1).

### 3.6 Wallet spans (removed)

The wallet system is fully deleted: the `hkask-wallet` crate, the crypto
wallet ledger, `WalletManager`/`Well`, `hkask-types::wallet_types`, the
`SpanCategory::Wallet` variant, the `reg.wallet.*`/`reg.well.*`/
`reg.tool.wallet` namespaces, and (2026-08-30) the last policy-side
residuals — the `WalletBalanceRatio`/`WalletKeyHealth` metrics, their
`RegulationReason`s, rules, and cycle handlers, which had no sensor and
could never fire. The ABW cloud wallet balance
(`hkask-mcp-swarm/src/abw_client.rs`) is a separate live system, never
wired to these spans.

### 3.7 Additional canonical namespace groups

Every string below appears verbatim in `event.rs:75-431`. Selection (not
exhaustive — the array is the authority):

| Group | Namespaces (selected) |
|---|---|
| **Tool subsystems** | `reg.tool`, `reg.tool.{communication, companies, corpus, curator, filesystem, kanban, media, memory, registry, research, training, web_search}` — note: no `reg.tool.condenser` is registered |
| **Outcome** | `reg.outcome` (registered twice — `event.rs:192,215`, a benign duplicate), `.calibration`, `.coherence`, `.predictive` |
| **Memory** | `reg.memory`, `.budget`, `.decay`, `.encode`, `.health` — no `.episodic` is registered |
| **MCP** | `reg.mcp`, `.cap`, `.health`, `.media.face` |
| **Pipeline** | `reg.pipeline`, `.calibration`, `.decimation`, `.decimation.binarize`, `.triage`, `.pdf_extract`, `.ocr` + 5 `.ocr.*` failure modes |
| **Platform metrics** | `reg.platform.metric` + DORA (4) + SPACE (5) + `.loyalty` |
| **Skill phases** | `reg.lora.*` (6), `reg.bughunt.*` (6), `reg.codereview.*` (5), `reg.supply_chain.*` (4), `reg.taxonomy.*` (4), `reg.runtime.*` (4), `reg.eqm*` (7) |
| **Training providers** | `reg.training.provider` + 7 `.runpod.*` HTTP observability spans; `reg.training.checkpoint.resume` |
| **Sovereignty** | `reg.sovereignty` + 4 sub-namespaces |
| **Heal** | `reg.heal` + 9 sub-namespaces |

No `reg.guard*` or `reg.meta*` strings are registered — those went with the
guard crate and the curator self-observation cleanup; historic records
carrying them remain in the audit trail only.

---

## 4. Span Lifecycle

### 4.1 Emission

Two mechanisms:

1. **Tracing path** — `RegulationSpan::emit(operation)` and raw
   `tracing::info!(target: "reg…", …, "REG")` calls. The MCP framework's
   `ToolSpanGuard` (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:15`)
   is an RAII guard that emits a `reg.tool` span on drop via
   `emit_tool_span` (`tool_span.rs:149-152`), with `ok()`/`error()` helpers.
   `execute_tool` (`tool_span.rs:186`) wraps every `#[tool]` method;
   `execute_tool_semantic` (`tool_span.rs:214`) additionally tags the span
   with a domain ontology concept and `tracing::warn!`s when a registered
   tool lacks an anchor (`tool_span.rs:228-236`).

   > The `reg.tool` span is an **observability signal only** — it is written
   > to the server process's stderr and is NOT consumed by the Regulation
   > loop. Production outcome recording happens client-side:
   > `CyberneticsLoop::record_outcome` via `McpRuntime::invoke`, and
   > `agent::record_mcp_tool_outcome` via `ContextServerTool::run`
   > (`tool_span.rs:160-172`).

2. **ν-event path** — constructs `RegulationRecord` with a `Span`
   (namespace + path), `CyclePhase`, observation JSON, and optional
   regulation/outcome metadata; persists via `RegulationSink::persist()`
   (`event.rs:745-762`, including the deduplicating `persist_if_absent`).

### 4.2 Query (programmatic only)

The `kask regulation` CLI and the kask panel (D10) are deleted. The
equivalent surfaces on `RegulationLedger`
(`kask/crates/hkask-regulation/src/runtime.rs`):

- `health()` → `LedgerHealth` (`runtime.rs:536`) — aggregate deficit, alert
  counts, variety EMA, alert-log cap status (`kask/crates/hkask-types/src/regulation.rs:44-66`).
- `alerts()` → `Vec<RuntimeAlert>` (`runtime.rs:594`).
- `variety()` → `HashMap<String, u64>` of domain → distinct-state count
  (domains are MCP server names — the tool-dispatch feed taxonomy);
  `variety_for_domain` (`runtime.rs`).
- `increment_variety(domain, state_name)` (`runtime.rs`) — feeds the
  algedonic check. Wired from both tool-dispatch paths:
  `CyberneticsLoop::record_variety` (called by `McpRuntime::invoke`) and
  the agent-path outcome hook (`main.rs`), with the tool name as the
  observed state.

### 4.3 Variety window and algedonic alerting

- Variety window: 60 seconds (`DEFAULT_VARIETY_WINDOW_SECS`,
  `runtime.rs:131`).
- Default variety max deficit: 100.0 (`DEFAULT_VARIETY_MAX_DEFICIT`,
  `kask/crates/hkask-regulation/src/set_points.rs:18`). The effective
  signal set-point scales with the `kask.curator.algedonic_threshold`
  setting (default 0.8 → effective 20.0; see the D8-F4 wiring in
  `crates/zed/src/main.rs`).
- Default expected variety per domain: 3 distinct tools per window
  (`DEFAULT_EXPECTED_VARIETY`, `algedonic.rs`); per-domain override via
  `RegulationLedger::calibrate_threshold`.
- Per-domain expected variety: `AlgedonicManager::set_expected_variety()`
  (`kask/crates/hkask-regulation/src/algedonic.rs:278`).
- The in-memory algedonic log is a capped ring buffer (default 200);
  `LedgerHealth.alert_log_approaching_cap` fires at ≥ 80% so the operator
  (or the `algedonic-review` skill) can review before eviction
  (`regulation.rs:53-65`).

> **Deleted surface:** `DecayConfig` (per-category exponential decay
> constants) no longer exists — only stale doc-comment references remain at
> `event.rs:508,521`. Do not cite per-category half-lives as current.

### 4.4 Adding a new span

1. Add the namespace string to `CANONICAL_NAMESPACES` in
   `kask/crates/hkask-types/src/event.rs` (the array at L75-431 is the
   single source of truth; hierarchical descendants are auto-valid).
2. If the span is constructed in 2+ crates from different dependency
   domains, add a `RegulationSpan` variant in `regulation.rs` with
   `as_str()` matching the registry byte-for-byte.
3. Add a test verifying `SpanNamespace::new(span.as_str())` succeeds.
4. Emit via `tracing::info!(target: "reg…", …, "REG")` and/or construct a
   `RegulationRecord` and persist via `RegulationSink`.
5. If the span should trigger algedonic alerts, call
   `RegulationLedger::increment_variety(domain, state_name)`.
6. Mirror any `is_canonical` logic change in `scripts/check-reg-canonical.sh`
   (`event.rs:435`).

---

## Footnotes

[^otel-spans]: OpenTelemetry. (2024). *OpenTelemetry Specification*. Cloud Native Computing Foundation. https://opentelemetry.io/docs/specs/otel/
    Cited for the span-based observability model the Regulation tracing infrastructure follows.

[^beer-cybernetics]: Beer, S. (1979). *The Heart of Enterprise*. John Wiley & Sons.
    Cited for the cybernetic-nervous-system concept that Loop 6 operationalizes through spans.
