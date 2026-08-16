---
title: "hKask Architecture Principles"
audience: [architects, developers, agents]
last_updated: 2026-08-04
version: "0.35.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask Architecture Principles

**Purpose:** Twelve principles governing hKask architecture, grounded in the Principle of Least Action (§0). The first four principles are the Magna Carta principles; all remaining principles flow from them. In the contract system (see `zed-host-architecture-plan.md`), each principle can serve as a **goal principle** (driving the explicit user functional expectation of a contract) or a **constraining principle** (shaping how the goal is delivered without overriding it).

**Related:** [`AGENTS.md`](../../../AGENTS.md), [`zed-host-architecture-plan.md`](zed-host-architecture-plan.md), [`TESTING_DISCIPLINE.md`](TESTING_DISCIPLINE.md)

**Cross-reference:** §1.6 Goal Principle Anchoring — see `zed-host-architecture-plan.md` and `TESTING_DISCIPLINE.md` §1.2 `expect:` syntax.

---

## 0. Lazy Grounding: The Principle of Least Action

**hKask is grounded in laziness — the universe's, not ours.**

*Don't just do something, stand there.*

The Principle of Least Action says physical systems evolve through paths that minimize (or make stationary) action. Water, light, orbits, and fields do not "try harder"; they follow the path selected by minimum action.

This is the grounding model for hKask architecture:

1. **Least action is not always the obvious path.** Sometimes the straight line is worse than the cycloid; in architecture, short-term structural work can reduce total long-term complexity.
2. **Stationary action implies robustness.** Good designs tolerate small perturbations without catastrophic behavior.
3. **Global order emerges from local moves.** The system should evolve by disciplined, local, evidence-based changes rather than speculative master-planning.

Everything below is the architectural expression of this lazy-universe grounding.

---

## 1. The Twelve Principles

### 1.1 Magna Carta Principles (Foundational)

#### P1 — User Sovereignty
Users own their data and delegation boundaries. Data categorization, control, and portability are first-class guarantees.

#### P2 — Affirmative Consent
Default is deny. Access requires explicit, scoped, version-aware, and revocable consent.

#### P3 — Generative Space
Within user-defined boundaries, hKask remains maximally generative. No hidden or engineer-only control plane.

**P3.1 — Social Generativity (v0.31.0):** The Generative Space is socially generative — it operates within the social conventions of the jurisdiction where it is used. Criminal or systemically harmful use is not generative; it is destructive to the Generative Space itself. Core content safety controls were previously implemented in `hkask-guard` but the crate was removed 2026-08-10 — the `RoleOverride` scanner's bare `system:` substring match produced false positives that blocked legitimate skill execution. Provider-side safety and refusal fallbacks remain. The former controls were aligned with:

- **OWASP Top 10 for LLM Applications** (primary reference): LLM01 (Prompt Injection), LLM02 (Insecure Output Handling), LLM04 (Model DoS), LLM06 (Sensitive Information Disclosure)
- **NIST AI RMF 1.0** (2023): Technical controls for validity, reliability, security, and resiliency
- **ENISA Multilayer Framework** (2024): Security-by-design for AI systems
- **Martin et al. (2025)** arXiv:2603.29878: Few-shot pattern-based detection as primary defense
- **Zaratiana et al. (2026)** arXiv:2605.07982: Schema-conditioned classification for LLM safeguards

They are the floor, not the ceiling — the Generative Space requires a safe container.

#### P4 — Clear Boundaries
P1–P3 are enforced through explicit capability boundaries: capability *separation* — a list of what a caller may reach, written by someone other than that caller. No ambient authority and no admin bypass. Per **Miller's Object Capability model** (Miller, 2006): no ambient authority; authority only attenuates, never amplifies.[^miller-ocap] hKask takes the separation principle and not the unforgeable-reference mechanism: a per-call token check whose value the caller supplies is not a boundary, which is why the one that existed at `McpRuntime::invoke` was removed on 2026-08-12 (`kask/security/regressions/RR-0056.yaml`).

**P4.1 — Per-User Data Directory as Capability Enforcement Perimeter (v0.31.1, re-anchored):** The per-user data directory IS the enforcement perimeter. Each user's encrypted SQLCipher file (`{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`) is the isolation boundary — no connection handle to another user's file means no cross-user data access is structurally possible. The `hkask-pods` crate (ActivePods, PodDeployment, PodFactory, PodRegistry, `PerPodToolBinding`, etc.) was **deleted** in the 2026-07-25 cleanup; the per-user data directory replaces the pod abstraction as the enforcement perimeter. Tool dispatch is scoped to the active user's MCP server bindings — cross-user dispatch is an invalid state because no user has a handle to another user's data directory. This structural perimeter is the whole of P4.1: it does not depend on any per-call check, and it never did — the in-process capability token this paragraph previously cited was removed on 2026-08-12 as vacuous (RR-0056), and no P4.1 guarantee changed with it.

