---
title: "Sovereignty and Observability"
audience: [operators, developers]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, trust, lifecycle]
---

# Sovereignty and Observability

Inspect and verify hKask's Magna Carta principles (P1–P4), manage delegation tokens and consent records, audit pod boundaries, and read Regulation (Cybernetic Nervous System) spans, variety counters, and algedonic alerts to understand system health. Sovereignty is the foundational guarantee; the Regulation is the observability substrate that verifies enforcement.

## In-process observation surface

zed-kask has no standalone user-facing CLI and no HTTP API. Sovereignty and Regulation state are observed and managed through three in-process surfaces:

| Surface | What it exposes | How to reach it |
|---------|-----------------|-----------------|
| **Agent Panel** (zed native) | Conversational interaction with the Curator agent (D2) and any registered userpod; skills execute via `ManifestExecutor` (D1) | `cmd-t` → select agent (e.g. `Curator`) |
| **Kask Panel** (D10) | Per-MCP-server one-on-one window: direct `/tool args` invocation (OCAP-gated via `PanelToolInvoker` → `BridgeToolPort`) and scoped inference (via `PanelScopedInference` → `GuardedInferencePort`) | `kask_panel::Toggle` / `kask_panel::ToggleFocus` action |
| **`magna-carta-verifier` skill** | Structural audits of P1–P4 enforcement against the codebase (YAML assertions + Jinja2 report templates) | Invoke from the Agent Panel as a skill |
| **`kask` admin CLI** | Backup, wallet, repair, admin only — **not** a user surface for sovereignty/Regulation inspection | `kask <admin-subcommand>` |

The deleted `hkask-api` HTTP server and the deleted `hkask-cli` user surface (`kask sovereignty status`, `kask token list`, `kask regulation health`, etc.) have no equivalents — their function moved in-process. The examples below use the in-process surfaces.

---

## Sovereignty Principles

Sovereignty is hKask's foundational guarantee: users own their data and control how it is accessed. All access requires explicit, scoped, version-aware, revocable consent. No ambient authority exists — every tool invocation passes through the OCAP gate.

The Magna Carta defines four principles, each enforced by specific code paths:

| Principle | Core Guarantee | Enforcement | Fail Mode |
|-----------|---------------|-------------|-----------|
| **P1 — User Sovereignty** | Users own their data and delegation boundaries | `SovereigntyChecker::can_access()` + `require_sovereignty()` | All access denied if no checker wired |
| **P2 — Affirmative Consent** | Default is deny; access requires explicit, scoped, revocable consent | `ConsentManager::has_consent()` (`unwrap_or(false)`) | Storage errors → deny |
| **P3 — Generative Space** | No hidden control plane; content safety is a mandatory floor, not a ceiling | `hkask-guard` at every LLM boundary; no `is_admin` flag | Structurally impossible to hide settings |
| **P4 — Clear Boundaries (OCAP)** | All resource access is capability-gated; no ambient authority | `CapabilityChecker::verify()` + `GovernedTool::invoke()` | Empty roots → reject all; no "god token" |
| **P4.1 — Pod Boundaries** | Pods cannot structurally reach other pods' MCP servers | Type system enforcement via `PerPodToolBinding` | Always enforced (structural) |

### Data Categories

| Category | Classification | Access Rule |
|----------|---------------|-------------|
| `episodic_memory` | Sovereign | Consent **AND** owner match required |
| `personal_context` | Sovereign | Consent **AND** owner match required |
| `capability_tokens` | Sovereign | Consent **AND** owner match required |
| `ocap_boundaries` | Sovereign | Consent **AND** owner match required |
| `semantic_memory` | Shared | Consent required (any WebID) |
| `template_invocations` | Shared | Consent required (any WebID) |
| `template_registry` | Public | No consent required |

---

## Viewing Sovereignty Status

There is no `kask sovereignty status` command. Sovereignty state is observed in-process:

