---
title: "hKask Architecture Principles"
audience: [architects, developers, agents]
last_updated: 2026-10-08
version: "0.43.1"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask Architecture Principles

**Purpose:** Twelve principles governing hKask architecture, grounded in the Principle of Least Action (§0). The first four principles are the Magna Carta principles; all remaining principles flow from them. In the contract system (see `zed-host-architecture-plan.md`), each principle can serve as a **goal principle** (driving the explicit user functional expectation of a contract) or a **constraining principle** (shaping how the goal is delivered without overriding it).

**Related:** [`AGENTS.md`](../../../../AGENTS.md), [`zed-host-architecture-plan.md`](../zed-host-architecture-plan.md)

**Cross-reference:** §1.6 Goal Principle Anchoring — see `zed-host-architecture-plan.md`.

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

**Values grounding (2026-10-07):** P1–P12 are grounded in the dignity of the human person — the values charter is [`magnifica-humanitas.md`](magnifica-humanitas.md), distilled from *Magnifica Humanitas* (Leo XIV, 2026); the liberties charter is [`magna-carta.md`](magna-carta.md). The charter's eight values: V1 ontological dignity, V2 truth as common good, V3 accountability, V4 subsidiarity, V5 non-neutrality, V6 dignity of work, V7 the limit as positive, V8 universal destination & solidarity. Its discernment test, from the encyclical's own closing question: **does this make human life more human?**

#### P1 — User Sovereignty
Users own their data and delegation boundaries. Data categorization, control, and portability are first-class guarantees.

#### P2 — Affirmative Consent
Default is deny. Access requires explicit, scoped, version-aware, and revocable consent.

#### P3 — Generative Space
Within user-defined boundaries, hKask remains maximally generative. No hidden or engineer-only control plane.

**P3.1 — Social Generativity (v0.31.0):** The Generative Space is socially generative — it operates within the social conventions of the jurisdiction where it is used. Criminal or systemically harmful use is not generative; it is destructive to the Generative Space itself. Provider-side safety and refusal fallbacks remain the active defense. The former controls were aligned with:

- **OWASP Top 10 for LLM Applications** (primary reference): LLM01 (Prompt Injection), LLM02 (Insecure Output Handling), LLM04 (Model DoS), LLM06 (Sensitive Information Disclosure)
- **NIST AI RMF 1.0** (2023): Technical controls for validity, reliability, security, and resiliency
- **ENISA Multilayer Framework** (2024): Security-by-design for AI systems
- **Martin et al. (2025)** arXiv:2603.29878: Few-shot pattern-based detection as primary defense
- **Zaratiana et al. (2026)** arXiv:2605.07982: Schema-conditioned classification for LLM safeguards

They are the floor, not the ceiling — the Generative Space requires a safe container.

#### P4 — Clear Boundaries
P1–P3 are enforced through explicit capability boundaries: capability *separation* — a list of what a caller may reach, written by someone other than that caller. No ambient authority and no admin bypass. Per **Miller's Object Capability model** (Miller, 2006): no ambient authority; authority only attenuates, never amplifies.[^miller-ocap] hKask takes the separation principle and not the unforgeable-reference mechanism: a per-call token check whose value the caller supplies is not a boundary.

**P4.1 — Per-User Data Directory as Capability Enforcement Perimeter (v0.31.1, re-anchored):** The per-user data directory IS the enforcement perimeter. Each user's encrypted SQLCipher file (`{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`) is the isolation boundary — no connection handle to another user's file means no cross-user data access is structurally possible. The `hkask-pods` crate (ActivePods, PodDeployment, PodFactory, PodRegistry, `PerPodToolBinding`, etc.) was **deleted** in the 2026-07-25 cleanup; the per-user data directory replaces the pod abstraction as the enforcement perimeter. Tool dispatch is scoped to the active user's MCP server bindings — cross-user dispatch is an invalid state because no user has a handle to another user's data directory. This structural perimeter is the whole of P4.1: it does not depend on any per-call check.

