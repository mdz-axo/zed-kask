# Event Substrate Proposal — An Agent Lightning-Style Harness for Local Swarm Agents

Status: PROPOSAL (analysis complete, no code written)
Date: 2026-08-20 (revised: swarm-harness-first framing)
Origin: comparative analysis of `microsoft/agent-lightning` (v1.0, main) vs. the
post-strip-down kask regulation system. Code citations are from the current tree.

## Thesis

Port Agent Lightning's **data plane** — rollout lifecycle, event store, capture
at the inference boundary, evaluator hooks — as Rust core services whose
**primary consumer is a rollout harness for local swarm agents**. The harness
runs an agent card against a task set N times, captures trajectories, stamps
verdicts, and reports pass rates with variance. The same substrate then feeds
three downstream consumers: agent evaluation, training-data generation, and
regulation.

What we do **not** adopt: AL's trainer (verl, vLLM, PPO policy updates). Kask
already has a training pipeline (`hkask-mcp-training`) and already has
live-system learners (`StrategyEvaluator`, `StagnationDetector`). AL's trainer
is offline and batch; it maps onto the *existing* cloud training path, fed by
harness trajectories instead of static datasets — not onto the live regulation
tick.

## Why the local swarm is the right primary client

The local swarm already has the two things AL requires of an agent:

