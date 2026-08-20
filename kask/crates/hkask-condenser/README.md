# hkask-condenser

Domain logic for context condensation — compression algorithms, ontology-aware
saliency weighting, and engine state management. Pure domain crate: no MCP,
no HTTP, no async.

## Architecture

| Module | Role |
|--------|------|
| `algorithms` | Three compression algorithms (`rtk_style`, `word_rank`, `flashrank`) with domain-aware scoring |
| `ontology_graph` | Cross-domain concept relationship index (FIBO, SUMO, GOLEM, ML-Schema, PKO, DC+BIBO) |
| `types` | `OntologyAnchor`, `Profile`, `ContextCategory`, `CompressedOutput`, health signals |
| `engine` | `CondenserEngine` — compression dispatch and profile management |
| `inference` | Prompt formatting and token estimation for LLM thread summarization |
| `saliency` | Persona word-overlap scoring, memory query-word extraction, memory result scoring |

## Compression Profiles

| Profile | Retention | Max Lines | Action Threshold | Use Case |
|---------|-----------|-----------|-----------------|----------|
| `heavy` | 10% | 30 | 0.10 | Aggressive compression — minimal representation |
| `normal` | 20% | 80 | 0.25 | Default — balanced |
| `soft` | 60% | 200 | 0.50 | Moderate — preserves more context |
| `light` | 95% | — | 0.90 | Minimal compression — user sovereignty |

## Algorithms

### rtk_style
Head/tail ellipsis truncation. Keeps the first N% and last M% of lines with
a `...` separator. Default for ShellCommand, TestOutput, BuildOutput.
Uses ontology density factor to adjust head/tail ratio (FIBO gets more tail).

### word_rank
TF-IDF bag-of-words compression with structural bonus and ontology anchoring.
Scores every line, keeps the highest-scoring budget lines. Default for
ConversationHistory, LogOutput.

Scoring formula:
```
score = TF-IDF_average + structural_bonus + domain_saliency
```

- **TF-IDF_average:** mean word frequency across the input — rare words score higher
- **structural_bonus:** error=2.0, warning=1.0, heading=0.5, list=0.2
- **domain_saliency:** direct domain keyword match (0.3–0.5) + graph adjacency bonus (up to 0.5)

### flashrank
Greedy marginal-utility selection under token budget. Balances relevance,
novelty, and brevity. Default for FileContents, StructuredData, Unknown.

## Ontology Anchoring (P5.4/P8.1)

The condenser derives the ontology anchor from the `tool_name` — every MCP
server links against the same bridge crates, so no wire-protocol fields
are needed.

| Tool prefix | Ontology tier | Domain bridge |
|-------------|--------------|---------------|
| `company_*`, `stock_*`, `dcf_*`, `portfolio_*` | Domain supplement | FIBO |
| `memory_*`, `episodic_*`, `semantic_*` | Domain supplement | SUMO |
| `replica_*`, `author_*` | Domain supplement | GOLEM |
| `training_*`, `adapter_*`, `sweep_*` | Domain supplement | ML-Schema |
| `kanban_*`, `task_*`, `spec_*`, `research_*`, `skill_*` | Dual-axis (PKO) | — |
| `file_*`, `web_*`, `registry_*`, `wallet_*` | Dual-axis (DC+BIBO) | — |
| Everything else | Core (5W1H) | — |

The ontology graph encodes concept relationships (e.g., `fibo:Corporation` →
`HasProperty` → `fibo:MarketCapitalization`) and serves as a saliency
multiplier — lines containing graph-adjacent concepts get bonus scores.

## Regulation Spans

The `reg.condenser` tracing spans are **diagnostic logging** for human inspection — NOT cybernetic feedback signals. They are not consumed by any regulation policy or feedback loop. The actual feedback channel is the daemon's `store_experience` call in the MCP server layer.

| Span | Fields | When |
|------|--------|------|
| `reg.condenser` compress | `algorithm`, `category`, `tool_name`, `ontology_tier` | Every compression |
| `reg.condenser` compression_ratio | `reduction_pct`, `original_bytes`, `compressed_bytes`, `latency_ms` | Every compression |

## Consumers

- `kask_bridge` — `BridgeThreadCondenser`: the runtime tool-result compression path wired into the agent turn loop via `agent::set_thread_condenser` (gated on `kask.condenser.auto_compress_tool_results`, default off)
- `hkask-mcp-condenser` — MCP server exposing `condenser_ping`, `condenser_persist`, `condenser_thread_summary`, `condenser_score_saliency`

## Saliency Architecture

The saliency module is split between the domain crate (pure logic) and the MCP server (I/O dispatch):

- **Domain crate** (`saliency.rs`): `score_against_persona`, `extract_query_words`, `score_memory_results` — pure functions, fully testable without memory stores.
- **MCP server** (`condenser_score_saliency` tool): Dispatches to semantic or episodic memory stores, delegates scoring to the domain crate.

The `word_frequencies` function is the canonical word-frequency computation shared with `WordRankAlgorithm` — the algorithm delegates to `saliency` instead of maintaining a copy.

Persona keywords are configurable via the `HKASK_CONDENSER_PERSONA_KEYWORDS` env var at the MCP server level, and per-request via the `persona_keywords` parameter on the `SaliencyRequest` schema.
