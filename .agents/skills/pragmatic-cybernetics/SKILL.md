---
name: pragmatic-cybernetics
core: true
description: Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.
audience: curator
---

# Pragmatic Cybernetics

Cybernetic reasoning framework for hKask's Regulation. VSM mapping, feedback loop analysis (5 properties), variety engineering (Ashby's Law), Good Regulator check, and spec drift as cybernetic signal.

## Initial and target condition

- **Initial condition:** the loop/system description and concrete evidence for sensing, decision, action and return paths; counted disturbance and response classes; the observed S1–S5 mapping and algedonic-channel state.
- **Target condition:** the convergence gate returns `viable`: no broken/none loop property, the algedonic channel is open, and regulator variety is at least system variety. Any other verdict re-enters the named failing analysis once with remediation or missing evidence applied; a second failure is the reported diagnosis, not another iteration.

## When to Use

- Analyze a feedback loop's health across five properties (polarity, delay, gain, closure, fidelity) to diagnose failures and prescribe targeted remediation.
- Evaluate variety balance using Ashby's Law of Requisite Variety to identify deficits and recommend attenuation or amplification strategies.
- Map hKask components to Viable System Model (VSM) S1–S5 subsystems to assess overall system viability and identify unviable components requiring structural intervention.

## When NOT to Use

- Classifying statements (IS/OUGHT, constraint force) — use `pragmatic-semantics`; this skill analyzes loops and variety, not sentences.
- Operating the regulation loops — `algedonic-review` (alert triage and its gemba walk) runs them; this skill diagnoses their design.
- Implementing control systems — it is an analysis lens (VSM, Ashby, loop properties), not a builder.

## D/P labelling

VSM mapping, loop-property assessment, and variety-engineering proposals are
P — judgment over the observed evidence, critiqued by the Good Regulator check
(can this model of the system actually regulate it?), by measurement against
the recorded spec (spec drift is measured, not judged), and by the operator.
The gate `PC-1` is D: one `lisp_eval` form over the three analyses' outputs as
returned — it counts the broken loop properties, the enumerated classes and
the uncovered subsystems itself. A loop with no observable feedback path is an
open loop no matter how well it is described, and the verdict comes from the
form's output, never from the analyst's confidence.

## Instructions

### cybernetics-analyze-loop

1. Identify the loop's sensing mechanism (what is measured).
2. Identify the decision mechanism (how the measurement is interpreted).
3. Identify the action mechanism (what changes as a result).
4. Trace the return path (how the action's effect is sensed again).
5. Assess each of the 5 properties (polarity, delay, gain, closure, fidelity) against healthy, degraded, or broken criteria.
6. Diagnose failures explicitly if any property is rated at its broken value (`none`, `excessive` delay, `zero` gain, `open`, `low`).
7. Prescribe targeted remediation steps that name specific mechanisms, parameters, or code paths.

The variety check and the VSM map below both read only this loop analysis:
issue their two renders together.

### cybernetics-variety-check

1. Enumerate the distinct disturbance classes the system can produce (`disturbance_classes`).
2. Enumerate the distinct response classes the regulator can produce (`response_classes`).
3. List the disturbance classes actually observed in the evidence window (`observed_disturbance_classes`; empty for an idle window).
4. Propose attenuation strategies to reduce system variety for each identified deficit.
5. Propose amplification strategies to increase regulator variety for each identified deficit.
6. Reference concrete hKask mechanisms (Regulation spans, crates, data structures, or configuration parameters) for all recommendations.

The analysis names the classes; it does not count them, subtract or compare.

### cybernetics-vsm-map

1. Identify which hKask components belong to each S1–S5 subsystem for the focus area.
2. Verify anti-oscillatory S2 channels exist between S1 units.
3. Verify S3 has both monitoring and resource-allocation paths to S1.
4. Verify S4 has spec-drift and algedonic sensing capability.
5. Verify S5 has clear policy that S3/S4 can reference.
6. Verify the algedonic channel (S1 → S5 direct) exists and is not blocked (`algedonic_channel: boolean`).
7. Record where a present subsystem is weak (`degradation_notes`) and each unviable component with its required structural intervention.

### Gate (`PC-1`)

1. Call `lisp_eval` once, after all three analyses, with:
   - form: `(begin (define broken-value (list (list "polarity" "none") (list "delay" "excessive") (list "gain" "zero") (list "closure" "open") (list "fidelity" "low"))) (define broken-of (lambda (pairs) (if (is_null pairs) (list) (if (string= (assoc! (car (car pairs)) loop_analysis) (car (cdr (car pairs)))) (cons (car (car pairs)) (broken-of (cdr pairs))) (broken-of (cdr pairs)))))) (define empty-of (lambda (names) (if (is_null names) (list) (if (is_null (assoc! (car names) vsm_mapping)) (cons (car names) (empty-of (cdr names))) (empty-of (cdr names)))))) (let ((broken (broken-of broken-value)) (uncovered (empty-of (list "S1_operations" "S2_coordination" "S3_control" "S4_intelligence" "S5_policy"))) (system_variety (length disturbance_classes)) (regulator_variety (length response_classes)) (idle (= (length observed_disturbance_classes) 0))) (let ((deficit (- system_variety regulator_variety))) (let ((ashby (if idle "idle_no_signal" (if (<= deficit 0) "satisfied" (if (and (> critical_threshold 0) (> deficit critical_threshold)) "critical" "deficit"))))) (list (list "verdict" (cond ((not (is_null broken)) "broken_loop") ((or (not (eq algedonic_channel t)) (not (is_null uncovered))) "unviable") ((string= ashby "idle_no_signal") "idle_no_signal") ((> deficit 0) "variety_deficit") (t "viable"))) (list "broken_properties" broken) (list "uncovered_subsystems" uncovered) (list "system_variety" system_variety) (list "regulator_variety" regulator_variety) (list "deficit" deficit) (list "ashby_status" ashby))))))`
   - env: `{ "loop_analysis": <the analyze-loop output, unchanged>, "vsm_mapping": <the vsm-map output's vsm_mapping, unchanged>, "algedonic_channel": <the vsm-map output's algedonic_channel>, "disturbance_classes": <from the variety check>, "response_classes": <from the variety check>, "observed_disturbance_classes": <from the variety check>, "critical_threshold": <the domain's algedonic threshold from the regulation configuration; 0 when the domain has none> }`
   Pass each analysis output as returned; type no count.
2. Read `verdict`, in the form's clause order: `broken_loop` (a property at its broken value; `broken_properties` names them) → `unviable` (the algedonic channel is blocked, or `uncovered_subsystems` names an S1–S5 list with no component) → `idle_no_signal` (no disturbance class was observed, so the window shows nothing about variety health) → `variety_deficit` → `viable`.
3. `ashby_status` is reported beside the verdict. `critical` — a deficit above `critical_threshold` — requires an explicit escalation directive in the report (the runtime rule: `RuntimeAlert::new`, Critical when deficit > threshold).
4. On anything but `viable`, re-run the failing analysis once with its remediation applied or the missing evidence gathered, then run the gate again; a second non-`viable` result is reported as the diagnosis, not iterated. `idle_no_signal` is resolved by evidence from an active window, not by a re-run over the same window.

## Reference models

Ashby, *An Introduction to Cybernetics* (1956) — requisite variety; Conant & Ashby, "Every good regulator of a system must be a model of that system" (1970); Beer, *Brain of the Firm* (1972) — the Viable System Model. The five-property loop framework (polarity, delay, gain, closure, fidelity) is this project's own synthesis — a diagnostic decomposition informed by that control-theory literature, not a framework from a published source; it is labeled as such wherever it is taught.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `cybernetics-analyze-loop.j2` | Analyze a feedback loop on 5 properties: polarity, delay, gain, closure, fidelity. Diagnose broken loops and prescribe remediation. |
| `cybernetics-variety-check.j2` | Enumerate the disturbance classes, the regulator's response classes and the classes actually observed (Ashby's Law of Requisite Variety); recommend attenuation or amplification strategies. |
| `cybernetics-vsm-map.j2` | Map hKask components to VSM S1–S5, report the algedonic channel, and identify unviable components requiring structural intervention. |

