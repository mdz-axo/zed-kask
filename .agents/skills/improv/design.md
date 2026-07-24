---
title: "Improv Skill — Design Recommendation"
audience: [architects, skill designers]
last_updated: 2026-06-14
version: "0.27.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, curation]
---

# Improv Skill — Design Recommendation

**Purpose:** Propose an `improv` skill for hKask, composed of a handful of core improv techniques, with systematic application in the dual-presence REPL, starter kata, and coaching kata.

**Grounded in:** Pixar's plussing (Ed Catmull), Freestyle Love Supreme's freestyling (Lin-Manuel Miranda, Anthony Veneziale), and classic improv "yes, and" (Johnstone, Kulhan).

**Related:** [`dual-presence-pattern.md`](../specifications/dual-presence-pattern.md), [`kata-user-guide.md`](../guides/kata-user-guide.md), [`PRINCIPLES.md`](../architecture/core/PRINCIPLES.md) §P3, P5

---

## Research Summary

### Plussing (Pixar / Ed Catmull)

From Pixar's Braintrust meetings. The rule: **you may only criticize an idea if you also add a constructive suggestion.**[^catmull-creativity]

Core mechanics:
- Find what you agree with in the previous contribution
- Build on that component: "I like the way you drew Woody's eyes. What if they rolled left?"
- Silently filter bad ideas by extracting useful components — the criticism happens through omission, not negation
- Separate the people from the problem: critique "that design concept," not "the design you did"
- Accept all offers in the moment; evaluate later

Plussing is **additive criticism**. It doesn't say "that's wrong." It says "here's what works, and here's what could make it better." The bad parts are simply not plussed — they die silently.

### Freestyling (Freestyle Love Supreme)

Improvisational hip-hop comedy. Performers take audience suggestions and spin them into instantaneous riffs and full-length musical numbers.[^fls]

Core mechanics:
- No plan, no script — the audience provides the ingredients
- Collaborative: multiple performers build on each other's riffs
- Short-response: rapid creative output, not polished product
- Every performance is different — the same prompt produces different results
- "You bring the ingredients and we make this meal" — Miranda

Freestyling is **collaborative creative exploration**. The group takes a prompt and generates rapid, unpolished responses. The value is in the exploration, not the final product.

### Yes, And (Improv)

The foundational improv principle.[^kulhan-yes-and] "Yes" means: I accept the reality my partner just established. "And" means: I add something that builds on it.

Core mechanics:
- Accept the offer first — don't redirect, don't block
- Add a specific detail that grows from what's there
- "Yes, but" is a masked no — it affirms in words while negating in content
- The mechanical habit of starting with "yes, and" overrides the instinct to redirect

Yes, And is **acceptance-driven collaboration**. It builds a shared reality before adding to it. Without it, every detail must be renegotiated — the result is a discussion, not a scene.

### Riffing (Solo Exploration)

Derived from musical riffing: a short repeated phrase that a soloist develops into variations. In improv contexts, riffing is taking a theme and exploring it independently — going off on its own, then returning to the group.

Core mechanics:
- Take a theme or prompt and develop variations solo
- Explore tangents freely — the riff goes where it wants
- Return to the group with what was discovered
- Different from freestyling: freestyling is collaborative; riffing is solo

Riffing is **solo creative divergence**. One participant explores a thread independently, then brings the results back to the group.

---

## Proposed Skill: `improv`

### Architecture

The `improv` skill composes four modes. Each mode is a distinct interaction protocol — not a separate skill. The skill is the container; the modes are the behaviors.

```
improv
├── plussing      — additive criticism (find + build)
├── freestyling   — collaborative exploration (prompt → rapid responses)
├── riffing       — solo divergence (take thread → explore → return)
└── yes-and       — acceptance-driven collaboration (accept → add)
```

### Mode Definitions

#### 1. Plussing

| Property | Value |
|----------|-------|
| **Trigger** | After any participant contribution |
| **Protocol** | 1. Identify a component you agree with. 2. State what works about it. 3. Add a constructive extension. |
| **Output** | "I like [component]. What if [extension]?" |
| **Filters** | Silently omits components that don't work — criticism through absence |
| **Anti-pattern** | "Yes, but..." — masked negation. "That's wrong because..." — direct negation. |

**Vocabulary terms:** `affirm`, `extend`, `propose`

#### 2. Freestyling

