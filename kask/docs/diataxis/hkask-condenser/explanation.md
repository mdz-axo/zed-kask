---
title: "hkask-condenser — Explanation"
audience: [developers, architects, agents]
last_updated: 2026-08-20
version: "1.2.0"
status: "Active"
domain: "Condensation"
mds_categories: [trust, curation]
---

# hkask-condenser — Explanation

Tool-result compression solves a context-window problem. As an agent
conversation grows, verbose tool output (shell commands, logs, file dumps)
exceeds the model's context limit. The condenser compresses each tool
result by classifying it, deriving an ontology anchor, selecting an
algorithm, and scoring lines by domain saliency — discarding low-salience
lines and keeping high-salience ones within a `Profile`-derived budget. The
condenser is a deep module: a simple interface (`compress`) over a complex
implementation (three algorithms + ontology graph + saliency scoring).

## Source citations

| Symbol | Location |
|--------|----------|
| `CondenserEngine` | `kask/crates/hkask-condenser/src/engine.rs:28` |
| `CondenserEngine::compress` | `kask/crates/hkask-condenser/src/engine.rs:47` |
| `CondenserAlgorithm` trait | `kask/crates/hkask-condenser/src/algorithms.rs:33` |
| `AlgorithmRegistry::select` | `kask/crates/hkask-condenser/src/algorithms.rs:493` |
| `classify_tool` | `kask/crates/hkask-condenser/src/algorithms.rs:528` |
| `derive_ontology_anchor` | `kask/crates/hkask-condenser/src/algorithms.rs:559` |
| `domain_saliency` | `kask/crates/hkask-condenser/src/algorithms.rs:231` |
| `compute_budget` | `kask/crates/hkask-condenser/src/algorithms.rs:26` |
| `OntologyGraph::graph_adjacency_bonus` | `kask/crates/hkask-condenser/src/ontology_graph.rs:290` |
| `score_against_persona` | `kask/crates/hkask-condenser/src/saliency.rs:52` |
| `BridgeThreadCondenser` | `kask/crates/kask_bridge/src/condenser_bridge.rs:22` |
| `KaskCondenserSettings` | `kask/crates/kask_bridge/src/settings.rs:333` |
| `set_thread_condenser` | `crates/agent/src/agent.rs:3144` |
| `NO_COMPRESS_TOOLS` | `crates/agent/src/thread.rs:175` |
| Deferred-task wiring | `crates/zed/src/main.rs:1929` |

## The compression cycle

`CondenserEngine::compress` (`kask/crates/hkask-condenser/src/engine.rs:47`)
runs a single-pass cycle. There is no re-classification loop and no
learning override — each `compress` call is one pass. The previous
learning subsystem (history ring buffer, `recommend_algorithm`,
`compression_stats`, `suggest_profile`, `check_global_health`) was removed
because it was dormant in the default-off configuration and existed only
to justify MCP tools that surfaced it; the runtime bridge path
(`BridgeThreadCondenser`) never used it (see the doc comment at
`kask/crates/hkask-condenser/src/engine.rs:22`).

