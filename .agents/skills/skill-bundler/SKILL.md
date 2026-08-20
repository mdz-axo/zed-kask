---
name: skill-bundler
core: true
description: "Run peer-level skills concurrently and merge their outputs into a single unified report. The bundler does not compose or iterate — it dispatches each skill in parallel, collects results, and synthesizes them with per-skill summaries, cross-skill insights, conflicts, and prioritized recommendations."
---

# Skill Bundler

Run peer-level skills concurrently and merge their outputs into a single unified report. The parallel fan-out happens in Rust (`BridgeManifestExecutor::compose_and_execute_bundle`); this skill's manifest handles only the merge step. There is no iterative composition, PKO graph synthesis, or convergence loop — the bundler dispatches each skill in parallel, collects results (allSettled — partial results OK if a skill errors), and synthesizes them.

## When to Use

- You have run 3+ peer-level skills concurrently on the same task and need their outputs merged into one coherent report.
- You need per-skill summaries, cross-skill insights (what the combination reveals that no single skill did), explicit conflicts between skills' conclusions, and prioritized recommendations.
- You do NOT need skill composition, ordering resolution, or ontology anchoring — the bundler merges; it does not compose.

## Instructions

### bundler-merge

1. **Per-skill summary**: For each skill, write 2-3 sentences capturing its key findings, verdict, or recommendations. Label each with the skill name. Every skill gets a summary, even if it errored.
2. **Cross-skill insights**: Identify points where skills complement, contradict, or build on each other. Only include insights that require looking at 2+ skills together — don't repeat what a single skill already said.
3. **Conflicts**: If two skills reached contradictory conclusions, surface them explicitly. State each skill's position and the nature of the disagreement.
4. **Recommendations**: Produce a prioritized list of actionable recommendations derived from the combined output. Each recommendation should cite which skill(s) informed it.
5. **Merged report**: Write a single cohesive report that weaves the per-skill summaries, cross-skill insights, and recommendations into a readable document. The report should read as one analysis, not a stapled-together list of skill outputs.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `bundler-merge.j2` | Merge the outputs of N concurrently-executed skills into a single cohesive report with per-skill summaries, cross-skill insights, conflict surfacing, and prioritized recommendations. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- `bundler-merge.j2`: Public.
- Single-pass merge — no PDCA loop. `convergence_mode: ""` with `max_iterations: 1` runs the merge exactly once.
- Do not invent findings that no skill produced.
- Do not omit a skill from the summaries — every skill gets a summary, even if it errored.
- The merged report must reference each skill by name at least once.
- Keep the merged report under 2000 words.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
