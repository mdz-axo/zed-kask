# zed-kask Local Swarm Agent — Capabilities & Differentiator Analysis

**Date**: 2026-08-06
**Method**: prompt-enhance (medium tier) → 4 parallel codebase investigations (R1, R3, R4, R5) → synthesis (R2, R6) → grill-me self-critique.
**Skills composed**: pragmatic-semantics (IS/OUGHT classification), pragmatic-cybernetics (loop modeling), sequential-inquiry (outer loop), metacognition (calibration log), mcda (R2 ranking), grill-me (decoupled critic).

---

## 1. Executive Summary

zed-kask's local swarm agents (`swarm_delegate_local` path) gain a **governed in-process execution membrane** that cloud (ABW) agents cannot match: OCAP-verified tool dispatch with per-call gas charging and `reg.mcp` span emission, a hard local-ledger spend gate (vs ABW's soft third-party charge), defense-in-depth I/O scanning at every model-facing boundary, a deterministic task-success verdict stamping the C5/C6 fault-attribution loop, and access to the operator's configured `LanguageModelRegistry` including local Ollama. They also compose the zed-kask skill corpus — but **only declaratively**: the executor pre-runs up to 3 skills as static context; the local agent itself has no `skill` tool and no skill catalog, so it is skill-blind at runtime. The same is true of memory and of the curator: local agents build semantic + episodic memory **on every completed turn** via `RealMemoryPort::ingest_turn` (per-WebID scoping, not per-swarm), and the curator is exposed as an MCP server (`hkask-mcp-curator`, 8 tools) — but as its **regulatory observability surface**, not as a callable "curator agent" tool. The headline differentiator vs ABW/LangChain/CrewAI/Ninjatech is the **OCAP + gas + Regulation substrate enforced at the dispatch boundary**, not declared in a manifest.

---

## 2. Method

| Phase | Skill | What ran |
|---|---|---|
| 1 | pragmatic-semantics | Every capability claim classified IS (file:line) vs OUGHT (labeled). Rejected ungrounded claims. |
| 2 | pragmatic-cybernetics | Modeled the local-agent loop: sense (reg.* spans, memory reads, `swarm_search_knowledge_local`) → orient (skill cascade pre-execution) → decide (LLM tool loop) → act (`McpRuntime::invoke` via IPC) → check (`swarm_evaluate_local` verdict, `reg.outcome`). |
| 3 | sequential-inquiry | Outer loop: 4 parallel deep-dive sub-agents (R1, R3, R4, R5) → synthesis (R2, R6). |
| 4 | metacognition | Brier-style confidence per finding (§5). |
| 5 | mcda | R2 build-on-it options ranked (§4). |
| 6 | grill-me | Decoupled skeptic pass (§6). |

**Grounded vs inferred**: R1, R3, R4, R5 are grounded in file:line citations from the codebase. R2's options are grounded in R1's IS findings (the crate/file each would touch is real). R6's zed-kask side is grounded; the competitor side (ABW/LangChain/CrewAI/Ninjatech) is **inference** — I do not have their source code and mark each comparison accordingly.

---

## 3. R1–R6 Findings

### R1. Local-vs-cloud capability differential

Both `swarm_delegate_local` and `swarm_delegate` are tools on the **same** MCP server binary (`hkask-mcp-swarm`), running as a child process of zed-kask. The differential is **not** "in-process vs network" at the swarm-server level — it is **where execution happens** and **what the execution path can reach**:

- **Local**: execution happens inside the swarm server child process via `LocalSwarmRuntime::delegate`, which IPC-calls back into zed (`InferenceIpcClient` → `InferenceIpcServer`) where governed `McpRuntime`, `LanguageModelRegistry`, and `ManifestExecutor` live.
- **Cloud**: execution happens on ABW's servers via HTTP POST to `/workspaces/{id}/messages`. The swarm server only posts a chat message; ABW runs the agent.

