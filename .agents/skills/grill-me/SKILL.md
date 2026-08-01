---
name: grill-me
visibility: public
description: "Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis.
"
---

# Grill Me

Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis.


## When to Use

- When testing deep understanding of a topic through Socratic interrogation with escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis).
- When calibrating a user's baseline knowledge on a specific topic.
- When dynamically adapting question difficulty based on answer quality ratios.
- When probing for knowledge gaps and challenging assumptions during an oral examination.
- When synthesizing a final gap analysis with per-area ratings and prioritized study recommendations.

## Instructions

### grill-me-round

1. Conduct a rigorous oral examination on the specified topic.
2. Generate 2-3 questions at the current difficulty level, adhering to the question taxonomy (Recall, Mechanism, Rationale, Edge Cases, Synthesis).
3. If mode is "calibrate", assess the user's baseline knowledge directly and precisely.
4. If mode is "interrogate", evaluate previous answers to escalate if solid (≥80%), probe deeper if partial (40-80%), or re-probe with different angles if gaps exist (<40%).
5. After 3 failed attempts on a question, briefly explain the correct answer and move on.
6. Maintain a direct, sharp tone akin to a demanding technical interviewer, using specific challenging phrases without being mean-spirited.
7. Give minimal hints if requested, without solving the questions for the user.
8. Output a JSON object containing questions, evaluations, current level, and round verdict.

### grill-me-assess

1. Synthesize a comprehensive final knowledge assessment for the specified topic.
2. Rate each knowledge area as Solid, Partial, or Gap based on all answers and running assessment.
3. Formulate specific study recommendations, prioritized by impact with the most critical gaps first.
4. Do not sugarcoat gaps, but avoid being demoralizing.
5. Output a JSON object containing the summary, recommendations, and overall assessment.

### grill-me-escalate

1. Determine the next difficulty action based on the current level and solid answer ratio.
2. Escalate to the next level (maximum 5) if the solid ratio is 0.8 or higher.
3. Hold at the current level to probe deeper if the solid ratio is between 0.4 and 0.8.
4. Reprobe at the current level with different angles if the solid ratio is below 0.4.
5. Output a JSON object containing the new level, action, and a brief reason.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `grill-me-round.j2` | KnowAct | Generate interrogation questions at the appropriate difficulty level, evaluate answers, and decide whether to escalate, hold, or re-probe.  |
| `grill-me-assess.j2` | KnowAct | Synthesize final assessment with per-area ratings (Solid/Partial/Gap) and prioritized study recommendations.  |
| `grill-me-escalate.j2` | KnowAct | Decide whether to escalate difficulty, hold, or re-probe based on answer quality ratio.  |

## Constraints

- `grill-me-round.j2`: Public.
- `grill-me-assess.j2`: Public.
- `grill-me-escalate.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