**P4.2 — Tool authority is separated, not re-checked per call (2026-08-12):** `McpRuntime::invoke` meters and dispatches; it does not authorize. Its `agent: WebID` argument is an accounting identity, and its only pre-dispatch refusal is the runaway-loop call breaker (`EnergyBudgetExceeded`), which is fail-open on an agent the composition root never seeded. Which tools a caller may reach at all is decided at three boundaries whose contents the caller does not write: the per-request `tool_allowlist` on the inference IPC `tool_invoke` dispatch (`kask_bridge/src/inference_ipc_server.rs`, fail-closed on missing or empty), each swarm agent card's declared `mcp_tools` allowlist (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:262-274, 541-547`), and the per-server MCP env/credential allowlists (`kask_bridge/src/mcp_servers.rs`). There is no fourth gate. Information flow is **not** gated: Defense Layer 5 (information flow control) is **absent by decision** (taint check removed: its inputs were constants), in the same register as Layer 3 (no instruction-hierarchy layer, by decision). Treat every tool path as taint-unaware.

---

### 1.2 Operational Principles (How We Build)

#### P2.1 — Shared vs Public Visibility (v0.31.0)
Shared data is **consent-bound** and must pass `require_sovereignty` + `require_capability` gates (P2/P4) — **both OUGHT, not yet implemented** (zero hits in `kask/crates/` as of 2026-09-04; see magna-carta.md IS vs OUGHT Status). Public data is **unrestricted** and requires no consent gate. Semantic memory defaults to **Shared**; only explicitly public artifacts (e.g., template registry) use **Public**. The live per-artifact classification is the `Visibility` enum (`kask/crates/hkask-types/src/visibility.rs:34-39`).

#### P5 — Essentialism & Minimalism
Remove before adding. Every module must earn existence by reducing total system action.

**P5.1 — Single Source of Truth for Skills (updated 2026-09-15):** Every skill has exactly one canonical source: its `SKILL.md` file in `.agents/skills/`. `SkillTool::run` resolves the current catalog and delegates body loading to the project-aware resolver injected at session registration (`crates/agent/src/tools/skill_tool.rs:194-240`; resolver at `crates/agent/src/agent.rs:4641-4680`, registered at `:1029-1031`). Project-local bodies come through project buffers, including unsaved edits and remote workspaces; global bodies come through the filesystem. Slash activation uses the same resolver (`agent.rs:2320-2350`). The `*.j2` files are companion resources retrieved through `render_template`, not a parallel skill specification. 59 skills are authored in `.agents/skills/` (counted 2026-09-28: `find .agents/skills -maxdepth 2 -name SKILL.md | wc -l`), and every one is available to every install and every user; skills and templates are evolving drafts with no release state (operator ruling 2026-09-26). PDCA iteration remains model-coordinated through the resolved body, `lisp_eval`, and `render_template`.

**P5.5 — Done Includes Removal (operator ruling 2026-09-26):** Writing the code is half the task. A code-producing task is done only when the new code is written *and* every excess or obsolete path it supersedes has been removed — the old code, its tests, its settings and its docs — with no compatibility state left behind (there is no backward-compatibility requirement). Without a removal step, a coding task is never done. The definition of done (`onto_anchor` → derived `definition_of_done`) therefore carries three checks: (1) the change names what it replaces, and that code is deleted in the same change; (2) tests of removed behavior are deleted, not kept green; (3) the change is measured — net lines, duplicate paths, surviving references (full-repo sweep of every removed identifier) — and a change that only adds is reported as incomplete. A new instance of an existing pattern is a signal to consolidate the pattern, not copy it (the 2026-09-26 pass replaced four copied process-global recorder hooks with one `RecorderHook`, and two skill-outcome recorders with one). This is P5's "remove before adding" applied to every task, not only to design. Every buildup pass is followed by a cleanup pass over what it touched (operator ruling 2026-09-26): stale citations and comments the buildup moved, test doubles that duplicate existing ones, test-only wrappers that exist only because a seam was missing, and copies that should have been one mechanism. The cleanup pass reports its own net lines; it is part of the work, not optional follow-up.

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