**P4.2 — Tool authority is separated, not re-checked per call (2026-08-12):** `McpRuntime::invoke` meters and dispatches; it does not authorize. Its `agent: WebID` argument is an accounting identity, and its only pre-dispatch refusal is the runaway-loop call breaker (`EnergyBudgetExceeded`), which is fail-open on an agent the composition root never seeded (RR-0057). Which tools a caller may reach at all is decided at three boundaries whose contents the caller does not write: the per-request `tool_allowlist` on the inference IPC `tool_invoke` dispatch (`kask_bridge/src/inference_ipc_server.rs`, fail-closed on missing or empty), each swarm agent card's declared `mcp_tools` allowlist (`hkask-mcp-swarm/src/agent_executor.rs`), and the per-server MCP env/credential allowlists (`kask_bridge/src/mcp_servers.rs`, RR-0038). There is no fourth gate. Information flow is **not** gated: a FIDES `Source`→`Sink` check in `hkask-templates`'s `invoke_tool` was deleted the same day and for the same reason as the capability gate — both of its inputs were constants (every tool was labelled `Pure`, and the untrusted-input flag read context markers the write path had stopped emitting), so it could not deny. Defense Layer 5 (information flow control) is **absent by decision** (RR-0053, now an absence check), in the same register as Layer 3 (RR-0010). Treat every tool path as taint-unaware.

---

### 1.2 Operational Principles (How We Build)

#### P2.1 — Shared vs Public Visibility (v0.31.0)
Shared data is **consent-bound** and must pass `require_sovereignty` + `require_capability` gates (P2/P4). Public data is **unrestricted** and requires no consent gate. Semantic memory defaults to **Shared**; only explicitly public artifacts (e.g., template registry) use **Public**.

#### P5 — Essentialism & Minimalism
Remove before adding. Every module must earn existence by reducing total system action.

**P5.1 — Single Source of Truth for Skills:** Every skill has exactly one canonical source: its registry crate (`manifest.yaml` + `*.j2` templates). The SKILL.md file is a generated companion for development tooling, derived from the registry — not independently authored. Maintaining parallel representations of the same skill semantics across two formats is a P5 violation. When registry and SKILL.md disagree, the registry is authoritative.

**P5.2 — 5W1H Ontological Core (v0.31.0):** Essentialism requires an anchor. The 5W1H framework — **Who, What, When, Where, Why, How** — is hKask's drop-dead-simple ontological core. Every artifact, module, representation, and claim in hKask must answer at least one of these six questions. An artifact that answers none is ontological noise and fails the minimalism test.

This is not abstract philosophy — it's an operational filter with teeth:

- **Who** — agent (generic), human user, per-user data directory, role, owner (anchored by P12 authenticated host mandate)
- **What** — entity, artifact, resource, data, input, output, state
- **When** — time, sequence, ordering, duration, schedule, temporal scope
- **Where** — location, per-user data directory boundary, namespace, domain, spatial context
- **Why** — goal, purpose, intent, constraint motivation, principle anchoring (anchored by P1–P4 Magna Carta)
- **How** — method, mechanism, procedure, transformation, execution path

The 5W1H core is grounded in Ontology Design Pattern (ODP) methodology as described by Norouzi et al. (2025, arXiv:2509.23776): instead of navigating entire complex ontologies, hKask extracts compact, requirement-driven patterns. The 6 questions are the universal requirements — the minimal set that distinguishes "understood" from "not understood."

**P5.3 — Minimalist Test (the 5W1H gate):** Before any module, type, or abstraction is added, ask: which of the 5W1H does it answer? If the answer is "none," the addition is a P5 violation. If the answer is "it bridges to a domain ontology that answers one," the bridge itself must justify its existence by the same test. Bridges earn their keep by connecting a 5W1H question to domain-specific depth — they are not free passes.

**P5.4 — Dual-Axis Ontological Framework (v0.31.0):** hKask anchors on two complementary ontological axes — no single source of truth, by design.

