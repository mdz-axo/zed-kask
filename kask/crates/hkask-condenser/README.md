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
| `saliency` | Word-frequency computation shared with `WordRankAlgorithm` |

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

The `hkask-mcp-condenser` MCP server was removed during the skill-system migration cleanup (2026-08). The condenser is now a pure domain library consumed only by `kask_bridge`.

## Saliency

The `saliency` module (`pub(crate)`) provides `word_frequencies` — the canonical
word-frequency computation that `WordRankAlgorithm` delegates to instead of
maintaining a copy.