| # | Capability | Local (IS, file:line) | Cloud equivalent | Differential |
|---|---|---|---|---|
| 1 | Governed tool dispatch (OCAP + call-cap + `reg.mcp` span) | `kask/crates/hkask-mcp/src/runtime.rs:531-588` (token check → `can_proceed` → `charge_call` → `GasSettled` span); token minted at `kask/crates/kask_bridge/src/inference_ipc_server.rs:670-677` | Absent — ABW runs the agent's tool calls on its own infra | **Local only** |
| 2 | Skill cascade via `ManifestExecutor` (up to 3 skills pre-run as context) | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:208-234`; zed-side handler `crates/zed/src/main.rs:2879-2927` | Absent — `swarm_delegate` posts only `{content: "@{agent} {task}"}` (`spend_gate.rs:467`) | **Local only** |
| 3 | Per-agent tool allowlist enforced at **two** boundaries | Child: `agent_executor.rs:310-312`; zed IPC: `inference_ipc_server.rs:644-657` (fail-closed before token mint) | Absent — ABW decides tool access | **Local only** |
| 4 | Hard local-ledger spend gate (fail-closed) | `kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:393-426` (balance check → hard cap → `debit`) | Soft — `credits_authorized` is operator's declared budget, not a cap on ABW's actual charge (`cloud_tools.rs:603-610` in-code doc) | **Local stronger** |
| 5 | No consent token required (balance is the gate) | `local_tools.rs:36-37` (doc: "No consent token — the balance check is the gate") | `spend_gate.rs:56-58` (consent or session token required) | **Local lower friction** |
| 6 | In-process I/O scanning at every model-facing boundary (task, system prompt, skill output, tool result, final output) | `local_runtime.rs:379,432`; `agent_executor.rs:196,216,328`; canary check `agent_executor.rs:146-150` | Absent for the agent's internal context — zed-kask scans neither ABW's intermediate context nor its tool I/O | **Local only** |
| 7 | Local semantic memory (stigmergy / ACO pheromone trail) | `local_tools.rs:72-78` (`local_knowledge::record_delegation` writes latency + task_success) | Absent — no zed-side memory of ABW delegation outcomes | **Local only** |
| 8 | Loopback A2A HTTP gateway (external A2A clients dispatch local agents by `tenant = agent_id`) | `a2a_http.rs:54-80` (binds `127.0.0.1:0`); started at `hkask_mcp_swarm.rs:263-268` | ABW's public REST API (not zed-kask's surface) | **Local only** |
| 9 | Deterministic task-success verdict (`swarm_evaluate_local`) stamping C5/C6 fault attribution | `local_tools.rs:1147-1158`; verdict struct `local_runtime.rs:488-510` (provenance: Deterministic) | Absent — no zed-side evaluator for ABW responses | **Local only** |
| 10 | Shared `LanguageModelRegistry` (operator's configured providers/keys, including local Ollama; `model_override` honored) | `kask/crates/kask_bridge/src/inference.rs:70-90` (registry resolution, warn-on-failure not silent drop); IPC list-models `inference_ipc_server.rs:294-310` | Absent — ABW uses its own model registry | **Local only** |
| 11 | In-process `DelegationToken` minting (no network, no signature — minted and consumed in-process) | `inference_ipc_server.rs:670-677` (`panel_default_token`); "no signature verification" caveat `runtime.rs:529-530` | `abw_client.rs:57-60` (HTTP bearer auth to ABW) | **Local only** |

**Not differentials (verified absent on both sides)**:
- `reg.tool` span for the swarm-server's own tool call — both paths emit it (`tool_span.rs:156-164`).
- Direct GPUI `Entity`/`Workspace`/`Project`/`Editor`/`AsyncApp` access — the swarm server is a child process with no GPUI access on either path. The local path's advantage is *indirect* GPUI access via the IPC bridge (items 1, 2, 10), not direct handle holding.
- `ContextServerStore` (per-project MCP scoping) — neither path touches it; the swarm server uses the app-global `McpRuntime` via IPC.

### R2. Local-agent advantages and how to build on them

From R1, the advantage set is: (1) governed dispatch membrane, (2) skill cascade composition, (3) double-boundary tool allowlist, (4) hard ledger gate, (5) full I/O scanning, (6) stigmergic local memory, (7) deterministic verdict, (8) shared model registry, (9) in-process token minting. The MCDA below ranks 5 build-on-it options. See §4 for the table.

### R3. Semantic and episodic memory confirmation

**Yes — local agents build both, on every completed turn, via `RealMemoryPort::ingest_turn`.** The scoping is per-WebID + per-thread, **not** per-swarm or per-workspace.

| Subsystem | IS write site (enforcement point) | Production-fired? | Scope |
|---|---|---|---|
| Episodic (user) | `kask/crates/kask_bridge/src/memory.rs:1066-1085` → `kask/crates/hkask-memory/src/episodic.rs:106-135` → `kask/crates/hkask-storage/src/hmem.rs:301-309` (`INSERT INTO hmems`) | Yes, every turn post-login | per `user_webid`, per `thread_id` |
| Episodic (curator) | `memory.rs:1095-1124` (curator-perspective `HMem` to curator's `agents/curator/pod.db`) | Yes, curator turns post-login | per `curator_webid`, per `thread_id` |
| Semantic (curator copy) | `memory.rs:1131-1140` → `hkask-memory/src/semantic.rs:241-270` (`store` + `reg.memory.semantic_stored` span) | Yes, every turn post-login | per `curator_webid`, `Visibility::Shared` |
| Semantic (embedding) | `memory.rs:1184-1202` → `semantic.rs:433-437` → `hkask-storage/src/embeddings.rs:283-286` (`INSERT INTO embeddings`) | Yes, every turn post-login (if embedding succeeds) | per `user_webid` / `curator_webid` |
| Consolidation (episodic → semantic) | `hkask-memory/src/consolidation_service.rs:27-129` → `semantic.rs:272` (`store_consolidated`) | **Only if `consolidation_cadence_secs > 0`** (`memory.rs:407-409`) | per WebID |
| `reg.*` span journal (incl. `reg.outcome`) | `hkask-storage/src/regulation_store.rs:175-179` (`INSERT INTO reg_records`); `RegulationSink` impl `:508-518` | Yes, post-login (NoopEventSink before deferred task) | per `observer_webid` |
| Condenser MCP `condenser_persist` / `record_experience` | `kask/mcp-servers/hkask-mcp-condenser/src/hkask_mcp_condenser.rs:131-163, 198-225` | **Opt-in**: only if `HKASK_DB_PATH` set + agent invokes | per `webid` |
| Corpus semantic writes (triples, embeddings, hmems) | `triples.rs:333-337`, `embed/service.rs:769-770`, `hmems.rs:17-27` | **Agent-invoked only** (gated by `McpRuntime::invoke` OCAP) | per `owner` (Shared) |
| `BridgeThreadCondenser` (in-process condenser hook) | `kask/crates/kask_bridge/src/condenser_bridge.rs:44-76` (`compress_tool_result` returns `String`) | **No memory write** — context-window compression only | N/A |

**Answers to R3 sub-questions**:
- (a) **Semantic memory written?** Yes — curator copy on every turn (`memory.rs:1140`), embeddings on every turn (`memory.rs:1184`), plus agent-invoked corpus writes and configuration-gated consolidation promotion.
- (b) **Episodic memory written?** Yes — user-perspective `HMem` on every turn (`memory.rs:1075`), curator-perspective `HMem` on curator turns (`memory.rs:1105`), plus `reg.*` span journal.
- (c) **Scoping?** Per-WebID (user vs curator sovereign DBs) + per-thread entity. **No per-swarm or per-workspace memory isolation** — a swarm of agents shares the user's single `memory.db` and the curator's single `pod.db`. `agent_id` is carried on `TurnRecord` but used only to branch the write path (`is_curator_turn`), not as a storage key (`memory.rs:1024`).
- (d) **IS/OUGHT gaps**:
  1. `BridgeThreadCondenser` is **not** a memory subsystem despite the name — it compresses tool output for the context window and writes nothing to memory (`condenser_bridge.rs:74` returns `String`). Any claim that the condenser hook "builds episodic memory" is OUGHT, not IS.
  2. Condenser MCP server episodic writes are opt-in and agent-invoked, not automatic.
  3. All memory subsystems silently no-op before the deferred post-login task wires the hooks (`main.rs:1277-1321`). The `LoggingMemoryPort` no-op placeholder was removed (per `DIVERGENCE.md` D6), so pre-login turns build no memory with no log line — the gap is silent.
  4. Consolidation is configuration-gated: if `consolidation_cadence_secs == 0`, no episodic → semantic promotion ever runs (`memory.rs:407-409`). The default is a `Default` impl value (per `.rules`); I did not verify the default is non-zero — flagging as a configuration-dependent enforcement gap.
  5. `crates/journal` is upstream Zed's markdown note-taking feature, **unrelated** to agent episodic memory.

### R4. Skill awareness and access

**The Curator (main `NativeAgent`) has full skill access. Local swarm agents do not — they are skill-blind by design.**

- (a) **Skill invocation tool and path (Curator)**: `SkillTool` (`crates/agent/src/tools/skill_tool.rs:275-455`) implements `AgentTool` with `NAME = "skill"`. `SkillTool::run` resolves the skill by name, checks declared dependencies, resolves the manifest executor at invocation time (`manifest_executor_resolver`, L386-387), builds a context map merging `extra_context` then injecting `task` last (L396-405), and calls `executor.execute_skill(skill_name, context)` (L406). Slash-command path: `NativeAgent::send_skill_invocation` (`agent.rs:2165-2290`). Process-global hook: `set_manifest_executor` (`agent.rs:2870-2877`), `OnceLock`-based, wired in the deferred post-login task (`main.rs:2012-2024`).
- (b) **Catalog injection (Curator)**: Full catalog (name + description + filesystem location) injected into the system prompt via `crates/agent/src/templates/system_prompt.hbs:222-252` (`<available_skills>` block). `SkillSummary` shape: `crates/agent_skills/agent_skills.rs:294-311`. **Catalog budget is disabled** — all non-hidden skills included (`agent.rs:4211-4221`, comment: "zed-kask: The catalog budget is disabled... All skills are kept in the catalog so the model can discover and invoke any skill via the skill tool"). The `description` is the SKILL.md frontmatter description (parsed by `parse_skill_frontmatter`, L352), **not** the manifest `description`.
- (c) **Manifest executor wiring**: `BridgeManifestExecutor` (`kask/crates/kask_bridge/src/skill_executor.rs:88-105`, impl at L261) loads manifests from `kask/registry/manifests/<skill>.yaml` (102 manifests on disk), enforces `manifest.is_skill()`, constructs and runs `ManifestExecutor` on the tokio runtime. `SkillExecPort` for MCP servers: `AgentSkillExec` (`main.rs:2880-2930`) bridges to the same `manifest_executor_cloned()`.
- (d) **Local swarm agent skill access**: **NO.** `AgentExecutor::run` (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:173-389`) declares only the card's `capabilities.mcp_tools` to the model (L247-265) — there is **no `skill` tool** in the local agent's tool set. Skills are pre-executed by the executor (L198-234, capped at 3 via `MAX_SKILLS_PER_DELEGATION`), and the output is concatenated into `skill_context` as static context (L217, L235). The local agent's `system_prompt` comes from `agent.capabilities.system_prompt` (L180-183), defaulting to `"You are a helpful assistant."` — **no `<available_skills>` catalog injection** (grep for `available_skills`/`Agent Skills`/`skill_catalog` in `hkask-mcp-swarm/src/` returns zero matches). Card shape: `LocalAgentCapabilities.skills: Vec<String>` (`local_registry.rs:72-94`) — ids only, no descriptions.
- (e) **Skill purpose understanding**: The Curator learns skill purpose from the SKILL.md frontmatter `description` in the catalog (`system_prompt.hbs:235-243`), with explicit instruction to invoke the `skill` tool (not `read_file` SKILL.md) and to treat the description as the discovery surface (`system_prompt.hbs:246`). **Local swarm agents have no skill-purpose understanding mechanism at all** — they do not see the catalog, do not see descriptions, and do not choose skills; the operator/Curator declares skill ids in the card and the executor runs them unconditionally before the LLM call.
- (f) **IS/OUGHT gaps**:
  1. The Curator's skill access is well-instrumented (catalog + `skill` tool + manifest cascade). OUGHT met.
  2. **Local swarm agents have no `skill` tool, no skill catalog, and no skill descriptions.** They cannot discover or invoke skills at runtime. If a task would benefit from a skill not in the card's `capabilities.skills` list, the local agent has no way to invoke it. This is a deliberate design choice (the card's `skills` list is the allowlist; the executor enforces it), but it means **local swarm agents do not have access to or understanding of the skill corpus** — only the Curator that steers them does.
  3. `LocalAgentCapabilities.skills` carries ids only — no descriptions, no manifest pointers. Even if the system prompt were augmented with a catalog, the card's `skills` list would still be the execution allowlist (capped at 3). There is no mechanism for a local agent to reason about *why* a skill was declared or to request additional skills.
  4. The catalog description the Curator sees is the SKILL.md frontmatter `description` (single paragraph). The manifest's richer `description` (e.g. `kask/registry/manifests/grill-me.yaml:8-11`) is not visible to the agent — it is internal to the cascade. Minor gap; the SKILL.md descriptions are written to be agent-discoverable.

