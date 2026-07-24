---
title: "Sovereignty and Observability"
audience: [operators, developers]
last_updated: 2026-07-12
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, trust, lifecycle]
---

# Sovereignty and Observability

Inspect and verify hKask's Magna Carta principles (P1–P4), manage delegation tokens and consent records, audit pod boundaries, and read Regulation (Cybernetic Nervous System) spans, variety counters, and algedonic alerts to understand system health. Sovereignty is the foundational guarantee; the Regulation is the observability substrate that verifies enforcement.

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

Get the full sovereignty picture for the current user:

```bash
kask sovereignty status
```

Output:

```
Sovereignty Status
==================

Consent State:
  WebID: webid://cli-user
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

---

## Delegation Tokens

OCAP (Object Capability) enforcement uses Ed25519-signed `DelegationToken` objects.

### List Tokens

```bash
kask token list
kask token list --userpod <userpod-name>
```

Output:

```
curator — tool:*, inference:*, memory:read — 2026-06-15T10:30:00Z
userpod-alice — tool:web_search, tool:condenser — 2026-06-20T14:22:00Z
```

### Issue a Token

```bash
kask token issue \
  --userpod my-userpod \
  --capabilities "tool:web_search,tool:condenser,inference:*" \
  --ttl 24h
```

Output:

```json
{
  "token_id": "...",
  "capabilities": [...],
  "expires_at": 1234567890
}
```

Use the token in an IDE or deployment:

```bash
export HKASK_DELEGATION_TOKEN='{"token_id":"...","capabilities":[...],"expires_at":1234567890}'
```

### Revoke a Token

```bash
kask token revoke <token_id>
```

### Check Pod-Level Capability Bindings

```bash
kask pod status <pod_id> --verbose
```

This shows per-pod capability bindings — which tokens authorize which tools.

---

## Consent Management

### Grant Consent

Grant consent for a data category. Use `--agent curator` to authorize the Curator daemon:

```bash
# Grant curator access to episodic memory
kask sovereignty grant --category episodic_memory --agent curator

# Grant for the CLI user
kask sovereignty grant --category semantic_memory
```

### Revoke Consent

Revoke ALL consent for the current user:

```bash
kask sovereignty revoke
```

### Check Access for a Specific Category

```bash
kask sovereignty check --category episodic_memory
```

Output:

```
Data Access Check
=================
  Category: episodic_memory
  Classification: SOVEREIGN
  Access required: CONSENT + OWNER_MATCH
  Access: DENIED
  Use 'kask sovereignty grant --category episodic_memory' to grant.
```

---

## Pod Boundary Auditing

List all active agent pods:

```bash
kask pod list
```

Output:

```
Agent pods (2):
  curator-primary (active)
    WebID: webid://curator
    Name:  curator
  userpod-alice (active)
    WebID: webid://alice
    Name:  alice
```

Inspect a pod's tool bindings and OCAP state:

```bash
kask pod status curator-primary --verbose
```

Each pod has its own `PerPodToolBinding`, dedicated SQLCipher file, and per-pod variety counters. Pods are structurally isolated — cross-pod dispatch is impossible at the type level.

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

Three startup gates control MCP server access:
- **Gate 1 (auth):** Server refuses to start on failure → `McpError::Auth`
- **Gate 2 (assignment):** Server refuses to start on failure → `McpError::RoleAssignment`
- **Gate 3 (capability per tool):** Non-fatal — server starts in degraded mode with denied tools unavailable

---

## Magna Carta Verification

Run structural audits against the codebase to verify P1–P4 enforcement:

```bash
# Full verification report
kask sovereignty verify

# Verify a specific principle
kask sovereignty verify --principle user_sovereignty
kask sovereignty verify --principle affirmative_consent
kask sovereignty verify --principle generative_space
kask sovereignty verify --principle clear_boundaries

# JSON output for CI/automation
kask sovereignty verify --json
```

Sample output:

```
Magna Carta Verification Report
==============================

## User Sovereignty (P1)

  ✓ P1-001 sovereignty_checker_configured check: pass
    → SovereigntyChecker found in crate hkask-pods
  ✓ P1-002 require_sovereignty_enforced check: pass
    → All pod accesses route through require_sovereignty()
  △ P1-003 data_portability_export check: gap
    → Export endpoint exists but not tested
    ⚑ Add integration test for kask sovereignty export

  Principle summary: 2 pass, 0 fail, 1 gap
```

### API Consent Status

If the API server is running:

```bash
curl -H "Authorization: Bearer $HKASK_API_KEY" \
  http://localhost:3000/sovereignty
