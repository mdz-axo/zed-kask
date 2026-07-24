---
title: "Magna Carta — Reference"
audience: [developers, operators, auditors]
last_updated: 2026-07-07
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, trust, curation]
last-verified-against: "3d1a876f"
---

# Magna Carta Reference

The Magna Carta is hKask's charter of liberties. It defines four foundational principles that every
module, agent, and pod must honour. This document is a **reference**: it states what exists, how it
is enforced, and how to verify it. It does not explain *why* (see `docs/architecture/core/magna-carta.md`
for rationale) or *how to fix violations* (see `docs/how-to/audit-sovereignty.md`).

## Table of Contents

1. [Principle Hierarchy](#principle-hierarchy)
2. [P1 — User Sovereignty](#p1--user-sovereignty)
3. [P2 — Affirmative Consent](#p2--affirmative-consent)
4. [P3 — Generative Space](#p3--generative-space)
5. [P4 — Clear Boundaries (OCAP)](#p4--clear-boundaries-ocap)
6. [P4.1 — Pod Boundary Constraint](#p41--pod-boundary-constraint)
7. [Audit Commands](#audit-commands)
8. [Enforcement Trace Summary](#enforcement-trace-summary)

---

## Principle Hierarchy

hKask principles are classified by **constraint force**:

| Tier | Force | Examples | Description |
|------|-------|----------|-------------|
| **Prohibition** | Absolute | P1, P2, P4 | Violation is a runtime error or structural impossibility. Must fail closed. |
| **Guardrail** | Conditional | P3, P3.1 | Violation is prevented at boundaries but the space inside is generative. |
| **Guideline** | Advisory | P5–P12 | Violation is a design smell; CI `invariants` job flags regressions. |

The Magna Carta (P1–P4) is entirely in the **Prohibition** tier. P4.1 is a structural Prohibition —
it cannot be violated because the type system prevents it.

---

## P1 — User Sovereignty

> **Exact wording:** "Users own their data and delegation boundaries. Data categorization, control, and portability are first-class guarantees."

**Prohibition level:** Prohibition (fail-closed).

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `DataSovereigntyBoundary` | `hkask-types::curation` | Defines sovereign / shared / public category sets |
| `SovereigntyChecker` | `hkask-pods::sovereignty` | Runtime gate: `can_access(category, requester)` with consent lookup |
| `SovereigntyConsent` trait | `hkask-pods::sovereignty` | Pluggable consent port; `DenyAllConsent` is the default |
| `ConsentManager` | `hkask-pods::consent` | Production implementation: SQLite-backed, Regulation-span-emitting |
| `PodContext::require_sovereignty()` | `hkask-pods::pod::context` | Called before every data access; fail-closed on missing checker |
| `SovereigntyBoundaryStore` | `hkask-storage::sovereignty` | SQL persistence of user boundaries |
| `sovereignty_router()` | `hkask-api::routes::sovereignty` | `GET/POST /sovereignty` endpoints |
| `kask sovereignty` | `hkask-cli::commands::sovereignty` | CLI: `status`, `grant`, `revoke`, `check` |

### What Happens When Violated

When `require_sovereignty()` is called without consent:

1. `SovereigntyChecker::can_access()` returns `false` for sovereign data without matching owner + consent, or shared data without consent.
2. `PodContext::require_sovereignty()` returns `AgentPodError::SovereigntyDenied { category, requester }`.
3. **If no `SovereigntyChecker` is configured at all**, the pod returns `SovereigntyDenied` immediately — sovereignty fails closed.
4. The caller (agent loop, tool, or API) receives the error and cannot proceed.

Data categories and their defaults:

| Category | Classification | Access Rule |
|----------|---------------|-------------|
| `episodic_memory` | Sovereign | Consent **AND** owner match required |
| `personal_context` | Sovereign | Consent **AND** owner match required |
| `capability_tokens` | Sovereign | Consent **AND** owner match required |
| `ocap_boundaries` | Sovereign | Consent **AND** owner match required |
| `semantic_memory` | Shared | Consent required (any WebID) |
| `template_invocations` | Shared | Consent required (any WebID) |
| `template_registry` | Public | No consent required |

### How to Audit

```bash
# View sovereignty status for current user
kask sovereignty status

# Verify P1 assertions via structural audit
kask sovereignty verify --principle user_sovereignty
kask sovereignty verify --principle user_sovereignty --json

# Check a specific access (from CLI)
kask sovereignty check --category episodic_memory --requester webid://alice
```

---

## P2 — Affirmative Consent

> **Exact wording:** "Default is deny. Access requires explicit, scoped, version-aware, and revocable consent."

**Prohibition level:** Prohibition (fail-closed).

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `DataSovereigntyBoundary::requires_affirmative_consent` | `hkask-types::curation` | Set to `true` by default (P2 charter) |
| `ConsentManager::has_consent()` | `hkask-pods::consent` | Fail-closed: `unwrap_or(false)` — storage errors are deny |
| `SovereigntyConsent::has_consent()` | `hkask-pods::sovereignty` | `DenyAllConsent` impl returns `false` for everything |
| `DenyAllConsent` | `hkask-pods::sovereignty` | Default port; used until a real `ConsentManager` is wired |
| `ConsentRecord` | `hkask-pods::consent` | Per-WebID, active/revoked, time-stamped |
| `SovereigntyBoundaryEntry::requires_affirmative_consent` | `hkask-storage::sovereignty` | Stored as `"required"` / `"open"` in SQL |
| Regulation spans | `reg.sovereignty` | `consent_granted`, `consent_revoked`, `consent_checked` |

### Consent Properties

| Property | Enforcement |
|----------|------------|
| **Scoped** | Per `(WebID, DataCategory)` pair |
| **Version-bound** | Consent invalidated when a category resource is upgraded |
| **Time-bound** | `ConsentRecord` has `granted_at` and `revoked_at` timestamps |
| **Revocable** | `ConsentManager::revoke_consent()` sets `active = false` |
| **Hierarchical** | Master > per-agent > per-agent-type; most-specific grant wins |
| **Fail-closed** | `DenyAllConsent` default + `unwrap_or(false)` in `has_consent()` |

### What Happens When Violated

1. `SovereigntyConsent::has_consent()` is called for a `(webid, category)` pair.
2. If no grant exists → `false`. If grant exists but is revoked → `false`. If storage fails → `false` (fail-closed).
3. `SovereigntyChecker::can_access()` returns `false`, and `require_sovereignty()` returns `SovereigntyDenied`.
4. Regulation emits `reg.sovereignty consent_checked result=denied`.

### How to Audit

```bash
# Grant consent for a category
kask sovereignty grant --category episodic_memory --agent curator

# Revoke consent
kask sovereignty revoke --category episodic_memory

# Verify P2 assertions
kask sovereignty verify --principle affirmative_consent

# Check API consent status
curl -H "Authorization: Bearer $HKASK_API_KEY" \
  http://localhost:3000/sovereignty
```

---

## P3 — Generative Space

> **Exact wording:** "Within user-defined boundaries, hKask remains maximally generative. No hidden or engineer-only control plane."

**Prohibition level:** Guardrail (mandatory floor, open ceiling).

**P3.1 — Social Generativity (v0.31.0):** The Generative Space operates within the social conventions
of the jurisdiction where it is used. Core content safety controls (prompt injection, role override,
secret leakage) are mandatory at every LLM boundary and cannot be disabled. These controls are
implemented in `hkask-guard` and aligned with OWASP Top 10 for LLM Applications.

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `InferenceConfig` | `hkask-inference` | Exposes `temperature`, `top_k`, `top_p`, `repeat_penalty` — no hidden params |
| `hkask-guard` | `crates/hkask-guard/` | Mandatory content safety at every LLM boundary (P3.1 floor) |
| No admin bypass | Codebase-wide | No `is_admin` check, no `engineer_mode` feature flag, no hidden control plane |
| Open-source | AGPL-3.0 | All weights/settings exposed; closed-source providers are excluded by charter |
| `FusionSkill` enum | `hkask-inference::config` | Skills are user-selectable; no system-imposed defaults |

### What Happens When Violated

- **Hidden settings:** Not possible structurally — all inference parameters are in `LLMParameters` and exposed through the router.
- **Content safety bypass:** `hkask-guard` runs at every LLM boundary. Bypassing it requires modifying source code.
- **Engineer-only access:** No code path grants elevated access based on role. If one were added, it would be a Magna Carta violation flagged by `kask sovereignty verify`.
- **Non-open-source providers:** Cannot satisfy this principle; hKask is limited to open-weight/open-code providers by charter.

### How to Audit

```bash
# Verify P3 assertions
kask sovereignty verify --principle generative_space

# List exposed inference settings
kask settings show

# Verify guard configuration (structural — no runtime guard command exists)
kask sovereignty verify --principle generative_space
```

---

## P4 — Clear Boundaries (OCAP)

> **Exact wording:** "P1–P3 are enforced through explicit capability boundaries. No ambient authority and no admin bypass."

**Prohibition level:** Prohibition (fail-closed, type-enforced).

### Dual Enforcement Gate

Every resource access passes through two gates:

1. **`require_capability`** — Ed25519-signed `DelegationToken` verification
2. **`require_sovereignty`** — Data category consent check

No code path can access resources without going through both gates.

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `DelegationToken` | `hkask-capability::token_types` | Ed25519-signed, unforgeable, attenuating capability token |
| `CapabilityChecker` | `hkask-capability::verification::checker` | Verifies signature + trusted-root membership; fail-closed (empty roots reject all) |
| `GovernedTool<P>` | `hkask-regulation::governed_tool` | Membrane wrapping `ToolPort`: OCAP check → gas reserve → Regulation span → delegate → settle |
| `GovernedTool::invoke()` | `hkask-regulation::governed_tool` | Step 0: verify token signature; Step 1: exact-match or domain-match capability; Step 2: gas budget; Step 3–5: execute, settle, emit |
| `PodContext::require_capability()` | `hkask-pods::pod::context` | Verifies token signature + delegated_to match; fail-closed on missing checker |
| `DaemonClient::capability_query()` | `hkask-mcp::daemon` | Server-side capability check at startup (Gate 3) |
| `verify_startup_gates()` | `hkask-mcp::startup` | Gate 1 (auth) → Gate 2 (assignment) → Gate 3 (capability per tool) |

### Token Properties

| Property | Enforcement |
|----------|------------|
| **Unforgeable** | Ed25519 signature must verify against a trusted root; `enforce_roots: true` rejects self-signed tokens from unknown keys |
| **Attenuating** | `SYSTEM_MAX_ATTENUATION` limits delegation depth; `SYSTEM_MAX_RECURSION` limits recursive delegation |
| **No admin override** | No "god token" exists; all access goes through the same `CapabilityChecker::verify()` gate |
| **Bearer-token gate** | API middleware uses `CapabilityChecker::with_trusted_roots(vec![])` — empty roots reject ALL tokens (fail-closed for API auth misconfiguration) |

### The GovernedTool Membrane

The `GovernedTool<P>` struct is the **singular membrane** through which all tool invocations pass.
It is the OCAP enforcement point at runtime:

```
Caller → GovernedTool.invoke(server, tool, args, token)
           │
           ├─ Step 0: token.verify() → cryptographic authenticity
           ├─ Step 1: verify_capability_exact(token, tool) || verify_capability_domain_fallback(token, tool)
           │           → OCAP authority (exact-match or domain-based)
           ├─ Step 2: cybernetics.can_proceed(agent, estimated_cost)
           │           → gas budget check (hold-settle pattern)
           ├─ Step 3: emit reg.tool.invoked span
           ├─ Step 4: inner.invoke(server, tool, args, token) → delegate
           └─ Step 5: settle_gas(agent, reserved, actual) → refund if over-estimated
```

### What Happens When Violated

1. **Invalid token signature** → `ToolPortError::CapabilityDenied("Token failed cryptographic verification")`
2. **No capability for tool** → `ToolPortError::CapabilityDenied("Token does not authorize tool: X")`
3. **Gas budget exceeded** → `ToolPortError::EnergyBudgetExceeded(...)`
4. **No CapabilityChecker configured** → `AgentPodError::CapabilityDenied` (fail-closed)
5. **Gate 1 failure (auth)** → `McpError::Auth` — server refuses to start
6. **Gate 2 failure (assignment)** → `McpError::RoleAssignment` — server refuses to start
7. **Gate 3 failure (capability)** → Non-fatal; server starts in degraded mode with denied tools unavailable

### How to Audit

```bash
# Verify P4 assertions
kask sovereignty verify --principle clear_boundaries

# Inspect delegation token
kask capability inspect --token <token_id>

# Check active pod's capability bindings
kask pod status <pod_id> --verbose
```

---

## P4.1 — Pod Boundary Constraint

> **Exact wording:** "The pod boundary IS the OCAP enforcement perimeter. Tool dispatch cannot cross pod boundaries structurally — a pod has no handle to another pod's MCP servers. `PerPodToolBinding` makes cross-pod dispatch an invalid state."

**Prohibition level:** Prohibition (structural — type-enforced).

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `PerPodToolBinding` | `hkask-pods::pod::deployment` | Scoped MCP runtime + GovernedTool per pod |
| `PerPodRegulationLedger` | `hkask-pods::pod::deployment` | Per-pod variety counters |
| `PerPodStorage` | `hkask-pods::pod::deployment` | Dedicated SQLCipher file per pod at `{data_dir}/agents/{sanitized_name}/pod.db` |
| `PodDeployment` | `hkask-pods::pod::deployment` | Complete pod: identity, storage, Regulation, tools, capability checker |
| `ActivePods` | `hkask-pods::pod::active_pods` | Registry of all pods; no shared state between entries |

### What Happens When Violated

**Cross-pod dispatch is structurally impossible.** A pod has no reference to another pod's `PerPodToolBinding`,
`McpRuntime`, or `CapabilityChecker`. The type system enforces this:

- Each `PodDeployment` owns its own `PerPodToolBinding` (not `Arc`-shared)
- `PodContext` is constructed from a single `PodDeployment` — it cannot reach another pod
- `PodContext::invoke_tool()` routes through the pod's own `governed_tool`, never another pod's

### How to Audit

```bash
# List all active pods
kask pod list

# Inspect a pod's tool bindings
kask pod status <pod_id>

# Verify Regulation isolation (per-pod variety counters)
# Check reg.* spans for pod_id prefix
```

---

## Audit Commands

### Magna Carta Verification

The `magna-carta-verifier` skill runs structural audits against the codebase, loaded from
`.agents/skills/magna-carta-verifier/manifests/`:

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

### Regulation Span Audit

P1–P4 enforcement is observable through Regulation spans:

```bash
# View sovereignty-related spans
kask regulation alerts

# View tool invocation spans (OCAP enforcement)
kask regulation alerts

# View P4 startup gate spans
kask regulation alerts
```

### Consent Management

```bash
# Grant consent for a data category
kask sovereignty grant --category episodic_memory --agent curator

# Revoke consent
kask sovereignty revoke --category episodic_memory

# Check current consent state
kask sovereignty status
```

---

## Enforcement Trace Summary

| Principle | Prohibition Level | Primary Enforcement | Fail-Closed? | Regulation Spans |
|-----------|------------------|--------------------|--------------|-----------|
| P1 (Sovereignty) | Prohibition | `SovereigntyChecker::can_access()` + `require_sovereignty()` | Yes | `reg.sovereignty` |
| P2 (Consent) | Prohibition | `ConsentManager::has_consent()` (`unwrap_or(false)`) | Yes | `reg.sovereignty` |
| P3 (Generative) | Guardrail | `hkask-guard` (floor); no admin bypass (ceiling) | N/A (guardrail) | `reg.guard` |
| P4 (OCAP) | Prohibition | `CapabilityChecker::verify()` + `GovernedTool::invoke()` | Yes (empty roots) | `reg.tool` |
| P4.1 (Pod Boundary) | Prohibition (structural) | `PerPodToolBinding` type isolation | Always (type system) | Per-pod `pod_id` in spans |

### Failure Modes

| Scenario | Behaviour | Error |
|----------|-----------|-------|
| No `SovereigntyChecker` wired | All access denied | `AgentPodError::SovereigntyDenied` |
| No `CapabilityChecker` wired | All tool calls denied | `AgentPodError::CapabilityDenied` |
| No `ConsentManager` wired | All consent checks fail | `DenyAllConsent` returns `false` |
| Storage error in consent check | Consent denied | `unwrap_or(false)` |
| Token signature invalid | Tool call rejected | `ToolPortError::CapabilityDenied` |
| Token expired | Tool call rejected | `CapabilityChecker::verify_with_time()` returns `false` |
| Gas budget exhausted | Tool call rejected | `ToolPortError::EnergyBudgetExceeded` |
| API key without budget | Request rejected | `reg.gas.depleted` span emitted |
| Gate 1 (auth) fails | MCP server refuses to start | `McpError::Auth` |
| Gate 2 (assignment) fails | MCP server refuses to start | `McpError::RoleAssignment` |
| Gate 3 (capability denied) | Server starts, denied tools unavailable | `StartupGateResult::denied_tools` non-empty |
