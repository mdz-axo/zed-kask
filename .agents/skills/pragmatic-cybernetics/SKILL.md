---
name: pragmatic-cybernetics
visibility: public
description: "Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.
"
---

# Pragmatic Cybernetics

Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.


## When to Use

- Analyze a feedback loop's health across five properties (polarity, delay, gain, closure, fidelity) to diagnose failures and prescribe targeted remediation.
- Evaluate variety balance using Ashby's Law of Requisite Variety to identify deficits and recommend attenuation or amplification strategies.
- Map hKask components to Viable System Model (VSM) S1–S5 subsystems to assess overall system viability and identify unviable components requiring structural intervention.

## Instructions

### cybernetics-analyze-loop

1. Identify the loop's sensing mechanism (what is measured).
2. Identify the decision mechanism (how the measurement is interpreted).
3. Identify the action mechanism (what changes as a result).
4. Trace the return path (how the action's effect is sensed again).
5. Assess each of the 5 properties (polarity, delay, gain, closure, fidelity) against healthy, degraded, or broken criteria.
6. Diagnose failures explicitly if any property is rated "broken" or "none".
7. Prescribe targeted remediation steps that name specific mechanisms, parameters, or code paths.

### cybernetics-variety-check

1. Enumerate the distinct disturbance classes the system can produce.
2. Enumerate the distinct response classes the regulator can produce.
3. Compare regulator variety against system variety and quantify the deficit if regulator variety is insufficient.
4. Propose attenuation strategies to reduce system variety for each identified deficit.
5. Propose amplification strategies to increase regulator variety for each identified deficit.
6. Reference concrete hKask mechanisms (Regulation spans, crates, data structures, or configuration parameters) for all recommendations.

### cybernetics-vsm-map

1. Identify which hKask components belong to each S1–S5 subsystem for the focus area.
2. Verify anti-oscillatory S2 channels exist between S1 units.
3. Verify S3 has both monitoring and resource-allocation paths to S1.
4. Verify S4 has spec-drift and algedonic sensing capability.
5. Verify S5 has clear policy that S3/S4 can reference.
6. Verify the algedonic channel (S1 → S5 direct) exists and is not blocked.
7. Assess overall system viability (viable, degraded, or unviable) based on the mapping.
8. Identify unviable components and define the required structural interventions for viability.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `cybernetics-analyze-loop.j2` | KnowAct | Analyze a feedback loop on 5 properties: polarity, delay, gain, closure, fidelity. Diagnose broken loops and prescribe remediation.  |
| `cybernetics-variety-check.j2` | KnowAct | Evaluate variety balance using Ashby's Law of Requisite Variety. Identify deficits, recommend attenuation or amplification strategies.  |
| `cybernetics-vsm-map.j2` | KnowAct | Map hKask components to VSM S1–S5. Assess system viability and identify unviable components requiring structural intervention.  |

## Constraints

- `cybernetics-analyze-loop.j2`: Public.
- `cybernetics-variety-check.j2`: Public.
- `cybernetics-vsm-map.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
