---
title: "MCP Dispatch Diagrams — Runtime Invoke, Tool-Call Sequence, CMP Tool Flow"
audience: [architects, developers, agents]
last_updated: 2026-08-28
version: "1.0.0"
status: "Active"
domain: "Trust"
mds_categories: [trust, composition, domain]
---

# MCP Dispatch Diagrams

Consolidated diagrams for the MCP dispatch path: the `McpRuntime::invoke`
metering flow, the end-to-end tool-call sequence from the agent's tool-use
loop, and the CMP research tool-call flow across servers. Unique
`DIAGRAM_ALIGNMENT` IDs are preserved from the originals.

## MCP Runtime Invoke — Metering and Dispatch Flow

**`invoke` does not authorize.** It charges one call against the agent's
per-tick runaway ceiling, dispatches, and emits the outcome span. The only
pre-dispatch refusal is an exhausted ceiling. Verified current (unchanged
since the 2026-08-12 RR-0056/RR-0057 corrections).

```mermaid
flowchart TD
    A["invoke(server, tool, args, agent)"] --> B{"governance.is_some()?"}
    B -- "No" --> G["call_tool_inner — dispatch unmetered"]
    B -- "Yes" --> C["charge_call_metered(agent)"]
    C -- "Charged" --> G
    C -- "AutoRegistered (log wiring gap)" --> G
    C -- "CeilingReached" --> K["Return EnergyBudgetExceeded<br/>(runaway-loop breaker)"]
    G --> I["Emit reg.gas.settled span<br/>(target reg.mcp)"]
    I --> J["Return result"]
```

The call meter is fail-open on an *unregistered* agent: it auto-registers at
`DEFAULT_RUNAWAY_CALL_CEILING` (10 000) and logs the wiring gap rather than
refusing — a missing seed is a wiring omission, not an authorization
decision (RR-0057). A runtime with no governance wired dispatches unmetered
rather than failing closed.

Where authority is enforced instead:

- the per-request `tool_allowlist` on the inference IPC `tool_invoke`
  dispatch (`kask/crates/kask_bridge/src/inference_ipc_server.rs`),
  fail-closed on a missing or empty allowlist
- each swarm agent card's declared `mcp_tools` allowlist
  (`kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs`)
- the per-server MCP env/credential allowlists
  (`kask/crates/kask_bridge/src/mcp_servers.rs`, RR-0038)

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CAP-002
verified_date: 2026-08-28
verified_against: kask/crates/hkask-mcp/src/runtime.rs (impl hkask_tool_port::ToolPort for McpRuntime, call_tool_inner); kask/crates/hkask-regulation/src/energy.rs (CallMeterOutcome L30-40, DEFAULT_RUNAWAY_CALL_CEILING L26); kask/crates/hkask-mcp/tests/invoke_gate.rs
status: VERIFIED
-->

## MCP Tool Call — LazyToolRouter to McpRuntime::invoke to unwrap_tool_envelope

The `LazyToolRouter` filters MCP candidates by keyword score but **bypasses
built-in tools** (they are never candidates). The retained tool set reaches
`McpRuntime::invoke`, which meters one call against the agent's per-tick
runaway ceiling and dispatches — it does **not** authorize. The result is
unwrapped from its `{"content": value}` envelope by `unwrap_tool_envelope`.
Verified current.

