---
title: "Regulation Span Registry — Reference"
audience: [developers, operators, agents]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, curation]
---

## 1. Purpose

Regulation spans are the observability substrate of hKask's Cybernetic Nervous System (Loop 6). Every operation that affects system state — tool invocations, inference calls, gas consumption, contract lifecycle events — emits a **span** through the Regulation tracing infrastructure.

> **Hosting note (v0.31.0):** hKask runs in-process inside zed-kask. The standalone `kask` CLI and
> `hkask-api` HTTP server have been **deleted**. Span query/subscribe commands previously exposed
> via `kask regulation ...` are now available through the in-process kask panel (D10) and
> programmatic `RegulationLedger` calls. The `ApiRequestSpan` enum (§3.11) is retained below as a
> **deleted** historical reference — it metered the deleted HTTP API and is no longer emitted.
> Skill spans (§3.9) are now emitted by `hkask-templates` (which owns `ManifestExecutor` and skill
> execution via D1), replacing the deleted `hkask-services-skill`.

A **span** is a typed identifier that pins an observation to a canonical dot-separated namespace (e.g., `reg.tool.web_search`). Spans carry an **operation** verb (e.g., `invoked`, `completed`, `reserved`) and optional structured fields. They flow through two paths:

1. **tracing infrastructure** — `tracing::info!(target: "regulation", reg_domain = …, operation = …)` — for structured logging
2. **ν-event persistence** — `RegulationRecord::new()` → `RegulationSink::persist()` — for the cybernetic audit trail

### Span vs. RegulationRecord

| Concept | Role | Contains |
|---|---|---|
| **ObservableSpan** (trait) | Typed span enums implement this; provides `as_str()`, `emit()`, `to_event()`, and `emit_to()` | Canonical namespace string |
| **Span** (struct) | Pair of `SpanNamespace` + path (e.g., `reg.tool` + `invoked` → `reg.tool.invoked`) | Namespace + full path |
| **SpanNamespace** (newtype) | Validated string wrapper; construction validates against `CANONICAL_NAMESPACES` | Dot-separated namespace string |
| **RegulationRecord** (struct) | Full cybernetic observation: who observed, what span, what phase, what was observed | Span, `WebID`, `CyclePhase`, `observation` (JSON), `regulation`, `outcome`, recursion depth |
| **SpanKind** (enum) | Typed constructors for common span paths (eliminates string typos) | Canonical (namespace, path) pairs |

Spans describe *what* happened; RegulationRecords describe *who observed it, when, in what context, and what they saw*.

### Span validation

All namespace strings are registered in `CANONICAL_NAMESPACES` (`crates/hkask-types/src/event.rs`) — a ~100-entry array that is the single source of truth. `SpanNamespace::new()` panics on unknown namespaces; `SpanNamespace::parse()` returns `None`. Domain span enums construct namespaces via `SpanNamespace::from_observable()` which also validates.

---

## 2. Span Namespace Taxonomy

Namespaces form a tree rooted at `regulation`. The namespace prefix maps to a `SpanCategory` for typed dispatch:

| Category | Prefixes | Examples |
|---|---|---|
| **Cybernetics** | `reg.variety*`, `reg.gas*`, `reg.regulation*` | `reg.variety`, `reg.gas.reserved`, `reg.regulation.impact_verified` |
| **Curation** | `reg.curation*`, `reg.spec*` | `reg.curation.directive_acknowledged` |
| **Inference** | `reg.inference*` | `reg.inference` |
| **Episodic** | `reg.agent_pod*`, `reg.connector*` | `reg.agent_pod.registered` |
| **Wallet** | `reg.wallet*` | `reg.wallet.balance`, `reg.wallet.key_issued` |
| **Unknown** | Everything else | `reg.tool.web_search`, `reg.consent`, `reg.api.request` |

---

## 3. Domain-Specific Span Enums

### 3.1 RegulationSpan — Core Regulation Spans

**File:** `crates/hkask-types/src/regulation.rs`

