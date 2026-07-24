---
name: semantic-graph-audit
visibility: public
description: "Domain-agnostic semantic dependency graph analysis. Accepts any directed graph (code modules, crates, skills, ADRs, Regulation spans, decision trees, data flows), classifies edges by constraint force, detects cycles/redundancies/gaps/orphans, and evaluates graph health through pragmatic-semantics, pragmatic-cybernetics, essentialist, and grill-me lenses.
"
---

# Semantic Graph Audit

Domain-agnostic semantic dependency graph analysis. Accepts any directed graph (code modules, crates, skills, ADRs, Regulation spans, decision trees, data flows), classifies edges by constraint force, detects cycles/redundancies/gaps/orphans, and evaluates graph health through pragmatic-semantics, pragmatic-cybernetics, essentialist, and grill-me lenses.


## When to Use

- When you need to analyze the health and viability of any directed dependency graph, including code modules, crates, skills, ADRs, Regulation spans, decision trees, or data flows.
- When you need to classify the binding strength of graph edges using the pragmatic-semantics constraint hierarchy (Prohibition, Guardrail, Guideline, Evidence, Hypothesis).
- When you need to evaluate graph behavior through cybernetic, essentialist, Socratic (grill-me), and semantic coherence lenses.
- When you need to detect structural pathologies like cycles, redundancies, gaps, orphans, and fan-in/out anomalies.
- When you need a normalized graph-health convergence metric and actionable markdown report to identify and prioritize structural fixes.

## Instructions

### semantic-graph-audit-classify

1. Classify every directed edge by its constraint force using the pragmatic-semantics hierarchy (Prohibition, Guardrail, Guideline, Evidence, Hypothesis).
2. State the provenance for each edge (spec, implementation, observation, inference, unknown).
3. Provide a one-line rationale anchored to the edge's semantics/label and the domain.
4. If an edge's binding strength is ambiguous, classify at the weakest force the evidence supports.
5. Flag over-constraint in the summary if Prohibition edges are used where Guideline would suffice.
6. Ensure every input edge appears in the output 1:1 and do not invent new edges.

### semantic-graph-audit-analyze

1. Evaluate the graph through the pragmatic-cybernetics lens by identifying cycles, assessing their 5 properties (polarity, delay, gain, closure, fidelity), checking Ashby's Law (requisite variety), and verifying the Good Regulator condition.
2. Apply the essentialist lens using the deletion test (Exist), counting fan-out for surface violations (Surface), and tracing abstraction boundaries for pass-throughs (Contract).
3. Execute the grill-me lens using a 5-level Socratic probe (Recall, Mechanism, Rationale, Edge Cases, Synthesis) to rate knowledge areas and surface gaps.
4. Assess pragmatic-semantics by checking force coherence, ranking conflicts by OT ranking, and flagging unanchored Hypothesis edges.
5. Synthesize the overall graph health, identifying the top 3 issues by constraint force and the most material lens finding.

### semantic-graph-audit-detect

1. Detect structural pathologies purely from graph topology and prior classification/analysis without re-classifying edges.
2. Identify cycles, redundancies, gaps, orphans, fan-in anomalies, fan-out anomalies, and force/structure mismatches.
3. Assign severity (critical, high, medium, low) based on the pathology type and the constraint force of involved edges (e.g., a Prohibition cycle is critical).
4. Cross-reference the prior analysis where a lens already flagged a structure to avoid contradictions.
5. Summarize the counts per type and severity, explicitly naming the single most critical issue.

### semantic-graph-audit-report

1. Synthesize the classification, four-lens analysis, and structural detection into a single normalized graph-health convergence metric.
2. Calculate the metric starting at 0.0 and adding weighted penalties for critical/high/medium issues, variety deficits, Good Regulator violations, Prohibition cycles, unanchored edges, and surviving deletion candidates.
3. Determine the `graph_health` verdict (healthy, viable_with_issues, degraded, unsound) based on the convergence metric bands.
4. Produce a concise markdown report containing a one-paragraph verdict, top issues ranked by severity, the most material lens, and recommended actions ordered by constraint force.
5. List specific blockers preventing a healthy verdict.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `semantic-graph-audit-classify.j2` | KnowAct | Force-classify every graph edge by the pragmatic-semantics constraint hierarchy (Prohibition > Guardrail > Guideline > Evidence > Hypothesis) with provenance and a per-edge rationale. Produces classified_edges + a summary flagging over-constraint (Prohibition used where Guideline would do).  |
| `semantic-graph-audit-analyze.j2` | KnowAct | Evaluate graph health through four lenses: pragmatic-cybernetics (cycle 5-properties, Ashby requisite variety, Good Regulator), essentialist (deletion test on elements, surface count, pass-through trace), grill-me (5-level gap probe), pragmatic-semantics (force coherence + OT conflict ranking). Produces lens_evaluations + a synthesis.  |
| `semantic-graph-audit-detect.j2` | KnowAct | Detect structural pathologies from graph topology: cycles, redundancies, gaps, orphans, fan-in/out anomalies, and force/structure mismatches. Severity reflects constraint force (a Prohibition cycle is critical). Produces structural_issues + an issue_summary.  |
| `semantic-graph-audit-report.j2` | KnowAct | Synthesize the classification, four-lens analysis, and structural detection into a normalized graph-health convergence metric + a readable markdown report. The convergence step (convergence_field points here). Threshold 0.15.  |

## Constraints

- `semantic-graph-audit-classify.j2`: Public.
- `semantic-graph-audit-analyze.j2`: Public.
- `semantic-graph-audit-detect.j2`: Public.
- `semantic-graph-audit-report.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
