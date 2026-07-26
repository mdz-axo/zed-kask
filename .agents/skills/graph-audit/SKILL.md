---
name: graph-audit
visibility: public
description: "Unified graph analysis skill with three modes. Code mode: query, traverse, analyze, and assemble context from the code graph via the hkask-mcp-codegraph MCP server. Semantic mode: domain-agnostic dependency graph health analysis — classify edges by constraint force, detect cycles/redundancies/gaps/orphans, evaluate through pragmatic-semantics, pragmatic-cybernetics, essentialist, and grill-me lenses. Dual mode: extract the code graph via MCP tools, then run the semantic audit on the extracted graph. Any userpod may invoke this skill."
---

# Graph Audit

Unified graph analysis skill with three modes:

- **Code mode** (folded from `codegraph`): Query, traverse, analyze, and assemble context from the code graph. Orchestrates the `hkask-mcp-codegraph` MCP server's tools through a convergent PDCA cycle. Use for debugging, refactoring, impact analysis, onboarding, or context assembly. Includes the context-expansion mode (folded from `zoom-out`) for broader architectural context when lost in implementation details.

- **Semantic mode** (folded from `semantic-graph-audit`): Domain-agnostic dependency graph health analysis. Accepts any directed graph (code modules, crates, skills, ADRs, Regulation spans, decision trees, data flows) as data, classifies edges by constraint force, detects structural issues (cycles, redundancies, gaps, orphans), and evaluates graph health through four analytical lenses: pragmatic-cybernetics, essentialist, pragmatic-semantics, and grill-me. No MCP tools required.

- **Dual mode**: Extract the code graph via MCP tools (code mode), then run the semantic audit on the extracted graph (semantic mode). Produces both code understanding and graph health. This mode eliminates the manual chaining that was previously required.

## When to Use

### Code mode
- You need to understand the structure, dependencies, or impact of a codebase area before acting
- You are debugging and need to map callers, dependencies, or blast radius for a symbol
- You are refactoring and need to assess complexity, dead code, or structural risks
- You are onboarding to an unfamiliar module and need a map of its architecture
- You need token-budgeted context assembled from the code graph for an LLM prompt
- You want to find dead code, high-complexity symbols, or untested paths
- You need to assess the risk of changing a specific symbol (impact analysis)
- You are lost in implementation details and need broader architectural context, module maps, caller graphs, data flows, or boundary summaries (context-expansion)

### Semantic mode
- You need to analyze the health and viability of any directed dependency graph, including code modules, crates, skills, ADRs, Regulation spans, decision trees, or data flows
- You need to classify the binding strength of graph edges using the pragmatic-semantics constraint hierarchy (Prohibition, Guardrail, Guideline, Evidence, Hypothesis)
- You need to evaluate graph behavior through cybernetic, essentialist, Socratic (grill-me), and semantic coherence lenses
- You need to detect structural pathologies like cycles, redundancies, gaps, orphans, and fan-in/out anomalies
- You need a normalized graph-health convergence metric and actionable markdown report

### Dual mode
- You need both code understanding AND graph health analysis of the same codebase area
- You want to extract the code graph and audit its structural health in a single invocation
- You are doing architectural review and need both the "what is there" (code mode) and "is it healthy" (semantic mode) perspectives

## PDCA Loop

All three modes follow a **Plan -> Do -> Check -> Act** cycle:

```
Plan:   Step 1 - Discover/Classify  -> Code: index + map structure; Semantic: classify edges by force; Dual: index + classify
Do:     Step 2 - Query/Analyze      -> Code: query for symbols; Semantic: 4-lens analysis; Dual: query + analyze
Do:     Step 3 - Traverse/Detect    -> Code: traverse + quality analysis; Semantic: detect pathologies; Dual: traverse + detect
Do:     Step 4 - Context/Report     -> Code: assemble context (optional); Semantic: synthesize report; Dual: context + report
Check:  Step 5 - Converge           -> Coverage saturation (code) or graph-health metric (semantic/dual)
Act:    Step 6 - Loop               -> If not converged, re-enter with refined target
```

## Improvement Measure

**Code mode convergence**: Coverage Saturation Detection (field: `convergence_metric` (top-level, bound by mode-specific loop step)). Threshold: 0.25.

**Semantic/Dual mode convergence**: Graph-health convergence metric (field: `convergence_metric` (top-level, bound by mode-specific loop step)). Threshold: 0.15.

## MCP Tools (code and dual modes)

The skill delegates to the `hkask-mcp-codegraph` MCP server:

| Tool | Purpose |
|------|---------|
| `codegraph_query` | Search/lookup symbols by keyword, name, or semantic query |
| `codegraph_traverse` | Forward (dependencies) or reverse (callers) graph traversal |
| `codegraph_impact` | Blast radius analysis for a target symbol |
| `codegraph_analysis` | Dead code or complexity analysis |
| `codegraph_context` | Assemble token-budgeted context for LLM prompts |
| `codegraph_structure` | Project overview - top symbols by PageRank |
| `codegraph_stats` | Index statistics (symbol/file/edge counts) |
| `codegraph_reindex` | Force full re-index of the workspace |

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `code-discover.j2` | `KnowAct` | Discover and map the target codebase area (code mode step 1) |
| `code-query.j2` | `KnowAct` | Query the code graph for goal-relevant symbols (code mode step 2) |
| `code-analyze.j2` | `KnowAct` | Traverse the dependency graph and run quality analysis (code mode step 3) |
| `code-context.j2` | `KnowAct` | Assemble token-budgeted context for downstream LLM use (code mode step 4) |
| `code-convergence-check.j2` | `KnowAct` | Compute coverage saturation convergence metric for code mode |
| `semantic-classify.j2` | `KnowAct` | Force-classify every graph edge by pragmatic-semantics hierarchy (semantic mode step 1) |
| `semantic-analyze.j2` | `KnowAct` | Evaluate graph health through four lenses (semantic mode step 2) |
| `semantic-detect.j2` | `KnowAct` | Detect structural pathologies from graph topology (semantic mode step 3) |
| `semantic-report.j2` | `KnowAct` | Synthesize graph-health convergence metric + markdown report (semantic mode step 4) |
| `dual-convergence-check.j2` | `KnowAct` | Compute combined convergence metric for dual mode (code coverage + graph health) |
| `symbol-summarize.j2` | `KnowAct` | Generate one-sentence summaries of code symbols (utility, used by MCP server) |
| `analysis-complexity.j2` | `KnowAct` | SQL query for complexity analysis (utility, used by MCP server) |
| `analysis-dead-code.j2` | `KnowAct` | SQL query for dead code detection (utility, used by MCP server) |
| `fix-suggestion.j2` | `KnowAct` | Generate fix suggestions for code issues (utility, used by MCP server) |
| `symbol-embedding.j2` | `KnowAct` | Generate embeddings for code symbols (utility, used by MCP server) |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility.
- Gas cap: 100,000 per invocation. Maximum 3 iterations.
- Traversal depth: `immediate-neighbors` (default), `transitive` (2-hop), `full` (recursive CTE).
- Only report findings relevant to the goal - don't list every symbol in the graph.
- Impact analysis should identify the riskiest change points.
- Quality findings should be actionable, not just descriptive.
- Jinja2 sandboxed execution: no arbitrary Python code when safety mode is enabled.
- Registry is authoritative - when this SKILL.md disagrees with registry templates, the registry wins.
