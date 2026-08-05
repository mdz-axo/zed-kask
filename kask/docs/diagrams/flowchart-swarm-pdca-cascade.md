---
title: "Swarm Intelligence PDCA Cascade"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Swarm Intelligence PDCA Cascade

The `swarm-intelligence` skill runs a 10-step PDCA cascade that composes and steers swarms. Steps 4, 7, 8, 9 are deterministic `compute` steps (no LLM) — the cybernetic accumulators and guards live in the math layer, not in LLM templates, because an LLM cannot reliably maintain a running set/sum across LOOP iterations. The FILTER (step 4) deterministically enforces the C3 failed-edit and C7 influence guards. The CONVERGE steps (7–9) check convergence, accumulate iteration_log/failed_edits/influence_scores/fault_count, and run the second-order monitor (C1 reasoning-loop + sensor-truth-divergence + C2 Go See cadence). The LOOP (step 10) threads the accumulators back. See the [Cybernetic Swarm Plan](../plans/cybernetic-swarm-plan.md) and the [Swarm MCP Server Reference](../reference/mcp-servers/swarm.md).

```mermaid
flowchart TD
    S1["1 SENSE select<br/>Measure swarm state Onto4MAT + backend"]
    S2["2 ORIENT select<br/>Classify gap + fault attribution C5<br/>reads delegate_results"]
    S3["3 DECIDE select<br/>Propose moves PSO/ACO/Reynolds<br/>reads failed_edits, influence, fault_count"]
    S4["4 FILTER compute<br/>Enforce C3 + C7 guards<br/>swarm.filter_proposed_moves"]
    S5["5 ACT select<br/>Emit gated calls + steering_directive"]
    S6["6 CHECK select<br/>Re-measure, compute d, next_focus"]
    S7["7 CONVERGE compute<br/>Cauchy on d kata.convergence_check"]
    S8["8 CONVERGE compute<br/>Accumulate iteration_log,<br/>failed_edits, influence, fault_count<br/>swarm.converge_accumulate"]
    S9["9 CONVERGE compute<br/>Second-order monitor C1 + C2 cadence<br/>swarm.second_order_monitor"]
    S10["10 LOOP<br/>Thread accumulators back to SENSE"]

    S1 --> S2 --> S3 --> S4 --> S5 --> S6 --> S7 --> S8 --> S9 --> S10
    S10 -->|not converged| S1
    S10 -->|converged| DONE[Converged exit]

    S10 -.->|iteration_log| S8
    S10 -.->|failed_edits influence| S4
    S10 -.->|fault_count| S3
    S10 -.->|second_order| S3
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-DIA-SWARM-009
verified_date: 2026-08-04
verified_against: .agents/skills/swarm-intelligence/SKILL.md:63,66,120,130
status: VERIFIED
-->