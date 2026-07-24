---
title: "Loyalty Without Lock-In — The hKask Strategy for Distributed Agent Networks"
audience: [architects, developers, agents, curator]
last_updated: 2026-06-26
version: "0.31.0-draft"
status: "Draft — for review and refinement"
domain: "Strategy / Architecture"
mds_categories: [domain, composition, trust, curation]
---

# Loyalty Without Lock-In

## The hKask Strategy for Distributed Agent Networks

**Purpose:** Articulate the strategic rationale for hKask's distributed, loyalty-driven agent architecture as a counter-position to the platform lock-in strategies dominant in AI infrastructure. Ground the approach in Reichheld's loyalty economics framework and identify the architectural primitives (5W1H, federated curator, composable skills) that make it technically viable.

**Source texts:** 
- Carl Shapiro and Hal R. Varian, [*Information Rules: A Strategic Guide to the Network Economy*](https://www.hbs.edu/faculty/Pages/item.aspx?num=531) (Harvard Business School Press, 1999).
- Frederick F. Reichheld, [*The Loyalty Effect: The Hidden Force Behind Growth, Profits, and Lasting Value*](https://www.hbs.edu/faculty/Pages/item.aspx?num=385) (Harvard Business School Press, 1996).

**Related:** [`PRINCIPLES.md`](../architecture/core/PRINCIPLES.md), [`AGENTS.md`](../../AGENTS.md), [`hKask-architecture-master.md`](../architecture/core/hKask-architecture-master.md)

---

## 1. The Strategic Landscape

### 1.1 The Lock-In Playbook

Varian and Shapiro (1999) identified the structural dynamics of information markets: durable investments in complementary assets create switching costs that lock customers into vendor relationships. The playbook is well-understood and widely deployed:

| Tactic | Mechanism | AI Infrastructure Example |
|---|---|---|
| Data ownership | User data lives on provider servers; export is lossy or impossible | Conversation history, prompts, model preferences locked in platform silos |
| Complementary asset control | Tools, skills, and plugins bound to a specific runtime | MCP tools that only execute within a vendor's server environment |
| Network effects as moat | Value accrues to the marketplace, not the participants | "Everyone's skills are on our marketplace" — the marketplace IS the lock-in |
| Proprietary interfaces | The API is the protocol; no standard exists independent of the platform | MCP structured as remote procedure calls across a boundary the platform controls |
| Durable contracts | Per-seat pricing, enterprise agreements, credit systems denominated in platform currency | Inference credits, tiered subscriptions, volume commitments |

The structural insight: MCP's remote-call architecture is not an accident of engineering. It is a lock-in mechanism. The protocol's form (client→server RPC) mirrors the lock-in intent (tools live on the provider's side of the wire). Historical precedent is unambiguous: HTTP did not free users from browsers — it made Chrome the platform. SMTP did not free users from email providers — it made Gmail the platform. Protocols have consistently served as beachheads for platform consolidation.

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

The 39 skills in the capability catalog serve two functions:

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

hKask's skill architecture is structurally anti-lock-in:

- **Skills are local files** — `manifest.yaml` + `*.j2` templates, stored in the user's registry crate. They are copyable, versionable, forkable, and shareable independent of any platform.
- **The registry is local** — `SqliteRegistry` indexes skills on the user's filesystem. No marketplace. No remote dependency.
- **Selection intelligence lives in Jinja2/LLM** (P3 Generative Space) — the cascade (`select → populate → execute`) runs locally. The skill is a self-contained artifact that carries its own execution logic.
- **Gas budgets are user-denominated** — `gas.cap` and `rjoule.cap` are set per-skill. 1 rJ = 250,000 gas cycles. The user controls the budget, not the provider.

The contrast with MCP is structural: an MCP tool is "call this remote function." A hKask skill is "this is what it is (What), why it exists (Why), how it works (How), who made it (Who), when (When), and where it operates (Where)." The protocol is not "do this" — it is "know this."

### 2.3 Customer Selection Through Architecture

Reichheld emphasizes that the best companies are ruthless about *which* customers they keep — selecting for those who value what they uniquely provide. hKask's Magna Carta prohibitions function as selection mechanisms:

| Prohibition | What It Filters For | What It Filters Out |
|---|---|---|
| No visual UI, dashboards, Grafana, Prometheus | Users comfortable with CLI/REPL, programmatic observability via Regulation | Users who want managed experiences, graphical interfaces |
| No `todo!()`, `unimplemented!()`, stubs, feature flags | Users who value completeness and integrity | Users who tolerate half-finished features |
| No anonymous agency — every action has an authenticated author | Users who value accountability and provenance | Users who want convenience over transparency |
| No hidden parameters or admin-gated settings | Users who want visibility and control | Users who prefer managed/curated experiences |
| No pass-through abstractions | Users who value depth over surface area | Users who want shallow convenience wrappers |

These are not merely engineering constraints — they are customer selection. They attract users who value agency and sovereignty, and repel users who want a managed AI experience. This is strategic: loyal customers are those whose values align with what you uniquely provide.

---

## 3. The Distributed Network Architecture

### 3.1 The 5W1H Primitive

The hard problem of distributed agent networks — coordination without centralization — is structurally analogous to problems that have been solved before. Git solved distributed collaboration through content-addressable storage (Merkle DAG). Bitcoin solved distributed consensus through proof-of-work + longest chain rule. The answer in each case was not "try harder at coordination" but "find the primitive that makes coordination a side effect of correct operation."

For hKask, that primitive is the 5W1H ontological framework (P5.2):

| Dimension | Question | Verification Property |
|---|---|---|
| **Who** | Which pod authored this? WebID verifiable? | P12 userpod host mandate — every action carries an accountable identity |
| **What** | What artifact type? WordAct, FlowDef, KnowAct? Content hash? | Artifact integrity verification; type-level composition checking |
| **When** | When was it published? Versioned? | Temporal ordering; staleness detection; convergence windows |
| **Where** | Which pod namespace? What domain? | Pod boundary enforcement (P4.1); domain scoping |
| **Why** | What principle does it serve? What goal? | Magna Carta anchoring (P1–P4); reject artifacts that violate sovereignty |
| **How** | What's the cascade? Gas budget? Convergence threshold? | Composability validation; resource drain prevention |

A pod receiving a skill from another pod can verify all six dimensions independently — no central registry, no trusted authority, no platform mediation required. The ontology IS the protocol.

### 3.2 The Federated Curator: Capability Surface, Not Control Surface

The Curator daemon (P12.1) already exists as a per-system regulatory loop. Its current metacognition templates (calibrate, diagnose, escalate, system_state_gather) operate on system health metrics. The extension to a federated curator means operating those templates on 5W1H dimensions for cross-pod decisions:

| Curator Template | Current Scope | Federated Scope |
|---|---|---|
| `system_state_gather` | Local pod health, Regulation spans, bot success rates | Cross-pod 5W1H consistency, federation health, peer pod state |
| `metacognition-diagnose` | Local alert cascades, resource exhaustion | Ontological conflicts between pods, skill composition failures, trust degradation |
| `metacognition-calibrate` | Local threshold tuning, gas budget adjustment | Federation trust thresholds, convergence window sizing, sync interval tuning |
| `metacognition-self-calibrate` | Curator self-management — generates its own escalation-threshold adjustment from self-quality + effectiveness delta (generative-first, Rust safety-rail fallback; `reg.meta.self_calibration` spans) | Cross-pod self-calibration policy exchange, federated GEPA evolution of the self-calibration template |
| `metacognition-escalate` | Administrator alerts for local issues | Cross-pod conflict escalation, federation integrity violations, sovereignty breaches |

The critical architectural principle: the federated curator is a **capability surface**, not a control surface. It does not block, gate, or reject. It surfaces the 5W1H answers — "this artifact was authored by pod X, serves principle P3, has a convergence threshold of 0.15, and its content hash is Y." The user retains sovereignty (P1). The curator's job is to make ontological answers visible and verifiable, enabling the user to make capability decisions without performing the analysis themselves.

This is the inversion of the traditional platform model. In a centralized platform, the curator is a gatekeeper — it controls what's allowed. In hKask, the curator is an enabler — it reveals what's true, and the user decides.

### 3.3 The Platform Solves Its Own Problems

The architecture's evolution model: as more pods federate, the curator's 5W1H diagnosis improves because it observes more patterns of what composes well and what doesn't. The network's value is not in its size (network effects) but in the curator's improving ability to help users compose capabilities across pods (learning effects).

This is a qualitative difference from the Varian model:

| | Varian Lock-In Network | hKask Loyalty Network |
|---|---|---|
| Value driver | Number of participants | Quality of ontological diagnosis |
| Growth mechanism | Network effects (more users → more valuable) | Learning effects (more pods → better composition advice) |
| User relationship | Customer (consumes platform services) | Agent (develops capability through platform) |
| Exit dynamic | Switching costs prevent departure | Loyalty makes departure undesirable |
| Curator role | Gatekeeper (controls access) | Enabler (surfaces truth) |

---

## 4. Loyalty Metrics for Agent Networks

### 4.1 What Not to Measure

Standard SaaS metrics (DAU/MAU, session count, feature adoption) measure consumption, not capability. A user who learns the Kata methodology and then spends a week applying it to a real problem offline is more valuable than a user who pings the platform 50 times a day for convenience. Consumption metrics would penalize capability development.

### 4.2 Proposed Loyalty Metrics

| Metric | What It Measures | Anti-Lock-in Signal |
|---|---|---|
| **Skill convergence rate** | Are users' skills reaching quality thresholds (convergence ≤ threshold)? | User is getting value from skill execution, not just invoking tools |
| **Kata automaticity scores** | Is scientific thinking (PDCA, observation vs. interpretation) becoming habitual? | Capability is internalizing; user carries the skill independent of the platform |
| **Pod federation depth** | Are users connecting their pods to others? How many peer pods? | Network growth is organic and user-initiated, not platform-mandated |
| **Skill composition breadth** | How many distinct skills are users composing together via bundler? | Value grows through composition depth, not feature count |
| **Voluntary retention** | Are users staying when alternatives exist? | Distinguishes loyalty from lock-in |
| **Referral rate** | Are users bringing other users? | Loyalty's economic engine is working |
| **Capability attestation** | Can users demonstrate skills learned through the platform independently? | Capability is genuine and portable |

The key metric: **voluntary retention vs. capability growth correlation**. If users who show the highest capability growth (Kata automaticity, skill convergence) also show the highest retention, the loyalty engine is working. If retention is high but capability is flat, lock-in may be masquerading as loyalty.

---

## 5. Open Questions

1. **Federation discovery.** How do pods discover each other without a central registry? The Matrix-based transport provides point-to-point connections, but discovery remains an open design question.

2. **Trust calibration.** How does the curator calibrate trust in artifacts from unknown pods? The 5W1H framework provides verification, but verification is not trust. A pod can produce a verifiable artifact that is nevertheless malicious or low-quality.

3. **Capability attestation.** How can users prove they've developed capabilities through hKask without revealing their data? A zero-knowledge attestation of "this user has achieved Kata automaticity score X" without revealing the underlying session data.

4. **Economic sustainability without lock-in.** What is the revenue model when switching costs are zero? Reichheld's data shows loyal customers are profitable, but the transition from "capture value through lock-in" to "earn value through loyalty" requires different pricing structures.

5. **Minimum viable federation.** What is the smallest set of artifacts that must carry 5W1H answers for federation to be meaningful? If only skills carry full 5W1H but hMems carry only partial answers, the curator's diagnostic capability is limited.

6. **The protocol trap revisited.** If MCP evolves in lock-in-favoring directions (authentication tied to platform identity, tool discovery gated by marketplace), how does hKask maintain protocol compatibility without accepting platform dependency?

---

## 6. Summary

hKask's strategy is to build loyalty through capability enablement rather than lock-in through switching costs. The technical primitives — 5W1H ontological anchoring, composable skill templates, per-pod sovereignty, federated curator as capability surface — are not merely engineering decisions but strategic counter-positions to the dominant platform model in AI infrastructure.

The bet: in an era where AI accelerates individual capability development, the platform that enables users to become more capable will earn loyalty that no lock-in mechanism can match. The economics of loyalty (Reichheld) will outperform the economics of lock-in (Varian) because AI makes the capability compounding rate the dominant variable — and lock-in cannot produce capability.

**Status:** Draft v0.1.0 — for review, critique, and refinement. The open questions in §5 represent active research areas.
