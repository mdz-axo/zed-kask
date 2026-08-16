---
title: "Verification Ladder — Agent Ecology Grounding Flow"
audience: [architects, developers]
last_updated: 2026-08-16
version: "2.0.0"
status: "Active"
domain: "Trust"
mds_categories: [trust, composition]
---

# Verification Ladder — Agent Ecology Grounding Flow

Flowchart of the four-rung verification ladder applied to `kanban-task-*`
agent delegations. Rungs 1 (Presence) and 2 (Typing) run at authoring time
(slow clock); Rungs 3 (Grounding) and 4 (Binding) run at invocation time
(fast clock). The grounding check nulls unsourced fields before the response
persists — it is a control, not a metric. See [Verification for Agent
Ecologies](../architecture/verification-for-agent-ecologies.md).

```mermaid
flowchart TD
    subgraph authoring["Authoring gate — slow clock"]
        A1["swarm_create_local_agent<br/>or write_card"]
        A2["Rung 1: Presence<br/>validate_presence"]
        A3["Rung 2: Typing<br/>validate_typing vs PortRegistry"]
        A4{"Pass?"}
        A5["Reject<br/>McpToolError::invalid_argument"]
        A6["Write card + reload"]
        A1 --> A2 --> A3 --> A4
        A4 -- "No" --> A5
        A4 -- "Yes" --> A6
    end

    subgraph invocation["Invocation gate — fast clock"]
        I1["kanban_task_spawn<br/>→ spawn_via_local_runtime"]
        I2["Rung 4: Binding<br/>check_bind card vs task"]
        I3["runtime.delegate<br/>skill cascade + tool loop + LLM"]
        I4["Rung 3: Grounding<br/>enforce_grounding<br/>null unsourced + stamp provenance"]
        I5["Rung 2: Schema validation<br/>schema_validate<br/>AFTER grounding, BEFORE consume"]
        I6{"Unsourced fields?"}
        I7["Null unsourced fields<br/>retain truncated preview<br/>scan narrative for leaks<br/>LeakRule::Quantity"]
        I8["Keep cleaned JSON<br/>stamp provenance keys<br/>log warn if nulled"]
        I9["Build delegation envelope<br/>envelope::build<br/>provenance survives the hop"]
        I10["Record result + comment<br/>retain raw_response"]
        I1 --> I2 --> I3 --> I4 --> I5 --> I6
        I6 -- "Yes" --> I7 --> I8
        I6 -- "No" --> I8
        I8 --> I9 --> I10
    end

    A6 --> I1

    subgraph vocabulary["Six-valued provenance"]
        V1["Sourced<br/>tool returned it → keep"]
        V2["Inferred<br/>commissioned judgment → keep"]
        V3["Derived<br/>platform-computed from sourced → keep"]
        V4["UncommissionedInference<br/>not commissioned but plausible → keep"]
        V5["Narrative<br/>prose → keep, scan"]
        V6["Unsourced<br/>no tool could supply → null"]
    end

    subgraph truth["Rung 2: Truth (slow clock)"]
        T1["rollup_trust<br/>cost vs cost_uncapped<br/>balance: None ≠ 0"]
    end

    I4 -.->|per field| vocabulary
    I10 -.->|documents| truth
```

## The two clocks

Verification runs on two independent schedules. The authoring gate
(slow clock) prevents bad cards from entering the catalogue. The invocation
gate (fast clock) catches what the authoring gate cannot know — that this
particular output, on this particular run, contains something no tool could
have supplied.

## What the grounding check catches

The paper's headline defect: an agent that claims "I wrote the file at
`/src/main.rs`" without ever calling a file-writing tool. The
`deliverable_path` field is nulled, the narrative leak is detected if the
agent restates the path in prose, and the operator sees a `warn!` log naming
the nulled field.

## What it does not catch (paper §6)

- Semantic hallucination inside a sourced field (needs outcome scoring)
- Prose output (grounding is a no-op for non-JSON)
- Fields the contract doesn't declare (marked UncommissionedInference, not nulled)

## Related

- [Verification for Agent Ecologies](../architecture/verification-for-agent-ecologies.md) — full architecture doc
- [Swarm Steering Loop](./sequence-swarm-steering-loop.md) — where delegations run
- [Architecture Principles](../architecture/core/PRINCIPLES.md) — P8.2 Agent Output Grounding

<!-- DIAGRAM_ALIGNMENT
id: DIAG-FLOW-VERIFICATION-LADDER-001
verified_date: 2026-08-16
verified_against: kask/mcp-servers/hkask-mcp-swarm/src/local_registry.rs (validate_presence, validate_typing); kask/mcp-servers/hkask-mcp-swarm/src/port_registry.rs (PortRegistry); kask/mcp-servers/hkask-mcp-swarm/src/local_runtime.rs (check_bind, classify_request, raw_response); kask/mcp-servers/hkask-mcp-kata-kanban/src/grounding.rs (enforce_grounding, ProvenanceTag, task_agent_contract, LeakRule, NARRATIVE_LEAK_RULES, provenance_stamp); kask/mcp-servers/hkask-mcp-kata-kanban/src/card_contract.rs (validate); kask/mcp-servers/hkask-mcp-kata-kanban/src/schema_validate.rs (validate); kask/mcp-servers/hkask-mcp-kata-kanban/src/envelope.rs (build); kask/mcp-servers/hkask-mcp-kata-kanban/src/rollup_trust.rs (ROLLUP_CONTRACTS); kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs (spawn_via_local_runtime grounding wiring, build_task_agent_card system prompt)
status: VERIFIED
-->
