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
use gpui_util::ResultExt as _;
use hkask_tool_invoker::{BlockProvenance, shared_tool_invoker};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize, prelude::*};

use crate::block::{
    EventNode, FIBO_BRIER_SCORE, FIBO_FORECAST_ID, FIBO_SCENARIO_PROBABILITY, ScenariosBlockBody,
};

/// Server that hosts the scenario pipeline tools. Used as the fallback dispatch
/// target when a block carries no dispatchable provenance (T2 hardcoded path).
const DEFAULT_SERVER: &str = "hkask-mcp-scenarios";

/// Visible hint surfaced when provenance is present but its `tool` does not
/// match the clicked rung — the widget refuses to re-issue the wrong tool and
/// asks the user to route through the agent instead.
const PROVENANCE_MISMATCH_MSG: &str = "provenance mismatch — ask the agent";

/// Visible hint surfaced when the process-global `ToolInvoker` has not been
/// wired (e.g. before the post-login deferred task runs). Per the repo `.rules`
/// startup-failure-signal trap, this is a visible state, not a silent no-op.
const INVOKER_NOT_WIRED_MSG: &str = "tool invoker not wired";

/// The scenarios widget view. Renders inline in agent markdown (via the D18
/// seam composed by `hkask-viz-core`).
pub struct ScenariosWidget {
    body: ScenariosBlockBody,
    focus_handle: FocusHandle,
    /// The tool name currently being dispatched, if a rung click is in flight.
    dispatch_in_flight: Option<String>,
    /// Visible error/hint surfaced when dispatch cannot proceed (missing
    /// invoker, provenance mismatch). Never silently dropped per repo `.rules`.
    dispatch_error: Option<String>,
    /// Text returned by the most recent successful dispatch, if any. Kept
    /// minimal — the agent conversation is the durable record.
    dispatch_result: Option<String>,
}

impl ScenariosWidget {
    /// Create a new scenarios widget for the parsed block body.
    pub fn new(body: ScenariosBlockBody, cx: &mut Context<Self>) -> Self {
        hkask_tool_invoker::record_render(
            body.provenance.tool.clone(),
            body.provenance.span_id.clone(),
        );
        tracing::info!(
            target: "reg.widget.render",
            tool = body.provenance.tool.as_deref().unwrap_or(""),
            span_id = body.provenance.span_id.as_deref().unwrap_or(""),
            "REG",
        );
        Self {
            body,
            focus_handle: cx.focus_handle(),
            dispatch_in_flight: None,
            dispatch_error: None,
            dispatch_result: None,
        }
    }