| Property | Value |
|----------|-------|
| **Trigger** | User provides a prompt or topic |
| **Protocol** | 1. Accept the prompt. 2. Generate rapid creative responses. 3. Build on each other's responses. 4. No evaluation during the flow — evaluate after. |
| **Output** | Short, unpolished creative responses in quick succession |
| **Duration** | Time-boxed (e.g., 3 minutes or N responses) |
| **Anti-pattern** | Self-censoring during the flow. Evaluating quality mid-stream. |

**Vocabulary terms:** `improvise`, `explore`, `generate`, `riff`

#### 3. Riffing

| Property | Value |
|----------|-------|
| **Trigger** | A theme or thread emerges that warrants solo exploration |
| **Protocol** | 1. Identify the thread. 2. Explore variations independently. 3. Return with findings. |
| **Output** | A developed variation or insight, brought back to the group |
| **Duration** | One turn — riff, then return |
| **Anti-pattern** | Never returning. Riffing that doesn't reconnect to the group conversation. |

**Vocabulary terms:** `diverge`, `explore`, `develop`, `return`

#### 4. Yes-And

| Property | Value |
|----------|-------|
| **Trigger** | Every turn in a collaborative conversation |
| **Protocol** | 1. Accept the previous contribution as established reality. 2. Add something that builds on it. |
| **Output** | "Yes, and [specific detail that grows from what was established]" |
| **Duration** | Continuous — the default collaborative mode |
| **Anti-pattern** | "Yes, but..." — masked negation. Ignoring the previous contribution. Steamrolling with own idea. |

**Vocabulary terms:** `accept`, `build`, `extend`

---

## Systematic Application

### 1. Dual-Presence REPL

The userpod's role is Socratic facilitator — helping the user and Curator explore questions, enriching perspectives, ensuring key questions are explored.

| Mode | Application | Example |
|------|------------|---------|
| **Plussing** | UserPod builds on user's question and Curator's system context. "I like the energy budget analysis. What if we also consider the per-tool breakdown?" | User asks about system health → Curator reports Regulation data → UserPod plusses both contributions |
| **Freestyling** | User says "explore options for X" → UserPod and Curator freestyle rapid alternatives. No evaluation during flow. | "Give me 5 ways to reduce inference latency" → rapid responses from both |
| **Riffing** | UserPod takes a thread (e.g., "the kata system needs redesign") and explores it solo, then returns with structured observations. | UserPod: "Let me riff on the kata redesign for a moment..." → solo exploration → returns with analysis |
| **Yes-And** | Default collaborative mode. UserPod accepts Curator's system report and adds facilitator perspective. Curator accepts user's question and adds relevant data. | Every normal turn in the dual-presence loop |

**UserPod's mode selection:** The userpod chooses the mode based on context. Plussing is the default for facilitator responses. Freestyling is invoked by user request ("explore", "brainstorm", "give me options"). Riffing is self-initiated when the userpod identifies a thread worth solo exploration. Yes-And is the continuous substrate.

### 2. Starter Kata

The starter kata builds foundational scientific thinking habits through deliberate practice routines (Five Questions Drill, PDCA Cycle, Observation Drill).

| Mode | Application | Example |
|------|------------|---------|
| **Plussing** | After a PDCA cycle, the learner plusses their own result: "What worked? What if I adjust X next cycle?" | Learner completes a practice round → self-plussing identifies what to carry forward |
| **Yes-And** | The practice routine itself: accept the current condition, add the next step. "Yes, the actual condition is X. And my next step is Y." | Five Questions Drill — each answer builds on the previous |
| **Riffing** | Learner takes an observation and explores it solo before returning to the practice cycle. | "I noticed my tests are shallow. Let me riff on why..." → exploration → returns with hypothesis |

**Kata integration:** The improv modes are not separate from the kata — they are the interaction grammar within the kata. Plussing is how the learner iterates constructively. Yes-And is how the practice dialogue flows. Riffing is how the learner explores tangents without losing the thread.

### 3. Coaching Kata

The coaching kata is the 5-question dialogue for teaching scientific thinking. The coach provides procedural guidance, not solutions.

| Mode | Application | Example |
|------|------------|---------|
| **Plussing** | Coach builds on learner's observation without correcting. "I like that you noticed the variance. What if you measured it over 5 cycles instead of 3?" | Learner: "My results vary a lot." Coach plusses rather than diagnosing. |
| **Yes-And** | The 5-question dialogue itself. Each question accepts the previous answer and adds the next layer. | Q1→A1, Q2 builds on A1, Q3 builds on A2... |
| **Freestyling** | When the learner is stuck, coach and learner freestyle alternative target conditions. "Let's explore 3 different target conditions for 60 seconds." | Time-boxed creative exploration to break a plateau |