```mermaid
stateDiagram-v2
    [*] --> Classify: receive tool output
    Classify --> Anchor: classify_tool -> ContextCategory
    Anchor --> Select: derive_ontology_anchor -> OntologyAnchor
    Select --> Budget: compute_budget(lines, profile)
    Budget --> Compress: registry.select (static default_for)
    Compress --> Emit: algorithm.compress -> (content, health_signals)
    Emit --> [*]: return CompressedOutput
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COND-004
verified_date: 2026-08-13
verified_against: kask/crates/hkask-condenser/src/engine.rs:47,53,59,67,69,85; kask/crates/hkask-condenser/src/algorithms.rs:26,493,528,559
status: VERIFIED
-->

The cycle steps, in order:

1. **Classify** — `classify_tool` (`kask/crates/hkask-condenser/src/algorithms.rs:528`)
   maps the tool name to a `ContextCategory` via exact token match, then
   substring fallback.
2. **Anchor** — `derive_ontology_anchor`
   (`kask/crates/hkask-condenser/src/algorithms.rs:559`) maps the tool name
   to an `OntologyAnchor` via `select_ontology_anchor`
   (`kask/crates/hkask-bridge-ontology/src/axis.rs:220`).
3. **Select** — `AlgorithmRegistry::select`
   (`kask/crates/hkask-condenser/src/algorithms.rs:493`) walks the registry
   in registration order and returns the first algorithm whose
   `default_for()` contains the category.
4. **Budget** — `compute_budget`
   (`kask/crates/hkask-condenser/src/algorithms.rs:26`) derives the line
   budget from `retention_pct * lines`, capped by `max_lines` and `lines`.
   If the budget meets or exceeds the input, the algorithm returns the
   input unchanged (passthrough).
5. **Compress** — the selected algorithm's `compress` method
   (`kask/crates/hkask-condenser/src/algorithms.rs:40`) returns
   `(content, health_signals)`.
6. **Emit** — the engine assembles a `CompressedOutput`
   (`kask/crates/hkask-condenser/src/engine.rs:86`) and emits two
   diagnostic `hkask.condenser` spans
   (`kask/crates/hkask-condenser/src/engine.rs:68` and `:84`).

## Why ontology anchoring

The `derive_ontology_anchor` function
(`kask/crates/hkask-condenser/src/algorithms.rs:559`) maps a tool name to
an `OntologyAnchor` (`kask/crates/hkask-bridge-ontology/src/axis.rs:133`).
This anchor connects compression to the dual-axis ontology (PKO +
DC+BIBO) plus domain supplements (FIBO, ESO, GOLEM, ML-Schema, SDMX,
SUMO). The anchor provides the keywords that `domain_saliency`
(`kask/crates/hkask-condenser/src/algorithms.rs:231`) uses to score lines.

Without ontology anchoring, the condenser would score lines by generic
word frequency, which loses domain-specific signal. A line about
`market_capitalization` is salient in a FIBO-anchored financial context
but not in a GOLEM narrative context. The anchor tells the scorer which
domain the conversation belongs to, and the ontology graph supplies the
adjacency bonus — lines referencing concepts related to the anchor
concept (e.g., `fibo:MarketCapitalization` when anchored to
`fibo:Corporation`) receive a bonus via
`OntologyGraph::graph_adjacency_bonus`
(`kask/crates/hkask-condenser/src/ontology_graph.rs:290`).

The anchor is derived from the tool name alone because every MCP server
links against the same bridge crates — no wire-protocol fields are needed.
This keeps the condenser's interface narrow: `compress(tool_name, output,
category)` is the entire surface.

## Why three algorithms

The three algorithms offer different tradeoffs:

- `RtkStyleAlgorithm` (`kask/crates/hkask-condenser/src/algorithms.rs:49`)
  is a deterministic structural scorer — head/tail preservation with an
  ontology-aware split ratio. It is the right choice for shell, test, and
  build output, where the first and last lines carry the command and the
  result.
- `WordRankAlgorithm` (`kask/crates/hkask-condenser/src/algorithms.rs:119`)
  ranks by TF-IDF word frequency, structural bonus, and ontology anchoring.
  It is the right choice for conversation history and logs, where salient
  lines are scattered through the input.
- `FlashrankAlgorithm`
  (`kask/crates/hkask-condenser/src/algorithms.rs:326`) uses greedy
  marginal-utility selection within a budget, balancing relevance, novelty,
  and brevity. It is the right choice for file contents and structured
  data, and is the universal fallback because it is registered last.

The separation allows the condenser to degrade gracefully. If a ranking
model is unavailable, the registry falls back to the deterministic
`RtkStyleAlgorithm` or the universal `FlashrankAlgorithm` fallback. There
is no learning loop that could drift the selection — the static
`default_for()` mapping is the only selection path.

## Why health signals, not errors

`CondenserHealthSignal` (`kask/crates/hkask-condenser/src/types.rs:195`)
is emitted when an algorithm exhibits unexpected behavior —
`negative_compression` (rtk_style produced larger output),
`low_signal` (word_rank found no usable signal), `budget_shortfall`
(flashrank could not fill the budget). These are diagnostic ν-event
candidates for the Regulation layer: they indicate deviation from expected
bounds, not failure. Content is still returned. The condenser never
blocks the agent turn on a compression anomaly — it logs the signal and
moves on, so a misbehaving algorithm cannot stall the conversation.

## Wiring: deferred post-login task

The `set_thread_condenser` hook
(`crates/agent/src/agent.rs:3144`) is a `Mutex`-based process-global
(re-settable). It is wired from the deferred post-login task in
`crates/zed/src/main.rs:1929`, which constructs a
`BridgeThreadCondenser` (`kask/crates/kask_bridge/src/condenser_bridge.rs:22`)
wrapping a `CondenserEngine`. The wiring is conditional on
`KaskCondenserSettings.auto_compress_tool_results`
(`kask/crates/kask_bridge/src/settings.rs:343`), which defaults to `false`
(`kask/crates/kask_bridge/src/settings.rs:359`); when disabled, the hook
is left `None` and tool results pass through uncompressed.

Code-reading tools (`read_file`, `grep`, `list_directory`, etc.) bypass
the condenser via `NO_COMPRESS_TOOLS` (`crates/agent/src/thread.rs:175`).
The condenser's line-level elision (joining non-consecutive selected lines
with `...`) is destructive for source code, so these tools' output passes
through verbatim even when a condenser is wired.

## See also

- [hkask-condenser Reference](./reference.md): class diagram of the
  algorithms, registry, and ontology graph.
- [hkask-condenser How-to](./how-to.md): tuning salience weights and profiles.
- [hkask-condenser Tutorial](./tutorial.md): compressing your first tool output.

---

[^salience]: Itti, L., Koch, C., & Niebur, E. (1998). *A model of saliency-based visual attention for rapid scene analysis.* IEEE Transactions on Pattern Analysis and Machine Intelligence, 20(11), 1254–1259. <https://ieeexplore.ieee.org/document/730558>. The saliency model adapted for text compression.

[^ousterhout]: Ousterhout, J. (2018). *A Philosophy of Software Design.* Yakny Press. <https://web.stanford.edu/~ouster/cgi-bin/book.php>. The deep-module principle: the condenser exposes a simple interface (`compress`) over a complex implementation (three algorithms + ontology graph + saliency scoring).
