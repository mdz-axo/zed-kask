---
title: "hkask-condenser — Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-20
version: "1.5.2"
status: "Active"
domain: "Condensation"
mds_categories: [domain, lifecycle]
---

# hkask-condenser — Reference

`hkask-condenser` exposes synchronous compression domain logic. Public modules
are `engine` and `types`; algorithms, ontology graph, and saliency remain
crate-private (`kask/crates/hkask-condenser/src/hkask_condenser.rs:39-43`).

## Public types and methods

| Surface | Purpose | Evidence |
| --- | --- | --- |
| `CondenserEngine::new` / `Default::default` | create an engine storing only `Profile::Normal`; `default` delegates to `new` | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::new` and `impl Default for CondenserEngine` |
| `CondenserEngine::compress` | classify, select, compress, and report metrics | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress` |
| `CondenserEngine::set_profile` / `profile` | mutate/read the active profile | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::{set_profile, profile}` |
| `Profile` | heavy, normal, soft, light budgets | `kask/crates/hkask-condenser/src/types.rs:27-82` |
| `ContextCategory` | eight dispatch categories | `kask/crates/hkask-condenser/src/types.rs:84-96` |
| `CompressedOutput` | content, route, profile, size, reduction, signals | `kask/crates/hkask-condenser/src/types.rs:129-145` |
| `CondenserHealthSignal` | non-fatal algorithm anomaly data | `kask/crates/hkask-condenser/src/types.rs:147-169` |

## Class and integration diagram

```mermaid
classDiagram
    class CondenserEngine {
        -profile: Profile
        +new()
        +compress(tool, output, category) CompressedOutput
        +set_profile(profile)
        +profile() Profile
    }
    class CondenserAlgorithm {
        <<interface>>
        +name()
        +compress(input, profile, category, anchor)
        +compress_budgeted(input, lines, budget, anchor)
    }
    class RtkStyleAlgorithm
    class WordRankAlgorithm
    class FlashrankAlgorithm
    class BridgeThreadCondenser {
        +compress_tool_result(tool, output)
    }
    class NativeCompaction {
        +stream_compaction()
    }
    CondenserEngine ..> RtkStyleAlgorithm : selects
    CondenserEngine ..> WordRankAlgorithm : selects
    CondenserEngine ..> FlashrankAlgorithm : selects
    CondenserEngine ..> CondenserAlgorithm : invokes via borrowed trait object
    RtkStyleAlgorithm ..|> CondenserAlgorithm
    WordRankAlgorithm ..|> CondenserAlgorithm
    FlashrankAlgorithm ..|> CondenserAlgorithm
    BridgeThreadCondenser --> CondenserEngine
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COND-003
verified_date: 2026-10-20
verified_against: kask/crates/hkask-condenser/src/engine.rs (CondenserEngine, impl Default for CondenserEngine, CondenserEngine::{new, compress, set_profile, profile}); kask/crates/hkask-condenser/src/algorithms.rs (CondenserAlgorithm and its three implementations)
status: VERIFIED
-->

## Profiles

| Profile | Retention | Maximum lines | Evidence |
| --- | ---: | ---: | --- |
| `Heavy` | 0.10 | 30 | `kask/crates/hkask-condenser/src/types.rs:39-55` |
| `Normal` | 0.20 | 80 | `kask/crates/hkask-condenser/src/types.rs:39-55` |
| `Soft` | 0.60 | 200 | `kask/crates/hkask-condenser/src/types.rs:39-55` |
| `Light` | 0.95 | none | `kask/crates/hkask-condenser/src/types.rs:39-55` |

Inputs below `MIN_COMPRESSIBLE_LINES` (8 lines) pass through uncompressed at
every profile (`kask/crates/hkask-condenser/src/algorithms.rs`).

## Algorithm routes

The engine stores only the active profile. Its exhaustive category match
borrows the selected stateless algorithm as `&dyn CondenserAlgorithm`, through
which it invokes compression. `Unknown` is an explicit match alternative, not
an algorithm-selection fallback:

| Algorithm | Categories | Evidence |
| --- | --- | --- |
| `&RtkStyleAlgorithm` | `ShellCommand`, `TestOutput`, `BuildOutput` | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress` |
| `&WordRankAlgorithm` | `ConversationHistory`, `LogOutput` | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress` |
| `&FlashrankAlgorithm` | `FileContents`, `StructuredData`, `Unknown` | `kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress` |

`classify_tool` performs exact token matching before substring fallback
(`kask/crates/hkask-condenser/src/algorithms.rs`, `classify_tool`). An explicit
category overrides tool-name classification. The engine derives an ontology
anchor from the tool name before invoking the selected algorithm
(`kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress`).
The retained `CondenserAlgorithm::compress` default body shares line collection,
profile budgeting, and unchanged-input passthrough before calling the selected
implementation's `compress_budgeted`
(`kask/crates/hkask-condenser/src/algorithms.rs`, `CondenserAlgorithm`).

## Runtime bridge contract

