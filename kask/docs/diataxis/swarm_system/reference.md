---
title: "Swarm Systems — Reference: The 50-Tool Surface and Components"
audience: [developers, operators]
last_updated: 2026-08-04
version: "0.1.1"
status: "Active"
domain: "Swarm"
mds_categories: [domain]
---

# Swarm Systems — Reference: The 50-Tool Surface and Components

A reference for the `hkask-mcp-swarm` tool surface, the panel components, and
the two skills. The surface is pinned by `tool_surface_is_exactly_50_registered_tools`
(`hkask_mcp_swarm.rs:3355`) — 27 ABW + 23 local, both sets always registered
in either mode; `kask.swarm.mode` selects the substrate, not the surface. See
the [class diagram](../../diagrams/class-swarm-server.md) for the type
relationships.

## Tool surface (50)

### ABW tools (27) — cloud, `mode: abw`

| Tool | Purpose | Gate |
|------|---------|------|
| `swarm_list_agents` | catalogue read | none |
| `swarm_get_swarm` | workspace detail | none |
| `swarm_get_agent` | agent detail | none |
| `swarm_list_apps` | ABW apps | none |
| `swarm_ontology_templates` | ontology templates | none |
| `swarm_execute_agent` | one LLM call (text consult) | none |
| `swarm_hire_cost` | pre-hire cost check (`within_budget`) | read |
| `swarm_request_consent` | mint single-use consent token | — |
| `swarm_authorize_session` | pre-authorize session budget (headless) | — |
| `swarm_hire` | hire (own `/add` 2 cr; third-party `/hire` 5 cr base) | consent + ceiling |
| `swarm_delegate` | delegate (1 cr + tokens) | consent + ceiling |
| `swarm_delegate_and_wait` | delegate + poll for response | consent + ceiling |
| `swarm_fanout` | parallel multi-agent fan-out (cap `MAX_FANOUT_ABW`) | consent per dispatch |
| `swarm_run_status` | run status / messages | read |
| `swarm_generate_prompt` | generate agent prompt | read |
| `swarm_generate_ontology` | generate ontology | read |
| `swarm_create_agent` | create agent | — |
| `swarm_create_swarm` | create workspace (`POST /api/teams`) | consent (auto-hires deps) |
| `swarm_xaman` | delegate to Xaman Ek curator (steering built-in) | consent |
| `swarm_create_app` | create ABW app | — |
| `swarm_fire` | remove from roster (reversible) | — |
| `swarm_delete_agent` | permanent agent deletion | — |
| `swarm_delete_swarm` | permanent workspace delete (`DELETE /api/teams/{id}`) | — |
| `swarm_search_knowledge` | vector knowledge-graph search | read |
| `swarm_publish_checks` | publish preflight | read |
| `swarm_publish_agent` | catalogue publish (admin force-publish path) | — |
| `swarm_fork_agent` | derivative fork | — |

### Local tools (23) — `mode: local`, zed-kask substrate

| Tool | Purpose | Gate |
|------|---------|------|
| `swarm_fund_local` | fund operator ledger (starts at 0) | — |
| `swarm_balance_local` | read balance (proactive algedonic) | read |
| `swarm_local_history` | recent ledger transactions (reconciliation) | read |
| `swarm_delegate_local` | run a local agent (debit-before-scan) | balance + ceiling |
| `swarm_fanout_local` | parallel fan-out | balance per dispatch |
| `swarm_pipeline_local` | sequential pipeline with `{{prev_output}}` | balance |
| `swarm_a2a_send` | A2A protocol message (in-process) | — |
| `swarm_a2a_card` | A2A Agent Card discovery | read |
| `swarm_list_local_agents` | local registry read | read |
| `swarm_clone_to_local` | cloud → local (carries `cloud_id`) | — |
| `swarm_push_to_cloud` | local → cloud | — |
| `swarm_remove_local` | delete local card (cloud untouched) | — |
| `swarm_create_local_agent` | create local agent | — |
| `swarm_reconfigure_local_agent` | re-prompt in place (C6) | — |
| `swarm_create_local_swarm` | create a named local swarm | — |
| `swarm_list_local_swarms` | list named local swarms | read |
| `swarm_get_local_swarm` | get a named local swarm's detail | read |
| `swarm_delete_local_swarm` | delete a named local swarm | — |
| `swarm_add_agent_local` | add an agent to a local swarm | — |
| `swarm_remove_agent_local` | remove an agent from a local swarm | — |
| `swarm_search_knowledge_local` | search the agent's prefix-scoped `hkask-memory` (EAV) | read |
| `swarm_generate_prompt_local` | local-LLM system-prompt authoring aid (memory-seeded, guard-scanned) | read |
| `swarm_generate_ontology_local` | local-LLM seed Mermaid ER from a domain / agent memory | read |

Note: `swarm_pipeline_local` and the A2A pair are omitted from the Steer
system prompt's curated tool list (audit Gap S2); they remain available via
the governed tool surface.

## Local Knowledge Tools — search & author over `hkask-memory`

