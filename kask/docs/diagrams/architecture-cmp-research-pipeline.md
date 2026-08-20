---
title: "CMP-First Research Pipeline Architecture"
audience: [developers, researchers, architects, agents]
last_updated: 2026-08-07
version: "0.36.0"
status: "Active"
domain: "Bayesian-APT research"
mds_categories: [domain, composition, trust]
---

# CMP-First Research Pipeline Architecture

The Bayesian-APT research program (v2, CMP-first) is a four-phase pipeline that
builds constant-maturity prediction (CMP) indices from raw prediction-market
catalogs, composes them into scenario trees, computes risk and coherence
measures, and runs falsification tests on the H1–H5 hypotheses. The pipeline
spans four crates: `hkask-forecast` (pure math), `hkask-mcp-prediction-markets`
(CMP construction), `hkask-mcp-scenarios` (composition), and
`hkask-mcp-companies` (tree-weighted valuation).

```mermaid
graph TD
    subgraph catalogs["Catalogs (on-disk JSONL)"]
        kalshi["Kalshi events<br/>9,542 records"]
        gamma["Polymarket events<br/>2,100 records"]
        contracts["Per-family contracts<br/>7 families × 2 venues"]
    end

    subgraph phase0["Phase 0 — CMP Foundation"]
        direction TB
        classify["classify_base_object_from_catalog<br/>FIBO-anchored semantic mapping"]
        build["build_cmp_indices<br/>C0.4 index builder"]
        cohort["solve_portfolio_cohort<br/>C0.5 single-cohort fallback"]
        classify --> build
        build --> cohort
    end

    subgraph phase1["Phase 1 — Re-point Machinery"]
        direction TB
        compose["compose_cmp_tree<br/>R1: CMP → EventTree"]
        deps["compose_cmp_tree_with_deps<br/>R1: dependency edges"]
        duration["duration_vs_cmp_tenors<br/>R2: equity duration vs CMP"]
        tree_weight["EventTreeProjection<br/>R3: CMP provenance in weighting"]
        compose --> deps
        compose --> duration
        compose --> tree_weight
    end

    subgraph phase2["Phase 2 — Risk and Coherence"]
        direction TB
        risk["cmp_scenario_risk_measure<br/>R4: σ_scenario with CMP provenance"]
        coherence["contract_price_coherence<br/>R5: tree-implied vs market price"]
        risk --> coherence
    end

    subgraph phase3["Phase 3 — Falsification"]
        direction TB
        h2["h2_duration_test<br/>H2: duration falsification"]
        h3["h3_coherence_test<br/>H3: coherence falsification"]
        log["falsification_log<br/>H1–H5 status log"]
        h2 --> log
        h3 --> log
    end

    subgraph mcp_tools["MCP Tool Surface"]
        direction TB
        tool_cmp["scenario_from_cmp_indices<br/>scenarios server"]
        tool_analysis["scenario_analysis<br/>companies server"]
        tool_duration["equity_duration<br/>companies server"]
    end

    kalshi --> contracts
    gamma --> contracts
    contracts --> classify
    phase0 -->|"ProvenancedCmpIndex"| phase1
    phase0 -->|"ProvenancedCmpIndex"| phase2
    phase1 -->|"EventTree"| phase2
    phase1 -->|"EventTree"| tool_analysis
    phase2 --> phase3
    compose --> tool_cmp
    duration --> tool_duration
    tree_weight --> tool_analysis
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-ARCH-001
verified_date: 2026-08-07
verified_against: kask/crates/hkask-forecast/src/hkask_forecast.rs; kask/crates/hkask-forecast/src/falsification.rs; kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_index_builder.rs; kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_portfolio.rs; kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs; kask/mcp-servers/hkask-mcp-companies/src/superforecast.rs
status: VERIFIED
-->

## Phase 0 — CMP Foundation

The foundation layer takes raw prediction-market catalogs and produces
constant-maturity, constant-orientation synthetic portfolio indices. Each index
is a weighted portfolio of real contracts whose weighted-average maturity
matches a fixed target (1m/3m/6m). The time axis is taken out of the equation
so the only thing that moves is the probability.

```mermaid
flowchart TD
    records["Catalog records<br/>Kalshi / Gamma JSONL"] --> classify["classify_base_object_from_catalog<br/>FIBO-anchored semantic mapping"]
    classify -->|"BaseEconomicObject"| eligible["build_oriented_constituents<br/>strike extraction + orientation"]
    eligible -->|"OrientedConstituent[]" --> buckets["select_available_buckets<br/>maturity window check"]
    buckets -->|"available buckets"| bracket["solve_portfolio<br/>bracket pair interpolation"]
    buckets -->|"available buckets"| cohort["solve_portfolio_cohort<br/>C0.5 single-cohort fallback"]
    bracket -->|"Interpolated"| index["ProvenancedCmpIndex<br/>family + venue + portfolio"]
    cohort -->|"BucketedSparse"| index
    bracket -->|"None — no bracket"| cohort
    cohort -->|"None — beyond tolerance"| withhold["Withheld<br/>never fabricate"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-ARCH-002
verified_date: 2026-08-07
verified_against: kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_index_builder.rs:build_cmp_indices_from_lines; kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_portfolio.rs:solve_portfolio; kask/mcp-servers/hkask-mcp-prediction-markets/src/cmp_portfolio.rs:solve_portfolio_cohort
status: VERIFIED
-->

## Phase 1 — Composition and Re-pointing

CMP indices flow into the scenario composition machinery. Each index becomes a
root `ScenarioEvent` with its index probability as the prior. The tree cites
the index identity (`cmp:{family}:{tenor}:{orientation}`), not a decaying
contract. Optional dependency edges between indices enable joint probability
computation for the H3 coherence test.

```mermaid
flowchart TD
    indices["ProvenancedCmpIndex[]<br/>from build_cmp_indices"] --> convert["convert_cmp_index<br/>CMP → ScenarioEvent"]
    convert -->|"observation_date"| events["ScenarioEvent[]<br/>id=cmp:family:tenor:orientation"]
    events -->|"no deps"| flat["compose_cmp_tree<br/>flat independent tree"]
    events -->|"with deps"| dep_tree["compose_cmp_tree_with_deps<br/>dependent tree"]
    dep_tree -->|"CmpDependencySpec[]"| build["build_event_tree<br/>topo sort + marginalize"]
    flat --> build
    build -->|"EventTree"| output["tree: marginals + joint<br/>+ cmp_provenance"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-ARCH-003
verified_date: 2026-08-07
verified_against: kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs:compose_cmp_tree; kask/mcp-servers/hkask-mcp-scenarios/src/superforecast.rs:compose_cmp_tree_with_deps
status: VERIFIED
-->

## Phase 2–3 — Risk, Coherence, and Falsification

The risk measure computes σ_scenario over CMP-controlled branches. The
coherence measure compares tree-implied joint probabilities against observed
market prices within a transaction-cost band. The falsification suite runs
the computable tests (H2 duration, H3 coherence) and records the blocked
tests (H1 systemic risk, H4 complexity, H5 LLM leverage).

```mermaid
flowchart TD
    tree["EventTree<br/>from compose_cmp_tree"] --> branches["CmpBranchOutcome[]<br/>probability + branch_return + cmp_source"]
    branches --> risk["cmp_scenario_risk_measure<br/>σ_scenario + cmp_controlled flag"]
    tree -->|"root marginals"| pairs["(tree_implied, market_price)[]<br/>from tree + parlay prices"]
    pairs --> coherence["contract_price_coherence<br/>divergence + coherent flag"]
    risk --> h_log["falsification_log"]
    coherence --> h_log
    h2_dur["h2_duration_test<br/>equity duration vs CMP tenors"] --> h_log
    h_log --> statuses["H1: blocked<br/>H2: corroborated/refuted<br/>H3: corroborated/refuted<br/>H4: open<br/>H5: blocked"]
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-ARCH-004
verified_date: 2026-08-07
verified_against: kask/crates/hkask-forecast/src/hkask_forecast.rs:cmp_scenario_risk_measure; kask/crates/hkask-forecast/src/hkask_forecast.rs:contract_price_coherence; kask/crates/hkask-forecast/src/falsification.rs:falsification_log
status: VERIFIED
-->

## Crate dependency graph

The pure-math crate `hkask-forecast` has no MCP dependencies. The three MCP
servers depend on it for the shared computation engine. The scenarios server
depends on the prediction-markets server for the `ProvenancedCmpIndex` type.
The companies server does not depend on the scenarios server (the integration
seam is caller-mediated paste bridging via `EventTreeProjection`).

```mermaid
graph TD
    forecast["hkask-forecast<br/>pure math: R2, R4, R5, R6"]
    pm["hkask-mcp-prediction-markets<br/>C0.1–C0.5, ONT-6"]
    scenarios["hkask-mcp-scenarios<br/>R1: compose_cmp_tree"]
    companies["hkask-mcp-companies<br/>R3: tree-weighted valuation"]

    pm -->|"depends on"| forecast
    scenarios -->|"depends on"| forecast
    scenarios -->|"depends on"| pm
    companies -->|"depends on"| forecast
    companies -.->|"caller-mediated<br/>(EventTreeProjection JSON)"|-. scenarios
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-CMP-ARCH-005
verified_date: 2026-08-07
verified_against: kask/crates/hkask-forecast/Cargo.toml; kask/mcp-servers/hkask-mcp-prediction-markets/Cargo.toml; kask/mcp-servers/hkask-mcp-scenarios/Cargo.toml; kask/mcp-servers/hkask-mcp-companies/Cargo.toml
status: VERIFIED
-->

## See also

- [Research Plan (v2, CMP-first)](../../../tasks/bayesian-apt/plan.md) — the full plan with dependency graph and phase structure.
- [CMP Foundation Spec](../../../tasks/bayesian-apt/cmp-foundation.md) — the three-step index process, passed variables, and publication format.
- [All-Families Probe Results](../../../tasks/bayesian-apt/all-families-probe.md) — the structural maturity-ladder finding and C0.5 cohort fallback results.
- [Falsification Log](../../../tasks/bayesian-apt/falsification-log.md) — H1–H5 statuses and falsifiers.
- [C0.4 Decisions](../../../tasks/bayesian-apt/c0.4-decisions.md) — the four open-question resolutions and CP-CMP checkpoint findings.