`BridgeThreadCondenser` owns a mutex-protected engine and the incoming-result
auto-compression flag
(`kask/crates/kask_bridge/src/condenser_bridge.rs:19-39`). Its single method's
gate:

| Method | Gate | Mutation target | Evidence |
| --- | --- | --- | --- |
| `compress_tool_result` | `auto_compress_tool_results` must be true | incoming stored result text | `kask/crates/kask_bridge/src/condenser_bridge.rs:42-73` |

The exact protected source-tool list is at `crates/agent/src/thread.rs`
(`NO_COMPRESS_TOOLS`) — an ingestion policy.

## Compaction

Compaction does not use this package. The deterministic pre-shrink
(run-length collapse + head+tail windowing) is owned by
`crates/agent/src/kask_compaction.rs`, runs in-process on the summarizer's
request copy for both manual and automatic compaction, and is specified in
`kask/docs/architecture/compaction-pipeline-spec.md`. The composition root
installs the bridge for ingestion only
(`crates/zed/src/main.rs`).

## Diagnostics

The engine emits diagnostic `hkask.condenser` events for routing and reduction;
these are not `reg.*` feedback signals
(`kask/crates/hkask-condenser/src/engine.rs`, module telemetry documentation
and `CondenserEngine::compress`). Health signals are
returned with content and describe anomalies rather than turning them into
compression failures (`kask/crates/hkask-condenser/src/types.rs:141-169`).

## Procedures

### Tune compression

Use this procedure to change compression aggressiveness and verify the runtime
entry point without changing native summary persistence.

```mermaid
flowchart TD
    A[Choose profile] --> B[Verify category-to-algorithm route]
    B --> C[Test incoming-result behavior]
    C --> D[Verify protected content remains unchanged]
    D --> E[Run focused tests]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COND-002
verified_date: 2026-09-28
verified_against: kask/crates/hkask-condenser/src/types.rs:27-78; kask/crates/hkask-condenser/src/engine.rs (CondenserEngine::{new, compress, set_profile, profile}); kask/crates/kask_bridge/src/condenser_bridge.rs:42-125; crates/agent/src/thread.rs:3687-3715
status: VERIFIED
-->

#### 1. Choose a profile

Set `kask.condenser.profile` to one of the four values parsed by `Profile`
(`kask/crates/hkask-condenser/src/types.rs:27-82`):

| Profile | Retention | Maximum lines |
| --- | ---: | ---: |
| `heavy` | 10% | 30 |
| `normal` | 20% | 80 |
| `soft` | 60% | 200 |
| `light` | 95% | none |

Inputs below `MIN_COMPRESSIBLE_LINES` (8 lines) pass through uncompressed at
every profile — a fenced one-line terminal result is three lines and must
reach the agent intact (`kask/crates/hkask-condenser/src/algorithms.rs`).

The bridge defaults to `normal`; incoming-result compression defaults off
(`kask/crates/kask_bridge/src/settings.rs:301-327`). The composition root
installs the condenser even when that flag is false so enabling it later needs
no restart beyond the settings observer
(`crates/zed/src/main.rs`).

#### 2. Verify algorithm selection

Run a representative output through `CondenserEngine::compress`. Without an
explicit category, `classify_tool` classifies the tool name; the engine's
exhaustive match selects the algorithm
(`kask/crates/hkask-condenser/src/engine.rs`, `CondenserEngine::compress`;
`kask/crates/hkask-condenser/src/algorithms.rs`, `classify_tool`). Expected routes are:

- shell/test/build → `rtk_style`;
- conversation/log → `word_rank`;
- file/structured/unknown → `flashrank`.

The mappings are declared in `CondenserEngine::compress`
(`kask/crates/hkask-condenser/src/engine.rs`).

#### 3. Test incoming tool-result compression

With `auto_compress_tool_results = false`, verify
`compress_tool_result(tool, output)` returns the original. Enable it and verify
the result is no larger than the source. Short inputs (below
`MIN_COMPRESSIBLE_LINES`) must return the original at every profile. The gate
and dispatch are at
`kask/crates/kask_bridge/src/condenser_bridge.rs:42-73`; focused tests are at
`kask/crates/kask_bridge/src/condenser_bridge.rs:75-126`.

#### 4. Verify preservation boundaries

Confirm the following remain byte-for-byte unchanged at ingestion:

- user and assistant prose (never routed through the condenser);
- tools in `NO_COMPRESS_TOOLS` (`crates/agent/src/thread.rs`);
- non-text content.

#### 5. Validate

Run focused tests for `hkask-condenser` and `kask_bridge` condenser behavior.
Inspect `CompressedOutput.reduction_pct` and
`health_signals`, defined at
`kask/crates/hkask-condenser/src/types.rs:129-169`. A smaller line-level output
is evidence of reduction, not proof that the provider's token ceiling is met.
For the compaction pipeline's own validation, see
`kask/docs/architecture/compaction-pipeline-spec.md`.

## Further reading

- [Condenser explanation](./explanation.md)