**Every MCP server uses BOTH axes.** Domain-specific bridges (FIBO, GOLEM, SUMO, ML-Schema) are layered on top where DC+BIBO's state axis isn't specific enough for a domain. They are NOT alternatives to the dual-axis core — they supplement it.

| MCP Server | Process Axis | State Axis | Domain Bridge |
|---|---|---|---|
| **companies** | PKO | DC+BIBO | FIBO (financial concepts) |
| **corpus** | PKO | DC+BIBO | GOLEM (narrative structure, for the replica sub-system) |
| **curator** | PKO | DC+BIBO | — (the curator IS the 5W1H core applied as Socratic inquiry) |
| **kata-kanban** | PKO | DC+BIBO | — |
| **portfolio** | PKO | DC+BIBO | — (provider-agnostic, no ontology mapping) |
| **research** | PKO | DC+BIBO | — |
| **scenarios** | PKO | DC+BIBO | — |
| **swarm** | PKO | DC+BIBO | — (no vocabulary bridge module; Onto4MAT[^onto4mat] is cited interpretively in `local_runtime.rs` comments) |
| **training** | PKO | DC+BIBO | ML-Schema (ML experiments) |
| **prediction-markets** | PKO | DC+BIBO | FIBO (financial contracts — CMP economic-object mapping) |
| **media** | PKO | DC+BIBO | OMC (MovieLabs Ontology for Media Creation — `hkask-bridge-ontology/src/omc.rs`; added 2026-08-28) |
| **spreadsheet** | PKO | DC+BIBO | — (no vocabulary bridge module; the LogiSheets deep module in `kask/crates/hkask-spreadsheet`) |

> **Note (v0.31.0, in-process pivot; updated 2026-09-19):** The four servers `skill`, `memory`, `communication`, and `filesystem` are absent. Skill lifecycle uses the in-process `SkillTool` with the shared project-aware body resolver; project-local tool and slash activation therefore see unsaved buffers and remote workspaces consistently (`crates/agent/src/agent.rs:1029-1031,2320-2350,4641-4680`). 68 skills were authored in `.agents/skills/` at that date. Memory is owned by the per-user SQLCipher store; the `communication` server depended on the deleted Matrix transport; filesystem access is mediated by zed's own file I/O surfaces. `docproc` and `replica` were folded into `corpus`. The 13 servers above are the surviving set on disk, registered in `BUILT_IN_MCP_SERVERS` at `kask/crates/kask_bridge/src/mcp_servers.rs:55-572` (curator may be unloaded via `kask.mcp.overrides`); `swarm` was added 2026-08-01, `prediction-markets` 2026-08-05, `media` 2026-08-28, `spreadsheet` 2026-09-18, and `experimentation` 2026-09-30. The condenser library crate (`kask/crates/hkask-condenser`) remains for in-process thread condensation via `kask_bridge::BridgeThreadCondenser`.

**Bridge locations (v0.33.0 — single shared crate):**
- Universal axes (DC+BIBO+CiTO state, PKO process) and published domain vocabularies (FIBO Release, SEPIO, GOLEM, ML-Schema, OMC, W3C RDF Data Cube, SUMO, schema.org) live in the shared crate `kask/crates/hkask-bridge-ontology/`. The domain-selection logic (`OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor`) lives in the same crate.
- Architectural invariant (user directive 2026-08-05): ontologies are domain maps; MCP servers are functional-area maps; these are orthogonal. No ontology vocabulary lives inside an MCP server. Every server that does tagging depends on `hkask-bridge-ontology`.
- The former `crates/hkask-bridge-dublincore/` was absorbed into `hkask-bridge-ontology` (rename, not a wrapper — the single-crate design avoids pass-through re-exports). The former server-local *vocabulary-bearing* bridge modules were deleted; their vocabulary moved to the shared crate (whose `eso.rs` was replaced by `sepio.rs` on 2026-08-29). The server-local files that remain (`companies/src/fibo.rs`, `media/src/omc.rs`, `kata-kanban/src/pko.rs`) are dispatch-only — they re-export the shared crate's verified terms and define zero local ontology constants (verified 2026-09-04).
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
3. **Pinned-source build** — `sha2` verifies each vendored source; `oxrdf`/`oxrdfio` parse RDF during the build. The resolver does not fetch sources at runtime.
4. **No reasoners, no graph databases** — bridges are vocabulary layers, not ontology engines. The full vocabularies are read from pinned sources for their terms, labels, direct parents and published definitions; axioms are never evaluated.