- **Consent state per data category** — queryable through the Curator agent in the Agent Panel ("show me the current consent state for episodic_memory"), which reads `ConsentManager` directly.
- **Active delegation tokens** — visible via the kask panel (D10) by invoking the appropriate MCP tool on the `replica` or `curator` server through `PanelToolInvoker` (OCAP-gated).
- **Per-pod capability bindings** — surfaced by the Curator agent, which holds the singleton `CuratorHandle::system()` and can report `PerPodToolBinding` state.

A representative consent-state view (as the Curator would render it):

```
Sovereignty Status
==================

Consent State:
  WebID: webid://zed-user
  • episodic_memory: GRANTED
  • personal_context: DENIED
  • capability_tokens: GRANTED
  • ocap_boundaries: DENIED
  • semantic_memory: GRANTED
  • template_invocations: DENIED
  • template_registry: GRANTED (public)

Data Boundaries:
  • Sovereign: episodic_memory, personal_context, capability_tokens, ocap_boundaries
  • Shared: semantic_memory, template_invocations
  • Public: template_registry

Affirmative Consent:
  • Requires Affirmative Consent: true
```

The `WebID` is the resolved zed-kask user (single-user in-process; no federation).

---

## Delegation Tokens

OCAP (Object Capability) enforcement uses Ed25519-signed `DelegationToken` objects. Tokens are minted in-process by the `CapabilityChecker` (root issuer is the zed-kask host / `CuratorHandle::system()` singleton — A2A root authority is deferred, not live).

### Listing tokens

Tokens are not listed via a CLI. Use the kask panel (D10) to invoke the appropriate tool on the `replica` or `curator` MCP server, or ask the Curator agent in the Agent Panel. A representative listing:

```
curator — tool:*, inference:*, memory:read — 2026-06-15T10:30:00Z
userpod-alice — tool:web_search, tool:condenser — 2026-06-20T14:22:00Z
```

### Issuing a token

Tokens are issued in-process through the `CapabilityChecker::grant()` / `grant_tool()` / `grant_registry()` API. The kask panel's `PanelToolInvoker` obtains its token from the `a2a_secret` resolved at startup (composition root in `zed/src/main.rs`); skill cascades receive tokens from the `ManifestExecutor` (D1). There is no `kask token issue` command.

To authorize a new binding programmatically (Rust):

```rust
use hkask_capability::verification::CapabilityChecker;

let token = checker.grant_tool("web_search", issuer_webid, holder_webid)?;
```

### Revoking a token

Revocation is performed through the same in-process `CapabilityChecker` surface (the consent store flips `active = false`). There is no `kask token revoke` command. The Curator agent can revoke on operator direction through `CuratorContext::issue_directive()` (which itself passes through OCAP).

### Checking pod-level capability bindings

Per-pod bindings (`PerPodToolBinding` in `crates/hkask-pods/src/pod/deployment.rs`) are inspected by asking the Curator agent or by invoking the appropriate `replica` MCP tool through the kask panel. Each pod's binding shows which tokens authorize which tools.

---

## Consent Management

Consent is managed in-process through `ConsentManager` (`crates/hkask-pods/src/consent.rs`). There is no `kask sovereignty grant` / `revoke` / `check` CLI.

### Granting consent

Grant consent by directing the Curator agent (Agent Panel) to grant a category, or by invoking the appropriate MCP tool through the kask panel. The underlying call is `ConsentManager::grant_consent(webid, category)`, which updates the in-memory cache and persists to the SQLite-backed `ConsentStore`.

### Revoking consent

`ConsentManager::revoke_consent(webid)` flips `active = false` and sets `revoked_at = Some(now)`. Revoke all consent for the current user by directing the Curator agent to do so. A `reg.sovereignty` span with `operation=consent_revoked` is emitted.

### Checking access for a specific category

A consent check returns a verdict shaped like:

```
Data Access Check
=================
  Category: episodic_memory
  Classification: SOVEREIGN
  Access required: CONSENT + OWNER_MATCH
  Access: DENIED
  → Grant via the Curator agent or the kask panel's consent tool.
```

