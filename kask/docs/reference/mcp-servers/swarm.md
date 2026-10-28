---
title: "Swarm MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-22
version: "0.40.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, lifecycle, curation]
---

# Swarm MCP Server Reference

### Model policy

"Local swarm" names the execution substrate, not an LLM provider class. Agent
cards inherit the same host inference defaults as the curator; Settings → Kask
→ Models controls the platform default, including approved cloud models. No
smaller/local model is selected to save cost or avoid requesting permission.
Explicit per-agent overrides must be operator-selected; missing/unavailable
approved routing is an error, not permission to downgrade. The unused
`min_provider_class` metadata was removed from the Rust card type and seeds.

### Local evaluation boundary

Local evaluation supports **contains, not_contains, regex** only. Shell execution
and ambient file-existence checks have been removed, with no compatibility
fallback. These are deterministic response scores, not proof of independent
ground truth or useful improvement. Empty responses are scored; empty specs
and malformed regexes are errors. The rollout harness parses evaluators once
before inference and reuses them across repeats. Plans/suites validate supplied
specs before any delegation; direct delegation validates card evaluators first.

Per-task rates use every requested repeat, including completed incorrect
responses and delegation errors. `incorrect` is separate from `errors`.
`capture_status: "unavailable"` plus `capture_error` reports event-store open
failure; zero drop counters alone do not establish recorded evidence. Available
capture still requires checking dropped-event counters and durable records.
Pinned through the public tool by
`harness_counts_wrong_answers_and_reports_capture_unavailable` in
`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`.

Implementation: `ResponseEvaluator` and `swarm_eval_agent_local` in
`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs`.
Public contract tests `evaluator_admission_rejects_effectful_specs_before_agent_lookup`,
`evaluator_specs_are_validated_before_every_batch`, and
`response_evaluator_held_out_admission_matrix` live in
`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs`.
This follows parse-before-execute separation; Rust's bounded regex engine is
the existing implementation, not a shell-based oracle.[^response-regex]

[^response-regex]: Rust regex contributors. *regex crate: performance and untrusted input*. https://docs.rs/regex/latest/regex/#untrusted-input. Parsing is effect-free; semantic validity and held-out acceptance remain operator responsibilities.

**Crate:** `kask/mcp-servers/hkask-mcp-swarm`
**Tools:** 37 — all local. The live router is pinned by
`tool_surface_is_exactly_37_registered_tools`; `TOOL_NAMES` is generated from
router-annotated functions in `build.rs`.
The partition is 7 delegation/workflow + 3 swarm-scoped thread + 6 fleet/evaluation
+ 6 agent-registry + 8 swarm-lifecycle + 4 knowledge + 3 A2A tools.
**Substrate:** zed-kask's local substrate only — the former ABW cloud backend
(48 tools, consent/spend gates, Xaman Ek curator) was removed 2026-10-22.
**Auth:** none — local agents run on the operator's own substrate; there is no
budget and no consent token. The memory DB opens with the one shared
`HKASK_DB_PASSPHRASE` credential.

The swarm server exposes the local substrate for agent registry, team
composition, agent authoring, and dispatch:

- **Local** — routes to zed-kask's local substrate: `hkask-inference`
  (Ollama/cloud via the zed IPC bridge). No consent token and
  no budget: local agents run on the operator's own substrate (operator
  ruling 2026-09-04 — the budget concept is deprecated; timeouts are the
  enforcement/kill mechanism).