```

---

## Understanding Denial Events

When access is denied, Regulation emits spans that help trace the root cause.

### `reg.tool` (ToolError)

Emitted when a tool invocation fails, including OCAP denials. Look for error messages containing "CapabilityDenied" or "EnergyBudgetExceeded":

- **Token signature invalid** → `ToolPortError::CapabilityDenied("Token failed cryptographic verification")` — check token validity and trusted roots
- **No capability for tool** → `ToolPortError::CapabilityDenied("Token does not authorize tool: X")` — issue a token with the required capability
- **Gas budget exceeded** → `ToolPortError::EnergyBudgetExceeded(...)` — increase the energy cap or reduce consumption
- **No CapabilityChecker configured** → `AgentPodError::CapabilityDenied` (fail-closed)

### `reg.sovereignty` (consent_checked)

Emitted when a consent check is performed. The observation field contains the result (`granted` or `denied`):

- **No consent grant** → `has_consent()` returns `false` — run `kask sovereignty grant`
- **Storage error** → `unwrap_or(false)` in `has_consent()` — check database connectivity
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
| API key without budget | `reg.gas.depleted` span emitted | Increase energy budget |

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

```bash
kask regulation health
```

Output breakdown:

```
Regulation Health Status
=================

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

```bash
kask regulation alerts
```

Output:

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

Variety measures how many distinct operational states each namespace is experiencing:

```bash
kask regulation variety
```

Output:

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

Set points define the Regulation's expected operating parameters. View current set points:

```bash
kask regulation set-points
```

Output:

```
Regulation Set-Points
==============
  gas_min_remaining:       100
  variety_max_deficit:        50
  error_rate_max:             0.25
  connector_latency_max_secs: 5
  communication_backpressure_threshold: 1000
```

| Set Point | Meaning | What Happens When Breached |
|-----------|---------|---------------------------|
| `gas_min_remaining` | Minimum energy budget before depletion signal | `DepletionSignal` broadcast to subscribers |
| `variety_max_deficit` | Maximum tolerated variety drop | Algedonic alert fires; severity depends on deficit size |
| `error_rate_max` | Maximum tolerated error rate (0.0–1.0) | Error rate alert fires |
| `connector_latency_max_secs` | Maximum connector response latency | Latency alert fires |
| `communication_backpressure_threshold` | Queue depth before backpressure engages | Backpressure alert fires |

To configure set points, provide the flags:

```bash
kask regulation set-points \
  --gas-min-remaining 50 \
  --variety-max-deficit 100 \
  --error-rate-max 0.30
```

---

## Filtering Spans by Namespace

Query Regulation spans by namespace to focus on specific subsystems:

```bash
# Sovereignty-related spans (P1–P2 enforcement)
kask regulation subscribe --agent curator --spans reg.sovereignty

# Tool invocation spans (P4 OCAP enforcement)
kask regulation subscribe --agent curator --spans reg.tool

# MCP startup gate spans
kask regulation subscribe --agent curator --spans reg.mcp

# Federation spans
kask regulation subscribe --agent curator --spans reg.federation

# Communication spans
kask regulation subscribe --agent curator --spans reg.communication

# Guard violation spans
kask regulation subscribe --agent curator --spans reg.guard.input,reg.guard.output
```

### Live Event Subscription

Subscribe to live Regulation events for specific span namespaces:

```bash
kask regulation subscribe --agent curator --spans reg.tool.web_search,reg.inference
```

Output:

```
Regulation Event Subscription
=====================
  Agent: curator
  Span namespaces:
    • reg.tool.web_search
    • reg.inference

  Note: Subscription is active for the lifetime of this process.
  Events matching the specified namespaces will be delivered.
```

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
| `reg.federation.*` | Federation link lifecycle | Yes — link degradation |

For the full span catalog, see `docs/reference/regulation-spans.md` (100+ entries across 11 domain enum types).

---

## Responding to Critical Alerts

1. **Identify the domain** — The alert message names the affected namespace (e.g., `reg.tool`)

2. **Check variety counters** — `kask regulation variety` to see which namespace is deficient

3. **Check recent alerts** — `kask regulation alerts` to see active alerts, or `kask regulation subscribe --agent curator --spans <namespace>` to monitor a specific namespace

4. **Check the energy budget** — `kask regulation health` shows gas status; depletion can cascade into tool failures

5. **Inspect pod state** — `kask pod list` then `kask pod status <pod_id>` to verify agents are healthy

6. **Review escalation log** — If the alert triggered a `DepletionSignal`, check agent pod escalation records

7. **Address the root cause**:
   - **Variety deficit**: The system is seeing too few distinct inputs — check connectivity, tool availability, or inference model health
   - **Gas depletion**: Increase the energy cap or reduce consumption
   - **Error rate spike**: Check `reg.tool.*` for tool failures, `reg.inference` for model errors
   - **SLO breach**: Review the breached service-level objective and its time window

8. **Escalate if unresolved** — Persistent critical alerts should be escalated to the Curator daemon for metacognitive review:

   ```bash
   kask curator escalations
   kask curator metacognition
   ```

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

---

## Related

- [Magna Carta Reference](../reference/magna-carta.md) — Full principle text, enforcement traces, failure modes
- [Regulation Span Registry](../reference/regulation-spans.md) — Full span taxonomy (100+ entries)
- [Install and Configure hKask](install-and-configure.md) — Content guard configuration and `reg.guard.*` spans
- [Agents and Pods](install-and-configure.md) — Pod status and capability inspection