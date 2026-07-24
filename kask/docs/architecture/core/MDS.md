---
title: "MDS — Minimal Domain Specification"
audience: [architects, developers, agents]
last_updated: 2026-07-12
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# MDS — Minimal Domain Specification

**Purpose:** A minimal, capability-driven specification framework for hKask. Specs are grants ("CAN verb on resource via interface"), not fences ("MUST NOT"). Five categories, five tools, one completeness predicate.

**Supersedes:** The previous 9-category DDMVSS. All MDS references in the codebase should be updated.

**Related:** [`PRINCIPLES.md`](PRINCIPLES.md), [`magna-carta.md`](magna-carta.md), [`FUNCTIONAL_SPECIFICATION.md`](FUNCTIONAL_SPECIFICATION.md)

---

## 1. Domain Ontology

The domain ontology is grounded in **Ontology Design Pattern (ODP) methodology** as described by Norouzi et al. (2025, arXiv:2509.23776): compact, requirement-driven extraction patterns rather than navigating entire complex ontologies.[^norouzi-odp]

### 1.1 Core Entities

| Entity | Crate | Description | Goal Principle |
|--------|-------|-------------|---------------|
| `HumanUser` | `hkask-storage` | Human identity with WebID, role (Admin\|Member), OAuth provider link | P1 |
| `UserPod` | `hkask-types` | Agent identity with persona, voice, wallet link | P6 |
| `AgentPod` | `hkask-pods` | Runtime container for a userpod (Inactive\|Active\|ServerMode) | P1 |
| `Wallet` | `hkask-wallet` | rJoule balance, encumbrance, multi-chain deposits | P9 |
| `ApiKey` | `hkask-wallet` | Scoped API key with spending limits and expiry | P1 |
| `hMem` | `hkask-storage` | Entity-Attribute-Value knowledge representation, bitemporal | P3 |
| `RegulationLedger` | `hkask-regulation` | Cybernetic nervous system — variety monitoring, alerts, gas budgets | P9 |
| `GasBudget` | `hkask-regulation` | Per-agent gas budget with cap, replenish rate, hold-settle pattern | P9 |
| `CircuitBreaker` | `hkask-regulation` | Failure-gating state machine for external service calls | P9 |

### 1.2 Kata-Kanban Domain

**Crate:** `hkask-services-kata-kanban` | **Goal Principle:** P3 (Generative Space) — Toyota Kata scientific thinking applied through headless kanban task boards. PDCA phases map to task statuses: Plan→Backlog, Do→InProgress, Check→Review, Act→Done.

| Entity | Description | Key Attributes |
|--------|-------------|---------------|
| `Board` | Named task board scoped to owner WebID | `board_id: BoardId`, `name`, `owner: WebID`, `columns: Vec<ColumnDef>` |
| `ColumnDef` | Ordered column on a board representing a workflow phase | `column_id: ColumnId`, `name`, `status: TaskStatus`, `wip_limit: Option<u32>` |
| `Task` | Unit of work with status lifecycle, priority, verification criteria | `task_id: TaskId`, `title`, `status: TaskStatus`, `priority: Priority`, `owner: WebID`, `board_id: BoardId` |
| `Priority` | Task urgency level | `Low \| Medium \| High \| Critical` |
| `TaskStatus` | Strict column-ordered lifecycle state | `Backlog → Ready → InProgress → Review → Done` |
| `VerificationCriterion` | Acceptance spec with optional LLM evaluation prompt | `description: String`, `llm_prompt: Option<String>` |
| `KataEngine` | Orchestrates kata cycles (starter, improvement, coaching) | `state: KataState`, `manifest: KataManifest` |
| `KataState` | Current state of a kata practice cycle | `step_outputs: HashMap`, `learner_bot: String`, `context: HashMap` |
| `KataManifest` | Declarative definition of a kata (steps, coaching questions, routines) | `manifest: ManifestMeta`, `gas: KataGasConfig`, `steps: Vec<KataStep>` |
| `KataStep` | A single step in a kata improvement cycle | `ordinal: u32`, `action: String`, `description: String` |

**5 coaching kata questions:** (1) Target condition? (2) Actual condition now? (3) What obstacles? Which ONE? (4) Next step? What do you expect? (5) How quickly can we go and see?

**Regulation spans:** `reg.kata` — KataImprovEffectiveness, coaching loop events; `reg.kanban` — TaskCreated, TaskMoved, TaskAssigned, TaskVerified, BoardCreated

