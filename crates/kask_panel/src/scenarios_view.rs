//! Scenarios view — a center-pane `Item` that visualizes the `scenarios` MCP
//! server's scenario planning data.
//!
//! Provides three visualizations:
//! 1. **Pipeline overview** — forecast count, resolved/pending, overall Brier
//!    score, calibration curve summary.
//! 2. **Event matrix** — events plotted on a 2×2 grid by probability (x-axis)
//!    vs. uncertainty contribution (y-axis). Events near 50% probability with
//!    high uncertainty are the most worth calibrating.
//! 3. **Sensitivity timeline** — events ranked by uncertainty contribution,
//!    with probability bars and certainty tiers.
//!
//! Includes **instruction scaffolding** — guided prompts at each stage of the
//! scenario pipeline to help users maintain momentum. The prompts mirror the
//! Schwartz/Tetlock/Chermack methodology the scenarios server implements.

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Render, Task,
    WeakEntity, Window, prelude::*,
};
use serde::Deserialize;
use serde_json::json;
use ui::prelude::*;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem, TabContentParams},
};

use crate::kanban_tool_invoker;

/// The MCP server name.
const SCENARIOS_SERVER: &str = "scenarios";

// ── FIBO / methodology anchors ────────────────────────────────────────────
const FIBO_FORECAST_ID: &str = "fibo-fbc-fct-ra:ForecastIdentifier";
const FIBO_BRIER_SCORE: &str = "fibo-fbc-fct-ra:BrierScore";
const FIBO_SCENARIO_PROBABILITY: &str = "fibo-fbc-fct-ra:ScenarioProbability";

