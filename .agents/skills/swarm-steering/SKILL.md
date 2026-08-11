---
name: swarm-steering
visibility: public
description: "Focused local-swarm steering skill for the Kask Curator or a human in the loop. Codifies the execute-and-feed-back loop: take a swarm-intelligence plan (emitted_calls), produce the swarm_execute_plan_local delegation sequence + the delegate_results collection shape (LocalDelegateResult array with task_success verdicts) + the re-invoke instruction. The Curator/human calls swarm_execute_plan_local and feeds delegate_results back to swarm-intelligence, closing the C5/C6 feedback loop. Anchored to PKO (procedure execution) and the Conant-Ashby Good Regulator (the actuator must model the swarm it steers). Pairs with swarm-intelligence (the planner). Emits reg.skill.swarm-steering.* spans. Any userpod may invoke this skill."
---

# Swarm Steering

Codify the local-swarm execute-and-feed-back loop for the Kask Curator (or a
human in the loop): take a swarm-intelligence plan, produce the
`swarm_execute_plan_local` delegation sequence (with optional deterministic
evaluators), the `delegate_results` collection shape, and the re-invoke
instruction — so the executor calls one tool that runs the plan, evaluates
results, and returns the collected array, activating C5 (fault attribution)
and C6 (reconfigure) in the next swarm-intelligence iteration.

## Substrate: local swarms (zed-kask v2 §15)

A local swarm runs on the zed-kask substrate: `hkask-inference` (Ollama/cloud),
`hkask-ledger` (operator-funded credits). The
Kask Curator (`Agent::Curator`, `CURATOR_AGENT_ID`) is the in-process agent
with governed tool access (the MCP servers via `McpRuntime`), sovereign
memory, and the regulation/metacognition loops. In steering mode it executes
the swarm-intelligence plan by calling `swarm_execute_plan_local`, which runs
each delegation, evaluates results (when evaluators are provided), and returns
the collected `LocalDelegateResult` array with `task_success` verdicts stamped.
The Curator re-invokes swarm-intelligence with `delegate_results` set to that
array — closing the feedback loop without a new FlowDef execution surface (the
Curator's normal tool-call turn IS the execution).

## Ontological anchors

- **PKO** (Procedural Knowledge Ontology, Carriero et al. 2025): the
  swarm-intelligence plan is a Procedure (specification); this skill produces
  the StepExecution sequence (the `swarm_execute_plan_local` delegation array)
  - the StepExecution result collection (`delegate_results`). PKO's
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
  `swarm_execute_plan_local` delegation array to run + the collection/re-invoke
  shape, without the full swarm-intelligence composition PDCA.

Do NOT use for:

- Composing/steering the swarm itself (use `swarm-intelligence` — this skill
  consumes its plan; it does not compose).
- Cloud (ABW) swarms (Xaman Ek has steering built in — delegate via
  `swarm_xaman`; this skill is local-mode only).

## PDCA shape (emergent from the anchors)

```
Receive:  the swarm-intelligence plan (emitted_calls) + swarm state + credit budget
Direct:   the swarm_execute_plan_local delegation array (pre-flight + delegations with optional evaluators)
Collect:  the delegate_results collection shape (LocalDelegateResult array with task_success verdicts)
Feedback: the re-invoke instruction (re-invoke swarm-intelligence with delegate_results + steering_mode: steering)
```

One-shot directive producer — the Curator/human executes the directive; this
skill does not execute delegations itself (no `action: execute` step). The
shape emerges from PKO's specification/execution separation + the Good
Regulator's "model the system you control."

## The delegate_results contract (C5/C6 activation)

`delegate_results` is an array of `swarm_execute_plan_local` results
(`LocalDelegateResult`-shaped): `agent_id`, `response`, `model`, `tokens_used`,
`cost`, `balance`, `latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`),
`executed_skills[]` (each `{skill, ok, error?}`), `task_success` (optional
deterministic verdict stamped by the tool when an evaluator was provided).
The `swarm_execute_plan_local` tool returns the array directly; the executor
feeds it back as `delegate_results` on the next swarm-intelligence invocation.
ORIENT attributes fault from `delegate_results[].task_success.pass` (highest
fidelity, when present) and `delegate_results[].tool_calls[].ok` /
`executed_skills[].ok`; `fault_count` accumulates (deterministic, in
`swarm.converge_accumulate`); C6 reconfigures the most-blamed agent.

## Known limitations (audit 2026-08-03)

The directive this skill produces closes the C5/C6 feedback loop. The loop's
fidelity was raised in the 2026-08-03 structural fixes (full analysis in the
[Swarm Cybernetics/Semantics Audit](../../../kask/docs/audits/swarm-cybernetics-semantics-audit.md)):

- **Graded fidelity (was binary).** The directive now instructs the executor to
  stamp a deterministic `task_success` per `LocalDelegateResult` (the
  `task_success: Option<TaskSuccessVerdict>` field). ORIENT's C5 reads it as the
  highest-fidelity fault signal, so an agent that returns `ok: true` with the
  wrong output is now attributable. `provenance: llm_judged` is downgraded
  (Gap S3). **Residual:** for open tasks with no oracle, leave `task_success =
null` — the Go See loop (C2) is the only cover; the cascade cannot detect a
  healthy-but-wrong agent without a deterministic evaluator.
- **Latency is now regulated.** `latency_ms` is still collected on every
  `LocalDelegateResult` (C4), and ORIENT now surfaces `latency_outliers` so
  DECIDE proposes `reconfigure_agent` for slow agents. The directive's
  collection shape is unchanged (it already collected `latency_ms`); the
  regulation is downstream in the planner.

These do not block execution — the directive is sound; the residual open-task
gap is the irreducible one the Go See loop covers.

## Composed with

| Skill                | Role                                                     | When Invoked                                                                     |
| -------------------- | -------------------------------------------------------- | -------------------------------------------------------------------------------- |
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
