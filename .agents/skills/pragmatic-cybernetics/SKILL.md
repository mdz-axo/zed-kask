---
name: pragmatic-cybernetics
core: true
description: Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.
---

# Pragmatic Cybernetics

Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.

## When to Use

- Analyze a feedback loop's health across five properties (polarity, delay, gain, closure, fidelity) to diagnose failures and prescribe targeted remediation.
- Evaluate variety balance using Ashby's Law of Requisite Variety to identify deficits and recommend attenuation or amplification strategies.
- Map hKask components to Viable System Model (VSM) S1–S5 subsystems to assess overall system viability and identify unviable components requiring structural intervention.

## When NOT to Use

- Classifying statements (IS/OUGHT, constraint force) — use `pragmatic-semantics`; this skill analyzes loops and variety, not sentences.
- Operating the regulation loops — `algedonic-review` (alert triage and its gemba walk) runs them; this skill diagnoses their design.
- Implementing control systems — it is an analysis lens (VSM, Ashby, loop properties), not a builder.

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

### Convergence

1. Gate — call `lisp_eval` with:
   - form: `(cond ((> broken_properties 0) "broken_loop") ((< regulator_variety system_variety) "variety_deficit") ((not algedonic_channel) "unviable") (t "viable"))`
   - env: `{ "broken_properties": <loop properties rated broken or none>, "regulator_variety": <counted response classes>, "system_variety": <counted disturbance classes>, "algedonic_channel": <true if the S1 → S5 channel exists and is unblocked> }`
   The three analyses are P (judgment against evidence, each grounded in named hKask mechanisms, critiqued by the operator); the gate over their counted outputs is D. On anything but `viable`, re-run the failing analysis once with its remediation applied or the missing evidence gathered; a second failure is reported as the diagnosis, not iterated.

## Reference models

Ashby, *An Introduction to Cybernetics* (1956) — requisite variety; Conant & Ashby, "Every good regulator of a system must be a model of that system" (1970); Beer, *Brain of the Firm* (1972) — the Viable System Model.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `cybernetics-analyze-loop.j2` | Analyze a feedback loop on 5 properties: polarity, delay, gain, closure, fidelity. Diagnose broken loops and prescribe remediation. |
| `cybernetics-variety-check.j2` | Evaluate variety balance using Ashby's Law of Requisite Variety. Identify deficits, recommend attenuation or amplification strategies. |
| `cybernetics-vsm-map.j2` | Map hKask components to VSM S1–S5. Assess system viability and identify unviable components requiring structural intervention. |

To render a template, call the `render_template` tool with the template ref (e.g., `pragmatic-cybernetics/cybernetics-analyze-loop`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `cybernetics-analyze-loop.j2`: `loop_description`,`system_context`
- `cybernetics-variety-check.j2`: `loop_analysis`,`system_context`
- `cybernetics-vsm-map.j2`: `loop_analysis`,`variety_result` `system_context`


## Constraints

- `cybernetics-analyze-loop.j2`: Public. Every property assessment must be grounded in evidence. Broken/none property → broken loop. Remediation must name specific mechanisms. No external monitoring stacks (Prometheus, Grafana) — hKask is headless.
- `cybernetics-variety-check.j2`: Public. Every recommendation must reference a concrete hKask mechanism. Algedonic thresholds follow the runtime rule (`RuntimeAlert::new`, `hkask-regulation/src/algedonic.rs`): Warning when deficit > threshold/2, Critical when deficit > threshold; the deficit is counted in distinct classes. Critical status requires explicit escalation directive.
- `cybernetics-vsm-map.j2`: Public. Every component maps to exactly one primary subsystem. Missing/blocked algedonic channel (S1 → S5) → unviable (non-negotiable). S4 must have spec-drift detection. S5 must reference Magna Carta principles.
- Convergence check incorporates all three analysis steps (loop analysis, variety assessment, VSM mapping), not just loop analysis alone — defined in the Convergence section above.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