Core spans used across 2+ crates. This is the foundational enum implementing `ObservableSpan`.

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `Tool { subsystem }` | `reg.tool.{subsystem}` | MCP tool invocation | Any MCP server dispatches a tool call. Subsystem identifies which server |
| `Inference` | `reg.inference` | LLM inference request/response | GovernedInference prepares/executes/checks an inference call |
| `AgentPod` | `reg.agent_pod` | Agent pod lifecycle events | Pod registration, activation, deactivation |
| `Gas` | `reg.gas` | Gas (energy/budget) consumption | Gas reserved, settled, or depleted for any operation |
| `Curation` | `reg.curation` | Curation loop operations | Registry sync, pod sync, directive issuance |
| `SelfHeal` | `reg.heal` | Self-healing operation | The Regulation runtime's heal callback fires |
| `MemoryEncode` | `reg.memory.encode` | Memory encoding operation | Episodic or semantic memory encodes an observation |

**ToolSubsystem variants** for `RegulationSpan::Tool`:

`WebSearch`, `Condenser`, `Training`, `Replica`, `Research`, `Registry`, `Wallet`, `Media`, `Kanban`, `Memory`, `Companies`, `Docproc`, `Filesystem`, `Curator`, `Other` (catch-all).

> **Deleted variant (v0.31.0):** `Communication` has been removed. The `hkask-mcp-communication`
> server (Matrix/TTS) was deleted; comms/voip/CRDT are now owned by zed-kask upstream and do not
> emit `reg.tool.communication.*` spans.

### 3.2 AcpSpan — Agent Client Protocol

**File:** `crates/hkask-regulation/src/acp_span.rs`

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `AcpUserPodMemorySize` | `reg.acp.userpod.memory_size` | UserPod memory size reported via ACP | [INFERRED] On userpod state sync or ACP handshake |
| `AcpIdeConnectionState` | `reg.acp.ide.connection_state` | IDE connection state change | [INFERRED] IDE client connects or disconnects |

### 3.3 ClassifySpan — Classification Operations

**File:** `crates/hkask-regulation/src/classify_span.rs`

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `ClassifyDualFidelity` | `reg.classify.dual_fidelity` | Dual-fidelity classification decision | [INFERRED] High-fidelity vs. low-fidelity classification mode selected |
| `ClassifyDrift` | `reg.classify.drift` | Classification drift detected | [INFERRED] Model output distribution shifts beyond threshold |

### 3.4 ContractSpan — Spec Contract Lifecycle

**File:** `crates/hkask-regulation/src/contract_span.rs`

Emitted through `emit_contract_*()` functions in `crates/hkask-regulation/src/contract_events.rs`. All events use `CyclePhase::Act` and are persisted via `RegulationSink`.

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `ContractProposed` | `reg.contract.proposed` | UserPod proposes a spec contract | Phase B2–B4: userpod submits contract for human review |
| `ContractAccepted` | `reg.contract.accepted` | Human accepts the contract | Phase B3: reviewer approves the proposed contract |
| `ContractRejected` | `reg.contract.rejected` | Human rejects the contract | Phase B3: reviewer rejects with a reason |
| `ContractViolated` | `reg.contract.violated` | Contract violation during testing | Test harness detects contract non-conformance |

**Algedonic threshold:** Contract violations feed into contract quality metrics. No direct threshold on individual violations — aggregated into contract coverage and quality scores.

### 3.5 InfraSpan — Infrastructure Spans

**File:** `crates/hkask-regulation/src/infra_span.rs`

Cross-subsystem spans used by curator, governance, and wallet components.

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `CiInvariantViolation` | `reg.ci.invariant.violation` | CI invariant check failed | CI pipeline detects a structural invariant break |
| `GuardViolation` | `reg.guard.violation` | Guard rule triggered | A prohibition or constraint guard fires |
| `CuratorConsolidation` | `reg.curator.consolidation` | Curator consolidation run | Curator consolidates pod state from Regulation telemetry |
| `WalletConversion` | `reg.wallet.conversion` | Currency conversion | rJ ↔ USDC conversion executed |

