---
title: "Swarm Systems — Reference: The 90-Tool Surface and Components"
audience: [developers, operators]
last_updated: 2026-09-28
version: "2.3.0"
status: "Active"
domain: "Swarm"
mds_categories: [domain, composition, trust]
---

# Swarm Systems — Reference: Tools and Components

`hkask-mcp-swarm` registers 91 tools: 48 cloud and 43 non-cloud (36 local,
4 knowledge, 3 A2A). The server pins the live count, name-set equality,
and cloud partition in its `tool_surface_tests`
(`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1016-1047`; the 48/36/4/3
partition is the per-file count of the same `pub(crate) async fn swarm_*`
signature pattern). The build script derives both lists from `swarm_*` function
signatures and router annotations (`kask/mcp-servers/hkask-mcp-swarm/build.rs:36-74`).

## Component map

```mermaid
classDiagram
    class SwarmServer {
        +client: SwarmClient
        +consent: ConsentStore
        +local_registry: LocalAgentRegistry
        +local_runtime: LazyLocalSwarmRuntime
        +local_swarms: LocalSwarmRegistry
        +local_memory: LazyLocalMemory
        +agent_stats: AgentStatsStore
        +event_store: LazyEventStore
        +thread_store: SwarmThreadStore
        +combined_router()
    }
    class CloudRouter {
        +48 tools
    }
    class LocalRouter {
        +35 tools
    }
    class KnowledgeRouter {
        +4 tools
    }
    class A2ARouter {
        +3 tools
    }
    SwarmServer --> CloudRouter
    SwarmServer --> LocalRouter
    SwarmServer --> KnowledgeRouter
    SwarmServer --> A2ARouter
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-SWARM-020
verified_date: 2026-09-28
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:149-158 (SwarmServer fields),:1026-1060 (tool_surface_tests); kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:173 (cloud_swarm_router); kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:347 (local_router); kask/mcp-servers/hkask-mcp-swarm/src/knowledge_tools.rs:12 (knowledge_router); kask/mcp-servers/hkask-mcp-swarm/src/a2a_tools.rs:18 (a2a_router)
status: VERIFIED
-->

## Cloud tools — 48

All 48 are defined in
`kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:173-2851` (the
`cloud_swarm_router` is declared at `:173`):

| Group | Tools |
| --- | --- |
| Catalogue and workspace | `swarm_list_agents`, `swarm_get_swarm`, `swarm_get_agent`, `swarm_update_agent`, `swarm_run_status`, `swarm_search_knowledge`, `swarm_ontology_templates`, `swarm_generate_prompt`, `swarm_generate_ontology` |
| Authorization and execution | `swarm_execute_agent`, `swarm_hire_cost`, `swarm_request_consent`, `swarm_authorize_session`, `swarm_hire`, `swarm_delegate`, `swarm_delegate_and_wait`, `swarm_fanout`, `swarm_create_swarm`, `swarm_xaman` |
| Agent and workspace lifecycle | `swarm_create_agent`, `swarm_fire`, `swarm_delete_agent`, `swarm_delete_swarm`, `swarm_publish_checks`, `swarm_publish_agent`, `swarm_fork_agent` |
| Apps | `swarm_list_apps`, `swarm_get_app`, `swarm_create_app`, `swarm_create_app_direct`, `swarm_update_app`, `swarm_publish_app`, `swarm_archive_app`, `swarm_spawn_app_workspace`, `swarm_list_app_workspaces`, `swarm_get_app_schema`, `swarm_fork_workspace_to_app` |
| Workspace actions and files | `swarm_workspace_list_actions`, `swarm_workspace_pending_actions`, `swarm_workspace_mutate_document`, `swarm_workspace_fork_state`, `swarm_workspace_accept_action`, `swarm_workspace_reject_action`, `swarm_workspace_annotate`, `swarm_workspace_list_annotations`, `swarm_workspace_list_files`, `swarm_workspace_read_file`, `swarm_workspace_write_file` |

The full 90-name surface is verified against the registered routers by the
name-set pin tests at
`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1037-1060`.

## Non-cloud tools — 42

### Local tools — 35

Defined in `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs` (router at
`:347`):

| Group | Tools |
| --- | --- |
| Delegation and workflow | `swarm_delegate_local` (standalone), `swarm_delegate_in_thread_local` (member), `swarm_fanout_local`, `swarm_pipeline_local`, `swarm_workflow_check_local`, `swarm_run_workflow_local`, `swarm_observed_seams_local`, `swarm_execute_plan_local`, `swarm_thread_local`, `swarm_list_local_threads` |
| Fleet and evaluation | `swarm_fleet_digest_local`, `swarm_who_answers_local`, `swarm_select_agent_local`, `swarm_evaluate_local`, `swarm_task_board`, `swarm_eval_suite_local`, `swarm_eval_agent_local` |
| Agent registry | `swarm_list_local_agents`, `swarm_get_local_agent`, `swarm_clone_to_local`, `swarm_push_to_cloud`, `swarm_remove_local`, `swarm_create_local_agent`, `swarm_reconfigure_local_agent`, `swarm_ai_assist` |
| Local swarms and synchronization | `swarm_create_local_swarm`, `swarm_list_local_swarms`, `swarm_get_local_swarm`, `swarm_delete_local_swarm`, `swarm_add_agent_local`, `swarm_remove_agent_local`, `swarm_update_local_swarm`, `swarm_clone_local_swarm`, `swarm_push_local_swarm`, `swarm_pull_swarm_to_local` |

`swarm_get_local_agent` returns the full local card and measured execution
statistics (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:963-1008`;
stats persistence at `kask/mcp-servers/hkask-mcp-swarm/src/agent_stats.rs:175-185`).
`swarm_workflow_check_local` validates a declared workflow before execution
(`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1009-1057`). A provided
`swarm_id` binds supported dispatches to roster membership and the ordered
encrypted member conversation; without it they remain standalone
(the binding mechanism `dispatch_in_thread` at
`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:278-352`, its callers at
`:354-900,1058-1183,2882-3087`). The eval
suite's
`swarm_id` only selects the task board.

