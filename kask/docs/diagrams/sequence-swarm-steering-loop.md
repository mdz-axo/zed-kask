---
title: "Swarm Steering Loop"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, trust]
---

# Swarm Steering Loop

The steering loop closes the C5/C6 feedback boundary. The swarm-intelligence cascade plans (emits `emitted_calls`); the executor (the Kask Curator in steering mode, or the operator in advisory mode) runs the delegations via `swarm_delegate_local`, collects `LocalDelegateResult` objects, and feeds them back as `delegate_results` on the next swarm-intelligence invocation — activating C5 (fault attribution from `tool_calls[].ok`/`executed_skills[].ok`) and C6 (reconfigure the most-blamed agent). The `swarm-steering` skill codifies the execute-and-feed-back directive. See the [Cybernetic Swarm Plan](../plans/cybernetic-swarm-plan.md) and the [swarm-steering SKILL.md](../../.agents/skills/swarm-steering/SKILL.md).

```mermaid
sequenceDiagram
    participant Curator as Kask Curator
    participant SI as swarm-intelligence
    participant SS as swarm-steering
    participant Swarm as hkask-mcp-swarm

    Note over Curator,Swarm: advisory mode operator executes manually; steering mode Curator executes
    Curator->>SI: invoke with task + swarm_id + steering_mode
    SI->>SI: 10-step PDCA cascade plans
    SI-->>Curator: emitted_calls plan + steering_directive

    alt steering mode
        Curator->>SS: invoke with emitted_calls
        SS-->>Curator: steering directive execution_sequence + collection_shape
        loop each delegate emitted call
            Curator->>Swarm: swarm_delegate_local agent task credits
            Swarm->>Swarm: Rung 4 Binding check_bind<br/>Rung 1-2 admission already passed at authoring
            Swarm->>Swarm: skill cascade + tool loop + ledger debit
            Swarm->>Swarm: Rung 3 Grounding enforce_grounding<br/>null unsourced fields, scan narrative
            Swarm-->>Curator: LocalDelegateResult agent_id response tool_calls bind_matched
        end
        Curator->>Curator: collect LocalDelegateResults into delegate_results array
        Curator->>SI: re-invoke with delegate_results + steering_mode
        Note right of SI: ORIENT attributes fault C5; fault_count accumulates; C6 reconfigures
        SI->>SI: next PDCA iteration with real telemetry
    else advisory mode
        Curator-->>Curator: plan is final output; operator executes manually
        Note over Curator: operator feeds delegate_results back on next invocation
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-010
verified_date: 2026-08-16
verified_against: .agents/skills/swarm-steering/SKILL.md:60,64; .agents/skills/swarm-intelligence/SKILL.md:147,156,184; kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs:39,73,check_bind; kask/crates/hkask-verification/src/grounding.rs:enforce_grounding
status: VERIFIED
-->