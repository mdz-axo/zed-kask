---
title: "Improv Skill — Open Questions & Future Work"
audience: [agents, skill designers]
last_updated: 2026-06-14
version: "0.27.0"
status: "Draft"
domain: "Cross-cutting"
mds_categories: [composition, curation]
---

# Improv Skill — Open Questions & Future Work

> Captured 2026-06-14. Updated as experiments yield answers through kata cycles.

---

## 1. Mode Composition Algebra

**Question:** Can modes compose beyond sequential application? Can they nest?

**Current state:** Sequential composition only (e.g., Plussing → Riffing). The improv manifest declares four composition sequences.

**Open:**
- Can a Riffing tangent internally use Plussing to filter its own ideas?
- Can Freestyling sessions have an outer mode (e.g., "freestyle within Yes But constraints")?
- Is there a composition algebra (commutative? associative?) or is it always ordered?

**Experiment:** Run ensemble sessions with nested mode declarations; measure output coherence via `hkask.improv.ensemble.coherence`.

---

## 2. Regulation Metrics & Thresholds

**Question:** What are the healthy ranges for improv Regulation spans?

**Current state:** Five spans registered in `CANONICAL_NAMESPACES`. No threshold values defined.

**Open:**
- `hkask.improv.plussing.ratio` — What constructive ratio is "good"? 0.5? 0.7? Does it depend on conversation context?
- `hkask.improv.freestyle.coherence` — How do we measure emergent coherence without imposing a goal that contradicts freestyling's nature?
- `reg.kata.improv.effectiveness` — What delta in automaticity score is significant? 0.1? 0.3?
- At what threshold should an algedonic alert fire?

**Experiment:** Run starter kata with and without Plussing; measure automaticity score delta. Run coaching kata with and without Yes But; measure PDCA cycle completion rate. Use these baselines to set initial thresholds.

---

## 3. Additional Improv Techniques

**Question:** Are there other improv techniques that compose cleanly with the five core modes?

**Constraint:** The improv skill must remain ≤7 modes total (deep-module discipline).

**Candidates:**
- **Heightening** (UCB): Escalate the emotional or logical stakes of a scene. Could map to "Yes And" with intensity amplification.
- **If Then** (Second City): Conditional exploration — "if we assume X, then Y follows." Could map to Riffing with a conditional seed.
- **Status Transfer** (Johnstone): Shift relative status between participants. Could be a meta-mode rather than a content mode.

**Open:** Should there be a "meta-mode" where the agent itself selects the appropriate improv mode based on conversation context? This would be a 6th mode: `AutoSelect`.

---

## 4. UserPod Personality Integration

**Question:** How does improv mode interact with userpod persona (defined in `persona.yaml`)?

**Current state:** Persona constraints filter forbidden patterns from model output. Improv mode sets interaction posture. They operate independently.

**Open:**
- Does Plussing override a naturally critical persona, or compose with it?
- Should persona definitions include an `improv_affinity` field that weights mode effectiveness? (e.g., a "Socratic" persona has high affinity for Yes But, low for Freestyling)
- If persona forbids certain language and Plussing requires constructive language, do they conflict or reinforce?

**Experiment:** Define personas with explicit improv affinities; measure mode effectiveness by persona.

---

## 5. Empirical Validation Plan

**Question:** Does improv actually improve agent interaction quality?

**Proposed experiments (each is an Improvement Kata cycle):**

| Experiment | Independent Variable | Dependent Variable | Regulation Span |
|-----------|---------------------|-------------------|----------|
| Starter Kata + Plussing | Plussing ON vs OFF in Observation Drill | Automaticity score after 5 sessions | `reg.kata.improv.effectiveness` |
| Coaching Kata + Yes But | Yes But ON vs OFF in Q4 | Learner PDCA cycle completion rate | `reg.kata.improv.effectiveness` |
| Ensemble + Freestyling | Freestyling session vs unstructured chat | Idea count, idea novelty, participant satisfaction | `hkask.improv.ensemble.coherence` |
| Dual-presence + Plussing | Plussing as default vs no mode | User-reported conversation quality | `hkask.improv.plussing.ratio` |

**Meta-observation:** The improv skill is both the subject and the tool of its own validation — these experiments are themselves Improvement Kata cycles that can use improv modes during their execution.

---

## 6. Inference Integration

**Question:** When does Plussing switch from heuristic scoring to LLM-based semantic scoring via `hkask-inference`?

**Current state:** Heuristic keyword-based agreeableness detection in `plussing.rs`. Comment notes: "This is a placeholder for LLM-based semantic scoring via `hkask-inference`."

**Open:**
- What is the gas cost of LLM-based agreeableness scoring per turn?
- Is the accuracy improvement worth the gas cost?
- Should the switch be configurable (heuristic vs LLM) or automatic based on gas budget?

**Experiment:** Run Plussing with both heuristic and LLM scoring on the same inputs; measure agreement rate and gas cost delta.

---

## 7. Mode Switching Mid-Conversation

**Question:** Is mode switching mid-conversation coherent, or should mode be session-scoped?

**Current state:** The REPL `/improv` command allows switching at any time. The SKILL.md says "Mode switching mid-conversation is supported."

**Open:**
- Does rapid mode switching confuse the conversation partner?
- Should there be a "mode lock" for critical conversations (e.g., coaching kata sessions)?
- What Regulation span tracks mode transition frequency?

---

## 8. Freestyling Session Persistence

**Question:** Should freestyling sessions persist across REPL sessions?

**Current state:** `FreestyleSession` is in-memory only. Session state is lost on REPL exit.

**Open:**
- Should freestyling sessions be persisted to episodic memory?
- If persisted, can a session be resumed across REPL restarts?
- What is the Regulation span for session persistence?

---

## Resolution Log

| Date | Question | Resolution | Evidence |
|------|----------|------------|----------|
| — | — | — | — |

*(Populated as kata experiments yield answers.)*