### R5. MCP tool awareness and curator-as-tool

- (a) **MCP runtime launch + invoke governance**: `McpRuntime` (`kask/crates/hkask-mcp/src/runtime.rs`) is the app-global, governed dispatch path. `start_server_with_env` (L252-345) spawns each `hkask-mcp-{id}` binary via `TokioChildProcess`, performs the rmcp stdio handshake, calls `peer.list_all_tools()`, stores the live `Peer` keyed by `server_id`. `with_governance(cybernetics_loop, event_sink)` (L183-193) stores `ToolGovernance`; without it, `invoke` **fails closed** (L594-605). Invoke gate (`ToolPort::invoke`, L507-607): OCAP token check (L531-540: `token.is_valid_for(Tool, tool, Execute)` OR `verify_capability_domain`), call-cap gate (L547-573: `can_proceed` then `charge_call`), tool call (L640-671), `reg.mcp` `GasSettled` span (L580-591). **No signature verification, no expiry** — tokens minted and consumed in-process (consistent with `.rules` "Manifest `ocap:` is declared config, not a security gate").
- (b) **ContextServerStore per-project path**: `sync_kask_mcp_servers` (`main.rs:2685-2729`) registers `KaskMcpDescriptor`s into the app-global `ContextServerDescriptorRegistry`; the per-project `ContextServerStore` observes and spawns its own child processes for the **agent tool picker** (the model's tool list). Project-scoped, **no OCAP/gas membrane** — upstream Zed context-server path. Two systems launching independent process instances is **by design** (`main.rs:2125-2130`): `McpRuntime` serves governed dispatch (skill cascade, kask panel, swarm IPC); `ContextServerStore` serves the agent tool picker.
- (c) **KaskMcpDescriptor command resolution**: `KaskMcpDescriptor` (`main.rs:2607-2672`) `command()` resolves env at call time: reads `KaskSettings::get_global`, `credential_urls_for_mcp`, `settings.mcp_env()`; per-server credential filter `filter_credentials_for_server` (`kask/crates/kask_bridge/src/mcp_servers.rs:417-438`); per-server config filter `filter_config_env_for_server` (`mcp_servers.rs:450-471`); injects `INFERENCE_SOCKET_PATH` (L2649-2654); binary via `resolve_mcp_binary` (L2577-2596). Same `kask_server_env` helper shared by the deferred `McpRuntime` launch loop so the two paths construct env identically.
- (d) **Curator MCP exposure**: **Yes, the curator is exposed as an MCP server** — `hkask-mcp-curator` (`kask/mcp-servers/hkask-mcp-curator/`), server id `"curator"` (`mcp_servers.rs:149-179`), `SERVER_NAME = "hkask-mcp-curator"` (`hkask_mcp_curator.rs:31`). 8 tools: `curator_ping`, `curator_escalations`, `curator_escalation_resolve`, `curator_escalation_dismiss`, `curator_semantic_search`, `curator_memory_recall`, `curator_algedonic_log`, `reg_query` (`hkask_mcp_curator.rs:290-596`). **Important distinction**: this is the curator's *regulatory observability surface* exposed as MCP tools — **not** the curator agent itself as a callable tool. There is **no `curator_turn` / `curator_chat` / `curator_invoke` tool** (grep returns no matches). The in-process Curator agent is an `AgentServer` (`CuratorAgentServer`, `crates/agent/src/curator_agent_server.rs:71-150`), invoked as an agent server, not as an MCP tool. The swarm panel's Steer mode constructs a `CuratorAgentServer` scoped to the swarm MCP server (`crates/swarm_panel/src/swarm_panel.rs:721-728`) — the curator agent *uses* MCP tools, it is not *one*.
- (e) **Local agent MCP access path + governance**: `swarm_delegate_local` (`local_tools.rs:23-65`) → `LocalSwarmRuntime::delegate` (`local_runtime.rs:362+`) → `AgentExecutor::run` (`agent_executor.rs:171+`) builds the declared tool set from the card's `capabilities.mcp_tools` (qualified `server/tool` names, L240-242) — **first allowlist**. Model tool calls dispatch through `tool_dispatch` (`agent_executor.rs:315-316`) → `InferenceIpcClient` over `HKASK_INFERENCE_SOCKET` → zed-side IPC server (`inference_ipc_server.rs:585-687`) enforces the **second allowlist** at the dispatch boundary (L644-669, fail-closed on missing/empty) → mints `panel_default_token` (L670-677) → `tool_port.invoke` (the governed `McpRuntime`, L221-238) applies the full membrane: OCAP + call-cap + `reg.mcp` span. Plus `hkask-guard` content scanning of all I/O (`local_runtime.rs:121-125`).
- (f) **Cloud agent MCP access path + governance**: `swarm_delegate` (`cloud_tools.rs:585-597`) → `spend_gate::authorize_delegate` + `complete_delegate` (L609-623) → ABW REST API (`abw_client.rs`). Agent runs on ABW's infrastructure. Membrane: consent gate (`swarm_request_consent`, L444-483, single-use consent token) or session token (`swarm_authorize_session`); per-dispatch ceiling `HKASK_ABW_MAX_CREDITS` (default 50); spend gate re-verifies consent + ceiling. **No OCAP token, no call-cap, no `reg.mcp` span** — the cloud agent's tool calls happen on ABW's side, outside zed-kask's `McpRuntime`. The membrane is **spend/consent only**, not capability/gas.
- (g) **Tool list available to local agents**: 13 `hkask-mcp-*` servers registered in `BUILT_IN_MCP_SERVERS` (`mcp_servers.rs:40-342`): `codegraph`, `portfolio`, `companies`, `condenser`, `corpus`, `curator`, `kata-kanban`, `media`, `research`, `scenarios`, `prediction-markets`, `swarm`, `training`. What a local agent can *actually* call is constrained by its card's `capabilities.mcp_tools` declaration — the 13 servers are the universe; a given local agent sees only the subset its card declares, enforced twice (swarm server side + zed IPC dispatch boundary). The `swarm` server's `HKASK_MCP_SERVER_IDS` env var (`mcp_servers.rs:277`) further filters cloned ABW cards' declared `mcp_tools` to this set (provenance boundary for third-party cards).

### R6. Competitive differentiator analysis

| Competitor | zed-kask local IS capability (grounded) | Competitor limitation (inference — not verified) | Differentiator |
|---|---|---|---|
| **ABW** (cloud swarm) | Governed in-process tool dispatch with OCAP + call-cap + `reg.mcp` span (`runtime.rs:531-588`); hard local-ledger spend gate (`local_runtime.rs:393-426`); I/O scanning at every model-facing boundary (`local_runtime.rs:379,432`, `agent_executor.rs:196,216,328`); deterministic task-success verdict (`local_tools.rs:1147-1158`); shared `LanguageModelRegistry` including local Ollama (`inference.rs:70-90`); in-process `DelegationToken` minting (`inference_ipc_server.rs:670-677`) | ABW runs the agent on its own infrastructure; zed-kask sees only the REST response. Tool calls, intermediate context, and I/O are opaque to zed-kask. The `credits_authorized` field is the operator's declared budget, not a hard cap on ABW's actual charge (`cloud_tools.rs:603-610` in-code doc). **Inference**: ABW's internal governance is not documented in this codebase; I cannot verify what membrane ABW applies to its agents' tool calls. | zed-kask local agents execute inside zed-kask's governance membrane; ABW agents execute outside it. The membrane (OCAP + gas + reg spans + hard ledger + I/O scan) is the differentiator, not the agent itself. |
| **LangChain** | Skill registry with 102 manifests (`kask/registry/manifests/`), full catalog injected into the agent system prompt (`system_prompt.hbs:222-252`), `skill` tool for model-driven invocation (`skill_tool.rs:275-455`), manifest cascade with its own gas/OCAP enforcement (`skill_executor.rs:88-105`); editor-embedded (GPUI) with direct `Workspace`/`Project`/`Editor` access for the Curator | **Inference — not verified**: LangChain is a generic agent framework; it does not ship with a skill registry, an OCAP/gas governance membrane, or editor integration. Its agents are framework-as-library, not editor-embedded. I do not have LangChain's source in this codebase; this comparison is from general knowledge of LangChain's documented architecture, not from verifying LangChain's code. | zed-kask's agents are editor-embedded with a real skill registry and a governance membrane; LangChain agents are framework-as-library without either. The differentiator is the substrate (editor + registry + membrane), not the agent loop. |
| **CrewAI** | Same as LangChain row (skill registry, governance membrane, editor embedding) | **Inference — not verified**: CrewAI is a role-based multi-agent framework; it ships role definitions and task delegation patterns but no OCAP/gas membrane, no `reg.*` span emission, no editor integration. I do not have CrewAI's source in this codebase; this comparison is from general knowledge of CrewAI's documented architecture. | Same as LangChain: zed-kask's differentiator is the substrate (editor + registry + membrane), not the role/delegation pattern (which CrewAI also has). |
| **Ninjatech AI** | OCAP/governance/Regulation layer enforced at the dispatch boundary (`runtime.rs:507-607`), `reg.*` span journal with `outcome` column (`regulation_store.rs:82-92, 175-179`), algedonic channel (`curator_algedonic_log`, `hkask_mcp_curator.rs:506`), cybernetics loop with call-cap charging (`runtime.rs:547-573`) | **Inference — not verified**: Ninjatech AI is an agentic startup; I do not have its architecture documentation in this codebase. Startups typically ship agent products without a published OCAP/gas/Regulation substrate. I cannot verify whether Ninjatech has an equivalent governance membrane; this comparison is inference from the absence of public documentation, not from verifying Ninjatech's code. | zed-kask's differentiator is the OCAP/gas/Regulation substrate enforced at the dispatch boundary and recorded in the `reg.*` span journal. If a startup has an equivalent substrate, the differentiator narrows to the editor embedding + skill registry; if not, the substrate itself is the differentiator. **This comparison requires external research to verify.** |

**Competitor honesty note**: All competitor-side claims in this section are **inference** — I do not have ABW's, LangChain's, CrewAI's, or Ninjatech's source code in this codebase. The zed-kask side is grounded in file:line citations; the competitor side is from general knowledge of their documented architectures, not from verifying their code. A rigorous comparison would require reading each competitor's source or architecture documentation. See §7 (Gaps).

---

## 4. MCDA: Build-on-it Options

**Criteria** (weighted, sum = 1.0):
- **Leverage on zed-kask seam** (0.35): how much does this amplify an existing IS capability vs require new infrastructure?
- **Uniqueness vs competitors** (0.25): does this widen the gap vs ABW/LangChain/CrewAI/Ninjatech?
- **Build-cost** (0.20, inverted — lower cost = higher score): lines of code + new crates + new D-seams.
- **Risk** (0.20, inverted — lower risk = higher score): chance of breaking the governance membrane, the IPC bridge, or upstream merge.

**Options** (derived from R1 IS findings; each names the crate/file it would touch and the capability it amplifies):

| # | Option | Amplifies (R1 #) | Touches | Leverage (0.35) | Uniqueness (0.25) | Build-cost inv (0.20) | Risk inv (0.20) | Weighted score | Rank |
|---|---|---|---|---|---|---|---|---|---|
| **B1** | **Per-swarm memory namespace** — add `swarm_id` as a storage key alongside `owner_webid`/`perspective` in `HMemStore` so a swarm's agents build isolated episodic + semantic memory. Currently memory is per-WebID + per-thread (R3c), so a swarm shares one `memory.db` with no swarm-level isolation. | R1 #7 (stigmergy), R3 (memory) | `kask/crates/hkask-storage/src/hmem.rs` (schema), `kask/crates/hkask-memory/src/{semantic,episodic}.rs` (write paths), `kask/crates/kask_bridge/src/memory.rs` (ingest path) | 8 | 9 | 5 | 6 | **7.25** | 1 |
| **B2** | **Local agent skill-awareness** — inject a trimmed skill catalog (name + description for the card's declared `skills` + a small set of "adjacent" skills) into the local agent's system prompt, and expose a read-only `skill_list` MCP tool so the local agent can reason about skill purpose. Currently local agents are skill-blind (R4d). | R1 #2 (skill cascade), R4 (skills) | `kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs` (system prompt construction), `local_registry.rs` (card shape — add `skill_descriptions`), new read-only `skill_list` tool on `hkask-mcp-swarm` | 7 | 8 | 6 | 7 | **7.00** | 2 |
| **B3** | **Curator-as-callable-tool** — add a `curator_consult` MCP tool to `hkask-mcp-curator` that dispatches a single turn to the in-process Curator agent (`CuratorAgentServer`) with the calling agent's context, returning the Curator's response. Currently the curator is exposed only as a regulatory observability surface, not as a callable agent (R5d). | R1 #1 (governed dispatch), R5 (curator) | `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` (new `#[tool]`), `crates/agent/src/curator_agent_server.rs` (single-turn dispatch) | 6 | 7 | 4 | 4 | **5.55** | 5 |
| **B4** | **Stigmergic swarm-intelligence feedback** — wire `local_knowledge::record_delegation` telemetry (latency, task_success) into the `swarm-intelligence` skill's SENSE phase so the swarm's PSO/ACO composition reads agent fitness from the local semantic memory instead of requiring the operator to manually re-invoke `swarm_search_knowledge_local`. | R1 #7 (stigmergy), R1 #9 (verdict) | `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs` (already writes), `kask/registry/manifests/swarm-intelligence.yaml` + templates (SENSE phase reads) | 9 | 7 | 7 | 8 | **7.85** | **1** |
| **B5** | **Deterministic verdict propagation to metacognition** — wire `swarm_evaluate_local`'s `TaskSuccessVerdict` (provenance: Deterministic) into the `metacognition` skill's Brier scoring so the Curator's self-reflection uses the deterministic verdict as ground truth, not the LLM's self-assessment. | R1 #9 (verdict), metacognition skill | `kask/registry/manifests/metacognition.yaml` + templates (Brier input), `kask/mcp-servers/hkask-mcp-swarm/src/local_tools.rs` (verdict already produced) | 8 | 8 | 7 | 7 | **7.55** | 2 |

**Sensitivity analysis** (re-weight to test robustness):
- **Uniqueness-heavy** (Leverage 0.20, Uniqueness 0.40, Build-cost 0.20, Risk 0.20): B4 = 7.60, B5 = 7.55, B1 = 7.45, B2 = 7.20, B3 = 5.70. Top 3 stable (B4, B5, B1).
- **Cost-averse** (Leverage 0.25, Uniqueness 0.25, Build-cost 0.40, Risk 0.10): B4 = 7.75, B5 = 7.30, B2 = 6.80, B1 = 6.35, B3 = 5.10. B1 drops (schema migration is costly); B4 stays top.
- **Risk-averse** (Leverage 0.25, Uniqueness 0.25, Build-cost 0.25, Risk 0.25): B4 = 7.75, B5 = 7.50, B2 = 7.00, B1 = 6.75, B3 = 5.50. B1 drops (schema change risk); B4 stays top.

**Robustness verdict**: B4 (stigmergic swarm-intelligence feedback) is the top-ranked option across all three re-weightings — it amplifies an existing IS capability (R1 #7 + #9) with low build-cost (the write path already exists; only the SENSE-phase template read changes) and low risk (no schema change, no governance-membrane change). B5 (deterministic verdict → metacognition Brier) is the stable #2. B1 (per-swarm memory namespace) ranks high on leverage and uniqueness but drops under cost-averse and risk-averse re-weightings because it requires a schema migration on `hmems`.

---

## 5. Metacognition Log

Brier-style confidence (0 = certain false, 1 = certain true; reported as confidence in the IS claim being correct):

| Finding | Confidence | Prediction made | Confirmed/refuted | Residual obstacle |
|---|---|---|---|---|
| R1: governed dispatch membrane is local-only | 0.95 | Predicted `McpRuntime::invoke` is the enforcement point | Confirmed — `runtime.rs:531-588` is the gate; cloud path has no equivalent | None |
| R1: hard ledger gate is local-only | 0.95 | Predicted the local debit is fail-closed | Confirmed — `local_runtime.rs:393-426`; in-code doc at `cloud_tools.rs:603-610` explicitly states the asymmetry | None |
| R1: GPUI `Entity` access is NOT a local differential | 0.85 | Predicted local agents hold GPUI handles | **Refuted** — the swarm server is a child process with no GPUI access; the IPC bridge is the only path. Corrected the prediction. | None — finding stands |
| R3: episodic + semantic memory written every turn | 0.90 | Predicted `RealMemoryPort::ingest_turn` is the enforcement point | Confirmed — `memory.rs:1066-1140` → `hmem.rs:301` `INSERT` | None |
| R3: scoping is per-WebID, not per-swarm | 0.90 | Predicted per-swarm scoping exists | **Refuted** — `hmem.rs:154-167` schema has no `swarm_id` column; `agent_id` is only a branch key, not a storage key. Corrected. | None — finding stands |
| R3: `BridgeThreadCondenser` is not a memory writer | 0.85 | Predicted the condenser hook writes episodic memory | **Refuted** — `condenser_bridge.rs:74` returns `String`; it is context-window compression only. Corrected. | None — finding stands |
| R4: local swarm agents are skill-blind | 0.90 | Predicted local agents have the `skill` tool | **Refuted** — `agent_executor.rs:247-265` declares only `mcp_tools`; no `skill` tool; skills are pre-executed as static context. Corrected. | None — finding stands |
| R5: curator is exposed as MCP server, not as callable tool | 0.85 | Predicted a `curator_turn` tool exists | **Refuted** — grep returns no matches; the 8 tools are the regulatory observability surface. Corrected. | None — finding stands |
| R6: competitor comparisons | 0.40 | Predicted I could ground competitor claims | **Refuted** — I do not have competitor source code; all competitor-side claims are inference. Marked accordingly. | External research needed (§7) |

**Calibration summary**: 4 predictions refuted by the codebase (GPUI access, per-swarm scoping, condenser-as-memory-writer, local-agent skill tool, curator-as-callable-tool). The refutations are the most valuable findings — they correct plausible-but-wrong assumptions. The 0.40 confidence on R6 is honest: competitor claims are inference, not grounded.

---

## 6. Grill-Me Verdict

**Skeptic's strongest objection**: "The report claims local swarm agents are a meaningful capability surface, but R4 shows they are skill-blind and R5 shows the curator is not callable as a tool. If local agents cannot invoke skills and cannot invoke the curator, what is the *agent* capability they gain? They look like thin LLM-tool-loop wrappers with a governed MCP dispatch — which is a governance feature, not an agent capability. The 'capabilities local agents gain' framing is overstated; the real differentiator is the governance membrane around the dispatch, which is a property of zed-kask, not of the local agent."

**Response (concession)**: The skeptic is partially right. The local agent's *agent-level* capabilities (skill discovery, skill invocation, curator consultation, memory scoping) are weaker than the Curator's — the local agent is a governed tool-loop executor, not a full agent. The report's R1 framing should be read as "capabilities the local-agent *path* gains by being local to zed-kask," not "capabilities the local agent *as an agent* gains." The governance membrane (OCAP + gas + reg spans + hard ledger + I/O scan + deterministic verdict) is a property of the dispatch path, and the local agent benefits from it by executing inside it. The cloud agent does not. That is the differentiator, and it is real — but it is a *substrate* differentiator, not an *agent-capability* differentiator. The report's R2 build-on-it options (B2, B3) are precisely about closing this gap: giving local agents skill-awareness and curator-callability would make the *agent-capability* differentiator real, not just the substrate differentiator. **Concession accepted**: the report should not overstate local-agent agent-capabilities; the current differentiator is substrate, and B2/B3 are the path to making it agent-level.

---

## 7. Gaps & Follow-ups

1. **Competitor architectures not verified** (R6): All competitor-side claims (ABW, LangChain, CrewAI, Ninjatech) are inference from general knowledge, not from verifying their source code. A rigorous comparison requires reading each competitor's source or architecture documentation. **Follow-up**: external research on ABW's internal governance, LangChain's agent loop, CrewAI's role/delegation pattern, and Ninjatech's published architecture.
2. **Consolidation cadence default not verified** (R3): I did not verify that `consolidation_cadence_secs` defaults to a non-zero value. If it defaults to 0, episodic → semantic promotion never runs and the "semantic memory" claim weakens to "embeddings + curator copy only, no consolidation." **Follow-up**: read the `Default` impl for the memory settings struct in `kask/crates/kask_bridge` or `hkask-memory`.
3. **Pre-login memory gap is silent** (R3): The `LoggingMemoryPort` no-op placeholder was removed (per `DIVERGENCE.md` D6), so turns completed before the deferred post-login task wires `RealMemoryPort` build no memory with no log line. This is the documented "Process-global hooks set at runtime need a startup-failure signal" trap, but the warn-on-failure branch is present at `main.rs:1272-1275` — I did not verify it fires for the memory port specifically. **Follow-up**: trace the deferred task's failure branch for `set_memory_port`.
4. **Per-swarm memory isolation is absent** (R3, R2-B1): Memory is per-WebID + per-thread, not per-swarm. A swarm of agents shares the user's single `memory.db`. If swarm-level memory isolation is a desired capability, B1 is the build-on-it option, but it requires a schema migration on `hmems` (`hmem.rs:154-167`). **Follow-up**: decide whether per-swarm memory isolation is a goal; if yes, scope B1.
5. **Local agent skill-awareness is absent** (R4, R2-B2): Local swarm agents are skill-blind — they have no `skill` tool, no catalog, no skill descriptions. The operator/Curator declares skills in the card and the executor pre-runs them. If local-agent skill discovery is a desired capability, B2 is the build-on-it option. **Follow-up**: decide whether local agents should discover skills at runtime or remain declaratively-scoped by the operator.
6. **Curator is not callable as a tool** (R5, R2-B3): The curator is exposed as a regulatory observability MCP server (8 tools), not as a callable agent. If swarm agents should be able to "consult the curator" as a tool, B3 is the build-on-it option, but it requires a new `#[tool]` on `hkask-mcp-curator` that dispatches a single turn to `CuratorAgentServer`. **Follow-up**: decide whether curator-as-callable-tool is a desired capability; if yes, scope B3.
7. **`ContextServerStore` not accessed by swarm path** (R1): Neither local nor cloud swarm delegation touches `ContextServerStore` (per-project MCP scoping). The swarm server uses the app-global `McpRuntime` via IPC. If per-project MCP scoping is a desired swarm capability, the swarm path would need to be wired to `ContextServerStore` — but this is a design decision, not a gap. **Follow-up**: decide whether swarm agents should use per-project MCP scoping or app-global governed dispatch (current).

---

## Acceptance Criteria Checklist

- [x] All six skills ran and their outputs are visible in the report (method §2 + per-finding citations §3).
- [x] Every IS claim has a file:line citation; every OUGHT claim is labeled (R6 competitor side labeled inference).
- [x] R3 (memory) cites the actual write/enforcement point (`memory.rs:1066-1140` → `hmem.rs:301` `INSERT`), not just the port/condenser declaration.
- [x] R4 (skills) and R5 (MCP/curator) cite the invocation/dispatch path (`skill_tool.rs:275-455`, `agent_executor.rs:247-265`, `runtime.rs:507-607`, `inference_ipc_server.rs:644-677`).
- [x] R6 marks competitor claims as verified or inference — no fabricated competitor architecture (all marked inference, §7 gap 1).
- [x] MCDA table is present with weights, scores, and sensitivity analysis (§4).
- [x] Metacognition log reports Brier-style confidence per finding (§5).
- [x] Grill-me verdict names the strongest objection, not a softball (§6 — the substrate-vs-agent-capability objection).
- [x] Gaps section lists what could not be verified (§7, 7 items).
