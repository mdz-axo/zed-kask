---
title: "Swarm System — Pragmatic-Semantics + Pragmatic-Cybernetics Audit"
audience: [architects, developers, operators]
last_updated: 2026-08-04
version: "0.1.1"
status: "Active"
domain: "Swarm"
mds_categories: [trust, composition, lifecycle]
---

# Swarm System — Pragmatic-Semantics + Pragmatic-Cybernetics Audit

A combined gap analysis (pragmatic-semantics) and feedback-loop
composition/efficiency analysis (pragmatic-cybernetics) of the zed-kask swarm
system: the `hkask-mcp-swarm` MCP server, the `crates/swarm_panel` UI, and the
`swarm-intelligence` / `swarm-steering` skills. The audit reads the live code
and the skill manifests, classifies the load-bearing claims, and assesses the
four feedback loops that govern swarm behavior.

## Scope and source anchors

| Component | Primary source |
|----------|----------------|
| MCP server (50 tools) | `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:115` (`SwarmServer`), `:3003` (`tool_surface_is_exactly_50_registered_tools`) |
| Consent gate | `kask/mcp-servers/hkask-mcp-swarm/src/consent.rs:56` (`ConsentStore`), `:77` (`CONSENT_TTL_SECS`), `:150`/`:184`/`:227` (mint/consume/refund) |
| Spend gate | `kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs:83`/`:253`/`:334` (authorize_hire/delegate/curate) |
| Local runtime | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:39` (`LazyLocalSwarmRuntime`), `:73` (`LocalSwarmRuntime`); `agent_executor.rs:55` (`AgentExecutor`), `:33`/`:38` (round/skill caps) |
| A2A transport | `kask/mcp-servers/hkask-mcp-swarm/src/a2a.rs:24` (`to_a2a_card`) |
| Panel UI | `crates/swarm_panel/src/swarm_panel.rs:100` (`steer_system_prompt`), `:289` (`PanelMode`), `:1798`/`:1834` (`set_mode`/`set_swarm_mode`), `:230`/`:261` (`init` + deploy-and-focus fix), `:1870` (`ensure_steer_conversation`) |
| Tool invoker hook | `crates/swarm_panel/src/tool_invoker.rs:22` (`ToolInvoker`), `:33` (`set_tool_invoker`) |
| Planner skill | `.agents/skills/swarm-intelligence/SKILL.md` (PDCA `:62`–`:73`, convergence `:96`–`:108`, components `:117`–`:128`, steering modes `:138`–`:187`) |
| Actuator skill | `.agents/skills/swarm-steering/SKILL.md` (directive shape `:59`–`:64`, delegate_results contract `:71`–`:81`) |

## Part 1 — Pragmatic-semantics gap analysis

Each load-bearing claim is classified on the three axes (ontological IS/OUGHT,
epistemic declarative/probabilistic/subjunctive, ontology anchoring) with its
constraint force and provenance. Gaps are statements where the provenance chain
breaks or where an advertised invariant lacks an enforcement point.

### Gap S1 — Tool-count drift in the planner SKILL.md (stale specification)

- **Claim (SKILL.md:203):** "MCP tool surface (31 tools, both sets always
  available): 20 ABW + 11 local".
- **Implementation (`hkask_mcp_swarm.rs:3355`):** `tool_surface_is_exactly_50_registered_tools`
  pins **50 tools = 27 ABW + 23 local**.
- **Classification:** IS, declarative, ontology anchoring = domain_supplement
  (ABW substrate). Constraint force = Guideline. Provenance = Specification
  (SKILL.md) **conflicts** with Implementation (test).
- **Resolution (OT ranking):** same ontological mode (IS), same epistemic mode
  (declarative); tie broken on provenance authority — **Implementation > Specification**
  for a count claim (the test is the ground truth; the SKILL.md is a generated
  companion). Winner: **50 tools (27 ABW + 23 local)**.
- **Finding:** This is the `.rules` trap "Convention priors drawn from .rules must
  be verified against the codebase" applied to a SKILL.md. The drift is also
  reproduced in `kask/docs/diagrams/flowchart-swarm-architecture.md:3` (same
  "31 tools" wording). The new ABW tools absent from the SKILL enumeration are
  `swarm_get_agent`, `swarm_list_apps`, `swarm_ontology_templates`,
  `swarm_delegate_and_wait`, `swarm_create_app`, `swarm_fire`, `swarm_delete_agent`,
  `swarm_delete_swarm`, `swarm_search_knowledge`, `swarm_publish_checks`,
  `swarm_publish_agent`, `swarm_fork_agent`. The new local tools are
  `swarm_pipeline_local`, `swarm_a2a_send`, `swarm_a2a_card`, and the local-swarm
  lifecycle set: `swarm_create_local_swarm`, `swarm_list_local_swarms`,
  `swarm_get_local_swarm`, `swarm_delete_local_swarm`, `swarm_add_agent_local`,
  `swarm_remove_agent_local` (local mode now has explicit named swarms/rosters,
  not just an ephemeral session).
- **Remediation:** Regenerate the `swarm-intelligence` SKILL.md from the
  registry (registry is authoritative — `SKILL.md` is a companion), and correct
  the architecture diagram. The doc fix is applied in this audit; the SKILL.md
  regeneration belongs to a `skill-maintenance` run. **Note:** the surface
  subsequently grew 47 > 50 with the three local knowledge tools
  (`swarm_search_knowledge_local` / `swarm_generate_prompt_local` /
  `swarm_generate_ontology_local`) added 2026-08-03 — see
  [Local Knowledge Tools design](../plans/local-swarm-knowledge-tools.md). The
  drift mechanism (test is ground truth; SKILL.md is a generated companion)
  is the durable finding, independent of the current count.

### Gap S2 — Steering system prompt omits pipeline tool

- **Claim (`swarm_panel.rs:139`–`:156`):** the Steer prompt's local-tools list
  names 12 of 14 local tools, omitting `swarm_pipeline_local` and the A2A pair
  (`swarm_a2a_send` / `swarm_a2a_card`).
- **Classification:** IS, declarative, domain_supplement. Constraint force =
  Guideline. Provenance = Implementation (the prompt string is code).
- **Finding:** `swarm_pipeline_local` (sequential pipeline with `{{prev_output}}`
  substitution) is a composition primitive the curator could plausibly want in
  Steer mode. `swarm_local_history` (the local-ledger reconciliation surface) IS
  named in the prompt (`:155`), so reconciliation is covered. The A2A pair is a
  sub-protocol, defensibly omitted.
- **Severity:** Low. The curator can still discover the tool via the governed
  tool list; the prompt is a curated subset for context economy, not an
  allowlist. No behavior is blocked.
- **Remediation:** Optionally add `swarm_pipeline_local` to the Steer prompt's
  local-tools sentence. Not blocking.

### Gap S3 — `task_success` determinism is enforced by convention, not by code

- **Claim (SKILL.md:98`–`103`, `swarm_panel.rs:201`–`211`):** "Do NOT use an LLM
  to score the output as `task_success`; the judge must be deterministic."
