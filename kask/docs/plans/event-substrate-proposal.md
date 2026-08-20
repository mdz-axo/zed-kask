# Event Substrate Proposal — Porting Agent Lightning's Data Plane to Kask

Status: PROPOSAL (analysis complete, no code written)
Date: 2026-08-20
Origin: comparative analysis of `microsoft/agent-lightning` (v1.0, main) vs. the
post-strip-down kask regulation system. Code citations are from the current tree.

## Problem

The regulation loop's sensing and alerting survived the strip-down; loop closure
mostly did not:

- `verify_impact` handles only `EnergyRemaining` and `VarietyDeficit` — every
  other `RegulationData` variant hits `_ => continue`
  (`kask/crates/hkask-regulation/src/cybernetics_loop.rs:1302-1315`).
- Impact reports, skill feedback spans, tool stats, and delegation results each
  live in different structures with no common substrate, so no component can
  query "what happened and what did we do about it" in one place.
- The grounding/verification layer was removed; comments referencing
  `enforce_and_stamp` / `enforce_grounding`
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:97,198,327`) and the
  always-`None` envelope path (`local_runtime.rs:438`, consumed at
  `local_tools.rs:347-352`) are dead surface — lying documentation.
- The existing learners (`StrategyEvaluator`, `StagnationDetector`,
  `verify_impact`) have learning logic but starve for training data.

Agent Lightning's core architectural insight applies directly: **the loop is
only as good as the event substrate everything reads from and writes to.** In
AL, the reward event closes the loop because it lands in the same store as the
trajectory it judges. Kask has no such unification.

## What we adopt (and what we don't)

**Adopt: AL's data plane** — event store, gateway capture, rollout lifecycle,
evaluator hooks. Ported as Rust core services.

**Do not adopt: AL's trainer** — batch policy-gradient updates (verl, vLLM,
GPUs) are incompatible with a 10s live tick in an IDE-embedded binary, and a
frozen trained policy is *less* adaptive than the current rule table. Kask's
existing bandit-style learners (`StrategyEvaluator` accept/stage/block,
`StagnationDetector` substitution ladder) are the live-system replacement for a
trainer. What they lack is not learning logic — it's gradients to eat. The
event substrate is how they get them.

**Optional later phase:** an offline replay job over the event store to tune
agent cards / model selection (AL's trainer pattern, legitimately offline).
Deferred until there is event history worth replaying. Falsifier: if the event
store lands and the existing evaluators still can't close loops, the trainer
was never the problem and the offline job is moot.

## Service map

| AL module | Rust core service | Seam | Notes |
|---|---|---|---|
| `schemas.py` + `store.py` | `hkask-event-store` (new crate) | additive workspace member | Append-only log on SQLite via existing `hkask-storage::DatabaseDriver` |
| `proxy.py` capture | Capture hook in `kask_bridge::inference_ipc_server` | **D8** | Every inference call → `model_request` event. Zero changes to callers — the proxy is the sense organ |
| `Rollout` lifecycle | Rollout identity on delegations/turns | **D3** | State machine with enforced transitions, attempts, idempotent creation |
| `hooks.py` + `RewardData` | Promote `TaskSuccessVerdict` / `run_evaluator` to a declared per-card evaluator contract | **D3** (swarm MCP server) | Verdicts append as events with `source` provenance |
| `trainer.py` | Offline replay job (phase 4, optional) | new, offline | Only place batch logic is legitimate |

## The one hard modeling decision: what is a rollout?

A rollout is **the unit that has a lifecycle and a judge**. Proposal:

- **Rollout = a swarm delegation or a curator/user turn.** Both have terminal
  states and can carry verdicts.
- **Events within a rollout:** inference calls (`model_request`), tool
  invocations, regulatory actions, and verdicts.
- Inference calls and tool invocations are NOT rollouts — they are events
  inside one. Modeling them as rollouts would explode cardinality and break
  the compaction story.

Getting this wrong poisons everything downstream. This decision should be
confirmed before any code is written.

## Event model (AL's discipline, applied)

Two well-known event kinds; everything else is opaque pass-through:

1. `model_request` — captured automatically at the inference IPC boundary.
   Payload: model, model_version, request shape (not full body — see
   retention), response status, latency_ms, usage, finish_reason, retry_count.
2. `verdict` — reported by an evaluator (deterministic per-card evaluator,
   operator feedback, or the metacognition loop's impact judgment). Payload:
   value (scalar or pass/fail), source (provenance: `deterministic_evaluator`
   | `operator` | `regulation_impact`), reason.

Opaque pass-through for everything else (skill spans, tool stats) — the store
does not parse what it doesn't need to. Position in the log is identity; no
separate event ID (AL's `schemas.py` pattern).

Store interface target (~4 functions — the deletion test applied up front):
`append`, `query`, `compact`, `cursor`. If the interface grows past this, the
design is wrong.

## The loop this enables

```
events → sensors (derived views) → deviations → actions → verdicts → events
```

Verdicts of regulatory actions are themselves events. That is the closure AL
has and kask lacks: `verify_impact` stops being a special-case struct walker
and becomes a query — "for rollout R, what was the metric before action A and
after it?"

## Risks and mitigations

1. **Volume.** AL is batch and deletes completed rollouts before polling
   (`agl_rollout_manager.py`). A live IDE logs continuously. Mitigation, from
   day one: terminal rollouts compact to summaries; hard cap on event count;
   drop-oldest with a *counted* drop (never silent — the broken-feedback-loop
   rule: absence must be distinguishable from zero).
2. **Hot-path writes.** The inference IPC boundary is hot. Capture must be
   fire-and-forget over a bounded channel; it must never block a generation
   call. Backpressure drops are counted and surfaced as a sensor signal, not
   swallowed.
3. **Over-building.** AL's entire data plane is ~700 lines of Python. If the
   Rust port of store + lifecycle exceeds a few hundred lines, the design is
   wrong. Budget: `hkask-event-store` ≤ ~500 lines including tests.
4. **Dead-surface cleanup is part of this work, not separate.** The stale
   `enforce_and_stamp` comments, the always-`None` envelope path, and the
   unreachable `Grounding*` alert arms (`cybernetics_loop.rs:1166-1184`) get
   removed in the same PR series — per the strip-down's own logic, dead surface
   is lying documentation.

## Build order

1. **`hkask-event-store`** — schema, append/query/compact/cursor, retention.
   Additive crate; no callers yet.
2. **IPC capture** — `model_request` events at the inference boundary (D8).
   Fire-and-forget channel; counted drops.
3. **Evaluator contract** — promote `TaskSuccessVerdict`/`run_evaluator` to
   per-card declared evaluators; verdicts append as events (D3).
4. **Rewire regulation** — `verify_impact` and the metacognition drift
   detector read from the store; impact verdicts write back to it.
5. **(Optional, gated)** offline replay job — only if 1-4 land and the
   evaluators still can't close loops.

Each phase is independently shippable and independently deletable — the
strangler-fig discipline: the old paths keep working until the new substrate
provably replaces them.

## Divergence surface impact

- **D8** (bridge + adapters): inference IPC capture hook.
- **D3** (hKask tools in-process): rollout identity on delegations; evaluator
  contract in the swarm MCP server.
- New crate is additive (workspace member), no upstream files touched.
- Per `.rules` hygiene: `.rules` updates (if any) are proposed in the PR
  description, not edited inline.
