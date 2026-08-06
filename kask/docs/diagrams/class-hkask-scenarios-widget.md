---
title: "hKask Scenarios Widget — Class Diagram"
audience: [architects, developers]
last_updated: 2026-08-04
version: "1.0.0"
status: "Active"
domain: "Composition"
mds_categories: [composition]
---

# hKask Scenarios Widget — Class Diagram

`hkask-scenarios-widget` renders ```` ```scenarios ```` fenced blocks as a
forecasting dashboard: pipeline overview, calibration summary, event matrix,
event-tree list, and recent forecasts. The event-tree DAG itself (`viz:
"event_tree"`) is rendered separately by `hkask-graph-widget`. The widget is a
passive renderer over the parsed `ScenariosBlockBody` (mirrors the
`scenario_status` tool response).

```mermaid
classDiagram
    class ScenariosBlockBody {
        +viz: Option~String~
        +pipeline: PipelineOverview
        +calibration: Option~CalibrationSummary~
        +event_tree: Option~EventTreeSummary~
        +recent_forecasts: Vec~RecentForecast~
        +provenance: BlockProvenance
        +ontology: Option~String~
    }
    class PipelineOverview {
        +forecast_count: usize
        +resolved_count: usize
        +pending_count: usize
        +overall_brier: Option~f64~
        +recent_forecasts: Vec~RecentForecast~
    }
    class RecentForecast {
        +forecast_id: String
        +event_id: String
        +event_name: String
        +subject: Option~String~
        +probability: f64
        +outcome: Option~bool~
    }
    class CalibrationSummary {
        +total_forecasts: usize
        +resolved_forecasts: usize
        +overall_brier: Option~f64~
        +overconfidence_score: Option~f64~
        +interpretation: Option~String~
    }
    class EventTreeSummary {
        +subject: String
        +event_count: usize
        +joint_probability: Option~f64~
        +root_ids: Vec~String~
        +nodes: Vec~EventNode~
    }
    class EventNode {
        +id: String
        +name: String
        +question: Option~String~
        +probability: Option~f64~
        +marginal_probability: Option~f64~
        +certainty_tier: Option~Value~
        +basis: Option~String~
        +parent_ids: Vec~String~
        +brier_score: Option~f64~
    }
    class ScaffoldingPrompt {
        +stage: String
        +prompt: String
        +tool_hint: String
    }
    class ScenariosWidget {
        +body: ScenariosBlockBody
        +focus_handle: FocusHandle
        +new(body, cx) ScenariosWidget
        +render_pipeline_overview()
        +render_calibration()
        +render_event_matrix()
        +render_event_tree_list()
        +render_recent_forecasts()
    }
    class create_scenarios_widget {
        +create_scenarios_widget(body, cx) Option~Entity~ScenariosWidget~~
    }

    ScenariosBlockBody "1" o-- "1" PipelineOverview : pipeline
    ScenariosBlockBody "1" o-- "0..1" CalibrationSummary : calibration
    ScenariosBlockBody "1" o-- "0..1" EventTreeSummary : event_tree
    ScenariosBlockBody "1" o-- "many" RecentForecast : recent_forecasts
    PipelineOverview "1" o-- "many" RecentForecast : recent_forecasts
    EventTreeSummary "1" o-- "many" EventNode : nodes
    ScenariosWidget --> ScenariosBlockBody
    ScenariosWidget ..|> gpui_Focusable : Focusable
    ScenariosWidget ..|> gpui_Render : Render
    create_scenarios_widget ..> ScenariosWidget : viz is scenarios
```

**Block shape:** a JSON body with `viz: "scenarios"`. All sub-objects default
to empty/`None` so partial bodies render the present sections. A body with
`viz: "event_tree"` is NOT claimed (it goes to `hkask-graph-widget`).

**Scaffolding:** `scaffolding_for_state` maps the current pipeline state to a
`ScaffoldingPrompt` (stage + prompt + tool_hint) — e.g. an empty pipeline
suggests `scenario_frame`, pending forecasts suggest `scenario_quantify`, and
resolved forecasts with a calibration suggest `scenario_assess`.

**FIBO / methodology anchors:** `FIBO_FORECAST_ID`, `FIBO_BRIER_SCORE`,
`FIBO_SCENARIO_PROBABILITY` anchor displayed metrics to the FIBO ontology.

<!-- DIAGRAM_ALIGNMENT
id: DIAG-VIZ-SCENARIOS
verified_date: 2026-08-04
verified_against: crates/hkask-scenarios-widget/src/block.rs; crates/hkask-scenarios-widget/src/view.rs
status: VERIFIED
-->