# Prediction-Markets Data Service — Architecture

**Diataxis type:** explanation (orienting diagram + data-flow reference).
**Companion:** `02-zed-kask-integration.md` (design), `tasks/plan.md` (build).

## System context

```mermaid
flowchart TD
    subgraph External["External platforms (read-only)"]
        PM["Polymarket<br/>Gamma + CLOB REST<br/>public market WS"]
        KX["Kalshi<br/>Predictions REST v2<br/>(no credentials held)"]
    end

    subgraph Server["hkask-mcp-prediction-markets (MCP server)"]
        direction TB
        Providers["provider_polymarket<br/>provider_kalshi<br/>(fetch + parse, per-variant errors)"]
        Contract["types.rs — annotated MarketRecord<br/>probability + spread + volume +<br/>calibration + volatility + ontology"]
        Analytics["cmp.rs — CMP + index<br/>residual.rs — base-event exposure<br/>matcher.rs — event resolution"]
        Loop["calibration.rs — Brier store (JSONL journal)<br/>reliability_tier demotion (negative loop)"]
        Providers --> Contract
        Contract --> Analytics
        Loop --> Contract
    end

    subgraph Consumers["Consumers"]
        Scen["hkask-mcp-scenarios<br/>scenario_from_markets bridge<br/>cross_validate / synthesize"]
        SF["superforecasting FlowDef<br/>stage 2 outside view · stage 4 evidence"]
        SB["scenario-builder FlowDef<br/>key-forces pre-weighting"]
    end

    PM --> Providers
    KX --> Providers
    Contract -->|"caller-mediated JSON / crate type"| Scen
    Contract -->|"market_context cascade input"| SF
    Contract -->|"market_context cascade input"| SB
    Analytics --> Scen
```

## The calibration feedback loop (the cybernetic core)

```mermaid
flowchart LR
    subgraph Sense["Sense"]
        R1["market_record_resolution<br/>(manual / agent)"]
        R2["market_check_resolutions<br/>(settled-market scanner)"]
        WS["market_subscribe_resolutions<br/>(notify-only stream)"]
    end
    Store["CalibrationStore<br/>bucket → resolved (p, outcome) pairs<br/>JSONL journal, atomic rename"]
    Decide["Decide: per-bucket Brier<br/>(hkask-forecast brier_score_multi)"]
    Act["Act: reliability_tier demotion<br/>High→Medium when Brier > 0.25<br/>over ≥5 observations"]
    Record["MarketRecord.calibration +<br/>reliability_tier on every lookup"]

    R1 --> Store
    R2 --> Store
    WS -.->|"notify only — no pre-resolution<br/>price on the wire"| R1
    Store --> Decide --> Act --> Record
    Record -.->|"consumers see degraded tier"| Sense
```

**Loop invariants (all pinned by tests):** negative-only polarity (good calibration never promotes); a missing/failed calibration read is `stale: true`, never `brier: 0` (a synthetic 0 would read as "perfectly calibrated" and invert the loop); ambiguous 50-50 resolutions are skipped, never recorded (arXiv:2604.20421 "Unknown" resolutions).

## The annotated contract (data shape reference)

```mermaid
classDiagram
    class MarketRecord {
        +Source source
        +string event_id / market_id / series
        +string question / deadline / category
        +f64 probability  +ProbabilityMethod probability_method
        +f64? spread  +f64 volume  +VolumeGrain volume_grain
        +f64? liquidity  +f64? open_interest
        +MarketStatus status  +bool? resolved_outcome
        +Calibration calibration
        +Volatility volatility
        +ReliabilityTier reliability_tier
        +OntologyBlock ontology
    }
    class Calibration {
        +f64? brier
        +string? domain_bias
        +string bias_source
        +u64 sample_size
        +bool stale
    }
    class Volatility {
        +f64? realized_variance
        +StructuralFlag structural_flag
        +string interpretation
        +VolatilityForecast? dras_forecast
    }
    class OntologyBlock {
        +ProcessAxis process  (PKO: lifecycle stage)
        +StateAxis state  (Dublin Core: id/title/temporal/provenance)
        +u32 mapping_version
    }
    MarketRecord *-- Calibration
    MarketRecord *-- Volatility
    MarketRecord *-- OntologyBlock
```

**Load-bearing rule:** no field is ever a bare probability. Every `probability` travels with its reliability covariates, calibration state, and ontology mapping — a consumer cannot be naive by default.

## The CMP index (term structure)

```mermaid
xychart
    x-axis ["7d", "30d", "90d", "180d", "365d", "730d"]
    y-axis "P(no Fed change)" 0 --> 0.16
    line [0.061, 0.061, 0.096, 0.095, 0.118, 0.139]
```

*Live curve, KXFEDDECISION, 2026-08-05 (12 cohorts, log-odds interpolated). The 30d→1y slope (+0.79 log-odds/yr) is the term-structure signal: expectations of holding steady strengthen with horizon.*

## Tool surface (18 tools)

| Tool | Role |
|---|---|
| `market_lookup` | annotated records by free-text query |
| `market_match` | event ↔ market entity resolution (confidence-tiered) |
| `market_ontology_map` | the dual-axis mapping document |
| `market_calibration` | per-bucket Brier reading |
| `market_record_resolution` | sense arm: record an outcome |
| `market_check_resolutions` | sense arm: scan settled markets (idempotent) |
| `market_subscribe_resolutions` | notify-only resolution stream |
| `market_ladder` | series duration profile |
| `market_cmp` | single-tenor CMP point |
| `market_cmp_index` | the full published curve + slope |
| `market_cmp_index_store` | store the CMP curve as a transaction-ledger portfolio |
| `market_cmp_portfolio_store` | store the solved-portfolio CMP index set (maturity-bucketed, orientation-tagged) |
| `market_cmp_context_suggest` | propose curated/live economic context with reasoning |
| `market_volatility` | DR-AS structural volatility forecast (arXiv:2607.08199) |
| `market_residual` | niche-event base exposure (β, r²) |
| `market_history` | price history + realized variance + regime |