The check flows through `ConsentManager::has_consent()` → `SovereigntyChecker::can_access()`. Sovereign categories require both consent **and** owner match.

---

## Pod Boundary Auditing

There is no `kask pod list` / `kask pod status` CLI. Pod state is observed in-process:

- **Active pods** — surfaced by the Curator agent (which holds the singleton `CuratorHandle::system()`). A representative listing:

```
Agent pods (2):
  curator-primary (active)
    WebID: webid://curator
    Name:  curator
  userpod-alice (active)
    WebID: webid://alice
    Name:  alice
```

- **Per-pod tool bindings and OCAP state** — inspected via the Curator agent or the kask panel. Each pod has its own `PerPodToolBinding`, dedicated SQLCipher file, and per-pod variety counters. Pods are structurally isolated — cross-pod dispatch is impossible at the type level.

---

## OCAP Enforcement

OCAP enforcement runs through the `GovernedTool` membrane. Every tool call passes five gates:

```
Caller → GovernedTool.invoke(server, tool, args, token)
           ├─ Step 0: token.verify() — cryptographic authenticity
           ├─ Step 1: verify_capability_exact(token, tool) || verify_capability_domain_fallback(token, tool)
           ├─ Step 2: cybernetics.can_proceed(agent, estimated_cost) — gas budget check
           ├─ Step 3: emit reg.tool.invoked span
           ├─ Step 4: inner.invoke(server, tool, args, token) → delegate
           └─ Step 5: settle_gas(agent, reserved, actual) → refund if over-estimated
```