**Key contracts:** 34 `KAN-SVC-*` IDs (migration in progress), 27 `P{N}-svc-kata-*` IDs

### 1.3 Adapter Domain

**Crate:** `hkask-mcp-training::adapter` | **Goal Principle:** P3 (Generative Space) — LoRA adapter lifecycle management for agent-specialized inference

| Entity | Description | Key Attributes |
|--------|-------------|---------------|
| `TrainedLoRAAdapter` | A trained LoRA adapter with provenance metadata | `id: Uuid`, `source: AdapterSource`, `checksum: Checksum`, `expertise: Expertise`, `owner: WebID`, `skill_name: Option<String>`, `lifecycle: AdapterLifecycle` |
| `AdapterSource` | Provenance of the adapter | `HuggingFace { repo }` |
| `AdapterStore` | CRUD store for trained adapters with checksum verification | Store, get_by_id, get_by_expertise, get_by_skill_name, list_all, list_owner, delete, store_blob, get_blob |
| `AdapterRouter` | Routes inference requests to the best-matching adapter | CompositionEstimate, provider selection, endpoint guard |
| `EndpointLifecycle` | State machine for inference endpoint lifecycle | `EndpointPhase`: `Cold \| Warming \| Active \| Draining \| Removed` |
| `EndpointPhase` | Lifecycle phase of a deployed inference endpoint | `Cold → Warming → Active → Draining → Removed` |
| `AdapterConfig` | Configuration for adapter deployment | `model_id`, `base_url`, `timeout_secs`, `max_concurrency` |
| `Expertise` | Describes the domain expertise of a trained adapter | `domains: Vec<MdsDomain>`, `provenance: TrainingProvenance`, `capabilities: Vec<String>` |
| `CompositionEstimate` | Cost/time estimate for adapter composition | `estimated_cost_rj: f64`, `estimated_latency_ms: u64` |
| `ProviderSelection` | Selected inference provider for an adapter endpoint | `provider: String`, `model: String`, `cost_per_token_rj: f64` |

**Regulation spans:** `reg.adapter` — AdapterStored, AdapterRetrieved, AdapterDeleted, endpoint lifecycle transitions

**Key contracts:** 44 pub fns with `expect:` + `[P{N}]` annotations

### 1.4 Service Layer Subsystems

**Crate:** `hkask-services-core` + 10 specialized subcrates | **Goal Principle:** P5 (Essentialism) — thin orchestration layer, delegates to domain crates

| Subcrate | Domain | Contract Prefix | Count | Status |
|----------|--------|----------------|-------|--------|
| `hkask-services-core` | Foundation: config, error types, settings | — | — | ✅ Decomposed |
| `hkask-services-chat` | Chat orchestration | — | — | ✅ Decomposed |
| `hkask-services-compose` | Template composition | — | — | ✅ Decomposed |
| `hkask-services-context` | Service context and contract monitoring | `P{N}-svc-context-*` | 31 | ✅ Realigned |
| `hkask-services-corpus` | Content corpus: discovery + embed | `P{N}-svc-corpus-*` | 30 | ✅ Realigned |
| `hkask-services-chat` (metacognition) + `hkask-services-context` (escalation CRUD) | Curation services | — | — | ✅ Deleted 2026-07-11 |
| `hkask-services-kata-kanban` | Toyota Kata + Kanban board coordination | `P{N}-svc-kata-*` / `KAN-SVC-*` | 61 | ⚠️ Migration in progress |
| `hkask-services-onboarding` | Onboarding services | — | — | ✅ Decomposed |
| `hkask-services-runtime` | Runtime services: classify + daemon | `P{N}-svc-runtime-*` | 13 | ✅ Realigned |
| `hkask-services-skill` | Skill management | — | — | ✅ Decomposed |
| `hkask-services-wallet` | Wallet/Payments | — | — | ✅ Decomposed |
| `hkask-inference` | Inference orchestration | `P{N}-svc-inference-*` | 7 | ✅ Realigned |

---

## 2. Five Categories

