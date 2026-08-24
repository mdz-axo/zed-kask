---
title: "CMP Tool Call Flow"
audience: [developers, agents, operators]
last_updated: 2026-08-07
version: "0.38.0"
status: "Active"
domain: "Bayesian-APT research"
mds_categories: [domain, composition, curation]
---

# CMP Tool Call Flow

The CMP research pipeline is accessible via MCP tools. The agent or panel calls
the tools in sequence: build CMP indices from catalogs, compose them into a
scenario tree, feed the tree into tree-weighted valuation, and run the
falsification tests. The integration seam between the scenarios server and the
companies server is caller-mediated — the caller pastes the tree JSON from
`scenario_from_cmp_indices` into the `event_tree` parameter of
`scenario_analysis`.

```mermaid
flowchart TD
    step1["1. build_cmp_indices<br/>(prediction-markets server)"] -->|"ProvenancedCmpIndex[]"| step2
    step2["2. scenario_from_cmp_indices<br/>(scenarios server)"] -->|"EventTree JSON<br/>+ cmp_provenance"| step3
    step3["3. scenario_analysis<br/>(companies server)<br/>event_tree = paste tree JSON"] -->|"weighted scenarios<br/>+ expected intrinsic"| step4
    step4["4. equity_duration<br/>(companies server)"] -->|"duration_years"| step5
    step5["5. h2_duration_test<br/>(hkask-forecast library)"] -->|"H2DurationResult"| step6
    step6["6. h3_coherence_test<br/>(hkask-forecast library)<br/>pairs = tree joints vs parlay prices"] -->|"H3CoherenceResult"| step7
    step7["7. falsification_log<br/>(hkask-forecast library)<br/>h2 + h3 results"] -->|"H1–H5 statuses"| output["Falsification log<br/>corroborated / refuted / blocked"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-FLOW-001
verified_date: 2026-08-07
verified_against: kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_index_builder.rs:build_cmp_indices; kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs:scenario_from_cmp_indices; kask/mcp-servers/hkask-mcp-companies/src/tools/analytics.rs:scenario_analysis; kask/crates/hkask-forecast/src/falsification.rs:falsification_log
status: VERIFIED
-->

## The caller-mediated seam

The scenarios server and the companies server do not depend on each other
directly. The integration is caller-mediated: the agent (or panel) takes the
tree JSON output from `scenario_from_cmp_indices` and pastes it into the
`event_tree` parameter of `scenario_analysis`. The `EventTreeProjection` struct
in the companies server is the documented contract of what the bridge consumes.

```mermaid
sequenceDiagram
    participant Agent
    participant PM as prediction-markets
    participant Scen as scenarios
    participant Comp as companies
    participant Forecast as hkask-forecast

    Agent->>PM: build_cmp_indices(family, venue, config)
    PM-->>Agent: ProvenancedCmpIndex[]

    Agent->>Scen: scenario_from_cmp_indices(indices, date, deps)
    Scen-->>Agent: EventTree JSON + cmp_provenance

    Agent->>Comp: scenario_analysis(symbol, event_tree=paste)
    Comp-->>Agent: weighted scenarios + expected intrinsic

    Agent->>Comp: equity_duration(symbol)
    Comp-->>Agent: duration_years

    Agent->>Forecast: h2_duration_test(duration_years)
    Forecast-->>Agent: H2DurationResult

    Agent->>Forecast: h3_coherence_test(pairs, cost_band)
    Forecast-->>Agent: H3CoherenceResult

    Agent->>Forecast: falsification_log(h2, h3)
    Forecast-->>Agent: H1–H5 status log
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-FLOW-002
verified_date: 2026-08-07
verified_against: kask/mcp-servers/hkask-mcp-scenarios/src/hkask_mcp_scenarios.rs:scenario_from_cmp_indices; kask/mcp-servers/hkask-mcp-companies/src/tools/analytics.rs:scenario_analysis; kask/crates/hkask-forecast/src/falsification.rs:falsification_log
status: VERIFIED
-->

## See also

- [CMP Research Pipeline Architecture](architecture-cmp-research-pipeline.md) — the full four-phase architecture with crate dependencies.
- [Research Plan (v2, CMP-first)](../../../tasks/bayesian-apt/plan.md) — the plan with acceptance criteria for each phase.
- [Falsification Log](../../../tasks/bayesian-apt/falsification-log.md) — the H1–H5 status table and falsifiers.
