---
name: swarm-steering
core: true
visibility: public
description: "Local-swarm steering skill for the Kask Curator or a human in the loop. Takes a swarm-intelligence plan, produces the delegation sequence + delegate_results collection shape + re-invoke instruction, closing the execute-and-feed-back loop."
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
- **Loop A closure — now structural (Gap 4 fix, 2026-08-18).** The
  `swarm-intelligence` manifest's default `steering_mode` is now `steering`,
  and a post-Act execute step (step 8) calls `swarm_execute_plan_local`
  deterministically and feeds `delegate_results` into the next LOOP
  iteration. This skill's directive is now the human-inspectable representation
  of what the manifest does structurally — the operator can still use it to
  understand or override the manifest's execution.

## Composed with

| Skill                | Role                                                     | When Invoked                                                                     |
| -------------------- | -------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `swarm-intelligence` | upstream planner — produces the plan this skill executes | the executor runs swarm-intelligence first, then this skill on its emitted_calls |

## Registry

Registry is authoritative — when this SKILL.md disagrees with registry
templates, the registry wins.

- Template manifest: `kask/registry/templates/swarm-steering/manifest.yaml`
- Templates: `kask/registry/templates/swarm-steering/swarm-steering-direct.j2` (KnowAct — produce the local-swarm steering directive: delegation sequence + delegate_results collection shape + re-invoke instruction)
- Process manifest: `kask/registry/manifests/swarm-steering.yaml` (1 step: DIRECT, single-pass — `max_iterations: 1`)
- rJoule cap: 3 per invocation
- Span namespace: `reg.skill.swarm-steering`
- Pairs with `swarm-intelligence` (the planner); this skill is the actuator's
  instructions (Cybernetic Swarm Plan `steering_mode`).

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `swarm-steering-direct.j2` | KnowAct | Take the swarm-intelligence plan (emitted_calls) + the swarm state + the credit budget, produce a structured steering directive: pre-flight checks (agents exist via swarm_list_local_agents; NO ledger-funding check — local delegation is never gated on funds), the ordered swarm_delegate_local execution sequence (agent_name, task, credits_authorized per delegate call), the delegate_results collection shape (LocalDelegateResult array), and the re-invoke instruction (re-invoke swarm-intelligence with delegate_results + steering_mode: steering). The Curator/human executes the directive. |