The three local knowledge tools are the kask-vernacular analogs of ABW's
`swarm_search_knowledge` / `swarm_generate_prompt` / `swarm_generate_ontology`.
Where ABW backs them with fermi's per-agent dreaming-memory KG + fermi's LLM
generation, the local analogs back them with the **operator's own
`hkask-memory`** and the **local `InferencePort`** (Ollama/cloud via the zed IPC
bridge). They execute and resolve entirely on the kask substrate — no ABW
round-trips, no fermi code. Design rationale: [Local Knowledge Tools design](../../plans/local-swarm-knowledge-tools.md).

The unifying idea: **memory IS the knowledge graph.** A local agent's
"knowledge graph" is its prefix-scoped slice (`agent:<agent_id>:`) of the
operator's consolidated `SemanticMemory` (entity-attribute-value triples).
Consolidation (`ConsolidationBridge`) already promotes episodic memories into
semantic triples — the local KG is that consolidated memory.

### Contracts

```
swarm_search_knowledge_local({agent_name, query, limit?}) -> {fragments[], source, agent_name, note}
swarm_generate_prompt_local({description, agent_name, agent_type?}) -> {prompt, raw}
swarm_generate_ontology_local({domain_description, agent_name?}) -> {ontology, raw}
```

- **`swarm_search_knowledge_local`** — searches the agent's prefix-scoped
  semantic memory. The `query` is matched case-insensitively against each
  triple's entity, attribute, and value (EAV retrieval — "memory as a graph").
  Returns `fragments[]` of `{entity, attribute, value, confidence}`. This is
  the EAV path; vector-KNN search is a future option (see the design doc).
- **`swarm_generate_prompt_local`** — a local one-shot LLM generate that turns a
  `description` into a system prompt, seeded with the agent's memory when
  available. Output is guard-scanned (canary/secret). Returns `{prompt, raw}`
  (matches the ABW `swarm_generate_prompt` envelope).
- **`swarm_generate_ontology_local`** — a local one-shot LLM generate of a
  Mermaid `erDiagram` for a domain, optionally seeded with an agent's
  semantic-memory graph. Guard-scanned. Returns `{ontology, raw}`.

The generate tools use the **already-resolved** `InferencePort` (via
`LocalSwarmRuntime::inference()`), not a second inference path. Generated
output is scanned by the same `ContentGuard` as the delegate loop.

### Configuration & the default passphrase

The semantic-memory store is SQLCipher-encrypted. The passphrase is read
from `HKASK_SWARM_MEMORY_PASSPHRASE`; the **pre-release default is `"allostery"`**
(the kask-wide default for any user-facing passphrase that isn't an internally
generated key), so the tools work out of the box without operator config.
Override the passphrase for a real secret; the DB path is
`<hkask data dir>/swarm_memory.db` (override `HKASK_SWARM_MEMORY_DB`). The
embedding-dim config (`HKASK_SWARM_EMBEDDING_DIM`, default 1024) is reserved
for the future vector-KNN path; the EAV search does not depend on it.

### Graceful degradation

If the store cannot be opened (e.g., an existing DB was created under a
different passphrase), `swarm_search_knowledge_local` returns
`{fragments: [], note: "memory_unconfigured: ..."}` — never a panic, never a
fabricated hit (the `.rules` `unwrap_or(0)` trap avoided). The generate tools
proceed unseeded (memory is an enhancement, not a dependency) — they still
produce a prompt/ontology via the local LLM, just without the memory seed.

### Source

| Symbol | Location |
|--------|----------|
| `LazyLocalMemory` / `search_agent_knowledge` / `one_shot_generate` | `local_knowledge.rs` |
| `SwarmConfig.memory_passphrase` / `memory_db_path` / `embedding_dim` | `config.rs` |
| `swarm_search_knowledge_local` / `swarm_generate_prompt_local` / `swarm_generate_ontology_local` | `hkask_mcp_swarm.rs` |
| `SemanticMemory` (`search_similar`, `query_deduped`, `query_by_attribute`) | `kask/crates/hkask-memory/src/semantic.rs` |

## Server components

| Component | Source | Role |
|-----------|--------|------|
| `SwarmServer` | `hkask_mcp_swarm.rs:115` | the rmcp server; `combined_router` (`:124`) registers all 50 tools |
| `AbwClient` | `abw_client.rs` | ABW REST; 200 body may carry upstream LLM error, 500 for domain failure (`SwarmError` inspects body) |
| `ConsentStore` | `consent.rs:56` | real-time spend gate; `mint`/`consume`/`refund`; sqlite or memory; TTL `:77` enforced |
| `SpendGate` | `spend_gate.rs` | `authorize_hire`/`complete_hire`, `authorize_delegate`, `authorize_curate`, session variants; ceiling refunds on refusal |
| `LocalAgentRegistry` | `local_registry.rs` | reads `agents/local/curated/<id>/agent_card.json`; `LocalAgentCard` carries `cloud_id` |
| `LazyLocalSwarmRuntime` | `local_runtime.rs:39` | `OnceCell` defers async init to first call; caches resolved ports |
| `LocalSwarmRuntime` | `local_runtime.rs:73` | owns spending policy (ceiling, balance, cost, debit) + final output scan |
| `AgentExecutor` | `agent_executor.rs:55` | owns agent-run policy (input scan, skill cascade, tool loop); `MAX_TOOL_ROUNDS=4`, `MAX_SKILLS_PER_DELEGATION=3` |
| `A2A` | `a2a.rs:24` | in-process transport; wraps `delegate` in `a2a-lf` types (AgentCard, Task, Message, Part, Artifact); HTTP binding deferred |