- **A real harness.** `AgentExecutor::run`
  (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:67`) — multi-round
  inference, tool loop, `mcp_tools` allowlist enforcement at dispatch
  (`agent_executor.rs:155-217`).
- **A judge.** `TaskSuccessVerdict` + `run_evaluator` with deterministic
  provenance (`kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs` — the
  executor stamps verdicts after running declared evaluators against the
  delegation response).

And kask already has the training half — dataset ingestion
(`hkask-mcp-training/src/dataset.rs` normalizes to `ChatConversation` /
`PreferenceExample`), LoRA job submission to cloud hosts
(`providers/runpod.rs`, `providers/nebius.rs`), adapter metrics, and an
evaluator (`tools/evaluate.rs`). What is missing is the piece AL exists to
provide: **exercising an agent in its harness and capturing what happened.**
The training pipeline today consumes static datasets; it never sees the agents'
real tool-loop behavior.

AL's rollout ≈ local swarm delegation. AL's `model_request` capture ≈ a hook in
`AgentExecutor`'s inference calls. AL's reward event ≈ `TaskSuccessVerdict`.
AL's trainer ≈ the existing `hkask-mcp-training` pipeline, fed by
verdict-labeled trajectories.

This yields a genuinely closed loop that the regulation-engine framing could
not: run agents → judge rollouts → train adapters → deploy adapters back into
agent cards → run again. That is AL's actual loop, transplanted into kask's
existing organs.

## The three consumers (in priority order)

1. **Agent evaluation (primary).** "Is this agent card + model + prompt
   actually good?" Run N rollouts against a task set, get pass rates per card.
   Nobody can answer this today. `training_evaluate`
   (`hkask-mcp-training/src/tools/evaluate.rs:15`) evaluates *models* on
   datasets; nothing evaluates *agents* in their harness.
2. **Training data.** Verdict-labeled trajectories are SFT/DPO material. A
   passed and a failed rollout on the same task is a preference pair; the
   harness is a preference-pair generator grounded in real harness behavior —
   AL's core thesis. Feeds `dataset.rs`'s existing canonical formats.
3. **Regulation (downstream).** Everything from the original event-substrate
   analysis still holds — `verify_impact` starves on 2 of 31 metrics
   (`kask/crates/hkask-regulation/src/cybernetics_loop.rs:1302-1315`), and
   impact/feedback data lives in scattered structures. The harness becomes the
   richest producer of regulation events; regulation becomes a query client.

## Service map

| AL module | Rust core service | Seam | Notes |
|---|---|---|---|
| `Rollout` lifecycle (`schemas.py`) | Rollout identity on local swarm delegations | **D3** | State machine with enforced transitions, attempts, idempotent creation — folds onto `LocalDelegateResult` / swarm delegation paths |
| `schemas.py` + `store.py` | `hkask-event-store` (new crate) | additive workspace member | Append-only log on SQLite via existing `hkask-storage::DatabaseDriver` |
| `proxy.py` capture | Capture hook in `AgentExecutor`'s inference calls (and, for governed paths, the inference IPC boundary) | **D3** / **D8** | Every inference call → `model_request` event. Zero changes to agent cards — the executor is the sense organ |
| `hooks.py` + `RewardData` | Promote `TaskSuccessVerdict` / `run_evaluator` to a declared per-card evaluator contract | **D3** (swarm MCP server) | Verdicts append as events with `source` provenance |
| Rollout runner (AL's controller, minus K8s) | **Rollout harness** — run a card against a task set N times, aggregate pass rates + variance | **D3** | New tool surface (e.g. `swarm_eval_agent_local`); reuses `AgentExecutor` + `run_evaluator` |
| `trainer.py` | Existing `hkask-mcp-training` pipeline + a trajectory→dataset bridge | existing (D9-adjacent) | The only place batch logic is legitimate; offline, operator- or curator-triggered |

## The one hard modeling decision: what is a rollout?

A rollout is **the unit that has a lifecycle and a judge**. Proposal:

- **Rollout = a local swarm delegation** (a single `delegate()` call against a
  task). It has terminal states and can carry a verdict. A curator/user turn is
  a rollout too, for the regulation consumer.
- **Events within a rollout:** inference calls (`model_request`), tool
  invocations, regulatory actions, and verdicts.
- Inference calls and tool invocations are NOT rollouts — they are events
  inside one. Modeling them as rollouts would explode cardinality and break
  the compaction story.

Getting this wrong poisons everything downstream. Confirm before any code.

## Event model (AL's discipline, applied)

Two well-known event kinds; everything else is opaque pass-through:

1. `model_request` — captured automatically inside `AgentExecutor`'s inference
   loop. Payload: model, request shape (not full body — see retention),
   response status, latency_ms, usage, finish_reason, retry_count.
2. `verdict` — reported by an evaluator (deterministic per-card evaluator,
   operator feedback, or the metacognition loop's impact judgment). Payload:
   value (scalar or pass/fail), source (`deterministic_evaluator` |
   `operator` | `regulation_impact`), reason.

Opaque pass-through for everything else (skill spans, tool stats) — the store
does not parse what it doesn't need to. Position in the log is identity; no
separate event ID (AL's `schemas.py` pattern).

Store interface target (~4 functions — the deletion test applied up front):
`append`, `query`, `compact`, `cursor`. If the interface grows past this, the
design is wrong.

## The loop this enables

```
harness runs card × task × N
  → events (model_request, tool invocations)
  → verdicts (deterministic evaluators)
  → pass-rate report (eval consumer)
  → preference pairs / SFT examples (training consumer)
  → regulation signals (regulation consumer)
  → trained adapters redeployed into cards
  → harness runs again
```

For regulation specifically: verdicts of regulatory actions are themselves
events. `verify_impact` stops being a special-case struct walker
(`cybernetics_loop.rs:1302-1315`) and becomes a query — "for rollout R, what
was the metric before action A and after it?"

## Risks and mitigations

1. **Scale / cold start.** AL's headline results assume 6K+ rollouts and GPU
   clusters. Local rollouts run one-at-a-time on the operator's machine.
   Fine for eval (N=20 gives useful pass rates); thin for training (DPO wants
   hundreds of pairs). Data accumulates locally; training happens on RunPod.
   Expect a period where the eval harness is valuable and the training loop is
   starved. Do not gate the harness on training-data volume.
2. **Non-determinism.** Local inference is sampled; identical rollouts
   diverge. AL handles this with `num_repeat` groups. The harness must run
   repeat counts and report variance, or pass rates are noise. Evaluators stay
   deterministic (`TaskSuccessProvenance::Deterministic`); the *rollouts* are
   the sampled part.
3. **Hot-path writes.** `AgentExecutor`'s inference loop is hot. Capture must
   be fire-and-forget over a bounded channel; it must never block a generation
   call. Backpressure drops are counted and surfaced as a sensor signal, not
   swallowed.
4. **Volume / retention.** AL is batch and deletes completed rollouts before
   polling (`agl_rollout_manager.py`). A live IDE logs continuously.
   Mitigation from day one: terminal rollouts compact to summaries; hard cap
   on event count; drop-oldest with a *counted* drop (never silent — the
   broken-feedback-loop rule: absence must be distinguishable from zero).
5. **Card heterogeneity (falsifier).** If agent cards turn out to need fully
   bespoke eval tasks with no shared structure, the harness's leverage
   collapses toward per-agent bespoke testing — still useful, much less
   compounding. Early probe: write eval tasks for 3 existing local agents and
   measure how much task structure actually shares.
6. **Over-building.** AL's entire data plane is ~700 lines of Python. If the
   Rust port of store + lifecycle exceeds a few hundred lines, the design is
   wrong. Budget: `hkask-event-store` ≤ ~500 lines including tests.
7. **Dead-surface cleanup is part of this work, not separate.** The stale
   `enforce_and_stamp` comments, the always-`None` envelope path
   (`local_runtime.rs:438`, consumed at `local_tools.rs:347-352`), and the
   unreachable `Grounding*` alert arms (`cybernetics_loop.rs:1166-1184`) get
   removed in the same PR series — dead surface is lying documentation.

## Build order

1. **Rollout harness (thin slice).** Run one card against a small task set,
   N repeats, using `AgentExecutor` + `run_evaluator`; report pass rates +
   variance. In-memory only — no store yet. This validates the falsifier
   (risk 5) before any infrastructure is built.
2. **`hkask-event-store`.** Schema, append/query/compact/cursor, retention.
   Additive crate; the harness becomes its first writer.
3. **Capture hook in `AgentExecutor`** — `model_request` events,
   fire-and-forget, counted drops (D3). Inference IPC capture for governed
   paths follows (D8).
4. **Evaluator contract** — per-card declared evaluators; verdicts append as
   events (D3).
5. **Trajectory → dataset bridge** — verdict-labeled rollouts normalized into
   `dataset.rs`'s canonical formats, feeding the existing training pipeline.
6. **Regulation rewiring** — `verify_impact` and the metacognition drift
   detector read from the store; impact verdicts write back to it.

Each phase is independently shippable and independently deletable — the
strangler-fig discipline: the old paths keep working until the new substrate
provably replaces them. Phase 1 is deliberately store-less so the highest-risk
assumption (task-set reuse across cards) is tested with zero infrastructure.

## Divergence surface impact

- **D3** (hKask tools in-process): rollout identity on delegations; capture
  hook in `AgentExecutor`; evaluator contract and the harness tool surface in
  the swarm MCP server.
- **D8** (bridge + adapters): inference IPC capture for governed paths.
- New crate (`hkask-event-store`) is additive (workspace member); no upstream
  files touched.
- Per `.rules` hygiene: `.rules` updates (if any) are proposed in the PR
  description, not edited inline.