All dispatch runs through the kask MCP runtime (per-agent call metering,
`hkask.mcp.swarm` telemetry targets; tool reach itself is
bounded by the card's `mcp_tools` allowlist, not by the runtime). The server is the
substrate for the **Agent Swarm panel** (`crates/swarm_panel`), the
**`swarm-intelligence` skill** (including its local steering loop).

## The three surfaces

The server's tools map onto the three things an operator does with a swarm
substrate.[^reynolds-swarm-surfaces]

| Surface         | What                       | Tools                                                                                                                                                                                                                                      |
| --------------- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Authoring**   | Create new agents          | `swarm_create_local_agent`, `swarm_reconfigure_local_agent`, `swarm_generate_prompt_local`, `swarm_generate_ontology_local`, `swarm_ai_assist`                                                                                             |
| **Composition** | Group agents into teams    | `swarm_create_local_swarm`, `swarm_list_local_swarms`, `swarm_get_local_swarm`, `swarm_update_local_swarm`, `swarm_delete_local_swarm`, `swarm_clone_local_swarm`, `swarm_add_agent_local`, `swarm_remove_agent_local`, `swarm_fanout_local`, `swarm_pipeline_local` |
| **Operation**   | Browse, run, manage        | `swarm_list_local_agents`, `swarm_get_local_agent`, `swarm_remove_local`, `swarm_delegate_local`, `swarm_delegate_in_thread_local`, `swarm_a2a_send`, `swarm_a2a_card`, `swarm_a2a_broadcast`, `swarm_search_knowledge_local`, `swarm_recall_local` |
| **Inspection** | Schema checks, topology, response predicates | `swarm_workflow_check_local`, `swarm_run_workflow_local`, `swarm_observed_seams_local`, `swarm_fleet_digest_local`, `swarm_who_answers_local`, `swarm_evaluate_local`, `swarm_eval_suite_local`, `swarm_eval_agent_local`, `swarm_execute_plan_local`, `swarm_task_board`, `swarm_thread_local`, `swarm_list_local_threads` |

## Tool reference — 37 local tools

> The canonical list is the server's live router, pinned by its tool-surface
> tests (`TOOL_NAMES` is generated in `build.rs` from the `#[tool]`-annotated
> functions).

| Partition | Canonical tools |
|---|---|
| Delegation and workflow | `swarm_delegate_local`, `swarm_fanout_local`, `swarm_pipeline_local`, `swarm_workflow_check_local`, `swarm_run_workflow_local`, `swarm_observed_seams_local`, `swarm_execute_plan_local` |
| Swarm-scoped threads | `swarm_delegate_in_thread_local`, `swarm_thread_local`, `swarm_list_local_threads` |
| Fleet and evaluation | `swarm_fleet_digest_local`, `swarm_who_answers_local`, `swarm_evaluate_local`, `swarm_task_board`, `swarm_eval_suite_local`, `swarm_eval_agent_local` |
| Agent registry | `swarm_list_local_agents`, `swarm_get_local_agent`, `swarm_remove_local`, `swarm_create_local_agent`, `swarm_reconfigure_local_agent`, `swarm_ai_assist` |
| Local swarms | `swarm_create_local_swarm`, `swarm_list_local_swarms`, `swarm_get_local_swarm`, `swarm_update_local_swarm`, `swarm_delete_local_swarm`, `swarm_clone_local_swarm`, `swarm_add_agent_local`, `swarm_remove_agent_local` |
| Knowledge | `swarm_search_knowledge_local`, `swarm_recall_local`, `swarm_generate_prompt_local`, `swarm_generate_ontology_local` |
| A2A | `swarm_a2a_send`, `swarm_a2a_card`, `swarm_a2a_broadcast` |

All tools route to zed-kask's local substrate (`hkask-inference`).

### Delegation (the local execution path)

| Tool                   | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `swarm_delegate_local` | Run a local agent against a task. The execution path is the bounded tool loop: a tool call outside the card's declared `mcp_tools` allowlist is refused in `agent_executor`; allowed calls dispatch through `McpRuntime`, which meters them against the agent's per-tick call ceiling and emits the span. Returns a `LocalDelegateResult` (see shape below). No budget or consent token — local agents run on the operator's own substrate. |
| `swarm_fanout_local`   | Parallel multi-agent fan-out: dispatch N agents in one call and aggregate. Capped at `MAX_FANOUT` (10) — the substrate-level primitive the `swarm-intelligence` CHECK step reads.                                                                                                                                                                                                           |
| `swarm_pipeline_local` | Sequential local pipeline: run N agents in order with `{prev_output}` substitution (each step's task may reference the previous step's response). Capped at 10 steps.                                                                                                                                                                                                                                                                 |
| `swarm_a2a_send`       | Send an A2A (Agent2Agent) protocol message to a local agent: wraps in A2A types (Message/Task/Artifact) and dispatches in-process. No HTTP — MCP tool dispatch is the transport. Agents declare this tool in `mcp_tools` to communicate with each other.                                                                                                                                                                              |
| `swarm_a2a_card`       | Get the A2A Agent Card for a local agent (or all local agents when `agent_name` is omitted): capabilities, skills, supported interface. A2A-compliant discovery.                                                                                                                                                                                                                                                                      |
| `swarm_a2a_broadcast`  | Broadcast an A2A protocol message to all members of a local swarm; every member receives the message, sharing one context id across the dispatches.                                                                                                                                                                                                                                                               |

### Local swarms (team registry)

Local swarms are named groupings of local agent ids with a mission and a
durable ordered thread. No cost, no consent token.

| Tool                       | Purpose                                                                                                                                                          |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `swarm_create_local_swarm` | Create a local swarm (name, mission, optional seed `agents`).                                                                                                    |
| `swarm_list_local_swarms`  | List all local swarms (id, name, mission, members, created_at).                                                                                                   |
| `swarm_get_local_swarm`    | Get one local swarm by `swarm_id`, including its member roster. Not-found if absent.                                                                             |
| `swarm_update_local_swarm` | Update a local swarm's name and mission in place (`swarm_id` preserved).                                                                                          |
| `swarm_delete_local_swarm` | Permanently delete a local swarm by `swarm_id`; the roster is dropped, member agents are NOT deleted. Its recorded thread stays readable.                         |
| `swarm_clone_local_swarm`  | Clone a local swarm by `swarm_id`: creates a new swarm with a fresh id, copying the mission and roster.                                                           |
| `swarm_add_agent_local`    | Add a local agent to a swarm's roster by `swarm_id` + `agent_name`. Idempotent; the agent need not exist yet (roster is ids).                                     |
| `swarm_remove_agent_local` | Remove a local agent from a swarm's roster by `swarm_id` + `agent_name`. Idempotent; does NOT delete the agent.                                                   |

### Local agent registry

Local agent cards live at the configured `HKASK_LOCAL_AGENTS_DIR`
(default `mcp/swarm/agents/curated/` under the global data directory). The
registry is read by `swarm_list_local_agents` and
`swarm_delegate_local`.

| Tool                            | Purpose                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `swarm_list_local_agents`       | List local agent cards from the registry.                                                                                                                                                                                                                                                                                                    |
| `swarm_get_local_agent`         | Get a single local agent's full card.                                                                                                                                                                                                                                                                                                        |
| `swarm_create_local_agent`      | Write a new local agent card to the registry (`mcp/swarm/agents/curated/<id>/agent_card.json`) and reload the registry.                                                                                                                                                                                                                         |
| `swarm_reconfigure_local_agent` | Update an existing local agent's `system_prompt` (and optional model/tools/skills) in place (the C6 reconfigure step in the cybernetic swarm plan); preserves identity and provenance fields, then reloads the registry.                                                                                                                       |
| `swarm_remove_local`            | Delete a local agent card (path-safety checked; a synced-card remnant is not possible — there is no cloud counterpart).                                                                                                                                                                                                                     |
| `swarm_generate_prompt_local`   | Generate a system prompt for a local agent from a description. Uses the local `InferencePort`; optionally seeded with the agent's consolidated memory.                                                                                                                                                                                      |
| `swarm_generate_ontology_local` | Generate a seed ontology (Mermaid ER) for a knowledge domain. Uses the local `InferencePort`; optionally seeded with an agent's semantic-memory graph.                                                                                                                                                                                        |
| `swarm_search_knowledge_local`  | Search a local agent's prefix-scoped semantic memory; returns entity-attribute-value triples. Degrades to an empty result with a `memory_unconfigured` note when the store cannot be opened.                                                                                                                                                 |
| `swarm_recall_local`            | Recall prior response passages from shared local swarm memory by semantic similarity (KNN over prior turn task embeddings; per-agent scope optional).                                                                                                                                                                                         |
| `swarm_ai_assist`               | AI assist for the swarm panel authoring forms: suggests completions for partial inputs or validates well-formedness. `validate` runs the deterministic local contract checks first (`contract.rs`; the LLM cannot flip `valid`), then an inline inference prompt adds advisory notes; `suggest` uses the inline prompt only (`local_tools.rs`, `swarm_ai_assist`). The Jinja2 guidance `swarm-intelligence/swarm-compose-guide.j2` is rendered by the `swarm-intelligence` skill's authoring aid, not by this tool. |

### Swarm-intelligence support

| Tool                       | Purpose                                                                                                                                                                                                              |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `swarm_evaluate_local`     | Run a response predicate and return its match result under the historical `TaskSuccessVerdict` type, not task-truth certification. The evaluator name + spec are caller-supplied; the verdict feeds the swarm-intelligence ORIENT step. |
| `swarm_execute_plan_local` | Execute a swarm-intelligence plan: run each delegation via the local runtime, evaluate each result with a deterministic check (when an evaluator is provided), and return the collected `LocalDelegateResult` array with response-predicate results under `task_success`; without an evaluator, the board stays Pending. Capped at 10 delegations. |
| `swarm_eval_agent_local`   | Rollout harness: run one local agent against a task set N times each, evaluate each rollout with a deterministic evaluator, and report per-task pass rates with standard error plus totals (tokens, overall pass rate). Each rollout is recorded as `model_request` + `verdict` events in the event store (`mcp/swarm/events.db`, operator-configurable via `HKASK_SWARM_EVENTS_PATH`); a store failure is counted in `events_dropped`, never swallowed. Tasks capped at 10, repeats at 10, total rollouts at 50. |
| `swarm_task_board`         | Query a local swarm's task board for persistent task progress (status, attempt count, fail count, last result).                                                                                                                                                                                                 |

### `LocalDelegateResult` shape

Every `swarm_delegate_local` and `swarm_fanout_local` entry returns this shape.
It is the contract the `swarm-intelligence` ORIENT/CHECK steps and its steering loop consume. Optional fields are omitted when absent; empty traces remain arrays.
The server does not assign factual-reliability or completeness grades.
`task_success` is the historical wire key for a response-predicate result, not
certification of task accomplishment. When no evaluator runs, a recorded attempt remains
`pending`. Execution statistics report `completed_executions`, errors, tokens,
and latency only; they do not rank competence. Plan/fanout `succeeded` counts
returned responses, not accomplished tasks. Suite cases without predicates
remain `PENDING` (nullable per-case `passed` and per-delegation `pass`).

```json
{
  "agent_id": "string",
  "response": "string",
  "model": "string (e.g. ollama/qwen3:32b)",
  "tokens_used": 1234,
  "latency_ms": 4200,
  "tool_calls": [{ "tool": "string", "ok": true, "error": null }],
  "advertised_tools": ["server/tool"],
  "mcp_tools_dropped": [{ "tool": "string", "reason": "string" }],
  "task_success": { "pass": true, "score": null, "detail": "response_predicate=...", "provenance": "deterministic" },
  "bind_matched": true,
  "rollout_id": "string — event-store capture id (absent when capture is unwired)",
  "reasoning_steps": [],
  "input_contract_check": { "status": "valid | invalid | unverified_*" },
  "output_contract_check": { "status": "valid | invalid | unverified_*" },
  "memory": { "attempted": 1, "stored": 1, "embedded": 1, "failed": 0 },
  "dispatch": { "route_reason": "direct", "question_source": "operator" }
}
```

- `latency_ms` is the C4 latency signal `T_q`.
- `tool_calls[].ok` records tool execution; it does not establish a response claim.
- `advertised_tools` disambiguates the empty `tool_calls` state (the
  2026-10-23 instrument-avoidance trap): `[]` + `[]` = the card advertised
  no tools — a capability gap, so check `mcp_tools_dropped`; non-empty +
  `[]` = tools were advertised and the model made no calls.
- `mcp_tools_dropped` surfaces the create/reconfigure-time drop report
  (unqualified names, `host/*` entries, ungoverned servers) with each
  entry's reason; absent when nothing was dropped. The
  `swarm_create_local_agent`/`swarm_reconfigure_local_agent` results carry
  the same report, and the card persists it for `swarm_get_local_agent`.
- `task_success` is absent without evaluation. Its `pass` value records only
  whether a supplied response predicate matched; its provenance describes
  how the check was produced, not whether the task was accomplished.
- `dispatch` is the dispatch-provenance stamp (fermi `stamp_invocation` port):
  `route_reason` names the dispatching surface (`direct`, `thread`, `fanout`,
  `pipeline`, `workflow`, `plan`, `eval`, `a2a`) and `question_source` where
  the task text came from. Server-authored on every production dispatch
  surface — callers cannot supply it. It carries no quality judgment; its
  consumer is blame attribution — routing-attributable failures
  (`bind_matched: false`, `input_contract_check: invalid`) fix the
  composition, agent-attributable ones reconfigure the agent, and
  `route_reason: "eval"` results feed stats, never blame.

## Error model

`LocalSwarmError` maps local failures per-variant to the right MCP wire-level
kind (`map_local_swarm_error`) instead of collapsing the surface to
`Internal`.[^owasp-swarm-errors]

| Variant         | Trigger                                                                 | Surface             |
| --------------- | ----------------------------------------------------------------------- | ------------------- |
| `Io`            | filesystem I/O failure (registry cards, swarm dirs)                     | `internal`         |
| `Database`      | storage/pool/schema failure (semantic memory, event store)               | `internal`         |
| `InvalidInput`  | caller input failed validation (empty name, bad slug, JSON parse)        | `invalid_argument` |
| `NotFound`      | a referenced agent/swarm was not found                                   | `not_found`        |
| `Sanitize`      | path-safety / `sanitize_agent_id` rejection (a resolved path escaped)    | `invalid_argument` |
| `Unavailable`   | a dependency is unavailable (inference port, bound socket)                | `unavailable`      |

## The algedonic channel

Local delegation has no balance: local agents run on the operator's own
substrate (operator ruling 2026-09-04 — no local budget). The algedonic
signal is the execution error surface — a failed balance of receipts is
surfaced as an error (`tracing::warn!` at the tool boundary), never collapsed
to a silent zero (the `.rules` `unwrap_or(0)` trap).[^beer-swarm-algedonic]

## The swarm-intelligence skill ecosystem

The swarm server is the substrate for the convergent skill that composes and
steers swarms. The skill lives in `.agents/skills/swarm-intelligence/`
(its steering loop absorbed the former `swarm-steering` skill, 2026-09-25); this
section documents how it consumes the server's tool surface.[^pso-swarm-ecosystem][^aco-swarm-ecosystem]

### Observed swarm-composition loop (`swarm-intelligence`)

The skill senses real local roster state, orients against a target,
proposes moves using PSO/ACO/Reynolds as *analogies*, and emits an action plan.
Advisory is the safe default; only explicit steering causes the Curator/human
to execute a local batch once and reconcile its returned ordered receipts.
CHECK rereads state and receipts;
`lisp_eval` computes receipt coverage, distance and target-plus-stability from
those observed inputs. A plan or a stable off-target distance is not convergence.

| Phase | Observed handoff |
|---|---|
| SENSE → ORIENT | Read real roster, task board and any prior execution receipts; missing sensors remain unknown. |
| DECIDE → preflight | Propose roster/dispatch moves, check recorded failed signatures and actual tool/membership contracts. |
| ACT | Emit intents. In explicit steering, execute once and retain the tool response; advisory stops at the plan. |
| CHECK | Re-read state; distinguish an error receipt from task success; surface execution and algedonic failures. |
| Gate → next cycle | Caller uses `lisp_eval` on observed axes for distance and target-plus-stability; carry actual history forward, or stop with the open gap. |

### Cybernetic Swarm Plan components (C0–C8)

There are no registered `swarm.*` filter, accumulator or second-order monitor
primitives. Failed-edit and influence evidence must come from actual prior
receipts/task-board entries; the caller uses `lisp_eval` only for the pinned
arithmetic and exact-signature checks. An unavailable history never becomes a
deterministic fault count.

| Component | Name | Where it lives |
|---|---|---|
| C0 | Task success | Only observed `deterministic_evaluator`/operator verdicts; absent oracle leaves the task unassessed. |
| C1–C2 | Second-order / Go See | Human/Curator review of recorded traces and explicit task failures; no registered automatic monitor. |
| C3 / C7 | Failed edits / influence | Caller compares recorded signatures and measured influence; missing history is unverified. |
| C4–C6 | Latency, fault, reconfiguration | Tool latency/receipts and task-board evidence feed ORIENT; a reconfiguration is a separately observed action. |
| C8 | Task-gated alignment | SENSE assesses task-relevant roster edges; its judgment is not a tool-computed fact. |

**Absent `delegate_results`, C5/C6 are inert** — the planning cascade emits
intents, not executed results. This is why the steering modes (below) matter:
advisory mode produces a plan with no results, so the first pass through the
cascade cannot attribute fault; the operator must feed `delegate_results`
back and re-invoke to close the loop.

### Steering modes (the execution boundary)

The `swarm-intelligence` skill's `steering_mode` setting controls who executes
the plan and feeds `delegate_results` back. This is the seam between planning
and execution.

| Mode                   | Who executes                       | Who feeds `delegate_results` back   | When to use                                |
| ---------------------- | ---------------------------------- | ----------------------------------- | ------------------------------------------ |
| **advisory** (default) | The operator (manually)            | The operator (manually)             | Human-in-the-loop; the plan IS the output. |
| **steering**           | The Kask Curator (or the human)   | The Curator / operator autonomously | Autonomous closed-loop composition.        |

In **advisory** mode, the cascade emits `emitted_calls` (a list of
`swarm_delegate_local` invocations) and stops. The operator — or
swarm-intelligence's steering loop — executes them and feeds the resulting
`LocalDelegateResult[]` back as `delegate_results` on the next invocation. In
**steering** mode, the Curator or the operator executes the plan in-process and
feeds the results back autonomously, closing C5/C6 within a single cascade run.

### The `delegate_results` contract

The `delegate_results` input to ORIENT/CHECK is an array of
`LocalDelegateResult`-shaped objects (the same shape `swarm_delegate_local`
returns). The contract:

```json
[
  {
    "agent_id": "string",
    "response": "string",
    "model": "string",
    "tokens_used": 1234,
    "latency_ms": 4200,
    "tool_calls": [{ "tool": "string", "ok": true, "error": null }],
    "task_success": { "pass": true, "detail": "contains: expected", "provenance": "deterministic" }
  }
]
```

- ORIENT reads tool-call outcomes and an evaluator-stamped `task_success`; failures increment `fault_count` for `agent_id` deterministically.
- C6 `reconfigure_agent` targets the agent with the highest `fault_count`.
- CHECK reads `latency_ms` for C4; local delegation has no cost or balance fields.
- If `delegate_results` is absent or empty, C5/C6 are inert (the cascade emits
  intents, not executed results).

### Steering a local swarm

swarm-intelligence's "Steering a local swarm" loop (formerly the separate
`swarm-steering` skill) is the execute-and-feed-back step for local swarms:
from the plan (`emitted_calls`) it renders the `steering_directive`,
makes one `swarm_execute_plan_local` call, checks the ordered receipt with
`lisp_eval`, and feeds only the returned `results` back as `delegate_results`,
closing C5/C6. It sequences and collects; it never re-plans.

