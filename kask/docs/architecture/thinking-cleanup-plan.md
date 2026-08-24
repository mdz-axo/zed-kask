---
title: "Thinking Control Cleanup Plan"
audience: [developers, architects]
last_updated: 2026-08-24
version: "0.2.0"
status: "Draft"
domain: "Application"
mds_categories: [composition, lifecycle]
---

# Thinking Control Cleanup Plan

## Problem

8 variables across 62 files control a single concept: is the model allowed to
think (reason internally) or not. The variables are layered across 4 abstraction
boundaries (MCP server, IPC client, IPC server, zed provider), each adding
its own gating logic. The final gate depends on model metadata that
OpenAI-compatible providers don't populate, breaking the chain.

## Current State

### Variables (8 total)

| # | Variable | Location | Type | Meaning | Used by |
|---|----------|----------|------|---------|---------|
| 1 | `disable_thinking` | `LLMParameters` (hkask-types) | `bool` | MCP server: "don't think" | 12 files |
| 2 | `thinking_allowed` | `LanguageModelRequest` (language_model_core) | `bool` | Zed: "is thinking allowed?" | 23 files |
| 3 | `thinking_effort` | `LanguageModelRequest` (language_model_core) | `Option<String>` | UI effort selector (was supposed to be iterations) | 29 files |
| 4 | `thinking_enabled` | `Thread` (agent) | `bool` | Conversation: thinking on/off | 10 files |
| 5 | `reasoning_effort` | `AvailableModel` (open_ai) | `Option<ReasoningEffort>` | Model metadata: default reasoning level | 23 files |
| 6 | `ReasoningEffort` | `open_ai` | enum | API parameter: None/Minimal/Low/Medium/High/XHigh/Max | 20 files |
| 7 | `supports_thinking` | `LanguageModel` trait | `fn -> bool` | Does model support thinking? | 38 files |
| 8 | `supports_disabling_thinking` | `LanguageModel` trait | `fn -> bool` | Can thinking be turned off? | 9 files |

### The translation chain (IPC path)

```
corpus_tag_chunks sets LLMParameters.disable_thinking = true
  → InferenceIpcClient serializes to JSON, sends over UnixStream
    → IPC server deserializes, calls LanguageModelInferencePort::generate_with_model
      → build_request translates: thinking_allowed = !disable_thinking
        → Provider reads thinking_allowed
          → chat_completion_reasoning_effort checks:
            1. model.reasoning_effort == None? → return None (skip)
            2. request.thinking_allowed? → use thinking_effort or model default
            3. else supports_none_reasoning_effort(model)? → return None
            4. else → return None (thinking NOT disabled — THE BUG)
```

Step 4 is the bug: `supports_none_reasoning_effort` checks
`model.reasoning_effort.is_some()`, which is `false` for OpenAI-compatible
models whose metadata doesn't include `reasoning_effort`. So the provider
returns `None` (no reasoning_effort parameter), and the model thinks.

### The direct path (bypass)

```
corpus_tag_chunks sets LLMParameters.disable_thinking = true
  → DirectEmbeddingPort::generate_with_model
    → sends reasoning: {"effort": "none"} directly in the API body
```

One hop. No translation chain. No model metadata gate. Works.

### IPC path impedance (6 hops vs 1)

The IPC path adds:
1. JSON serialize (InferenceIpcClient)
2. UnixStream connect + write
3. JSON deserialize (IPC server)
4. Channel send (LanguageModelInferencePort → GPUI foreground task)
5. Channel receive + model.stream() dispatch
6. JSON serialize (provider → API)

The direct path is a single HTTP call. The IPC path adds 5 extra hops,
each with serialization, I/O, or scheduling overhead.

## Target State

### Collapse to 1 variable

**`thinking_allowed: bool`** on `LanguageModelRequest`. This is the single
source of truth.

- `false` → provider sends `reasoning_effort: "none"` to the API. Period.
  No model metadata gate. No `supports_none_reasoning_effort` check.
- `true` → provider sends the model's default reasoning effort (from
  `AvailableModel.reasoning_effort`) or no parameter if the model has no
  reasoning support.

### Remove

| Variable | Action | Reason |
|----------|--------|--------|
| `disable_thinking` | **Remove** from `LLMParameters` | Inverted duplicate of `thinking_allowed`. MCP server sets `thinking_allowed: false` directly via the IPC params. |
| `thinking_effort` | **Keep** but rename to `reasoning_iterations: Option<u32>` | Was supposed to be iterations, not thinking level. The UI menu maps iterations to the API's reasoning_effort levels. |
| `thinking_enabled` | **Remove** from `Thread` | Redundant with `thinking_allowed` on the request. The thread sets `thinking_allowed` when building the request. |
| `supports_disabling_thinking` | **Remove** from `LanguageModel` trait | Redundant with `supports_thinking`. If the model supports thinking, it supports disabling it. |
| `supports_none_reasoning_effort` | **Remove** | Model metadata gate that broke the chain. If `thinking_allowed == false`, send `None`. |