**Bridge hierarchy (v0.33.0 — single shared crate):**
- **Universal anchors + domain supplements:** `crates/hkask-bridge-ontology/` — the single shared vocabulary crate. Owns DC+BIBO+CiTO (state axis), PKO (process axis), published domain vocabularies (FIBO Q2 Release, RDF Data Cube, SEPIO, GOLEM/CIDOC-CRM/LRMoo, OMC, ML-Schema, P-Plan, PROV, RDF/RDFS), full SUMO and schema.org. Also owns the domain-selection logic (`axis` module: `OntologyAxis`, `OntologyNamespace`, `OntologyAnchor`, `select_ontology_anchor`). Every server that does tagging depends on this crate.
- **Server-specific dispatch:** Servers keep only their own dispatch helpers (mapping their tool names or provider field names to the shared vocabulary) — e.g. `fmp_field_to_fibo` in companies. These are the server's business, not the ontology's.

The bridge's named constants are small consumer-facing selections, not the limit of term resolution. The sourced index reads published terms, direct relations and definitions when supplied; it does not run an ontology reasoner.

The architectural invariant (revised 2026-09-27, operator directive "all of the ontologies — not fragments"): **published vocabularies resolve from pinned source distributions, not hand-picked term fixtures**. Current pinned coverage: SUMO, schema.org, DCMI/BIBO/CiTO, PKO/P-Plan/PROV, SEPIO, GOLEM/CIDOC-CRM/LRMoo, OMC, ML-Schema, RDF/RDFS, W3C RDF Data Cube and FIBO's Q2 **Release** maturity set (157 modules, 6,443 terms). FIBO Provisional is not loaded. The local `sdmx:` Information Model aliases and five unlicensed `dlp:` constants were removed; unmatched concepts are not rebranded as `qb:` terms. A source may omit a definition; do not fabricate one. Servers still interact through the bridge's thin API (named constants, `resolve_term`, `canonicalize_terms`); the dual-axis core (PKO + DC+BIBO) remains the artifact anchoring baseline (`kask/crates/hkask-bridge-ontology/sources/SOURCES.lock`, `src/term_resolution.rs`).

**P8.2 — Agent Output Grounding (v0.38.0):** LLM-produced agent output is grounded against a field → tool contract per invocation. The contract declares which output fields must be sourced from successful tool calls vs. inferred by the LLM. Unsourced fields are nulled before the response persists; narrative is scanned for leaked removed values. The six-valued provenance vocabulary (Sourced / Inferred / Derived / UncommissionedInference / Narrative / Unsourced) distinguishes commissioned judgment from uncommissioned inference and platform derivations — do not collapse the latter into Unsourced.

**P8.3 — The Fallback Ladder (v0.39.0):** Ontology anchoring is a scope-broadening walk, never a single pick. When a concept has no fit in the narrowest applicable ontology, the anchor falls to progressively broader scopes until one fits:

1. **Domain supplement** — a pinned published domain term (FIBO Q2 Release first, then W3C RDF Data Cube, OMC, PKO/P-Plan/PROV, SEPIO, GOLEM/CIDOC-CRM/LRMoo, ML-Schema, RDF/RDFS). Never force a concept into an ontology that has no place for it.
2. **Derived concepts** — authority-cited compositions and operator rulings recorded in the derived registry.
3. **Upper ontology** — full pinned SUMO: formal categorization when no domain or derived concept fits.
4. **General vocabulary** — full pinned schema.org, after SUMO so a formal category is preferred.
5. **State-axis published senses** — Dublin Core, BIBO and CiTO after schema.org; these remain visible as alternatives without outranking a formal category. PKO participates in domain term resolution; `select_ontology_anchor` separately selects PKO process and DC state identities for artifacts.
6. **Interrogative ground** — the 5W1H core (P5.2): the guaranteed final rung.

