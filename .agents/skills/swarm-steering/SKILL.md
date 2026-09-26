---
name: swarm-steering
core: true
description: "Local-swarm steering skill for the Kask Curator or a human in the loop. Takes a swarm-intelligence plan, produces the delegation sequence + delegate_results collection shape + re-invoke instruction, closing the execute-and-feed-back loop."
---

# Swarm Steering

Codify the local-swarm execute-and-feed-back loop for the Kask Curator (or a
human in the loop): take a swarm-intelligence plan, produce the
`swarm_execute_plan_local` delegation sequence (with optional deterministic
evaluators), the `delegate_results` collection shape, and the re-invoke
instruction — so you call one tool that runs the plan, evaluate
results, and returns the collected array, activating C5 (fault attribution)
and C6 (reconfigure) in the next swarm-intelligence iteration.

## Substrate: local swarms (zed-kask v2 §15)

A local swarm runs on the zed-kask substrate: `hkask-inference` (Ollama/cloud).
Local delegation carries no credit cost or local ledger. A scoped plan runs
against the swarm's shared thread. The
Kask Curator (`Agent::Curator`, `CURATOR_AGENT_ID`) is the in-process agent
with governed tool access (the MCP servers via `McpRuntime`), sovereign
memory, and the regulation/metacognition loops. In steering mode it executes
the swarm-intelligence plan by calling `swarm_execute_plan_local`, which runs
each delegation, evaluates results (when evaluators are provided), and returns
the collected `LocalDelegateResult` array with `task_success` verdicts stamped.
The Curator re-invokes swarm-intelligence with `delegate_results` set to that
array — closing the feedback loop without a new skill execution surface (the
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
  swarm it steers (the roster + the plan).

## Initial and target condition

- **Initial condition:** the emitted plan, an existing local `swarm_id`, the current roster, its ordered delegation entries (up to the tool's 10-entry cap), and any caller-supplied deterministic evaluators. A missing result is not a failed result.
- **Target condition:** each admitted delegation has one actual tool result or explicit per-entry error in input order; only the returned `results` array becomes `delegate_results`. A tool-level error or missing receipt blocks feedback rather than manufacturing success. First-pass completeness closes locally without rerunning agents.

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

## When NOT to Use

- Composing or re-planning — `swarm-intelligence` owns DECIDE; steering sequences and collects, it never re-plans (its own constraint).
- Advisory-mode execution — in advisory mode the plan is the final output and the operator executes manually.
- Stamping LLM-judged task success — deterministic evaluators or null only; the Go See loop covers open tasks.

## Instructions

```
Receive:  the swarm-intelligence plan (emitted_calls) + local swarm_id
Direct:   the swarm_execute_plan_local delegation array (pre-flight + delegations with optional evaluators)
Collect:  the delegate_results collection shape (LocalDelegateResult array with task_success verdicts)
Feedback: the re-invoke instruction (re-invoke swarm-intelligence with delegate_results + steering_mode: steering)
```

The skill prepares the directive; the Curator/human executes it with the real tool. Rendering a directive is not executing it. The local loop checks the directive and its subsequent execution receipt; `swarm-intelligence` owns any re-planning or reconfiguration.

### Local PDCA — directive and receipt

1. **Plan:** identify the intended delegation subset of `emitted_calls` and render `swarm-steering/swarm-steering-direct` with `task`, `swarm_id`, and the unmodified plan. If there are no delegations, report `empty` and make no tool call. Otherwise check 1–10 entries, membership in the actual roster, and evaluator shape before labeling the directive `ready`. Composition changes are not delegation receipts.
2. **Do:** the Curator/human calls `swarm_execute_plan_local` once using that directive. Rendering it is not evidence of execution. If the tool returns an error rather than a complete `results` array, report the execution state unknown—some earlier calls may have run—and do not retry or fabricate receipts.
3. **Check:** for a successful tool response, derive `expected_names` from the submitted delegations and `observed_names` in order from each returned success `agent_id` or error entry `agent_name`. Run `lisp_eval` with `(begin (define same-names (lambda (a b) (if (= (length a) 0) (= (length b) 0) (if (= (length b) 0) nil (and (string= (car a) (car b)) (same-names (cdr a) (cdr b))))))) (and (> (length expected_names) 0) (<= (length expected_names) 10) (same-names expected_names observed_names)))`. Preserve per-entry `{agent_name, ok:false, error}` objects; do not map them to success. The tool's `succeeded` counts dispatches, not passed tasks; a deterministic `task_success.pass=false` is still a returned execution, and absent `task_success` is unscored.
4. **Act:** on a malformed *pre-execution* directive, correct its mapping once and repeat only the read-only preflight. After execution, a count/order mismatch or tool error blocks feedback and requires investigation; never automatically replay delegations. On a complete matched array, feed precisely `results` to `swarm-intelligence` as `delegate_results` and close the local loop. Its next iteration, not this actuator, decides any reconfiguration.

## The delegate_results contract (C5/C6 activation)

`delegate_results` is an array of `swarm_execute_plan_local` results
(`LocalDelegateResult`-shaped): `agent_id`, `response`, `model`, `tokens_used`,
`latency_ms`, `tool_calls[]` (each `{tool, ok, error?}`), `task_success` (optional
deterministic verdict stamped by the tool when an evaluator was provided).
`swarm_execute_plan_local` returns an object with `results`, `total_tokens`,
`failed`, `succeeded` and `task_board`. Only after the ordered receipt check,
feed its `results` array back as `delegate_results`. Dispatch errors have
`agent_name`, `ok: false`, `error` instead of success fields; a failed evaluator
is a successful dispatch with `task_success.pass: false`.
ORIENT attributes fault from `delegate_results[].task_success.pass` (highest
fidelity, when present) and `delegate_results[].tool_calls[].ok` /
`response` failure when observable; `fault_count` accumulates (deterministic, in
`swarm.converge_accumulate`); C6 reconfigures the most-blamed agent.

## Known limitations (audit 2026-08-03)

The directive this skill produces closes the C5/C6 feedback loop. The loop's
fidelity was raised in the 2026-08-03 structural fixes (full analysis in the
[Swarm Cybernetics/Semantics Audit](../../../kask/docs/audits/swarm-cybernetics-semantics-audit.md)):

- **Graded fidelity (was binary).** The directive now instructs you to
  stamp a deterministic `task_success` per `LocalDelegateResult` (the
  `task_success: Option<TaskSuccessVerdict>` field). ORIENT's C5 reads it as the
  highest-fidelity fault signal, so an agent that returns `ok: true` with the
  wrong output is now attributable. `provenance: llm_judged` is downgraded
  (Gap S3). **Residual:** for open tasks with no oracle, leave `task_success =
  null` — the Go See loop (C2) is the only cover; the process cannot detect a
  healthy-but-wrong agent without a deterministic evaluator.
- **Latency is now regulated.** `latency_ms` is still collected on every
  `LocalDelegateResult` (C4), and ORIENT now surfaces `latency_outliers` so
  DECIDE proposes `reconfigure_agent` for slow agents. The directive's
  collection shape is unchanged (it already collected `latency_ms`); the
  regulation is downstream in the planner.
- **Loop A closure depends on an actual receipt.** A rendered directive is not a structural tool call; the Curator/human executes `swarm_execute_plan_local`, checks its result, then re-invokes `swarm-intelligence`. Without that observed call and complete handoff, closure is unverified.

## Composed with

| Skill                | Role                                                     | When Invoked                                                                     |
| -------------------- | -------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `swarm-intelligence` | upstream planner — produces the plan this skill executes | run swarm-intelligence first, then this skill on its emitted_calls |

## Registry


This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

- Templates: `kask/registry/templates/swarm-steering/swarm-steering-direct.j2` (prompt step — produce the local-swarm steering directive: delegation sequence + delegate_results collection shape + re-invoke instruction)
- Span namespace: `reg.skill.swarm-steering`
- Pairs with `swarm-intelligence` (the planner); this skill is the actuator's
  instructions (Cybernetic Swarm Plan `steering_mode`).

## Registry Templates

| Template | Purpose |
|----------|---------|
| `swarm-steering-direct.j2` | Take the plan and local swarm id; check member agents; produce a scoped `swarm_execute_plan_local` call with optional deterministic evaluators, a `LocalDelegateResult` collection shape, and a feedback instruction. |

To render a template, call the `render_template` tool with the template ref (e.g., `swarm-steering/swarm-steering-direct`) and a context object with the required variables.

## Constraints

- The directive is derived from the swarm-intelligence plan — steering sequences and collects, it never re-plans.
- Stamp a deterministic `task_success` per `LocalDelegateResult` when an evaluator exists; leave it null for open tasks (the Go See loop covers them) — never LLM-judge.
- `latency_ms` is collected on every delegation result (C4) and flows into ORIENT's `latency_outliers`.
- In steering mode, only a real `swarm_execute_plan_local` receipt may close this skill's handoff. In advisory mode, leave the plan to the operator without claiming execution.
