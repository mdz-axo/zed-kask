//! The `ScenariosWidget` GPUI view — renders the scenario pipeline overview,
//! calibration summary, event matrix, event tree list, and recent forecasts
//! inline in agent markdown. Read-only.
//!
//! This replaces the deleted standalone `ScenariosView` from `kask_panel`.
//! Data comes from the parsed `ScenariosBlockBody` instead of from `ToolInvoker`
//! MCP tool fetches — the agent (curator) calls `scenario_status` and emits
//! the result as a ```scenarios fenced block.

use gpui::{
    AnyElement, App, Context, FocusHandle, Focusable, IntoElement, ParentElement, Render, Styled,
    Window, div,
};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize, prelude::*};

use crate::block::{
    EventNode, FIBO_BRIER_SCORE, FIBO_FORECAST_ID, FIBO_SCENARIO_PROBABILITY, ScenariosBlockBody,
};

/// The scenarios widget view. Renders inline in agent markdown (via the D18
/// seam composed by `hkask-viz-core`).
pub struct ScenariosWidget {
    body: ScenariosBlockBody,
    focus_handle: FocusHandle,
}

impl ScenariosWidget {
    /// Create a new scenarios widget for the parsed block body.
    pub fn new(body: ScenariosBlockBody, cx: &mut Context<Self>) -> Self {
        Self {
            body,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_scaffolding(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let prompt = scaffolding_for_state(&self.body);

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
        let pipeline = &self.body.pipeline;

        let brier_text = pipeline
            .overall_brier
            .map(|b| format!("{b:.3}"))
            .unwrap_or_else(|| "—".to_string());

        let tiles = vec![
            ("Forecasts", format!("{}", pipeline.forecast_count)),
            ("Resolved", format!("{}", pipeline.resolved_count)),
            ("Pending", format!("{}", pipeline.pending_count)),
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
        let Some(cal) = &self.body.calibration else {
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
        let Some(tree) = &self.body.event_tree else {
            return div().into_any_element();
        };
        if tree.nodes.is_empty() {
            return div().into_any_element();
        }

        let nodes: Vec<&EventNode> = tree.nodes.iter().collect();

        let dots: Vec<AnyElement> = nodes
            .iter()
            .map(|node| {
                let prob = node
                    .probability
                    .or(node.marginal_probability)
                    .unwrap_or(0.5);
                let uncertainty = (prob - 0.5).abs();
                let color = if uncertainty > 0.3 {
                    Color::Warning
                } else if prob > 0.5 {
                    Color::Created
                } else {
                    Color::Muted
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
                Label::new(format!(
                    "Event Matrix ({FIBO_SCENARIO_PROBABILITY}) — {} events",
                    tree.nodes.len()
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .child(
                Label::new("Events sorted by probability. High-uncertainty events (near 50%) are worth calibrating.")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .children(dots)
            .into_any_element()
    }

    fn render_event_tree_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let Some(tree) = &self.body.event_tree else {
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
        let recent = &self.body.recent_forecasts;
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

impl Focusable for ScenariosWidget {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ScenariosWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .track_focus(&self.focus_handle)
            .child(
                h_flex().child(
                    Label::new("Scenario Planning")
                        .size(LabelSize::Large)
                        .color(Color::Default),
                ),
            )
            .child(self.render_scaffolding(cx))
            .child(self.render_pipeline_stages(cx))
            .child(self.render_pipeline_overview(cx))
            .child(self.render_calibration(cx))
            .child(self.render_event_matrix(cx))
            .child(self.render_event_tree_list(cx))
            .child(self.render_recent_forecasts(cx))
    }
}

// ── Instruction scaffolding ───────────────────────────────────────────────

struct ScaffoldingPrompt {
    stage: &'static str,
    prompt: String,
    tool_hint: String,
}

fn scaffolding_for_state(body: &ScenariosBlockBody) -> ScaffoldingPrompt {
    let p = &body.pipeline;

    if p.forecast_count == 0 {
        return ScaffoldingPrompt {
            stage: "Frame",
            prompt: "No forecasts recorded. Run scenario_frame to start a 7-turn coaching conversation that scopes your decision. Then scenario_brainstorm to generate candidate events.".to_string(),
            tool_hint: "scenario_frame".to_string(),
        };
    }

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

    let brier_text = p
        .overall_brier
        .map(|b| format!("Overall Brier: {b:.3}"))
        .unwrap_or_else(|| "No Brier score yet".to_string());

    if let Some(ref cal) = body.calibration {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{CalibrationSummary, PipelineOverview};

    #[test]
    fn scaffolding_empty_pipeline_suggests_frame() {
        let body = ScenariosBlockBody::default();
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Frame");
        assert_eq!(prompt.tool_hint, "scenario_frame");
    }

    #[test]
    fn scaffolding_pending_forecasts_suggests_quantify() {
        let body = ScenariosBlockBody {
            viz: Some("scenarios".into()),
            pipeline: PipelineOverview {
                forecast_count: 3,
                resolved_count: 0,
                pending_count: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Calibrate");
        assert_eq!(prompt.tool_hint, "scenario_quantify");
    }

    #[test]
    fn scaffolding_resolved_with_calibration_suggests_assess() {
        let body = ScenariosBlockBody {
            viz: Some("scenarios".into()),
            pipeline: PipelineOverview {
                forecast_count: 5,
                resolved_count: 3,
                pending_count: 2,
                overall_brier: Some(0.12),
                ..Default::default()
            },
            calibration: Some(CalibrationSummary {
                total_forecasts: 5,
                resolved_forecasts: 3,
                overall_brier: Some(0.12),
                overconfidence_score: Some(0.05),
                interpretation: Some("good".to_string()),
            }),
            ..Default::default()
        };
        let prompt = scaffolding_for_state(&body);
        assert_eq!(prompt.stage, "Learn");
        assert_eq!(prompt.tool_hint, "scenario_assess");
    }
}
