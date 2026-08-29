---
title: "Swarm Systems — Explanation: Why the Loops Are Shaped This Way"
audience: [architects, developers]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Swarm"
mds_categories: [trust, curation]
---

# Swarm Systems — Explanation: Why the Loops Are Shaped This Way

The swarm system's shape is not arbitrary — it emerges from a cybernetic
constraint: a swarm is a feedback loop, so the skill that governs it is a
feedback loop, and the components that close it are named (not implicit).
This explanation covers the four-loop architecture, why each loop has its
specific weak property, and the structural invariants that keep the loops
honest. It presupposes the [tutorial](./tutorial.md) and the
[reference](./reference.md).

## The shape: a planner, an actuator, and two gates

The system separates **planning** (composing/steering the swarm) from
**execution** (running the delegations) by design. The `swarm-intelligence`
skill is the planner — it emits a plan and never executes. The
`swarm-steering` skill is the actuator — it takes the plan, produces the
exact `swarm_delegate_local` sequence, and the re-invoke instruction. Two
gates sit under both: the consent/ceiling gate (Loop C) and the second-order
monitor + Go See (Loop D).

This separation is the Conant-Ashby Good Regulator theorem made literal: the
actuator must model the swarm it steers (the roster + the plan + the credit
budget). The steering skill's directive carries exactly that model.

## The four loops

```mermaid
stateDiagram-v2
    [*] --> Sense
    Sense: SENSE — balance, history, run_status, task_board
    Sense --> Orient: curator reads sense inputs
    Orient: ORIENT — swarm-intelligence PDCA cascade
    Orient --> Decide: plan emitted
    Decide: DECIDE — swarm-steering picks delegate sequence
    Decide --> Act: delegate_local calls
    Act: ACT — LocalSwarmRuntime::delegate
    Act --> Consent: spend gate (abw) or ceiling (local)
    Consent: Loop C — consent + ceiling
    Consent --> Record: authorize → complete → debit
    Record --> Sense: balance / history / task board updated
    Sense --> [*]: target reached or budget exhausted
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-SWARM-030
verified_date: 2026-08-28
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:473-519; kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs:169-480; kask/mcp-servers/hkask-mcp-swarm/src/ledger_tools.rs:71-133; kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1954-2216; .agents/skills/swarm-intelligence/SKILL.md; .agents/skills/swarm-steering/SKILL.md
status: VERIFIED
-->

A loop is healthy when all five properties (polarity, delay, gain, closure,
fidelity) are healthy. The swarm's four loops each have one weak property:

- **Loop A (planner → actuator):** weak *fidelity* — the plan is a natural
  language directive, not a typed contract. A prompt-injected curator could
  emit a plan that doesn't match the operator's intent. Mitigation: the
  actuator's tool surface is the governed MCP server, and the Steer
  prompt's tool advertisement is verified against the server's generated
  `TOOL_NAMES` (`hkask_steer::ensure_steer`, called at
  `crates/swarm_panel/src/swarm_panel.rs:1305`; pinned by
  `steer_prompt_mentions_only_known_tools`, `swarm_panel.rs:4304+`) — the
  plan can only call tools that exist.
- **Loop B (actuator → delegation):** weak *delay* — the tool loop runs
  multiple inference rounds (`MAX_TOOL_ROUNDS = 4`,
  `agent_executor.rs:22`), so the actuator's view of "done" lags the actual
  completion. Mitigation: `swarm_delegate_and_wait`
  (`cloud_swarm_tools.rs:753`) polls for the run status.
- **Loop C (consent + ceiling):** weak *gain* — the per-dispatch ceiling
  bounds a single dispatch but not a cascade. A swarm of N agents each
  spending up to the ceiling can amplify cost N-fold. Mitigation: the
  session budget (`swarm_authorize_session`,
  `cloud_swarm_tools.rs:583`) bounds the total.
- **Loop D (monitor + Go See):** weak *closure* — the curator reads
  `swarm_balance_local` / `swarm_local_history` as the sense input, but the
  loop only closes if the curator actually calls them. Mitigation: the
  `swarm-intelligence` skill's SENSE step is a required cascade step, and
  the task board (`swarm_task_board`, `local_tools.rs:2176`) gives ORIENT a
  durable progress record instead of re-derived ephemeral state.

## Why local mode has no funding gate

The local ledger is **accounting, not authorization**
(`ledger_tools.rs:4-13`). Local agents run on the operator's own substrate
(their machine, their inference credentials), so there is nothing for the
server to withhold: refusing to run costs the operator the work while saving
them nothing. Funding gates belong on *cloud* delegation, where credits buy
someone else's compute (`local_runtime.rs:492-497`).

The per-dispatch ceiling IS retained: it bounds a single runaway dispatch (a
cost-amplification limit), which is a different concern from whether an
account is funded (`local_runtime.rs:505-507`). A negative balance is
therefore normal and meaningful — it is the operator's unreconciled local
spend, not a fault.

## Why `cost_uncapped` is carried alongside `cost`

`cost` stays capped at `credits_authorized` — that is the operator's declared
budget and what the ledger charges (`local_runtime.rs:544-545`). But the cap
makes the recorded figure under-state real spend whenever a delegation
overruns it, and the local ledger is purely a reconciliation surface, so a
silent understatement corrupts the only data that surface exists to provide.
`cost_uncapped` is carried alongside so the gap is visible, and a bounded
overrun is warned about rather than swallowed (`local_runtime.rs:525-535`,
warn at `:546-558`).

## Why the consent store is shared SQLite

```mermaid
sequenceDiagram
    participant Panel as Panel (governed server)
    participant Steer as Steer curator (per-project server)
    participant Store as mcp/swarm/consent.db
    Panel->>Store: open_sqlite(consent.db)
    Steer->>Store: open_sqlite(consent.db)
    Panel->>Store: mint(action=hire, target, credits)
    Store-->>Panel: consent_token
    Panel->>Panel: passes token to swarm_hire
    Note over Panel,Steer: token must be consumable cross-process
    Steer->>Store: consume(token, action, target, cost)
    Store->>Store: DELETE-affected-rows check (atomic single-use)
    Store-->>Steer: ceiling or error
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-SWARM-031
verified_date: 2026-08-28
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:181-199,368-394; kask/mcp-servers/hkask-mcp-swarm/src/consent.rs:46-54,462-470
status: VERIFIED
-->