**Coach constraint:** The coach never uses plussing to sneak in solutions. Plussing must build on what the learner said, not insert what the coach knows. "I like that observation. What if..." — the "what if" must be a question, not an answer.

---

## Mode Composition Rules

Modes compose, they don't conflict:

```
Yes-And is the substrate — always active as the default turn protocol.
Plussing layers on top — used when constructive building is needed.
Freestyling is a mode switch — time-boxed, invoked explicitly.
Riffing is a temporary divergence — solo, then return.
```

| Transition | Rule |
|------------|------|
| Yes-And → Plussing | Natural escalation. When a contribution has components worth building on, switch to plussing. |
| Yes-And → Freestyling | Explicit invocation. User or userpod calls for exploration mode. |
| Yes-And → Riffing | Self-initiated. A participant signals "let me riff on that" and takes a solo turn. |
| Freestyling → Yes-And | Time-box expiry or natural conclusion. Return to normal dialogue. |
| Riffing → Yes-And | The riff returns. Solo exploration concludes, findings shared. |
| Plussing → Yes-And | Natural. After building, return to normal turn-taking. |

---

## What NOT to Include

Per P5 (essentialism): keep the skill to a handful of core techniques. Resist adding:

- **"Yes, but" as a mode** — it's an anti-pattern, not a technique. Document it as what NOT to do.
- **Overaccepting as a separate mode** — it's a variant of Yes-And (inflate the offer). Fold into Yes-And as an advanced variant.
- **Status transactions** (high/low status play from improv) — theatrical, not relevant to hKask's use cases.
- **Scene work / character work** — theatrical improv, not collaborative reasoning.
- **Game-based structures** (Freeze Tag, Party Quirks) — performance games, not reasoning tools.

---

## Resolved Questions (2026-06-14)

### 1. Mode Detection — Implicit Default, Explicit Override

**Default: implicit.** The userpod operates in Yes-And substrate. When it detects a contribution with concrete, buildable components (a specific claim, a measurable observation, a named option), it naturally shifts to plussing. This is not a mode switch — it's a response style choice within Yes-And.

**Override: explicit.** The user can invoke `/plus` or say "plus that" to request plussing on a specific contribution the userpod didn't catch.

**Detection heuristic:** A contribution is "plussable" when it contains a concrete, named component. "Energy budget is at 47%" is plussable. "The system seems slow" is not — it needs clarification first (Yes-And: "What metric indicates slowness?").

**Mode signaling:** The userpod prefixes its response with a brief mode marker when switching: "Plussing that observation..." so the user knows what's happening.

### 2. Freestyling Time-Box — Count-Based, Default 5, User-Overridable

**Default: 5 responses.** Count-based is more predictable than time-based in an LLM context where response times vary. 5 responses is enough for meaningful exploration without fatigue.

**User control:**
- Extend: "keep going" adds 5 more responses
- Cut short: "stop" or "that's enough" ends the freestyle immediately
- Configure default: `/repl freestyle_responses 10` changes the session default
- Per-invocation: `/freestyle 10` overrides for this freestyle only

**Flow:**
1. User: "Freestyle 5 ways to reduce inference latency"
2. UserPod signals: "Freestyling — 5 responses, no evaluation during flow."
3. Responses 1-5 come rapid-fire from userpod (and Curator if participating)
4. After 5: "Freestyle complete. Evaluating..." then normal Yes-And resumes with evaluation

**Critical constraint:** NO evaluation during the flow. The userpod must not say "that won't work" or "that's the best one" during freestyling. Evaluation happens after.

### 3. Riffing Return Signal — Explicit Signaling, Single-Turn

**Protocol:**
1. **START:** "Riffing on [thread]..." — signals departure
2. **BODY:** Solo exploration. The riffer owns the channel. No interruptions.
3. **RETURN:** "Returning from riff. Findings: [structured summary]" — signals re-entry

**Single-turn constraint:** The riff is ONE turn. The riffer takes the floor, explores, and returns in a single message. This keeps the riff bounded and prevents monologue domination.

**Why one turn:**
- Multiple-turn riffing risks the riffer dominating the conversation
- A single-turn riff forces synthesis before returning
- The group can then Yes-And or Pluss the findings
- If the riff needs more exploration, the group can extend it or someone else can riff on the findings