```mermaid
sequenceDiagram
    participant Agent as Agent (Thread)
    participant Router as LazyToolRouter<br/>crates/agent/src/tool_router.rs
    participant Enabled as Thread::enabled_tools<br/>crates/agent/src/thread.rs
    participant ToolPort as ToolPort trait<br/>hkask-tool-port/src/tool_port.rs
    participant Runtime as McpRuntime<br/>hkask-mcp/src/runtime.rs
    participant Cap as CallCapManager<br/>hkask-regulation/src/energy.rs
    participant Server as MCP server child<br/>kask/mcp-servers/hkask-mcp-*
    participant Unwrap as unwrap_tool_envelope<br/>hkask-types/src/tool_response.rs

    Agent->>Enabled: build tool descriptions (name, description)
    Enabled->>Router: apply_router_bypassing_built_ins(router, tools, message, open_files, built_in_names)
    Note over Router: Built-in tools (grep, read_file, skill, spawn_agent, ...) are never candidates.<br/>The router scores MCP tools only by keyword overlap against descriptions.
    alt router should_activate (complex message or explicit tool name)
        Router->>Router: select_tools(context) → Some(Vec<tool_name>)
        Router-->>Enabled: retained set = built_ins ∪ selected MCP tools
    else router does not activate (simple message)
        Router-->>Enabled: None → fail-open (retain all tools)
    end
    Enabled-->>Agent: enabled_tools set

    Agent->>ToolPort: invoke(server, tool, args, agent: WebID)
    ToolPort->>Runtime: McpRuntime::invoke (impl ToolPort for McpRuntime)
    alt governance is wired
        Runtime->>Cap: charge_call_metered(agent)
        alt Charged
            Cap-->>Runtime: CallMeterOutcome::Charged
        else AutoRegistered (unseeded agent)
            Cap-->>Runtime: CallMeterOutcome::AutoRegistered<br/>(auto-register at DEFAULT_RUNAWAY_CALL_CEILING, log wiring gap)
        else CeilingReached
            Cap-->>Runtime: CallMeterOutcome::CeilingReached { ceiling }
            Runtime-->>ToolPort: Err(ToolPortError::EnergyBudgetExceeded)<br/>(runaway-loop breaker — only pre-dispatch refusal)
            ToolPort-->>Agent: error
        end
    else no governance (tests, lightweight embedders)
        Runtime->>Runtime: dispatch unmetered
    end
    Runtime->>Server: dispatch over stdio (one bounded reconnect on closed transport)
    Server-->>Runtime: tool output (JSON)
    Runtime->>Runtime: emit reg.gas.settled span (target reg.mcp)
    Runtime-->>ToolPort: result Value
    ToolPort-->>Agent: result Value
    Agent->>Unwrap: unwrap_tool_envelope(result)
    Note over Unwrap: If result is {"content": value}, return value.<br/>Otherwise return result unchanged (bare payload).
    Unwrap-->>Agent: unwrapped result
```

`apply_router_bypassing_built_ins` (in `crates/agent/src/tool_router.rs`) is
the single seam that protects built-in tools: the router's candidate set is
built from MCP tools only; built-ins are passed through unconditionally via
the `built_in_names` set. `LazyToolRouter::select_tools` activates only when
the message is complex (≥ `complex_word_threshold` = 6 words) or explicitly
mentions a tool name; otherwise it returns `None` (fail-open — retain all
tools). The thresholds (`threshold: 0.30`, `complex_word_threshold: 6`) are
pinned by `default_thresholds_are_the_documented_values` and must match
`KaskToolRouterSettings::default()` in `kask_bridge`.

Every MCP tool response is a `{"content": <value>}` envelope.
`unwrap_tool_envelope` (`hkask-types/src/tool_response.rs:61`) is the single
seam that extracts the inner value; the property
`unwrap_tool_envelope({"content": P}) == P` for all JSON payloads `P` is
pinned by proptest in `hkask-types/src/tool_response.rs`.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-SEQ-MCP-TOOL-CALL-001
verified_date: 2026-08-28
verified_against: crates/agent/src/tool_router.rs (LazyToolRouter, apply_router_bypassing_built_ins, select_tools, should_activate, default_thresholds_are_the_documented_values); crates/agent/src/thread.rs (enabled_tools); kask/crates/hkask-tool-port/src/tool_port.rs (ToolPort, ToolPortError::EnergyBudgetExceeded); kask/crates/hkask-mcp/src/runtime.rs (impl ToolPort for McpRuntime, charge_call_metered); kask/crates/hkask-regulation/src/energy.rs (CallMeterOutcome L30-40, DEFAULT_RUNAWAY_CALL_CEILING L26); kask/crates/hkask-types/src/tool_response.rs (unwrap_tool_envelope L61); kask/crates/kask_bridge/src/inference_ipc_server.rs (tool_allowlist gate); kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs (mcp_tools allowlist); kask/crates/kask_bridge/src/mcp_servers.rs (BuiltinMcpServer.credentials)
status: VERIFIED
-->