The swarm server is launched by two independent paths: the governed
`McpRuntime` (app-global, used by the panel) and the per-project
`ContextServerStore` (used by the Steer curator). A consent token minted by
the panel's process must be consumable by the Steer curator's process. Both
processes open the same SQLite store at `mcp/swarm/consent.db`
(`hkask_mcp_swarm.rs:368-394`). Single-use is enforced atomically via the
DELETE-affected-rows check — two processes racing on the same token cannot
double-spend it (`consent.rs:462-470`).

On open failure, the server degrades to the session-local in-memory store
with a loud error — same-process consent still works; cross-process flows
(panel confirm → Steer spend) do not (`hkask_mcp_swarm.rs:384-393`). The
startup-failure-signal rule requires this so an operator reading logs can
distinguish "not configured" from "configured but broken."

## Why the executor does not debit the ledger

`AgentExecutor::run` returns a `RawDelegateResult` carrying the raw output
text, model, token usage, and tool/reasoning summaries — it does NOT debit
the ledger (`agent_executor.rs:9-12`). The caller
(`LocalSwarmRuntime::delegate` → `debit_and_build`) computes the cost and
debits (`local_runtime.rs:536-600`). This separation keeps the agent-run
policy (skill cascade, tool-loop orchestration) ledger-unaware, so the
executor can be unit-tested with stubbed ports and the runtime owns the
single spending seam (`local_runtime.rs:130-134`).