- **Classification:** OUGHT, declarative, core ontology (cybernetic determinism).
  Constraint force = **Guardrail**. Provenance = Specification (skill + prompt).
- **Finding:** This is an advertised invariant. Per the `.rules` trap
  "Advertised invariants need enforcement points," the enforcement point is the
  caller (the Curator / operator), not a code gate. There is no validation that
  rejects an `task_success` value sourced from an LLM. A compromised or lazy
  Curator could pass an LLM-judged `task_success` and the cascade would accept
  it, corrupting the `d` metric's fourth axis.
- **Remediation:** Either (a) document explicitly that the invariant is
  "enforced by convention, not by code" in the SKILL.md (acceptable for a
  guardrail whose only enforcer is the operator), or (b) add a
  `task_success.provenance` field the cascade can tag and a warning when it is
  `llm_judged`. (a) is the lower-cost fix and matches the existing determinism
  discipline; (b) is stronger but adds surface area. Recommend (a) for now.

### Gap S4 — Consent TTL is enforced; do not conflate with DelegationToken expiry

- **Claim (`.rules` OCAP block):** "Token expiry is NOT enforced …
  `DelegationToken` carries no `expires_at` field."
- **Implementation (`consent.rs:77`, `:890` test):** `CONSENT_TTL_SECS` exists and
  `consent_store_sqlite_expired_grant_is_unspendable` asserts an expired
  `ConsentGrant` is rejected.
- **Classification:** IS, declarative, core (security). Constraint force =
  Prohibition (consent must not outlive its window). Provenance = Implementation.
- **Finding:** Not a gap — the ABW **consent** token (a spend authorization,
  scoped to action+target+credits, single-use, TTL-bounded) DOES enforce expiry.
  The `.rules` "no expiry" statement refers to the **OCAP `DelegationToken`**
  (capability, in-process, no signature). These are two different tokens. The
  user guide must not conflate them: the consent gate is a real-time blocking
  gate with TTL; the OCAP token is a capability handle with no expiry.
- **Status:** No drift. Documented here to prevent a future reader from
  over-generalizing the OCAP rule onto the consent token.

