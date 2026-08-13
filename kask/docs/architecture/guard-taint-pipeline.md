---
title: "Guard and Taint Pipeline — ContentGuard, GuardedInferencePort, FIDES tool taint (both removed)"
audience: [architects, developers, agents]
last_updated: 2026-08-12
version: "0.2.0"
status: "Deprecated"
domain: "architecture"
mds_categories: [trust, composition, domain]
---

> **DEPRECATED — nothing documented here is live.** Both halves of this pipeline
> have been deleted from the codebase. This document is retained as the record of
> what existed, why it was removed, and what bar a replacement must clear.
>
> - **Guard layer removed 2026-08-10.** The `hkask-guard` crate and every
>   component it held (`ContentGuard`, `GuardConfig`, `CanaryToken`,
>   `GuardedInferencePort`, `GuardedStream`, `Spotlighter`) are gone. The
>   `RoleOverride` scanner's bare `system:` substring pattern produced false
>   positives that blocked legitimate skill cascade template rendering, making the
>   guard a net-negative failure mode.
> - **FIDES taint / runtime-policy layer removed 2026-08-12.** `ToolTaint`,
>   `can_flow_to`, `DefaultPolicy`, `PolicyVerdict`, `PolicyConfig`,
>   `check_untrusted_input`, `collect_referenced_keys`, `with_runtime_policy`,
>   `ToolInfo.taint`, `StepResult.taint`, and `taint_of_key` no longer exist
>   anywhere in the tree. The gate was **deleted rather than repaired** — see
>   [Why the taint layer was deleted](#why-the-taint-layer-was-deleted). Governing
>   regression: `kask/security/regressions/RR-0053.yaml`, rewritten from a wiring
>   assertion into an **absence check** that forbids re-introducing an inert gate.
>
> **Defense Layer 5 (information flow control) is therefore ABSENT BY DECISION**,
> recorded the same way Layer 3 (instruction hierarchy) already is under RR-0010:
> de-advertised rather than deployed. This is a recorded architectural choice, not
> a regression. Layers 1, 2 and 8 are absent as a *consequence* of the guard
> deletion; Layer 5 is absent because an operator chose absence over pretence.

# Guard and Taint Pipeline (historical)

How untrusted content **was** marked, scanned, and gated as it crossed hKask's two
boundaries: the LLM I/O boundary (prompts in, completions out) and the tool
invocation boundary (context values into MCP tool arguments). The pipeline
combined two mechanisms — `ContentGuard` scanning (llm-guard pipelines) and
FIDES[^fides] information-flow taint labels — plus spotlighting[^spotlighting]
of tool outputs before they re-entered the LLM context.

Everything below is past tense on purpose. For what actually bounds tool calls
today, see [What bounds tool calls now](#what-bounds-tool-calls-now).

## Why the taint layer was deleted

The gate ran. It could not decide anything, because **both of its inputs were
constants.** This analysis is unchanged from the 2026-08-11 revision of this
document — it is precisely the justification the operator acted on:

1. **The untrusted-input flag was always `false`.** `check_untrusted_input` read
   taint from legacy `__taint__{key}` map markers, but the write path
   (`StepContext::store_result` / `store_named`) had stopped emitting those
   markers when taint moved to an inline `StepResult.taint` field. Read side and
   write side had silently drifted apart.
2. **Every MCP tool was labelled `Pure`.** `McpRuntime::get_tool_info` hardcoded
   `ToolTaint::Pure` at the only site that constructed a `ToolInfo`, so the `Sink`
   arm of the policy never matched.

With `has_untrusted_input` pinned false and no tool ever `Sink`, the one rule the
lattice encoded — block `Source` → `Sink` — **could not fire on any input**. The
supporting wiring test passed the whole time, because a wiring test proves a call
happens, not that the call can ever decide anything. That is the same failure
class as the vacuous per-call capability gate (RR-0056) and the call meter that
refused on a wiring omission (RR-0057): three surfaces, one lesson.

Repair was possible but was not chosen. The operator's reasoning: an inert gate is
worse than no gate, because it invites reliance on a protection that does not
exist, and every doc and audit downstream re-credits it. Deleting it makes the
absence legible.

**Deleted with the gate (2026-08-12):**

| Removed                                                          | Was at                                             |
| ---------------------------------------------------------------- | -------------------------------------------------- |
| `ToolTaint`, `can_flow_to`, `can_flow_to_matrix`                 | `hkask-capability/src/tool_taint.rs` (whole file)   |
| `DefaultPolicy`, `PolicyVerdict`, `PolicyConfig`                 | `hkask-regulation/src/runtime_policy.rs` (whole file) |
| `check_untrusted_input`, `collect_referenced_keys`               | `hkask-templates/src/step_actions.rs`              |
| `with_runtime_policy`, `runtime_policy_is_wired`                 | `hkask-templates/src/executor.rs`                  |
| `ToolInfo.taint`                                                 | `hkask-capability/src/tool_port.rs`                |
| `StepResult.taint`, `taint_of()`, the taint parameters on `StepContext::store_result` / `store_named` | `hkask-templates/src/step_context.rs` |
| `taint_of_key` on the `ContextLookup` trait                      | `hkask-templates/src/step_context.rs`              |

Three dependencies were orphaned by the deletion and removed with it: `serde`
from `hkask-capability/Cargo.toml`, `hkask-capability` from
`hkask-regulation/Cargo.toml`, and `hkask-regulation` from
`hkask-templates/Cargo.toml`.

Removal-rationale comments were left at each deletion site — in
`hkask-capability/src/hkask_capability.rs`, `hkask-templates/src/step_actions.rs`
(above `invoke_tool`), and `kask_bridge/src/skill_executor.rs` (above
`build_executor`) — so a future reader does not "restore" the gate without
reading why it went. RR-0053's grep pattern deliberately skips comment lines so
those comments do not trip its own absence check.

## Components (all removed)

| Component                         | Was at                                                | Role                                                                                                                                     |
| --------------------------------- | ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `ContentGuard`                    | `hkask-guard/src/pipeline.rs` (crate deleted)         | mandatory input/output scanner pair (injection, role override, token limit in; secrets + canary out)                                      |
| `GuardConfig` / `from_env`        | `hkask-guard/src/pipeline.rs`                          | scanner parameters (`HKASK_GUARD_TOKEN_LIMIT`, default 32 000) — presence was not configurable                                            |
| `CanaryToken`                     | `hkask-guard/src/pipeline.rs`                          | per-session 32-byte hex token embedded in system prompts; its appearance in output signalled prompt exfiltration (OWASP LLM07)             |
| `GuardedInferencePort`            | `hkask-guard/src/guarded_inference.rs`                 | `InferencePort` decorator: scanned input before delegation and output after; wrapped the primary port at the composition root             |
| `GuardedStream`                   | `hkask-guard/src/guarded_inference.rs`                 | streaming output accumulator; scanned on stream end and emitted a `finish_reason: "redacted"` chunk with sanitized text                   |
| `Spotlighter` / `SpotlightMode`   | `hkask-guard/src/spotlight.rs`                         | transformed untrusted tool output (`Delimit` default; `Datamark`; `Encode`) so the LLM treated it as data, not instructions               |
| `ToolTaint`                       | `hkask-capability/src/tool_taint.rs`                   | FIDES label lattice: `Source` / `Sink` / `Pure` / `Endorser`; `can_flow_to` blocked only `Source → Sink`                                  |
| `DefaultPolicy` / `PolicyVerdict` | `hkask-regulation/src/runtime_policy.rs`               | pre-execution gate: Allow / Block / RequireHuman / Log                                                                                    |
| `ManifestExecutor` taint fields   | `hkask-templates/src/executor.rs`                      | `spotlighter`, `runtime_policy`, `taint_labels` — the executor-side wiring of the pipeline                                                |

`ManifestExecutor` now carries no defense-layer fields at all: its struct holds
`inference`, `tools`, `default_params`, `template_renderer`, `terminal_check`,
`progress`, and `title`. `terminal_check` is a profile check (proposer/evaluator
separation), not part of this pipeline.

## Mechanism as it stood

### LLM boundary — GuardedInferencePort

`GuardedInferencePort` wrapped any `InferencePort`, and the composition root
wrapped the primary inference port once (built from `GuardConfig::from_env` +
`ContentGuard::mandatory`), making the boundary universal by construction rather
than per-caller opt-in. The manifest executor was wired with the guarded port.

- **Non-streaming** (`generate`, `generate_with_model`, …): input was scanned via
  `ContentGuard::scan_input` before delegation — prompt injection, role override,
  deobfuscated injection patterns, and the token limit; a violation returned
  `InferenceError::Generation` and emitted `reg.guard.input`. Output was scanned
  via `scan_output`: secret leakage and canary appearance; secret-bearing output
  was **redacted in place**, not rejected, and emitted `reg.guard.output`.
- **Streaming** (`generate_stream*`): input was scanned before delegation. Output
  scanning was **post-hoc**: `GuardedStream` forwarded chunks unchanged
  (preserving latency), accumulated up to `GUARD_ACCUMULATION_LIMIT = 256 KB`,
  and on stream end scanned the accumulated text, emitting a final
  `finish_reason: "redacted"` chunk containing the sanitized replacement.

> **The limitation that this design never solved:** `GuardedStream` was post-hoc
> redaction, not real-time blocking. The consumer may already have rendered the
> leaked text in real-time chunks; only the _stored_ version was sanitized. Any
> replacement output filter must decide this explicitly rather than inherit the
> ambiguity.

### Tool boundary — FIDES taint + DefaultPolicy

Every MCP tool carried a `ToolTaint` label. The FIDES policy was a single lattice
rule: `Source → Sink` was blocked by `can_flow_to`; every other flow was allowed,
and the full 4×4 matrix was pinned by a `can_flow_to_matrix` test. Inside the
manifest executor:

1. **Labeling.** `invoke_tool` returned each tool result together with the tool's
   taint label, stored in `taint_labels` under `step_{ordinal}_result`. Inert in
   practice: the label was always `Pure`.
2. **Spotlighting.** Before the result entered the LLM context it passed through
   `spotlight_tool_output`, applying the executor's `Spotlighter`
   (`SpotlightMode::Delimit`). Removed 2026-08-10 with `hkask-guard`.
3. **Propagation.** Taint was carried as a field on `StepResult.taint`, written by
   `StepContext::store_result` / `store_named` and read via `taint_of`. The
   earlier `propagate_taint_for_binding` had already been dropped in the
   `executor.rs` → `step_actions.rs` / `step_context.rs` / `step_machine.rs`
   refactor, leaving the gate reading `__taint__{key}` markers nobody wrote.
4. **Gating.** Before each tool invocation, `check_untrusted_input` scanned the
   bound input JSON for both `{"$ref": …}` objects and inline-Jinja
   `{{ step_N_result }}` strings, then `invoke_tool` called `DefaultPolicy::check`:
   `Block`/`RequireHuman` aborted the step, `Log` emitted a
   `reg.guard.runtime_policy` span, `Allow` proceeded. Inert for the two reasons
   in [Why the taint layer was deleted](#why-the-taint-layer-was-deleted).

```mermaid
flowchart TD
    subgraph LLM boundary — removed 2026-08-10
        A[Prompt input] --> G1[ContentGuard.scan_input]
        G1 -->|violation| REJ[InferenceError::Generation<br/>reg.guard.input]
        G1 -->|clean| LLM[InferencePort]
        LLM --> G2[scan_output / GuardedStream]
        G2 -->|secret or canary| RED[redacted stored version<br/>reg.guard.output]
        G2 -->|clean| OUT[completion]
    end
    subgraph Tool boundary — removed 2026-08-12
        TOOL[MCP tool result] --> LAB[label with ToolTaint<br/>always Pure — inert]
        LAB --> SPOT[Spotlighter Delimit]
        SPOT --> CTX[(cascade context<br/>taint_labels)]
        CTX --> INS[context.insert]
        INS --> GATE[check_untrusted_input<br/>always false — inert]
        GATE --> POL{DefaultPolicy}
        POL -->|Block / RequireHuman<br/>unreachable| STOP[step aborted]
        POL -->|Allow / Log| CALL[tool invoke]
    end
```

## What bounds tool calls now

No information-flow check runs on any tool invocation path. `invoke_tool` in
`hkask-templates/src/step_actions.rs` resolves the tool's server via
`get_tool_info` and dispatches through `ToolPort::invoke`. What remains:

| Bound                                    | Enforcement point                                                                                             |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Which tools a caller may reach at all    | per-request `tool_allowlist` on the inference IPC `tool_invoke` dispatch (`kask_bridge/src/inference_ipc_server.rs`, fail-closed on missing/empty) |
| Which tools a swarm agent may reach      | each agent card's declared `mcp_tools` allowlist (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs`)     |
| Which credentials a server process sees  | per-server MCP env / credential allowlists (`kask_bridge/src/mcp_servers.rs`, RR-0038)                          |
| Runaway tool loops                       | per-tick call ceiling charged in `McpRuntime::invoke` (`ToolPortError::EnergyBudgetExceeded`)                   |
| Runaway cascade recursion                | `SYSTEM_MAX_RECURSION` (`hkask-capability/src/token_types.rs`)                                                 |

The first three are authority *separation* — lists written by an actor other than
the one they constrain. The last two are breakers and meters, not authorization,
and none of them inspect data flow. **Treat every tool path as taint-unaware.**

## The bar for a replacement

RR-0053 states the conditions a real information-flow gate must meet. Restating
them here so a future implementer does not have to reverse-engineer them from a
grep pattern:

- Tool taint labels derived from something real (per-tool metadata or a server
  declaration) — **not** hardcoded at the `ToolInfo` construction site.
- An untrusted-input signal read from the **same field the write path sets**, with
  a test that fails if read and write drift apart. This drift is what killed the
  original.
- A test proving that a `Source`-labelled input to a `Sink` tool is actually
  **blocked**, on a path production can reach — not a test proving a function was
  called.

RR-0053 must then be rewritten to assert that behavior. Re-adding the type names
does not satisfy it; the entry's detection is an absence check precisely so a
cosmetic restoration fails the gate.

## Superseded regressions

| Entry                                              | Status               |
| -------------------------------------------------- | -------------------- |
| RR-0053 (FIDES taint / runtime policy)             | `enforced` — rewritten 2026-08-12 as an absence check |
| RR-0012, RR-0013, RR-0026, RR-0027, RR-0033, RR-0034 | `obsolete` — all measured plumbing around a gate with no live inputs |
| RR-0010 (Layer 3 instruction hierarchy)            | `retired` — the precedent for de-advertising rather than deploying |
| RR-0011 (spotlighting), RR-0014, RR-0023, RR-0024, RR-0030 (output filtering), RR-0055 (`reg.guard.*` spans) | `obsolete` — removed with `hkask-guard` |

---

[^fides]: Microsoft Research. (2025). _FIDES: Information flow control for LLM agents_ (arXiv:2505.23643). The Source/Sink/Pure/Endorser taint lattice and the Source→Sink endorsement rule. hKask's implementation of this lattice was deleted on 2026-08-12; the citation is retained as the academic source for the design, not as a claim that the design is deployed.

[^spotlighting]: Microsoft Research. (2024). _Defending LLMs against prompt injection with spotlighting_ (arXiv:2403.14720). The delimit/datamark/encode transforms. hKask's implementation (`hkask-guard`'s `Spotlighter`) was deleted on 2026-08-10; the citation is retained as an academic source only.

[^rlm-overthinking]: Wang, D. (2026). _Think, But Don't Overthink: Reproducing Recursive Language Models_ (arXiv:2603.02615v1). Documents the "parametric hallucination" failure mode at RLM recursion depth=2: models abandon input context and emit pre-trained constants from parametric memory (§4.4). This was the empirical argument for taint surviving `input_mapping` binding. It is recorded here as an unaddressed risk: with taint deleted, nothing anchors context provenance across a cascade binding, and the only remaining defense against runaway depth is the `SYSTEM_MAX_RECURSION` breaker. The paper also documents the `<thinking>` tag format-collapse failure (Appendix A.4) that `normalize_model_output` defends against — that defense is unaffected.
