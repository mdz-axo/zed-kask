---
title: "Dokkodo Mindset — User Guide and Research Companion"
audience: [agents, developers, curators, architects]
last_updated: 2026-07-19
version: "0.31.0"
status: "Active"
domain: "Metacognition"
mds_categories: [domain, composition, curation, trust]
---

# Dokkodo Mindset — User Guide and Research Companion

**Purpose:** How to use the `dokkodo-mindset` skill, how it composes with other hKask skills, and the philosophical research that grounds it.

**Source texts:**
- Miyamoto Musashi, *Dokkodo* ("The Way of Walking Alone"), 1645 — 21 precepts written days before his death
- Jonathan Hall, *American Ronin* — contemporary commentary on the Dokkodo as resilience in adversity

**Related:** [`PRINCIPLES.md`](../architecture/core/PRINCIPLES.md), [`hKask-architecture-master.md`](../architecture/core/hKask-architecture-master.md)

**Registry:** `registry/templates/dokkodo-mindset/` — `manifest.yaml` + `dokkodo-perceive.j2`
**Skill file:** `.agents/skills/dokkodo-mindset/SKILL.md`

---

## Contents

1. [Quick Start](#1-quick-start) — Activation triggers and when to apply
2. [What It Does](#2-what-it-does) — The skill's function and output
3. [How It Works](#3-how-it-works) — The perceptual transformation process and convergence loop (δP = 0)
4. [Composition Patterns](#4-composition-patterns) — How it composes with pragmatic-laziness, essentialist, and other skills
5. [The 21 Precepts](#5-the-21-precepts) — Full precept table with cluster groupings
6. [Architectural Role](#6-architectural-role) — Where the perceptual layer sits in hKask's architecture
7. [Research Grounding](#7-research-grounding) — Philosophical foundations, Hall's American Ronin, the lazy universe
8. [Design Decisions](#8-design-decisions) — Why KnowAct, why separate skill, why 4 clusters
9. [Open Questions](#9-open-questions) — Deferred research and v0.31+ candidates

---

## 1. Quick Start

### Activation Triggers

Invoke the Dokkodo mindset by saying any of these:

| Say this | To get this |
|----------|-------------|
| **"apply the Dokkodo"** / "warrior mindset" / "see this clearly" | Full perceptual filter — all four precept clusters |
| "perceptual reset" / "clear the lens" | Cluster A only — clear self-importance, preference, custom |
| "what am I attached to here?" | Cluster B only — identify desire/attachment distortions |
| "what emotional friction is here?" / "am I resenting this?" | Cluster C only — identify regret, envy, resentment |
| "what would this look like if I accepted death?" | Cluster D only — existential posture, brachistochrone frame |
| "is my perception clear?" / "δP = 0?" | Convergence check — has perception reached stationary state? |

### When to Apply

| You feel… | Apply… | Because… |
|-----------|--------|----------|
| Stuck on a decision | Full Dokkodo | Preference or attachment may be creating a false local minimum |
| Resentful about past work | Cluster C | Resentment is energy spent wishing the past were different |
| Attached to code you wrote | Cluster B | Attachment makes deletion look harder than it is |
| Afraid of a risky change | Cluster D | Fear distorts the action landscape; the brachistochrone may be invisible |
| "The obvious path feels wrong" | Cluster A | Custom or preference may be masquerading as least action |
| Something just failed | Full Dokkodo | Accept things exactly as they are — then act |

### Prerequisites

This guide assumes familiarity with:
- **hKask skill types:** KnowAct ("how to think"), FlowDef ("what to do"), WordAct ("what to say") — see [`hKask-architecture-master.md`](../architecture/core/hKask-architecture-master.md) Pattern A
- **Pragmatic laziness:** The skill that finds the path of least structural action (δS = 0) by decomposing, mapping loops, and applying the deletion test
- **pragmatic-semantics:** The skill that classifies constraints as Prohibition, Guardrail, Guideline, Evidence, or Hypothesis

Terms defined on first use below.

---

## 2. What It Does

The Dokkodo mindset is a **metacognitive perceptual filter** — a KnowAct that applies Miyamoto Musashi's 21 precepts as a lens. It does not advise, plan, or act. It *perceives*.

**Input:** A situation (decision space, problem, or context) and an optional domain (`code`, `architecture`, `decision`, `relationship`, `self`, `general`).

**Output:** A JSON object with:
- `clarified_perception` — the situation seen through the Dokkodo lens (attachment-free, preference-free, custom-free)
- `distortions_removed` — each perceptual distortion found, mapped to its precept and type
- `precept_alignment` — per-precept scores (0.0 = fully violated, 1.0 = fully aligned)
- `action_landscape_shift` — before/after summary plus a `shift_magnitude` scalar (0.0 = no change)
- `brachistochrone_candidates` — paths that look harder short-term but minimize long-term action
- `stationary` / `should_continue` / `escalate` — convergence state

**Governing precept:** Precept 1 — *Accept things exactly as they are.*
**Meta-precept:** Precept 21 — *Never stray from the Way.* Resolves conflicts when precepts clash.

### Key Concept: Mindset Transforms the Action Landscape

The pragmatic-laziness skill finds the path of least structural action (written δS = 0 — "the change in action S is zero, meaning the configuration is stationary"). But **what counts as "action" depends on your perceptual frame.**

| Perceptual distortion | How it warps the action landscape | Removed by |
|-----------------------|-----------------------------------|------------|
| **Attachment** | Clinging looks easier than releasing. Maintaining dead code appears cheaper than deleting it. | Precepts 5, 14 |
| **Preference** | The path you *like* looks shorter than the path that minimizes action. | Precept 11 |
| **Resentment** | Complaining about a bad decision feels productive. It isn't — it's friction without traction. | Precepts 6, 9 |
| **Fear** | Avoiding a risky refactor appears safer than executing it. Maintenance cost accumulates silently. | Precept 17 |
| **Custom** | "We've always done it this way" makes habit look like truth. | Precept 15 |

The Dokkodo removes these distortions. Pragmatic laziness then evaluates the *clarified* landscape — finding the true δS = 0 path, not the path that looked shortest through a distorted lens.

### Key Concept: The Brachistochrone

A **brachistochrone** (from Greek *brachistos* "shortest" + *chronos* "time") is the curve of fastest descent between two points. Counter-intuitively, it is not a straight line — it's a **cycloid** that dips *below* the endpoint before rising. The straight line looks shorter but is slower. The brachistochrone looks longer but gets there faster.

This concept appears in both skills:
- **Pragmatic laziness:** Sometimes you must go *through* apparent complexity to extract the deeper pattern that reduces total system action.
- **Dokkodo:** Accepting death (Precept 17), relinquishing attachment (Precept 5), and enduring separation (Precept 8) are brachistochrone operations — they look harder short-term but minimize action across time.

---

## 3. How It Works

### The Perceptual Transformation Loop

The skill runs a single template (`dokkodo-perceive.j2`) with embedded convergence logic, following the same pattern as `pragmatic-laziness-flow.j2`:

```
STEP 1: IDENTIFY DISTORTIONS
├─ Cluster A (Perceptual Reset): Where is perception distorted by
│  self-importance, preference, custom, or wishful thinking?
├─ Cluster B (Desire/Attachment): Where does wanting something
│  make a path look easier or harder than it is?
├─ Cluster C (Emotional Resilience): What emotional friction
│  (regret, envy, resentment, grief) is adding perceived action
│  that produces nothing?
└─ Cluster D (Existential Posture): What would the landscape
   look like if death were accepted and tools were minimal?
         ▼
STEP 2: APPLY THE LENS
   For each distortion: state it → apply the precept → re-perceive
         ▼
STEP 3: MAP THE LANDSCAPE SHIFT
   Which paths now appear shorter? Longer? Any brachistochrone
   candidates — paths that look harder but minimize total action?
         ▼
STEP 4: CONVERGENCE CHECK (δP = 0)
   ┌──────────────────────────────────────────────────────┐
   │ No new distortions AND shift_magnitude ≈ 0?           │
   │ → δP = 0. STATIONARY. Perception is clear. Done.      │
   │                                                       │
   │ New distortions found?                                │
   │ → δP ≠ 0. Continue if iterations remain (max 3).      │
   │                                                       │
   │ More distortions than previous iteration?             │
   │ → REGRESSION. Escalate — lens is adding noise.        │
   │                                                       │
   │ Max iterations reached without δP = 0?                │
   │ → ESCALATE to human with clearest perception found.   │
   └──────────────────────────────────────────────────────┘
```

**δP = 0** ("delta P equals zero") means the perceptual frame has reached stationary state — applying the precepts again produces no new distortions and the action landscape stops shifting. This is the perceptual analog of δS = 0 in pragmatic laziness.

### What "Perceptual Convergence" Looks Like

```
Pass 1: Apply all 4 clusters. 3 distortions found.
        shift_magnitude = 0.4 → δP ≠ 0 → Continue

Pass 2: Apply lens to clarified perception from pass 1.
        1 new distortion found (was hidden by the previous ones).
        shift_magnitude = 0.1 → δP ≠ 0 → Continue

Pass 3: No new distortions. shift_magnitude ≈ 0.0
        → δP = 0 → STATIONARY. Perception is clear. Done.
```

If the first pass finds no distortions and `shift_magnitude` is 0.0, perception was already clear — that IS valid convergence. No loop needed.

---

## 4. Composition Patterns

### Primary Chain: Perception → Laziness → Deletion

```
Dokkodo (perceive) → Pragmatic Laziness (δS = 0) → Essentialist (delete)
```

Use when a design decision feels stuck or "the obvious path" feels wrong. The Dokkodo clears the lens; pragmatic laziness finds the true least-action path; essentialist verifies by deletion test.

**Example:**
> "I need to decide whether to keep the legacy adapter or rewrite it. Apply the Dokkodo, then find the lazy path."

The Dokkodo surfaces: attachment to code you wrote (Precept 14), preference for the more interesting rewrite (Precept 11), resentment at the legacy code's quality (Precept 9). Pragmatic laziness then evaluates the clarified landscape: the adapter is a pass-through with zero behavior — deletion reduces total system action.

### Resilience Chain: Error → Stabilize → Diagnose

```
Regulation alert / error state → Dokkodo (stabilize perception) → Diagnose → Fix
```

This is the **American Ronin** pattern (see §7.3). When the agent encounters a constraint violation, adversarial input, or unexpected failure, the Dokkodo prevents panic, resentment, or outcome-attachment from distorting diagnosis. Accept things exactly as they are — the error happened. Now perceive clearly what must be done.

### Invocation from Other Skills

- `pragmatic-laziness` can invoke Dokkodo as a pre-filter before Phase 1 (Decompose)
- `pragmatic-semantics` can invoke it when a constraint conflict feels emotionally loaded
- `coding-guidelines` can invoke it when "simplicity first" is distorted by attachment to elegant code
- `essentialist` can invoke it when the deletion test produces resistance ("but we spent months on this module")

`pragmatic-semantics` runs across all layers and is **never relaxed** — the Dokkodo clarifies perception but does not override Prohibitions or Guardrails.

---

## 5. The 21 Precepts

Grouped into four functional clusters. The clustering is for the *process* (which questions to ask), not the output — individual alignment scores are still produced for every precept.

### Cluster A — Perceptual Reset (1, 4, 11, 12, 15, 19)
Clear the lens of self-importance, preference, custom, and wishful thinking.

| # | Precept |
|---|---------|
| 1 | Accept things exactly as they are |
| 4 | Think lightly of yourself but seriously of the world |
| 11 | In all things, have no preferences |
| 12 | Be indifferent to where you live |
| 15 | Do not act following customary beliefs |
| 19 | Respect Buddha and the Gods without counting on their help |

### Cluster B — Desire/Attachment Management (2, 5, 10, 13, 14, 18)
Identify where wanting distorts seeing — paths look easier because you want them to, or harder because you're clinging.

| # | Precept |
|---|---------|
| 2 | Do not seek pleasure for its own sake |
| 5 | Avoid attachment to desire for as long as you live |
| 10 | Do not let yourself be guided by the feeling of lust or love |
| 13 | Do not pursue the taste of good food |
| 14 | Do not hold on to possessions you no longer need |
| 18 | Do not seek to either goods or fiefs for your old age |

### Cluster C — Emotional Resilience (3, 6, 7, 8, 9)
Eliminate friction that produces nothing: regret (action spent on an unchangeable past), envy (comparing your path to another's), resentment (wishing the past were different).

| # | Precept |
|---|---------|
| 3 | Do not ever rely on a partial feeling |
| 6 | Do not regret anything that you have done |
| 7 | Never be envious |
| 8 | Never let yourself be saddened by a separation |
| 9 | Resentment and complaining are not appropriate for the warrior or for anyone else |

### Cluster D — Existential Posture (16, 17, 20, 21)
The ultimate brachistochrone frame. When death is accepted, no path is too costly.

| # | Precept |
|---|---------|
| 16 | Do not collect weapons or train with weapons beyond what is useful |
| 17 | Do not fear Death |
| 20 | You may abandon your own body, but you must preserve your honor |
| 21 | Never stray from the Way |

---

## 6. Architectural Role

### 6.1 Where It Sits

The Dokkodo introduces a **perceptual layer** that sits before hKask's existing skill layers:

```
┌─────────────────────────────────────────────────────────────┐
│ PERCEPTUAL LAYER                                             │
│ dokkodo-mindset — clears attachment, preference, resentment, │
│ fear, and custom from the perceived action landscape         │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ REGULATIVE LAYER                                             │
│ pragmatic-semantics — classifies and enforces boundaries.    │
│ Runs across ALL layers. Prohibitions and Guardrails are      │
│ never relaxed by perceptual transformation.                  │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ ANALYTIC LAYER                                               │
│ pragmatic-laziness → essentialist → grill-me                 │
│ Decompose, identify loops, find stationary action             │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ EXECUTIVE LAYER                                              │
│ coding-guidelines, skill-specific KnowActs/FlowDefs          │
│ Constrained implementation                                   │
└─────────────────────────────────────────────────────────────┘
```

**Relationship to hKask's four essential patterns** (Patterns A–D in [`hKask-architecture-master.md`](../architecture/core/hKask-architecture-master.md)): The perceptual layer is an extension of Pattern A (the Skills Model) — it adds a new composition position (pre-regulative) to the existing KnowAct/FlowDef/WordAct type system. It does not modify Patterns B (Regulation), C (OCAP), or D (Pods).

### 6.2 Why Perception Precedes Analysis

If pragmatic-laziness evaluates a perception-distorted landscape, it finds the δS = 0 path of the *distorted* landscape — which may not be the true least-action path. The Dokkodo removes the distortion first, then pragmatic-laziness finds the genuine stationary action point.

This is the **brachistochrone synergy**: when the Dokkodo clears the perceptual field and pragmatic-laziness evaluates the clarified landscape, brachistochrone candidates that were invisible (because fear made them look longer, or attachment made alternatives look shorter) become visible. Neither skill alone would find these paths.

---

## 7. Research Grounding

### 7.1 The Dokkodo as Perceptual Discipline (Not Moral Code)

The 21 precepts are **not a moral code**. They do not tell you what to value — they remove the distortions that prevent you from seeing clearly what you *already* value.

| System | Function | Example |
|--------|----------|---------|
| Moral code | Tells you what is right and wrong | "Do not steal" |
| Ethical framework | Provides principles for deciding what is right | "Act so that your action could be universal law" |
| **Dokkodo** | Removes perceptual distortions so you can see what is right | "Accept things exactly as they are" — *then* decide |

Precept 19 makes this explicit: "Respect Buddha and the Gods without counting on their help." Acknowledge the sacred, but do not outsource agency. The warrior sees clearly and acts — does not pray and wait.

### 7.2 Mindset and the Lazy Universe

hKask's grounding principle is the Principle of Least Action (§0 of [`PRINCIPLES.md`](../architecture/core/PRINCIPLES.md)): physical systems evolve through paths that minimize (or make stationary) action. This is not a tendency — it is the selection mechanism that chooses which path reality takes.

> **Interpretive framework (author's contribution):** The Dokkodo identifies specific sources of *perceptual action* — the "distance" the mind travels when distorted by attachment, preference, resentment, or fear. These are not established physical quantities; they are a proposed taxonomy for reasoning about how mindset shapes the perceived action landscape.

| Perceptual action type | Definition | Addressed by |
|------------------------|------------|-------------|
| **Attachment-action** | Energy spent maintaining what you cling to | Precepts 5, 14 |
| **Preference-action** | Energy spent distinguishing between artificially differentiated options | Precept 11 |
| **Resentment-action** | Energy spent wishing the past were different | Precepts 6, 9 |
| **Fear-action** | Energy spent avoiding paths that fear makes look longer than they are | Precept 17 |
| **Custom-action** | Energy spent conforming to patterns with no justification beyond familiarity | Precept 15 |

When these are removed, the true action landscape becomes visible — and pragmatic-laziness can find the genuine δS = 0 configuration. This claim is the central hypothesis of the Dokkodo-pragmatic-laziness composition; empirical verification is a deferred research question.

### 7.3 Jonathan Hall's "American Ronin"

Hall reads the Dokkodo as a **resilience discipline for the displaced**. The ronin — the masterless samurai — is the archetype of the warrior who has lost their place in the social order but must continue to act with clarity, honor, and effectiveness. Hall connects this to the modern condition: the individual navigating institutions they cannot control, markets they cannot predict, and circumstances they did not choose.

This maps to the agent's situation in hKask:

| Ronin condition | Agent analog |
|----------------|-------------|
| No feudal lord to provide direction | No human operator for autonomous operation periods |
| Must maintain honor without external validation | Must maintain P12 (no anonymous agency) without constant human oversight |
| Hostile or indifferent environment | Adversarial input, constraint violations, error states |
| Cannot control circumstances | Cannot control user input, external API behavior, system failures |
| **CAN control perception and response** | **CAN apply the Dokkodo lens to see clearly and act with minimum wasted motion** |

This interpretation suggests the Dokkodo should be **especially activated during error states and adversarial conditions** — the Resilience Chain composition pattern (§4).

### 7.4 The Precept 16 Self-Test

During the essentialist review of this skill's design, Precept 16 ("Do not collect weapons or train with weapons beyond what is useful") was applied as a self-test: *does this skill itself violate the precept?*

The original design had three templates. The essentialist deletion test eliminated two:
- A separate `dokkodo-flow.yaml` FlowDef — deleted; its loop pattern duplicated `pragmatic-laziness-flow.j2`
- A separate `dokkodo-calibrate.j2` KnowAct — deleted; its convergence logic was folded into `dokkodo-perceive.j2`

One template remains. The skill is the **minimum useful weapon** — it encodes genuine behavioral constraint that would vanish on deletion but adds no structural overhead beyond what's necessary.

---

## 8. Design Decisions

### 8.1 KnowAct, Not WordAct

The Dokkodo produces a structured *judgment* (clarified perception, alignment scores, distortion identification), not an *utterance*. A WordAct persona would influence tone; a KnowAct lens transforms the data that downstream processes consume. The `falstaffian-perspective` skill provides precedent: a KnowAct that applies a perspective transform.

### 8.2 Separate Skill, Not Baked Into Pragmatic-Laziness

Layer separation. Pragmatic laziness evaluates structures (δS = 0). The Dokkodo evaluates perceptions (δP = 0). Different contracts, different convergence criteria, different lexicon terms. The composition pattern is cleaner than a monolithic hybrid.

### 8.3 Embedded Convergence, Not a Separate Calibrate Template

The `pragmatic-laziness-flow.j2` template embeds its convergence check inline. The Dokkodo follows the same pattern. If future use cases require iterative deepening with separate calibration, a `dokkodo-calibrate.j2` can be extracted — but Precept 16 says don't create it until it's useful.

### 8.4 Four Clusters, Not 21 Individual Checks

21 individual precept checks would produce excessive output for minimal additional discrimination. The four clusters are functionally coherent groups. Individual precept alignment scores are still produced — the clustering is for the process (which questions to ask), not the output.

---

## 9. Open Questions

### 9.1 Persona vs. Skill

The Dokkodo could be an **always-on persona** — a perpetual perceptual filter wrapping all agent cognition — rather than an on-demand skill. Musashi didn't "apply the Dokkodo when he needed clarity"; he lived it.

**Arguments for persona:** Consistent perceptual posture. No activation overhead.
**Arguments for skill:** Established composability mechanism. Respects Precept 16 (don't apply weapons where not useful).
**Stance:** Skill for v0.30.0. Revisit if usage patterns show consistent demand for always-on mode.

### 9.2 Regulation Integration

Should perceptual calibration emit Regulation spans? A `RegulationSpan::PerceptualCalibration` could track precept alignment scores over time, shift magnitude trends, and activation frequency. Precept 4 ("think seriously of the world") suggests the Regulation should take perceptual calibration seriously.

**Stance:** Deferred. Requires Regulation span registry extension (v0.31+).

### 9.3 Error-State Auto-Activation

The American Ronin pattern suggests automatic Dokkodo activation on Regulation error escalation. This requires changes to the manifest executor and Regulation rule engine.

**Stance:** Manual invocation for v0.30.0. Auto-activation is v0.31+.

### 9.4 Precept Conflict Resolution

When precepts appear to conflict, the current template flags the conflict. A resolution protocol could apply Precept 21 as tiebreaker, use pragmatic-semantics constraint hierarchy, and weight clusters by domain.

**Stance:** Flag and escalate to human. Automated resolution is v0.31+.

### 9.5 Refined Perceptual Action Measurement

The current `shift_magnitude` scalar is coarse. A refined measurement could decompose it by cluster (`shift_magnitude.perceptual_reset`, `.desire_management`, `.emotional_resilience`, `.existential_posture`), enabling targeted invocation and better Regulation tracking.

**Stance:** Single scalar is sufficient for v0.30.0. Decomposition deferred.

---

## Appendix: Quick Reference

**The loop in one sentence:** *Clear the perceptual field by identifying and removing attachment, preference, resentment, and fear — then evaluate the clarified landscape.*

**Governing precept:** Precept 1 — *Accept things exactly as they are.* Every other precept serves this one. If you can truly accept the situation as it is — without wishing it were different, without resenting how it got this way, without clinging to what it used to be — then perception is clear. From clarity comes correct action.

**Meta-precept:** Precept 21 — *Never stray from the Way.* When precepts conflict, the Way decides. The Way is the path that minimizes total action across all 21 precepts — the true least-action path, not the one that looks easiest from any single precept's perspective.

**Lexicon terms:** `accept`, `assess`, `clarify`, `classify`, `converge`, `detach`, `discriminate`, `endure`, `evaluate`, `perceive`, `reflect`, `relinquish`, `renounce`

---

*"Accept things exactly as they are."* — Miyamoto Musashi, Dokkodo, Precept 1
*"Don't just do something, stand there."* — hKask PRINCIPLES.md, §0