## Single launch path (2026-08-29 migration)

The swarm server — like all kask MCP servers — has ONE launch path: the
governed `McpRuntime` (app-global). It serves every consumer: the skill
body-injection path, the kask panel, and the agent's tool surface (routed
via `agent::set_kask_tool_source`, a process-global `KaskToolSource` wired
in `main.rs`).

The prior dual-path design (per-project `ContextServerStore` instances via
`KaskMcpDescriptor` for the agent tool picker, plus `McpRuntime` instances
for the panel) was retired: it shipped keyless per-project duplicates
whose env came from raw settings entries, and crash-looping churn with no
stop condition. `KaskMcpDescriptor` and the per-project spawn path are
deleted; `sync_kask_mcp_servers` now only defensively unregisters stale
descriptors and removes raw `context_servers` entries for kask IDs (the
namespace guard). Env for every spawn is composed exclusively by
`build_mcp_server_env` (the `ServerEnv` invariant).

## Configuration

`KaskSwarmSettings` follows the `Default`-as-source-of-truth pattern (no serde
attributes, `From` reads from `Default`, `mcp_env` compares against `Default`).
The memory DB passphrase is a keychain credential
(`kask://credentials/hkask_db_passphrase`), injected by the governed
`build_mcp_server_env` credential path — it never appears in the config env
map.[^owasp-swarm-config]