### Keep

| Variable | Action | Reason |
|----------|--------|--------|
| `thinking_allowed` | **Keep** — the one variable | Single source of truth: is thinking allowed? |
| `reasoning_effort` | **Keep** on `AvailableModel` | Model metadata: the model's default reasoning level. Used when `thinking_allowed == true`. |
| `ReasoningEffort` | **Keep** enum | The API parameter. |
| `supports_thinking` | **Keep** on `LanguageModel` trait | Does the model support thinking at all? Used for UI to show/hide the thinking toggle. |

### Result: 4 variables (down from 8)

| Variable | Meaning |
|----------|---------|
| `thinking_allowed: bool` | The one switch. On the request. |
| `reasoning_iterations: Option<u32>` | Iterations (renamed from `thinking_effort`). On the request. |
| `ReasoningEffort` enum | The API parameter. Derived from `thinking_allowed` + `reasoning_iterations`. |
| `supports_thinking` | Model capability. Used for UI only. |

### Provider logic (simplified)

```rust
fn chat_completion_reasoning_effort(
    request: &LanguageModelRequest,
    model: &AvailableModel,
) -> Option<open_ai::ReasoningEffort> {
    if !request.thinking_allowed {
        return Some(open_ai::ReasoningEffort::None);
    }
    // Thinking allowed — use the model's default or the request's iterations.
    default_thinking_reasoning_effort(model)
}
```

5 lines. No `supports_none_reasoning_effort` check. No model metadata gate.
If the caller says no thinking, the model doesn't think.

## Execution Plan

### Phase 1: Fix the provider (small, high-impact)

1. `open_ai_compatible.rs`: `chat_completion_reasoning_effort` — when
   `thinking_allowed == false`, always return `ReasoningEffort::None`.
   Remove `supports_none_reasoning_effort` from this path. **DONE** (committed,
   needs rebuild).

2. `kask_bridge/src/inference_chat.rs`: `build_request` — set
   `thinking_allowed: !parameters.disable_thinking`. Remove the
   `thinking_effort` setting (redundant — the provider handles it).
   **DONE** (committed, needs rebuild).

### Phase 2: Collapse `disable_thinking` → `thinking_allowed` (medium)

1. Add `thinking_allowed: bool` to `InferenceParams` (the IPC wire type).
2. MCP servers set `thinking_allowed` directly instead of
   `LLMParameters.disable_thinking`.
3. IPC server passes `thinking_allowed` through to `build_request`.
4. Remove `disable_thinking` from `LLMParameters`.
5. Update all 12 files that reference `disable_thinking`.

### Phase 3: Rename `thinking_effort` → `reasoning_iterations` (medium)

1. Rename `LanguageModelRequest.thinking_effort` to
   `reasoning_iterations: Option<u32>`.
2. Update the UI menu to map iterations to `ReasoningEffort` levels.
3. Update `selected_thinking_reasoning_effort` to parse `u32` instead of
   `String`.
4. Update all 29 files that reference `thinking_effort`.

### Phase 4: Remove `thinking_enabled` from `Thread` (small)

1. `Thread` sets `thinking_allowed` on the request directly.
2. Remove `thinking_enabled` field and its propagation.
3. Update all 10 files that reference `thinking_enabled`.

### Phase 5: Remove `supports_disabling_thinking` (small)

1. Remove from `LanguageModel` trait.
2. Replace all checks with `supports_thinking`.
3. Update all 9 files.

### Phase 6: Remove `supports_none_reasoning_effort` (trivial)

1. Remove the function from `open_ai_compatible.rs`.
2. Already removed from the `chat_completion_reasoning_effort` path in Phase 1.

### Phase 7: IPC path impedance audit (larger)

1. Profile the IPC path: measure each hop's latency.
2. Eliminate unnecessary serialization (e.g. pass `InferenceParams` directly
   instead of JSON round-trip if the types align).
3. Consider making the direct path primary for embedding + generation,
   with IPC bridge only for tool dispatch and worktree spawn.
4. This is a larger architectural change — defer to a separate task.

## Validation

Each phase compiles clean, passes clippy, and passes tests. After all phases:
- 4 variables (down from 8)
- 1 code path for thinking control (down from 4-layer chain)
- Provider sends `reasoning_effort: none` when `thinking_allowed == false` — no gate
