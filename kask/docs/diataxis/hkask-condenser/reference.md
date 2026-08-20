---
title: "hkask-condenser — Reference"
audience: [developers, architects, agents]
last_updated: 2026-08-20
version: "1.1.0"
status: "Active"
domain: "Condensation"
mds_categories: [domain, lifecycle]
---

# hkask-condenser — Reference

`hkask-condenser` is the pure domain crate for context condensation. It
classifies each tool result into a `ContextCategory`, derives an
`OntologyAnchor`, selects a `CondenserAlgorithm` via the `AlgorithmRegistry`,
and scores lines by domain saliency, persona keywords, and structural
bonuses. The crate provides three algorithms and a `CondenserEngine` that
dispatches compression via the static `default_for()` mapping. No MCP, no
HTTP, no async — the crate is fully testable in-process.

## Source citations

| Symbol | Location |
|--------|----------|
| `CondenserEngine` | `kask/crates/hkask-condenser/src/engine.rs:28` |
| `CondenserEngine::new` | `kask/crates/hkask-condenser/src/engine.rs:40` |
| `CondenserEngine::compress` | `kask/crates/hkask-condenser/src/engine.rs:47` |
| `CondenserEngine::set_profile` | `kask/crates/hkask-condenser/src/engine.rs:99` |
| `CondenserEngine::profile` | `kask/crates/hkask-condenser/src/engine.rs:104` |
| `CondenserAlgorithm` trait | `kask/crates/hkask-condenser/src/algorithms.rs:33` |
| `RtkStyleAlgorithm` | `kask/crates/hkask-condenser/src/algorithms.rs:49` |
| `WordRankAlgorithm` | `kask/crates/hkask-condenser/src/algorithms.rs:119` |
| `FlashrankAlgorithm` | `kask/crates/hkask-condenser/src/algorithms.rs:326` |
| `AlgorithmRegistry` | `kask/crates/hkask-condenser/src/algorithms.rs:473` |
| `AlgorithmRegistry::new` | `kask/crates/hkask-condenser/src/algorithms.rs:484` |
| `AlgorithmRegistry::select` | `kask/crates/hkask-condenser/src/algorithms.rs:493` |
| `compute_budget` | `kask/crates/hkask-condenser/src/algorithms.rs:26` |
| `domain_saliency` | `kask/crates/hkask-condenser/src/algorithms.rs:231` |
| `classify_tool` | `kask/crates/hkask-condenser/src/algorithms.rs:528` |
| `derive_ontology_anchor` | `kask/crates/hkask-condenser/src/algorithms.rs:559` |
| `OntologyGraph` | `kask/crates/hkask-condenser/src/ontology_graph.rs:43` |
| `OntologyRelation` | `kask/crates/hkask-condenser/src/ontology_graph.rs:28` |
| `OntologyGraph::related` | `kask/crates/hkask-condenser/src/ontology_graph.rs:282` |
| `OntologyGraph::graph_adjacency_bonus` | `kask/crates/hkask-condenser/src/ontology_graph.rs:290` |
| `graph()` | `kask/crates/hkask-condenser/src/ontology_graph.rs:310` |
| `anchor_keywords` | `kask/crates/hkask-condenser/src/ontology_graph.rs:316` |
| `word_frequencies` | `kask/crates/hkask-condenser/src/saliency.rs:25` |
| `score_against_persona` | `kask/crates/hkask-condenser/src/saliency.rs:52` |
| `extract_query_words` | `kask/crates/hkask-condenser/src/saliency.rs:91` |
| `score_memory_results` | `kask/crates/hkask-condenser/src/saliency.rs:104` |
| `SUMMARY_SYSTEM_PROMPT` | `kask/crates/hkask-condenser/src/inference.rs:13` |
| `format_conversation_text` | `kask/crates/hkask-condenser/src/inference.rs:21` |
| `build_summarization_prompt` | `kask/crates/hkask-condenser/src/inference.rs:35` |
| `build_summary_output` | `kask/crates/hkask-condenser/src/inference.rs:48` |
| `approx_token_count` | `kask/crates/hkask-condenser/src/inference.rs:72` |
| `OntologyAnchor` | `kask/crates/hkask-bridge-ontology/src/axis.rs:133` |
| `OntologyAxis` | `kask/crates/hkask-bridge-ontology/src/axis.rs:33` |
| `OntologyNamespace` | `kask/crates/hkask-bridge-ontology/src/axis.rs:47` |
| `select_ontology_anchor` | `kask/crates/hkask-bridge-ontology/src/axis.rs:220` |
| `Profile` enum | `kask/crates/hkask-condenser/src/types.rs:49` |
| `ContextCategory` enum | `kask/crates/hkask-condenser/src/types.rs:130` |
| `CompressedOutput` | `kask/crates/hkask-condenser/src/types.rs:174` |
| `CondenserHealthSignal` | `kask/crates/hkask-condenser/src/types.rs:195` |
| `PersistRequest` | `kask/crates/hkask-condenser/src/types.rs:38` |
| `ThreadSummaryRequest` | `kask/crates/hkask-condenser/src/types.rs:214` |
| `ThreadSummaryOutput` | `kask/crates/hkask-condenser/src/types.rs:234` |