| Axis | Master Ontology | Question | Domain |
|---|---|---|---|
| **Process (Flow)** | PKO (Procedural Knowledge Ontology) | How did this come to be? What flow is it part of? | Procedures, steps, executions, actions, transformations — the *verb* dimension |
| **State (Entity)** | Dublin Core + BIBO | What is this? What type, who made it, when? | Entities, resources, types, metadata, relationships — the *noun* dimension |

Every artifact in hKask has both a state identity and a process identity — it is simultaneously a noun AND a verb. This is the Planck constant at the architectural level: you cannot reduce one axis to the other. And per Heisenberg, the more precisely you measure state (DC typing), the less you can know about process position (PKO flow), and vice versa. You are always sampling, never arriving at truth. The bridges are sampling instruments, not truth claims.

**Every MCP server uses BOTH axes.** Domain-specific bridges (FIBO, GOLEM, SUMO, ML-Schema, OMC) are layered on top where DC+BIBO's state axis isn't specific enough for a domain. They are NOT alternatives to the dual-axis core — they supplement it.

| MCP Server | Process Axis | State Axis | Domain Bridge |
|---|---|---|---|
| **codegraph** | PKO | DC+BIBO | — |
| **companies** | PKO | DC+BIBO | FIBO (financial concepts) |
| **condenser** | PKO | DC+BIBO | — (DC is the connective tissue for graph saliency) |
| **corpus** | PKO | DC+BIBO | GOLEM (narrative structure, for the replica sub-system) |
| **curator** | PKO | DC+BIBO | — (the curator IS the 5W1H core applied as Socratic inquiry) |
| **kata-kanban** | PKO | DC+BIBO | — |
| **media** | PKO | DC+BIBO | OMC (media creation) |
| **research** | PKO | DC+BIBO | — |
| **scenarios** | PKO | DC+BIBO | — |
| **swarm** | PKO | DC+BIBO | Onto4MAT (multi-agent teaming; Reynolds/Kennedy-Eberhart/Dorigo swarm-intelligence substrate) |
| **training** | PKO | DC+BIBO | ML-Schema (ML experiments) |
| **prediction-markets** | PKO | DC+BIBO | FIBO (financial contracts — CMP economic-object mapping) |

> **Note (v0.31.0, in-process pivot):** The four servers `skill`, `memory`, `communication`, and `filesystem` were deleted. Skill lifecycle is now driven by the in-process skill registry (`kask/registry/` manifests + `hkask-templates`/`ManifestExecutor`); memory is owned by the per-user SQLCipher store; the `communication` server depended on the deleted Matrix transport; filesystem access is mediated by zed's own file I/O surfaces. `docproc` and `replica` were folded into `corpus`. The servers above plus `portfolio` (added 2026-08-12 — provider-agnostic, no ontology mapping) are the surviving set on disk (13 total; curator may be unloaded via `kask.mcp.overrides`); `swarm` was added 2026-08-01 (Agent Bestiary World integration) and `prediction-markets` 2026-08-05. (Corrected 2026-07-29: the prior reference to `crates/hkask-skills` was stale — that crate does not exist; the skill registry lives at `kask/registry/`.)

**Bridge locations (v0.33.0 — single shared crate):**
- Universal axes (DC+BIBO+CiTO state, PKO process) and all domain supplements (FIBO, ESO, GOLEM, OMC, ML-Schema) live in the single shared crate `crates/hkask-bridge-ontology/`. The domain-selection logic (`OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor`) lives in the same crate.
- Architectural invariant (user directive 2026-08-05): ontologies are domain maps; MCP servers are functional-area maps; these are orthogonal. No ontology vocabulary lives inside an MCP server. Every server that does tagging depends on `hkask-bridge-ontology`.
- The former `crates/hkask-bridge-dublincore/` was absorbed into `hkask-bridge-ontology` (rename, not a wrapper — the single-crate design avoids pass-through re-exports). The former server-local bridge modules (`companies/fibo.rs`, `kask/crates/hkask-bridge-ontology/src/{fibo,eso,golem}.rs`, `media/omc.rs`, `training/mlschema.rs`) were deleted; their vocabulary moved to the shared crate, and only server-specific dispatch helpers (e.g. `fmp_field_to_fibo`, `tool_to_omc`) remain in the servers.
- The former condenser-local `OntologyNamespace`/`OntologyAxis`/`OntologyAnchor`/`derive_ontology_anchor` moved to the shared crate's `axis` module; the condenser re-exports them. `derive_ontology_anchor`'s substring-on-tool-names classifier was replaced by `select_ontology_anchor(domain)`, which centralizes the domain-selection logic in one place.

