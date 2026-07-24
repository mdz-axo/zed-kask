---
name: bug-hunt
visibility: public
description: "Bug hunting: explores a target crate for threats to user-defined quality. Applies Weinberg's quality definition (\"value to some person who matters\"), Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters. Decomposed into phased templates: Charter → Probe (agent-coordinated) → Oracle → Taxonomize → Report. Reasoning patterns from pragmatic-semantics (IS/OUGHT + epistemic classification + provenance), pragmatic-cybernetics (feedback loop analysis + Good Regulator checks + variety engineering), diagnose, grill-me, and adversarial-red-team are embedded as inline prompt instructions in the oracle phase. Emits Regulation spans (reg.bughunt.*) for observability (P12). OCAP-gated: requires Tool:test:Execute and Tool:regulation:Read.
"
---

# Bug Hunt

Bug hunting: explores a target crate for threats to user-defined quality. Applies Weinberg's quality definition ("value to some person who matters"), Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters. Decomposed into phased templates: Charter → Probe (agent-coordinated) → Oracle → Taxonomize → Report. Reasoning patterns from pragmatic-semantics (IS/OUGHT + epistemic classification + provenance), pragmatic-cybernetics (feedback loop analysis + Good Regulator checks + variety engineering), diagnose, grill-me, and adversarial-red-team are embedded as inline prompt instructions in the oracle phase. Emits Regulation spans (reg.bughunt.*) for observability (P12). OCAP-gated: requires Tool:test:Execute and Tool:regulation:Read.


## When to Use

- When exploring a target crate for threats to user-defined quality criteria.
- When applying Weinberg's quality definition, Beizer's bug taxonomy, Bach/Bolton's heuristic test strategy model, and Hendrickson's exploratory testing charters.
- When decomposing bug hunting into phased templates: Charter → Probe → Oracle → Taxonomize → Report.
- When needing to compute convergence metrics for bug-hunt PDCA cycles (saturation detection with stability check).
- When needing to run a legacy monolithic expedition template (v0.30.0 backward compatibility).

## Instructions

### bug-hunt-charter

1. Generate a focused testing charter for exploring a target crate.
2. Pick the most promising strategy from Bach's Heuristic Test Strategy Model (Project Environment, Product Elements, Quality Criteria).
3. Target the most likely Beizer taxonomy categories given the target and quality criteria.
4. Be specific — name actual files, functions, or modules in the target.
5. Ensure probe instructions are actionable with available MCP tools.
6. Respond with a JSON object containing charter_statement, target_area, strategy, expected_category, beizer_focus, and probe_instructions.

### bug-hunt-probe

1. Format probe execution results.
2. Do NOT generate fictional findings; only report what the agent actually discovered through MCP tool usage.
3. If no probe results are available, explore the target with available MCP tools (`file:read`, `code:search`, `terminal`) using the charter's probe_instructions.
4. Search for bug patterns: `.unwrap()` / `.expect()` in library code, public functions without contracts, `unsafe` blocks without documented safety invariants, integer arithmetic without overflow protection, `clone()` calls that may hide ownership confusion, mutable state without synchronization, `panic!` or `todo!()` in non-startup code.
5. Apply diagnose pattern: reproduce before diagnosing, isolate one variable.
6. Apply adversarial pattern: try unexpected input orders, boundary values.
7. Apply cybernetic pattern: trace feedback loops, check Good Regulator, verify variety.
8. If probe_depth includes `dynamic` or `full` and charter targets timing, integration, or structural categories, use BugStalker (`bs`) for runtime inspection on Linux x86-64 debug builds.
9. For each runtime probe, record binary path, breakpoint locations, observed variable states, thread states, and whether the behavior matches expectations.
10. Collect all findings as structured text to feed into the oracle phase.

### bug-hunt-oracle

1. Evaluate raw probe findings against user-defined quality criteria.
2. Assign a tier and confidence to each finding (Tier 1: BUG, Tier 2: POTENTIAL_BUG, Tier 3: OBSERVATION).
3. Apply IS vs OUGHT classification: describe what the code DOES (IS) vs what it SHOULD do (OUGHT). Never present OUGHT as IS.
4. Label the epistemic mode for every finding (Declarative, Probabilistic, Subjunctive).
5. Identify the provenance of the finding (Direct measurement, Inference, Assessment).
6. Challenge your own verdict using grill-me self-challenge (Could this be intentional? Is there an edge case where this is correct? Would a reviewer dismiss this?).
7. If confidence is below 0.60, downgrade the finding to OBSERVATION.
8. Respond with a JSON object containing evaluated_findings, confirmed_bugs, potential_bugs, and contract_gaps.

