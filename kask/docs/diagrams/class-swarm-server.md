---
title: "Swarm Server Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-14
version: "1.0.2"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust]
---

# Swarm Server Class Diagram

The `hkask-mcp-swarm` server (`SwarmServer`) exposes 52 tools (27 ABW + 25
local) — both sets always registered; `kask.swarm.mode` selects the substrate,
not the surface — pinned by
`tool_surface_is_exactly_52_registered_tools` (`hkask_mcp_swarm.rs`). `SwarmServer` composes five collaborators:
the ABW REST client, the consent store (real-time spend gate with TTL), the
local agent registry, the lazily-initialized local runtime, and the central
verification store (grounding ledger). The spend gate
consumes consent grants before any debit; the local runtime owns the
debit-before-return invariant (it debits the ledger, then `AgentExecutor`
returns the result so a failed delegation still costs credits). The verification
store runs grounding enforcement on every delegation via `enforce_and_stamp()`.
The A2A layer
wraps the existing `delegate` in protocol-compliant types over the in-process
transport (no HTTP server required). See the [Swarm MCP Server Architecture](flowchart-swarm-architecture.md).

```mermaid
classDiagram
    direction TD
    class SwarmServer {
        +client: AbwClient
        +consent: ConsentStore
        +local_registry: LocalAgentRegistry
        +local_runtime: LazyLocalSwarmRuntime
        +combined_router() Router
    }
    class AbwClient {
        ABW REST base agent-bestiary.world
        Bearer auth Pro tier key
        200 body may carry LLM error
    }
    class ConsentStore {
        +mint(action,target,credits) ConsentGrant
        +consume(token,action,target,cost) Result
        +refund(grant)
        +open_session(...) SessionGrant
        -CONSENT_TTL_SECS enforced
        -sqlite or memory backing
    }
    class spend_gate_module {
        <<module>>
        +authorize_hire()
        +complete_hire()
        +authorize_delegate()
        +complete_delegate()
        +authorize_curate()
        +resolve_auth()
        ceiling HKASK_ABW_MAX_CREDITS
        crate-private fns no SpendGate struct
    }
    class LazyLocalSwarmRuntime {
        -ledger_path: String
        -inner: OnceCell
        +lazy(path) Self
        +get_or_init() LocalSwarmRuntime
    }
    class LocalSwarmRuntime {
        -ledger: Arc~Ledger~
        -executor: AgentExecutor
        -operator_account: String
        +delegate(card,task,credits) LocalDelegateResult
        debit before return invariant
    }
    class AgentExecutor {
        -inference: InferencePort
        -tool_dispatch: ToolDispatchPort
        -skill_exec: SkillExecPort
        +run(card,task) RawDelegateResult
        MAX_TOOL_ROUNDS 4
        MAX_SKILLS_PER_DELEGATION 3
    }
    class LocalAgentRegistry {
        reads agents/local/curated
        LocalAgentCard carries cloud_id
    }
    class A2A {
        +to_a2a_card(card,base_url) AgentCard
        +message_from_text(text,ctx) Message
        in-process transport no HTTP
        wraps LocalSwarmRuntime.delegate
    }
    class LocalDelegateResult {
        agent_id
        response
        model
        tokens_used
        cost
        balance
        latency_ms
        tool_calls[] ok error
        executed_skills[] ok error
    }

    SwarmServer --> AbwClient : abw mode
    SwarmServer --> ConsentStore
    SwarmServer --> LocalAgentRegistry : local mode
    SwarmServer --> LazyLocalSwarmRuntime : local mode
    spend_gate_module ..> ConsentStore : consumes grants
    SwarmServer ..> spend_gate_module : hire delegate fanout xaman
    LazyLocalSwarmRuntime ..> LocalSwarmRuntime : get_or_init
    LocalSwarmRuntime --> AgentExecutor : run
    LocalSwarmRuntime ..> LocalDelegateResult : produces
    A2A ..> LocalSwarmRuntime : wraps delegate
    LocalAgentRegistry ..> LocalSwarmRuntime : reads cards

    note for SwarmServer "53 tools = 27 ABW + 26 local\nBoth sets always registered\nkask.swarm.mode selects the substrate not the surface\nSpend mutating tools are consent gated\npinned by tool_surface_is_exactly_53_registered_tools"
    note for LocalDelegateResult "Fed back as delegate_results\nto swarm-intelligence ORIENT\nactivates C5 fault attribution\nand C6 reconfigure"
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-006
verified_date: 2026-08-14
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs (combined_router, tool_surface_is_exactly_53_registered_tools); kask/mcp-servers/hkask-mcp-swarm/src/consent.rs; kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs (crate-private authorize_*/complete_* fns, no pub struct SpendGate); kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs; kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs (AgentExecutor: inference, tool_dispatch, skill_exec — no guard field, no scan_input/scan_output); kask/mcp-servers/hkask-mcp-swarm/src/a2a.rs
status: VERIFIED
-->