| # | Category | Completeness Predicate | Min Artifacts | Cross-References |
|---|----------|----------------------|---------------|-----------------|
| 1 | **Domain** | Every entity has a named term and a bounded-context map | Domain ontology sketch | → Composition (verbs), → Lifecycle (persistence) |
| 2 | **Composition** | Every domain verb has a granted composition, registered interface, and composable path | Capability grant table, interface equivalence matrix, registry schema | → Domain (ontology), → Trust (tokens) |
| 3 | **Trust** | Every capability operation has a threat-model entry and an OCAP-bound mitigation | Threat model, keystore config, capability attenuation policy | → Composition (capabilities), → Lifecycle (audit) |
| 4 | **Lifecycle** | Bootstrap, evolution, deprecation, lifecycle, and persistence are expressible as spec transitions | Bootstrap manifest, evolution rules, deprecation policy, Regulation span registry | → Domain (entities), → Trust (audit) |
| 5 | **Curation** | Every spec artifact has been evaluated for coherence by a curator with documented rationale | Curation decision log, coherence score | → Domain (grounding), → Lifecycle (health) |

[^evans-ddd]: Evans, Eric. *Domain-Driven Design: Tackling Complexity in the Heart of Software.* Addison-Wesley, 2003. — Bounded contexts, ubiquitous language, and the domain model that MDS categories extend.

---

## 3. Completeness Predicate

```
complete?(G, category) :=
  ∀ goal ∈ G[category]:
    ∃ criterion ∈ goal.criteria:
      criterion.satisfied = true
  ∧ ∀ cross_ref ∈ G[category].cross_references:
    complete?(G, cross_ref.target_category)

curated?(G) :=
  coherence_score(G.artifacts) ≥ threshold
  ∧ ∀ artifact ∈ G.artifacts:
    curation_decision ∈ {Accept, Revise, Reject}
    ∧ decision.rationale documented
```

A goal-set G is **MDS-complete** iff `complete?(G, c)` holds for all 5 categories **and** `curated?(G)` holds.

Curation decisions (Accept/Revise/Reject) are made by the Curator or human — not by any automated tool. The QA system validates coherence; the Curator makes decisions.

[^hoare-triple]: Hoare, C.A.R. "An Axiomatic Basis for Computer Programming." *Communications of the ACM*, 1969. — The {P} C {Q} Hoare triple that inspires MDS's completeness predicate: precondition → command → postcondition.

---

## 4. Spec Operations & QA Integration

Specifications are managed through CLI and API surfaces, plus QA validation. MCP does not expose spec capture/list/validate/cultivate; it only surfaces spec drift via the Curator server.

### 4.1 CLI Surface (`kask spec`)

Thin passthrough to `SpecStore` in `hkask-storage`. No intermediate service layer — the CLI builds `Spec` domain objects and persists them directly.

| Command | Operation | Delegate |
|---------|-----------|----------|
| `kask spec capture` | Create a spec with name, category, domain, criteria | `SpecStore::save()` |
| `kask spec list` | List specs, optionally filtered by MDS category | `SpecStore::list_all()` / `list_by_category()` |
| `kask spec validate` | Evaluate a single spec via `DefaultSpecCurator::evaluate()` | Curator agent |
| `kask spec cultivate` | Validate + display per-category coherence requirements | Curator agent |
| `kask spec render` | Render a spec through a Jinja2 template | `minijinja` + `SpecStore::load()` |

### 4.2 API Surface

REST endpoints in `hkask-api` read and write specs directly through `AgentService::spec_store()`. Same `SpecStore` backend, same domain types, no service-layer intermediary.

| Endpoint | Operation |
|----------|----------|
| `GET /api/specs` | List specs with optional category filter |
| `GET /api/specs/{id}` | Get spec detail with requirements |
| `POST /api/specs/capture` | Capture a spec from description + context |
| `GET /api/specs/coherence` | Category coverage ratio across all specs |
| `GET /api/specs/{id}/writing-quality` | Structural quality check (name, criteria, completeness) |

### 4.3 QA Integration (planned)

Spec validation, coherence checking, and quality assessment will move into the QA system when `kask qa spec-check` is built. Currently, spec validation runs through `DefaultSpecCurator::evaluate()` directly.

| Command | Operation | Status |
|---------|----------|--------|
| `kask qa spec-check` | Full collection check: category coverage + per-spec quality | Not yet built |
| `kask qa spec-check --spec-id <uuid>` | Single-spec validation via `DefaultSpecCurator::evaluate()` | Not yet built |

### 4.4 Replica Integration (`replica_rewrite`)

The Gentle-Lovelace prose rewriting capability moved to `hkask-mcp-replica` as the `replica_rewrite` tool. It takes a passage/code snippet + quality dimension (gentle/schriver/hopper/lovelace/composite) and delegates to `ComposeService::compose()` with dimension-specific prompts.

