---
name: graph-audit
visibility: public
description: "Unified graph analysis skill with three modes. Code mode: query, traverse, and analyze the code graph via the hkask-mcp-codegraph MCP server. Semantic mode: dependency graph health analysis. Dual mode: extract then audit."
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
| `code-discover.j2` | KnowAct | Discover and map the target codebase area. Synthesizes index statistics and structure overview into a discovery summary with relevant crates and entry symbols for further traversal. |
| `code-query.j2` | KnowAct | Query the code graph for symbols relevant to the goal. Scores results by relevance, identifies traversal targets, and flags gaps. |
| `code-analyze.j2` | KnowAct | Traverse the dependency graph and run quality analysis. Organizes results into dependency maps, caller maps, impact analysis, and quality findings with fix suggestions. |
| `code-context.j2` | KnowAct | Assemble token-budgeted context from gathered analysis results for downstream LLM use. Formats symbol definitions, doc comments, and key relationships within the specified token budget. |
| `semantic-classify.j2` | KnowAct | Force-classify every graph edge by the pragmatic-semantics constraint hierarchy (Prohibition > Guardrail > Guideline > Evidence > Hypothesis) with provenance and a per-edge rationale. |
| `semantic-analyze.j2` | KnowAct | Evaluate graph health through four lenses: pragmatic-cybernetics (cycle 5-properties, Ashby requisite variety, Good Regulator), essentialist (deletion test, surface count, pass-through trace), grill-me (5-level gap probe), pragmatic-semantics (force coherence). |
| `semantic-detect.j2` | KnowAct | Detect structural pathologies from graph topology: cycles, redundancies, gaps, orphans, fan-in/out anomalies, and force/structure mismatches. Severity reflects constraint force (a Prohibition cycle is critical). |
| `semantic-report.j2` | KnowAct | Synthesize the classification, four-lens analysis, and structural detection into a normalized graph-health convergence metric and a readable markdown report. |
| `symbol-summarize.j2` | KnowAct | Generate one-sentence summaries of code symbols. |
| `analysis-complexity.j2` | KnowAct | Analyze code complexity metrics for symbols and modules. |
| `analysis-dead-code.j2` | KnowAct | Detect dead code and unused symbols. |
| `fix-suggestion.j2` | KnowAct | Generate fix suggestions for code issues. |
| `symbol-embedding.j2` | KnowAct | Generate embeddings for code symbols. |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility.
- rJoule cap: 3 per invocation. Maximum 10 iterations.
- Traversal depth: `immediate-neighbors` (default), `transitive` (2-hop), `full` (recursive CTE).
- Only report findings relevant to the goal - don't list every symbol in the graph.
- Impact analysis should identify the riskiest change points.
- Quality findings should be actionable, not just descriptive.
- Jinja2 sandboxed execution: no arbitrary Python code when safety mode is enabled.
- Registry is authoritative - when this SKILL.md disagrees with registry templates, the registry wins.

## Known Limitations

- **Workspace-only indexing (code/dual modes):** The `codegraph_structure` (step 2) and `codegraph_stats` (step 1) MCP tools operate on the indexed zed-kask workspace only. Against an external codebase that has not been indexed by `hkask-mcp-codegraph`, these tools time out (30s `timeout_seconds` in the manifest) and return no data. The `on_failure: report` fallback allows the cascade to proceed without index stats / PageRank data, but code-mode and dual-mode audits against external codebases will have degraded discovery. To audit an external codebase, either (a) index it first via `codegraph_reindex`, or (b) use semantic mode, which accepts any directed graph as input data and requires no MCP tools.
- **`codegraph_impact` and `codegraph_reindex` not wired into PDCA steps:** These MCP tools are available on the `hkask-mcp-codegraph` server but are not referenced by any `execute` step in `manifest.yaml`. They can be invoked directly by an agent between cascade steps, but the manifest's convergent loop does not call them.
