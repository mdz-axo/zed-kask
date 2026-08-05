---
title: "Swarm Systems — Explanation: Why the Loops Are Shaped This Way"
audience: [architects, developers]
last_updated: 2026-08-04
version: "0.1.1"
status: "Active"
domain: "Swarm"
mds_categories: [trust, curation]
---

# Swarm Systems — Explanation: Why the Loops Are Shaped This Way

The swarm system's shape is not arbitrary — it emerges from a cybernetic
constraint: a swarm is a feedback loop, so the skill that governs it is a
feedback loop, and the components that close it are named (not implicit).
This explanation covers the four-loop architecture, why each loop has its
specific weak property, and the two structural gaps the audit found. It
presupposes the [tutorial](./tutorial.md) and the [reference](./reference.md).

## The shape: a planner, an actuator, and two gates

The system separates **planning** (composing/steering the swarm) from
**execution** (running the delegations) by design. The `swarm-intelligence`
skill is the planner — it emits a plan (`emitted_calls`) and never executes.
The `swarm-steering` skill is the actuator — it takes the plan, produces the
exact `swarm_delegate_local` sequence, and the re-invoke instruction. Two gates
sit under both: the consent/ceiling gate (Loop C) and the second-order monitor
+ Go See (Loop D).

This separation is the Conant-Ashby Good Regulator theorem made literal: the
actuator must model the swarm it steers (the roster + the plan + the credit
budget). The steering skill's directive carries exactly that model
(`swarm-steering SKILL.md:59`).

## The four loops and their weak properties

A loop is healthy when all five properties (polarity, delay, gain, closure,
fidelity) are healthy. The swarm's four loops each have one weak property; the
[audit](../../audits/swarm-cybernetics-semantics-audit.md) gives the per-property
evidence. The [feedback-loop map](../../diagrams/flowchart-swarm-feedback-loops.md)
annotates each loop with its health.

### Loop A (PDCA convergence) — weak: closure

The planner's inner loop is well-composed internally — deterministic guards
(FILTER enforces C3/C7), Cauchy convergence (`|d_i − d_{i−1}| < 0.03`), and an
algedonic override (402 escalates regardless of `d`). Its weakness is
**closure**: the default execution mode is **advisory**, where the operator
must feed `delegate_results` back. If they don't, the loop is open — it
produces plans but cannot observe their effect.