| Tool | Server | Description |
|------|--------|-------------|
| `replica_rewrite` | `hkask-mcp-replica` | Rewrite prose optimized for a Gentle Lovelace quality dimension |
| `replica_compose` | `hkask-mcp-replica` | Generate prose in any author's style (underlying engine) |
| `replica_compare` | `hkask-mcp-replica` | Evaluate document against persona centroids (per-dimension scoring) |

### 4.5 The Spec Store

The canonical persistence surface is `hkask_storage::SpecStore` (implemented by `SqliteSpecStore`). All spec operations — CLI, API, and QA — read and write through this single interface. Domain types (`Spec`, `GoalSpec`, `SpecCategory`, `SpecId`) live in `hkask-storage::spec_types`.

```
CLI ──→ SpecStore ──→ SQLite
API ──→ SpecStore ──→ SQLite
QA  ──→ SpecStore ──→ SQLite  (spec-check)
     ──→ DefaultSpecCurator  (validation)
```

---

### 4.6 Replica Server Tools

The replica server provides 9 tools for style corpus management, prose generation, and author comparison:

| Server | Tools | Domain | Status |
|--------|-------|--------|--------|
| `hkask-mcp-replica` | `replica_build`, `replica_compose`, `replica_rewrite`, `replica_mashup`, `replica_compare`, `replica_registry`, `replica_explain`, `replica_discover`, `replica_cache_work` | Style replication + prose rewriting | ✅ Implemented |

### 4.7 Replica Exemplar Architecture

The replica system models a **human exemplar** — a named individual whose body of work constitutes a representational corpus. The logical validity of the replica derives from the relationship between the human and their work: the corpus *is* the evidence of their voice, style, and intellectual framework. Each passage is a sample of that relationship.

**Corpus sources by exemplar type:**

| Exemplar type | Discovery | Source examples | Status |
|--------------|-----------|----------------|--------|
| Public domain author | Static YAML (`works:` list pointing to Gutenberg URLs) | Hemingway, Woolf, Austen, Wilde, Twain, Grant, Christie, Eliot | ✅ Implemented |
| Mashup persona | Two-author centroid interpolation; exemplars drawn from both source corpora | Jane Wilde (Austen×Wilde), Ulysses S. Twain (Grant×Twain), Agatha Eliot (Christie×Eliot) | ✅ Implemented |
| Academic author | Dynamic corpus discovery via research MCP tools; disambiguation required | "David Dunning" → "David Dunning, University of Michigan" | 🔮 Planned |

### Academic Author Pipeline (Planned)

For academic exemplars, the corpus is not statically declared — it is discovered dynamically through the existing research infrastructure. The research MCP server (`hkask-mcp-research`) provides tools that can discover, extract, and cache academic content without replicating infrastructure:

| Research tool | Role in corpus discovery |
|--------------|--------------------------|
| `web_search` | Find the author's papers, talks, interviews, and profiles across the open web |
| `web_extract` | Download full-text content from discovered URLs (papers, transcripts, blog posts) |
| `web_find_similar` | Expand the corpus by finding related work and responses to the author |
| `web_browse` | Navigate academic profiles (Google Scholar, Semantic Scholar, arXiv author pages) to enumerate works |

The planned `replica_discover` tool would orchestrate this pipeline:

1. **Name disambiguation**: Given a name (e.g., "David Dunning"), search academic and open sources, present candidate matches to the Curator for confirmation. This is a consent boundary — the Curator selects *which* David Dunning.
2. **Work enumeration**: From the confirmed identity, enumerate their known works across sources (arXiv, Semantic Scholar, open web, institutional pages, conference proceedings, transcripts).
3. **Content acquisition**: Download and cache each work via `web_extract`, producing `.cache/{slug}.txt` files mirroring the public-domain author pattern.
4. **Corpus config generation**: Produce a `corpus.yaml` with the discovered works, ready for `replica_build`.
5. **Embedding and replication**: Standard pipeline from this point forward — chunk, tag, embed, store hMems, compute centroid.


---

## 5. Capability-Driven Model

MDS is capability-driven, not constraint-driven:

| Aspect | Constraint-Driven | MDS (Capability-Driven) |
|--------|-------------------|-------------------------|
| Spec as | Fence ("MUST NOT") | Grant ("CAN verb on resource via interface") |
| Validation | Static checks, lints | Composability test, POLA audit |
| Growth | Add constraints | Compose capabilities |
| Lifecycle | Governed (gates) | Curated (invitations) |
| Failure mode | Over-constrained | Under-governed |
| hKask alignment | — | OCAP, capability tokens, attenuation |