| Setting                               | Env var                             | Default                          | Notes                                                         |
| ------------------------------------- | ----------------------------------- | -------------------------------- | ------------------------------------------------------------- |
| `kask.swarm.default_agent_model`      | `HKASK_SWARM_DEFAULT_AGENT_MODEL`   | empty                           | Default for new local agents and unpinned local agents at execution time. When empty, execution inherits the host session model; explicit card models win. |
| `kask.swarm.a2a_http_enabled`         | `HKASK_A2A_HTTP_ENABLE`             | `false`                          | Enable A2A HTTP gateway (loopback JSON-RPC; opens a port)     |
| —                                     | `HKASK_DB_PASSPHRASE`               | — (no fallback)                  | The ONE shared SQLCipher passphrase (credential; the swarm memory DB opens with it — an unresolvable passphrase degrades the store with a `memory_unconfigured` note, never a default-key DB) |
| `kask.swarm.embedding_dim`            | `HKASK_SWARM_EMBEDDING_DIM`         | `1024`                           | Embedding vector dimension for semantic-memory store — the width requested from MRL embedding models on every embed (`local_knowledge.rs`)          |
| —                                     | `HKASK_LOCAL_AGENTS_DIR`            | `mcp/swarm/agents/curated`       | Local agent cards directory (derived from global `data_dir`) |
| —                                     | `HKASK_LOCAL_SWARMS_DIR`            | `mcp/swarm/swarms`               | Local swarms directory (derived from global `data_dir`)      |
| —                                     | `HKASK_SWARM_MEMORY_DB`             | `mcp/swarm/memory.db`            | Local swarm semantic-memory DB path (derived from global `data_dir`) |
| —                                     | `HKASK_SWARM_EVENTS_PATH`           | `mcp/swarm/events.db`            | Event-store path (operator env var override)                 |
| —                                     | `HKASK_SWARM_ROLLOUT_RETENTION_DAYS`| —                                | Rollout-event retention sweep knob (operator env var override) |
| —                                     | `HKASK_MCP_SERVER_IDS`             | (governed server set)            | The card `mcp_tools` filter boundary: imported cards' declared tools are filtered to these servers |