### Knowledge tools — 4

Defined in `kask/mcp-servers/hkask-mcp-swarm/src/knowledge_tools.rs:23-229`:

- `swarm_search_knowledge_local`
- `swarm_recall_local`
- `swarm_generate_prompt_local`
- `swarm_generate_ontology_local`

### A2A tools — 3

Defined in `kask/mcp-servers/hkask-mcp-swarm/src/a2a_tools.rs:30-217`:

- `swarm_a2a_send`
- `swarm_a2a_card`
- `swarm_a2a_broadcast`

## Typing and validation

The built-in port registry and schema-bearing types live in
`kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs:41-170`. Agent-card
admission rejects unresolved `accepts` or `produces` labels
(`kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs:46-63`). Runtime task
classification is deliberately not guessed; only universal `text` is a
positive bind (`check_bind`,
`kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:551-556`).

The minimal schema validator supports `type`, `properties`, `required`,
`items`, `enum`, `const`, and `oneOf`; unsupported keywords produce
`UnsupportedSchema`
(`kask/mcp-servers/hkask-mcp-swarm/src/schema_validate.rs:10-18,222-229`).

## Local result model

`LocalDelegateResult` and its task-success, reliance, bind, contract-check,
and grounding data are defined in the local runtime public surface
(`kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:658-730`) and stamped
once by `LocalSwarmRuntime::delegate`
(`kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:318-423`). Agent
execution statistics are persisted and surfaced by local-agent detail/list
operations (`kask/mcp-servers/hkask-mcp-swarm/src/agent_stats.rs:12-18,175-185`).
The event store records rollout and observed-seam evidence through the local
tools; task progress is read through `swarm_task_board`
(`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:2882-3123`).

## Configuration and storage

`SwarmServer` owns the cloud client, consent store, local registries/runtime,
local knowledge, statistics, event store, and thread store
(`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:149-158`). The shared
consent-store path defaults to `mcp/swarm/consent.db`
(`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:188-205`). The server
resolves the one shared `HKASK_DB_PASSPHRASE` through the canonical MCP helper
before opening encrypted local memory
(`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:208-253`).

## Zed-side surfaces

`crates/swarm_panel` is the editor-side face of the swarm system: a center-pane
`Item` listing ABW agents and workspaces as cards, plus a Steer-mode
conversation framed around swarm work
(`crates/swarm_panel/src/swarm_panel.rs:1-25`). All ABW calls flow through the
governed tool invoker to this server — never ad-hoc HTTP from the UI. The
panel's lifecycle actions (hire with cost/consent gating, clone-to-local,
push-to-cloud, remove-local, fire, delete, and publish with a
`swarm_publish_checks` preflight then `swarm_publish_agent`, plus an audited
admin force-publish path) wire to `invoke_tool` calls; its Steer prompt renders
the ABW/local split from the generated `CLOUD_SWARM_TOOL_NAMES` const and the
complement of `TOOL_NAMES`, never a hand-maintained list
(`crates/swarm_panel/src/swarm_panel.rs:117-118,154-158`). The panel decomposes
into twelve feature modules beside its root — `agent_edit`, `ai_assist`,
`app_edit`, `author`, `card`, `compose`, `fetch`, `hire`, `member_turns`,
`panel_button`, `parse`, `swarm_ops` — each extracted from the original panel
body (`crates/swarm_panel/src/swarm_panel.rs:31-41`).

The local-swarm public surface is also consumed by `hkask-mcp-kata-kanban`,
which imports `LocalAgentCard`, `LocalAgentRegistry`, and the delegation
result types (`kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs:29`;
`kask/mcp-servers/hkask-mcp-kata-kanban/src/types.rs:564-567`).

## Procedures

These procedures use the current 91-tool surface: 48 cloud tools and 43
non-cloud tools (`kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:1016-1047`).

### Choose the execution path