#### P6 — Space for Per-User Data Directories
hKask exists as a generative container for **human user agency** (each user via their own per-user data directory) and **AI tools** (skills + MCP servers), coordinated by the Curator — a native in-process agent (D2) running inside zed-kask, not a daemon — under sovereignty and capability constraints.

**P6.1 — Per-User Data Directory Model (v0.31.1, re-anchored from v0.29.0 Per-UserPod):** Each user inhabits exactly one persistent per-user data directory (1:1; multi-persona removed). The data directory IS the deployment unit — not a cache entry in a shared manager — and persists for the life of the account. A user's data directory owns its SQLCipher file (`{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`), its Regulation runtime (per-user variety counters), and its MCP server bindings (no cross-user dispatch). The per-user data directory makes shared state structurally impossible. The `hkask-pods` crate (ActivePods, PodDeployment, PodFactory, PodRegistry, PodContext, PerPodLedger, LoopScheduler, AgentPod, PodKind, PodLifecycleState) was **deleted** in the 2026-07-25 cleanup; the per-user data directory replaces the pod abstraction. See `zed-host-architecture-plan.md` §13.3 for the composition-root wiring.

#### P7 — Evolutionary Architecture
Types and seams should emerge from real usage, not speculative abstraction.

---

### 1.3 Regulatory Principles (How We Sustain)

#### P8 — Semantic Grounding
System claims must be grounded in traceable, provenance-aware representations.

**P8.1 — Ontological Bridging (v0.31.0):** The 5W1H core (P5.2) is the default grounding level. Anchored beneath it are two complementary ontological axes — no single source of truth, by design.

**Dual-axis grounding:** Every artifact carries both a state identity (DC+BIBO — the noun) and a process identity (PKO — the verb). You cannot reduce one axis to the other, and per Heisenberg, the more precisely you sample one, the less you can know about the other. Bridging is always sampling, never arriving at truth. The bridges are sampling instruments calibrated to universal anchors (PKO namespace, DC namespace) but deployed from domain-specific perspectives.

**Every bridge follows the `fibo.rs` pattern (v0.33.0 — shared-crate variant):**

1. **Concept URI constants** — `pub const CONCEPT_NAME: OntologyConcept = "namespace:LocalName"` — in the shared `hkask-bridge-ontology` crate's domain submodule.
2. **Field-to-concept mapping functions** — `pub fn internal_field_to_ontology(field: &str) -> Option<OntologyConcept>` — server-specific dispatch stays in the server; the vocabulary it references lives in the shared crate.
3. **No dependencies** — the shared crate is pure Rust with zero external crates (vocabulary only); servers depend on it but it depends on nothing.
4. **No reasoners, no OWL parsing, no graph databases** — bridges are thin vocabulary layers, not ontology engines.

**Bridge hierarchy (v0.33.0 — single shared crate):**
- **Universal anchors + domain supplements:** `crates/hkask-bridge-ontology/` — the single shared vocabulary crate. Owns DC+BIBO+CiTO (state axis), PKO (process axis), and all domain supplements (FIBO, ESO, GOLEM, OMC, ML-Schema) as submodules. Also owns the domain-selection logic (`axis` module: `OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor`). Every server that does tagging depends on this crate.
- **Server-specific dispatch:** Servers keep only their own dispatch helpers (mapping their tool names or provider field names to the shared vocabulary) — e.g. `fmp_field_to_fibo` in companies, `tool_to_omc` in media. These are the server's business, not the ontology's.

Bridges use the STAR extraction pattern (seed terms + direct logical entailments, no intermediate hierarchy) from Norouzi et al. (2025). Each bridge module is typically ≤150 lines.

The architectural invariant: **hKask never requires knowledge of a full domain ontology.** All interaction with domain ontologies flows through thin bridges. The dual-axis core (PKO + DC+BIBO) provides the minimum viable ontology for any server; domain bridges are opt-in specificity.

#### P9 — Homeostatic Self-Regulation
The system must remain observable and self-correcting through cybernetic feedback loops.

**§9.1 — Regulation Span Coverage (v0.31.0)**

