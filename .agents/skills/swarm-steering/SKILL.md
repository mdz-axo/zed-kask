---
name: swarm-steering
visibility: public
description: "Focused local-swarm steering skill for the Kask Curator or a human in the loop. Codifies the execute-and-feed-back loop: take a swarm-intelligence plan (emitted_calls), produce the swarm_delegate_local execution sequence + the delegate_results collection shape (LocalDelegateResult array) + the re-invoke instruction. The Curator/human executes the directive and feeds delegate_results back to swarm-intelligence, closing the C5/C6 feedback loop. Anchored to PKO (procedure execution) and the Conant-Ashby Good Regulator (the actuator must model the swarm it steers). Pairs with swarm-intelligence (the planner). Emits reg.skill.swarm-steering.* spans. Any userpod may invoke this skill."
---

# Swarm Steering

Codify the local-swarm execute-and-feed-back loop for the Kask Curator (or a
human in the loop): take a swarm-intelligence plan, produce the exact
`swarm_delegate_local` execution sequence, the `delegate_results` collection
shape, and the re-invoke instruction — so the executor runs the plan and feeds
the real results back, activating C5 (fault attribution) and C6 (reconfigure)
in the next swarm-intelligence iteration.

## Substrate: local swarms (zed-kask v2 §15)

A local swarm runs on the zed-kask substrate: `hkask-inference` (Ollama/cloud),
`hkask-ledger` (operator-funded credits), `hkask-guard` (I/O scanning). The
Kask Curator (`Agent::Curator`, `CURATOR_AGENT_ID`) is the in-process agent
with governed tool access (the MCP servers via `McpRuntime`), sovereign
memory, and the regulation/metacognition loops. In steering mode it executes
the swarm-intelligence plan by calling `swarm_delegate_local`, collects the
`LocalDelegateResult` objects, and re-invokes swarm-intelligence with
`delegate_results` — closing the feedback loop without a new FlowDef execution
surface (the Curator's normal tool-call turn IS the execution).

## Ontological anchors

- **PKO** (Procedural Knowledge Ontology, Carriero et al. 2025): the
  swarm-intelligence plan is a Procedure (specification); this skill produces
  the StepExecution sequence (the `swarm_delegate_local` invocations) + the
  StepExecution result collection (`delegate_results`). PKO's
  specification/execution separation is the core anchor.
- **Conant-Ashby Good Regulator** (Conant & Ashby 1970): the steering skill is
  the actuator that closes the feedback loop the swarm-intelligence planner
  opens. The Good Regulator theorem: the steering directive must model the
  swarm it steers (the roster + the plan + the credit budget).

## When to Use

- Execute a swarm-intelligence plan (emitted_calls) on a local swarm and feed
  the real `delegate_results` back (the execute-and-feed-back loop).
- Close the C5/C6 feedback loop: without `delegate_results`, swarm-intelligence
  cannot attribute fault or reconfigure the blamed agent; this skill produces
  the directive that generates the telemetry.
- A human in the loop managing a local swarm wants the exact
  `swarm_delegate_local` calls to run + the collection/re-invoke shape, without
  the full swarm-intelligence composition PDCA.

Do NOT use for:
- Composing/steering the swarm itself (use `swarm-intelligence` — this skill
  consumes its plan; it does not compose).
- Cloud (ABW) swarms (Xaman Ek has steering built in — delegate via
  `swarm_xaman`; this skill is local-mode only).

## PDCA shape (emergent from the anchors)

```
Receive:  the swarm-intelligence plan (emitted_calls) + swarm state + credit budget
Direct:   the ordered swarm_delegate_local execution sequence (pre-flight + invocations)
Collect:  the delegate_results collection shape (LocalDelegateResult array)
Feedback: the re-invoke instruction (re-invoke swarm-intelligence with delegate_results + steering_mode: steering)
```

One-shot directive producer — the Curator/human executes the directive; this
skill does not execute delegations itself (no `action: execute` step). The
shape emerges from PKO's specification/execution separation + the Good
Regulator's "model the system you control."

## The delegate_results contract (C5/C6 activation)

`delegate_results` is an array of `swarm_delegate_local` results
(`LocalDelegateResult`-shaped): `agent_id`, `response`, `model`, `tokens_used`,
`cost`, `balance`, `latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`),
`executed_skills[]` (each `{skill, ok, error?}`). The executor collects one per
`execution_sequence` entry, in order, and feeds the array back as
`delegate_results` on the next swarm-intelligence invocation. ORIENT attributes
fault from `delegate_results[].tool_calls[].ok` / `executed_skills[].ok`;
`fault_count` accumulates (deterministic, in `swarm.converge_accumulate`);
C6 reconfigures the most-blamed agent.

## Known limitations (audit 2026-08-03)

The directive this skill produces closes the C5/C6 feedback loop, but the loop
it closes has two limits an operator should know (full analysis in the
[Swarm Cybernetics/Semantics Audit](../../docs/audits/swarm-cybernetics-semantics-audit.md)):

- **Binary fidelity.** The `delegate_results` this skill collects carry
  `tool_calls[].ok` / `executed_skills[].ok` — execution success, not task
  success. ORIENT's fault attribution (C5) cannot detect an agent that returns
  `ok: true` with the wrong output. For tasks with a deterministic oracle,
  pass `task_success` on the re-invoke so the planner's C0 axis covers it; for
  open tasks, the Go See loop (C2) is the only cover.
- **`latency_ms` is collected but not yet regulated.** Each `LocalDelegateResult`
  carries `latency_ms` (Cybernetic Swarm Plan C4), but no DECIDE move consumes it
  yet — a slow agent is sensed with no reconfigure response. The directive
  still collects it (forward-compatible) so a future planner that regulates
  latency gets the telemetry.

These do not block execution — the directive is sound; they bound what the
*next* swarm-intelligence iteration can infer from the results.

## Composed with

| Skill | Role | When Invoked |
|-------|------|-------------|
| `swarm-intelligence` | upstream planner — produces the plan this skill executes | the executor runs swarm-intelligence first, then this skill on its emitted_calls |

## Registry

Registry is authoritative — when this SKILL.md disagrees with registry
templates, the registry wins.

- Template manifest: `kask/registry/templates/swarm-steering/manifest.yaml`
- Templates: `kask/registry/templates/swarm-steering/swarm-steering-direct.j2`
- Process manifest: `kask/registry/manifests/swarm-steering.yaml` (1 step:
  DIRECT, single-pass)
- Span namespace: `reg.skill.swarm-steering`
- Pairs with `swarm-intelligence` (the planner); this skill is the actuator's
  instructions (Cybernetic Swarm Plan `steering_mode`).