---
title: "Magna Carta — Reference"
audience: [developers, operators, auditors]
last_updated: 2026-07-25
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, trust, curation]
---

# Magna Carta Reference

The Magna Carta is hKask's charter of liberties. It defines four foundational principles that every
module, agent, and user/curator data directory must honour. This document is a **reference**: it states what exists, how it
is enforced, and how to verify it. It does not explain *why* (see `docs/architecture/core/magna-carta.md`
for rationale) or *how to fix violations* (see `explanation/sovereignty-and-observability.md`).

> **Hosting note (v0.31.0, updated 2026-07-25):** hKask now runs in-process inside zed-kask. The standalone `kask` CLI,
HTTP API server (`hkask-api`), daemon process, and `hkask-pods` pod abstraction have been **deleted**. Enforcement traces below
reference the surviving in-process equivalents: `hkask-types::visibility` (sovereignty/consent types), `hkask-capability`, `hkask-guard`,
`hkask-regulation`, and the guard layer wired into zed-kask's inference path (D4). Where a trace
previously pointed at `hkask-api::routes::sovereignty`, `hkask-cli::commands::sovereignty`, or `hkask-pods::pod::context`, the
functionality now lives in the in-process sovereignty checker invoked from the agent loop and the
kask panel (D10). See [`docs/architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md).
>
> **P4.1 note:** The pod boundary constraint (P4.1) was structural when the `hkask-pods` pod abstraction existed. With pod abstraction deleted and replaced by user/curator data directories, the equivalent boundary is the per-user data directory isolation enforced by the in-process sovereignty checker and OCAP membranes. The principle's intent (cross-user dispatch is structurally prevented) is preserved.

## Table of Contents

1. [Principle Hierarchy](#principle-hierarchy)
2. [P1 — User Sovereignty](#p1--user-sovereignty)
3. [P2 — Affirmative Consent](#p2--affirmative-consent)
4. [P3 — Generative Space](#p3--generative-space)
5. [P4 — Clear Boundaries (OCAP)](#p4--clear-boundaries-ocap)
6. [P4.1 — Per-User Boundary Constraint](#p41--per-user-boundary-constraint)
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
| `SovereigntyChecker` | `hkask-types::visibility` | Runtime gate: `can_access(category, requester)` with consent lookup |
| `SovereigntyConsent` trait | `hkask-types::visibility` | Pluggable consent port; `DenyAllConsent` is the default |
| `ConsentManager` | `hkask-types::visibility` | Production implementation: SQLite-backed, Regulation-span-emitting |
| In-process sovereignty gate | zed-kask agent loop + `hkask-types::visibility` | Called before every data access; fail-closed on missing checker. Replaces the deleted `hkask-pods::pod::context::require_sovereignty()` and `hkask-api::routes::sovereignty` HTTP endpoints |
| `SovereigntyBoundaryStore` | `hkask-storage::sovereignty` | SQL persistence of user boundaries |
| Kask panel sovereignty view | `crates/kask_panel` (D10) | Replaces the deleted `hkask-cli::commands::sovereignty` CLI surface; status / grant / revoke / check are exposed through the panel UI |

### What Happens When Violated

When `require_sovereignty()` is called without consent:

1. `SovereigntyChecker::can_access()` returns `false` for sovereign data without matching owner + consent, or shared data without consent.
2. The in-process sovereignty gate returns `SovereigntyDenied { category, requester }`.
3. **If no `SovereigntyChecker` is configured at all**, the gate returns `SovereigntyDenied` immediately — sovereignty fails closed.
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

The deleted `kask sovereignty` CLI has been replaced by the in-process kask panel (D10) and the
`magna-carta-verifier` skill. The commands below describe the panel/skill surfaces; there is no
standalone CLI binary.

```bash
# View sovereignty status for current user (kask panel, D10)
#   Open the kask panel → Sovereignty tab → Status

# Verify P1 assertions via structural audit (magna-carta-verifier skill)
#   Skill reads manifests from .agents/skills/magna-carta-verifier/manifests/
#   Invoke through the agent panel or skill registry:
#       magna-carta-verifier --principle user_sovereignty
#       magna-carta-verifier --principle user_sovereignty --json

# Check a specific access (in-process, via the agent loop)
#   The in-process sovereignty gate is called before
#   every data access; failures surface as SovereigntyDenied errors in the
#   agent panel and reg.sovereignty spans.
```

---

## P2 — Affirmative Consent

> **Exact wording:** "Default is deny. Access requires explicit, scoped, version-aware, and revocable consent."

**Prohibition level:** Prohibition (fail-closed).

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| `DataSovereigntyBoundary::requires_affirmative_consent` | `hkask-types::curation` | Set to `true` by default (P2 charter) |
| `ConsentManager::has_consent()` | `hkask-types::visibility` | Fail-closed: `unwrap_or(false)` — storage errors are deny |
| `SovereigntyConsent::has_consent()` | `hkask-types::visibility` | `DenyAllConsent` impl returns `false` for everything |
| `DenyAllConsent` | `hkask-types::visibility` | Default port; used until a real `ConsentManager` is wired |
| `ConsentRecord` | `hkask-types::visibility` | Per-WebID, active/revoked, time-stamped |
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

The deleted `kask sovereignty` CLI and `hkask-api` HTTP endpoints have been replaced by the
in-process kask panel (D10) and the `magna-carta-verifier` skill.

```bash
# Grant consent for a category (kask panel, D10)
#   Open the kask panel → Sovereignty tab → Grant
#   Equivalent in-process call: ConsentManager::grant_consent(webid, category, agent)

# Revoke consent (kask panel, D10)
#   Open the kask panel → Sovereignty tab → Revoke
#   Equivalent in-process call: ConsentManager::revoke_consent(webid, category)

# Verify P2 assertions (magna-carta-verifier skill)
#   magna-carta-verifier --principle affirmative_consent

# Check consent state in-process (no HTTP API; hkask-api is deleted)
#   ConsentManager::has_consent(webid, category) — fail-closed via unwrap_or(false)
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
| `LanguageModelRegistry` + `KaskSettings` | zed `crates/language_models` + `kask/crates/kask_bridge/src/settings.rs` (D9a) | The user-facing inference-settings surface in zed-kask. Provider selection, model choice, and kask-scoped inference config live here — no hidden params. `hkask-inference`'s `InferenceConfig` is **not** the user-facing surface; it is MCP-server-internal only (see below). |
| `hkask-guard` | `crates/hkask-guard/` | Mandatory content safety at every LLM boundary (P3.1 floor). Wrapped by `GuardedInferencePort` (D4) over `LanguageModelInferencePort` (D8). |
| No admin bypass | Codebase-wide | No `is_admin` check, no `engineer_mode` feature flag, no hidden control plane |
| Open-source | AGPL-3.0 | All weights/settings exposed; closed-source providers are excluded by charter |
| `hkask-inference` (MCP-server-internal) | `kask/crates/hkask-inference` | **Not user-facing.** Retained only for MCP-server-internal use (e.g. `hkask-mcp-condenser`'s `condenser_thread_summary`). Reads API keys via `keyring` crate (D9b), not env vars. The `InferenceConfig` / `FusionSkill` types it exposes are server-internal and do not constitute the user-facing generative-settings surface. |

### What Happens When Violated

- **Hidden settings:** Not possible structurally — the user-facing inference surface is zed's `LanguageModelRegistry` + `KaskSettings` (D9a), both exposed in the Settings UI and the kask panel (D10). `hkask-inference`'s `InferenceConfig` is MCP-server-internal and not a user control plane.
- **Content safety bypass:** `hkask-guard` runs at every LLM boundary. Bypassing it requires modifying source code.
- **Engineer-only access:** No code path grants elevated access based on role. If one were added, it would be a Magna Carta violation flagged by the magna-carta-verifier skill.
- **Non-open-source providers:** Cannot satisfy this principle; hKask is limited to open-weight/open-code providers by charter.

### How to Audit

The deleted `kask sovereignty` and `kask settings` CLI commands have been replaced by the
in-process kask panel (D10) and the `magna-carta-verifier` skill.

```bash
# Verify P3 assertions (magna-carta-verifier skill)
#   magna-carta-verifier --principle generative_space

# List exposed inference settings (kask panel, D10, or KaskSettings page, D9)
#   Open the kask panel → Inference tab, or Settings → Kask section
#   User-facing surface: LanguageModelRegistry + KaskSettings (D9a)
#   hkask-inference's InferenceConfig is MCP-server-internal only (not user-facing)

# Verify guard configuration (structural — no runtime guard command exists)
#   magna-carta-verifier --principle generative_space
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
| In-process capability gate | `hkask-capability::verification::checker` + zed-kask guard layer (D4) | Verifies token signature + delegated_to match; fail-closed on missing checker. Replaces the deleted `DaemonClient::capability_query()` from `hkask-mcp::daemon` and `PodContext::require_capability()` from `hkask-pods::pod::context`; capability verification now happens in-process at the GovernedTool membrane and the inference guard layer |
| Startup gates | `hkask-mcp::startup` (deleted) | Gate 1 (auth) → Gate 2 (assignment) → Gate 3 (capability per tool) were invoked in-process when an MCP server was loaded by zed's `context_server` host. The `verify_startup_gates()` function was deleted in the 2026-07-25 cleanup; `bootstrap_mcp_server()` resolves userpod identity only. |

### Token Properties

| Property | Enforcement |
|----------|------------|
| **Unforgeable** | Ed25519 signature must verify against a trusted root; `enforce_roots: true` rejects self-signed tokens from unknown keys |
| **Attenuating** | `SYSTEM_MAX_ATTENUATION` limits delegation depth; `SYSTEM_MAX_RECURSION` limits recursive delegation |
| **No admin override** | No "god token" exists; all access goes through the same `CapabilityChecker::verify()` gate |
| **Bearer-token gate** | In-process callers (agent loop, MCP server dispatch) use `CapabilityChecker::with_trusted_roots(vec![])` — empty roots reject ALL tokens (fail-closed for misconfiguration). The deleted `hkask-api` HTTP middleware is no longer the bearer gate; the in-process GovernedTool membrane is |

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
4. **No CapabilityChecker configured** → `CapabilityDenied` (fail-closed)
5. **Gate 1 failure (auth)** → `McpError::Auth` — server refuses to start
6. **Gate 2 failure (assignment)** → `McpError::RoleAssignment` — server refuses to start
7. **Gate 3 failure (capability)** → Non-fatal; server starts in degraded mode with denied tools unavailable

### How to Audit

The deleted `kask sovereignty`, `kask capability`, and `kask pod` CLI commands have been replaced
by the in-process kask panel (D10) and the `magna-carta-verifier` skill.

```bash
# Verify P4 assertions (magna-carta-verifier skill)
#   magna-carta-verifier --principle clear_boundaries

# Inspect delegation token (kask panel, D10 → Capability tab)
#   Equivalent in-process call: CapabilityChecker::verify(token) → TokenInfo

# Check active user's capability bindings (kask panel, D10 → Capability tab)
#   Equivalent in-process call: CapabilityChecker::verify(token) → TokenInfo
```

---

## P4.1 — Per-User Boundary Constraint

> **Exact wording (original, with `hkask-pods` pod abstraction):** "The pod boundary IS the OCAP enforcement perimeter. Tool dispatch cannot cross pod boundaries structurally — a pod has no handle to another pod's MCP servers. `PerPodToolBinding` makes cross-pod dispatch an invalid state."
>
> **2026-07-25 update:** The `hkask-pods` pod abstraction (`PodDeployment`, `ActivePods`, `PerPodToolBinding`, `PerPodRegulationLedger`, `PerPodStorage`, `PodContext`) was deleted in the 2026-07-25 cleanup. The equivalent boundary is now the per-user data directory isolation enforced by the in-process sovereignty checker and OCAP membranes. The principle's intent (cross-user dispatch is structurally prevented) is preserved: each user/curator data directory has its own scoped MCP runtime, GovernedTool membrane, and capability checker, with no shared state between directories.

**Prohibition level:** Prohibition (structural — type-enforced).

### Enforcement Trace

| Artefact | Crate/Module | Role |
|----------|-------------|------|
| Per-user MCP runtime | zed-kask `context_server` host + `hkask-mcp` | Scoped MCP runtime + GovernedTool per user/curator data directory |
| Per-user Regulation ledger | `hkask-regulation::RegulationLedger` | Per-user variety counters |
| Per-user storage | `hkask-storage` | Dedicated SQLCipher file per user at `{data_dir}/agents/{sanitized_name}/pod.db` |
| In-process sovereignty gate | `hkask-types::visibility` + zed-kask agent loop | Per-user capability + sovereignty enforcement; no cross-user handle |

### What Happens When Violated

**Cross-user dispatch is structurally impossible.** A user/curator data directory has no reference to another directory's MCP runtime, `McpRuntime`, or `CapabilityChecker`. The type system enforces this:

- Each user/curator data directory owns its own scoped MCP runtime (not `Arc`-shared across users)
- The in-process sovereignty gate is constructed from a single user's data directory — it cannot reach another user's tools
- Tool invocation routes through the user's own `governed_tool`, never another user's

### How to Audit

The deleted `kask pod` CLI has been replaced by the in-process kask panel (D10).

```bash
# List all active user/curator data directories (kask panel, D10 → Users tab)
#   Equivalent in-process call: enumerate active user data directories

# Inspect a user's tool bindings (kask panel, D10 → Users tab → select user)
#   Equivalent in-process call: inspect the user's scoped MCP runtime bindings

# Verify Regulation isolation (per-user variety counters)
#   Check reg.* spans for user_id prefix via the kask panel or RegulationLedger::variety()
```

---

## Audit Commands

### Magna Carta Verification

The `magna-carta-verifier` skill runs structural audits against the codebase, loaded from
`.agents/skills/magna-carta-verifier/manifests/`. The deleted `kask sovereignty verify` CLI command
has been replaced by invoking this skill through the agent panel or skill registry:

```bash
# Full verification report (magna-carta-verifier skill)
#   Invoke via agent panel or skill registry; no standalone CLI binary.

# Verify a specific principle
#   magna-carta-verifier --principle user_sovereignty
#   magna-carta-verifier --principle affirmative_consent
#   magna-carta-verifier --principle generative_space
#   magna-carta-verifier --principle clear_boundaries

# JSON output for CI/automation
#   magna-carta-verifier --json
```

### Regulation Span Audit

P1–P4 enforcement is observable through Regulation spans. The deleted `kask regulation` CLI has
been replaced by the in-process kask panel (D10) and programmatic `RegulationLedger` queries:

```bash
# View sovereignty-related spans (kask panel, D10 → Regulation tab)
#   Equivalent in-process call: RegulationLedger::alerts().await

# View tool invocation spans (OCAP enforcement)
#   Equivalent in-process call: RegulationLedger::query_algedonic(span=reg.tool.*)

# View P4 startup gate spans
#   Equivalent in-process call: RegulationLedger::query_algedonic(span=reg.mcp.startup.*)
```

### Consent Management

The deleted `kask sovereignty` CLI has been replaced by the in-process kask panel (D10):

```bash
# Grant consent for a data category (kask panel, D10 → Sovereignty tab → Grant)
#   Equivalent in-process call: ConsentManager::grant_consent(webid, category, agent)

# Revoke consent (kask panel, D10 → Sovereignty tab → Revoke)
#   Equivalent in-process call: ConsentManager::revoke_consent(webid, category)

# Check current consent state (kask panel, D10 → Sovereignty tab → Status)
#   Equivalent in-process call: ConsentManager::has_consent(webid, category)
```

---

## Enforcement Trace Summary

| Principle | Prohibition Level | Primary Enforcement | Fail-Closed? | Regulation Spans |
|-----------|------------------|--------------------|--------------|-----------|
| P1 (Sovereignty) | Prohibition | `SovereigntyChecker::can_access()` + `require_sovereignty()` | Yes | `reg.sovereignty` |
| P2 (Consent) | Prohibition | `ConsentManager::has_consent()` (`unwrap_or(false)`) | Yes | `reg.sovereignty` |
| P3 (Generative) | Guardrail | `hkask-guard` (floor); no admin bypass (ceiling) | N/A (guardrail) | `reg.guard` |
| P4 (OCAP) | Prohibition | `CapabilityChecker::verify()` + `GovernedTool::invoke()` | Yes (empty roots) | `reg.tool` |
| P4.1 (Per-User Boundary) | Prohibition (structural) | Per-user data directory isolation + scoped MCP runtime | Always (type system) | Per-user `user_id` in spans |

### Failure Modes

| Scenario | Behaviour | Error |
|----------|-----------|-------|
| No `SovereigntyChecker` wired | All access denied | `SovereigntyDenied` |
| No `CapabilityChecker` wired | All tool calls denied | `CapabilityDenied` |
| No `ConsentManager` wired | All consent checks fail | `DenyAllConsent` returns `false` |
| Storage error in consent check | Consent denied | `unwrap_or(false)` |
| Token signature invalid | Tool call rejected | `ToolPortError::CapabilityDenied` |
| Token expired | Tool call rejected | `CapabilityChecker::verify_with_time()` returns `false` |
| Gas budget exhausted | Tool call rejected | `ToolPortError::EnergyBudgetExceeded` |
| Gate 1 (auth) fails | MCP server refuses to start | `McpError::Auth` |
| Gate 2 (assignment) fails | MCP server refuses to start | `McpError::RoleAssignment` |
| Gate 3 (capability denied) | Server starts, denied tools unavailable | `StartupGateResult::denied_tools` non-empty |