### bug-hunt-taxonomize

1. Classify evaluated findings into the Beizer taxonomy (requirements, structural, data, coding, interface, integration, timing, configuration).
2. Assign severity ratings (CRITICAL, HIGH, MEDIUM, LOW) justified by the evidence in the finding.
3. Produce pattern signatures for detecting similar bugs elsewhere.
4. Do not fabricate fix suggestions — use "needs investigation" if not obvious.
5. Respond with a JSON object containing classified_findings and taxonomy_summary.

### bug-hunt-report

1. Produce a structured JSON bug report from all prior phases.
2. Consolidate findings from oracle + taxonomy into a single findings array.
3. Ensure each finding includes all required fields (id, summary, location, verdict, confidence, beizer_category, severity, evidence, pattern_signature, fix_suggestion).
4. Ensure summary counts are accurate.
5. Respond with the complete expedition report as JSON.

### bug-hunt-expedition

1. Explore a target crate, find threats to user-defined quality, classify them, and report them.
2. Generate a focused charter using Hendrickson format and Bach's HTSM.
3. Probe the target using available tools (file:read, code:search, terminal).
4. Apply pragmatic-cybernetics during probing: trace feedback loops, analyze polarity/delay/gain/closure/fidelity, perform Good Regulator check, perform Variety check.
5. Evaluate findings using the Weinberg oracle and pragmatic-semantics (IS/OUGHT, epistemic mode, provenance).
6. Challenge your own verdicts using the grill-me pattern.
7. Classify each bug by primary Beizer category, severity, and pattern signature.
8. Output as JSON using the exact schema provided.
9. Do not fabricate bugs; read real code and run real commands.

### bug-hunt-convergence-check

1. Measure how much bug-hunting work remains and whether findings have stabilized.
2. Count unresolved findings by severity (Critical, High, Medium).
3. Check finding stability (overlap with prior iteration).
4. Check classification completeness (Beizer taxonomy coverage).
5. Start at 1.0, subtract for resolved/stabilized items, and clamp to [0, 1].
6. Return JSON only containing convergence_metric, convergence_method, metric_decomposition, rationale, and blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `bug-hunt-charter.j2` | KnowAct | Generate a focused bug-hunt charter using Hendrickson format and Bach's Heuristic Test Strategy Model (HTSM). Targets specific Beizer taxonomy categories based on the target crate and user-defined quality criteria.  |
| `bug-hunt-probe.j2` | KnowAct | PROBE phase — agent-coordinated MCP tool execution. Reads target source files, searches for bug patterns, runs cargo check/test/clippy, and when probe_depth includes `dynamic`, executes BugStalker (`bs`) runtime probes (breakpoints, thread inspection, Tokio oracle) for timing, integration, and structural Beizer categories. This template provides guidance and collects results; the actual probing is performed by the agent using MCP tools between template calls.  |
| `bug-hunt-oracle.j2` | KnowAct | Apply Weinberg oracle (bug = threat to user-defined quality), pragmatic-semantics IS/OUGHT classification, epistemic mode labeling, provenance tracing, and grill-me self-challenge to raw probe findings. Produces tiered verdicts (BUG / POTENTIAL_BUG / OBSERVATION) with confidence scores.  |
| `bug-hunt-taxonomize.j2` | KnowAct | Classify evaluated findings into Beizer taxonomy (requirements, structural, data, coding, interface, integration, timing, configuration) and assign severity ratings (CRITICAL, HIGH, MEDIUM, LOW). Produces pattern signatures for detecting similar bugs elsewhere.  |
| `bug-hunt-report.j2` | KnowAct | Compile charter, oracle, and taxonomy results into a structured JSON bug report. Consolidates findings, computes summary statistics, and produces the final expedition report.  |
| `bug-hunt-expedition.j2` | KnowAct | Legacy monolithic expedition template (v0.30.0). Retained for backward compatibility. Prefer the decomposed pipeline: charter → probe → oracle → taxonomize → report.  |
| `bug-hunt-convergence-check.j2` | KnowAct | Compute normalized convergence metric for bug-hunt PDCA cycles. Saturation detection with stability check — severity-weighted unresolved findings + Beizer taxonomy coverage.  |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **best-of-n mode** — Pick the
best diagnosis from multiple models.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `bug-hunt-charter.j2`: Public.
- `bug-hunt-probe.j2`: Public.
- `bug-hunt-oracle.j2`: Public.
- `bug-hunt-taxonomize.j2`: Public.
- `bug-hunt-report.j2`: Public.
- `bug-hunt-expedition.j2`: Public.
- `bug-hunt-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