## Why a failed balance measurement is not 0

`swarm_balance_local` returns an error, not 0, when the ledger query fails
(`ledger_tools.rs:85-95`). The `.rules` trap is explicit: `unwrap_or(0)` on
regulation-loop sense inputs is a broken feedback loop — a DB outage returns
0, which the loop reads as "no deviation." `LocalDelegateResult::balance`
is `Option<i64>` and stays `None` on a failed measurement, serializing as
`null` (`local_runtime.rs:531-535`, `:561-583`). SENSE reads this as the
Onto4MAT `energy` property and DECIDE branches on it, so a fabricated value
would be read as a real measurement.

## Why port labels are type references, not free strings

A port label (`accepts`/`produces`) on an agent card looks like a free
string, but a label that resolves to nothing cannot form a composition seam
— it matches nothing at bind time. The typing layer prevents this by
construction: every label must resolve to a registered type at admission
(`validate_typing`, `local_registry.rs:46-63`, against
`PortRegistry::resolves`, `port_registry.rs:93-95`; built-in seed
`["text", "json", "task", "task_result"]` at `port_registry.rs:41`).

The runtime half of this story is deliberately minimal. The old
`classify_request` heuristic — trying to infer from free text whether a
request "is a task" — was **deleted**: widened, it swallowed structured
ports; narrowed, it missed real declarations; there was no correct setting
(`local_runtime.rs:708-720` documents the deletion). What remains is
`check_bind` (`local_runtime.rs:721-729`): `Some(true)` only for
`accepts: ["text"]` (universal accept), `None` for everything else.
Runtime bind matching against structured labels is the typing layer's
unfinished transition — the admission gate is the enforced surface, and
this doc says so rather than pretending the runtime checks more than it
does.

Output validation closes the loop at the other end: when a `produces` type
carries a schema (only `task_result` does today,
`port_registry.rs:53-63`), the agent's actual output is validated against
it after each delegation (`validate_output`, `port_registry.rs:132-170`;
invoked from `swarm_delegate_local` at `local_tools.rs:246`). The validator
supports exactly 7 JSON Schema keywords (`schema_validate.rs:10-18`) and an
unsupported keyword is **never a pass** — it surfaces as
`UnsupportedSchema` (`schema_validate.rs:84-93`, status enum at
`:222-229`) — because a validator that silently ignores what it cannot
interpret returns `valid` for a document it never checked.

## Why cloned cards import port labels via `port_types.json`

A third-party ABW agent card can declare `accepts`/`produces` labels that
are not in the built-in set. Rejecting them would make the catalogue
unusable; accepting them silently would paper over the admission gate. The
clone path instead persists the imported labels to a `port_types.json`
extension file in the agents dir (`PORT_TYPES_FILE`,
`local_registry.rs:195`) and merges them into the registry
(`promote_imported_port_types`, `local_registry.rs:294-332`) — so the gate
resolves them on this and every subsequent load, and locally-authored cards
still face the full built-in check.

## Why `swarm_clone_to_local` filters `allowed_tool_servers`

