---
name: codegraph
visibility: public
description: "Code understanding: query, traverse, analyze, and assemble context from the code graph. Any userpod that needs to understand code structure — for debugging, refactoring, impact analysis, onboarding, or context assembly — can invoke this skill. Orchestrates the hkask-mcp-codegraph MCP tools through a convergent PDCA cycle."
---

# Codegraph

Code understanding engine skill. Any userpod that needs to understand code structure — for debugging, refactoring, impact analysis, onboarding, or context assembly — can invoke this skill. Orchestrates the `hkask-mcp-codegraph` MCP server's 8 tools (query, traverse, impact, analysis, context, structure, stats, reindex, feedback, embed, dead_code) through a convergent PDCA cycle that iterates until the userpod has sufficient code understanding to act.

## When to Use

- You need to understand the structure, dependencies, or impact of a codebase area before acting
- You are debugging and need to map callers, dependencies, or blast radius for a symbol
- You are refactoring and need to assess complexity, dead code, or structural risks
- You are onboarding to an unfamiliar module and need a map of its architecture
- You need token-budgeted context assembled from the code graph for an LLM prompt
- You want to find dead code, high-complexity symbols, or untested paths
- You need to assess the risk of changing a specific symbol (impact analysis)

## PDCA Loop

The skill follows a **Plan → Do → Check → Act** cycle that iterates until convergence:

```
Plan:   Step 1 — Discover  → Index target area, get structure overview, identify entry symbols
Do:     Step 2 — Query     → Search for goal-relevant symbols, score by relevance, identify gaps
Do:     Step 3 — Analyze   → Traverse graph, run complexity/dead-code/impact analysis, synthesize
Do:     Step 4 — Context   → Assemble token-budgeted context for downstream use (optional)
Check:  Step 5 — Converge  → Coverage saturation detection: is understanding sufficient?
Act:    Step 6 — Loop      → If not converged, re-enter at Step 1 with refined target
```

## Improvement Measure

**Convergence metric: Coverage Saturation Detection** (field: `step_5_result.convergence_metric`)

The convergence metric measures whether the code understanding gathered is sufficiently stable for downstream action. It is computed by the `codegraph-convergence-check.j2` template:

| Score | Meaning |
|-------|---------|
| 0.00 | All goal-relevant symbols discovered, traversed, analyzed; context assembled — converged |
| 0.25 | Sufficient coverage for the goal, minor gaps remain — converged at threshold |
| 0.50 | Significant symbols unexplored, analysis incomplete — not converged |
| 1.00 | No meaningful code understanding gathered — not converged |

**Threshold**: 0.25 (code understanding is exploratory, not exhaustive). For deep investigation of specific findings, chain with `diagnose` or `bug-hunt`.

**Scoring breakdown** (start at 1.0, subtract for each satisfied check):

1. Discovery: relevant crates and entry symbols identified? → +0.20 if not
2. Query: matched symbols cover the goal scope? → +0.20 if major gaps
3. Traversal: dependencies and callers mapped for key symbols? → +0.20 if incomplete
4. Analysis: complexity, dead code, and impact assessed? → +0.15 if missing
5. Synthesis: analysis directly addresses the goal? → +0.15 if vague
6. Context: assembled context is token-budgeted and ready? → +0.10 if not

## Instructions

### 1. Discover

1. If the codebase is not yet indexed, invoke `codegraph_reindex` to build the code graph from the workspace.
2. Invoke `codegraph_stats` to get index statistics (total symbols, files, edges).
3. Invoke `codegraph_structure` to get the top symbols by PageRank — these reveal the architectural backbone.
4. The `codegraph-discover.j2` template synthesizes these into a discovery summary with relevant crates and 3–7 entry symbols for deeper traversal.
5. Prefer public symbols with high PageRank as entry points. If the codebase is large, narrow to the crates most relevant to the goal.

### 2. Query

1. Derive search queries from the goal — keyword, name lookup, semantic, or FTS5.
2. Invoke `codegraph_query` with the derived queries. Set the `name` field for exact symbol lookup.
3. The `codegraph-query.j2` template scores results by relevance (0.0–1.0), identifies which symbols need traversal, and flags gaps for further exploration.
4. For each matched symbol with `traverse_recommended: true`, note the recommended direction (forward, reverse, or both).

### 3. Analyze