When several rungs publish the same word, the first is the resolution and every other sense is returned alongside it — the ambiguity stays visible, never silently resolved.

The invariant: **nothing is ever untagged.** SUMO and the 5W1H core exist precisely so the ladder always terminates on a real anchor. Skipping rungs to force a fit, or stopping above a rung that fits (emitting no tag), both violate the ladder.

**P8.4 — Entropy-Matched Computation (D/P Routing) (v0.42.0, operator-ratified 2026-09-27):** Every computation step routes to the machine whose conditional entropy matches the task's. Where the answer is pinned and verification is cheap, the step is **D** (deterministic oracle — `lisp_eval`, `lean_check`, `cargo`, server-side oracles such as the `scenario_*` tools). Where knowledge is exhibited only in data, the step is **P** (learned model), split by its collapse path: propose-verify (P step, D gate), explicit probabilistic compute (D server over P inputs), or calibrated forecast (P judgment scored against external ground truth). The boundary is the verification-cost frontier, and it moves: a P step that could be D is a precision-improvement candidate; a D step where the answer has genuine multiplicity is false certainty. A step's regime is part of its provenance — the ex-ante routing face of P8.2's ex-post lattice (`tool_verified > model_inference`). The ruling vocabulary lives in the derived registry (`entropy`, `deterministic_computation`, `probabilistic_computation`, `verification_oracle`, `calibrated_forecast`); the frontier itself (which oracles exist, at what price) lives in prose here and drifts acceptably — the vocabulary must not freeze it.

**Enforcement status (honest, per the advertised-invariants rule):** the D/P labelling convention is adopted forward-looking — every computation-prescribing SKILL.md authored or materially revised carries a D/P labelling section naming each step's regime and oracle. Baseline at adoption (audited 2026-09-27 by filesystem walk): 12 of 55 computation-prescribing skills carried a D/P section; the convergence was completed 2026-09-28 through operator-directed measured batches (12 → 55 of 55 — skills that already carried the substance as Step-types tables were relabelled rather than duplicated), and the forward-looking rule remains for new and materially revised skills. The presence floor is mechanically enforced: check 5 of `kask/scripts/audit/skill-corpus-prescreen.sh` (codified 2026-09-28 from the repeatedly hand-run filesystem walk) flags every computation-prescribing SKILL.md that carries no D/P labelling section. Structural presence is the audit floor, not the ceiling: a section that labels a `lisp_eval`-calling skill "all P" is checkably wrong, and the routing correctness of any given label is P — critiqued by the operator and by the oracle it names.

#### P9 — Homeostatic Self-Regulation
The system must remain observable and self-correcting through cybernetic feedback loops.

**§9.1 — Regulation Span Coverage (v0.31.0)**

Regulation (Cybernetic Nervous System) spans are the primary observability primitive. Every subsystem must emit canonical `reg.*` spans for every security-sensitive, resource-sensitive, and correctness-sensitive operation. Essential domains carry typed `RegulationSpan` enum variants (P8 — Semantic Grounding), are registered in `CANONICAL_NAMESPACES`, mapped to a `SpanCategory`, and connected to a cybernetic loop via ν-events. The `reg.*` prefix is reserved for these canonical spans — every `reg.*` tracing target MUST be registered. Performative telemetry (CLI, API middleware, and other observability logs) uses `hkask.*` tracing targets, NOT `reg.*`; those are deliberately NOT registered, NOT categorized, and NOT loop-connected — they are observability logs, not regulated variables. The two are distinguished by registry presence: `SpanNamespace::new` accepts only canonical spans.

**§9.2 — Skill Outcome Measurement (supersedes the v0.31.0 six-span convention, 2026-09-08)**