[^ocap]: Miller, M. (2006). *Robust Composition: Towards a National Research Agenda for Object Capability Security.* HP Labs. — Object capability model: access is granted by possession of a capability token.

---

## 6. MDS Cycle

```
MDS_cycle(S, D) :=
  let spec = capture(D)            // Build Spec from domain description
  store.save(spec)                 // Persist via SpecStore
  curate(spec)                     // Validate via DefaultSpecCurator
  qa spec-check                    // Category coverage + quality gate
  human_or_curator decides:        // External governance
    Accept | Revise | Reject
```

Spec capture and listing go through `SpecStore` directly. Validation and curation delegate to `DefaultSpecCurator`. Collection-wide health checks run through `kask qa spec-check`. Curation decisions remain external.

[^beck-tdd]: Beck, Kent. *Test-Driven Development: By Example.* Addison-Wesley, 2003. — The red-green-refactor cycle that MDS's capture→decompose→validate→curate cycle parallels.

---

## 7. Template Manifests

Each category has a minimal YAML template. All use `schema_version: "0.30.0"`.

### 7.1 Domain Spec Template

```yaml
schema_version: "0.30.0"
category: domain
domain_anchor: hkask
bounded_context: "..."

ontology:
  entities:
    - name: Agent
      attributes: [webid, capabilities, persona]

focusing_assumptions:
  - id: FA-D1
    statement: "..."
    rationale: "..."

completeness_checklist:
  - "Every entity has a named term"
  - "Bounded-context map exists"

cross_references:
  - category: composition
    relation: "Entities expose composable verbs"
  - category: lifecycle
    relation: "Entity state persisted across lifecycle"
```

### 7.2 Composition Spec Template

```yaml
schema_version: "0.30.0"
category: composition
domain_anchor: hkask

verb_inventory:
  - verb: invoke_tool
    resource: McpServer
    interface: [mcp, cli, api]
  - verb: render_template
    resource: Template
    interface: [mcp, cli, api]

interface_equivalence:
  mcp: true
  cli: true
  api: true
  equivalent: true  # All three exercise same functional core

registry:
  type: unified
  discriminator: template_type
  cascade_depth_max: 7

ocap_policy:
  attenuation_max: 7
  token_ttl_seconds: 3600
```

### 7.3 Trust Spec Template

```yaml
schema_version: "0.30.0"
category: trust
domain_anchor: hkask

threat_model:
  adversaries:
    - name: malicious_template_author
      vector: template_injection
      mitigation: `minijinja` Rust sandbox (no filesystem/Python access, unlike Python Jinja2) + capability_gating[^minijinja]
    - name: compromised_dependency
      vector: supply_chain
      mitigation: cargo_deny + pinned_versions

ocap_boundaries:
  - "Every resource access passes through require_capability + require_sovereignty"
  - "Tokens are unforgeable, attenuating, no admin override"

keystore:
  encryption: AES-256-GCM
  key_derivation: Argon2id + HKDF-SHA256
  storage: OS_keychain + SQLCipher
```

### 7.4 Lifecycle Spec Template

```yaml
schema_version: "0.30.0"
category: lifecycle
domain_anchor: hkask

bootstrap:
  sequence: [resolve_secrets, open_databases, build_service_context, start_loops]

evolution:
  versioning: git_sha_only
  migration: "Schema migrations run on version bump"

deprecation:
  policy: "Prefer deletion over deprecation (P5)"

observability:
  reg_spans:
    - namespace: reg.tool
      covers: "Tool invocation governance"
    - namespace: reg.inference
      covers: "Inference budget tracking"
  variety_counters:
    - counter: tool_diversity
      threshold: 50
    - counter: template_diversity
      threshold: 30
  algedonic:
    trigger: "variety_deficit > threshold"
    escalation: "Curator → Human"

persistence:
  engine: SQLite + SQLCipher
  schema: bitemporal_triples
  vector_store: sqlite-vec
  memory_pipelines:
    - name: episodic
      visibility: private
    - name: semantic
      visibility: public
```

### 7.5 Curation Spec Template

```yaml
schema_version: "0.30.0"
category: curation
domain_anchor: hkask

curation_model:
  decisions: [Accept, Revise, Reject]
  curator:
    type: Daemon
    authority: "Human-augmented — curator proposes, human decides"
  guidance: |
    Accept — spec is coherent and complete, publish it.
    Revise — spec needs work, return with rationale.
    Reject — spec is not useful, remove it.

coherence_metric:
  method: "Jaccard similarity of declared vs. registered verbs"
  threshold: 0.7
```