## Panel components

| Component | Source | Role |
|-----------|--------|------|
| `SwarmPanel` | `swarm_panel.rs:345` | the center-pane `Item`; 4 modes |
| `PanelMode` | `swarm_panel.rs:322` | `Browse` / `Author` / `Compose` / `Steer` |
| `SwarmFilter` | `swarm_panel.rs:312` | `All` / `Swarms` / `Agents` (Browse) |
| `steer_system_prompt` | `swarm_panel.rs:112` | the Curator's system prompt in Steer |
| `set_mode` / `set_swarm_mode` | `:1798` / `:1834` | mode + backend toggles |
| `ensure_steer_conversation` | `:1870` | lazily builds the curator `ConversationView` |
| `init` (Toggle) | `:230` / `:261` | deploys panel + explicit focus fix (`.rules` deploy-and-focus trap) |
| `ToolInvoker` hook | `tool_invoker.rs:22` | global hook the panel uses for direct MCP dispatch (set from `main.rs`, Mutex-based, re-settable) |
| `SwarmPanelButton` | `panel_button.rs:13` | status-bar toggle, dispatches `Toggle` |

See the [panel modes state diagram](../../diagrams/state-swarm-panel-modes.md).

## Skills

| Skill | Role | Process manifest |
|-------|------|------------------|
| `swarm-intelligence` | planner — 10-step PDCA cascade (SENSE…LOOP); emits `emitted_calls` plan | `kask/registry/manifests/swarm-intelligence.yaml` |
| `swarm-steering` | actuator — execute-and-feed-back directive (single DIRECT step) | `kask/registry/manifests/swarm-steering.yaml` |

### `swarm-intelligence` Cybernetic Swarm Plan components (C0–C8)

| C | What | Enforcement |
|---|------|-------------|
| C0 | task-success `s` (fourth axis of `d`) | CHECK + manifest `task_success` input (deterministic only — Gap S3) |
| C1 | second-order monitor (reasoning loop + sensor-truth divergence) | `swarm.second_order_monitor` compute (step 9) |
| C2 | Go See cadence (every N convergences) | `cadence_every` param; SENSE surfaces `go_see` |
| C3 | failed-edit memory (anti-loop set) | `swarm.filter_proposed_moves` (step 4) |
| C4 | delegation latency `T_q` | `LocalDelegateResult.latency_ms` — **sensed, not regulated** (audit C4) |
| C5 | fault attribution (priority rule over delegate trace) | ORIENT + `fault_count` accumulate |
| C6 | `swarm_reconfigure_local_agent` (re-prompt blamed agent) | DECIDE move type; active only with C5 telemetry |
| C7 | influence-weighted rejection | `swarm.filter_proposed_moves` (step 4) |
| C8 | task-gated alignment (OFA-MAS TAGSE) | SENSE `alignment` definition |

### Convergence criterion

`d = sqrt( (1−variety)² + max(0, diversity_floor−diversity)² + (1−loop_closure)² )`
plus `(1−s)²` when `task_success` is supplied. Converged when
`|d_i − d_{i−1}| < 0.03` for 3 iterations. **Algedonic override:** a 402 or
un-acknowledged curator dispatch escalates regardless of `d`.

## Token model (do not conflate)

| Token | Scope | TTL | Enforcement |
|-------|-------|-----|-------------|
| `ConsentGrant` (ABW spend) | action + target + credits, single-use | `CONSENT_TTL_SECS` enforced | real-time blocking gate (`consent.rs:184`) |
| `SessionGrant` (ABW headless) | session budget + action set | enforced | `consume_session` (`:280`) |
| `DelegationToken` (OCAP) | resource + resource_id + action, in-process | **none** | `McpRuntime::invoke` (no signature, no unforgeability — see `.rules` OCAP block) |

## See also

- [Tutorial](./tutorial.md) · [How-to](./how-to.md) · [Explanation](./explanation.md)
- [Swarm Cybernetics/Semantics Audit](../../audits/swarm-cybernetics-semantics-audit.md)
- [Architecture](../../diagrams/flowchart-swarm-architecture.md) ·
  [PDCA Cascade](../../diagrams/flowchart-swarm-pdca-cascade.md) ·
  [Steering Loop](../../diagrams/sequence-swarm-steering-loop.md) ·
  [Feedback Loops](../../diagrams/flowchart-swarm-feedback-loops.md)