The inner `ToolPort` is `BridgeToolPort` (in `kask_bridge`, over zed's `McpRuntime`), **not** the deleted `McpDispatcher` from `hkask-mcp`.

Three startup gates control MCP server access:
- **Gate 1 (auth):** Server refuses to start on failure → `McpError::Auth`
- **Gate 2 (assignment):** Server refuses to start on failure → `McpError::RoleAssignment`
- **Gate 3 (capability per tool):** Non-fatal — server starts in degraded mode with denied tools unavailable

---

## Magna Carta Verification

Structural audits of P1–P4 enforcement are run through the **`magna-carta-verifier` skill**, invoked from the Agent Panel. The skill loads YAML assertion manifests (one per principle) and renders a verification report through Jinja2 templates. There is no `kask sovereignty verify` CLI.

To run a full verification, invoke the `magna-carta-verifier` skill from the Agent Panel. To verify a single principle, scope the skill invocation (e.g. "verify only `affirmative_consent`"). The skill's assertion manifests live in `.agents/skills/magna-carta-verifier/manifests/` (`p1-user-sovereignty.yaml`, `p2-affirmative-consent.yaml`, `p3-generative-space.yaml`, `p4-clear-boundaries.yaml`).

A representative report:

```
Magna Carta Verification Report
==============================

## User Sovereignty (P1)

  ✓ P1-001 sovereignty_checker_configured check: pass
    → SovereigntyChecker found in crate hkask-pods
  ✓ P1-002 require_sovereignty_enforced check: pass
    → All pod accesses route through require_sovereignty()
  △ P1-003 data_portability_export check: gap
    → Export path exists but not tested
    ⚑ Add integration test for the export tool

  Principle summary: 2 pass, 0 fail, 1 gap
```

### No HTTP consent endpoint

The deleted `hkask-api` HTTP server exposed `curl -H "Authorization: Bearer $HKASK_API_KEY" http://localhost:3000/sovereignty`. That endpoint no longer exists. Consent state is queried in-process through the surfaces above.

---

## Understanding Denial Events

When access is denied, Regulation emits spans that help trace the root cause. Observe these spans through the Curator agent or the kask panel (D10), which read from `RegulationLedger`.

### `reg.tool` (ToolError)

Emitted when a tool invocation fails, including OCAP denials. Look for error messages containing "CapabilityDenied" or "EnergyBudgetExceeded":

- **Token signature invalid** → `ToolPortError::CapabilityDenied("Token failed cryptographic verification")` — check token validity and trusted roots
- **No capability for tool** → `ToolPortError::CapabilityDenied("Token does not authorize tool: X")` — issue a token with the required capability
- **Gas budget exceeded** → `ToolPortError::EnergyBudgetExceeded(...)` — increase the energy cap or reduce consumption
- **No CapabilityChecker configured** → `AgentPodError::CapabilityDenied` (fail-closed)

### `reg.sovereignty` (consent_checked)

Emitted when a consent check is performed. The observation field contains the result (`granted` or `denied`):

- **No consent grant** → `has_consent()` returns `false` — grant consent via the Curator agent or kask panel
- **Storage error** → `unwrap_or(false)` in `has_consent()` — check database connectivity (`HKASK_DB_PATH`, `HKASK_DB_PASSPHRASE`)
- **Consent revoked** → `ConsentRecord::active = false` — re-grant if appropriate
- **Sovereign data without owner match** → Even with consent, sovereign data requires owner match

### Common Denial Scenarios

| Scenario | Error | Fix |
|----------|-------|-----|
| No `SovereigntyChecker` wired | `AgentPodError::SovereigntyDenied` | Wired automatically by `AgentService` |
| No `CapabilityChecker` wired | `AgentPodError::CapabilityDenied` | Wired automatically by `AgentService` |
| No `ConsentManager` wired | `DenyAllConsent` returns `false` | Wired automatically by `AgentService` |
| Storage error in consent check | Consent denied | Check `HKASK_DB_PATH` and `HKASK_DB_PASSPHRASE` |
| Token expired | `CapabilityChecker::verify_with_time()` returns `false` | Re-issue token with a longer TTL |
| Skill cascade without budget | `reg.gas.depleted` span emitted | Increase energy budget |

---

## Regulation Health Monitoring

The Regulation is Loop 6 of hKask's cybernetic architecture — an observability substrate that emits typed spans for every operation affecting system state. When variety drops or errors spike, the Regulation emits **algedonic alerts** (pleasure/pain signals) to the operator.

Regulation spans are typed identifiers in a dot-separated namespace (e.g., `reg.tool.web_search`, `reg.inference`, `reg.gas.reserved`). Every tool invocation, inference call, gas consumption, contract lifecycle event, and sovereignty check emits a span.

Spans flow through two paths:

| Path | Mechanism | Purpose |
|------|-----------|---------|
| **Tracing** | `tracing::info!(target: "regulation", ...)` | Structured logging |
| **ν-event** | `RegulationRecord` → `RegulationSink` → SQLite | Persistent cybernetic audit trail |

Spans describe *what* happened; ν-events describe *who observed it, when, in what context, and what they saw*.

### Reading Health Status

There is no `kask regulation health` CLI. Health status is observed in-process:

- Ask the Curator agent (Agent Panel) to report Regulation health — it reads `RegulationLedger::health()` directly.
- Or invoke the appropriate tool on the `replica` MCP server through the kask panel (D10).

A representative health view:

```
Regulation Health Status
========================

Runtime Status:
  • Healthy: true | false            ← Overall health
  • Overall variety deficit: <N>     ← How far below expected variety
  • Critical alerts: <N>             ← Critical threshold breaches
  • Warning alerts: <N>              ← Warning threshold breaches

Variety Counter Summary:
  • reg.tool.web_search: 12 states    ← Per-namespace variety counts
  • reg.inference: 8 states
  • reg.tool.condenser: 3 states
  ...

Active Algedonic Alerts:
  • [Critical] reg.tool: Tool variety critically low
  • [Warning] reg.inference: Inference error rate elevated
  ...

Energy Budget Status:
  • Model: Energy tracking (subsumes rate limiting)
  • Status: OPERATIONAL
```

Key indicators to watch:
- **`Healthy: false`** — immediate investigation needed
- **`Critical alerts: > 0`** — at least one domain has fallen below its critical threshold
- **`Overall variety deficit`** — growing deficit means the system is seeing fewer distinct operational patterns

---

## Regulation Alerts

### Viewing Active Alerts

Active algedonic alerts are surfaced by the Curator agent (which receives `CurationInput` messages on the `alerts_tx` channel) or via the kask panel. There is no `kask regulation alerts` CLI.

A representative alert view:

```
Algedonic alerts:
  • [Critical] reg.tool: Tool call failures exceeded threshold
  • [Warning] reg.gas: Energy budget running low
```

If no alerts are active:

```
Algedonic alerts:
  (no active alerts)
```

### Interpreting Algedonic Alerts

Algedonic alerts escalate through severity levels based on Ashby's Law of Requisite Variety:

| Deficit vs Threshold | Severity | Action |
|----------------------|----------|--------|
| deficit ≤ threshold/2 | **Info** | Logged, no escalation |
| deficit > threshold/2, ≤ threshold | **Warning** | `warn!` log emitted |
| deficit > threshold | **Critical** | `error!` log + `DepletionSignal` broadcast |

The default variety threshold is `DEFAULT_VARIETY_MAX_DEFICIT` (from `hkask_regulation`). Per-domain expected variety can be configured via `AlgedonicManager::set_expected_variety()`.

| Severity | Meaning | Response |
|----------|---------|----------|
| **Info** | Normal operation, noted for audit | No action required |
| **Warning** | Degraded but functional — variety dipping or errors rising | Monitor; check recent spans for patterns |
| **Critical** | Threshold breached — system may be blind to important states | Investigate immediately; review the affected domain |
| **Fatal** | System cannot continue | `DepletionSignal` broadcast; agent pods halt |

---

## Variety Counters

Variety measures how many distinct operational states each namespace is experiencing. Observe via the Curator agent or kask panel (D10); there is no `kask regulation variety` CLI.

A representative view:

```
Variety counters:
  • reg.tool.web_search: 12 states
  • reg.inference: 8 states
  • reg.gas: 5 states
  • reg.curation: 3 states
```

Low variety in a namespace signals the system is stuck in a narrow operational band — it is not exploring, adapting, or handling diverse inputs.

---

## Set Points

Set points define the Regulation's expected operating parameters. Observe and adjust them in-process; there is no `kask regulation set-points` CLI.

A representative view:

```
Regulation Set-Points
=====================
  gas_min_remaining:           0.20
  variety_max_deficit:         100
  error_rate_max:              0.30
  connector_latency_max_secs:  30.0
```

| Set Point | Meaning | What Happens When Breached |
|-----------|---------|---------------------------|
| `gas_min_remaining` | Minimum energy budget before depletion signal | `DepletionSignal` broadcast to subscribers |
| `variety_max_deficit` | Maximum tolerated variety drop | Algedonic alert fires; severity depends on deficit size |
| `error_rate_max` | Maximum tolerated error rate (0.0–1.0) | Error rate alert fires |
| `connector_latency_max_secs` | Maximum connector response latency | Latency alert fires |

> **Note:** The `communication_backpressure_threshold` set-point and the `CommunicationQueueDepth` signal metric have been removed. They belonged to the deleted `hkask-communication` Matrix transport; zed-kask has no communication queue and no `reg.communication` spans.

Set points are loaded from YAML via `SetPointsConfig::load_from_file()` and merged with defaults in `SetPoints::from_config()`. Adjust them by editing the YAML config and reloading, or by directing the Curator agent to issue a calibration directive (within its authority bounds).

---

## Filtering Spans by Namespace

Query Regulation spans by namespace in-process — through the Curator agent or the kask panel. There is no `kask regulation subscribe` CLI.

Common namespace filters:

| Filter | Purpose |
|--------|---------|
| `reg.sovereignty` | Sovereignty-related spans (P1–P2 enforcement) |
| `reg.tool` | Tool invocation spans (P4 OCAP enforcement) |
| `reg.mcp` | MCP startup gate spans |
| `reg.guard.input`, `reg.guard.output` | Guard violation spans |

> **Removed namespaces:** `reg.communication.*` (Matrix transport deleted) and `reg.federation.*` (single-user in-process; no federation) are no longer emitted. Do not filter for them.

### Live Event Observation

Live Regulation events are observed in-process through the Curator agent's `MetacognitionLoop::sense()` (which reads via `query_algedonic`) or through a kask panel tool subscription. There is no long-lived CLI subscription process.

---

## Common Regulation Span Namespaces

| Namespace | What It Tracks | Algedonic? |
|-----------|---------------|------------|
| `reg.tool.*` | MCP tool invocations (web_search, condenser, etc.) | Yes — variety |
| `reg.inference` | LLM inference calls | Yes — variety |
| `reg.gas` | Gas (energy budget) consumption | Yes — depletion |
| `reg.sovereignty` | Consent grants, revocations, checks | No (audit only) |
| `reg.curation` | Curator consolidation and directive operations | No (audit only) |
| `reg.contract.*` | Spec contract lifecycle (proposed, accepted, violated) | Aggregated into quality scores |
| `reg.guard.violation` | Guard rule triggered | Event-based |
| `reg.qa.repair_exhausted` | QA repair attempts exhausted | Strong signal — escalate to Curator |
| `reg.architecture.seam.drift` | Architecture seam divergence | Triggers warnings |
| `reg.slo.evaluated` | SLO metric evaluation | SLO breach → Critical if SLO severity is Critical |
| `reg.meta.*` | Curator self-calibration decisions | No (meta-level; deliberately not in algedonic categories) |

> **Removed:** `reg.communication.*` and `reg.federation.*` are no longer emitted (Matrix transport and federation deleted).

For the full span catalog, see `docs/reference/regulation-spans.md`.

---

## Responding to Critical Alerts

1. **Identify the domain** — The alert message names the affected namespace (e.g., `reg.tool`)

2. **Check variety counters** — Ask the Curator agent (or use the kask panel) to report variety per namespace

3. **Check active alerts** — Ask the Curator agent to list active algedonic alerts

4. **Check the energy budget** — Health status includes gas status; depletion can cascade into tool failures

5. **Inspect pod state** — Ask the Curator agent to report per-pod bindings and OCAP state

6. **Review escalation log** — If the alert triggered a `DepletionSignal`, check agent pod escalation records

7. **Address the root cause**:
   - **Variety deficit**: The system is seeing too few distinct inputs — check connectivity, tool availability, or inference model health
   - **Gas depletion**: Increase the energy cap or reduce consumption
   - **Error rate spike**: Check `reg.tool.*` for tool failures, `reg.inference` for model errors
   - **SLO breach**: Review the breached service-level objective and its time window

8. **Escalate if unresolved** — Persistent critical alerts escalate to the Curator's metacognition layer automatically (the `alerts_tx` channel delivers `CurationInput` messages to `CurationLoop`). Direct the Curator agent (Agent Panel) to review its escalation queue and self-calibration decisions (`reg.meta.*` spans).

---

## Programmatic Access

Within Rust code, access Regulation data through `RegulationLedger`:

```rust
use hkask_regulation::RegulationLedger;

let rt = RegulationLedger::with_threshold(100);
let variety = rt.variety().await; // HashMap<SpanNamespace, u64>
let health = rt.health().await;   // LedgerHealth { healthy, overall_deficit, ... }
let alerts = rt.alerts().await;   // Vec<RuntimeAlert>
```

This is the same surface the Curator agent and the kask panel use to render their views.

---

## Related

- [Magna Carta Reference](../reference/magna-carta.md) — Full principle text, enforcement traces, failure modes
- [Regulation Span Registry](../reference/regulation-spans.md) — Full span taxonomy
- [Install and Configure hKask](install-and-configure.md) — Content guard configuration and `reg.guard.*` spans
- [Architecture Patterns](architecture-patterns.md) — Hexagonal ports, bridge adapters (`BridgeToolPort`, `BridgeMemoryPort`, `LanguageModelInferencePort`)
- [Sovereignty and OCAP](sovereignty-and-ocap.md) — OCAP dispatch membrane and delegation token attenuation