Removed settings (2026-10-22, with the ABW cloud backend): `kask.swarm.mode`,
`kask.swarm.api_url`, `kask.swarm.max_credits_per_dispatch`,
`kask.swarm.curator_consent_default`. A settings.json that still carries them
parses (serde-unknown fields are ignored) and the keys are inert. The removed
keychain credential `kask://credentials/hkask_abw_api_key` is purged at
startup, logged loudly (DIVERGENCE.md D5).

Local model resolution is evaluated at delegation time: a non-empty
`LocalAgentCapabilities::model` wins, otherwise the operator's non-empty
`kask.swarm.default_agent_model` is passed through the inference bridge; if
both are empty, the bridge uses the host session model. Existing local cards
remain unpinned. A model-resolution or provider error is surfaced; the
executor does not silently switch to another model.

### Local-agent execution gaps (audited 2026-09-22)

- **Declared skills: wired (2026-09-24; `host/onto_anchor` added 2026-10-14).** A card with non-empty
  `LocalAgentCapabilities::skills` is offered the editor's own skill tools —
  `host/skill`, `host/render_template`, `host/lisp_eval`, `host/onto_anchor` — on the existing
  allowlisted IPC tool path (`kask_bridge/src/host_skill_tools.rs` wraps the
  governed `ToolPort`; each tool calls the same function as its in-editor
  tool, over the app-published `SkillIndex` catalog the Curator reads).
  The swarm server's parent grant always includes these four tools.
  `host/skill` is limited to the card's declared names; a card without skills
  is not offered them. Pinned by `declared_skill_loads_through_the_host_skill_tool`,
  `undeclared_skill_is_refused_before_dispatch`,
  `agent_without_skills_gets_no_host_tools` (swarm) and the
  `host_skill_tools` tests (kask_bridge).