## Class diagram

The `CondenserAlgorithm` trait (`kask/crates/hkask-condenser/src/algorithms.rs:33`)
defines the compression interface. Three implementations are registered in
`AlgorithmRegistry` (`kask/crates/hkask-condenser/src/algorithms.rs:473`).
`CondenserEngine` (`kask/crates/hkask-condenser/src/engine.rs:28`) owns the
registry and the active `Profile`. The ontology graph
(`kask/crates/hkask-condenser/src/ontology_graph.rs:43`) supplies the
adjacency bonus used by `domain_saliency`.

```mermaid
classDiagram
    class CondenserAlgorithm {
        <<interface>>
        +name() str
        +description() str
        +default_for() ~[ContextCategory]~
        +compress(input, profile, cat, anchor) (String, Vec~HealthSignal~)
    }
    class RtkStyleAlgorithm {
        +default_for() ShellCommand, TestOutput, BuildOutput
    }
    class WordRankAlgorithm {
        +default_for() ConversationHistory, LogOutput
    }
    class FlashrankAlgorithm {
        +default_for() FileContents, StructuredData, Unknown
    }
    class AlgorithmRegistry {
        -algorithms: Vec~Box~dyn CondenserAlgorithm~~
        +new()
        +select(cat) CondenserAlgorithm
    }
    class CondenserEngine {
        +registry: AlgorithmRegistry
        -profile: Profile
        +new()
        +compress(tool, output, cat) CompressedOutput
        +set_profile(p)
        +profile() Profile
    }
    class OntologyGraph {
        -edges: HashMap~str, Vec~(str, OntologyRelation)~~
        +related(kw) [(str, OntologyRelation)]
        +graph_adjacency_bonus(line, kws) f64
    }
    class OntologyRelation {
        <<enumeration>>
        PartOf
        Precedes
        HasProperty
        RelatedTo
        Contains
        CrossDomain
    }
    class Profile {
        <<enumeration>>
        Heavy
        Normal
        Soft
        Light
        +retention_pct() f64
        +max_lines() Option~usize~
        +action_threshold() f64
    }
    class ContextCategory {
        <<enumeration>>
        ShellCommand
        TestOutput
        BuildOutput
        FileContents
        ConversationHistory
        StructuredData
        LogOutput
        Unknown
    }
    class CompressedOutput {
        +content: String
        +algorithm: String
        +category: String
        +profile: String
        +original_lines: usize
        +compressed_lines: usize
        +original_bytes: usize
        +compressed_bytes: usize
        +reduction_pct: f64
        +health_signals: Vec~CondenserHealthSignal~
    }

    CondenserAlgorithm <|.. RtkStyleAlgorithm
    CondenserAlgorithm <|.. WordRankAlgorithm
    CondenserAlgorithm <|.. FlashrankAlgorithm
    AlgorithmRegistry --> CondenserAlgorithm : selects via default_for
    CondenserEngine --> AlgorithmRegistry : owns
    CondenserEngine --> Profile : holds active
    CondenserEngine ..> ContextCategory : classifies via classify_tool
    CondenserEngine ..> CompressedOutput : returns
    WordRankAlgorithm ..> OntologyGraph : graph_adjacency_bonus
    OntologyGraph --> OntologyRelation : edges typed by
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COND-003
verified_date: 2026-08-13
verified_against: kask/crates/hkask-condenser/src/algorithms.rs:33,49,119,326,473,493; kask/crates/hkask-condenser/src/engine.rs:28,47,99,104; kask/crates/hkask-condenser/src/ontology_graph.rs:28,43,282,290; kask/crates/hkask-condenser/src/types.rs:49,130,174
status: VERIFIED
-->

## Algorithms

### RtkStyleAlgorithm (`algorithms.rs:49`)

Head/tail ellipsis truncation. Keeps the first N% and last M% of lines
with a `...` separator. The head/tail split is ontology-aware: the
`density_factor` of the anchor (`kask/crates/hkask-bridge-ontology/src/axis.rs:170`)
adjusts the head ratio via `(0.3 / density_factor).clamp(0.15, 0.5)`
(`kask/crates/hkask-condenser/src/algorithms.rs:81`), so FIBO financial
data (density 1.3) gets more tail. Emits a `negative_compression` health
signal if the result is larger than the input
(`kask/crates/hkask-condenser/src/algorithms.rs:99`).

### WordRankAlgorithm (`algorithms.rs:119`)

TF-IDF bag-of-words compression with structural bonus and ontology
anchoring. Scores every line via `line_score`
(`kask/crates/hkask-condenser/src/algorithms.rs:128`):

```
score = TF-IDF_average + structural_bonus + domain_saliency
```

- **TF-IDF_average:** mean word frequency across the input — rare words
  score higher. Word frequencies are computed by `saliency::word_frequencies`
  (`kask/crates/hkask-condenser/src/saliency.rs:25`), the canonical
  implementation that the algorithm delegates to.
- **structural_bonus:** error=2.0, warning=1.0, heading=0.5, list=0.2
  (`kask/crates/hkask-condenser/src/algorithms.rs:142`).
- **domain_saliency:** direct domain keyword match (0.3–0.5) + graph
  adjacency bonus (up to 0.5) via `domain_saliency`
  (`kask/crates/hkask-condenser/src/algorithms.rs:231`).

Emits a `low_signal` health signal when more than half the lines score 0.0
(`kask/crates/hkask-condenser/src/algorithms.rs:203`).

### FlashrankAlgorithm (`algorithms.rs:326`)

Greedy marginal-utility selection under token budget. Balances
relevance, novelty, and brevity with weights `alpha=0.4`, `beta=0.3`,
`gamma=0.3` (`kask/crates/hkask-condenser/src/algorithms.rs:401`). Query
terms are extracted from the first 5 lines
(`kask/crates/hkask-condenser/src/algorithms.rs:405`). Emits a
`budget_shortfall` health signal when fewer lines than the budget are
selected (`kask/crates/hkask-condenser/src/algorithms.rs:454`).

## Saliency functions

The `saliency` module (`kask/crates/hkask-condenser/src/saliency.rs`)
exposes three public functions plus the canonical `word_frequencies`
helper:

| Function | Location | Returns |
|----------|----------|---------|
| `score_against_persona` | `saliency.rs:52` | 0.0–1.0 word-overlap score; 0.5 neutral if keywords or text empty |
| `extract_query_words` | `saliency.rs:91` | Up to 5 words with length > 3 |
| `score_memory_results` | `saliency.rs:104` | 0.2 if no results; `0.5 + count * 0.15` capped at 1.0 |
| `word_frequencies` | `saliency.rs:25` | Lowercase word → normalized frequency for words with length > 2 |

The domain crate owns the scoring formula and query-word extraction (pure,
testable). The MCP server (`condenser_score_saliency` tool) owns the I/O
dispatch to semantic or episodic memory stores and delegates scoring to
the domain crate.

## Ontology graph

The `OntologyGraph` (`kask/crates/hkask-condenser/src/ontology_graph.rs:43`)
is a lightweight cross-domain concept relationship index built once at
startup via `OnceLock` (`kask/crates/hkask-condenser/src/ontology_graph.rs:307`).
It encodes relationships across PKO, SUMO, FIBO, GOLEM, ML-Schema,
and cross-domain bridges. The `OntologyRelation` enum
(`kask/crates/hkask-condenser/src/ontology_graph.rs:28`) defines six
relation types: `PartOf`, `Precedes`, `HasProperty`, `RelatedTo`,
`Contains`, `CrossDomain`.

The `graph()` function (`kask/crates/hkask-condenser/src/ontology_graph.rs:310`)
returns the global singleton. `anchor_keywords`
(`kask/crates/hkask-condenser/src/ontology_graph.rs:316`) maps an
`OntologyAnchor` to the keywords used for graph lookup.
`graph_adjacency_bonus` (`kask/crates/hkask-condenser/src/ontology_graph.rs:290`)
adds 0.15 per related concept found in a line, capped at 0.5.

## Tool classification and anchor derivation

`classify_tool` (`kask/crates/hkask-condenser/src/algorithms.rs:528`)
maps a tool name to a `ContextCategory` via two phases: exact token match
on `_`/`-`-split parts, then substring fallback. The keyword table is
`KEYWORD_CATEGORIES` (`kask/crates/hkask-condenser/src/algorithms.rs:508`).

`derive_ontology_anchor` (`kask/crates/hkask-condenser/src/algorithms.rs:559`)
is a thin pass-through to `select_ontology_anchor`
(`kask/crates/hkask-bridge-ontology/src/axis.rs:220`), which maps a tool
name to an `OntologyAnchor` (`kask/crates/hkask-bridge-ontology/src/axis.rs:133`).
The anchor exposes `confidence_modifier`
(`kask/crates/hkask-bridge-ontology/src/axis.rs:156`),
`density_factor` (`kask/crates/hkask-bridge-ontology/src/axis.rs:170`),
`axis` (`kask/crates/hkask-bridge-ontology/src/axis.rs:190`), and
`tier_label` (`kask/crates/hkask-bridge-ontology/src/axis.rs:199`).

## Inference formatting

The `inference` module (`kask/crates/hkask-condenser/src/inference.rs`)
holds pure formatting functions for LLM-assisted thread summarization.
Inference itself is handled by the centralized `InferencePort`
(hkask-inference router); this module contains only the testable pure
logic with no HTTP or async.

| Function | Location | Purpose |
|----------|----------|---------|
| `SUMMARY_SYSTEM_PROMPT` | `inference.rs:13` | System prompt for the `condenser_thread_summary` tool |
| `format_conversation_text` | `inference.rs:21` | Render messages as `[role]: content\n\n` |
| `build_summarization_prompt` | `inference.rs:35` | Compose the user-turn summarization prompt |
| `build_summary_output` | `inference.rs:48` | Assemble a `ThreadSummaryOutput` with token estimates |
| `approx_token_count` | `inference.rs:72` | `chars / 4`, floored at 1 |

## Regulation spans

The `reg.condenser` tracing spans emitted at
`kask/crates/hkask-condenser/src/engine.rs:67` and
`kask/crates/hkask-condenser/src/engine.rs:83` are diagnostic logging
for human inspection, NOT cybernetic feedback signals. They are not
consumed by any regulation policy or feedback loop. The actual feedback
channel is the daemon's `store_experience` call in the MCP server layer.

| Span | Fields | When |
|------|--------|------|
| `reg.condenser` compress | `algorithm`, `category`, `tool_name`, `ontology_tier` | Every compression |
| `reg.condenser` compression_ratio | `reduction_pct`, `original_bytes`, `compressed_bytes`, `latency_ms` | Every compression |

## Consumers

- `kask_bridge` — `BridgeThreadCondenser`
  (`kask/crates/kask_bridge/src/condenser_bridge.rs:22`): the runtime
  tool-result compression path wired into the agent turn loop via
  `agent::set_thread_condenser` (`crates/agent/src/agent.rs:3144`),
  gated on `kask.condenser.auto_compress_tool_results` (default off).

> **Note (2026-08-20):** The `hkask-mcp-condenser` MCP server was deleted
> (commit `26215d845e`). The former MCP-server surface
> (`condenser_ping`, `condenser_persist`, `condenser_thread_summary`,
> `condenser_score_saliency`) no longer exists. The `hkask-condenser`
> **crate** remains as a pure domain crate consumed only by
> `kask_bridge::BridgeThreadCondenser` for in-process thread condensation.
> The `condenser_score_saliency` tool mentioned in the Saliency section
> above was part of the deleted server; the `saliency` module's public
> functions remain available to in-process callers.

## See also

- [hkask-condenser Explanation](./explanation.md): state diagram of the
  compression process and the ontology anchoring rationale.
- [hkask-condenser How-to](./how-to.md): tuning salience weights and profiles.
- [hkask-condenser Tutorial](./tutorial.md): compressing your first tool output.

---

[^salience]: Itti, L., Koch, C., & Niebur, E. (1998). *A model of saliency-based visual attention for rapid scene analysis.* IEEE Transactions on Pattern Analysis and Machine Intelligence, 20(11), 1254–1259. <https://ieeexplore.ieee.org/document/730558>. The saliency model that the `domain_saliency` function adapts for text.