[^fowler-poeaa]: Fowler, M. (2002). *Patterns of Enterprise Application Architecture.* Addison-Wesley. — Template pattern: a standard structure that captures domain knowledge in a reusable form.

---

## 8. Testing Protocol

### Principles

1. **Contract-anchored:** Every test verifies a behavioral contract via `expect:` + `[P{N}]` annotations.
2. **Public seam only:** Tests verify behavior through public interfaces, not implementation.
3. **Tracer bullet:** One RED→GREEN cycle per behavior. No horizontal slicing.
4. **Category coverage:** Each MDS category has at least one integration test.

### Category → Test Strategy

| Category | Test Strategy |
|----------|--------------|
| Domain | Entity definition + term validation |
| Composition | Capability composition + interface equivalence verification |
| Trust | OCAP boundary enforcement + threat model audit |
| Lifecycle | Bootstrap + evolution + deprecation + Regulation span emission |
| Curation | Coherence scoring + decision rationale documentation |

[^principles-p8]: hKask Team. (2026). *Architecture Principles — P8.* `docs/architecture/core/PRINCIPLES.md` (P8) — Every `#[test]` verifies a stated behavioral property of a public seam.

---

## 9. Documentation Structure

> **Incorporated from:** `docs/specifications/specs/MDS_SCAFFOLD.md`

### 9.1 Category → Directory Mapping

Where each MDS category's authoritative documents live:

| # | MDS Category | Primary Directory | Key Documents |
|---|--------------|-------------------|---------------|
| 1 | **Domain** | `architecture/` | MDS.md, FUNCTIONAL_SPECIFICATION.md |
| 2 | **Composition** | `architecture/` | MDS.md, architecture-master.md §Four-Loop Architecture |
| 3 | **Trust** | `architecture/` | magna-carta.md, PRINCIPLES.md |
| 4 | **Lifecycle** | `architecture/` + `plans/` | MDS.md, deployment-and-backup.md |
| 5 | **Curation** | `architecture/` + `specifications/` | WRITING_EXCELLENCE.md, DOCUMENTATION_STANDARDS.md |

**Rule:** New documents go in the directory of their primary MDS category. Cross-cutting documents go in the directory of their dominant category.

### 9.2 Document Lifecycle

```
Draft → Active → Deprecated → Superseded → Removed
```

| State | Rule |
|-------|------|
| **Active** | Must map to ≥1 MDS category via `mds_categories` frontmatter |
| **Deprecated** | Move to `docs/archive/YYYY-MM-DD-<label>/` |
| **Superseded** | Move to archive; successor must reference it |
| **Removed** | `git rm` from working tree; git history is archive of record |

### 9.3 Verification

```bash
bash docs/ci/check-links.sh    # Zero broken cross-references
```

---

## 10. References

[^w3c-rdf]: W3C. (2014). *RDF 1.1 Concepts and Abstract Syntax*. <https://www.w3.org/TR/rdf11-concepts/>.
[^miller-robust]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control*. Johns Hopkins University.
[^cockburn-hexagonal]: Cockburn, A. (2005). *Hexagonal Architecture*. <https://alistair.cockburn.us/hexagonal-architecture/>.
[^shostack-threat]: Shostack, A. (2014). *Threat Modeling: Designing for Security*. Wiley.
[^ronacher-jinja2]: Ronacher, A. (2026). *Jinja2 Template Designer Reference*. <https://jinja.palletsprojects.com/>.
[^norouzi-odp]: Norouzi, M. et al. (2025). "STAR: Seed Terms And Relationships — Ontology Design Pattern Extraction." arXiv:2509.23776.
[^minijinja]: minijinja crate. <https://crates.io/crates/minijinja>. Rust-native Jinja2-compatible template engine with sandbox by default — no Python runtime, no filesystem access, no network access.
[^fowler-strangler]: Fowler, M. (2004). "StranglerFigApplication." martinfowler.com. <https://martinfowler.com/bliki/StranglerFigApplication.html>.
[^conway]: Conway, M. E. (1968). "How Do Committees Invent?" Datamation, 14(4), 28-31.
[^ousterhout]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yaknyam Press.

---

*MDS v0.31.0 — five categories, SpecStore + QA. Five thin service passthroughs removed (contacts, scheduler, goal, pod, spec).*