To render a template, call the `render_template` tool with the template ref (e.g., `pragmatic-cybernetics/cybernetics-analyze-loop`) and a context object with the required variables.

Copyable `render_template` context blocks (variable names from each template's `[inference]` contract):

`pragmatic-cybernetics/cybernetics-analyze-loop`:
```json
{ "loop_description": "<the loop: what senses, decides, acts, and how the effect is sensed again>", "system_context": "<surrounding system, or an empty string>" }
```

`pragmatic-cybernetics/cybernetics-variety-check`:
```json
{ "loop_analysis": { "<the whole analyze-loop output>": "" }, "system_context": "<surrounding system, or an empty string>" }
```

`pragmatic-cybernetics/cybernetics-vsm-map`:
```json
{ "loop_analysis": { "<the whole analyze-loop output>": "" }, "system_context": "<surrounding system, or an empty string>" }
```

## Regression case

`PC-1` is evaluated on passing and failing cases by the
`scientific_gate_pc_1_` test
(`bash kask/scripts/check-skill-lisp-forms.sh`): a healthy loop with full
S1–S5 coverage, an open channel and matched variety is `viable`; one property
at its broken value is `broken_loop` and is named; a blocked algedonic channel
or an empty subsystem list is `unviable`, and neither is masked by a variety
deficit; an idle window is `idle_no_signal`, never `viable`; a shortfall is
`variety_deficit`, with `ashby_status` `critical` above the threshold and
`deficit` at or below it.

## Constraints

- `cybernetics-analyze-loop.j2`: Every property assessment must be grounded in evidence. A property at its broken value → broken loop. Remediation must name specific mechanisms. No external monitoring stacks (Prometheus, Grafana) — hKask is headless.
- `cybernetics-variety-check.j2`: Every recommendation must reference a concrete hKask mechanism. Algedonic thresholds follow the runtime rule (`RuntimeAlert::new`, `hkask-regulation/src/algedonic.rs`): Warning when deficit > threshold/2, Critical when deficit > threshold; the deficit is counted in distinct classes. An idle window (zero observed states) yields deficit 0 from `VarietyTracker::deficit` (`hkask-regulation/src/runtime.rs`, pinned by `deficit_is_zero_for_idle_window`) — read it as no signal, never as variety health; `PC-1` returns `idle_no_signal` for it. Critical status requires explicit escalation directive.
- `cybernetics-vsm-map.j2`: Every component maps to exactly one primary subsystem. Missing/blocked algedonic channel (S1 → S5) → unviable (non-negotiable). S4 must have spec-drift detection. S5 must reference Magna Carta principles.
- The gate incorporates all three analyses (loop analysis, variety assessment, VSM mapping), not loop analysis alone.
