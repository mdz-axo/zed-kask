---

name: gpa-evolution
description: "GEPA (Genetic-Pareto) evolutionary optimization over text artifacts. Samples execution trajectories, reflects in natural language to diagnose failures and surface rules, and recombines lessons from the Pareto frontier of (quality, cost)."
---


# GPA Evolution

GEPA (Genetic-Pareto) evolutionary optimization over text artifacts. The skill samples execution trajectories, reflects in natural language to diagnose failures and surface high-level rules, proposes and tests mutations, and recombines complementary lessons from the Pareto frontier of (quality, cost) until the frontier stabilizes. v1 implements the prompt artifact path.

## When to Use

- When you need to evolve a text artifact (LLM prompt) through evolutionary optimization rather than gradient-based tuning
- When natural-language reflection is preferred over sparse scalar rewards as a learning signal
- When multi-objective optimization over (quality, cost) with Pareto frontier management is required
- When the artifact type is `prompt` (v1 limitation — `manifest` and `template` paths are not yet implemented)
- When you want to diagnose failure patterns and surface transferable high-level rules from execution trajectories

## When NOT to Use

- Gradient-based fine-tuning — `lora-training` / `adapter-lifecycle` own the training loop; this skill evolves text through reflection, not weights.
- Non-prompt artifacts — v1 is prompt-only (`manifest` and `template` paths unimplemented).
- Single-pass prompt tweaks — use `prompt-enhance`; evolution needs a trajectory of executions to reflect on.

## Instructions

1. **Sample trajectories.** Run the target artifact against its eval set through a real executor (a tool or harness that sends the prompt to a model and records the response) and capture trajectories (input, output, reasoning, tool calls). Outcome scores come from the eval set's own evaluator, applied to recorded outputs — never from the model imagining how the prompt would perform. On iteration 1, sample from `target_artifact`; on iteration 2+, from the current Pareto frontier members. With no executor or evaluator available, stop and report `unverified`.

2. **Reflect in natural language.** Analyze trajectories to diagnose failures, surface high-level rules, and identify success and failure patterns. For each trajectory (or group of similar trajectories): diagnose why the outcome was poor or good (be specific), extract a general transferable rule, identify success patterns to preserve, identify failure patterns to eliminate, and map each objective score to what in the trajectory caused it. This reflection IS the gradient signal — it replaces sparse scalar rewards with rich, actionable prose.

3. **Propose mutations.** Generate 3–7 artifact variants from reflected lessons via two operators: **mutation** (targeted edit to the artifact based on a single reflected lesson — each variant tests one hypothesis) and **crossover** (recombine complementary strengths from non-dominated frontier members). Tag each variant with its parent (original or frontier member ID), operator type, hypothesis, and the rule addressed. Include the full mutated artifact content, not just the diff.

4. **Test variants.** Run each mutated artifact against the full eval set through the same executor and evaluator as step 1, and collect per-objective scores (mean, min, max) plus cost (rollouts, tokens, latency) from the recorded runs. Record whether each hypothesis was confirmed by the measurements. Keep run logs under `~/Documents/zk-data/skills/gpa-evolution/{date}-{run}/`. This is the most inference-intensive step.

5. **Update Pareto frontier.** Merge the current frontier with newly tested variants into a single pool. Perform non-dominated sort: variant A dominates variant B if A is at least as good as B on ALL objectives and strictly better on at least ONE. Keep only non-dominated members as the new frontier. If the frontier exceeds `frontier_size`, prune by crowding distance (remove variants in the most crowded region of objective space to maintain diversity). Record which variants were dominated and by whom for audit.

6. **Check convergence.** Call `lisp_eval` with form `(if (< iteration 2) 1.0 (let ((m (+ hypervolume_delta (* 0.05 new_members)))) (if (> m 1) 1 (if (< m 0) 0 m))))` over the measured `hypervolume_delta` between `frontier_before` and `frontier_after` and the count of new non-dominated members. Converged when the metric ≤ threshold (default 0.10). Bound: at most 5 iterations per session; on reaching it, stop and report the frontier and the remaining metric.

7. **Act — propose, never adopt.** The executing session does not replace the target prompt. Write the final frontier (each member's content, measured scores, cost, lineage, and the eval set identity) via `terminal` to `~/Documents/zk-data/curator/proposals/{target}/{date}-{run}.json`. The operator chooses whether any member replaces the target, in `algedonic-review`'s gemba walk (operator ruling 2026-09-24: skill evaluation is separated from execution).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `gpa-sample-trajectories.j2` | Step 1 — Assemble trajectories (input, output, reasoning, tool calls, evaluator scores) from `recorded_runs` of real executions; never simulated. On iteration 1, samples from target_artifact. On iteration 2+, samples from the current Pareto frontier. |
| `gpa-reflect.j2` | Step 2 — Reflect in natural language on trajectories. Diagnose failures, surface high-level rules, identify success and failure patterns. This reflection IS the gradient signal — it replaces sparse scalar rewards. |
| `gpa-propose-mutations.j2` | Step 3 — Generate artifact variants from reflected lessons via mutation (targeted edit) and crossover (recombine complementary lessons from non-dominated frontier members). Each variant tests one hypothesis. |
| `gpa-test-variants.j2` | Step 4 — Aggregate `recorded_runs` of each variant into per-objective scores (mean, min, max) plus cost (rollouts, tokens, latency); unrecorded runs are reported, not estimated. |
| `gpa-frontier-update.j2` | Step 5 — Update Pareto frontier. Merge current frontier with newly tested variants, keep non-dominated members, prune by crowding distance if frontier exceeds size limit. |

To render a template, call the `render_template` tool with the template ref (e.g., `gpa-evolution/gpa-sample-trajectories`) and a context object with the required variables.

## Constraints

- All templates have `visibility: Public`
- Only `artifact_type: "prompt"` is implemented in v1 — `"manifest"` and `"template"` paths return empty results with explanatory notes
- Minimum 2 iterations before convergence is allowed (iteration 1 always returns metric = 1.0); maximum 5 iterations per session
- Scores come from recorded executions and the eval set's evaluator, never from the model's own estimate of a prompt's performance
- The frontier is a proposal for the operator's algedonic review; this skill never adopts a variant
- Convergence threshold defaults to 0.10 (configurable via `_convergence.threshold`)
- Generate 3–7 variants per iteration — too few gives insufficient exploration, too many wastes inference
- Each variant must test exactly one hypothesis with a clear "if I change X, then Y will improve because Z" statement
- Pareto dominance requires strict improvement on at least one objective while being at least as good on all others
- Frontier pruning uses crowding distance to maintain diversity when frontier exceeds `frontier_size`
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