---

## AgentService Specification

> **Incorporated from:** docs/specifications/specs/MDS-agent-service.md

**Purpose:** Specification for the condensed service layer architecture. `AgentService` is the canonical service layer that owns all shared infrastructure. Both CLI and API surfaces compose an `AgentService` instance and add only presentation-specific state.

### Bounded Context

`AgentService` is the **single source of truth** for all shared infrastructure in hKask. **Boundary:** In-process only. MCP servers do NOT depend on `AgentService` (P1 Prohibition — out-of-process isolation).

All 9 fields are **private** and exposed through **20 public methods** grouped by concern. Four nested sub-context structs (`InfraContext`, `GovernanceContext`, `RegulationContext`, `StorageContext`) consolidate domain-coherent infrastructure; the remaining 5 fields hold cross-cutting state (WebID, curator signal, config, inference loop, governed tool).

#### Sub-Contexts

| Sub-Context | Field | Contains |
|-------------|-------|----------|
| `InfraContext` | `infra` | `inference`, `episodic`, `semantic`, `mcp` (McpRuntime), `pods` (ActivePods), `wallet`, `daemon`, `matrix`, `seams` (SeamWatcher), `wallet_gas` |
| `GovernanceContext` | `governance` | `checker` (CapabilityChecker), `consent` (ConsentManager), `dispatcher` (McpDispatcher), `a2a` (A2ARuntime), `escalations` (EscalationQueue), `events`, `curation_tx` |
| `RegulationContext` | `ledger` | `runtime` (RegulationLedger), `cybernetics` (CyberneticsLoop), `loops` (LoopScheduler), `events` (RegulationSink), `energy` (CalibratedEnergyEstimator), `tool_stats` (ToolStats) |

#### Public Methods (20)

| Method | Returns | Group |
|--------|---------|-------|
| `build(config)` | `Result<Self, ServiceError>` (async) | Construction |
| `infra()` | `&InfraContext` | Context accessor |
| `governance()` | `&GovernanceContext` | Context accessor |
| `ledger()` | `&RegulationContext` | Context accessor |
| `storage()` | `&StorageContext` | Context accessor |
| `config()` | `&ServiceConfig` | Identity |
| `webid()` | `&WebID` | Identity |
| `identity()` | `(&WebID, &Arc<A2ARuntime>)` | Identity |
| `seam_summary()` | `Option<SeamSummary>` (async) | Regulation / Infra |
| `curator_ready()` | `Result<(), String>` (async, `&mut self`) | Regulation / Infra |
| `governed_tool(webid)` | `Arc<GovernedTool<RawMcpToolPort>>` | Regulation / Infra |
| `inference_loop()` | `Option<&Arc<InferenceLoop>>` | Regulation / Infra |
| `set_inference_loop(il)` | `()` (`&mut self`) | Regulation / Infra |
| `inference_port()` | `Option<Arc<dyn InferencePort>>` | Regulation / Infra |
| `gas_remaining()` | `Option<u64>` | Regulation / Infra |
| `gas_cap()` | `Option<u64>` | Regulation / Infra |
| `build_per_agent_memory(db, sink)` | `PerAgentMemory` (assoc. fn) | Memory |
| `per_agent_memory(agent_name)` | `Result<PerAgentMemory, ServiceError>` | Memory |
| `consolidate_agent_memory(agent_name, request)` | `Result<ConsolidationOutcome, ServiceError>` | Memory |
| `consolidation_status_for(agent_name)` | `Result<(usize, usize, usize), ServiceError>` | Memory |

**Design rationale:** The nested sub-context pattern groups domain-coherent infrastructure into deep modules (Ousterhout) — callers access a sub-context once and navigate its public fields, rather than calling individual accessors for every field. Cross-cutting concerns (gas, governed tool, per-agent memory consolidation) remain direct methods on `AgentService` because they span multiple sub-contexts or require coordination logic.

### Service Layer Contracts