> **Deleted variant (v0.31.0):** `Chat` (`reg.chat`) has been removed from the active span set. It
> was emitted by the deleted `hkask-services-chat` crate. Chat/agent-panel events in zed-kask flow
> through zed's own telemetry and the in-process `MemoryPort` (D6) ingestion path, not through a
> `reg.chat.*` span. The variant is retained in the enum for compile-stability but is no longer
> emitted.

### 3.6 QaSpan — QA Repair Lifecycle

**File:** `crates/hkask-regulation/src/qa_span.rs`

Emitted by the QA test harness (`crates/hkask-test-harness/src/qa_script.rs`) and qa-script-builder.

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `QaRepairAttempted` | `reg.qa.repair_attempted` | Repair step attempted | QA script executes a repair action after a failure |
| `QaRepairVerified` | `reg.qa.repair_verified` | Repair outcome verified | Post-repair verification confirms fix or detects residual failure |
| `QaRepairExhausted` | `reg.qa.repair_exhausted` | Repair attempts exhausted | All repair strategies tried; none succeeded |

**Algedonic threshold:** `QaRepairExhausted` is a strong signal of quality degradation. [INFERRED] Accumulated exhausted repairs escalate to Curator.

### 3.7 SeamSpan — Architecture Seams

**File:** `crates/hkask-regulation/src/seam_span.rs`

Monitors architectural seam health — the boundaries where Strangler Fig migration patterns occur.

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `ArchitectureSeamCoverage` | `reg.architecture.seam.coverage` | Seam coverage measurement | Seam watcher (`seam_watcher.rs`) evaluates coverage of a seam boundary |
| `ArchitectureSeamDrift` | `reg.architecture.seam.drift` | Seam drift detected | Implementation diverges from the seam definition |

**Algedonic threshold:** Seam drift triggers warnings. Coverage below a configurable `seam_coverage_min` set-point triggers critical alerts.

### 3.8 SloSpan — SLO Evaluation

**File:** `crates/hkask-regulation/src/slo_span.rs`

| Variant | Namespace | Meaning | Emitted When |
|---|---|---|---|
| `SloEvaluated` | `reg.slo.evaluated` | SLO metric evaluated | SloManager evaluates a service-level objective against its window |

**Algedonic threshold:** SLO breach escalations are handled by `RegulationLedger` via `reg.slo.breach_escalated` (emitted as a tracing event, not a typed span variant). Severity is `Critical` if the SLO's `Severity` field is `Critical`.


### 3.9 Skill Spans

**File:** `crates/hkask-types/src/event.rs` (CANONICAL_NAMESPACES) · emitted by `crates/hkask-templates/src/manifest_executor.rs` (skill execution via D1)

Skill lifecycle, registry, cascade, convergence, budget, routing, and discovery spans. All namespaced under `reg.skill.*`. Unlike other span types (which have dedicated Rust enums), skill spans are canonical namespace strings emitted as tracing events by the skill execution layer.

