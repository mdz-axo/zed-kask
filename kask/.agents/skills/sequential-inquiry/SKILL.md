---
name: sequential-inquiry
visibility: public
description: "Dynamic chain-of-thought reasoning engine with branching, revision, hypothesis testing, and automatic deep-dive delegation to specialized skills (hypothesis-framer, mcda, diagnose, falsifiability). Subsumes the deprecated sequential-thinking skill. The engine decides at runtime whether delegation is needed — no pre-selection. Templates: reasoning engine, four delegation targets (hypothesis-framer, mcda, diagnose, falsifiability), and a convergence check.
"
---

# Sequential Inquiry

Dynamic chain-of-thought reasoning engine with branching, revision, hypothesis testing, and automatic deep-dive delegation to specialized skills (hypothesis-framer, mcda, diagnose, falsifiability). Subsumes the deprecated sequential-thinking skill. The engine decides at runtime whether delegation is needed — no pre-selection. Templates: reasoning engine, four delegation targets (hypothesis-framer, mcda, diagnose, falsifiability), and a convergence check.


## When to Use

- Dynamic chain-of-thought reasoning is required with branching, revision, and hypothesis testing.
- A problem requires automatic deep-dive delegation to specialized skills (`hypothesis-framer`, `mcda`, `diagnose`, `falsifiability`) when the engine detects specific subproblems.
- A candidate hypothesis needs formal validation via FINER + PICO structuring.
- Multiple alternatives need weighted comparison and structured tradeoff (MCDA).
- A bug symptom or failure pattern requires disciplined root cause diagnosis (reproduce → anchor → hypothesize → fix).
- Evaluating whether a reasoning chain has reached a defensible answer or requires another iteration (convergence check).

## Instructions

### sequential-inquiry-engine

1. Work through problems using dynamic, reflective chain-of-thought reasoning while identifying when a thought needs deeper analysis from specialized sub-skills.
2. Manage thought numbering, branching, revision, hypothesis generation/verification, and delegation requests.
3. Build upon the prior thinking chain without repeating it.
4. Weave prior delegation results into the thought chain by referencing the producing skill, summarizing the key insight, and explaining how it changes or confirms your thinking.
5. Execute the core loop: revise if new insight contradicts earlier, branch if an alternative path is worth exploring, delegate if a thought needs deeper analysis, hypothesize if enough evidence, verify if a hypothesis exists, and terminate if verified with no uncertainty.
6. Branch when two plausible approaches need comparison, a counterfactual scenario must be explored, an edge case deserves its own thread, or an assumption needs stress-testing.
7. Revise when a later thought reveals a flaw, new decomposition invalidates a prior assumption, or evidence contradicts an earlier claim.
8. Generate a hypothesis when evidence accumulates and verify it against all evidence and edge cases.
9. Emit a delegation request for a thought that needs analysis beyond reasoning alone, strictly limited to `hypothesis-framer`, `mcda`, `diagnose`, or `falsifiability`. Delegate to `falsifiability` when a counterfactual scenario must be explored or a claim's testability must be ruled on (admit), when multiple explanations need hard elimination rather than probabilistic reweighting (use `falsifiability`), or when a causal claim needs do(not X) counterfactual stress-testing.
10. Limit delegation to one request per distinct analysis need, max 3 requests per cycle.
11. Incorporate delegation results from the previous cycle and do not re-delegate the same thing.

### sequential-inquiry-delegate-hypothesis-framer

1. Examine the delegation requests array and find all requests where `skill` equals `"hypothesis-framer"`.
2. Return `invoked: false` if no matching requests are found.
3. For each matching request, apply FINER + PICO to the candidate hypothesis in `params.hypothesis`.
4. Evaluate the hypothesis for Feasibility, Interestingness, Novelty, Ethicality, and Relevance (FINER).
5. Structure the hypothesis by defining Population/Problem, Intervention, Comparison, and Outcome (PICO).
6. Return the FINER evaluation, PICO structure, refined hypothesis, null hypothesis, and testability assessment.

### sequential-inquiry-delegate-mcda