## CMP Tool Call Flow

The CMP research pipeline is accessible via MCP tools. The agent or panel
calls the tools in sequence: build CMP indices from catalogs, compose them
into a scenario tree, and feed the tree into tree-weighted valuation. The
integration seam between the scenarios server and the companies server is
caller-mediated — the caller pastes the tree JSON from
`scenario_from_cmp_indices` into the `event_tree` parameter of
`scenario_analysis`.

**Corrections (2026-08-28):** the falsification tail of the flow (steps 5–7:
`h2_duration_test`, `h3_coherence_test`, `falsification_log`) was deleted
from `hkask-forecast` — the flow now ends at `equity_duration`. The
`EventTreeProjection` contract is unchanged.

```mermaid
flowchart TD
    step1["1. build_cmp_indices<br/>(prediction-markets server)"] -->|"ProvenancedCmpIndex[]"| step2
    step2["2. scenario_from_cmp_indices<br/>(scenarios server)"] -->|"EventTree JSON<br/>+ cmp_provenance"| step3
    step3["3. scenario_analysis<br/>(companies server)<br/>event_tree = paste tree JSON"] -->|"weighted scenarios<br/>+ expected intrinsic"| step4
    step4["4. equity_duration<br/>(companies server)"] -->|"duration_years"| output["Valuation output<br/>weighted scenarios + duration"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-FLOW-001
verified_date: 2026-08-28
verified_against: kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_index_builder.rs (build_cmp_indices_from_lines L488); kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs (scenario_from_cmp_indices L626); kask/mcp-servers/hkask-mcp-companies/src/tools/analytics.rs (scenario_analysis L686); kask/mcp-servers/hkask-mcp-companies/src/tools/valuation.rs (equity_duration L481); falsification tail deleted — h2_duration_test / h3_coherence_test / falsification_log no longer exist in kask/crates/hkask-forecast/src/
status: VERIFIED
-->

### The caller-mediated seam

The scenarios server and the companies server do not depend on each other
directly. The integration is caller-mediated: the agent (or panel) takes the
tree JSON output from `scenario_from_cmp_indices` and pastes it into the
`event_tree` parameter of `scenario_analysis`. The `EventTreeProjection`
struct in the companies server is the documented contract of what the bridge
consumes.

```mermaid
sequenceDiagram
    participant Agent
    participant PM as prediction-markets
    participant Scen as scenarios
    participant Comp as companies

    Agent->>PM: build_cmp_indices(family, venue, config)
    PM-->>Agent: ProvenancedCmpIndex[]

    Agent->>Scen: scenario_from_cmp_indices(indices, date, deps)
    Scen-->>Agent: EventTree JSON + cmp_provenance

    Agent->>Comp: scenario_analysis(symbol, event_tree=paste)
    Comp-->>Agent: weighted scenarios + expected intrinsic

    Agent->>Comp: equity_duration(symbol)
    Comp-->>Agent: duration_years
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-FLOW-002
verified_date: 2026-08-28
verified_against: kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs (scenario_from_cmp_indices L626-687); kask/mcp-servers/hkask-mcp-companies/src/tools/analytics.rs (scenario_analysis L686); kask/mcp-servers/hkask-mcp-companies/src/superforecast.rs (EventTreeProjection L219); kask/mcp-servers/hkask-mcp-companies/src/tools/valuation.rs (equity_duration L481)
status: VERIFIED
-->

## See also

- [Architecture diagrams](./architecture.md) — tool port type system, skill/MCP/lisp seam, credential chain
- [Swarm diagrams](./swarm.md) — the swarm server dispatch surface