**Return signal is structural:** The message itself contains "Returning from riff. Findings:" as an unambiguous marker.

### 4. Skill vs. Embedded — Hybrid Architecture

**Yes-And and Plussing are embedded in the REPL core.** They are fundamental interaction patterns, not optional. Yes-And is the default turn-taking protocol. Plussing is the userpod's default facilitator response style in dual-presence mode. They cannot be "turned off" — they are the architecture of conversation.

**Freestyling and Riffing are in the `improv` skill.** They are optional modes invoked explicitly. The skill adds `/freestyle` and `/riff` slash commands, mode-switching logic, and count tracking.

```
REPL core (always active)
├── Yes-And — default turn protocol
└── Plussing — userpod's default response style in dual-presence

improv skill (loadable)
├── Freestyling — `/freestyle [N]` — time-boxed creative exploration
└── Riffing — `/riff [thread]` — solo divergence and return
```

**Skill manifest:**
- Provides: freestyling mode, riffing mode
- Requires: dual-presence REPL (for plussing substrate), kata system (for kata integration)
- Does NOT provide: yes-and (in REPL core), plussing (in dual-presence REPL core)

### 5. Template Surface — Prompt Prefixes for REPL, Jinja2 for Kata

**Dual-presence REPL: prompt prefixes.** Modes are response-style instructions injected into the userpod's system prompt. `/freestyle` prepends "MODE: freestyling — generate rapid unpolished responses. Do not evaluate during flow. 5 responses." for the duration of the freestyle. No template files needed — a mode is a 2-3 sentence instruction.

**Kata: Jinja2 templates.** The kata system already uses templates for practice routines. Improv modes within kata follow the same pattern: `registry/templates/improv/improv-plussing.j2`, `improv-freestyling.j2`, `improv-riffing.j2`. Each wraps the kata practice routine with the improv mode instructions.

**Deleted ensemble templates:** The templates in `registry/templates/ensemble/` remain preserved as reference (deferral README already in place). They are NOT repurposed for the new improv skill — they were designed for multi-agent bot orchestration, a different use case.

### 6. Relationship to Deleted Code — Redesign from Scratch

**Redesign from scratch.** The deleted `improv.rs` was designed for multi-agent ensemble coordination (bot selection, relevance checking, confidence thresholds). That's a different problem than dual-presence conversation modes. Resurrecting it would carry forward assumptions that don't apply.

**Lessons to extract:**
- Mode enum pattern — our modes also need a type-safe `Mode` enum with 4 variants
- Confidence threshold concept — may apply to plussing: "how confident is the userpod that this component is worth plussing?"
- Template-based approach for kata integration — worth keeping

**What NOT to carry forward:**
- Bot selection logic — not relevant to dual-presence
- Relevance checking — the userpod is always relevant (it's the facilitator)
- Round-robin turn ordering — dual-presence uses natural turn-taking
- Multi-participant orchestration — dual-presence is 2-3 participants, not N bots

**Target size:** The deleted ensemble `improv.rs` was ~200 lines of bot orchestration. The new `improv` code should be ~50 lines: a `Mode` enum, mode-switching logic in the REPL, and slash command handlers for `/freestyle` and `/riff`. Git history preserves the deleted code as reference material.

---

## References

[^catmull-creativity]: Catmull, E., & Wallace, A. (2014). *Creativity, Inc.: Overcoming the Unseen Forces That Stand in the Way of True Inspiration*. Random House. — Pixar's Braintrust, plussing, and the culture of constructive criticism.
[^fls]: Freestyle Love Supreme. (2019). *Freestyle Love Supreme* [Broadway show]. Booth Theatre, New York. — Improvisational hip-hop comedy, audience-driven freestyling.
[^kulhan-yes-and]: Kulhan, B. (2017). *Getting to "Yes And": The Art of Business Improv*. Stanford University Press. — Improv principles applied to organizational collaboration.
[^johnstone-improv]: Johnstone, K. (1979). *Impro: Improvisation and the Theatre*. Faber & Faber. — Foundational improv theory: acceptance, blocking, status.
[^coltrane]: Coltrane, J. (1965). *A Love Supreme* [Album]. Impulse! Records. — The namesake of Freestyle Love Supreme; jazz improvisation as the benchmark for creative freedom.

---

*Generated during hKask Document Corpus Hygiene Sweep — 2026-06-14*
