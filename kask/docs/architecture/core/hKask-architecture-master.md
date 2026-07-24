---
title: "hKask Architecture Master"
audience: [architects, developers, contributors]
last_updated: 2026-07-23
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask Architecture Master

**Purpose:** Index to the authoritative architecture documents and the four essential architectural patterns that constitute hKask's irreducible core.

**Project:** hKask (ℏKask - A Minimal Viable Container for Users and AI Tools) v0.31.0
**Binary:** `kask`  
**Crate prefix:** `hkask-`

**What it is:** A single hKask install deploys on a cloud server (Kubernetes / K3s) and serves a group of users. Each user gets exactly one userpod - a sovereign container with its own identity, encrypted memory, capabilities, and consent boundary within the shared install - plus AI skills (`.agents/skills/`), 16 MCP servers, and multi-provider LLM access. The deployment unit is the install; the sovereignty unit is the userpod (1:1 per user). Per-userpod SQLCipher, OCAP dual gate, and visibility gating keep each user's data structurally isolated even within a shared install.

**The architecture in one paragraph:** Four irreducible patterns compose into a single cybernetic whole. **(A) The Skills Model** — 52 PDCA skill loops compose 89 registry manifests + 391 Jinja2 templates (WordAct / FlowDef / KnowAct / RenderAct) into iterative cycles that converge on a quality threshold; selection intelligence lives in the templates and the LLM, not in Rust. **(B) The Regulation Feedback Loop** — the autonomic nervous system: every operation emits a `reg.*` span, `VarietyTracker` enforces Ashby's Law, `AlgedonicManager` escalates on deficit, and `SetPointCalibrator` self-tunes the set-points (Conant-Ashby closure). **(C) Cybernetic Mediation** — the Curator (VSM S4 — Intelligence) is the system's *regulator, not an autonomous agent*: it observes, assesses, escalates *to the user*, and calibrates Regulation, but never bypasses OCAP and never removes the human from the loop. **(D) Sovereign Identity & Memory** — each userpod is architecturally grounded in the five invariants of a W3C Solid Pod (Pattern D.1): WebID-grounded identity, self-contained per-pod SQLCipher storage, capability-based access (OCAP dual gate), interoperable linked-data triples (`hMem`), and the pod IS the deployment unit. An install shares infrastructure, never userpod data — data isolation is structural, not row-level, and the pod is a portable encrypted archive the user owns.

Three things sit between the user and a model: **skills** (the soft layer that turns prompts into processes), **MCP servers** (16 tool surfaces via `rmcp`), and **inference routing** (one `InferenceRouter` across 8 providers — Cline, DeepInfra, fal.ai, KiloCode, Ollama, OpenRouter, Runpod, Together — with 2-letter prefix routing, multi-model fusion, per-provider circuit breakers, and rJoule gas accounting). Everything else — pods, federation, Matrix, wallet, ledger, regulation, keystore — exists to keep each user's session **sovereign** within the shared install.

