# hkask-condenser

Domain logic for context condensation — compression algorithms, ontology-aware
saliency weighting, and engine state management. Pure domain crate: no MCP,
no HTTP, no async.

## Architecture

| Module | Role |
|--------|------|
| `algorithms` | Three compression algorithms (`rtk_style`, `word_rank`, `flashrank`) with domain-aware scoring |
| `ontology_graph` | Cross-domain concept relationship index (SUMO, GOLEM, ML-Schema, PKO, DC+BIBO) |
| `types` | `OntologyAnchor`, `Profile`, `ContextCategory`, `CompressedOutput`, health signals |
| `engine` | `CondenserEngine` — exhaustive category dispatch; active profile is its only stored state |
| `saliency` | Word-frequency computation shared with `WordRankAlgorithm` |

## Compression Profiles

| Profile | Retention | Max Lines | Use Case |
|---------|-----------|-----------|----------|
| `heavy` | 10% | 30 | Aggressive compression — minimal representation |
| `normal` | 20% | 80 | Default — balanced |
| `soft` | 60% | 200 | Moderate — preserves more context |
| `light` | 95% | — | Minimal compression — user sovereignty |

Inputs shorter than `MIN_COMPRESSIBLE_LINES` (8 lines) pass through
uncompressed at every profile. Terminal output arrives fenced, so a one-line
command is three lines; compressing that far saved no meaningful tokens while
eliding the payload entirely (`algorithms.rs::compute_budget`).

## Algorithms

`engine.rs`, `CondenserEngine::compress`, matches every `ContextCategory` to a
borrowed stateless algorithm (`&dyn CondenserAlgorithm`) and invokes compression
through that trait object. `Unknown` is an explicit match alternative, not an
algorithm-selection fallback. An explicit category overrides tool-name
classification. The retained `CondenserAlgorithm::compress`
default body in `algorithms.rs` shares line collection, profile budgeting, and
unchanged-input passthrough before calling `compress_budgeted`. Algorithm
behavior, profile values, and output metrics are unchanged.

### rtk_style
Head/tail ellipsis truncation. Keeps the first N% and last M% of lines with
a `...` separator. Selected for `ShellCommand`, `TestOutput`, `BuildOutput`.
Uses ontology density factor to adjust head/tail ratio (denser anchors get more tail).
The head always retains at least one real line: a budget too small to yield a
non-zero head would otherwise replace the whole beginning with an ellipsis.

### word_rank
TF-IDF bag-of-words compression with structural bonus and ontology anchoring.
Scores every line, keeps the highest-scoring budget lines. Selected for
`ConversationHistory`, `LogOutput`.

Scoring formula:
```
score = TF-IDF_average + structural_bonus + domain_saliency
```

- **TF-IDF_average:** mean word frequency across the input — rare words score higher
- **structural_bonus:** error=2.0, warning=1.0, heading=0.5, list=0.2
- **domain_saliency:** direct domain keyword match (0.3–0.5) + graph adjacency bonus (up to 0.5)

### flashrank
Greedy marginal-utility selection under the profile's retained-line limit. Balances relevance,
novelty, and brevity. Selected for `FileContents`, `StructuredData`, `Unknown`.

## Ontology Anchoring (P5.4/P8.1)

The condenser derives the ontology anchor from the `tool_name` — every MCP
server links against the same bridge crates, so no wire-protocol fields
are needed.

| Tool prefix | Ontology tier | Domain bridge |
|-------------|--------------|---------------|
| `corpus_*`, `media_*` | Domain supplement | GOLEM / OMC |
| `memory_*` | Domain supplement | SUMO |
| `replica_*`, `author_*` | Domain supplement | GOLEM |
| `training_*`, `adapter_*`, `sweep_*` | Domain supplement | ML-Schema |
| `kanban_*`, `task_*`, `spec_*`, `research_*`, `skill_*` | Dual-axis (PKO) | — |
| `file_*`, `web_*`, `registry_*`, `wallet_*` | Dual-axis (DC+BIBO) | — |
| Everything else | Core (5W1H) | — |

The ontology graph encodes concept relationships (e.g., `sumo:encoding` →
`Precedes` → `sumo:recall`) and serves as a saliency
multiplier — lines containing graph-adjacent concepts get bonus scores.

## Telemetry

The `hkask.condenser` tracing spans are **diagnostic logging** for human inspection — NOT cybernetic feedback signals. They are not consumed by any regulation policy or feedback loop, which is why they sit under `hkask.*` and not the reserved `reg.*` prefix (PRINCIPLES §9.1). The actual feedback channel is the daemon's `store_experience` call in the MCP server layer.

| Span | Fields | When |
|------|--------|------|
| `hkask.condenser` compress | `algorithm`, `category`, `tool_name`, `ontology_tier` | Every compression |
| `hkask.condenser` compression_ratio | `reduction_pct`, `original_bytes`, `compressed_bytes`, `latency_ms` | Every compression |

## Consumers

- `kask_bridge` — `BridgeThreadCondenser`, installed through `agent::set_thread_condenser`. Incoming tool-result compression honors `kask.condenser.auto_compress_tool_results` (default off). Manual native compaction (`/compact` or the composer icon) independently precompresses eligible older tool output in a request copy before the native LLM writes its summary. Prose, the latest exchange, protected tools, failed results, JSON, and non-text content remain unchanged. Stored originals are not modified. The existing algorithms operate on lines; reduction does not guarantee that the provider's token limit is met.

The `hkask-mcp-condenser` MCP server was removed during the skill-system migration cleanup (2026-08). The condenser is now a pure domain library consumed only by `kask_bridge`.

## Saliency

The `saliency` module (`pub(crate)`) provides `word_frequencies` — the canonical
word-frequency computation that `WordRankAlgorithm` delegates to instead of
maintaining a copy.