Skills are measured by execution outcome, recorded at runtime: `SkillTool::run` fires `agent::record_skill_outcome(skill_id, invoker, success, error)` at its outcome points (`invoker` is the session agent id — `Curator` or `Zed Agent` — or `delegated` for inference-IPC `host/skill`; operator ruling 2026-09-26: capture covers every agent) (rendered envelope → success; missing dependencies or unreadable body → failure). Not-found and authorization-denial are request errors, not skill outcomes — neither is recorded. The composition root (`crates/zed/src/main.rs`) wires the hook to `RegulationLedger::record_skill_span(skill_id, "outcome", payload)`, stored in the bounded `SkillSpanStore` (`kask/crates/hkask-regulation/src/runtime.rs`); the metacognition loop's `sense_feedback_drift` reads the store for per-skill success-rate decline and escalates drift. Pinned by `skill_outcome_recorder_records_and_is_replaceable` + `test_skill_tool_records_outcome` (agent crate).

Skill outcomes and operator feedback both have live writers. `SkillTool::run` records activation outcomes in the bounded `RegulationLedger` working view (`crates/zed/src/main.rs:976-1015, 1692`). The Curator-only `record_skill_feedback` tool is the one writer to the operator-feedback recorder (applying curator advice records no skill verdict), whose composition-root implementation first persists a `reg.skill.<id>.operator_feedback` sense record synchronously to `RegulationArchive` and reports failure to the caller; only after persistence does it enqueue the live ledger update (`crates/zed/src/main.rs:1726-1762`; `kask/crates/kask_bridge/src/memory/curator_stores.rs:49-104`). Startup reloads valid archived observations chronologically into the bounded working view while malformed records remain archived but excluded from drift sensing (`curator_stores.rs:109-156`). The loop measures activation reliability and operator acceptance; it does not infer downstream work quality from activation alone. Skill evaluation is logically separated from skill execution (operator ruling 2026-09-24; Goodhart's law): the operator evaluates skills in the algedonic review with the Curator, so `record_skill_feedback` is registered only in Curator sessions and an executing session can record outcomes and propose changes but never rate a skill or accept its own change (repair plan P6). Tool failures that occur while a skill is active in a thread are captured mechanically as `reg.skill.<id>.tool_failure` (invoker, tool, error; `Thread::run_tool` → `agent::record_skill_tool_failure` → `kask_bridge::persist_skill_tool_failure`), unclassified evidence for the gemba walk. The whole loop — observe, algedonic-review evaluation, proposal, delegated execution, verification — is diagrammed as `DIAG-ARCH-LEARNING-LOOP-001` in [`../../diagrams/architecture.md`](../../diagrams/architecture.md#skill-learning-loop).

| Event surface | Actual emission | Regulation consumption |
|---|---|---|
| MCP tool body | `ToolSpanGuard` emits tracing target `reg.tool` with `tool`, `outcome`, `duration_ms`, `error_kind`, and caller (`kask/crates/hkask-mcp-server/src/server/tool_span.rs:10-27,92-119`) | None directly: child stderr is observability and is not ingested by the Regulation loop (`tool_span.rs:128-131`) |
| Governed MCP dispatch | `McpRuntime::invoke` charges the per-agent call cap and emits cap diagnostics at `reg.mcp.cap` (`kask/crates/hkask-mcp/src/runtime.rs:1545-1590`) | It persists one `RegulationRecord` with `SpanKind::ToolCompleted`; `reg.mcp` is the warning target if persistence fails (`runtime.rs:1598-1612`) |
| Agent-path MCP outcome | Process-global recorder forwards server/tool success and error kind into the shared ledger (`crates/zed/src/main.rs:943-971`) | `ToolReliabilitySensor` reads the ledger outcome breakdown |
| Skill activation | Process-global skill outcome recorder writes `reg.skill.<id>.outcome` payloads (`crates/zed/src/main.rs:976-1015, 1692`) | Metacognition senses per-skill activation reliability |
| Operator skill feedback | The Curator-only `record_skill_feedback` rating uses the process-global feedback recorder; its production implementation persists to `RegulationArchive` before acknowledging the write (`crates/zed/src/main.rs:1726-1762`; `kask/crates/kask_bridge/src/memory/curator_stores.rs:49-104`) | Startup hydration rebuilds the bounded ledger view from valid chronological archive records; metacognition trends operator acceptance (`curator_stores.rs:109-156`) |

**§9.3 — Board-backed algedonic review (2026-09-26)**

An alert delivered to the Algedonic review board creates a card, or adds a
repeat observation to the existing open condition card. Fresh self-recovery is
recorded as evidence and moves the card to Review, never Done. The operator, or
the Curator under a grant recorded on the card, verifies through
`kanban_task_verify`. The seven-day post-advice observation belongs in that
verification evidence, not in a separate queue, receipt or progress score.
Rollout `ImpactReport`s remain a separate measured channel.

**§9.4 — Persistent-condition telemetry coalescing (2026-09-17)**

`LoopMetricsTelemetry` is transition-oriented. A timestamp-free semantic fingerprint covers deviation metric, value, set-point, magnitude, direction, and computed advisories. The first condition and every changed fingerprint emit immediately. Semantically identical scheduled cycles are coalesced; the existing 360-tick hourly boundary emits one `steady_state_heartbeat` carrying `suppressed_steady_state_cycles`. Clearing emits immediately with `condition_cleared`. Rollout impact reports force emission even when the condition fingerprint is unchanged.

This does not change board condition matching, archive retention, or the in-memory alert-log cap. `RegulationArchive` remains time-bounded by maintenance. Coalescing prevents unchanged loop-quality records from consuming the operational read budget. Idle heartbeats remain hourly and continue reporting the alert-log fill state.

**§9.5 — Operational algedonic recency (2026-09-17)**

`curator_algedonic_log` is an incident-response view: within its requested time window and 500-row bound, it returns newest events first and declares `ordering: "newest_first"`. The storage layer keeps this operation separate from chronological `query_algedonic`, so weighted replay and historical processing remain oldest-first. Ordering does not alter category selection, act-phase filtering, archive retention, or the ledger alert-log cap.

**§9.6 — Board-unavailable fallback alert deduplication (updated 2026-09-26)**

When board delivery is unavailable, a successful archive fallback latches the condition locally so an unchanged escalation disposition does not flood history. Failed archive persistence retries; clearing the disposition releases the latch and a recurrence emits again. When the board is readable, its open condition card governs deduplication and repeat observations are comments. Call-cap exhaustion is not coalesced: each alert requires fresh cap exhaustion after the per-tick reset.

> **Deleted rows (v0.31.0, in-process pivot; updated 2026-08-28):** The `reg.cli` (CLI command dispatch), `reg.api` (API middleware), `reg.deploy` deployment-sessions row, and `reg.deploy` backup-export-lifecycle row are removed. The standalone `kask` CLI is gone entirely — no `kask` binary ships (the only bin targets in `kask/` are the 13 MCP server executables plus three tooling binaries — the feature-gated `mcp-test-fixture` test fixture (`kask/crates/hkask-mcp/Cargo.toml:38-41`) and the two `hkask-regulation` checkers `check_principle_constraints` and `check_test_evidence` (`kask/crates/hkask-regulation/src/bin/`); verified 2026-09-04, recount 2026-09-28); the HTTP API (`hkask-api`) is deleted; cloud deployment and backup-export lifecycle are deleted.

**§9.7 — Event emission pattern**

There is no universal tracing-message contract across the process boundary.
Server tool wrappers emit message `REG` at target `reg.tool`; managed runtime
completion persists a typed `RegulationRecord`; cap and persistence diagnostics
use `reg.mcp.cap` and `reg.mcp`. Documentation must name which substrate carries
an event rather than treating every `reg.*` target as a ledger span. Typed
records carry WebID identity; tracing events carry the fields shown in the table
above.

---

### 1.4 Agent Principles (Nature of Agency)

#### P10 — User Agency
Users act as agents in the AI world through their per-user data directory. User agents present in A2A as agents (the generic "agent" concept is preserved); the hKask-specific bot/userpod role taxonomy is removed. User agency is bounded by sovereignty (P1) and capability (P4) — the per-user data directory is the unit of agency, not a separate "userpod" or "bot" role.

#### P11 — Digital Public/Private Sphere
Users, via their per-user data directory, can explicitly control what is private versus shared; visibility is consent-governed. (The generic "agent" concept remains for A2A interop.)

**P11.1 — SQLCipher File as Private Sphere Boundary (v0.29.0):** The per-user data directory's SQLCipher database file IS the private sphere boundary. Each user owns their own encrypted file at `{data_dir}/agents/{sanitized_name}/{sanitized_name}.db`. No cross-user data access is structurally possible — a user cannot accidentally query another user's data because it has no connection handle to that file. Backup IS copying the SQLCipher file. This was already the backup model; the storage layer now matches.

#### P12 — Authenticated Host Mandate
Every action should have an accountable host identity. The current system surfaces,
rather than hides, the two startup fallbacks: the editor proceeds immediately with
agent identity `kask` when the Zed account has not resolved
(`crates/zed/src/main.rs:1577-1592`), while an MCP child with missing or invalid
`HKASK_WEBID` warns and uses the anonymous WebID
(`kask/crates/hkask-mcp-server/src/server/transport.rs:93-105`). The latter is a
degraded attribution state, not a claim of authenticated agency.

**P12.1 — Surface-Host Mapping (v0.31.0, in-process pivot):**

The editor process hosts the Agent panel, Curator, shared Regulation graph, and managed MCP runtime. The 13 MCP servers are child processes over stdio, not a second in-process surface. The former Kask panel and standalone admin CLI are deleted; inline D18 widgets and cross-domain Steer panels are the live interaction surfaces.

| Surface | Host | WebID Source | Storage | Keychain |
|---------|------|-------------|---------|----------|
| **Agent panel and Steer panels** | Human user plus native Curator | Current Zed username when available; nonblocking `kask` fallback at startup (`crates/zed/src/main.rs:1577-1592`) | Internal data root plus visible artifact root by artifact class | `hkask-keystore` uses `oo7` for sovereignty entries |
| **Managed MCP children** | Per-child `ServerContext` | `HKASK_WEBID`, with warning + anonymous fallback when absent/invalid (`kask/crates/hkask-mcp-server/src/server/transport.rs:93-105`) | Per-server/agent DBs and allowlisted artifact routes | Credentials are injected as filtered child env; inference config reads env only |

**Dual-presence pattern:** The agent panel hosts both the user's agent AND the Curator (a native in-process agent, D2) in a single conversation. The user speaks; the Curator observes, surfaces Regulation alerts, provides memory summaries, and can be addressed directly as an agent-panel participant. This is not two separate sessions — it is one conversation with two participants. The user's agent is the sovereign host; the Curator is the system's in-process presence. The old `kask curator chat` REPL command is deleted.

[^dublin-core]: Dublin Core Metadata Initiative. *DCMI Metadata Terms*. ISO 15836. <https://www.dublincore.org/specifications/dublin-core/dcmi-terms/>.
[^bibo]: D'Arcus, B. & Giasson, F. *Bibliographic Ontology (BIBO)*. <https://bibliontology.com/>.
[^pko]: Carriero, V. A. et al. (2024). "The Procedural Knowledge Ontology (PKO)." ISWC 2024 / PERKS Project. <https://w3id.org/pko>.
[^miller-ocap]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control*. Johns Hopkins University.
[^onto4mat]: Hepworth, A. J., Baxter, D. P., & Abbass, H. A. (2022). Onto4MAT: A Swarm Shepherding Ontology for Generalized Multiagent Teaming. *IEEE Access*, 10, 59843–59861. https://doi.org/10.1109/ACCESS.2022.3180032 — prior art for multi-agent teaming concepts; it is not a vocabulary bridge module in `hkask-bridge-ontology`.

---