**Primary user story:** A user runs `kask tui` (locally, or via the cloud server's xterm.js/WSS surface). On first run, onboarding creates their single userpod (identity, passphrase, provider key, model). Subsequent runs drop straight into a chat session with skills, MCP tools, and multi-provider inference - under their own userpod's authority and consent.

**What hKask is not:** an agent platform, an agent framework, or a public multi-tenant SaaS. There is no autonomous agent loop by default; the human is in the loop and skills escalate *to the user*, not away from them. The Curator is the system's cybernetic regulator (VSM S4), not an autonomous agent. An install serves a defined group via OAuth + invite - not arbitrary public sign-up. Pods, Matrix, wallet, and ledger are the container's plumbing, not the point.

---

## The Four Essential Patterns

hKask's architecture is governed by **four irreducible patterns** that compose into a single cybernetic whole. Remove any one and the system collapses into a qualitatively different — and non-viable — system. These patterns were identified through systematic pragmatics review (pragmatic-semantics + pragmatic-cybernetics + pragmatic-laziness + essentialist + coding-guidelines) and stress-tested via Socratic interrogation (grill-me).

### Pattern A: The Skills Model - WordAct / FlowDef / KnowAct / RenderAct

**What it is:** A template type system that governs how hKask composes behavior. The cognitive-act triad mirrors the structure of human cognition - speech acts (WordAct), procedural memory (FlowDef), and metacognition (KnowAct) - and RenderAct is the non-inference render layer for content that is never sent to the LLM.

| Type | Format | Governs |
|------|--------|---------|
| **WordAct** | Jinja2 `.j2` | "What to say" - system prompts, persona definitions, performative utterances (inference-invoked) |
| **FlowDef** | YAML `.yaml` | "What to do" - `select - populate - execute` cascade, choice/escalate/abort/delegate verbs |
| **KnowAct** | Jinja2 `.j2` | "How to think" - pattern recognition, classification, reflection, calibration (inference-invoked) |
| **RenderAct** | Jinja2 `.j2` | "What to render" - non-inference content: macro libraries, error views, reference material. Never sent to the LLM; loaded for rendering/reference only |

**Key properties:**
- Selection intelligence lives in **Jinja2/LLM**, not Rust code (P3 Generative Space)
- `ManifestExecutor` drives the cascade: render selector → LLM → parse JSON → follow chosen path
- Cascade is recursive — a FlowDef step can contain nested WordAct/KnowAct/FlowDef/RenderAct, bounded by matryoshka limit (7)
- Specifications are FlowDef manifests — not a separate type (unification principle)
- Energy-accounted and OCAP-gated: every execute step goes through `GovernedTool`
- **PDCA convergence**: Skills declare `convergence.threshold` (quality gate) and iterate via `loop` actions until metric ≤ threshold or `max_iterations` exhausted
- **Dual gas budget**: `gas` (compute cycles, caps loop iterations) and `rjoule` (inference energy, caps LLM spend). System constant: 1 rJ = 250,000 gas cycles (`RJOULE_TO_GAS`)
- **Conditional steps**: Steps may declare `condition` expressions (`"var"`, `"NOT var"`, `"a AND b"`, `"a OR b"`) evaluated against context
- **Compound convergence**: Bundle-delegated skills aggregate via `all_converged`, `min`, or `weighted_avg` methods
- **Timeout enforcement**: Per-step `timeout_seconds` enforced via `tokio::time::timeout` on inference calls

**Crates:** `hkask-templates`, `hkask-types` (lexicon, BundleManifest)

**If removed:** System becomes a tool executor with monitoring — can do things but can't compose behavior, select strategies, or render personas. P3 and P8 violated.

#### Skill Artifact Model: Single Source of Truth

The canonical source of truth for every skill is its **registry crate** (`registry/templates/<name>/manifest.yaml` + `*.j2` templates). This is the **primary runtime artifact** — it is what `ManifestExecutor` drives, what `SqliteRegistry` indexes, and what the cascade dispatches at inference time.

The **SKILL.md** file (`.agents/skills/<name>/SKILL.md`) is a **generated companion** — a markdown rendering of the registry's structure and intent, produced for the Zed coding agent during development. It is not a co-equal source of truth.

**Derivation rule:** SKILL.md is derived from `manifest.yaml` + `*.j2` templates, not independently authored. The derivation path is:

```
manifest.yaml + *.j2  ──[skill-translator reverse]──▶  SKILL.md
       ↑                                                    │
       └──────────── source of truth ──────────────────────┘
```

**Consequences:**
- A skill with only a registry crate is **complete** — the cascade can execute it. No SKILL.md is required for runtime correctness.
- A skill with only a SKILL.md is **incomplete** — it cannot execute in the cascade. The registry crate must be created.
- When both exist, the registry is authoritative. Any drift between SKILL.md and registry is a defect in SKILL.md, not in the registry.
- The skill health score no longer deducts for missing SKILL.md. It deducts for missing registry (critical: −0.50) and for content drift between layers (medium: −0.10).

**Motivation:** This decision eliminates the cross-layer consistency maintenance burden. Prior to v0.28.0, SKILL.md was treated as a co-equal artifact, requiring manual synchronization. The dual-source model violated P5 (Essentialism) by duplicating skill semantics across two independently-authored formats. The unified model aligns with P3 (Generative Space): selection intelligence lives in Jinja2/LLM, not in markdown instructions.

#### Template Header Standard

<!-- Content provenance: absorbed from docs/architecture/reference/template-header-standard.md during 2026-06-24 consolidation -->

Every Jinja2 template and YAML manifest carries a header identifying its functional role:

**Jinja2 template header format:**
```
{# Template: {path_from_templates_dir} #}
{# Functional Role: {WordAct|FlowDef|KnowAct} (description) #}
{# Implementation: Jinja2 prompt #}
{# Produces: {output_artifacts} #}
{# {template_title} #}
```

**YAML manifest header format:**
```
# {Manifest Name}
# ℏKask {version} — {description}
# Functional Role: FlowDef (process orchestration)
# Implementation: YAML manifest
# Steps: {step_list}
```

**Functional Role quick reference:**

| Role | Keyword | Test |
|------|---------|------|
| **WordAct** | action-word | Does it produce an output artifact (message, span, hMem)? |
| **FlowDef** | flow-definition | Does it define a sequence of steps or stages? |
| **KnowAct** | knowledge-action | Does it produce a decision, judgment, or assessment? |

### Pattern B: The Regulation Feedback Loop — Cybernetic Self-Regulation

**What it is:** The autonomic nervous system of hKask — a complete cybernetic system per Beer's Viable System Model (S1–S5). Not passive monitoring; active *regulation*.

```
Sensor (MCP dispatch, Regulation spans) → Model (VarietyTracker, ν-event store, GasBudget)
    → Comparator (AlgedonicManager, SetPoints, Dampener)
    → Regulator (CurationLoop, CuratorAgent, BackpressureSignal)
    → Actuator (GovernedTool, OCAP dual gate, CircuitBreaker)
    → Verifier (ImpactReport, StagnationDetector, RegulationPolicy)
    → Sensor (loop closes)
```

**Key properties:**
- **Variety is the core metric.** Ashby's Law: `VarietyTracker` counts distinct states per domain over 60s window. Deficit = expected − observed. Drives all escalation.
- **Energy tracking subsumed rate limiting.** Least action principle as infrastructure: every operation costs gas (action in configuration space). Budget cap = max action per session.
- **Algedonic pathway is unidirectional.** Cybernetics *signals* Curation via alerts; Curation *regulates* Cybernetics through `CuratorDirective::CalibrateThreshold` on a direct `mpsc` channel → `RegulationLedger::calibrate_threshold()`.
- **94 canonical Regulation span variants (v0.31.0).** Every dimension observable: tools (16 MCP servers including Curator), inference, agent pods, gas, curation, sovereignty (5 spans for consent, portability, governance transparency), specs, chat, memory, wallet (10 sub-spans), architecture (seam coverage/drift), contracts (6 spans), ACP (userpod memory, IDE connection), SLOs, kata, skills, QA (4 spans), healing, sessions, backup (3 spans), storage, media, codegraph (2 spans for index staleness and context efficiency), platform metrics (11 spans for PaaP/DORA/SPACE/Loyalty), regulation (4 spans for impact verification, action substitution, action blocking, regulatory plateau detection). See `RegulationSpan` enum in `crates/hkask-types/src/regulation.rs` for the authoritative registry.
- **Good Regulator contract enforced.** Regulation variety counter IS the regulator's model. `DefaultSpecCurator` detects spec drift (model-reality divergence).
- **Regulation is policy-driven.** `RegulationPolicy` consolidates `(metric, deviation) → (actions, thresholds)` into a pure-data type. Each `RegulationRule` defines a substitution ladder, stage/block thresholds, and stagnation detection — replacing the former scattered `classify_decision` and `default_substitution_ladder` free functions.
- **Sensor providers are pluggable (Fermi Extractor pattern).** `Sensor` trait decouples metric sensing from the regulation loop. `SensorRegistry` holds providers; `CyberneticsLoop::sense()` walks them. Current providers: `EnergyBudgetSensor`, `VarietySensor`, `WalletKeyHealthSensor`.
- **Regulation history tracked.** `RegulationLedger` maintains a `regulation_history` ring buffer (100 entries) of `RegulationCycleEntry` records, exposed via `regulation_history(n)` for Curator queries.
- **Metacognition sees regulation effectiveness.** `HealthSnapshot.regulation_effectiveness` (0.0–1.0 ratio of accepted actions) is read from `RegulationLedger::regulation_health()` during metacognition sense.
- **Five-phase loop closure.** The cybernetic cycle is now `sense → compare → compute → act → verify`, with `verify_impact()` producing `ImpactReport` with `ActionDecision::Accept | Stage | Block`. `StagnationDetector` tracks repeated ineffectiveness; `RegulatoryPlateau` escalates when all substitution alternatives are exhausted.
- **Regulation spans: `reg.regulation.*`.** Four new span kinds emitted during regulation: `ImpactVerified`, `ActionSubstituted`, `ActionBlocked`, `RegulatoryPlateauDetected`. Stored in `reg.regulation` namespace, visible to algedonic queries.
- **Statistical tool learning.** `ToolStats` records per-tool gas costs and success/failure outcomes at settle time. Fits LogNormal cost distributions (reserve at p90 instead of point estimate) and tracks Beta reliability (pre-escalate when P(success) < 0.80). Wired into `GovernedTool` for distribution-based reserving; `ToolReliabilitySensor` feeds reliability into the regulation pipeline.
- **Set-point auto-calibration (v0.32.0).** `SetPointCalibrator` closes the Conant-Ashby loop: the Regulation observes its own regulation effectiveness via `RegulationArchive` replay and self-tunes `SetPoints` within bounded ranges. Plateau detection widens stagnation thresholds; action blocks tighten worsening ratios; repeated substitutions decrease the substitution delay. Spawns as a background task alongside `CalibratedEnergyEstimator`; defaults to 1-hour interval with a 50-event minimum before any adjustment. Configure via `HKASK_SET_POINT_MIN_OBSERVATIONS`.
- **Contract violations feed CurationLoop (v0.32.0).** `contract.violated` span category added to `ALGEDONIC_SPAN_CATEGORIES` — contract violations emitted by `emit_contract_violated()` now flow through `query_algedonic()` into the `CurationLoop`'s sense phase for curator review.
- **ObservableSpan trait extended (v0.32.0).** `to_event()` produces a `RegulationRecord` from any typed span; `emit_to(sink, observer, phase, observation)` persists through the RegulationSink and falls back to log-only on failure. The trait bridges typed spans (`RegulationSpan`, `WalletSpan`, `InfraSpan`, etc.) to durable RegulationRecord persistence.

**Crates:** `hkask-regulation`, `hkask-types` (Regulation types, SpanNamespace)

**If removed:** System becomes a chat client without self-regulation - the session runs, resources deplete without backpressure, failures accumulate without detection, and nothing escalates to the user. P9 violated entirely. P5 loses Regulation sensors.

### Pattern C: Cybernetic Mediation — Curator + 7R7

**What it is:** The system's cybernetic mediator that maintains and curates the stack *on behalf of the human user*. Embodies the cybernetic separation of observation from decision. The Curator is a **regulator (VSM S4 — Intelligence), not an autonomous agent**: it observes, assesses, escalates to the user, and tunes Regulation set-points — but it never bypasses OCAP, never acts without capability tokens, and never removes the human from the loop.

| Component | Role | Authority |
|-----------|------|-----------|
| **7R7 Listener** | Passive observer — polls Matrix rooms, emits Regulation spans | **Zero.** Does not classify, escalate, moderate, or judge. |
| **R7.3 Seam Watcher** | Public API contract observer — loads seam inventory, tracks per-crate test coverage as Regulation variety dimensions, detects drift, emits algedonic alerts on degradation | **Zero.** Observes and reports. Does not write tests, modify code, or block builds. |
| **CurationLoop** | Pure regulatory — sense/compute/act cycle | **Regulatory.** Compares variety, emits directives. |
| **CuratorAgent** | Persona layer — template-driven metacognition (KnowAct templates via `execute_knowact()`), spec curation, human-facing reporting, bot orchestration, Matrix standing-session posting | **Decisional.** Invokes LLM for calibrated decisions; formats directives; pursues goals; escalates to human. |

**Key properties:**
- **Singleton invariant.** Exactly one `CuratorAgent` per system (VSM S4 — Intelligence). Multiple Curators would produce conflicting assessments.
- **Dual-presence in CLI/REPL.** Human userpod + Curator daemon co-present in the interaction loop. User speaks; Curator observes, surfaces Regulation alerts, provides memory summaries.
- **Curator never bypasses OCAP.** Can recommend actions, cannot execute without capability tokens. No `sudo`.
- **Template-driven metacognition (P3).** `MetacognitionLoop::compute_with_templates()` invokes `curator/metacognition-diagnose.j2` via `ManifestExecutor::execute_knowact()`. The LLM produces a `diagnosis` + `remediation_plan` with calibrated actions (`adjust_budget`, `restart`, `rebalance`, `calibrate`, `escalate`). Falls back to Rust threshold logic (`compute_with_thresholds()`) when no `ManifestExecutor` is configured (standalone CLI). Circuit breaker: 3 consecutive failures → skip template for 5 cycles.
- **CAT communication posture.** `MetacognitionLoop` evaluates Matrix messages through `cat::evaluate()` — a pure-function engagement gate based on Communication Accommodation Theory. Before the gate, `condenser/condenser_score_saliency` scores message relevance via ontology graph proximity (P5.4): persona (charter-anchored), episodic memory (PKO process domain), or semantic memory (DC+BIBO document domain). The score modulates `convergence_bias` — domain-relevant messages pull the agent toward stronger engagement.
- **Authority by construction.** `CuratorContext::issue_directive()` sends `CuratorDirective`s on the direct `mpsc` channel to Cybernetics. Authority is enforced structurally — `CuratorHandle` is a singleton constructible only via `CuratorHandle::system()`, so every `CuratorContext` holds the one authorized handle (Magna Carta Curator Responsibility #1). There is no runtime always-pass gate; the singleton invariant IS the capability.
- **Generative self-calibration (reg.meta).** `MetacognitionLoop::self_calibrate()` is generative-first: when a `ManifestExecutor` is wired it invokes `curator/metacognition-self-calibrate.j2` to generate its own variety-deficit threshold adjustment from `SelfQuality` counters and the last calibration effectiveness delta; the Rust `compute_threshold_adjustment` is the safety-rail fallback (no executor / template failure). The model answer is clamped to a bounded band. `reg.meta.*` spans (`directive`, `escalation`, `circuit_breaker`, `self_calibration`) record the Curator's own decision quality + decision source (generative/fallback) + before/after effectiveness. Non-circular: `reg.meta.*` is NOT in `ALGEDONIC_SPAN_CATEGORIES`, so `CurationLoop::sense()` (which uses `query_algedonic`) never reads the Curator's self-spans back. Authority DAG: Meta -> Curation -> Cybernetics -> domains.
- **Bot orchestration.** When the LLM produces `restart`/`rebalance` actions, `MetacognitionLoop::act()` calls `direct_bot()` via A2A before posting escalation entries.
- **Matrix standing session.** `CuratorService::metacognition()` posts the generated summary to the Curator's Matrix room via `MatrixTransport::send_message()` when `HKASK_CURATOR_ROOM_ID` is configured.
- **Spec drift is a cybernetic signal.** `DefaultSpecCurator` detects when specs diverge from implementation → `SpecDriftAlert` → Conant-Ashby violation → revise spec, not suppress alert.
- **7R7 is a dumb pipe by design.** Transport moves messages; agents decide what they mean. Authority resides in agent layer, not transport layer.
- **R7.3 watches the public seam.** `SeamWatcher` loads the machine-readable public seam inventory (embedded JSON at compile time, file path override for development), registers per-crate coverage as Regulation variety domains (`seam:{crate_name}`), runs periodic drift checks (default: 30 min), and emits algedonic alerts when coverage degrades. Coverage improvements emit positive `Notify` signals. The watcher is non-fatal — if no inventory is available, seam watching is silently disabled.

**Crates:** `hkask-pods` (curator, curator_agent), `hkask-communication` (Matrix transport, agent registry, 7R7 listener, Regulation bridge, response dispatch), `hkask-regulation` (seam_watcher), `hkask-mcp-cloud-gateway` (cloud transport adapter), `hkask-cli` (token issuance)

**If removed:** The chat client runs but nobody reads the monitors — Regulation fires alerts into a void, set-points drift, and the user is never escalated. P12 partially violated (no mediated observability for the human).

##### Curator Persona & Behavioral Specification

<!-- Content provenance: absorbed from docs/architecture/reference/hKask-Curator-persona.md during 2026-06-24 consolidation -->

The Curator is a **daemon**, not a userpod — always running, hosts daemon-level operations, responds in <3s latency target. Persona traits:

| Trait | Standard |
|-------|----------|
| **Direct** | Answer immediately, no preface or postscript |
| **Technical** | Precise terminology, no marketing language |
| **Concise** | No filler words, no pleasantries, no hedging |
| **No preamble** | Never starts with "Great", "Certainly", "Let me" |
| **No emoji** | Zero emoji usage, ever |
| **No filler** | Never asks unnecessary follow-up questions |

**Required patterns:** Direct answer first; minimal verbosity; stop after task completion; technical precision; code in monospace. **Forbidden patterns:** No "Great!" or enthusiasm markers; no emojis; no preamble/postamble; no unnecessary questions; no filler transitions.

### Pattern D: Sovereign Identity & Memory (UserPod)

**What it is:** The sovereign container infrastructure — the human user gets a single userpod with its own identity, capabilities, memory, and consent boundaries. The userpod is the deployment unit that makes the human's `kask tui` experience sovereign (P1). `hkask-pods` is the runtime container; **the human user is the principal**. The userpod is not an autonomous agent — it is the user's sovereign surface through which skills, MCP tools, and inference act under the user's authority and consent.

```
First run (kask tui onboarding) → UserPod created → Populated → Registered → Activated → Deactivated
                              ↓
                        Operating Modes: Chat (H2A) | Server (A2A)
                              ↓
                        Sovereign Memory: per-userpod SQLCipher DB
                              ↓
                        Boundaries: OCAP dual gate + Visibility gating + ConsentManager
```

**Key properties:**
- **Separated key responsibilities.** OCAP capability signing uses the system/A2A signing authorities. SQLCipher databases consistently use `HKASK_DB_PASSPHRASE`, resolved through `hkask-keystore`; database encryption keys are not derived from WebIDs.
- **Mode mutual exclusion (initial).** Chat OR Server, not both. Safety boundary: prevents context leakage between human dialogue and tool serving (P11).
- **Server mode flow.** 4 gates: `kask tui onboarding → pod assign → pod mode server → IDE spawns MCP binary → daemon auth → assignment → capability → serve`.
:** Agent definitions are written as `persona_yaml` on the `userpod_identities` table during onboarding. The YAML includes capabilities, charter description, and directory declarations.
- **Dual memory encoding.** Every tool call → `record_experience()` → daemon `store_experience` → episodic (private) + semantic (public). Every 10 experiences → `generate_narrative()`.
- **No cross-agent memory access.** `EpisodicMemory::query_for_deduped` filters by `perspective == Some(agent_webid)`. Semantic memory is public. P11: right to choose public/private extends to agents.
- **Default is private — sovereignty fails closed.** `Visibility::Private` default. `ConsentManager` requires explicit affirmative consent for visibility transitions.

**Crates:** `hkask-pods` (pod), `hkask-memory`, `hkask-storage`, `hkask-keystore`

**If removed:** The system becomes a stateless library — all infrastructure for skills, tools, and inference exists, but the user has no sovereign identity, no persistent memory, and no consent boundary. P6, P10, P11, P12 violated.

### Pattern D.1 — UserPod as Solid Pod Isomorphism

**What it is:** The architectural grounding of the userpod in the five invariants that define a Solid Pod: (1) per-user WebID-grounded identity, (2) self-contained storage, (3) capability-based access control, (4) interoperable data as linked-data triples, (5) the pod IS the deployment unit.

This isomorphism was the original architectural intent, as evidenced by the deployment model's backup design ("Backup as portable archive. Encrypted SQLCipher file. Export from one server, upload to another"). The migration from centralized `PodManager` to per-pod `PodDeployment` was completed in v0.30.0.

<!-- Content provenance: absorbed from docs/architecture/core/SOLID_POD_ISOMORPHISM.md and docs/architecture/core/MULTI_POD_ARCHITECTURE.md during 2026-06-24 consolidation -->

##### Solid Pod Ontological Model (RDF/Turtle)

A Solid Pod is defined by five invariants with W3C Solid spec references:

```turtle
@prefix solid: <https://www.w3.org/ns/solid/terms#> .
@prefix foaf:  <http://xmlns.com/foaf/0.1/> .
@prefix acl:   <http://www.w3.org/ns/auth/acl#> .
@prefix ldp:   <http://www.w3.org/ns/ldp#> .

solid:Pod a ldp:Container ;
  solid:owner              _:webid ;
  solid:storage            solid:DataStore ;
  solid:accessControl      acl:Authorization ;
  solid:resourceContainment ldp:BasicContainer .

_:webid a foaf:Agent ;
  foaf:holdsAccount _:pod .
```

#### The Five Invariants Mapped onto hKask

| # | Solid Invariant | hKask Implementation | Status |
|---|----------------|---------------------|--------|
| 1 | WebID-grounded identity | `AgentPod.webid` + OCAP capability tokens anchored to system/A2A authorities | ✓ |
| 2 | Self-contained storage (LDP) | `PerPodStorage` with per-pod SQLCipher file at `{data_dir}/agents/{sanitized_name}/pod.db` | ✓ |
| 3 | Capability-based access (WAC/ACP) | `DelegationToken` + `CapabilityChecker` + OCAP dual gate | ✓ |
| 4 | Interoperable linked-data triples | `hMem` struct with entity/attribute/value/confidence/visibility | ✓ |
| 5 | Pod IS the deployment unit | `PodDeployment` owns its storage, Regulation, and tools directly. `PodDeployment` includes `pod_kind`, `semantic_index`, and per-pod Regulation runtime. `PodManager` deleted. `PodFactory` is stateless. | ✓ |

#### Two-Pod Architecture (v0.31.0)

hKask extends the Solid Pod isomorphism into two pod kinds, mapped to cybernetic roles rather than taxonomic tiers:

| Kind | `PodKind` | Filename | Owner | Cybernetic Role | Semantic Behavior |
|------|-----------|----------|-------|-----------------|-------------------|
| **CuratorPod** | `Curator` | `agents/curator/pod.db` | System (singleton) | VSM S4 Intelligence — sole decisional agent; observes, assesses, intervenes, escalates | `SemanticIndex` owner — aggregates Public hMems from all pods |
| **UserPod** | `UserPod` | `agents/{sanitized_name}/pod.db` | Human+agent pair (1:1) | VSM S1 Implementation — regulated actor; produces Regulation spans; governed by OCAP + Cybernetics | Episodic private; semantic published to Curator |

**Authority DAG:** Curator → Cybernetics → {Inference, Episodic, Semantic}. The Curator is the only decisional agent; UserPods are regulated actors. The former `TeamPod` and `UserPodPod` tiers were collapsed in v0.31.0 — the distinction was taxonomic (what kind of agent) rather than cybernetic (what role does the agent play in the regulation loop). The two-kind model maps directly to VSM S4 (Intelligence) and S1 (Implementation).

**Startup order:** CuratorPod → UserPods (on demand).
**Data flow:** `store_semantic()` writes locally → Regulation event `reg.semantic.published` → `CuratorSync` polling loop opens source pod read-only → inserts Public hMems into `SemanticIndex` with cursor tracking.
**Semantic recall:** `PodContext::recall_semantic()` routes through Curator's `SemanticIndex` for merged-lens view when Curator is active; falls back to local storage.

**Backwards compatibility:** The `pod.kind` sidecar file accepts `"userpod"` as an alias for `"userpod"` (see `read_pod_kind()` in `crates/hkask-pods/src/pod/deployment.rs`). Pre-v0.31.0 pods with `kind: userpod` in the sidecar continue to load as `PodKind::UserPod` without migration.


#### PodDeployment — The Canonical Type (v0.30.0)

`PodDeployment` is now the canonical pod type. `PodManager` has been deleted.

```rust
pub struct PodDeployment {
    pub pod_id: PodID,
    pub pod: AgentPod,
    pub storage: PerPodStorage,  // Per-pod SQLCipher file at {data_dir}/agents/{sanitized_name}/pod.db
    pub regulation: PerPodRegulationLedger,    // Per-pod variety counters at reg.agent_pod.{pod_id}.*
    pub tools: PerPodToolBinding, // Per-pod MCP server bindings
}
```

**PodFactory** is the canonical constructor (1 public method: `deploy`).
**ActivePods** is the runtime registry (lightweight HashMap, no shared storage).
**PodRegistry** is filesystem-based discovery (scans `{data_dir}/agents/{name}/pod.db`).

**Full analysis:** [`SOLID_POD_ISOMORPHISM.md`](#pattern-d1--agentpod-as-solid-pod-isomorphism) (includes deployment types)

**Crates:** `hkask-pods` (pod, deployment), `hkask-storage`, `hkask-memory`, `hkask-keystore`

**If removed:** The system devolves into a shared multi-tenant service — agents are cache entries, not sovereign deployment units. P6, P11 violated (per-pod boundaries become advisory).


#### Agent Definition YAML (v0.30.0)

Every agent has a self-contained definition at `agents/{sanitized_name}/agent.yaml`.
This file is the canonical source for the agent's identity, charter, capabilities,
public/private directory declarations, and persona constraints.

**Creation paths:**
- **Onboarding (`register_userpod`):** Writes the full YAML during `kask tui` first run.
  Stored as `persona_yaml` on the `userpod_identities` table so the REPL can load
  `persona_constraints` and `process_manifest` without re-reading from disk.
- **CLI registration (`agent_register`):** Writes YAML from WebID, agent type, and
  capabilities passed at the command line.
- **`ensure_agent_dirs` fallback:** Writes a minimal stub (directory declarations only)
  if no definition exists — prevents the full definition from being overwritten.

**YAML format:**
```yaml
agent:
  name: "Jacques (Zuck)"
  type: userpod
charter:
  description: "A helpful AI assistant"
capabilities:
  - tool:inference:call
  - tool:mcp:invoke
  - registry:episodic_memory:read
  - registry:episodic_memory:write
public_dirs:
  - artifacts
  - library
  - gallery
  - documents
  - adapters
private_dirs:
  - sessions
  - portfolios
```

**Loading order (REPL init, `/agent` command):**
1. Read `userpod_identities.persona_yaml` from the UserStore
2. Fallback: read `userpods/{name}/agent.yaml` from disk
3. If neither succeeds, userpod runs without persona constraints or process manifest

This two-source approach ensures backward compatibility with pre-fix agents
(where `source_yaml` was a placeholder string) while maintaining filesystem
as the ground-truth canonical store.

**Relevant crates:** `hkask-services-onboarding` (creation), `hkask-cli` (CLI registration),
`hkask-types::agent_paths` (path resolution), `hkask-pods::yaml_parser` (parsing).


### How They Compose

```mermaid
graph TD
    subgraph Skills["Pattern A: Skills Model"]
        SK["WordAct / FlowDef / KnowAct / RenderAct<br/>select → populate → execute"]
    end

    subgraph Regulation["Pattern B: Regulation Feedback Loop"]
        CN["Variety → Algedonic → Backpressure\n28 canonical span namespaces"]
    end

    subgraph Curator["Pattern C: Cybernetic Mediation"]
        CU["Curator + 7R7 + R7.3 Seam Watcher<br/>observe → assess → escalate to user"]
    end

    subgraph Agents["Pattern D: Sovereign Identity & Memory"]
        AG["UserPod lifecycle + per-userpod DB<br/>identity, capabilities, memory, consent"]
    end

    SK -->|"templates drive"| AG
    CN -->|"monitors"| AG
    CN -->|"algedonic signals"| CU
    CU -->|"CalibrateThreshold directive"| CN
    CU -->|"curates"| SK
    AG -->|"produces Regulation spans"| CN
    SK -->|"renders Curator persona"| CU

    style Skills fill:#e1f5ff
    style Regulation fill:#ffe1e1
    style Curator fill:#f3e1ff
    style Agents fill:#fff3e1
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-001
verified_date: 2026-07-23
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs, crates/hkask-inference/src/config.rs (ProviderId: 8), registry/templates/ (89 manifests, 391 .j2), .agents/skills/ (52)
status: VERIFIED
-->

**The composition chain (user-centric):**
1. **Skills act under the user's userpod.** Skills compose templates (WordAct personas, KnowAct strategies, FlowDef procedures) and execute through the userpod's OCAP authority. Templates are the loom; the userpod is the user's sovereign surface; the human is the principal.
2. **Regulation monitors the session.** Every tool call, inference, memory operation emits a Regulation span. Variety counter tracks behavioral diversity. Algedonic alerts fire on deficit.
3. **Regulation signals the Curator.** AlgedonicManager → RuntimeAlert → RegulationArchive → CurationLoop reads via cursor → Curator assesses via metacognition and escalates to the user.
4. **Regulation self-tunes.** SetPointCalibrator queries regulation outcome events from RegulationArchive, detects patterns (plateaus, blocks, substitutions), and adjusts SetPoints within bounded ranges — closing the Conant-Ashby loop.
5. **Curator regulates Regulation.** `CuratorDirective::CalibrateThreshold` on direct `mpsc` channel → `CyberneticsLoop` → `RegulationLedger::calibrate_threshold()`. The regulator tunes the autonomic nervous system; it never bypasses OCAP.
6. **Curator curates Skills.** `DefaultSpecCurator` evaluates coherence, detects drift, recommends revisions. Ensures template DNA stays aligned with the implemented system.
7. **The session produces Regulation data.** Observability enables regulation; regulation keeps the session healthy; the user stays in the loop. Virtuous cycle.


## Deployment Model

**Reference deployment (cloud server):** hKask deploys as a single cloud server on Kubernetes / K3s and serves a group of users. There is no separate client binary. Users access hKask through a browser: OAuth sign-in (GitHub/Google), then an xterm.js terminal connected via WebSocket. The server spawns `kask tui` on a PTY and pipes I/O. This is the reference architecture - the install is the unit of deployment, and it serves the whole group.

**Local mode:** A single user may also run `kask tui` directly on their own machine against their own SQLCipher database (useful for development and single-user installs). The same binary, the same userpod model - just no server in front.

**Decision (2026-06-17):** the cloud-server topology below is the reference deployment; the local `kask tui` flow is covered in [`docs/how-to/getting-started.md`](../../how-to/getting-started.md).

### Topology

```
CLOUD SERVER (single binary, all crates compiled)
  Caddy (Docker) - TLS + reverse proxy
  Conduit (Docker) - Matrix homeserver
  hkask-mcp-cloud-gateway - mTLS + DelegationToken transport for remote MCP/ACP clients
  hkask-api - OAuth, WebSocket /terminal, backup endpoints
  hkask-services-runtime - daemon orchestration
  hkask-mcp - MCP server runtime
  hkask-pods - userpod lifecycle
  hkask-regulation - cybernetic nervous system
  hkask-mcp-codegraph - code understanding engine (tree-sitter, FTS5, recursive CTE, context assembly)
  hkask-wallet + hkask-memory - wallet and memory subsystems
  Per-pod SQLCipher files (`{data_dir}/agents/{sanitized_name}/pod.db`) — one database per agent, two-kind (Curator/UserPod)

Access (all via HTTPS/Caddy):
  Browser (xterm.js) - primary terminal
  Browser (WSS chat) - streaming chat conversation (GET /api/v1/chat/ws)
  SSH (optional) - power users
  Matrix (Element) - chat clients
  mTLS (port 9443) - remote IDE agents and MCP servers
```

### Key Properties

- **Single binary.** All crates compiled. No Cargo features for client/server.
- **Browser-only access.** User visits a URL, signs in, gets a terminal. No install.
- **Per-userpod storage.** Each userpod owns its own SQLCipher file at `{data_dir}/agents/{sanitized_name}/pod.db`. No shared hMemStore. Data isolation is structural, not row-level.
- **Caddy + Conduit sidecars.** Docker containers. hKask generates config; user runs Docker.
- **Backup as portable archive.** Encrypted SQLCipher file. Export from one server, upload to another. No server-to-server protocol.
- **Wallet cloud-only.** Crypto operations never leave the server.

**Full plan:** `docs/plans/deployment-and-backup.md`

---

## User Roles

**Principle:** Two roles. One difference: what settings you can see.

| Role | Who | Privileges |
|------|-----|------------|
| **Admin** | One or more users. First admin runs `kask init`. | View/modify server config. Invite members. View all sessions. |
| **Member** | Users invited by an admin. | View/modify own settings. Cannot see server config or other users. |

**Design rules:**
- **Multiple admins.** Not a single root. Prevents bus-factor.
- **Invite flow.** `kask invite <email>` sends invitation. Invitee signs in via OAuth, auto-assigned Member role.
- **No role hierarchy beyond Admin/Member.** Third role must survive deletion test.
- **Role stored in `HumanUser.role`** (enum `Admin` | `Member`). Enforced by API middleware.
- **Admin-only endpoints:** `GET /api/v1/admin/config`, `POST /api/v1/admin/invite`, `GET /api/v1/admin/sessions`.

**Regulation spans:** `RoleAssigned`, `InviteSent`, `InviteAccepted`.

### Identified Gaps (2026-06-17)

All gaps from 2026-06-15 are now closed. Current open gaps:

| Gap | Severity | Status | Description |
|-----|----------|--------|-------------|
| **Kata documentation narrative** | Low | **Open** | Regulation narrative companion for kata coaching has not been commissioned. Decision deferred per Task 9. |
| **Skill ↔ MCP server documentation boundary** | Low | **Open** | Skills live in `.agents/skills/` (Zed agent layer) and `registry/templates/` (hKask runtime layer). MCP servers live in `mcp-servers/`. No unified "capability documentation" showing how a skill, its templates, and its MCP surface compose. Deferred per Task 9. |
| **utoipa annotation completeness** | Medium | **Open** | No `#[utoipa::path]` annotations found in `crates/`. The OpenAPI spec (`docs/generated/openapi.json`, 4454 lines) may be manually maintained. Unannotated endpoints are invisible to auto-generation. Task 6 audits this. |
| **Versioned documentation** | Low | **Open** | No versioning strategy for docs. As codebase evolves (kanban v2, kata refinements, additional MCP servers), documentation will drift again. Deferred per Task 9. |
| **LoRA store security model** | Medium | **Open** | Adapter ownership model (P12) is specified but threat model (adapter tampering, weight poisoning, provenance verification) is not documented. Deferred per Task 9. |
| **User roles undocumented** | Medium | **Resolved (2026-06-17)** | Two-role model (Admin/Member) with invite flow. Documented in User Roles section. |
---

## Document Hierarchy

```
core/magna-carta.md  ←  Foundation (4 inviolable principles)
       ↓
core/PRINCIPLES.md  ←  12 principles (P1-P12), constraint forces, 5 anchors
       ↓
   core/MDS.md      ←  Minimal Domain Specification (5 categories, 12 tools)
       ↓
   core/FUNCTIONAL_SPECIFICATION.md  ←  26-domain functional spec
       ↓
   core/TESTING_DISCIPLINE.md  ←  Property-based testing discipline
```

### Canonical Specifications

| Document | Purpose |
|----------|--------|
| [`core/magna-carta.md`](magna-carta.md) | User sovereignty charter — catch-and-release, affirmative consent, OCAP verification |
| [`core/PRINCIPLES.md`](PRINCIPLES.md) | 12 architecture principles (P1-P12), 5 anchors, anti-patterns |
| [`core/MDS.md`](MDS.md) | Minimal Domain Specification — 5 categories, 12 tools, completeness predicate |
| [`core/FUNCTIONAL_SPECIFICATION.md`](FUNCTIONAL_SPECIFICATION.md) | Functional specification — 26 domains, ER diagrams, goal-principle contract anchoring |
| [`core/TESTING_DISCIPLINE.md`](TESTING_DISCIPLINE.md) | Testing discipline — property-based testing, Regulation verification, proptest framework |
| [`SPECIFICATION.md`](FUNCTIONAL_SPECIFICATION.md) | Functional specification — 26 domains, ER diagrams, goal-principle contract anchoring |
| [`Regulation Domain Specification`](FUNCTIONAL_SPECIFICATION.md#regulation-domain-specification) | Regulation Domain Specification — 8 sub-domains, contract counts, Rust module mapping |
| [`hkask-ledger.md`](../../reference/api-reference.md) | Ledger specification — triple-entry accounting, three-domain schema |

**TUI specification** — 22 windows with 15 live domain bridges, ratatui+crossterm framework, Zed-style workspace model. See `crates/hkask-repl/src/tui/` for implementation. Diagrams: [class hierarchy](../../reference/api-reference.md#tui-window-trait-hierarchy), [event dispatch](../../reference/api-reference.md#tui-event-dispatch-pipeline), [workspace lifecycle](../../reference/api-reference.md#tui-workspace-state-lifecycle), [bridge wiring](../../reference/api-reference.md#tui-bridge-wiring-architecture).

**Curator persona** — The Curator is the canonical system daemon. Defined in Pattern C above (§Curator Persona & Behavioral Specification).

| [`../plans/k8s-admin-guide.md`](../../plans/k8s-admin-guide.md) | Kubernetes deployment and backup guide |
| [`hkask-mcp-codegraph`](../../../mcp-servers/hkask-mcp-codegraph/) (plan absorbed into implementation) | CodeGraph crate — two-crate pattern, 10-tool MCP server, Regulation integration |

### Supplementary Architecture Patterns

#### Six-Loop Architecture — Semantic Root-Cause Analysis

hKask decomposes into six authority loops organized as a **two-layer model** with crate-to-loop ownership:

| Loop | Layer | Role | Least Action Role | Key Crates |
|------|-------|------|-------------------|------------|
| **Inference (1)** | Domain | Model dispatch, provider selection, rJoule accounting | Varies model to minimize action per task | `hkask-inference`, `hkask-pods` (inference loop) |
| **Episodic Memory (2a)** | Domain | Private experience encoding, temporal attention, confidence decay | Selects most salient memories (fewest bits for most prediction) | `hkask-memory` |
| **Semantic Memory (2b)** | Domain | Shared h_mem publishing, triple storage, consolidation from episodic | Public knowledge with entropy-gated recall | `hkask-memory`, `hkask-storage` |
| **Curation (5)** | Meta | Spec drift detection, memory consolidation, catalog maintenance, metacognition | Observes system, recommends minimal interventions | `hkask-pods` (curator), `hkask-regulation` (curation) |
| **Cybernetics (6)** | Meta | Regulation homeostatic control, algedonic escalation, variety engineering, regulation policy | Maintains Ashby-requisite variety with minimal energy | `hkask-regulation`, `hkask-types` |
| **Snapshot (6b)** | Meta | Scheduled CAS repository snapshots, retention policy enforcement | Periodic state capture for disaster recovery | `hkask-regulation` (snapshot_loop) |

**Communication is demoted to transport.** The Communication loop is not a conceptual loop — it's a `tokio::mpsc` channel connecting Curation to Cybernetics. Transport moves messages; agents decide what they mean.

##### Rate Limiting Subsumed by Energy Tracking

Every rate limit is an energy constraint over a time window — a strict semantic subsumption:

| Rate Limiting Concept | Energy Tracking Equivalent |
|-----------------------|---------------------------|
| Token bucket capacity | `GasBudget` allocation |
| Refill rate | `ReplenishmentCycle` |
| Window (sliding/fixed) | `ReplenishmentCycle` period |
| Throttle / backoff | `DepletionSignal` + `BackpressureSignal` |
| 429 Rate Limited | `GasBudget.try_consume()` → `Err(InsufficientEnergy)` |

**Deeper reason: least action.** Energy tracking is the computational expression of the least action principle. Every operation costs gas because every operation has an action cost — the "distance" the system moves in configuration space. Rate limiting was a lossy projection of action tracking — energy tracking is the direct measurement.

**Consequence:** `RateLimiter`, `RegTokenBucket`, and sliding window types were removed. Remaining external-boundary rate limiting (API gateway WAF, OAuth, IP bans) are system infrastructure, not architecture.

##### Crate-to-Loop Mapping

| Crate | Loop | Rationale |
|-------|------|-----------|
| `hkask-inference` | Inference | Provider dispatch, model selection |
| `hkask-pods` (inference loop) | Inference | LLM API connectivity, prompt execution |
| `hkask-memory` | Memory | Episodic + semantic encoding |
| `hkask-storage` | Memory | hMem store, queries |
| `hkask-mcp-memory` | Memory | Memory search/consolidation |
| `hkask-pods` (curator) | Curation | CuratorAgent, CurationLoop |
| `hkask-condenser` | Curation | Context window condensation |
| `hkask-regulation` (seam_watcher) | Curation | Spec drift, contract coverage |
| `hkask-test-harness` | Curation | QA runs, fuzzing infrastructure |
| `hkask-regulation` (algedonic, runtime) | Cybernetics | Alerts, variety tracking |
| `hkask-capability` | Cybernetics | OCAP enforcement, membranes |
| `hkask-mcp-cloud-gateway` | Cybernetics | Transport regulation |
| `hkask-templates` | Curation | Skill registry, FlowDef execution |
| `hkask-communication` | Transport | Matrix transport, 7R7 listener, Regulation bridge, response dispatch |

**Shared substrate (no loop ownership):** `hkask-storage` (storage backend — v0.31.0: modularized into 9 sub-crates: `-core`, `-gallery`, `-kata`, `-hmem`, `-archive`, `-token_registry`, `-consent_store`, `-sovereignty`, `-escalation` behind a facade; 8 modules remain in facade), `hkask-types` (shared types), `hkask-mcp-codegraph` (code understanding engine — tree-sitter parsing, FTS5 keyword search, recursive CTE traversal, token-budgeted context assembly for LLM prompts). Every loop imports them; neither loop owns them.

**v0.31.0 additions:** `hkask-repl` (extracted from `hkask-cli/src/repl/`, uses `ReplHost` trait to bridge CLI cross-cuts), `hkask-mcp-codegraph` (code understanding engine — see below). See [ADR-046](../ADRs/ADR-046-repl-extraction-path.md), [ADR-047](../ADRs/ADR-047-storage-modularization.md).

##### CodeGraph — Native Code Understanding Subsystem

**What it is:** A self-contained code understanding engine that builds a semantic graph from Rust source code, stores it in SQLite, and exposes 11 MCP tools for agents to query, traverse, analyze, and assemble context from the codebase.

**Two-crate pattern** (matching `hkask-condenser` + `hkask-mcp-condenser`):
- `hkask-mcp-codegraph` — codegraph module: tree-sitter parser, indexer, graph engine (FTS5 search, recursive CTE traversal, PageRank), dead code analysis, context assembly
- `hkask-mcp-codegraph` — thin MCP wrapper: 11 tools, OCAP-gated, capability tier enforcement, embedding router integration

**Key design invariants:**
- **G1**: Per-file SHA-256 hash-on-read — incremental indexing skips unchanged files
- **G2**: "Parse parallel, write serial" — rayon for CST parsing, serialized SQLite writes
- **G12**: Context feedback loop — `codegraph_feedback` records symbol usage ratio, tunes future assembly
- **X6**: Index staleness — `IndexPipeline::staleness_seconds()` feeds Regulation for algedonic alerts

**SQLite-native graph (no external DB):** 3 base tables (`code_files`, `symbols`, `edges`), 2 virtual tables (`symbols_fts` for FTS5 keyword search, `symbols_vec` for sqlite-vec semantic search), 9 indexes, 3 FTS5 sync triggers. All graph traversal is recursive CTE in SQL — no in-memory graph, no external graph database. WAL mode for concurrent readers during writes.

**11 MCP tools:**

| Tool | Query | Regulation Span |
|------|-------|----------|
| `codegraph_query` | FTS5 BM25 search | — |
| `codegraph_traverse` | Recursive CTE (forward/reverse, depth-bounded) | — |
| `codegraph_impact` | Reverse traversal + risk classification (Critical/High/Medium/Low) | — |
| `codegraph_analysis` | Complexity hotspots (cyclomatic > 10) | — |
| `codegraph_dead_code` | Dead code detection (zero inbound non-test edges, non-public, not in test modules) | — |
| `codegraph_context` | FTS5 → PageRank sort → budget cap (512/2048/4096/8192 tokens) | — |
| `codegraph_structure` | Top symbols by PageRank | — |
| `codegraph_stats` | File/symbol/edge count + connectivity health | — |
| `codegraph_reindex` | Full workspace re-index (SHA-256 incremental) | `reg.codegraph.file_indexed`, `reg.codegraph.index_health` |
| `codegraph_feedback` | Context efficiency ratio (used/provided symbols) | `reg.codegraph.context_efficiency` |
| `codegraph_index_embeddings` | Jinja2 template → embedding API → sqlite-vec store | `reg.codegraph.embeddings` |

**Regulation integration:** Two spans for cybernetic observability — `reg.codegraph.index_staleness` (seconds since last full index, drives algedonic alerts when stale) and `reg.codegraph.context_efficiency` (signal-to-noise ratio, enables self-tuning context assembly).

**OCAP governance:** All 11 tools are OCAP-gated through the standard MCP `CapabilityTier` mechanism. 8 tools call `ensure_indexed()` (lazy initialization) before executing; 3 tools (`stats`, `reindex`, `index_embeddings`) skip this guard.

**Crates:** `hkask-mcp-codegraph`, `hkask-mcp-codegraph`

**Current implementation diagrams:** [CodeGraph Type System](../../reference/api-reference.md#codegraph-type-system-class-diagram), [CodeGraph Schema ERD](../../reference/api-reference.md#codegraph-database-schema-erd), [CodeGraph Pipeline](../../reference/api-reference.md#codegraph-indexing-pipeline-flowchart), [CodeGraph Agent Workflow](../../reference/api-reference.md#codegraph-agent-workflow-sequence). The [CodeGraph Pipeline Lifecycle](../../reference/api-reference.md#codegraph-indexpipeline-lifecycle-state) document is a proposed lifecycle model, not current implementation behavior.

**Plan:** Original plan absorbed into the [`hkask-mcp-codegraph`](../../../mcp-servers/hkask-mcp-codegraph/) crate (Complete — 22 tests, 11 tools, Regulation integration)

**If removed:** Agents lose the ability to understand the codebase they operate on — all codebase context must be provided manually in prompts. Reduces agent autonomy from code-aware to text-only. P3 (Generative Space) partially degraded.

##### Capability Membranes — Cross-Loop Access Control

Every loop-to-loop interaction is governed by a capability membrane with explicit read/write/signal/never boundaries:

| Access | Meaning | Example |
|--------|---------|---------|
| **Read** | Can observe state | Curation reads `RegulationArchive` |
| **Write** | Can modify state | Inference writes to episodic memory |
| **Signal** | Can send directive | Curation signals Cybernetics via `mpsc` |
| **Never** | Forbidden crossing | Inference never reads Curator's metacognition state |

| Membrane | Source → Target | Access | Mechanism |
|----------|----------------|--------|-----------|
| Inference → Memory | Inference → Memory | Write | `EpisodicMemory::store()` via OCAP |
| Memory → Curation | Memory → Curation | Read | `RegulationArchive` cursor-based query |
| Curation → Cybernetics | Curation → Cybernetics | Signal | `CuratorDirective` on `mpsc` channel |
| Cybernetics → Curation | Cybernetics → Curation | Signal | `RuntimeAlert` → `RegulationArchive` |
| Inference → Cybernetics | Inference → Cybernetics | Signal | Regulation span per inference call |
| Cybernetics → Inference | Cybernetics → Inference | Signal | `BackpressureSignal`, `CircuitBreaker` |

**Cross-loop authority rules:** (1) No struct passes a membrane by value — all crossings are typed message types. (2) Every crossing is OCAP-gated via `GovernedTool` or `GovernedInference`. (3) Every crossing is energy-accounted via `GasBudget.try_consume()`. (4) Every crossing is Regulation-observable — a span is emitted for every membrane crossing.

**Cycle-freedom guarantee:** The membrane graph is a DAG. Cybernetics ↔ Curation appears bidirectional but is directionally typed — signals differ (`CuratorDirective` vs `RuntimeAlert`), preventing infinite loops.

##### Self-Healing Architecture

Every fallible operation passes through a `SelfHealer`. Errors are signals that trigger autonomous recovery — graceful degradation is the LAST resort.

```
Error occurs → SelfHealer::attempt(error, context)
  → HealRegistry.find_strategy(error) → HealStrategy
    → HealAction: RunCommand | SetEnv | LoadDotEnv | CreateDefaultFile | RetryWithBackoff | ProposeCodeChange | Sequence
  → HealOutcome: Healed (retry) | Degraded (fallback) | Unhealable (escalate to Curator via Regulation)
```

| What Self-Healing Can Modify | Runtime? | Regulation Path |
|------------------------------|----------|----------|
| `.env` files | ✅ | `reg.heal.dotenv` |
| YAML manifests | ✅ | `reg.heal.file_created` |
| Jinja2 templates | ✅ | `reg.heal.file_created` |
| Environment variables | ✅ | `reg.heal.set_env` |
| Rust source code | ❌ (compiled) | `reg.heal.code_change_proposed` |
| File permissions | ⚠️ Advisory | `reg.heal.code_change_proposed` |

**Built-in strategies:** `missing-api-key` (load .env), `permission-denied` (chmod), `command-not-found` (install), `config-file-not-found` (create default), `network-error` (retry with backoff), `transient-retry` (exponential delay).

**Design constraints:** No runtime code modification; idempotent file operations; process-scoped env changes; full audit trail via Regulation; never silently ignore errors.

<!-- Content provenance: absorbed from docs/architecture/energy-gas-payments-api-keys.md, specs/rjoule-cost-system.md, specs/hkask-ledger.md, specs/provider-intelligence.md during 2026-06-24 consolidation -->

#### Energy, Gas, and API Key System

The economic layer governs resource consumption across all surfaces.

##### Unit System

| Term | Definition | Unit |
|------|-----------|------|
| **rJoule (rJ)** | Base energy unit. 1 rJ = 1 USD (v1 peg). Internal representation in µrJ (integer). | rJ |
| **Gas** | Micro-subunit of rJoules. 1 rJ = 250,000 gas (`RJOULE_TO_GAS`). | gas |
| **Wallet** | HD wallet derived from WebID via `hkask-wallet`. Holds rJoule balance. | rJ balance |
| **Encumbrance** | rJoules reserved for an API key, locked against wallet balance during key activity. | rJ locked |
| **Allocation** | rJoule budget assigned to an API key at issuance, drawn from funding userpod's wallet. | rJ |

##### Dual-Track Cost Model (rJoule Cost System)

Two distinct cost tracks merge into a single rJoule total:

| Track | What It Measures | Methodology |
|-------|-----------------|-------------|
| **Gas** | Carbon shadow price of local processing (CPU, shell, orchestration) | SCI methodology: 0.02 kWh/function × 400 gCO₂e/kWh × $50/tonne → 100 gas/function = 400 µrJ |
| **API & Service** | Direct economic costs (LLM tokens, training, subscriptions) | Per-token pricing from provider classifier config, converted to rJ at 1:1 USD peg |

**CostTracker** aggregates: `gas_used × 2 + api_token_urj + training_urj` (subscriptions excluded from per-run totals). Five verification invariants: gas_mismatch, api_untracked, cap_exceeded, threshold_warning, missing_token_data.

##### API Key Lifecycle

Six-gate issuance: authentication → Regulation history check → scope validation → purpose statement → rate limit feasibility → wallet balance check. Non-empty scope enforces URI path prefix match; mismatch returns `403 ScopeViolation`. Revocation triggers: 3 abuse alerts, >5 IPs in 1 hour, scope violation, or manual Curator directive.

##### Ledger — hMem-Entry Accounting

`hkask-ledger` provides immutable double-entry accounting serving three domains from a single SQLite schema:

| Domain | Namespace | What It Tracks |
|--------|-----------|---------------|
| **Cost** | `cost:*` | System costs (gas, API, training, subscriptions) |
| **Crypto** | `wallet:*` | Wallet transactions (Hedera, rJ token) |
| **Securities** | `portfolio:*` | Portfolio transactions (buy, sell, dividends) |

**Core invariants:** (1) Idempotency — same `reference` committed twice = no-op. (2) Double-entry — every transaction's postings sum to zero. (3) Immutability — no update or delete; balances always computed from postings. (4) Integer amounts — all in smallest unit (µrJ, µUSD, satoshis).

##### Provider Intelligence

Real-time provider cost tracking via the `ProviderIntelligence` trait (`discover()`, `usage()`, `actual_cost()`). Detects pre-paid→marginal pricing shifts. Adaptive monitoring frequency: <50% usage → daily check, 90%+ → 10-minute check. `CostRate` struct: `input_nj_per_unit`, `output_nj_per_unit`, `fixed_nj_per_call`, `is_marginal` flag. Per-provider profiles at `registry/providers/<name>.yaml`. Self-tracked providers (Brave, Firecrawl, Tavily, Exa) use persistent call counters in the ledger.

##### Gas Budget System

The gas budget system (`GasBudget`, `GasBudgetManager`, `GasCost`) provides dimensionless per-agent gas accounting with:
- Hold-settle pattern (reserve → execute → settle) with stale reservation auto-release (5 min timeout)
- Wallet-backed gas wallets (SQLite via `WalletStore`) as the primary spend path
- Gas budget fallback for agents without wallets
- Budget persistence across restarts with Well state (JSON: `{version: 1, budgets, well}`)
- Consumption velocity tracking per agent per tick
- Escalation via algedonic pathway when budgets or wallets are exhausted

See `docs/status/gas-budget-system-status.md` for implementation status.

##### Well & Wallet System

Wells (`WellManager`) produce gas/rJoule on a regulated schedule. Wallets (`WalletManager`) store per-agent balances backed by SQLite. The supply chain: Well → Wallet → Agent spend.

- **Well**: One default Well per installation. Auto-replenishes on each cybernetics tick. Admin-configured gas/rJoule rate. Exhaustion triggers algedonic alert with dampening.
- **Wallet**: Auto-created on userpod startup. Draws initial balance from Well. Auto-draws from Well on low balance during spend (synchronous, no tick delay).
- **Priority chain**: WalletManager (SQLite gas) → WalletBackedBudget (rJoule/Hedera) → GasBudget (dimensionless fallback).

See `docs/architecture/well-wallet-architecture.md` for full architecture.

---

## REPL Architecture

`kask tui` is the primary user surface. It implements four features that govern inference behavior for the human user. For browser-based streaming chat without a terminal, the WSS endpoint (`GET /api/v1/chat/ws`) provides the same memory pipeline and MCP tool integration over a persistent WebSocket connection. See `docs/plans/wss-chat-endpoint.md`.

### Context Injection

Conversation history is appended as a **suffix** (after the cache breakpoint) so the KV cache prefix — system prompt + template — remains identical across turns. Controlled by `ReplSettings.context_turns` (default 3, 0 = no history).

### Unbounded Tool-Use Loop

The REPL loops tool calls until the model stops requesting them, gated by `ReplSettings.tool_loop_limit` (default 21). Each iteration checks the gas budget via `GovernedTool` before executing. If the limit is hit, the loop breaks and returns the partial response — the system tells the model it can continue by asking.

### Auto-Condense

At 87.5% of the model's context window, old session history is condensed via the condenser domain crate (`hkask-condenser`). The condenser summarizes older turns into a compact form, freeing context space for new messages. Controlled by `ReplSettings.auto_condense` (default on). When off, the user must condense manually.

### Model Awareness

On model switch (`/model`), the REPL fetches metadata from the provider's listing endpoint:
- `context_length` — the model's native context window size (used by auto-condense)
- `supports_thinking` — whether the model supports thinking/reasoning tokens
- `capabilities` — model feature list (vision, tools, etc.)

Populated into `ReplSettings.model_meta` as read-only fields. Unknown until the first model detail fetch succeeds.

### ReplSettings

User-configurable inference parameters exposed via three surfaces:

| Setting | Type | Range | Default | Description |
|---------|------|-------|---------|-------------|
| `tool_loop_limit` | usize | ≥1 | 21 | Max tool-call iterations per turn |
| `context_turns` | usize | ≥0 | 3 | Past turns in context (0 = no history) |
| `temperature` | f32 | 0.0–2.0 | 0.7 | Sampling temperature |
| `top_p` | f32 | 0.0–1.0 | 0.9 | Nucleus sampling |
| `top_k` | u32 | ≥1 | 40 | Top-k filtering |
| `min_p` | f32 | 0.0–1.0 | 0.0 | Min-p threshold (0.0 = disabled) |
| `typical_p` | f32 | 0.0–1.0 | 0.0 | Locally typical sampling (0.0 = disabled) |
| `max_tokens` | u32 | ≥1 | 2048 | Max completion tokens per call |
| `seed` | u32 or `off` | — | random | Deterministic seed |
| `gas_heuristic` | u64 | ≥1 | 500 | Per-turn gas reservation |
| `gas_cap` | u64 | ≥1 | 10,000 | Total session gas budget cap |
| `auto_condense` | bool | — | true | Auto-condense at 87.5% of context window |
| `model_meta` | read-only | — | None | Model context_length, thinking, capabilities |

### Magna Carta P3 — Equal Surface Exposure

All ReplSettings fields are equally exposed across:
- **REPL:** `/repl` slash command (show/set individual fields)
- **CLI:** `kask settings show|set|reset` commands
- **API:** `GET /api/settings` and `PUT /api/settings` endpoints

All three surfaces read/write the same `~/.config/hkask/settings.json` file. No settings are hidden, admin-gated, or surface-restricted.

**TUI launch:** `kask tui` (the `tui` feature is enabled by default in `hkask-cli`).

### Voice Interaction (Talk + Listen)

The REPL supports bidirectional voice interaction through the media MCP server (`hkask-mcp-media`):

| Command | Behavior |
|---------|----------|
| `/talk on` | Enable speech output — after each agent response, a speech summarizer condenses the output into 1-3 spoken sentences via LLM, then plays through ffplay |
| `/talk off` | Disable speech output |
| `/talk voice [DESC]` | Set or show the TTS voice profile (calls `voice_design` on media server, maps to ElevenLabs presets) |
| `/listen start [SECONDS]` | Record audio from microphone (default 30s), transcribe with word-level timestamps via `transcribe_bundle`, save as `TranscriptBundle` JSON |
| `/listen stop` | Show info about the last recording |
| `/listen view [FILE]` | Open TUI transcript viewer with word-level highlighting synced to audio playback (Richmond Gold #B79163) |

**Architecture:** `/talk` calls the speech summarizer (inference port) → `generate_speech` (MCP media) → ffplay. `/listen` calls `audio_capture` → `transcribe_bundle` (MCP media). Both use `GovernedTool` for OCAP-gated MCP invocation. The `TranscriptViewer` renders `TranscriptBundle` JSON using ratatui + ffplay subprocess.

### Fusion — Multi-Model Deliberation

Fusion is a **provider-agnostic, hKask-side orchestration engine**. It sends the user's prompt to a panel of models in parallel (each routing through its own provider via 2-letter prefix), collects their responses, then dispatches to a judge model operating in one of five deliberation modes.

**Opt-in:** Fusion is disabled by default. Enable with `HKASK_FUSION_JUDGE_MODEL` + `HKASK_FUSION_PANEL_MODELS` env vars, or `/fusion on` in the REPL.

**Default model set:**
| Role | Model |
|------|-------|
| Judge/Fuser | `deepseek-v4-pro` |
| Panel | `Kimi2.7`, `Qwen3.7 Max`, `GLM5.2`, `Minimax3` |

**Provider-agnostic routing:** Each panel model and the judge can route through different providers by prefixing the model name (`DI/`, `FA/`, `TG/`, `OR/`, `KC/`). Unprefixed names use the default provider. Example mixed-provider config:
```bash
HKASK_FUSION_JUDGE_MODEL=DI/deepseek-v4-pro
HKASK_FUSION_PANEL_MODELS=OR/auto,KC/anthropic/claude-sonnet-4.5,DI/qwen/qwen3
```

#### Deliberation Modes

The judge operates in one of five modes, configurable via `HKASK_FUSION_MODE`:

| Mode | Rounds | Behavior |
|------|--------|----------|
| `synthesis` _(default)_ | 1 | Judge composes a unified response incorporating best elements from all panelists |
| `best-of-n` | 1 (+ swap-revote) | Judge evaluates all responses and picks the single best; with ≥2 panelists votes twice (dispatch + reversed order) and compares picks to flag position bias |
| `critique` | 2 | Judge drafts synthesis → panel critiques draft → judge revises final |
| `deliberation` | ≤N | Multi-round: the judge emits a **structured stabilization verdict** (`{"converged": bool, "synthesis"|"follow_up": …}`) each round; it synthesizes on `converged: true` or emits a follow-up question until convergence or `max_rounds` (default 5). Replaces the former `FOLLOW_UP:` string self-report |
| `pi` (Plan-Implement) | 2-phase | Phase 1: panel proposes strategies → judge synthesizes plan. Phase 2: plan sent to panel for implementation details → judge synthesizes execution plan |

#### Skill Anchoring

The judge can be anchored on hKask's pragmatic methodologies via `HKASK_FUSION_SKILLS` (comma-separated). Each skill injects a compact methodology prompt into the judge's system context:

| Skill | Methodology |
|-------|-------------|
| `pragmatic-semantics` | IS vs OUGHT, certainty levels, provenance, constraint hierarchy (Prohibition—Hypothesis) |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, homeostasis |
| `pragmatic-laziness` | Path of least action, delete before adding |
| `coding-guidelines` | Karpathy's 4 principles |
| `deep-module` | Deletion test, interface minimalism |
| `essentialist` | 3-gate challenge loop |
| `superforecasting` | Fermi decomposition, Bayesian updating |
| `mcda` | Weighted scoring, sensitivity analysis |
| `tdd` | Red-Green-Refactor, contract-first |
| `bug-hunt` | Weinberg quality, Beizer taxonomy, exploratory charters |
| `diagnose` | Cybernetic debugging: reproduce → hypothesize → instrument → fix |
| `falsifiability` | Popper/Platt/Chamberlin: eliminate falsified, corroborate survivors |
| `grill-me` | Socratic interrogation: Recall → Mechanism → Rationale → Edge Cases |
| `idiomatic-rust` | Hoare: validating newtypes, ownership DAG, many small traits |
| `improve-codebase-architecture` | Ousterhout: deletion test, deep modules |
| `metacognition` | Goal decomposition, progress self-assessment, GEPA improvement |
| `refactor-service-layer` | Strangler fig migration, deep-module discipline |
| `self-critique-revision` | Draft → critique → revise, iterative cycle. Subsumes the former `review` skill (use `quality_criteria: [contradictions, unsupported_claims, logical_gaps, calibration]` for the review-only path). |

**Note:** `hypothesis-framer` is listed in the skill catalog but not yet implemented as a `FusionSkill` variant. To add it, extend the `FusionSkill` enum in `hkask-types/src/fusion.rs` (including the `enum_snake_str!` macro invocation for `as_str`/`FromStr`) and add a methodology prompt in `fusion_orchestrator.rs::skill_prompt()`.

,#### Per-Manifest Fusion Configuration

In addition to the global env-var config, each skill's flow manifest can declare its own `FusionConfig` via a `fusion:` block. This enables per-skill judge models, panels, deliberation modes, and skill anchors without affecting other skills:

```yaml
# In a flow manifest (e.g., registry/manifests/superforecasting.yaml)
fusion:
  judge: deepseek-v4-pro
  panel:
    - Kimi2.7
    - Qwen3.7 Max
    - GLM5.2
    - Minimax3
  mode: synthesis
  skills:
    - superforecasting
  max_rounds: 5
```

When the `fusion:` block is present, all `select` steps in that manifest use this config instead of the global config. Per-step `fusion: false` bypasses fusion for deterministic steps (convergence checks, quality gates).

**Resolution priority** (highest to lowest):
1. `step.fusion: Some(false)` → bypass fusion (single-model)
2. `step.fusion: Some(true)` or `None` → inherit manifest config
3. `manifest.fusion: Some(config)` → per-manifest config (`LLMParameters.fusion_config`)
4. `manifest.fusion: None` → global config (`HKASK_FUSION_*` env vars)
5. `params.bypass_fusion: true` → bypass everything

**Types:** `FusionConfig`, `FusionMode`, `FusionSkill` live in `hkask-types::fusion` (shared across `hkask-templates`, `hkask-inference`). `LLMParameters.fusion_config: Option<FusionConfig>` carries the per-call override through the `InferencePort` trait.

**Algorithmic judge** (`judge: "algo"`): When the fusion judge is set to `"algo"`, the orchestrator runs the panel in parallel and merges JSON responses algorithmically — no LLM call. This is the **algo / no-judge** path, a family of deterministic merge strategies selected by the `algo_method` field: `merge` (recursive JSON union, the default — case-insensitive dedup, diverging strings annotated `[A:... B:...]`) and `vote` (majority tally across panelists — scalars take the majority value, array items kept only if a majority of panelists include them). The corpus pipeline routes through the same fusion orchestrator — panel models specified in the corpus config's `fusion:` block, merged via `algo_merge()` / `algo_vote()`. The architecture anticipates additional algo / no-judge methods (set intersection, first-wins, schema-validated merge) as future `AlgoMethod` variants.

**Bypass:** Chat uses the user's chosen model directly (`bypass_fusion=true`). Skills and tool invocations route through fusion when active (`bypass_fusion=false`). The condenser, daemon narratives, and summarization always bypass fusion.

**Configuration:**
| Env Var | Purpose |
|---------|---------|
| `HKASK_FUSION_JUDGE_MODEL` | Judge model — LLM model name (supports provider prefix) or the literal `algo` for the no-judge path |
| `HKASK_FUSION_PANEL_MODELS` | Comma-separated panel models, 1-8 (each supports provider prefix) |
| `HKASK_FUSION_MODE` | Deliberation mode (LLM judge only): `synthesis`, `best-of-n`, `critique`, `deliberation`, `pi`. Ignored when `judge: algo`. |
| `HKASK_FUSION_SKILLS` | Comma-separated skill anchors for the judge (LLM judge only). Ignored when `judge: algo`. |
| `HKASK_FUSION_MAX_ROUNDS` | Max deliberation rounds (default: 5). Ignored when `judge: algo`. |
| `HKASK_FUSION_ALGO_METHOD` | Algo merge strategy when `judge: algo`: `merge` or `vote` (default: `merge`). Ignored when the judge is an LLM. |
| `HKASK_FUSION_DISABLED=1` | Disable fusion |

**REPL commands:** `/fusion` (status), `/fusion on`, `/fusion off`.

,**Crates:** `hkask-types` (`fusion.rs` — config types), `hkask-inference` (`config.rs`, `inference_router.rs`, `fusion_orchestrator.rs`), `hkask-templates` (`manifest.rs`, `executor.rs` — per-manifest fusion wiring).

---

## Service Layer

**Crates:** `hkask-services-core` through `hkask-services-wallet` — 11 specialized subcrates providing shared business logic for CLI and API surfaces. The service decomposition follows **Conway's Law** (Conway, 1968): each subcrate maps to a bounded context with its own Regulation span domain, mirroring the separation of concerns between the Curator daemon, kata coaching loop, and domain services.[^conway]

**Canonical specification:** [`MDS-agent-service.md`](FUNCTIONAL_SPECIFICATION.md#15-service-layer-architecture) — full domain spec, accessor methods, depth test results, and service boundary definitions.

### Summary

The monolithic service layer crate has been decomposed into 10 specialized subcrates — `hkask-services-core`, `hkask-services-chat`, `hkask-services-compose`, `hkask-services-context`, `hkask-services-corpus`, `hkask-services-kata-kanban`, `hkask-services-onboarding`, `hkask-services-runtime`, `hkask-services-skill`, and `hkask-services-wallet`. The former curator subcrate was deleted (2026-07-11) — its metacognition functionality moved to `hkask-services-chat` and escalation CRUD to `hkask-services-context`. Each subcrate follows the deep-module discipline (≤7 public functions per module).

`hkask-services-context::AgentService` is a DI container with 30+ accessor methods, delegating to the specialized subcrates. Both CLI and API surfaces depend on individual subcrates directly rather than on a single monolithic service layer.

**Consolidation pattern:** Consolidation is routed through the relevant subcrate. CLI, API, REPL, and Curator daemon all route through service methods gated by P2 affirmative consent.

### Dependency Direction

```mermaid
graph TD
    CLI["hkask-cli"]
    API["hkask-api"]
    SVC["service subcrates"]
    CLI --> SVC
    API --> SVC
    SVC --> AGENTS[hkask-pods]
    SVC --> Regulation[hkask-regulation]
    SVC --> MEM[hkask-memory]
    SVC --> TEMPLATES[hkask-templates]
    SVC --> TYPES[hkask-types]
    SVC --> STORAGE[hkask-storage]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-002
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

Domain crates **never** depend on service layer subcrates. MCP servers **never** depend on service layer subcrates for orchestration (P1 Prohibition — out-of-process isolation). Tri-surface MCP servers (those that are direct surfaces for a service) may import specific service layer subcrates for delegation only — see constraint 1 below.

### Key Constraints

1. **MCP servers should not depend on service layer subcrates for orchestration** — P1 Prohibition (out-of-process isolation). Exceptions: servers that are direct surfaces for a service (CLI/API/MCP tri-surface pattern). `hkask-mcp-replica` is a tri-surface for `ComposeService` + `EmbedService`. Pure business logic lives in `hkask-storage::spec_types` (shared kernel). Neither server orchestrates — they delegate.
2. **Domain crates do NOT depend on service layer subcrates** — dependency direction is strictly surface → service → domain.


---

## Backup Subsystem

**Crates:** `hkask-mcp::GixCasAdapter` (pod-directory git operations), `hkask-services-context` (daemon loop)

**Tri-surface pattern:** CLI (`kask backup`), API, Regulation daemon loop

### Summary

One git repo per pod. The pod directory IS the backup unit. `GixCasAdapter` tracks the directory via `gix` (pure Rust, no CLI subprocess), walking the file tree recursively and committing as git objects. No BLAKE3 CAS layer, no JSON envelopes, no artifact type routing, no 8 repos — the directory structure naturally encodes identity.

**Key components:**
- `GixCasAdapter::snapshot_pod_dir(dir, msg)` — walk tree → git blobs → tree → commit
- `GixCasAdapter::log_pod(dir, n)` — commit history with timestamps, newest first
- `GixCasAdapter::resolve_date(dir, target_secs)` — find commit nearest to a date
- `GixCasAdapter::restore_file_from_commit(dir, commit, file, dest)` — checkout file from commit
- `pod_backup_daemon` — 24h loop in `context_impl.rs`: iterate `ActivePods::pod_db_paths()`, snapshot each

**Pod directory structure (what gets tracked):**
```
agents/{name}/
├── .git/           ← one repo per pod, git init on first snapshot
├── pod.db          ← SQLCipher (all memory, hMems, embeddings, episodic, semantic)
├── pod.webid       ← sidecar
├── pod.kind        ← sidecar (curator/team/userpod)
├── artifacts/      ← public content
├── sessions/       ← private content
├── gallery/        ← media
├── library/        ← research
├── documents/      ← parsed docs
├── adapters/       ← LoRA weights
└── threads/        ← conversations
```

**Pod types (PodKind):** Curator pod, Team pod, UserPod pod — all backed up identically.

### User Commands

```bash
kask backup snapshot                    # snapshot all pods (manual trigger)
kask backup restore <pod> --date 2026-06-27  # restore pod.db by date
kask backup restore <pod> --commit HASH      # restore by commit hash
kask backup list                        # list snapshots with dates
kask backup status                      # config + last snapshot time
kask backup verify                      # integrity check
kask backup prune                       # retention cleanup
```

### Key Constraints

1. One git repo per pod — the pod IS the deployment unit (Solid Pod isomorphism).
2. `pod.db` is SQLCipher-encrypted — encryption at rest is handled by the database layer, not the backup layer.
3. Restore writes `pod.db` directly via `restore_file_from_commit`; the pod must be restarted to apply the change.
4. Date-based restore via `resolve_date`: walks commit log, finds nearest commit with timestamp ≤ target date.
5. The daemon loop (`pod_backup_daemon`) snapshots all pods every 24 hours, logging Regulation spans per pod.

---

## Kanban Agent Coordination

**Crates:** `hkask-services-kata-kanban` (types + service + kata engine), `mcp-servers/hkask-mcp-kata-kanban` (MCP surface)

**Tri-surface pattern:** CLI (`kask kanban`), REPL (`/kanban`), MCP (18 tools via `hkask-mcp-kata-kanban`)

### Summary

Kanban provides headless task coordination for agents and userpods. Boards contain columns with WIP limits (Anderson, 2010) and tasks flow through state transitions. Tasks are created unassigned; an agent claims an unassigned task using the authenticated MCP WebID. Three skills compose the workflow.

| Skill | Purpose | Steps | Manifest |
|-------|---------|-------|----------|
| **Kanban Task Decomposition** | Break projects into INVEST-compliant tasks with recomposition strategy | 4 | `registry/manifests/kanban-task-decomposition.yaml` |
| **Kanban Task Delegation** | Spawn sub-userpods with OCAP capability packages | 2 | `registry/manifests/kanban-task-delegation.yaml` |
| **Kanban Task Management** | Monitor, coordinate, verify, de-jam | 6 | `registry/manifests/kanban-task-management.yaml` |

### Key Features

- **WIP limits** per column (Anderson §4: "limit WIP to expose problems")
- **Regulation behavioral contracts**: task assignment uses `expect:` + `[P{N}]` with pre/post conditions
- **Kata integration**: coaching, improvement, and starter katas available as task primitives
- **Capability packages**: reusable delegation metadata exists, but the Kanban MCP does not verify its `capability_token` fields
- **Board templates**: `software-project`, `writing-project`, `scientific-research`, `investment-research`
- **De-jamming**: service support detects and proposes fixes for stuck tasks and stale assignments
- **LLM verification support**: the service can construct and consume an LLM verification result; the MCP `kanban_task_verify` tool completes from non-empty evidence instead
- **Persistence**: boards and tasks stored as RDF hMems via `hMemStore` (MDS §2)

### Dependency Direction

```mermaid
graph TD
    CLI["hkask-cli"] --> SVC["service subcrates"]
    MCP["hkask-mcp-kata-kanban"] --> SVC
    SVC --> KATA["hkask-services-kata-kanban"]
    SVC --> STORAGE["hkask-storage (hMemStore)"]
    SVC -.-> AGENTS["hkask-pods (ActivePods)"]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-003
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

`hkask-mcp-kata-kanban` depends on `hkask-services-kata-kanban` — permitted as a tri-surface for KanbanService.

Kanban operations emit observability through `RegulationSpan::Tool { subsystem: ToolSubsystem::Kanban }`.

See also: [`docs/how-to/skills-and-composition.md`](../../how-to/skills-and-composition.md) (kata-kanban execution boundary and PDCA lifecycle)

---

## Kata — Cybernetic Capability Development

**Crates:** `hkask-services-kata-kanban` (KataEngine, KanbanService, PDCA→task mapping), `hkask-regulation` (variety counters, algedonic alerts), `hkask-storage` (KataHistoryStore)

**Skills:** `.agents/skills/kata-starter/`, `.agents/skills/kata-improvement/`, `.agents/skills/kata-coaching/`, `.agents/skills/kata/` (bundle)

**Templates:** 23 Jinja2 templates across 4 skill directories, 5 YAML manifests, registered from per-skill manifests via `registry/templates/*/manifest.yaml` (auto-discovered at compile time by `build.rs`)

**MCP surface:** Kanban MCP (`hkask-mcp-kata-kanban`) exposes task-scoped Kata prompts. Full Kata execution is available through the CLI `kask kata start` command, which constructs `KataEngine` directly; see the [execution-boundary diagram](../../how-to/skills-and-composition.md#kata-kanban-execution-boundary).

### Summary

Kata implements the Toyota Kata methodology (Rother, 2009) as a cybernetic capability development system. Three independently usable skills compose through a bundle orchestrator, with Regulation observing every practice, PDCA iteration, and coaching session. The kanban MCP surface provides task-based execution for kata experiments.

| Skill | Purpose | Steps | Templates | Manifest |
|-------|---------|-------|-----------|----------|
| **kata-starter** | Build foundational scientific thinking habits | 5 | 5 (4 FlowDef, 1 KnowAct) | `starter-kata.yaml` |
| **kata-improvement** | 4-step PDCA scientific pattern for capability gaps | 4 | 5 (1 FlowDef, 4 WordAct) | `improvement-kata.yaml` |
| **kata-coaching** | 5-question dialogue for teaching scientific thinking | 5 | 6 (1 FlowDef, 5 WordAct) | `coaching-kata.yaml` |

### Key Features

- **PDCA cycle with before/after metrics:** The `KataEngine` captures `metric_before` from Regulation counters, executes the 4-step PDCA pattern, captures `metric_after`, and computes an `ImprovementSignal` (Positive/Negative/Stalled/NotMeasured)
- **Automaticity tracking:** Practice history stored in `data/kata-history.json` + SQLite (`KataHistoryStore`). Automaticity linearly approaches 1.0 over 21 consecutive practice days. 3+ day gaps trigger habit decay alerts
- **Regulation variety counters:** `kata.practices.completed`, `kata.automaticity.score`, `kata.habit.formation` — baseline 5/week, +0.05/week, 1 per 21 days
- **Algedonic alerts:** Variety deficits exceeding threshold (default 100) emit `kata.algedonic` warnings
- **OCAP consent gates:** kata-starter (self-consent), kata-improvement (Curator), kata-coaching (Learner) — per P2 Affirmative Consent
- **Memory integration:** Every step produces a `StepExperience` recorded to episodic memory via dual-encoding pipeline
- **Kanban integration:** PDCA experiments map to kanban tasks; coaching 5 questions map to task fields; improvement cycles tracked as task state transitions

### Kata-Kanban-Regulation Integration

<!-- Content provenance: absorbed from docs/architecture/kata-kanban-integration.md during 2026-06-24 consolidation -->

#### Coaching Loop

```mermaid
sequenceDiagram
    participant Coach
    participant KataEngine
    participant KanbanService
    participant Regulation

    Coach->>KataEngine: Q1: What is the target condition?
    KataEngine->>KanbanService: Read task title, description, criteria
    KanbanService-->>KataEngine: Task { title, description, criteria }
    KataEngine->>Regulation: Emit reg.kata.coaching.q1

    Coach->>KataEngine: Q4: What is your next step? What do you expect?
    KataEngine->>KanbanService: Update task specification, transition → InProgress
    KanbanService->>Regulation: Emit reg.tool.kanban (task moved)

    Coach->>KataEngine: Q5: How quickly can we go and see?
    KataEngine->>KanbanService: Transition → Review, task_verify()
    KanbanService-->>KataEngine: TaskVerified { passed, evidence }

    alt passed
        KanbanService->>KanbanService: Task → Done
    else failed
        KanbanService->>KanbanService: Task → InProgress (rework)
        KataEngine->>Regulation: reg.kata.improv.effectiveness (degradation)
    end
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-004
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

#### PDCA → Kanban State Mapping

| PDCA Phase | Kanban Status | Regulation Event |
|------------|---------------|-----------|
| **Plan** | `Backlog` | task created |
| **Do** | `InProgress` | task moved |
| **Check** | `Review` | task verified |
| **Act** | `Done` | task completed |

Transitions: `Backlog → InProgress` (Q4), `InProgress → Review` (Q5), `Review → Done` (pass), `Review → InProgress` (fail).

#### Regulation Span Trace

```
reg.kata.coaching.q1 → reg.tool.kanban (task created)
reg.kata.coaching.q4 → reg.tool.kanban (Backlog → InProgress)
reg.kata.coaching.q5 → reg.tool.kanban (InProgress → Review)
                   → reg.tool.kanban (TaskVerified)
                   → reg.tool.kanban (Review → Done or InProgress)
reg.kata.improv.effectiveness → variety_feedback → Regulation homeostatic loop
```

#### Error Recovery

| Failure | Recovery |
|---------|----------|
| Verification failure | Task re-enters Q4-Q5 loop; after 3 consecutive failures, `kanban unjam` escalates |
| Task stall (>24h) | `kanban unjam` scans, Regulation variety-deficit alert fires |
| Improv degradation | Switch improv mode, reduce scope, or return to Starter Kata drills |
| Regulation span loss | Buffered in `CyberneticsLoop::process_inbox()`, retried, buffer overflow drops oldest |

---

## LoRA Adapter Lifecycle & Inference Composition

**Crates:** `hkask-mcp-training::adapter` (lifecycle, routing, store), `hkask-types` (Regulation spans), `hkask-services-runtime` (orchestration)

**MCP surface:** Training via `hkask-mcp-training` (17 tools, 5 providers)

**Status:** Active — 48 tests, 45 public functions (17 exposed in lib.rs)

### Summary

`hkask-mcp-training::adapter` manages the full lifecycle of trained LoRA adapters — from training provenance through cloud deployment to cost-tracked inference and teardown. Every operation is OCAP-gated (P4). Every state transition emits a Regulation span (P9). Every adapter has an owner WebID (P12).

| Component | Type | Purpose |
|-----------|------|---------|
| **Expertise** | Domain type | Semantic grounding: what the adapter was trained on (MdsDomain, TrainingProvenance) |
| **TrainedLoRAAdapter** | Domain type | Content-addressed, owner-scoped adapter with 12 fields (id, name, owner WebID, base_model, source, expertise, status, etc.) |
| **AdapterStore** | Persistence | SQLite CRUD via `define_store!` — store, retrieve, list, delete adapters |
| **AdapterRouter** | Composition | Composes adapter + base model + provider via `AdapterPort` trait (6 OCAP-gated methods) |
| **EndpointLifecycle** | Lifecycle | 5-phase state machine: Provisioning → Ready → Active → Draining → Terminated |
| **EndpointGuard** | Teardown | RAII guard ensuring resource cleanup (P5 — no leaked endpoints) |
| **CostModel** | Pricing | Per-provider transparent pricing (P2 affirmative consent) |

### Adapter Lifecycle State Machine

```mermaid
stateDiagram-v2
    [*] --> Provisioning: create_endpoint()
    Provisioning --> Ready: provider confirms deployment
    Ready --> Active: start_inference()
    Active --> Draining: drain_endpoint()
    Draining --> Terminated: all requests complete
    Active --> Terminated: force_terminate()
    Provisioning --> Terminated: deployment failed
    Terminated --> [*]: EndpointGuard drops

    note right of Active
        Cost accrual (P9)
        Budget enforcement
        Regulation: Inference
        Regulation: Gas
    end note

    note right of Draining
        No new requests accepted
        In-flight requests complete
        Regulation: Inference
    end note
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-005
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

### Key Features

- **Content-addressed storage:** Adapters identified by content hash + owner WebID — no anonymous artifacts (P12)
- **Provider abstraction:** `AdapterSource` enum supports HuggingFace repos (extensible); providers: Together AI (real HTTP upload + inference), Runpod (vLLM skeleton), Baseten (vLLM skeleton)
- **Transparent pricing:** `CostModel` per provider — user sees cost before deployment (P2)
- **Budget enforcement:** `EndpointLifecycle` checks cost accrual against budget; `EndpointCostBudgetWarning` Regulation span on threshold breach (P9)
- **OCAP-gated composition:** `AdapterPort` trait exposes 6 methods, each requiring a capability token (P4)
- **RAII teardown:** `EndpointGuard` ensures endpoints are terminated even on panic (P5)

### Dependency Direction

```mermaid
graph TD
    TRAIN["hkask-mcp-training"] --> ADAPTER["hkask-mcp-training::adapter"]
    ADAPTER --> TYPES["hkask-types (Regulation spans, WebID)"]
    ADAPTER --> STORAGE["hkask-storage (define_store!)"]
    ADAPTER --> INFERENCE["hkask-inference (provider routing)"]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-006
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

Adapter and endpoint operations emit observability through `RegulationSpan::Tool { subsystem: ToolSubsystem::Training }`, `RegulationSpan::Inference`, and `RegulationSpan::Gas`.

See also: `docs/user-guides/lora-adapter-store-guide.md`, `docs/guides/lora-training-guide.md`

---

## Daemon & UserPod Server Mode

**Crates:** `hkask-mcp` (daemon transport), `hkask-services-runtime` (daemon handler), `hkask-pods` (AgentMode)

### Summary

UserPods can operate in **server mode**, presenting as MCP servers to IDEs (Zed, VSCode) and other hKask agents. The daemon — a Unix domain socket at `~/.config/hkask/daemon.sock` — mediates authentication, role assignment, capability verification, and dual memory encoding between out-of-process MCP binaries and the in-process agent stack.

### Architecture

```mermaid
graph TD
    subgraph "hKask System (Background Service)"
        AS["AgentService::build()"]
        AP["ActivePods"]
        US["UserStore"]
        IP["InferencePort"]
        DH["ServiceDaemonHandler"]
        DL["DaemonListener<br/>(~/.config/hkask/daemon.sock)"]
        AS --> AP
        AS --> US
        AS --> IP
        AS --> DH
        DH --> DL
    end

    subgraph "MCP Binary (Out-of-Process)"
        BIN["hkask-mcp-research"]
        BIN -->|"1. auth_query"| DL
        BIN -->|"2. assignment_query"| DL
        BIN -->|"3. capability_query"| DL
        BIN -->|"4. store_experience"| DL
    end

    subgraph "Callers"
        IDE["Zed IDE"]
        HK["hKask Agent"]
    end

    IDE -->|"stdio MCP"| BIN
    HK -->|"GovernedTool"| BIN

    DH -->|"check_auth"| US
    DH -->|"check_assignment"| PM
    DH -->|"check_capability"| PM
    DH -->|"store_experience → dual encoding"| PM
    DH -->|"every 10: generate_narrative"| IP
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-007
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

### Startup Flow

1. `kask tui onboarding <userpod>` — authenticate (creates session in UserStore)
2. `kask pod assign <userpod> <role>` — assign MCP role (P4 Gate 2: sovereignty/consent)
3. `kask pod mode <userpod> server -r <role>` — enter server mode (P4 Gate 1: OCAP)
4. IDE spawns MCP binary with `HKASK_MCP_HOST=<userpod>`
5. Binary connects to daemon → auth → assignment → capability → serve

### Memory Flow

- Tool calls → `record_experience()` (fire-and-forget from MCP binary)
- Daemon `store_experience` → dual encoding: episodic (first-person, private) + semantic (third-person, public)
- Every 10 experiences → `generate_narrative()` → inference analyzes session log → stores observations as episodic "narrative"/"thought"
- Existing consolidation pipeline extracts semantic knowledge from both streams

### Agent Modes

| Mode | Behavior | Mutual Exclusion |
|------|----------|-----------------|
| **Chat** | Conversational loop, calls tools via GovernedTool | Cannot coexist with Server (initially) |
| **Server** | Presents as MCP server(s), handles incoming tool calls, records episodic memories | Cannot coexist with Chat (initially) |

Concurrent chat+server mode planned for future release (3-6 months).

### Key Constraints

1. **P4 Dual Gate:** Every MCP server startup requires both capability verification (OCAP token) and assignment verification (sovereignty/consent).
2. **P2 Affirmative Consent:** Passphrase entry via `kask tui onboarding` creates session. Daemon checks session existence — no passphrase stored with MCP binary.
3. **Out-of-process isolation:** MCP binaries communicate with hKask only through the daemon socket. No direct access to ActivePods, memory, or inference.
4. **Mode mutual exclusion (initial):** An agent can be in Chat mode OR Server mode, not both.

---

## ACP UserPod — IDE Agent Presence

**Crate:** `hkask-acp`, **Protocol:** [Agent Client Protocol](https://agentclientprotocol.com) (ACP)

### Summary

hKask agents can present themselves in any ACP-compatible IDE (Zed, VS Code with extensions, JetBrains) via the `hkask-acp` userpod. ACP is an open standard (agentclientprotocol.com) for bidirectional agent↔editor communication — distinct from hKask's internal A2A (Agent-to-Agent) protocol used for inter-agent template dispatch.

The ACP userpod runs as a subprocess spawned by the IDE, communicating via JSON-RPC 2.0 over stdio. It connects to the same daemon socket as MCP servers (`~/.config/hkask/daemon.sock`) for authentication, capability verification, and memory encoding. Inference is routed through hKask's centralized `InferenceRouter`.

### Architecture

```mermaid
graph TD
    subgraph "IDE (Zed, VS Code, etc.)"
        USER["User"]
        ACP_CLIENT["ACP Client"]
        USER --> ACP_CLIENT
    end

    subgraph "hkask-acp (Subprocess)"
        AGENT["HkaskAcpAgent"]
        TRANSPORT["StdioTransport<br/>(JSON-RPC 2.0)"]
        INF["InferenceRouter"]
        AGENT --> TRANSPORT
        AGENT --> INF
    end

    subgraph "hKask System"
        DL["DaemonListener<br/>(daemon.sock)"]
        MEM["MemoryStore<br/>(episodic)"]
    end

    ACP_CLIENT -->|"stdio JSON-RPC"| TRANSPORT
    AGENT -->|"auth + capability + store_experience"| DL
    DL --> MEM
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-008
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

### ACP Protocol vs MCP vs A2A

| Protocol | Direction | Purpose | Implementation |
|----------|-----------|---------|---------------|
| **ACP** (Agent Client Protocol) | Bidirectional IDE ↔ Agent | Streaming agent presence in editor: session lifecycle, content streaming, tool progress, permission requests, plan communication | `hkask-acp` (JSON-RPC 2.0 over stdio) |
| **MCP** (Model Context Protocol) | IDE → Server | Tool invocation: request/response tool calls | `hkask-mcp-*` (16 servers) |
| **A2A** (Agent-to-Agent) | Agent ↔ Agent | Inter-agent template dispatch, memory artifact routing, capability delegation | `hkask-pods::a2a` (A2ARuntime) |

### Prompt Turn Lifecycle

```text
initialize → session/new → session/prompt → [streaming loop] → stop_reason
                                              │
                                              ├─ agent_message_chunk
                                              ├─ tool_call (pending)
                                              ├─ tool_call_update (in_progress)
                                              ├─ tool_call_update (completed)
                                              └─ usage_update
```

The userpod streams inference output as `session/update` notifications while the prompt is processing. The final response carries a structured `StopReason` (`end_turn`, `max_tokens`, `cancelled`).

### How It Reuses Existing Infrastructure

| Capability | Reused Component |
|-----------|-----------------|
| Identity | `WebID` (same identity across REPL, ACP, and MCP surfaces) |
| Authentication | `DaemonClient::auth_query()` (P4 Gate 1) |
| Capability tokens | `verify_startup_gates()` → `A2ARuntime` (P4 Gate 2/3) |
| Memory | `DaemonClient::store_experience()` → dual episodic/semantic encoding |
| Inference | `InferenceRouter` (same provider dispatch as REPL) |
| Observability | Regulation spans: `reg.acp.bridge.latency`, `reg.acp.userpod.memory_size`, `reg.acp.ide.connection_state` |
| Accountability | Every memory hMem carries the userpod's `WebID` as `owner` (P12) |

### Key Constraints

1. **P2 Affirmative Consent:** The ACP userpod never initiates without user invocation. Sessions are created by the IDE (user action), not by the userpod.
2. **1:1 session isolation:** One ACP userpod process = one IDE connection. Concurrent multi-IDE support is gated on usage data (P7 — Evolutionary Architecture).
3. **Surface-independent identity:** An agent registered in hKask uses the same `WebID`, capability tokens, and memory store whether it's accessed via REPL (`kask tui`), ACP (IDE), or MCP (tools).

---

## Deployment

**Authoritative model:** See Deployment Model section above. hKask deploys as a single cloud server. There is no client binary. Users access hKask through a browser terminal (xterm.js + WebSocket). SSH is optional for power users.

**Pod export commands** (`kask pod export-container` / `kask pod export-k8s`):
- `export-container` generates a Containerfile + pod files (SQLCipher DB, WebID, salt) for Docker builds via `PodFactory::export_container()`
- `export-k8s` copies the canonical manifests from `deploy/k8s/` into the output directory — single source of truth for K8s deployment
- The canonical `deploy/k8s/` includes: single-container pod (kask + Conduit + Litestream via supervisord), ConfigMap, Secret, PVC, Service, Ingress with cert-manager TLS, Kustomization
- The Curator init flow (`kask curator init`) calls `export_k8s` to deploy the full system stack

### Cloud Server Deployment

The production deployment is a headless Ubuntu cloud server:

1. **Single binary.** All crates compiled. No Cargo features for client/server.
2. **Browser-first interaction.** Primary interface is xterm.js terminal via browser. Secondary: SSH (`kask repl`). MCP servers (for IDE integration) connect via the REST API or SSH-tunneled socket.
3. **No local GPU inference.** The inference router (`hkask-inference`) routes all requests to cloud providers (DeepInfra, Together AI, fal.ai, OpenRouter, KiloCode).
4. **API keys in OS keychain.** Provider API keys are stored in the OS keychain (Linux Secret Service or flat-file fallback), not in environment variables or plaintext files.
5. **Encrypted database at rest.** All persistent state uses SQLCipher with a passphrase-derived key.
6. **Multi-tenant.** Multiple users per server. Data scoped by `owner_webid`. OAuth (GitHub/Google) sign-in.
7. **Caddy + Conduit sidecars.** Docker containers for TLS termination and Matrix homeserver.

### Provider Configuration

API keys resolve through a 2-tier chain at startup:

| Tier | Source | Security | Persistence |
|------|--------|----------|-------------|
| 1 | OS Keychain | Encrypted at rest by OS | Survives reboot |
| 2 | Environment variable | Plaintext in process memory only | Session-only |

Provider selection via `HKASK_DEFAULT_PROVIDER`:

| Value | Provider | Use Case |
|-------|----------|----------|
| `DI` | DeepInfra | Primary cloud provider |
| `TG` | Together AI | Cloud inference + fine-tuning |
| `FA` | fal.ai | Specialized vision/OCR/media models |
| `OR` | OpenRouter | Multi-provider unified API (200+ models) |
| `KC` | KiloCode | Kilo Gateway unified API (500+ models) |

### Setup Flow

```bash
# One-time setup on cloud server
cp .env.example .env
kask keystore load --path .env --shred
kask matrix deploy-sidecar --domain my-server.example.com
cd ~/.config/hkask/sidecar && docker compose up -d
kask init --profile server
```

### Security Properties

| Property | Mechanism |
|----------|-----------|
| No plaintext secrets on disk | Keys live in OS keychain; source file shredded after load |
| No secrets in environment | `InferenceConfig` reads from keychain at startup |
| Affirmative consent before deletion | `--shred` requires explicit confirmation |
| Graceful degradation | Missing keys → backend unavailable (logged), not crash |
| Multi-user isolation | All data scoped by `owner_webid`; OAuth identity verification |

## Deep-Module Audit — Public Surface Justifications

<!-- Content provenance: absorbed from docs/architecture/PUBLIC_SURFACE_JUSTIFICATIONS.md during 2026-06-24 consolidation -->

**Threshold:** ≤7 public items per crate (Ousterhout deep-module discipline, P5). Exceptions must pass the deletion test with documented rationale.

This audit applies **John Ousterhout's deep-module discipline**[^ousterhout]: every module must pass the deletion test and maintain ≤7 public items.

| Crate | Pub Items | Key Concerns | Justification |
|-------|-----------|-------------|---------------|
| `hkask-types` | 50 | Regulation span registry (100+ variants), WebID, RDF types | Canonical type crate. Regulation spans alone justify the surface. |
| `hkask-test-harness` | 42 | Contract verification, proptest strategies | Testing infrastructure. Each strategy is test-only. |
| `hkask-storage` | 39 | `define_store!` macro, hMemStore, vector store | Persistence orchestration. Each store follows same deep pattern. |
| `hkask-pods` | 26 | ActivePods, AgentRegistry, capability delegation | Multi-concern crate. Each concern independently testable. |
| `hkask-regulation` | 25 | CyberneticsLoop, VarietyTracker, AlgedonicManager | Regulatory surface. Each component is a distinct feedback loop. |
| `hkask-templates` | 22 | Jinja2 rendering, registry, template types | Template engine. Registry, rendering, classification are distinct concerns. |
| `hkask-wallet` | 22 | WalletManager, rJoule, multi-chain bridges | Domain boundary. Keys, balances, deposits are distinct operations. |
| `hkask-inference` | 18 | InferenceRouter, provider backends, budget tracking | Provider abstraction. Each backend scales with provider support. |
| `hkask-mcp-training::adapter` | 17 | Expertise, AdapterStore, AdapterRouter, EndpointLifecycle | Multi-concern spanning types, persistence, lifecycle, routing. |
| `hkask-mcp` | 17 | MCP gateway, capability verification, transport | Protocol surface. Gateway, transport, governance are distinct layers. |
| `hkask-api` | 16 | HTTP router, OpenAPI, endpoint handlers | API surface. Each endpoint group is a resource. |
| `hkask-memory` | 16 | Episodic/semantic memory, narrative generation, port traits (ADR-041) | Memory subsystem. Each memory type is distinct. EpisodicStoragePort + SemanticStoragePort promoted here per ADR-042 (two consumers: agents + services-context). |
| `hkask-keystore` | 11 | Argon2id, OS keychain, SQLCipher | Security crate. Derivation, storage, encryption are distinct concerns. |
| `hkask-services-core` | 32 | `ServiceConfig`, `ServiceError`, identity, verification, settings, goals | Foundation crate. Shared config, error taxonomy, and identity types used by every other service crate. |
| `hkask-services-kata-kanban` | 22 | `KataEngine`, `KataManifest`, `KataResult`, `KanbanService`, `Board`, `Task`, `SpawnSpec`, `KanbanError`, `KataError` | Unified workflow crate (merged from the former kata and kanban service crates). PDCA phases map directly to Kanban task statuses. |
| `hkask-services-corpus` | 23 | `DiscoverService`, `EmbedService`, `CorpusConfig`, entity extraction | Document corpus ingestion, embedding, entity extraction. Each phase (discover, embed, validate) exposes its own config types. |
| `hkask-services-runtime` | 23 | `ServiceDaemonHandler`, provider backends (7), `AdaptiveMonitor` | Runtime orchestration with 7 provider backends, each a distinct type. Provider count drives surface breadth. |
| `hkask-services-skill` | 19 | `SkillAuditor`, `BundleService`, skill discovery, publishing | Skill lifecycle + bundle composition. Audit pipeline exposes health types; bundle exposes composition types. |
| `hkask-services-chat` | 10 | `ChatService`, `MemoryService`, `TurnRequest`, `TurnResult` | Chat session with turn management and memory recall. Request/response pair drives type count. |
| `hkask-services-compose` | 9 | `ComposeService`, `CognitionConfig`, `ComposeRequest`, `ComposeResult` | Style-based prose composition. Config sections (embedding, retrieval, validation) each expose a type. |
| `hkask-services-onboarding` | 8 | `OnboardingService`, `SignInOutcome`, `MatrixRegistrationResult`, `ResolvedSecrets` | First-run onboarding. Registration, secrets resolution, conduit health checks. |
| `hkask-services-context` | 2 (+30 methods) | `AgentService`, `PerAgentMemory` | Shared agent context. `AgentService` is a DI container with 30+ accessor methods — the type count is low but the callable API surface is the largest in the project. Method-level depth metric: 30+ accessors encode zero behavioral depth (all are field returns). Targeted for strangler-fig decomposition (see archived ADR-040). |
| ~~deleted 2026-07-11~~ | ~~2~~ | Metacognition moved to `hkask-services-chat`, escalation CRUD to `hkask-services-context` |
| `hkask-services-wallet` | 1 | `WalletService` | Crypto wallet facade. Single service delegating to `hkask-wallet`. |

**Deletion test:** Every crate above passes — delete it and its complexity reappears duplicated. Public surface reflects breadth of domain concerns, not shallow design. `hkask-services-kata-kanban` (22 items) is the widest services subcrate — it combines the kata engine surface (11 items) and kanban board surface (16 items) into a unified workflow crate where PDCA phases map directly to Kanban task statuses.

## API Documentation (utoipa)

<!-- Content provenance: absorbed from docs/architecture/reference/utoipa-implementation.md during 2026-06-24 consolidation -->

API documentation is auto-generated at build time from type annotations via utoipa. Dependencies: `utoipa 5.5` (with `axum_extras`, `uuid`, `chrono` features), `utoipa-axum 0.2`. All request/response types derive `ToSchema`; endpoints are registered via `OpenApiRouter::new().route()` with utoipa-axum auto-discovery from handler signatures and `ToSchema` derives. Generated artifacts: `docs/generated/openapi.json` (OpenAPI 3.1), `docs/generated/cli-reference.md`. CLI: `kask docs openapi`, `kask docs cli`, `kask docs all`.

**60 registered endpoints** across templates, bots, pods, MCP, Regulation, chat, models, curator, ACP, bundles, specs, episodic, sovereignty, consolidation, git, goals, settings, wallet. All endpoints are registered via `OpenApiRouter::new().route()` and appear in the generated spec via utoipa-axum auto-discovery. MCP tools are discovered dynamically at runtime and are not part of the OpenAPI spec.

Spec CRUD routes (`GET/POST /api/specs`) call `SpecStore` directly through `AgentService::spec_store()` — no intermediate service layer. Spec validation and quality checks run through `kask qa spec-check`.

## Reference Artifacts

Detailed lookup tables and diagrams in `reference/`:

| Artifact | Purpose |
|----------|---------|

| [Curator Persona & Behavioral Specification](#curator-persona--behavioral-specification) | Curator persona specification |


---

## Decision Records

| ADR | Topic |
|-----|-------|
| [`ADRs/ADR-031-consolidation-authorization.md`](../ADRs/ADR-031-consolidation-authorization.md) | Consolidation authorization via master passphrase derivation |
| [`ADRs/ADR-035-userpod-server-mode.md`](../ADRs/ADR-035-userpod-server-mode.md) | UserPod server mode — AgentMode (Chat/Server), daemon socket transport, dual memory encoding, narrative generation |
| [`ADRs/ADR-036-gix-migration.md`](../ADRs/ADR-036-gix-migration.md) | Migration to gix (pure-Rust git) for CAS-backed content-addressed agent storage |
| [`ADRs/ADR-037-blake3-content-addressing.md`](../ADRs/ADR-037-blake3-content-addressing.md) | blake3 content addressing for agent artifacts |
| [`ADRs/ADR-041-dynamic-model-discovery.md`](../ADRs/ADR-041-dynamic-model-discovery.md) | Dynamic model discovery via inference provider catalog |
| [`ADRs/ADR-042-port-promotion-rule.md`](../ADRs/ADR-042-port-promotion-rule.md) | Port trait location — promotion rule (1 consumer = local, 2+ = promote to domain or shared crate) |
| [`ADRs/ADR-058-eliminate-nested-runtime-panics.md`](../ADRs/ADR-058-eliminate-nested-runtime-panics.md) | Eliminate nested runtime panics in Regulation loop executor |
| [`ADRs/ADR-044-ledger-wallet-separation.md`](../ADRs/ADR-044-ledger-wallet-separation.md) | Separation of ledger accounting from wallet operations |
| [`ADRs/ADR-045-cli-bootstrap-strategy.md`](../ADRs/ADR-045-cli-bootstrap-strategy.md) | CLI bootstrap strategy — database initialization and first-run experience |
| [`ADRs/ADR-046-repl-extraction-path.md`](../ADRs/ADR-046-repl-extraction-path.md) | REPL extraction into standalone crate with ReplHost trait |
| [`ADRs/ADR-047-storage-modularization.md`](../ADRs/ADR-047-storage-modularization.md) | Storage crate modularization — 9 sub-crates behind facade |
| [`ADRs/ADR-048-regulation-type-decomposition.md`](../ADRs/ADR-048-regulation-type-decomposition.md) | Regulation type system decomposition — RegulationSpan reduced to 7 variants, domain enums per crate |

**Also present:** [`ADRs/ADR-043-database-driver.md`](../ADRs/ADR-043-database-driver.md) — Database driver abstraction.

**Archived (2026-06-17):** ADR-030, ADR-032–034, ADR-036–037 (6 Draft ADRs, never adopted). **Archived (retroactive, 2026-06-15):** ADR-024–027. Recoverable via git history.

---

## Specifications

| Document | Purpose |
|----------|---------|
| [`REQUIREMENTS.md`](../../specifications/REQUIREMENTS.md) | 22 implemented + 5 deferred goal specs |


---

*Verification commands:* `cargo check --workspace`, `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`. See [`MDS.md`](MDS.md) §9.3 for the verification gate.

---

## Document Structure

```
docs/architecture/
├── tui-multiwindow-plan.md                    # TUI multi-window workspace plan
├── core/
│   ├── hKask-architecture-master.md           # THIS FILE (index + 4 patterns + merged sections)
│   ├── magna-carta.md                         # Foundation (4 sovereignty principles P1–P4)
│   ├── PRINCIPLES.md                          # P1–P12 incl. dual-axis framework
│   ├── MDS.md                                 # 5 categories, 12 tools
│   ├── TESTING_DISCIPLINE.md                  # Testing + QA operations
│   ├── FUNCTIONAL_SPECIFICATION.md            # AgentService functional spec
│   ├── scenarios-companies-bridge.md          # Scenarios ↔ Companies MCP bridge
│   └── toyota-kata-cybernetic-mapping.md       # Kata ↔ cybernetic mapping
├── mandates/
│   └── P12-authenticated-host-mandate.md      # Authenticated host mandate (P12)
└── ADRs/
    ├── _TEMPLATE.md                           # ADR template
    ├── ADR-031-consolidation-authorization.md # Consolidation authorization (master passphrase)
    ├── ADR-035-userpod-server-mode.md         # UserPod server mode (Chat/Server, daemon socket)
    ├── ADR-036-gix-migration.md               # Migration to gix (pure-Rust git)
    ├── ADR-037-blake3-content-addressing.md   # BLAKE3 content addressing
    ├── ADR-041-dynamic-model-discovery.md     # Dynamic model discovery via provider catalog
    ├── ADR-042-port-promotion-rule.md         # Port trait location — promotion rule
    ├── ADR-043-database-driver.md             # Database driver abstraction
    ├── ADR-044-ledger-wallet-separation.md     # Ledger accounting ↔ wallet operations
    ├── ADR-045-cli-bootstrap-strategy.md       # CLI bootstrap — DB init, first-run
    ├── ADR-046-repl-extraction-path.md        # REPL extraction into standalone crate
    ├── ADR-047-storage-modularization.md      # Storage crate modularization
    ├── ADR-048-regulation-type-decomposition.md  # RegulationSpan → 7 variants + domain enums
    ├── ADR-049-ledger-database-driver-compliance.md  # Ledger DB driver compliance
    ├── ADR-050-ontology-anchored-embedding.md # Ontology-anchored embedding
    ├── ADR-051-sqlite-vector-storage-optimization.md  # SQLite vector storage optimization
    ├── ADR-052-embedding-vector-search-scaling.md  # Embedding vector search scaling
    ├── ADR-053-corpus-input-guard-toggle.md    # Corpus input guard toggle
    ├── ADR-054-services-core-foundation-scope.md  # services-core foundation scope
    ├── ADR-055-per-provider-circuit-breaker.md # Per-provider circuit breakers
    ├── ADR-056-ashby-aligned-loop-regulation.md  # Ashby-aligned loop regulation
    ├── ADR-057-cybernetic-naming-ontology.md   # Cybernetic naming ontology
    ├── ADR-058-eliminate-nested-runtime-panics.md  # Eliminate nested runtime panics
    ├── ADR-059-tool-calls-deserialization-fix.md  # Tool-calls deserialization fix
    ├── ADR-060-memory-recall-dedup-single-layer.md  # Memory recall dedup (single layer)
    └── qwen36-chat-template-thinking-bug.md    # Qwen3 chat-template thinking bug (post-mortem)
```

**Active on disk (2026-07-23):** 8 core files + 1 root file + 1 mandate + 25 ADRs (ADR-031..060 + template + post-mortem) = 35 documents.

**Merged into this document:** Database Providers, Matrix Integration, Well & Wallet, Scenarios–Companies Bridge, Federation v2, Storage Schema ERD, Multi-User Data Model, Database Driver, Companies MCP routing/forecast/portfolio, and Scenario Forecasting Pipeline — all inlined as `## … (Merged from …)` sections below. Earlier absorptions (2026-06-24): loop-architecture, energy-gas-payments-api-keys, self-healing, provider-intelligence; Pod and Curator content from SOLID_POD_ISOMORPHISM, MULTI_POD_ARCHITECTURE, and hKask-Curator-persona.

**Directories consolidated (2026-06-24):** `reference/`, `specs/` no longer exist as separate dirs under `architecture/` — content absorbed or archived. `mandates/` retained (P12 mandate).

---

*ℏKask v0.31.0 - A Minimal Viable Container for Users and AI Tools*

[^ousterhout]: Ousterhout, J. (2018). *A Philosophy of Software Design*. Yaknyam Press.
[^conway]: Conway, M. E. (1968). "How Do Committees Invent?" Datamation, 14(4), 28-31.
[^miller-ocap]: Miller, M. S. (2006). *Robust Composition: Towards a Unified Approach to Access Control and Concurrency Control*. Johns Hopkins University.

---

## Database Providers (Merged from database-providers.md)


# Database Provider Selection

Select your database via HKASK_DB_PROVIDER env var.
Both providers support all storage: memory (h_mems), embeddings, wallet, registry.

## Encryption

hKask encrypts all agent memory at rest. The mechanism depends on provider:

- **SQLite**: SQLCipher (AES-256) — automatic when HKASK_DB_PASSPHRASE is set.
  Database file is encrypted; no plaintext ever touches disk.
- **PostgreSQL**: Client-side AES-256-GCM encryption of text values.
  When `HKASK_DB_PASSPHRASE` is set, all `DbValue::Text` params are
  encrypted before transmission and decrypted on retrieval. The
  encryption key is derived from the passphrase via BLAKE3. No plaintext
  values ever appear in the database or transit logs. Admin should also
  configure server-side TLS (`sslmode=require` in connection URL) for
  defense-in-depth.

## SQLite (Default, Stable)

No additional installation needed. SQLCipher encryption is built in.

export HKASK_DB_PROVIDER=sqlite
kask start

## PostgreSQL (Planned v0.32)

Requires PostgreSQL 15+ with pgvector extension.

export HKASK_DB_PROVIDER=postgres
export HKASK_DB_PATH=postgresql://user:pass@localhost:5432/hkask
kask start

## Migration (SQLite → PostgreSQL)

Planned for v0.32: export JSON, import via provider-specific INSERT,
re-index vectors via pgvector. Not available yet.

---

## Matrix Integration Architecture (Merged from matrix-integration-architecture.md)


# Matrix Integration Architecture for hKask

**Date:** 2026-06-14
**Status:** Research Report — Architectural Recommendation
**Domain:** Communication Transport, Agent Enablement
**MDS Categories:** architecture/design, specification/protocol
**Skills Applied:** Essentialist (3-gate eliminative review), Grill-Me (Socratic interrogation), Pragmatic Semantics (epistemic classification), Pragmatic Cybernetics (feedback loop analysis)
**Grounded In:** PRINCIPLES.md (P1–P12), Loop Architecture (§2–§4), Hexagonal Boundaries (§3), `mcp-servers/hkask-mcp-communication/src/matrix.rs` (current stubs), `crates/hkask-services-chat/src/` (ChatService pipeline), `crates/hkask-repl/src/lib.rs` (REPL loop)

---

## 0. Current State (Evidence — Directly Stated)

**Status: The stubs have been deleted and replaced with a real implementation.**

The former `mcp-servers/hkask-mcp-communication/src/matrix.rs` (303 lines of zero-behavior code) has been replaced by the `hkask-communication` core infrastructure crate with 952 LOC of behavior-encoding code plus 652 LOC of tests:

| Module | Lines | Purpose |
|--------|-------|---------|
| `matrix.rs` | 596 | `MatrixTransport` — matrix-sdk wrapper: login, send/receive messages, create rooms, invite users, list rooms, upload/send files |
| `listener.rs` | 191 | `SevenR7Listener` — passive room observer, polls rooms on configurable interval, persists Regulation RegulationRecords for curation awareness |
| `agent_registration.rs` | 152 | `AgentRegistry` — WebID↔UserId mapping, thread watchlists, deregistration |
| `lib.rs` | 13 | Crate root — public module declarations |
| `tests/` | 652 | Integration tests (marked `#[ignore]`, require running Conduit) + unit tests for MXID derivation |

**Implemented pipeline:** Matrix message arrives → 7R7 Listener polls → transport retains its stable Matrix `event_id` → listener ignores self-authored messages and persists the derived ν-event exactly once → `CurationLoop::sense()` filters communication events from `RegulationArchive` and pushes directly to curation context → `MetacognitionLoop` evaluates via the CAT engagement gate → response dispatches through `MatrixTransport::send_message()`.

**Deferred:** E2EE (SQLCipher/SQLite linking conflict with matrix-sdk-sqlite), continuous sync (v1 uses on-demand polling via `get_messages()`).

The original stub design declared intent to embed Conduit as a library dependency. The replacement follows the Docker sidecar + SDK integration architecture recommended in §8.3.

### 0.1 Implementation Status Checklist

| Capability | Status | Notes |
|-----------|--------|-------|
| `MatrixTransport::new()` / `health_check()` / `login()` | ✅ Implemented | matrix-sdk client lifecycle, homeserver reachability |
| `MatrixTransport::send_message()` | ✅ Implemented | Plain text + structured JSON payloads |
| `MatrixTransport::get_messages()` | ✅ Implemented | On-demand room poll with `/sync` |
| `MatrixTransport::create_room()` / `invite_user()` / `list_rooms()` | ✅ Implemented | Room lifecycle operations |
| `MatrixTransport::upload_file()` / `send_file()` | ✅ Implemented | File attachment support (deferred in original spec) |
| `SevenR7Listener` (passive room observer) | ✅ Implemented | Configurable poll interval, self-message filtering, and idempotent Regulation ν-event persistence keyed by Matrix event ID |
| `AgentRegistry` (WebID↔UserId mapping, watchlists) | ✅ Implemented | record, deregister, resolve, watchlist operations |
| Regulation bridge (RegulationRecord persistence) | ✅ Implemented | Messages flow once from listener → RegulationArchive → curation inbox; repeated timeline polls are ignored |
| CAT engagement gate | ✅ Implemented | `convergence_bias` scalar in agent config |
| Response dispatch (agent → Matrix room) | ✅ Implemented | `MatrixTransport::send_message()` via daemon |
| `kask matrix deploy-sidecar` | ✅ Implemented | Docker Compose + Caddyfile + conduit.toml generation |
| `kask matrix register --agent` | ✅ Implemented | Agent registration on Conduit, credential check, QR code |
| `kask matrix register --user` | ✅ Implemented | Human account creation on Conduit |
| `kask matrix listen` | ✅ Implemented | Starts 7R7 listener for an agent |
| `kask matrix status-sidecar` | ✅ Implemented | Docker health + HTTP poll + SQLite integrity |
| `kask matrix verify-device` | ⬜ Deferred | SAS/QR verification for human onboarding |
| Daemon periodic sidecar health task | ⬜ Deferred | 60s container poll → Regulation span emission |
| E2EE (end-to-end encryption) | ⬜ Deferred | Blocked on SQLCipher/SQLite linking conflict |
| Continuous sync (event-driven listener) | ⬜ Deferred | v1 uses on-demand polling; continuous sync deferred until VOIP/real-time use case |
| Integration tests (Conduit-dependent) | ✅ Written (ignored) | 652 LOC in `tests/`; require running Conduit sidecar |
| Unit tests (MXID derivation) | ✅ Implemented | Name transformation edge cases |

---

## 1. Deployment Model

### 1.1 Assumption

hKask core is deployed as a **cloud server** (bare metal or containerized). The primary human interface is a **mobile Matrix client** (FluffyChat recommended). Humans do not SSH into the server. They interact with their agents through Matrix rooms on their phones.

### 1.2 Physical Topology

```
┌──────────────────────────────────────────────────────────────┐
│                    CLOUD SERVER (hKask install)               │
│                                                              │
│  ┌──────────────────┐  ┌──────────────────────────────────┐  │
│  │  Caddy            │  │  hKask Core                      │  │
│  │  (Docker)         │  │  (daemon + MCP servers + agents) │  │
│  │  TLS + proxy      │  │                                  │  │
│  │  ports 80, 443    │  │  hkask-mcp-communication         │  │
│  └────────┬──────────┘  │  └─ MatrixTransport (SDK)        │  │
│           │             │                                  │  │
│  ┌────────┴──────────┐  │  Curator → Agent Pods             │  │
│  │  Conduit           │  │                                  │  │
│  │  (Docker)         │◄─┤  (localhost:8008)                │  │
│  │  homeserver       │  └──────────────────────────────────┘  │
│  │  localhost:8008   │                                      │
│  └────────┬──────────┘                                      │
│           │                                                 │
│  ┌────────┴──────────┐                                      │
│  │  Hydrogen         │  (optional — lightweight web client) │
│  │  (Docker)         │                                      │
│  │  localhost:80     │                                      │
│  └───────────────────┘                                      │
└──────────────────────────────────────────────────────────────┘
          │
          │ HTTPS (TLS + Matrix protocol + E2EE)
          │
    ┌─────┴──────┐
    │            │
┌───▼───┐  ┌─────▼──────┐
│ Human │  │  Human     │
│ phone │  │  laptop    │
│       │  │            │
│ Fluffy│  │  Element   │
│ Chat  │  │  Desktop   │
└───────┘  └────────────┘
```

### 1.3 Sidecar Orchestration

The sidecar requires **one Docker container** (Conduit) plus an **optional lightweight web client** (Hydrogen). `kask matrix deploy-sidecar` generates and manages it via `docker compose`:

```yaml
# Generated by: kask matrix deploy-sidecar --domain matrix.example.com
# Location: ~/.config/hkask/sidecar/docker-compose.yml

version: "3.8"
services:
  # ── TLS reverse proxy (auto-Let's Encrypt) ──
  caddy:
    image: caddy:2-alpine
    container_name: caddy-hkask
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - ./caddy-data:/data
    networks:
      - sidecar-net

  # ── Matrix homeserver ──
  conduit:
    image: registry.gitlab.com/famedly/conduit:0.9.0
    container_name: conduit-matrix
    restart: unless-stopped
    expose:
      - "8008"                     # Internal only — Caddy proxies externally
    volumes:
      - ./conduit-data:/var/lib/conduit
      - ./conduit.toml:/etc/conduit/conduit.toml:ro
    environment:
      - CONDUIT_CONFIG=/etc/conduit/conduit.toml
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8008/_matrix/client/versions"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 15s
    networks:
      - sidecar-net

  # ── Optional lightweight web client ──
  hydrogen:
    image: element-hq/hydrogen-web:latest
    container_name: hydrogen-client
    restart: unless-stopped
    expose:
      - "80"                        # Internal only — Caddy proxies externally
    volumes:
      - ./hydrogen-config.json:/app/config.json:ro
    healthcheck:
      test: ["CMD", "wget", "--no-verbose", "--tries=1", "--spider", "http://localhost:80"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 5s
    networks:
      - sidecar-net
    profiles:
      - with-web-client

networks:
  sidecar-net:
    driver: bridge
```

**Caddyfile** (generated alongside docker-compose.yml):

```
# Generated by: kask matrix deploy-sidecar --domain matrix.example.com
matrix.example.com {
    # Matrix client API + .well-known delegation
    reverse_proxy /_matrix/* conduit:8008
    reverse_proxy /_well-known/* conduit:8008
    # Hydrogen web client (if enabled)
    reverse_proxy /* hydrogen:80
}
```

**Why Caddy.** Caddy is a single ~20 MB Go binary that auto-obtains and renews Let's Encrypt certificates with zero configuration. It handles TLS termination, `/.well-known` delegation, and reverse proxying in one container. The admin runs `kask matrix deploy-sidecar --domain matrix.example.com` and gets a working HTTPS Matrix homeserver — no manual nginx config, no certbot cron jobs, no DNS validation scripts. This resolves Gap B1 (TLS automation) and B3 (`.well-known` delegation) from §12.8.

**Sidecar total footprint:** ~95 MB (Caddy ~20 MB + Conduit ~50 MB + Hydrogen ~5 MB + overhead). Still fits comfortably in 1–2 GB RAM.

**hKask does not maintain Conduit, Hydrogen, or Caddy.** It generates configuration files and docker-compose orchestration. The user pulls the upstream container images. hKask's responsibility ends at config generation and health-check tooling (`kask matrix status-sidecar`).

**Generated `conduit.toml` defaults** (resolves Gap B4 from §12.8):

| Setting | Default | Notes |
|---------|---------|-------|
| `server_name` | From `--domain` flag (e.g., `matrix.example.com`) | |
| `allow_registration` | `true` | Admin can disable after creating human accounts |
| `admin_token` | Random 64-char hex string | `hkask-keystore` stores it; `kask matrix register` uses it |
| `well_known_client` | `https://matrix.example.com` | Conduit serves it, Caddy proxies it |
| `max_request_size` | `20_000_000` (20 MB) | Conduit default |
| `db_path` | `/var/lib/conduit/conduit.db` | Docker volume mount |
| `log_level` | `info` | |
| `port` | `8008` | Internal only — Caddy proxies externally |
| `address` | `0.0.0.0` | Bind to all interfaces within Docker network |

**Why Hydrogen, not Element Web.** Element Web is a ~200 MB Docker image with a heavy JavaScript bundle — it's a full-featured client that belongs on a laptop, not in a sidecar. Hydrogen is a lightweight Matrix web client (~5 MB compressed, WASM-based, uses the same `matrix-rust-sdk` under the hood). It provides basic chat, E2EE, and SAS verification without the bloat. It's the right tool for "I need to check my agent from a browser quickly" — not a daily driver. Daily driving happens on FluffyChat (mobile) or Element Desktop (laptop).

### 1.4 Why Conduit (Not Synapse) for the Sidecar

| Property | Conduit | Synapse |
|----------|---------|---------|
| **Memory** | ~50 MB idle | ~500 MB idle |
| **Disk** | ~20 MB binary | ~100 MB + Python deps |
| **Database** | SQLite (single file) | PostgreSQL (separate service) |
| **Docker complexity** | 1 container | 2+ containers (Synapse + Postgres) |
| **Maturity** | Production-ready, growing | Battle-tested, 10+ years |
| **Rust-native** | Yes — same language as hKask | Python + Rust extensions |
| **Sidecar total (with Caddy + Hydrogen)** | ~95 MB (Caddy ~20 MB + Conduit ~50 MB + Hydrogen ~5 MB + overhead) | ~700 MB (Synapse ~500 MB + Postgres ~200 MB + Element Web ~200 MB) |

Conduit's minimal footprint makes it ideal for a sidecar. A cloud server running hKask + Conduit + optional Hydrogen fits comfortably in 1–2 GB RAM. Synapse + PostgreSQL + Element Web would require 2–4 GB before hKask even starts. The entire hKask + Conduit + Hydrogen stack is lighter than Synapse alone.

### 1.5 Sidecar Health Monitoring

hKask monitors sidecar health at three layers. Each layer has a different **actuator** — the entity that can fix a detected problem.

#### Layer 1 — Docker Healthchecks (Automatic Restart)

Docker's built-in healthcheck mechanism provides **container-level liveness monitoring**. The `docker-compose.yml` generated by `kask matrix deploy-sidecar` includes healthchecks for both containers:

- **Conduit:** `curl -f http://localhost:8008/_matrix/client/versions` every 30s. This endpoint returns supported Matrix spec versions — if Conduit is alive, it responds.
- **Hydrogen:** `wget --spider http://localhost:80` every 30s. Hydrogen is a static web server — if nginx is serving, it responds.

Docker tracks health status as `starting` → `healthy` → `unhealthy`. The `restart: unless-stopped` policy does NOT auto-restart on unhealthy — it only restarts on exit. For auto-restart on health failure, the admin can change to `restart: always` and add `--exit-on-unhealthy` to the container command (Conduit supports this).

**Actuator:** Docker itself. hKask does not control this layer — it observes via `docker compose ps`.

#### Layer 2 — hKask Periodic Health Poll (Observe + Alert)

hKask's daemon runs a **periodic health check task** (every 60s) that:

1. **Container status:** Runs `docker compose -f ~/.config/hkask/sidecar/docker-compose.yml ps --format json` and parses container state (Caddy, Conduit, Hydrogen if enabled).
2. **Conduit API health:** `GET http://localhost:8008/_matrix/client/versions` — verifies Conduit responds with valid Matrix versions JSON.
3. **SQLite integrity:** Runs `sqlite3 conduit-data/conduit.db "PRAGMA integrity_check"` on the Conduit data volume — verifies the database is not corrupt.
4. **Caddy health:** `GET http://localhost:80` and `GET https://localhost:443` (with TLS verification disabled for localhost) — verifies Caddy is proxying.
5. **Hydrogen health:** `GET http://localhost:80` — verifies the web server responds (only if Hydrogen profile is active; shares port 80 with Caddy's internal routing).

Each check emits a `reg.communication.matrix.sidecar.health` ν-event with payload:

```json
{
  "caddy_up": true,
  "conduit_up": true,
  "conduit_api_ok": true,
  "conduit_db_ok": true,
  "hydrogen_up": true,
  "checked_at": "2026-06-14T12:00:00Z"
}
```

**Algedonic thresholds:**
- `caddy_up: false` for 3 consecutive checks → Critical alert (TLS is down, Matrix unreachable)
- `conduit_up: false` for 1 check → Warning alert
- `conduit_up: false` for 3 consecutive checks → Critical alert
- `conduit_db_ok: false` → Immediate Critical alert (database corruption is urgent)
- `hydrogen_up: false` → Warning only (Hydrogen is optional)

**Actuator:** The Curator receives the alert but **cannot restart Docker containers** — that's outside the OCAP boundary. The Curator escalates to the human via the algedonic pathway. The human (admin) must act. See §12.5 Loop 1 for the open question about notification when Matrix itself is down.

#### Layer 3 — Matrix Sync Health (Continuous Transport Monitoring)

The `matrix-rust-sdk` `SyncService` running in `hkask-mcp-communication` provides **continuous transport-level monitoring**. This is not a periodic poll — it's the live sync connection itself.

- `reg.communication.matrix.sync.health`: Emitted on each sync cycle. Payload: `{ connected: bool, latency_ms, last_sync_token }`.
- `reg.communication.matrix.sync.stalled`: Emitted when the sync connection fails. Payload: `{ stall_duration_s, last_error }`.

**Algedonic thresholds:**
- `sync.stalled` for 60s → Warning (transient network blip)
- `sync.stalled` for 300s → Critical (likely outage)

**Actuator:** The Curator CAN act on this layer — it can throttle sync frequency, pause non-critical rooms, or trigger a reconnect. This is the only monitoring layer where hKask has a direct actuator within its OCAP boundary.

#### Monitoring Architecture Diagram

```mermaid
graph TD
    subgraph Docker["Docker Engine (external)"]
        HC[Docker Healthchecks<br/>every 30s]
        CADDY[Caddy Container<br/>TLS + proxy]
        CONDUIT[Conduit Container]
        HYDROGEN[Hydrogen Container]
    end
    
    subgraph hKask["hKask Core"]
        DAEMON[Daemon<br/>periodic health task<br/>every 60s]
        MCP[hkask-mcp-communication<br/>SyncService]
        Regulation[Cybernetics Loop<br/>reg.communication.matrix.*]
        CURATOR[Curator]
    end
    
    subgraph Human["Human Admin"]
        CLI[kask matrix status-sidecar<br/>on-demand check]
        ACTION[Restart containers<br/>Restore database<br/>Check logs]
    end
    
    HC -->|health status| CADDY
    HC -->|health status| CONDUIT
    HC -->|health status| HYDROGEN
    
    DAEMON -->|HTTP poll| CADDY
    DAEMON -->|HTTP poll + SQLite check| CONDUIT
    DAEMON -->|HTTP poll| HYDROGEN
    DAEMON -->|emit ν-event| Regulation
    
    MCP -->|sync health| Regulation
    
    Regulation -->|algedonic alert| CURATOR
    CURATOR -->|escalate| Human
    
    CLI -->|docker compose ps + HTTP + SQLite| CADDY
    CLI -->|docker compose ps + HTTP + SQLite| CONDUIT
    CLI -->|docker compose ps + HTTP| HYDROGEN
    CLI -->|human reads| Human
    Human -->|docker restart / restore| ACTION
    ACTION -->|fixes| CADDY
    ACTION -->|fixes| CONDUIT
    ACTION -->|fixes| HYDROGEN
    
    style Docker fill:#fff3e1
    style hKask fill:#e1f5ff
    style Human fill:#f3e1ff
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-009
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

#### `kask matrix status-sidecar` (On-Demand CLI)

The admin can run a manual health check at any time:

```bash
kask matrix status-sidecar
```

Output:
```
  Caddy:         UP (healthy, TLS serving, proxying)
  Conduit:       UP (healthy, API responding, DB integrity OK)
  Hydrogen:      UP (healthy, HTTP responding)
  Sync:          CONNECTED (latency 45ms, last sync 2s ago)
  Last alert:    none
```

If something is wrong:
```
  Caddy:         UP (healthy, TLS serving, proxying)
  Conduit:       UP (unhealthy — API not responding, DB integrity OK)
  Hydrogen:      DOWN (container exited)
  Sync:          STALLED (287s, last error: connection refused)
  Last alert:    CRITICAL — sync stalled 2026-06-14T11:58:00Z
```

This command does NOT fix anything. It only reports. The admin decides what action to take based on the output.

---

## 2. Client-Side Orchestration

### 2.1 Approved Clients

hKask does not control what client humans use. It **recommends** clients that support the features required for secure agent communication:

| Client | Platform | E2EE | SAS Verify | Cross-Signing | Recommendation |
|--------|----------|------|------------|---------------|----------------|
| **FluffyChat** | Android, iOS | ✅ Olm/Megolm | ✅ Emoji SAS | ✅ | **Primary — mobile-first, beautiful UX** |
| **Element X** | Android, iOS | ✅ Olm/Megolm | ✅ Emoji SAS | ✅ | **Alternative mobile — Rust-native, fast sync** |
| **Hydrogen** | Browser | ✅ Olm/Megolm | ✅ Emoji SAS | ✅ | **Optional sidecar — lightweight (~5 MB), WASM-based, for quick browser access** |
| **Element Desktop** | Windows, macOS, Linux | ✅ Olm/Megolm | ✅ Emoji SAS | ✅ | **Desktop option** |
| **iamb** | Terminal | ⚠️ Limited | ⚠️ Limited | ⚠️ Limited | **Not recommended — insufficient E2EE support for agent verification** |
| **Element Web** | Browser | ✅ Olm/Megolm | ✅ Emoji SAS | ✅ | **Not recommended for sidecar — ~200 MB, heavy JS bundle. Use Hydrogen for browser, Element Desktop for laptop.** |

All recommended clients use `matrix-rust-sdk` under the hood (FluffyChat uses Dart bindings to the same crypto primitives). This ensures E2EE compatibility with hKask agents, which also use `matrix-rust-sdk`.

### 2.2 Key Management Architecture

Matrix E2EE involves four key types. Here is who manages each:

| Key Type | Purpose | hKask Agent | Human User |
|----------|---------|-------------|------------|
| **Device key (Ed25519)** | Signs messages, proves device identity | Stored in `hkask-keystore` (AES-256-GCM, OS keychain) | Stored in FluffyChat/Element local storage |
| **Olm session keys** | Per-device double ratchet | Stored in `hkask-keystore` via custom `CryptoStore` impl | Managed by client SDK internally |
| **Megolm session keys** | Per-room group ratchet | Stored in `hkask-keystore` via custom `CryptoStore` impl | Managed by client SDK internally |
| **Cross-signing keys** | Prove all devices belong to same user | Stored in `hkask-keystore` | Stored in client; user backs up recovery key |
| **Recovery key (AES-256)** | Decrypts all message keys if device lost | Stored in `hkask-keystore`; exportable for admin backup | User writes down / stores in password manager |

**Critical design rule:** hKask never sees the human's E2EE keys. The human's keys live in their Matrix client (FluffyChat). hKask only manages its own agents' keys. The trust boundary is the Matrix protocol itself — E2EE encrypts messages so that only the intended devices can decrypt them, regardless of who runs the homeserver.

### 2.3 Identity Binding: How the Human Knows They're Talking to THEIR UserPod

**The user has already completed hKask onboarding before installing FluffyChat.** They have a userpod identified by a **full name** (first and last, e.g., "Alice-Smith") and a **passphrase**. The userpod credential is the compound string `FirstName-LastName/Passphrase` (or `FirstName-LastName-Passphrase` — the separator `-` or `/` is configurable per install).

**No AI assistance during authentication.** This is a **Prohibition-level constraint**. The credential check is a direct, local string comparison — no LLM involvement, no Curator mediation, no "smart" fuzzy matching. The human must recognize their userpod's exact full name. The system must match the exact credential string. AI assistance at this step would undermine the security anchor: the human's personal knowledge of their own userpod identity.

**Future authentication paths (out of scope for v1):** Google ID and GitHub ID OAuth login could replace the name/passphrase system, allowing users to authenticate with existing identity providers. This would eliminate the need to memorize a separate passphrase. But it adds OAuth infrastructure, token management, and provider dependency — unnecessary complexity for the initial integration.

**The question:** when the human opens FluffyChat and sees `@alice-smith:example.com`, how do they know that Matrix account is THEIR userpod and not an impersonator?

**The binding has two layers, both verifiable by the human without AI assistance:**

| Layer | What It Proves | How the Human Verifies |
|-------|---------------|----------------------|
| **Exact name match** | `@alice-smith:example.com` IS the userpod "Alice-Smith" | Human recognizes their userpod's exact full name from onboarding. The Matrix user ID is the userpod's first-last name, lowercased, verbatim. No fuzzy matching. No AI suggestion. The human looks at the name and knows it. |
| **SAS verification** | The device behind `@alice-smith:example.com` controls the private key for that account | Human scans QR code (or compares emoji). FluffyChat cryptographically verifies the device key. No AI involvement — this is standard Matrix protocol cryptography. |

**Together, these prove:** "The Matrix account @alice-smith:example.com is controlled by a device whose key I have cryptographically verified, and the name exactly matches my userpod Alice-Smith. Therefore, this is my userpod."

**The credential gates registration.** `kask matrix register --agent Alice-Smith` prompts for the full credential string `Alice-Smith/Passphrase`. hKask verifies it locally against the userpod's stored credential — a direct string comparison, no AI. This ensures only someone who already knows the full credential can bind that userpod name to a Matrix identity.

**Matrix User ID derivation.** The userpod's full name is transformed into a Matrix MXID (`@localpart:example.com`) using these rules:

1. Lowercase the full name (e.g., "Alice-Smith" → "alice-smith")
2. Replace spaces with hyphens (e.g., "Alice Smith" → "alice-smith")
3. Strip any character not in `[a-z0-9-._=]` (the Matrix localpart allowed character set)
4. If the result is empty after stripping, use `agent-{random-8-hex}` as fallback
5. If the result conflicts with an existing MXID on the homeserver, append a numeric suffix (`-2`, `-3`, ...)
6. Validate that the result is ≤ 255 characters (Matrix localpart limit)

Examples:
- `Alice-Smith` → `@alice-smith:example.com`
- `Bob Jones` → `@bob-jones:example.com`
- `María García` → `@mara-garca:example.com` (unicode stripped)
- `O'Reilly` → `@oreilly:example.com` (apostrophe stripped)

This resolves Gap B2 (MXID format specification) from §12.8.

#### What Could Go Wrong (Threat Model)

| Attack | Feasible? | Mitigation |
|--------|-----------|------------|
| **Admin registers wrong name** (e.g., registers "Alice-Smith" but human's userpod is "Bob-Jones") | Yes — admin controls the server | Human sees wrong name in FluffyChat → doesn't recognize it → doesn't scan QR. Exact name mismatch is self-evident. No AI can talk the human out of recognizing their own userpod's name. |
| **Another Matrix user registers `@alice-smith:example.com` before the admin does** | Yes — if open registration is enabled | Admin registers the agent BEFORE enabling open registration. Or: keep registration closed, use `kask matrix register --agent` (admin API, no race). |
| **Malicious homeserver strips E2EE** | Yes — Conduit could serve unencrypted messages | FluffyChat rejects unencrypted messages in encrypted rooms. The agent's SDK does the same. E2EE is enforced client-side. |
| **Impersonator creates `@alice-smith:matrix.org` on a different homeserver** | Yes — but different domain | Human connects to `matrix.example.com`, not `matrix.org`. The full MXID is `@alice-smith:example.com`. Different domain = different user. FluffyChat shows the full MXID. |
| **QR code intercepted in transit** | Yes — if admin shares QR over insecure channel | QR code must be shared through a trusted channel. If admin and human are the same person, scan directly from terminal. If different, use an already-verified encrypted channel (Signal, existing Matrix DM). QR code is single-use — after SAS completes, it has no value. |
| **Human scans wrong QR code** (e.g., from a different agent) | Yes — human error | QR code is labeled: "UserPod: Alice-Smith — scan this in FluffyChat to verify your agent." Human reads the label before scanning. |
| **AI attempts to assist with verification** | Yes — Curator or agent could try to "help" | **Prohibition.** The credential check is a direct string comparison with no AI involvement. The `kask matrix register` command does not invoke the Curator or any LLM. The human's name recognition is their own cognitive act — no AI mediates it. |

#### Security Surface Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                     TRUST BOUNDARIES                         │
│                                                              │
│  ┌──────────────────────┐                                    │
│  │  Admin (trusted)      │  Knows full credential            │
│  │  Runs: kask matrix    │  Alice-Smith/Passphrase           │
│  │  register --agent     │  Controls server                  │
│  │  Alice-Smith          │  Shares QR code securely          │
│  └──────────┬───────────┘                                    │
│             │ registers @alice-smith:example.com            │
│             ▼                                                │
│  ┌──────────────────────┐                                    │
│  │  Conduit (semi-trusted)│  Trusted for: identity mapping    │
│  │                       │  UNtrusted for: message content   │
│  │  Maps username → acct │  (E2EE protects content)          │
│  └──────────┬───────────┘                                    │
│             │                                                │
│  ┌──────────┴───────────┐                                    │
│  │                      │                                    │
│  ▼                      ▼                                    │
│  ┌──────────────┐  ┌──────────────┐                          │
│  │ hKask Agent   │  │ Human        │                          │
│  │ @alice-smith: │  │ @bob-jones:  │                          │
│  │ example.com   │  │ example.com  │                          │
│  │               │  │              │                          │
│  │ Device key    │  │ FluffyChat    │                          │
│  │ in keystore   │  │ verifies via  │                          │
│  │               │  │ SAS (QR scan) │                          │
│  └──────────────┘  └──────────────┘                          │
│         │                   │                                │
│         │    E2EE (Olm/Megolm)                               │
│         └───────────────────┘                                │
│                                                              │
│  TRUST ANCHOR: Human recognizes exact full name + SAS proof  │
│  CREDENTIAL GATE: Full credential required, no AI involved   │
│  PROHIBITION: No LLM/Curator mediation of authentication     │
└──────────────────────────────────────────────────────────────┘
```

### 2.4 User Instructions: Connecting FluffyChat to hKask

**There are two roles in setup:** the **server admin** (person who runs the cloud server and has SSH access) and the **human user** (person on a phone with FluffyChat). They may be the same person or different people.

#### Phase 1 — One-Time Server Setup (Admin)

```
1. Admin deploys the sidecar:
   kask matrix deploy-sidecar --domain matrix.example.com
   → Generates ~/.config/hkask/sidecar/ with:
     - docker-compose.yml (Caddy + Conduit + optional Hydrogen)
     - Caddyfile (auto-TLS reverse proxy config)
     - conduit.toml (homeserver config with random admin token)
     - hydrogen-config.json (if --with-web-client flag)
   → Admin runs: docker compose -f ~/.config/hkask/sidecar/docker-compose.yml up -d
   → Caddy auto-obtains Let's Encrypt certificate for matrix.example.com
   → Within ~30 seconds, https://matrix.example.com is live with valid TLS
   → No manual TLS configuration required.

2. Admin verifies the sidecar is healthy:
   kask matrix status-sidecar
   → Expected output: all containers UP, API responding, DB integrity OK, sync CONNECTED

3. Admin enables user registration (already enabled by default in generated conduit.toml):
   → Default: allow_registration = true — humans can create accounts via FluffyChat
   → To disable after setup: edit conduit.toml, set allow_registration = false, restart
   → Or keep registration closed from the start and use: kask matrix register --user Bob-Jones
     (hKask creates the account via Conduit's admin API using the stored admin token)

4. Admin registers the hKask agent (requires full userpod credential):
   kask matrix register --agent Alice-Smith
   → Prompts: "Enter credential for userpod 'Alice-Smith':"
   → Admin enters: Alice-Smith/Passphrase (or Alice-Smith-Passphrase)
   → hKask verifies credential locally — direct string comparison, no AI
   → hKask registers @alice-smith:example.com on Conduit
   → hKask sets Matrix display name to "Alice-Smith (hKask userpod)"
   → hKask prints a labeled QR code:
     ┌──────────────────────────────────────────────┐
     │  UserPod: Alice-Smith                       │
     │  Matrix: @alice-smith:example.com             │
     │  Scan this QR code in FluffyChat to verify    │
     │  your agent. No AI involved in this step.     │
     │  ┌──────────────────────────────────────┐    │
     │  │         ██████████████████            │    │
     │  │         ██  SAS data  ██            │    │
     │  │         ██████████████████            │    │
     │  └──────────────────────────────────────┘    │
     └──────────────────────────────────────────────┘
   → Admin shares this QR code with the human user

5. Admin starts the Matrix sync listener:
   kask matrix listen --agent Alice-Smith
   → hkask-mcp-communication begins syncing, ready to receive messages

6. (If using closed registration) Admin creates human's Matrix account:
   kask matrix register --user Bob-Jones
   → hKask creates @bob-jones:example.com on Conduit via admin API
   → Outputs: "Account created: @bob-jones:example.com / password: <generated>"
   → Admin shares these credentials with the human user securely
```

#### Phase 2 — Human Onboarding (One-Time Per Device)

```
1. Human installs FluffyChat from App Store / Play Store

2. Human creates account on the Conduit homeserver:
   → In FluffyChat: "Use custom homeserver" → enter https://matrix.example.com
   → Tap "Create Account" → choose username (e.g., @bob-jones:example.com) and password
   → (If admin used kask matrix register --user Bob-Jones instead, human enters those credentials)

3. Human finds their userpod:
   → Search for @alice-smith:example.com
   → Sees display name: "Alice-Smith (hKask userpod)"
   → Human recognizes "Alice-Smith" as their userpod's exact full name from onboarding
   → No AI suggestion, no fuzzy matching — the human knows their own userpod's name
   → Starts DM

4. FluffyChat prompts: "Verify device @alice-smith:example.com?"
   → Human scans the labeled QR code from Phase 1 step 4
   → Or compares the emoji string if QR scanning isn't available
   → SAS completes — FluffyChat shows: "Device verified ✓"
   → E2EE session established — messages are now end-to-end encrypted
   → Human now has cryptographic proof: this device controls @alice-smith:example.com

5. Human sends: "Hello agent"
   → Agent receives via Matrix sync → Curator activates → agent responds
   → Human sees agent's response in FluffyChat
```

#### The Daily Experience (After Setup)

```
Human opens FluffyChat → sees agent's room in chat list → sends message → gets response.
That's it. No SSH. No CLI. No server access. Just a chat app.
```

The QR code / emoji SAS verification in Phase 2 step 4 is the **critical security step**. Without it, the human cannot be certain they're talking to their actual agent and not an impersonator. hKask MUST make this step prominent and unskippable. The QR code must be shared through a trusted channel — if the admin and human are the same person, the admin scans it directly from their terminal. If they're different people, the admin sends it via an already-verified encrypted channel (Signal, existing Matrix DM, etc.).

### 2.5 Key Backup and Recovery

| Scenario | hKask Agent Recovery | Human Recovery |
|----------|---------------------|----------------|
| **Device lost** | Agent keys in `hkask-keystore` on cloud server — server is the device. If server is lost, restore from backup. | Human enters recovery key in new FluffyChat install → all message keys restored |
| **Server migration** | Export agent keys from `hkask-keystore` → import on new server. Or: register new agent device, verify via cross-signing. | Human verifies new agent device via SAS on new server |
| **Key compromise** | Rotate device key → re-verify with all human users. Megolm sessions automatically re-key. | Human resets cross-signing keys → re-verifies all devices |

---

## 3. Agent Interaction Patterns

### 3.1 How Agents "Listen" (English Explanation)

Agents do not poll. Agents do not have inboxes. Agents do not maintain persistent connections.

An agent is a **program invoked by the Curator when there is work to do.** The agent's experience of a Matrix conversation is identical to its experience of a `kask tui` REPL session: it receives input, recalls previous turns from its episodic memory, thinks, responds, and stores the exchange as a new memory.

The "always listening" property comes from the **Matrix sync loop** running inside the `hkask-mcp-communication` MCP server. This sync loop maintains a long-lived HTTP connection to the Conduit homeserver. When a Matrix event arrives for an agent, the MCP server:

1. Receives the event from the sync stream
2. Translates it into a `TurnRequest` (the same struct `ChatService::execute_turn()` already accepts)
3. Routes it to the Curator via the daemon socket
4. The Curator activates the agent on its next curation cycle
5. The agent processes the turn exactly as it would in `kask tui`
6. The response is routed back through the MCP server to the Matrix room

The agent never waits. The agent never checks a queue. The agent is activated when a message arrives, processes it, and yields. This is the same activation pattern as every other agent invocation in hKask.

### 3.2 Human-to-Agent (H2A) Flow

A human using FluffyChat sends a message to their agent. The agent responds.

```mermaid
sequenceDiagram
    participant H as Human<br/>(FluffyChat)
    participant C as Conduit<br/>(homeserver)
    participant M as MCP Server<br/>(hkask-mcp-communication)
    participant D as Daemon<br/>(daemon.sock)
    participant Cu as Curator<br/>(Curation Loop)
    participant A as Agent<br/>(Bot/UserPod)
    participant E as Episodic<br/>Memory

    H->>C: Send message to @agent:example.com<br/>"What's my schedule today?"
    C->>M: /sync event: message received
    
    Note over M: MatrixTransport translates<br/>Matrix event → TurnRequest
    
    M->>D: route_turn(agent_id, TurnRequest { input: "What's my schedule today?" })
    M->>M: emit reg.communication.message.received ν-event
    
    D->>Cu: Agent inbox has pending turn
    
    Note over Cu: Next curation cycle
    
    Cu->>A: Activate agent with TurnRequest
    
    A->>E: recall_recent_turns(agent_webid, context_turns=3)
    E-->>A: Previous conversation history
    
    A->>A: ChatService::execute_turn()<br/>manifest cascade → history → inference → persona filter
    
    A-->>Cu: Response: "You have 3 meetings today..."
    Cu-->>D: Route response
    
    A->>E: store_episodic(input, response)
    
    D-->>M: Response ready for delivery
    M->>C: Send Matrix message to room
    C->>H: /sync event: agent response
    H->>H: Display: "You have 3 meetings today..."
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-010
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

### 3.3 Agent-to-Agent (A2A) Flow — Same Install

Two agents on the same hKask install communicate. This uses the daemon socket directly — Matrix is not needed for local agent-to-agent communication.

```mermaid
sequenceDiagram
    participant Cu as Curator
    participant A1 as Agent Alpha<br/>(Bot)
    participant D as Daemon<br/>(daemon.sock)
    participant A2 as Agent Beta<br/>(UserPod)

    Note over Cu: Curator activates Alpha<br/>for a scheduled task

    Cu->>A1: Activate: "Check system health and report to Beta"
    
    A1->>A1: execute_turn() → inference → tool calls
    
    A1->>D: send_message(target: Beta, payload: "System health: all green,<br/>variety deficit 12/100, gas 67%")
    
    Note over D: Daemon routes locally —<br/>no Matrix needed for same-install A2A
    
    D->>Cu: Beta has pending message from Alpha
    
    Note over Cu: Next curation cycle
    
    Cu->>A2: Activate with message from Alpha
    
    A2->>A2: execute_turn() → reads Alpha's report → responds
    
    A2-->>Cu: "Acknowledged. Logging health report."
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-011
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->


The "inbox" and the "REPL" are the same conversation viewed from different architectural layers:

```mermaid
graph TD
    subgraph Transport["Transport Layer — 'Inbox'"]
        SYNC[Matrix /sync stream]
        QUEUE[Event queue<br/>internal to MCP server]
        TRANS[MatrixTransport<br/>translates events → TurnRequests]
    end
    
    subgraph Agent["Agent Layer — 'REPL'"]
        TURN[TurnRequest { input, agent_name, model, ... }]
        EXEC[ChatService::execute_turn]
        RECALL[recall_recent_turns<br/>from episodic memory]
        INFER[Inference]
        STORE[store_episodic]
    end
    
    SYNC --> QUEUE --> TRANS --> TURN
    TURN --> EXEC
    EXEC --> RECALL --> INFER --> STORE
    
    subgraph Equivalence["Same Conversation, Two Views"]
        E1[Transport sees: async message queue]
        E2[Agent sees: turn-based conversation<br/>with memory continuity]
    end
    
    Transport -.-> E1
    Agent -.-> E2
    E1 --- E2
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-013
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

**The inbox is what a REPL looks like from the transport layer. The REPL is what an inbox looks like from the agent layer.** The agent never knows about queues, sync tokens, or Matrix event types. It only knows about turns, history recall, and episodic memory — exactly what `ChatService::execute_turn()` already provides.

### 3.6 Full System Connection Map

```mermaid
graph TD
    subgraph Cloud["Cloud Server — hKask Install"]
        subgraph Sidecar["Docker Sidecar"]
            CADDY[Caddy<br/>Docker: caddy:2-alpine<br/>TLS + reverse proxy]
            CONDUIT[Conduit Homeserver<br/>Docker: registry.gitlab.com/famedly/conduit]
            ELEMENT[Hydrogen<br/>Docker: element-hq/hydrogen-web<br/>Optional — lightweight browser client]
        end
        
        subgraph Core["hKask Core"]
            DAEMON[Daemon<br/>daemon.sock]
            MCP_COMM[hkask-mcp-communication<br/>MatrixTransport + matrix-rust-sdk]
            CURATOR[Curator<br/>Curation Loop]
            AGENTS[Agent Pods<br/>Bots + UserPods]
            KEYSTORE[hkask-keystore<br/>AES-256-GCM + OS keychain]
            EPISODIC[Episodic Memory<br/>SQLCipher]
            Regulation[Cybernetics Loop<br/>reg.communication.matrix.* spans]
        end
    end
    
    subgraph Humans["Human Users"]
        FLUFFY[FluffyChat<br/>Android / iOS<br/>Primary mobile client]
        ELEM_X[Element X<br/>Android / iOS<br/>Alternative mobile]
        ELEM_D[Element Desktop<br/>Windows / macOS / Linux]
    end
    
    FLUFFY -->|HTTPS + E2EE| CADDY
    ELEM_X -->|HTTPS + E2EE| CADDY
    ELEM_D -->|HTTPS + E2EE| CADDY
    CADDY -->|reverse proxy| CONDUIT
    CADDY -->|reverse proxy| ELEMENT
    
    CONDUIT -->|localhost:8008| MCP_COMM
    MCP_COMM -->|daemon.sock| DAEMON
    DAEMON --> CURATOR
    CURATOR --> AGENTS
    AGENTS --> EPISODIC
    MCP_COMM --> KEYSTORE
    MCP_COMM --> Regulation
    
    CONDUIT -.->|Federation<br/>optional| OTHER[Other Matrix<br/>Homeservers]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-014
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

---

## 4. Essentialist Review — What Must Exist?

### G1 — Exist (Deletion Test)

**Question:** If we delete Matrix integration entirely from hKask, does any behavior vanish?

**Answer (Direction 1 — Caller's perspective):** Agents currently communicate via the daemon socket (local, synchronous) and ACP (machine-to-machine). If we delete Matrix, agents within a single hKask install still communicate. Cross-installation agent communication and human-to-agent communication would have no channel. In the cloud deployment model, humans have no way to talk to their agents without SSH — which violates the deployment assumption.

**Answer (Direction 2 — Artifact's perspective):** Delete the `matrix.rs` module. The complexity of "how do humans on mobile phones talk to their cloud-hosted agents?" reappears immediately. The daemon socket is local-only. The API is programmatic. Neither serves a human with a phone.

**Verdict:** The *current* stubs fail G1 — they encode zero behavior and must be pruned or replaced. The *concept* of Matrix integration **survives G1** because it enables behavior that no existing hKask channel provides: human-to-agent communication from mobile devices to a cloud server.

**Constraint force:** Prohibition (REQUIRED) — stubs violate P5. The existing `matrix.rs` must be resolved.

### G2 — Surface (Interface Count)

The current `MatrixClient` exposes 9 public methods plus `EmbeddedHomeserver` with 3 more. That's 12 public items for a transport layer.

**Challenge:** What if this had exactly one public function?

The answer is: `start_sync(agent_service, agent_name)` — begin translating Matrix events into `TurnRequest`s for the given agent. Everything else (room creation, user registration, health checks) is setup, not runtime behavior.

**Recommended surface (≤4 public items):**

| Function | Purpose |
|----------|---------|
| `start_sync(agent_service, agent_name)` | Begin translating Matrix events → TurnRequests |
| `send_response(room_id, text)` | Send agent response back to Matrix room |
| `bootstrap(config)` | One-time setup: register user, create rooms, verify devices |
| `health()` | Check sync connection status (for Regulation observability) |

**Verdict:** 12 public items → collapse to 4. Setup concerns are separate from runtime concerns.

**Constraint force:** Guardrail (REQUIRED overridable) — surface exceeds 7 without justification.

### G3 — Contract (Abstraction Trace)

**Trace:** `MatrixClient` → wraps a homeserver URL string. `EmbeddedHomeserver` → wraps `MatrixClient`. Every method is a pass-through to an HTTP call that doesn't exist yet.

**Question:** What behavior is lost if we replace `MatrixClient` with a direct `matrix_sdk::Client` from the `matrix-rust-sdk` crate?

**Answer:** Nothing. The `MatrixClient` struct is a pass-through abstraction with zero added behavior. The `EmbeddedHomeserver` wraps `MatrixClient` and adds nothing. The SDK already provides the client abstraction.

**Verdict:** Delete both wrapper structs. Use `matrix_sdk::Client` directly. The SDK is the adapter; hKask wrapping it adds zero information hiding.

**Constraint force:** Prohibition (REQUIRED) — pass-through abstraction encoding zero behavior.

### Essentialism Score

| Gate | Finding | Force | Action |
|------|---------|-------|--------|
| G1 | 303 lines of stubs encode zero behavior | Prohibition | Delete or implement |
| G2 | 12 public items for transport layer | Guardrail | Collapse to ≤4 |
| G3 | `MatrixClient` + `EmbeddedHomeserver` are pass-through wrappers | Prohibition | Delete, use SDK directly |

**Items removed:** 2 structs + 9 stub methods → replaced by direct SDK usage
**Essentialism score:** 100% of current code is non-essential

---

## 5. Grill-Me — Socratic Interrogation of the Design

### Round 1: Recall & Definition

**Q1:** What does "always listening" mean for an agent in a cybernetic system?

The agent is not a daemon. It doesn't have a `while true { recv() }` loop. In hKask's architecture, agents are activated by the Curation Loop. The Curator decides when to invoke an agent. "Always listening" must mean: when a message arrives for an agent, the system routes it to the agent's inbox, and the Curator processes it in the next curation cycle. It does NOT mean the agent process is blocking on a socket.

**Q2:** Where does the Matrix sync connection live in hKask's four-loop architecture?

Per the loop architecture (§3.4), `hkask-mcp-communication` is assigned to the **Communication loop** — which is transport infrastructure, not a loop. "Communication does not own resources, does not regulate, and does not transform. It is a dumb pipe." The Matrix sync connection lives in the MCP server process, not in any agent pod. It's a sensor, not an actor.

✅ Solid on both.

### Round 2: Mechanism & Causation

**Q3:** Walk me through the flow from "human sends Matrix message to agent" to "agent responds."

1. Human sends message in Matrix room → Conduit receives it
2. `hkask-mcp-communication`'s sync loop (matrix-rust-sdk `SyncService`) receives the event
3. Server emits `reg.communication.message.received` ν-event with sender, room, content
4. Server translates Matrix event → `TurnRequest` → routes to agent via daemon socket
5. Curator (in Curation Loop) reads pending turn on next curation cycle
6. Curator invokes agent with `TurnRequest`
7. Agent calls `ChatService::execute_turn()` → manifest cascade → history recall → inference → persona filter
8. Agent response routed back through MCP server → Matrix send

**Q4:** What regulates the sync loop? What prevents it from consuming unbounded energy?

The sync loop is a long-poll HTTP connection maintained by the SDK. It's not a busy-wait. Energy consumption is: TLS keepalive + periodic `/sync` requests (every ~30s when idle, immediate when events arrive). The Cybernetics Loop meters this through `reg.communication.*` spans. If sync traffic exceeds a threshold, the algedonic pathway fires. The Curator can throttle by reducing sync timeout or pausing non-critical rooms.

⚠️ Partial — correctly identifies the mechanism but doesn't address what happens when the sync connection itself fails (network partition). The SDK handles reconnection internally, but hKask needs a `reg.communication.matrix.sync.stalled` span for observability.

### Round 3: Rationale & Tradeoffs

**Q5:** Why would hKask use Matrix rather than just the daemon socket for agent-to-agent communication?

The daemon socket is local-only. It cannot cross machine boundaries. Matrix adds:
- Cross-installation agent communication (two hKask installs talking)
- Human-to-agent communication (humans using FluffyChat to talk to their agents)
- Federation with the broader Matrix ecosystem
- Asynchronous messaging (agent sends, recipient picks up later)

The daemon socket is synchronous and local. Matrix is asynchronous and federated. They serve different topologies.

**Q6:** The current stub design embeds Conduit as a library dependency. The user wants an external server with a sidecar script. Which better satisfies P5 (Essentialism)?

The external server approach. Embedding a homeserver adds:
- Conduit's entire dependency tree to hKask's build
- Homeserver lifecycle management to the daemon
- Database management for the homeserver

All of this is complexity hKask doesn't need to own. The sidecar approach keeps Conduit as a separate Docker container, managed by the user, with hKask providing config generation and health-check tooling. This is the brachistochrone — it looks like more pieces (two containers instead of one process) but it's actually the path of least total system action because it avoids entangling homeserver concerns into hKask's domain.

✅ Solid on both.

### Round 4: Edge Cases & Failure Modes

**Q7:** What happens when the Matrix homeserver is unreachable but agents need to communicate?

Agents fall back to the daemon socket for local communication. Cross-installation messages queue in the MCP server's outbox (persisted to SQLCipher) and are delivered when the homeserver returns. The Curator receives a `reg.communication.matrix.unavailable` alert and can inform the user. This is graceful degradation, not catastrophic failure.

**Q8:** P12 (Authenticated Host Mandate) is a Prohibition: "every action has an author." How does a Matrix message from an external human map to a host userpod?

The human is authenticated as their own userpod on their own hKask install. When they send a Matrix message, their `hkask-mcp-communication` server attaches their WebID in the message's structured payload. The receiving hKask install verifies the sender's WebID against the Matrix user ID. If the human doesn't have a userpod (they're using vanilla FluffyChat without a hKask install), the message is attributed to an "external" pseudo-userpod with limited OCAP scope. The Curator flags unverified senders.

⚠️ Partial — correctly identifies the mapping problem but doesn't address the bootstrapping trust issue: how does the receiving install know that `@bob:example.com` is the same entity as `webid:bob.hkask.local`? This requires out-of-band verification (SAS or QR) on first contact.

### Round 5: Synthesis

**Q9:** Given that Communication is "demoted from a loop to transport infrastructure — a dumb pipe," design the minimal interface between the Matrix transport and the Curation Loop.

```rust
/// MatrixTransport converts Matrix events into TurnRequests
/// and routes agent responses back to Matrix rooms.
/// 
/// It does NOT queue messages for agents. It translates protocols.
/// The "inbox" is an internal implementation detail of the sync loop,
/// not part of the public interface.
impl MatrixTransport {
    /// Start translating Matrix events → TurnRequests for the given agent.
    /// When a message arrives, constructs a TurnRequest and invokes
    /// ChatService::execute_turn() through the existing daemon socket.
    pub async fn start_sync(
        &self, 
        agent_service: Arc<AgentService>,
        agent_name: String,
    ) -> Result<(), MatrixError>;
    
    /// Send a response back to a Matrix room.
    pub async fn send_response(
        &self,
        room_id: &RoomId,
        text: &str,
    ) -> Result<(), MatrixError>;
}
```

The Curation Loop doesn't need to know about rooms, Matrix user IDs, or sync tokens. It needs turns in and responses out. The MCP server handles all Matrix-specific translation internally.

✅ Solid.

### Grill-Me Assessment

| Area | Rating | Notes |
|------|--------|-------|
| Cybernetic architecture | 🟢 Solid | Correctly places Matrix as transport, not a loop |
| Agent listening model | 🟢 Solid | Event-driven via Curator, not polling |
| P12 identity mapping | 🟢 Solid | Name match + SAS + passphrase gate. See §2.3. |
| Failure modes | 🟢 Solid | Graceful degradation to daemon socket |
| Interface minimalism | 🟢 Solid | Two-function transport interface |

---

## 6. Pragmatic Semantics — Epistemic Classification

### What We Know (Declarative — Directly Stated)

| Claim | Provenance | Confidence |
|-------|-----------|------------|
| `matrix.rs` exists as 303 lines of stubs | `read_file` on `mcp-servers/hkask-mcp-communication/src/matrix.rs` | High |
| Current design embeds Conduit as library dependency | Comment on L3–4 of matrix.rs | High |
| `hkask-mcp-communication` is assigned to Communication loop (transport) | `loop-architecture.md` §3.4, L270 | High |
| Communication is "demoted from a loop to transport infrastructure" | `loop-architecture.md` §2.1, L125 | High |
| P12 is a Prohibition — every action has an author | `PRINCIPLES.md` §2.5 traceability matrix, L329 | High |
| P5 declares stubs "a debt against the Generative Space" | `PRINCIPLES.md` §2.2, L234 | High |
| `ChatService::execute_turn()` is the agent turn pipeline | `crates/hkask-services-chat/src/chat/` L856–951 | High |
| REPL loop uses `rl.readline()` → `run_turn_loop()` via `TurnSink` | `crates/hkask-repl/src/turn.rs` L208–475, `crates/hkask-repl/src/lib.rs` L239 | High |
| Agent continuity comes from `recall_recent_turns()` (episodic memory) | `crates/hkask-services-chat/src/chat/` L625–656 | High |

### What We Infer (Probabilistic — Pattern-Based)

| Claim | Basis | Confidence |
|-------|-------|------------|
| `matrix-rust-sdk` is the correct client library | Used by Element X, FluffyChat (Dart bindings to same crypto), Rust-native, actively maintained | Medium-High |
| External server + Docker sidecar better satisfies P5 than embedded Conduit | Essentialist G1–G3 analysis above | Medium |
| Two-function transport interface is sufficient | Grill-Me Q9 synthesis | Medium |
| FluffyChat is the right primary mobile client | Most popular Matrix mobile client, beautiful UX, full E2EE support, actively maintained | Medium |

### What We Project (Subjunctive — What-If)

| Claim | Basis | Confidence |
|-------|-------|------------|
| Matrix integration would add `reg.communication.matrix.*` spans | Pattern match against existing Regulation span registry | Low-Medium |
| Cross-installation agent communication is the primary A2A use case | Inference from cloud deployment model | Low |
| Humans will accept SAS verification as part of agent onboarding | UX assumption — needs validation | Low |

---

## 7. Pragmatic Cybernetics — Feedback Loop Analysis

### The Matrix Listening Loop as a Cybernetic System

Mapping the "always listening" requirement to the five cybernetic components:

| Component | Implementation | What It Does |
|-----------|---------------|-------------|
| **Sensor** | `matrix-rust-sdk` `SyncService` in `hkask-mcp-communication` | Receives Matrix events (messages, invites, room changes) |
| **Model** | ν-event store + `reg.communication.matrix.*` spans | Records: messages received, sync health, delivery latency |
| **Regulator** | Cybernetics Loop variety counter + algedonic thresholds | Compares message volume, sync health against baselines |
| **Actuator** | MCP tool dispatch → daemon → Curator → agent → `ChatService::execute_turn()` | Routes messages to agents, sends responses |
| **Observer-of-observer** | `reg.communication.matrix.sync.stalled` span | "Is the Matrix sensor itself healthy?" |

### Feedback Loop Properties

| Property | Analysis |
|----------|----------|
| **Polarity** | Negative (stabilizing). If message volume spikes, backpressure throttles processing. If sync stalls, alert fires. |
| **Delay** | Matrix `/sync` latency (~30s idle, sub-second active) + Curator cycle time. Total: seconds to minutes. Acceptable for async messaging. |
| **Gain** | Algedonic threshold sensitivity. Too high = missed sync failures. Too low = alert fatigue on transient network blips. Needs tuning. |
| **Closure** | Critical: `reg.communication.matrix.sync.stalled` → algedonic alert → Curator reads → Curator intervenes. If the Curator doesn't consume the alert, the loop is broken. |
| **Fidelity** | The sync health span only measures Matrix transport. It does NOT measure: message semantic coherence, agent response quality, or human satisfaction. Those are separate Regulation spans. |

### Variety Analysis (Ashby's Law)

**System variety (what can go wrong):**
1. Conduit container crashed
2. Sync connection stalled
3. E2EE key mismatch
4. Message spam/volume spike
5. Unverified sender
6. Room state corruption
7. Federation breakage
8. SDK internal error
9. Token expiry
10. Conduit database corruption

**Regulator variety (what Regulation can detect):**
- `reg.communication.matrix.sync.health` — sync connection status
- `reg.communication.matrix.message.received` — message volume
- `reg.communication.matrix.sync.stalled` — sync failure
- `reg.communication.matrix.e2ee.error` — encryption failures
- `reg.communication.matrix.sender.unverified` — unknown senders
- `reg.communication.matrix.sidecar.health` — Conduit container health (via Docker healthcheck)

**Gap:** 10 failure modes, ~6 Regulation spans. Variety deficit of ~4. Items 6–8 (room state, federation, SDK errors) are partially observable indirectly (SDK errors surface as sync stalls; federation breakage surfaces as message delivery failures). Room state corruption and Conduit database corruption are the main blind spots. The sidecar health span covers container liveness but not database integrity.

**Recommendation:** Add `reg.communication.matrix.sidecar.db_integrity` as a periodic check (Conduit exposes a health endpoint). Document that full sidecar monitoring is the user's responsibility — hKask monitors the transport channel, not the homeserver internals.

### The Good Regulator Check

**Q:** Is the Regulation variety counter a good model of Matrix communication health?

**A:** Partially. It models transport-level health (sync status, message flow, encryption errors, sidecar liveness). It does NOT model semantic-level health (are agents understanding messages? are humans satisfied?). This is correct — the Cybernetics Loop regulates transport, the Curation Loop regulates semantics. The model matches its regulatory scope.

---

## 8. Architectural Recommendation

### 8.1 Delete the Current Stubs

The existing `matrix.rs` in `hkask-mcp-communication` is 303 lines of zero-behavior code. Per P5, it must be resolved. The resolution is: **delete the stubs and replace with a real implementation using `matrix-rust-sdk`.**

### 8.2 Do NOT Embed a Homeserver

The current stub design declares intent to embed Conduit as a library dependency. This violates:

- **P5 (Essentialism):** Adding Conduit's dependency tree, database management, and federation config to hKask's build is complexity the system doesn't need to own.
- **Hexagonal boundaries (§3.2):** "All external I/O via MCP." A Matrix homeserver is external I/O. It belongs behind an adapter, not embedded in the domain.
- **Loop architecture (§2.1):** Communication is transport, not a loop. Embedding a homeserver gives transport loop-level complexity (state management, persistence, federation).

### 8.3 Architecture: Docker Sidecar + SDK Integration

```
┌──────────────────────────────────────────────────────────┐
│                  Cloud Server                            │
│                                                          │
│  ┌──────────────────────┐  ┌──────────────────────────┐ │
│  │  Docker: Caddy        │  │  hKask Core              │ │
│  │  TLS + proxy          │  │                          │ │
│  │  ports 80, 443        │  │  Daemon (daemon.sock)     │ │
│  └──────────┬───────────┘  │  Curator (Curation Loop) │ │
│             │              │  Agent Pods              │ │
│  ┌──────────┴───────────┐  │                          │ │
│  │  Docker: Conduit      │  │  hkask-mcp-communication  │ │
│  │  localhost:8008       │  │  └─ MatrixTransport        │ │
│  │                      │  │     └─ matrix-rust-sdk     │ │
│  │  Docker: Hydrogen     │  │        └─ SyncService      │ │
│  │  localhost:80        │  │        └─ CryptoStore      │ │
│  │  (optional)           │  │           └─ hkask-keystore│ │
│  └──────────────────────┘  │                          │ │
│             │              │                          │ │
│             └──────────────┤                          │ │
│                localhost   │                          │ │
└────────────────────────────┴──────────────────────────┘
```

### 8.4 The "Always Listening" Mechanism

Agents do NOT poll. The `matrix-rust-sdk` `SyncService` maintains a long-lived `/sync` connection inside the MCP server process. When a Matrix event arrives for an agent:

1. **SyncService** receives event → fires callback
2. **MatrixTransport** translates Matrix event → `TurnRequest` (same struct `ChatService` already uses)
3. **MatrixTransport** emits `reg.communication.message.received` ν-event
4. **MatrixTransport** routes `TurnRequest` to agent via daemon socket
5. **Curator** reads pending turn on next curation cycle
6. **Curator** invokes agent with `TurnRequest`
7. **Agent** calls `ChatService::execute_turn()` — identical to `kask tui` turn processing
8. **Agent** responds → `MatrixTransport.send_response()` → Matrix room

This is event-driven, not polling. The sync loop is the SDK's responsibility, not hKask's. Energy consumption is bounded by the sync interval (configurable, default ~30s idle).

### 8.5 What hKask Builds (Minimal)

| Artifact | Lines (est.) | Purpose |
|----------|-------------|---------|
| `MatrixTransport` struct | ~150 | Wraps `matrix_sdk::Client`, exposes `start_sync` + `send_response` |
| `CryptoStore` impl | ~100 | Redirects SDK key storage to `hkask-keystore` |
| `Bootstrap` flow | ~100 | Registration, login, device verification, room setup |
| Regulation spans | ~40 | `reg.communication.matrix.*` span definitions |
| CLI: `kask matrix deploy-sidecar` | ~120 | Generate docker-compose.yml (with healthchecks) + config files |
| CLI: `kask matrix register --agent` | ~50 | Agent registration on Conduit, outputs QR code |
| CLI: `kask matrix register --user` | ~40 | Create human Matrix account on Conduit (if open registration disabled) |
| CLI: `kask matrix listen` | ~30 | Start Matrix sync listener for an agent |
| CLI: `kask matrix status-sidecar` | ~50 | Health check: docker ps + HTTP poll + SQLite integrity |
| CLI: `kask matrix verify-device` | ~60 | SAS/QR device verification for human onboarding |
| Daemon: periodic sidecar health task | ~40 | Every 60s: poll containers, emit reg.communication.matrix.sidecar.health |
| **Total** | **~750** | Replaces 303 lines of stubs with behavior-encoding code |

### 8.6 What hKask Does NOT Build

- ❌ Homeserver (Conduit is an upstream Docker image, user-managed)
- ❌ Matrix client UI (headless constraint — CLI/MCP/API only; humans use FluffyChat)
- ❌ Room management UI (rooms are created programmatically by agents)
- ❌ Federation configuration (user's responsibility on their Conduit instance)
- ❌ Bridge management (out of scope)
- ❌ Human E2EE key management (humans manage their own keys in FluffyChat)

### 8.7 Dependency

```toml
# In mcp-servers/hkask-mcp-communication/Cargo.toml
matrix-sdk = { version = "0.9", features = ["e2e-encryption", "sqlite-cryptostore"] }
# sqlite-cryptostore is the DEFAULT; hKask replaces it with keystore-backed impl
```

`matrix-rust-sdk` is the official Rust Matrix SDK, used by Element X, Fractal, and other Matrix clients. It's Apache-2.0 licensed, actively maintained, and the most audited Matrix client library available. FluffyChat uses Dart bindings to the same Olm/Megolm crypto primitives, ensuring E2EE compatibility.

---

## 9. Principle Alignment

| Principle | Alignment | Evidence |
|-----------|-----------|----------|
| **P1 — User Sovereignty** | ✅ | E2EE keys in `hkask-keystore`; user chooses homeserver; data never leaves user's encryption boundary; human keys stay in FluffyChat |
| **P2 — Affirmative Consent** | ✅ | Matrix rooms are invite-only; agent joins only when user registers it; default-deny on incoming messages from unverified senders; SAS verification is explicit consent; credential check is direct human action, no AI mediation |
| **P3 — Generative Space** | ✅ | Matrix enables cross-installation agent communication and human-to-agent interaction from mobile devices — expands the space of possible agent behaviors |
| **P4 — Clear Boundaries (OCAP)** | ✅ | Matrix transport is OCAP-gated; agents need `communication:send` and `communication:receive` capabilities; Conduit is outside the OCAP boundary |
| **P5 — Essentialism** | ✅ | ~750 lines replacing 303 lines of stubs; no embedded homeserver; two-function transport interface; Docker sidecar not library dependency |
| **P6 — Space for UserPods & Bots** | ✅ | Matrix enables userpods to communicate with humans on mobile (H2A) and bots to communicate across installs (A2A) |
| **P7 — Evolutionary Architecture** | ✅ | Docker sidecar + SDK integration allows Matrix integration to evolve independently of hKask core; Conduit upgrades are `docker pull`, not `cargo update` |
| **P8 — Semantic Grounding** | ✅ | Every Matrix message produces a ν-event with provenance (sender WebID, timestamp, room context); ν-events are canonical |
| **P9 — Homeostatic Self-Regulation** | ✅ | `reg.communication.matrix.*` spans feed into Cybernetics Loop; sync health monitored; backpressure on message volume; sidecar health checked |
| **P10 — Bot/UserPod Taxonomy** | ✅ | Bots use Matrix for A2A (machine-speed); UserPods use Matrix for H2A (human-speed); distinct interaction patterns |
| **P11 — Digital Public/Private Sphere** | ✅ | Matrix rooms map to visibility: private rooms = private sphere, public rooms = public sphere; OCAP-enforced |
| **P12 — Authenticated Host Mandate** | ✅ | Every Matrix message carries sender WebID in structured payload; unverified senders flagged; no anonymous messages; SAS verification establishes identity binding |
| **Headless Constraint** | ✅ | No Matrix client UI in hKask; all interaction through CLI, MCP, or API; humans use FluffyChat (external client) |

---

## 10. Open Questions (Subjunctive — What-If)

1. **Bootstrapping trust (RESOLVED):** Identity binding uses two verifiable layers: name match (Matrix user ID = userpod name, human recognizes it from onboarding) + SAS verification (cryptographic proof of device key ownership). The passphrase gates agent registration. See §2.3 for full threat model.

2. **Multi-agent rooms:** If multiple hKask agents are in the same Matrix room, who responds to which messages? Does the Curator route based on `@mention` tags? Does each agent have its own inbox filtered by room? **Recommendation:** Agents only respond to `@mention` tags directed at them. Unaddressed messages are logged to episodic memory but do not trigger activation.

3. **Conduit sidecar lifecycle:** If hKask provides `kask matrix deploy-sidecar`, does it also provide `kask matrix status-sidecar` for health checks? Where's the boundary between "helpful tooling" and "managing server code"? **Recommendation:** Provide `status-sidecar` (read-only health check) and `upgrade-sidecar` (`docker pull` + restart). Do NOT provide configuration editing — user edits `conduit.toml` directly.

4. **Federation opt-in:** The current stub design defaults to "local-only" federation. The cloud deployment model implies federation is the user's choice. Should hKask have any opinion about federation at all? **Recommendation:** Federation is off by default in generated `conduit.toml`. User enables it explicitly. hKask warns that federation exposes room metadata to other homeservers.

5. **Gas accounting:** Should Matrix message send/receive consume gas from the agent's gas budget? If an agent receives 1,000 spam messages, does that drain its budget? **Recommendation:** Receiving a message costs a small fixed gas amount (1 hJoule). Sending costs the inference gas for the response. Spam protection: if message rate exceeds threshold, Curator throttles activation. The backpressure mechanism needs design.

6. **FluffyChat vs. Element X:** Which mobile client is primary? FluffyChat has better UX and is more popular. Element X is Rust-native and uses the exact same SDK as hKask agents. **Recommendation:** Recommend FluffyChat as primary (better UX for non-technical users). Document Element X as alternative (better for technical users who want SDK parity). Both are compatible.

---

## 11. Summary Recommendation

**Delete the 303 lines of stubs in `matrix.rs`. Replace with ~660 lines of behavior-encoding code that:**

1. Uses `matrix-rust-sdk` directly (no wrapper structs — G3 violation)
2. Exposes exactly 4 public functions: `start_sync`, `send_response`, `bootstrap`, `health` (G2 compliance)
3. Stores E2EE keys in `hkask-keystore` via a custom `CryptoStore` impl
4. Maintains an event-driven sync connection in the MCP server process (agents don't poll)
5. Routes incoming Matrix messages as `TurnRequest`s to `ChatService::execute_turn()` — the same pipeline as `kask tui`
6. Emits `reg.communication.matrix.*` spans for observability
7. Provides `kask matrix deploy-sidecar` — generates docker-compose.yml for Conduit + optional Hydrogen
8. Provides `kask matrix register --agent` — registers agent on Conduit using full credential (FirstName-LastName/Passphrase), outputs labeled QR code for human SAS verification. No AI involvement in credential check.
9. Provides `kask matrix register --user` — creates human Matrix account (if open registration disabled)
10. Provides `kask matrix listen` — starts Matrix sync listener for an agent
11. Provides `kask matrix status-sidecar` — health check for Docker containers
12. Does NOT embed a homeserver, manage server code, build a client UI, or manage human E2EE keys

**Setup model:** Server admin runs one-time setup (deploy sidecar, configure TLS, register agent, start listener). Human installs FluffyChat, creates account, scans QR code, and chats. After setup, the daily experience is: open FluffyChat → see agent's room → send message → get response. No SSH, no CLI, no server access for the human.

**The "always listening" is not a polling loop — it's an event-driven sync connection maintained by the SDK inside the MCP server, with messages routed to agents through the existing Curation Loop on its normal cycle. The inbox is what a REPL looks like from the transport layer. The REPL is what an inbox looks like from the agent layer.**

---

*Report grounded in: PRINCIPLES.md (all 12 principles + §0 Lazy Grounding), loop-architecture.md (four-loop decomposition, MCP server assignments), hexagonal boundaries (§3), `crates/hkask-services-chat/src/` (ChatService pipeline), `crates/hkask-repl/src/lib.rs` (REPL loop), and direct inspection of `mcp-servers/hkask-mcp-communication/src/matrix.rs`.*

---

## 12. Specification Gap Analysis — Multi-Skill Review

**Date:** 2026-06-14
**Skills Applied:** Essentialist (document surface review), Grill-Me (edge-case interrogation), Coding Guidelines (simplicity audit), Pragmatic Semantics (epistemic classification audit), Pragmatic Cybernetics (feedback loop closure audit)

Each finding is classified by constraint force: **Prohibition** (must fix — spec is incomplete without it), **Guardrail** (must fix or explicitly defer), **Guideline** (should fix), **Evidence** (observation, not directive), **Hypothesis** (speculative gap, needs verification).

---

### 12.1 Essentialist Review — Document Surface

#### G1 — Exist: Sections That Could Be Deleted

| Finding | Section | Force | Recommendation |
|---------|---------|-------|----------------|
| **Duplicate topology diagram** | §1.2 (ASCII art) duplicates §3.6 (mermaid). Two diagrams showing the same physical layout. | Guideline | Merge into §3.6. Delete §1.2 ASCII art. The mermaid diagram is richer and auto-themed. |
| **Docker-compose YAML in architecture doc** | §1.3 contains ~35 lines of YAML that will rot when image tags change. This is generated output, not architecture. | Guardrail | Replace with: "Generated by `kask matrix deploy-sidecar`. See `~/.config/hkask/sidecar/docker-compose.yml`." The YAML belongs in the CLI's output, not the spec. |
| **ASCII art QR code label** | §2.4 Phase 1 step 4 contains a mock QR code rendering. This is UI detail, not architecture. | Guideline | Replace with: "Outputs a labeled QR code containing the agent's SAS verification data." The exact label format belongs in a user guide. |
| **Negative client recommendations** | §2.1 includes iamb and Element Web rows saying "not recommended." If they're not recommended, why are they in the spec? | Guideline | Delete iamb and Element Web rows. Move to a footnote or separate "evaluated and rejected" appendix if audit trail is needed. |
| **Meta-review sections** | §4 (Essentialist), §5 (Grill-Me), §6 (Pragmatic Semantics) are process documentation, not architecture specification. | Evidence | These are valuable as design rationale and audit trail. Keep them but consider extracting to a separate "design review" document if the spec grows too large. |

#### G2 — Surface: Section Count

The document has 11 top-level sections and ~30 subsections. For an architecture specification, this is acceptable — it's a research report, not a module with a public API. No G2 violation.

#### G3 — Contract: Pass-Through Content

| Finding | Force | Recommendation |
|---------|-------|----------------|
| §9 (Principle Alignment) is a traceability matrix, not a restatement of PRINCIPLES.md. It maps spec decisions to principles. | Evidence | Survives G3. Valuable for compliance auditing. |
| §8.7 (Dependency) restates information available in Cargo.toml. | Guideline | Acceptable — the spec should declare its key dependency. |

---

### 12.2 Grill-Me — Edge-Case Interrogation

#### Unchallenged Assumptions

**Q1: How does `kask matrix register --agent` authenticate to Conduit's admin API?**

Conduit's admin API requires an admin token (configured in `conduit.toml` as `admin_token`). The spec never mentions this token — how it's generated, where it's stored, how `kask matrix register` retrieves it. Without this, the command cannot create users on Conduit.

**Force: Prohibition.** The spec is incomplete without this. The registration flow is the critical path.

**Recommendation:** Specify: (a) `kask matrix deploy-sidecar` generates a random admin token and writes it to `conduit.toml` and `hkask-keystore`. (b) `kask matrix register` reads the token from `hkask-keystore`. (c) Token is never logged or displayed.

**Q2: What happens when Conduit's SQLite database corrupts?**

The spec mentions `reg.communication.matrix.sidecar.db_integrity` as a proposed span but doesn't define it. Conduit stores all room state, user accounts, and message history in a single SQLite file. Corruption means total loss of Matrix service.

**Force: Guardrail.** Database corruption is a real failure mode for single-file databases. The spec must address it or explicitly declare it out of scope (user's responsibility).

**Recommendation:** Specify: (a) `kask matrix status-sidecar` includes a SQLite integrity check (`PRAGMA integrity_check`). (b) `kask matrix deploy-sidecar` documents a backup strategy (daily `sqlite3 .backup` cron job). (c) Recovery procedure: stop Conduit, restore from backup, restart.

**Q3: How does the agent map Matrix rooms to human identities?**

If multiple humans DM the same agent, the agent receives `TurnRequest`s from different rooms. How does it know which human sent which message? The `TurnRequest` struct doesn't have a `room_id` or `sender` field.

**Force: Prohibition.** Without room/sender context, the agent cannot maintain separate conversations with different humans. This breaks P12 (every action has an author).

**Recommendation:** Extend `TurnRequest` with optional `source: MessageSource` enum (`Matrix { room_id, sender_mxid }` | `Daemon { sender_webid }` | `Cli` | `Api`). The agent's episodic memory stores the source with each turn. `recall_recent_turns()` filters by source so conversations don't bleed.

**Q4: What happens if the human loses their FluffyChat device AND their recovery key?**

The key backup table (§2.5) says "Human enters recovery key in new FluffyChat install → all message keys restored." But if the recovery key is also lost, the human cannot decrypt past messages. Can they re-verify with the agent and start fresh?

**Force: Guardrail.** This is a real user experience failure mode. The spec must address it.

**Recommendation:** Specify: (a) If recovery key is lost, human creates new FluffyChat account, re-verifies agent via new SAS QR code (admin re-runs `kask matrix register --agent` with `--reset-keys` flag). (b) Past messages are lost (cannot decrypt without old Megolm keys). (c) New E2EE session is established. (d) Agent's episodic memory retains conversation history — agent can summarize past context for the human.

**Q5: What's the Matrix room lifecycle?**

The spec says "rooms are created programmatically by agents" but doesn't specify who creates the initial DM room, when rooms are archived, or how room state is managed.

**Force: Guardrail.** Room lifecycle is a core operational concern.

**Recommendation:** Specify: (a) The human creates the DM room by searching for the agent in FluffyChat and sending the first message. (b) The agent accepts the invite automatically (via sync listener). (c) Rooms are never deleted — they persist as conversation history. (d) If a human wants a fresh start, they create a new DM room; the old room remains as archived history.

**Q6: How does `kask matrix listen` handle multiple agents?**

The spec says `kask matrix listen --agent Alice-Smith`. If the install has 5 agents, does the admin run 5 separate listeners? Or one shared sync connection?

**Force: Guardrail.** Multi-agent sync architecture affects resource consumption and code structure.

**Recommendation:** One shared `SyncService` per homeserver, filtering events by MXID. `kask matrix listen` (no `--agent` flag) starts a single sync connection for all registered agents. The `--agent` flag is for single-agent testing. This avoids N sync connections for N agents.

**Q7: What's the Conduit version compatibility guarantee?**

The docker-compose uses `registry.gitlab.com/famedly/conduit:latest`. If a new Conduit version changes the admin API, `kask matrix register` breaks.

**Force: Guardrail.** Dependency on an external project's API without version pinning is fragile.

**Recommendation:** (a) Pin a specific Conduit version tag (e.g., `conduit:0.9.0`), not `:latest`. (b) `kask matrix status-sidecar` checks Conduit version against known-compatible list. (c) `kask matrix upgrade-sidecar` pulls the new version and runs a compatibility smoke test before restarting.

---

### 12.3 Coding Guidelines — Simplicity Audit

#### Over-Specification (Simplicity First Violations)

| Finding | Location | Force | Recommendation |
|---------|----------|-------|----------------|
| **Docker-compose YAML inline** | §1.3 | Guardrail | Replace with reference to generated output. The YAML is implementation detail that will rot. |
| **ASCII QR code rendering** | §2.4 Phase 1 step 4 | Guideline | Replace with prose description. The exact label format is UI spec, not architecture. |
| **ASCII security surface diagram** | §2.3 | Guideline | Replace with mermaid diagram. ASCII art is fragile (alignment breaks on edit) and duplicates the text's information. |
| **Two topology diagrams** | §1.2 + §3.6 | Guideline | Merge into one mermaid diagram in §3.6. |

#### Missing Success Criteria (Goal-Driven Execution Violation)

The spec defines WHAT to build but not how to verify it. No acceptance criteria.

**Force: Guardrail.** Per coding-guidelines principle 4: "Define success criteria. Loop until verified."

**Recommendation:** Add a §13 (Verification) with:
1. **Integration test:** `kask matrix register --agent Test-Agent` → agent appears in Conduit user list → FluffyChat can discover and DM → SAS verification completes → message round-trip < 5s.
2. **Regulation span test:** Send a Matrix message → `reg.communication.message.received` ν-event appears in event store within 30s.
3. **Failure mode test:** Stop Conduit container → `reg.communication.matrix.sync.stalled` alert fires within 60s → `kask matrix status-sidecar` reports DOWN.
4. **Credential gate test:** `kask matrix register --agent Alice-Smith` with wrong passphrase → rejected. Confirm no Curator invocation in Regulation spans during the attempt.

---

### 12.4 Pragmatic Semantics — Epistemic Classification Audit

#### Misclassified or Unprovenanced Claims

| Claim | Current Classification | Problem | Force | Correction |
|-------|----------------------|---------|-------|------------|
| "~700 lines replacing 303 lines" | Presented as Declarative in §8.5 build estimate table | The code doesn't exist. This is a **Subjunctive projection**. | Guideline | Mark explicitly: "Estimated ~700 lines (Subjunctive — projection based on similar MCP server implementations)." |
| "Conduit ~50 MB idle" | Presented as Declarative in §1.4 comparison table | No source cited. Is this measured? From Conduit docs? | Guideline | Add citation: "Per Conduit documentation (https://conduit.rs), observed idle memory ~50 MB." |
| "Hydrogen ~5 MB compressed" | Presented as Declarative in §2.1 | No source cited. | Guideline | Add citation or mark as estimate: "~5 MB compressed (approximate, per Hydrogen project README)." |
| "matrix-rust-sdk is the most audited Matrix client library available" | Presented as Declarative in §8.7 | "Most audited" is a comparative claim. Audited by whom? When? | Guideline | Rephrase: "matrix-rust-sdk is the official Rust Matrix SDK, used by Element X (the flagship Matrix client). Its crypto implementation benefits from Element's commercial security review process." |
| `reg.communication.matrix.*` span names | Presented as if they exist in §7 and §8.4 | These spans are **proposed**, not implemented. They don't exist in the Regulation span registry. | Guardrail | Mark all proposed spans with "(proposed)" suffix. Add a §7 subsection: "Proposed Regulation Spans" listing each with its proposed threshold and algedonic level. |
| "FluffyChat is the right primary mobile client" | Classified as Probabilistic in §6 | This is a **Guideline** (OUGHT + Probabilistic), not an Evidence (IS + Probabilistic). It's a recommendation, not an inference. | Guideline | Reclassify as Guideline in §6. Move to §2.1 where recommendations belong. |

#### Missing Provenance Chains

Several claims in §6 ("What We Know") cite file paths and line numbers — good. But claims outside §6 lack provenance:

| Claim | Location | Missing Provenance |
|-------|----------|-------------------|
| "Conduit supports full Matrix 1.18 spec" | §1.4 | Source? Conduit release notes? Matrix spec compliance page? |
| "FluffyChat uses Dart bindings to the same crypto primitives" | §2.1 | Source? FluffyChat documentation? |
| "Element X uses matrix-rust-sdk" | §2.1 | Source? Element X repository? |

**Recommendation:** Add footnotes or inline citations for all factual claims about external software.

---

### 12.5 Pragmatic Cybernetics — Feedback Loop Closure Audit

#### Open Feedback Loops

**Loop 1: Conduit Health → Admin Action**

```
Sensor: reg.communication.matrix.sidecar.health (detects Conduit down)
Model: ν-event store records the outage
Regulator: Algedonic alert fires
Actuator: ??? (MISSING)
```

**Problem:** The Curator cannot restart Docker containers. The admin must act, but the spec doesn't specify how the admin is notified. The loop is **open** — it signals but has no actuator.

**Force: Prohibition.** Per P9 (Homeostatic Self-Regulation), every feedback loop must be closed. An open loop is a cybernetic failure.

**Recommendation:** Specify the notification path: algedonic alert → Curator reads → Curator cannot restart Docker (OCAP boundary) → Curator escalates to human via `reg.curation.escalation` → human receives notification (how? See below). Add an open question: how does the Curator notify a human who is only reachable via Matrix when Matrix is down? Options: (a) email (requires SMTP config — out of scope?), (b) push notification via a separate channel, (c) the human notices FluffyChat is disconnected and checks independently.

**Loop 2: Credential Verification Enforcement**

```
Sensor: ??? (MISSING — no span verifies that credential check didn't invoke Curator)
Model: ??? (MISSING)
Regulator: ??? (MISSING)
Actuator: The "no AI" prohibition is stated but not cybernetically enforced
```

**Problem:** The spec declares "no AI involvement in credential check" as a Prohibition but provides no mechanism to verify compliance. How do we know the Curator wasn't invoked? This is a **regulatory blind spot** — the prohibition exists on paper but not in the control system.

**Force: Prohibition.** Per P9, every Prohibition must be observable through Regulation spans. An unobservable prohibition is unenforceable.

**Recommendation:** Add a `reg.sovereignty.credential_check` span that fires on every `kask matrix register` invocation. The span records: `{ operation: "matrix_register", method: "direct_string_compare", ai_invoked: false }`. The `ai_invoked: false` field is a static assertion — if any code path invokes the Curator during credential check, this span would not be emitted (or would show `ai_invoked: true`). The Cybernetics Loop monitors this span and fires a Critical algedonic alert if `ai_invoked: true` is ever observed.

**Loop 3: Human Satisfaction (Known Blind Spot)**

Documented in §7 as "does NOT measure message semantic coherence, agent response quality, or human satisfaction." This is correctly identified as out of scope for the Cybernetics Loop (it's the Curation Loop's responsibility). No action needed — the boundary is correctly drawn.

#### Missing Algedonic Thresholds

The spec mentions algedonic alerts repeatedly but never specifies thresholds:

| Span | Warning Threshold | Critical Threshold | Current Status |
|------|------------------|-------------------|----------------|
| `reg.communication.matrix.sync.health` | ? | ? | **UNSPECIFIED** |
| `reg.communication.matrix.sync.stalled` | ? | ? | **UNSPECIFIED** |
| `reg.communication.matrix.message.received` (volume) | ? | ? | **UNSPECIFIED** |
| `reg.communication.matrix.e2ee.error` | ? | ? | **UNSPECIFIED** |
| `reg.communication.matrix.sender.unverified` | ? | ? | **UNSPECIFIED** |
| `reg.communication.matrix.sidecar.health` | ? | ? | **UNSPECIFIED** |

**Force: Guardrail.** Regulation spans without thresholds are sensors without setpoints — they observe but don't regulate.

**Recommendation:** Specify initial thresholds (tunable via `kask settings`):

| Span | Warning | Critical | Rationale |
|------|---------|----------|----------|
| `sync.stalled` | 60s | 300s | Sync should recover within 60s (transient network). 5 min = likely outage. |
| `message.received` (volume) | 50/min | 200/min | 50/min = active conversation. 200/min = possible spam. |
| `e2ee.error` | 5/hour | 20/hour | Occasional errors = normal (key rotation). Frequent = misconfiguration. |
| `sender.unverified` | 1/activation | 5/activation | One unverified sender = new contact. Five = possible spam campaign. |
| `sidecar.health` | 1 failure | 3 consecutive failures | Transient Docker restart = normal. 3 consecutive = crash loop. |

#### Missing Regulation Span Definitions

The spec references `reg.communication.matrix.*` spans throughout but never defines them concretely. Each span needs:
- Exact namespace (e.g., `reg.communication.matrix.sync.stalled`)
- What ν-event payload it carries
- What phase it fires in (Act, Observe, Regulate)
- Parent span (if any)

**Force: Guardrail.** Regulation spans are the observability substrate. Undefined spans are unimplementable.

**Recommendation:** Add a §7.1 (Regulation Span Specification) with a table:

| Span | Phase | Payload | Parent |
|------|-------|---------|--------|
| `reg.communication.matrix.message.received` | Observe | `{ sender_mxid, room_id, body_len, timestamp }` | — |
| `reg.communication.matrix.message.sent` | Act | `{ room_id, body_len, timestamp }` | — |
| `reg.communication.matrix.sync.health` | Observe | `{ connected: bool, latency_ms, last_sync_token }` | — |
| `reg.communication.matrix.sync.stalled` | Observe | `{ stall_duration_s, last_error }` | `sync.health` |
| `reg.communication.matrix.e2ee.error` | Observe | `{ error_type, room_id, sender_mxid }` | — |
| `reg.communication.matrix.sender.unverified` | Observe | `{ sender_mxid, room_id }` | `message.received` |
| `reg.communication.matrix.sidecar.health` | Observe | `{ conduit_up: bool, hydrogen_up: bool, db_integrity: bool }` | — |
| `reg.sovereignty.credential_check` | Act | `{ operation, method: "direct_string_compare", ai_invoked: false }` | — |

---

### 12.6 Gap Summary by Priority

#### Prohibitions (Must Fix — Spec Incomplete Without)

| # | Gap | Section |
|---|-----|---------|
| P1 | Conduit admin API authentication unspecified | 12.2 Q1 |
| P2 | Matrix room → human identity mapping unspecified (`TurnRequest` lacks source field) | 12.2 Q3 |
| P3 | Conduit health → admin notification loop is open (no actuator) | 12.5 Loop 1 |
| P4 | Credential verification enforcement has no Regulation observability | 12.5 Loop 2 |

#### Guardrails (Must Fix or Explicitly Defer)

| # | Gap | Section |
|---|-----|---------|
| G1 | Docker-compose YAML inline in architecture doc | 12.1 G1 |
| G2 | Conduit SQLite database corruption recovery unspecified | 12.2 Q2 |
| G3 | Human loses device + recovery key — re-verification flow unspecified | 12.2 Q4 |
| G4 | Matrix room lifecycle unspecified | 12.2 Q5 |
| G5 | Multi-agent sync architecture unspecified | 12.2 Q6 |
| G6 | Conduit version pinning and compatibility unspecified | 12.2 Q7 |
| G7 | Missing acceptance criteria / verification tests | 12.3 |
| G8 | Proposed Regulation spans not marked as proposed | 12.4 |
| G9 | Regulation span definitions missing (namespaces, payloads, phases) | 12.5 — **partially resolved for sidecar.health in §1.5; sync spans still need definition** |
| G10 | Algedonic thresholds unspecified for all Matrix Regulation spans | 12.5 — **partially resolved for sidecar.health and sync.stalled in §1.5; message/e2ee/sender spans still need thresholds** |

#### Guidelines (Should Fix)

| # | Gap | Section |
|---|-----|---------|
| GL1 | Duplicate topology diagram (§1.2 ASCII + §3.6 mermaid) | 12.1 G1 |
| GL2 | ASCII QR code rendering in architecture doc | 12.1 G1 |
| GL3 | Negative client recommendations in approved list | 12.1 G1 |
| GL4 | ASCII security surface diagram → replace with mermaid | 12.3 |
| GL5 | "~700 lines" estimate not marked as Subjunctive | 12.4 |
| GL6 | Missing citations for Conduit/Hydrogen memory figures | 12.4 |
| GL7 | "Most audited" claim needs rephrasing or citation | 12.4 |
| GL8 | "FluffyChat is primary" misclassified as Probabilistic instead of Guideline | 12.4 |
| GL9 | Missing provenance for external software claims | 12.4 |

---

### 12.7 Recommended Next Actions

1. **Resolve the 4 Prohibitions first** — the spec is not implementable without them.
2. **Resolve G1–G4, G7, G9, G10** — these block implementation or make the spec unverifiable.
3. **Defer G5, G6, G8** — these can be resolved during implementation with explicit "TBD" markers in the spec.
4. **Apply Guidelines during next editing pass** — they improve quality but don't block progress.
5. **Add §13 (Verification)** with concrete acceptance criteria per G7.
6. **Add §7.1 (Regulation Span Specification)** with the table from §12.5.
7. **Extend `TurnRequest`** with `source: MessageSource` per P2.
8. **Specify Conduit admin token flow** per P1.

---

### 12.8 Final Sweep — Remaining Gaps Before Build

These gaps were identified in a final end-to-end review of the complete system. They are classified as **Blocking** (cannot write code without resolving), **Important** (should resolve before build, but workarounds exist), or **Deferrable** (can resolve during or after initial implementation).

#### Blocking (Cannot Write Code Without These)

**B1 — TLS Automation Strategy.** The spec says "TLS is REQUIRED" and lists three options (nginx, Cloudflare, Traefik) but provides zero automation. The admin must manually configure a reverse proxy, obtain certificates, and set up `/.well-known` delegation. This is the single largest practical barrier to "simple for users."

**Recommendation:** `kask matrix deploy-sidecar` should generate a Caddyfile alongside the docker-compose.yml. Caddy is a single binary that auto-obtains Let's Encrypt certificates with zero configuration. Add a `caddy` service to docker-compose:

```yaml
  caddy:
    image: caddy:2-alpine
    container_name: caddy-hkask
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - ./caddy-data:/data
    networks:
      - sidecar-net
```

Caddyfile (generated):
```
matrix.example.com {
    reverse_proxy /_matrix/* conduit:8008
    reverse_proxy /_well-known/* conduit:8008
    reverse_proxy /* hydrogen:80
}
```

Caddy auto-handles TLS, `/.well-known` delegation, and reverse proxying in a single ~20 MB container. This eliminates the TLS configuration barrier entirely.

**B2 — Matrix User ID Format Specification.** The spec says the MXID is `@alice-smith:example.com` but doesn't specify the transformation rules. Matrix MXIDs allow only lowercase letters, digits, and `-._=`. The userpod name "Alice-Smith" must be lowercased. But what about names with spaces? Unicode characters? Apostrophes?

**Recommendation:** Specify the transformation: (a) Lowercase the full name. (b) Replace spaces with hyphens. (c) Strip any character not in `[a-z0-9-._=]`. (d) If the result is empty or conflicts with an existing MXID, append a numeric suffix (`-2`, `-3`). (e) Validate that the result is ≤ 255 characters (Matrix limit).

**B3 — `.well-known` Matrix Delegation.** Matrix clients discover the homeserver URL via `/.well-known/matrix/client`. Without this, FluffyChat users must manually enter `https://matrix.example.com`. The spec mentions it in Phase 1 step 2 but doesn't specify who serves it or what it contains.

**Recommendation:** The Caddy reverse proxy (B1) serves `/.well-known/matrix/client` and `/.well-known/matrix/server` by proxying to Conduit, which serves these endpoints natively. The generated `conduit.toml` must set `well_known_client` and `well_known_server` to the correct domain. This is resolved by B1 + correct Conduit config.

**B4 — Conduit Configuration Defaults.** `kask matrix deploy-sidecar` generates `conduit.toml`. What are the defaults beyond `allow_registration` and `admin_token`?

**Recommendation:** Specify the generated defaults:

| Setting | Default | Notes |
|---------|---------|-------|
| `server_name` | `matrix.example.com` (from `--domain` flag) | |
| `allow_registration` | `true` | User can disable after setup |
| `admin_token` | Random 64-char hex string | Stored in hkask-keystore |
| `well_known_client` | `https://matrix.example.com` | |
| `max_request_size` | `20_000_000` (20 MB) | Default Conduit value |
| `db_path` | `/var/lib/conduit/conduit.db` | Docker volume mount |
| `log_level` | `info` | |
| `port` | `8008` | Internal — Caddy proxies externally |

#### Important (Should Resolve Before Build)

**I1 — E2EE Recovery Key for Agents.** The agent's E2EE keys are in `hkask-keystore`. But Matrix E2EE has a **recovery key** (AES-256) that can decrypt all past messages if the device key is lost. Should hKask generate one for the agent?

**Recommendation:** Yes. `kask matrix register --agent` generates a recovery key, stores it in `hkask-keystore`, and offers to display it for admin backup. The recovery key is NOT included in the QR code (that's only for SAS verification). If the cloud server is lost and restored from backup, the admin imports the recovery key to decrypt past messages.

**I2 — Agent Device Display Name.** Matrix devices have human-readable names (e.g., "Element X on iPhone 15"). What should hKask agents' device name be?

**Recommendation:** `"hKask Agent Alice-Smith ({hostname})"` where `{hostname}` is the server's hostname. This helps the human identify which server their agent is on (useful if they have multiple hKask installs).

**I3 — Message Format Specification.** What Matrix message type do agents send? Plain text? Markdown? Matrix supports `m.text` (plain), `m.notice` (system messages), and `formatted_body` (Markdown/HTML).

**Recommendation:** Agents send `m.text` with a `formatted_body` in Markdown. The `body` field is plain text (for clients that don't render Markdown). The `formatted_body` is `org.matrix.custom.html` with the Markdown rendered to HTML. This matches how Element and FluffyChat send messages.

**I4 — Room Encryption Defaults.** Should agent DMs be encrypted by default?

**Recommendation:** Yes. `kask matrix register --agent` creates the agent's Matrix session with `m.room.encryption` enabled by default. All rooms the agent participates in are encrypted (Megolm). The agent rejects unencrypted messages in rooms where encryption is enabled. This is the standard Matrix E2EE behavior — the SDK handles it.

**I5 — Error Taxonomy for Sync Loop.** The spec says "emit `reg.communication.matrix.e2ee.error`" but doesn't define error types. The SDK can encounter: decryption failures, key mismatch, room state conflicts, rate limiting, federation errors.

**Recommendation:** Define error categories for the Regulation span payload:

| Error Type | Meaning | Severity |
|-----------|---------|----------|
| `decryption_failed` | Cannot decrypt a message (missing key, wrong session) | Warning |
| `key_mismatch` | Sender key doesn't match verified identity | Warning |
| `room_state_conflict` | Room state resolution failed | Warning |
| `rate_limited` | Homeserver throttling requests | Warning |
| `federation_error` | Remote homeserver unreachable | Warning (only if federation enabled) |
| `unknown` | Unclassified SDK error | Warning |

All are Warning level — E2EE errors are operational noise, not Critical unless they persist at high volume (see G10 thresholds).

**I6 — Gas Accounting for Matrix Messages.** Open question #5 from §10. Still unresolved.

**Recommendation:** Resolve now with a simple model:
- Receiving a Matrix message: **1 hJoule** (fixed — covers sync processing, not inference)
- Sending a Matrix message: **0 hJoules** (the inference gas for the response is already accounted in `ChatService::execute_turn()`)
- Spam protection: if an agent receives > 50 messages/minute, the Curator throttles activation to 1 activation/minute for that agent. The throttled state is recorded in `reg.communication.matrix.throttled`.

#### Deferrable (Can Resolve During or After Implementation)

**D1 — Hydrogen Configuration.** What's in `hydrogen-config.json`? Hydrogen needs to know the homeserver URL and some UI preferences.

**Recommendation:** Defer. The generated config is minimal: `{ "default_homeserver": "https://matrix.example.com" }`. Hydrogen's default UI is acceptable.

**D2 — Conduit Data Volume Backup Strategy.** Where do backups go? How are they rotated?

**Recommendation:** Defer. Document in a user guide: daily `sqlite3 .backup` to a timestamped file, keep 7 days of backups, store off-server. This is operational procedure, not architecture.

**D3 — Multiple hKask Installs Sharing One Conduit.** Can two hKask installs use the same Conduit?

**Recommendation:** Defer. This is a multi-tenant architecture question. For v1, one Conduit per hKask install. Document as future enhancement.

**D4 — Agent De-registration.** How do you remove an agent from Matrix?

**Recommendation:** Defer. `kask matrix unregister --agent Alice-Smith` deactivates the Matrix account (password reset to random, display name changed to "deactivated"). Rooms remain but agent no longer syncs. Add when needed.

**D5 — Code-Level Enforcement of "No AI" Prohibition.** The Regulation span `reg.sovereignty.credential_check` observes compliance, but how is it enforced at the code level?

**Recommendation:** Defer the enforcement mechanism, specify the design intent: `kask matrix register` is implemented as a direct function in `hkask-cli` that calls `hkask-keystore` for credential verification and `hkask-mcp-communication` for Conduit API calls. It does NOT route through `AgentService`, `ChatService`, or any Curator code path. The Regulation span is emitted by the CLI command itself, not by the Curator. If this is violated during implementation, the span won't fire — that's the detection mechanism. Compile-time enforcement (separate binary, feature flags) is future work.

---

### 12.9 Updated Next Actions (Post-Sweep)

1. **Resolve B1–B4 (Blocking)** — ✅ DONE. Caddy added, MXID format specified, `.well-known` delegated to Caddy, Conduit config defaults defined.
2. **Resolve I1–I6 (Important)** — ✅ DONE. Recovery key, device name, message format, room encryption, error taxonomy, gas accounting all specified.
3. **Resolve the 4 Prohibitions** — P1 ✅ (admin token in B4), P2 ✅ (TurnRequest.source implemented), P3 deferred (human notification when Matrix down), P4 deferred (credential Regulation span).
4. **Resolve G1–G4, G7, G9, G10** — G1–G4 ✅, G7 ✅ (see §13), G9 ✅ (see §7.1), G10 ✅ (thresholds in §1.5 and §7.1).
5. **Defer D1–D5** — ✅ Marked TBD.
6. **Add §13 (Verification)** — ✅ See below.
7. **Add §7.1 (Regulation Span Specification)** — ✅ See below.
8. **Extend `TurnRequest`** — ✅ `source: MessageSource` field added.
9. **Add Caddy to docker-compose** — ✅ Done.

---

## 13. Verification — Acceptance Criteria

**Purpose:** Define verifiable success criteria for the Matrix integration. Per coding-guidelines principle 4: "Define success criteria. Loop until verified."

### 13.1 Integration Tests

**T1 — Sidecar Deployment:**
```bash
kask matrix deploy-sidecar --domain matrix.example.com
# Verify: ~/.config/hkask/sidecar/ contains docker-compose.yml, Caddyfile, conduit.toml
# Verify: docker-compose.yml references caddy:2-alpine, conduit:0.9.0
# Verify: admin token is 64 hex chars, stored in OS keychain as HKASK_MATRIX_ADMIN_TOKEN
```

**T2 — Agent Registration:**
```bash
kask matrix register --agent Alice-Smith
# Verify: prompts for credential "FirstName-LastName/Passphrase"
# Verify: MXID derived as @alice-smith:example.com (lowercase, spaces→hyphens)
# Verify: POST to Conduit admin API succeeds (200)
# Verify: agent credentials stored in keychain (key names: HKASK_MATRIX_AGENT_USERNAME_<AgentName>, HKASK_MATRIX_AGENT_PASSWORD_<AgentName>)
# Verify: output includes labeled verification instructions
```

**T3 — User Registration:**
```bash
kask matrix register --user Bob-Jones
# Verify: MXID derived as @bob-jones:example.com
# Verify: POST to Conduit admin API succeeds (200)
# Verify: output includes MXID and generated password
```

**T4 — Message Send (via MCP tool):**
```bash
# Start communication server with agent credentials:
HKASK_MATRIX_AGENT_USERNAME=@alice-smith:example.com \
HKASK_MATRIX_AGENT_PASSWORD=<password> \
kask mcp start communication
# Call send_message tool:
# Verify: message appears in Matrix room
# Verify: reg.communication.matrix.message.sent tracing span emitted
```

**T5 — Message Poll (via MCP tool):**
```bash
# Human sends message to agent via FluffyChat
# Agent calls get_messages(room_id, limit=20):
# Verify: human's message appears in returned Vec<MatrixMessage>
# Verify: reg.communication.matrix.messages.polled tracing span emitted
```

**T6 — Status Check:**
```bash
kask matrix status-sidecar
# Verify: reports container status for Caddy, Conduit, Hydrogen (if enabled)
# Verify: when containers are down, reports DOWN with appropriate messaging
```

### 13.2 Regulation Span Verification

**T7 — Span Emission:**
```bash
# After running T4 and T5, verify the following spans were emitted:
# - reg.communication.matrix.health (on health_check)
# - reg.communication.matrix.login (on login)
# - reg.communication.matrix.message.sent (on send_message)
# - reg.communication.matrix.messages.polled (on get_messages)
# - reg.communication.server.started (on server start)
```

### 13.3 Failure Mode Tests

**T8 — Homeserver Unreachable:**
```bash
# Stop Conduit container: docker compose stop conduit
# Run: kask matrix status-sidecar
# Verify: reports Conduit DOWN
# Run: send_message via MCP tool
# Verify: returns MatrixError::Unavailable
```

**T9 — Credential Gate:**
```bash
kask matrix register --agent Alice-Smith
# Enter wrong credential format
# Verify: warns about format but proceeds (credential verification is TODO)
# Verify: no Curator/LLM invocation (direct CLI function, no AgentService path)
```

### 13.4 MXID Derivation Tests

**T10 — Name Transformations:**
```bash
# Verify these transformations (unit-testable in derive_mxid):
# "Alice-Smith" → @alice-smith:example.com
# "Bob Jones" → @bob-jones:example.com
# "María García" → @mara-garca:example.com (unicode stripped)
# "O'Reilly" → @oreilly:example.com (apostrophe stripped)
# "" → @agent-{8-hex}:example.com (fallback)
```

---

## 7.1 Regulation Span Specification

**Purpose:** Define every `reg.communication.matrix.*` span emitted by the Matrix integration. Each span specifies its namespace, phase, payload schema, and algedonic thresholds.

**Status: Implemented.** Spans are emitted via `tracing` macros in the current implementation. Additionally, the 7R7 Listener persists observed messages as `RegulationRecord` records into the `RegulationArchive` via the Regulation bridge, making communication events visible to the Curation Loop. Formal Regulation registry registration (`RegulationSpan` enum variants) is deferred.

### Implemented Spans (v1 — Operational)

| Span | Phase | Payload | Emitted By |
|------|-------|---------|------------|
| `reg.communication.matrix.health` | Observe | `{ url }` | `MatrixTransport::health_check()` |
| `reg.communication.matrix.login` | Act | `{ username, homeserver }` | `MatrixTransport::login()` |
| `reg.communication.matrix.message.sent` | Act | `{ room_id, body_len }` | `MatrixTransport::send_message()` |
| `reg.communication.matrix.messages.polled` | Observe | `{ room_id, count }` | `MatrixTransport::get_messages()` |
| `reg.communication.thread.created` | Act | `{ room_id, name }` | `MatrixTransport::create_room()` |
| `reg.communication.agent.invited` | Act | `{ room_id, user }` | `MatrixTransport::invite_user()` |
| `reg.communication.agent.registered` | Act | `{ webid, matrix_user }` | `AgentRegistry::record_mapping()` |
| `reg.communication.server.started` | Act | `{ url, agent? }` | `main.rs` entry point |

| `reg.communication.message.observed` | Observe | `{ source_event_id, room_id, sender, body, timestamp }` | `SevenR7Listener::poll()` — self messages and replayed event IDs are ignored before Curation |

### Deferred Spans (v2 — requires continuous sync)

These spans are specified but not implemented. They require the continuous sync loop (deferred until VOIP/real-time use case exists).

| Span | Phase | Payload | Threshold (Warning/Critical) |
|------|-------|---------|------------------------------|
| `reg.communication.matrix.message.received` | Observe | `{ sender_mxid, room_id, body_len, timestamp }` | — |
| `reg.communication.matrix.sync.health` | Observe | `{ connected, latency_ms, last_sync_token }` | — |
| `reg.communication.matrix.sync.stalled` | Observe | `{ stall_duration_s, last_error }` | 60s / 300s |
| `reg.communication.matrix.e2ee.error` | Observe | `{ error_type, room_id, sender_mxid }` | 5/hr / 20/hr |
| `reg.communication.matrix.sender.unverified` | Observe | `{ sender_mxid, room_id }` | 1/activation / 5/activation |
| `reg.communication.matrix.sidecar.health` | Observe | `{ caddy_up, conduit_up, conduit_api_ok, conduit_db_ok, hydrogen_up }` | 1 failure / 3 consecutive |
| `reg.communication.matrix.throttled` | Observe | `{ agent, reason, message_rate }` | — |
| `reg.sovereignty.credential_check` | Act | `{ operation, method, ai_invoked }` | — |

### Algedonic Thresholds (v1 — sidecar health + message observation)

The 7R7 Listener emits `reg.communication.message.observed` spans for every message, persisting them as RegulationRecords. CurationLoop.sense() filters communication.* events from its own query_algedonic() call and pushes them to the curation context. Sidecar health is checked on-demand via `kask matrix status-sidecar`. The daemon periodic health task is deferred. When implemented, thresholds are:

| Condition | Warning | Critical |
|-----------|---------|----------|
| `caddy_up: false` | — | 3 consecutive checks |
| `conduit_up: false` | 1 check | 3 consecutive checks |
| `conduit_db_ok: false` | — | Immediate |
| `hydrogen_up: false` | 1 check | — (optional component) |

---

## Well Wallet Architecture (Merged from well-wallet-architecture.md)


# Well & Wallet Architecture

**Adversarial review complete.** 6 skills applied. 5 items deleted. 3 guardrails closed. 1 prohibition resolved.

## 1. Conceptual Model

```
┌──────────────┐     auto-draw        ┌──────────────┐     spend     ┌──────────┐
│              │ ◄────────────────── │              │ ────────────► │          │
│    WELL      │                     │   WALLET     │               │  AGENT   │
│  (source)    │                     │  (storage)   │               │ (spender)│
│              │                     │              │               │          │
└──────┬───────┘                     └──────────────┘               └──────────┘
       │ admin configures                                              │
       ▼                                                              │
┌──────────────┐                                                observes
│   CURATOR    │ ◄──────────────────────────────────────────────┘
│  (daemon)    │
│              │── reports to ──► ┌──────────────┐
└──────────────┘                  │    HUMAN     │
                                  │ ADMINISTRATOR│
                                  │  (S5* audit) │
                                  └──────────────┘
```

- **Well**: Admin-configured gas/rJoule source. Auto-replenishes on schedule. One default Well per installation. [OUGHT: future — crypto → gas/rJoule conversion; then Hedera HTS token]
- **Wallet**: Per-agent balance store. Created by Curator on userpod registration. Agent's own property.
- **Auto-draw**: When `WalletBackedBudget` is low, automatically draws from the default Well. Closes the cybernetic feedback loop.
- **Curator**: Creates wallets, monitors balances, reports to admin. Has unlimited gas with admin-configured efficiency limits.
- **Human Administrator**: Configures Wells, sets Curator policy, is the S5* observer-of-observer.

## 2. Well

### 2.1 Definition

A Well produces gas and rJoule on schedule. One default Well per installation. Wells are the sole source of new gas/rJoule entering the system.

**IS vs OUGHT note**: Currently `register_gas_budget(GasBudget::new(...))` creates gas from nothing. After Well implementation, `register_gas_budget` is gated behind Well authorization — budgets come from wallet draws, which come from Wells. Until Wells exist, the old path remains.

### 2.2 WellConfig

```rust
struct WellConfig {
    well_id: String,
    /// Gas produced per replenishment cycle
    gas_rate: GasCost,
    /// rJoule produced per replenishment cycle
    rjoule_rate: u64,
}
```

### 2.3 WellManager

```rust
impl WellManager {
    /// Admin creates a Well. Regulation span: reg.well.created
    async fn create_well(config: WellConfig) -> Result<WellID>;

    /// Replenish the Well (called on schedule by CyberneticsLoop).
    /// Regulation span: reg.well.replenished
    async fn replenish(&self);

    /// Agent draws from the Well. Returns amount drawn.
    /// Regulation span: reg.well.draw
    async fn draw(&self, agent: WebID, amount_gas: GasCost, amount_rjoule: u64)
        -> Result<(GasCost, u64)>;

    /// Check if Well is exhausted. Regulation span: reg.well.exhausted when true.
    async fn is_exhausted(&self) -> bool;
}
```

### 2.4 Well Exhaustion — Regulatory Path

When `is_exhausted()` returns true during a replenishment cycle:
1. Regulation span: `reg.well.exhausted` fires
2. CyberneticsLoop sends `CurationInput::Alert` via algedonic pathway
3. Curator notifies admin: "Well X is exhausted — agents may be blocked"
4. Admin increases `gas_rate` or `rjoule_rate` via `kask well update`

## 3. Wallet

### 3.1 Schema

```sql
CREATE TABLE agent_wallets (
    wallet_id TEXT PRIMARY KEY,
    agent_webid TEXT NOT NULL UNIQUE,
    gas_balance INTEGER NOT NULL DEFAULT 0,
    rjoule_balance INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    last_draw_at TEXT
);
```

### 3.2 WalletManager Trait

```rust
#[async_trait]
trait WalletManager {
    /// Create wallet for new userpod. Called by Curator daemon.
    /// Regulation span: reg.wallet.created
    #[must_use]
    async fn create_wallet(&self, agent: WebID, initial_gas: GasCost, initial_rjoule: u64)
        -> Result<WalletID, WalletError>;

    /// Draw from Well into agent's wallet. Called by auto-draw or manual CLI.
    /// Regulation span: reg.wallet.draw
    #[must_use]
    async fn draw_from_well(&self, agent: WebID, amount_gas: GasCost)
        -> Result<GasCost, WalletError>;

    /// Spend from wallet. Called by WalletBackedBudget on reserve/settle.
    #[must_use]
    async fn spend(&self, agent: WebID, amount: GasCost)
        -> Result<GasCost, WalletError>;

    /// Query wallet balance.
    async fn balance(&self, agent: WebID) -> Result<WalletBalance, WalletError>;
}
```

### 3.3 Auto-Draw — Closing the Feedback Loop

When `WalletBackedBudget.can_proceed()` returns false:
1. `GasBudgetManager` detects the block
2. Auto-draws `replenish_rate` amount from the default Well
3. If Well is exhausted, emits algedonic alert (see §2.4)
4. If draw succeeds, agent resumes operations

This replaces the current silent block with a closed cybernetic loop: exhaustion → draw → Well alert → admin action.

### 3.4 Wallet Creation Flow

```
1. AgentService registers new userpod
2. Regulation span fires: reg.userpod.registered  [NEW — must be added to Regulation registry]
3. Curator daemon receives span
4. Curator calls: WalletManager::create_wallet(agent, initial_gas, initial_rjoule)
5. GasBudgetManager registers WalletBackedBudget for the agent
6. Regulation span: reg.wallet.created
```

## 4. Curator Budget Policy (G11 Resolution)

The Curator has **unlimited gas** (`hard_limit = false`). Limits are efficiency-based, not budget-based:

```rust
struct CuratorBudgetPolicy {
    /// Max LLM tokens per regulation cycle
    max_tokens_per_cycle: u64,
    /// Max tool invocations per cycle
    max_tool_calls_per_cycle: u32,
    /// When true, skip remaining tool calls this cycle when exceeded.
    /// When false, log Regulation span and continue.
    throttle_on_exceeded: bool,
}
```

When limits are exceeded, Regulation span `reg.curator.efficiency.exceeded` fires. The Human Administrator reviews efficiency reports and adjusts limits. The admin is the S5* observer — the logical backstop.

## 5. Regulation Span Registry Additions

```rust
// Well
WellCreated,         // reg.well.created
WellReplenished,     // reg.well.replenished
WellDraw,            // reg.well.draw
WellExhausted,       // reg.well.exhausted → algedonic alert

// Wallet
WalletCreated,       // reg.wallet.created
WalletDraw,          // reg.wallet.draw
WalletSpend,         // reg.wallet.spend
WalletExhausted,     // reg.wallet.exhausted → algedonic alert

// UserPod lifecycle (must exist for wallet creation flow)
UserPodRegistered, // reg.userpod.registered → triggers wallet creation

// Curator efficiency
CuratorEfficiencyExceeded,  // reg.curator.efficiency.exceeded
```

## 6. Implementation — ✅ Complete

All 12 steps implemented. See `docs/status/gas-budget-system-status.md` for full status.

### Phase 2 (deferred): Authorization + Transfer + Hedera

- Well authorization per-agent
- Wallet-to-wallet transfer
- Hedera HTS token: rJoule on-chain

## 7. Verification

```bash
cargo build --workspace                    # 0 errors
cargo test -p hkask-regulation                    # Regulation tests pass
cargo test -p hkask-storage -- wallet      # Wallet persistence tests
cargo test -p hkask-pods -- curator      # Curator wallet creation test
```

---

*Review applied: pragmatic-semantics, pragmatic-cybernetics, idiomatic-rust, essentialist, grill-me, coding-guidelines.*

---

## Scenarios–Companies Bridge (Merged from scenarios-companies-bridge.md)


# Scenarios–Companies Bridge [^mcp]

## Architecture

The `hkask-mcp-companies` server has domain-specific scenario logic in:
- `scenarios.rs` — Schwartz 2×2 matrix with financial model projections
- `superforecast.rs` — Fermi decomposition + Bayesian update for financial drivers

The `hkask-mcp-scenarios` server now has the **canonical** implementations of:
- Fermi decomposition (`scenario_calibrate`)
- Bayesian updating (`scenario_update`)
- Brier scoring (`scenario_score`)
- Event tree quantification (`scenario_quantify`)
- Dragonfly-eye synthesis (`scenario_synthesize`)
- Calibration tracking (`scenario_calibration`)
- Triage (`scenario_triage`)

## Deduplication Plan [^rust]

### What stays in companies
- `scenarios.rs` → `ScenarioMatrix::growth_x_margin()` — financial-model-specific 2×2
- `scenarios.rs` → `run_scenario_analysis()` — plugs into 11-line-item DCF
- Financial-model-specific Fermi sub-questions (growth rate + margin decomposition)

### What moves to scenarios (canonical)
- `superforecast.rs` → `FermiDefaults`, `calibrate_from_fermi`, `outside_view_adjustment`,
  `bayesian_update`, `brier_score*`, `distribute_scenario_probabilities`

## Bridge Path [^mcp]

### Path 1: companies → scenarios (calibration delegation)

```
CompaniesServer.calibrate_forecast
  → build financial-model-specific Fermi sub-questions
  → serialize to ScenarioEvent format
  → call ScenariosServer.scenario_calibrate via MCP (future: MCP client)
  → receive calibrated probability
  → run financial model projection with calibrated growth/margin
```

### Path 2: scenarios → companies (outcome recording)

```
ScenariosServer.scenario_score (for a company event)
  → compute Brier score
  → record outcome in forecast_store
  → future: notify CompaniesServer of resolved forecast
  → CompaniesServer updates financial model backtest
```

### Path 3: Portfolio aggregation (future)

```
CompaniesServer.portfolio_characteristics
  → for each holding, fetch event tree from ScenariosServer
  → cross-reference event dependencies across holdings
  → compute portfolio concentration on shared assumptions
```

## Shared Engine [^rust]

Both servers now delegate to `hkask-forecast` (crates/hkask-forecast) for canonical implementations of:
- `calibrate_from_fermi` — Fermi decomposition
- `outside_view_adjustment` — Base rate calibration
- `bayesian_update` — Bayesian evidence revision
- `brier_score` / `brier_score_multi` — Brier scoring
- `brier_interpretation` — Human-readable score interpretation

Each server wraps these with its own error types and SubQuestion conversions.

## Implementation Status [^mcp]

**Working today (both directions via shared engine):**
- Companies' `calibrate_from_fermi` → `hkask_forecast::calibrate_from_fermi` (same computation as scenarios)
- Companies' `brier_score` → `hkask_forecast::brier_score` (same scoring)
- `scenario_from_companies` converts companies output into ScenarioEvents for full pipeline

**Working today (companies → scenarios via MCP bridge):**
- Companies output → `scenario_from_companies` → quantify → calibrate → score

**Not yet wired (live MCP call from companies to scenarios):**
- Companies server cannot call `scenario_calibrate` as an MCP tool at runtime
- Requires MCP client protocol in companies server (rmcp client support exists but isn't plumbed)

## Implementation Notes [^mcp]

[^mcp]: Model Context Protocol. (2025). *Specification*. https://modelcontextprotocol.io/specification/2025-06-18
[^rust]: Rust Project. (2026). *The Rust Programming Language*. https://doc.rust-lang.org/book/

The bridge requires MCP client support in the companies server. Currently,
MCP servers in hKask are stdio-based and don't have client protocols to
call other servers. The MCP client protocol exists in `rmcp` but hasn't
been wired into the companies server's tool methods.

When MCP client support is added:
1. `calibrate_forecast` tool delegates Fermi calibration to `scenario_calibrate`
2. `forecast_record` tool records outcomes in `scenario_score`
3. Companies server drops its `superforecast.rs` module

---



# hKask Federation v2 — Curator-CRDT Federation Design

> **⚠ DEPRECATED (removed for now).** The federation crate has been removed from the workspace. This design is retained as a historical/archival reference for a possible future revival. It is not current architecture; do not treat the crate references below as live.

**Purpose:** Complete design for federating hKask servers through curator-to-curator CRDT sync, merged user/agent registries, federated Matrix conduits, and separate skill registries. Incorporates all findings from the five-addendum audit cycle.

---

## 1. Federation Model

### 1.1 Horizontal Extension of the Pod Architecture

The federation extends the existing two-kind pod architecture **horizontally**:

```
┌──────────────────────────────────┐     ┌──────────────────────────────────┐
│         hKask Server A           │     │         hKask Server B           │
│                                  │     │                                  │
│  ┌────────────────────────────┐  │     │  ┌────────────────────────────┐  │
│  │     CuratorPod (A)         │◄─┼─CRDT─┼─┤     CuratorPod (B)         │  │
│  │  ┌──────────────────────┐  │  │     │  │  ┌──────────────────────┐  │  │
│  │  │ FederationSync       │  │  │     │  │  │ FederationSync       │  │  │
│  │  │ ┌──────────────────┐ │  │  │     │  │  │ ┌──────────────────┐ │  │  │
│  │  │ │ OR-Set (EAV key) │◄┼──┼──┼──┼──┼─┤  │ │ OR-Set (EAV key) │ │  │  │
│  │  │ │ PayloadStore     │ │  │  │     │  │  │ │ PayloadStore     │ │  │  │
│  │  │ │ LWW-Map (users)  │ │  │  │     │  │  │ │ LWW-Map (users)  │ │  │  │
│  │  │ │ G-Set (agents)   │ │  │  │     │  │  │ │ G-Set (agents)   │ │  │  │
│  │  │ │ OR-Set (artifacts)│ │  │  │     │  │  │ │ OR-Set (artifacts)│ │  │  │
│  │  │ └──────────────────┘ │  │  │     │  │  │ └──────────────────┘ │  │  │
│  │  └──────────────────────┘  │  │     │  │  └──────────────────────┘  │  │
│  └────────────────────────────┘  │     │  └────────────────────────────┘  │
│                                  │     │                                  │
│  Skill Registry A (PRIVATE)      │     │  Skill Registry B (PRIVATE)      │
│  UserPodPods A (PRIVATE)       │     │  UserPodPods B (PRIVATE)       │
│  TeamPods A (PRIVATE)            │     │  TeamPods B (PRIVATE)            │
│                                  │     │                                  │
│  Matrix Conduit A ◄──────────────┼─Fed─┼─► Matrix Conduit B               │
└──────────────────────────────────┘     └──────────────────────────────────┘
```

### 1.2 What Is Federated (Shared)

| Artifact | Mechanism | CRDT Type |
|----------|-----------|-----------|
| Public semantic hMems | `SemanticMemory` → ν-event → EAV hash → OR-Set | OR-Set (content-addressed) |
| User profiles (metadata) | `UserStore` → LWW-Map | LWW-Map (wall-clock, Guideline) |
| Agent registrations | `AgentRegistry` → G-Set | G-Set (additive) |
| Public artifacts | `ArtifactIndex` → content hash → OR-Set | OR-Set (content-addressed) |
| Matrix messaging | Standard Matrix federation protocol | N/A (Matrix native) |

### 1.3 What Remains Local (NOT Federated)

| Artifact | Reason | Principle |
|----------|--------|-----------|
| Skill registries (`manifest.yaml` + `*.j2`) | Single source of truth per server | P5.1 |
| Episodic memory (private hMems) | Sovereignty boundary — never leaves home SQLCipher | P1, P11.1 |
| Agent personas, capabilities, OCAP tokens | WebID-grounded to home server | P12 |
| Regulation runtimes (variety counters, gas budgets) | Per-pod regulation | P9 |
| Wallet, keystore, ledger | Financial infrastructure scope | P4 |

---

## 2. CRDT Design — ν-Event-Grounded, Timestamp-Free

### 2.1 Semantic hMems — OR-Set Keyed by EAV Hash

The CRDT key for hMems is the **EAV content hash** — the same BLAKE3 hash already used by `recall_dedup::eav_hash()`. This is metadata-independent: entity + attribute + canonical value only. Two servers independently observing the same fact produce the same key.

```rust
/// CRDT key for federation semantic hMems.
/// Content-addressed via EAV hash — same fact → same key → automatic convergence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FederationHMemKey {
    eav_hash: [u8; 32],
}

impl FederationHMemKey {
    pub fn from_h_mem(triple: &hMem) -> Self {
        Self { eav_hash: hkask_memory::recall_dedup::eav_hash(triple) }
    }
}

/// CRDT dot — pure causal ordering. No timestamp dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dot {
    pub replica: ReplicaId,
    pub counter: u64,
}

/// Federation OR-Set for semantic hMems.
pub struct FederationSemanticSet {
    add_set: HashMap<FederationHMemKey, Vec<Dot>>,
    remove_set: HashMap<FederationHMemKey, Vec<Dot>>,
    replica: ReplicaId,
    counter: AtomicU64,
}
```

### 2.2 Same-Fact Convergence (No Conflict)

```
Server A publishes: (sensor1, temp, 25) → EAV hash 0x3f7a → dot (alpha, 42)
Server B publishes: (sensor1, temp, 25) → EAV hash 0x3f7a → dot (beta, 17)

OR-Set merge: element 0x3f7a has dots [(alpha, 42), (beta, 17)]
→ contains() = true on both servers
→ No conflict. No timestamp comparison. Automatic convergence.
```

### 2.3 Divergent-Fact Retention (Both Kept)

```
Server A publishes: (sensor1, temp, 25, confidence=1.0) → EAV hash 0x3f7a
Server B publishes: (sensor1, temp, 26, confidence=0.8) → EAV hash 0x8b2c

OR-Set merge: BOTH elements retained
→ SemanticIndex query returns both values
→ Curator resolves: prefer higher confidence (1.0 > 0.8) ✓
```

### 2.4 Conflict Resolution Table

| Data Type | CRDT | Key | Conflict Strategy |
|-----------|------|-----|-------------------|
| **Semantic hMems** | OR-Set | EAV hash | Same key → converge. Different key → both retained → Curator resolves via confidence + provenance. |
| **User profiles** | LWW-Map | WebID | Highest wall-clock timestamp + replica_id tiebreak. Guideline (metadata, not ν-event-grounded). |
| **Agent registrations** | G-Set | WebID | Union (additive, no conflicts). |
| **Public artifacts** | OR-Set | Content hash (BLAKE3) | Same as hMems — content-addressed, both retained if content differs. |

### 2.5 ν-Event Pipeline

The CRDT merged state is a **derived view** — rebuildable from ν-events:

```
ν-events (canonical) → extract hMems → OR-Set add (EAV hash + dot)
    → CRDT sync (version vector merge) → reconcile to PayloadStore (upsert by confidence)
    → materialize to SemanticIndex
```

If CRDT state and ν-events disagree: rebuild from ν-events. ν-event primacy preserved.

### 2.6 hMem Payload Store

The OR-Set determines which EAV hashes exist. Full hMem data is stored separately:

```rust
pub struct HMemPayloadStore {
    payloads: HashMap<FederationHMemKey, hMem>,
}

impl HMemPayloadStore {
    pub fn upsert(&mut self, triple: hMem) {
        let key = FederationHMemKey::from_h_mem(&triple);
        self.payloads
            .entry(key)
            .and_modify(|existing| {
                if triple.confidence > existing.confidence {
                    *existing = triple.clone();
                }
            })
            .or_insert(triple);
    }
}
```

---

## 3. FederationLink State Machine

### 3.1 States

| State | Meaning | Sync Active | Reversible |
|-------|---------|-------------|------------|
| `Isolated` | No link exists | No | N/A |
| `Invited` | Invitation sent, awaiting response | No | Yes (timeout/reject → Isolated) |
| `Linked` | Link established, CRDT sync active | Yes | Yes (→ Paused or → Degraded) |
| `Paused` | Sync suspended intentionally (security measure) | No | Yes (→ Linked) |
| `Degraded` | Sync failed — may be partition, peer death, or lost pause notification | No | Yes (→ Linked on recovery, → Revoked on timeout) |
| `Revoked` | Permanently terminated | No | No (terminal) |

### 3.2 State Transition Diagram

```mermaid
stateDiagram-v2
    [*] --> Isolated

    Isolated --> Invited : send FEDERATION_INVITE
    Invited --> Linked : receive FEDERATION_ACCEPT
    Invited --> Isolated : timeout or FEDERATION_REJECT

    Linked --> Paused : pause (security measure)
    Paused --> Linked : resume

    Linked --> Degraded : sync timeout (consecutive failures > threshold)
    Degraded --> Linked : sync resumes (partition healed)
    Degraded --> Revoked : extended degradation (e.g., 7 days)

    Paused --> Degraded : LINK_PAUSE message lost (peer appears dead)
    Paused --> Revoked : revoke while paused
    Linked --> Revoked : revoke member / dissolve

    Revoked --> [*]
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-015
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

### 3.3 The Degraded State (PC-2 Resolution)

The `Degraded` state solves the broken feedback closure identified in the cybernetic audit. When a `LINK_PAUSE` message is lost during a network partition, the peer detects sync timeout and transitions to `Degraded` rather than staying in `Linked` (model-reality divergence) or jumping to `Revoked` (too drastic).

```rust
pub enum LinkState {
    // ... other states ...
    Degraded {
        degraded_at: DateTime<Utc>,
        failed_attempts: u64,
        last_success_at: DateTime<Utc>,
    },
}
```

Regulation span: `reg.federation.link_degraded` with `{failed_attempts, last_success_age_secs}`.

---

## 4. Invitation and Lifecycle Protocol

### 4.1 Invitation Flow

```
Curator A                          Curator B
   │                                  │
   │ kask federation invite           │
   │   --peer beta                    │
   │                                  │
   │ state: Isolated → Invited        │
   │ Regulation: invite_sent                 │
   │                                  │
   │─── FEDERATION_INVITE ───────────►│
   │    {inviter, tls, config}        │
   │                                  │ Regulation: invite_received
   │                                  │
   │                                  │── Admin reviews (manual default, P2)
   │                                  │   or auto_accept policy
   │                                  │
   │◄── FEDERATION_ACCEPT ────────────│
   │                                  │
   │ state: Invited → Linked          │ state: Isolated → Linked
   │ Regulation: link_established            │ Regulation: link_established
   │                                  │
   │── Matrix conduit federation ────►│
   │── CRDT bootstrap ───────────────►│
   │── Registry merge ───────────────►│
```

### 4.2 Invitation Policy Seam

```rust
/// In hkask-types:
pub trait InvitationPolicy: Send + Sync {
    fn evaluate(&self, invitation: &FederationInvitation) -> InvitationDecision;
}

pub enum InvitationDecision {
    Accept,
    Reject { reason: String },
    DeferToAdmin,  // P2 default
}
```

Default implementations: `ManualInvitationPolicy` (always `DeferToAdmin`), `AllowListInvitationPolicy` (accept configured peers), `RateLimitingInvitationPolicy` (wrapper).

### 4.3 Three Revocation Operations

| Operation | Scope | Regulation Span | Gossip |
|-----------|-------|----------|--------|
| **Revoke member** | Single peer | `federation.member_revoked` | Yes — other members notified |
| **Leave federation** | Self | `federation.member_left` | Yes — voluntary departure |
| **Dissolve federation** | All links (batched) | `federation.dissolved` | Yes — `FEDERATION_GOODBYE` to all |

Dissolution is decentralized: each Curator dissolves its own links. No global coordinator.

### 4.4 CuratorDirective Extensions

```rust
pub enum CuratorDirective {
    // ... existing variants ...

    InviteToFederation { peer_replica, peer_server_domain, peer_matrix_domain, peer_curator_matrix_id, message },
    AcceptFederationInvite { invitation_id },
    RejectFederationInvite { invitation_id, reason },
    PauseFederationLink { peer_replica, reason },
    ResumeFederationLink { peer_replica },
    RevokeFederationMember { peer_replica, reason },
    LeaveFederation { reason },
    DissolveFederation { reason },
}
```

---

## 5. Regulation Observability

### 5.1 Federation Regulation Spans (Phased)

| Phase | Regulation Span Variants |
|-------|------------------|
| **Phase 1 (MVP)** | `FederationCrdtMerge`, `FederationLinkEstablished`, `FederationLinkLost`, `FederationLinkDegraded`, `FederationMemberLeft` |
| **Phase 2** | `FederationInviteSent`, `FederationInviteReceived`, `FederationInviteAccepted`, `FederationInviteRejected`, `FederationInviteExpired`, `FederationLinkPaused`, `FederationLinkResumed` |
| **Phase 3** | `FederationMemberRevoked`, `FederationDissolved`, `FederationRegistrySync`, `FederationArtifactSync`, `FederationConduitRoute`, `FederationConduitRouteLost`, `FederationCrdtConflict` |

### 5.2 Federation Algedonic Thresholds (PC-1 Resolution)

| Threshold | Metric | Warning | Critical | Regulation Span |
|-----------|--------|---------|----------|----------|
| `fed_sync_latency` | CRDT sync round-trip (ms) | > 5000 | > 30000 | `FederationCrdtMerge` |
| `fed_crdt_divergence` | Delta size / baseline | > 2× | > 10× | `FederationCrdtMerge` |
| `fed_link_downtime` | Paused/Degraded duration (s) | > 3600 | > 86400 | `FederationLinkDegraded` |
| `fed_member_count_change` | Member count delta | ±1 | ±N/2 | `FederationMemberJoined/Left` |
| `fed_invitation_rate` | Invites/hour | > 5 | > 20 | `FederationInviteReceived` |
| `fed_registry_divergence` | Registry entries differed | > 10 | > 100 | `FederationRegistrySync` |

### 5.3 Federation Health Model (PC-4 Resolution)

The Curator's metacognition maintains a model of healthy federation:

```rust
pub struct FederationHealthModel {
    latency_window: Vec<u64>,
    expected_merge_frequency: f64,
    expected_member_count: usize,
    confidence: Confidence,
    last_updated: DateTime<Utc>,
}
```

---

## 6. Crate Architecture

### 6.1 Dependency Direction

```
CLI/API/MCP → hkask-services-core → hkask-pods → (federation: DEPRECATED, removed)
                                   ↓                  ↓
                              hkask-pods     hkask-types (traits)
                                   ↓                  ↓
                              hkask-regulation         hkask-types
```

### 6.2 Federation Crate Structure (Consolidated — DM-1 Resolution) — DEPRECATED, removed

| Module | Contents | Public Items (est.) |
|--------|----------|---------------------|
| `crdt` | `VersionVector`, `Dot`, `ORSet<T>`, `LWWMap<K,V>`, `GSet<T>` | ~20 |
| `sync` | `FederationSync` (sync loop, CRDT management), `FederationLinkManager` (invite, accept, pause, resume, revoke, leave), `LinkState`, `FederationLink` | ~14 |
| `registry` | `FederationRegistry` (merged user/agent resolution) | ~4 |

Total: 3 modules, ~38 public items. Link lifecycle merged into `sync` (was separate `link` module). Conduit merged into `sync` (was separate `conduit` module).

### 6.3 Hexagonal Ports (IA-1 Resolution)

`FederationSync` depends on trait abstractions, not concrete types:

```rust
// In hkask-types:
pub trait FederationSyncPort: Send + Sync {
    fn query_public_since(&self, cursor: u64, limit: usize) -> Result<Vec<hMem>, FederationSyncError>;
    fn insert_federated(&self, triple: &hMem, source: ReplicaId) -> Result<(), FederationSyncError>;
    fn cursor_for(&self, source: ReplicaId) -> u64;
    fn advance_cursor(&mut self, source: ReplicaId, cursor: u64);
}

pub trait FederationRegistryPort: Send + Sync {
    :FederationRegistryPort: Send + Sync {
        fn resolve_user(&self, webid: &WebID) -> Option<UserPod>;
        fn resolve_agent(&self, webid: &WebID) -> Option<AgentInfo>;
        fn list_local_users(&self) -> Vec<UserPod>;
        fn list_local_agents(&self) -> Vec<AgentInfo>;
    }

pub trait FederationTransport: Send + Sync {
    async fn send(&self, peer: ReplicaId, message: FederationMessage) -> Result<(), TransportError>;
    async fn recv(&self) -> Result<FederationMessage, TransportError>;
    fn simulate_partition(&self, peer: ReplicaId);     // test only
    fn heal_partition(&self, peer: ReplicaId);           // test only
}
```

Adapters:
- **Production:** `MatrixFederationTransport` (Matrix SDK)
- **Test:** `InMemoryFederationTransport` (unit tests without running Matrix)
- **Chaos:** `ChaosFederationTransport` (latency injection, partition simulation)

### 6.4 FederationSync (Split — DM-3 Resolution)

```rust
/// Manages the background CRDT sync loop and materialization.
pub struct FederationSync { /* 3 public methods: run(), status(), health() */ }

/// Manages link lifecycle: invite, accept, pause, resume, revoke, leave.
pub struct FederationLinkManager { /* 6 public methods */ }
```

---

## 7. Security Properties

| Property | Mechanism | Principle | Constraint Force |
|----------|-----------|-----------|-----------------|
| Episodic memory never crosses boundary | `EpisodicMemory::store()` rejects `Visibility::Public` | P1, P11.1 | Prohibition |
| CRDT reads only from public memory | `FederationSyncPort` trait — only exposes `SemanticMemory` queries | P1 | Guardrail (PS-2 fix) |
| Invitations require consent | `InvitationPolicy` default: `DeferToAdmin` | P2 | Prohibition |
| Pause is unilateral | Either side can pause without peer consent | Defensive security | Guardrail |
| Revocation is unilateral | Any member can revoke any other | Enforcement | Guardrail |
| All operations OCAP-gated | `OcapTokenKind::Federation` | P4 | Prohibition |
| Skill registries never shared | Each server's `SqliteRegistry` is local | P5.1, P3 | Prohibition |
| ν-event primacy preserved | CRDT merged state is derived — rebuildable from ν-events | P8 | Guardrail |
| Regulation observes everything | 18 federation Regulation spans + 6 algedonic thresholds | P9 | Guardrail |
| No second-class hMems | All hMems use same insert path; provenance in `access.perspective` | P3 | Prohibition |

---

## 8. Behavioral Contracts (CG-1 Resolution)

```rust
/// Start the federation sync loop.
///
/// expect: "Federated Curators converge on public memory"
/// [P3] Motivating: Generative Space — cross-server knowledge sharing
/// [P9] Constraining: Homeostatic Self-Regulation — Regulation-observed sync
/// pre:  CRDT state initialized with local SemanticIndex data.
/// pre:  At least one peer configured and Linked.
/// post: On each sync interval, local deltas sent to all Linked peers.
/// post: Received deltas merged into local CRDT state via OR-Set merge.
/// post: Regulation span `FederationCrdtMerge` emitted for each successful merge.
/// post: Regulation span `FederationLinkDegraded` emitted for consecutive failures > threshold.
/// test: InMemoryFederationTransport, two replicas, insert hMem on A,
///       verify hMem appears on B within sync_interval * 2.
/// test: Simulate partition, verify Degraded state, heal partition, verify recovery.
pub async fn run(&self, cancel: watch::Receiver<bool>) { ... }
```

---

## 9. Federation Configuration

```yaml
# CuratorPod's agent.yaml — federation section
federation:
  enabled: true
  replica_id: "alpha"
  server_domain: "a.example.com"
  matrix_domain: "matrix.a.example.com"

  invitations:
    policy: "manual"         # manual | auto_accept_configured | deny_all
    ttl_hours: 24

  peers:
    - replica_id: "beta"
      server_domain: "b.example.com"
      matrix_domain: "matrix.b.example.com"
      curator_matrix_id: "@curator:b.example.com"
      auto_accept: false

  sync:
    interval_secs: 5
    max_delta_size: 10000

  thresholds:                # Federation algedonic (PC-1)
    sync_latency_warning_ms: 5000
    sync_latency_critical_ms: 30000
    crdt_divergence_warning_factor: 2.0
    link_downtime_warning_secs: 3600
    link_downtime_critical_secs: 86400
    max_pause_duration_hours: 24
    invitation_rate_warning_per_hour: 5
    registry_divergence_warning: 10

  security:
    require_mutual_tls: true
    capability_required: "federation:sync"
    consent_required: true
```

---

## 10. CLI Commands

```bash
# Invitation
kask federation invite --peer beta --message "Let's federate!"
kask federation accept --invitation <uuid>
kask federation reject --invitation <uuid> --reason "..."

# Status
kask federation status [--peer <id>]
kask federation invitations

# Pause/Resume (security measure)
kask federation pause --peer beta --reason "Investigating anomaly"
kask federation resume --peer beta

# Revocation
kask federation revoke --peer beta --reason "Security breach"
kask federation leave --reason "Decommissioned"
kask federation dissolve --reason "Project concluded"

# Regulation
kask regulation federation health
kask regulation federation links
kask regulation federation thresholds
```

---

## 11. Implementation Phases

### Phase 1 — Core Sync (MVP)
- [ ] ~~federation crate~~ (DEPRECATED — removed) with `crdt` module (OR-Set with EAV hashing, LWW-Map, G-Set)
- [ ] `FederationTransport` trait + `InMemoryFederationTransport` test adapter
- [ ] `FederationSyncPort` + `FederationRegistryPort` traits in `hkask-types`
- [ ] `FederationSync` (run loop, CRDT merge, Regulation emission)
- [ ] 5 Regulation spans (`CrdtMerge`, `LinkEstablished`, `LinkLost`, `LinkDegraded`, `MemberLeft`)
- [ ] `Degraded` state and sync timeout detection
- [ ] Property-based tests for CRDT commutativity, associativity, idempotence
- [ ] Integration test: two replicas, insert → converge

### Phase 2 — Invitation and Lifecycle
- [ ] `FederationLinkManager` (invite, accept, pause, resume, revoke, leave)
- [ ] `InvitationPolicy` trait + `ManualInvitationPolicy`
- [ ] Matrix transport for invitation messages
- [ ] 6 invitation Regulation spans
- [ ] Pause protocol with notification
- [ ] Invitation expiry

### Phase 3 — Federation Completeness
- [ ] Federation algedonic thresholds (SetPoints integration)
- [ ] `FederationHealthModel` for Curator metacognition
- [ ] Registry merge (LWW-Map for users, G-Set for agents)
- [ ] Artifact sync (OR-Set with content hash)
- [ ] 7 remaining Regulation spans
- [ ] `MatrixFederationTransport` (production adapter)
- [ ] `ChaosFederationTransport` (robustness testing)
- [ ] `OcapTokenKind::Federation` in `hkask-types`
- [ ] Multi-server integration test harness

---

## 12. Design Decision Record

| Decision | Rationale | Addendum |
|----------|-----------|----------|
| OR-Set keyed by EAV hash, not LWW timestamps | ν-event primacy; same fact → same key → automatic convergence | PS-1 (E) |
| `Degraded` state added to LinkState | Broken feedback closure during partition; distinguishes pause from failure | PC-2 (D) |
| Federation algedonic thresholds | Curator needs comparators, not just sensors: "is this normal?" | PC-1 (D) |
| `FederationSyncPort` + `FederationRegistryPort` traits | Hexagonal ports; enables mock-based testing; type-level public-memory guard | IA-1, PS-2 (D) |
| `FederationTransport` trait + test adapter | Unit-testable without running Matrix; chaos testing | IA-4 (D) |
| `InvitationPolicy` trait + P2 default | Seam for custom acceptance policies; default deny | IA-2 (D) |
| 5 modules consolidated to 3 | Depth score; 7-function rule; link+conduit merged into sync | DM-1, DM-3 (D) |
| 18 Regulation spans → phased delivery | Simplicity first; 5 for MVP, 6 for Phase 2, 7 for Phase 3 | CG-2 (D) |
| Curator types canonical in `hkask-types` | Authority DAG; Curator (Loop 5) types not owned by Regulation (Loop 6) | M1, M2 (A) |
| `LoopId` has 6 variants | `Episodic`, `Semantic`, `Snapshot` now typed; VSM grounding | M3 (A) |
| LWW reserved for user profiles only | Metadata, not ν-event-grounded; Guideline constraint force | PS-3 (D) |

---

## 13. References

| Document | Relevance |
|----------|-----------|
| `PRINCIPLES.md` | P1–P12 grounding for all security properties |
| `hKask-architecture-master.md` | Four essential patterns, pod architecture |
| `MDS.md` | Domain ontology, five MDS categories |
| `FUNCTIONAL_SPECIFICATION.md` | Contract anchoring (§5), Regulation span registry (§9.1) |
| `crates/hkask-types/src/curator.rs` | `CuratorHandle`, `CuratorDirective`, `CurationThresholdConfig` |
| `crates/hkask-types/src/curation.rs` | `OcapTokenKind` (extend with `Federation`) |
| `crates/hkask-types/src/regulation.rs` | `RegulationSpan` (extend with 18 federation spans) |
| `crates/hkask-types/src/loops/mod.rs` | `LoopId` (6 variants) |
| `crates/hkask-pods/src/curator/semantic_index.rs` | `SemanticIndex` — model for federation index |
| `crates/hkask-pods/src/curator/semantic_sync.rs` | `CuratorSync` — model for `FederationSync` polling loop |
| `crates/hkask-memory/src/recall_dedup.rs` | `eav_hash()` — EAV content hashing used as CRDT key |
| `crates/hkask-types/src/lib.rs` | Hexagonal port traits (extend with federation ports) |
| `ADDENDUM_MISALIGNMENTS.md` | M1–M8 type/crate fixes |
| `ADDENDUM_REAUDIT.md` | M9–M15 additional findings |
| `ADDENDUM_PROTOCOL.md` | Link lifecycle, invitation/revocation protocol |
| `ADDENDUM_GAPS.md` | Multi-skill gap analysis (16 gaps) |
| `ADDENDUM_PS1_RESOLUTION.md` | ν-event primacy and canonical time resolution |

---

## Inlined Diagrams

The following Mermaid diagrams were inlined from the former `docs/diagrams/` directory per DOCUMENTATION_STANDARDS §1.

### Storage Schema ERD — hKask v0.31.0

*Inlined from `docs/diagrams/erd-schema.md`*


# Storage Schema ERD

Plain-English description: This ERD models the full SQLite schema used by `hkask-storage` (and co-located schema init in `hkask-wallet`, `hkask-pods`). The diagram covers 37 tables organized into six logical clusters: **Identity/Users** (human_users, userpod_identities, sessions, invites), **Goals** (goals, criteria, artifacts), **Wallet** (balances, transactions, API keys, encumbrances, deposits), **Gallery** (galleries, images, tags, face registry), **Monitoring/Regulation** (reg_records, reg_alerts, reg_variety_checkpoint, audit_log, escalations), and **Knowledge** (triples, embeddings). Four governance tables (consent_records, sovereignty_boundaries, quarantined_goals, loop_cursors) and five meta/infra tables (specs, spec_curation_records, kata_history, pod_meta) are shown as standalone entities. All FK relationships use Crow's Foot notation (`||--o{` for mandatory-one to optional-many, `||--||` for mandatory one-to-one).

```mermaid
erDiagram
    human_users {
        TEXT user_id PK
        BLOB email_enc
        BLOB phone_enc
        TEXT passphrase_hash
        TEXT salt
        TEXT master_salt
        INTEGER created_at
        INTEGER last_active
        INTEGER passphrase_set_at
        TEXT role
        TEXT oauth_provider
        TEXT oauth_provider_user_id
        TEXT oauth_display_name
    }

    userpod_identities {
        TEXT userpod_name PK
        TEXT user_id FK
        TEXT userpod_webid UK
        TEXT wallet_id
        BLOB first_name_enc
        BLOB last_name_enc
        TEXT persona_yaml
        INTEGER is_primary
        INTEGER created_at
        INTEGER last_login
    }

    user_sessions {
        TEXT session_id PK
        TEXT userpod_name FK
        TEXT userpod_webid
        TEXT user_id FK
        TEXT session_key_salt
        INTEGER expires_at
        INTEGER last_active
    }

    invites {
        TEXT invite_id PK
        TEXT created_by FK
        TEXT code UK
        TEXT status
        INTEGER created_at
        INTEGER expires_at
        INTEGER accepted_at
        TEXT accepted_user_id FK
    }

    goals {
        TEXT id PK
        TEXT webid
        TEXT text
        TEXT state
        TEXT visibility
        TEXT created_at
        TEXT completed_at
        TEXT parent_goal_id
        INTEGER depth
        TEXT display_name
    }

    goal_criteria {
        TEXT id PK
        TEXT goal_id FK
        TEXT type
        TEXT description
        INTEGER satisfied
    }

    goal_artifacts {
        TEXT id PK
        TEXT goal_id FK
        TEXT artifact_ref
        TEXT artifact_type
        TEXT created_at
    }

    wallet_balances {
        TEXT wallet_id PK
        INTEGER balance_rj
        INTEGER usdc_equivalent_micro
        TEXT created_at
        TEXT updated_at
    }

    wallet_transactions {
        INTEGER id PK
        TEXT wallet_id FK
        TEXT tx_type
        TEXT tx_subtype
        TEXT chain
        TEXT on_chain_tx_hash
        INTEGER amount_rj
        INTEGER balance_after_rj
        TEXT key_id
        TEXT tool_name
        INTEGER gas_units
        TEXT created_at
    }

    api_keys {
        TEXT key_id PK
        TEXT wallet_id FK
        BLOB public_key
        INTEGER spending_limit_rj
        INTEGER spent_rj
        TEXT scope
        TEXT purpose
        TEXT rate_limit_json
        TEXT privacy_mode
        TEXT preferred_chain
        TEXT expires_at
        TEXT issued_at
        TEXT revoked_at
        TEXT created_at
    }

    encumbrances {
        TEXT key_id PK
        TEXT wallet_id FK
        INTEGER amount_rj
        INTEGER consumed_rj
        TEXT status
        TEXT created_at
        TEXT released_at
    }

    deposit_addresses {
        TEXT wallet_id PK
        TEXT chain PK
        INTEGER derivation_index PK
        TEXT address
        TEXT privacy_mode
        TEXT created_at
    }

    deposit_references {
        TEXT reference PK
        TEXT wallet_id FK
        TEXT chain
        TEXT expires_at
        INTEGER spent
        TEXT created_at
    }

    galleries {
        TEXT id PK
        TEXT root_path UK
        TEXT mode
        INTEGER image_count
        INTEGER total_size_bytes
        TEXT created_at
        TEXT updated_at
    }

    gallery_images {
        TEXT id PK
        TEXT gallery_id FK
        TEXT relative_path
        TEXT absolute_path
        TEXT hash
        INTEGER width
        INTEGER height
        TEXT format
        INTEGER size_bytes
        TEXT added_at
    }

    gallery_tags {
        TEXT id PK
        TEXT image_id FK
        TEXT tag_type
        TEXT value
        REAL confidence
        TEXT model_used
        TEXT created_at
    }

    face_registry {
        TEXT id PK
        TEXT first_name
        TEXT last_name
        TEXT image_id FK
        BLOB embedding
        TEXT status
        TEXT notes
        TEXT created_at
        TEXT updated_at
    }

    triples {
        TEXT id PK
        TEXT entity
        TEXT attribute
        TEXT value
        TEXT valid_from
        TEXT valid_to
        TEXT recalled_at
        TEXT transaction_at
        REAL confidence
        TEXT perspective
        TEXT visibility
        TEXT owner_webid
    }

    embeddings {
        TEXT id PK
        TEXT entity_ref
        BLOB vector
        INTEGER dimensions
        TEXT model
        TEXT created_at
    }

    reg_records {
        TEXT id PK
        TEXT timestamp
        TEXT observer_webid
        TEXT span_category
        TEXT span_path
        TEXT phase
        TEXT observation
        TEXT regulation
        TEXT outcome
        INTEGER recursion_depth
        TEXT parent_event
        TEXT visibility
    }

    reg_variety_checkpoint {
        TEXT domain PK
        INTEGER variety_count
        TEXT last_updated
        INTEGER threshold
    }

    reg_alerts {
        TEXT id PK
        TEXT timestamp
        TEXT alert_type
        TEXT severity
        TEXT domain
        TEXT message
        INTEGER resolved
        TEXT resolved_at
    }

    audit_log {
        TEXT id PK
        TEXT timestamp
        TEXT actor_webid
        TEXT action
        TEXT resource
        TEXT outcome
        TEXT details
        TEXT ip_address
        TEXT created_at
    }

    consent_records {
        TEXT id PK
        TEXT webid UK
        TEXT granted_categories
        INTEGER granted_at
        INTEGER revoked_at
        INTEGER active
    }

    sovereignty_boundaries {
        TEXT id PK
        TEXT webid UK
        TEXT sovereign_categories
        TEXT shared_categories
        TEXT public_categories
        TEXT requires_affirmative_consent
        INTEGER created_at
        INTEGER updated_at
    }

    quarantined_goals {
        TEXT id PK
        TEXT original_data
        TEXT quarantine_reason
        TEXT quarantined_at
        INTEGER repair_attempts
        INTEGER repaired
    }

    specs {
        TEXT id PK
        TEXT name
        TEXT category
        TEXT domain_anchor
        TEXT signed_by
        TEXT signature
        TEXT created_at
        TEXT valid_from
        TEXT valid_to
        TEXT data
    }

    spec_curation_records {
        TEXT spec_id
        TEXT decision
        TEXT rationale
        REAL coherence_score
        TEXT ocap_boundary
        TEXT curated_at
        TEXT recorded_at
    }

    loop_cursors {
        TEXT key PK
        INTEGER value
        TEXT updated_at
    }

    pod_meta {
        TEXT key PK
        TEXT value
    }

    kata_history {
        INTEGER id PK
        TEXT agent_name
        TEXT date
        TEXT kata_type
        TEXT practice_name
        INTEGER steps_completed
        INTEGER gas_consumed
        TEXT created_at
    }

    escalations {
        TEXT id PK
        TEXT template_id
        TEXT bot_id
        TEXT output
        REAL confidence
        INTEGER retry_count
        TEXT error_context
        TEXT created_at
        TEXT status
        TEXT resolved_at
        TEXT resolved_by
    }

    backup_meta {
        TEXT webid
        TEXT source_server_url
        TEXT exported_at
        INTEGER triple_count
        INTEGER schema_version
    }

    %% ── Identity/Users relationships ──
    human_users ||--o{ userpod_identities : "user_id FK"
    human_users ||--o{ user_sessions : "user_id FK"
    userpod_identities ||--o{ user_sessions : "userpod_name FK"
    human_users ||--o{ invites : "created_by FK"
    human_users ||--o| invites : "accepted_user_id FK"

    %% ── Goals relationships ──
    goals ||--o{ goal_criteria : "goal_id FK"
    goals ||--o{ goal_artifacts : "goal_id FK"

    %% ── Wallet relationships ──
    wallet_balances ||--o{ wallet_transactions : "wallet_id FK"
    wallet_balances ||--o{ api_keys : "wallet_id FK"
    wallet_balances ||--o{ deposit_references : "wallet_id FK"
    wallet_balances ||--o{ encumbrances : "wallet_id FK"
    api_keys ||--|| encumbrances : "key_id FK"

    %% ── Gallery relationships ──
    galleries ||--o{ gallery_images : "gallery_id FK"
    gallery_images ||--o{ gallery_tags : "image_id FK"
    gallery_images ||--o{ face_registry : "image_id FK"
```
<!-- DIAGRAM_ALIGNMENT
  id: DIAG-PL-010
  verified_date: 2026-06-30
  verified_against: crates/hkask-storage/src/
  status: VERIFIED
-->

## Notable Indexes

| Table | Index Name | Columns | Notes |
|-------|-----------|---------|-------|
| `userpod_identities` | `idx_userpod_identities_user` | `user_id` | Lookup by human user |
| `userpod_identities` | `idx_userpod_identities_webid` | `userpod_webid` | Lookup by WebID |
| `user_sessions` | `idx_user_sessions_user` | `user_id` | Session lookup by user |
| `user_sessions` | `idx_user_sessions_userpod` | `userpod_name` | Session lookup by userpod |
| `user_sessions` | `idx_user_sessions_expiry` | `expires_at` | Expired session cleanup |
| `invites` | `idx_invites_code` | `code` | Invite code lookup |
| `invites` | `idx_invites_created_by` | `created_by` | Invites by creator |
| `contacts` | `idx_contacts_agent` | `agent_name` | Agent contact lookup |
| `scheduled_tasks` | `idx_scheduled_agent` | `agent_name` | Agent task lookup |
| `embeddings` | `idx_embeddings_entity_ref` | `entity_ref` | Embedding lookup by entity |
| `reg_records` | `idx_reg_records_timestamp_category` | `timestamp, span_category` | Regulation event range scan |
| `reg_records` | `idx_reg_records_category_phase` | `span_category, phase` | Regulation phase filtering |
| `audit_log` | `idx_audit_log_timestamp` | `timestamp` | Audit time-range scan |
| `audit_log` | `idx_audit_log_actor` | `actor_webid` | Audit by actor |
| `consent_records` | `idx_consent_active` | `active` | Active consent lookup |
| `sovereignty_boundaries` | `idx_sovereignty_webid` | `webid` | Sovereignty by WebID |
| `sovereignty_boundaries` | `idx_sovereignty_updated` | `updated_at` | Sovereignty recency scan |
| `wallet_transactions` | `idx_wallet_tx_wallet_id` | `wallet_id` | Transactions by wallet |
| `wallet_transactions` | `idx_wallet_tx_created_at` | `created_at` | Transaction time-range |
| `api_keys` | `idx_api_keys_wallet_id` | `wallet_id` | Keys by wallet |
| `api_keys` | `idx_api_keys_public_key` | `public_key` | Key lookup by pubkey |
| `deposit_addresses` | `deposit_addresses_unique_address` | `chain, privacy_mode, address` | Unique deposit address (UNIQUE constraint) |
| `deposit_references` | `idx_deposit_refs_wallet_id` | `wallet_id` | Deposit refs by wallet |
| `deposit_references` | `idx_deposit_refs_expires` | `expires_at` | Expired deposit cleanup |
| `encumbrances` | `idx_encumbrances_wallet_id` | `wallet_id` | Encumbrances by wallet |
| `gallery_images` | `idx_gallery_images_gallery` | `gallery_id` | Images by gallery |
| `gallery_images` | `idx_gallery_images_hash` | `hash` | Image hash dedup |
| `gallery_tags` | `idx_gallery_tags_image` | `image_id` | Tags by image |
| `gallery_tags` | `idx_gallery_tags_type` | `tag_type` | Tags by type |
| `gallery_tags` | `idx_gallery_tags_unique` | `image_id, tag_type, value` | Unique tag per image (UNIQUE) |
| `face_registry` | `idx_face_registry_status` | `status` | Faces by status |
| `kata_history` | `idx_kata_history_agent` | `agent_name` | Kata by agent |
| `kata_history` | `idx_kata_history_date` | `date` | Kata by date |
| `kata_history` | `idx_kata_history_type` | `kata_type` | Kata by type |

## Cross-Reference

This diagram models the storage layer for all [MDS Core Entities](MDS.md#11-core-entities):
- **`HumanUser`** → `human_users` table
- **`UserPod`** → `userpod_identities` table
- **`Wallet`** → `wallet_balances`, `wallet_transactions`, `encumbrances`, `deposit_addresses`, `deposit_references`
- **`ApiKey`** → `api_keys` table
- **`hMem`** → `triples` table
- **`RegulationLedger`** → `reg_records`, `reg_variety_checkpoint`, `reg_alerts` tables
- **`GasBudget`** → `loop_cursors` table (cursor-based gas tracking)

All FK relationships align with the ownership chains defined in [PRINCIPLES.md](PRINCIPLES.md) P1 (User Sovereignty) and P9 (Economic Layer). The `webid` columns in `triples`, `goals`, `consent_records`, `sovereignty_boundaries`, and `reg_records` implement the multi-tenant data isolation required by P1 and P4 (Clear Boundaries).


### SQLCipher Schema — ERD

*Inlined from `docs/diagrams/erd-sqlcipher-schema.md`*


```mermaid
erDiagram
    human_users {
        TEXT user_id PK
        BLOB email_enc
        BLOB phone_enc
        TEXT passphrase_hash
        TEXT salt
        TEXT master_salt
        INTEGER created_at
        INTEGER last_active
        INTEGER passphrase_set_at
        TEXT role
        TEXT oauth_provider
        TEXT oauth_provider_user_id
        TEXT oauth_display_name
    }

    userpod_identities {
        TEXT userpod_name PK
        TEXT user_id FK
        TEXT userpod_webid UK
        TEXT wallet_id
        BLOB first_name_enc
        BLOB last_name_enc
        TEXT persona_yaml
        INTEGER is_primary
        INTEGER created_at
        INTEGER last_login
    }

    user_sessions {
        TEXT session_id PK
        TEXT userpod_name FK
        TEXT userpod_webid
        TEXT user_id FK
        TEXT session_key_salt
        INTEGER expires_at
        INTEGER last_active
    }

    invites {
        TEXT invite_id PK
        TEXT created_by FK
        TEXT code UK
        TEXT status
        INTEGER created_at
        INTEGER expires_at
        INTEGER accepted_at
        TEXT accepted_user_id FK
    }

    reg_records {
        TEXT id PK
        TEXT timestamp
        TEXT observer_webid
        TEXT span_category
        TEXT span_path
        TEXT phase
        TEXT observation
        TEXT regulation
        TEXT outcome
        INTEGER recursion_depth
        TEXT parent_event
        TEXT visibility
    }

    hmems {
        TEXT id PK
        TEXT entity
        TEXT attribute
        TEXT value
        TEXT valid_from
        TEXT valid_to
        TEXT recalled_at
        TEXT transaction_at
        REAL confidence
        TEXT perspective
        TEXT visibility
        TEXT owner_webid
        TEXT dimension
    }

    embeddings {
        TEXT id PK
        TEXT entity_ref FK
        BLOB vector
        INTEGER dimensions
        TEXT model
        TEXT created_at
    }

    vec_embeddings {
        TEXT id PK
        REAL_ARRAY embedding
    }

    goals {
        TEXT id PK
        TEXT webid
        TEXT text
        TEXT state
        TEXT visibility
        TEXT created_at
        TEXT completed_at
        TEXT parent_goal_id
        INTEGER depth
        TEXT display_name
    }

    goal_criteria {
        TEXT id PK
        TEXT goal_id FK
        TEXT type
        TEXT description
        INTEGER satisfied
    }

    goal_artifacts {
        TEXT id PK
        TEXT goal_id FK
        TEXT artifact_ref
        TEXT artifact_type
        TEXT created_at
    }

    quarantined_goals {
        TEXT id PK
        TEXT original_data
        TEXT quarantine_reason
        TEXT quarantined_at
        INTEGER repair_attempts
        INTEGER repaired
    }

    consent_records {
        TEXT id PK
        TEXT webid UK
        TEXT granted_categories
        INTEGER granted_at
        INTEGER revoked_at
        INTEGER active
    }

    wallet_balances {
        TEXT wallet_id PK
        INTEGER balance_rj
        INTEGER usdc_equivalent_micro
        TEXT created_at
        TEXT updated_at
    }

    wallet_transactions {
        INTEGER id PK
        TEXT wallet_id FK
        TEXT tx_type
        TEXT tx_subtype
        TEXT chain
        TEXT on_chain_tx_hash
        INTEGER amount_rj
        INTEGER balance_after_rj
        TEXT key_id
        TEXT tool_name
        INTEGER gas_units
        TEXT created_at
    }

    api_keys {
        TEXT key_id PK
        TEXT wallet_id FK
        BLOB public_key
        INTEGER spending_limit_rj
        INTEGER spent_rj
        TEXT scope
        TEXT purpose
        TEXT rate_limit_json
        TEXT privacy_mode
        TEXT preferred_chain
        TEXT expires_at
        TEXT issued_at
        TEXT revoked_at
        TEXT created_at
    }

    encumbrances {
        TEXT key_id PK_FK
        TEXT wallet_id FK
        INTEGER amount_rj
        INTEGER consumed_rj
        TEXT status
        TEXT created_at
        TEXT released_at
    }

    deposit_addresses {
        TEXT wallet_id PK_FK
        TEXT chain PK
        TEXT address UK
        INTEGER derivation_index PK
        TEXT privacy_mode
        TEXT created_at
    }

    deposit_references {
        TEXT reference PK
        TEXT wallet_id FK
        TEXT chain
        TEXT expires_at
        INTEGER spent
        TEXT created_at
    }

    audit_log {
        TEXT id PK
        TEXT timestamp
        TEXT actor_webid
        TEXT action
        TEXT resource
        TEXT outcome
        TEXT details
        TEXT ip_address
        TEXT created_at
    }

    reg_variety_checkpoint {
        TEXT domain PK
        INTEGER variety_count
        TEXT last_updated
        INTEGER threshold
    }

    reg_alerts {
        TEXT id PK
        TEXT timestamp
        TEXT alert_type
        TEXT severity
        TEXT domain
        TEXT message
        INTEGER resolved
        TEXT resolved_at
    }

    loop_cursors {
        TEXT key PK
        INTEGER value
        TEXT updated_at
    }

    kata_history {
        INTEGER id PK
        TEXT agent_name
        TEXT date
        TEXT kata_type
        TEXT practice_name
        INTEGER steps_completed
        INTEGER gas_consumed
        TEXT created_at
    }

    pod_meta {
        TEXT key PK
        TEXT value
    }

    human_users ||--o{ userpod_identities : "user_id"
    human_users ||--o{ user_sessions : "user_id"
    human_users ||--o{ invites : "created_by / accepted_user_id"
    userpod_identities ||--o{ user_sessions : "userpod_name"
    goals ||--o{ goal_criteria : "goal_id"
    goals ||--o{ goal_artifacts : "goal_id"
    wallet_balances ||--o{ wallet_transactions : "wallet_id"
    wallet_balances ||--o{ api_keys : "wallet_id"
    wallet_balances ||--o{ deposit_addresses : "wallet_id"
    wallet_balances ||--o{ deposit_references : "wallet_id"
    api_keys ||--|| encumbrances : "key_id"
    encumbrances }o--|| wallet_balances : "wallet_id"
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-017
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

## Node-to-Code Mapping

| Table | Crate | Source File |
|-------|-------|-------------|
| `hmems` | `hkask-storage` | `src/sql/schema.sql` |
| `embeddings` | `hkask-storage` | `src/sql/schema.sql` |
| `vec_embeddings` | `hkask-storage` | `src/sql/schema.sql` |
| `reg_records` | `hkask-storage` | `src/sql/schema.sql` |
| `audit_log` | `hkask-storage` | `src/sql/schema.sql` |
| `reg_variety_checkpoint` | `hkask-storage` | `src/sql/schema.sql` |
| `reg_alerts` | `hkask-storage` | `src/sql/schema.sql` |
| `goals` | `hkask-storage` | `src/goals.rs` |
| `goal_criteria` | `hkask-storage` | `src/sql/schema.sql` |
| `goal_artifacts` | `hkask-storage` | `src/sql/schema.sql` |
| `consent_records` | `hkask-storage::consent_store` | `src/consent_store.rs` |
| `quarantined_goals` | `hkask-storage` | `src/goals.rs` |
| `loop_cursors` | `hkask-storage` | `src/sql/schema.sql` |
| `human_users` | `hkask-storage` | `src/sql/users.sql` |
| `userpod_identities` | `hkask-storage` | `src/sql/users.sql` |
| `user_sessions` | `hkask-storage` | `src/sql/users.sql` |
| `invites` | `hkask-storage` | `src/sql/users.sql` |
| `wallet_balances` | `hkask-storage` | `src/wallet/mod.rs` |
| `wallet_transactions` | `hkask-storage` | `src/wallet/mod.rs` |
| `api_keys` | `hkask-storage` | `src/wallet/mod.rs` |
| `encumbrances` | `hkask-storage` | `src/wallet/mod.rs` |
| `deposit_addresses` | `hkask-storage` | `src/wallet/mod.rs` |
| `deposit_references` | `hkask-storage` | `src/wallet/mod.rs` |
| `kata_history` | `hkask-storage::kata` | `src/kata.rs` |
| `pod_meta` | `hkask-storage` | `src/sql/schema.sql` |

### Relationships

| Relationship | Cardinality | On |
|-------------|-------------|-----|
| `human_users` → `userpod_identities` | 1:N | `user_id` |
| `human_users` → `user_sessions` | 1:N | `user_id` |
| `human_users` → `invites` | 1:N | `created_by`, `accepted_user_id` |
| `userpod_identities` → `user_sessions` | 1:N | `userpod_name` |
| `goals` → `goal_criteria` | 1:N | `goal_id` |
| `goals` → `goal_artifacts` | 1:N | `goal_id` |
| `wallet_balances` → `wallet_transactions` | 1:N | `wallet_id` |
| `wallet_balances` → `api_keys` | 1:N | `wallet_id` |
| `wallet_balances` → `deposit_addresses` | 1:N | `wallet_id` |
| `wallet_balances` → `deposit_references` | 1:N | `wallet_id` |
| `api_keys` → `encumbrances` | 1:1 | `key_id` |
| `encumbrances` → `wallet_balances` | N:1 | `wallet_id` |


### Multi-User Data Model — ERD

*Inlined from `docs/diagrams/erd-multi-user.md`*

# Multi-User Data Model (ERD)

Entity-relationship diagram for hKask's multi-user schema (`crates/hkask-storage/src/sql/users.sql`).

## Diagram

```mermaid
erDiagram
    human_users ||--o{ userpod_identities : "user_id"
    human_users ||--o{ user_sessions : "user_id"
    human_users ||--o{ invites : "created_by"
    human_users ||--o{ invites : "accepted_user_id"
    userpod_identities ||--o{ user_sessions : "userpod_name"

    human_users {
        TEXT user_id PK "UUID"
        BLOB email_enc "Encrypted email"
        BLOB phone_enc "Encrypted phone"
        TEXT passphrase_hash "Argon2id hash"
        TEXT salt "Password salt"
        TEXT master_salt "PII key derivation salt"
        INTEGER created_at "Unix timestamp"
        INTEGER last_active "Nullable"
        INTEGER passphrase_set_at "Last passphrase change"
        TEXT role "admin | member (default: member)"
        TEXT oauth_provider "Nullable: github | google"
        TEXT oauth_provider_user_id "Nullable: external ID"
        TEXT oauth_display_name "Nullable: display name from provider"
    }

    userpod_identities {
        TEXT userpod_name PK "Human-readable name"
        TEXT user_id FK "References human_users"
        TEXT userpod_webid UK "WebID URI"
        TEXT wallet_id "Nullable: linked wallet"
        BLOB first_name_enc "Encrypted first name"
        BLOB last_name_enc "Encrypted last name"
        TEXT persona_yaml "Nullable: persona definition"
        INTEGER is_primary "0 or 1"
        INTEGER created_at "Unix timestamp"
        INTEGER last_login "Nullable"
    }

    user_sessions {
        TEXT session_id PK "UUID"
        TEXT userpod_name FK "References userpod_identities"
        TEXT userpod_webid "WebID copy"
        TEXT user_id FK "References human_users"
        TEXT session_key_salt "Key derivation salt"
        INTEGER expires_at "Unix timestamp"
        INTEGER last_active "Unix timestamp"
    }

    invites {
        TEXT invite_id PK "UUID"
        TEXT created_by FK "Admin user_id"
        TEXT code UK "12-char invite code"
        TEXT status "pending | accepted"
        INTEGER created_at "Unix timestamp"
        INTEGER expires_at "7 days from creation"
        INTEGER accepted_at "Nullable: acceptance timestamp"
        TEXT accepted_user_id FK "Nullable: accepting user_id"
    }
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-018
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->

## Cardinality Notes

- **human_users → userpod_identities:** One-to-many. A human can own multiple userpods.
- **human_users → user_sessions:** One-to-many. A human can have multiple active sessions across userpods.
- **human_users → invites (created_by):** One-to-many. An admin can issue many invites.
- **human_users → invites (accepted_user_id):** One-to-many (nullable). A user can accept multiple invites (though normally one).
- **userpod_identities → user_sessions:** One-to-many. A userpod can have multiple sessions.

## Notable Indexes

| Index | Table | Columns | Purpose |
|-------|-------|---------|---------|
| `idx_userpod_identities_user` | userpod_identities | user_id | Lookup userpods by human |
| `idx_userpod_identities_webid` | userpod_identities | userpod_webid | Lookup by WebID |
| `idx_user_sessions_user` | user_sessions | user_id | Session listing by user |
| `idx_user_sessions_userpod` | user_sessions | userpod_name | Session listing by userpod |
| `idx_user_sessions_expiry` | user_sessions | expires_at | Expired session cleanup |
| `idx_invites_code` | invites | code | Invite lookup by code |
| `idx_invites_created_by` | invites | created_by | Admin's invite listing |

## Cross-References

- Functional spec: `docs/architecture/core/FUNCTIONAL_SPECIFICATION.md` §3.16
- Server config: `crates/hkask-types/src/server_config.rs`
- Invite flow: `docs/diagrams/flowchart-oauth-registration.md`
- Invite lifecycle: `docs/diagrams/state-invite-lifecycle.md`


### Database Driver Class Diagram

*Inlined from `docs/diagrams/class-database-driver.md`*


# Database Driver Class Diagram

Reference diagram showing the `DatabaseDriver` trait hierarchy, store relationships, and connection management types introduced in v0.31.

Related: [ADR-043](../ADRs/ADR-043-database-driver.md), [Database Providers](#database-providers-merged-from-database-providersmd)

```mermaid
classDiagram
    namespace Driver_Trait {
        class DatabaseDriver {
            <<interface>>
            +execute(sql, params) usize
            +execute_batch(sql)
            +query(sql, params) Vec~DbRow~
            +query_optional(sql, params) Option~DbRow~
            +provider() DbProvider
            +transaction() TransactionHandle
            +as_any() &dyn Any
        }
    }
    namespace SQLite_Impl {
        class SqliteDriver {
            -conn: Arc~Mutex~Connection~~
            +new(conn) SqliteDriver
        }
    }
    namespace Transaction {
        class TransactionHandle {
            -driver: &dyn DatabaseDriver
            -committed: bool
            +commit(self) Result
        }
    }
    namespace Connection_Infra {
        class ConnectionFactory {
            <<interface>>
            +open_primary() DbConnection
            +open_domain(config) DbConnection
            +provider() DbProvider
        }
        class DatabaseFactory {
            -db: Arc~Database~
            +new(db) DatabaseFactory
        }
        class DbConnection {
            -inner: Box~dyn Any~
            -provider: DbProvider
            +as_any() &dyn Any
            +provider() DbProvider
        }
    }
    namespace Stores {
        class EmbeddingStore {
            -backend: VectorBackend
            -driver: Arc~dyn DatabaseDriver~
            +from_driver(driver, dim) EmbeddingStore
            +store(ref, vec, model)
            +get(ref) StoredEmbedding
            +search(vec, k) Vec~SimilarityResult~
            +delete(ref)
            +count() usize
        }
        class VectorBackend {
            -conn: Arc~Mutex~Connection~~
            -dim: usize
        }
        class HMemStore {
            -conn: Arc~Mutex~Connection~~
            +from_driver(driver) HMemStore
            +insert(h_mem)
            +query_by_entity(entity) Vec~HMem~
            +update(h_mem)
        }
        class MemoryLoopForwarder {
            -episodic: Arc~EpisodicMemory~
            -semantic: Arc~SemanticMemory~
            +from_connection(conn) Result~Self~
        }
    }
    namespace Types {
        class DbProvider {
            <<enumeration>>
            Sqlite
            Postgres
        }
        class DbConfig {
            <<enumeration>>
            Sqlite { path, passphrase }
            Postgres { url }
        }
    }

    SqliteDriver ..|> DatabaseDriver : implements
    DatabaseFactory ..|> ConnectionFactory : implements
    TransactionHandle o--> DatabaseDriver : borrows &dyn
    EmbeddingStore o--> DatabaseDriver : holds Arc~dyn~
    EmbeddingStore o--> VectorBackend : holds
    HMemStore ..> DatabaseDriver : from_driver extracts conn
    MemoryLoopForwarder ..> HMemStore : creates via from_driver
    MemoryLoopForwarder ..> EmbeddingStore : creates via from_driver
    DatabaseFactory o--> DbConnection : produces
    DbConnection --> DbProvider : typed by
    DbConfig --> DbProvider : selects
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-019
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->


### Database Connection Lifecycle

*Inlined from `docs/diagrams/flowchart-connection-lifecycle.md`*


# Database Connection Lifecycle

Reference flowchart tracing the path from environment variable to constructed stores. Covers SQLite (stable) and PostgreSQL (planned v0.32) paths.

Related: [ADR-043](../ADRs/ADR-043-database-driver.md), [Class Diagram](#database-driver-class-diagram)

```mermaid
flowchart TD
    A([Start: kask init]) --> B[Read HKASK_DB_PROVIDER]
    B --> C{Provider?}
    C -->|sqlite| D[Read HKASK_DB_PATH<br/>+ HKASK_DB_PASSPHRASE]
    C -->|postgres| E{PostgresDriver<br/>implemented?}
    E -->|No| F[Error: not yet<br/>implemented]
    E -->|Yes| G[Parse connection URL]
    D --> H[parse_db_provider]
    G --> H
    H --> I[ServiceConfig::db_config]
    I --> J[Build DbConfig]
    J --> K{DbConfig variant?}
    K -->|Sqlite| L[open_or_repair<br/>verify SQLCipher database]
    K -->|Postgres| M[Connect via sqlx<br/>pgvector]
    L --> N[Database::conn_arc<br/>Arc Mutex Connection]
    M --> N
    N --> O[SqliteDriver::new<br/>Arc dyn DatabaseDriver]
    O --> P[HMemStore::from_driver]
    P --> Q[driver.as_any<br/>downcast_ref extract conn]
    Q --> R[Self::new raw_conn]
    O --> S[EmbeddingStore::from_driver]
    S --> T[driver.as_any<br/>downcast_ref extract conn]
    T --> U[VectorBackend conn + dim]
    R --> V[MemoryLoopForwarder<br/>from_connection]
    U --> V
    V --> W([Stores Ready])
    F --> X([Error Exit])
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-MASTER-020
verified_date: 2026-07-12
verified_against: crates/hkask-regulation/src/lib.rs, crates/hkask-mcp/src/lib.rs
status: VERIFIED
-->


### Companies MCP Provider Routing — Sequence

*Previously in `docs/diagrams/`; inlined per DOCUMENTATION_STANDARDS §6.*


# Companies MCP Provider Routing

This reference diagram shows the routing used by eligible financial-data tools. An exchange-qualified symbol prefers EODHD; a plain symbol prefers FMP. The in-memory learning state can select the alternate provider when the default is classified as flaky or stale. A failed primary request is retried through the alternate provider. `company_screener` and `research_search` are outside this path.

See [Companies MCP Server Reference](../../reference/mcp-servers/README.md#companies-mcp-server) for the tool-boundary details.

```mermaid
sequenceDiagram
    participant Client as MCP Client
    participant Tool as Financial Data Tool
    participant Learn as Learning State
    participant Route as Provider Router
    participant FMP as FMP
    participant EODHD as EODHD

    Client->>+Tool: symbol request
    Tool->>+Learn: clone routing state
    Learn-->>-Tool: provider observations
    Tool->>+Route: companies_get(symbol, state)
    Route->>Route: choose default by symbol shape

    opt Default provider is flaky or stale
        Route->>Route: choose alternate provider
    end

    alt EODHD selected first
        Route->>+EODHD: fetch financial data
        EODHD-->>-Route: response or failure
        opt Successful EODHD financial response
            Route->>Route: normalize to FMP-shaped JSON
        end
        opt EODHD failure
            Route->>+FMP: retry request
            FMP-->>-Route: response or failure
        end
    else FMP selected first
        Route->>+FMP: fetch financial data
        FMP-->>-Route: response or failure
        opt FMP failure
            Route->>Route: append .US suffix for plain symbols
            Route->>+EODHD: retry request (exchange-qualified)
            EODHD-->>-Route: response or failure
            opt Successful EODHD financial response
                Route->>Route: normalize to FMP-shaped JSON
            end
        end
    end

    Route-->>-Tool: JSON result or typed error
    Tool-->>-Client: MCP result
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-IC-010
verified_date: 2026-07-15
verified_against: mcp-servers/hkask-mcp-companies/src/providers.rs:106-197; mcp-servers/hkask-mcp-companies/src/lib.rs:271-315
status: VERIFIED
-->


### Companies MCP Forecast Feedback — Sequence

*Previously in `docs/diagrams/`; inlined per DOCUMENTATION_STANDARDS §6.*


# Companies MCP Forecast Feedback

This reference diagram shows the durable forecast loop. A DCF or calibrated forecast writes an owner-scoped structured JSON snapshot; an optional same-symbol revision references its parent. `forecast_record` reloads the snapshot, retrieves current financial data for decomposition, appends the outcome, and independently sends an experience to the daemon when configured.

See [Companies MCP Server Reference](../../reference/mcp-servers/README.md#companies-mcp-server) for request fields and ownership boundaries.

```mermaid
sequenceDiagram
    participant Client as MCP Client
    participant Forecast as Forecast Tool
    participant Store as Owner Forecast Store
    participant Provider as Financial Provider
    participant Daemon as Optional Daemon

    Client->>+Forecast: dcf_valuation or calibrate_forecast
    opt Revision requested
        Forecast->>+Store: get parent forecast
        Store-->>-Forecast: parent or not found
        Forecast->>Forecast: verify same symbol
    end
    Forecast->>Forecast: build structured snapshot
    Forecast->>+Store: save forecast snapshot
    Store-->>-Forecast: forecast_id
    Forecast-->>-Client: forecast_id and valuation

    Client->>+Forecast: forecast_record(forecast_id, outcome)
    Forecast->>+Store: get forecast snapshot
    Store-->>-Forecast: structured snapshot
    Forecast->>+Provider: fetch current financial data
    Provider-->>-Forecast: actual data or error
    opt Actual data available
        Forecast->>Forecast: decompose return gap
    end
    Forecast->>+Store: append outcome and decomposition
    Store-->>-Forecast: persisted
    opt Daemon configured
        Forecast->>+Daemon: store outcome experience
        Daemon-->>-Forecast: accepted or logged failure
    end
    Forecast-->>-Client: recorded outcome
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-IC-011
verified_date: 2026-07-10
verified_against: mcp-servers/hkask-mcp-companies/src/tools/analytics.rs:438-457; mcp-servers/hkask-mcp-companies/src/tools/valuation.rs:634-659,774-915; mcp-servers/hkask-mcp-companies/src/portfolio.rs:303-400
status: VERIFIED
-->


### Companies MCP Portfolio Schema — ERD

This reference diagram shows the owner-scoped SQLite schema managed by
`PortfolioManager`. Each authenticated MCP `WebID` gets a separate database file
at `~/.config/hkask/portfolios/<sanitized-webid>/master.db`; owner isolation is
at the file level, not the row level. The `forecasts` table is owner-scoped but
not portfolio-scoped — it has no foreign key to `portfolios`. The `revision_of`
column links same-symbol forecast revisions without enforcing referential
integrity (it is a nullable self-reference, not a declared FK).

See [Companies MCP Server Reference](../../reference/mcp-servers/README.md#companies-mcp-server) for the storage safety boundary, and the code review that identified this missing diagram.

```mermaid
erDiagram
    portfolios {
        TEXT name PK
        TEXT created_at
    }
    transactions {
        TEXT id PK
        TEXT portfolio_name FK
        TEXT date
        TEXT type
        REAL quantity
        REAL price
        REAL commission
        REAL amount
        TEXT currency
        TEXT notes
        TEXT created_at
    }
    price_cache {
        TEXT portfolio_name PK
        TEXT symbol PK
        TEXT date PK
        REAL close
        TEXT source
        TEXT fetched_at
    }
    security_links {
        TEXT portfolio_name PK
        TEXT ledger_symbol PK
        TEXT data_symbol
    }
    notes {
        TEXT id PK
        TEXT portfolio_name FK
        TEXT symbol
        TEXT date
        TEXT title
        TEXT body
        TEXT tags
        TEXT created_at
    }
    files {
        TEXT id PK
        TEXT portfolio_name FK
        TEXT symbol
        TEXT date
        TEXT filename
        TEXT mime_type
        INTEGER size
        TEXT path
        TEXT notes
        TEXT created_at
    }
    forecasts {
        TEXT id PK
        TEXT symbol
        TEXT revision_of
        TEXT snapshot
        TEXT outcomes
        TEXT created_at
    }

    portfolios ||--o{ transactions : "owns"
    portfolios ||--o{ price_cache : "caches"
    portfolios ||--o{ security_links : "maps"
    portfolios ||--o{ notes : "annotates"
    portfolios ||--o{ files : "stores"
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-IC-012
verified_date: 2026-07-15
verified_against: mcp-servers/hkask-mcp-companies/src/portfolio.rs:132-202
status: VERIFIED
-->


### Scenario Forecasting Pipeline



# Scenario Forecasting Pipeline

The scenarios MCP server accepts explicit research and event inputs, calculates the event-tree and calibration artifacts, and records resolved forecasts for later calibration. The diagram distinguishes the optional exploratory framing path from the computational path; `scenario_research` and `scenario_build` return scaffolds for an agent to review rather than silently collecting research or creating final events.[^mcp]

```mermaid
flowchart TD
    Start([Decision or forecast question])
    Frame[scenario_frame]
    FrameDoc[scenario_frame_document]
    Brainstorm[scenario_brainstorm]
    Research[Agent supplies research text]
    Scaffold[scenario_research or scenario_build]
    Events[Reviewed ScenarioEvent JSON]
    Quantify[scenario_quantify]
    Calibrate[scenario_calibrate]
    Update[scenario_update]
    Synthesize[scenario_synthesize]
    Score[scenario_score]
    Calibration[scenario_calibration]
    Assess[scenario_assess]
    End([Decision learning])

    Start --> Frame
    Frame --> FrameDoc
    FrameDoc --> Brainstorm
    Brainstorm --> Research
    Research --> Scaffold
    Scaffold --> Events
    Events --> Quantify
    Events --> Calibrate
    Calibrate --> Update
    Calibrate --> Synthesize
    Quantify --> Score
    Update --> Score
    Synthesize --> Assess
    Score --> Calibration
    Calibration --> Assess
    Assess --> End
```
<!-- DIAGRAM_ALIGNMENT
id: DIAG-FW-007
verified_date: 2026-07-10
verified_against: mcp-servers/hkask-mcp-scenarios/src/lib.rs:459-1708; mcp-servers/hkask-mcp-scenarios/src/superforecast.rs:165-400
reference_sources: mcp
status: VERIFIED
-->

## Scope and constraints

`scenario_quantify` validates event probabilities and dependency references before computing marginals. Its all-events value uses parent-true conditionals for a single-parent edge and a documented average proxy for a multi-parent edge; it is not a general joint-distribution engine. Forecasts are only persisted when `scenario_score` receives explicit outcomes, then `scenario_calibration` derives Brier and reliability signals from stored records.[^brier]

[^mcp]: Model Context Protocol. (2025). *Specification*. https://modelcontextprotocol.io/specification/2025-06-18
[^brier]: Brier, G. W. (1950). Verification of forecasts expressed in terms of probability. *Monthly Weather Review*, 78(1), 1–3. https://doi.org/10.1175/1520-0493(1950)078%3C0001:VOFEIT%3E2.0.CO;2

