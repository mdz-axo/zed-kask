---

name: gpa-evolution
visibility: public
description: "GEPA (Genetic-Pareto) evolutionary optimization over text artifacts. Samples execution trajectories, reflects in natural language to diagnose failures and surface high-level rules, proposes and tests mutations, and recombines complementary lessons from the Pareto frontier of (quality, cost) until the frontier stabilizes. v1 implements the prompt artifact path."
---


# GPA Evolution

GEPA (Genetic-Pareto) evolutionary optimization over text artifacts. The skill samples execution trajectories, reflects in natural language to diagnose failures and surface high-level rules, proposes and tests mutations, and recombines complementary lessons from the Pareto frontier of (quality, cost) until the frontier stabilizes. v1 implements the prompt artifact path.

## When to Use

- When you need to evolve a text artifact (LLM prompt) through evolutionary optimization rather than gradient-based tuning
- When natural-language reflection is preferred over sparse scalar rewards as a learning signal
- When multi-objective optimization over (quality, cost) with Pareto frontier management is required
- When the artifact type is `prompt` (v1 limitation — `manifest` and `template` paths are not yet implemented)
- When you want to diagnose failure patterns and surface transferable high-level rules from execution trajectories

## Instructions

1. **Sample trajectories.** Execute the target artifact against its eval set and capture trajectories (input, output, reasoning, tool calls, outcome scores). On iteration 1, sample from `target_artifact`. On iteration 2+, sample from the current Pareto frontier members. For each eval input, execute the prompt and capture the full trajectory including per-objective scores.

2. **Reflect in natural language.** Analyze trajectories to diagnose failures, surface high-level rules, and identify success and failure patterns. For each trajectory (or group of similar trajectories): diagnose why the outcome was poor or good (be specific), extract a general transferable rule, identify success patterns to preserve, identify failure patterns to eliminate, and map each objective score to what in the trajectory caused it. This reflection IS the gradient signal — it replaces sparse scalar rewards with rich, actionable prose.

3. **Propose mutations.** Generate 3–7 artifact variants from reflected lessons via two operators: **mutation** (targeted edit to the artifact based on a single reflected lesson — each variant tests one hypothesis) and **crossover** (recombine complementary strengths from non-dominated frontier members). Tag each variant with its parent (original or frontier member ID), operator type, hypothesis, and the rule addressed. Include the full mutated artifact content, not just the diff.

4. **Test variants.** Execute each mutated artifact against the full eval set and collect per-objective scores (mean, min, max) plus cost (rollouts, gas, latency). For each variant: use its content as the prompt, execute against every eval input, score the output against each objective, aggregate scores, and record whether the hypothesis was confirmed. This is the most gas-intensive step.

5. **Update Pareto frontier.** Merge the current frontier with newly tested variants into a single pool. Perform non-dominated sort: variant A dominates variant B if A is at least as good as B on ALL objectives and strictly better on at least ONE. Keep only non-dominated members as the new frontier. If the frontier exceeds `frontier_size`, prune by crowding distance (remove variants in the most crowded region of objective space to maintain diversity). Record which variants were dominated and by whom for audit.

6. **Check convergence.** Compute the Pareto-frontier stability convergence metric. Calculate hypervolume delta between `frontier_before` and `frontier_after` under the given objectives. Count new non-dominated members not present in the prior frontier. If iteration < 2, set metric = 1.0 (don't converge too early). Otherwise, metric = hypervolume_delta + (0.05 × new_members). Clamp to [0, 1]. Converged when metric ≤ threshold (default 0.10). Return the metric, decomposition, rationale, and any blockers preventing convergence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `gpa-sample-trajectories.j2` | `KnowAct` | Step 1 — Execute the target artifact against its eval set and capture trajectories (input, output, reasoning, tool calls, outcome scores). On iteration 1, samples from target_artifact. On iteration 2+, samples from the current Pareto frontier. |
| `gpa-reflect.j2` | `KnowAct` | Step 2 — Reflect in natural language on trajectories. Diagnose failures, surface high-level rules, identify success and failure patterns. This reflection IS the gradient signal — it replaces sparse scalar rewards. |
| `gpa-propose-mutations.j2` | `KnowAct` | Step 3 — Generate artifact variants from reflected lessons via mutation (targeted edit) and crossover (recombine complementary lessons from non-dominated frontier members). Each variant tests one hypothesis. |
| `gpa-test-variants.j2` | `KnowAct` | Step 4 — Execute each mutated artifact against the eval set and collect per-objective scores (mean, min, max) plus cost (rollouts, gas, latency). |
| `gpa-frontier-update.j2` | `KnowAct` | Step 5 — Update Pareto frontier. Merge current frontier with newly tested variants, keep non-dominated members, prune by crowding distance if frontier exceeds size limit. |
| `gpa-convergence-check.j2` | `KnowAct` | Step 6 — Compute Pareto-frontier stability convergence metric. Converged when hypervolume delta is small AND no new non-dominated members were added. Returns convergence_metric plus rationale and blockers. |

## Constraints

- All templates have `visibility: Public`
- Energy caps: sample-trajectories (4096), reflect (5120), propose-mutations (5120), test-variants (8192), frontier-update (3072), convergence-check (2048)
- Only `artifact_type: "prompt"` is implemented in v1 — `"manifest"` and `"template"` paths return empty results with explanatory notes
- Minimum 2 iterations before convergence is allowed (iteration 1 always returns metric = 1.0)
- Convergence threshold defaults to 0.10 (configurable via `_convergence.threshold`)
- Generate 3–7 variants per iteration — too few gives insufficient exploration, too many wastes gas
- Each variant must test exactly one hypothesis with a clear "if I change X, then Y will improve because Z" statement
- Pareto dominance requires strict improvement on at least one objective while being at least as good on all others
- Frontier pruning uses crowding distance to maintain diversity when frontier exceeds `frontier_size`
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins
