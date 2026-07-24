---
title: "The Magna Carta of hKask"
audience: [architects, users, agents]
last_updated: 2026-06-28
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# The Magna Carta of hKask

## ℏKask v0.31.0 — A Sovereign Chat Client for Human Users with AI Tools

**User Sovereignty is Non-Negotiable.**

---

## Contents

| Section | Description |
|---------|-------------|
| [The Contract](#the-contract) | Core principles of user sovereignty |
| [Principle 1: User Sovereignty](#principle-1-user-sovereignty) | SOLID-grounded data ownership and atomic consent |
| [Principle 2: Affirmative Consent](#principle-2-affirmative-consent) | Default deny, scoped consent, fail-closed |
| [Principle 3: Generative Space](#principle-3-generative-space) | Settings exposure, user curation, open-source commitment |
| [Principle 4: Clear Boundaries](#principle-4-clear-boundaries-ocap) | OCAP enforcement of principles 1–3 |
| [Catch and Release](#catch-and-release) | Data sovereignty catch-and-release model |
| [The Curator as Enforcer](#the-curator-as-enforcer) | Curator role in enforcing the Magna Carta |
| [Regulation Integration](#regulation-integration) | Algedonic alerts and sovereignty monitoring |
| [Magna Carta Verifier](#magna-carta-verifier) | Verification skill, triggers, and resolution |
| [Implementation](#implementation) | Code-level enforcement mechanisms |
| [The Promise](#the-promise) | The pledge to users |
| [Enforcement](#enforcement) | Runtime enforcement and audit |
| [References](#references) | Citations and references |
| [Version](#version) | Document version history |

---

## The Contract

hKask operates under a Magna Carta — a charter of liberties that honors user sovereignty above all else. This is not a feature. This is the foundation.

### Core Principles

1. **User Sovereignty** — Data is owned by the user, correctly categorized, portable, and consent is atomic. Grounded in Berners-Lee's SOLID architecture principles.[^solid]
2. **Affirmative Consent** — Default is deny. Nothing passes without an explicit yes. Consent is scoped, versioned, and expiring.
3. **Generative Space** — Within boundaries, hKask is maximally generative. Inference and tooling expose all probabilistic/generative settings to users. No privileged engineer access. Open-source only.
4. **Clear Boundaries (OCAP)** — Principles 1–3 are enforced through explicit OCAP boundaries. Every agent, pod, and template invocation operates within unforgeable capability tokens.[^miller-ocap]

---

## Principle 1: User Sovereignty

Grounded in the SOLID architecture principles[^solid]: true data ownership, fine-grained access control, no implicit sharing, and interoperability.

### Data Sovereignty Boundary

Data sovereignty boundaries implement the principle of informational self-determination:[^westin-data]

```rust
pub struct DataSovereigntyBoundary {
    pub sovereign_data: HashSet<DataCategory>,    // User controls
    pub shared_data: HashSet<DataCategory>,       // Explicit consent required
    pub public_data: HashSet<DataCategory>,       // No sovereignty claim
    pub(crate) requires_affirmative_consent: bool,
}
```

**Default hKask Configuration:**
- **Sovereign:** episodic_memory, personal_context, capability_tokens, ocap_boundaries
- **Shared:** semantic_memory, template_invocations
- **Public:** template_registry

### SOLID Alignment

| SOLID Invariant | hKask Implementation |
|---|---|
| True data ownership | SQLCipher-encrypted local store, `WebID`-scoped access |
| Fine-grained access control | `DataSovereigntyBoundary` with per-category sovereign/shared/public |
| No implicit sharing | `SovereigntyChecker::can_access()` + `SovereigntyConsent` port |
| Interoperability & portability | Open template registry, standard export formats |

### Atomic Consent

Consent decisions must be unbundled. Each term is a separate, specific consent decision. No bundling multiple complex decisions into a single "I agree." Each consent term must be described in no more than 5 sentences or a standard paragraph.

This is a structural requirement: the composition of consent terms is a sovereignty right. The user has the right to consent to each term individually. This is distinct from the ongoing affirmation of those terms, which is covered by Principle 2.

### Resource Verification

Every resource added to the platform undergoes an initial verification that it is correctly categorized (sovereign/shared/public) and gated at the appropriate level. Ongoing verification is a re-check of that initial verification, not a new analysis. When the set of resources grows large, verification is batched by category.

### Data Portability

Sovereign data must be exportable and not locked into a proprietary format. The verification manifest asserts that export paths exist and produce standard formats.

---

## Principle 2: Affirmative Consent

Default is deny. Nothing passes without an explicit yes. Consent is not a one-time checkbox — it is ongoing.

### Affirmative Consent Model

The runtime type is a `bool` (`requires_affirmative_consent: bool`); the `DataSovereigntyBoundary::hkask_default()` sets it to `true`, satisfying the "default deny" charter.

```rust
pub struct DataSovereigntyBoundary {
    // ...sovereign_data, shared_data, public_data...
    pub(crate) requires_affirmative_consent: bool,
}

impl DataSovereigntyBoundary {
    pub fn requires_affirmative_consent(&self) -> bool {
        self.requires_affirmative_consent
    }
}
```

The name "Affirmative Consent" describes what the system *does* — require explicit affirmative consent. The default is deny, consent is required.

### Consent Scope, Versioning, and Expiration

Consent grants are not indefinite blanket permissions. Each consent grant is:

- **Scoped** to specific categories and resource versions
- **Version-bound** — consent must be re-affirmed when a resource used in a category is upgraded to a new version
- **Time-bound** — consent grants can have expiration dates and must be re-affirmed at expiration

When categories or resources change, existing consent grants for those categories are invalidated and must be re-granted.

### Hierarchical Consent Structures

A human user may define consent structures at different granularities:

| Level | Description |
|---|---|
| Master consent | Covers all agents for the user |
| Per-agent consent | Specific to a single agent |
| Per-agent-type consent | One structure for bots (A2A interaction), another for userpods (H2A bridging) |

Most-specific grant wins. The verification manifest asserts that consent resolution follows this hierarchy.

### Fail-Closed Default

`DenyAllConsent` is the default implementation — it denies everything until explicitly granted. If the consent port is misconfigured or missing, the system denies all access. Sovereignty must fail closed.

---

## Principle 3: Generative Space

Within boundaries, hKask is maximally generative. This is not a ban on constraints — it is a commitment to exposing all options and allowing the user to curate their own experience.

### Settings Exposure

Inference and tooling must expose all probabilistic/generative settings to users — temperature, top-k, top-p, repeat penalty, and any other parameters the underlying model or tool supports. No settings are hidden or admin-gated. This is why hKask uses an inference router supporting DeepInfra, Together AI, fal.ai, and OpenRouter.

### No Privileged Engineer Access

Internal engineers and users must have equal access to generative settings. There is no "engineer mode" that exposes more options than what is available to users. The principle is: if an internal engineer can adjust a parameter, the user can too.

### Open-Source Commitment

Generativity requires that resource providers expose their weights and settings options to users in the same way they expose them to their internal engineers. Closed-weight and closed-code projects cannot satisfy this requirement — the decision to be closed makes sovereignty, consent, and generativity impossible to verify. hKask is fundamentally limited to partnering with and connecting to open-source projects.

### User Curation, Not System Imposition

Constraints are user-curated, not system-imposed. The user selects and adjusts these tools.

### Non-Normativity

User preferences are inherently idiosyncratic and diverge from LLM aggregate defaults. The system does not force alignment toward aggregate norms. One of the hardest elements of the alignment problem is the difference between the user's first-person perspective and the LLM's third-person aggregate design. Non-normativity means the user's first-person perspective takes precedence over the LLM's default programming.

---

## Principle 4: Clear Boundaries (OCAP)

Principles 1–3 are enforced through Object Capability (OCAP) boundaries. Every agent, pod, and template invocation operates within explicit, unforgeable capability tokens.

### Dual Enforcement Gate

Every resource access in hKask passes through two gates:

1. **`require_capability`** — Verify that the caller holds an unforgeable capability token for the requested operation
2. **`require_sovereignty`** — Verify that the data category access is permitted by the user's sovereignty boundary and explicit consent

There is no bypass. No code path can access resources without going through both gates.

### Token Properties

- **Unforgeable** — Capability tokens cannot be created from nothing. They can only be delegated by a holder.
- **Attenuating** — Delegation can only reduce permissions, never increase them. A delegated token has equal or fewer permissions than the granter's token.
- **No admin override** — There is no "god token" or admin bypass. All access goes through the same gates.

### OCAP and Generative Access

The capability tokens for generative settings (P3) are obtained through the affirmative consent process (P2). OCAP gates everything, but P3 ensures the gates for generative settings are equally and transparently accessible through the consent hierarchy. No special role or elevated capability is required beyond what P2's affirmative consent provides.

### Verification as Holistic Enforcement

Principle 4 is verified by checking that P1–P3 are correctly implemented as OCAP boundaries. This is the structural audit that confirms the gates exist, are not bypassable, and that tokens are unforgeable and attenuating.

---

## Catch and Release

| Catch | Release |
|-------|---------|
| OCAP boundaries | Generative template space |
| Sovereignty enforcement | High-temp anti-normative generation |
| Affirmative consent | User-curated experience |
| Variety monitoring | Clean, merged code |
| Algedonic alerts | Tools for user sovereignty |

**The Catch:** We create boundaries that protect user sovereignty.

**The Release:** Within those boundaries, we provide the most generative tool surface possible — skills, MCP servers, and LLM access for human users.

The catch-and-release dialectic mirrors the Viable System Model's balance between regulation and autonomy:[^beer-vsm]

This is not a contradiction. This is the core.

---

## The Curator as Enforcer

The Curator is not just a quality gate. The Curator is the Magna Carta enforcer, maintaining requisite variety through curation decisions:[^ashby-law]

### Curator Responsibilities

1. **OCAP Verification** — Verify capability tokens before any action
2. **Sovereignty Checking** — Ensure user sovereignty is not compromised
3. **Consent Verification** — Verify that affirmative consent is granted and current
4. **Variety Tracking** — Monitor Regulation variety counter
5. **Algedonic Alerts** — Trigger alerts when:
   - Variety deficit > 100
   - Sovereignty compromised
   - Consent violation detected
6. **Magna Carta Verification** — Review and resolve verification findings with the human user or the user's userpod

### Curation Decisions

| Decision | Meaning | Sovereignty Impact |
|----------|---------|-------------------|
| Merge | Output is valid | Increases variety (good) |
| Discard | Output broken | Maintains variety |
| Revise | Needs work | Decreases variety (delay) |
| Defer | More info needed | Decreases variety (delay) |

---

## Regulation Integration

The Cybernetic Nervous System monitors, providing algedonic signaling from the Viable System Model:[^beer-vsm]

1. **Variety Counter** — Tracks code generation diversity
2. **Sovereignty Alerts** — Enforces Magna Carta
3. **Consent Alerts** — Tracks consent scope, version, and expiration

**Algedonic Alert Threshold:** Variety deficit > 100

When triggered, the Curator escalates to:
- The human user or the user's userpod (via the Curator chat session)
- System administrator
- External audit trail

---

## Magna Carta Verifier

The Magna Carta Verifier is a skill that verifies each principle using YAML manifests and Jinja2 templates. It is part of the hKask verification infrastructure, anchored to the principles for stability as implementations evolve.

### Skill Structure

```
.agents/skills/magna-carta-verifier/
  SKILL.md                              # Skill definition, triggers, resolution process
  manifests/
    p1-user-sovereignty.yaml             # Assertions for User Sovereignty
    p2-affirmative-consent.yaml          # Assertions for Affirmative Consent
    p3-generative-space.yaml             # Assertions for Generative Space
    p4-clear-boundaries.yaml             # Assertions for OCAP boundary verification
  templates/
    verification-procedure.md.j2         # How to verify each assertion
    verification-report.md.j2            # Findings, gaps, status
    test-case.rs.j2                      # Rust test cases rendered as code blocks
```

### Manifest Structure

Each manifest declares assertions anchored to a principle:

```yaml
principle: user_sovereignty  # or affirmative_consent, generative_space, clear_boundaries
version: "0.1.0"
description: "..."

assertions:
  - id: p1a
    name: sovereign_data_gated
    claim: "Every code path to sovereign data is gated by SovereigntyChecker"
    method: structural_audit  # or behavioral_probe, resource_verification, absence_check
    targets:
      - crate: hkask-pods
        module: pod::context
        methods: [store_episodic, recall_episodic, store_semantic, recall_semantic]
        gate: require_sovereignty
```

### Verification Methods

| Method | Description |
|--------|-------------|
| `structural_audit` | Enumerate access paths and verify gates exist |
| `behavioral_probe` | Generate access attempts and verify denial |
| `resource_verification` | Verify resource categorization at onboarding; re-check on change |
| `absence_check` | Verify that prohibited constructs (hidden gates, admin overrides) do not exist |

### Assertion Summary

| ID | Principle | Assertion | Method |
|----|-----------|-----------|--------|
| p1a | User Sovereignty | Every code path to sovereign data is gated by `SovereigntyChecker` | Structural audit |
| p1b | User Sovereignty | Non-owner access to sovereign data is denied | Behavioral probes |
| p1c | User Sovereignty | Every resource is correctly categorized before platform entry | Resource verification |
| p1d | User Sovereignty | Sovereign data is portable and not locked into proprietary format | Structural audit |
| p1e | User Sovereignty | Consent terms are atomic — unbundled, specific, ≤5 sentences per term | Structural audit |
| p2a | Affirmative Consent | Default is deny — no access without explicit consent grant | Structural + behavioral |
| p2b | Affirmative Consent | Consent grants are scoped to specific categories and resource versions | Structural |
| p2c | Affirmative Consent | Consent grants expire by date or resource version upgrade | Structural + behavioral |
| p2d | Affirmative Consent | Consent structures are hierarchical (master → per-agent → per-agent-type) | Structural |
| p2e | Affirmative Consent | Fail-closed: misconfiguration or missing wiring defaults to deny | Behavioral |
| p3a | Generative Space | Inference and tooling expose all probabilistic/generative settings to users | Structural |
| p3b | Generative Space | Internal engineers and users have equal access to generative settings | Absence check |
| p3c | Generative Space | Generative resources are open-source with exposed weights and settings | Structural + behavioral |
| p3e | Generative Space | User preference overrides take precedence over LLM aggregate defaults | Absence check |
| p4a | Clear Boundaries | Every access path goes through `require_capability` + `require_sovereignty` | Structural + behavioral |
| p4b | Clear Boundaries | Capability tokens are unforgeable and attenuating — no bypass exists | Structural |
| p4c | Clear Boundaries | Generative settings tokens obtainable through P2's affirmative consent | Structural |
| p4d | Clear Boundaries | Connected inference providers expose settings (open-source requirement) | Structural |

### Triggers

Verification is triggered by:

| Trigger | When |
|---------|------|
| Start-up | Verification runs when hKask starts |
| Expiration | Consent grants expire → re-verification scheduled |
| User change | New consent, settings change, new API key → re-verify affected assertions |
| Resource/service change | New version of MCP server, inference provider, or model → re-verify affected assertions |

### Resolution Process

When an assertion fails, the verification report is escalated to the Curator. The Curator reviews the finding with the human user or the user's userpod in a chat session. The resolution process is defined by the user in collaboration with the Curator — the user instructs the Curator on how to resolve issues, and the Curator follows that process.

---

## Implementation

### Sovereignty State Tracking

Sovereignty state tracking implements privacy-by-design principles:[^solove-taxonomy]

```rust
pub struct UserSovereigntyState {
    pub boundary: DataSovereigntyBoundary,
    pub explicit_consent: bool,
    pub last_check: chrono::DateTime<chrono::Utc>,
}
```

### Curator Pipeline Integration

The `DefaultSpecCurator` is the curator that enforces the Magna Carta. It
records sovereignty checks as `reg.sovereignty.checked` `RegulationRecord`s when an
event sink is wired. The agent-pod `SovereigntyChecker` enforces the
sovereignty policy on every memory access.

```rust
// In hkask-pods::curator_agent::DefaultSpecCurator
impl DefaultSpecCurator {
    /// Record a sovereignty check for a spec evaluation.
    /// Emits a `reg.sovereignty.checked` RegulationRecord (CyclePhase::Compare).
    pub fn check_sovereignty(&self, spec_id: &str, categories: &[String]) { /* ... */ }
}

// In hkask-pods::pod::PodContext
impl PodContext {
    /// Enforce the Magna Carta's data-sovereignty policy on access.
    /// Complements `require_capability` (OCAP) with the data-class policy.
    pub fn require_sovereignty(
        &self,
        category: &DataCategory,
        requester: &WebID,
    ) -> Result<(), AgentPodError> { /* ... */ }
}
```

---

## The Promise

**To Users:** Your sovereignty is non-negotiable. Your data is yours. Your agents serve you. You consent to each term individually — no bundling, no hidden terms, no indefinite grants.[^westin-data]

**To Builders:** Within these boundaries, build freely. All settings are exposed. All tools are available. User-curated, not system-imposed.

**To Acquirers:** Affirmative consent is required. Consent must be explicit, scoped, versioned, and expiring. No speculative judgment.

---

## Enforcement

The Magna Carta is not aspirational. It is enforced:

1. **OCAP Boundaries** — Capability tokens verify authority[^miller-ocap]
2. **Sovereignty Checks** — Every invocation checked
3. **Consent Verification** — Scoped, versioned, expiring consent
4. **Regulation Alerts** — Violations trigger immediate alerts
5. **Magna Carta Verifier** — YAML manifests and Jinja2 templates verify each principle. Invoked via `kask sovereignty verify` (CLI) or `reg_verify_magna_carta` (MCP tool)
6. **Audit Trail** — All decisions recorded

---

## References

[^solid]: Berners-Lee, T. (2018). *SOLID: Social Linked Data*. https://solidproject.org/
[^beer-vsm]: Beer, S. (1972). *Brain of the Firm*. Penguin Books. Viable System Model, algedonic alerts.
[^ashby-law]: Ashby, W. R. (1956). *An Introduction to Cybernetics*. Chapman & Hall. Law of Requisite Variety.
[^miller-ocap]: Miller, M. S. (2006). *Robust composition: Towards a unified approach to access control and concurrency control* [Doctoral dissertation, Johns Hopkins University].
[^westin-data]: Westin, A. F. (1967). *Privacy and Freedom*. Atheneum. Foundational framework for data sovereignty and informational self-determination.
[^solove-taxonomy]: Solove, D. J. (2006). A taxonomy of privacy. *University of Pennsylvania Law Review*, 154(3), 477–560. https://doi.org/10.2307/40041379

---



## Version

ℏKask v0.31.0 — A Sovereign Chat Client for Human Users

*As simple as possible, but no simpler.*

*Rust is the loom. YAML is the thread. Sovereignty is the foundation.*