> **Ownership note (v0.31.0):** Skill execution moved from the deleted `hkask-services-skill`
> crate to `hkask-templates` (`ManifestExecutor` + registry + cascade + PDCA), invoked in-process
> via the D1 seam (`agent/tools/skill_tool.rs` → bridge.ManifestExecutor). The span emission
> points are unchanged; only the emitting crate moved.

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.lifecycle` | `reg.skill.lifecycle` | Skill lifecycle events (activation, loading, publishing) |
| `reg.skill.lifecycle.skill_activated` | `reg.skill.lifecycle.skill_activated` | A skill is activated for an agent session |
| `reg.skill.lifecycle.skills_loaded` | `reg.skill.lifecycle.skills_loaded` | Skills loaded from the registry |
| `reg.skill.lifecycle.skills_discovered` | `reg.skill.lifecycle.skills_discovered` | Skills discovered during registry scan |
| `reg.skill.lifecycle.skill_published` | `reg.skill.lifecycle.skill_published` | A skill is published to the local registry (legacy: "public zone" publish concept is vestigial — there is no public zone in zed-kask; skills are installed into the local `kask/skills/` registry) |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.registry` | `reg.skill.registry` | Registry validation events |
| `reg.skill.registry.registry_validated` | `reg.skill.registry.registry_validated` | Registry manifest validated successfully |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.cascade` | `reg.skill.cascade` | Cascade step execution |
| `reg.skill.cascade.step_executed` | `reg.skill.cascade.step_executed` | A cascade step was executed |
| `reg.skill.cascade.compute` | `reg.skill.cascade.compute` | Cascade computation event |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.convergence` | `reg.skill.convergence` | Cascade convergence outcomes |
| `reg.skill.convergence.converged` | `reg.skill.convergence.converged` | A skill cascade converged (metric <= threshold) |
| `reg.skill.convergence.escalated` | `reg.skill.convergence.escalated` | A skill cascade escalated (max iterations exhausted) |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.budget` | `reg.skill.budget` | Gas and rJoule budget events |
| `reg.skill.budget.gas_exhausted` | `reg.skill.budget.gas_exhausted` | Gas budget exhausted for a skill execution |
| `reg.skill.budget.gas_alert` | `reg.skill.budget.gas_alert` | Gas alert threshold reached |
| `reg.skill.budget.rjoule_exhausted` | `reg.skill.budget.rjoule_exhausted` | rJoule budget exhausted |
| `reg.skill.budget.rjoule_alert` | `reg.skill.budget.rjoule_alert` | rJoule alert threshold reached |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.frontmatter` | `reg.skill.frontmatter` | SKILL.md frontmatter parse errors |
| `reg.skill.frontmatter.missing` | `reg.skill.frontmatter.missing` | SKILL.md frontmatter is missing |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.manifest` | `reg.skill.manifest` | Registry manifest errors |
| `reg.skill.manifest.unparseable` | `reg.skill.manifest.unparseable` | Registry manifest could not be parsed |
| `reg.skill.manifest.absent` | `reg.skill.manifest.absent` | Registry manifest file is absent |
| `reg.skill.manifest.unreadable` | `reg.skill.manifest.unreadable` | Registry manifest file is unreadable |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.routing` | `reg.skill.routing` | Skill-to-task routing (skill-router) |
| `reg.skill.routing.matched` | `reg.skill.routing.matched` | skill-router produced a ranked recommendation |
| `reg.skill.routing.uncovered` | `reg.skill.routing.uncovered` | skill-router found no matching skill (gap signal) |

| Variant | Namespace | Emitted When |
|---|---|---|
| `reg.skill.discovery` | `reg.skill.discovery` | Capability gap detection and candidate evaluation (skill-discovery) |
| `reg.skill.discovery.gap_detected` | `reg.skill.discovery.gap_detected` | skill-discovery classified a capability gap |
| `reg.skill.discovery.searched` | `reg.skill.discovery.searched` | skill-discovery searched the catalog for candidates |
| `reg.skill.discovery.evaluated` | `reg.skill.discovery.evaluated` | skill-discovery scored a candidate skill |


### 3.10 Wallet Spans

**File:** `crates/hkask-wallet/src/reg_span.rs`

14 variants covering wallet lifecycle: `Balance`, `Deposit`, `DepositShielded`, `Withdrawal`, `Conversion`, `KeyIssued`, `KeyRevoked`, `KeyExpired`, `KeyExhausted`, `ChainError`, `Created`, `Draw`, `Spend`, `Exhausted`.

All namespaced under `reg.wallet.*`. Emitted through `crates/hkask-wallet/src/manager/reg.rs` which bridges wallet operations to the Regulation event sink.

### 3.11 ApiRequestSpan — API Metering (DELETED)

> **DELETED (v0.31.0):** The `hkask-api` HTTP server and its API key auth middleware have been
> removed. hKask now runs in-process inside zed-kask; there is no HTTP API surface to meter.
> `ApiRequestSpan` is retained below as a historical reference only. The span enum, the
> `reg.api.request` namespace, and the `ApiMeter` learning loop are **no longer emitted**. Gas
> consumption for in-process callers is settled by `GovernedTool` / `GovernedInference` and
> tracked via `reg.gas.*` spans (§3.1).

**File (deleted):** `crates/hkask-regulation/src/api_metering.rs` (span enum) · `crates/hkask-api/src/middleware/api_key_auth.rs` (emission point — both deleted)

