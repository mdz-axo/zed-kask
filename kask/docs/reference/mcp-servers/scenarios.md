---
title: "Scenarios MCP Server Reference"
audience: [developers, architects]
last_updated: 2026-07-21
version: "0.31.0"
status: "Active"
domain: "Composition"
mds_categories: [composition, lifecycle]
---

# Scenarios MCP Server Reference

**Crate:** `mcp-servers/hkask-mcp-scenarios`
**Tools:** 18 — `scenario_frame`, `scenario_frame_document`, `scenario_brainstorm`, `scenario_build`, `scenario_research`, `scenario_quantify`, `scenario_calibrate`, `scenario_update`, `scenario_sensitivity`, `scenario_synthesize`, `scenario_cross_validate`, `scenario_score`, `scenario_calibration`, `scenario_assess`, `scenario_triage`, `scenario_status`, `scenario_from_companies`, `scenario_full`
**Auto-start:** No (in `CORE_EXCLUDED` — requires explicit opt-in via `/mcp start`)

## Pipeline Architecture (DIAG-RF-005)

This diagram shows the control flow between the 18 MCP tools in the scenarios server, grouped by pipeline phase. Solid arrows indicate the expected predecessor relationship enforced by `check_sequence` (warn-only, non-blocking). Dashed arrows indicate optional or independent paths. The `scenario_full` tool compresses the entire chain into a single call by delegating to the same engine functions.

```mermaid
flowchart TD
    subgraph Framing["Framing Phase (PKO)"]
        frame["scenario_frame\n7-turn conversational protocol"]
        frame_doc["scenario_frame_document\nStructure to FramingDocument"]
        frame --> frame_doc
    end

    subgraph Ideation["Ideation Phase (PKO)"]
        brainstorm["scenario_brainstorm\n4-round temperature protocol"]
        frame_doc --> brainstorm
    end

    subgraph Structuring["Structuring Phase"]
        build["scenario_build\nEvent tree scaffold"]
        research["scenario_research\nExtract from web text"]
        brainstorm --> build
        research -.-> build
    end

    subgraph Computation["Computation Phase (Dublin Core)"]
        quantify["scenario_quantify\nMarginal + joint probabilities"]
        calibrate["scenario_calibrate\nFermi + outside view"]
        update["scenario_update\nBayesian revision"]
        sensitivity["scenario_sensitivity\nVariance ranking"]
        build --> quantify
        quantify --> calibrate
        calibrate --> update
        quantify --> sensitivity
    end

    subgraph Aggregation["Aggregation Phase"]
        synthesize["scenario_synthesize\nDragonfly-eye weighting"]
        cross_validate["scenario_cross_validate\nLLM vs computation"]
        calibrate --> synthesize
        calibrate --> cross_validate
    end

    subgraph Tracking["Tracking Phase"]
        score["scenario_score\nBrier + ForecastStore"]
        calibration["scenario_calibration\nCalibration curve"]
        quantify --> score
        score --> calibration
    end

    subgraph Assessment["Assessment Phase"]
        assess["scenario_assess\nChermack 5-phase"]
        synthesize --> assess
    end

    subgraph Independent["Independent Tools"]
        triage["scenario_triage\nGoldilocks classification"]
        status["scenario_status\nState snapshot"]
        companies["scenario_from_companies\nFIBO bridge"]
        full["scenario_full\nAll-in-one pipeline"]
    end

    triage -.-> quantify
    companies --> quantify
    full -.-> |delegates to engine| quantify
    full -.-> |delegates to engine| calibrate
    full -.-> |delegates to engine| synthesize
    full -.-> |delegates to engine| assess

    subgraph Engine["superforecast.rs (shared engine)"]
        engine_tree["build_event_tree"]
        engine_fermi["calibrate_from_fermi"]
        engine_bayes["bayesian_update"]
        engine_brier["score_forecast"]
        engine_curve["compute_calibration_curve"]
        engine_synth["synthesize_perspectives"]
        engine_assess["assess_project"]
        engine_cross["cross_validate"]
        engine_companies["convert_companies_output"]
    end

    quantify --> engine_tree
    calibrate --> engine_fermi
    update --> engine_bayes
    score --> engine_brier
    calibration --> engine_curve
    synthesize --> engine_synth
    assess --> engine_assess
    cross_validate --> engine_cross
    companies --> engine_companies
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-RF-005
verified_date: 2026-07-21
verified_against: mcp-servers/hkask-mcp-scenarios/src/lib.rs (18 tool routers + check_sequence), mcp-servers/hkask-mcp-scenarios/src/superforecast.rs (engine functions: build_event_tree, calibrate_from_fermi, bayesian_update, score_forecast, compute_calibration_curve, synthesize_perspectives, assess_project, cross_validate, convert_companies_output), mcp-servers/hkask-mcp-scenarios/src/types.rs
status: VERIFIED
-->

## Key paths

- **Standard pipeline:** `scenario_frame` → `scenario_frame_document` → `scenario_brainstorm` → `scenario_build` → `scenario_quantify` → `scenario_calibrate` → `scenario_synthesize` → `scenario_score` → `scenario_assess`
- **Research entry:** `scenario_research` → `scenario_build` (skip brainstorming if events are extracted from web text)
- **Companies bridge:** `scenario_from_companies` → `scenario_quantify` (skip framing/brainstorming — events come from DCF model)
- **Single-call:** `scenario_full` delegates to `triage_question`, `build_event_tree`, `sensitivity_ranking`, `calibrate_from_fermi`, `outside_view_adjustment`, `synthesize_perspectives`, `assess_project`
- **Independent:** `scenario_triage`, `scenario_status` callable at any point

## Cross-links

- [Superforecasting: Layered Model](../../explanation/forecasting-and-scenarios.md) — three-layer model (skill, math, servers)
- Scenarios Adversarial Review — code smell inventory and action items
- Scenarios Semantic Graph Audit — cross-skill/server dependency graph
- [MCP Server Registry](README.md) — built-in server index
- [Diagram Index](../../DIAGRAMS_INDEX.md) — DIAG-RF-005 registration