1. For each symbol flagged for traversal, invoke `codegraph_traverse` with the appropriate direction:
   - **Forward** — what the symbol depends on (its dependencies)
   - **Reverse** — who depends on the symbol (its callers)
2. Invoke `codegraph_analysis` with `analysis_type: "dead_code"` to find unused private symbols.
3. Invoke `codegraph_analysis` with `analysis_type: "complexity"` to find high-complexity symbols.
4. If analyzing a specific change, invoke `codegraph_impact` with the target symbol to assess blast radius.
5. The `codegraph-analyze.j2` template organizes results into dependency maps, caller maps, impact analysis, and quality findings with fix suggestions.

### 4. Context (Conditional)

1. This step runs only when `requires_context` is true (default). Set `requires_context: false` to skip context assembly when only analysis is needed.
2. Invoke `codegraph_context` with the query and budget (`small`/`medium`/`large`).
3. The `codegraph-context.j2` template formats the assembled context from the raw context results and analysis output, within the specified token budget.
4. The assembled context includes symbol definitions, doc comments, and relevant code snippets prioritized by relevance and impact.

### 5. Check Convergence

1. Evaluate whether discovery, query, traversal, analysis, and context assembly are sufficiently complete for the userpod's goal.
2. The convergence check receives all prior step results (discovery, query, analysis, context) to score all 6 dimensions.
3. Compute the convergence metric using the scoring guidance above.
4. If converged (≤ 0.25), proceed to act on the gathered understanding.
5. If not converged, identify the specific gap (missing traversal, incomplete analysis, vague synthesis) and re-enter the cycle.

### 6. Act / Loop

1. If converged, the analysis results are ready for downstream use — debugging, refactoring, impact assessment, or context assembly.
2. If not converged, re-enter at Step 1 with a refined target derived from the convergence blockers.
3. Maximum 3 iterations. If convergence is not reached after 3 iterations, escalate.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `codegraph-discover.j2` | `KnowAct` | Discover and map the target codebase area. Synthesizes index statistics and structure overview into a discovery summary with relevant crates and entry symbols. |
| `codegraph-query.j2` | `KnowAct` | Query the code graph for goal-relevant symbols. Scores results by relevance, identifies traversal targets, and flags gaps. |
| `codegraph-analyze.j2` | `KnowAct` | Traverse the dependency graph and run quality analysis. Organizes results into dependency maps, caller maps, impact analysis, and quality findings with fix suggestions. |
| `codegraph-context.j2` | `KnowAct` | Assemble token-budgeted context from gathered analysis results for downstream LLM use. |
| `codegraph-convergence-check.j2` | `KnowAct` | Compute normalized convergence metric for codegraph cycles via coverage saturation detection. |
| `symbol-summarize.j2` | `KnowAct` | Generate one-sentence summaries of code symbols (utility, used by MCP server). |
| `analysis-complexity.j2` | `KnowAct` | SQL query for complexity analysis (utility, used by MCP server). |
| `analysis-dead-code.j2` | `KnowAct` | SQL query for dead code detection (utility, used by MCP server). |
| `fix-suggestion.j2` | `KnowAct` | Generate fix suggestions for code issues (utility, used by MCP server). |

## MCP Tools

The skill delegates to the `hkask-mcp-codegraph` MCP server:

| Tool | Purpose |
|------|---------|
| `codegraph_query` | Search/lookup symbols by keyword, name, or semantic query |
| `codegraph_traverse` | Forward (dependencies) or reverse (callers) graph traversal |
| `codegraph_impact` | Blast radius analysis for a target symbol |
| `codegraph_analysis` | Dead code or complexity analysis |
| `codegraph_context` | Assemble token-budgeted context for LLM prompts |
| `codegraph_structure` | Project overview — top symbols by PageRank |
| `codegraph_stats` | Index statistics (symbol/file/edge counts) |
| `codegraph_reindex` | Force full re-index of the workspace |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility.
- Energy caps: discover (4096), query (4096), analyze (6144), convergence-check (2000).
- Gas cap: 100,000 per invocation. Maximum 3 iterations.
- Traversal depth: `immediate-neighbors` (default), `transitive` (2-hop), `full` (recursive CTE).
- Only report findings relevant to the goal — don't list every symbol in the graph.
- Impact analysis should identify the riskiest change points.
- Quality findings should be actionable, not just descriptive.
- Jinja2 sandboxed execution: no arbitrary Python code when safety mode is enabled.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.