Historically, this was a single-variant span (`reg.api.request`) emitted for every authenticated API request after the rate limit check passed. It captured:

| Field | Description |
|-------|-------------|
| `key_id` | The API key ID making the request |
| `endpoint` | Request URI path |
| `scope_matched` | Whether the key's scope matched the path (always `true` — scope violations returned early) |
| `gas_consumed` | rJoules consumed (0 at admission; gas was settled downstream by `GovernedTool`) |
| `allocation_remaining` | Remaining rJoules in the key's encumbrance |
| `rate_limit_status` | `ok`, `rate_exceeded`, or `tokens_exceeded` |

It was emitted through `ApiRequestSpan::emit_to()` in the API key auth middleware. The span was an **admission observation** (`CyclePhase::Sense`) — it recorded that a request entered the system, not its completion. Gas consumption was settled later by `GovernedTool`/`GovernedInference` and tracked via `reg.gas.*` spans.

**Configuration (deleted):** `RateLimitConfig::from_env()` read `HKASK_API_RATE_LIMIT_*` environment variables. Per-key limits adapted over time via `ApiMeter::learn()` (LogNormal cost distribution learning).

---

## 4. Span Lifecycle

```
EMISSION ────────► STORAGE ────────► QUERY ────────► DECAY
    │                  │                │               │
    ▼                  ▼                ▼               ▼
tracing::info!    RegulationArchive    GasReport       VarietyTracker
(target: "regulation")   (SQLite)        RegulationLedger      (EMA α=0.1)
    │                                               │
    ▼                                               ▼
RegulationRecord::new()                              AlgedonicManager
+ sink.persist()                            (binary thresholds)
```

### 4.1 Emission

Spans are emitted through two mechanisms:

1. **Tracing path** (`ObservableSpan::emit()` / `RegulationSpan::emit()`): writes `tracing::info!(target: "regulation", reg_domain = …, operation = …, "Regulation")`. Used by `RegulationSpan` variants and by domain-span enums that delegate to `ObservableSpan`.

2. **ν-event path**: constructs `RegulationRecord` with a `Span` (namespace + path), `CyclePhase`, observation JSON, and optional regulation/outcome metadata; persists via `RegulationSink::persist()`. Used by contract events, seam watcher, wallet Regulation manager, governed inference/tool, cybernetics loop, and consent manager.

### 4.2 Storage

RegulationRecords are persisted to a `RegulationArchive` (SQLite-backed) via the `LedgerStoragePort` trait. The store supports:

- **`query_algedonic()`** — filtered queries by span category, time window, and agent. Used by `GasReport` to aggregate gas consumption per tool/agent.

### 4.3 Query

The deleted `kask regulation` CLI has been replaced by the in-process kask panel (D10) and
programmatic `RegulationLedger` queries:

- **kask panel (D10) → Regulation tab** — displays overall health (variety deficit, critical/warning counts), variety counter summary, active algedonic alerts, and energy budget status. Replaces `kask regulation health`.
- **kask panel (D10) → Regulation tab → Alerts** — lists only active algedonic alerts. Replaces `kask regulation alerts`.
- **kask panel (D10) → Regulation tab → Variety** — prints per-namespace variety counters. Replaces `kask regulation variety`.
- **Live event stream** — in-process subscribers register via `RegulationLedger::subscribe()` filtered to specific span namespaces. Replaces the deleted `kask regulation subscribe` SSE stream.
- `RegulationLedger::variety()` — programmatic `HashMap<SpanNamespace, u64>`.
- `RegulationLedger::health()` — `LedgerHealth` struct with aggregate deficit and alert counts.
- `GasReport` — programmatic gas consumption aggregation over time windows.

### 4.4 Decay

Variety tracking uses a **sliding window with exponential moving average (EMA)**:

- **Window:** 60 seconds (`DEFAULT_VARIETY_WINDOW_SECS`)
- **EMA decay factor α:** 0.1 per window reset
- **Formula:** new EMA = 0.9 × old EMA + 0.1 × current raw variety
- **Rationale:** The EMA survives window resets, distinguishing "spiked and died" from sustained low variety

