---
title: "hkask-condenser — Tutorial: Compressing Your First Tool Output"
audience: [developers new to hkask-condenser]
last_updated: 2026-08-28
version: "1.2.0"
status: "Active"
domain: "Condensation"
mds_categories: [lifecycle]
---

# hkask-condenser — Tutorial: Compressing Your First Tool Output

This tutorial walks through compressing an agent tool-result output with
the `CondenserEngine`. The condenser reduces verbose tool output (shell
commands, logs, file dumps) to a smaller form that fits the model's context
window while preserving salient lines. The crate is pure domain logic —
no MCP, no HTTP, no async — so the tutorial runs entirely in-process.

## Learning path

```mermaid
flowchart TD
    A[Step 1: Construct CondenserEngine] --> B[Step 2: Provide tool name and output]
    B --> C[Step 3: Call compress]
    C --> D[Step 4: Read CompressedOutput fields]
    D --> E[Step 5: Inspect health_signals]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-COND-001
verified_date: 2026-08-28
verified_against: kask/crates/hkask-condenser/src/engine.rs:41,48,86; kask/crates/hkask-condenser/src/types.rs:154,177
status: VERIFIED
-->

## Step 1: Construct the engine

Construct a `CondenserEngine` via `CondenserEngine::new()`
(`engine.rs:41`). The engine owns an `AlgorithmRegistry`
(`algorithms.rs:463`) constructed once and immutable, plus the active
`Profile` (`types.rs:29`), which defaults to `Profile::Normal`
(`engine.rs:44`).

```rust
use hkask_condenser::engine::CondenserEngine;

let mut engine = CondenserEngine::new();
```

## Step 2: Provide the tool name and output

The engine takes the tool name and the raw output string. The tool name
drives two derivations: the `ContextCategory` (`types.rs:110`) via
`classify_tool` (`algorithms.rs:518`), and the `OntologyAnchor` via
`select_ontology_anchor` (`kask/crates/hkask-bridge-ontology/src/axis.rs:210`),
called inside `compress` (`engine.rs:60`). You pass neither explicitly —
both are derived from the tool name.

## Step 3: Call compress

Call `engine.compress(tool_name, output, category)` (`engine.rs:48`).
Pass `None` for `category` to let `classify_tool` derive it; pass a
`Some(ContextCategory)` to override classification. The engine selects
an algorithm via `AlgorithmRegistry::select` (`algorithms.rs:483`),
which walks the registry in registration order and returns the first
algorithm whose `default_for()` contains the category.

```rust
let output = "line1\nline2\nline3\n".repeat(20);
let result = engine.compress("bash_execute", &output, None);
// result.algorithm == "rtk_style", result.category == "shell_command"
// (pinned by the engine test at engine.rs:123-131)
```

## Step 4: Read the CompressedOutput

The returned `CompressedOutput` (`types.rs:154`) carries:

| Field | Meaning |
|-------|---------|
| `content` | The compressed text |
| `algorithm` | Algorithm name (`rtk_style`, `word_rank`, `flashrank`) |
| `category` | Resolved `ContextCategory` label |
| `profile` | Active `Profile` label |
| `original_lines` / `compressed_lines` | Line counts before/after |
| `original_bytes` / `compressed_bytes` | Byte counts before/after |
| `reduction_pct` | `100 * (1 - compressed_bytes / original_bytes)` |
| `health_signals` | Diagnostics for unexpected algorithm behavior |

The reduction is computed at `engine.rs:77-81` and the `CompressedOutput`
is assembled at `engine.rs:86`.

## Step 5: Inspect health_signals

`health_signals` (`types.rs:177`) is empty when the algorithm performed
within expected bounds. The three signal types are
`negative_compression` (rtk_style), `low_signal` (word_rank), and
`budget_shortfall` (flashrank) — see the `signal_type` field doc
(`types.rs:180`). They are diagnostic ν-event candidates for the
Regulation layer — they indicate deviation, not failure (content is
still returned). Promoting them to real ν-events would require a `reg.*`
namespace and a wired consumer, neither of which exists today
(`types.rs:170-175`).

## Source citations

| Symbol | Location |
|--------|----------|
| `CondenserEngine::new` | `kask/crates/hkask-condenser/src/engine.rs:41` |
| `CondenserEngine::compress` | `kask/crates/hkask-condenser/src/engine.rs:48` |
| `AlgorithmRegistry` | `kask/crates/hkask-condenser/src/algorithms.rs:463` |
| `AlgorithmRegistry::select` | `kask/crates/hkask-condenser/src/algorithms.rs:483` |
| `classify_tool` | `kask/crates/hkask-condenser/src/algorithms.rs:518` |
| `select_ontology_anchor` | `kask/crates/hkask-bridge-ontology/src/axis.rs:210` |
| `Profile` enum | `kask/crates/hkask-condenser/src/types.rs:29` |
| `ContextCategory` enum | `kask/crates/hkask-condenser/src/types.rs:110` |
| `CompressedOutput` | `kask/crates/hkask-condenser/src/types.rs:154` |
| `CondenserHealthSignal` | `kask/crates/hkask-condenser/src/types.rs:177` |

## See also

- [hkask-condenser Reference](./reference.md): class diagram of algorithms
  and types.
- [hkask-condenser How-to](./how-to.md): tuning profiles and keyword
  weights.
- [hkask-condenser Explanation](./explanation.md): the compression cycle
  and ontology anchoring rationale.

---

[^salience]: Itti, L., Koch, C., & Niebur, E. (1998). *A model of saliency-based visual attention for rapid scene analysis.* IEEE Transactions on Pattern Analysis and Machine Intelligence, 20(11), 1254–1259. <https://ieeexplore.ieee.org/document/730558>. The saliency model adapted for text compression.