- **Declared MCP tool schemas: wired.** The executor requests each declared
  `server/tool` definition from the host before inference and advertises its
  registered description and JSON Schema; invalid names, absent metadata and
  non-object schemas fail visibly instead of receiving a placeholder. The
  lookup is server-scoped — a same-named tool on another server can neither
  answer the request nor shadow the declared one.
  `InferenceMethod::ToolDefinition` checks the card allowlist
  and parent-held grant before returning metadata, while `ToolInvoke` keeps
  its independent dispatch gate. A required-argument definition is pinned by
  the bridge/client/executor tests. This is a schema-delivery guarantee, not
  a claim that every provider will call every MCP tool correctly.
- **Round exhaustion now fails visibly.** An agent that requests tools in all
  four rounds without a final answer receives an execution error instead of
  an empty successful response. The 4-round bound still applies to reasoning
  steps and MCP tool calls together.

## Security posture

The server's defense-in-depth coverage (per the security-audit skill's defense-layer catalog):[^owasp-swarm-security]

- **Input filtering** — empty-string validation on all name/ID paths,
  `sanitize_agent_id` + `url_encode_segment`-class path-safety on every
  card/swarm path (a resolved path that escapes its directory is rejected).
- **Capability gating** — local tool reach is bounded by each card's
  `mcp_tools` allowlist (itself filtered against the governed server set);
  undeclared tools are not reachable, even if the agent requests them.