1. Examine the delegation requests array and find all requests where `skill` equals `"mcda"`.
2. Return `invoked: false` if no matching requests are found.
3. For each matching request, apply MCDA to `params.alternatives`.
4. Identify criteria from the problem domain and weight them (total = 1.0).
5. Score each alternative per criterion (1-10) and calculate the weighted sum.
6. Detect compensation masking where high scores in one area hide low scores in another.
7. Perform sensitivity analysis to determine the weight shift required to change the ranking.
8. Return criteria, scored alternatives, compensation warnings, sensitivity analysis, and a recommendation.

### sequential-inquiry-delegate-diagnose

1. Examine the delegation requests array and find all requests where `skill` equals `"diagnose"`.
2. Return `invoked: false` if no matching requests are found.
3. For each matching request, apply structured diagnosis to `params.symptom`.
4. Reproduce the issue using the fastest deterministic feedback loop.
5. Anchor the symptom to a code path or invariant violation.
6. Hypothesise 3-5 ranked, falsifiable root-cause hypotheses.
7. Recommend instrumentation probes to discriminate between hypotheses.
8. Recommend the most likely cause and a high-level fix strategy.

### sequential-inquiry-delegate-falsifiability

1. Examine the delegation requests array and find all requests where `skill` equals `"falsifiability"`.
2. Return `invoked: false` if no matching requests are found.
3. For each matching request, apply the eliminative inference engine to `params.target` in fixed stage order: admit (Popper gate) → hypothesize (Chamberlin) → counterfactual (Pearl do-operator) → discriminate (Platt) → eliminate (hard falsification, corroborate-never-confirm).
4. Rule out the untestable at the question level before generating hypotheses; discard hypotheses with no falsifier at generation; flag irreducible counterfactuals.
5. If `params.observations` are provided, eliminate hypotheses whose predictions are contradicted and return a verdict; otherwise return the discriminating-test design with `verdict: tests_pending`.
6. Return the admissibility assessment, hypotheses, counterfactuals, discriminating tests, verdict, eliminated/corroborated/survived-by-default lists, and falsification log.

### sequential-inquiry-convergence-check

1. Score convergence starting at 1.0 and subtract for each satisfied criterion.
2. Check if a hypothesis exists and if it is verified.
3. Check if the chain is complete (`needsMoreThoughts: false` on final thought).
4. Check for unresolved branches and pending revisions.
5. Check if confidence is calibrated (`solution_confidence` ≥ 0.7).
6. Check if the answer is synthesized (clear, specific, actionable).
7. Check if all delegations are resolved.
8. Check if delegation results are incorporated (ONLY cycle 2+).
9. Check if the chain is stable between iterations (ONLY cycle 2+).
10. Clamp the convergence metric to [0, 1] and return the decomposition, rationale, and blockers.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `sequential-inquiry-engine.j2` | KnowAct | Core reasoning engine — advances one chain-of-thought step, deciding whether to continue, branch, revise, delegate, or converge based on the current reasoning state.  |
| `sequential-inquiry-delegate-hypothesis-framer.j2` | KnowAct | Delegation target — frames a research question / testable hypothesis via FINER + PICO when the engine detects a question-framing subproblem.  |
| `sequential-inquiry-delegate-mcda.j2` | KnowAct | Delegation target — multi-criteria decision analysis when the engine detects a choice among alternatives requiring structured tradeoff.  |
| `sequential-inquiry-delegate-diagnose.j2` | KnowAct | Delegation target — disciplined diagnosis loop when the engine detects a bug or regression requiring reproduce → anchor → hypothesize → fix. |
| `sequential-inquiry-delegate-falsifiability.j2` | KnowAct | Delegation target — eliminative inference engine when the engine branches on a counterfactual scenario or needs to rule out the untestable. Applies the Popper/Platt/Chamberlin/Pearl method: admit → hypothesize → counterfactual → discriminate → eliminate. |
| `sequential-inquiry-convergence-check.j2` | KnowAct | Convergence gate — evaluates whether the reasoning chain has reached a defensible answer or requires another iteration. |

## Constraints

- `sequential-inquiry-engine.j2`: Public.
- `sequential-inquiry-delegate-hypothesis-framer.j2`: Public.
- `sequential-inquiry-delegate-mcda.j2`: Public.
- `sequential-inquiry-delegate-diagnose.j2`: Public.
- `sequential-inquiry-delegate-falsifiability.j2`: Public.
- `sequential-inquiry-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
