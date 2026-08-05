---
title: "Swarm Feedback Loops — Cybernetic Map"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, trust]
---

# Swarm Feedback Loops — Cybernetic Map

The swarm system runs four coupled feedback loops. **Loop A** (PDCA
convergence) is the planner's inner loop; **Loop B** (C5/C6 steering execution)
closes only when the `swarm-steering` skill or the Curator in steering mode
feeds `delegate_results` back; **Loop C** (credit/consent algedonic) is the
strongest — the 402 / un-acknowledged curator dispatch escalates regardless of
the swarm-state distance `d` (the "never read as no deviation" invariant);
**Loop D** (Go See) is the intentionally-human outer meta-loop. The diagram
annotates each loop with its 5-property health (polarity, delay, gain, closure,
fidelity) and marks the two structural gaps found in the audit: Loop B's binary
`ok` fidelity (no task-success sensing for open tasks) and the C4 latency
sub-loop that is sensed but not regulated. See the [Swarm Cybernetics/Semantics Audit](../audits/swarm-cybernetics-semantics-audit.md) for the full per-property evidence and the [PDCA Cascade](flowchart-swarm-pdca-cascade.md) for the step decomposition.

```mermaid
flowchart TD
    SENSE["SENSE<br/>Onto4MAT + backend<br/>ABW wallet / local ledger"]
    ORIENT["ORIENT<br/>gap + C5 fault attribution<br/>reads delegate_results"]
    DECIDE["DECIDE<br/>PSO/ACO/Reynolds moves<br/>reads failed_edits influence fault_count"]
    FILTER["FILTER compute<br/>C3 failed-edit + C7 influence guards"]
    ACT["ACT<br/>emitted_calls plan<br/>+ steering_directive"]
    CHECK["CHECK<br/>re-measure compute d<br/>next_focus"]
    CONV["CONVERGE compute<br/>Cauchy on d<br/>C1 second-order monitor"]
    LOOP["LOOP<br/>thread accumulators back"]

    SENSE --> ORIENT --> DECIDE --> FILTER --> ACT --> CHECK --> CONV --> LOOP
    LOOP -->|not converged| SENSE
    LOOP -->|converged| DONE[Converged exit]

    subgraph exec["Execution boundary"]
        direction LR
        ADV["advisory default<br/>operator executes manually<br/>LOOP A closure DEGRADED"]
        STEER["steering mode<br/>Curator runs swarm_delegate_local<br/>LOOP A closure HEALTHY"]
    end
    ACT -.->|emitted_calls| exec
    exec -.->|delegate_results| ORIENT

    subgraph c6["Loop B C5/C6 actuator"]
        direction LR
        DL["swarm_delegate_local<br/>per emitted call"]
        RES["LocalDelegateResult<br/>tool_calls ok executed_skills ok<br/>BINARY fidelity"]
        RC["swarm_reconfigure_local_agent<br/>most-blamed agent only"]
        DL --> RES --> ORIENT
        ORIENT -.->|fault_count| RC
        RC -.->|re-prompt| DL
    end

    subgraph cred["Loop C credit/consent algedonic"]
        direction LR
        CEIL["ceiling HKASK_ABW_MAX_CREDITS<br/>+ swarm_hire_cost within_budget"]
        CONSENT["ConsentStore mint/consume<br/>TTL enforced real-time block"]
        WALLET["wallet reconciliation<br/>/api/wallet/transactions"]
        CEIL --> CONSENT --> WALLET
    end
    ACT -.->|hire delegate| cred
    WALLET -.->|loop_closure signal| CHECK

    ALG["ALGEDONIC override<br/>402 or un-ack curator dispatch<br/>escalates regardless of d"]
    cred -.->|402| ALG
    ALG -.->|force escalate| CONV

    GSEE["Loop D Go See C2<br/>human cadence every N convergences<br/>closure DEGRADED by design"]
    CONV -.->|go_see directive| GSEE
    GSEE -.->|operator descends| SENSE

    LAT["C4 latency_ms<br/>SENSED in LocalDelegateResult<br/>NOT regulated by DECIDE<br/>VARIETY DEFICIT"]
    RES -.-> LAT
    LAT -.->|no consumer| X["open sub-loop"]

    noteA["Loop A polarity healthy gain healthy<br/>delay degraded closure degraded fidelity degraded"]
    noteB["Loop B polarity healthy gain healthy<br/>delay healthy in steering<br/>fidelity DEGRADED binary ok"]
    noteC["Loop C polarity healthy delay healthy<br/>gain healthy closure healthy fidelity degraded local"]
    noteD["Loop D polarity healthy gain healthy<br/>delay degraded closure degraded by design fidelity healthy"]

    SENSE ~~~ noteA
    RC ~~~ noteB
    WALLET ~~~ noteC
    GSEE ~~~ noteD
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-008
verified_date: 2026-08-04
verified_against: .agents/skills/swarm-intelligence/SKILL.md:62,96,104,105,122,124,182; .agents/skills/swarm-steering/SKILL.md:59; kask/mcp-servers/hkask-mcp-swarm/src/consent.rs:77,184; kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs:83; crates/swarm_panel/src/swarm_panel.rs:191
status: VERIFIED
-->