- **Runtime monitoring** — `tracing::warn!` on stale/failing signals; errors
  surface per-variant, never collapse to a silent zero.
- **Credential scoping** — `credentials: &["HKASK_DB_PASSPHRASE"]` (never
  `None`); the server receives only the one shared SQLCipher passphrase, not
  other servers' secrets (`kask/crates/kask_bridge/src/mcp_servers.rs`).
- **No local spend gate** — local execution has no credits, balance, or consent
  token; timeout and bounded execution constrain work.

Out-of-scope layers: **Layer 5 (information flow control) is absent by decision**
platform-wide — the FIDES taint machinery was deleted 2026-08-12 because both
inputs to its `Source`→`Sink` block were constants, so it could not deny. A
replacement IFC gate must first give tools real labels and propagate taint on
context write; until then the layer stays absent rather than advertised. This is
a recorded architectural choice, in the same register as Layer 3 — not a deferral
this server can close. Layer 8 (deception
detection) remains deferred by design with documented re-entry conditions — see the
plan's §14.

## Cross-links

- [Swarm system reference](../../diataxis/swarm_system/reference.md) — local substrate semantics, tool tables, and the swarm-intelligence skill process and procedures
- [Swarm system explanation](../../diataxis/swarm_system/explanation.md) — components C0–C8, the cascade, steering modes
- [Swarm diagrams](../../diagrams/swarm.md) — server topology, the observed composition loop, and the advisory-vs-steering boundary (consolidated)
- [MCP Server Registry](README.md) — fleet-wide patterns and the 9-server catalog