```mermaid
flowchart TD
    A[Choose a task] --> B{Cloud ABW or local substrate?}
    B -->|Cloud| C[Preflight cost]
    C --> D[Obtain consent or session token]
    D --> E[Call cloud operation]
    B -->|Local| F[Inspect agent with swarm_get_local_agent]
    F --> G{Declared workflow?}
    G -->|Yes| H[Validate with swarm_workflow_check_local]
    H --> I[Run swarm_run_workflow_local]
    G -->|No| J[Delegate, fan out, pipeline, or execute plan]
    E --> K[Read result or run status]
    I --> K
    J --> K
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-SWARM-010
verified_date: 2026-09-28
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:539-977 (hire cost, consent, session, run status); kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:963-1057 (get agent, workflow check); kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:354-776,2882-3087 (delegation and plan dispatch)
status: VERIFIED
-->

### Hire or delegate through ABW

1. Call `swarm_hire_cost` before hiring
   (`kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:539-614`).
2. Obtain a single-use token with `swarm_request_consent`, or open a bounded
   headless session with `swarm_authorize_session`
   (`kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:616-730`).
3. Pass exactly one authorization form to `swarm_hire`, `swarm_delegate`,
   `swarm_delegate_and_wait`, `swarm_fanout`, `swarm_execute_agent`,
   `swarm_create_swarm`, or `swarm_xaman`. These operations route through the
   cloud authorization path in their definitions
   (`kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:476-528,697-860,1170-1387,1462-1555`;
   `swarm_xaman` at `:1316`).
4. For asynchronous workspace work, inspect `swarm_run_status`
   (`kask/mcp-servers/hkask-mcp-swarm/src/cloud_swarm_tools.rs:936-977`).

### Delegate to a local swarm member

1. Select a local swarm and inspect its roster with `swarm_get_local_swarm`.
2. Call `swarm_delegate_in_thread_local` with `swarm_id`, a roster member's
   `agent_name`, and the task. Nonmembers are refused before inference. The
   next member receives the earlier committed turns as ordered chat messages
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:278-352,354-381`).
3. Read the same durable conversation with `swarm_thread_local`
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:382-418`); Steer shows
   the selected local swarm's turns beside the Curator conversation. Use
   `swarm_list_local_threads` to find retained history after swarm deletion
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:419-458`). A cloned
   swarm begins with an empty conversation.

`swarm_delegate_local` is a **standalone** agent call: it accepts an agent
name and task without a swarm and neither reads nor writes member turns
(`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:459-566`). Local calls
require no ABW consent token. Declared tool and output-contract checks still
apply (`kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:658-730`).

### Run a declared workflow safely

1. Inspect the agent with `swarm_get_local_agent`
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:963-1008`).
2. Call `swarm_workflow_check_local`; do not start execution if the declared
   workflow is invalid
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1009-1057`).
3. Call `swarm_run_workflow_local` with `swarm_id` when its stages must
   participate in that swarm's ordered conversation; omitting it runs a
   standalone workflow. The tool also records observed handoff seams
   (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1058-1183`).
4. Use `swarm_observed_seams_local` to compare observed handoffs with declared
   ports (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1184-1288`).

### Fan out, pipeline, or execute a plan

- `swarm_fanout_local`, `swarm_pipeline_local`, and `swarm_run_workflow_local`
  use the shared conversation when `swarm_id` is supplied; without it they
  run standalone. Scoped fanout is ordered, so `parallel: true` with
  `swarm_id` is rejected instead of silently changing its meaning
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:567-776,1058-1183`;
  the fanout cap `MAX_FANOUT` at
  `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:654`).
- `swarm_execute_plan_local` uses the shared conversation when `swarm_id` is
  supplied; it also records task progress on the swarm's task board
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:2882-3087`).
  Omitting `swarm_id` keeps execution standalone.
- `swarm_task_board` reports progress and verdicts, **not** the conversation
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:3088-3123`).

### Measure agent or swarm reliability

Use `swarm_eval_agent_local` for repeated single-agent rollouts and
`swarm_eval_suite_local` for multi-delegation cases
(`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:3124-4336`, capped by
`MAX_EVAL_TASKS`/`MAX_EVAL_REPEATS`/`MAX_EVAL_ROLLOUTS` at `:29-40`). These
are standalone evaluations, not member conversation turns. The suite's optional
`swarm_id` selects task-board progress only; it does not admit members to the
thread.

### Move agents and swarms between local and cloud

- `swarm_clone_to_local`, `swarm_push_to_cloud`, `swarm_remove_local`,
  `swarm_create_local_agent`, and `swarm_reconfigure_local_agent` manage local
  cards and cloud linkage
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:1446-2174,2605-2843`;
  registry listing/detail at `:901-1057`).
- Local swarm CRUD and membership tools are defined at
  `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:2175-2366`.
- `swarm_clone_local_swarm`, `swarm_push_local_swarm`, and
  `swarm_pull_swarm_to_local` synchronize swarm composition with ABW
  (`kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs:2367-2604`).

## Further reading

- [Swarm explanation](./explanation.md)
