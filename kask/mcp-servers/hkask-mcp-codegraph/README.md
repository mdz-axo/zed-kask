# hkask-mcp-codegraph

Code understanding MCP server — exposes `hkask-codegraph` as a set of MCP tools for semantic search, graph traversal, impact analysis, context assembly, and embedding generation.

Part of hKask's code intelligence layer. Indexes the current workspace on first tool invocation, with incremental re-indexing on subsequent calls.

## Tools (10)

| Tool | Description | Requires Config |
|------|-------------|-----------------|
| `codegraph_query` | Search for symbols by keyword, or look up by exact name | — |
| `codegraph_traverse` | Traverse the code graph: forward (dependencies) or reverse (callers) | — |
| `codegraph_impact` | Analyze blast radius for a symbol with risk classification | — |
| `codegraph_analysis` | Run analysis: `dead_code` or `complexity` | — |
| `codegraph_context` | Assemble token-budgeted context for LLM prompts | — |
| `codegraph_structure` | Get project overview: top symbols ranked by PageRank | — |
| `codegraph_stats` | Get index statistics with optional health and meta breakdown | — |
| `codegraph_reindex` | Force full re-index of the workspace | — |
| `codegraph_feedback` | Record which symbols from a context_id were actually used (feedback loop) | — |
| `codegraph_index_embeddings` | Generate embeddings for all symbols via inference router | `DI_API_KEY` or `OR_API_KEY` |

## Context Budgets

| Budget | Tokens | Max Symbols | Content |
|--------|--------|-------------|---------|
| `minimal` | ~512 | 10 | Signatures only |
| `focused` | ~2048 | 20 | Signatures + doc comments |
| `standard` | ~4096 | 40 | Signatures + line ranges |
| `full` | ~8192 | 80 | Everything relevant |

## Risk Levels (Impact Analysis)

| Level | Criteria |
|-------|----------|
| `critical` | Public traits — changing these breaks external contracts |
| `high` | Public types and functions |
| `medium` | Crate-visible types and implementations |
| `low` | Private code and tests |

## Configuration

| Env Variable | Description | Default |
|-------------|-------------|---------|
| `HKASK_CODEGRAPH_DB` | SQLite database path for persistent index | In-memory (re-index on each start) |
| `DI_API_KEY` / `OR_API_KEY` | Inference API key for embedding generation | Embeddings disabled without these |
| `HKASK_EMBEDDING_MODEL` | Embedding model for symbol vectorization | `DI/Qwen/Qwen3-Embedding-0.6B` |

## Request Types

| Request | Fields |
|---------|--------|
| `QueryRequest` | `query` (search terms), `limit` (default 10), `name` (exact match) |
| `TraverseRequest` | `symbol` (name), `direction` (forward/reverse), `max_depth` (default 5) |
| `ImpactRequest` | `symbol` (name), `max_depth` (default 5) |
| `ContextRequest` | `query`, `budget` (minimal/focused/standard/full) |
| `AnalysisRequest` | `kind` (dead_code/complexity) |
| `StructureRequest` | `limit` (default 20) |
| `StatsRequest` | `include_health` (bool), `include_meta` (bool) |
| `FeedbackRequest` | `context_id`, `symbols_provided`, `symbols_used` |
| `EmbedIndexRequest` | `model` (optional), `batch_size` (default 32) |

## Running

```bash
# As part of kask (auto-started with other MCP servers)
kask chat

# Standalone stdio MCP server
hkask-mcp-codegraph

# With persistent index
HKASK_CODEGRAPH_DB=/path/to/codegraph.db hkask-mcp-codegraph

# With embedding support
DI_API_KEY=your-key HKASK_CODEGRAPH_DB=/path/to/codegraph.db hkask-mcp-codegraph
```

## Dependencies

- `hkask-codegraph` — Domain engine (search, traversal, analysis, context)
- `hkask-mcp` — MCP server framework (`mcp_server!`, `execute_tool`, `run_server`)
- `hkask-types` — `WebID`
- `hkask-inference` — `EmbeddingRouter` for embedding generation
- `rmcp` — MCP protocol
- `minijinja` — Jinja2 templates for embedding prompts
- `tracing` — Regulation event emission