    fn render_scaffolding(&self, cx: &mut Context<Self>) -> AnyElement {
        let border_color = cx.theme().colors().border;
        let prompt = scaffolding_for_state(&self.body);
        let rung_tool = prompt.tool_hint.clone();

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border_color)
            .child(
                h_flex().gap_2().child(
                    div()
                        .id("scenarios-scaffolding-next")
                        .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                            this.dispatch_rung(&rung_tool.clone(), cx);
                        }))
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
                let rung_tool = (*tool).to_string();
                v_flex()
                    .gap_0p5()
                    .id((*tool).to_string())
                    .on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                        this.dispatch_rung(&rung_tool.clone(), cx);
                    }))
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

    /// Render a visible dispatch status row: pending spinner label, error/hint,
    /// or the truncated result of the last successful dispatch. Emits nothing
    /// when idle so the widget stays compact.
    fn render_dispatch_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(tool) = &self.dispatch_in_flight {
            return Some(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("Dispatching /{tool} …"))
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.dispatch_error {
            let border_color = cx.theme().colors().border;
            return Some(
                div()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(border_color)
                    .child(
                        Label::new(error.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Warning),
                    )
                    .into_any_element(),
            );
        }
        if let Some(result) = &self.dispatch_result {
            // Truncate long tool output so the conversation remains the durable record.
            let preview = if result.chars().count() > 120 {
                let truncated: String = result.chars().take(120).collect();
                format!("{truncated}…")
            } else {
                result.clone()
            };
            return Some(
                Label::new(format!("Dispatched: {preview}"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .into_any_element(),
            );
        }
        None
    }

    /// Shared `on_click` body for the scaffolding `Next:` label and each
    /// `PIPELINE_STAGES` rung. Resolves the dispatch plan from the block's
    /// provenance and the rung's tool, then routes through the governed
    /// `shared_tool_invoker()` (OCAP/gas-budgeted in production via
    /// `McpRuntime`).
    ///
    /// Surfaced states (never silent per repo `.rules`):
    /// - `INVOKER_NOT_WIRED_MSG` when `shared_tool_invoker()` returns `None`.
    /// - `PROVENANCE_MISMATCH_MSG` when provenance is dispatchable but its
    ///   `tool` differs from the rung's tool.
    pub(crate) fn dispatch_rung(&mut self, rung_tool: &str, cx: &mut Context<Self>) {
        let plan = build_dispatch_args(
            &self.body.provenance,
            rung_tool,
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        );
        let (server, tool, args) = match plan {
            Ok(tuple) => tuple,
            Err(message) => {
                self.dispatch_error = Some(message.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
        };

        let invoker = match shared_tool_invoker() {
            None => {
                self.dispatch_error = Some(INVOKER_NOT_WIRED_MSG.to_string());
                self.dispatch_in_flight = None;
                cx.notify();
                return;
            }
            Some(invoker) => invoker,
        };

        self.dispatch_error = None;
        self.dispatch_in_flight = Some(tool.clone());
        let task = invoker.invoke_tool(&server, &tool, args);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                this.dispatch_in_flight = None;
                match outcome {
                    Ok(text) => {
                        this.dispatch_result = Some(text);
                        this.dispatch_error = None;
                    }
                    Err(error) => {
                        this.dispatch_error = Some(error);
                        this.dispatch_result = None;
                    }
                }
                cx.notify();
            })
            .log_err();
        })
        .detach();
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
            .when_some(self.render_dispatch_status(cx), |this, status| {
                this.child(status)
            })
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

// ── Pure dispatch-planning logic (T2/T4) ──────────────────────────────────
//
// Kept free of GPUI executor / global state so the dispatch decision is
// unit-testable directly (the repo `.rules` racy-global trap: never unit-test by
// mutating `set_tool_invoker`). The `on_click` handler in `dispatch_rung`
// composes this pure function with `shared_tool_invoker()`.

/// Merge `rung_override` (an object) into `base` (an object). Non-object inputs
/// are treated as empty objects so a `null`/absent `args` still merges cleanly —
/// a block produced before args were recorded re-issues with just the override.
fn merge_args(base: &serde_json::Value, rung_override: &serde_json::Value) -> serde_json::Value {
    let mut merged = serde_json::Map::new();
    if let serde_json::Value::Object(map) = base {
        for (key, value) in map {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let serde_json::Value::Object(overrides) = rung_override {
        for (key, value) in overrides {
            merged.insert(key.clone(), value.clone());
        }
    }
    serde_json::Value::Object(merged)
}

/// Pure: decide the `(server, tool, args)` dispatch tuple for a rung click,
/// given the block's provenance and the rung's tool name.
///
/// - provenance dispatchable AND `provenance.tool` matches `rung_tool` →
///   re-issue the originating `provenance.server` / `rung_tool` with
///   `rung_override` merged into a clone of `provenance.args` (object merge).
/// - provenance absent / not dispatchable → fall back to the T2 hardcoded
///   dispatch: `(default_server, rung_tool, default_args)`.
/// - provenance dispatchable but `provenance.tool` differs from `rung_tool` →
///   `Err(PROVENANCE_MISMATCH_MSG)`: the widget refuses to re-issue the wrong
///   tool and surfaces an "ask the agent" hint instead.
fn build_dispatch_args(
    provenance: &BlockProvenance,
    rung_tool: &str,
    default_server: &str,
    default_args: serde_json::Value,
    rung_override: &serde_json::Value,
) -> Result<(String, String, serde_json::Value), &'static str> {
    if provenance.is_dispatchable() {
        // `is_dispatchable()` guarantees `tool` is `Some`; `.unwrap_or_default()` keeps
        // this panic-free (returns an empty string only if the invariant were violated).
        let provenance_tool = provenance.tool.as_deref().unwrap_or_default();
        if provenance_tool == rung_tool {
            let server = provenance.server.clone().unwrap_or_default();
            let merged = merge_args(&provenance.args, rung_override);
            Ok((server, rung_tool.to_string(), merged))
        } else {
            Err(PROVENANCE_MISMATCH_MSG)
        }
    } else {
        Ok((
            default_server.to_string(),
            rung_tool.to_string(),
            default_args,
        ))
    }
}

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

    // ── build_dispatch_args pure-logic tests (required minimum) ────────────

    fn provenance_with(
        tool: Option<&str>,
        server: Option<&str>,
        args: serde_json::Value,
    ) -> BlockProvenance {
        BlockProvenance {
            tool: tool.map(str::to_string),
            server: server.map(str::to_string),
            args,
            span_id: None,
        }
    }

    #[test]
    fn dispatch_args_empty_provenance_falls_back_to_hardcoded() {
        // T2 fallback: a block with no provenance dispatches the rung's tool
        // against the default server with the hardcoded default args.
        let provenance = BlockProvenance::default();
        assert!(!provenance.is_dispatchable());

        let result = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        );
        let (server, tool, args) = result.expect("empty provenance falls back");
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_frame_rung_yields_scenario_frame_default() {
        // Pins the T2 acceptance shape: the Frame rung dispatches the
        // `scenario_frame` tool against the scenarios server with empty args.
        let provenance = BlockProvenance::default();
        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        )
        .expect("fallback dispatch");
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn dispatch_args_dispatchable_match_merges_override_into_provenance_args() {
        // T4: provenance.tool matches the rung → re-issue the originating
        // server/tool with the rung override merged into provenance.args.
        let provenance = provenance_with(
            Some("scenario_quantify"),
            Some("hkask-mcp-scenarios"),
            serde_json::json!({"event_id": "e1", "subject": "AAPL"}),
        );
        assert!(provenance.is_dispatchable());

        let override_args = serde_json::json!({"event_id": "e2"});
        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_quantify",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &override_args,
        )
        .expect("dispatchable + match merges");
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_quantify");
        // Override wins for `event_id`; untouched provenance keys survive.
        assert_eq!(args["event_id"], "e2");
        assert_eq!(args["subject"], "AAPL");
    }

    #[test]
    fn dispatch_args_dispatchable_match_uses_provenance_server_not_default() {
        // The merge case dispatches against provenance.server, not the default
        // server — a block produced by a different (aliased) server re-issues
        // through that server.
        let provenance = provenance_with(
            Some("scenario_quantify"),
            Some("hkask-mcp-scenarios-staging"),
            serde_json::json!({}),
        );
        let (server, tool, _args) = build_dispatch_args(
            &provenance,
            "scenario_quantify",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        )
        .expect("dispatchable + match");
        assert_eq!(server, "hkask-mcp-scenarios-staging");
        assert_eq!(tool, "scenario_quantify");
    }

    #[test]
    fn dispatch_args_mismatch_surfaces_error_not_wrong_dispatch() {
        // T4: provenance.tool is Some but differs from the rung's tool → the
        // widget refuses to dispatch the wrong tool and surfaces an error.
        let provenance = provenance_with(
            Some("scenario_status"),
            Some("hkask-mcp-scenarios"),
            serde_json::json!({}),
        );
        assert!(provenance.is_dispatchable());

        let result = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        );
        let error = result.expect_err("mismatch must error");
        assert_eq!(error, PROVENANCE_MISMATCH_MSG);
    }

    #[test]
    fn dispatch_args_non_dispatchable_with_tool_some_falls_back() {
        // tool is Some but server is None → not dispatchable → fallback (the
        // "not dispatchable" gate takes precedence over the mismatch rule).
        let provenance = provenance_with(Some("scenario_status"), None, serde_json::json!({}));
        assert!(!provenance.is_dispatchable());

        let (server, tool, args) = build_dispatch_args(
            &provenance,
            "scenario_frame",
            DEFAULT_SERVER,
            serde_json::json!({}),
            &serde_json::Value::Null,
        )
        .expect("not dispatchable falls back");
        assert_eq!(server, "hkask-mcp-scenarios");
        assert_eq!(tool, "scenario_frame");
        assert_eq!(args, serde_json::json!({}));
    }

    #[test]
    fn merge_args_treats_null_base_as_empty_object() {
        // A block produced before args were recorded re-issues with just the override.
        let merged = merge_args(
            &serde_json::Value::Null,
            &serde_json::json!({"event_id": "e1"}),
        );
        assert_eq!(merged, serde_json::json!({"event_id": "e1"}));
    }

    #[test]
    fn merge_args_override_overrides_base_keys() {
        let merged = merge_args(
            &serde_json::json!({"event_id": "e1", "keep": true}),
            &serde_json::json!({"event_id": "e2", "added": 1}),
        );
        assert_eq!(merged["event_id"], "e2");
        assert_eq!(merged["keep"], true);
        assert_eq!(merged["added"], 1);
    }

    // ── GPUI integration tests via the governed `shared_tool_invoker()` ──────
    //
    // These mutate the process-global `ToolInvoker`, which is racy across
    // parallel tests (the repo `.rules` racy-global trap). `GLOBAL_TEST_LOCK`
    // serializes the two tests below within this crate's test binary so they
    // never observe each other's invoker.
    static GLOBAL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Records `(server, tool, args)` for every `invoke_tool` call and returns
    /// canned JSON. Implements `Send + Sync` so it can be stored behind the
    /// `Arc<dyn ToolInvoker>` process global.
    #[derive(Default)]
    struct MockToolInvoker {
        calls: std::sync::Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl hkask_tool_invoker::ToolInvoker for MockToolInvoker {
        fn invoke_tool(
            &self,
            server: &str,
            tool: &str,
            args: serde_json::Value,
        ) -> gpui::Task<Result<String, String>> {
            self.calls
                .lock()
                .expect("MockToolInvoker calls mutex poisoned")
                .push((server.to_string(), tool.to_string(), args));
            gpui::Task::ready(Ok("{}".to_string()))
        }
    }

    /// RAII guard that restores the global invoker to `None` on drop so a test
    /// failure cannot leak a mock into sibling tests.
    struct InvokerGuard;
    impl Drop for InvokerGuard {
        fn drop(&mut self) {
            hkask_tool_invoker::set_tool_invoker(None);
        }
    }

    #[gpui::test]
    async fn dispatch_rung_routes_frame_rung_through_invoker(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        // Empty body → empty (non-dispatchable) provenance → T2 hardcoded fallback.
        let widget =
            cx.update(|cx| cx.new(|cx| ScenariosWidget::new(ScenariosBlockBody::default(), cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let calls = mock.calls.lock().expect("calls poisoned").clone();
        assert_eq!(calls.len(), 1, "exactly one dispatch");
        assert_eq!(calls[0].0, "hkask-mcp-scenarios");
        assert_eq!(calls[0].1, "scenario_frame");
        assert_eq!(calls[0].2, serde_json::json!({}));

        // The widget cleared in-flight and recorded the canned result.
        let (in_flight, has_result) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_in_flight.clone(),
                    widget.dispatch_result.is_some(),
                )
            })
        });
        assert!(in_flight.is_none(), "in-flight cleared after completion");
        assert!(has_result, "successful dispatch stored a result");
    }

    #[gpui::test]
    async fn dispatch_rung_surfaces_missing_invoker_as_visible_error(
        cx: &mut gpui::TestAppContext,
    ) {
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        hkask_tool_invoker::set_tool_invoker(None);

        let widget =
            cx.update(|cx| cx.new(|cx| ScenariosWidget::new(ScenariosBlockBody::default(), cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert_eq!(error.as_deref(), Some(INVOKER_NOT_WIRED_MSG));
        assert!(in_flight.is_none());
    }

    #[gpui::test]
    async fn dispatch_rung_surfaces_provenance_mismatch(cx: &mut gpui::TestAppContext) {
        let _guard = GLOBAL_TEST_LOCK.lock().expect("test lock poisoned");
        let _restore = InvokerGuard;
        // A mock is wired so the mismatch is detected *before* the invoker is
        // consulted — no call should be recorded.
        let mock = std::sync::Arc::new(MockToolInvoker::default());
        hkask_tool_invoker::set_tool_invoker(Some(mock.clone()));

        let body = ScenariosBlockBody {
            provenance: BlockProvenance {
                tool: Some("scenario_status".into()),
                server: Some("hkask-mcp-scenarios".into()),
                args: serde_json::json!({}),
                span_id: None,
            },
            ..Default::default()
        };
        let widget = cx.update(|cx| cx.new(|cx| ScenariosWidget::new(body, cx)));
        cx.update(|cx| {
            widget.update(cx, |widget, cx| widget.dispatch_rung("scenario_frame", cx));
        });
        cx.run_until_parked();

        let (error, in_flight) = cx.update(|cx| {
            widget.read_with(cx, |widget, _cx| {
                (
                    widget.dispatch_error.clone(),
                    widget.dispatch_in_flight.clone(),
                )
            })
        });
        assert_eq!(error.as_deref(), Some(PROVENANCE_MISMATCH_MSG));
        assert!(in_flight.is_none());
        assert!(
            mock.calls.lock().expect("calls poisoned").is_empty(),
            "no dispatch on mismatch"
        );
    }
}