A third-party ABW agent card can declare `capabilities.mcp_tools` with any
qualified `server/tool` names. If the clone copied them verbatim, the cloned
local agent would extend the delegated tool surface beyond the operator's own
governed servers — a privilege escalation through the catalogue. The clone
filters the declared tools against `allowed_tool_servers` (sourced from
`HKASK_MCP_SERVER_IDS`, the parent's `BUILT_IN_MCP_SERVERS_IDS`) so only
tools on the operator's own governed servers survive the clone
(`config.rs:100-106`; `local_tools.rs:667`). The declared list is also the
runtime allowlist: the executor only declares listed tools to the model and
the qualified list travels with every dispatch so the zed-side IPC server
enforces it at the dispatch boundary (`agent_executor.rs:211-233`).

## Why cards carry their own evaluators

Without a declared oracle, every delegation is an open task — `task_success`
stays `null` and judging quality is the curator's manual job. The evaluator
contract (`capabilities.evaluators`, `local_registry.rs:158-166`) lets a card
declare deterministic checks (contains / not_contains / regex / exit_code /
file_exists, `run_evaluator`, `local_tools.rs:43-81`); `swarm_delegate_local`
runs them all and stamps the verdict with
`provenance: DeterministicEvaluator` (`local_tools.rs:214-240`). Two
discipline points in the implementation: the declared evaluators are
conjunctive (all must pass), and a broken evaluator spec propagates as an
error rather than stamping `pass: false` — the agent must not be blamed for
a broken oracle (`local_tools.rs:38-42`).

## Why the A2A transport is in-process

The A2A (Agent2Agent) integration uses the `a2a-lf` crate's data model types
(AgentCard, Task, Message, Part, Artifact) to wrap the existing
`LocalSwarmRuntime::delegate` in A2A-compliant types. No HTTP server is
required — the MCP tool dispatch path IS the A2A transport. Agents
communicate by calling `swarm_a2a_send` as an MCP tool, which internally
creates an A2A Message, delegates to the target agent, and returns an A2A
Task with the response as an Artifact (`a2a.rs:1-12`). An HTTP binding
(`a2a_http.rs`, opt-in via `HKASK_A2A_HTTP_ENABLE`) can be added for
cross-machine communication — the types are already wire-compatible
(`hkask_mcp_swarm.rs:330-366`).

## Source citations

| Concept                          | Location                                                                |
| -------------------------------- | ----------------------------------------------------------------------- |
| Planner/actuator separation      | `.agents/skills/swarm-intelligence/SKILL.md`; `.agents/skills/swarm-steering/SKILL.md` |
| `steer_system_prompt` (curator)  | `crates/swarm_panel/src/swarm_panel.rs:155`                             |
| Steer advertisement verification | `crates/swarm_panel/src/swarm_panel.rs:1303-1309`                       |
| Local ledger = accounting        | `kask/mcp-servers/hkask-mcp-swarm/src/ledger_tools.rs:4-13`              |
| No balance gate (local)          | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:492-507`         |
| `cost_uncapped` rationale        | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:525-558`         |
| Shared SQLite consent store      | `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:368-394`       |
| DELETE-affected-rows single-use  | `kask/mcp-servers/hkask-mcp-swarm/src/consent.rs:462-470`               |
| Executor does not debit          | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:9-12`           |
| `balance` is `Option<i64>`       | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:531-535,561-583` |
| `swarm_balance_local` error path | `kask/mcp-servers/hkask-mcp-swarm/src/ledger_tools.rs:85-95`            |
| Typing admission gate            | `kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs:46-63`          |
| `check_bind` (classification deleted) | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:708-729`     |
| `BUILTIN_PORT_TYPES` / `task_result_schema` | `kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs:41,53-63` |
| Unsupported-keyword-is-not-a-pass | `kask/mcp-servers/hkask-mcp-swarm/src/schema_validate.rs:84-93,222-229` |
| `port_types.json` extension      | `kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs:192-195,294-332` |
| `allowed_tool_servers` filter    | `kask/mcp-servers/hkask-mcp-swarm/src/config.rs:100-106`                |
| Runtime tool allowlist           | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:211-233`        |
| Evaluator contract               | `kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs:158-166`; `local_tools.rs:43-81,214-240` |
| Task board (Loop D closure)      | `kask/mcp-servers/hkask-mcp-swarm/src/task_board.rs:1-13`; `local_tools.rs:2176` |
| A2A in-process transport         | `kask/mcp-servers/hkask-mcp-swarm/src/a2a.rs:1-12`                      |
| A2A HTTP gateway (opt-in)        | `kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:330-366`       |
| `MAX_TOOL_ROUNDS`                | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:22`              |
| Spend gate two-phase shape       | `kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs:1-22`                |
| Session budget (Loop C gain)     | `kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:583`          |