## Footnotes

[^reynolds-swarm-surfaces]:
    Reynolds, C. W. (1987). Flocks, herds and schools: A distributed behavioral model. _ACM SIGGRAPH Computer Graphics_, 21(4), 25–34. https://doi.org/10.1145/37402.37406
    Cited for the three-surface model (authoring, composition, operation) that maps the local tools onto the operator's actions.

[^owasp-swarm-errors]:
    OWASP. (2025). _OWASP Top 10 for Large Language Model Applications_. OWASP Foundation. https://owasp.org/www-project-top-10-for-large-language-model-applications/
    Cited for the per-variant error-classification pattern the LocalSwarmError model implements.

[^beer-swarm-algedonic]:
    Beer, S. (1979). _The Heart of Enterprise_. John Wiley & Sons.
    Cited for the algedonic-signal concept — the error surface that closes the feedback loop.

[^pso-swarm-ecosystem]:
    Kennedy, J., & Eberhart, R. (1995). Particle Swarm Optimization. _Proceedings of IEEE International Conference on Neural Networks_, 1942–1948. https://doi.org/10.1109/ICNNN.1995.488968
    Cited for the PSO velocity-tuning metaphor the swarm-intelligence skill's DECIDE step uses.

[^aco-swarm-ecosystem]:
    Dorigo, M., & Stützle, T. (2004). _Ant Colony Optimization_. MIT Press. https://mitpress.mit.edu/9780262042192/
    Cited for the ACO pheromone-deposition metaphor the swarm-intelligence skill's DECIDE step uses.

[^owasp-swarm-config]:
    OWASP. (2023). _OWASP Secrets Management Cheat Sheet_. OWASP Foundation. https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
    Cited for the keychain-credential principle the DB passphrase configuration follows.

[^owasp-swarm-security]:
    OWASP. (2025). _OWASP Top 10 for Large Language Model Applications_. OWASP Foundation. https://owasp.org/www-project-top-10-for-large-language-model-applications/
    Cited for the defense-in-depth security model the security-audit skill assesses the server against.