Regulation (Cybernetic Nervous System) spans are the primary observability primitive. Every subsystem must emit canonical `reg.*` spans for every security-sensitive, resource-sensitive, and correctness-sensitive operation. Essential domains carry typed `RegulationSpan` enum variants (P8 — Semantic Grounding), are registered in `CANONICAL_NAMESPACES`, mapped to a `SpanCategory`, and connected to a cybernetic loop via ν-events. The `reg.*` prefix is reserved for these canonical spans — every `reg.*` tracing target MUST be registered. Performative telemetry (CLI, API middleware, and other observability logs) uses `hkask.*` tracing targets, NOT `reg.*`; those are deliberately NOT registered, NOT categorized, and NOT loop-connected — they are observability logs, not regulated variables. The two are distinguished by registry presence: `SpanNamespace::new` accepts only canonical spans.

**§9.2 — Unified Skill Feedback Standard (v0.31.0)**

Every skill emits cybernetic feedback through exactly one regulated namespace: `reg.skill.<skill-id>.*`. This is the single channel from skills to the Curator and the Regulation nervous system — variety-counted, algedonic-escalated, and comparable across the corpus. The `reg.skill` prefix is registered in `CANONICAL_NAMESPACES`; the hierarchical `is_canonical` function makes `reg.skill.<any-id>.*` valid without per-skill registration.

Every skill emits six semantic spans, one per PDCA phase, mapped to the cybernetic loop:

| Span | Phase | Cybernetic role |
|---|---|---|
| `reg.skill.<id>.classify` | Sense | What is this? |
| `reg.skill.<id>.gather` | Sense | What's missing? |
| `reg.skill.<id>.draft` | Act | Produce artifact |
| `reg.skill.<id>.evaluate` | Check | How good is it? |
| `reg.skill.<id>.convergence` | Check | Are we done? |
| `reg.skill.<id>.write` | Act | Commit artifact |

These six spans are the same for every skill, regardless of domain. The typed enum `SkillFeedbackSpan` (`kask/crates/hkask-regulation/src/skill_span.rs`) encodes them. Fine-grained execution telemetry uses `hkask.template.<skill-id>.*` (performative, unregulated) via the manifest's `telemetry_namespace` field. The `spans:` list in manifest `ledger` blocks is abolished — it was ambiguous and unused by the executor. CI gate: `kask/scripts/check-skill-span-namespace.sh`.

| Domain | Target | Spans | Status | RegulationSpan Variant |
|--------|--------|-------|--------|-----------------|
| Tool dispatch (all MCP servers) | `reg.tool.*` | ~206 (one per tool method, counted via `grep -rn 'Parameters<' kask/mcp-servers/hkask-mcp-*/src/` on 2026-08-01) | ✅ `ToolSpanGuard` per-tool | `Tool { subsystem }` |
| Inference (zed `LanguageModelRegistry` via `LanguageModelInferencePort` in `kask_bridge` — D4) | `reg.inference` | 53 | ✅ generate/generate_vision across whatever providers zed's registry has configured (Anthropic, OpenAI, Ollama, Copilot Chat, Google, Mistral, DeepSeek, etc.) | `Inference` |
| Keystore | `reg.keystore` | 25 | ✅ resolve, store, derive, sign | `Keystore` |
| Adapter (LoRA) | `reg.adapter` | 23 | ✅ store/get_by_id/delete + router | `Adapter` |
| Backup | `reg.backup` | 22 | ✅ snapshot/restore/verify/prune/delete_blob | `Backup` |
| Condenser | `reg.condenser` | 3 | ✅ compression ratio + health | `Condenser` |
| Skill lifecycle | `reg.skill` | 5 | ✅ activate/load/discover/publish/validate | `Skill` |
| MCP server infra | `reg.mcp.*` | 47 | ✅ startup gates + in-process wiring | *(stringly-typed)* |
| Kata coaching | `reg.kata` | 20 | ✅ PDCA cycles, automaticity | `Kata` |
| Agent pod | `reg.agent_pod` | — | ~~✅ revert, spawn_agent (via PodBackupOps)~~ **Removed (v0.31.1):** `hkask-pods` deleted; `PodBackupOps` and the `AgentPod` variant removed. Per-user data directory replaces the pod abstraction. | ~~`AgentPod`~~ (deleted) |
| Wallet | `reg.wallet.*` | — | ✅ pre-existing | `WalletBalance` etc. |
| Memory | `reg.memory.*` | — | ✅ pre-existing | `MemoryEncode` |
| Curation | `reg.curation` | — | ✅ pre-existing | `Curation` |