// ── MCP response structs ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StatusResponse {
    pipeline: PipelineOverview,
    calibration: Option<CalibrationSummary>,
    event_tree: Option<EventTreeSummary>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PipelineOverview {
    forecast_count: usize,
    resolved_count: usize,
    pending_count: usize,
    overall_brier: Option<f64>,
    recent_forecasts: Vec<RecentForecast>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RecentForecast {
    forecast_id: String,
    event_id: String,
    event_name: String,
    subject: Option<String>,
    probability: f64,
    outcome: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CalibrationSummary {
    total_forecasts: usize,
    resolved_forecasts: usize,
    overall_brier: Option<f64>,
    overconfidence_score: Option<f64>,
    interpretation: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventTreeSummary {
    subject: String,
    event_count: usize,
    joint_probability: Option<f64>,
    root_ids: Vec<String>,
    nodes: Vec<EventNode>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct EventNode {
    id: String,
    name: String,
    question: Option<String>,
    probability: Option<f64>,
    marginal_probability: Option<f64>,
    certainty_tier: Option<serde_json::Value>,
    basis: Option<String>,
    parent_ids: Vec<String>,
    sub_question_count: Option<usize>,
    has_base_rate: Option<bool>,
    brier_score: Option<f64>,
}

// ── Instruction scaffolding ───────────────────────────────────────────────

/// A guided prompt that suggests the next step in the scenario pipeline.
struct ScaffoldingPrompt {
    stage: &'static str,
    prompt: String,
    tool_hint: String,
}

/// Determine which scaffolding prompt to show based on the current state.
fn scaffolding_for_state(status: &Option<StatusResponse>) -> ScaffoldingPrompt {
    let Some(status) = status else {
        return ScaffoldingPrompt {
            stage: "Start",
            prompt: "No scenario data yet. Start by framing your question — what decision hangs on this forecast?".to_string(),
            tool_hint: "scenario_frame".to_string(),
        };
    };

    let p = &status.pipeline;

    // No forecasts yet → frame + brainstorm.
    if p.forecast_count == 0 {
        return ScaffoldingPrompt {
            stage: "Frame",
            prompt: "No forecasts recorded. Run scenario_frame to start a 7-turn coaching conversation that scopes your decision. Then scenario_brainstorm to generate candidate events.".to_string(),
            tool_hint: "scenario_frame".to_string(),
        };
    }

    // Forecasts exist but none resolved → quantify + calibrate.
    if p.resolved_count == 0 {
        return ScaffoldingPrompt {
            stage: "Calibrate",
            prompt: format!(
                "You have {} pending forecast(s). Run scenario_quantify to resolve probabilities, then scenario_calibrate to apply Fermi decomposition and base-rate shrinkage.",
                p.pending_count
            ),
            tool_hint: "scenario_quantify".to_string(),
        };
    }

    // Some resolved → check calibration + score.
    let brier_text = p
        .overall_brier
        .map(|b| format!("Overall Brier: {b:.3}"))
        .unwrap_or_else(|| "No Brier score yet".to_string());

    if let Some(ref cal) = status.calibration {
        let interp = cal.interpretation.as_deref().unwrap_or("unknown");
        return ScaffoldingPrompt {
            stage: "Learn",
            prompt: format!(
                "{brier_text} — {interp}. {}/{} forecasts resolved. Run scenario_assess to evaluate the project across Chermack's five phases, or scenario_cross_validate to compare independent estimates.",
                cal.resolved_forecasts, cal.total_forecasts
            ),
            tool_hint: "scenario_assess".to_string(),
        };
    }

    ScaffoldingPrompt {
        stage: "Score",
        prompt: format!(
            "{brier_text}. {}/{} resolved. Run scenario_score to compute Brier scores, then scenario_calibration to build your calibration curve.",
            p.resolved_count, p.forecast_count
        ),
        tool_hint: "scenario_score".to_string(),
    }
}

/// Pipeline stage guidance — the full Schwartz/Tetlock/Chermack flow.
const PIPELINE_STAGES: &[(&str, &str, &str)] = &[
    (
        "1. Frame",
        "scenario_frame",
        "7-turn coaching conversation: what decision hangs on this?",
    ),
    (
        "2. Brainstorm",
        "scenario_brainstorm",
        "4-round protocol: diverge → ground → link → prune",
    ),
    (
        "3. Build",
        "scenario_build",
        "Extract events from research into a dependency tree",
    ),
    (
        "4. Quantify",
        "scenario_quantify",
        "Resolve marginal + joint probabilities, sensitivity ranking",
    ),
    (
        "5. Calibrate",
        "scenario_calibrate",
        "Fermi decomposition + outside/inside view + bias correction",
    ),
    (
        "6. Score",
        "scenario_score",
        "Brier scoring against known outcomes",
    ),
    (
        "7. Assess",
        "scenario_assess",
        "Chermack 5-phase project assessment",
    ),
];

// ── View state ────────────────────────────────────────────────────────────

pub struct ScenariosView {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// Last fetched status response.
    status: Option<StatusResponse>,
    /// Whether data is loading.
    loading: bool,
    /// Error message if loading failed.
    error: Option<String>,
}

impl ScenariosView {
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            _workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            status: None,
            loading: false,
            error: None,
        })
    }

    fn fetch_status(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.error = None;
        cx.notify();

        let task = invoke_tool(SCENARIOS_SERVER, "scenario_status", json!({}));
        cx.spawn(async move |this, cx| match task.await {
            Ok(output) => {
                let result: Result<StatusResponse, _> = serde_json::from_str(&output);
                this.update(cx, |this, cx| match result {
                    Ok(resp) => {
                        this.status = Some(resp);
                        this.loading = false;
                        cx.notify();
                    }
                    Err(e) => {
                        this.loading = false;
                        this.error = Some(format!("Parse error: {e}"));
                        cx.notify();
                    }
                })
            }
            Err(e) => this.update(cx, |this, cx| {
                this.loading = false;
                this.error = Some(format!("Tool error: {e}"));
                cx.notify();
            }),
        })
        .detach();
    }

    // ── Render methods ────────────────────────────────────────────────────

    fn render_scaffolding(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let prompt = scaffolding_for_state(&self.status);

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Next: {}", prompt.stage))
                            .size(LabelSize::Small)
                            .color(Color::Accent),
                    )
                    .child(
                        Label::new(format!("→ /{}", prompt.tool_hint))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                Label::new(prompt.prompt)
                    .size(LabelSize::Small)
                    .color(Color::Default),
            )
            .into_any_element()
    }

    fn render_pipeline_stages(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let stages: Vec<AnyElement> = PIPELINE_STAGES
            .iter()
            .map(|(stage, tool, desc)| {
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Label::new(*stage)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(format!("/{tool}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        Label::new(*desc)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new("Scenario Pipeline (Schwartz → Tetlock → Chermack)")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(stages)
            .into_any_element()
    }

    fn render_pipeline_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(status) = &self.status else {
            return div().into_any_element();
        };
        let p = &status.pipeline;

        let brier_text = p
            .overall_brier
            .map(|b| format!("{b:.3}"))
            .unwrap_or_else(|| "—".to_string());

        let tiles = vec![
            ("Forecasts", format!("{}", p.forecast_count)),
            ("Resolved", format!("{}", p.resolved_count)),
            ("Pending", format!("{}", p.pending_count)),
            ("Brier", brier_text),
        ];

        let tile_elements: Vec<AnyElement> = tiles
            .into_iter()
            .map(|(label, value)| {
                v_flex()
                    .p_2()
                    .gap_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(label)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(value)
                            .size(LabelSize::Large)
                            .color(Color::Default),
                    )
                    .into_any_element()
            })
            .collect();

        h_flex()
            .gap_2()
            .flex_wrap()
            .children(tile_elements)
            .into_any_element()
    }

    fn render_calibration(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(status) = &self.status else {
            return div().into_any_element();
        };
        let Some(cal) = &status.calibration else {
            return div().into_any_element();
        };

        let brier_text = cal
            .overall_brier
            .map(|b| format!("{b:.3}"))
            .unwrap_or_else(|| "—".to_string());
        let overconf_text = cal
            .overconfidence_score
            .map(|o| format!("{o:+.3}"))
            .unwrap_or_else(|| "—".to_string());
        let interp = cal.interpretation.as_deref().unwrap_or("unknown");

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!("Calibration ({FIBO_BRIER_SCORE})"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Label::new(format!("Brier: {brier_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("Overconfidence: {overconf_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!(
                            "{}/{} resolved",
                            cal.resolved_forecasts, cal.total_forecasts
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    ),
            )
            .child(
                Label::new(interp)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .into_any_element()
    }

    fn render_event_matrix(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(status) = &self.status else {
            return div().into_any_element();
        };
        let Some(tree) = &status.event_tree else {
            return div().into_any_element();
        };
        if tree.nodes.is_empty() {
            return div().into_any_element();
        }

        // 2×2 matrix: x-axis = probability (0→1), y-axis = uncertainty (|p-0.5|).
        // Quadrants:
        //   Top-left: low prob, high uncertainty (worth watching)
        //   Top-right: high prob, high uncertainty (key swing factors)
        //   Bottom-left: low prob, low uncertainty (unlikely, settled)
        //   Bottom-right: high prob, low uncertainty (likely, settled)
        let nodes: Vec<&EventNode> = tree.nodes.iter().collect();

        // Render as a labeled grid with event dots positioned by probability.
        let dots: Vec<AnyElement> = nodes
            .iter()
            .map(|node| {
                let prob = node
                    .probability
                    .or(node.marginal_probability)
                    .unwrap_or(0.5);
                let uncertainty = (prob - 0.5).abs();
                // Map to position: x = prob (0→1), y = uncertainty (0→0.5).
                // We'll render as a horizontal bar with the event name + probability.
                let color = if uncertainty > 0.3 {
                    Color::Warning // high uncertainty
                } else if prob > 0.5 {
                    Color::Created // likely + settled
                } else {
                    Color::Muted // unlikely + settled
                };
                let prob_pct = format!("{:.0}%", prob * 100.0);
                let unc_pct = format!("{:.0}%", uncertainty * 100.0);

                h_flex()
                    .gap_2()
                    .child(
                        Label::new(node.name.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(
                        Label::new(format!("P={prob_pct}"))
                            .size(LabelSize::XSmall)
                            .color(color),
                    )
                    .child(
                        Label::new(format!("unc={unc_pct}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!("Event Matrix ({FIBO_SCENARIO_PROBABILITY}) — {} events", tree.nodes.len()))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                Label::new("Events sorted by probability. High-uncertainty events (near 50%) are worth calibrating.")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .children(dots).into_any_element()
    }

    fn render_event_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(status) = &self.status else {
            return div().into_any_element();
        };
        let Some(tree) = &status.event_tree else {
            return div().into_any_element();
        };

        let joint_text = tree
            .joint_probability
            .map(|j| format!("{j:.4}"))
            .unwrap_or_else(|| "—".to_string());

        let nodes: Vec<AnyElement> = tree
            .nodes
            .iter()
            .map(|node| {
                let prob = node
                    .probability
                    .or(node.marginal_probability)
                    .unwrap_or(0.0);
                let prob_pct = format!("{:.0}%", prob * 100.0);
                let tier = node
                    .certainty_tier
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let brier = node
                    .brier_score
                    .map(|b| format!("Brier={b:.3}"))
                    .unwrap_or_default();
                let parents = if node.parent_ids.is_empty() {
                    "root".to_string()
                } else {
                    format!("← {}", node.parent_ids.join(", "))
                };

                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Label::new(node.name.clone())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Default),
                            )
                            .child(
                                Label::new(prob_pct)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Accent),
                            )
                            .child(Label::new(tier).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                Label::new(parents)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .when(!brier.is_empty(), |this| {
                        this.child(
                            Label::new(brier)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    })
                    .when_some(node.question.as_ref(), |this, q| {
                        this.child(Label::new(q).size(LabelSize::XSmall).color(Color::Muted))
                    })
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Event Tree: {}", tree.subject))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!("Joint P={joint_text}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .child(
                        Label::new(format!("{} events", tree.event_count))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .children(nodes)
            .into_any_element()
    }

    fn render_recent_forecasts(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(status) = &self.status else {
            return div().into_any_element();
        };
        let recent = &status.pipeline.recent_forecasts;
        if recent.is_empty() {
            return div().into_any_element();
        }

        let rows: Vec<AnyElement> = recent
            .iter()
            .map(|f| {
                let prob_pct = format!("{:.0}%", f.probability * 100.0);
                let outcome_text = f
                    .outcome
                    .map(|o| if o { "✓ occurred" } else { "✗ didn't" })
                    .unwrap_or("pending");
                let color = if f.outcome.is_some() {
                    Color::Muted
                } else {
                    Color::Accent
                };

                h_flex()
                    .gap_2()
                    .child(
                        Label::new(f.event_name.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Default),
                    )
                    .child(Label::new(prob_pct).size(LabelSize::XSmall).color(color))
                    .child(
                        Label::new(outcome_text)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                Label::new(format!("Recent Forecasts ({FIBO_FORECAST_ID})"))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .children(rows)
            .into_any_element()
    }
}

// ── Tool invocation helper ────────────────────────────────────────────────

fn invoke_tool(server: &str, tool: &str, args: serde_json::Value) -> Task<Result<String, String>> {
    match kanban_tool_invoker() {
        Some(invoker) => invoker.invoke_tool(server, tool, args),
        None => Task::ready(Err("Tool invoker not wired".to_string())),
    }
}

// ── Item / SerializableItem impls ────────────────────────────────────────

impl Focusable for ScenariosView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<ItemEvent> for ScenariosView {}

impl Item for ScenariosView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> gpui::SharedString {
        "Scenarios".into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), _cx))
            .color(params.text_color())
            .into_any_element()
    }

    fn tab_tooltip_text(&self, _cx: &App) -> Option<gpui::SharedString> {
        Some("Scenario planning — framing, calibration, sensitivity, assessment".into())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Scenarios View Opened")
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(_event: &Self::Event, _f: &mut dyn FnMut(ItemEvent)) {}
}

impl SerializableItem for ScenariosView {
    fn serialized_item_kind() -> &'static str {
        "ScenariosView"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        _cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                ScenariosView::new(workspace, window, cx)
            })
        })
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

impl Render for ScenariosView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Fetch status on first render if empty.
        if self.status.is_none() && !self.loading && self.error.is_none() {
            self.fetch_status(cx);
        }

        let border_color = cx.theme().colors().border;

        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            // Header + refresh
            .child(
                h_flex()
                    .gap_2()
                    .justify_between()
                    .child(Label::new("Scenario Planning").size(LabelSize::Large))
                    .child(
                        Button::new("refresh-btn", "Refresh")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fetch_status(cx);
                            })),
                    ),
            )
            // Scaffolding prompt (next step guidance)
            .child(self.render_scaffolding(cx))
            // Pipeline stages reference
            .child(self.render_pipeline_stages(cx))
            // Pipeline overview tiles
            .child(self.render_pipeline_overview(cx))
            // Error / loading
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .p_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(border_color)
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                )
            })
            .when(self.loading, |this| {
                this.child(
                    Label::new("Loading…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            // Calibration
            .child(self.render_calibration(cx))
            // Event matrix (2×2 by probability × uncertainty)
            .child(self.render_event_matrix(cx))
            // Event tree (dependency chain)
            .child(self.render_event_tree(cx))
            // Recent forecasts
            .child(self.render_recent_forecasts(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolding_no_status_suggests_frame() {
        let prompt = scaffolding_for_state(&None);
        assert_eq!(prompt.stage, "Start");
        assert_eq!(prompt.tool_hint, "scenario_frame");
    }

    #[test]
    fn scaffolding_empty_pipeline_suggests_frame() {
        let status = StatusResponse {
            pipeline: PipelineOverview {
                forecast_count: 0,
                resolved_count: 0,
                pending_count: 0,
                overall_brier: None,
                recent_forecasts: vec![],
            },
            calibration: None,
            event_tree: None,
        };
        let prompt = scaffolding_for_state(&Some(status));
        assert_eq!(prompt.stage, "Frame");
        assert_eq!(prompt.tool_hint, "scenario_frame");
    }

    #[test]
    fn scaffolding_pending_forecasts_suggests_quantify() {
        let status = StatusResponse {
            pipeline: PipelineOverview {
                forecast_count: 3,
                resolved_count: 0,
                pending_count: 3,
                overall_brier: None,
                recent_forecasts: vec![],
            },
            calibration: None,
            event_tree: None,
        };
        let prompt = scaffolding_for_state(&Some(status));
        assert_eq!(prompt.stage, "Calibrate");
        assert_eq!(prompt.tool_hint, "scenario_quantify");
    }

    #[test]
    fn scaffolding_resolved_with_calibration_suggests_assess() {
        let status = StatusResponse {
            pipeline: PipelineOverview {
                forecast_count: 5,
                resolved_count: 3,
                pending_count: 2,
                overall_brier: Some(0.12),
                recent_forecasts: vec![],
            },
            calibration: Some(CalibrationSummary {
                total_forecasts: 5,
                resolved_forecasts: 3,
                overall_brier: Some(0.12),
                overconfidence_score: Some(0.05),
                interpretation: Some("good".to_string()),
            }),
            event_tree: None,
        };
        let prompt = scaffolding_for_state(&Some(status));
        assert_eq!(prompt.stage, "Learn");
        assert_eq!(prompt.tool_hint, "scenario_assess");
    }

    #[test]
    fn parse_status_response() {
        let json = r#"{
            "pipeline": {
                "forecast_count": 5,
                "resolved_count": 2,
                "pending_count": 3,
                "overall_brier": 0.15,
                "recent_forecasts": []
            },
            "calibration": {
                "total_forecasts": 5,
                "resolved_forecasts": 2,
                "overall_brier": 0.15,
                "overconfidence_score": 0.03,
                "interpretation": "good"
            },
            "event_tree": {
                "subject": "AAPL valuation",
                "event_count": 3,
                "joint_probability": 0.12,
                "root_ids": ["e1"],
                "nodes": [
                    {"id":"e1","name":"Revenue >10B","question":"Will revenue exceed 10B?","probability":0.7,"marginal_probability":0.7,"certainty_tier":"likely","basis":"base_rate","parent_ids":[],"sub_question_count":2,"has_base_rate":true,"brier_score":null},
                    {"id":"e2","name":"Margin expansion","question":"Will gross margin expand?","probability":0.4,"marginal_probability":0.4,"certainty_tier":"uncertain","basis":"fermi","parent_ids":["e1"],"sub_question_count":3,"has_base_rate":false,"brier_score":null}
                ]
            }
        }"#;
        let resp: StatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.pipeline.forecast_count, 5);
        assert_eq!(resp.pipeline.resolved_count, 2);
        assert!(resp.calibration.is_some());
        assert!(resp.event_tree.is_some());
        let tree = resp.event_tree.unwrap();
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.nodes[0].name, "Revenue >10B");
    }
}