| Subcrate | Domain | Contract Prefix | Count | Status |
|----------|--------|----------------|-------|--------|
| `hkask-services-core` | Foundation: config, error types, settings | — | — | Decomposed |
| `hkask-services-chat` | Chat orchestration | — | — | Decomposed |
| `hkask-services-compose` | Template composition | — | — | Decomposed |
| `hkask-services-context` | Service context and contract monitoring | `P{N}-svc-context-*` | 31 | Realigned |
| `hkask-services-corpus` | Content corpus: discovery + embed | `P{N}-svc-corpus-*` | 30 | Realigned |
| `hkask-services-chat` + `hkask-services-context` | Curation services | — | — | Deleted — moved to chat + context |
| `hkask-services-kata-kanban` | Toyota Kata + Kanban board coordination | `P{N}-svc-kata-*` / `KAN-SVC-*` | 61 | Migration in progress |
| `hkask-services-onboarding` | Onboarding services | — | — | Decomposed |
| `hkask-services-runtime` | Runtime services: classify + daemon | `P{N}-svc-runtime-*` | 13 | Realigned |
| `hkask-services-skill` | Skill management | — | — | Decomposed |
| `hkask-services-wallet` | Wallet/Payments | — | — | Decomposed |
| `hkask-inference` | Inference orchestration | `P{N}-svc-inference-*` | 7 | Realigned |

### Crate-to-Domain Mappings

| Crate | MDS Category | Key Entities |
|-------|-------------|-------------|
| `hkask-services-core` | Domain | `ServiceConfig`, error types, settings |
| `hkask-services-chat` | Composition | Chat orchestration, message routing |
| `hkask-services-compose` | Composition | Template composition, rendering |
| `hkask-services-context` | Lifecycle | `ContextService`, contract monitoring |
| `hkask-services-corpus` | Domain | `CorpusService`, embedding pipelines |
| `hkask-services-chat` (metacognition) + `hkask-services-context` (escalation CRUD) | Curation | Curation services, coherence checks — deleted 2026-07-11 |
| `hkask-services-kata-kanban` | Domain, Curation | `KataEngine`, `KataManifest`, `Board`, `Task`, Kanban coordination |
| `hkask-services-onboarding` | Lifecycle | Onboarding workflows, agent setup |
| `hkask-services-runtime` | Lifecycle | `ClassifyService`, `DaemonService` |
| `hkask-services-skill` | Composition | Skill management, capability registry |
| `hkask-services-wallet` | Trust | Wallet service, payment processing |
| `hkask-inference` | Composition | Inference orchestration, provider routing |

### Dependency Direction

```mermaid
graph TD
    CLI["hkask-cli"]
    API["hkask-api"]
    subgraph SERVICES["hkask services subcrates"]
        CORE[core]
        CHAT[chat]
        COMPOSE[compose]
        CONTEXT[context]
        CORPUS[corpus]
        CURATOR[curator]
        KATA[kata-kanban]
        ONBOARD[onboarding]
        RUNTIME[runtime]
        SKILL[skill]
        WALLET[wallet]
    end
    CLI --> SERVICES
    API --> SERVICES
    SERVICES --> AGENTS[hkask-pods]
    SERVICES --> Regulation[hkask-regulation]
    SERVICES --> MEM[hkask-memory]
    SERVICES --> TEMPLATES[hkask-templates]
    SERVICES --> TYPES[hkask-types]
    SERVICES --> STORAGE[hkask-storage]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MDS-001
verified_date: 2026-07-12
verified_against: crates/hkask-services-context/src/context_impl.rs
status: VERIFIED
-->

Domain crates **never** depend on service layer subcrates. MCP servers **never** depend on service layer subcrates for orchestration (P1 Prohibition).

### OCAP Boundaries

| Boundary | Enforcement | Principle |
|----------|-------------|-----------|
| Tool invocation | `CapabilityChecker` gating via `governed_tool` | P4 |
| Inference calls | `governed_inference` membrane with gas budget checks | P4 |
| MCP server isolation | Out-of-process; no `AgentService` dependency | P1 |
| Capability attenuation | Max depth limit, TTL expiry on tokens | P4 |

### Bootstrap Sequence

1. `AgentService::build(config)` assembles all shared infrastructure
2. Per-agent memory created via `build_per_agent_memory(db)`
3. Consolidation is routed through `AgentService::consolidate_agent_memory(agent_name, request)` — the single OCAP-gated, consent-checked entry point
4. CLI surface wraps with `ReplState` (= `AgentService` + REPL fields)
5. API surface wraps with `ApiState` (= `Arc<AgentService>` + HTTP fields)

### Interface Equivalence

Both CLI and API surfaces use identical `AgentService` accessors and the same `consolidate_agent_memory` entry point. All 20 public methods are equivalent across surfaces — surface-specific state (daemon handler, Matrix transport) is accessed via the `infra()` sub-context, not through dedicated accessor methods.