> **Deleted rows (v0.31.0, in-process pivot):** The `reg.cli` (CLI command dispatch), `reg.api` (API middleware), `reg.deploy` deployment-sessions row, and `reg.deploy` backup-export-lifecycle row are removed. The standalone `kask` CLI is gone (only a slim admin CLI for backup/wallet/repair/admin remains, which emits `hkask.*` performative logs, not registered `reg.*` spans); the HTTP API (`hkask-api`) is deleted; cloud deployment and backup-export lifecycle are deleted. Performative telemetry for the surviving admin CLI uses `hkask.*` tracing targets, NOT registered `reg.*` spans.

**§9.2 — Span Emission Pattern**

```rust
// Regulation span emission — pre: {precondition}, post: reg.{domain} span emitted
tracing::info!(target: "reg.{domain}", operation = "{verb}", {key} = %{value}, ..., "Regulation");
```

- Target: `"reg.{canonical_domain}"` — uses the `reg.*` namespace convention. Essential domains map to `RegulationSpan` variants in `hkask-types::regulation`; performative spans (CLI, API) use stringly-typed tracing targets.
- Message: Must be `"Regulation"` — enables ν-event filtering
- Latency: Use `std::time::Instant`, emit as `latency_ms`
- Authority: Every span carries a `webid` or `owner` WebID

---

### 1.4 Agent Principles (Nature of Agency)

#### P10 — User Agency
Users act as agents in the AI world through their per-user data directory. User agents present in A2A as agents (the generic "agent" concept is preserved); the hKask-specific bot/userpod role taxonomy is removed. User agency is bounded by sovereignty (P1) and capability (P4) — the per-user data directory is the unit of agency, not a separate "userpod" or "bot" role.

#### P11 — Digital Public/Private Sphere
Users, via their per-user data directory, can explicitly control what is private versus shared; visibility is consent-governed. (The generic "agent" concept remains for A2A interop.)

**P11.1 — SQLCipher File as Private Sphere Boundary (v0.29.0):** The per-user data directory's SQLCipher database file IS the private sphere boundary. Each user owns their own encrypted file at `{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`. No cross-user data access is structurally possible — a user cannot accidentally query another user's data because it has no connection handle to that file. Backup IS copying the SQLCipher file. This was already the backup model; the storage layer now matches.

#### P12 — Authenticated Host Mandate
Every action has an accountable host identity. No anonymous agency.

**P12.1 — Surface-Host Mapping (v0.31.0, in-process pivot):**

> **Incorporated from:** `docs/architecture/mandates/P12-authenticated-host-mandate.md`

Every interaction with hKask carries a per-user data directory (or Curator) host identity. After the in-process pivot, there is no standalone CLI, no HTTP API, and no daemon — hKask runs compiled into zed-kask. Four in-process interaction surfaces map to host classes:

| Surface | Host | WebID Source | Storage | Keychain |
|---------|------|-------------|---------|----------|
| **Agent panel** (zed Assistant) | Human user (via per-user data directory) + Curator as a native in-process agent (D2) | zed-kask composition root resolves the active user from `KaskSettings` | `{data_dir}/agents/{sanitized_name}/{sanitized_name}.db` (SQLCipher) | OS keychain via `hkask-keystore` |
| **Kask panel (D10)** | Human user (via per-user data directory) | Same composition-root resolution | Same per-user SQLCipher file | OS keychain via `hkask-keystore` |
| **Kask admin CLI** (slim — backup/wallet/repair/admin only) | Human user (via per-user data directory) | `kask admin` subcommand resolves the user from settings | Same per-user SQLCipher file | OS keychain via `hkask-keystore` |
| **MCP servers** (13, child processes over stdio governed by the in-process `McpRuntime`) | The active per-user data directory | Capability tokens minted at composition-root wiring time | Per-user SQLCipher DB | User-attested HKDF keys |

**Dual-presence pattern:** The agent panel hosts both the user's agent AND the Curator (a native in-process agent, D2) in a single conversation. The user speaks; the Curator observes, surfaces Regulation alerts, provides memory summaries, and can be addressed directly as an agent-panel participant. This is not two separate sessions — it is one conversation with two participants. The user's agent is the sovereign host; the Curator is the system's in-process presence. The old `kask curator chat` REPL command is deleted.

[^dublin-core]: Dublin Core Metadata Initiative. *DCMI Metadata Terms*. ISO 15836. <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>.
[^bibo]: D'Arcus, B. & Giasson, F. *Bibliographic Ontology (BIBO)*. <https://bibliontology.com/>.
[^pko]: Carriero, V. A. et al. (2024). "The Procedural Knowledge Ontology (PKO)." ISWC 2024 / PERKS Project. <https://w3id.org/pko>.
[^miller-ocap]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control*. Johns Hopkins University.

---