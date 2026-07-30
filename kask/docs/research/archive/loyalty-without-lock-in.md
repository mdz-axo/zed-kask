---
title: "Loyalty Without Lock-In — The hKask Strategy for a Sovereign In-Process Agent Platform"
audience: [architects, developers, agents, curator]
last_updated: 2026-07-29
version: "0.31.0"
status: "Active"
domain: "Strategy / Architecture"
mds_categories: [domain, composition, trust, curation]
---

# Loyalty Without Lock-In

## The hKask Strategy for a Sovereign In-Process Agent Platform

**Purpose:** Articulate the strategic rationale for hKask's loyalty-driven, in-process agent architecture as a counter-position to the platform lock-in strategies dominant in AI editors. Ground the approach in Reichheld's loyalty economics and Shapiro/Varian's network-economics framework, and identify the architectural primitives (sovereign skills, composable templates, in-process MCP, the guard layer) that make it technically viable inside an open-source editor fork.

**Source texts:**
- Carl Shapiro and Hal R. Varian, [*Information Rules: A Strategic Guide to the Network Economy*](https://www.hbs.edu/faculty/Pages/item.aspx?num=531) (Harvard Business School Press, 1999).
- Frederick F. Reichheld, [*The Loyalty Effect: The Hidden Force Behind Growth, Profits, and Lasting Value*](https://www.hbs.edu/faculty/Pages/item.aspx?num=385) (Harvard Business School Press, 1996).

**Related:** [`PRINCIPLES.md`](../architecture/core/PRINCIPLES.md), [`magna-carta.md`](../architecture/core/magna-carta.md), [`zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md)

---

## 1. The Strategic Landscape

### 1.1 The Lock-In Playbook

Varian and Shapiro (1999) identified the structural dynamics of information markets: durable investments in complementary assets create switching costs that lock customers into vendor relationships. The playbook is well-understood and widely deployed across the AI-editor category:

| Tactic | Mechanism | AI Editor Example |
|---|---|---|
| Data ownership | User data lives on provider servers; export is lossy or impossible | Conversation history, prompts, agent traces, model preferences locked in a cloud silo |
| Complementary asset control | Tools, skills, and plugins bound to a specific runtime | Agent skills that only execute within the vendor's hosted inference path |
| Network effects as moat | Value accrues to the marketplace, not the participants | "Everyone's agents/skills are on our registry" — the registry IS the lock-in |
| Proprietary interfaces | The API is the protocol; no standard exists independent of the platform | MCP structured as remote procedure calls across a boundary the platform controls |
| Durable contracts | Per-seat pricing, enterprise agreements, credit systems denominated in platform currency | Inference credits, tiered subscriptions, volume commitments, seat-based agent licensing |

The structural insight: an AI editor that hosts skills, agents, and inference behind a remote boundary is not an accident of engineering. It is a lock-in mechanism. The protocol's form (client→server RPC, hosted agent runtime) mirrors the lock-in intent (tools and traces live on the provider's side of the wire). Historical precedent is unambiguous: HTTP did not free users from browsers — it made Chrome the platform. SMTP did not free users from email providers — it made Gmail the platform. Protocols have consistently served as beachheads for platform consolidation. The same dynamic now applies to "AI-native editors" that bundle agent, model, and tool into one closed surface.

### 1.2 The Loyalty Alternative

Reichheld (1996) demonstrated an alternative economic engine: genuine loyalty produces superior financial outcomes without switching costs. His core finding — a 5% increase in customer retention produces a 25–95% increase in profits — operates through mechanisms that are structurally distinct from lock-in:

| Reichheld Mechanism | Description | Lock-In Counterpart |
|---|---|---|
| Lower cost to serve | Loyal customers know the system and require less support | Trapped customers demand concessions and workarounds |
| Expanding relationships | Loyal customers buy more over time as trust deepens | Locked-in customers minimize spend, waiting for alternatives |
| Referrals | Loyal customers become advocates; acquisition cost trends toward zero | Locked-in customers become detractors; reputation damage increases marketing spend |
| Price insensitivity | Loyal customers value the relationship, not the transaction | Locked-in customers are acutely price-sensitive when alternatives appear |
| Honest feedback | Loyal customers want the provider to succeed and give constructive input | Locked-in customers withhold feedback or weaponize complaints |

The critical distinction: trapped customers (high switching costs, no alternative) look identical to loyal customers in retention metrics. Both have low churn. But trapped customers defect en masse when switching costs drop, while loyal customers stay because they *don't want to leave*. Retention is the same number; the underlying dynamic determines whether it's an asset or a liability.

---

## 2. The hKask Counter-Position

### 2.1 Capability Enablement as Loyalty Engine

hKask's strategy is to invert the lock-in gradient: instead of "the platform gets stickier over time," hKask aims for "the user gets more capable over time, and hKask becomes more valuable because the user is more capable."

The skills in the capability catalog serve two functions:

**Capability-building skills** (create new user capabilities → loyalty through enablement):
- Kata bundle (starter/improvement/coaching) — scientific thinking, PDCA methodology
- Pragmatic-semantics — epistemic discipline, distinguishing IS from OUGHT
- Sequential-inquiry — structured chain-of-thought reasoning
- Grill-me — Socratic self-examination
- Superforecasting — calibrated probability judgment
- MCDA — structured decision analysis
- Scenario-builder — strategic thinking under uncertainty

**Productivity skills** (make work faster → loyalty through effectiveness):
- Coding-guidelines, TDD, diagnose, bug-hunt, deep-module, etc.

The capability-building skills are the strategic differentiator. A user who learns scientific thinking through the Kata bundle does not just use hKask better — they think better. That capability is portable. It leaves with the user. The loyalty created is not "I can't leave" but "I wouldn't want to — this relationship made me who I am."

### 2.2 Skills as Composable Templates, Not Platform Plugins

hKask's skill architecture is structurally anti-lock-in. Inside zed-kask, skills execute in-process via the D1 seam (`SkillTool` + `BridgeManifestExecutor`), but the skill *artifacts* themselves are sovereign:

- **Skills are local files** — `manifest.yaml` + `*.j2` templates, stored in the user's registry crate. They are copyable, versionable, forkable, and shareable independent of any platform.
- **The registry is local** — `SqliteRegistry` indexes skills on the user's filesystem. No marketplace. No remote dependency. No vendor-controlled discovery.
- **Selection intelligence lives in Jinja2/LLM** (P3 Generative Space) — the cascade (`select → populate → execute`) runs locally, in-process. The skill is a self-contained artifact that carries its own execution logic.
- **Gas budgets are user-denominated** — `gas.cap` and `rjoule.cap` are set per-skill. 1 rJ = 250,000 gas cycles. The user controls the budget, not the provider.
- **Inference is user-routed** — zed-kask owns the provider keystore and inference routing (`crates/language_model*`, `crates/credentials_provider`). hKask never sees the API key. The user picks the model; the guard layer (D4) only inspects content, never exfiltrates credentials.

The contrast with a hosted-agent editor is structural: a hosted agent is "call this remote function on our runtime, with our model, against our stored context." A hKask skill is "this is what it is (What), why it exists (Why), how it works (How), who made it (Who), when (When), and where it operates (Where)." The protocol is not "do this" — it is "know this." The artifact is portable; the runtime is the user's own machine.

### 2.3 Customer Selection Through Architecture

Reichheld emphasizes that the best companies are ruthless about *which* customers they keep — selecting for those who value what they uniquely provide. hKask's Magna Carta prohibitions function as selection mechanisms:

| Prohibition | What It Filters For | What It Filters Out |
|---|---|---|
| No `todo!()`, `unimplemented!()`, stubs, feature flags | Users who value completeness and integrity | Users who tolerate half-finished features |
| No anonymous agency — every action has an authenticated author | Users who value accountability and provenance | Users who want convenience over transparency |
| No hidden parameters or admin-gated settings | Users who want visibility and control | Users who prefer managed/curated experiences |
| No pass-through abstractions | Users who value depth over surface area | Users who want shallow convenience wrappers |
| Sovereign keys — hKask never holds the provider credential | Users who want to own their inference spend and model choice | Users who want the editor to be the billing surface |

These are not merely engineering constraints — they are customer selection. They attract users who value agency and sovereignty, and repel users who want a managed AI experience. This is strategic: loyal customers are those whose values align with what you uniquely provide.

---

## 3. The In-Process Sovereign Architecture

### 3.1 The Sovereignty Primitive

The hard problem of a sovereign agent platform — capability without dependence — is structurally analogous to problems that have been solved before. Git solved distributed collaboration through content-addressable storage (Merkle DAG). Bitcoin solved distributed consensus through proof-of-work + longest chain rule. The answer in each case was not "try harder at coordination" but "find the primitive that makes the desired property a side effect of correct operation."

For hKask inside zed-kask, that primitive is **in-process sovereignty**: the agent, the tools, the memory, and the guard all run in the user's own process, against the user's own keys, on the user's own filesystem. The architecture plan (`zed-host-architecture-plan.md` §2, §13) codifies this as the minimal-divergence fork: zed owns the editor, chat/Agent Panel, inference routing, and provider keystore; hKask plugs in through ten named seams (D1–D10) and nothing else.

| Dimension | Question | Verification Property |
|---|---|---|
| **Who** | Which userpod authored this? OCAP-signed? | P12 userpod host mandate — every action carries an accountable identity |
| **What** | What artifact type? WordAct, FlowDef, KnowAct? Content hash? | Artifact integrity verification; type-level composition checking |
| **When** | When was it produced? Versioned? | Temporal ordering; staleness detection; convergence windows |
| **Where** | Which userpod namespace? What domain? | Pod boundary enforcement (P4.1); domain scoping |
| **Why** | What principle does it serve? What goal? | Magna Carta anchoring (P1–P4); reject artifacts that violate sovereignty |
| **How** | What's the cascade? Gas budget? Convergence threshold? | Composability validation; resource drain prevention |

A user inspecting any artifact produced by the agent can verify all six dimensions locally — no central registry, no trusted authority, no platform mediation required. The ontology IS the protocol. Because execution is in-process, verification does not depend on a remote service remaining available or trustworthy.

### 3.2 The Curator: Capability Surface, Not Control Surface

The Curator (P12.1) is a per-userpod regulatory loop. Its metacognition templates (calibrate, diagnose, escalate, system_state_gather) operate on system health metrics and Regulation spans emitted by the in-process tools. The Curator's scope inside zed-kask:

| Curator Template | Scope |
|---|---|
| `system_state_gather` | Local pod health, Regulation spans, tool success rates, guard outcomes |
| `metacognition-diagnose` | Local alert cascades, resource exhaustion, skill composition failures |
| `metacognition-calibrate` | Local threshold tuning, gas budget adjustment, convergence window sizing |
| `metacognition-self-calibrate` | Curator self-management — generates its own escalation-threshold adjustment from self-quality + effectiveness delta (generative-first, Rust safety-rail fallback; `reg.meta.self_calibration` spans) |
| `metacognition-escalate` | User-visible alerts for local issues; sovereignty breaches; guard refusals |

The critical architectural principle: the Curator is a **capability surface**, not a control surface. It does not block, gate, or reject on the user's behalf. It surfaces the 5W1H answers — "this artifact was authored by userpod X, serves principle P3, has a convergence threshold of 0.15, and its content hash is Y." The user retains sovereignty (P1). The Curator's job is to make ontological answers visible and verifiable, enabling the user to make capability decisions without performing the analysis themselves.

This is the inversion of the traditional platform model. In a centralized AI editor, the curator-equivalent is a gatekeeper — it controls what's allowed, what's moderated, what reaches the model. In hKask, the Curator is an enabler — it reveals what's true, and the user decides.

### 3.3 The Platform Solves Its Own Problems

The architecture's evolution model: as the user composes more skills and accumulates more memory, the Curator's diagnosis improves because it observes more patterns of what composes well and what doesn't. The platform's value is not in its user count (network effects) but in the Curator's improving ability to help the user compose capabilities (learning effects). Because skills are local and shareable, this learning can propagate user-to-user through skill forks without a central marketplace.

This is a qualitative difference from the Varian model:

| | Varian Lock-In Network | hKask Loyalty Platform |
|---|---|---|
| Value driver | Number of participants on the platform | Quality of ontological diagnosis for the individual user |
| Growth mechanism | Network effects (more users → more valuable) | Learning effects (more use → better composition advice); skill forks propagate learning peer-to-peer |
| User relationship | Customer (consumes platform services) | Agent (develops capability through the platform) |
| Exit dynamic | Switching costs prevent departure | Loyalty makes departure undesirable; skills leave with the user |
| Curator role | Gatekeeper (controls access) | Enabler (surfaces truth) |
| Credential holder | Platform holds the API key | User holds the API key (D5, D9) |

---

## 4. Loyalty Metrics for a Sovereign Agent Platform

### 4.1 What Not to Measure

Standard SaaS metrics (DAU/MAU, session count, feature adoption) measure consumption, not capability. A user who learns the Kata methodology and then spends a week applying it to a real problem offline is more valuable than a user who pings the agent 50 times a day for convenience. Consumption metrics would penalize capability development. Worse, in a sovereign model there is no server-side telemetry to collect them from — the user's process is their own.

### 4.2 Proposed Loyalty Metrics

| Metric | What It Measures | Anti-Lock-in Signal |
|---|---|---|
| **Skill convergence rate** | Are users' skills reaching quality thresholds (convergence ≤ threshold)? | User is getting value from skill execution, not just invoking tools |
| **Kata automaticity scores** | Is scientific thinking (PDCA, observation vs. interpretation) becoming habitual? | Capability is internalizing; user carries the skill independent of the platform |
| **Skill composition breadth** | How many distinct skills are users composing together via the bundler? | Value grows through composition depth, not feature count |
| **Voluntary retention** | Are users staying when alternatives exist? | Distinguishes loyalty from lock-in |
| **Referral rate** | Are users bringing other users? | Loyalty's economic engine is working |
| **Skill fork/share rate** | Are users exporting, forking, and sharing skills? | Sovereignty is genuine; artifacts are portable |
| **Capability attestation** | Can users demonstrate skills learned through the platform independently? | Capability is genuine and portable |

The key metric: **voluntary retention vs. capability growth correlation**. If users who show the highest capability growth (Kata automaticity, skill convergence) also show the highest retention, the loyalty engine is working. If retention is high but capability is flat, lock-in may be masquerading as loyalty.

---

## 5. Open Questions

1. **Skill discovery without a marketplace.** How do users discover skills authored by others without a central registry? The local-registry model preserves sovereignty but creates a discovery gap. Candidate answers: git-cloneable skill repos, signed skill bundles, community-curated lists — none of which require platform mediation.

2. **Trust calibration for imported skills.** How does the Curator calibrate trust in skills authored by other users? The 5W1H framework provides verification (the artifact is well-formed and signed), but verification is not trust. A skill can be verifiable and nevertheless malicious or low-quality. OCAP scopes the blast radius, but reputation remains an open design question.

3. **Capability attestation.** How can users prove they've developed capabilities through hKask without revealing their data? A zero-knowledge attestation of "this user has achieved Kata automaticity score X" without revealing the underlying session data.

4. **Economic sustainability without lock-in.** What is the revenue model when switching costs are zero? Reichheld's data shows loyal customers are profitable, but the transition from "capture value through lock-in" to "earn value through loyalty" requires different pricing structures. For an open-source fork, this question folds into the broader sustainability of open-source development.

5. **Minimum viable sovereignty.** What is the smallest set of artifacts that must carry 5W1H answers for the Curator's diagnosis to be meaningful? If only skills carry full 5W1H but memories carry only partial answers, the Curator's diagnostic capability is limited.

6. **The protocol trap revisited.** If MCP evolves in lock-in-favoring directions (authentication tied to platform identity, tool discovery gated by marketplace), how does zed-kask maintain protocol compatibility without accepting platform dependency? The in-process hosting model (D3) is a partial answer — the MCP servers run as local child processes, not remote endpoints — but the question of which MCP servers remain safe to host remains live.

---

## 6. Summary

hKask's strategy is to build loyalty through capability enablement rather than lock-in through switching costs. Inside zed-kask, the technical primitives — in-process skill execution (D1), the Curator as capability surface, composable skill templates, sovereign keys (D5/D9), the guard layer (D4), and the minimal-divergence fork discipline (D1–D10) — are not merely engineering decisions but strategic counter-positions to the dominant closed-AI-editor model.

The bet: in an era where AI accelerates individual capability development, the platform that enables users to become more capable — while keeping their keys, their data, and their skills sovereign — will earn loyalty that no lock-in mechanism can match. The economics of loyalty (Reichheld) will outperform the economics of lock-in (Varian) because AI makes the capability compounding rate the dominant variable — and lock-in cannot produce capability. The fork is the load-bearing strategic choice: it is what makes sovereignty structurally enforceable rather than merely promised.