Outcome tracking (success/failure distribution) uses a hard-reset window — no EMA. Counts are cleared on each 60s window expiry.

### 4.5 Algedonic Alerting

When `increment_variety()` is called, the `AlgedonicManager` checks each domain:

| Deficit vs Threshold | Severity | Action |
|---|---|---|
| deficit ≤ threshold/2 | Info | No escalation |
| deficit > threshold/2, ≤ threshold | Warning | `warn!` log |
| deficit > threshold | **Critical** | `error!` log + `DepletionSignal` broadcast to subscribers |

The default threshold is `DEFAULT_VARIETY_MAX_DEFICIT`. Per-domain expected variety can be set via `AlgedonicManager::set_expected_variety()`.

---

## 5. How to Read Spans

### CLI

The deleted `kask regulation` CLI has been replaced by the in-process kask panel (D10). The
equivalent panel surfaces are:

```sh
# Overall Regulation health with span count summary
#   kask panel (D10) → Regulation tab → Health
#   Equivalent in-process call: RegulationLedger::health().await

# Active algedonic alerts
#   kask panel (D10) → Regulation tab → Alerts
#   Equivalent in-process call: RegulationLedger::alerts().await

# Per-namespace variety counters
#   kask panel (D10) → Regulation tab → Variety
#   Equivalent in-process call: RegulationLedger::variety().await

# Subscribe to live events for specific spans
#   In-process: RegulationLedger::subscribe(filter) — filtered to span namespaces
#   e.g. filter to reg.tool.web_search, reg.inference
```

### Programmatic

```rust
use hkask_regulation::RegulationLedger;

let rt = RegulationLedger::with_threshold(100);
let variety = rt.variety().await; // HashMap<SpanNamespace, u64>
let health = rt.health().await;   // LedgerHealth
let alerts = rt.alerts().await;   // Vec<RuntimeAlert>
```

### Adding a New Span

1. Create or extend a domain span enum implementing `ObservableSpan`
2. Add the namespace string to `CANONICAL_NAMESPACES` in `crates/hkask-types/src/event.rs`
3. Add a test verifying `SpanNamespace::new(span.as_str())` succeeds
4. Emit through `SpanNamespace::from_observable()` → `Span::new()` → `RegulationRecord::new()` → `sink.persist()`
5. (Optional) If the span should trigger algedonic alerts, call `RegulationLedger::increment_variety(domain, state_name)`

---

## 6. Cross-Reference: ObservableSpan vs RegulationRecord

| | ObservableSpan (trait) | RegulationRecord (struct) |
|---|---|---|
| **What it is** | A typed span identifier with a canonical namespace string | A full cybernetic observation record |
| **Implements** | `Display + Debug + Send + Sync + 'static` | `Serialize + Deserialize + Clone` |
| **Key fields** | `as_str() -> &'static str`, `emit(operation)`, `to_event(operation, observer, phase, observation) -> Option<RegulationRecord>`, `emit_to(sink, operation, observer, phase, observation)` | `id` (EventID), `span` (Span), `observer_webid` (WebID), `phase` (CyclePhase), `observation` (JSON Value), `regulation`, `outcome`, `recursion_depth` |
| **How emitted** | `tracing::info!(target: "regulation", reg_domain = ..., operation = ...)` | Constructed explicitly, persisted via `RegulationSink` |
| **Validation** | Namespace string validated at `SpanNamespace` construction against `CANONICAL_NAMESPACES` | None beyond serde deserialization |
| **Purpose** | Lightweight, type-safe span emission | Persistent audit trail with full provenance |
| **Example** | `RegulationSpan::Tool { subsystem: ToolSubsystem::WebSearch }.emit("invoked")` | `RegulationRecord::new(webid, span, CyclePhase::Act, observation, 0)` |

RegulationRecords *contain* spans. The `Span` inside a RegulationRecord holds a `SpanNamespace` constructed from an `ObservableSpan` implementation via `SpanNamespace::from_observable()`. The reverse is not true — RegulationRecords are the persistent record; ObservableSpans are the typed factory for constructing them.