### Non-gaps confirmed

- **Consent gate is a real-time blocking gate** (`consent.rs:184` consume
  rejects scope/action/over-spend mismatches; `:700`/`:707`/`:718`/`:727`/`:736`
  tests pin each rejection). This is NOT post-hoc redaction — it blocks before
  the spend. Contrast with `GuardedStream` (post-hoc), which the `.rules` flags.
- **Algedonic override is enforced** (SKILL.md:105`–`108`): a 402 or
  un-acknowledged curator dispatch escalates regardless of `d`. This is the
  `.rules` "unwrap_or(0)" trap enforced as a convergence invariant — healthy.
- **`LocalSwarmRuntime` debit-before-scan invariant** (`agent_executor.rs:11`–`18`):
  the runtime debits, then scans output, so a guard-quarantined result still
  costs credits. Load-bearing and documented. Healthy.

## Part 2 — Pragmatic-cybernetics feedback-loop analysis

Four loops govern the swarm. Each is assessed on the five properties (polarity,
delay, gain, closure, fidelity) as healthy / degraded / broken.

### Loop A — PDCA convergence loop (the planner)

Sense → Orient → Decide → Filter → Act → Check → Converge → Loop.

| Property | Rating | Evidence |
|----------|--------|----------|
| Polarity | healthy | Negative loop — seeks `d → 0` (`SKILL.md:96`); `d` decreases as variety/diversity/closure improve. |
| Delay | degraded | One full PDCA iteration per `delegate_results` round-trip. In steering mode the delay is the execution time of all `swarm_delegate_local` calls; in advisory mode it is operator-driven and unbounded. |
| Gain | healthy | Bounded by the Cauchy threshold `|d_i − d_{i−1}| < 0.03` (`SKILL.md:104`) and the FILTER (C3/C7) guards. No unbounded correction. |
| Closure | degraded | `loop_closure = 1.0` is a target condition, but the **default mode is advisory**, where closure depends on the operator feeding `delegate_results` back. If the operator takes the plan and never returns results, the loop is open. The steering skill closes it; the planner alone does not. |
| Fidelity | degraded | `d` uses derived metrics (variety_coverage, diversity, loop_closure). The task-success axis (C0) is only present when the caller supplies a deterministic `task_success` — for open tasks the loop optimizes swarm health, not task success, and relies on the human Go See loop (Loop D). |

**Diagnosis:** Closure is the weak property. The loop is well-composed internally
(deterministic guards, Cauchy convergence, algedonic override) but its closure is
contingent on the execution mode. In advisory mode without the steering skill,
Loop A is an open loop — it produces plans but cannot observe their effect.

**Remediation:** Make the steering skill the default execution path for the
Curator (steering mode), so closure is structural rather than optional. The
SKILL.md already notes the Curator can steer with swarm-intelligence in steering
mode directly (`:170`–`178`); the focused `swarm-steering` skill should be the
canonical actuator so the closure is named, not implicit.

### Loop B — C5/C6 steering execution loop (the actuator)

Plan → `swarm_delegate_local` → `delegate_results` → ORIENT fault attribution →
C6 `swarm_reconfigure_local_agent`.

| Property | Rating | Evidence |
|----------|--------|----------|
| Polarity | healthy | Negative — reconfigure the most-blamed agent to reduce `fault_count`. |
| Delay | healthy (steering) / degraded (advisory) | Steering mode: the Curator re-invokes automatically after collecting results — short. Advisory: operator-driven — long. |
| Gain | healthy | One reconfigure per iteration (most-blamed agent only, `SKILL.md:124`–`126`). Bounded. |
| Closure | healthy (when steering skill used) | The `swarm-steering` skill produces the directive that generates `delegate_results` and the re-invoke instruction (`swarm-steering SKILL.md:59`–`64`), so closure is designed-in. |
| Fidelity | **degraded (the key finding)** | Fault attribution reads `delegate_results[].tool_calls[].ok` and `executed_skills[].ok` (`SKILL.md:182`–`187`) — **binary execution success**, not task success. A tool that returns `ok: true` with wrong output is not flagged. This is exactly the gap C0 covers, but only when an oracle exists. |

**Diagnosis:** Loop B senses "did it crash," not "did it solve the task." For
tasks with a deterministic oracle, C0 closes the gap; for open tasks, Loop B can
reconfigure forever on a healthy-but-wrong agent without detecting the problem.
The fidelity ceiling is the binary `ok` signal.

**Remediation:** Enrich `LocalDelegateResult` with an optional `task_success`
field populated by a deterministic evaluator the operator declares per task
(test pass/fail, schema validation, exit code). Feed it into ORIENT alongside
`tool_calls[].ok` so fault attribution distinguishes "executed but failed the
task" from "crashed." This raises Loop B's fidelity from binary to graded without
introducing an LLM judge (preserves the determinism constraint, Gap S3).

### Loop C — Credit/consent algedonic loop (the budget)

`swarm_hire_cost` within_budget → ceiling → consent mint → spend → wallet
reconciliation → 402 algedonic.

| Property | Rating | Evidence |
|----------|--------|----------|
| Polarity | healthy | Negative — spend reduces balance; blocks when insufficient. |
| Delay | healthy | Balance checked per-dispatch (short); wallet reconciliation per-iteration (longer). Local ledger balance is a synchronous read. |
| Gain | healthy | The ceiling `HKASK_ABW_MAX_CREDITS` (default 50, `swarm_panel.rs:191`) is the gain knob; the consent `credits_authorized` scopes a single spend. Bounded. |
| Closure | healthy | The 402 / un-acknowledged curator dispatch escalates regardless of `d` (the algedonic override). This is the well-composed part — a broken algedonic channel is never read as "no deviation." |
| Fidelity | degraded (local mode) | In local mode there is no consent token — the balance check is the gate (`swarm_panel.rs:149`). A depleted local balance is sensed **reactively** (the `swarm_delegate_local` call returns `PaymentRequired`) rather than proactively at SENSE. The loop sees the failure at ACT, not at SENSE. |

**Diagnosis:** Loop C is the strongest loop — the algedonic override is
correctly wired and the ceiling is a hard server-side gate. The local-mode
fidelity is reactive-only: the planner can propose a delegation that will fail
the funds check and only learn at execution time. `swarm_balance_local` exists
as a proactive read; the question is whether SENSE calls it in local mode.

**Remediation:** Confirm SENSE reads `swarm_balance_local` in local mode (the
steering prompt names it, but the SENSE template's local branch must call it).
If it does not, add it — the local balance is the algedonic signal and must be
sensed proactively, not only via `PaymentRequired` at ACT.

### Loop D — Go See loop (C2, the human meta-loop)

| Property | Rating | Evidence |
|----------|--------|----------|
| Polarity | healthy | Negative — descend with the section-5 checklist to correct drift. |
| Delay | degraded | Human-cadence (`cadence_every N convergences`, `SKILL.md:122`). Not continuous. |
| Gain | healthy | Bounded by the checklist; one descent per trigger. |
| Closure | **degraded (by design)** | Closure depends on a human descending. The second-order monitor (C1) recommends `go_see`; the operator must act. No automated closure. |
| Fidelity | healthy | C1 detects reasoning loops and sensor-truth divergence (`d` improving while `s` declines) — high-fidelity meta-signals. |

**Diagnosis:** Loop D is intentionally human. It is the acknowledged gap-cover
for open tasks (where C0 is absent). Its weakness is closure — a human who
ignores the `go_see` directive breaks the meta-loop. This is acceptable for a
meta-loop but means the system's outermost loop is the least closed.

**Remediation:** Surface the `go_see` directive in the Steer UI (not only in
the cascade output) so the operator sees an actionable prompt, not a log line.
The panel's `render_run_status_strip` (`swarm_panel.rs:2340`) is the natural
surface; currently it shows run messages, not Go See directives.

## Part 3 — VSM mapping

| Subsystem | Components | Status |
|----------|-----------|--------|
| **S1** (operations) | The agents themselves — ABW workspace agents (cloud) and `LocalAgentCard` agents (local) doing the work. | viable |
| **S2** (anti-oscillation) | Consent gate + ceiling (`spend_gate.rs`) prevent runaway spend oscillation; A2A (`a2a.rs`) coordinates between agents without a central scheduler. | viable |
| **S3** (monitoring + resource allocation) | `swarm-intelligence` ORIENT/CHECK + the credit budget (`swarm_fund_local`, ceiling, `swarm_hire_cost` within_budget). | viable |
| **S4** (spec-drift + algedonic sensing) | Second-order monitor C1 (`swarm.second_order_monitor`) + Go See C2 + algedonic 402 escalation. | viable |
| **S5** (policy) | The operator's task + target condition + `kask.swarm.mode` setting (the policy the swarm is steered toward). | viable |
| **Algedonic (S1→S5)** | 402 / un-acknowledged curator dispatch escalation bypasses `d` convergence. | **present, not blocked** — viable. |

**Overall viability:** Viable. The algedonic channel is present and not blocked
(the non-negotiable VSM criterion). The weakest subsystem is S4's reliance on a
human for Go See closure, which degrades but does not break viability.

## Part 4 — Variety check (Ashby)

| Disturbance class | Regulator response | Variety status |
|-------------------|-------------------|----------------|
| Agent crash (`tool_calls[].ok = false`) | `swarm_fire` / `swarm_remove_local` + `swarm_hire` / `swarm_create_local_agent` | covered |
| Agent wrong output (`ok = true`, task fails) | C6 `swarm_reconfigure_local_agent` — **but only detectable with a C0 oracle** | **deficit for open tasks** (Loop B fidelity) |
| Budget exhaustion | `swarm_fund_local` (local) / 402 algedonic (ABW) | covered |
| Consent denial | re-`swarm_request_consent` / abort | covered |
| Premature convergence (PSO collapse) | diversity metric `>= 0.25` sensed → DECIDE reshapes roster | covered |
| Spec drift | C1 second-order monitor → Go See C2 | covered (human-cadence) |
| Sensor-truth divergence (`d` up, `s` down) | C1 flags → Go See | covered (human-cadence) |
| **Delegation latency spike (C4 `latency_ms`)** | **measured but not regulated** | **DEFICIT** |
| A2A message failure | `swarm_a2a_send` returns error → surfaced in `delegate_results` | covered |
| Guard quarantine | debit-before-scan — quarantined result still costs credits; surfaced | covered |

**Key variety deficit — C4 latency is sensed but not regulated.**
`LocalDelegateResult.latency_ms` is measured (`SKILL.md:124`, C4) but no DECIDE
move type consumes it. The system can detect a slow agent but has no response
class for "reconfigure the slow agent" or "flag the slow agent for Go See." The
loop is open on the latency axis: sense without act.

**Remediation:** Feed `latency_ms` into DECIDE as a fault signal alongside
`tool_calls[].ok` (e.g., a latency threshold above which the agent is a
reconfigure candidate), or into C1's second-order monitor as a sensor-truth
correlate. This closes the C4 sub-loop and raises regulator variety to match
the disturbance class. Algedonic threshold per `.rules`: deficit count 1 (well
under the warning threshold of 50), but it is a structural closure gap, not a
magnitude gap — worth fixing for composition quality, not urgency.

## Summary of findings

| ID | Finding | Severity | Loop/Axis |
|----|---------|----------|-----------|
| S1 | Tool-count drift (31 vs 47) in SKILL.md + architecture diagram | Medium (doc drift, `.rules` trap) | docs |
| S2 | Steer prompt omits `swarm_pipeline_local` | Low | docs |
| S3 | `task_success` determinism enforced by convention, not code | Medium (advertised invariant, weak enforcement point) | C0 |
| S4 | Consent TTL is enforced — do not conflate with OCAP `DelegationToken` | None (clarification) | consent |
| A-closure | PDCA loop closure contingent on execution mode (advisory default) | Medium | Loop A closure |
| B-fidelity | C5/C6 fault attribution is binary (`ok`), not task success | **High** | Loop B fidelity |
| C-fidelity | Local-mode balance sensed reactively, not proactively at SENSE | Low–Medium | Loop C fidelity |
| D-closure | Go See loop closure depends on a human | Low (by design) | Loop D closure |
| C4 | Delegation latency sensed but not regulated | **High** (structural) | Variety / Loop B |

**Top two structural fixes (cybernetic):** (1) add an optional deterministic
`task_success` field to `LocalDelegateResult` and feed it into ORIENT (raises
Loop B fidelity, closes the open-task gap); (2) feed `latency_ms` into DECIDE
as a reconfigure signal (closes the C4 sub-loop, raises regulator variety).

**Top doc fix (semantics):** regenerate `swarm-intelligence` SKILL.md from the
registry so the 50-tool surface and the C4/task_success findings are reflected
(S1), and correct the architecture diagram (applied in this audit).

## Cross-links

- [Swarm MCP Server Architecture](../diagrams/flowchart-swarm-architecture.md)
- [Swarm Intelligence PDCA Cascade](../diagrams/flowchart-swarm-pdca-cascade.md)
- [Swarm Steering Loop](../diagrams/sequence-swarm-steering-loop.md)
- [Swarm Server Class Diagram](../diagrams/class-swarm-server.md)
- [Swarm Panel Modes (state)](../diagrams/state-swarm-panel-modes.md)
- [Swarm Feedback Loops (cybernetic map)](../diagrams/flowchart-swarm-feedback-loops.md)
- [Swarm Systems User Guide (Diataxis)](../diataxis/swarm_system/tutorial.md)