This is why the `swarm-steering` skill exists as a named artifact rather than
an implicit Curator behavior: it makes the closure structural. In steering
mode, the Curator runs the delegations and re-invokes the planner with the
results — closing the loop without a new FlowDef execution surface (the
Curator's normal tool-call turn IS the execution).

### Loop B (C5/C6 steering) — weak: fidelity

The actuator loop senses `delegate_results[].tool_calls[].ok` and
`executed_skills[].ok` — **binary execution success**. It can reconfigure a
crashed agent (C6 `swarm_reconfigure_local_agent`) but cannot detect an agent
that returns `ok: true` with the wrong output. That is exactly what C0
(`task_success`) covers — but only when the caller supplies a deterministic
oracle. For open tasks, Loop B can reconfigure forever on a healthy-but-wrong
agent.

This is the higher of the two structural gaps. The fix is to enrich
`LocalDelegateResult` with an optional deterministic `task_success` field and
feed it into ORIENT alongside `tool_calls[].ok`, raising fidelity from binary
to graded — without introducing an LLM judge (which would violate the
determinism constraint, Gap S3).

### Loop C (credit/consent algedonic) — weak: fidelity (local mode)

This is the strongest loop. The algedonic override is correctly wired: a 402
or un-acknowledged curator dispatch escalates regardless of `d`. This is the
`.rules` "unwrap_or(0)" trap enforced as a convergence invariant — a broken
algedonic channel is never read as "no deviation." The ceiling is a hard
server-side gate that refunds the consent grant on refusal.

Its weakness is local-mode fidelity: with no consent token, the balance check
is the gate, and a depleted balance is sensed **reactively**
(`PaymentRequired` at delegate time) rather than proactively at SENSE.
`swarm_balance_local` exists as a proactive read; the question is whether
SENSE's local branch calls it. If not, the loop sees the failure at ACT, not
at SENSE — a reactive algedonic signal is still a signal, but proactive sensing
would let the planner avoid proposing a delegation that will fail.

### Loop D (Go See) — weak: closure (by design)

The outer meta-loop is intentionally human. The second-order monitor (C1)
detects reasoning loops (same deficit + action repeating with no `d`
improvement) and sensor-truth divergence (`d` improving while `s` declines —
the swarm looks healthier but fails more tasks). When it recommends `go_see`,
the operator must descend. Its closure depends on a human — the weakest
closure, by design. It is the acknowledged gap-cover for open tasks where C0
is absent.

The remediation is to surface the `go_see` directive in the Steer UI (the
panel's `render_run_status_strip`, `swarm_panel.rs:2405`) as an actionable
prompt, not a log line — so the human closure is more likely to fire.

## Why the determinism constraint (Gap S3)

The skill repeatedly insists: "Do NOT use an LLM to score the output as
`task_success`; the judge must be deterministic." Why? Because `d`'s fourth
axis `(1−s)²` would be gamed by an LLM judge that returns `pass: true` to be
helpful — a healthy swarm that fails the task would converge. The convergence
criterion trusts `s`; if `s` is unreliable, the whole loop's polarity inverts
(false negative feedback). The enforcement point is the caller (the Curator /
operator), not a code gate — an advertised invariant with a convention-level
enforcement point (audit Gap S3). This is acceptable for a guardrail whose
only enforcer is the operator, but the user guide states it explicitly so a
lazy Curator does not silently corrupt `d`.

## Why the debit-before-scan invariant

`LocalSwarmRuntime` debits the ledger, then calls `AgentExecutor::scan_output`
on the raw text (`agent_executor.rs:11`). Why this order? So a guard-quarantined
result still costs credits — the compute was already spent. Reversing the
order (scan, then debit) would let a guard quarantine reclaim the cost, which
an attacker could exploit by injecting quarantine-triggering content to get
free compute. The invariant is load-bearing and documented in the executor's
module header.

## Why two launch paths (not a bug)

The swarm MCP server is launched by two independent systems (see the
[architecture diagram](../../diagrams/flowchart-swarm-architecture.md)):
`McpRuntime` (app-global, governed dispatch for the skill cascade) and
`ContextServerStore` (per-project, for the agent tool picker). Both launching
independent process instances is correct — removing either breaks its
consumers. This is the `.rules` "Kask MCP servers have two parallel launch
paths by design" trap; do not try to unify them.

## Why `kask.swarm.mode` selects the substrate, not the surface

All 50 tools are always registered; `kask.swarm.mode` selects which substrate
the tools route to. This is deliberate: a mode toggle must not hide tools from
the agent (the model discovering the loss only via "tool not found" mid-turn
is the `LazyToolRouter` trap, generalized). The substrate switch happens at
the tool's call site, not at registration. The tool-surface test
(`hkask_mcp_swarm.rs:3355`) pins the count at 50 so a future tool addition
cannot silently change the surface — it must update the test in the same
commit, which is the single source of truth for the count (audit Gap S1
found the SKILL.md drifted to "31" because the companion was not regenerated
after tools were added).

## The two structural fixes, ranked

1. **Loop B fidelity (High):** add an optional deterministic `task_success` to
   `LocalDelegateResult` and feed it into ORIENT. Closes the open-task gap;
   preserves the determinism constraint.
2. **C4 latency (High, structural):** `latency_ms` is measured but no DECIDE
   move consumes it. Feed it into DECIDE as a reconfigure signal (or into C1
   as a sensor-truth correlate). Closes the sense-without-act sub-loop; raises
   regulator variety to match the latency-spike disturbance class.

These are composition-quality fixes, not urgent (the algedonic and ceiling
gates keep the system safe in the meantime). They are the difference between
a swarm that converges on swarm-health and a swarm that converges on
task-success with bounded cost.

## See also

- [Swarm Cybernetics/Semantics Audit](../../audits/swarm-cybernetics-semantics-audit.md)
  — the full per-property evidence and the variety check.
- [Feedback Loops diagram](../../diagrams/flowchart-swarm-feedback-loops.md)
- [Tutorial](./tutorial.md) · [How-to](./how-to.md) · [Reference](./reference.md)