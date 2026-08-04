# Swarm Server Class Diagram

The `hkask-mcp-swarm` server (`SwarmServer`) exposes 41 tools (27 ABW + 14
local) selected by `kask.swarm.mode`. `SwarmServer` composes four collaborators:
the ABW REST client, the consent store (real-time spend gate with TTL), the
local agent registry, and the lazily-initialized local runtime. The spend gate
consumes consent grants before any debit; the local runtime owns the
debit-before-scan invariant (it debits the ledger, then `AgentExecutor` scans
the output so a guard-quarantined result still costs credits). The A2A layer
wraps the existing `delegate` in protocol-compliant types over the in-process
transport (no HTTP server required). See the [Swarm Cybernetics/Semantics Audit](../audits/swarm-cybernetics-semantics-audit.md) and the [Swarm MCP Server Architecture](flowchart-swarm-architecture.md).

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
    class SpendGate {
        +authorize_hire()
        +complete_hire()
        +authorize_delegate()
        +authorize_curate()
        +authorize_hire_with_session()
        +authorize_delegate_with_session()
        ceiling HKASK_ABW_MAX_CREDITS
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
        debit before scan invariant
    }
    class AgentExecutor {
        -inference: InferencePort
        -tool_dispatch: ToolDispatchPort
        -skill_exec: SkillExecPort
        -guard: ContentGuard
        +run(card,task) RawDelegateResult
        +scan_input(text)
        +scan_output(text)
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
    SpendGate ..> ConsentStore : consumes grants
    SwarmServer ..> SpendGate : hire delegate fanout xaman
    LazyLocalSwarmRuntime ..> LocalSwarmRuntime : get_or_init
    LocalSwarmRuntime --> AgentExecutor : run then scan
    LocalSwarmRuntime ..> LocalDelegateResult : produces
    A2A ..> LocalSwarmRuntime : wraps delegate
    LocalAgentRegistry ..> LocalSwarmRuntime : reads cards

    note for SwarmServer "41 tools = 27 ABW + 14 local\nBoth sets always registered\nkask.swarm.mode selects the substrate not the surface\nSpend mutating tools are consent gated\npinned by tool_surface_is_exactly_41_registered_tools"
    note for LocalDelegateResult "Fed back as delegate_results\nto swarm-intelligence ORIENT\nactivates C5 fault attribution\nand C6 reconfigure"
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-006
verified_date: 2026-08-03
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/hkask_mcp_swarm.rs:115,124,2822; kask/mcp-servers/hkask-mcp-swarm/src/consent.rs:56,77,150,184,227; kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs:83,253,334; kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:39,73; kask/mcp-servers/hkask-mcp-swarm/src/agent_executor.rs:33,38,55; kask/mcp-servers/hkask-mcp-swarm/src/a2a.rs:24
status: VERIFIED
-->